use super::*;
use aaa_proto::{DeviceRole, FrontRequest};
use std::sync::Arc;

struct Lab {
    root: PathBuf,
    h: Arc<Host>,
}
impl Lab {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("aaa-account-rename-{}", aaa_common::util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = root.join("projects").display().to_string();
        cfg.notify.web_push = false;
        let h = Host::open(cfg, root.join("host")).unwrap();
        h.project_create("demo", Default::default(), "local:cli").unwrap();
        h.accounts.import("work", "codex", AUTH).unwrap();
        Self { root, h }
    }
    fn conn(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(self.root.join("host/host.db")).unwrap()
    }
}
impl Drop for Lab {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
const AUTH: &str =
    r#"{"tokens":{"access_token":"fixture-access-0123456789","refresh_token":"fixture-refresh-0123456789"}}"#;
fn request(to: &str) -> FrontRequest {
    FrontRequest::AccountRename { name: "work".into(), to: to.into() }
}

#[tokio::test]
async fn rename_reencrypts_preserves_identity_and_moves_only_account_references() {
    let l = Lab::new();
    let h = &l.h;
    let old = h.accounts.credential("work").unwrap();
    h.set_default_account("codex", Some("work")).unwrap();
    let metadata = r#"{"email":"work@example.com","first_auth_ms":123,"last_auth_ms":456,"plan":"plus"}"#;
    h.db.set_meta("account-meta:work", metadata).unwrap();
    let quota = r#"{"generation":"fixture","attempted_ms":123,"next_ms":456,"readings":{}}"#;
    h.db.set_meta("account-quota-v1/work", quota).unwrap();
    h.db.set_meta("account-pool-v1",r#"{"revision":"before","projects":["demo"],"members":[{"account":"work","group":"work","enabled":true,"models":[] }]}"#).unwrap();
    h.db.set_meta(
        "account-pool-v2",
        r#"{"revision":"before","projects":[],"members":[{"account":"work","group":"work","models":[]}]}"#,
    )
    .unwrap();
    h.db.set_meta(
        "account-job:demo:fixture",
        r#"{"args":{"account":"work","profile":"work","task":"work"},"view":{"reason":"work"}}"#,
    )
    .unwrap();
    h.db.set_meta("image-job:fixture", r#"{"begin":{"account":"work"},"view":{"account":"work","state":"succeeded"}}"#)
        .unwrap();
    let binding = crate::account_policy::PaneAccount {
        run: "demo/t-0001".into(),
        account: "work".into(),
        harness: "codex".into(),
        machine: "m".into(),
        pane: Some("ended".into()),
        profile: Some("work".into()),
    };
    h.save_account_run(&binding).unwrap();
    h.inbox_add("demo", "account", "work", "끊김", "확인 필요: work");
    let mut rec = crate::orch::ThreadRec::starting(
        "demo",
        "작업",
        "m",
        "work",
        aaa_proto::Role::Worker,
        aaa_proto::ThreadKind::Dir,
        0,
    );
    rec.id = "t-0001".into();
    rec.account = Some("work".into());
    rec.status = aaa_proto::ThreadStatus::Parked;
    h.db.thread_import("demo", &rec.id, "parked", &serde_json::to_string(&rec).unwrap()).unwrap();
    // 현 schema에 별도 account 칸이 없는 task/profile 이름은 계정 별칭과 같아도 그대로다.
    let path = h.project_dir("demo").unwrap().join("automations");
    std::fs::create_dir_all(&path).unwrap();
    let path = path.join("fixture.toml");
    std::fs::write(&path, "# work\n[do.start]\naccount = 'work'\nprofile = 'work'\ntext = 'work'\n").unwrap();
    let sealed = std::fs::read(h.state.join("accounts/work.json")).unwrap();
    h.call(DeviceRole::Controller, "device:phone", request("renamed")).await.unwrap();
    assert!(!h.state.join("accounts/work.json").exists());
    let new = h.accounts.credential("renamed").unwrap();
    assert_eq!((new.harness, new.version, new.auth_generation), (old.harness, old.version, old.auth_generation));
    assert!(new.secret.0 == old.secret.0, "인증은 바뀌지 않는다");
    assert!(sealed != std::fs::read(h.state.join("accounts/renamed.json")).unwrap(), "암호문을 새로 만든다");
    assert!(h.accounts.credential("work").is_err());
    assert_eq!(h.default_account("codex").unwrap().as_deref(), Some("renamed"));
    for (prefix, value) in [("account-meta:", metadata), ("account-quota-v1/", quota)] {
        assert!(h.db.meta(&format!("{prefix}work")).unwrap().is_none());
        assert_eq!(h.db.meta(&format!("{prefix}renamed")).unwrap().as_deref(), Some(value));
    }
    assert!(h.db.inbox_open_exists("demo", "account", "renamed", "끊김").unwrap());
    assert!(!h.db.inbox_open_exists("demo", "account", "work", "끊김").unwrap());
    assert_eq!(h.db.inbox_list(Some("demo"), true).unwrap()[0].subject, "renamed");
    let pool = h.account_pool().unwrap();
    assert_eq!(pool.members[0].account, "renamed");
    assert_eq!(pool.members[0].group, "renamed");
    let legacy: serde_json::Value = serde_json::from_str(&h.db.meta("account-pool-v1").unwrap().unwrap()).unwrap();
    assert_eq!(legacy["members"][0]["group"], "work");
    assert_ne!(pool.revision, "before");
    let rec = h.load("demo", "t-0001").unwrap();
    assert_eq!(rec.account.as_deref(), Some("renamed"));
    assert_eq!(rec.profile, "work");
    assert_eq!(h.account_run("demo/t-0001").unwrap().unwrap().account, "renamed");
    assert_eq!(h.account_run("demo/t-0001").unwrap().unwrap().profile.as_deref(), Some("work"));
    let job: serde_json::Value =
        serde_json::from_str(&h.db.meta("account-job:demo:fixture").unwrap().unwrap()).unwrap();
    assert_eq!(job["args"]["account"], "renamed");
    assert_eq!(job["args"]["profile"], "work");
    assert_eq!(job["args"]["task"], "work");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "# work\n[do.start]\naccount = \"renamed\"\nprofile = 'work'\ntext = 'work'\n"
    );
    assert!(h.db.meta(JOURNAL).unwrap().is_none());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(h.state.join("accounts/renamed.json")).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let detail: String =
        l.conn().query_row("SELECT detail FROM audit WHERE action='account_rename'", [], |r| r.get(0)).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&detail).unwrap(),
        serde_json::json!({"from":"work","to":"renamed"})
    );
    // 파일명·이름 칸만 바꾼 암호문은 새 AAD로 열리지 않는다.
    let path = h.state.join("accounts/renamed.json");
    let mut tampered: serde_json::Value = serde_json::from_slice(&sealed).unwrap();
    tampered["name"] = "renamed".into();
    std::fs::write(&path, tampered.to_string()).unwrap();
    assert!(h.accounts.credential("renamed").is_err());
}

#[tokio::test]
async fn rename_rejects_agents_collisions_and_running_bindings_without_changes() {
    let l = Lab::new();
    let h = &l.h;
    for (role, actor) in [
        (DeviceRole::Reader, "device:tv"),
        (DeviceRole::Local, "local:cli@coordinator"),
        (DeviceRole::Local, "worker:t-0001"),
        (DeviceRole::Controller, "worker:t-0001"),
    ] {
        assert_eq!(h.call(role, actor, request("new")).await.unwrap_err().code, ErrorCode::Forbidden);
    }
    for name in ["work", "bad/name", ""] {
        assert_eq!(
            h.call(DeviceRole::Local, "local:cli", request(name)).await.unwrap_err().code,
            ErrorCode::BadRequest
        );
    }
    h.accounts.import("taken", "claude", "fixture-token-0123456789").unwrap();
    assert_eq!(h.call(DeviceRole::Local, "local:cli", request("taken")).await.unwrap_err().code, ErrorCode::NameTaken);
    // 준비 중 binding은 아직 pane이 없어도 중계·시작 경합으로 거부한다.
    let binding = crate::account_policy::PaneAccount {
        run: "demo/t-0002".into(),
        account: "work".into(),
        harness: "codex".into(),
        machine: "m".into(),
        pane: None,
        profile: None,
    };
    h.save_account_run(&binding).unwrap();
    assert_eq!(h.call(DeviceRole::Local, "local:cli", request("new")).await.unwrap_err().code, ErrorCode::Conflict);
    assert!(h.accounts.credential("work").is_ok());
    assert!(h.accounts.credential("new").is_err());
    assert!(h.db.meta(JOURNAL).unwrap().is_none());
}

#[tokio::test]
async fn db_failure_rolls_back_files_vault_and_references_and_startup_recovers_both_sides() {
    let l = Lab::new();
    let h = &l.h;
    h.set_default_account("codex", Some("work")).unwrap();
    let file = h.project_dir("demo").unwrap().join("project.toml");
    let before = "account = 'work'\n".to_string();
    std::fs::write(&file, &before).unwrap();
    l.conn().execute_batch("CREATE TRIGGER fail_account_rename BEFORE INSERT ON audit WHEN NEW.action='account_rename' BEGIN SELECT RAISE(FAIL,'fixture rename commit failure'); END;").unwrap();
    let e = h.call(DeviceRole::Local, "local:cli", request("new")).await.unwrap_err();
    assert!(e.message.contains("fixture rename commit failure"), "{}", e.message);
    assert_eq!(h.default_account("codex").unwrap().as_deref(), Some("work"));
    assert!(h.accounts.credential("work").is_ok());
    assert!(h.accounts.credential("new").is_err());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), before);
    assert!(h.db.meta(JOURNAL).unwrap().is_none());
    l.conn().execute_batch("DROP TRIGGER fail_account_rename").unwrap();
    for committed in [false, true] {
        let after = format!("{before}\n# 계정 이름 이전 fixture\n");
        let journal = Journal {
            from: "work".into(),
            to: "new".into(),
            committed,
            files: vec![FileChange { path: file.clone(), before: before.clone(), after: after.clone() }],
        };
        h.db.set_meta(JOURNAL, &serde_json::to_string(&journal).unwrap()).unwrap();
        h.accounts.stage_rename("work", "new").unwrap();
        durable(&file, after.as_bytes()).unwrap();
        // 실제 Host::open이 인증을 읽기 전 복구한다(실패/성공 결정점 모두).
        let mut cfg = h.cfg.clone();
        cfg.notify.web_push = false;
        let reopened = Host::open(cfg, h.state.clone()).unwrap();
        assert!(reopened.accounts.credential(if committed { "new" } else { "work" }).is_ok());
        assert!(reopened.accounts.credential(if committed { "work" } else { "new" }).is_err());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), if committed { after } else { before.clone() });
        assert!(reopened.db.meta(JOURNAL).unwrap().is_none());
    }
}

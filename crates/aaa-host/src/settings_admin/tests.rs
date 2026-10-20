use super::*;
use aaa_proto::{DeviceRole, FrontRequest};
use std::sync::Arc;

struct Lab {
    root: PathBuf,
    h: Arc<Host>,
}
impl Drop for Lab {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
impl Lab {
    fn host_toml(&self) -> String {
        std::fs::read_to_string(self.root.join("c/host.toml")).unwrap_or_default()
    }
    async fn call(&self, req: FrontRequest) -> Result<FrontResponse, ErrorBody> {
        self.h.call(DeviceRole::Controller, "device:c", req).await
    }
    async fn safety(&self, scope: SettingsScope) -> SafetyView {
        match self.call(FrontRequest::SafetyShow { project: "demo".into(), scope }).await.unwrap() {
            FrontResponse::Safety(v) => v,
            other => panic!("{other:?}"),
        }
    }
    async fn profiles(&self) -> ProfileAdminView {
        match self.call(FrontRequest::ProfileAdmin).await.unwrap() {
            FrontResponse::ProfileAdmin(v) => v,
            other => panic!("{other:?}"),
        }
    }
}
fn lab(host_toml: &str) -> Lab {
    let root = std::env::temp_dir().join(format!("aaa-settings-admin-{}", aaa_common::util::new_id()));
    std::fs::create_dir_all(root.join("c")).unwrap();
    std::fs::write(root.join("c/host.toml"), host_toml).unwrap();
    let mut cfg = aaa_config::HostConfig::default();
    cfg.projects.root = root.join("projects").to_string_lossy().into();
    cfg.notify.web_push = false;
    let h = Host::open(cfg, root.join("host")).unwrap();
    h.config_dir.set(root.join("c")).unwrap();
    h.project_create("demo", Default::default(), "local:cli").unwrap();
    Lab { root, h }
}

/// 웹은 yolo·start_threads·trust_screens·coordinator_deploy만, 본 판 위에서만 바꾼다(답 2A)
#[tokio::test]
async fn the_web_sets_only_four_safety_keys_on_the_revision_it_read() {
    let lab = lab("schema = 1 # 내 주석\n\n[safety.default]\nthread_agent_args = [\"--verbose\"]\n");
    let v = lab.safety(SettingsScope::Project).await;
    let editable: Vec<_> = v.rows.iter().filter(|r| r.editable).map(|r| r.key.as_str()).collect();
    assert_eq!(editable, WEB_SAFETY_KEYS);
    let args = v.rows.iter().find(|r| r.key == "thread_agent_args").unwrap();
    assert_eq!((args.value.as_str(), args.source.as_str()), ("--verbose", "모든 프로젝트"));

    let set = |scope, key: &str, value: Option<&str>, expected: &str| FrontRequest::SafetySet {
        project: "demo".into(),
        scope,
        key: key.into(),
        value: value.map(str::to_string),
        expected: expected.into(),
    };
    // 명령·인자 목록은 웹에서 바꾸지 않는다
    let before = lab.host_toml();
    for key in ["thread_agent_args", "coordinator_agent_args", "routine_commands"] {
        let e = lab.call(set(SettingsScope::Project, key, Some("on"), &v.rev)).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::Forbidden, "{key}: {}", e.message);
    }
    assert_eq!(lab.host_toml(), before);

    let FrontResponse::Safety(after) = lab.call(set(SettingsScope::Project, "yolo", Some("on"), &v.rev)).await.unwrap()
    else {
        panic!()
    };
    let yolo = after.rows.iter().find(|r| r.key == "yolo").unwrap();
    assert_eq!((yolo.value.as_str(), yolo.source.as_str()), ("on", "프로젝트"));
    assert_eq!(after.notes, [safety::RESTART_NOTE]);
    let text = lab.host_toml();
    assert!(text.starts_with("schema = 1 # 내 주석\n") && text.contains("[safety.demo]\nyolo = true"), "{text}");

    // 그사이 바뀐 판에는 쓰지 않는다(앞의 판으로 다시 보내면 conflict, 파일 그대로)
    let e = lab.call(set(SettingsScope::Global, "start_threads", Some("auto"), &v.rev)).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Conflict);
    assert_eq!(lab.host_toml(), text);
    // 모든 프로젝트 기본은 [safety.default]에 쓰고, default는 그 표의 key를 지운다
    lab.call(set(SettingsScope::Global, "start_threads", Some("auto"), &after.rev)).await.unwrap();
    let g = lab.safety(SettingsScope::Global).await;
    assert!(g.rows.iter().any(|r| r.key == "start_threads" && r.value == "auto" && r.source == "모든 프로젝트"));
    lab.call(set(SettingsScope::Project, "yolo", None, &g.rev)).await.unwrap();
    assert!(!lab.host_toml().contains("[safety.demo]"), "{}", lab.host_toml());
}

/// 웹은 custom profile(실행 명령)을 만들거나 고치거나 지우지 못하고, env 값을 받지 못한다(답 1A)
#[tokio::test]
async fn web_profiles_never_touch_custom_commands_or_show_env_values() {
    let lab = lab(concat!(
        "schema = 1\n\n[profiles.tool]\nagent = \"custom\"\ncommand = [\"/opt/tool\", \"--run\"]\n\n",
        "[profiles.opus]\nagent = \"claude\"\nmodel = \"opus\"\nenv = { API_TOKEN = \"zq-envvalue-not-shown\" }\n"
    ));
    let v = lab.profiles().await;
    let json = serde_json::to_string(&v).unwrap();
    assert!(!json.contains("zq-envvalue-not-shown") && !json.contains("/opt/tool"), "{json}");
    let opus = v.profiles.iter().find(|p| p.name == "opus").unwrap();
    assert_eq!(
        (opus.env_names.as_slice(), opus.launch.as_slice()),
        (&["API_TOKEN".to_string()][..], &["--model".to_string(), "opus".to_string()][..])
    );
    assert!(v.profiles.iter().any(|p| p.name == "tool" && p.custom));
    assert!(v.profiles.iter().any(|p| p.name == "codex" && p.builtin));

    let before = lab.host_toml();
    let edit = |name: &str, harness: Option<&str>| ProfileChangeReq::Edit {
        name: name.into(),
        harness: harness.map(str::to_string),
        model: None,
        effort: None,
        description: None,
        args: Some(vec!["--x".into()]),
    };
    let add_custom = ProfileChangeReq::Add {
        name: "mine".into(),
        harness: "custom".into(),
        model: String::new(),
        effort: String::new(),
        description: String::new(),
        args: vec![],
    };
    for change in
        [add_custom, edit("tool", None), edit("opus", Some("custom")), ProfileChangeReq::Remove { name: "tool".into() }]
    {
        let e = lab
            .call(FrontRequest::ProfileApply {
                project: "demo".into(),
                change: change.clone(),
                expected: v.rev.clone(),
            })
            .await
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::Forbidden, "{change:?}: {}", e.message);
    }
    assert_eq!(lab.host_toml(), before);

    // 미리보기는 쓰지 않고, 적용은 미리 본 그대로 쓴다. env는 그대로 남는다
    let change = edit("opus", None);
    let FrontResponse::ProfilePlan(plan) =
        lab.call(FrontRequest::ProfilePlan { project: "demo".into(), change: change.clone() }).await.unwrap()
    else {
        panic!()
    };
    assert_eq!(lab.host_toml(), before);
    assert_eq!(plan.after.as_ref().unwrap().launch, ["--model", "opus", "--x"]);
    let FrontResponse::ProfileApplied { view, .. } = lab
        .call(FrontRequest::ProfileApply { project: "demo".into(), change: change.clone(), expected: plan.rev.clone() })
        .await
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(view.profiles.iter().find(|p| p.name == "opus").unwrap().launch, plan.after.unwrap().launch);
    assert!(lab.host_toml().contains("zq-envvalue-not-shown"));
    // 본 판이 낡았으면 conflict
    let e =
        lab.call(FrontRequest::ProfileApply { project: "demo".into(), change, expected: plan.rev }).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Conflict);
}

/// 허용 목록·기본값은 있는 profile 이름만 받고, 프로젝트 표나 [defaults]에 쓴다
#[tokio::test]
async fn allow_lists_and_defaults_take_only_known_profiles() {
    let lab = lab("schema = 1\n\n[profiles.opus]\nagent = \"claude\"\n");
    let rev = lab.profiles().await.rev;
    let apply = |change, expected: &str| FrontRequest::ProfileApply {
        project: "demo".into(),
        change,
        expected: expected.into(),
    };
    let allow = |names: &[&str]| ProfileChangeReq::Allow {
        scope: SettingsScope::Project,
        role: "thread".into(),
        names: Some(names.iter().map(|s| s.to_string()).collect()),
    };
    let e = lab.call(apply(allow(&["opus", "nope"]), &rev)).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::BadRequest, "{}", e.message);
    lab.call(apply(allow(&["opus", "codex"]), &rev)).await.unwrap();
    assert!(lab.host_toml().contains("[safety.demo]\nthread_profiles = [\"opus\", \"codex\"]"), "{}", lab.host_toml());
    assert!(lab.h.check_profile_allowed("demo", "opus").is_ok());
    assert!(lab.h.check_profile_allowed("demo", "claude").is_err());

    let rev = lab.profiles().await.rev;
    lab.call(apply(ProfileChangeReq::Default { role: "coordinator".into(), name: "opus".into() }, &rev)).await.unwrap();
    assert_eq!(lab.h.default_coordinator_profile("demo"), "opus");
}

/// 설정을 바꾸는 요청은 사람만(세션 안 agent 거부), 읽기도 보기 전용 기기는 못 한다
#[tokio::test]
async fn settings_changes_refuse_session_agents_and_reads_refuse_readers() {
    let lab = lab("schema = 1\n");
    let rev = lab.safety(SettingsScope::Project).await.rev;
    let writes = [
        FrontRequest::SafetySet {
            project: "demo".into(),
            scope: SettingsScope::Project,
            key: "yolo".into(),
            value: Some("on".into()),
            expected: rev.clone(),
        },
        FrontRequest::ProfileApply {
            project: "demo".into(),
            change: ProfileChangeReq::Remove { name: "opus".into() },
            expected: rev.clone(),
        },
        FrontRequest::WorkRootCheck { machine: "mac-a".into(), path: None, mount: None },
        FrontRequest::WorkRootSet { machine: "mac-a".into(), path: "/w".into(), mount: None },
        FrontRequest::HookSet { machine: "mac-a".into(), install: true },
    ];
    for req in writes {
        for actor in ["local:cli@demo:t-0001", "worker:t-0001", "detached:x"] {
            let e = lab.h.call(DeviceRole::Local, actor, req.clone()).await.unwrap_err();
            assert_eq!(e.code, ErrorCode::Forbidden, "{actor} {req:?}");
            assert!(e.message.contains("safety·profile·작업 root·hook"), "{}", e.message);
        }
    }
    assert_eq!(lab.host_toml(), "schema = 1\n");
    for req in [
        FrontRequest::SafetyShow { project: "demo".into(), scope: SettingsScope::Project },
        FrontRequest::ProfileAdmin,
        FrontRequest::WorkRootShow { machine: "mac-a".into() },
        FrontRequest::HookStatus { machine: "mac-a".into() },
    ] {
        let e = lab.h.call(DeviceRole::Reader, "device:r", req.clone()).await.unwrap_err();
        assert!(e.message.contains("보기 권한"), "{req:?}: {}", e.message);
    }
}

/// 작업 root는 절대 경로만 머신에 보낸다(`~`·상대 경로는 agent에 닿기 전에 거부)
#[tokio::test]
async fn work_root_takes_only_absolute_paths() {
    let lab = lab("schema = 1\n");
    for path in ["~/work", "work", "./w", "C:work"] {
        let e = lab
            .call(FrontRequest::WorkRootSet { machine: "mac-a".into(), path: path.into(), mount: None })
            .await
            .unwrap_err();
        assert!(e.code == ErrorCode::BadRequest && e.message.contains("절대 경로"), "{path}: {}", e.message);
    }
    for path in ["/work", "C:\\work", "D:/work"] {
        let e = lab
            .call(FrontRequest::WorkRootSet { machine: "mac-a".into(), path: path.into(), mount: None })
            .await
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::NotFound, "{path}: 머신이 없어서만 거부된다: {}", e.message);
    }
}

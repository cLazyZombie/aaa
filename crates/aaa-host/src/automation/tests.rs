use super::*;
#[path = "convert_batch_tests.rs"]
mod convert_batch_tests;
use std::path::PathBuf;
struct Project(PathBuf);
impl Project {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("aaa-auto-{}", aaa_common::util::new_id()));
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
    fn write(&self, file: &str, text: &str) {
        let path = self.0.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn automation_ids_survive_edits_restore_conversion_restart_and_project_rename() {
    let p = Project::new();
    p.write("projects/demo/PROJECT.md", "# demo\n");
    p.write("projects/demo/routines/check.md", "+++\nschedule='every 10m'\n+++\n자료\n");
    let mut cfg = aaa_config::HostConfig::default();
    cfg.projects.root = p.0.join("projects").to_string_lossy().into();
    cfg.notify.web_push = false;
    let h = crate::Host::open(cfg.clone(), p.0.join("host")).unwrap();
    let id = h.routines("demo").unwrap()[0].short_id.clone().unwrap();
    assert_eq!(id, "a-0001");
    // 같은 이름의 TOML 변환은 같은 규칙이다.
    h.automation_convert("demo", false, None, "local:cli").await.unwrap();
    let doc = h.automation_read("demo", "check").unwrap();
    assert_eq!(h.routines("demo").unwrap()[0].short_id.as_deref(), Some(id.as_str()));
    p.write(
        "projects/demo/automations/check.toml",
        &doc.text.replace("title = \"check\"", "title = \"긴 제목으로 수정\""),
    );
    assert_eq!(h.routines("demo").unwrap()[0].short_id.as_deref(), Some(id.as_str()));
    let doc = h.automation_read("demo", "check").unwrap();
    h.automation_delete("demo", "check", &doc.sha256, "local:cli").await.unwrap();
    assert!(h.routines("demo").unwrap().is_empty());
    p.write("projects/demo/automations/other.toml", "[on.schedule]\nevery='10m'\n[do.inbox]\n");
    assert_eq!(h.routines("demo").unwrap()[0].short_id.as_deref(), Some("a-0002"));
    let trash = h.automation_trash("demo").unwrap();
    h.automation_restore("demo", &trash[0].id, "local:cli").await.unwrap();
    assert_eq!(
        h.routines("demo").unwrap().iter().find(|r| r.name == "check").unwrap().short_id.as_deref(),
        Some(id.as_str())
    );
    drop(h);
    let h = crate::Host::open(cfg, p.0.join("host")).unwrap();
    assert_eq!(
        h.routines("demo").unwrap().iter().find(|r| r.name == "check").unwrap().short_id.as_deref(),
        Some(id.as_str())
    );
    h.db.project_rename("demo", "renamed", str::to_owned).unwrap();
    assert_eq!(h.db.automation_short_id("renamed", "check").unwrap(), id);
    assert_eq!(h.db.project_export("renamed").unwrap()["automation_ids"].as_array().unwrap().len(), 2);
}

#[test]
fn public_toml_contract_loads_real_files_and_preserves_legacy_command_approval() {
    let p = Project::new();
    p.write("automations/note.md", "자료를 분류한다.\n");
    let new = "title = '확인'\n[on.command]\nevery = '10m'\ncommand = 'echo data'\nmachine = 'mac-a'\n[do.inbox]\ntext_file = 'automations/note.md'\n[limits]\nmax_per_day = 3\n";
    p.write("automations/check.toml", new);
    let (all, bad) = crate::routine::load_all(&p.0);
    assert!(bad.is_empty(), "{bad:?}");
    assert_eq!(all.len(), 1);
    let old = crate::routine::parse(
        "check",
        "+++\nschedule='every 10m'\ncommand='echo data'\nmachine='mac-a'\n+++\n자료를 분류한다.\n",
    )
    .unwrap();
    assert_eq!(all[0].trigger, old.trigger);
    assert_eq!(all[0].command_sha256(), old.command_sha256());
    assert_eq!(all[0].prompt, old.prompt);
    assert_eq!(all[0].limits.max_per_day, Some(3));
    let unlimited = new.replace("max_per_day = 3", "max_per_day = 0\nmin_gap = 0");
    let unlimited = parse(&p.0, "check", &unlimited).unwrap();
    assert_eq!(unlimited.limits.max_per_day, None);
    assert_eq!(unlimited.limits.min_gap.as_deref(), Some("0m"));
    let cosmetic =
        new.replace("title = '확인'", "# 주석과 순서는 승인 범위가 아니다\nenabled = true\ntitle = '다른 제목'");
    assert_eq!(parse(&p.0, "check", &cosmetic).unwrap().command_sha256(), old.command_sha256());
    p.write("routines/check.md", "+++\nschedule='every 10m'\n+++\nold\n");
    let (all, bad) = crate::routine::load_all(&p.0);
    assert!(all.is_empty());
    assert_eq!(bad.len(), 2);
    assert!(bad.iter().all(|(_, _, e)| e.contains("같은 이름")));
}

#[test]
fn invalid_rules_report_source_position_and_field_and_never_load() {
    let p = Project::new();
    let cases = [
        ("[on.git]\nrepo='app'\nbranches=['main']\nunknown = 1\n[do.inbox]\n", "on.git", ":4:"),
        ("[on.git]\nrepo='app'\nbranches=['main']\n[on.ci]\nrepo='app'\nbranches=['main']\n[do.inbox]\n", "on", ":1:"),
        ("[on.schedule]\nevery='10m'\n[do.inbox]\ntext='x'\ntext_file='note.md'\n", "do.inbox.text_file", ":5:"),
        ("[on.schedule]\nevery='10m'\n[do.inbox]\ntext_file='../outside.md'\n", "do.inbox.text_file", ":4:"),
        ("[on.schedule]\nevery='10m'\n[do.inbox]\n[limits]\nmax_per_day=-1\n", "limits.max_per_day", ":5:"),
    ];
    for (text, field, line) in cases {
        let error = parse(&p.0, "bad", text).unwrap_err();
        assert!(error.starts_with("automations/bad.toml:"), "{error}");
        assert!(error.contains(field) && error.contains(line), "{field} {line}: {error}");
    }
    let error = crate::routine::parse("bad", "+++\nschedule='every 10m'\nunknown=1\n+++\n").unwrap_err();
    assert!(error.starts_with("routines/bad.md:3:"), "{error}");
    assert!(error.contains("unknown"), "{error}");
}

#[cfg(unix)]
#[test]
fn text_file_rejects_final_and_intermediate_symlinks() {
    let p = Project::new();
    p.write("real/text.md", "data");
    std::os::unix::fs::symlink(p.0.join("real/text.md"), p.0.join("link.md")).unwrap();
    std::os::unix::fs::symlink(p.0.join("real"), p.0.join("folder")).unwrap();
    for path in ["link.md", "folder/text.md"] {
        let error =
            parse(&p.0, "bad", &format!("[on.schedule]\nevery='10m'\n[do.inbox]\ntext_file='{path}'\n")).unwrap_err();
        assert!(error.contains("symlink") && error.contains("do.inbox.text_file"), "{error}");
    }
}

#[test]
fn converter_preserves_values_archives_sources_and_second_run_is_quiet() {
    let p = Project::new();
    let body = "자료를 읽는다.\n".repeat(600);
    let legacy = format!("+++\nschedule='daily 03:00'\ncommand='echo data'\nmachine='mac-a'\n+++\n{body}");
    p.write("routines/read.md", &legacy);
    let mission = "+++\ntitle='감시'\nprofile='fake'\nevents=['thread_failed']\nrepos=['app']\nschedule='daily 04:00'\nmax_wakes=8\nmax_minutes=25\nreset='daily 5:00'\n+++\n자료를 분류한다.\n";
    p.write("missions/watch.md", mission);
    let expected = crate::routine::parse("read", &legacy).unwrap();
    let files = convert(&p.0).unwrap();
    for file in [
        "routines/.converted/read.md",
        "automations/read.toml",
        "automations/read.brief.md",
        "missions/.converted/watch.md",
        "automations/watch.brief.md",
        "automations/watch.memory/memory.md",
        "automations/automation.schema.json",
    ] {
        assert!(files.iter().any(|f| f == file), "{file}: {files:?}");
    }
    assert_eq!(std::fs::read_to_string(p.0.join("routines/.converted/read.md")).unwrap(), legacy);
    assert_eq!(std::fs::read_to_string(p.0.join("missions/.converted/watch.md")).unwrap(), mission);
    assert_eq!(std::fs::read_to_string(p.0.join("automations/watch.brief.md")).unwrap(), "자료를 분류한다.\n");
    assert!(!p.0.join("routines/read.md").exists());
    let (rules, errors) = crate::routine::load_all(&p.0);
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(rules.len(), 3);
    let read = rules.iter().find(|r| r.name == "read").unwrap();
    assert_eq!(read.command_sha256(), expected.command_sha256());
    assert_eq!(read.trigger, expected.trigger);
    assert_eq!(read.prompt, expected.prompt);
    assert!(
        matches!(&rules.iter().find(|r|r.name=="watch").unwrap().trigger,crate::routine::Trigger::Thread(t) if t.events==["thread_failed"] && t.repos==["app"])
    );
    assert!(convert(&p.0).unwrap().is_empty());
}

#[test]
fn converter_collision_fails_before_any_source_or_definition_changes() {
    let p = Project::new();
    let source = "+++\nschedule='every 1m'\n+++\n확인\n";
    p.write("routines/watch.md", source);
    p.write("missions/watch.md", "+++\nprofile='fake'\nevents=['thread_failed']\n+++\n자료\n");
    let error = convert(&p.0).unwrap_err();
    assert!(error.contains("겹친다"), "{error}");
    assert_eq!(std::fs::read_to_string(p.0.join("routines/watch.md")).unwrap(), source);
    assert!(!p.0.join("routines/.converted").exists());
    assert!(!p.0.join("automations").exists());
}

#[test]
fn public_schema_artifacts_match_rust_types_and_exclusive_tables() {
    let schemas = [("automation.schema.json", schema())];
    for (file, value) in schemas {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs").join(file);
        let text = format!("{}\n", serde_json::to_string_pretty(&value).unwrap());
        if std::env::var_os("AAA_UPDATE_AUTOMATION_SCHEMA").is_some() {
            std::fs::write(&path, &text).unwrap();
        }
        assert_eq!(std::fs::read_to_string(path).unwrap(), text, "Rust 정본 타입이 바뀌면 공개 스키마도 함께 검토한다");
    }
    let rule = schema();
    assert_eq!(rule["$defs"]["AutomationTrigger"]["oneOf"].as_array().unwrap().len(), 2);
    assert_eq!(rule["$defs"]["AutomationAction"]["oneOf"].as_array().unwrap().len(), 5);
    assert_eq!(rule["$defs"]["AutomationNotice"]["not"]["required"], serde_json::json!(["text", "text_file"]));
}

fn convert(root: &std::path::Path) -> Result<Vec<String>, String> {
    super::convert::apply(root, super::convert::prepare(root, "mac-a")?)
}

#[test]
fn converter_reuses_identical_backups_and_numbers_different_followup_sources() {
    for identical in [true, false] {
        let p = Project::new();
        let source = crate::routine::PR_FOLLOWUP_TEMPLATE;
        let old =
            if identical { source } else { "+++\non='pr'\nevents=['review']\nenabled=false\n+++\nOld English copy\n" };
        p.write(crate::routine::PR_FOLLOWUP, source);
        p.write("routines/.converted/pr-followup.md", old);
        // 번호가 이미 쓰였으면 그 다음 이름을 고른다.
        p.write("routines/.converted/pr-followup.1.md", "earlier copy\n");
        let before = snapshot(&p.0);
        let plan = super::convert::prepare(&p.0, "mac-a").unwrap();
        let backup =
            if identical { "routines/.converted/pr-followup.md" } else { "routines/.converted/pr-followup.2.md" };
        assert!(plan.listing().contains(&backup.to_string()), "{:?}", plan.listing());
        assert_eq!(snapshot(&p.0), before, "미리보기가 썼다");
        assert_eq!(
            super::convert::apply(&p.0, plan)
                .unwrap()
                .iter()
                .filter(|f| f.starts_with("routines/.converted/"))
                .collect::<Vec<_>>(),
            vec![&backup.to_string()]
        );
        assert_eq!(std::fs::read_to_string(p.0.join("routines/.converted/pr-followup.md")).unwrap(), old);
        assert_eq!(
            std::fs::read_to_string(p.0.join("routines/.converted/pr-followup.1.md")).unwrap(),
            "earlier copy\n"
        );
        assert_eq!(std::fs::read_to_string(p.0.join(backup)).unwrap(), source);
        let expected = crate::routine::parse("pr-followup", source).unwrap();
        let (rules, errors) = crate::routine::load_all(&p.0);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].file, "automations/pr-followup.toml");
        assert_eq!(rules[0].trigger, expected.trigger);
        assert_eq!(rules[0].prompt, expected.prompt);
        assert_eq!(rules[0].action, expected.action);
        assert_eq!(rules[0].enabled, expected.enabled);
        assert!(convert(&p.0).unwrap().is_empty());
    }
}

#[test]
fn converter_rollback_preserves_reused_backup_and_restores_source() {
    let p = Project::new();
    let source = crate::routine::PR_FOLLOWUP_TEMPLATE;
    p.write(crate::routine::PR_FOLLOWUP, source);
    p.write("routines/.converted/pr-followup.md", source);
    let plan = super::convert::prepare(&p.0, "mac-a").unwrap();
    // 계획 뒤 출력 자리가 차면 기존 파일과 재사용한 보존 사본을 모두 지킨다.
    p.write("automations/pr-followup.toml", "occupied\n");
    let before = snapshot(&p.0);
    let error = super::convert::apply(&p.0, plan).unwrap_err();
    assert!(error.contains("원본 복구 완료"), "{error}");
    assert_eq!(snapshot(&p.0), before);
}

#[test]
fn converter_revalidates_archive_choices_and_keeps_toml_collisions_read_only() {
    for reuse in [true, false] {
        let p = Project::new();
        let source = crate::routine::PR_FOLLOWUP_TEMPLATE;
        p.write(crate::routine::PR_FOLLOWUP, source);
        if reuse {
            p.write("routines/.converted/pr-followup.md", source);
        }
        let plan = super::convert::prepare(&p.0, "mac-a").unwrap();
        let hash = plan.sha256();
        p.write("routines/.converted/pr-followup.md", "changed backup\n");
        let before = snapshot(&p.0);
        let next = super::convert::prepare(&p.0, "mac-a").unwrap();
        assert_ne!(hash, next.sha256(), "보존 위치 변경도 본 계획에 포함한다");
        let error = super::convert::apply(&p.0, plan).unwrap_err();
        assert!(error.contains("계획 뒤 사본"), "{error}");
        assert_eq!(snapshot(&p.0), before);
        p.write("automations/pr-followup.toml", "existing rule\n");
        let before = snapshot(&p.0);
        assert!(convert(&p.0).unwrap_err().contains("이미 있는 파일"));
        assert_eq!(snapshot(&p.0), before);
    }
}

#[cfg(unix)]
#[test]
fn converter_never_reuses_a_symlink_as_an_archive() {
    for folder in [true, false] {
        let p = Project::new();
        p.write(crate::routine::PR_FOLLOWUP, crate::routine::PR_FOLLOWUP_TEMPLATE);
        p.write("saved/pr-followup.md", crate::routine::PR_FOLLOWUP_TEMPLATE);
        if folder {
            std::os::unix::fs::symlink(p.0.join("saved"), p.0.join("routines/.converted")).unwrap();
        } else {
            std::fs::create_dir_all(p.0.join("routines/.converted")).unwrap();
            std::os::unix::fs::symlink(
                p.0.join("saved/pr-followup.md"),
                p.0.join("routines/.converted/pr-followup.md"),
            )
            .unwrap();
        }
        let before = snapshot(&p.0);
        assert!(convert(&p.0).is_err());
        assert_eq!(snapshot(&p.0), before);
        assert!(p.0.join(crate::routine::PR_FOLLOWUP).is_file());
    }
}

#[test]
fn converter_numbers_extensionless_mission_briefs_and_old_schema_backups() {
    let p = Project::new();
    p.write("missions/watch.toml", "profile='fake'\nevents=['thread_failed']\nbrief_file='brief'\n");
    p.write("brief", "현재 본문\n");
    p.write(".converted/brief", "이전 본문\n");
    p.write("automations/automation.schema.json", "old schema\n");
    p.write("automations/.converted/automation.schema.json", "older schema\n");
    let files = convert(&p.0).unwrap();
    for (file, text) in [
        (".converted/brief", "이전 본문\n"),
        (".converted/brief.1", "현재 본문\n"),
        ("automations/.converted/automation.schema.json", "older schema\n"),
        ("automations/.converted/automation.schema.1.json", "old schema\n"),
    ] {
        assert_eq!(std::fs::read_to_string(p.0.join(file)).unwrap(), text);
    }
    assert!(files.contains(&".converted/brief.1".into()));
    assert!(files.contains(&"automations/.converted/automation.schema.1.json".into()));
    assert!(crate::routine::load_all(&p.0).1.is_empty());
}

#[test]
fn start_toml_captures_safe_brief_and_rejects_ambiguous_or_outside_files() {
    let p = Project::new();
    p.write("scratch/nightly.md", "대상 {machine}의 {repo_path}를 검사한다.\n");
    let raw = "[on.schedule]\nevery='10m'\n\n[do.start]\ntitle='점검 {date}'\nrepo='app'\nprofiles=['reviewer','helper']\nmachines=['mac-a','mac-b']\nbrief_file='scratch/nightly.md'\nwait=['timing']\n";
    let rule = parse(&p.0, "nightly", raw);
    assert!(rule.is_ok(), "do.start의 안전한 외부 brief와 명시 대기는 유효하다: {:?}", rule.err());
    let rule = rule.unwrap();
    let action = serde_json::to_value(&rule.action).unwrap();
    assert_eq!(action["start"]["repo"], "app");
    assert_eq!(action["start"]["profiles"], serde_json::json!(["reviewer", "helper"]));
    assert_eq!(action["start"]["text"], "대상 {machine}의 {repo_path}를 검사한다.");
    let zero = parse(&p.0, "nightly", &format!("{raw}wait_max=0\n[limits]\nmax_running=0\n")).unwrap();
    let Action::Start(a) = zero.action else { panic!() };
    assert_eq!(a.wait_max.as_deref(), Some("0m"));
    assert_eq!(zero.limits.max_running, None);
    for (bad, field) in [
        (raw.replace("scratch/nightly.md", "../outside.md"), "do.start.brief_file"),
        (format!("{raw}text='다른 본문'\n"), "do.start.brief_file"),
        (raw.replace("profiles=['reviewer','helper']", "profiles=[]"), "do.start.profiles"),
        (raw.replace("machines=['mac-a','mac-b']", "machines=[]"), "do.start.machines"),
    ] {
        let why = parse(&p.0, "nightly", &bad).unwrap_err();
        assert!(why.starts_with("automations/nightly.toml:") && why.contains(field), "{why}");
    }
}

#[test]
fn deploy_proposal_toml_requires_main_pushes_and_bounded_commit_threshold() {
    let p = Project::new();
    let source =
        "do = 'deploy'\n[on.git]\nrepo = 'aaa'\nbranches = ['main']\n[limits]\nmin_commits = 2\nmax_per_day = 2\n";
    p.write("automations/deploy.toml", source);
    let (rules, bad) = crate::routine::load_all(&p.0);
    assert!(bad.is_empty(), "{bad:?}");
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].action.label(), "배포 제안");
    for invalid in [
        source.replace("['main']", "['topic']"),
        source.replace("min_commits = 2", "min_commits = 0"),
        source.replace("min_commits = 2", "min_commits = 101"),
        source.replace("repo = 'aaa'", "repo = 'aaa'\nevents = ['forced']"),
        source.replace("do = 'deploy'", "[do.inbox]\ntext = '안내'"),
        "do = 'deploy'\n[on.schedule]\nevery = '1m'\n".into(),
    ] {
        p.write("automations/deploy.toml", &invalid);
        let (rules, bad) = crate::routine::load_all(&p.0);
        assert!(rules.is_empty() && !bad.is_empty(), "{invalid}");
    }
}

#[tokio::test]
async fn deploy_proposal_survives_receipt_failure_prune_and_restart_without_starting() {
    use aaa_proto::{GitChange, GitCommit};
    let p = Project::new();
    p.write("projects/demo/PROJECT.md", "# demo\n");
    p.write(
        "projects/demo/automations/deploy.toml",
        "do='deploy'\n[on.git]\nrepo='aaa'\nbranches=['main']\n[limits]\nmin_commits=2\n",
    );
    let mut cfg = aaa_config::HostConfig::default();
    cfg.projects.root = p.0.join("projects").to_string_lossy().into();
    cfg.notify.web_push = false;
    let host_root = p.0.join("host");
    let h = crate::Host::open(cfg.clone(), host_root.clone()).unwrap();
    let mut rule = crate::routine::load_all(&p.0.join("projects/demo")).0.remove(0);
    let commit = "b".repeat(40);
    let change = GitChange {
        branch: "main".into(),
        before: Some("a".repeat(40)),
        after: Some(commit.clone()),
        kind: "pushed".into(),
        commits: vec![
            GitCommit { sha: commit.clone(), subject: "/private/must-not-copy-marker".into(), owners: vec![] },
            GitCommit { sha: "c".repeat(40), subject: "second".into(), owners: vec![] },
        ],
        changed_files: 2,
        truncated: false,
    };
    let body = serde_json::json!({"change":change,"repo_path":"/private/must-not-copy-marker"}).to_string();
    let mut notices = h.notice_events.subscribe();
    h.automation_submit("demo", &rule, "git-proof", body.clone(), None).unwrap();
    // 실제 DB의 인계 영수증 저장만 실패시켜 제안 commit 뒤 ACK 전 중단을 만든다.
    let sql = rusqlite::Connection::open(host_root.join("host.db")).unwrap();
    sql.execute_batch("CREATE TRIGGER fail_proposal_receipt BEFORE INSERT ON meta WHEN NEW.key LIKE 'automation_done:%' BEGIN SELECT RAISE(ABORT, 'receipt write failed'); END;").unwrap();
    let now = aaa_common::util::now_ms();
    assert!(h.automation_drain_at("demo", &rule, now).await.is_err());
    let mut items = h.db.inbox_list(Some("demo"), true).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].item_kind, "deploy_proposal");
    assert!(items[0].body.contains(&format!("scripts/deploy-build.sh {commit}")));
    let reader = h
        .call(
            aaa_proto::DeviceRole::Reader,
            "web:reader",
            aaa_proto::FrontRequest::Inbox { project: "demo".into(), all: true, peek: true },
        )
        .await
        .unwrap();
    assert!(
        !serde_json::to_string(&reader).unwrap().contains("must-not-copy-marker"),
        "Reader inbox에 내부 경로/원문을 복제하지 않는다"
    );
    assert_eq!(notices.try_recv().unwrap().kind, aaa_proto::NoticeKind::Automation);
    assert!(notices.try_recv().is_err());
    assert_eq!(h.db.meta("rollout:active").unwrap(), None, "제안이 배포를 시작하면 안 된다");
    assert!(h.db.threads(Some("demo")).unwrap().is_empty(), "빌드 thread를 자동으로 띄우지 않는다");
    items[0].done = true;
    items[0].done_at_ms = Some(now);
    h.db.inbox_put(&items[0]).unwrap();
    h.db.inbox_prune(now + 1).unwrap();
    sql.execute_batch("DROP TRIGGER fail_proposal_receipt;").unwrap();
    drop(sql);
    drop(notices);
    drop(h);
    let h = crate::Host::open(cfg, host_root).unwrap();
    // 입력은 이전 임계값으로 이미 내구 큐에 받았다. 새 설정이 ACK 재개를 막으면 안 된다.
    rule.limits.min_commits = Some(3);
    let mut notices = h.notice_events.subscribe();
    h.automation_drain_at("demo", &rule, now + 1).await.unwrap();
    h.automation_submit("demo", &rule, "git-proof", body, None).unwrap();
    h.automation_drain_at("demo", &rule, now + 2).await.unwrap();
    assert!(h.db.inbox_list(Some("demo"), true).unwrap().is_empty(), "prune·ACK 실패·재시작 뒤 제안을 되살리지 않는다");
    assert!(notices.try_recv().is_err(), "같은 제안의 transient 알림은 반복하지 않는다");
    assert_eq!(h.db.meta("rollout:active").unwrap(), None);
    drop(h);
}

#[tokio::test]
async fn converted_followup_survives_doctor_and_project_create() {
    let p = Project::new();
    let mut cfg = aaa_config::HostConfig::default();
    cfg.projects.root = p.0.join("projects").display().to_string();
    cfg.notify.web_push = false;
    let h = crate::Host::open(cfg, p.0.join("host")).unwrap();
    h.project_create("demo", Default::default(), "local:cli").unwrap();
    let dir = p.0.join("projects/demo");
    legacy_followup(&dir);
    convert(&dir).unwrap();
    let converted = std::fs::read(dir.join("automations/pr-followup.toml")).unwrap();
    let response = h.project_files(true, "local:cli");
    assert!(!dir.join("routines/pr-followup.md").exists(), "doctor가 옛 routine을 되살렸다: {response:?}");
    assert!(crate::orch::files_problems(&dir, "demo").is_empty());
    // PROJECT.md 없는 기존 폴더에서 create를 이어가는 경로도 변환본을 지킨다.
    std::fs::remove_file(dir.join("PROJECT.md")).unwrap();
    h.project_create("demo", Default::default(), "local:cli").unwrap();
    assert!(!dir.join("routines/pr-followup.md").exists());
    assert_eq!(std::fs::read(dir.join("automations/pr-followup.toml")).unwrap(), converted);
    let (rules, errors) = crate::routine::load_all(&dir);
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(rules.iter().filter(|r| r.name == "pr-followup").count(), 1);
}

#[tokio::test]
async fn archived_followup_without_toml_is_not_revived_by_doctor_or_create() {
    let p = Project::new();
    let h = demo_host(&p);
    let dir = p.0.join("projects/demo");
    legacy_followup(&dir);
    convert(&dir).unwrap();
    std::fs::remove_file(dir.join("automations/pr-followup.toml")).unwrap();
    let before = snapshot(&dir);
    assert!(crate::orch::files_problems(&dir, "demo").is_empty());
    h.project_files(true, "local:cli");
    assert_eq!(snapshot(&dir), before);
    std::fs::remove_file(dir.join("PROJECT.md")).unwrap();
    h.project_create("demo", Default::default(), "local:cli").unwrap();
    assert_eq!(snapshot(&dir), before);
}

fn legacy_followup(root: &std::path::Path) {
    // 이관 시험의 실제 구형 파일. 새 프로젝트 기본값(v2)과 구별한다.
    std::fs::remove_file(root.join("automations/pr-followup.toml")).unwrap();
    std::fs::create_dir_all(root.join("routines")).unwrap();
    std::fs::write(root.join(crate::routine::PR_FOLLOWUP), crate::routine::PR_FOLLOWUP_TEMPLATE).unwrap();
}

fn demo_host(p: &Project) -> std::sync::Arc<crate::Host> {
    let mut cfg = aaa_config::HostConfig::default();
    cfg.projects.root = p.0.join("projects").display().to_string();
    cfg.notify.web_push = false;
    let h = crate::Host::open(cfg, p.0.join("host")).unwrap();
    h.project_create("demo", Default::default(), "local:cli").unwrap();
    h
}
/// 프로젝트 폴더의 모든 파일 내용. 미리보기·거부가 아무것도 쓰지 않았는지 본다.
fn snapshot(root: &std::path::Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    let mut out = std::collections::BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).unwrap().filter_map(Result::ok) {
            let path = e.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.insert(path.strip_prefix(root).unwrap().display().to_string(), std::fs::read(&path).unwrap());
            }
        }
    }
    out
}
fn convert_req(dry_run: bool, plan_sha256: Option<String>) -> aaa_proto::FrontRequest {
    aaa_proto::FrontRequest::AutomationConvert { project: "demo".into(), dry_run, plan_sha256 }
}

#[tokio::test]
async fn convert_preview_writes_nothing_and_runs_only_the_plan_it_showed() {
    use aaa_proto::{DeviceRole::*, ErrorCode, FrontResponse};
    let p = Project::new();
    let h = demo_host(&p);
    let dir = p.0.join("projects/demo");
    p.write("projects/demo/routines/check.md", "+++\nschedule='every 1m'\n+++\n확인\n");
    p.write(
        "projects/demo/missions/watch.md",
        "+++\nprofile='fake'\nmachine='mac-a'\nevents=['thread_failed']\n+++\n자료\n",
    );
    let before = snapshot(&dir);
    // 보기 권한도 미리보기는 본다. 쓰지 않는다
    let Ok(FrontResponse::AutomationConverted { files: preview, dry_run: true, plan_sha256 }) =
        h.call(Reader, "device:r", convert_req(true, None)).await
    else {
        panic!("미리보기 응답이 아니다")
    };
    for file in [
        "automations/check.toml",
        "automations/watch.brief.md",
        "routines/.converted/check.md",
        "missions/.converted/watch.md",
    ] {
        assert!(preview.iter().any(|f| f == file), "{file}: {preview:?}");
    }
    assert_eq!(snapshot(&dir), before, "미리보기가 파일을 썼다");
    let e = h.call(Reader, "device:r", convert_req(false, Some(plan_sha256.clone()))).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Forbidden);
    // 본 계획이 아니면(다른 hash, 미리보기 뒤 원본 변경) 아무것도 바꾸지 않는다
    let e = h.call(Controller, "device:c", convert_req(false, Some("0".repeat(64)))).await.unwrap_err();
    assert!(e.message.contains("미리보기를 다시"), "{e:?}");
    assert_eq!(snapshot(&dir), before);
    p.write("projects/demo/routines/check.md", "+++\nschedule='every 2m'\n+++\n확인\n");
    let changed = snapshot(&dir);
    let e = h.call(Controller, "device:c", convert_req(false, Some(plan_sha256))).await.unwrap_err();
    assert!(e.message.contains("미리보기를 다시"), "{e:?}");
    assert_eq!(snapshot(&dir), changed);
    let Ok(FrontResponse::AutomationConverted { files: preview, plan_sha256, .. }) =
        h.call(Controller, "device:c", convert_req(true, None)).await
    else {
        panic!("미리보기 응답이 아니다")
    };
    let Ok(FrontResponse::AutomationConverted { files, dry_run: false, .. }) =
        h.call(Controller, "device:c", convert_req(false, Some(plan_sha256))).await
    else {
        panic!("변환 응답이 아니다")
    };
    let sorted = |mut v: Vec<String>| {
        v.sort();
        v
    };
    assert_eq!(sorted(files), sorted(preview), "미리보기 목록이 실제 변환과 다르다");
    assert!(dir.join("routines/.converted/check.md").exists() && !dir.join("routines/check.md").exists());
    let Ok(FrontResponse::AutomationConverted { files, .. }) =
        h.call(Reader, "device:r", convert_req(true, None)).await
    else {
        panic!()
    };
    assert!(files.is_empty(), "변환 뒤 미리보기는 비어야 한다: {files:?}");
}

/// runtime State의 내구 JSON(자동화 큐). 보낸 결과를 모르는 묶음은 host 중단·전송 오류 뒤 이 모양으로 남는다.
fn queue_state(action: serde_json::Value, inflight: &[&str], pending: &[&str]) -> String {
    queue_state_at(action, inflight, pending, true)
}
/// `unknown`이 false면 아직 보내는 중인 묶음이다(다음 판정이 다시 만나기 전)
fn queue_state_at(action: serde_json::Value, inflight: &[&str], pending: &[&str], unknown: bool) -> String {
    let event = |id: &str| serde_json::json!({"id":id,"at_ms":1000,"owner":"t-0007","body":"{}","action":action.clone(),"source":"events"});
    serde_json::json!({
        "pending": pending.iter().map(|id| event(id)).collect::<Vec<_>>(),
        "inflight": {"id":"automation-seen","events":inflight.iter().map(|id| event(id)).collect::<Vec<_>>(),"day":0,"unknown":unknown},
    })
    .to_string()
}
fn unblock(name: &str, batch: &str, discard: bool) -> aaa_proto::FrontRequest {
    aaa_proto::FrontRequest::AutomationUnblock {
        project: "demo".into(),
        name: name.into(),
        batch: batch.into(),
        discard,
    }
}

#[tokio::test]
async fn unblock_releases_only_the_seen_message_batch_and_never_resends_it() {
    use aaa_proto::{DeviceRole::*, ErrorCode};
    let p = Project::new();
    let h = demo_host(&p);
    let message = serde_json::json!({"message":{"thread":"owner","text":"확인"}});
    p.write(
        "projects/demo/automations/msg.toml",
        "[on.thread]\nevents=['thread_failed']\n[do.message]\nthread='owner'\ntext='확인'\n",
    );
    let queue =
        |h: &crate::Host| h.routines("demo").unwrap().into_iter().find(|r| r.name == "msg").unwrap().queue.unwrap();
    let rule = crate::routine::load_all(&p.0.join("projects/demo")).0.into_iter().find(|r| r.name == "msg").unwrap();
    // 보내는 중인 묶음은 아직 모름이 아니며 풀 수 없다: 보내기와 풀기가 겹치지 않는다
    h.db.set_meta("automation:demo:msg", &queue_state_at(message.clone(), &["e1", "e2"], &["e3", "e4"], false))
        .unwrap();
    assert!(!queue(&h).inflight.unwrap().unknown);
    let e = h.call(Controller, "device:c", unblock("msg", "automation-seen", false)).await.unwrap_err();
    assert!(e.message.contains("보내는 중"), "{e:?}");
    // 다음 판정이 처리 중 묶음을 다시 만나면 모름으로 적고 다시 보내지 않는다(대상 thread가 없어도 보내려 하지 않는다)
    h.automation_drain("demo", &rule).await.unwrap();
    let runs = h.routine_runs("demo", "msg").unwrap();
    assert_eq!((runs[0].status.as_str(), runs[0].detail.as_str()), ("waiting", "입력 전달 모름 · 확인 필요"));
    let q = queue(&h);
    assert_eq!(q.pending, 2);
    let inflight = q.inflight.unwrap();
    assert_eq!(
        (inflight.id.as_str(), inflight.events, inflight.thread.as_deref(), inflight.unknown),
        ("automation-seen", 2, Some("t-0007"), true)
    );
    // 세션 안 CLI·worker·평가자·분리 실행·보기 권한은 풀 수 없다(사람이 대상 thread를 확인한다)
    for (role, actor) in [
        (Local, "local:p1@labA"),
        (Local, "worker:t-0001"),
        (Local, "evaluator:t-0002"),
        (Local, "detached:t-0003"),
        (Reader, "device:r"),
    ] {
        let e = h.call(role, actor, unblock("msg", "automation-seen", false)).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::Forbidden, "{actor}");
    }
    // 화면에서 본 묶음이 아니면 바꾸지 않는다
    let e = h.call(Controller, "device:c", unblock("msg", "automation-other", false)).await.unwrap_err();
    assert!(e.message.contains("이미 풀렸거나"), "{e:?}");
    assert_eq!(queue(&h).inflight.unwrap().id, "automation-seen");
    h.call(Controller, "device:c", unblock("msg", "automation-seen", false)).await.unwrap();
    let q = queue(&h);
    assert_eq!((q.pending, q.inflight), (2, None), "계속: 묶음만 풀고 남은 입력은 다음 차례에 보낸다");
    let runs = h.routine_runs("demo", "msg").unwrap();
    for id in ["e1", "e2"] {
        assert!(
            runs.iter().any(|r| r.id == id && r.status == "skipped" && r.detail.contains("다시 보내지 않음")),
            "{runs:?}"
        );
    }
    // 푼 묶음의 같은 입력이 다시 와도 받지 않는다(재전송 없음). 새 입력은 받는다
    h.automation_submit("demo", &rule, "e1", "{}".into(), Some("t-0007".into())).unwrap();
    assert_eq!(queue(&h).pending, 2);
    h.automation_submit("demo", &rule, "e5", "{}".into(), Some("t-0007".into())).unwrap();
    assert_eq!(queue(&h).pending, 3);
    let e = h.call(Controller, "device:c", unblock("msg", "automation-seen", false)).await.unwrap_err();
    assert!(e.message.contains("이미 풀렸거나"), "{e:?}");

    // 버리기: 묶음과 남은 입력 모두. 버린 입력도 다시 받지 않는다
    h.db.set_meta("automation:demo:msg", &queue_state(message, &["f1"], &["f2", "f3"])).unwrap();
    h.call(Local, "local:cli", unblock("msg", "automation-seen", true)).await.unwrap();
    let q = queue(&h);
    assert_eq!((q.pending, q.inflight), (0, None));
    let runs = h.routine_runs("demo", "msg").unwrap();
    assert!(runs.iter().any(|r| r.id == "f3" && r.status == "skipped" && r.detail.contains("버림")), "{runs:?}");
    h.automation_submit("demo", &rule, "f2", "{}".into(), Some("t-0007".into())).unwrap();
    assert_eq!(queue(&h).pending, 0);

    // 결과를 모르는 것은 글 묶음뿐이다: 다른 동작의 처리 중 묶음은 다음 판정이 다시 고르므로 풀지 않는다
    p.write("projects/demo/automations/note.toml", "[on.thread]\nevents=['thread_failed']\n[do.inbox]\ntext='확인'\n");
    h.db.set_meta("automation:demo:note", &queue_state(serde_json::json!({"inbox":{"text":"확인"}}), &["g1"], &[]))
        .unwrap();
    let note = h.routines("demo").unwrap().into_iter().find(|r| r.name == "note").unwrap().queue.unwrap();
    assert!(!note.inflight.unwrap().unknown);
    let e = h.call(Controller, "device:c", unblock("note", "automation-seen", false)).await.unwrap_err();
    assert!(e.message.contains("글 묶음이 아니다"), "{e:?}");
}

#[tokio::test]
async fn routine_runs_carries_rule_source_and_body_file() {
    use aaa_proto::{DeviceRole::*, FrontRequest, FrontResponse};
    let p = Project::new();
    let h = demo_host(&p);
    let rule =
        "#:schema ./automation.schema.json\n[on.schedule]\nevery='1h'\n[do.inbox]\ntext_file='automations/long.md'\n";
    p.write("projects/demo/automations/long.toml", rule);
    p.write("projects/demo/automations/long.md", &"가".repeat(30000));
    let runs = |name: &str| FrontRequest::RoutineRuns { project: "demo".into(), name: name.into() };
    let Ok(FrontResponse::RoutineRuns { source: Some(s), .. }) = h.call(Reader, "device:r", runs("long")).await else {
        panic!("원문이 없다")
    };
    assert_eq!(
        (s.file.as_str(), s.text.as_str(), s.body_file.as_deref()),
        ("automations/long.toml", rule, Some("automations/long.md"))
    );
    use sha2::Digest;
    assert_eq!(s.sha256, hex::encode(sha2::Sha256::digest(rule.as_bytes())));
    assert!(
        !s.truncated && s.body_truncated && s.body.len() <= 64 * 1024 && s.body.chars().all(|c| c == '가'),
        "짧은 원문은 그대로, 본문만 글자 경계에서 자른다"
    );
    let Ok(FrontResponse::RoutineRuns { source: None, .. }) = h.call(Reader, "device:r", runs("mission:watch")).await
    else {
        panic!("임무 조건은 mission_show가 원문을 준다")
    };
}

/// 자기 사건 거르기(plan t-0475 A1 §3.5): 규칙 a가 띄운 thread의 report_ready는 a도, include_automation 없는 b도 깨우지 않는다.
/// include_automation인 b는 받고, 그래도 b 자신이 띄운 thread의 사건은 늘 뺀다. 사람이 띄운 thread의 사건은 그대로 받는다.
#[test]
fn thread_events_skip_automation_threads_unless_included_and_never_the_rules_own() {
    let p = Project::new();
    let h = demo_host(&p);
    let thread = |id: &str, automation: Option<&str>| {
        let mut r = crate::orch::ThreadRec::starting(
            "demo",
            id,
            "m",
            "fake",
            aaa_proto::Role::Worker,
            aaa_proto::ThreadKind::Dir,
            1,
        );
        r.id = id.into();
        r.automation = automation.map(str::to_string);
        assert!(h.db.thread_import("demo", id, "open", &serde_json::to_string(&r).unwrap()).unwrap());
        h.db.flow_insert(&aaa_proto::FlowEvent {
            project: "demo".into(),
            seq: 0,
            at_ms: 1,
            boot_id: "b".into(),
            kind: "report_ready".into(),
            thread: Some(id.into()),
            repo: None,
            actor: None,
            role: Some(aaa_proto::Role::Worker),
            profile: None,
            machine: None,
            evidence: None,
            detail: String::new(),
        })
        .unwrap();
    };
    thread("t-0001", None);
    thread("t-0002", Some("a"));
    thread("t-0003", Some("b"));
    let seen = |rule: &str, include_automation: bool| {
        let wanted = Threads { events: vec!["report_ready".into()], repos: vec![], include_automation };
        let (target, events) = h.automation_flow_events("demo", 0, &wanted, rule).unwrap();
        assert_eq!(target, 3, "건너뛴 사건도 cursor는 지나간다");
        events.into_iter().map(|e| e.thread.unwrap()).collect::<Vec<_>>()
    };
    assert_eq!(seen("a", false), ["t-0001"]);
    assert_eq!(seen("b", false), ["t-0001"]);
    assert_eq!(seen("b", true), ["t-0001", "t-0002"]);
    assert_eq!(seen("a", true), ["t-0001", "t-0003"]);
    assert_eq!(seen("c", true), ["t-0001", "t-0002", "t-0003"]);
}

/// 줄 표지에 쓰는 규칙 제목: 파일의 title, 비었거나 파일이 없어지면 이름
#[test]
fn automation_title_reads_rule_title_and_falls_back_to_name() {
    let p = Project::new();
    let h = demo_host(&p);
    p.write(
        "projects/demo/automations/ci-main.toml",
        "title = \"CI 분류\"\n[on.schedule]\nevery='1h'\n[do.inbox]\ntext='x'\n",
    );
    p.write("projects/demo/automations/plain.toml", "[on.schedule]\nevery='1h'\n[do.inbox]\ntext='x'\n");
    assert_eq!(h.automation_title("demo", "ci-main"), "CI 분류");
    assert_eq!(h.automation_title("demo", "plain"), "plain");
    assert_eq!(h.automation_title("demo", "gone"), "gone");
}

#[test]
fn start_reuse_memory_and_close_idle_are_validated_in_public_toml() {
    let p = Project::new();
    let base = "[on.thread]\nevents=['thread_failed']\n[do.start]\ntitle='점검'\ntext='분류'\nprofiles=['fake']\nmachines=['mac-a']\n";
    for invalid in ["memory=true", "close_idle='30m'", "reuse='other'", "reuse='open'\nclose_idle='bad'"] {
        assert!(parse(&p.0, "check", &format!("{base}{invalid}\n")).is_err(), "{invalid}");
    }
    let r =
        parse(&p.0, "check", &format!("{base}reuse='open'\nmemory=true\nreports=true\nclose_idle='30m'\n")).unwrap();
    let Action::Start(a) = r.action else { panic!() };
    assert!(a.memory && a.reports && a.reuse == Reuse::Open);
}

#[test]
fn converter_rejects_malformed_legacy_values_before_writing() {
    for (field, input) in [
        ("machine", "machine=17"),
        ("repo", "repo=17"),
        ("events", "events=['thread_failed',17]"),
        ("reports", "reports='yes'"),
        ("schedule", "schedule=17"),
        ("limits.batch", "limits={batch=17}"),
        ("limits.usage_session_below", "limits={usage_session_below='unknown'}"),
        ("unknown", "unknown=17"),
    ] {
        let p = Project::new();
        let source = format!(
            "+++\nprofile='fake'\n{}{}{}\n+++\n자료\n",
            if field == "machine" { "" } else { "machine='mac-a'\n" },
            if field == "events" { "" } else { "events=['thread_failed']\n" },
            input
        );
        p.write("missions/watch.md", &source);
        let error = super::convert::prepare(&p.0, "mac-a").err().expect("잘못된 옛 값이 기본값으로 변환됐다");
        assert!(error.contains(field), "{field}: {error}");
        assert_eq!(std::fs::read_to_string(p.0.join("missions/watch.md")).unwrap(), source);
        assert!(!p.0.join("automations").exists());
    }
}

#[test]
fn converter_rejects_malformed_legacy_targets_before_writing() {
    for (path, source, field) in [
        ("routines/check.md", "+++\nmission=17\nschedule='every 10m'\n+++\n자료\n", "mission"),
        (
            "automations/check.toml",
            "[on.schedule]\nevery='10m'\n[do.wake]\nmission='watch'\nreports='yes'\n",
            "do.wake.reports",
        ),
    ] {
        let p = Project::new();
        p.write("missions/watch.md", "+++\nprofile='fake'\nmachine='mac-a'\nevents=['thread_failed']\n+++\n자료\n");
        p.write(path, source);
        let error = super::convert::prepare(&p.0, "mac-a").err().expect("잘못된 옛 대상이 기본값으로 변환됐다");
        assert!(error.contains(field), "{field}: {error}");
        assert_eq!(std::fs::read_to_string(p.0.join(path)).unwrap(), source);
        assert!(!p.0.join("missions/.converted").exists());
        assert!(!p.0.join("automations/watch.toml").exists());
    }
}

// ───── 관리 GUI W2: 읽기·검사·저장·삭제·되살리기·지금 실행(plan §8.342) ─────

fn doc_of(r: Result<aaa_proto::FrontResponse, aaa_proto::ErrorBody>) -> aaa_proto::AutomationDoc {
    match r {
        Ok(aaa_proto::FrontResponse::AutomationDoc { doc }) => doc,
        other => panic!("문서 응답이 아니다: {other:?}"),
    }
}
fn save(name: &str, text: &str, expected: Option<&str>) -> aaa_proto::FrontRequest {
    aaa_proto::FrontRequest::AutomationSave {
        project: "demo".into(),
        name: name.into(),
        text: text.into(),
        expected_sha256: expected.map(str::to_owned),
        body: None,
        body_expected_sha256: None,
    }
}
fn sha(text: &str) -> String {
    use sha2::Digest as _;
    hex::encode(sha2::Sha256::digest(text.as_bytes()))
}

#[tokio::test]
async fn save_writes_only_the_seen_revision_and_never_resurrects_a_deleted_rule() {
    use aaa_proto::{DeviceRole::*, ErrorCode, FrontRequest, FrontResponse};
    let p = Project::new();
    let h = demo_host(&p);
    let dir = p.0.join("projects/demo");
    let read = |name: &str| FrontRequest::AutomationRead { project: "demo".into(), name: name.into() };
    assert_eq!(h.call(Reader, "device:r", read("check")).await.unwrap_err().code, ErrorCode::NotFound);
    // 폼으로 새 규칙의 글을 만든다(Reader도 검사는 한다)
    let form = serde_json::json!({"title":"점검","on":{"thread":{"events":["thread_failed"]}},"do":{"inbox":{"text":"본다"}},"limits":{"usage_session_below":5,"usage_week_below":10}});
    let Ok(FrontResponse::AutomationChecked { text, check }) = h
        .call(
            Reader,
            "device:r",
            FrontRequest::AutomationValidate {
                project: "demo".into(),
                name: "check".into(),
                text: String::new(),
                form: Some(form),
                body: None,
            },
        )
        .await
    else {
        panic!("검사 응답이 아니다")
    };
    assert!(check.errors.is_empty(), "{:?}", check.errors);
    assert_eq!(check.review.as_ref().unwrap().start_threads, h.safety("demo").start_threads);
    assert!(!dir.join("automations/check.toml").exists(), "검사가 파일을 썼다");
    assert_eq!(h.call(Reader, "device:r", save("check", &text, None)).await.unwrap_err().code, ErrorCode::Forbidden);
    let doc = doc_of(h.call(Controller, "device:a", save("check", &text, None)).await);
    assert_eq!(std::fs::read_to_string(dir.join("automations/check.toml")).unwrap(), text);
    assert_eq!(doc.sha256, sha(&text));
    // 같은 이름 새로 만들기·본 판이 아닌 저장은 아무것도 쓰지 않는다(다른 화면, 파일 직접 편집)
    let e = h.call(Controller, "device:b", save("check", "title='b'\n", None)).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Conflict);
    let edited = text.replace("본다", "직접 고침");
    p.write("projects/demo/automations/check.toml", &edited);
    let before = snapshot(&dir);
    let e = h
        .call(Controller, "device:a", save("check", &text.replace("본다", "a 화면"), Some(&doc.sha256)))
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Conflict, "{e:?}");
    assert_eq!(snapshot(&dir), before);
    // 잘못된 글은 저장하지 않는다
    let e = h
        .call(Controller, "device:a", save("check", "[on.schedule]\nevery='x'\n", Some(&sha(&edited))))
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::BadRequest);
    assert_eq!(snapshot(&dir), before);
    // b가 지운 뒤 a의 낡은 편집기 저장은 되살리지 않는다
    let Ok(FrontResponse::Routines { routines, trash }) = h
        .call(
            Controller,
            "device:b",
            FrontRequest::AutomationDelete {
                project: "demo".into(),
                name: "check".into(),
                expected_sha256: sha(&edited),
            },
        )
        .await
    else {
        panic!("삭제 응답")
    };
    assert!(routines.iter().all(|r| r.name != "check"));
    assert_eq!(trash.len(), 1);
    assert_eq!((trash[0].name.as_str(), trash[0].title.as_str()), ("check", "점검"));
    let trashed = std::fs::read_to_string(dir.join("automations/.trash").join(&trash[0].id)).unwrap();
    assert!(trashed.contains("enabled = false"), "{trashed}");
    let e = h.call(Controller, "device:a", save("check", &edited, Some(&sha(&edited)))).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::NotFound);
    assert!(!dir.join("automations/check.toml").exists());
    // 되살리면 꺼진 채로 돌아오고, 같은 것을 두 번 되살리지 않는다
    let restore = FrontRequest::AutomationRestore { project: "demo".into(), id: trash[0].id.clone() };
    let Ok(FrontResponse::Routines { routines, trash: left }) = h.call(Controller, "device:a", restore.clone()).await
    else {
        panic!("되살리기 응답")
    };
    assert!(left.is_empty());
    let back = routines.iter().find(|r| r.name == "check").unwrap();
    assert!(!back.enabled && back.error.is_none(), "{back:?}");
    assert_eq!(h.call(Controller, "device:a", restore).await.unwrap_err().code, ErrorCode::NotFound);
    // 옛 .md는 읽기만 한다
    p.write("projects/demo/routines/old.md", "+++\nschedule='every 1m'\n+++\n확인\n");
    assert!(doc_of(h.call(Reader, "device:r", read("old")).await).legacy);
    let e = h.call(Controller, "device:a", save("old", &text, None)).await.unwrap_err();
    assert!(e.message.contains("이전 형식"), "{e:?}");
}

#[tokio::test]
async fn delete_cancels_waiting_inputs_but_keeps_the_inflight_batch_and_records() {
    let p = Project::new();
    let h = demo_host(&p);
    let text = "[on.thread]\nevents=['thread_failed']\n[do.message]\nthread='owner'\ntext='확인'\n";
    p.write("projects/demo/automations/msg.toml", text);
    let message = serde_json::json!({"message":{"thread":"owner","text":"확인"}});
    h.db.set_meta("automation:demo:msg", &queue_state(message, &["e1"], &["e2", "e3"])).unwrap();
    h.automation_delete("demo", "msg", &sha(text), "device:a").await.unwrap();
    let state: serde_json::Value = serde_json::from_str(&h.db.meta("automation:demo:msg").unwrap().unwrap()).unwrap();
    assert_eq!(state["pending"].as_array().unwrap().len(), 0, "{state}");
    assert_eq!(state["inflight"]["id"], "automation-seen", "{state}");
    let runs = h.routine_runs("demo", "msg").unwrap();
    for id in ["e2", "e3"] {
        assert!(runs.iter().any(|r| r.id == id && r.status == "skipped"), "{id}: {runs:?}");
    }
}

#[tokio::test]
async fn run_now_queues_one_manual_input_per_key_and_refuses_unapproved_scope() {
    use aaa_proto::{DeviceRole::*, ErrorCode, FrontRequest, FrontResponse};
    let p = Project::new();
    let h = demo_host(&p);
    let run = |name: &str, key: &str| FrontRequest::AutomationRun {
        project: "demo".into(),
        name: name.into(),
        key: key.into(),
    };
    p.write("projects/demo/automations/note.toml", "[on.schedule]\nevery='1h'\n[do.inbox]\ntext='본다'\n");
    let rule = crate::routine::load_all(&p.0.join("projects/demo")).0.into_iter().find(|r| r.name == "note").unwrap();
    // 쌓인 일정 차례가 있어도 지금 실행은 따로 받는다
    h.automation_submit("demo", &rule, "schedule:1", String::new(), None).unwrap();
    assert!(matches!(
        h.call(Controller, "device:a", run("note", "k1")).await,
        Ok(FrontResponse::AutomationRan { accepted: true, .. })
    ));
    assert!(matches!(
        h.call(Controller, "device:b", run("note", "k1")).await,
        Ok(FrontResponse::AutomationRan { accepted: false, .. })
    ));
    assert_eq!(h.automation_queue("demo", "note").unwrap().pending, 2);
    assert_eq!(h.call(Reader, "device:r", run("note", "k2")).await.unwrap_err().code, ErrorCode::Forbidden);
    // 꺼짐·주인 thread·배포·승인되지 않은 시작 범위·승인되지 않은 명령은 거부하고 큐를 바꾸지 않는다
    p.write(
        "projects/demo/automations/off.toml",
        "enabled=false\n[on.schedule]\nevery='1h'\n[do.inbox]\ntext='본다'\n",
    );
    p.write(
        "projects/demo/automations/owner.toml",
        "[on.thread]\nevents=['thread_failed']\n[do.message]\nthread='owner'\ntext='본다'\n",
    );
    p.write("projects/demo/automations/ship.toml", "do='deploy'\n[on.git]\nrepo='app'\nbranches=['main']\n");
    p.write(
        "projects/demo/automations/cmd.toml",
        "[on.command]\nevery='1h'\ncommand='echo x'\n[do.inbox]\ntext='본다'\n",
    );
    p.write(
        "projects/demo/automations/go.toml",
        "[on.schedule]\nevery='1h'\n[do.start]\ntitle='점검'\nprofiles=['fake']\nmachines=['local']\ntext='본다'\n",
    );
    for (name, code) in [
        ("off", ErrorCode::BadRequest),
        ("owner", ErrorCode::BadRequest),
        ("ship", ErrorCode::BadRequest),
        ("cmd", ErrorCode::Forbidden),
        ("missing", ErrorCode::NotFound),
    ] {
        let e = h.call(Controller, "device:a", run(name, "k3")).await.unwrap_err();
        assert_eq!(e.code, code, "{name}: {e:?}");
    }
    // 시작 대상을 풀지 못하는 범위도 돌리지 않는다(승인 필요 거부는 실제 머신이 있는 웹 E2E 55가 본다)
    h.call(Controller, "device:a", run("go", "k3")).await.unwrap_err();
    for name in ["off", "owner", "go"] {
        assert_eq!(h.automation_queue("demo", name).unwrap().pending, 0, "{name}");
    }
}

#[tokio::test]
async fn body_file_is_written_first_only_at_its_seen_revision_and_inside_automations() {
    use aaa_proto::{DeviceRole::*, ErrorCode, FrontRequest};
    let p = Project::new();
    let h = demo_host(&p);
    let dir = p.0.join("projects/demo");
    let text = "[on.schedule]\nevery='1h'\n[do.inbox]\ntext_file='automations/note.md'\n";
    let with_body =
        |text: &str, expected: Option<&str>, body: &str, body_expected: Option<&str>| FrontRequest::AutomationSave {
            project: "demo".into(),
            name: "note".into(),
            text: text.into(),
            expected_sha256: expected.map(str::to_owned),
            body: Some(body.into()),
            body_expected_sha256: body_expected.map(str::to_owned),
        };
    let doc = doc_of(h.call(Controller, "device:a", with_body(text, None, "첫 본문\n", None)).await);
    assert_eq!(std::fs::read_to_string(dir.join("automations/note.md")).unwrap(), "첫 본문\n");
    assert_eq!(doc.body.as_deref(), Some("첫 본문\n"));
    p.write("projects/demo/automations/note.md", "직접 고친 본문\n");
    let before = snapshot(&dir);
    let e = h
        .call(Controller, "device:a", with_body(text, Some(&doc.sha256), "a 본문\n", doc.body_sha256.as_deref()))
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Conflict);
    assert_eq!(snapshot(&dir), before);
    // 규칙 파일·보존 폴더는 본문 파일이 될 수 없다(다른 규칙을 본문으로 덮지 않는다)
    p.write("projects/demo/automations/other.toml", "[on.schedule]\nevery='1h'\n[do.inbox]\ntext='x'\n");
    let other = std::fs::read_to_string(dir.join("automations/other.toml")).unwrap();
    for target in ["automations/other.toml", "automations/.trash/x.md"] {
        let e = h
            .call(
                Controller,
                "device:a",
                with_body(
                    &text.replace("automations/note.md", target),
                    Some(&doc.sha256),
                    "덮기\n",
                    Some(&sha(&other)),
                ),
            )
            .await
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::BadRequest, "{target}: {e:?}");
    }
    let before = snapshot(&dir);
    let outside = text.replace("automations/note.md", "TASKS.md");
    let e = h
        .call(Controller, "device:a", with_body(&outside, Some(&doc.sha256), "덮어쓰기\n", Some(&sha("x"))))
        .await
        .unwrap_err();
    assert!(e.message.contains("automations/ 안"), "{e:?}");
    assert_eq!(snapshot(&dir), before);
}

#[tokio::test]
async fn command_approval_is_allowed_from_web_controllers_but_not_from_sessions_or_workers() {
    use aaa_proto::{DeviceRole::*, ErrorCode, FrontRequest};
    let p = Project::new();
    let h = demo_host(&p);
    p.write(
        "projects/demo/automations/cmd.toml",
        "[on.command]\nevery='1h'\ncommand='echo x'\n[do.inbox]\ntext='본다'\n",
    );
    let rule = crate::routine::load_all(&p.0.join("projects/demo")).0.into_iter().find(|r| r.name == "cmd").unwrap();
    let approve = || FrontRequest::RoutineApprove {
        project: "demo".into(),
        name: "cmd".into(),
        command_sha256: rule.command_sha256(),
    };
    for actor in ["local:jo@t-0001", "worker:t-0001", "evaluator:t-0001", "detached:x"] {
        let e = h.call(Local, actor, approve()).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::Forbidden, "{actor}");
    }
    assert_eq!(h.call(Reader, "device:r", approve()).await.unwrap_err().code, ErrorCode::Forbidden);
    assert!(!h.routine_approved("demo", &rule));
    h.call(Controller, "device:c", approve()).await.unwrap();
    assert!(h.routine_approved("demo", &rule));
}

#[tokio::test]
async fn idle_audit_is_read_only_on_web_and_local_cli_keeps_control() {
    use aaa_proto::{DeviceRole::*, ErrorCode, FrontRequest as R, FrontResponse};
    let p = Project::new();
    let h = demo_host(&p);
    let dir = p.0.join("projects/demo");
    // 구형 프로젝트 fixture를 변환해 웹 읽기 전용 경계를 검사한다.
    legacy_followup(&dir);
    let (files, _) = h.automation_convert("demo", false, None, "local:cli").await.unwrap();
    assert!(files.contains(&"routines/.converted/pr-followup.md".to_string()));
    // 웹 보호는 프로젝트 생략을 채운 뒤 적용한다. 일반 변환의 기존 계약을 유지한다.
    let converted = h
        .call(
            Controller,
            "device:a",
            R::AutomationConvert { project: String::new(), dry_run: false, plan_sha256: None },
        )
        .await
        .unwrap();
    assert!(matches!(converted, FrontResponse::AutomationConverted { files, .. } if files.is_empty()));
    let text = "title='운영 점검'\n[on.schedule]\nevery='10m'\n[do.inbox]\ntext='점검한다'\n";
    let read = || R::AutomationRead { project: "demo".into(), name: "idle-audit".into() };
    // 이름은 웹 새 규칙에서도 보호한다. 기존 파일 유무·본문 표지에 기대지 않는다.
    let before = snapshot(&dir);
    assert_eq!(
        h.call(Controller, "device:a", save("idle-audit", text, None)).await.unwrap_err().code,
        ErrorCode::Forbidden
    );
    assert_eq!(snapshot(&dir), before);
    let doc = doc_of(h.call(Local, "local:cli", save("idle-audit", text, None)).await);
    let reason = doc.web_read_only_reason.as_deref().unwrap();
    assert!(reason.contains("시스템 자동화"));
    for role in [Reader, Controller] {
        assert_eq!(doc_of(h.call(role, "device:a", read()).await).text, text);
        let view = h.routines("demo").unwrap().into_iter().find(|r| r.name == "idle-audit").unwrap();
        assert_eq!(view.web_read_only_reason.as_deref(), Some(reason));
        let source = h
            .call(role, "device:a", R::RoutineRuns { project: "demo".into(), name: "idle-audit".into() })
            .await
            .unwrap();
        assert!(matches!(source, FrontResponse::RoutineRuns { source: Some(s), .. } if s.text == text));
    }
    let before = snapshot(&dir);
    let project = "demo".to_string();
    let name = "idle-audit".to_string();
    for req in [
        save(&name, &text.replace("점검한다", "웹 변경"), Some(&doc.sha256)),
        R::RoutineToggle { project: project.clone(), name: name.clone(), on: Some(false) },
        R::AutomationDelete { project: project.clone(), name: name.clone(), expected_sha256: doc.sha256.clone() },
        R::AutomationRun { project: project.clone(), name: name.clone(), key: "web-run".into() },
        R::RoutineApprove { project: project.clone(), name: name.clone(), command_sha256: "unseen".into() },
        R::AutomationApprove { project: project.clone(), name: name.clone(), approval_sha256: "unseen".into() },
        R::AutomationUnblock { project: project.clone(), name: name.clone(), batch: "unseen".into(), discard: true },
    ] {
        let error = h.call(Controller, "device:a", req).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::Forbidden, "{error:?}");
        assert_eq!(error.message, reason);
        assert_eq!(snapshot(&dir), before);
    }
    assert!(h.routine_runs("demo", &name).unwrap().is_empty());
    // 다른 운영 자동화로 범위를 넓히지 않는다. Controller의 기존 쓰기는 그대로다.
    for other in ["ci-watch", "rc-verify", "nightly-coverage", "git-backup", "idle-audit-copy"] {
        let other = doc_of(h.call(Controller, "device:a", save(other, text, None)).await);
        assert!(other.web_read_only_reason.is_none());
    }
    h.call(Local, "local:cli", R::RoutineToggle { project: project.clone(), name: name.clone(), on: Some(false) })
        .await
        .unwrap();
    h.call(Local, "local:cli", R::RoutineToggle { project: project.clone(), name: name.clone(), on: Some(true) })
        .await
        .unwrap();
    assert!(matches!(
        h.call(
            Local,
            "local:cli",
            R::AutomationRun { project: project.clone(), name: name.clone(), key: "cli-run".into() }
        )
        .await
        .unwrap(),
        FrontResponse::AutomationRan { accepted: true, .. }
    ));
    let current = doc_of(h.call(Local, "local:cli", read()).await);
    h.call(
        Local,
        "local:cli",
        R::AutomationDelete { project: project.clone(), name: name.clone(), expected_sha256: current.sha256 },
    )
    .await
    .unwrap();
    let trash = h.automation_trash("demo").unwrap();
    assert_eq!(trash.len(), 1);
    assert_eq!(trash[0].web_read_only_reason.as_deref(), Some(reason));
    let restore = R::AutomationRestore { project: project.clone(), id: trash[0].id.clone() };
    let before = snapshot(&dir);
    assert_eq!(h.call(Controller, "device:a", restore.clone()).await.unwrap_err().code, ErrorCode::Forbidden);
    assert_eq!(snapshot(&dir), before);
    h.call(Local, "local:cli", restore).await.unwrap();
    assert!(!h.routines("demo").unwrap().into_iter().find(|r| r.name == name).unwrap().enabled);
    // 파일 오류여도 표지를 보존한다. 옛 규칙 일괄 변환도 웹 쓰기 우회가 되지 않는다.
    p.write("projects/demo/automations/idle-audit.toml", "broken[");
    assert!(h.routines("demo").unwrap().into_iter().find(|r| r.name == name).unwrap().web_read_only_reason.is_some());
    std::fs::remove_file(dir.join("automations/idle-audit.toml")).unwrap();
    p.write("projects/demo/routines/idle-audit.md", "+++\nschedule='every 10m'\n+++\n점검한다\n");
    let before = snapshot(&dir);
    let error = h
        .call(
            Controller,
            "device:a",
            R::AutomationConvert { project: String::new(), dry_run: false, plan_sha256: None },
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Forbidden);
    assert_eq!(snapshot(&dir), before);
}

#[test]
fn v2_preserves_scope_profile_timing_and_command_approval() {
    let p = Project::new();
    let rules = [
        "[on.schedule]\nevery='10m'\n[do.inbox]\ntext='안내'",
        "[on.command]\nevery='10m'\ncommand='git status --porcelain'\nmachine='mac-a'\n[do.inbox]\n",
        "[on.pr]\nevents=['review']\n[do.message]\nthread='owner'\ntext='확인'",
        "[on.ci]\nrepo='aaa'\nbranches=['main']\nevents=['failed']\n[do.start]\ntitle='CI'\nrepo='aaa'\nprofiles=['migration-time-profile']\nmachines=['mac-a']\ntext='분류'\nreuse='open'\nmemory=true\nreports=true\nclose_idle='30m'\n[limits]\nbatch='2m'\nusage_week_below=10",
        "[on.thread]\nevents=['report_ready']\nrepos=['rc']\n[do.start]\ntitle='검증'\nrepo='rc'\nprofiles=['another-current-profile']\nmachines=['mac-a']\ntext='검증'\nreuse='open'\nmemory=true",
        "enabled=false\n[on.schedule]\ndaily='04:00'\ncatch_up='skip'\n[do.start]\ntitle='예약'\nprofiles=['reviewer']\nmachines=['mac-a']\ntext='예약'",
        "[on.schedule]\ndaily='03:00'\n[do.inbox]\ntext='야간 검사는 coordinator가 시작한다'",
    ];
    for raw in rules {
        let old = model::read("automations/check.toml", raw).unwrap();
        let encoded = model::encode(&old).unwrap();
        assert_eq!(model::read("automations/check.toml", &encoded).unwrap(), old);
        let before = parse(&p.0, "check", raw).unwrap();
        let after = parse(&p.0, "check", &encoded).unwrap();
        assert_eq!(before.command_sha256(), after.command_sha256());
        assert_eq!(File::from_routine(&before), File::from_routine(&after));
    }
    let direct = "schema=2\n[trigger]\nkind='schedule'\nevery='10m'\n[action]\nkind='command'\ncommand='git status --porcelain'\nmachine='mac-a'";
    let observed = parse(&p.0, "check", rules[1]).unwrap();
    let direct = parse(&p.0, "check", direct).unwrap();
    assert_ne!(observed.command_sha256(), direct.command_sha256(), "조회 승인이 직접 실행 권한으로 커지면 안 된다");
}

#[test]
fn v2_rejects_unknown_fields_and_inapplicable_conditions() {
    let p = Project::new();
    for extra in ["schema=3", "schema=2\nunknown=true"] {
        assert!(
            parse(&p.0, "bad", &format!("{extra}\n[trigger]\nkind='schedule'\nevery='10m'\n[action]\nkind='notice'"))
                .is_err()
        );
    }
    assert!(
        parse(
            &p.0,
            "bad",
            "schema=2\n[trigger]\nkind='schedule'\nevery='10m'\n[condition]\nbranches=['main']\n[action]\nkind='notice'"
        )
        .is_err()
    );
}

#[test]
fn migration_journal_resumes_partial_files_and_refuses_unrelated_edits() {
    let p = Project::new();
    let old = "[on.schedule]\ndaily='03:00'\n[do.inbox]\ntext='그대로'";
    p.write("automations/night.toml", old);
    p.write("automations/night.memory/processed.txt", "old-input.json\n");
    p.write("automations/second.toml", old);
    let plan = convert::prepare(&p.0, "mac-a").unwrap();
    let encoded = serde_json::to_string(&plan).unwrap();
    // 첫 파일 rename 뒤 host가 중단된 상태를 만든다. 남은 파일과 백업은 복구가 완성한다.
    let partial: serde_json::Value = serde_json::from_str(&encoded).unwrap();
    p.write("automations/night.toml", partial["writes"]["automations/night.toml"].as_str().unwrap());
    let plan: convert::Plan = serde_json::from_str(&encoded).unwrap();
    convert::resume(&p.0, &plan).unwrap();
    let first = snapshot(&p.0);
    convert::resume(&p.0, &plan).unwrap();
    assert_eq!(snapshot(&p.0), first, "복구 재실행은 같은 bytes");
    assert_eq!(
        std::fs::read_to_string(p.0.join("automations/night.memory/processed.txt")).unwrap(),
        "old-input.json\n"
    );
    assert!(convert::prepare(&p.0, "mac-a").unwrap().listing().is_empty());
    p.write("automations/night.toml", "# 다른 편집\n");
    assert!(convert::resume(&p.0, &plan).unwrap_err().contains("다른 편집"));
}

#[tokio::test]
async fn equal_schedule_input_ids_in_different_rules_create_distinct_notices() {
    let p = Project::new();
    let h = demo_host(&p);
    let root = h.project_dir("demo").unwrap();
    for name in ["first", "second"] {
        let rule = parse(
            &root,
            name,
            "schema=2\n[trigger]\nkind='schedule'\nevery='1m'\n[action]\nkind='notice'\ntext='독립 안내'",
        )
        .unwrap();
        h.automation_submit("demo", &rule, "schedule:42", String::new(), None).unwrap();
        h.automation_drain_at("demo", &rule, aaa_common::util::now_ms()).await.unwrap();
    }
    let items = h.db.inbox_list(Some("demo"), true).unwrap();
    let a = items.iter().find(|i| i.subject == "first").unwrap();
    let b = items.iter().find(|i| i.subject == "second").unwrap();
    assert_ne!(a.id, b.id);
    assert_eq!(a.body, b.body);
}

#[test]
fn scheduled_open_thread_condition_roundtrips_and_rejects_wrong_triggers() {
    let p = Project::new();
    let source = "schema=2\n[trigger]\nkind='schedule'\nevery='10m'\n[condition]\nmin_open_threads=1\n[action]\nkind='notice'\ntext='점검'\n";
    for observe in ["", "[observe]\nkind='command'\ncommand='echo data'\n"] {
        let source = if observe.is_empty() {
            source.to_string()
        } else {
            source
                .replace("[condition]", &format!("{observe}[condition]"))
                .replace("min_open_threads=1", "min_open_threads=1\noutput_changed=true")
        };
        let r = parse(&p.0, "check", &source).unwrap();
        assert_eq!(r.min_open_threads, Some(1));
        assert!(crate::routine::trigger_label(&r).contains("열린 일반 thread 1개 이상"));
        let encoded = model::encode(&File::from_routine(&r)).unwrap();
        assert_eq!(parse(&p.0, "check", &encoded).unwrap().min_open_threads, Some(1));
        assert!(parse(&p.0, "check", &source.replace("min_open_threads=1", "min_open_threads=0")).is_err());
        assert!(parse(&p.0, "check", &source.replace("min_open_threads=1", "min_open_threads=-1")).is_err());
    }
    let invalid = source.replace("kind='schedule'\nevery='10m'", "kind='event'\nsource='thread'");
    assert!(parse(&p.0, "check", &invalid).unwrap_err().contains("condition"));
}

#[tokio::test]
async fn schedule_requires_open_worker_threads_without_catching_up_skips() {
    use aaa_proto::{Role, ThreadStatus};
    let p = Project::new();
    let h = demo_host(&p);
    let dir = p.0.join("projects/demo");
    p.write("projects/demo/automations/check.toml", "schema=2\n[trigger]\nkind='schedule'\nevery='1m'\n[condition]\nmin_open_threads=1\n[action]\nkind='notice'\ntext='점검'\n");
    let add = |project: &str, id: &str, status, automation: Option<&str>, role| {
        let mut r = crate::orch::ThreadRec::starting(project, id, "m", "fake", role, aaa_proto::ThreadKind::Dir, 1);
        r.status = status;
        r.automation = automation.map(str::to_string);
        assert!(h.db.thread_import(project, id, "fixture", &serde_json::to_string(&r).unwrap()).unwrap());
    };
    let due = || {
        h.db.set_meta(
            "routine:demo:check",
            &serde_json::json!({"last_run_ms": aaa_common::util::now_ms() - 120_000}).to_string(),
        )
        .unwrap()
    };
    let check_skip = || {
        due();
        h.routine_pass("demo", &dir, true);
        let views = h.routines("demo").unwrap();
        let view = views.iter().find(|r| r.name == "check").unwrap();
        assert_eq!(view.last_skip.as_ref().unwrap().detail, "열린 일반 thread 부족: 0/1");
        assert!(h.db.inbox_list(Some("demo"), false).unwrap().iter().all(|i| i.subject != "check"));
    };
    check_skip();
    add("demo", "t-0001", ThreadStatus::Open, Some("ci-watch"), Role::Worker);
    check_skip();
    add("demo", "t-0002", ThreadStatus::Parked, None, Role::Worker);
    check_skip();
    add("demo", "t-0003", ThreadStatus::Resolved, None, Role::Worker);
    add("demo", "t-0004", ThreadStatus::Failed, None, Role::Worker);
    add("demo", "t-0005", ThreadStatus::Open, None, Role::Coordinator);
    add("other", "t-0001", ThreadStatus::Open, None, Role::Worker);
    check_skip();
    add("demo", "t-0006", ThreadStatus::Open, None, Role::Worker);
    // 방금 소비한 차례를 같은 분에 catch_up으로 깨우지 않는다.
    h.routine_pass("demo", &dir, true);
    let rule = parse(&dir, "check", &std::fs::read_to_string(dir.join("automations/check.toml")).unwrap()).unwrap();
    h.automation_drain("demo", &rule).await.unwrap();
    assert!(h.db.inbox_list(Some("demo"), false).unwrap().iter().all(|i| i.subject != "check"));
    due();
    h.routine_pass("demo", &dir, true);
    let rule = parse(&dir, "check", &std::fs::read_to_string(dir.join("automations/check.toml")).unwrap()).unwrap();
    h.automation_drain("demo", &rule).await.unwrap();
    assert_eq!(
        h.db.inbox_list(Some("demo"), false).unwrap().iter().filter(|i| i.subject == "check").count(),
        1,
        "queue={:?}, runs={:?}",
        h.automation_queue("demo", "check"),
        h.routine_runs("demo", "check")
    );
}

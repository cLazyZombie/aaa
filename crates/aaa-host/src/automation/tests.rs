use super::*;
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
        "automations/read.md",
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
    assert_eq!(rule["$defs"]["Condition"]["oneOf"].as_array().unwrap().len(), 6);
    assert_eq!(rule["$defs"]["Action"]["oneOf"].as_array().unwrap().len(), 4);
    assert_eq!(rule["$defs"]["Inbox"]["not"]["required"], serde_json::json!(["text", "text_file"]));
}

fn convert(root: &std::path::Path) -> Result<Vec<String>, String> {
    super::convert::apply(root, super::convert::prepare(root, "mac-a")?)
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

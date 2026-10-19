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
    let new = "title = '확인'\n[on.command]\nevery = '10m'\ncommand = 'echo data'\nmachine = 'mac-a'\n[do.wake]\nmission = 'watch'\ntext_file = 'automations/note.md'\n[limits]\nmax_per_day = 3\n";
    p.write("automations/check.toml", new);
    let (all, bad) = crate::routine::load_all(&p.0);
    assert!(bad.is_empty(), "{bad:?}");
    assert_eq!(all.len(), 1);
    let old = crate::routine::parse(
        "check",
        "+++\nschedule='every 10m'\ncommand='echo data'\nmachine='mac-a'\nmission='watch'\n+++\n자료를 분류한다.\n",
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
    let legacy =
        format!("+++\nschedule='daily 03:00'\ncommand='echo data'\nmachine='mac-a'\nmission='watch'\n+++\n{body}");
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
        "missions/watch.toml",
        "missions/watch.md",
        "automations/watch-events.toml",
        "automations/watch-schedule.toml",
        "automations/automation.schema.json",
        "missions/mission.schema.json",
    ] {
        assert!(files.iter().any(|f| f == file), "{file}: {files:?}");
    }
    assert_eq!(std::fs::read_to_string(p.0.join("routines/.converted/read.md")).unwrap(), legacy);
    assert_eq!(std::fs::read_to_string(p.0.join("missions/.converted/watch.md")).unwrap(), mission);
    assert_eq!(std::fs::read_to_string(p.0.join("missions/watch.md")).unwrap(), "자료를 분류한다.\n");
    assert!(!p.0.join("routines/read.md").exists());
    let (rules, errors) = crate::routine::load_all(&p.0);
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(rules.len(), 3);
    let read = rules.iter().find(|r| r.name == "read").unwrap();
    assert_eq!(read.command_sha256(), expected.command_sha256());
    assert_eq!(read.trigger, expected.trigger);
    assert_eq!(read.prompt, expected.prompt);
    assert!(
        matches!(&rules.iter().find(|r|r.name=="watch-events").unwrap().trigger,crate::routine::Trigger::Thread(t) if t.events==["thread_failed"] && t.repos==["app"])
    );
    assert!(convert(&p.0).unwrap().is_empty());
}

#[test]
fn converter_collision_fails_before_any_source_or_definition_changes() {
    let p = Project::new();
    let source = "+++\nschedule='every 1m'\n+++\n확인\n";
    p.write("routines/watch-events.md", source);
    p.write("missions/watch.md", "+++\nprofile='fake'\nevents=['thread_failed']\n+++\n자료\n");
    let error = convert(&p.0).unwrap_err();
    assert!(error.contains("겹친다"), "{error}");
    assert_eq!(std::fs::read_to_string(p.0.join("routines/watch-events.md")).unwrap(), source);
    assert!(!p.0.join("routines/.converted").exists());
    assert!(!p.0.join("automations").exists());
}

#[test]
fn public_schema_artifacts_match_rust_types_and_exclusive_tables() {
    let schemas = [("automation.schema.json", schema()), ("mission.schema.json", crate::mission::files::schema())];
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
    super::convert::apply(root, super::convert::prepare(root)?)
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

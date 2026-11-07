//! 기록 경로의 owner: 운영 홈 없이 저장된 실행 spec·계정 홈·감시 사건을 함께 확인한다.
use super::*;
use serde_json::Value;
use std::io::Write;

const ID: &str = "019a4c5e-1f2b-7c3d-8e4f-a5b6c7d8e9f0";
const HOOK: &str = "019a4c5e-1f2b-7c3d-8e4f-a5b6c7d8e9f1";

struct Fixture {
    agent: Arc<Agent>,
    spec: SpecFile,
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let state = std::env::temp_dir().join(format!("aaa-chat-path-{}", util::new_id()));
        let agent = Agent::new(state.clone(), "m".into(), "mac-a".into(), true, None).unwrap();
        let mut spec = super::tests::session(Some("codex"), &["codex"], SessionState::Running).0;
        spec.spec.cwd = Some(state.to_string_lossy().into_owned());
        spec.account_run = Some("demo/t-0001".into());
        let root = aaa_session::files::account_home(&state, "demo/t-0001").unwrap().join("sessions");
        let fixture = Self { agent, spec, root };
        fixture.save();
        fixture
    }
    fn save(&self) {
        let p = SessionPaths::new(&self.agent.state, "s");
        std::fs::create_dir_all(&p.dir).unwrap();
        util::write_json(&p.spec(), &self.spec).unwrap();
    }
    fn rollout(&self, id: &str, cwd: &Path, source: Value, timestamp: &str) -> PathBuf {
        let dir = self.root.join("2026/10/21");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("rollout-test-{id}.jsonl"));
        let mut f = std::io::BufWriter::new(std::fs::File::create(&path).unwrap());
        writeln!(
            f,
            "{}",
            serde_json::json!({"type":"session_meta","payload":{
                "id":id,"session_id":id,"source":source,"cwd":cwd,"timestamp":timestamp
            }})
        )
        .unwrap();
        for n in 0..301 {
            writeln!(
                f,
                "{}",
                serde_json::json!({"type":"response_item","payload":{
                    "type":"message","role":"assistant","content":[{"type":"output_text","text":format!("기록 {n}")}]
                }})
            )
            .unwrap();
        }
        f.flush().unwrap();
        path
    }
    fn path(&self, id: &str) -> Result<PathBuf, chat::Error> {
        self.agent.chat_path(Some("s"), "codex", None, Some(id))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        for (_, task) in self.agent.chat_watches.lock().unwrap().drain() {
            task.abort();
        }
        std::fs::remove_dir_all(&self.agent.state).unwrap();
    }
}

#[tokio::test]
async fn account_chat_uses_its_rollout_when_hook_id_differs_and_watches_the_same_file() {
    let f = Fixture::new();
    let path = f.rollout(ID, &f.agent.state, serde_json::json!("cli"), "2026-10-21T00:00:00.000Z");
    assert_eq!(f.path(HOOK).unwrap(), path.canonicalize().unwrap());
    let newest = chat::read_path("codex", f.path(HOOK).unwrap(), false, false, false, None).unwrap();
    assert_eq!(newest.messages.last().unwrap().text, "기록 300");
    let older = chat::read_path("codex", f.path(HOOK).unwrap(), false, false, false, newest.before.as_deref()).unwrap();
    assert_eq!(older.messages.last().unwrap().text, "기록 150");
    let (tx, mut out) = aaa_common::conn::frame_channel();
    f.agent.set_host(Some(tx));
    f.agent.watch_chat("s".into(), "codex".into(), None, Some(HOOK.into()));
    async fn changed(out: &mut aaa_common::conn::FrameOut) -> String {
        let (_, bytes) = tokio::time::timeout(Duration::from_secs(3), out.recv()).await.unwrap().unwrap();
        match serde_json::from_slice::<LinkMsg>(&bytes).unwrap() {
            LinkMsg::Event(AgentEvent::ChatChanged { key, signature }) => {
                assert_eq!(key, "s");
                signature
            }
            other => panic!("{other:?}"),
        }
    }
    let initial = changed(&mut out).await;
    writeln!(std::fs::OpenOptions::new().append(true).open(path).unwrap(), "{{}}").unwrap();
    assert_ne!(initial, changed(&mut out).await);
}

#[test]
fn account_chat_rejects_ambiguous_foreign_nested_and_old_records() {
    let mut f = Fixture::new();
    let first = f.rollout(ID, &f.agent.state, serde_json::json!("cli"), "2026-10-21T00:00:00.000Z");
    let second = f.rollout(HOOK, &f.agent.state, serde_json::json!("cli"), "2026-10-21T00:00:00.000Z");
    let unknown = "019a4c5e-1f2b-7c3d-8e4f-a5b6c7d8e9f2";
    assert!(f.path(unknown).unwrap_err().to_string().contains("여러 개"));
    assert_eq!(f.path(ID).unwrap(), first.canonicalize().unwrap(), "정확한 ID 우선");
    std::fs::remove_file(second).unwrap();
    for (cwd, source, time) in [
        (f.agent.state.parent().unwrap(), serde_json::json!("cli"), "2026-10-21T00:00:00.000Z"),
        (&f.agent.state as &Path, serde_json::json!({"subagent":"review"}), "2026-10-21T00:00:00.000Z"),
        (&f.agent.state as &Path, serde_json::json!("cli"), "1970-01-01T00:00:00.000Z"),
    ] {
        f.rollout(ID, cwd, source, time);
        assert!(f.path(unknown).is_err());
    }
    assert!(f.path("../bad").is_err());
    assert!(f.agent.chat_spec(Some("../s")).is_none());
    f.spec.spec.argv = vec!["codex".into(), "--no-daemon".into(), "resume".into(), unknown.into()];
    f.save();
    assert_eq!(f.path(unknown).unwrap(), first.canonicalize().unwrap(), "재개는 원래 기록 시각을 허용한다");
}

#[test]
fn codex_chat_resolves_versioned_rollout_from_id_hint_and_account_metadata() {
    let f = Fixture::new();
    let old = f.rollout(ID, &f.agent.state, serde_json::json!("cli"), "2026-10-21T00:00:00.000Z");
    let versioned = old.with_file_name(format!("rollout-test-{ID}_{HOOK}.jsonl"));
    std::fs::rename(old, &versioned).unwrap();
    let expected = versioned.canonicalize().unwrap();
    assert_eq!(f.path(ID).unwrap(), expected);
    assert_eq!(f.path(HOOK).unwrap(), expected);
    assert_eq!(f.agent.chat_path(Some("s"), "codex", versioned.to_str(), Some(ID)).unwrap(), expected);
    for tail in ["bad", "../bad", "", "01900000-0000-7000-9000-000000000001_extra"] {
        assert!(!chat::codex_record_name(Path::new(&format!("rollout-test-{ID}_{tail}.jsonl")), ID));
    }
    assert!(!chat::codex_record_name(&versioned, HOOK), "기록 ID를 대화 ID로 썼다");
}

#[test]
fn codex_chat_uses_the_exact_bound_file_even_when_the_id_has_a_backup_copy() {
    let f = Fixture::new();
    let first = f.rollout(ID, &f.agent.state, serde_json::json!("cli"), "2026-10-21T00:00:00.000Z");
    let dir = f.root.join("2026/10/22");
    std::fs::create_dir_all(&dir).unwrap();
    let second = dir.join(first.file_name().unwrap());
    std::fs::copy(&first, &second).unwrap();
    for path in [&first, &second] {
        assert_eq!(
            f.agent.chat_path(Some("s"), "codex", path.to_str(), Some(ID)).unwrap(),
            path.canonicalize().unwrap()
        );
    }
    let outside = f.agent.state.join(format!("rollout-outside-{ID}.jsonl"));
    std::fs::copy(&first, &outside).unwrap();
    assert!(chat::codex_hint(&f.root, ID, outside.to_str().unwrap()).is_none());
    assert!(chat::codex_hint(&f.root, HOOK, first.to_str().unwrap()).is_none());
    assert!(chat::codex_hint(&f.root, "../bad", first.to_str().unwrap()).is_none());
    // 옛 hook의 틀린 hint는 기존 ID 해석으로 돌아간다.
    assert_eq!(f.agent.chat_path(Some("s"), "codex", outside.to_str(), Some(ID)).unwrap(), f.path(ID).unwrap());
}

#[test]
fn codex_chat_uses_saved_home_and_account_home_overrides_profile_home() {
    let mut f = Fixture::new();
    let profile_home = f.agent.state.join("profile-codex");
    f.spec.spec.env.push(("CODEX_HOME".into(), profile_home.to_string_lossy().into_owned()));
    f.save();
    assert_eq!(f.agent.codex_root_for(Some("s")), f.root);
    f.spec.account_run = None;
    f.save();
    assert_eq!(f.agent.codex_root_for(Some("s")), profile_home.join("sessions"));
    let dir = profile_home.join("sessions/2026/10/21");
    std::fs::create_dir_all(&dir).unwrap();
    let account_file = f.rollout(ID, &f.agent.state, serde_json::json!("cli"), "2026-10-21T00:00:00.000Z");
    std::fs::copy(&account_file, dir.join(account_file.file_name().unwrap())).unwrap();
    assert!(f.path(HOOK).is_err(), "공유 profile 홈에서 ID가 다른 기록을 추측하지 않는다");
    f.spec.spec.env = vec![("CODEX_HOME".into(), "relative-home".into())];
    f.save();
    assert_eq!(f.agent.codex_root_for(Some("s")), chat::codex_root());
}

impl Fixture {
    async fn chat(&self, harness: &str, transcript: Option<&Path>) -> Result<AgentResponse, ErrorBody> {
        let (tx, mut out) = aaa_common::conn::frame_channel();
        self.agent
            .clone()
            .handle_deadline(
                "chat".into(),
                AgentRequest::Chat {
                    session: Some("s".into()),
                    harness: harness.into(),
                    transcript: transcript.map(|p| p.to_string_lossy().into_owned()),
                    harness_session: Some(ID.into()),
                    thinking: false,
                    compact: true,
                    commands: true,
                    before: None,
                },
                None,
                Some(tx),
            )
            .await;
        let (_, bytes) = out.recv().await.unwrap();
        match serde_json::from_slice::<LinkMsg>(&bytes).unwrap() {
            LinkMsg::Res { result: Outcome::Ok(response), .. } => Ok(response),
            LinkMsg::Res { result: Outcome::Err(error), .. } => Err(error),
            other => panic!("{other:?}"),
        }
    }
}

#[tokio::test]
async fn new_chat_is_empty_until_claude_and_codex_create_the_record() {
    for harness in ["claude", "codex"] {
        let mut f = Fixture::new();
        f.spec.spec.harness = Some(harness.into());
        f.save();
        let root = if harness == "claude" {
            let home = f.root.parent().unwrap().with_file_name("claude-home");
            std::fs::create_dir_all(&home).unwrap();
            home.join("projects")
        } else {
            f.root.clone()
        };
        let path = root.join(format!("2026/11/07/rollout-test-{ID}.jsonl"));
        for directory_exists in [false, true] {
            if directory_exists {
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            }
            let response = f.chat(harness, Some(&path)).await.unwrap();
            let AgentResponse::Chat { messages, truncated, before, history } = response else { panic!("{response:?}") };
            assert!(messages.is_empty());
            assert!(!truncated);
            assert!(before.is_none());
            assert!(history.is_none(), "없는 기록에 history를 발급했다");
        }
        let line = if harness == "claude" {
            serde_json::json!({"type":"user","message":{"content":"첫 질문"}})
        } else {
            serde_json::json!({"type":"response_item","payload":{"type":"message","role":"user",
                "content":[{"type":"input_text","text":"첫 질문"}]}})
        };
        std::fs::write(&path, format!("{line}\n")).unwrap();
        let response = f.chat(harness, Some(&path)).await.unwrap();
        let AgentResponse::Chat { messages, history, .. } = response else { panic!("{response:?}") };
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].text, "첫 질문");
        assert!(history.is_some());
        // 경로 검증 직후 파일이 없어지는 경합도 빈 대화의 같은 상태다.
        let resolved = f.agent.chat_path(Some("s"), harness, path.to_str(), Some(ID)).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert!(matches!(chat::read_path(harness, resolved, false, true, true, None), Err(chat::Error::Missing)));
    }
}

#[tokio::test]
async fn chat_keeps_path_rejections_and_real_io_errors() {
    let mut f = Fixture::new();
    f.spec.spec.harness = Some("claude".into());
    f.save();
    let home = f.root.parent().unwrap().with_file_name("claude-home");
    let root = home.join("projects");
    std::fs::create_dir_all(&root).unwrap();
    let outside = f.agent.state.join("outside.jsonl");
    for path in [&outside, &root.join("../outside.jsonl"), &root.join("bad.txt"), Path::new("relative.jsonl")] {
        assert!(f.chat("claude", Some(path)).await.unwrap_err().message.contains("기록 폴더 밖"));
    }
    std::fs::write(&outside, "{}\n").unwrap();
    assert!(f.chat("claude", Some(&outside)).await.unwrap_err().message.contains("기록 폴더 밖"));
    let directory = root.join("directory.jsonl");
    std::fs::create_dir(&directory).unwrap();
    assert!(f.chat("claude", Some(&directory)).await.is_err());
    // Codex 탐색의 실제 I/O 오류도 없는 기록으로 바뀌지 않는다.
    std::fs::create_dir_all(f.root.parent().unwrap()).unwrap();
    std::fs::write(&f.root, "폴더가 아닌 파일").unwrap();
    assert!(f.chat("codex", None).await.is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let link = root.join("escape.jsonl");
        symlink(&outside, &link).unwrap();
        assert!(f.chat("claude", Some(&link)).await.unwrap_err().message.contains("기록 폴더 밖"));
        let path = root.join("private.jsonl");
        std::fs::write(&path, "{}\n").unwrap();
        let original = std::fs::metadata(&path).unwrap().permissions();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
        let response = f.chat("claude", Some(&path)).await;
        std::fs::set_permissions(&path, original).unwrap();
        assert!(response.unwrap_err().message.contains("PermissionDenied"));
    }
}

#[tokio::test]
async fn codex_account_chat_keeps_unreadable_or_invalid_record_errors() {
    let f = Fixture::new();
    let path = f.rollout(HOOK, &f.agent.state, serde_json::json!("cli"), "1970-01-01T00:00:00.000Z");
    let response = f.chat("codex", None).await.unwrap();
    assert!(
        matches!(response, AgentResponse::Chat { messages, history: None, .. } if messages.is_empty()),
        "이전 기록만 있을 때 새 기록은 아직 없다"
    );
    std::fs::write(&path, "{손상된 session_meta}\n").unwrap();
    assert!(f.chat("codex", None).await.is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let original = std::fs::metadata(&path).unwrap().permissions();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
        let response = f.chat("codex", None).await;
        std::fs::set_permissions(&path, original).unwrap();
        assert!(response.is_err());
    }
}

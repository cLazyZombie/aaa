//! harness hook·진행 보고 수신(§8.3, §8.11). 세션 안의 `aaa hook`·`aaa report`가 이 머신 agent의 로컬 socket으로 보낸다.
//!
//! - 세션을 띄울 때 env에 `AAA_HOOK_SOCK`·`AAA_HOOK_TOKEN`을 넣는다. token은 세션 token에서 만든 값이라 따로 저장하지 않는다.
//! - harness 상태(작업 중·idle·질문)는 hook으로 정하지 않는다: pump의 화면 감지(`detect`, plan §8.93)가 정한다.
//!   hook은 herdr 0.9.5처럼 harness 세션 ID를 잇는 데(Claude SessionStart, Codex notify)와 herdr-projects 진행 보고
//!   정리(UserPromptSubmit: 「Waiting for you」를 지운다)에만 쓴다.
//! - `aaa hook`은 harness를 막지 않는다: 연결이 안 되면 조용히 끝나고, 아무것도 출력하지 않는다(결정을 내리지 않음).

use std::path::{Path, PathBuf};
use std::time::Duration;

use aaa_proto::HarnessEvent;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub const ENV_SOCK: &str = "AAA_HOOK_SOCK";
pub const ENV_TOKEN: &str = "AAA_HOOK_TOKEN";
pub const ENV_SESSION: &str = "AAA_SESSION_ID";
const MAX_MSG: usize = 256 * 1024;

pub fn socket(state: &Path) -> PathBuf {
    state.join("run").join("hook.sock")
}

/// 세션 token에서 hook token을 만든다(세션 socket token 자체는 harness에 주지 않는다)
pub fn hook_token(session_token: &str) -> String {
    hex::encode(&Sha256::digest(format!("aaa-hook:{session_token}").as_bytes())[..16])
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookMsg {
    pub session: String,
    pub token: String,
    /// claude·codex·report
    pub harness: String,
    pub payload: Value,
}

fn s(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(|x| x.as_str()).map(|x| x.to_string())
}

fn clip(t: &str, n: usize) -> String {
    let mut out: String = t.chars().filter(|c| !c.is_control() || *c == ' ').take(n).collect();
    if t.chars().count() > n {
        out.push('…');
    }
    out
}

/// harness 사건 → 정규화. 모르는 사건은 None(무시). 옛 agent가 띄운 세션은 Stop·PermissionRequest 등도 보내지만
/// 상태는 화면 감지가 정하므로 버린다.
pub fn normalize(session: &str, harness: &str, p: &Value, now_ms: u64) -> Option<HarnessEvent> {
    let mut ev = HarnessEvent {
        session: session.to_string(),
        harness: harness.to_string(),
        kind: String::new(),
        harness_session: s(p, "session_id").or_else(|| s(p, "thread-id")),
        message: None,
        question_kind: None,
        percent: None,
        at_ms: now_ms,
        transcript: s(p, "transcript_path"),
    };
    if harness == "report" {
        ev.kind = "progress".into();
        ev.percent = p.get("percent").and_then(|x| x.as_u64()).map(|x| x.min(100) as u8);
        ev.message = s(p, "activity").map(|a| clip(&a, 40));
        return Some(ev);
    }
    // Codex notify(`agent-turn-complete`): 세션 ID(`thread-id`)만 쓴다. herdr는 Codex SessionStart hook을
    // `~/.codex/hooks.json`에 깔지만 aaa는 사용자 설정을 고치지 않고 실행마다 `-c notify`만 준다.
    if s(p, "type").as_deref() == Some("agent-turn-complete") {
        ev.kind = "session".into();
        return Some(ev);
    }
    let name = s(p, "hook_event_name")?;
    ev.kind = match name.as_str() {
        // herdr `claude_settings::install`: SessionStart(startup·resume·clear·compact·fork)로 세션 ID를 잇는다
        "SessionStart" => "session_start",
        // herdr-projects progress.rs `respond`: 사람이 답했으니 옛 「Waiting for you」는 맞지 않다
        "UserPromptSubmit" => "prompt_submit",
        _ => return None,
    }
    .to_string();
    // 하위 agent의 사건은 버린다(herdr-projects progress.rs `eligible`)
    if subagent(p) {
        return None;
    }
    Some(ev)
}

/// 하위 agent(subagent)가 낸 사건인가: 자기 ID나 `/subagents/` 아래 기록을 가진다(herdr-projects progress.rs `eligible`)
fn subagent(p: &Value) -> bool {
    ["agent_id", "subagent_id", "agent_transcript_path"].iter().any(|k| p.get(k).is_some_and(|v| !v.is_null()))
        || s(p, "transcript_path").is_some_and(|t| t.contains("/subagents/"))
}

/// 이 세션이 시작한 harness 세션과 다른 세션의 사건인가(herdr-projects progress.rs `foreign_session`).
/// `started`는 SessionStart 때 적은 harness 세션 ID다. 모르면 버리지 않는다.
pub fn foreign_session(started: Option<&str>, ev: &HarnessEvent) -> bool {
    match (started, ev.harness_session.as_deref()) {
        (Some(a), Some(b)) => ev.kind != "session_start" && !a.is_empty() && a != b,
        _ => false,
    }
}

/// Codex에 실행마다 줄 notify 설정(`codex -c <이것>`). 사용자 ~/.codex 설정은 고치지 않는다.
/// Codex는 턴이 끝날 때 JSON을 마지막 인자로 붙여 이 명령을 실행한다(`agent-turn-complete`, `thread-id`).
pub fn codex_notify(exe: &Path) -> String {
    let q = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
    format!("notify=[{},\"hook\",\"--agent\",\"codex\"]", q(&exe.to_string_lossy()))
}

/// herdr `claude_settings.rs` `SESSION_START_MATCHER`: Claude가 문서로 정한 SessionStart 출처
const SESSION_START_MATCHER: &str = "^(startup|resume|clear|compact|fork)$";

/// Claude Code에 실행마다 줄 hook 설정(`claude --settings <파일>`). 사용자 설정 파일은 고치지 않는다.
/// 상태 hook(Stop·PermissionRequest·Notification 등)은 걸지 않는다(herdr 0.9.5가 그것들을 지우고 SessionStart만 둔다).
pub fn claude_settings(exe: &Path) -> Value {
    let cmd = format!("'{}' hook --agent claude", exe.to_string_lossy().replace('\'', "'\\''"));
    let hook = serde_json::json!({ "type": "command", "command": cmd, "timeout": 10 });
    serde_json::json!({
        "hooks": {
            "SessionStart": [{ "matcher": SESSION_START_MATCHER, "hooks": [hook.clone()] }],
            "UserPromptSubmit": [{ "hooks": [hook] }],
        }
    })
}

/// 세션 안에서: hook 사건 하나를 agent에 보낸다. 실패해도 harness를 막지 않게 짧게 끝낸다.
pub async fn send(harness: &str, payload: Value) -> Result<()> {
    let sock = std::env::var(ENV_SOCK).context("AAA_HOOK_SOCK이 없다(aaa 세션 밖)")?;
    let token = std::env::var(ENV_TOKEN).context("AAA_HOOK_TOKEN이 없다")?;
    let session = std::env::var(ENV_SESSION).context("AAA_SESSION_ID가 없다")?;
    let msg = HookMsg { session, token, harness: harness.to_string(), payload };
    let body = serde_json::to_vec(&msg)?;
    if body.len() > MAX_MSG {
        bail!("사건이 너무 크다");
    }
    tokio::time::timeout(Duration::from_secs(3), async {
        let mut s = aaa_common::ipc::connect(Path::new(&sock)).await?;
        s.write_all(&body).await?;
        s.shutdown().await?;
        let mut reply = Vec::new();
        s.read_to_end(&mut reply).await?;
        anyhow::ensure!(reply.starts_with(b"ok"), "agent가 거부했다: {}", String::from_utf8_lossy(&reply));
        Ok(())
    })
    .await
    .context("agent 응답 시간 초과")?
}

/// agent 쪽: 연결 하나에서 사건 하나를 읽는다
pub async fn read_msg(s: &mut aaa_common::ipc::LocalStream) -> Result<HookMsg> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let n = tokio::time::timeout(Duration::from_secs(3), s.read(&mut chunk)).await??;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.len() > MAX_MSG {
            bail!("사건이 너무 크다");
        }
    }
    Ok(serde_json::from_slice(&buf)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn subagent_and_foreign_session_events_are_dropped() {
        // herdr-projects progress.rs eligible·foreign_session 시험과 같은 경우
        let n = |p: Value| normalize("S", "claude", &p, 1);
        assert!(n(json!({"hook_event_name":"UserPromptSubmit","session_id":"a","agent_id":"x"})).is_none());
        assert!(
            n(json!({"hook_event_name":"UserPromptSubmit","session_id":"a","transcript_path":"/p/subagents/x.jsonl"}))
                .is_none()
        );
        assert!(n(json!({"hook_event_name":"UserPromptSubmit","session_id":"a","agent_id":null})).is_some());
        let other = n(json!({"hook_event_name":"UserPromptSubmit","session_id":"b"})).unwrap();
        assert!(foreign_session(Some("a"), &other));
        assert!(!foreign_session(Some("b"), &other));
        assert!(!foreign_session(None, &other));
        let start = n(json!({"hook_event_name":"SessionStart","session_id":"b"})).unwrap();
        assert!(!foreign_session(Some("a"), &start));
    }

    #[test]
    fn hooks_only_link_sessions_and_never_set_state() {
        let n = |h: &str, p: Value| normalize("S", h, &p, 1);
        let e = n("claude", json!({"hook_event_name":"SessionStart","session_id":"abc","transcript_path":"/t.jsonl"}))
            .unwrap();
        assert_eq!((e.kind.as_str(), e.harness_session.as_deref()), ("session_start", Some("abc")));
        assert_eq!(e.transcript.as_deref(), Some("/t.jsonl"));
        let p = n("claude", json!({"hook_event_name":"UserPromptSubmit","session_id":"abc","prompt":"x"})).unwrap();
        assert_eq!(p.kind, "prompt_submit");
        // 옛 agent가 띄운 세션의 상태 hook은 버린다: 상태는 화면 감지가 정한다
        for old in [
            json!({"hook_event_name":"Stop","session_id":"abc"}),
            json!({"hook_event_name":"PermissionRequest","tool_name":"Bash","tool_input":{"command":"rm x"}}),
            json!({"hook_event_name":"Notification","notification_type":"permission_prompt","message":"m"}),
            json!({"hook_event_name":"Notification","notification_type":"idle_prompt"}),
            json!({"hook_event_name":"PostToolUse","session_id":"abc"}),
            json!({"hook_event_name":"SessionEnd","session_id":"abc"}),
            json!({"x":1}),
        ] {
            assert!(n("claude", old.clone()).is_none(), "{old}");
        }
        let c =
            n("codex", json!({"type":"agent-turn-complete","thread-id":"t1","last-assistant-message":"done"})).unwrap();
        assert_eq!((c.kind.as_str(), c.harness_session.as_deref(), c.message), ("session", Some("t1"), None));
        let r = n("report", json!({"percent":250,"activity":"테스트 중\n이상한 줄"})).unwrap();
        assert_eq!((r.kind.as_str(), r.percent), ("progress", Some(100)));
        assert_eq!(r.message.as_deref(), Some("테스트 중이상한 줄"));
    }

    #[test]
    fn codex_notify_is_toml() {
        let v = codex_notify(Path::new("/A b/q\"x\\/aaa"));
        assert_eq!(v, r#"notify=["/A b/q\"x\\/aaa","hook","--agent","codex"]"#);
    }

    #[test]
    fn settings_quote_exe_and_register_only_session_hooks() {
        let v = claude_settings(Path::new("/A b/it's/aaa"));
        let cmd = v["hooks"]["SessionStart"][0]["hooks"][0]["command"].as_str().unwrap();
        assert_eq!(cmd, "'/A b/it'\\''s/aaa' hook --agent claude");
        assert_eq!(v["hooks"]["SessionStart"][0]["matcher"], "^(startup|resume|clear|compact|fork)$");
        let mut events: Vec<&str> = v["hooks"].as_object().unwrap().keys().map(String::as_str).collect();
        events.sort();
        assert_eq!(events, ["SessionStart", "UserPromptSubmit"]);
    }
}

//! harness hook·진행 보고 수신(§8.3, §8.11). 세션 안의 `aaa hook`·`aaa report`가 이 머신 agent의 로컬 socket으로 보낸다.
//!
//! - 세션을 띄울 때 env에 `AAA_HOOK_SOCK`·`AAA_HOOK_TOKEN`을 넣는다. token은 세션 token에서 만든 값이라 따로 저장하지 않는다.
//! - harness 상태(작업 중·idle·질문)는 hook으로 정하지 않는다: pump의 화면 감지(`detect`, plan §8.110)가 정한다.
//!   hook은 herdr 0.9.5처럼 harness 세션 ID를 잇는 데(Claude SessionStart, Codex notify)와 herdr-projects 진행 보고
//!   정리(UserPromptSubmit: 「Waiting for you」를 지운다)에 쓴다.
//! - Claude hook은 herdr-projects progress.rs `hook`처럼 진행 보고 안내를 넣는다(SessionStart 지침, UserPromptSubmit 안내,
//!   PostToolUse 1분에 한 번 알림, `additionalContext`). 안내만 하고 상태는 바꾸지 않는다: PostToolUse는 host에 보내지 않는다.
//! - `aaa hook`은 harness를 막지 않는다: 연결이 안 되면 조용히 끝나고 아무것도 출력하지 않는다(결정을 내리지 않음). 출력은
//!   agent가 돌려준 안내 JSON 한 줄뿐이다.

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

/// harness 사건 → 정규화. 모르는 사건은 None(무시). 옛 agent가 띄운 세션은 Stop·PermissionRequest·Notification 등도
/// 보내지만 상태는 화면 감지가 정하므로 버린다(herdr 0.9.5 `integration/assets/claude/herdr-agent-state.sh`도 옛 설치의
/// 상태 hook을 `session` 말고는 exit 0으로 버린다). 답한 확인창 뒤 늦게 온 Notification이 질문을 다시 열지 않는다(t-0333).
pub fn normalize(session: &str, harness: &str, p: &Value, now_ms: u64) -> Option<HarnessEvent> {
    let mut ev = HarnessEvent {
        session: session.to_string(),
        harness: harness.to_string(),
        kind: String::new(),
        // UUID 꼴이 아니면 버린다(`aaa_common::util::harness_session_ok`): 이 값은 `--resume` 인자가 된다
        harness_session: s(p, "session_id")
            .or_else(|| s(p, "thread-id"))
            .filter(|x| aaa_common::util::harness_session_ok(x)),
        message: None,
        question_kind: None,
        percent: None,
        at_ms: now_ms,
        transcript: s(p, "transcript_path"),
        plan: None,
        background: None,
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

/// 진행 알림을 다시 넣는 가장 짧은 간격(herdr-projects progress.rs `REMIND_SECS`)
pub const REMIND_MS: u64 = 60_000;

/// 세션 하나의 진행 알림 기록(herdr-projects progress.rs `Record` 가운데 알림이 보는 칸). agent가 기억한다.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Progress {
    /// 마지막 `aaa report`(ms). 세션이 시작한 뒤 없으면 0
    pub reported_at: u64,
    /// 마지막 보고의 백분율(`--unknown`이면 없다)
    pub percent: Option<u8>,
    /// 마지막으로 넣은 안내(ms)
    pub reminded_at: u64,
}

/// 안내를 넣을 Claude 사건이면 그 이름(herdr-projects progress.rs `eligible`): 맨 위 agent의 SessionStart·UserPromptSubmit·
/// PostToolUse. 하위 agent의 사건과 `aaa report` 자신의 PostToolUse는 아니다.
pub fn guide_kind(p: &Value) -> Option<&'static str> {
    if subagent(p) {
        return None;
    }
    let kind = match s(p, "hook_event_name")?.as_str() {
        "SessionStart" => "SessionStart",
        "UserPromptSubmit" => "UserPromptSubmit",
        "PostToolUse" => "PostToolUse",
        _ => return None,
    };
    let input = p.get("tool_input").map(|t| t.to_string()).unwrap_or_default();
    if kind == "PostToolUse" && input.contains("aaa") && input.contains(" report ") {
        return None;
    }
    Some(kind)
}

/// 사건 하나에 넣을 안내(herdr-projects progress.rs `respond`, 글은 한국어로 옮김). 시계는 인자로 받는다.
pub fn respond(rec: &mut Progress, kind: &str, prefix: &str, now_ms: u64) -> Option<String> {
    match kind {
        "SessionStart" => {
            // 이 pane의 새 세션: 옛 보고는 더 이 세션을 말하지 않는다
            *rec = Progress { reminded_at: now_ms, ..Progress::default() };
            Some(format!("# 진행 보고(aaa)\n\n{}", aaa_proto::progress_guidance(prefix)))
        }
        "UserPromptSubmit" => {
            rec.reminded_at = now_ms;
            Some(format!(
                "작업 도구를 쓰거나 막히는 질문을 하기 전에 이 요청이 새 일을 시작하는지 본다. 그렇다면 새 어림을 보고한다. \
                 그리고 사용자를 기다리기 전에 보고한다. 예: `{prefix} report --unknown --activity 'Waiting for you'`."
            ))
        }
        "PostToolUse" => {
            let done = rec.percent == Some(100);
            let since = |t: u64| now_ms.saturating_sub(t) >= REMIND_MS;
            if since(rec.reminded_at) && !done && (rec.reported_at == 0 || since(rec.reported_at)) {
                rec.reminded_at = now_ms;
                Some(format!(
                    "자연스러운 경계라면 진행을 보고할 때다: `{prefix} report --percent N --activity '...'`. 지금 일을 다시 \
                     어림하고 진행을 지어내지 않는다. 사용자에게 묻기 전에는 `Waiting for you`로 보고한다."
                ))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// hook의 답(Claude Code가 읽는 꼴, herdr-projects progress.rs `output`)
pub fn output(native: &str, text: &str) -> String {
    serde_json::json!({ "hookSpecificOutput": { "hookEventName": native, "additionalContext": text } }).to_string()
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

/// Codex 중첩 thread(하위 agent, 세션 안에서 다시 띄운 codex)의 사건인가: hook 프로세스가 물려받은 `CODEX_THREAD_ID`가
/// 있고 사건의 세션 ID와 다르다(herdr `integration/assets/codex/herdr-agent-state.sh`, herdr-projects progress.rs `hook`).
/// 세션 ID가 없는 사건도 다르다고 본다(herdr와 같다).
pub fn nested_codex(inherited: Option<&str>, p: &Value) -> bool {
    let id = s(p, "session_id").or_else(|| s(p, "thread-id"));
    inherited.is_some_and(|t| !t.is_empty() && Some(t) != id.as_deref())
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
            "UserPromptSubmit": [{ "hooks": [hook.clone()] }],
            // 진행 알림만(herdr-projects setup.rs): 상태는 화면 감지가 정한다
            "PostToolUse": [{ "hooks": [hook] }],
        }
    })
}

/// 세션 안에서: hook 사건 하나를 agent에 보낸다. 실패해도 harness를 막지 않게 짧게 끝낸다. agent가 돌려준 출력(진행
/// 안내 JSON 한 줄)이 있으면 준다. 옛 agent는 `ok`만 답하고, 옛 `aaa hook`은 `ok` 뒤를 읽지 않는다.
pub async fn send(harness: &str, payload: Value) -> Result<Option<String>> {
    // Codex 중첩 thread의 사건은 보내지 않는다: 이 pane의 thread만 알린다
    if harness == "codex" && nested_codex(std::env::var("CODEX_THREAD_ID").ok().as_deref(), &payload) {
        return Ok(None);
    }
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
        let out = String::from_utf8_lossy(&reply[2..]).trim().to_string();
        Ok((!out.is_empty()).then_some(out))
    })
    .await
    .context("agent 응답 시간 초과")?
}

/// agent 쪽: 연결 하나에서 완성된 JSON 사건 하나를 읽는다.
/// Named Pipe의 shutdown은 EOF를 보내지 않으므로 쓰기 반쪽 닫기를 기다리지 않는다.
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
        match serde_json::from_slice(&buf) {
            Ok(msg) => return Ok(msg),
            Err(e) if e.is_eof() => {} // JSON·UTF-8이 read 경계에서 잘렸으면 나머지를 받는다.
            Err(e) => return Err(e.into()),
        }
    }
    Ok(serde_json::from_slice(&buf)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn hook_json_completion_and_errors_do_not_need_eof() {
        let payload = json!({"body": "한글\"\\\n".repeat(20)});
        let valid = serde_json::to_vec(&HookMsg {
            session: "s1".into(),
            token: "test-hook-secret".into(),
            harness: "post".into(),
            payload: payload.clone(),
        })
        .unwrap();
        // 작은 버퍼가 JSON·UTF-8을 여러 read로 나눈다. writer는 응답을 기다리듯 열린 채로 남긴다.
        for (bytes, close, expected) in [
            (valid.clone(), false, "완성"),
            (valid, true, "완성"),
            (b"{!".to_vec(), false, "문법"),
            ([b"{\"payload\":\"".as_slice(), &vec![b'x'; MAX_MSG]].concat(), false, "한도"),
            (b"{\"session\":".to_vec(), true, "미완성"),
        ] {
            let capacity = if expected == "완성" { 7 } else { MAX_MSG + 64 };
            let (mut writer, reader) = tokio::io::duplex(capacity);
            let writing = tokio::spawn(async move {
                writer.write_all(&bytes).await.unwrap();
                if close {
                    writer.shutdown().await.unwrap();
                }
                writer
            });
            let mut reader: aaa_common::ipc::LocalStream = Box::new(reader);
            let result = read_msg(&mut reader).await;
            // join 결과의 writer를 여기까지 보존하므로 EOF로 거짓 통과할 수 없다.
            let _writer = writing.await.unwrap();
            match expected {
                "완성" => {
                    let msg = result.unwrap();
                    assert_eq!(
                        (msg.session.as_str(), msg.harness.as_str(), msg.payload),
                        ("s1", "post", payload.clone())
                    );
                }
                "한도" => assert!(result.unwrap_err().to_string().contains("사건이 너무 크다")),
                _ => assert!(result.unwrap_err().downcast_ref::<serde_json::Error>().is_some(), "{expected}"),
            }
        }
    }

    #[test]
    fn subagent_and_foreign_session_events_are_dropped() {
        // herdr-projects progress.rs eligible·foreign_session 시험과 같은 경우
        let n = |p: Value| normalize("S", "claude", &p, 1);
        assert!(n(json!({"hook_event_name":"UserPromptSubmit","session_id":"0f5b2c1e-7a3d-4c8e-9b21-5d6e7f809a1b","agent_id":"x"})).is_none());
        assert!(
            n(json!({"hook_event_name":"UserPromptSubmit","session_id":"0f5b2c1e-7a3d-4c8e-9b21-5d6e7f809a1b","transcript_path":"/p/subagents/x.jsonl"}))
                .is_none()
        );
        assert!(n(json!({"hook_event_name":"UserPromptSubmit","session_id":"0f5b2c1e-7a3d-4c8e-9b21-5d6e7f809a1b","agent_id":null})).is_some());
        let other =
            n(json!({"hook_event_name":"UserPromptSubmit","session_id":"7d0c9b8a-6f5e-4d3c-8b2a-1f0e9d8c7b6a"}))
                .unwrap();
        assert!(foreign_session(Some("0f5b2c1e-7a3d-4c8e-9b21-5d6e7f809a1b"), &other));
        assert!(!foreign_session(Some("7d0c9b8a-6f5e-4d3c-8b2a-1f0e9d8c7b6a"), &other));
        assert!(!foreign_session(None, &other));
        let start =
            n(json!({"hook_event_name":"SessionStart","session_id":"7d0c9b8a-6f5e-4d3c-8b2a-1f0e9d8c7b6a"})).unwrap();
        assert!(!foreign_session(Some("0f5b2c1e-7a3d-4c8e-9b21-5d6e7f809a1b"), &start));
    }

    #[test]
    fn hooks_only_link_sessions_and_never_set_state() {
        let n = |h: &str, p: Value| normalize("S", h, &p, 1);
        let e = n("claude", json!({"hook_event_name":"SessionStart","session_id":"0f5b2c1e-7a3d-4c8e-9b21-5d6e7f809a1b","transcript_path":"/t.jsonl"}))
            .unwrap();
        assert_eq!(
            (e.kind.as_str(), e.harness_session.as_deref()),
            ("session_start", Some("0f5b2c1e-7a3d-4c8e-9b21-5d6e7f809a1b"))
        );
        assert_eq!(e.transcript.as_deref(), Some("/t.jsonl"));
        let p = n("claude", json!({"hook_event_name":"UserPromptSubmit","session_id":"0f5b2c1e-7a3d-4c8e-9b21-5d6e7f809a1b","prompt":"x"})).unwrap();
        assert_eq!(p.kind, "prompt_submit");
        // UUID 꼴이 아닌 세션 ID는 버린다(보안 감사 t-0340: `--resume` 인자가 된다). 사건은 그대로 받는다
        for bad in ["--dangerously-skip-permissions", "x; touch pwned", "abc"] {
            let e = n("claude", json!({"hook_event_name":"SessionStart","session_id":bad})).unwrap();
            assert_eq!((e.kind.as_str(), e.harness_session.as_deref()), ("session_start", None), "{bad}");
        }
        // 옛 agent가 띄운 세션의 상태 hook은 버린다: 상태는 화면 감지가 정한다
        for old in [
            json!({"hook_event_name":"Stop","session_id":"0f5b2c1e-7a3d-4c8e-9b21-5d6e7f809a1b"}),
            json!({"hook_event_name":"PermissionRequest","tool_name":"Bash","tool_input":{"command":"rm x"}}),
            json!({"hook_event_name":"Notification","notification_type":"permission_prompt","message":"m"}),
            json!({"hook_event_name":"Notification","notification_type":"idle_prompt"}),
            json!({"hook_event_name":"PostToolUse","session_id":"0f5b2c1e-7a3d-4c8e-9b21-5d6e7f809a1b"}),
            json!({"hook_event_name":"SessionEnd","session_id":"0f5b2c1e-7a3d-4c8e-9b21-5d6e7f809a1b"}),
            json!({"x":1}),
        ] {
            assert!(n("claude", old.clone()).is_none(), "{old}");
        }
        let c =
            n("codex", json!({"type":"agent-turn-complete","thread-id":"019a4c5e-1f2b-7c3d-8e4f-a5b6c7d8e9f0","last-assistant-message":"done"})).unwrap();
        assert_eq!(
            (c.kind.as_str(), c.harness_session.as_deref(), c.message),
            ("session", Some("019a4c5e-1f2b-7c3d-8e4f-a5b6c7d8e9f0"), None)
        );
        let r = n("report", json!({"percent":250,"activity":"테스트 중\n이상한 줄"})).unwrap();
        assert_eq!((r.kind.as_str(), r.percent), ("progress", Some(100)));
        assert_eq!(r.message.as_deref(), Some("테스트 중이상한 줄"));
    }

    /// herdr `tests/cli/hooks.rs`의 Codex 세션 시험과 같은 경우들
    #[test]
    fn codex_events_of_nested_threads_are_not_sent() {
        let notify = |id: &str| json!({"type":"agent-turn-complete","thread-id":id});
        assert!(!nested_codex(None, &notify("a")));
        assert!(!nested_codex(Some(""), &notify("a")));
        assert!(!nested_codex(Some("a"), &notify("a")));
        assert!(nested_codex(Some("parent"), &notify("nested")));
        assert!(!nested_codex(Some("s"), &json!({"hook_event_name":"SessionStart","session_id":"s"})));
        assert!(nested_codex(Some("s"), &json!({"type":"agent-turn-complete"})));
    }

    #[test]
    fn codex_notify_is_toml() {
        let v = codex_notify(Path::new("/A b/q\"x\\/aaa"));
        assert_eq!(v, r#"notify=["/A b/q\"x\\/aaa","hook","--agent","codex"]"#);
    }

    #[test]
    fn settings_quote_exe_and_register_session_and_guidance_hooks() {
        let v = claude_settings(Path::new("/A b/it's/aaa"));
        let cmd = v["hooks"]["SessionStart"][0]["hooks"][0]["command"].as_str().unwrap();
        assert_eq!(cmd, "'/A b/it'\\''s/aaa' hook --agent claude");
        assert_eq!(v["hooks"]["SessionStart"][0]["matcher"], "^(startup|resume|clear|compact|fork)$");
        let mut events: Vec<&str> = v["hooks"].as_object().unwrap().keys().map(String::as_str).collect();
        events.sort();
        // 상태 hook(Stop·PermissionRequest·Notification)은 없다. PostToolUse는 진행 알림만 한다(herdr-projects setup.rs)
        assert_eq!(events, ["PostToolUse", "SessionStart", "UserPromptSubmit"]);
    }

    /// herdr-projects progress.rs `helpers_and_the_reporters_own_call_do_not_trigger`. PostToolUse는 안내만 하고 host 사건은
    /// 아니다(상태는 화면 감지가 정한다).
    #[test]
    fn helpers_and_the_reporters_own_call_get_no_guidance() {
        assert_eq!(guide_kind(&json!({"hook_event_name":"PostToolUse","agent_id":"child"})), None);
        assert_eq!(
            guide_kind(&json!({"hook_event_name":"PostToolUse","transcript_path":"/x/subagents/y.jsonl"})),
            None
        );
        let own =
            json!({"hook_event_name":"PostToolUse","tool_input":{"command":"/p/aaa report --percent 5 --activity x"}});
        assert_eq!(guide_kind(&own), None);
        let other = json!({"hook_event_name":"PostToolUse","tool_input":{"command":"/p/aaa thread list demo"}});
        assert_eq!(guide_kind(&other), Some("PostToolUse"));
        assert!(normalize("S", "claude", &other, 1).is_none(), "PostToolUse가 host 사건이 됐다");
        assert_eq!(guide_kind(&json!({"hook_event_name":"SessionStart","source":"compact"})), Some("SessionStart"));
        assert_eq!(guide_kind(&json!({"hook_event_name":"Stop"})), None);
    }

    /// herdr-projects progress.rs `session_start_injects_instructions_and_resets_the_record`·
    /// `reminders_are_throttled_to_once_a_minute_and_stop_at_done`(초를 ms로)
    #[test]
    fn session_start_guides_and_reminders_are_throttled_to_once_a_minute() {
        let mut rec = Progress { percent: Some(50), reported_at: 5, reminded_at: 0 };
        let text = respond(&mut rec, "SessionStart", "/p/aaa", 1_000_000).unwrap();
        assert!(text.contains("`/p/aaa report --percent 25"), "{text}");
        assert_eq!(rec, Progress { reported_at: 0, percent: None, reminded_at: 1_000_000 });

        assert!(respond(&mut rec, "PostToolUse", "aaa", 1_030_000).is_none());
        assert!(respond(&mut rec, "PostToolUse", "aaa", 1_061_000).is_some());
        assert_eq!(rec.reminded_at, 1_061_000);
        // 새 보고도 1분 동안 알림을 멈춘다
        rec.reported_at = 1_100_000;
        assert!(respond(&mut rec, "PostToolUse", "aaa", 1_130_000).is_none());
        assert!(respond(&mut rec, "PostToolUse", "aaa", 1_200_000).is_some());
        rec.percent = Some(100);
        assert!(respond(&mut rec, "PostToolUse", "aaa", 9_000_000).is_none());
        assert!(respond(&mut rec, "UserPromptSubmit", "aaa", 9_001_000).unwrap().contains("Waiting for you"));
        assert!(respond(&mut rec, "Stop", "aaa", 9_002_000).is_none());
        let out: Value = serde_json::from_str(&output("PostToolUse", "hi")).unwrap();
        assert_eq!(out, json!({"hookSpecificOutput":{"hookEventName":"PostToolUse","additionalContext":"hi"}}));
    }
}

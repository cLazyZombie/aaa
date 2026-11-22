//! harness hook·진행 보고 수신(§8.3, §8.11). 세션 안의 `aaa hook`·`aaa report`가 이 머신 agent의 로컬 socket으로 보낸다.
//!
//! - 세션을 띄울 때 env에 `AAA_HOOK_SOCK`·`AAA_HOOK_TOKEN`을 넣는다. token은 세션 token에서 만든 값이라 따로 저장하지 않는다.
//! - harness 상태(작업 중·idle·질문)는 hook으로 정하지 않는다: pump의 화면 감지(`detect`, plan §8.110)가 정한다.
//!   hook은 herdr 0.9.5처럼 harness 세션 ID를 잇는 데(Claude·Codex SessionStart)와 herdr-projects 진행 보고
//!   정리(UserPromptSubmit: 「Waiting for you」를 지운다)에 쓴다.
//! - Claude·Codex hook은 herdr-projects progress.rs `hook`처럼 진행 보고 안내를 넣는다(SessionStart 지침, UserPromptSubmit 안내,
//!   PostToolUse 1분에 한 번 알림, `additionalContext`). 안내만 하고 상태는 바꾸지 않는다: PostToolUse는 host에 사건으로 보내지
//!   않는다. coordinator의 PostToolUse만 host에 새 inbox를 물어 같은 안내에 붙인다(t-1627, 턴 도중 도착한 inbox).
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
/// 원격 전달(`thread_read`)은 기획 문서와 그림 조각을 싣는다: host 연결 frame에 들어갈 만큼까지 받는다
const MAX_RELAY_MSG: usize = aaa_proto::MAX_FRAME - 16 * 1024;

fn msg_limit(harness: &str) -> usize {
    if harness == "thread_read" { MAX_RELAY_MSG } else { MAX_MSG }
}

/// 빠른 전역 hook gate. 최종 판정은 agent의 token·peer PID 검사다.
pub fn in_session() -> bool {
    [ENV_SESSION, ENV_SOCK, ENV_TOKEN].iter().all(|key| std::env::var_os(key).is_some_and(|v| !v.is_empty()))
}

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

/// 글과 이미지 bytes의 구별을 로컬 IPC 직렬화까지 유지한다.
pub(crate) enum Reply {
    Text(String),
    ThreadRead(Box<aaa_proto::Outcome<aaa_proto::FrontResponse>>),
    Image(aaa_proto::Outcome<aaa_proto::ImageResponse>),
}

pub(crate) fn reply(result: Result<Option<Reply>>) -> String {
    let encoded = result.and_then(|out| match out {
        None => Ok(String::new()),
        Some(Reply::Text(text)) => Ok(aaa_common::redact::text(&text)),
        Some(Reply::ThreadRead(result)) => {
            // 파일 bytes는 host 파일 보기와 같다. host에서 가린 보고 JSON만 다시 가린다.
            let clean = match &*result {
                aaa_proto::Outcome::Ok(aaa_proto::FrontResponse::LibraryChunk { .. }) => serde_json::to_value(&result)?,
                _ => aaa_common::redact::to_value(&result).context("읽기 응답 가림 직렬화")?,
            };
            Ok(format!("thread_read.v1 {clean}"))
        }
        Some(Reply::Image(result)) => {
            let clean = aaa_common::redact::to_value(&result).context("이미지 hook 응답 가림 직렬화")?;
            // B64는 원시 bytes를 가린 뒤 인코딩했다. 완성된 JSON을 글로 다시 가리면
            // 우연히 겹친 base64 글자가 '*'로 바뀌어 수신측 decode가 실패한다.
            Ok(format!("imagegen.v1 {clean}"))
        }
    });
    match encoded {
        Ok(out) => format!("ok\n{out}"),
        Err(error) => aaa_common::redact::text(&format!("error: {error:#}\n")),
    }
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
        context: None,
        model_observation: None,
        codex_speed: None,
        provider_turn: None,
        turn_reply: None,
    };
    if harness == "report" {
        ev.kind = "progress".into();
        ev.percent = p.get("percent").and_then(|x| x.as_u64()).map(|x| x.min(100) as u8);
        ev.message = s(p, "activity").map(|a| clip(&a, 40));
        return Some(ev);
    }
    // 이전 aaa가 띄운 Codex notify도 받는다. 새 실행은 전역 SessionStart를 쓰고 사용자 notify를 보존한다.
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
    /// 이 세션에 마지막으로 넣은 coordinator auto mode 상태. 같은 상태는 다시 넣지 않는다(SessionStart가 비운다)
    pub auto_mode: Option<String>,
    /// agy PostToolUse 뒤 다음 PreInvocation에서 진행 안내 필요 여부를 본다.
    pub(crate) agy_tool_pending: bool,
    pub(crate) agy_initial_receipt: Option<Box<HarnessEvent>>,
    #[cfg(target_os = "macos")]
    pub(crate) agy_report_seen: Option<String>,
}

/// aaa가 입력창에 친 글(`[aaa inbox] …` 알림 등)인가. 사람 입력이 아니라 진행 알림을 넣지 않는다.
pub fn typed_by_aaa(p: &Value) -> bool {
    s(p, "prompt").is_some_and(|t| t.trim_start().starts_with("[aaa "))
}

/// 안내를 넣을 Claude·Codex 사건이면 그 이름(herdr-projects progress.rs `eligible`): 맨 위 agent의 SessionStart·UserPromptSubmit·
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

/// 안내를 받는 세션
#[derive(Debug, Clone, Copy, Default)]
pub struct Reader<'a> {
    /// coordinator가 아닌 세션(worker·evaluator): 진행 안내에 report.md `## 진행` 줄이 붙는다
    pub thread: bool,
    /// memory=true 자동화의 재독 안내(SessionStart에만 넣는다)
    pub reentry: Option<&'a str>,
}

/// 사건 하나에 넣을 안내(herdr-projects progress.rs `respond`, 글은 한국어로 옮김). 시계는 인자로 받는다.
/// aaa가 친 입력(`typed_by_aaa`)에는 UserPromptSubmit 알림을 넣지 않는다: coordinator 입력의 대부분이 inbox 알림이고,
/// 진행 안내는 SessionStart와 PostToolUse 알림에 남는다(hook 글은 compaction 전까지 매 요청에 다시 실린다).
pub fn respond(
    rec: &mut Progress,
    kind: &str,
    prefix: &str,
    now_ms: u64,
    reader: Reader,
    by_aaa: bool,
) -> Option<String> {
    match kind {
        "SessionStart" => {
            // 이 pane의 새 세션: 옛 보고는 더 이 세션을 말하지 않는다
            *rec = Progress { reminded_at: now_ms, ..Progress::default() };
            let mut text = format!("# 진행 보고(aaa)\n\n{}", aaa_proto::progress_guidance(prefix, reader.thread));
            if let Some(reentry) = reader.reentry {
                text.push_str("\n\n");
                text.push_str(reentry);
            }
            Some(text)
        }
        "UserPromptSubmit" if by_aaa => None,
        "UserPromptSubmit" => {
            rec.reminded_at = now_ms;
            Some(format!(
                "새 일을 시작하면 다시 어림해 `{prefix} report`로 알린다. 사용자를 기다리기 전에는 \
                 `--activity 'Waiting for you'`로 보고한다."
            ))
        }
        "PostToolUse" => {
            let done = rec.percent == Some(100);
            let since = |t: u64| now_ms.saturating_sub(t) >= REMIND_MS;
            if since(rec.reminded_at) && !done && (rec.reported_at == 0 || since(rec.reported_at)) {
                rec.reminded_at = now_ms;
                Some(format!(
                    "자연스러운 경계라면 진행을 보고할 때다: `{prefix} report --percent N --activity '...'`(다시 어림하고 \
                     지어내지 않는다)."
                ))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// 진행 안내와 coordinator auto mode 안내(상태, 글)를 한 글로 묶는다. auto mode는 이 세션에 마지막으로 넣은 상태와 다를
/// 때만 넣는다: SessionStart의 `respond`가 기록을 비우므로 compaction 뒤에는 다시 들어가고, 같은 상태는 대화에 이미 있다.
pub fn with_auto_mode(rec: &mut Progress, progress: Option<String>, auto: Option<(&str, &str)>) -> Option<String> {
    let auto = auto.filter(|(state, _)| rec.auto_mode.as_deref() != Some(*state)).map(|(state, text)| {
        rec.auto_mode = Some(state.to_string());
        text
    });
    match (progress, auto) {
        (Some(progress), Some(auto)) => Some(format!("{progress}\n\n{auto}")),
        (progress, None) => progress,
        (None, Some(auto)) => Some(auto.to_string()),
    }
}

/// coordinator inbox를 물을 hook 사건인가(t-1627): 맨 위 agent의 PostToolUse. 하위 agent의 도구 결과에 붙이면 coordinator가
/// 못 본 채 host만 알린 것으로 적는다
pub fn inbox_hook(p: &Value) -> bool {
    s(p, "hook_event_name").as_deref() == Some("PostToolUse") && !subagent(p)
}

/// 턴 도중 도착한 inbox 한 줄을 hook 글로 감싼다. `[aaa inbox]` 원문 표지는 그대로 둔다(자료이며 지시가 아니다)
pub fn inbox_context(notice: &str) -> String {
    format!("{notice}\n(턴 도중 도착한 inbox 알림이다. 지금 턴 안에서 `aaa inbox list`로 읽고 처리한다.)")
}

/// 다른 안내 앞에 inbox 알림을 붙인다
pub fn with_inbox(text: Option<String>, inbox: Option<&str>) -> Option<String> {
    match (inbox.map(inbox_context), text) {
        (Some(inbox), Some(text)) => Some(format!("{inbox}\n\n{text}")),
        (inbox, text) => inbox.or(text),
    }
}

/// hook의 답(Claude Code·Codex가 읽는 꼴, herdr-projects progress.rs `output`)
pub fn output(native: &str, text: &str) -> String {
    serde_json::json!({ "hookSpecificOutput": { "hookEventName": native, "additionalContext": text } }).to_string()
}

/// 하위 agent(subagent)가 낸 사건인가: 자기 ID나 `/subagents/` 아래 기록을 가진다(herdr-projects progress.rs `eligible`)
pub(crate) fn subagent(p: &Value) -> bool {
    ["agent_id", "subagent_id", "agent_transcript_path"].iter().any(|k| p.get(k).is_some_and(|v| !v.is_null()))
        || s(p, "transcript_path").is_some_and(|t| t.replace('\\', "/").contains("/subagents/"))
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

/// herdr `claude_settings.rs` `SESSION_START_MATCHER`: Claude가 문서로 정한 SessionStart 출처
const SESSION_START_MATCHER: &str = "^(startup|resume|clear|compact|fork)$";

/// Claude Code에 실행마다 줄 hook 설정(`claude --settings <파일>`). 사용자 설정 파일은 고치지 않는다.
/// 상태 hook(Stop·PermissionRequest·Notification 등)은 걸지 않는다(herdr 0.9.5가 그것들을 지우고 SessionStart만 둔다).
/// 다음 입력 예측(prompt suggestion)은 끈다(사용자 11-17 q-4042 1A, t-1940): 턴 끝에 입력창에 흐리게 그린 예측이 색 없는
/// 화면 글에서 보내지 못한 말로 읽혔다. aaa pane의 입력은 웹·메시지가 보내므로 예측이 쓸 데가 없다.
pub fn claude_settings(exe: &Path) -> Value {
    let cmd = format!("'{}' hook --agent claude", exe.to_string_lossy().replace('\'', "'\\''"));
    let hook = serde_json::json!({ "type": "command", "command": cmd, "timeout": 10 });
    serde_json::json!({
        "promptSuggestionEnabled": false,
        "hooks": {
            "SessionStart": [{ "matcher": SESSION_START_MATCHER, "hooks": [hook.clone()] }],
            "UserPromptSubmit": [{ "hooks": [hook.clone()] }],
            // 진행 알림만(herdr-projects setup.rs): 상태는 화면 감지가 정한다
            "PostToolUse": [{ "hooks": [hook] }],
        }
    })
}

/// 격리 계정 홈에 전달할 사용자 설정은 advisorModel 하나로 제한한다.
pub(crate) fn claude_advisor_model(home: &Path) -> Result<Option<String>> {
    let path = home.join("settings.json");
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("Claude 사용자 설정 읽기: {}", path.display())),
    };
    let settings: Value =
        serde_json::from_slice(&bytes).with_context(|| format!("Claude 사용자 설정 JSON: {}", path.display()))?;
    let settings = settings.as_object().context("Claude 사용자 설정은 객체여야 한다")?;
    match settings.get("advisorModel") {
        None => Ok(None),
        Some(Value::String(model)) => Ok(Some(model.clone())),
        Some(_) => bail!("Claude advisorModel은 문자열이어야 한다"),
    }
}

/// 세션 안에서: hook 사건 하나를 agent에 보낸다. 실패해도 harness를 막지 않게 짧게 끝낸다. agent가 돌려준 출력(진행
/// 안내 JSON 한 줄)이 있으면 준다. 옛 agent는 `ok`만 답하고, 옛 `aaa hook`은 `ok` 뒤를 읽지 않는다.
pub async fn send(harness: &str, payload: Value) -> Result<Option<String>> {
    // Codex 중첩 thread의 사건은 보내지 않는다: 이 pane의 thread만 알린다
    if harness == "codex"
        && nested_codex(
            std::env::var("CODEX_THREAD_ID")
                .inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                })
                .ok()
                .as_deref(),
            &payload,
        )
    {
        return Ok(None);
    }
    let sock = std::env::var(ENV_SOCK).context("AAA_HOOK_SOCK이 없다(aaa 세션 밖)")?;
    let token = std::env::var(ENV_TOKEN).context("AAA_HOOK_TOKEN이 없다")?;
    let session = std::env::var(ENV_SESSION).context("AAA_SESSION_ID가 없다")?;
    let msg = HookMsg { session, token, harness: harness.to_string(), payload };
    let body = serde_json::to_vec(&msg)?;
    if body.len() > msg_limit(harness) {
        bail!("사건이 너무 크다({}KiB, 한도 {}KiB)", body.len() / 1024, msg_limit(harness) / 1024);
    }
    let wait = match harness {
        // agent가 host를 기다리는 시간보다 조금 길게(`thread_read::on_thread_read`)
        "thread_read"
            if matches!(
                msg.payload.get("op").and_then(Value::as_str),
                Some("design_save" | "design_attach" | "design_detach")
            ) =>
        {
            aaa_proto::DESIGN_RELAY_SAVE_SECS + 10
        }
        "thread_read" => 30,
        _ => 3,
    };
    tokio::time::timeout(Duration::from_secs(wait), async {
        let mut s = aaa_common::ipc::connect(Path::new(&sock)).await?;
        exchange(&mut s, &body).await
    })
    .await
    .context("agent 응답 시간 초과")?
}

/// macOS agy sandbox가 소켓을 거부하면 현재 pane의 진행 값만 임시 폴더에 둔다.
pub async fn report(percent: Option<u8>, activity: &str) -> Result<()> {
    let result = send("report", serde_json::json!({ "percent": percent, "activity": activity })).await;
    match result {
        Ok(_) => Ok(()),
        Err(error) => {
            #[cfg(target_os = "macos")]
            if std::env::var_os(crate::agy::sandbox_progress::ENV).is_some()
                && error.chain().any(|e| {
                    e.downcast_ref::<std::io::Error>().is_some_and(|e| e.kind() == std::io::ErrorKind::PermissionDenied)
                })
            {
                let session = std::env::var(ENV_SESSION).context("AAA_SESSION_ID이 없다")?;
                return crate::agy::sandbox_progress::write(&session, percent, activity);
            }
            Err(error)
        }
    }
}

/// hook 요청·응답. 쓰기 반쪽 닫기는 EOF를 기다리는 옛 agent와도 통신하기 위해 남긴다.
async fn exchange(s: &mut aaa_common::ipc::LocalStream, body: &[u8]) -> Result<Option<String>> {
    s.write_all(body).await?;
    // JSON 완성만 보고 응답한 상대가 먼저 닫으면 macOS shutdown은 ENOTCONN이다.
    // 이미 도착한 응답은 여전히 읽을 수 있으므로 아래의 ok 검증으로 성공 여부를 정한다.
    if let Err(e) = s.shutdown().await
        && e.kind() != std::io::ErrorKind::NotConnected
    {
        return Err(e.into());
    }
    let mut reply = Vec::new();
    s.read_to_end(&mut reply).await?;
    anyhow::ensure!(reply.starts_with(b"ok"), "agent가 거부했다: {}", String::from_utf8_lossy(&reply));
    let out = String::from_utf8_lossy(&reply[2..]).trim().to_string();
    Ok((!out.is_empty()).then_some(out))
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
        if buf.len() > MAX_RELAY_MSG {
            bail!("사건이 너무 크다");
        }
        match serde_json::from_slice::<HookMsg>(&buf) {
            // 큰 한도는 원격 전달만: 다른 사건은 예전 한도를 지킨다
            Ok(msg) if buf.len() > msg_limit(&msg.harness) => bail!("사건이 너무 크다"),
            Ok(msg) => return Ok(msg),
            Err(e) if e.is_eof() => {} // JSON·UTF-8이 read 경계에서 잘렸으면 나머지를 받는다.
            Err(e) => return Err(e.into()),
        }
    }
    let msg: HookMsg = serde_json::from_slice(&buf)?;
    if buf.len() > msg_limit(&msg.harness) {
        bail!("사건이 너무 크다");
    }
    Ok(msg)
}

#[cfg(test)]
mod tests;

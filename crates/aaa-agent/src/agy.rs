//! 개인 구독용 agy 실행 설정 확인. 공식 CLI 설정의 승인·과금 선택만 읽는다.

use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

use aaa_common::child_process::TokioCommandExt as _;
use aaa_proto::{AgyQuotaBucket, AgyQuotaSample, HarnessEvent};
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

const MAX_SETTINGS_BYTES: u64 = 1 << 20;
pub(crate) const HOOK_GROUP: &str = "aaa-agy";

/// macOS agy sandbox는 소켓과 작업 폴더 쓰기를 거부할 수 있다. 시스템 임시 폴더의 진행 값만 공식 hook이 회수한다.
#[cfg(target_os = "macos")]
pub(crate) mod sandbox_progress {
    use super::*;
    use std::fs::{self, DirBuilder, OpenOptions};
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
    use std::path::PathBuf;

    pub(crate) const ENV: &str = "AAA_AGY_SANDBOX_PROGRESS";

    #[derive(serde::Serialize, Deserialize)]
    pub(crate) struct Report {
        pub nonce: String,
        pub percent: Option<u8>,
        pub activity: String,
    }

    fn dir(session: &str) -> Result<PathBuf> {
        ensure!(
            session.len() == 26 && session.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit()),
            "agy 보고의 pane ID 형식이 아니다"
        );
        Ok(Path::new("/private/tmp").join(format!("aaa-agy-report-{session}")))
    }

    fn check_dir(path: &Path) -> Result<()> {
        let m = fs::symlink_metadata(path)?;
        let uid = unsafe { libc::geteuid() };
        ensure!(m.is_dir() && m.uid() == uid && m.mode() & 0o777 == 0o700, "agy 진행 보고 임시 폴더를 확인할 수 없다");
        Ok(())
    }

    pub(crate) fn write(session: &str, percent: Option<u8>, activity: &str) -> Result<()> {
        let dir = dir(session)?;
        match DirBuilder::new().mode(0o700).create(&dir) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e).context("agy 진행 보고 임시 폴더 만들기"),
        }
        check_dir(&dir)?;
        let value = Report {
            nonce: aaa_common::util::new_id(),
            percent: percent.map(|n| n.min(100)),
            activity: activity.chars().take(40).collect(),
        };
        aaa_common::util::write_json(&dir.join("progress.json"), &value).context("agy 임시 폴더에 진행 보고 쓰기")
    }

    pub(crate) fn read(session: &str) -> Option<Report> {
        let dir = dir(session).ok()?;
        check_dir(&dir).ok()?;
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(dir.join("progress.json"))
            .ok()?;
        let meta = file.metadata().ok()?;
        if !meta.is_file() || meta.len() > 1024 {
            return None;
        }
        serde_json::from_reader(file.take(1024)).ok()
    }

    pub(crate) fn report_tool_succeeded(payload: &Value, exe: &Path) -> bool {
        if payload.pointer("/toolCall/name").and_then(Value::as_str) != Some("run_command")
            || payload.get("error").and_then(Value::as_str).is_some_and(|s| !s.is_empty())
        {
            return false;
        }
        let Some(command) = payload.pointer("/toolCall/args/CommandLine").and_then(Value::as_str) else {
            return false;
        };
        let exe = exe.to_string_lossy();
        let single = format!("'{}'", exe.replace('\'', "'\\''"));
        let double = format!("\"{exe}\"");
        [single.as_str(), double.as_str(), exe.as_ref(), "aaa"].into_iter().any(|prefix| {
            command
                .trim_start()
                .strip_prefix(prefix)
                .and_then(|s| s.strip_prefix(char::is_whitespace))
                .and_then(|s| s.trim_start().strip_prefix("report"))
                .is_some_and(|s| s.starts_with(char::is_whitespace))
        })
    }

    pub(crate) fn clean(session: &str) {
        if let Ok(dir) = dir(session)
            && check_dir(&dir).is_ok()
        {
            let _ = fs::remove_file(dir.join("progress.json"));
            let _ = fs::remove_dir(dir);
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Settings {
    agent_mode: Option<String>,
    tool_permission: Option<String>,
    model_provider: Option<String>,
    use_g1_credits: Option<bool>,
}

fn check_settings(text: &[u8]) -> Result<()> {
    // serde_json의 오류에 파일 값이 들어갈 수 있어 줄·칸만 돌려준다.
    let settings: Settings = serde_json::from_slice(text)
        .map_err(|e| anyhow::anyhow!("agy settings.json 형식이 다르다({}줄 {}칸)", e.line(), e.column()))?;
    ensure!(settings.agent_mode.as_deref().is_none_or(str::is_empty), "agy 실행 모드를 기본으로 정한다");
    ensure!(
        matches!(settings.tool_permission.as_deref(), Some("request-review" | "strict") | None),
        "agy 도구 권한을 request-review 또는 strict로 정한다"
    );
    ensure!(settings.use_g1_credits != Some(true), "agy Use AI Credits를 끈 뒤 시작한다");
    ensure!(
        settings.model_provider.as_deref().is_none_or(str::is_empty),
        "agy에서 개인 Google 구독 provider를 고른 뒤 시작한다"
    );
    Ok(())
}

pub(crate) fn check_local_settings(home: &Path) -> Result<()> {
    let path = home.join(".gemini/antigravity-cli/settings.json");
    ensure!(
        std::fs::metadata(&path).context("agy settings.json을 찾지 못했다")?.is_file(),
        "agy settings.json이 일반 파일이 아니다"
    );
    let file = File::open(path).context("agy settings.json을 열지 못했다. 실행 머신의 OS 사용자 설정을 확인한다")?;
    ensure!(file.metadata()?.is_file(), "agy settings.json이 일반 파일이 아니다");
    let mut data = Vec::new();
    file.take(MAX_SETTINGS_BYTES + 1).read_to_end(&mut data).context("agy settings.json 읽기 실패")?;
    ensure!(data.len() as u64 <= MAX_SETTINGS_BYTES, "agy settings.json이 너무 크다");
    check_settings(&data)?;
    for key in ["GEMINI_API_KEY", "GOOGLE_API_KEY", "GOOGLE_GENAI_USE_VERTEXAI", "AGY_ADC_AUTH"] {
        ensure!(
            std::env::var_os(key).is_none_or(|v| v.is_empty()),
            "agy 개인 구독 실행 머신 환경에서 {key}를 해제한다"
        );
    }
    Ok(())
}

/// agy 1.5.13은 `--settings`를 현재 실행에만 적용한다.
pub(crate) fn status_settings(exe: &Path) -> String {
    serde_json::json!({
        "agentMode": "",
        "statusLine": {
            "type": "command",
            "command": aaa_common::codex_hooks::hook_command(exe, "agy-status"),
            "stack_with_default": true
        }
    })
    .to_string()
}

pub(crate) fn hook_definition(exe: &Path) -> Value {
    let handler = |kind| serde_json::json!({"type":"command", "command":aaa_common::codex_hooks::hook_command(exe, kind), "timeout":3});
    serde_json::json!({
        "SessionStart": [handler("agy-session-start")],
        "PreInvocation": [handler("agy-pre-invocation")],
        "PostToolUse": [{"matcher":"", "hooks":[handler("agy-post-tool-use")]}],
        "Stop": [handler("agy-stop")]
    })
}

fn hook_file(path: &Path) -> Option<Value> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_SETTINGS_BYTES {
        return None;
    }
    let file = File::open(path).ok()?;
    serde_json::from_reader(file.take(MAX_SETTINGS_BYTES)).ok()
}

pub(crate) fn hooks_ready(home: &Path, cwd: Option<&Path>, exe: &Path) -> bool {
    let expected = hook_definition(exe);
    if let Some(local) = cwd.and_then(|p| hook_file(&p.join(".agents/hooks.json")))
        && let Some(definition) = local.get(HOOK_GROUP)
    {
        return definition == &expected;
    }
    hook_file(&home.join(".gemini/config/hooks.json")).is_some_and(|v| v.get(HOOK_GROUP) == Some(&expected))
}

pub(crate) fn matches_account(account: Option<&aaa_config::AgyAccount>, payload: &Value) -> bool {
    let Some(email) = payload.get("email").and_then(Value::as_str).filter(|s| !s.is_empty()) else { return false };
    account.is_some_and(|a| {
        hex::encode(Sha256::digest(email.trim().to_ascii_lowercase().as_bytes())).eq_ignore_ascii_case(&a.login_sha256)
    })
}

pub(crate) fn quota_sample(session: &str, payload: &Value, at_ms: u64) -> Option<AgyQuotaSample> {
    let label = |s: &str| {
        !s.is_empty() && s.len() <= 80 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b" -_.".contains(&b))
    };
    let iso = |v: &Value| {
        let text = v.as_str()?;
        (text.len() < 40 && aaa_common::util::parse_iso(text).is_some()).then(|| text.to_owned())
    };
    let quota = payload.get("quota")?.as_object()?;
    let mut buckets: Vec<_> = quota
        .iter()
        .filter(|(id, sample)| label(id) && sample.is_object())
        .take(32)
        .map(|(id, sample)| AgyQuotaBucket {
            id: id.clone(),
            remaining_fraction: sample["remaining_fraction"].as_f64().filter(|n| (0.0..=1.0).contains(n)),
            remaining_amount: sample["remaining_amount"].as_f64().filter(|n| n.is_finite() && *n >= 0.0),
            resets_at: iso(&sample["reset_time"]),
            observed_at: iso(&sample["observed_at"]),
            disabled: sample["disabled"].as_bool().unwrap_or(false),
        })
        .collect();
    buckets.sort_unstable_by(|a, b| a.id.cmp(&b.id));
    let plan = payload["plan_tier"].as_str().filter(|s| label(s)).map(str::to_owned);
    Some(AgyQuotaSample { session: session.into(), at_ms, plan, buckets })
}

fn has_model(list: &str, model: &str, effort: &str) -> bool {
    list.lines().any(|line| {
        let Some((id, _label)) = line.split_once('\t') else { return false };
        id == model && (effort.is_empty() || id.rsplit_once('-').is_some_and(|(_, suffix)| suffix == effort))
    })
}

/// 1.5.13의 모델 목록은 인가된 계정의 ID와 보이는 이름을 탭으로 구분한다.
pub(crate) async fn check_model(argv: &[String]) -> Result<()> {
    let exe = argv.first().context("agy 실행 경로가 없다")?;
    let argument = |name| argv.windows(2).find(|a| a[0] == name).map(|a| a[1].as_str());
    let model = argument("--model").context("agy 실행에 모델 ID가 없다")?;
    let effort = argument("--effort").unwrap_or_default();
    let output = tokio::time::timeout(
        Duration::from_secs(15),
        tokio::process::Command::new(exe)
            .arg("models")
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true)
            .output_guarded(),
    )
    .await
    .context("agy models 시간 초과")?
    .context("agy models 실행 실패")?;
    ensure!(output.status.success(), "agy models 실패({})", output.status);
    let list = std::str::from_utf8(&output.stdout).context("agy models 출력이 UTF-8이 아니다")?;
    ensure!(has_model(list, model, effort), "이 머신의 agy models에 지정한 모델과 effort가 없다");
    Ok(())
}

fn conversation_id<'a>(payload: &'a Value, field: &str) -> Option<&'a str> {
    payload.get(field)?.as_str().filter(|id| aaa_common::util::harness_session_ok(id))
}

pub(crate) fn status_conversation(payload: &Value) -> Option<&str> {
    if payload.get("product")?.as_str()? == "antigravity" { conversation_id(payload, "conversation_id") } else { None }
}

pub(crate) fn status_activity(payload: &Value) -> Option<(&'static str, Option<&'static str>)> {
    if payload.get("product")?.as_str()? != "antigravity" {
        return None;
    }
    let state = payload.get("agent_state")?.as_str()?;
    match state {
        "authenticating" => Some(("agy_starting", Some("Google 로그인 확인 중"))),
        "initializing" => Some(("agy_starting", None)),
        "error" => Some(("error", Some("agy 실행 오류"))),
        _ if payload.get("tool_confirmation_pending").and_then(Value::as_bool) == Some(true) => None,
        "working" | "tool_use" | "reviewing" => Some(("working", None)),
        "idle"
            if payload.get("email").and_then(Value::as_str).is_some_and(|s| !s.is_empty())
                && payload.pointer("/sandbox/enabled").and_then(Value::as_bool) == Some(true)
                && payload.get("cycle_mode").and_then(Value::as_str).is_none_or(str::is_empty)
                && payload.get("pending_input_count").and_then(Value::as_u64).unwrap_or(0) == 0 =>
        {
            Some(("idle", None))
        }
        _ => Some(("agy_starting", Some("agy 입력 준비 상태 확인 필요"))),
    }
}

pub(crate) fn root_hook<'a>(root: Option<&str>, payload: &'a Value) -> Option<&'a str> {
    let id = conversation_id(payload, "conversationId")?;
    let child_transcript = payload
        .get("transcriptPath")
        .and_then(Value::as_str)
        .is_some_and(|p| p.replace('\\', "/").contains("/subagents/"));
    (root == Some(id) && !child_transcript && !crate::hooks::subagent(payload)).then_some(id)
}

pub(crate) fn hook_reply(
    harness: &str,
    root: Option<&str>,
    payload: &Value,
    progress: &mut crate::hooks::Progress,
    prefix: &str,
    now: u64,
) -> String {
    let is_root = root_hook(root, payload).is_some();
    match harness {
        "agy-post-tool-use" if is_root => progress.agy_tool_pending = true,
        "agy-pre-invocation" if is_root && std::mem::take(&mut progress.agy_tool_pending) => {
            let reader = crate::hooks::Reader { thread: true, reentry: None };
            if let Some(text) = crate::hooks::respond(progress, "PostToolUse", prefix, now, reader, false) {
                return serde_json::json!({"injectSteps": [{"ephemeralMessage": text}]}).to_string();
            }
        }
        "agy-stop" => {
            if is_root && payload.get("fullyIdle").and_then(Value::as_bool) == Some(true) {
                progress.agy_tool_pending = false;
            }
            return r#"{"decision":""}"#.into();
        }
        _ => {}
    }
    "{}".into()
}

pub(crate) fn pre_invocation(session: &str, root: Option<&str>, payload: &Value, at_ms: u64) -> Option<HarnessEvent> {
    let id = root_hook(root, payload)?;
    let input = payload.get("lastUserInput")?.as_str()?;
    if input.is_empty() {
        return None;
    }
    let receipt = Sha256::digest(aaa_proto::input::receipt_text(input).as_bytes());
    Some(event(session, "agy_receipt", Some(id), Some(hex::encode(receipt)), at_ms))
}

pub(crate) fn event(session: &str, kind: &str, id: Option<&str>, message: Option<String>, at_ms: u64) -> HarnessEvent {
    HarnessEvent {
        session: session.into(),
        harness: "agy".into(),
        kind: kind.into(),
        harness_session: id.map(str::to_owned),
        message,
        question_kind: None,
        percent: None,
        at_ms,
        transcript: None,
        plan: None,
        background: None,
        context: None,
        model_observation: None,
        codex_speed: None,
        turn_reply: None,
        provider_turn: None,
    }
}

/// 첫 PreInvocation은 상태줄의 첫 ID보다 먼저 올 수 있다. 상태줄에서 대조할 때까지 해시만 보관한다.
pub(crate) fn initial_receipt(session: &str, payload: &Value, at_ms: u64) -> Option<HarnessEvent> {
    if payload.get("invocationNum").and_then(Value::as_u64) != Some(0) {
        return None;
    }
    pre_invocation(session, conversation_id(payload, "conversationId"), payload, at_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_tool_completion_reminds_at_next_invocation_once() {
        let root = "00000000-0000-4000-8000-000000000001";
        let child = serde_json::json!({"conversationId": "00000000-0000-4000-8000-000000000002"});
        let event = serde_json::json!({"conversationId": root, "fullyIdle": true});
        let mut progress = crate::hooks::Progress::default();
        let mut reply = |kind, payload, at| hook_reply(kind, Some(root), payload, &mut progress, "aaa", at);
        assert_eq!(reply("agy-post-tool-use", &child, 60000), "{}");
        assert_eq!(reply("agy-pre-invocation", &event, 60000), "{}");
        assert_eq!(reply("agy-post-tool-use", &event, 60000), "{}");
        let answer: Value = serde_json::from_str(&reply("agy-pre-invocation", &event, 60000)).unwrap();
        assert!(answer["injectSteps"][0]["ephemeralMessage"].as_str().unwrap().contains("aaa report"));
        assert_eq!(reply("agy-pre-invocation", &event, 120000), "{}");
        assert_eq!(reply("agy-post-tool-use", &event, 120000), "{}");
        assert_eq!(reply("agy-stop", &event, 120000), r#"{"decision":""}"#);
        assert_eq!(reply("agy-pre-invocation", &event, 120000), "{}");
    }

    #[test]
    fn first_input_evidence_uses_only_the_observed_root_and_hides_the_input() {
        let root = "00000000-0000-4000-8000-000000000001";
        let child = "00000000-0000-4000-8000-000000000002";
        let status = serde_json::json!({"product": "antigravity", "conversation_id": root});
        assert_eq!(status_conversation(&status), Some(root));
        let mut hook = serde_json::json!({"conversationId": child, "lastUserInput": "brief body"});
        assert!(pre_invocation("pane", Some(root), &hook, 20).is_none());
        hook["conversationId"] = root.into();
        assert!(pre_invocation("pane", None, &hook, 20).is_none());
        let receipt = pre_invocation("pane", Some(root), &hook, 20).unwrap();
        assert_eq!(receipt.harness_session.as_deref(), Some(root));
        assert_eq!(receipt.message.unwrap(), hex::encode(Sha256::digest(b"brief body")));
    }

    #[test]
    fn settings_require_explicit_default_mode_and_refuse_permission_or_billing_overrides() {
        let ok = br#"{"agentMode":"","toolPermission":"request-review","useG1Credits":false}"#;
        assert!(check_settings(ok).is_ok());
        for (settings, reason) in [
            (r#"{"agentMode":"auto"}"#, "실행 모드"),
            (r#"{"agentMode":"","toolPermission":"always-proceed"}"#, "도구 권한"),
            (r#"{"agentMode":"","useG1Credits":true}"#, "Use AI Credits"),
            (r#"{"agentMode":"","modelProvider":"gemini"}"#, "개인 Google 구독"),
        ] {
            assert!(check_settings(settings.as_bytes()).unwrap_err().to_string().contains(reason));
        }
    }

    #[test]
    fn session_override_stacks_the_status_command_with_default_status() {
        let settings: Value = serde_json::from_str(&status_settings(Path::new("/opt/a b/aaa"))).unwrap();
        assert_eq!(settings["statusLine"]["type"], "command");
        assert_eq!(settings["statusLine"]["stack_with_default"], true);
        assert_eq!(settings["agentMode"], "");
        assert_eq!(settings.as_object().unwrap().len(), 2);
        #[cfg(not(windows))]
        assert_eq!(settings["statusLine"]["command"], "'/opt/a b/aaa' hook --agent agy-status");
    }

    #[test]
    fn model_id_and_effort_must_match_the_installed_cli_catalog() {
        let list = "gemini-3.8-flash-low\tGemini 3.8 Flash (Low)\r\nclaude-sonnet-4-6\tClaude Sonnet 4.6 (Thinking)";
        assert!(has_model(list, "gemini-3.8-flash-low", "low"));
        assert!(has_model(list, "claude-sonnet-4-6", ""));
        assert!(!has_model(list, "gemini-3.8-flash-low", "high"));
        assert!(!has_model(list, "gemini-3.8-flash", "low"));
        assert!(!has_model(list, "Gemini 3.8 Flash (Low)", ""));
    }

    #[test]
    fn quota_sample_keeps_observation_and_reset_independent() {
        let payload = serde_json::json!({
            "plan_tier": "Antigravity Starter Quota",
            "email": "owner@example.com",
            "quota": {
                "gemini-weekly": {"remaining_fraction": 0.9927, "reset_time": "2030-11-21T06:00:00Z"},
                "another-window": {"remaining_amount": 0, "remaining_fraction": 2, "disabled": true},
                "malformed": null
            }
        });
        let sample = quota_sample("pane", &payload, 1).unwrap();
        assert_eq!(sample.buckets.len(), 2);
        assert!(!serde_json::to_string(&sample).unwrap().contains("owner@example.com"));
        let weekly = &sample.buckets[1];
        assert_eq!(weekly.id, "gemini-weekly");
        assert_eq!(weekly.remaining_fraction, Some(0.9927));
        assert!(weekly.observed_at.is_none() && weekly.resets_at.is_some());
        assert_eq!(sample.buckets[0].remaining_amount, Some(0.0));
        assert!(sample.buckets[0].remaining_fraction.is_none() && sample.buckets[0].disabled);
    }
}

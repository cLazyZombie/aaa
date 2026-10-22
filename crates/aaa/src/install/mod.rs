//! 탐지·계획·상태 조회. 쓰기 CLI는 I6의 검증된 승인 executor까지 needs_user로 닫는다.
use std::io::{IsTerminal, Write};
use std::path::Path;
use std::process::ExitCode;

use aaa_proto::install::*;

mod args;
mod detect;
mod engine;
mod output;
mod platform;
mod profiles;
mod prompt;
mod review;
mod services;
mod summary;
mod wizard;
pub use args::Args;

pub(crate) fn service_dispatch(context: &Path, sha: &str) -> ExitCode {
    engine::service_dispatch(context, sha)
}

pub(crate) fn probe_dispatch(state: &Path, executor_sha256: &str) -> ExitCode {
    engine::probe_dispatch(state, executor_sha256)
}
pub(crate) fn bootstrap_dispatch(state: &Path, executor_sha256: &str, issuer_sha256: &str) -> ExitCode {
    engine::bootstrap_dispatch(state, executor_sha256, issuer_sha256)
}

pub(crate) fn worker_dispatch(state: &Path, id: &str, sha: &str, resume: bool) -> ExitCode {
    engine::worker_dispatch(state, id, sha, resume)
}

/// 문법 오류가 나도 install의 출력 계약을 선택한다. 다른 명령의 인자 안 install은 세지 않는다.
pub fn requested_output(command: &clap::Command) -> Option<(bool, bool)> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let flag = |name: &str| {
        args.iter().any(|a| {
            a.to_str().is_some_and(|s| s == name || s.strip_prefix(name).is_some_and(|rest| rest.starts_with('=')))
        })
    };
    let mut i = 0;
    while i < args.len() {
        if ["--config-dir", "--state-dir", "--instance"].iter().any(|v| args[i] == *v) {
            i += 2;
            continue;
        }
        if let Some(cmd) = command.get_subcommands().find(|cmd| {
            args[i]
                .to_str()
                .is_some_and(|name| cmd.get_name() == name || cmd.get_all_aliases().any(|alias| alias == name))
        }) {
            return (cmd.get_name() == "install").then(|| (flag("--json"), flag("--json-stream")));
        }
        // 모르는 앞 옵션/잘못된 bool 값 때문에 설치 문법 오류가 원문 clap 출력으로 빠지지 않는다.
        i += 1;
    }
    None
}

pub(super) fn context(plan: &InstallPlan, result: &mut InstallResult) {
    result.role = plan.choices.role;
    result.instance = plan.choices.instance.clone();
    result.platform = Some(plan.context.platform.clone());
    result.binary_sha256 = Some(plan.context.binary_sha256.clone());
    summary::populate(plan, result);
}

pub fn error(code: &str, exit: u8) -> Box<InstallResult> {
    let mut r = InstallResult::invalid();
    r.exit_code = exit;
    match exit {
        3 => {
            r.status = InstallStatus::NeedsUser;
            r.error = None;
            r.needs_user.push(InstallNeedsUser {
                code: code.into(),
                target: "installation".into(),
                message: need_action(code).unwrap_or("보호된 승인·입력과 실제 상태를 확인하세요").into(),
            });
        }
        130 => {
            r.status = InstallStatus::Cancelled;
            r.error = None;
        }
        _ => {
            r.error = Some(InstallError {
                code: code.into(),
                message: "설치 계획의 입력과 현지 전제를 확인하세요".into(),
                details: vec![],
            })
        }
    }
    Box::new(r)
}

/// 즉시 결과와 내구 status가 같은 사용자 조작을 안내한다. 원격 오류 원문은 받지 않는다.
pub(super) fn need_action(code: &str) -> Option<&'static str> {
    match code {
        "installation_secret_input_outside_required" => Some(
            "서비스 암호는 사람이 웹의 전용 가림 입력칸 또는 그 PC의 aaa 밖 콘솔에 직접 넣으세요. 대화·질문 답·메모·agent 도구로 전달하지 마세요",
        ),
        "installation_enrollment_window_closed" => Some(
            "host 웹 또는 aaa machine enroll --open 10m으로 등록 창을 열고 agent SAS를 확인한 뒤 한 줄 설치에서 같은 ID로 계속하세요",
        ),
        "installation_resume_executor_changed" => Some(
            "이전 승인은 다른 실행 파일에 결속돼 있습니다. 원래 기록을 보존하고 한 줄 설치에서 새 repair 계획을 검토하세요",
        ),
        _ => None,
    }
}

fn needs(code: &str, target: &str) -> Box<InstallResult> {
    let mut result = error(code, 3);
    result.error = None;
    result.status = InstallStatus::NeedsUser;
    result.needs_user.clear();
    result.needs_user.push(InstallNeedsUser {
        code: code.into(),
        target: target.into(),
        message: need_action(code).unwrap_or("보호된 승인·입력과 실제 상태를 확인하세요").into(),
    });
    result
}

pub fn run(args: &Args, json: bool, instance: Option<&str>, config: Option<&Path>, state: Option<&Path>) -> ExitCode {
    let mut reporter = output::Reporter::new(args.stream());
    let result = (|| {
        let instance = instance.map(str::to_owned).or_else(aaa_config::paths::instance);
        let config_env = std::env::var_os("AAA_CONFIG_DIR").filter(|v| !v.is_empty()).map(std::path::PathBuf::from);
        let state_env = std::env::var_os("AAA_STATE_DIR").filter(|v| !v.is_empty()).map(std::path::PathBuf::from);
        let invocation = args
            .invocation(json, instance.as_deref(), config.or(config_env.as_deref()), state.or(state_env.as_deref()))
            .map_err(|_| Box::new(InstallResult::invalid()))?;
        // 비밀 stdin을 읽기 전에 거부한다. thread/coordinator 도구는 보호 입력의 중계자가 될 수 없다.
        if invocation.secrets_stdin
            && (std::env::var_os("AAA_SESSION_ID").is_some() || std::env::var_os("AAA_HOOK_SOCK").is_some())
        {
            return Err(error("installation_secret_input_outside_required", 3));
        }
        if let Some(action) = args.action() {
            return review::run(args, action, &invocation, json);
        }
        aaa_config::install::validate_invocation(&invocation, std::io::stdin().is_terminal()).map_err(|e| {
            let mut r = InstallResult::invalid();
            r.error = Some(e);
            r
        })?;
        if args.draft().is_some() && args.automated(json) {
            return Err(error("draft_requires_interactive", 2));
        }
        if !args.automated(json)
            && invocation.status.is_none()
            && invocation.resume.is_none()
            && !invocation.authorization_stdin
            && invocation.authorization_file.is_none()
            && !invocation.secrets_stdin
        {
            return wizard::run(args, &invocation, &mut reporter);
        }
        if let Some(id) = &invocation.status {
            if invocation.events_file.is_some() {
                return Err(error("status_is_read_only", 2));
            }
            let state = state
                .or(state_env.as_deref())
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| aaa_config::paths::default_state_dir_for(instance.as_deref()));
            return engine::status(&state, instance.as_deref(), id);
        }
        if let Some(id) = &invocation.resume {
            engine::validate_id(id)?;
            let state = state
                .or(state_env.as_deref())
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| aaa_config::paths::default_state_dir_for(instance.as_deref()));
            let state = engine::normalize_state(&state)?;
            if !args.automated(json)
                && invocation.authorization_file.is_none()
                && !invocation.authorization_stdin
                && !invocation.secrets_stdin
                && engine::resume_requires_consent(&state, id, aaa_common::util::now_ms())?
            {
                return review::resume(args, &invocation, &mut reporter);
            }
            return engine::apply(&invocation, None, &state, instance.as_deref(), &mut reporter);
        }
        if invocation.dry_run && invocation.events_file.is_some() {
            return Err(error("dry_run_events_file_forbidden", 2));
        }
        let (result, plan) = detect::inspect(&invocation)?;
        if !invocation.dry_run {
            let state = std::path::PathBuf::from(&plan.context.state_dir);
            return engine::apply(&invocation, Some((result, plan)), &state, instance.as_deref(), &mut reporter);
        }
        Ok(result)
    })()
    .unwrap_or_else(|r| *r);
    reporter.finish(result, json)
}

fn authorization_required(result: Option<InstallResult>) -> InstallResult {
    let mut result = result.unwrap_or_else(|| *error("authorization_required", 3));
    result.needs_user.retain(|need| need.code != "authorization_required");
    result.error = None;
    result.status = InstallStatus::NeedsUser;
    result.exit_code = 3;
    result.needs_user.push(InstallNeedsUser {
        code: "installation_authorization_required".into(),
        target: "installation".into(),
        message: "검증된 설치 승인 경로가 필요합니다. --yes와 입력 파일은 권한이 아닙니다.".into(),
    });
    result
}

pub fn emit(result: InstallResult, json: bool, stream: bool) -> ExitCode {
    emit_at_seq(result, json, stream, 1)
}
fn emit_at_seq(result: InstallResult, json: bool, stream: bool, seq: u64) -> ExitCode {
    let code = result.exit_code;
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let written = if stream {
        let item = InstallStreamItem::Result {
            schema: InstallEventSchema::V1,
            installation_id: result.installation_id.clone(),
            seq,
            result: Box::new(result),
        };
        serde_json::to_writer(&mut out, &item).is_ok() && writeln!(out).is_ok()
    } else if json {
        serde_json::to_writer(&mut out, &result).is_ok() && writeln!(out).is_ok()
    } else {
        let text = match result.status {
            InstallStatus::Planned if result.changes.iter().any(|c| c.kind == "authorization") => {
                "승인서를 사용자 전용 파일로 발급했습니다. 설치는 아직 적용하지 않았습니다."
            }
            InstallStatus::Planned => "설치 계획을 검증했습니다. 파일 적용·서비스 실행은 하지 않았습니다.",
            InstallStatus::Cancelled => "설치를 중단했습니다.",
            InstallStatus::Completed => "설치를 완료했습니다.",
            InstallStatus::Configured => "설정을 적용했습니다.",
            InstallStatus::Unchanged => "요청한 내용이 이미 적용돼 있습니다.",
            InstallStatus::NeedsUser
                if result.needs_user.iter().any(|need| need.code == "installation_resume_executor_changed") =>
            {
                "원래 기록을 보존하고 새 repair 계획을 검토하세요."
            }
            InstallStatus::NeedsUser => "다음 작업을 마친 뒤 같은 ID로 재개하세요.",
            _ => result.error.as_ref().map_or("설치 계획을 확인하세요", |e| &e.message),
        };
        writeln!(out, "{text}").is_ok()
            && result.installation_id.as_ref().is_none_or(|id| writeln!(out, "installation_id: {id}").is_ok())
            && result.error.as_ref().is_none_or(|e| writeln!(out, "code: {}", e.code).is_ok())
            && result
                .needs_user
                .iter()
                .all(|need| writeln!(out, "{} ({}; {})", need.message, need.code, need.target).is_ok())
            && result.plan_sha256.as_ref().is_none_or(|hash| writeln!(out, "plan_sha256: {hash}").is_ok())
    };
    if !written || out.flush().is_err() { ExitCode::FAILURE } else { ExitCode::from(code) }
}

#[cfg(test)]
mod outcome_contract_tests {
    use super::*;
    #[test]
    fn public_failure_builder_respects_the_independent_v1_status_exit_contract() {
        for exit in [1, 2, 3, 4, 5, 6, 130] {
            let result = error("installation_fixture", exit);
            assert!(result.valid_exit(), "aaa.install.v1 status/exit 불일치: status={:?}, exit={exit}", result.status);
            if exit == 3 {
                assert_eq!(result.status, InstallStatus::NeedsUser);
            }
            if exit == 130 {
                assert_eq!(result.status, InstallStatus::Cancelled);
            }
        }
    }
}

//! 탐지·계획·상태 조회. 쓰기 CLI는 I6의 검증된 승인 executor까지 needs_user로 닫는다.
use std::io::{IsTerminal, Write};
use std::path::Path;
use std::process::ExitCode;

use aaa_proto::install::*;

mod args;
mod detect;
mod engine;
mod platform;
mod profiles;
mod services;
pub use args::Args;

pub(crate) fn service_dispatch(context: &Path, sha: &str) -> ExitCode {
    engine::service_dispatch(context, sha)
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

pub fn error(code: &str, exit: u8) -> Box<InstallResult> {
    let mut r = InstallResult::invalid();
    r.exit_code = exit;
    r.error = Some(InstallError {
        code: code.into(),
        message: "설치 계획의 입력과 현지 전제를 확인하세요".into(),
        details: vec![],
    });
    Box::new(r)
}

pub fn run(args: &Args, json: bool, instance: Option<&str>, config: Option<&Path>, state: Option<&Path>) -> ExitCode {
    let result = (|| {
        let instance = instance.map(str::to_owned).or_else(aaa_config::paths::instance);
        let config_env = std::env::var_os("AAA_CONFIG_DIR").filter(|v| !v.is_empty()).map(std::path::PathBuf::from);
        let state_env = std::env::var_os("AAA_STATE_DIR").filter(|v| !v.is_empty()).map(std::path::PathBuf::from);
        let invocation = args
            .invocation(json, instance.as_deref(), config.or(config_env.as_deref()), state.or(state_env.as_deref()))
            .map_err(|_| Box::new(InstallResult::invalid()))?;
        aaa_config::install::validate_invocation(&invocation, std::io::stdin().is_terminal()).map_err(|e| {
            let mut r = InstallResult::invalid();
            r.error = Some(e);
            r
        })?;
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
            return Ok(authorization_required(None));
        }
        if invocation.dry_run && invocation.events_file.is_some() {
            return Err(error("dry_run_events_file_forbidden", 2));
        }
        let r = detect::plan(&invocation)?;
        if !invocation.dry_run {
            return Ok(authorization_required(Some(r)));
        }
        Ok(r)
    })()
    .unwrap_or_else(|r| *r);
    emit(result, json, args.stream())
}

fn authorization_required(result: Option<InstallResult>) -> InstallResult {
    let mut result = result.unwrap_or_else(|| *error("authorization_required", 3));
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
    let code = result.exit_code;
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let written = if stream {
        let item = InstallStreamItem::Result {
            schema: InstallEventSchema::V1,
            installation_id: result.installation_id.clone(),
            seq: 1,
            result: Box::new(result),
        };
        serde_json::to_writer(&mut out, &item).is_ok() && writeln!(out).is_ok()
    } else if json {
        serde_json::to_writer(&mut out, &result).is_ok() && writeln!(out).is_ok()
    } else {
        let text = match result.status {
            InstallStatus::Planned => "설치 계획을 검증했습니다. 파일 적용·서비스 실행은 하지 않았습니다.",
            _ => result.error.as_ref().map_or("설치 계획을 확인하세요", |e| &e.message),
        };
        writeln!(out, "{text}").is_ok()
            && result.error.as_ref().is_none_or(|e| writeln!(out, "code: {}", e.code).is_ok())
            && result.plan_sha256.as_ref().is_none_or(|hash| writeln!(out, "plan_sha256: {hash}").is_ok())
    };
    if !written || out.flush().is_err() { ExitCode::FAILURE } else { ExitCode::from(code) }
}

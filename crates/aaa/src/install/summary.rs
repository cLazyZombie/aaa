//! 결과 요약은 관측 check를 사용한다. 요청·journal phase만으로 ready를 추정하지 않는다.
use super::*;
fn aggregate(result: &InstallResult, roles: &[&str], suffix: &str) -> InstallCheckState {
    let states: Vec<_> = roles
        .iter()
        .map(|role| {
            result
                .checks
                .iter()
                .rev()
                .find(|check| check.name == format!("service_{role}_{suffix}"))
                .map_or(InstallCheckState::Unknown, |check| check.state)
        })
        .collect();
    if states.iter().all(|s| *s == InstallCheckState::Passed) {
        InstallCheckState::Passed
    } else if states.contains(&InstallCheckState::Failed) {
        InstallCheckState::Failed
    } else if states.iter().all(|s| *s == InstallCheckState::NotRequested) {
        InstallCheckState::NotRequested
    } else {
        InstallCheckState::Unknown
    }
}
fn state(result: &InstallResult, check: &str, requested: bool) -> InstallOutcomeState {
    if !requested {
        return InstallOutcomeState::NotRequested;
    }
    match result.checks.iter().rev().find(|c| c.name == check).map(|c| c.state) {
        Some(InstallCheckState::Passed) => InstallOutcomeState::Completed,
        Some(InstallCheckState::Failed) => InstallOutcomeState::Failed,
        Some(InstallCheckState::NotRun) => InstallOutcomeState::Pending,
        Some(InstallCheckState::NotRequested) => InstallOutcomeState::NotRequested,
        _ => InstallOutcomeState::Unknown,
    }
}
pub(super) fn populate(plan: &InstallPlan, result: &mut InstallResult) {
    let manager = plan.choices.service.unwrap_or(InstallService::None);
    let disabled = manager == InstallService::None;
    let mut roles = Vec::new();
    if plan.choices.role == Some(InstallRole::Host) {
        roles.push("host");
    }
    if plan.choices.role == Some(InstallRole::Agent) || plan.choices.local_agent == Some(InstallToggle::Enabled) {
        roles.push("agent");
    }
    let registered = if disabled { InstallCheckState::NotRequested } else { aggregate(result, &roles, "registered") };
    result.service = Some(InstallServiceSummary {
        manager,
        account_mode: plan.choices.service_account,
        requested_user_matched: registered,
        registered,
        running: if disabled { InstallCheckState::NotRequested } else { aggregate(result, &roles, "running") },
        application_ready: if disabled { InstallCheckState::NotRequested } else { aggregate(result, &roles, "ready") },
        startup: plan.choices.startup,
        credential_retained_by: (manager == InstallService::WindowsScm && registered == InstallCheckState::Passed)
            .then_some(InstallCredentialRetainer::WindowsScm),
    });
    let requested = plan.choices.tailscale_serve == Some(InstallServe::On);
    let pairing = state(result, "first_controller_pairing", plan.choices.pairing == Some(InstallPairing::Controller));
    let actual = result.web.take();
    result.web = Some(InstallWebSummary {
        local_listen: (plan.choices.web != Some(InstallWeb::Off)).then(|| plan.choices.web_listen.clone()).flatten(),
        tailscale_serve: InstallServeSummary {
            requested,
            state: state(result, "tailscale_exact_mapping", requested),
            serve_port: requested.then_some(plan.choices.serve_port).flatten(),
            target_port: requested
                .then(|| {
                    plan.choices
                        .web_listen
                        .as_deref()
                        .and_then(|address| {
                            address
                                .parse::<std::net::SocketAddr>()
                                .inspect_err(|error| {
                                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                                })
                                .ok()
                        })
                        .map(|a| a.port())
                })
                .flatten(),
            reused: actual.as_ref().and_then(|s| s.tailscale_serve.reused),
            ownership: actual
                .map_or(if requested { InstallOwnership::Unknown } else { InstallOwnership::NotRequested }, |s| {
                    s.tailscale_serve.ownership
                }),
        },
        pairing: InstallPairingSummary { state: pairing },
    });
}

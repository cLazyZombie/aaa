//! 오래 산 session-host도 agent의 재연결 때 실제 책임 경로를 새로 관찰한다.
use aaa_proto::{Responsibility, ResponsibilityState, SessionInfo};
use aaa_session::files::SessionPaths;

pub(super) fn observe(paths: &SessionPaths, info: &SessionInfo) -> Option<Responsibility> {
    if !cfg!(target_os = "macos") || !info.state.is_running() {
        return None;
    }
    let observed = aaa_common::util::read_json::<aaa_session::files::MetaFile>(&paths.meta())
        .ok()
        .filter(|meta| meta.started_at_ms == info.started_at_ms && meta.child_pid == info.pid)
        .and_then(|meta| aaa_common::process::responsible(meta.host_pid))
        .and_then(|pid| aaa_common::process::image(pid).map(|path| (pid, path)));
    Some(classify(observed))
}

fn classify(observed: Option<(u32, std::path::PathBuf)>) -> Responsibility {
    let Some((pid, path)) = observed else {
        return Responsibility { state: ResponsibilityState::Unknown, pid: None, path: None };
    };
    Responsibility {
        state: if aaa_common::process::is_launcher(&path) {
            ResponsibilityState::Launcher
        } else {
            ResponsibilityState::Detached
        },
        pid: Some(pid),
        path: Some(path.to_string_lossy().into_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_path_distinguishes_launcher_detached_and_failed_lookup() {
        assert_eq!(
            classify(Some((23, "/opt/example/aaa-launcher.app/Contents/MacOS/aaa-launcher".into()))).state,
            ResponsibilityState::Launcher
        );
        let detached = classify(Some((1, "/sbin/launchd".into())));
        assert_eq!(detached.state, ResponsibilityState::Detached);
        assert_eq!(detached.path.as_deref(), Some("/sbin/launchd"));
        assert_eq!(classify(None).state, ResponsibilityState::Unknown);
        assert_eq!(classify(Some((24, "/opt/example/aaa-launcher".into()))).state, ResponsibilityState::Detached);
    }
}

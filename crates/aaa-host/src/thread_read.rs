//! thread_read.v1: agent 신원에 세션을 결속하고 Reader 권한으로만 호출한다.
use crate::{Host, err};
use aaa_proto::{DeviceRole, ErrorBody, ErrorCode, FrontRequest, FrontResponse};
use std::sync::Arc;

impl Host {
    pub(crate) async fn relay_thread_read(
        self: &Arc<Self>,
        machine: &str,
        session: &str,
        request: FrontRequest,
    ) -> Result<FrontResponse, ErrorBody> {
        if !request.thread_read_allowed() {
            return Err(err(ErrorCode::Forbidden, "원격 전달을 허용한 읽기 명령이 아니다"));
        }
        {
            let i = self.inner.lock().unwrap();
            if !i.panes.get(session).is_some_and(|p| p.machine_id == machine && p.info.state.is_running()) {
                return Err(err(ErrorCode::Forbidden, "실행 중인 자기 머신 세션의 읽기 요청만 허용한다"));
            }
        }
        // coordinator·일반 pane도 이 경로에서는 Reader다. actor는 자기 신고 대신 host 세션에서 만든다.
        self.call(DeviceRole::Reader, &format!("worker:{session}"), request).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aaa_proto::{Role, SessionInfo, SessionState};

    #[tokio::test]
    async fn relay_checks_machine_live_session_and_explicit_read_allowlist() {
        let root = std::env::temp_dir().join(format!("aaa-read-{}", aaa_common::util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = root.join("projects").display().to_string();
        cfg.notify.web_push = false;
        let h = Host::open(cfg, root.join("host")).unwrap();
        let mut info = SessionInfo {
            responsibility: None,
            id: "s1".into(),
            name: "reader".into(),
            argv: vec![],
            cwd: None,
            pid: None,
            started_at_ms: 1,
            boot_id: "boot".into(),
            cols: 80,
            rows: 24,
            state: SessionState::Running,
            role: Some(Role::Coordinator),
            label: None,
            harness: None,
        };
        h.on_session("m1", info.clone(), false);
        assert!(matches!(
            h.relay_thread_read("m1", "s1", FrontRequest::Projects { all: true }).await.unwrap(),
            FrontResponse::Projects { .. }
        ));
        for (machine, session) in [("m2", "s1"), ("m1", "missing")] {
            assert_eq!(
                h.relay_thread_read(machine, session, FrontRequest::Projects { all: true }).await.unwrap_err().code,
                ErrorCode::Forbidden
            );
        }
        // 조작 없는 요청도 명시적 목록 밖이면 닫는다. coordinator도 예외가 없다.
        for request in [
            FrontRequest::TickerTick,
            FrontRequest::EnrollOpen { minutes: 1 },
            FrontRequest::LibraryMarkRead { reports: vec![] },
            FrontRequest::Status,
        ] {
            assert_eq!(h.relay_thread_read("m1", "s1", request).await.unwrap_err().code, ErrorCode::Forbidden);
        }
        info.state = SessionState::Exited { code: Some(0) };
        h.on_session("m1", info, false);
        assert_eq!(
            h.relay_thread_read("m1", "s1", FrontRequest::Projects { all: true }).await.unwrap_err().code,
            ErrorCode::Forbidden
        );
        drop(h);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod frame_tests {
    use super::*;
    use aaa_proto::{B64, FrontMsg, MAX_FRAME, Outcome};

    #[test]
    fn relay_frame_preserves_file_bytes_and_returns_bounded_error() {
        let secret = aaa_test_support::token("read-frame-bytes");
        aaa_common::redact::remember(&secret);
        let raw = [b"\0\xff".as_slice(), secret.as_bytes()].concat();
        let encode = |raw: Vec<u8>| {
            crate::front::res_frame_within(
                String::new(),
                Ok(FrontResponse::LibraryChunk { data: B64(raw), next: None }),
                MAX_FRAME - 1024,
            )
        };
        let small: FrontMsg = serde_json::from_slice(&encode(raw.clone())).unwrap();
        assert!(
            matches!(small, FrontMsg::Res { result: Outcome::Ok(FrontResponse::LibraryChunk { data, .. }), .. } if data.0 == raw)
        );
        let big = encode(vec![0; MAX_FRAME]);
        assert!(big.len() < MAX_FRAME - 1024);
        let big: FrontMsg = serde_json::from_slice(&big).unwrap();
        assert!(matches!(big, FrontMsg::Res { result: Outcome::Err(ErrorBody { code: ErrorCode::TooLarge, .. }), .. }));
    }
}

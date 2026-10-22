use super::*;

/// 시작 때 자가 점검(§10.4 6): 계보·키체인·FDA·첫 실행 지연. 결과는 상태 폴더 selfcheck.json과 다음 hello에 담는다.
/// macOS에서는 App Nap·자동 종료를 막는 활동도 이때 잡는다.
pub fn start_selfcheck(agent: &Arc<Agent>, cfg: &aaa_config::SelfcheckSection) {
    #[cfg(target_os = "macos")]
    let held = aaa_doctor::macos::hold_activity("aaa agent backend: agent 세션 실행");
    #[cfg(not(target_os = "macos"))]
    let held = false;
    let opts = aaa_doctor::ProbeOptions {
        samples: cfg.first_exec_samples,
        keychain_service: Some(cfg.keychain_service.clone()).filter(|s| !s.is_empty()),
        volume_dir: Some(cfg.volume_dir.clone()).filter(|s| !s.is_empty()),
        label: Some("agent-selfcheck".into()),
        throughput: false,
        activity_held: held,
        fda_check: cfg!(target_os = "macos"),
    };
    let agent = agent.clone();
    if let Err(error) = spawn(agent, move || aaa_doctor::probe(&opts)) {
        tracing::warn!(%error, "자가 점검 thread 시작 실패");
    }
}

// TCC의 open은 취소할 수 없고 끝나는 시한도 없다. Tokio blocking pool에 넣으면
// agent runtime의 drop이 기다려 launcher 보존 exec·서비스 종료까지 멈춘다.
// 전용 thread는 join하지 않는다. 살아 있는 동안은 정상 결과를 파일과 hello 요약에
// 남기고, 프로세스 종료/exec 때 진행 중 검사는 OS가 정리한다. 직전 완료 파일은 유지한다.
fn spawn(
    agent: Arc<Agent>,
    probe: impl FnOnce() -> aaa_doctor::ProbeReport + Send + 'static,
) -> std::io::Result<std::thread::JoinHandle<()>> {
    std::thread::Builder::new().name("aaa-selfcheck".into()).spawn(move || {
        let r = probe();
        if let Err(error) = util::write_json(&agent.state.join("selfcheck.json"), &r) {
            tracing::warn!(%error, "자가 점검 결과 저장 실패");
        }
        let summary = serde_json::json!({
            "pass": r.pass,
            "reasons": r.reasons,
            "responsible_path": r.lineage.responsible_path,
            "first_exec": r.first_exec,
            "keychain_unlocked": r.keychain.default_unlocked,
            "fda": r.fda,
            "activity_held": r.activity_held,
            "dev_mode": agent.dev_mode,
        });
        if !r.pass {
            tracing::warn!("자가 점검 실패: {:?}", r.reasons);
        }
        *agent.selfcheck.lock().unwrap() = Some(summary);
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::sync::mpsc;

    /// FDA와 같이 파일 열기에서 막혀도 runtime 종료는 검사를 기다리지 않는다.
    /// 종료 뒤 같은 프로세스에서 검사가 끝나면 파일과 다음 hello 요약도 보존한다.
    #[test]
    fn blocked_open_does_not_hold_runtime_shutdown_and_keeps_completed_result() {
        let state = std::env::temp_dir().join(format!("aaa-selfcheck-{}", util::new_id()));
        let agent = Agent::new(state.clone(), "m".into(), "mac-a".into(), true, None).unwrap();
        let previous = aaa_doctor::ProbeReport { label: Some("previous".into()), ..Default::default() };
        util::write_json(&state.join("selfcheck.json"), &previous).unwrap();
        let fifo = state.join("blocked-open");
        let path = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
        let (entered_tx, entered_rx) = mpsc::channel();
        let (stopped_tx, stopped_rx) = mpsc::channel();
        let checking = agent.clone();
        let opened = fifo.clone();
        let runtime = std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            let task = rt.block_on(async {
                spawn(checking, move || {
                    entered_tx.send(()).unwrap();
                    // writer는 runtime 종료 관측 뒤에만 연다: open은 그때까지 끝날 수 없다.
                    let _file = std::fs::File::open(opened).unwrap();
                    aaa_doctor::ProbeReport {
                        pass: false,
                        reasons: vec!["blocked probe completed".into()],
                        fda: "denied".into(),
                        label: Some("completed".into()),
                        ..Default::default()
                    }
                })
                .unwrap()
            });
            drop(rt);
            let _ = stopped_tx.send(());
            task
        });
        entered_rx.recv_timeout(Duration::from_secs(30)).unwrap();
        let stopped = stopped_rx.recv_timeout(Duration::from_secs(3));
        let saved: aaa_doctor::ProbeReport = util::read_json(&state.join("selfcheck.json")).unwrap();
        assert_eq!(saved.label.as_deref(), Some("previous"));
        // 실패 때도 열린 파일을 풀고 자기 thread를 회수한 뒤 원래 실패를 보고한다.
        let writer = std::fs::OpenOptions::new().write(true).open(&fifo).unwrap();
        drop(writer);
        runtime.join().unwrap().join().unwrap();
        let report: aaa_doctor::ProbeReport = util::read_json(&state.join("selfcheck.json")).unwrap();
        assert_eq!(report.label.as_deref(), Some("completed"));
        assert_eq!(report.fda, "denied");
        let summary = agent.selfcheck.lock().unwrap().clone().unwrap();
        assert_eq!(summary["pass"], false);
        assert_eq!(summary["fda"], "denied");
        assert_eq!(summary["reasons"][0], "blocked probe completed");
        assert_eq!(summary["dev_mode"], true);
        std::fs::remove_dir_all(state).unwrap();
        stopped.expect("파일 열기 대기 때문에 runtime 종료가 막혔다");
    }
}

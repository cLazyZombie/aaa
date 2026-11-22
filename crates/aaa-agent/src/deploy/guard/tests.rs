use super::*;
#[tokio::test]
async fn blocked_service_operation_still_answers_status_and_observes_the_new_agent() {
    let temp = if cfg!(unix) { PathBuf::from("/tmp") } else { std::env::temp_dir() };
    let root = temp.join(format!("gr-{}", util::new_id()));
    std::fs::create_dir_all(root.join("deployments")).unwrap();
    write_identity(&root.join("deployments")).unwrap();
    let exe = root.join("aaa");
    std::fs::write(&exe, b"candidate").unwrap();
    let mut v = view(DeployService::Agent, false);
    v.spec.expected_sha256 = sha(&exe).unwrap();
    apply(&mut v, Control::Activate, util::now_ms()).unwrap();
    let id = v.spec.id.clone();
    save(&root, &v).unwrap();
    let listener = LocalListener::bind(&socket(&root, &id, "g")).unwrap();
    let client = Client::default();
    let status = async {
        tokio::join!(
            client.rpc(&root, &id, Control::Status),
            client.rpc(&root, &id, Control::Status),
            client.rpc(&root, &id, Control::Status)
        )
    };
    tokio::pin!(status);
    #[cfg(windows)]
    {
        use std::future::{Future, poll_fn};
        use std::task::Poll;
        // 첫 pipe는 연결됐지만 accept task가 아직 실행되지 않은 순서를 고정한다.
        // 기존 BUSY 창만 가상 시계로 넘긴다. RPC/서비스 작업의 상한은 그대로다.
        tokio::time::pause();
        poll_fn(|cx| {
            assert!(status.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        tokio::time::advance(Duration::from_millis(251)).await;
        poll_fn(|cx| {
            assert!(status.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        tokio::time::resume();
    }
    let mut controls = Controls::new(listener);
    let (release, pending) = tokio::sync::oneshot::channel();
    let server_root = root.clone();
    let server = tokio::spawn(async move {
        during(&mut controls, &server_root, &exe, &mut v, pending).await.unwrap().unwrap();
        restart_failed(&mut v, "늦게 도착한 SCM 오류".into(), util::now_ms());
        v
    });
    // 시간 경과가 아니라 세 개의 실제 RPC 응답을 받은 뒤에만 서비스 작업을 놓는다.
    tokio::time::timeout(Duration::from_secs(3), async {
        let (a, b, c) = status.await;
        for response in [a, b, c] {
            assert_eq!(response.unwrap().phase, Phase::AwaitingNew);
        }
        let ready = client
            .rpc(
                &root,
                &id,
                Control::Observe {
                    service: DeployService::Agent,
                    identity: RuntimeIdentity {
                        sha256: sha(&root.join("aaa")).unwrap(),
                        software: "aaa 0.1.2+new".into(),
                        instance: "new-agent".into(),
                        started_ms: util::now_ms(),
                        web: None,
                    },
                },
            )
            .await
            .unwrap();
        assert_eq!(ready.phase, Phase::NewReady);
        assert_eq!(client.rpc(&root, &id, Control::Commit).await.unwrap().phase, Phase::VerifiedNew);
        release.send(()).unwrap();
        assert_eq!(server.await.unwrap().phase, Phase::VerifiedNew);
    })
    .await
    .unwrap();
    std::fs::remove_dir_all(&root).unwrap();
}
#[test]
fn windows_restart_is_owned_by_the_guard_and_is_not_repeated_after_resume() {
    let mut v = view(DeployService::Agent, false);
    assert!(!start_windows_restart(&mut v, 1));
    apply(&mut v, Control::Activate, 2).unwrap();
    assert!(start_windows_restart(&mut v, 3));
    let mut resumed: DeployGuardView = serde_json::from_slice(&serde_json::to_vec(&v).unwrap()).unwrap();
    assert!(!start_windows_restart(&mut resumed, 4));
    begin_restore(&mut resumed, "SCM start 실패".into(), 5);
    let recovery_deadline = resumed.deadline_ms;
    restart_failed(&mut resumed, "늦은 중복 실패".into(), 6);
    assert_eq!(resumed.deadline_ms, recovery_deadline);
    assert!(!start_windows_restart(&mut resumed, 6));
    assert_eq!(resumed.phase, Phase::Restoring);
    resumed.phase = Phase::Restored;
    restart_failed(&mut resumed, "확인 뒤 늦은 SCM 실패".into(), 7);
    assert_eq!(resumed.phase, Phase::Restored);
    let mut host = view(DeployService::Host, true);
    apply(&mut host, Control::Activate, 2).unwrap();
    assert!(!start_windows_restart(&mut host, 3));
}
fn view(service: DeployService, host_machine: bool) -> DeployGuardView {
    DeployGuardView {
        spec: DeployGuardSpec {
            id: util::new_id(),
            rollout: util::new_id(),
            project: "demo".into(),
            service,
            host_machine,
            baseline: RuntimeIdentity {
                sha256: "a".repeat(64),
                software: "aaa 0.1.2+old".into(),
                instance: "old-agent".into(),
                started_ms: 0,
                web: None,
            },
            expected_sha256: "b".repeat(64),
            expected_software: "aaa 0.1.2+new".into(),
            old_host_instance: "old-host".into(),
            install_job: Some(util::new_id()),
            rollback_job: util::new_id(),
            restart_job: util::new_id(),
            host_restart_job: util::new_id(),
        },
        phase: Phase::Armed,
        worker_pid: 1,
        deadline_ms: WINDOW_MS,
        install_started: false,
        activated: false,
        restore_started: false,
        restart_index: 0,
        restart_started: false,
        host_restart_ms: 0,
        agent_restart_ms: 0,
        observed: vec![],
        installed_sha256: "a".repeat(64),
        error: None,
        pending_web: None,
    }
}
fn identity(sha: &str, software: &str, instance: &str) -> RuntimeIdentity {
    RuntimeIdentity {
        sha256: sha.into(),
        software: software.into(),
        instance: instance.into(),
        started_ms: 100,
        web: None,
    }
}
#[test]
fn guard_requires_a_new_matching_process_and_does_not_renew_failure_deadlines() {
    for service in [DeployService::Host, DeployService::Agent] {
        let mut v = view(service, service == DeployService::Host);
        let spec = v.spec.clone();
        assert!(apply(&mut v, Control::Commit, 0).is_err());
        apply(&mut v, Control::Activate, 10).unwrap();
        let deadline = v.deadline_ms;
        apply(&mut v, Control::Activate, 20).unwrap();
        assert_eq!(v.deadline_ms, deadline);
        let old = if service == DeployService::Host { "old-host" } else { "old-agent" };
        let candidate = identity(&spec.expected_sha256, &spec.expected_software, old);
        apply(&mut v, Control::Observe { service, identity: candidate }, 30).unwrap();
        assert_eq!(v.phase, Phase::AwaitingNew, "옛 프로세스를 새 판으로 확인하면 안 된다");
        v.installed_sha256 = spec.expected_sha256.clone();
        apply(
            &mut v,
            Control::Observe { service, identity: identity(&spec.expected_sha256, &spec.expected_software, "new") },
            40,
        )
        .unwrap();
        assert_eq!(v.phase, Phase::NewReady);
        assert_eq!(v.deadline_ms, deadline);
        apply(&mut v, Control::Commit, 50).unwrap();
        assert_eq!(v.phase, Phase::VerifiedNew);
        assert_eq!(v.spec, spec, "관찰/재개에서 작업 ID가 바뀌었다");
        for (sha, software) in
            [("c".repeat(64), spec.expected_software.clone()), (spec.expected_sha256.clone(), "aaa 0.1.2+wrong".into())]
        {
            let mut bad = view(service, service == DeployService::Host);
            apply(&mut bad, Control::Activate, 10).unwrap();
            apply(&mut bad, Control::Observe { service, identity: identity(&sha, &software, "new") }, 20).unwrap();
            assert_eq!(bad.phase, Phase::Restoring, "다른 SHA/판을 성공으로 넘겼다");
        }
        let mut timeout = view(service, service == DeployService::Host);
        apply(&mut timeout, Control::Activate, 10).unwrap();
        expired(&mut timeout, deadline - 1);
        assert_eq!(timeout.phase, Phase::AwaitingNew);
        expired(&mut timeout, deadline);
        assert_eq!(timeout.phase, Phase::Restoring);
        let recovery_deadline = timeout.deadline_ms;
        let recovery_spec = timeout.spec.clone();
        apply(&mut timeout, Control::Recover { reason: "같은 실패".into() }, deadline + 10).unwrap();
        assert_eq!(timeout.deadline_ms, recovery_deadline);
        assert_eq!(timeout.spec, recovery_spec, "복구/재개에서 새 작업 ID를 만들면 안 된다");
    }
}
#[test]
fn a_new_host_is_ready_only_after_its_web_answers_within_the_same_deadline() {
    let with_web = |sha: &str, software: &str, web: &str| RuntimeIdentity {
        web: Some(web.into()),
        ..identity(sha, software, "new")
    };
    let mut v = view(DeployService::Host, true);
    let spec = v.spec.clone();
    apply(&mut v, Control::Activate, 10).unwrap();
    let deadline = v.deadline_ms;
    v.installed_sha256 = spec.expected_sha256.clone();
    let new = with_web(&spec.expected_sha256, &spec.expected_software, "127.0.0.1:7431");
    apply(&mut v, Control::Observe { service: DeployService::Host, identity: new.clone() }, 20).unwrap();
    assert_eq!(v.phase, Phase::AwaitingNew, "실행 신원만으로 새 판 준비로 넘겼다");
    assert_eq!(v.pending_web.as_deref(), Some("127.0.0.1:7431"));
    assert!(apply(&mut v, Control::Commit, 30).is_err(), "웹 응답 전에 확정했다");
    // 다시 붙어 같은 신원을 알려도 기다림은 그대로다
    apply(&mut v, Control::Observe { service: DeployService::Host, identity: new.clone() }, 40).unwrap();
    assert_eq!(v.phase, Phase::AwaitingNew);
    web_answered(&mut v);
    assert_eq!((v.phase, v.pending_web.as_deref()), (Phase::NewReady, None));
    assert_eq!(v.deadline_ms, deadline, "웹 응답이 시한을 늘렸다");
    apply(&mut v, Control::Commit, 50).unwrap();
    assert_eq!(v.phase, Phase::VerifiedNew);

    // 웹이 답하지 않으면 같은 시한에 옛 판으로 되돌린다
    let mut silent = view(DeployService::Host, true);
    apply(&mut silent, Control::Activate, 10).unwrap();
    apply(&mut silent, Control::Observe { service: DeployService::Host, identity: new }, 20).unwrap();
    let limit = silent.deadline_ms;
    expired(&mut silent, limit - 1);
    assert_eq!(silent.phase, Phase::AwaitingNew);
    expired(&mut silent, limit);
    assert_eq!((silent.phase, silent.pending_web.as_deref()), (Phase::Restoring, None));
    web_answered(&mut silent);
    assert_eq!(silent.phase, Phase::Restoring, "복구 중에 늦은 웹 응답으로 준비가 됐다");

    // 웹을 끈 설정·옛 host(주소 없음)는 실행 신원만 본다. agent 감시는 웹을 보지 않는다
    for (service, web) in
        [(DeployService::Host, None), (DeployService::Host, Some("")), (DeployService::Agent, Some("127.0.0.1:7431"))]
    {
        let mut v = view(service, service == DeployService::Host);
        apply(&mut v, Control::Activate, 10).unwrap();
        let identity = RuntimeIdentity {
            web: web.map(str::to_owned),
            ..identity(&spec.expected_sha256, &spec.expected_software, "new")
        };
        apply(&mut v, Control::Observe { service, identity }, 20).unwrap();
        assert_eq!((v.phase, v.pending_web.as_deref()), (Phase::NewReady, None), "{service:?} {web:?}");
    }
}
#[test]
fn unchanged_installs_stop_but_changed_installs_restore_and_verify_all_local_roles() {
    let mut unchanged = view(DeployService::Agent, false);
    apply(&mut unchanged, Control::Cancel, 1).unwrap();
    assert_eq!(unchanged.phase, Phase::Cancelled);
    let mut expired_unchanged = view(DeployService::Host, true);
    expired(&mut expired_unchanged, WINDOW_MS);
    assert_eq!(expired_unchanged.phase, Phase::Cancelled);
    let mut pending = view(DeployService::Host, true);
    let job_id = pending.spec.install_job.clone().unwrap();
    apply(&mut pending, Control::BeginInstall { job_id }, 1).unwrap();
    assert!(pending.install_started);
    let mut pending_expiry = pending.clone();
    apply(&mut pending, Control::Cancel, 2).unwrap();
    assert_eq!(pending.phase, Phase::Restoring, "설치 중에는 현재 SHA가 옛것이어도 감시를 종료하지 않는다");
    expired(&mut pending_expiry, WINDOW_MS);
    assert_eq!(pending_expiry.phase, Phase::Restoring);
    // 재시작만 하는 감시(기대 판 = 배포 전 판, 설치 없음): 띄우기 전이면 그 사이 개별 update가 바꾼 설치 파일을 되돌리지 않는다
    let restart_only = || {
        let mut v = view(DeployService::Agent, false);
        v.spec.expected_sha256 = v.spec.baseline.sha256.clone();
        v.spec.install_job = None;
        v.installed_sha256 = "c".repeat(64);
        v
    };
    let mut cancel = restart_only();
    apply(&mut cancel, Control::Cancel, 1).unwrap();
    let mut expiry = restart_only();
    expired(&mut expiry, WINDOW_MS);
    assert_eq!((cancel.phase, expiry.phase), (Phase::Cancelled, Phase::Cancelled));
    let mut activated = restart_only();
    apply(&mut activated, Control::Activate, 1).unwrap();
    expired(&mut activated, 1 + WINDOW_MS);
    assert_eq!(activated.phase, Phase::Restoring, "띄운 뒤 새 프로세스를 확인하지 못하면 복구다");
    let cancelled_job = unchanged.spec.install_job.clone().unwrap();
    assert!(apply(&mut unchanged, Control::BeginInstall { job_id: cancelled_job }, 2).is_err());
    let mut changed = view(DeployService::Agent, true);
    changed.installed_sha256 = changed.spec.expected_sha256.clone();
    let spec = changed.spec.clone();
    apply(&mut changed, Control::Cancel, 1).unwrap();
    assert_eq!(changed.phase, Phase::Restoring);
    changed.phase = Phase::AwaitingOld;
    changed.deadline_ms = 2 * WINDOW_MS;
    changed.agent_restart_ms = 5;
    changed.host_restart_ms = 5;
    let mut earlier = identity(&spec.baseline.sha256, &spec.baseline.software, "failed-before-restore");
    earlier.started_ms = 4;
    apply(&mut changed, Control::Observe { service: DeployService::Agent, identity: earlier }, 2).unwrap();
    assert!(changed.observed.is_empty(), "복구 재시작 전에 이미 뜬 프로세스를 복구 완료로 보았다");
    apply(
        &mut changed,
        Control::Observe {
            service: DeployService::Agent,
            identity: identity(&spec.baseline.sha256, &spec.baseline.software, "old-agent"),
        },
        2,
    )
    .unwrap();
    assert!(changed.observed.is_empty());
    apply(
        &mut changed,
        Control::Observe {
            service: DeployService::Agent,
            identity: identity(&spec.baseline.sha256, &spec.baseline.software, "restored-agent"),
        },
        3,
    )
    .unwrap();
    assert_eq!(changed.phase, Phase::AwaitingOld, "같은 파일을 쓰는 host 확인도 필요하다");
    apply(
        &mut changed,
        Control::Observe {
            service: DeployService::Host,
            identity: identity(&spec.expected_sha256, &spec.baseline.software, "restored-host"),
        },
        4,
    )
    .unwrap();
    assert_eq!(changed.phase, Phase::AwaitingOld, "옛 host의 실제 SHA를 대조한다");
    apply(
        &mut changed,
        Control::Observe {
            service: DeployService::Host,
            identity: identity(&spec.baseline.sha256, &spec.baseline.software, "restored-host"),
        },
        5,
    )
    .unwrap();
    assert_eq!(changed.phase, Phase::Restored);
    assert_eq!(changed.spec, spec);
    let mut failed = view(DeployService::Agent, false);
    failed.phase = Phase::AwaitingOld;
    expired(&mut failed, WINDOW_MS);
    assert_eq!(failed.phase, Phase::Failed, "재연결 없는 복구를 성공으로 표시하면 안 된다");
}

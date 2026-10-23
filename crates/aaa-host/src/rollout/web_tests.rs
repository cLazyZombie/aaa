//! 웹 「배포」의 권한·기록·요약(관리 GUI W6a, plan §8.361)과 재시작만 하는 rollout의 시작(W6c, plan §8.402). rollout 진행
//! 자체는 m13 `rollout`이 실제 host·agent로 본다.
use super::*;
use aaa_proto::{AgentHello, DeviceRole, FrontRequest};

const WEB: &str = "device:phone";

#[test]
fn mixed_platform_candidates_are_pinned_before_transfer_and_guard_uses_target_sha() {
    in_lab(|h| async move {
        {
            let mut inner = h.inner.lock().unwrap();
            let local = inner.machines.get_mut("m").unwrap();
            local.os = Some("macos".into());
            local.build.arch = Some("aarch64".into());
        }
        let (writer, _reader) = tokio::io::duplex(65536);
        h.agent_connected(
            &AgentHello {
                proto: AGENT_LINK,
                machine_id: "win".into(),
                machine_name: "win-a".into(),
                boot_id: "boot".into(),
                os: "windows".into(),
                arch: "x86_64".into(),
                software: "test".into(),
                features: vec![],
                dev_mode: true,
                sha256: None,
                selfcheck: None,
                sessions: vec![],
                identity: None,
                harness: vec![],
            },
            false,
            crate::AgentConn::new(aaa_common::conn::spawn_writer(writer, MAX_FRAME), 2),
        );
        let mut r = super::tests::record();
        r.spec.machines = vec!["m".into(), "win".into()];
        r.spec.source_machine = Some("m".into());
        assert!(h.pin_artifacts(&r.spec).unwrap_err().message.contains("windows/x86_64"));
        let artifact = RolloutArtifact {
            os: "windows".into(),
            arch: "x86_64".into(),
            source_machine: "m".into(),
            from: "/candidate/windows/aaa.exe".into(),
            sha256: "b".repeat(64),
        };
        r.spec.artifacts = vec![artifact.clone()];
        r.steps = h.pin_artifacts(&r.spec).unwrap();
        assert!(r.steps.iter().filter(|s| s.machine == "m").all(|s| s.artifact.is_none()));
        assert!(r.steps.iter().filter(|s| s.machine == "win").all(|s| s.artifact.as_ref() == Some(&artifact)));
        let baseline = RuntimeIdentity {
            sha256: "c".repeat(64),
            software: "aaa 0.1.5".into(),
            instance: "before".into(),
            started_ms: 1,
            web: None,
        };
        r.steps.iter_mut().find(|s| s.machine == "win" && s.phase == RolloutPhase::Preflight).unwrap().baseline =
            Some(baseline);
        let cursor = r.steps.iter().position(|s| s.machine == "win" && s.phase == RolloutPhase::AgentUpdate).unwrap();
        let restart = h.set_guard_spec(&mut r, cursor).unwrap().unwrap();
        assert_eq!(r.steps[restart].guard.as_ref().unwrap().expected_sha256, artifact.sha256);
        // 내구 기록을 읽은 뒤 hello의 플랫폼이 달라져도 다시 고르지 않는다.
        let restored: RolloutView = serde_json::from_slice(&serde_json::to_vec(&r).unwrap()).unwrap();
        h.inner.lock().unwrap().machines.get_mut("win").unwrap().os = Some("macos".into());
        assert_eq!(machine_spec(&restored, "win").sha256, artifact.sha256);
        assert_eq!(machine_spec(&restored, "m").sha256, r.spec.sha256);
        let mut stage = DeployStageView {
            rollout: r.id.clone(),
            project: r.project.clone(),
            sha256: artifact.sha256,
            total: 1,
            offset: 1,
            complete: true,
            version: r.version.clone(),
        };
        assert!(stage_receipt(&restored, &machine_spec(&restored, "win").sha256, &stage).is_ok());
        stage.sha256 = r.spec.sha256;
        assert!(stage_receipt(&restored, &machine_spec(&restored, "win").sha256, &stage).is_err());
        assert!(candidate_path(r"C:\candidate\aaa.exe", true));
        assert!(!candidate_path(r"C:relative\aaa.exe", true));
    });
}
/// 시험 몸을 따로 만든 runtime에서 돌리고, runtime을 끝낸 뒤 상태 폴더를 지운다. `agent_connected`가 띄운 task(`rewatch` 등)는
/// 이 시험에서 한 번도 돌지 않은 채 Host를 쥐고 있어, runtime을 끝내야 Host와 host DB가 닫힌다. Windows는 열린 DB가 든 폴더를
/// 지우지 못한다(os error 32, `deploy` 시험과 같다). Host가 정말 닫혔는지는 모든 OS에서 본다
fn in_lab<F: std::future::Future<Output = ()>>(body: impl FnOnce(Arc<Host>) -> F) {
    in_lab_with(None, body)
}
/// 붙어 있는 가짜 agent: hello에 알린 실행 신원, 감시를 세우라는 요청에 「설치 파일이 지금 실행 판과 다르다」로 답하는가,
/// 첫 세우기의 답이 끊기는가(host에는 시한), 세우라고 받은 감시 ID, 취소한 감시 ID, 감시 세우기·취소 밖에 받은 요청
#[derive(Clone, Default)]
struct LiveAgent {
    identity: Option<aaa_proto::RuntimeIdentity>,
    installed_changed: bool,
    first_lost: bool,
    cancelled: Arc<std::sync::Mutex<Vec<String>>>,
    others: Arc<std::sync::Mutex<Vec<String>>>,
    armed: Arc<std::sync::Mutex<Vec<String>>>,
}
fn live(identity: Option<aaa_proto::RuntimeIdentity>, installed_changed: bool) -> LiveAgent {
    LiveAgent { identity, installed_changed, ..Default::default() }
}
/// `agent`가 있으면 그 agent가 붙어 있는 실험실(재시작 rollout 시작). 없으면 기능을 알린 머신이 끊긴 뒤
fn in_lab_with<F: std::future::Future<Output = ()>>(agent: Option<LiveAgent>, body: impl FnOnce(Arc<Host>) -> F) {
    let root = std::env::temp_dir().join(format!("aaa-rollout-web-{}", aaa_common::util::new_id()));
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let host = rt.block_on(async {
        let h = lab(&root, agent);
        let host = Arc::downgrade(&h);
        body(h).await;
        host
    });
    drop(rt);
    assert!(host.upgrade().is_none(), "runtime을 끝냈는데 Host가 남아 있다(DB가 열린 채다)");
    std::fs::remove_dir_all(root).unwrap();
}
fn lab(root: &std::path::Path, agent: Option<LiveAgent>) -> Arc<Host> {
    let mut cfg = aaa_config::HostConfig::default();
    cfg.projects.root = root.join("projects").to_string_lossy().into();
    cfg.notify.web_push = false;
    let h = Host::open(cfg, root.join("host")).unwrap();
    h.project_create("demo", Default::default(), "test").unwrap();
    // 기능을 알린 머신이 끊긴 뒤: 요청은 agent에 닿기 전에 끝난다
    let (writer, reader) = tokio::io::duplex(65536);
    let conn = crate::AgentConn::new(aaa_common::conn::spawn_writer(writer, aaa_proto::MAX_FRAME), 1);
    let features = ["deployment.v1", "rollout.v1", "deploy_stage.v1", "rollout_recovery.v1", "deploy_candidate.v1"];
    h.agent_connected(
        &AgentHello {
            proto: aaa_proto::AGENT_LINK,
            machine_id: "m".into(),
            machine_name: "mac-a".into(),
            boot_id: "boot".into(),
            os: "test".into(),
            arch: "test".into(),
            software: "test".into(),
            features: features.iter().map(|f| f.to_string()).collect(),
            dev_mode: true,
            sha256: None,
            selfcheck: None,
            sessions: vec![],
            identity: agent.as_ref().and_then(|a| a.identity.clone()),
            harness: vec![],
        },
        true,
        conn.clone(),
    );
    match agent {
        None => h.agent_disconnected("m", 1),
        Some(agent) => {
            tokio::spawn(answer_guards(conn, reader, agent));
        }
    }
    h
}
/// 감시 세우기(세운 감시 또는 `conflict`)와 취소에만 답한다. 나머지 요청은 답하지 않는다
async fn answer_guards(conn: crate::AgentConn, reader: tokio::io::DuplexStream, agent: LiveAgent) {
    use aaa_proto::{DeployGuardControl, DeployGuardPhase, DeployGuardView, LinkMsg, Outcome};
    let view = |spec, phase| DeployGuardView {
        spec,
        phase,
        worker_pid: 1,
        deadline_ms: 1,
        install_started: false,
        activated: false,
        restore_started: false,
        restart_index: 0,
        restart_started: false,
        host_restart_ms: 0,
        agent_restart_ms: 0,
        observed: vec![],
        installed_sha256: String::new(),
        error: None,
        pending_web: None,
    };
    let mut specs = std::collections::HashMap::new();
    let mut rx = aaa_common::conn::FrameRx::new(reader, aaa_proto::MAX_FRAME);
    while let Ok(msg) = rx.control::<LinkMsg>().await {
        let LinkMsg::Req { id, body, .. } = msg else { continue };
        let spec = match body {
            AgentRequest::DeployGuardArm { spec } => spec,
            AgentRequest::DeployGuardControl { id: guard, control: DeployGuardControl::Cancel } => {
                let Some(spec) = specs.get(&guard).cloned() else { continue };
                agent.cancelled.lock().unwrap().push(guard);
                let cancelled = view(spec, DeployGuardPhase::Cancelled);
                if let Some(waiting) = conn.pending.lock().unwrap().remove(&id) {
                    let _ = waiting.send(Outcome::Ok(AgentResponse::DeployGuard { guard: Box::new(cancelled) }));
                }
                continue;
            }
            other => {
                agent.others.lock().unwrap().push(format!("{other:?}"));
                continue;
            }
        };
        // 답이 끊긴 세우기도 agent에는 감시가 선다
        specs.insert(spec.id.clone(), spec.clone());
        let first = agent.armed.lock().unwrap().is_empty();
        agent.armed.lock().unwrap().push(spec.id.clone());
        let answer = if first && agent.first_lost {
            (id, Outcome::Err(ErrorBody::new(ErrorCode::Timeout, "답이 끊겼다")))
        } else if agent.installed_changed {
            (id, Outcome::Err(ErrorBody::new(ErrorCode::Conflict, "설치 파일이 지금 실행 판과 다르다")))
        } else {
            (id, Outcome::Ok(AgentResponse::DeployGuard { guard: Box::new(view(spec, DeployGuardPhase::Armed)) }))
        };
        if let Some(waiting) = conn.pending.lock().unwrap().remove(&answer.0) {
            let _ = waiting.send(answer.1);
        }
    }
}
fn restart() -> DeployAction {
    DeployAction::Restart { service: DeployService::Agent }
}

#[test]
fn completed_rollout_allows_the_next_record_without_reusing_jobs_or_losing_history() {
    for state in [RolloutState::Done, RolloutState::Stopped, RolloutState::StoppedRolledBack] {
        in_lab(|h| async move {
            let mut done = super::tests::record();
            done.state = state;
            done.cursor = done.steps.len() as u32;
            h.db.start_rollout(&done.id, &serde_json::to_string(&done).unwrap()).unwrap();
            assert!(!h.rollout_busy().unwrap());
            let next = super::tests::record();
            h.db.start_rollout(&next.id, &serde_json::to_string(&next).unwrap()).unwrap();
            assert!(h.rollout_busy().unwrap());
            assert_eq!(h.load_rollout(&done.id).unwrap(), done);
            assert_eq!(h.load_rollout(&next.id).unwrap(), next);
            assert!(next.steps.iter().all(|step| done.steps.iter().all(|old| old.job_id != step.job_id)));
        });
    }
}

#[test]
fn web_controllers_restart_only_and_session_agents_cannot_plan() {
    in_lab(web_controllers_restart_only_and_session_agents_cannot_plan_body);
}
async fn web_controllers_restart_only_and_session_agents_cannot_plan_body(h: Arc<Host>) {
    let start = |action| FrontRequest::DeployStart { machine: Some("mac-a".into()), id: new_id(), action };
    // 웹은 개별 배포를 하지 않는다: 교체는 rollout으로, 재시작은 재시작 rollout으로만(W6c)
    let update = DeployAction::CheckedUpdate { from: "/tmp/aaa".into(), sha256: "a".repeat(64), dry_run: false };
    for action in [update, restart()] {
        let e = h.call(DeviceRole::Controller, WEB, start(action)).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::Forbidden, "{}", e.message);
    }
    // 재시작 rollout은 권한을 통과해 대상 머신 연결에서 멈춘다(여기서는 끊겨 있다). 아무것도 기록하지 않는다
    let restart_rollout =
        |service| FrontRequest::RolloutStart { id: new_id(), spec: restart_spec(&["mac-a"], service) };
    assert_eq!(
        h.call(DeviceRole::Controller, WEB, restart_rollout(DeployService::Agent)).await.unwrap_err().code,
        ErrorCode::AgentDisconnected
    );
    assert!(!h.rollout_busy().unwrap());
    // 보기 전용 기기·세션 안 연결은 재시작도 점검도 못 한다
    assert_eq!(h.call(DeviceRole::Reader, "device:tv", start(restart())).await.unwrap_err().code, ErrorCode::Forbidden);
    assert_eq!(
        h.call(DeviceRole::Reader, "device:tv", restart_rollout(DeployService::Agent)).await.unwrap_err().code,
        ErrorCode::Forbidden
    );
    let plan =
        |path: &str| FrontRequest::RolloutPlan { source_machine: "mac-a".into(), path: path.into(), machines: vec![] };
    for (role, actor) in [
        (DeviceRole::Local, "local:cli@coordinator"),
        (DeviceRole::Local, "worker:t-1"),
        (DeviceRole::Reader, "device:tv"),
    ] {
        assert_eq!(
            h.call(role, actor, plan("/opt/aaa/candidate/aaa")).await.unwrap_err().code,
            ErrorCode::Forbidden,
            "{actor}"
        );
    }
    // 조작 기기는 점검 권한을 통과한다: 경로 형식, 그다음 빌드 머신 연결에서 멈춘다
    assert_eq!(
        h.call(DeviceRole::Controller, WEB, plan("candidate/aaa")).await.unwrap_err().code,
        ErrorCode::BadRequest
    );
    assert_eq!(
        h.call(DeviceRole::Controller, WEB, plan("~/aaa-candidate/aaa")).await.unwrap_err().code,
        ErrorCode::AgentDisconnected
    );
    // 웹 rollout은 빌드 머신 후보만(미리 복사한 경로 L1은 받지 않는다)
    let spec = RolloutSpec {
        from: "/opt/aaa".into(),
        source_machine: None,
        sha256: "a".repeat(64),
        commit: None,
        machines: vec![],
        artifacts: vec![],
        restart: None,
    };
    let e = h.call(DeviceRole::Controller, WEB, FrontRequest::RolloutStart { id: new_id(), spec }).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::BadRequest);
}

fn new_id() -> String {
    aaa_common::util::new_id()
}
fn restart_spec(machines: &[&str], service: DeployService) -> RolloutSpec {
    RolloutSpec {
        from: String::new(),
        source_machine: None,
        sha256: String::new(),
        commit: None,
        machines: machines.iter().map(|m| m.to_string()).collect(),
        artifacts: vec![],
        restart: Some(service),
    }
}

#[test]
fn restart_rollouts_pin_the_identity_before_restart_on_one_machine() {
    let agent = aaa_proto::RuntimeIdentity {
        sha256: "b".repeat(64),
        software: "aaa 0.1.2".into(),
        instance: "agent-before".into(),
        started_ms: 5,
        web: None,
    };
    let changed = agent.clone();
    in_lab_with(Some(live(Some(agent.clone()), false)), |h| async move {
        // 개별 재시작은 감시 없이 돌지 않는다: coordinator도 재시작 rollout으로 보낸다
        let direct = FrontRequest::DeployStart { machine: None, id: new_id(), action: restart() };
        let e = h.call(DeviceRole::Local, "local:cli@coordinator", direct).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::BadRequest, "{}", e.message);
        let two =
            FrontRequest::RolloutStart { id: new_id(), spec: restart_spec(&["mac-a", "mac-a"], DeployService::Agent) };
        assert_eq!(h.call(DeviceRole::Controller, WEB, two).await.unwrap_err().code, ErrorCode::BadRequest);

        let id = new_id();
        let start = |machines: &[&str]| FrontRequest::RolloutStart {
            id: id.clone(),
            spec: restart_spec(machines, DeployService::Agent),
        };
        let FrontResponse::RolloutCards { cards } =
            h.call(DeviceRole::Controller, WEB, start(&["mac-a"])).await.unwrap()
        else {
            panic!("웹은 표시용 요약을 받는다")
        };
        assert_eq!((cards[0].restart, cards[0].steps.len()), (Some(DeployService::Agent), 1));
        // 감시의 기준: 재시작 전 agent 실행 신원과 그 판 그대로
        let r = h.load_rollout(&id).unwrap();
        assert_eq!((r.spec.sha256.as_str(), r.version.as_deref()), (agent.sha256.as_str(), Some("0.1.2")));
        assert_eq!((r.steps[0].phase, r.steps[0].baseline.as_ref()), (RolloutPhase::AgentRestart, Some(&agent)));
        assert!(r.recovery_enabled && r.spec.commit.is_none(), "재시작 rollout은 자동화의 배포 기준 커밋이 아니다");
        // 같은 ID는 같은 기록(대상을 비우면 host 머신: 여기서는 mac-a 그 자체), 다른 서비스에는 못 쓴다
        for machines in [&["mac-a"][..], &[]] {
            let again = h.call(DeviceRole::Controller, WEB, start(machines)).await.unwrap();
            assert!(matches!(again, FrontResponse::RolloutCards { cards } if cards[0].id == id));
        }
        let host = FrontRequest::RolloutStart { id: id.clone(), spec: restart_spec(&[], DeployService::Host) };
        assert_eq!(h.call(DeviceRole::Controller, WEB, host).await.unwrap_err().code, ErrorCode::BadRequest);
        // 진행 중이면 새 재시작을 받지 않는다
        let other = FrontRequest::RolloutStart { id: new_id(), spec: restart_spec(&[], DeployService::Host) };
        let e = h.call(DeviceRole::Controller, WEB, other).await.unwrap_err();
        assert!(e.message.contains("진행 중"), "{}", e.message);
    });
    // 실행 신원을 알리지 않는 옛 agent는 재시작 rollout의 대상이 아니다. host 재시작은 host 자신의 신원으로 세운다
    in_lab_with(Some(live(None, false)), |h| async move {
        let agent = FrontRequest::RolloutStart { id: new_id(), spec: restart_spec(&["mac-a"], DeployService::Agent) };
        assert_eq!(h.call(DeviceRole::Controller, WEB, agent).await.unwrap_err().code, ErrorCode::UnsupportedVersion);
        let id = new_id();
        let host = FrontRequest::RolloutStart { id: id.clone(), spec: restart_spec(&[], DeployService::Host) };
        h.call(DeviceRole::Controller, WEB, host).await.unwrap();
        let r = h.load_rollout(&id).unwrap();
        let baseline = r.steps[0].baseline.clone().unwrap();
        assert_eq!(
            (r.steps[0].phase, r.spec.machines.as_slice()),
            (RolloutPhase::HostRestart, ["m".to_string()].as_slice())
        );
        assert_eq!((baseline.sha256, baseline.instance), (h.running_sha256.clone(), h.rollout_instance.clone()));
        assert!(r.steps[0].guard_ready, "감시를 세운 뒤에 기록한다");
    });
    // 설치 파일이 지금 실행 판과 다르면(개별 교체 뒤) 순수 재시작이 아니다: 재시작하지 않고 중단으로 남기며 웹은 `conflict`다
    // (coordinator는 같은 ID의 감시 밖 개별 재시작으로 간다: m13 `coordinator_deployment…`)
    in_lab_with(Some(live(Some(changed.clone()), true)), |h| async move {
        let id = new_id();
        let start = FrontRequest::RolloutStart { id: id.clone(), spec: restart_spec(&["mac-a"], DeployService::Agent) };
        let e = h.call(DeviceRole::Controller, WEB, start).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::Conflict, "{}", e.message);
        let r = h.load_rollout(&id).unwrap();
        assert_eq!((r.state, r.steps[0].guard_ready), (RolloutState::Stopped, false));
        assert!(r.error.unwrap().contains("재시작하지 않았다") && !h.rollout_busy().unwrap());
    });
    // 감시 세우기의 답을 못 받으면 기록을 둔 채 진행 루프가 같은 정본으로 다시 세운다. 그때 설치 파일이 바뀌었다는 답이면
    // 재시작하지 않고 중단이다(감시 없이 재시작하거나 없는 감시의 복구로 가지 않는다)
    let agent = LiveAgent { first_lost: true, ..live(Some(changed.clone()), true) };
    let (armed, cancelled, others) = (agent.armed.clone(), agent.cancelled.clone(), agent.others.clone());
    in_lab_with(Some(agent), |h| async move {
        let id = new_id();
        let mut changes = h.rollout_changed.subscribe();
        let start = FrontRequest::RolloutStart { id: id.clone(), spec: restart_spec(&["mac-a"], DeployService::Agent) };
        assert!(matches!(
            h.call(DeviceRole::Controller, WEB, start).await.unwrap(),
            FrontResponse::RolloutCards { .. }
        ));
        let r = loop {
            let r = h.load_rollout(&id).unwrap();
            if r.state != RolloutState::Running {
                break r;
            }
            changes.changed().await.unwrap();
        };
        assert_eq!((r.state, r.steps[0].guard_ready), (RolloutState::Stopped, false), "{:?}", r.error);
        let guard = r.steps[0].guard.clone().unwrap().id;
        assert_eq!(*armed.lock().unwrap(), [guard.clone(), guard.clone()]);
        assert_eq!(*cancelled.lock().unwrap(), [guard], "답을 잃은 세우기로 선 감시를 취소한다");
        assert!(others.lock().unwrap().is_empty(), "재시작을 보냈다: {:?}", others.lock().unwrap());
    });
    // 같은 ID가 동시에 와도 기록과 감시는 하나다: 뒤 요청은 감시를 세우지 않고 먼저 남긴 기록을 돌려준다(기록 없이 남는 감시가
    // 개별 교체를 시한에 되돌리지 않게)
    let agent = live(Some(changed), false);
    let armed = agent.armed.clone();
    in_lab_with(Some(agent), |h| async move {
        let id = new_id();
        let start =
            || FrontRequest::RolloutStart { id: id.clone(), spec: restart_spec(&["mac-a"], DeployService::Agent) };
        let (a, b) =
            tokio::join!(h.call(DeviceRole::Controller, WEB, start()), h.call(DeviceRole::Controller, WEB, start()));
        let cards = |r: Result<FrontResponse, ErrorBody>| match r.unwrap() {
            FrontResponse::RolloutCards { cards } => cards,
            other => panic!("{other:?}"),
        };
        assert_eq!(cards(a)[0].id, cards(b)[0].id);
        let kept = h.load_rollout(&id).unwrap().steps[0].guard.clone().unwrap().id;
        assert_eq!(*armed.lock().unwrap(), [kept]);
    });
}

#[test]
fn web_approval_lasts_the_rollout_and_web_reads_only_display_cards() {
    in_lab(web_approval_lasts_the_rollout_and_web_reads_only_display_cards_body);
}
async fn web_approval_lasts_the_rollout_and_web_reads_only_display_cards_body(h: Arc<Host>) {
    // coordinator_deploy가 꺼져 있어도 사람·웹 기기의 승인은 그 rollout 끝까지 남는다. coordinator 시작은 매번 safety를 본다
    let mut r = super::tests::record();
    for (actor, permitted) in [(WEB, true), ("person:42", true), ("local:test@coordinator", false), ("device:", false)]
    {
        r.actor = actor.into();
        assert_eq!(h.rollout_permitted(&r), permitted, "{actor}");
    }
    // 기록 두 개와 다른 프로젝트 하나: 웹은 이 프로젝트의 요약만 새것부터 받는다
    let mut old = super::tests::record();
    old.actor = "private-actor-marker".into();
    old.spec.from = "/private/candidate-marker/aaa".into();
    old.error = Some("staging /private/install-marker 실패".into());
    old.created_ms = 1;
    let mut new = super::tests::record();
    new.created_ms = 2;
    let mut other = super::tests::record();
    other.project = "elsewhere".into();
    for r in [&old, &new, &other] {
        h.db.set_meta(&format!("rollout:{}", r.id), &serde_json::to_string(r).unwrap()).unwrap();
    }
    let FrontResponse::RolloutCards { cards } =
        h.call(DeviceRole::Reader, "device:tv", FrontRequest::Rollouts).await.unwrap()
    else {
        panic!("요약 목록이 아니다")
    };
    assert_eq!(cards.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), vec![new.id.as_str(), old.id.as_str()]);
    // 같은 ID를 다시 열면 조작 기기에도 요약만 간다(coordinator·사람 터미널은 정본 그대로)
    let status = || FrontRequest::RolloutStatus { id: old.id.clone() };
    let FrontResponse::RolloutCards { cards } = h.call(DeviceRole::Controller, WEB, status()).await.unwrap() else {
        panic!("요약이 아니다")
    };
    let wire = serde_json::to_string(&cards).unwrap();
    for private in ["private-actor-marker", "candidate-marker", "install-marker", old.steps[0].job_id.as_str()] {
        assert!(!wire.contains(private), "웹에 내부 자료가 나갔다: {private}");
    }
    assert_eq!(cards[0].error.as_deref().map(|e| e.contains("coordinator의 배포 기록")), Some(true));
    assert_eq!(h.call(DeviceRole::Reader, "device:tv", status()).await.unwrap_err().code, ErrorCode::Forbidden);
    let theirs = FrontRequest::RolloutStatus { id: other.id.clone() };
    assert_eq!(h.call(DeviceRole::Controller, WEB, theirs).await.unwrap_err().code, ErrorCode::Forbidden);
    // 진행 중인 rollout이 있으면 점검하지 않는다(빌드 머신의 dry-run이 그 단계와 겹친다)
    h.db.set_meta("rollout:active", &new.id).unwrap();
    let plan = FrontRequest::RolloutPlan { source_machine: "mac-a".into(), path: "~/c/aaa".into(), machines: vec![] };
    let e = h.call(DeviceRole::Controller, WEB, plan).await.unwrap_err();
    assert!(e.code == ErrorCode::BadRequest && e.message.contains("진행 중"), "{}", e.message);
    // 응답을 잃고 같은 ID로 다시 보내면, 빌드 머신 agent가 잠시 끊겨 있어도 기존 rollout 요약을 돌려준다
    let mut sent = super::tests::record();
    sent.actor = WEB.into();
    sent.spec.from = "~/c/aaa".into();
    sent.spec.source_machine = Some("m".into());
    sent.spec.machines = vec!["m".into()];
    h.db.set_meta(&format!("rollout:{}", sent.id), &serde_json::to_string(&sent).unwrap()).unwrap();
    let again = RolloutSpec { source_machine: Some("mac-a".into()), machines: vec![], ..sent.spec.clone() };
    let resent =
        h.call(DeviceRole::Controller, WEB, FrontRequest::RolloutStart { id: sent.id.clone(), spec: again }).await;
    assert!(matches!(&resent, Ok(FrontResponse::RolloutCards { cards }) if cards[0].id == sent.id), "{resent:?}");
}

#[test]
fn web_job_list_is_newest_first_and_hides_executor_details() {
    in_lab(web_job_list_is_newest_first_and_hides_executor_details_body);
}
async fn web_job_list_is_newest_first_and_hides_executor_details_body(h: Arc<Host>) {
    let action = serde_json::to_value(restart()).unwrap();
    let put = |id: &str, v: serde_json::Value| h.db.set_meta(&format!("deploy:{id}"), &v.to_string()).unwrap();
    let (a, b, legacy, other) = (new_id(), new_id(), new_id(), new_id());
    put(&a, serde_json::json!({"project":"demo","machine":"m","action":action,"created_ms":10,"web":true}));
    // coordinator의 개별 교체: 후보 경로는 웹에 가지 않는다(종류만)
    let update = DeployAction::CheckedUpdate {
        from: "/private/candidate-marker/aaa".into(),
        sha256: "a".repeat(64),
        dry_run: false,
    };
    put(&b, serde_json::json!({"project":"demo","machine":"m","action":update,"created_ms":20,"web":false}));
    // 시각·출처를 남기기 전의 coordinator 기록
    put(&legacy, serde_json::json!({"project":"demo","machine":"m","action":action}));
    put(&other, serde_json::json!({"project":"elsewhere","machine":"m","action":action,"created_ms":30}));
    // 작업이 끝나면 웹이 목록을 다시 읽게 `rollout` 사건을 보낸다(재시작은 응답보다 늦게 끝날 수 있다)
    let mut changed = h.rollout_changed.subscribe();
    changed.borrow_and_update();
    let finished: DeployJob = serde_json::from_value(serde_json::json!({"id":a,"project":"demo","action":action,"state":"completed",
        "instance":"agent","executor_pid":1,"expected_sha256":"a".repeat(64),"result":null,"created_ms":10,"updated_ms":11}))
    .unwrap();
    h.deployment_changed("m", finished);
    assert!(changed.has_changed().unwrap(), "작업 끝을 웹에 알리지 않았다");
    let FrontResponse::DeployJobs { jobs } =
        h.call(DeviceRole::Reader, "device:tv", FrontRequest::DeployJobs).await.unwrap()
    else {
        panic!("작업 목록이 아니다")
    };
    assert_eq!(jobs.iter().map(|j| j.id.as_str()).collect::<Vec<_>>(), vec![b.as_str(), a.as_str(), legacy.as_str()]);
    assert_eq!(jobs.iter().map(|j| j.web).collect::<Vec<_>>(), vec![false, true, false]);
    assert!(jobs.iter().all(|j| j.state.is_none() && j.machine == "mac-a"), "끊긴 머신의 상태는 모른다고 보인다");
    assert_eq!(jobs[0].work, DeployWork::Update);
    assert_eq!(jobs[1].work, DeployWork::Restart { service: DeployService::Agent });
    assert!(!serde_json::to_string(&jobs).unwrap().contains("candidate-marker"), "작업 원문(후보 경로)이 웹에 갔다");
    // agent 영수증이 있으면 상태와 고정 까닭만: PID·결과 원문은 싣지 않는다
    let job = DeployJob {
        id: a.clone(),
        project: "demo".into(),
        action: restart(),
        state: DeployState::Failed,
        instance: "agent".into(),
        executor_pid: 987654,
        expected_sha256: "a".repeat(64),
        result: Some("launchctl /private/plist-marker 실패".into()),
        created_ms: 11,
        updated_ms: 12,
    };
    let record: serde_json::Value = serde_json::from_str(&h.db.meta(&format!("deploy:{a}")).unwrap().unwrap()).unwrap();
    let card = crate::deploy::job_card(a.clone(), "mac-a".into(), &record, Some(&job));
    let wire = serde_json::to_string(&card).unwrap();
    assert!(!wire.contains("987654") && !wire.contains("plist-marker"), "{wire}");
    assert_eq!((card.state, card.updated_ms), (Some(DeployState::Failed), 12));
    assert!(card.error.unwrap().contains("coordinator의 배포 기록"));
}

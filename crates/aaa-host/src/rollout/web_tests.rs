//! 웹 「배포」의 권한·기록·요약(관리 GUI W6a, plan §8.361). rollout 진행 자체는 m13 `rollout`이 실제 host·agent로 본다.
use super::*;
use aaa_proto::{AgentHello, DeviceRole, FrontRequest};

const WEB: &str = "device:phone";
/// 시험 몸을 따로 만든 runtime에서 돌리고, runtime을 끝낸 뒤 상태 폴더를 지운다. `agent_connected`가 띄운 task(`rewatch` 등)는
/// 이 시험에서 한 번도 돌지 않은 채 Host를 쥐고 있어, runtime을 끝내야 Host와 host DB가 닫힌다. Windows는 열린 DB가 든 폴더를
/// 지우지 못한다(os error 32, `deploy` 시험과 같다). Host가 정말 닫혔는지는 모든 OS에서 본다
fn in_lab<F: std::future::Future<Output = ()>>(body: impl FnOnce(Arc<Host>) -> F) {
    let root = std::env::temp_dir().join(format!("aaa-rollout-web-{}", aaa_common::util::new_id()));
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let host = rt.block_on(async {
        let h = lab(&root);
        let host = Arc::downgrade(&h);
        body(h).await;
        host
    });
    drop(rt);
    assert!(host.upgrade().is_none(), "runtime을 끝냈는데 Host가 남아 있다(DB가 열린 채다)");
    std::fs::remove_dir_all(root).unwrap();
}
fn lab(root: &std::path::Path) -> Arc<Host> {
    let mut cfg = aaa_config::HostConfig::default();
    cfg.projects.root = root.join("projects").to_string_lossy().into();
    cfg.notify.web_push = false;
    let h = Host::open(cfg, root.join("host")).unwrap();
    h.project_create("demo", Default::default(), "test").unwrap();
    // 기능을 알린 머신이 끊긴 뒤: 요청은 agent에 닿기 전에 끝난다
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
            harness: vec![],
        },
        true,
        crate::AgentConn::new(aaa_common::conn::spawn_writer(tokio::io::sink(), aaa_proto::MAX_FRAME), 1),
    );
    h.agent_disconnected("m", 1);
    h
}
fn restart() -> DeployAction {
    DeployAction::Restart { service: DeployService::Agent }
}

#[test]
fn web_controllers_restart_only_and_session_agents_cannot_plan() {
    in_lab(web_controllers_restart_only_and_session_agents_cannot_plan_body);
}
async fn web_controllers_restart_only_and_session_agents_cannot_plan_body(h: Arc<Host>) {
    let start = |action| FrontRequest::DeployStart { machine: Some("mac-a".into()), id: new_id(), action };
    // 웹은 실행 파일을 직접 바꾸지 못한다(교체는 rollout으로만)
    let update = DeployAction::CheckedUpdate { from: "/tmp/aaa".into(), sha256: "a".repeat(64), dry_run: false };
    let e = h.call(DeviceRole::Controller, WEB, start(update)).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Forbidden, "{}", e.message);
    // 재시작은 권한을 통과해 기록을 남긴 뒤 agent에 닿는다(여기서는 끊겨 있다)
    let id = new_id();
    let restart_req = || FrontRequest::DeployStart { machine: Some("mac-a".into()), id: id.clone(), action: restart() };
    assert_eq!(
        h.call(DeviceRole::Controller, WEB, restart_req()).await.unwrap_err().code,
        ErrorCode::AgentDisconnected
    );
    let record: serde_json::Value =
        serde_json::from_str(&h.db.meta(&format!("deploy:{id}")).unwrap().unwrap()).unwrap();
    assert_eq!((record["project"].as_str(), record["web"].as_bool()), (Some("demo"), Some(true)));
    let created = record["created_ms"].as_u64().unwrap();
    // 같은 ID로 다시 보내도 처음 기록 그대로, 다른 작업에는 쓰지 못한다
    assert_eq!(
        h.call(DeviceRole::Controller, WEB, restart_req()).await.unwrap_err().code,
        ErrorCode::AgentDisconnected
    );
    let again: serde_json::Value = serde_json::from_str(&h.db.meta(&format!("deploy:{id}")).unwrap().unwrap()).unwrap();
    assert_eq!(again["created_ms"].as_u64(), Some(created));
    let host = FrontRequest::DeployStart {
        machine: Some("mac-a".into()),
        id: id.clone(),
        action: DeployAction::Restart { service: DeployService::Host },
    };
    assert_eq!(h.call(DeviceRole::Controller, WEB, host).await.unwrap_err().code, ErrorCode::BadRequest);
    // 보기 전용 기기·세션 안 연결은 재시작도 점검도 못 한다
    assert_eq!(h.call(DeviceRole::Reader, "device:tv", start(restart())).await.unwrap_err().code, ErrorCode::Forbidden);
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
    };
    let e = h.call(DeviceRole::Controller, WEB, FrontRequest::RolloutStart { id: new_id(), spec }).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::BadRequest);
}

fn new_id() -> String {
    aaa_common::util::new_id()
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

//! pairing·기기(§6.4, 차이 목록 P1·P2). 시각은 넣어 준 `now`로 잰다(herdr-web-ui devices.test.ts처럼).
use super::*;

struct Tmp(PathBuf);
impl Drop for Tmp {
    fn drop(&mut self) {
        aaa_common::diagnostic::ignored(
            std::fs::remove_dir_all(&self.0),
            "정리 실패가 본 작업의 결과를 덮지 않게 한다",
        );
    }
}

fn host() -> (Arc<Host>, Tmp) {
    let dir = std::env::temp_dir().join(format!("aaa-pairing-test-{}", util::new_id()));
    let mut cfg = HostConfig::default();
    cfg.notify.web_push = false;
    (Host::open(cfg, dir.clone()).unwrap(), Tmp(dir))
}

async fn mint(h: &Arc<Host>, by: DeviceRole, role: DeviceRole) -> Result<String, ErrorBody> {
    match h.call(by, "test", FrontRequest::WebPair { role }).await? {
        FrontResponse::PairCode { code, expires_in_s, .. } => {
            assert_eq!(expires_in_s, 600);
            Ok(code)
        }
        other => panic!("{other:?}"),
    }
}

async fn devices(h: &Arc<Host>) -> Vec<aaa_proto::DeviceView> {
    match h.call(DeviceRole::Reader, "test", FrontRequest::Devices).await.unwrap() {
        FrontResponse::Devices { devices } => devices,
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn trust_management_requires_a_person_outside_the_session() {
    let (h, _t) = host();
    let requests = [
        FrontRequest::WebPair { role: DeviceRole::Controller },
        FrontRequest::EnrollOpen { minutes: 1 },
        FrontRequest::EnrollApprove { name: "missing".into(), code: "000000".into() },
        FrontRequest::MachineRevoke { name: "missing".into() },
        FrontRequest::DeviceRevoke { id: "missing".into() },
        FrontRequest::WebPublish,
        FrontRequest::WebPublishPort { port: 7443 },
        FrontRequest::WebPublishOff,
    ];
    for actor in ["local:cli@coordinator", "local:cli@unverified", "worker:thread", "evaluator:thread", "detached:cli"]
    {
        for request in &requests {
            // actor 경계 자체가 Local 권한으로도 거부해야 한다.
            let e = h.call(DeviceRole::Local, actor, request.clone()).await.expect_err(actor);
            assert_eq!(e.code, ErrorCode::Forbidden, "{actor}: {request:?}: {e:?}");
            assert!(e.message.contains("사람이 aaa 밖 터미널이나 웹에서"), "{e:?}");
        }
    }
    // 웹에서 폐기할 기기 밖에 조작 기기 하나를 남긴다(W5b: 마지막 조작 기기는 웹에서 폐기하지 않는다)
    let spare = mint(&h, DeviceRole::Local, DeviceRole::Controller).await.unwrap();
    h.redeem_pairing(&spare, "spare").unwrap();
    for (role, actor) in [(DeviceRole::Local, "local:cli"), (DeviceRole::Controller, "device:person")] {
        let code = match h.call(role, actor, requests[0].clone()).await.unwrap() {
            FrontResponse::PairCode { code, .. } => code,
            other => panic!("{other:?}"),
        };
        let (token, _) = h.redeem_pairing(&code, "person").unwrap();
        let (id, _) = h.device_role(&token).unwrap();
        h.call(role, actor, FrontRequest::DeviceRevoke { id }).await.unwrap();
        assert!(h.device_role(&token).is_none());
    }
    h.call(DeviceRole::Local, "local:cli", requests[1].clone()).await.unwrap();
    for request in &requests[2..4] {
        assert_ne!(
            h.call(DeviceRole::Local, "local:cli", request.clone()).await.unwrap_err().code,
            ErrorCode::Forbidden
        );
    }
    // W5b(FRONT_API 1.80)부터 머신 등록 창은 웹 조작 기기도 연다. 보기 기기는 여전히 못 한다
    h.call(DeviceRole::Controller, "device:person", requests[1].clone()).await.unwrap();
    assert_eq!(
        h.call(DeviceRole::Reader, "device:viewer", requests[1].clone()).await.unwrap_err().code,
        ErrorCode::Forbidden
    );
}

/// 관리 GUI W5b(사용자 답 2A): 마지막 조작 기기는 웹에서 폐기하지 않고(host 터미널은 된다), host 머신도 웹에서 폐기하지 않는다
#[tokio::test]
async fn the_web_keeps_its_last_controller_and_the_host_machine() {
    let (h, _t) = host();
    let pair = |role| {
        let h = h.clone();
        async move {
            let code = mint(&h, DeviceRole::Local, role).await.unwrap();
            let (token, _) = h.redeem_pairing(&code, "기기").unwrap();
            h.device_role(&token).unwrap().0
        }
    };
    let first = pair(DeviceRole::Controller).await;
    let viewer = pair(DeviceRole::Reader).await;
    let revoke = |id: &str| FrontRequest::DeviceRevoke { id: id.into() };
    let e = h.call(DeviceRole::Controller, &format!("device:{first}"), revoke(&first)).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Forbidden, "보기 기기가 있어도 조작 기기는 하나뿐이다: {e:?}");
    let second = pair(DeviceRole::Controller).await;
    h.call(DeviceRole::Controller, &format!("device:{second}"), revoke(&first)).await.unwrap();
    h.call(DeviceRole::Controller, &format!("device:{second}"), revoke(&viewer)).await.unwrap();
    assert_eq!(
        h.call(DeviceRole::Controller, &format!("device:{second}"), revoke(&second)).await.unwrap_err().code,
        ErrorCode::Forbidden
    );
    h.call(DeviceRole::Local, "local:cli", revoke(&second)).await.unwrap();
    assert!(devices(&h).await.is_empty(), "host 터미널은 마지막 기기도 폐기한다");

    h.inner.lock().unwrap().machines.insert(
        "m-local".into(),
        MachineRt {
            name: "mac-a".into(),
            local: true,
            os: None,
            boot_id: None,
            dev_mode: false,
            conn: None,
            last_seen_ms: None,
            health: None,
            health_sent: None,
            features: vec![],
            identity: None,
            agent_pid: None,
            colors: None,
            build: Default::default(),
        },
    );
    let e = h
        .call(DeviceRole::Controller, "device:x", FrontRequest::MachineRevoke { name: "mac-a".into() })
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Forbidden, "{e:?}");
}

#[tokio::test]
async fn a_code_is_used_once_and_carries_its_role() {
    let (h, _t) = host();
    let code = mint(&h, DeviceRole::Local, DeviceRole::Reader).await.unwrap();
    let (token, role) = h.redeem_pairing(&code, "  iPhone \n · Safari ").unwrap();
    assert_eq!(role, DeviceRole::Reader);
    assert_eq!(h.device_role(&token).map(|(_, r)| r), Some(DeviceRole::Reader));
    assert_eq!(h.redeem_pairing(&code, "x").unwrap_err().code, ErrorCode::Unauthorized);
    let list = devices(&h).await;
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].label, "iPhone · Safari");
    assert_eq!(list[0].role, DeviceRole::Reader);
    // 처음 들어올 때 「마지막으로 본 때」를 적는다
    assert!(list[0].last_seen_ms.is_some());
}

#[tokio::test]
async fn a_new_code_replaces_the_one_still_out() {
    let (h, _t) = host();
    let first = mint(&h, DeviceRole::Local, DeviceRole::Controller).await.unwrap();
    let mut second = mint(&h, DeviceRole::Controller, DeviceRole::Controller).await.unwrap();
    // 같은 숫자가 나올 수 있다(1/10⁶): 다른 코드가 나올 때까지 다시 만든다
    while second == first {
        second = mint(&h, DeviceRole::Controller, DeviceRole::Controller).await.unwrap();
    }
    assert_eq!(h.redeem_pairing(&first, "").unwrap_err().code, ErrorCode::Unauthorized);
    let (_, role) = h.redeem_pairing(&second, "").unwrap();
    assert_eq!(role, DeviceRole::Controller);
    assert_eq!(devices(&h).await[0].label, db::DEVICE_LABEL_FALLBACK);
}

#[tokio::test]
async fn a_code_expires_after_ten_minutes() {
    let (h, _t) = host();
    let now = Instant::now();
    let (code, _) = h.mint_pairing(DeviceRole::Controller, now);
    assert!(
        h.redeem_pairing_at(&code, "", now + PAIR_CODE_TTL)
            .as_ref()
            .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
            .is_err()
    );
    let (code, _) = h.mint_pairing(DeviceRole::Controller, now);
    assert!(
        h.redeem_pairing_at(&code, "", now + PAIR_CODE_TTL - Duration::from_secs(1))
            .as_ref()
            .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
            .is_ok()
    );
}

#[tokio::test]
async fn five_wrong_tries_spend_the_code_and_then_wait_doubles_up_to_a_minute() {
    let (h, _t) = host();
    let now = Instant::now();
    let (code, _) = h.mint_pairing(DeviceRole::Controller, now);
    let wrong = if code == "000000" { "111111" } else { "000000" };
    for _ in 0..5 {
        assert_eq!(h.redeem_pairing_at(wrong, "", now).unwrap_err().code, ErrorCode::Unauthorized);
    }
    // 기다리는 동안은 맞는 코드도 비교하지 않는다
    assert_eq!(h.redeem_pairing_at(&code, "", now).unwrap_err().code, ErrorCode::Unavailable);
    // 1초 뒤에는 받지만 그 코드는 이미 버렸다(한 코드로 5번)
    let mut at = now + PAIR_BACKOFF;
    assert_eq!(h.redeem_pairing_at(&code, "", at).unwrap_err().code, ErrorCode::Unauthorized);
    // 틀릴 때마다 기다림이 두 배, 60초에서 멈춘다(참고 1초·60초)
    let mut waits = vec![];
    for _ in 0..10 {
        let wait = (0..=120)
            .map(Duration::from_secs)
            .find(|d| h.redeem_pairing_at(wrong, "", at + *d).unwrap_err().code == ErrorCode::Unauthorized)
            .unwrap();
        waits.push(wait.as_secs());
        at += wait;
    }
    assert_eq!(waits, [2, 4, 8, 16, 32, 60, 60, 60, 60, 60]);
}

#[tokio::test]
async fn wrong_tries_older_than_ten_minutes_are_forgotten() {
    let (h, _t) = host();
    let now = Instant::now();
    h.mint_pairing(DeviceRole::Controller, now);
    for i in 0..5 {
        // 다른 코드로 틀린다(한 코드의 5번 한도와 따로 센다)
        aaa_common::diagnostic::ignored(
            h.redeem_pairing_at(&format!("{i:06}x"), "", now),
            "보조 작업 실패 뒤 기존 처리를 계속한다",
        );
    }
    assert_eq!(h.redeem_pairing_at("1", "", now).unwrap_err().code, ErrorCode::Unavailable);
    // 창이 지나면 처음처럼 기다림 없이 다섯 번
    let later = now + PAIR_FAIL_WINDOW;
    for _ in 0..5 {
        assert_eq!(h.redeem_pairing_at("1", "", later).unwrap_err().code, ErrorCode::Unauthorized);
    }
    assert_eq!(h.redeem_pairing_at("1", "", later).unwrap_err().code, ErrorCode::Unavailable);
}

#[tokio::test]
async fn a_new_code_from_the_owner_lifts_the_wait() {
    let (h, _t) = host();
    let now = Instant::now();
    h.mint_pairing(DeviceRole::Controller, now);
    for _ in 0..12 {
        aaa_common::diagnostic::ignored(h.redeem_pairing_at("1", "", now), "보조 작업 실패 뒤 기존 처리를 계속한다");
    }
    // CLI(`aaa web pair`)나 controller 기기가 새 코드를 만들면 남이 틀려 둔 잠금 없이 바로 쓴다
    let code = mint(&h, DeviceRole::Local, DeviceRole::Controller).await.unwrap();
    assert!(
        h.redeem_pairing_at(&code, "", now)
            .as_ref()
            .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
            .is_ok()
    );
}

#[tokio::test]
async fn a_view_path_ends_with_the_device_that_got_it() {
    let (h, _t) = host();
    let code = mint(&h, DeviceRole::Local, DeviceRole::Controller).await.unwrap();
    let (token, _) = h.redeem_pairing(&code, "").unwrap();
    let (id, _) = h.device_role(&token).unwrap();
    let until = Instant::now() + library::VIEW_TTL;
    let cap = |device: Option<&str>| library::ViewCap {
        project: "nope".into(),
        thread: "t-0001".into(),
        until,
        device: device.map(str::to_string),
        attachment: None,
    };
    h.view_caps.lock().unwrap().insert("dev".into(), cap(Some(&id)));
    h.view_caps.lock().unwrap().insert("cli".into(), cap(None));
    // 경로 검사를 지나면 다음은 프로젝트 찾기(없는 프로젝트)다
    let reach = |name: &'static str| {
        let h = h.clone();
        async move { h.view_source(name, "a.txt").await.err().map(|e| e.code) }
    };
    assert_ne!(reach("dev").await, Some(ErrorCode::Unauthorized));
    h.revoke_device(&id).unwrap();
    assert_eq!(reach("dev").await, Some(ErrorCode::Unauthorized));
    assert_ne!(reach("cli").await, Some(ErrorCode::Unauthorized));
}

#[tokio::test]
async fn the_web_mints_controller_codes_and_a_reader_manages_nothing() {
    let (h, _t) = host();
    assert_eq!(mint(&h, DeviceRole::Controller, DeviceRole::Reader).await.unwrap_err().code, ErrorCode::Forbidden);
    assert_eq!(mint(&h, DeviceRole::Reader, DeviceRole::Controller).await.unwrap_err().code, ErrorCode::Forbidden);
    let code = mint(&h, DeviceRole::Local, DeviceRole::Controller).await.unwrap();
    h.redeem_pairing(&code, "Mac · Chrome").unwrap();
    let id = devices(&h).await[0].id.clone();
    for req in [
        FrontRequest::DeviceRename { id: id.clone(), label: "x".into() },
        FrontRequest::DeviceRevoke { id: id.clone() },
    ] {
        assert_eq!(h.call(DeviceRole::Reader, "test", req).await.unwrap_err().code, ErrorCode::Forbidden);
    }
    let long = "가".repeat(60);
    h.call(DeviceRole::Controller, "test", FrontRequest::DeviceRename { id: id.clone(), label: long }).await.unwrap();
    assert_eq!(devices(&h).await[0].label.chars().count(), 48);
    let missing = FrontRequest::DeviceRename { id: "nope".into(), label: "x".into() };
    assert_eq!(h.call(DeviceRole::Controller, "test", missing).await.unwrap_err().code, ErrorCode::NotFound);
}

#[tokio::test]
async fn revoking_tells_open_connections_and_drops_the_token_and_push() {
    let (h, _t) = host();
    let code = mint(&h, DeviceRole::Local, DeviceRole::Controller).await.unwrap();
    let (token, _) = h.redeem_pairing(&code, "").unwrap();
    let (id, _) = h.device_role(&token).unwrap();
    let sub = db::PushSub {
        endpoint: "https://push.example/a".into(),
        p256dh: "k".into(),
        auth: "a".into(),
        device: id.clone(),
        alerts: None,
    };
    h.db.push_put_at(&sub, None).unwrap();
    let mut heard = h.device_revoked.subscribe();
    assert!(!h.is_revoked(&id));
    h.call(DeviceRole::Controller, "test", FrontRequest::DeviceRevoke { id: id.clone() }).await.unwrap();
    assert_eq!(heard.try_recv().unwrap(), id);
    assert!(h.is_revoked(&id));
    assert!(h.device_role(&token).is_none());
    assert!(h.db.push_list(None).unwrap().is_empty());
    assert!(devices(&h).await.is_empty());
    let again = FrontRequest::DeviceRevoke { id };
    assert_eq!(h.call(DeviceRole::Local, "test", again).await.unwrap_err().code, ErrorCode::NotFound);
}

/// 보안 감사 t-0340 B10: endpoint만 아는 다른 기기는 구독을 가져가지 못하고, 키까지 같은 같은 브라우저(다시 pairing)는 옮긴다
#[tokio::test]
async fn another_device_takes_a_push_endpoint_only_with_its_keys() {
    let (h, _t) = host();
    let mut ids = Vec::new();
    for _ in 0..2 {
        let code = mint(&h, DeviceRole::Local, DeviceRole::Controller).await.unwrap();
        let (token, _) = h.redeem_pairing(&code, "").unwrap();
        ids.push(h.device_role(&token).unwrap().0);
    }
    let sub = |device: &str, auth: &str| db::PushSub {
        endpoint: "https://push.example/a".into(),
        p256dh: "k".into(),
        auth: auth.into(),
        device: device.into(),
        alerts: None,
    };
    assert!(h.db.push_put_at(&sub(&ids[0], "a"), None).unwrap());
    // 같은 기기는 키를 바꿀 수 있다
    assert!(h.db.push_put_at(&sub(&ids[0], "a2"), None).unwrap());
    assert!(!h.db.push_put_at(&sub(&ids[1], "x"), None).unwrap());
    assert_eq!(h.db.push_list(Some(&ids[0])).unwrap().len(), 1);
    assert!(h.db.push_list(Some(&ids[1])).unwrap().is_empty());
    assert!(h.db.push_put_at(&sub(&ids[1], "a2"), None).unwrap());
    assert!(h.db.push_list(Some(&ids[0])).unwrap().is_empty());
    assert_eq!(h.db.push_list(Some(&ids[1])).unwrap().len(), 1);
    // 기기가 고른 알림 종류(N3): 주면 바꾸고, 없이 다시 넣으면(불러올 때마다 다시 구독) 둔다
    let alerts = |h: &Host| h.db.push_list(Some(&ids[1])).unwrap()[0].alerts.unwrap();
    assert_eq!(alerts(&h), db::Alerts::default());
    let quiet = db::Alerts { input: false, done: true };
    assert!(h.db.push_put_at(&db::PushSub { alerts: Some(quiet), ..sub(&ids[1], "a2") }, None).unwrap());
    assert_eq!(alerts(&h), quiet);
    assert!(h.db.push_put_at(&sub(&ids[1], "a2"), None).unwrap());
    assert_eq!(alerts(&h), quiet);
}

/// 보안 감사 t-0340 B5: 제어 문자·공백이 든 이름으로는 등록을 기다리지 못한다(`aaa machine pending` 화면·push 본문 위조)
#[tokio::test]
async fn enroll_rejects_machine_names_with_control_characters() {
    let (h, _t) = host();
    h.enroll.lock().unwrap().open_until = Some(Instant::now() + Duration::from_secs(60));
    for bad in ["mac\x1b[2Ja", "mac a", "a\u{202e}b", ""] {
        let e = h.enroll_request_bound(bad, "01M", "fp", "sas", None).unwrap_err();
        assert_eq!(e.code, ErrorCode::BadRequest, "{bad:?}");
    }
    assert!(h.enroll.lock().unwrap().pending.is_empty());
    assert!(
        h.enroll_request_bound("mac-b", "01M", "fp", "sas", None)
            .as_ref()
            .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
            .is_ok()
    );
}

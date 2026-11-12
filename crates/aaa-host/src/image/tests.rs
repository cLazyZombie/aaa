use super::*;
use std::time::Instant;

struct Lab(PathBuf);
impl Drop for Lab {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

fn fixture(index: usize) -> Job {
    let id = format!("{index:026}");
    Job {
        begin: ImageBegin {
            id: id.clone(),
            prompt: "그".repeat(10_666),
            profile: "image".into(),
            account: None,
            transparent: false,
            inputs: (0..5).map(|_| ImageFile { bytes: 10 * 1024 * 1024, sha256: "a".repeat(64) }).collect(),
        },
        machine: "m".into(),
        session: "s".into(),
        project: "demo".into(),
        owner: "coordinator".into(),
        model: "model-a".into(),
        effort: "high".into(),
        view: ImageView {
            id,
            state: ImageState::Succeeded,
            account: Some("a".repeat(64)),
            reason: match index % 3 {
                0 => "완".repeat(1024),
                1 => "\u{01}".repeat(1024),
                _ => "\"\\\n\r\t".repeat(200),
            },
            outputs: (0..5).map(|_| ImageFile { bytes: 50 * 1024 * 1024, sha256: "b".repeat(64) }).collect(),
            total_tokens: Some(u64::MAX),
        },
        allocation: None,
        launched: true,
    }
}

async fn list(h: &Arc<Host>, expected: usize) {
    let started = Instant::now();
    let response = h.image_request("m", "s", ImageRequest::List).await.unwrap();
    let elapsed = started.elapsed();
    let ImageResponse::Jobs { jobs } = &response else { panic!("이미지 목록 응답") };
    assert_eq!(jobs.len(), expected);
    assert!(jobs.iter().all(|j| serde_json::to_string(&j.reason).unwrap().len() <= 202));
    assert_eq!(jobs[0].outputs.len(), 5, "목록에서도 결과 정보를 보존한다");
    let reply = LinkMsg::Req {
        id: format!("image-{}", "A".repeat(26)),
        deadline_ms: None,
        body: AgentRequest::ImageReply { request_id: "A".repeat(26), result: Outcome::Ok(response) },
    };
    eprintln!("{expected}건 List: {elapsed:?}, frame {} bytes", serde_json::to_vec(&reply).unwrap().len() + 1);
    let (tx, _frames) = aaa_common::conn::frame_channel();
    tx.json(&reply).await.expect("목록 전체가 실제 agent 전송 한도 안에 있어야 한다");
}

#[tokio::test]
async fn ledger_accepts_1023_rejects_1024_and_forget_frees_a_slot() {
    let lab = Lab(std::env::temp_dir().join(format!("aaa-image-limit-{}", util::new_id())));
    let mut cfg = aaa_config::HostConfig::default();
    cfg.projects.root = lab.0.join("projects").display().to_string();
    cfg.notify.web_push = false;
    cfg.profiles.insert(
        "image".into(),
        aaa_config::Profile {
            harness: "codex".into(),
            model: "model-a".into(),
            effort: "high".into(),
            ..Default::default()
        },
    );
    let h = Host::open(cfg.clone(), lab.0.join("host")).unwrap();
    h.project_create("demo", Default::default(), "test").unwrap();
    h.db.upsert_machine("m", "mac-a", None, true).unwrap();
    drop(h);
    let h = Host::open(cfg, lab.0.join("host")).unwrap();
    h.on_session(
        "m",
        SessionInfo {
            responsibility: None,
            id: "s".into(),
            name: "coordinator".into(),
            argv: vec!["codex".into()],
            cwd: Some(h.project_dir("demo").unwrap().display().to_string()),
            pid: None,
            started_at_ms: 0,
            boot_id: "boot".into(),
            cols: 80,
            rows: 24,
            state: SessionState::Running,
            role: Some(Role::Coordinator),
            label: None,
            harness: Some("codex".into()),
        },
        false,
    );
    h.db.set_meta("default-account:codex", "work").unwrap();
    h.db.image_transaction(|tx| {
        for index in 0..1023 {
            let job = fixture(index);
            tx.execute(
                "INSERT INTO meta(key,value) VALUES(?1,?2)",
                [key(&job.begin.id), serde_json::to_string(&job)?],
            )?;
        }
        Ok(())
    })
    .unwrap();
    list(&h, 1023).await;
    let mut begin = fixture(1023).begin;
    begin.prompt = "fixture".into();
    begin.inputs.clear();
    let started = Instant::now();
    let response = h.image_request("m", "s", ImageRequest::Begin(begin.clone())).await.unwrap();
    eprintln!("1023건에서 Begin: {:?}", started.elapsed());
    assert!(matches!(response, ImageResponse::Job(ImageView { state: ImageState::Draft, .. })));

    list(&h, 1024).await;
    let ImageResponse::Job(status) =
        h.image_request("m", "s", ImageRequest::Status { id: fixture(0).begin.id, wait: false }).await.unwrap()
    else {
        panic!("이미지 상태 응답");
    };
    assert_eq!(status.reason, "완".repeat(1024), "개별 조회에는 전체 사유를 보존한다");
    assert!(h.image_request("m", "s", ImageRequest::Begin(begin)).await.is_ok(), "한도에서도 같은 ID는 재조회한다");

    let mut next = fixture(1024).begin;
    next.prompt = "fixture".into();
    next.inputs.clear();
    let error = h.image_request("m", "s", ImageRequest::Begin(next.clone())).await.unwrap_err();
    assert_eq!(error.code, ErrorCode::BadRequest);
    assert_eq!(error.message, "host 이미지 원장 한도(1024건)에 도달했다");
    assert!(h.db.meta(&key(&next.id)).unwrap().is_none());
    assert!(!h.image_dir(&next.id).unwrap().exists());

    h.image_request("m", "s", ImageRequest::Forget { id: fixture(0).begin.id }).await.unwrap();
    assert!(h.image_request("m", "s", ImageRequest::Begin(next)).await.is_ok());
    let ImageResponse::Jobs { jobs } = h.image_request("m", "s", ImageRequest::List).await.unwrap() else {
        panic!("이미지 목록 응답");
    };
    assert_eq!(jobs.len(), 1024, "forget 영수증은 한도와 목록에서 제외한다");
    assert_eq!(h.db.meta_suffixes(PREFIX).unwrap().len(), 1025, "중복 방지 영수증은 보존한다");
}

#[test]
fn list_reason_limits_json_bytes_without_cutting_unicode() {
    for (text, expected) in [
        ("a".repeat(201), "a".repeat(200)),
        ("한".repeat(100), "한".repeat(66)),
        ("\u{01}".repeat(100), "\u{01}".repeat(33)),
        ("\"".repeat(101), "\"".repeat(100)),
        ("짧은 사유".into(), "짧은 사유".into()),
    ] {
        assert_eq!(list_reason(&text), expected);
    }
}

fn account_lab() -> (Lab, Arc<Host>) {
    let lab = Lab(std::env::temp_dir().join(format!("aaa-image-account-{}", util::new_id())));
    let mut cfg = aaa_config::HostConfig::default();
    cfg.projects.root = lab.0.join("projects").display().to_string();
    cfg.notify.web_push = false;
    let h = Host::open(cfg, lab.0.join("host")).unwrap();
    h.project_create("demo", Default::default(), "test").unwrap();
    let auth = serde_json::json!({"tokens":{"access_token":aaa_test_support::token("image-account-access"),"refresh_token":aaa_test_support::token("image-account-refresh")}}).to_string();
    for _ in 0..3 {
        h.accounts.import("work", "codex", &auth).unwrap();
    }
    (lab, h)
}

fn account_job(h: &Host, index: usize, state: ImageState) -> Job {
    let mut job = fixture(index);
    job.begin.prompt = aaa_test_support::token("image-private-prompt");
    job.begin.account = Some("work".into());
    job.begin.inputs.clear();
    job.owner = "thread:t-0001".into();
    job.view.account = Some("work".into());
    job.view.state = state;
    job.view.reason = "host 재시작 · 생성 자동 반복 없음".into();
    job.view.outputs.clear();
    job.allocation = Some(crate::account_pool::Allocation {
        generation: h.accounts.generation("work").unwrap(),
        ended_at_ms: None,
        released: false,
        reserve_percent: Some(5),
        selection: AccountSelection {
            size: AccountTaskSize::Small,
            group: "work".into(),
            unobserved: false,
            reason: "fixture".into(),
            selected_at_ms: 123456789,
        },
    });
    h.image_save(&job).unwrap();
    job
}

#[tokio::test]
async fn account_image_unknown_confirmation_unblocks_rename_without_releasing_or_replaying() {
    let (_lab, h) = account_lab();
    let job = account_job(&h, 1, ImageState::Unknown);
    let mut thread =
        crate::orch::ThreadRec::starting("demo", "이미지 소유자", "m", "codex", Role::Worker, ThreadKind::Dir, 0);
    thread.id = "t-0001".into();
    thread.account = Some("work".into());
    thread.pane = Some("s".into());
    thread.status = ThreadStatus::Open;
    h.db.thread_import("demo", "t-0001", "open", &serde_json::to_string(&thread).unwrap()).unwrap();
    h.on_session(
        "m",
        SessionInfo {
            responsibility: None,
            id: "s".into(),
            name: "owner".into(),
            argv: vec!["codex".into()],
            cwd: Some(h.project_dir("demo").unwrap().display().to_string()),
            pid: None,
            started_at_ms: 0,
            boot_id: "boot".into(),
            cols: 80,
            rows: 24,
            state: SessionState::Running,
            role: Some(Role::Worker),
            label: None,
            harness: Some("codex".into()),
        },
        false,
    );
    let original = h.accounts.credential("work").unwrap();
    assert_eq!(original.version, 3);
    h.set_default_account("codex", Some("work")).unwrap();
    let dir = h.image_dir(&job.begin.id).unwrap();
    util::ensure_private_dir(&dir).unwrap();
    util::write_private(&dir.join("input-0"), b"retained-input").unwrap();
    let rename = || FrontRequest::AccountRename { name: "work".into(), to: "renamed".into() };
    let error = h.call(DeviceRole::Controller, "device:phone", rename()).await.unwrap_err();
    assert_eq!(error.code, ErrorCode::Conflict);
    assert!(error.message.contains(&job.begin.id) && error.message.contains("결과 불명"));
    assert!(error.message.contains("확인 후 정리하면 이름 변경 가능"));
    assert!(h.db.meta(crate::account_rename::JOURNAL).unwrap().is_none());
    let listed = h
        .call(DeviceRole::Controller, "device:phone", FrontRequest::AccountImageJobs { name: "work".into() })
        .await
        .unwrap();
    let FrontResponse::AccountImageJobs { jobs } = &listed else { panic!("계정 이미지 목록") };
    assert_eq!(jobs.len(), 1);
    assert_eq!(
        (&jobs[0].id, jobs[0].owner.as_str(), jobs[0].state.as_str(), jobs[0].selected_at_ms),
        (&job.begin.id, "t-0001", "unknown", Some(123456789))
    );
    let wire = serde_json::to_string(&listed).unwrap();
    assert!(!wire.contains(&job.begin.prompt) && !wire.contains("machine") && !wire.contains("generation"));
    h.call(
        DeviceRole::Controller,
        "device:phone",
        FrontRequest::AccountImageDismiss { name: "work".into(), id: job.begin.id.clone() },
    )
    .await
    .unwrap();
    let dismissed = h.image_load(&job.begin.id).unwrap();
    assert_eq!(dismissed.view.state, ImageState::Forgotten);
    assert_eq!(dismissed.begin, job.begin, "같은 요청의 영수증은 바꾸지 않는다");
    assert_eq!(
        serde_json::to_value(&dismissed.allocation).unwrap(),
        serde_json::to_value(&job.allocation).unwrap(),
        "불명 사용량 예약을 해제하거나 종료로 추정하지 않는다"
    );
    assert_eq!(std::fs::read(dir.join("input-0")).unwrap(), b"retained-input");
    assert_eq!(h.account_image_jobs("work").unwrap(), FrontResponse::AccountImageJobs { jobs: vec![] });
    let begun = h.image_request("m", "s", ImageRequest::Begin(job.begin.clone())).await.unwrap();
    assert!(matches!(begun, ImageResponse::Job(ImageView { state: ImageState::Forgotten, .. })));
    let started = h.image_request("m", "s", ImageRequest::Start { id: job.begin.id.clone() }).await.unwrap();
    assert!(
        matches!(started, ImageResponse::Job(ImageView { state: ImageState::Forgotten, .. })),
        "같은 ID를 다시 시작해도 생성하지 않는다"
    );
    h.call(DeviceRole::Local, "local:cli", rename()).await.unwrap();
    let renamed = h.accounts.credential("renamed").unwrap();
    assert_eq!(renamed.version, 3);
    assert_eq!(renamed.auth_generation, original.auth_generation);
    assert!(renamed.secret.0 == original.secret.0, "인증 원문 보존");
    assert_eq!(h.default_account("codex").unwrap().as_deref(), Some("renamed"));
    let receipt = h.image_load(&job.begin.id).unwrap();
    assert_eq!(receipt.view.state, ImageState::Forgotten);
    assert_eq!(receipt.view.account.as_deref(), Some("renamed"));
    assert_eq!(receipt.begin.account.as_deref(), Some("renamed"));
    let reopened = Host::open(h.cfg.clone(), h.state.clone()).unwrap();
    assert_eq!(
        reopened.image_load(&job.begin.id).unwrap().view.state,
        ImageState::Forgotten,
        "재시작도 재생성하지 않는다"
    );
    assert!(!reopened.image_load(&job.begin.id).unwrap().allocation.unwrap().released);
}

#[tokio::test]
async fn account_image_management_rejects_non_humans_running_jobs_and_changed_accounts() {
    let (_lab, h) = account_lab();
    let unknown = account_job(&h, 1, ImageState::Unknown);
    let running = account_job(&h, 2, ImageState::Running);
    let succeeded = account_job(&h, 3, ImageState::Succeeded);
    let old = h.db.meta(&key(&unknown.begin.id)).unwrap();
    for (role, actor) in [
        (DeviceRole::Reader, "device:tv"),
        (DeviceRole::Local, "local:cli@coordinator"),
        (DeviceRole::Local, "worker:t-0001"),
        (DeviceRole::Controller, "worker:t-0001"),
    ] {
        for req in [
            FrontRequest::AccountImageJobs { name: "work".into() },
            FrontRequest::AccountImageDismiss { name: "work".into(), id: unknown.begin.id.clone() },
        ] {
            assert_eq!(h.call(role, actor, req).await.unwrap_err().code, ErrorCode::Forbidden);
        }
    }
    assert_eq!(h.db.meta(&key(&unknown.begin.id)).unwrap(), old);
    let FrontResponse::AccountImageJobs { jobs } = h.account_image_jobs("work").unwrap() else {
        panic!("계정 이미지 목록")
    };
    assert_eq!(jobs.len(), 2, "성공한 작업은 이름 변경을 막지 않는다");
    for id in [&running.begin.id, &succeeded.begin.id] {
        assert_eq!(
            h.call(
                DeviceRole::Local,
                "local:cli",
                FrontRequest::AccountImageDismiss { name: "work".into(), id: id.clone() }
            )
            .await
            .unwrap_err()
            .code,
            ErrorCode::Conflict
        );
    }
    h.accounts.import("other", "claude", &aaa_test_support::token("other-account")).unwrap();
    assert_eq!(
        h.call(
            DeviceRole::Controller,
            "device:phone",
            FrontRequest::AccountImageDismiss { name: "other".into(), id: unknown.begin.id.clone() }
        )
        .await
        .unwrap_err()
        .code,
        ErrorCode::Conflict
    );
    assert!(h.account_image_jobs("bad/name").is_err());
    assert!(h.account_image_jobs("absent").is_err());
    assert!(h.account_image_dismiss("work", "../bad", "local:cli").await.is_err());
    // 결과 불명만 정리해도 실행 중인 작업은 별도 보호한다.
    h.account_image_dismiss("work", &unknown.begin.id, "local:cli").await.unwrap();
    let error = h
        .call(DeviceRole::Local, "local:cli", FrontRequest::AccountRename { name: "work".into(), to: "renamed".into() })
        .await
        .unwrap_err();
    assert!(error.message.contains(&running.begin.id) && error.message.contains("생성 중 · 완료 뒤 이름 변경 가능"));
    let mut ended = running.clone();
    ended.view.state = ImageState::Succeeded;
    h.image_save(&ended).unwrap();
    h.call(DeviceRole::Local, "local:cli", FrontRequest::AccountRename { name: "work".into(), to: "renamed".into() })
        .await
        .unwrap();
}

#[tokio::test]
async fn account_image_recovery_keeps_unknown_until_human_confirmation_and_serializes_dismissals() {
    let (_lab, h) = account_lab();
    let job = account_job(&h, 1, ImageState::Running);
    h.image_recover().unwrap();
    assert_eq!(h.image_load(&job.begin.id).unwrap().view.state, ImageState::Unknown);
    let dismiss = || FrontRequest::AccountImageDismiss { name: "work".into(), id: job.begin.id.clone() };
    let held = h.thread_op_lock("image", &job.begin.id).lock_owned().await;
    let first = h.call(DeviceRole::Controller, "device:first", dismiss());
    let second = h.call(DeviceRole::Controller, "device:second", dismiss());
    tokio::pin!(first, second);
    assert!(matches!(futures_util::poll!(&mut first), std::task::Poll::Pending));
    assert!(matches!(futures_util::poll!(&mut second), std::task::Poll::Pending));
    drop(held);
    let (first, second) = tokio::join!(first, second);
    assert!(first.is_ok());
    assert_eq!(second.unwrap_err().code, ErrorCode::Conflict);
    h.image_recover().unwrap();
    let after = h.image_load(&job.begin.id).unwrap();
    assert_eq!(after.view.state, ImageState::Forgotten);
    assert_eq!(serde_json::to_value(after.allocation).unwrap(), serde_json::to_value(job.allocation).unwrap());
}

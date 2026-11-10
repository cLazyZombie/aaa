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

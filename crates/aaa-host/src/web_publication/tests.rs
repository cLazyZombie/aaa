use super::*;
use crate::db::PushSub;
use aaa_proto::{DeviceRole, FrontRequest, PostInput, PostResult, PostType};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tower::ServiceExt;

const OLD: &str = "https://mac-a.example.ts.net:8443";
const NEW: &str = "https://mac-a.example.ts.net:7443";
struct Lab(PathBuf);
impl Drop for Lab {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn host() -> (Arc<Host>, Lab) {
    let path = std::env::temp_dir().join(format!("aaa-publication-{}", aaa_common::util::new_id()));
    let mut cfg = aaa_config::HostConfig::default();
    cfg.projects.root = path.join("projects").to_string_lossy().into();
    cfg.notify.web_push = false;
    let h = Host::open(cfg, path.clone()).unwrap();
    h.web_port.store(7000, std::sync::atomic::Ordering::Relaxed);
    h.db.set_meta("publish_port", "8443").unwrap();
    h.db.set_meta("publish_origin", OLD).unwrap();
    (h, Lab(path))
}
#[test]
fn origin_reads_the_last_committed_mapping_while_a_writer_holds_the_connection() {
    let (h, _lab) = host();
    let mut conn = h.db.conn_for_test();
    let tx = conn.transaction().unwrap();
    tx.execute("UPDATE meta SET value=? WHERE key='publish_origin'", [NEW]).unwrap();
    let (send, received) = std::sync::mpsc::channel();
    let reader = h.clone();
    let read = std::thread::spawn(move || send.send(reader.public_origin()).unwrap());
    let before = received.recv_timeout(std::time::Duration::from_secs(3));
    tx.commit().unwrap();
    drop(conn);
    read.join().unwrap();
    assert_eq!(before.expect("쓰기 연결을 놓기 전에 조회를 마친다").as_deref(), Some(OLD));
    assert_eq!(h.public_origin().as_deref(), Some(NEW));
    h.db.observe_publication(None, Some("mac-a.example.ts.net")).unwrap();
    assert_eq!(h.public_origin(), None);
}

struct Fixture {
    config: Mutex<Value>,
    calls: Mutex<Vec<String>>,
    fail_on: Mutex<bool>,
}
impl Fixture {
    fn new() -> Self {
        Self {
            config: Mutex::new(json!({"TCP":{"8443":{"HTTPS":true},"443":{"HTTPS":true}},"Web":{
                "mac-a.example.ts.net:8443":{"Handlers":{"/":{"Proxy":"http://127.0.0.1:7000"}}},
                "mac-a.example.ts.net:443":{"Handlers":{"/":{"Proxy":"http://127.0.0.1:9000"}}}
            }})),
            calls: Mutex::new(vec![]),
            fail_on: Mutex::new(false),
        }
    }
}
impl Serve for Fixture {
    async fn read(&self) -> Option<tailscale::TsOutput> {
        Some(tailscale::TsOutput {
            status: Some(r#"{"BackendState":"Running","Self":{"DNSName":"mac-a.example.ts.net."}}"#.into()),
            serve: Some(self.config.lock().unwrap().to_string()),
        })
    }
    async fn on(&self, port: u16, target: u16) -> anyhow::Result<()> {
        self.calls.lock().unwrap().push(format!("on:{port}"));
        if *self.fail_on.lock().unwrap() {
            anyhow::bail!("fixture: Serve 등록 실패");
        }
        let mut config = self.config.lock().unwrap();
        config["TCP"][port.to_string()] = json!({"HTTPS":true});
        config["Web"][format!("mac-a.example.ts.net:{port}")] =
            json!({"Handlers":{"/":{"Proxy":format!("http://127.0.0.1:{target}")}}});
        Ok(())
    }
    async fn off(&self, port: u16) -> anyhow::Result<()> {
        self.calls.lock().unwrap().push(format!("off:{port}"));
        let mut config = self.config.lock().unwrap();
        config["TCP"].as_object_mut().unwrap().remove(&port.to_string());
        config["Web"].as_object_mut().unwrap().remove(&format!("mac-a.example.ts.net:{port}"));
        Ok(())
    }
}
fn post(h: &Host) -> PostInput {
    let input = PostInput {
        post_id: "00000000000000000000000000000001".into(),
        to: "user".into(),
        post_type: PostType::Notice,
        topic: "report".into(),
        title: "공유 보고".into(),
        body: format!("[질문]({OLD}/#question=q-0001) [{OLD}/v/cap/?a=1#x]"),
        needs_user: false,
        refs: vec![format!("{OLD}/v/cap/"), "https://other.example.ts.net:8443/".into()],
    };
    assert!(matches!(h.front_post("demo", input.clone(), "local:person").unwrap(), PostResult::Accepted { .. }));
    input
}
fn subscription(h: &Host) {
    h.db.add_device("device", "controller", "fixture-token", "기기").unwrap();
    h.db.push_put_at(
        &PushSub {
            endpoint: "https://push.example.com/fixture".into(),
            p256dh: "fixture".into(),
            auth: "fixture".into(),
            device: "device".into(),
            alerts: None,
        },
        None,
    )
    .unwrap();
}
async fn request(h: &Arc<Host>, authority: &str, path: &str) -> axum::response::Response {
    crate::web::router(h.clone())
        .oneshot(
            axum::http::Request::builder().uri(path).header("host", authority).body(axum::body::Body::empty()).unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn port_change_moves_stored_links_preserves_replay_and_auth_and_releases_old_port() {
    let (h, lab) = host();
    let input = post(&h);
    subscription(&h);
    let original = h.db.post_list("demo").unwrap()[0].clone();
    let local_sub = PushSub {
        endpoint: "https://push.example.com/loopback".into(),
        p256dh: "fixture".into(),
        auth: "fixture".into(),
        device: "device".into(),
        alerts: None,
    };
    h.db.push_put_at(&local_sub, Some("http://127.0.0.1:7000")).unwrap();
    let old_sub = PushSub { endpoint: "https://push.example.com/old".into(), ..local_sub.clone() };
    h.db.push_put_at(&old_sub, Some(OLD)).unwrap();
    let io = Fixture::new();
    let foreign = io.config.lock().unwrap()["Web"]["mac-a.example.ts.net:443"].clone();
    let status = h.change_port(&io, 7443, "local:person").await.unwrap();
    assert_eq!(status.url.as_deref(), Some(NEW));
    assert_eq!(*io.calls.lock().unwrap(), ["off:8443", "on:7443"]);
    assert!(io.config.lock().unwrap()["TCP"].get("8443").is_none());
    assert_eq!(io.config.lock().unwrap()["Web"]["mac-a.example.ts.net:443"], foreign);
    let moved = h.db.post_list("demo").unwrap().remove(0);
    assert_eq!(moved.input.body, input.body.replace(OLD, NEW));
    assert_eq!(moved.input.refs[0], format!("{NEW}/v/cap/"));
    assert_eq!(moved.input.refs[1], input.refs[1]);
    assert_eq!(moved.body_sha256, hex::encode(Sha256::digest(moved.input.body.as_bytes())));
    assert_eq!(moved.id, original.id);
    assert_eq!(
        h.front_post("demo", input.clone(), "local:person").unwrap(),
        PostResult::Accepted { id: original.id.clone() }
    );
    let subscriptions = h.db.push_list(None).unwrap();
    assert_eq!(subscriptions.len(), 1);
    assert_eq!(subscriptions[0].endpoint, local_sub.endpoint);
    assert!(h.db.device("device").unwrap().is_some());
    let inbox = h.db.inbox_list(None, true).unwrap().remove(0);
    assert_eq!(inbox.body, crate::post::body(&moved));
    let projection =
        std::fs::read_to_string(lab.0.join("projects/demo/inbox").join(format!("{}.md", inbox.id))).unwrap();
    assert!(projection.contains(NEW) && !projection.contains(OLD));
    let sql = rusqlite::Connection::open(lab.0.join("host.db")).unwrap();
    let saved: String =
        sql.query_row("SELECT data FROM web_link_originals WHERE kind='posts'", [], |r| r.get(0)).unwrap();
    assert_eq!(serde_json::from_str::<aaa_proto::PostEnvelope>(&saved).unwrap(), original);
    assert_eq!(
        request(&h, "mac-a.example.ts.net:8443", "/").await.status(),
        axum::http::StatusCode::MISDIRECTED_REQUEST
    );
    assert_eq!(
        request(&h, "mac-a.example.ts.net:7443", "/api/v1/me").await.status(),
        axum::http::StatusCode::UNAUTHORIZED
    );
    assert!(h.web_origin("mac-a.example.ts.net:7443").is_some());
    assert!(h.web_origin("mac-a.example.ts.net:443").is_none());
    assert!(h.pending_publication().unwrap().is_none());
    let status = h.change_port(&io, 7443, "local:person").await.unwrap();
    assert_eq!(status.url.as_deref(), Some(NEW));
    assert_eq!(io.calls.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn failed_transition_survives_restart_and_resumes_only_the_same_target() {
    let (h, lab) = host();
    let input = post(&h);
    subscription(&h);
    let io = Fixture::new();
    *io.fail_on.lock().unwrap() = true;
    assert_eq!(h.change_port(&io, 7443, "local:person").await.unwrap_err().code, ErrorCode::Unavailable);
    assert_eq!(h.db.post_list("demo").unwrap()[0].input.body, input.body);
    assert_eq!(h.db.push_list(None).unwrap().len(), 1);
    assert!(h.pending_publication().unwrap().is_some());
    h.publication_status(io.read().await.as_ref());
    assert!(h.public_origin().is_none(), "매핑 없음 확인 뒤 옛 포트 fallback 금지");
    let cfg = h.cfg.clone();
    drop(h);
    let h = Host::open(cfg, lab.0.clone()).unwrap();
    h.web_port.store(7000, std::sync::atomic::Ordering::Relaxed);
    assert_eq!(h.change_port(&io, 17443, "local:person").await.unwrap_err().code, ErrorCode::Conflict);
    assert_eq!(h.no_pending_publication().unwrap_err().code, ErrorCode::Conflict);
    *io.fail_on.lock().unwrap() = false;
    h.change_port(&io, 7443, "local:person").await.unwrap();
    assert_eq!(*io.calls.lock().unwrap(), ["off:8443", "on:7443", "on:7443"]);
    assert_eq!(h.db.post_list("demo").unwrap()[0].input.body, input.body.replace(OLD, NEW));
}

#[tokio::test]
async fn moving_a_question_body_preserves_its_ids_and_partial_answers() {
    let (h, _lab) = host();
    let input = PostInput {
        post_id: "00000000000000000000000000000002".into(),
        to: "user".into(),
        post_type: PostType::Request,
        topic: "user-action".into(),
        title: "두 질문".into(),
        body: format!(
            "## Questions\n### 1. 링크를 열까요?\n[보고]({OLD}/v/cap/)\n- A 연다 (추천) — 확인\n- B 보류 — 나중\n### 2. 이어 할까요?\n- A 진행 (추천) — 계속\n- B 보류 — 나중\n"
        ),
        needs_user: true,
        refs: vec![],
    };
    assert!(matches!(h.front_post("demo", input, "local:person").unwrap(), PostResult::Accepted { .. }));
    let before = h.user_requests("demo").unwrap().remove(0);
    let questions = before.questions.unwrap();
    assert_eq!(questions.items.len(), 2);
    let ids: Vec<_> = questions.items.iter().map(|q| q.id.clone().unwrap()).collect();
    let target = h.db.question_target(Some("demo"), &ids[0]).unwrap().unwrap();
    let answer = aaa_proto::QuestionAnswerView {
        delivery: None,
        comments: Vec::new(),
        picks: vec![Some(0)],
        text: None,
        summary: "1A".into(),
        by: "CLI".into(),
        at_ms: 10,
    };
    h.db.question_set_draft(&target, Some(&answer)).unwrap();
    h.change_port(&Fixture::new(), 7443, "local:person").await.unwrap();
    let after = h.user_requests("demo").unwrap().remove(0).questions.unwrap();
    assert_ne!(after.report_hash, questions.report_hash);
    assert_eq!(
        after.items.iter().map(|q| q.id.clone().unwrap()).collect::<Vec<_>>(),
        ids,
        "주소만 바꿀 때 질문 ID를 새로 만들지 않는다"
    );
    assert_eq!(after.draft.unwrap().picks, vec![Some(0), None]);
    let target = h.db.question_target(Some("demo"), &ids[0]).unwrap().unwrap();
    assert_eq!(target.revision, after.report_hash);
    assert_eq!(target.draft.unwrap(), answer);
}

#[tokio::test]
async fn foreign_mapping_and_unreadable_status_cannot_start_a_transition() {
    for port in [443, 8443] {
        let (h, _lab) = host();
        let io = Fixture::new();
        if port == 8443 {
            io.config.lock().unwrap()["Web"]["mac-a.example.ts.net:8443"]["Handlers"]["/"]["Proxy"] =
                json!("http://127.0.0.1:9000");
        }
        assert_eq!(
            h.change_port(&io, if port == 443 { 443 } else { 7443 }, "local:person").await.unwrap_err().code,
            ErrorCode::Conflict
        );
        assert!(io.calls.lock().unwrap().is_empty());
        assert!(h.pending_publication().unwrap().is_none());
    }
    let (h, _lab) = host();
    let io = Fixture::new();
    *io.config.lock().unwrap() = json!([]);
    assert_eq!(h.change_port(&io, 7443, "local:person").await.unwrap_err().code, ErrorCode::Unavailable);
    assert!(io.calls.lock().unwrap().is_empty());
    h.publication_status(io.read().await.as_ref());
    assert_eq!(h.public_origin().as_deref(), Some(OLD));
    h.publication_status(None);
    assert_eq!(h.public_origin().as_deref(), Some(OLD));
}

#[tokio::test]
async fn port_change_requires_a_person_at_the_local_cli() {
    let (h, _lab) = host();
    for (role, actor) in [
        (DeviceRole::Reader, "device:reader"),
        (DeviceRole::Controller, "device:controller"),
        (DeviceRole::Local, "local:cli@coordinator"),
    ] {
        assert_eq!(
            h.call(role, actor, FrontRequest::WebPublishPort { port: 7443 }).await.unwrap_err().code,
            ErrorCode::Forbidden
        );
    }
    assert!(h.pending_publication().unwrap().is_none());
}

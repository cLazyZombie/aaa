//! 사람이 요청한 공개 포트 변경. 잠시 끊어도 되며 옛 포트는 비운다. 실패 뒤에는 같은 명령으로 이어 한다.
use crate::{Host, err, tailscale};
use aaa_proto::{ErrorBody, ErrorCode, PublishState};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PortChange {
    pub id: String,
    pub old_port: u16,
    pub new_port: u16,
    pub old_origin: String,
    pub new_origin: String,
    pub web_port: u16,
}

trait Serve {
    fn read(&self) -> impl Future<Output = Option<tailscale::TsOutput>> + Send;
    fn on(&self, port: u16, target: u16) -> impl Future<Output = anyhow::Result<()>> + Send;
    fn off(&self, port: u16) -> impl Future<Output = anyhow::Result<()>> + Send;
}
struct Native;
impl Serve for Native {
    async fn read(&self) -> Option<tailscale::TsOutput> {
        tailscale::read().await
    }
    async fn on(&self, port: u16, target: u16) -> anyhow::Result<()> {
        tailscale::serve(port, target).await.map(|_| ())
    }
    async fn off(&self, port: u16) -> anyhow::Result<()> {
        tailscale::serve_off(port).await
    }
}
fn unavailable(error: impl std::fmt::Display) -> ErrorBody {
    err(ErrorCode::Unavailable, format!("{error}. 전환이 남아 있으면 같은 aaa web publish --port 명령으로 이어 한다"))
}
fn exact(out: Option<&tailscale::TsOutput>, web: u16, port: u16) -> Result<tailscale::Exact, ErrorBody> {
    tailscale::exact(out, web, port)
        .map_err(|e| err(if e.conflict { ErrorCode::Conflict } else { ErrorCode::Unavailable }, e.code))
}

impl Host {
    fn pending_publication(&self) -> Result<Option<PortChange>, ErrorBody> {
        self.db
            .meta("publish_change")
            .map_err(unavailable)?
            .map(|s| serde_json::from_str(&s).map_err(unavailable))
            .transpose()
    }
    pub(crate) fn no_pending_publication(&self) -> Result<(), ErrorBody> {
        if let Some(pending) = self.pending_publication()? {
            return Err(err(
                ErrorCode::Conflict,
                format!("공개 포트 전환이 남아 있다: aaa web publish --port {}", pending.new_port),
            ));
        }
        Ok(())
    }

    pub(crate) async fn publish_status(&self) -> PublishState {
        let _gate = self.publish_gate.lock().await;
        self.publication_status(Native.read().await.as_ref())
    }

    fn publication_status(&self, out: Option<&tailscale::TsOutput>) -> PublishState {
        let selected = self
            .db
            .meta("publish_selected_port")
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()
            .flatten()
            .and_then(|s| {
                s.parse::<u16>()
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
            });
        let plan = tailscale::installation_plan(out, self.web_port(), &self.cfg.web.publish_ports, selected);
        let known = plan.state == "running"
            && out
                .and_then(|o| o.serve.as_deref())
                .and_then(|s| {
                    serde_json::from_str::<serde_json::Value>(s)
                        .inspect_err(|error| {
                            aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                        })
                        .ok()
                })
                .is_some_and(|v| v.is_object() || v.is_null());
        // 없음도 관측한 상태다. origin 삭제와 관측 표지는 한 transaction으로 기록한다.
        let observed = !known
            || self
                .db
                .observe_publication(
                    plan.serving.as_ref().map(|(url, _)| url.to_ascii_lowercase()).as_deref(),
                    plan.dns_name.as_deref(),
                )
                .as_ref()
                .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
                .is_ok();
        let pending = self
            .pending_publication()
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()
            .flatten();
        PublishState {
            tailscale: plan.state.into(),
            dns_name: plan.dns_name,
            url: plan.serving.map(|(url, _)| url),
            owned_port: self
                .db
                .meta("publish_port")
                .inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                })
                .ok()
                .flatten()
                .and_then(|p| {
                    p.parse()
                        .inspect_err(|error| {
                            aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                        })
                        .ok()
                }),
            web_port: self.web_port(),
            message: if let Some(p) = pending {
                Some(format!("공개 포트 전환이 남아 있다: aaa web publish --port {}", p.new_port))
            } else {
                match plan.state {
                    "missing" => Some("tailscale CLI가 없다. 웹은 127.0.0.1에서만 열린다".into()),
                    "stopped" => Some("Tailscale이 꺼졌거나 로그인 전이다".into()),
                    _ if !known => Some("Tailscale Serve 상태를 읽지 못했다. 저장한 공개 주소를 유지한다".into()),
                    _ if !observed => Some("공개 주소 상태를 저장하지 못했다".into()),
                    _ if self.public_origin().is_none() => Some("아직 공개하지 않았다: aaa web publish".into()),
                    _ => None,
                }
            },
        }
    }

    pub(crate) async fn publish(&self) -> Result<PublishState, ErrorBody> {
        let _gate = self.publish_gate.lock().await;
        self.no_pending_publication()?;
        if self.web_port() == 0 {
            return Err(err(ErrorCode::BadRequest, "host.toml의 web.listen이 꺼져 있다"));
        }
        self.db.del_meta("publish_selected_port").map_err(unavailable)?;
        let out = Native.read().await;
        let plan = tailscale::plan(out.as_ref(), self.web_port(), &self.cfg.web.publish_ports);
        if plan.state != "running" {
            return Ok(self.publication_status(out.as_ref()));
        }
        if plan.serving.is_none() {
            let port = plan.free_port.ok_or_else(|| {
                err(ErrorCode::Unavailable, format!("후보 포트가 모두 쓰이고 있다: {:?}", plan.taken))
            })?;
            let before = exact(out.as_ref(), self.web_port(), port)?;
            Native.on(port, self.web_port()).await.map_err(unavailable)?;
            let after = exact(Native.read().await.as_ref(), self.web_port(), port)?;
            if !after.reused || !before.preserves(&after) {
                return Err(err(ErrorCode::Conflict, "Serve 공개 결과가 계획과 다르다"));
            }
            self.db.set_meta("publish_port", &port.to_string()).map_err(unavailable)?;
        }
        Ok(self.publication_status(Native.read().await.as_ref()))
    }

    pub(crate) async fn publish_off(&self) -> Result<PublishState, ErrorBody> {
        let _gate = self.publish_gate.lock().await;
        self.no_pending_publication()?;
        let port = self
            .db
            .meta("publish_port")
            .map_err(unavailable)?
            .and_then(|p| {
                p.parse()
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
            })
            .ok_or_else(|| err(ErrorCode::NotFound, "aaa가 만든 공개가 없다(다른 서비스의 매핑은 건드리지 않는다)"))?;
        let before = exact(Native.read().await.as_ref(), self.web_port(), port)?;
        if before.reused {
            Native.off(port).await.map_err(unavailable)?;
        }
        let out = Native.read().await;
        let after = exact(out.as_ref(), self.web_port(), port)?;
        if after.reused || !before.preserves(&after) {
            return Err(err(ErrorCode::Conflict, "Serve 공개 해제 결과가 계획과 다르다"));
        }
        self.db.del_meta("publish_port").map_err(unavailable)?;
        Ok(self.publication_status(out.as_ref()))
    }

    pub(crate) async fn publish_port_change(&self, port: u16, actor: &str) -> Result<PublishState, ErrorBody> {
        let _gate = self.publish_gate.lock().await;
        self.change_port(&Native, port, actor).await
    }

    async fn change_port(&self, io: &impl Serve, port: u16, actor: &str) -> Result<PublishState, ErrorBody> {
        if port == 0 || self.web_port() == 0 {
            return Err(err(ErrorCode::BadRequest, "공개 포트와 웹 listen은 1~65535여야 한다"));
        }
        let out = io.read().await;
        let target = exact(out.as_ref(), self.web_port(), port)?;
        let change = if let Some(pending) = self.pending_publication()? {
            if pending.new_port != port || pending.web_port != self.web_port() || pending.new_origin != target.url() {
                return Err(err(
                    ErrorCode::Conflict,
                    format!("남은 전환의 주소·listen을 유지한다: aaa web publish --port {}", pending.new_port),
                ));
            }
            pending
        } else {
            let old_port = self
                .db
                .meta("publish_port")
                .map_err(unavailable)?
                .and_then(|p| {
                    p.parse()
                        .inspect_err(|error| {
                            aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                        })
                        .ok()
                })
                .ok_or_else(|| err(ErrorCode::NotFound, "aaa가 만든 공개가 없다. 먼저 aaa web publish로 공개한다"))?;
            let old = exact(out.as_ref(), self.web_port(), old_port)?;
            if old_port == port && target.reused {
                return Ok(self.publication_status(out.as_ref()));
            }
            if target.reused {
                return Err(err(
                    ErrorCode::Conflict,
                    "새 포트에 이미 매핑이 있다. aaa가 소유하지 않은 공개는 바꾸지 않는다",
                ));
            }
            if let Some(origin) = self.public_origin()
                && origin != old.url()
            {
                return Err(err(
                    ErrorCode::Conflict,
                    "저장된 공개 주소와 aaa가 만든 포트가 다르다. aaa web status로 확인한다",
                ));
            }
            let change = PortChange {
                id: aaa_common::util::new_id(),
                old_port,
                new_port: port,
                old_origin: old.url(),
                new_origin: target.url(),
                web_port: self.web_port(),
            };
            self.db
                .set_meta("publish_change", &serde_json::to_string(&change).map_err(unavailable)?)
                .map_err(unavailable)?;
            change
        };
        // 옛 포트를 먼저 비운다. 실패/종료 뒤에도 journal은 남고 각 단계의 실제 상태부터 확인한다.
        if change.old_port != change.new_port {
            let before = exact(io.read().await.as_ref(), self.web_port(), change.old_port)?;
            if before.url() != change.old_origin {
                return Err(err(ErrorCode::Conflict, "옛 공개 주소가 바뀌었다"));
            }
            if before.reused {
                io.off(change.old_port).await.map_err(unavailable)?;
            }
            let after = exact(io.read().await.as_ref(), self.web_port(), change.old_port)?;
            if after.reused || !before.preserves(&after) {
                return Err(err(ErrorCode::Conflict, "옛 포트 해제 결과가 계획과 다르다"));
            }
        }
        let before = exact(io.read().await.as_ref(), self.web_port(), port)?;
        if !before.reused {
            io.on(port, self.web_port()).await.map_err(unavailable)?;
        }
        let out = io.read().await;
        let after = exact(out.as_ref(), self.web_port(), port)?;
        if !after.reused || !before.preserves(&after) || after.url() != change.new_origin {
            return Err(err(ErrorCode::Conflict, "새 포트 공개 결과가 계획과 다르다"));
        }
        let (moved, subscriptions) = self.db.finish_port_change(&change, actor).map_err(unavailable)?;
        for item in self.db.inbox_list(None, true).map_err(unavailable)? {
            self.inbox_publish(&item);
        }
        self.asks_bump();
        self.bump();
        let mut status = self.publication_status(out.as_ref());
        status.message = Some(format!(
            "저장 기록 {moved}건 이전, 옛 푸시 구독 {subscriptions}건 정리. 옛 외부 링크는 끊겼다. 새 주소에서 알림을 다시 켠다"
        ));
        Ok(status)
    }
}

#[cfg(test)]
mod tests {
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
        h.db.push_put(&PushSub {
            endpoint: "https://push.example.com/fixture".into(),
            p256dh: "fixture".into(),
            auth: "fixture".into(),
            device: "device".into(),
            alerts: None,
        })
        .unwrap();
    }
    async fn request(h: &Arc<Host>, authority: &str, path: &str) -> axum::response::Response {
        crate::web::router(h.clone())
            .oneshot(
                axum::http::Request::builder()
                    .uri(path)
                    .header("host", authority)
                    .body(axum::body::Body::empty())
                    .unwrap(),
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
        assert!(h.host_allowed("mac-a.example.ts.net:7443"));
        assert!(!h.host_allowed("mac-a.example.ts.net:443"));
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
}

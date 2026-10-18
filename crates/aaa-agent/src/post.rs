//! 세션 post: peer+token 판정 뒤 내구 outbox에 적고 post.v1 상대에게만 보낸다.
use std::path::PathBuf;
use std::time::Duration;

use crate::{Agent, hooks::HookMsg};
use aaa_common::{
    caller::{Caller, judge},
    util,
};
use aaa_proto::{AgentEvent, LinkMsg, PostInput, PostResult};
use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Pending {
    pub session: String,
    pub input: PostInput,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Receipt {
    input_sha: String,
    result: PostResult,
}
fn input_sha(input: &PostInput) -> String {
    use sha2::Digest;
    hex::encode(sha2::Sha256::digest(serde_json::to_vec(input).unwrap()))
}

pub fn valid_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit())
}

/// 옛 agent의 `ok`만인 답을 성공으로 오인하지 않는다.
pub async fn send(payload: serde_json::Value) -> Result<PostResult> {
    let out = crate::hooks::send("post", payload).await?;
    let text = out
        .as_deref()
        .and_then(|s| s.strip_prefix("post.v1 "))
        .ok_or_else(|| anyhow::anyhow!("이 agent는 post를 모른다(post.v1 필요)"))?;
    Ok(serde_json::from_str(text)?)
}

impl Agent {
    pub(crate) async fn reject_post(&self, msg: &HookMsg, reason: &str) {
        if *self.host_post.lock().unwrap() != Some(true) {
            return;
        }
        use sha2::Digest;
        let body = msg.payload.get("body").and_then(|v| v.as_str()).unwrap_or("");
        let id = msg.payload.get("post_id").and_then(|v| v.as_str()).filter(|v| valid_id(v)).unwrap_or("");
        self.send_host(&LinkMsg::Event(AgentEvent::PostRejected {
            session: msg.session.clone(),
            post_id: id.into(),
            bytes: body.len() as u64,
            sha256: hex::encode(sha2::Sha256::digest(body.as_bytes())),
            reason: reason.into(),
        }))
        .await;
    }
    fn post_out(&self) -> PathBuf {
        self.state.join("post/out")
    }
    fn post_receipt(&self, session: &str, id: &str) -> PathBuf {
        self.state.join("post/status").join(format!("{session}-{id}.json"))
    }
    pub(crate) async fn on_post(&self, msg: HookMsg, pid: Option<u32>) -> Result<String> {
        let panes: Vec<_> = self
            .sessions()
            .into_iter()
            .filter(|s| s.state.is_running())
            .filter_map(|s| s.pid.map(|p| (s.id, p)))
            .collect();
        ensure!(
            pid.is_some_and(|p| judge(p, &panes, &[]) == Caller::Pane(msg.session.clone())),
            "post 보낸 프로세스가 이 세션 안에 없다(peer pid)"
        );
        if *self.host_post.lock().unwrap() == Some(false) {
            bail!("이 host는 post를 모른다(post.v1 필요)");
        }
        let result = if let Some(id) = msg.payload.get("status").and_then(|v| v.as_str()) {
            ensure!(valid_id(id), "post ID 형식이 아니다");
            let receipt = self.post_receipt(&msg.session, id);
            if receipt.exists() {
                util::read_json::<Receipt>(&receipt)?.result
            } else {
                let p: Pending = util::read_json(&self.post_out().join(format!("{id}.json")))?;
                ensure!(p.session == msg.session, "자기 post만 볼 수 있다");
                PostResult::Queued { post_id: id.into() }
            }
        } else {
            let input: PostInput = serde_json::from_value(msg.payload)?;
            ensure!(valid_id(&input.post_id), "post ID 형식이 아니다");
            // host도 검사한다. 여기서는 디스크 상한을 넘기기 전에 큰 글을 막는다.
            ensure!(
                input.body.len() <= 65536 && input.title.chars().count() <= 120,
                "post 크기 한도(본문 64KiB·제목 120자)"
            );
            let p = Pending { session: msg.session.clone(), input };
            let id = p.input.post_id.clone();
            let rx = {
                let _gate = self.post_gate.lock().unwrap();
                let receipt = self.post_receipt(&p.session, &id);
                if receipt.exists() {
                    let receipt: Receipt = util::read_json(&receipt)?;
                    ensure!(receipt.input_sha == input_sha(&p.input), "이미 다른 글에 쓴 post ID다");
                    return Ok(format!("post.v1 {}", serde_json::to_string(&receipt.result)?));
                }
                util::ensure_private_dir(&self.post_out())?;
                let path = self.post_out().join(format!("{id}.json"));
                if path.exists() {
                    let old: Pending = util::read_json(&path)?;
                    ensure!(old.session == p.session && old.input == p.input, "이미 다른 글에 쓴 post ID다");
                } else {
                    let mut count = 0;
                    let mut bytes = 0;
                    for e in std::fs::read_dir(self.post_out())?.flatten() {
                        if let Ok(old) = util::read_json::<Pending>(&e.path())
                            && old.session == p.session
                        {
                            count += 1;
                            bytes += e.metadata()?.len();
                        }
                    }
                    let data = serde_json::to_vec(&p)?;
                    ensure!(
                        count < 20 && bytes + data.len() as u64 <= 1024 * 1024,
                        "post outbox 한도(세션당 20건·1MiB)"
                    );
                    util::write_private(&path, &data)?;
                }
                let (tx, rx) = tokio::sync::oneshot::channel();
                self.post_waiters.lock().unwrap().insert(id.clone(), tx);
                rx
            };
            if self.host_tx().is_none() || *self.host_post.lock().unwrap() != Some(true) {
                self.post_waiters.lock().unwrap().remove(&id);
                return Ok(format!("post.v1 {}", serde_json::to_string(&PostResult::Queued { post_id: id })?));
            }
            if self.host_tx().is_some() && *self.host_post.lock().unwrap() == Some(true) {
                self.send_host(&LinkMsg::Event(AgentEvent::Post { session: p.session, input: p.input })).await;
            }
            let result = tokio::time::timeout(Duration::from_millis(2500), rx)
                .await
                .ok()
                .and_then(Result::ok)
                .unwrap_or_else(|| PostResult::Queued { post_id: id.clone() });
            self.post_waiters.lock().unwrap().remove(&id);
            result
        };
        Ok(format!("post.v1 {}", serde_json::to_string(&result)?))
    }
    pub(crate) fn post_ack(&self, id: &str, result: PostResult) {
        if !valid_id(id) || *self.host_post.lock().unwrap() != Some(true) {
            return;
        }
        let _gate = self.post_gate.lock().unwrap();
        let path = self.post_out().join(format!("{id}.json"));
        let Ok(p) = util::read_json::<Pending>(&path) else { return };
        let receipt = self.post_receipt(&p.session, id);
        let stored = util::ensure_private_dir(receipt.parent().unwrap()).and_then(|_| {
            util::write_json(&receipt, &Receipt { input_sha: input_sha(&p.input), result: result.clone() })
        });
        if stored.is_err() {
            return;
        }
        let _ = std::fs::remove_file(path);
        if let Some(tx) = self.post_waiters.lock().unwrap().remove(id) {
            let _ = tx.send(result);
        }
    }
    pub(crate) async fn replay_posts(&self) {
        if *self.host_post.lock().unwrap() != Some(true) {
            return;
        }
        let Ok(entries) = std::fs::read_dir(self.post_out()) else {
            return;
        };
        let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
        paths.sort();
        for path in paths {
            if let Ok(p) = util::read_json::<Pending>(&path) {
                self.send_host(&LinkMsg::Event(AgentEvent::Post { session: p.session, input: p.input })).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Entry;
    use aaa_common::conn::{FrameRx, spawn_writer};
    use aaa_proto::{SessionInfo, SessionState, SpawnSpec};
    use aaa_session::files::{SessionPaths, SpecFile};
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    struct Lab(PathBuf);
    impl Drop for Lab {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn agent() -> (Arc<Agent>, Lab) {
        let path = std::env::temp_dir().join(format!("aap-{}", util::new_id()));
        let a = Agent::new(path.clone(), "m".into(), "mac-a".into(), true, None).unwrap();
        for (sid, pid) in [("s1", std::process::id()), ("s2", 0)] {
            let spec = SpecFile {
                id: sid.into(),
                token: "test-session-secret".into(),
                boot_id: "b".into(),
                created_at_ms: 1,
                label: None,
                colors: None,
                spec: SpawnSpec {
                    name: "test".into(),
                    argv: vec![],
                    cwd: None,
                    env: vec![],
                    cols: 80,
                    rows: 24,
                    role: None,
                    harness: None,
                    profile: None,
                    resolved_profile: None,
                },
            };
            let paths = SessionPaths::new(&path, sid);
            util::ensure_private_dir(paths.spec().parent().unwrap()).unwrap();
            util::write_json(&paths.spec(), &spec).unwrap();
            let info = SessionInfo {
                id: sid.into(),
                name: "test".into(),
                argv: vec![],
                cwd: None,
                pid: Some(pid),
                started_at_ms: 1,
                boot_id: "b".into(),
                cols: 80,
                rows: 24,
                state: SessionState::Running,
                role: None,
                label: None,
                harness: None,
            };
            a.sessions.lock().unwrap().insert(sid.into(), Entry { info, pump: None });
        }
        a.start_hooks().unwrap();
        (a, Lab(path))
    }
    fn input(n: u32) -> PostInput {
        PostInput {
            post_id: format!("{n:032x}"),
            to: "coordinator".into(),
            post_type: aaa_proto::PostType::Request,
            topic: "ci-red".into(),
            title: "시험".into(),
            body: "본문".into(),
            needs_user: false,
            refs: vec![],
        }
    }
    async fn hook(a: &Agent, sid: &str, token: &str, payload: serde_json::Value) -> String {
        let mut c = aaa_common::ipc::connect(&crate::hooks::socket(&a.state)).await.unwrap();
        c.write_all(
            &serde_json::to_vec(&HookMsg { session: sid.into(), token: token.into(), harness: "post".into(), payload })
                .unwrap(),
        )
        .await
        .unwrap();
        // Named Pipe는 쓰기 반쪽을 닫지 못한다. Unix에서도 연결을 열어 둔 채 같은 계약을 검사한다.
        let mut s = String::new();
        c.read_to_string(&mut s).await.unwrap();
        s
    }
    fn token() -> String {
        crate::hooks::hook_token("test-session-secret")
    }
    async fn next(rx: &mut FrameRx<tokio::io::DuplexStream>) -> LinkMsg {
        tokio::time::timeout(Duration::from_secs(3), rx.control()).await.unwrap().unwrap()
    }
    #[tokio::test]
    async fn post_peer_token_queue_limits_ack_and_replay() {
        let (a, _lab) = agent();
        let (host, peer) = tokio::io::duplex(1 << 20);
        a.set_host(Some(spawn_writer(host, aaa_proto::MAX_FRAME)), false);
        *a.host_post.lock().unwrap() = Some(true);
        let mut rx = FrameRx::new(peer, aaa_proto::MAX_FRAME);
        let bad = hook(&a, "s1", "forged", serde_json::to_value(input(1)).unwrap()).await;
        assert!(bad.contains("token"), "{bad}");
        assert!(
            matches!(next(&mut rx).await,LinkMsg::Event(AgentEvent::PostRejected { reason,.. }) if reason.contains("token"))
        );
        let wrong = hook(&a, "s2", &token(), serde_json::to_value(input(1)).unwrap()).await;
        assert!(wrong.contains("peer pid"), "{wrong}");
        assert!(
            matches!(next(&mut rx).await,LinkMsg::Event(AgentEvent::PostRejected { reason,.. }) if reason.contains("peer pid"))
        );
        assert!(!a.post_out().exists(), "인증 실패는 outbox에 넣지 않는다");
        a.set_host(None, false);
        for n in 1..=20 {
            let out = hook(&a, "s1", &token(), serde_json::to_value(input(n)).unwrap()).await;
            assert!(out.contains("queued"), "{out}");
        }
        let out = hook(&a, "s1", &token(), serde_json::to_value(input(21)).unwrap()).await;
        assert!(out.contains("outbox 한도"), "{out}");
        // agent 재시작 뒤 디스크 outbox를 그대로 읽는다. 실제 link frame의 내용과 Ack 뒤 상태를 본다.
        let path = a.state.clone();
        drop(a);
        let a = Agent::new(path, "m".into(), "mac-a".into(), true, None).unwrap();
        let (host, peer) = tokio::io::duplex(1 << 20);
        a.set_host(Some(spawn_writer(host, aaa_proto::MAX_FRAME)), false);
        *a.host_post.lock().unwrap() = Some(true);
        let mut rx = FrameRx::new(peer, aaa_proto::MAX_FRAME);
        a.replay_posts().await;
        for n in 1..=20 {
            let msg = next(&mut rx).await;
            assert!(
                matches!(msg,LinkMsg::Event(AgentEvent::Post { ref session,input:ref i }) if session=="s1" && i.post_id==format!("{n:032x}")),
                "{msg:?}"
            );
            let result = if n == 1 {
                PostResult::Refused { reason: "행렬 거부".into(), existing: None }
            } else {
                PostResult::Accepted { id: format!("p-{n:08}") }
            };
            a.clone()
                .handle(
                    "ack".into(),
                    aaa_proto::AgentRequest::PostAck { post_id: format!("{n:032x}"), result: result.clone() },
                )
                .await;
            assert_eq!(util::read_json::<Receipt>(&a.post_receipt("s1", &format!("{n:032x}"))).unwrap().result, result);
        }
        assert_eq!(std::fs::read_dir(a.post_out()).unwrap().count(), 0, "거부한 글도 다시 수거하지 않는다");
        a.replay_posts().await;
        // 같은 writer의 뒤따르는 Pong이 울타리다. 재전송한 글이 없음을 시간으로 짐작하지 않는다.
        a.send_host(&LinkMsg::Pong { seq: 42 }).await;
        assert_eq!(next(&mut rx).await, LinkMsg::Pong { seq: 42 });
        let mut changed = input(2);
        changed.body = "다른 글".into();
        let old = hook(&a, "s1", &token(), serde_json::to_value(changed).unwrap()).await;
        assert!(old.contains("이미 다른 글에 쓴 post ID"), "{old}");
        // JSON 봉투까지 더해 세션당 1MiB를 검사한다. 16건째는 건수 20보다 먼저 크기로 막힌다.
        for n in 30..45 {
            let mut large = input(n);
            large.body = "x".repeat(65536);
            assert!(hook(&a, "s1", &token(), serde_json::to_value(large).unwrap()).await.contains("queued"));
        }
        let mut large = input(45);
        large.body = "x".repeat(65536);
        assert!(hook(&a, "s1", &token(), serde_json::to_value(large).unwrap()).await.contains("outbox 한도"));
    }
    #[tokio::test]
    async fn post_needs_host_feature_and_does_not_emit_unknown_event() {
        let (a, _lab) = agent();
        let (host, peer) = tokio::io::duplex(1 << 20);
        a.set_host(Some(spawn_writer(host, aaa_proto::MAX_FRAME)), false);
        *a.host_post.lock().unwrap() = Some(false);
        let mut rx = FrameRx::new(peer, aaa_proto::MAX_FRAME);
        let out = hook(&a, "s1", &token(), serde_json::to_value(input(1)).unwrap()).await;
        assert!(out.contains("이 host는 post를 모른다"), "{out}");
        assert!(!a.post_out().exists());
        a.replay_posts().await;
        a.send_host(&LinkMsg::Pong { seq: 43 }).await;
        assert_eq!(next(&mut rx).await, LinkMsg::Pong { seq: 43 });
    }
}

//! `aa host`: host backend. 정본 목록·권한·Front API·웹을 갖고, 각 머신의 agent backend와 Agent link로 통신한다.
//!
//! - host는 자식 프로세스를 만들지 않는다. 예외: tailscale CLI, 개발 모드의 로컬 agent 기동(§4.2).
//! - pane 화면은 agent(session host)가 정본이다. host는 마지막 화면 mirror를 가져 agent가 끊겨도 읽기 전용으로 보여 준다.

pub mod db;
pub mod front;
pub mod link;
pub mod tailscale;
pub mod web;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use aa_config::{HostConfig, LocalAgentMode};
use aa_proto::{
    AgentRequest, AgentResponse, Connection, DeviceRole, ErrorBody, ErrorCode, FRONT_API, FrontRequest, FrontResponse,
    HostStatus, LayoutChange, LayoutNode, LeaseView, LinkMsg, MachineView, Outcome, PaneView, PendingEnroll,
    PublishState, ScreenText, SessionInfo, SessionState, SpawnSpec, Tab, TerminalChunk, Workspace,
};
use anyhow::{Context, Result};
use sha2::Digest;
use tokio::sync::{broadcast, oneshot, watch};

use crate::conn::FrameTx;
use crate::util;
use crate::vt::Vt;

pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

type Pending = Arc<Mutex<HashMap<String, oneshot::Sender<Outcome<AgentResponse>>>>>;

/// 연결된 agent 하나. 같은 머신이 다시 붙으면 generation이 바뀌고 이전 연결의 사건은 버린다.
#[derive(Clone)]
pub struct AgentConn {
    pub tx: FrameTx,
    pub pending: Pending,
    pub generation: u64,
    seq: Arc<AtomicU64>,
}

impl AgentConn {
    pub fn new(tx: FrameTx, generation: u64) -> Self {
        AgentConn { tx, pending: Arc::new(Mutex::new(HashMap::new())), generation, seq: Arc::new(AtomicU64::new(0)) }
    }

    pub async fn request(&self, body: AgentRequest) -> Result<AgentResponse, ErrorBody> {
        let id = format!("c{}", self.seq.fetch_add(1, Ordering::Relaxed));
        let (otx, orx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id.clone(), otx);
        let msg = LinkMsg::Req { id: id.clone(), deadline_ms: Some(REQUEST_TIMEOUT.as_millis() as u64), body };
        if self.tx.json(&msg).await.is_err() {
            self.pending.lock().unwrap().remove(&id);
            return Err(ErrorBody::new(ErrorCode::AgentDisconnected, "agent 연결이 끊겼다"));
        }
        match tokio::time::timeout(REQUEST_TIMEOUT, orx).await {
            Ok(Ok(o)) => o.into_result(),
            Ok(Err(_)) => Err(ErrorBody::new(ErrorCode::AgentDisconnected, "agent 연결이 끊겼다")),
            Err(_) => {
                self.pending.lock().unwrap().remove(&id);
                Err(ErrorBody::new(ErrorCode::Timeout, "agent 응답 시간 초과"))
            }
        }
    }

    /// 연결이 끝났을 때 대기 중인 요청을 모두 실패로 끝낸다(다시 보내지 않는다: §8.5 a).
    pub fn fail_all(&self) {
        let drained: Vec<_> = self.pending.lock().unwrap().drain().collect();
        for (_, tx) in drained {
            let _ = tx.send(Outcome::Err(ErrorBody::new(ErrorCode::AgentDisconnected, "agent 연결이 끊겼다")));
        }
    }
}

struct MachineRt {
    name: String,
    local: bool,
    os: Option<String>,
    boot_id: Option<String>,
    dev_mode: bool,
    conn: Option<AgentConn>,
    last_seen_ms: Option<u64>,
}

struct Mirror {
    vt: Vt,
    offset: u64,
    synced: bool,
}

/// 웹 터미널 등 구독자에게 보내는 pane 사건
#[derive(Clone, Debug)]
pub enum PaneEvent {
    Output(u64, Arc<Vec<u8>>),
    /// 화면을 snapshot으로 다시 받아야 한다
    Reset,
    /// 상태(끊김·종료·크기)가 바뀌었다
    State,
}

/// 웹 터미널 attach 결과: (사건 구독, replay, offset, cols, rows)
pub type Attach = (broadcast::Receiver<PaneEvent>, Vec<u8>, u64, u16, u16);

struct PaneRt {
    machine_id: String,
    info: SessionInfo,
    mirror: Option<Mirror>,
    events: broadcast::Sender<PaneEvent>,
    /// 조작 lease(대화형 front 연결 하나). 연결이 끊기거나 다른 front가 가져가면 바뀐다.
    lease: Option<FrontRef>,
}

/// 대화형 front 연결(TUI의 UDS 연결, 웹 터미널 WebSocket 하나)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrontRef {
    pub id: String,
    pub label: String,
}

pub const DEFAULT_WORKSPACE: &str = "main";

struct Inner {
    machines: HashMap<String, MachineRt>,
    panes: HashMap<String, PaneRt>,
    generation: u64,
}

struct PendingRt {
    machine_id: String,
    fingerprint: String,
    sas: String,
    since_ms: u64,
    attempts: u32,
    accept: Option<oneshot::Sender<()>>,
}

#[derive(Default)]
struct Enroll {
    open_until: Option<Instant>,
    pending: HashMap<String, PendingRt>,
}

#[derive(Default)]
struct Pairing {
    codes: Vec<(String, DeviceRole, Instant)>,
    failures: u32,
    locked_until: Option<Instant>,
}

pub struct Host {
    pub cfg: HostConfig,
    pub state: PathBuf,
    pub host_id: String,
    pub db: db::Db,
    pub identity: crate::tls::Identity,
    pub dev_mode: bool,
    /// 실제로 listen한 웹 포트(설정이 `:0`이면 bind 뒤에 정해진다)
    pub web_port: std::sync::atomic::AtomicU16,
    inner: Mutex<Inner>,
    pub changed: watch::Sender<u64>,
    enroll: Mutex<Enroll>,
    pairing: Mutex<Pairing>,
    pub agent_listen: Mutex<Option<String>>,
    pub web_listen: Mutex<Option<String>>,
    workspaces: Mutex<HashMap<String, Workspace>>,
    /// workspace가 바뀔 때(TUI가 구독한다)
    pub layout_events: broadcast::Sender<Workspace>,
    front_seq: AtomicU64,
}

pub fn token_hash(token: &str) -> String {
    hex::encode(sha2::Sha256::digest(token.as_bytes()))
}

fn err(code: ErrorCode, msg: impl Into<String>) -> ErrorBody {
    ErrorBody::new(code, msg)
}

impl Host {
    pub fn open(cfg: HostConfig, state: PathBuf) -> Result<Arc<Host>> {
        util::ensure_private_dir(&state)?;
        let db = db::Db::open(&state.join("host.db")).context("host DB")?;
        let host_id = match db.meta("host_id")? {
            Some(v) => v,
            None => {
                let v = util::new_id();
                db.set_meta("host_id", &v)?;
                v
            }
        };
        let identity = crate::tls::load_or_create(&state, "aa-host")?;
        let mut machines = HashMap::new();
        for m in db.machines()? {
            machines.insert(
                m.id.clone(),
                MachineRt {
                    name: m.name,
                    local: m.local,
                    os: m.os,
                    boot_id: m.boot_id,
                    dev_mode: false,
                    conn: None,
                    last_seen_ms: m.last_seen_ms,
                },
            );
        }
        let mut workspaces = HashMap::new();
        for (name, json) in db.workspaces()? {
            match serde_json::from_str::<Workspace>(&json) {
                Ok(w) => {
                    workspaces.insert(name, w);
                }
                Err(e) => tracing::warn!(%name, "workspace 기록을 읽지 못했다: {e}"),
            }
        }
        let web_port = cfg.web.listen.rsplit_once(':').and_then(|(_, p)| p.parse().ok()).unwrap_or(0);
        let dev_mode = cfg.local_agent.mode == LocalAgentMode::Child;
        Ok(Arc::new(Host {
            cfg,
            state,
            host_id,
            db,
            identity,
            dev_mode,
            web_port: std::sync::atomic::AtomicU16::new(web_port),
            inner: Mutex::new(Inner { machines, panes: HashMap::new(), generation: 0 }),
            changed: watch::channel(0).0,
            enroll: Mutex::new(Enroll::default()),
            pairing: Mutex::new(Pairing::default()),
            agent_listen: Mutex::new(None),
            web_listen: Mutex::new(None),
            workspaces: Mutex::new(workspaces),
            layout_events: broadcast::channel(64).0,
            front_seq: AtomicU64::new(0),
        }))
    }

    fn bump(&self) {
        self.changed.send_modify(|v| *v += 1);
    }

    // ───────────── agent 연결 ─────────────

    pub fn next_gen(&self) -> u64 {
        let mut i = self.inner.lock().unwrap();
        i.generation += 1;
        i.generation
    }

    /// hello를 받아들였다: 머신·pane 목록을 갱신하고 모든 세션을 구독한다.
    pub fn agent_connected(self: &Arc<Self>, hello: &aa_proto::AgentHello, local: bool, conn: AgentConn) {
        let _ = self.db.touch_machine(&hello.machine_id, &hello.os, &hello.boot_id);
        let subscribe: Vec<(String, Option<u64>)>;
        {
            let mut i = self.inner.lock().unwrap();
            if let Some(old) = i.machines.get(&hello.machine_id).and_then(|m| m.conn.clone()) {
                old.fail_all();
            }
            let m = i.machines.entry(hello.machine_id.clone()).or_insert(MachineRt {
                name: hello.machine_name.clone(),
                local,
                os: None,
                boot_id: None,
                dev_mode: false,
                conn: None,
                last_seen_ms: None,
            });
            m.os = Some(hello.os.clone());
            m.boot_id = Some(hello.boot_id.clone());
            m.dev_mode = hello.dev_mode;
            m.conn = Some(conn);
            m.last_seen_ms = Some(util::now_ms());
            let ids: std::collections::HashSet<_> = hello.sessions.iter().map(|s| s.id.clone()).collect();
            i.panes.retain(|id, p| p.machine_id != hello.machine_id || ids.contains(id));
            for s in &hello.sessions {
                let p = i.panes.entry(s.id.clone()).or_insert_with(|| PaneRt {
                    machine_id: hello.machine_id.clone(),
                    info: s.clone(),
                    mirror: None,
                    events: broadcast::channel(1024).0,
                    lease: None,
                });
                p.info = s.clone();
                if let Some(m) = p.mirror.as_mut() {
                    m.synced = false;
                }
                let _ = p.events.send(PaneEvent::State);
            }
            subscribe = hello
                .sessions
                .iter()
                .map(|s| (s.id.clone(), i.panes.get(&s.id).and_then(|p| p.mirror.as_ref()).map(|m| m.offset)))
                .collect();
        }
        self.bump();
        for (sid, from) in subscribe {
            self.spawn_subscribe(hello.machine_id.clone(), sid, from);
        }
    }

    pub fn agent_disconnected(&self, machine_id: &str, generation: u64) {
        let mut i = self.inner.lock().unwrap();
        let Some(m) = i.machines.get_mut(machine_id) else { return };
        if m.conn.as_ref().map(|c| c.generation) != Some(generation) {
            return; // 이미 새 연결로 바뀌었다
        }
        if let Some(c) = m.conn.take() {
            c.fail_all();
        }
        m.last_seen_ms = Some(util::now_ms());
        for p in i.panes.values_mut().filter(|p| p.machine_id == machine_id) {
            if let Some(mm) = p.mirror.as_mut() {
                mm.synced = false;
            }
            let _ = p.events.send(PaneEvent::State);
        }
        drop(i);
        self.bump();
    }

    fn conn_of(&self, machine_id: &str) -> Option<AgentConn> {
        self.inner.lock().unwrap().machines.get(machine_id).and_then(|m| m.conn.clone())
    }

    fn spawn_subscribe(self: &Arc<Self>, machine_id: String, session: String, from: Option<u64>) {
        let me = self.clone();
        tokio::spawn(async move {
            let Some(conn) = me.conn_of(&machine_id) else { return };
            if let Err(e) = conn.request(AgentRequest::Subscribe { session: session.clone(), from }).await {
                tracing::debug!(%session, "구독 실패: {e}");
            }
        });
    }

    /// 링크 reader가 순서대로 부른다: 구독 응답은 출력 chunk보다 먼저 mirror에 반영된다.
    pub fn on_subscribed(&self, session: &str, snapshot: Option<aa_proto::Snapshot>, offset: u64) {
        let mut i = self.inner.lock().unwrap();
        let Some(p) = i.panes.get_mut(session) else { return };
        match snapshot {
            // 끝난 세션인데 agent에 마지막 화면 파일이 없다(재부팅 등으로 session host가 기록 없이 사라짐):
            // host가 가진 마지막 화면을 그대로 둔다(§4.4 「마지막 snapshot과 tail」).
            Some(s) if s.replay.0.is_empty() && !p.info.state.is_running() && p.mirror.is_some() => {
                if let Some(m) = p.mirror.as_mut() {
                    m.synced = true;
                }
            }
            Some(s) => {
                let mut vt = Vt::new(s.rows, s.cols);
                vt.feed(&s.replay.0);
                p.mirror = Some(Mirror { vt, offset: s.offset, synced: true });
                let _ = p.events.send(PaneEvent::Reset);
            }
            None => match p.mirror.as_mut() {
                Some(m) if m.offset == offset => m.synced = true,
                _ => tracing::warn!(%session, "구독 offset이 mirror와 다르다"),
            },
        }
    }

    pub fn on_chunk(self: &Arc<Self>, machine_id: &str, c: TerminalChunk) {
        let resync = {
            let mut i = self.inner.lock().unwrap();
            let Some(p) = i.panes.get_mut(&c.session) else { return };
            let Some(m) = p.mirror.as_mut() else { return };
            if !m.synced {
                return;
            }
            let end = c.offset + c.data.len() as u64;
            if end <= m.offset {
                return;
            }
            if c.offset > m.offset {
                m.synced = false;
                Some(m.offset)
            } else {
                let data = c.data[(m.offset - c.offset) as usize..].to_vec();
                let start = m.offset;
                m.vt.feed(&data);
                m.offset = end;
                let _ = p.events.send(PaneEvent::Output(start, Arc::new(data)));
                None
            }
        };
        if let Some(from) = resync {
            tracing::warn!(session = %c.session, "출력에 빈틈: 다시 구독한다");
            self.spawn_subscribe(machine_id.to_string(), c.session, Some(from));
        }
    }

    pub fn on_session(self: &Arc<Self>, machine_id: &str, info: SessionInfo, started: bool) {
        {
            let mut i = self.inner.lock().unwrap();
            let p = i.panes.entry(info.id.clone()).or_insert_with(|| PaneRt {
                machine_id: machine_id.to_string(),
                info: info.clone(),
                mirror: None,
                events: broadcast::channel(1024).0,
                lease: None,
            });
            if let Some(m) = p.mirror.as_mut()
                && (p.info.cols, p.info.rows) != (info.cols, info.rows)
            {
                m.vt.resize(info.rows, info.cols);
            }
            p.info = info.clone();
            let _ = p.events.send(PaneEvent::State);
        }
        self.bump();
        if started {
            self.spawn_subscribe(machine_id.to_string(), info.id, None);
        }
    }

    pub fn on_resync(self: &Arc<Self>, machine_id: &str, session: &str) {
        let from = {
            let mut i = self.inner.lock().unwrap();
            i.panes.get_mut(session).and_then(|p| p.mirror.as_mut()).map(|m| {
                m.synced = false;
                m.offset
            })
        };
        self.spawn_subscribe(machine_id.to_string(), session.to_string(), from);
    }

    // ───────────── 보기 ─────────────

    pub fn machine_views(&self) -> Vec<MachineView> {
        let i = self.inner.lock().unwrap();
        let mut v: Vec<_> = i
            .machines
            .iter()
            .map(|(id, m)| MachineView {
                id: id.clone(),
                name: m.name.clone(),
                local: m.local,
                connection: if m.conn.is_some() { Connection::Connected } else { Connection::Disconnected },
                os: m.os.clone(),
                boot_id: m.boot_id.clone(),
                dev_mode: m.dev_mode,
                last_seen_ms: m.last_seen_ms,
            })
            .collect();
        v.sort_by(|a, b| b.local.cmp(&a.local).then(a.name.cmp(&b.name)));
        v
    }

    fn pane_view(i: &Inner, id: &str, p: &PaneRt, role: DeviceRole) -> PaneView {
        let m = i.machines.get(&p.machine_id);
        let connected = m.is_some_and(|m| m.conn.is_some());
        PaneView {
            id: id.to_string(),
            machine_id: p.machine_id.clone(),
            machine: m.map(|m| m.name.clone()).unwrap_or_default(),
            name: p.info.name.clone(),
            argv: p.info.argv.clone(),
            cwd: p.info.cwd.clone(),
            state: p.info.state.clone(),
            connection: if connected { Connection::Connected } else { Connection::Disconnected },
            readonly: !connected || !p.info.state.is_running() || !role.can_control(),
            cols: p.info.cols,
            rows: p.info.rows,
            lease: p.lease.as_ref().map(|l| LeaseView { front: l.id.clone(), label: l.label.clone() }),
        }
    }

    pub fn pane_views(&self, role: DeviceRole) -> Vec<PaneView> {
        let i = self.inner.lock().unwrap();
        let mut v: Vec<_> = i.panes.iter().map(|(id, p)| Self::pane_view(&i, id, p, role)).collect();
        v.sort_by(|a, b| a.machine.cmp(&b.machine).then(a.id.cmp(&b.id)));
        v
    }

    pub fn pane_view_of(&self, id: &str, role: DeviceRole) -> Option<PaneView> {
        let i = self.inner.lock().unwrap();
        i.panes.get(id).map(|p| Self::pane_view(&i, id, p, role))
    }

    /// 웹 터미널 attach: 사건 구독과 snapshot을 같은 잠금 안에서 잡아 빈틈이 없게 한다.
    pub fn attach_pane(&self, id: &str) -> Option<Attach> {
        let i = self.inner.lock().unwrap();
        let p = i.panes.get(id)?;
        let rx = p.events.subscribe();
        let (replay, offset, rows, cols) = match &p.mirror {
            Some(m) => {
                let (rows, cols) = m.vt.size();
                (m.vt.replay(), m.offset, rows, cols)
            }
            None => (Vec::new(), 0, p.info.rows, p.info.cols),
        };
        Some((rx, replay, offset, cols, rows))
    }

    fn resolve_pane(&self, prefix: &str) -> Result<(String, String), ErrorBody> {
        let i = self.inner.lock().unwrap();
        if let Some(p) = i.panes.get(prefix) {
            return Ok((prefix.to_string(), p.machine_id.clone()));
        }
        let up = prefix.to_ascii_uppercase();
        let mut hits: Vec<_> = i.panes.iter().filter(|(id, _)| prefix.len() >= 4 && id.starts_with(&up)).collect();
        if hits.is_empty() {
            // ID가 아니면 이름: 실행 중인 것을 먼저, 없으면 끝난 것까지
            let named: Vec<_> = i.panes.iter().filter(|(_, p)| p.info.name == prefix).collect();
            let running: Vec<_> = named.iter().copied().filter(|(_, p)| p.info.state.is_running()).collect();
            hits = if running.is_empty() { named } else { running };
        }
        match hits.as_slice() {
            [(id, p)] => Ok(((*id).clone(), p.machine_id.clone())),
            [] => Err(err(ErrorCode::NotFound, format!("pane이 없다: {prefix}"))),
            _ => Err(err(ErrorCode::BadRequest, format!("pane ID가 모호하다: {prefix}"))),
        }
    }

    fn resolve_machine(&self, name: Option<&str>) -> Result<String, ErrorBody> {
        let i = self.inner.lock().unwrap();
        let found = match name {
            Some(n) => i.machines.iter().find(|(id, m)| m.name == n || id.as_str() == n),
            None => i.machines.iter().find(|(_, m)| m.local),
        };
        found.map(|(id, _)| id.clone()).ok_or_else(|| {
            err(
                ErrorCode::NotFound,
                match name {
                    Some(n) => format!("머신이 없다: {n}"),
                    None => "로컬 agent가 없다. --machine으로 머신을 고른다".into(),
                },
            )
        })
    }

    fn live_conn(&self, machine_id: &str) -> Result<AgentConn, ErrorBody> {
        self.conn_of(machine_id).ok_or_else(|| err(ErrorCode::AgentDisconnected, "agent와 연결이 끊겨 있다(읽기 전용)"))
    }

    fn mirror_text(&self, id: &str) -> Option<ScreenText> {
        let i = self.inner.lock().unwrap();
        i.panes.get(id).and_then(|p| p.mirror.as_ref()).map(|m| m.vt.text(id))
    }

    // ───────────── Front API ─────────────

    pub async fn call(
        self: &Arc<Self>,
        role: DeviceRole,
        actor: &str,
        req: FrontRequest,
    ) -> Result<FrontResponse, ErrorBody> {
        self.call_from(role, actor, None, req).await
    }

    /// `front`는 대화형 연결(TUI·웹 터미널)일 때만 준다. 조작 lease는 이 연결에 묶인다.
    pub async fn call_from(
        self: &Arc<Self>,
        role: DeviceRole,
        actor: &str,
        front: Option<&FrontRef>,
        req: FrontRequest,
    ) -> Result<FrontResponse, ErrorBody> {
        if req.local_only() && role != DeviceRole::Local {
            return Err(err(ErrorCode::Forbidden, "이 머신의 CLI에서만 할 수 있다"));
        }
        if req.needs_control() && !role.can_control() {
            return Err(err(ErrorCode::Forbidden, "보기 권한으로는 조작할 수 없다"));
        }
        match req {
            FrontRequest::Status => Ok(FrontResponse::Status(HostStatus {
                host_id: self.host_id.clone(),
                software: util::software(),
                proto: FRONT_API,
                dev_mode: self.dev_mode,
                agent_listen: self.agent_listen.lock().unwrap().clone(),
                web_listen: self.web_listen.lock().unwrap().clone(),
                machines: self.machine_views(),
            })),
            FrontRequest::Machines => Ok(FrontResponse::Machines { machines: self.machine_views() }),
            FrontRequest::Panes => Ok(FrontResponse::Panes { panes: self.pane_views(role) }),
            FrontRequest::PaneRun { machine, name, argv, cwd, cols, rows } => {
                let mid = self.resolve_machine(machine.as_deref())?;
                let spec = SpawnSpec {
                    name: name.unwrap_or_default(),
                    argv,
                    cwd,
                    env: vec![],
                    cols: cols.unwrap_or(120),
                    rows: rows.unwrap_or(32),
                };
                let id = self.spawn_on(&mid, spec, actor).await?;
                self.pane_view_of(&id, role)
                    .map(FrontResponse::Pane)
                    .ok_or_else(|| err(ErrorCode::Internal, "pane 목록에 없다"))
            }
            FrontRequest::PaneRead { pane } => {
                let (id, mid) = self.resolve_pane(&pane)?;
                let running = self.pane_view_of(&id, role).is_some_and(|p| p.state == SessionState::Running);
                if let (Some(conn), true) = (self.conn_of(&mid), running)
                    && let Ok(AgentResponse::Screen(s)) =
                        conn.request(AgentRequest::Screen { session: id.clone() }).await
                {
                    return Ok(FrontResponse::Screen(s));
                }
                // 끊겼거나 끝났다: 마지막 화면(mirror)
                self.mirror_text(&id)
                    .map(FrontResponse::Screen)
                    .ok_or_else(|| err(ErrorCode::AgentDisconnected, "화면 기록이 없다"))
            }
            FrontRequest::PaneSend { pane, data } => {
                let (id, mid) = self.resolve_pane(&pane)?;
                self.check_running(&id)?;
                let conn = self.live_conn(&mid)?;
                self.db.audit(actor, "pane_send", &format!("{id} {} bytes", data.0.len()));
                conn.request(AgentRequest::Input { session: id, data }).await.map(|_| FrontResponse::Done)
            }
            FrontRequest::PaneResize { pane, cols, rows } => {
                let (id, mid) = self.resolve_pane(&pane)?;
                self.check_running(&id)?;
                // PTY 크기는 lease 보유자가 정한다. 한 번 보내는 CLI는 아무도 조작하지 않을 때만 바꾼다.
                match front {
                    Some(f) => self.lease_acquire(&id, f)?,
                    None => self.lease_free(&id)?,
                }
                let conn = self.live_conn(&mid)?;
                conn.request(AgentRequest::Resize { session: id, cols, rows }).await.map(|_| FrontResponse::Done)
            }
            FrontRequest::PaneInput { pane, data } => {
                let f = front.ok_or_else(|| {
                    err(ErrorCode::BadRequest, "대화형 front 연결에서만 쓴다(한 번 보내기는 pane_send)")
                })?;
                let (id, mid) = self.resolve_pane(&pane)?;
                self.check_running(&id)?;
                let conn = self.live_conn(&mid)?;
                self.lease_acquire(&id, f)?;
                conn.request(AgentRequest::Input { session: id, data }).await.map(|_| FrontResponse::Done)
            }
            FrontRequest::PaneTake { pane } => {
                let f = front.ok_or_else(|| err(ErrorCode::BadRequest, "대화형 front 연결에서만 쓴다"))?;
                let (id, _) = self.resolve_pane(&pane)?;
                self.check_running(&id)?;
                self.lease_set(&id, Some(f.clone()));
                self.db.audit(actor, "pane_take", &id);
                Ok(FrontResponse::Done)
            }
            FrontRequest::PaneRelease { pane } => {
                let f = front.ok_or_else(|| err(ErrorCode::BadRequest, "대화형 front 연결에서만 쓴다"))?;
                let (id, _) = self.resolve_pane(&pane)?;
                let mine = self.inner.lock().unwrap().panes.get(&id).is_some_and(|p| p.lease.as_ref() == Some(f));
                if mine {
                    self.lease_set(&id, None);
                }
                Ok(FrontResponse::Done)
            }
            FrontRequest::PaneResume { pane } => self.resume(&pane, role, actor).await,
            FrontRequest::PaneForget { pane } => {
                let (id, mid) = self.resolve_pane(&pane)?;
                self.forget(&id, &mid, actor).await?;
                Ok(FrontResponse::Done)
            }
            FrontRequest::PaneWatch { .. } | FrontRequest::PaneUnwatch { .. } => {
                Err(err(ErrorCode::BadRequest, "화면 구독은 같은 머신 front 연결(aa attach)에서만 쓴다"))
            }
            FrontRequest::Workspace { name } => {
                Ok(FrontResponse::Workspace(self.workspace(name.as_deref().unwrap_or(DEFAULT_WORKSPACE))))
            }
            FrontRequest::LayoutApply { workspace, change } => {
                let name = workspace.unwrap_or_else(|| DEFAULT_WORKSPACE.to_string());
                self.layout_apply(&name, change, actor).await.map(FrontResponse::Workspace)
            }
            FrontRequest::PaneKill { pane } => {
                let (id, mid) = self.resolve_pane(&pane)?;
                let conn = self.live_conn(&mid)?;
                self.db.audit(actor, "pane_kill", &id);
                conn.request(AgentRequest::Kill { session: id }).await.map(|_| FrontResponse::Done)
            }
            FrontRequest::EnrollOpen { minutes } => {
                let minutes = minutes.clamp(1, 60);
                self.enroll.lock().unwrap().open_until =
                    Some(Instant::now() + Duration::from_secs(minutes as u64 * 60));
                self.db.audit(actor, "enroll_open", &format!("{minutes}m"));
                Ok(FrontResponse::Done)
            }
            FrontRequest::EnrollPending => {
                let e = self.enroll.lock().unwrap();
                let pending = e
                    .pending
                    .iter()
                    .map(|(name, p)| PendingEnroll {
                        name: name.clone(),
                        machine_id: p.machine_id.clone(),
                        fingerprint_prefix: p.fingerprint[..16].to_string(),
                        since_ms: p.since_ms,
                    })
                    .collect();
                Ok(FrontResponse::Pending { pending })
            }
            FrontRequest::EnrollApprove { name, code } => self.approve(&name, &code, actor),
            FrontRequest::MachineRevoke { name } => {
                let id = self.db.revoke_machine(&name).map_err(|e| err(ErrorCode::Internal, e.to_string()))?;
                let Some(id) = id else { return Err(err(ErrorCode::NotFound, format!("머신이 없다: {name}"))) };
                let conn = {
                    let mut i = self.inner.lock().unwrap();
                    i.panes.retain(|_, p| p.machine_id != id);
                    i.machines.remove(&id).and_then(|m| m.conn)
                };
                if let Some(c) = conn {
                    let _ = c.tx.json(&LinkMsg::Reject(err(ErrorCode::NotEnrolled, "등록이 폐기됐다"))).await;
                    c.fail_all();
                }
                self.db.audit(actor, "machine_revoke", &name);
                self.bump();
                Ok(FrontResponse::Done)
            }
            FrontRequest::WebPair { role: r } => {
                if r == DeviceRole::Local {
                    return Err(err(ErrorCode::BadRequest, "reader 또는 controller만 줄 수 있다"));
                }
                let code = format!("{:06}", rand::random_range(0..1_000_000u32));
                let mut p = self.pairing.lock().unwrap();
                let now = Instant::now();
                p.codes.retain(|(_, _, exp)| *exp > now);
                if p.codes.len() >= 5 {
                    return Err(err(ErrorCode::Unavailable, "아직 쓰지 않은 pairing 코드가 너무 많다"));
                }
                p.codes.push((code.clone(), r, now + Duration::from_secs(300)));
                self.db.audit(actor, "web_pair_code", &format!("{r:?}"));
                Ok(FrontResponse::PairCode { code, role: r, expires_in_s: 300 })
            }
            FrontRequest::WebStatus => Ok(FrontResponse::Publish(self.publish_status().await)),
            FrontRequest::WebPublish => {
                self.db.audit(actor, "web_publish", "");
                self.publish().await.map(FrontResponse::Publish)
            }
            FrontRequest::WebPublishOff => {
                self.db.audit(actor, "web_publish_off", "");
                self.publish_off().await.map(FrontResponse::Publish)
            }
        }
    }

    /// agent에 세션을 띄우고 pane 목록에 넣는다. 새 pane ID를 돌려준다.
    async fn spawn_on(self: &Arc<Self>, mid: &str, spec: SpawnSpec, actor: &str) -> Result<String, ErrorBody> {
        let conn = self.live_conn(mid)?;
        self.db.audit(actor, "pane_run", &serde_json::to_string(&spec).unwrap_or_default());
        match conn.request(AgentRequest::Spawn(spec)).await? {
            AgentResponse::Session(info) => {
                let id = info.id.clone();
                self.on_session(mid, info, false);
                Ok(id)
            }
            other => Err(err(ErrorCode::Internal, format!("예상하지 못한 응답: {other:?}"))),
        }
    }

    // ───────────── 조작 lease(§4.5) ─────────────

    pub fn new_front(&self, label: String) -> FrontRef {
        FrontRef { id: format!("f{}", self.front_seq.fetch_add(1, Ordering::Relaxed) + 1), label }
    }

    fn lease_set(&self, id: &str, to: Option<FrontRef>) {
        {
            let mut i = self.inner.lock().unwrap();
            let Some(p) = i.panes.get_mut(id) else { return };
            if p.lease == to {
                return;
            }
            p.lease = to;
            let _ = p.events.send(PaneEvent::State);
        }
        self.bump();
    }

    /// 비어 있으면 이 front가 갖고, 이미 가졌으면 그대로, 다른 front가 가졌으면 거부한다.
    fn lease_acquire(&self, id: &str, f: &FrontRef) -> Result<(), ErrorBody> {
        let holder = self.inner.lock().unwrap().panes.get(id).and_then(|p| p.lease.clone());
        match holder {
            Some(h) if h.id == f.id => Ok(()),
            Some(h) => Err(err(ErrorCode::LeaseHeld, format!("{}이 조작 중이다(가져오려면 take)", h.label))),
            None => {
                self.lease_set(id, Some(f.clone()));
                Ok(())
            }
        }
    }

    fn lease_free(&self, id: &str) -> Result<(), ErrorBody> {
        match self.inner.lock().unwrap().panes.get(id).and_then(|p| p.lease.clone()) {
            Some(h) => Err(err(ErrorCode::LeaseHeld, format!("{}이 조작 중이다", h.label))),
            None => Ok(()),
        }
    }

    /// front 연결이 끝났다: 그 연결의 lease를 모두 푼다.
    pub fn front_gone(&self, front_id: &str) {
        let freed: Vec<String> = {
            let i = self.inner.lock().unwrap();
            i.panes
                .iter()
                .filter(|(_, p)| p.lease.as_ref().is_some_and(|l| l.id == front_id))
                .map(|(k, _)| k.clone())
                .collect()
        };
        for id in freed {
            self.lease_set(&id, None);
        }
    }

    // ───────────── 재개·정리 ─────────────

    /// 끝난 pane을 같은 spec(이름·argv·cwd·크기)으로 다시 띄우고 layout 자리를 새 pane으로 바꾼 뒤 옛 기록을 지운다.
    async fn resume(self: &Arc<Self>, pane: &str, role: DeviceRole, actor: &str) -> Result<FrontResponse, ErrorBody> {
        let (id, mid) = self.resolve_pane(pane)?;
        let info = self
            .inner
            .lock()
            .unwrap()
            .panes
            .get(&id)
            .map(|p| p.info.clone())
            .ok_or_else(|| err(ErrorCode::NotFound, "pane이 없다"))?;
        if info.state.is_running() {
            return Err(err(ErrorCode::BadRequest, "실행 중인 pane은 재개할 수 없다"));
        }
        let spec = SpawnSpec {
            name: info.name.clone(),
            argv: info.argv.clone(),
            cwd: info.cwd.clone(),
            env: vec![],
            cols: info.cols,
            rows: info.rows,
        };
        let new_id = self.spawn_on(&mid, spec, actor).await?;
        self.db.audit(actor, "pane_resume", &format!("{id} -> {new_id}"));
        {
            let mut ws = self.workspaces.lock().unwrap();
            let changed: Vec<Workspace> =
                ws.values_mut().filter_map(|w| crate::layout::ws_replace(w, &id, &new_id).then(|| w.clone())).collect();
            drop(ws);
            for w in changed {
                self.save_workspace(&w);
            }
        }
        if let Err(e) = self.forget(&id, &mid, actor).await {
            tracing::warn!(pane = %id, "재개 뒤 옛 기록 정리 실패: {}", e.message);
        }
        self.pane_view_of(&new_id, role)
            .map(FrontResponse::Pane)
            .ok_or_else(|| err(ErrorCode::Internal, "pane 목록에 없다"))
    }

    async fn forget(self: &Arc<Self>, id: &str, mid: &str, actor: &str) -> Result<(), ErrorBody> {
        if self.inner.lock().unwrap().panes.get(id).is_some_and(|p| p.info.state.is_running()) {
            return Err(err(ErrorCode::BadRequest, "실행 중인 pane은 지울 수 없다"));
        }
        let conn = self.live_conn(mid)?;
        conn.request(AgentRequest::Forget { session: id.to_string() }).await?;
        self.inner.lock().unwrap().panes.remove(id);
        let changed: Vec<Workspace> = {
            let mut ws = self.workspaces.lock().unwrap();
            ws.values_mut().filter_map(|w| crate::layout::ws_remove(w, id).then(|| w.clone())).collect()
        };
        for w in changed {
            self.save_workspace(&w);
        }
        self.db.audit(actor, "pane_forget", id);
        self.bump();
        Ok(())
    }

    // ───────────── workspace layout(host 정본) ─────────────

    pub fn workspace(&self, name: &str) -> Workspace {
        self.workspaces
            .lock()
            .unwrap()
            .get(name)
            .cloned()
            .unwrap_or_else(|| Workspace { name: name.to_string(), tabs: vec![] })
    }

    fn save_workspace(&self, w: &Workspace) {
        if let Err(e) = self.db.save_workspace(&w.name, &serde_json::to_string(w).unwrap_or_default()) {
            tracing::warn!(workspace = %w.name, "workspace 저장 실패: {e}");
        }
        self.workspaces.lock().unwrap().insert(w.name.clone(), w.clone());
        let _ = self.layout_events.send(w.clone());
    }

    async fn layout_apply(
        self: &Arc<Self>,
        name: &str,
        change: LayoutChange,
        actor: &str,
    ) -> Result<Workspace, ErrorBody> {
        let pane_machine = |id: &str| self.inner.lock().unwrap().panes.get(id).map(|p| p.machine_id.clone());
        let mut w = self.workspace(name);
        match change {
            LayoutChange::NewTab { machine, name: pname, argv, cwd } => {
                let mid = self.resolve_machine(machine.as_deref())?;
                let spec = SpawnSpec { name: pname.unwrap_or_default(), argv, cwd, env: vec![], cols: 120, rows: 32 };
                let id = self.spawn_on(&mid, spec, actor).await?;
                // spawn 사이에 다른 변경이 있었을 수 있다: 최신을 다시 읽어 붙인다
                w = self.workspace(name);
                w.tabs.push(Tab { id: util::new_id(), name: String::new(), root: LayoutNode::Pane { pane: id } });
            }
            LayoutChange::Split { pane, dir, machine, argv, cwd } => {
                let (target, target_mid) = self.resolve_pane(&pane)?;
                if !crate::layout::ws_contains(&w, &target) {
                    return Err(err(ErrorCode::NotFound, format!("workspace {name}에 pane {pane}이 없다")));
                }
                let mid = match machine {
                    Some(m) => self.resolve_machine(Some(&m))?,
                    None => pane_machine(&target).unwrap_or(target_mid),
                };
                let spec = SpawnSpec { name: String::new(), argv, cwd, env: vec![], cols: 80, rows: 24 };
                let id = self.spawn_on(&mid, spec, actor).await?;
                w = self.workspace(name);
                let done = w.tabs.iter_mut().any(|t| crate::layout::split(&mut t.root, &target, &id, dir));
                if !done {
                    return Err(err(ErrorCode::NotFound, "나누는 사이에 pane이 layout에서 빠졌다"));
                }
            }
            LayoutChange::AddTab { pane } => {
                let (id, _) = self.resolve_pane(&pane)?;
                if crate::layout::ws_contains(&w, &id) {
                    return Err(err(ErrorCode::BadRequest, "이미 이 workspace에 있다"));
                }
                w.tabs.push(Tab { id: util::new_id(), name: String::new(), root: LayoutNode::Pane { pane: id } });
            }
            LayoutChange::Close { pane, kill } => {
                let (id, mid) = self.resolve_pane(&pane)?;
                if kill && self.inner.lock().unwrap().panes.get(&id).is_some_and(|p| p.info.state.is_running()) {
                    let conn = self.live_conn(&mid)?;
                    self.db.audit(actor, "pane_kill", &id);
                    conn.request(AgentRequest::Kill { session: id.clone() }).await?;
                }
                if !crate::layout::ws_remove(&mut w, &id) {
                    return Err(err(ErrorCode::NotFound, format!("workspace {name}에 pane {pane}이 없다")));
                }
            }
        }
        self.db.audit(actor, "layout", name);
        self.save_workspace(&w);
        Ok(w)
    }

    fn check_running(&self, id: &str) -> Result<(), ErrorBody> {
        let i = self.inner.lock().unwrap();
        match i.panes.get(id).map(|p| &p.info.state) {
            Some(SessionState::Running) => Ok(()),
            Some(_) => Err(err(ErrorCode::SessionExited, "세션이 끝났다")),
            None => Err(err(ErrorCode::NotFound, "pane이 없다")),
        }
    }

    // ───────────── enrollment ─────────────

    pub fn enroll_open(&self) -> bool {
        self.enroll.lock().unwrap().open_until.is_some_and(|t| t > Instant::now())
    }

    /// 등록 요청을 대기열에 넣는다. 승인되면 receiver가 끝난다.
    pub fn enroll_request(
        &self,
        name: &str,
        machine_id: &str,
        fingerprint: &str,
        sas: &str,
    ) -> Result<oneshot::Receiver<()>, ErrorBody> {
        if !self.enroll_open() {
            return Err(err(
                ErrorCode::EnrollClosed,
                "host에서 `aa machine enroll --open 10m`으로 등록 창을 먼저 연다",
            ));
        }
        if name.is_empty() || name.len() > 64 {
            return Err(err(ErrorCode::BadRequest, "머신 이름은 1~64자"));
        }
        if let Ok(Some(m)) = self.db.machine_by_name(name)
            && m.id != machine_id
        {
            return Err(err(ErrorCode::NameTaken, format!("이름 {name}은 다른 머신이 쓴다")));
        }
        let mut e = self.enroll.lock().unwrap();
        if e.pending.len() >= 8 && !e.pending.contains_key(name) {
            return Err(err(ErrorCode::Unavailable, "대기 중인 등록이 너무 많다"));
        }
        let (tx, rx) = oneshot::channel();
        e.pending.insert(
            name.to_string(),
            PendingRt {
                machine_id: machine_id.to_string(),
                fingerprint: fingerprint.to_string(),
                sas: sas.to_string(),
                since_ms: util::now_ms(),
                attempts: 0,
                accept: Some(tx),
            },
        );
        Ok(rx)
    }

    pub fn enroll_cancel(&self, name: &str, fingerprint: &str) {
        let mut e = self.enroll.lock().unwrap();
        if e.pending.get(name).is_some_and(|p| p.fingerprint == fingerprint) {
            e.pending.remove(name);
        }
    }

    fn approve(&self, name: &str, code: &str, actor: &str) -> Result<FrontResponse, ErrorBody> {
        let mut e = self.enroll.lock().unwrap();
        let Some(p) = e.pending.get_mut(name) else {
            return Err(err(ErrorCode::NotFound, format!("대기 중인 등록이 없다: {name}")));
        };
        let code: String = code.chars().filter(|c| c.is_ascii_digit()).collect();
        if code != p.sas {
            p.attempts += 1;
            if p.attempts >= 3 {
                e.pending.remove(name); // sender drop → agent 쪽은 거부로 끝난다
                self.db.audit(actor, "enroll_reject", name);
                return Err(err(ErrorCode::EnrollCodeMismatch, "확인 코드가 3번 틀려 이 등록 요청을 버렸다"));
            }
            return Err(err(ErrorCode::EnrollCodeMismatch, "확인 코드가 다르다(agent 화면의 코드를 입력한다)"));
        }
        let p = e.pending.remove(name).unwrap();
        self.db
            .upsert_machine(&p.machine_id, name, Some(&p.fingerprint), false)
            .map_err(|e| err(ErrorCode::Internal, e.to_string()))?;
        self.inner.lock().unwrap().machines.insert(
            p.machine_id.clone(),
            MachineRt {
                name: name.to_string(),
                local: false,
                os: None,
                boot_id: None,
                dev_mode: false,
                conn: None,
                last_seen_ms: None,
            },
        );
        if let Some(tx) = p.accept {
            let _ = tx.send(());
        }
        self.db.audit(actor, "enroll_approve", name);
        drop(e);
        self.bump();
        Ok(FrontResponse::Done)
    }

    // ───────────── pairing ─────────────

    /// 웹 pairing 코드를 장치 token으로 바꾼다. 틀린 시도가 5번을 넘으면 점점 길게 잠근다.
    pub fn redeem_pairing(&self, code: &str) -> Result<(String, DeviceRole), ErrorBody> {
        let mut p = self.pairing.lock().unwrap();
        let now = Instant::now();
        if let Some(t) = p.locked_until
            && t > now
        {
            return Err(err(
                ErrorCode::Unavailable,
                format!("시도가 너무 많다. {}초 뒤 다시 한다", (t - now).as_secs() + 1),
            ));
        }
        p.codes.retain(|(_, _, exp)| *exp > now);
        let code: String = code.chars().filter(|c| c.is_ascii_digit()).collect();
        let Some(pos) = p.codes.iter().position(|(c, _, _)| *c == code) else {
            p.failures += 1;
            if p.failures >= 5 {
                let secs = 30u64 << (p.failures - 5).min(7);
                p.locked_until = Some(now + Duration::from_secs(secs));
            }
            return Err(err(ErrorCode::Unauthorized, "코드가 틀렸거나 만료됐다"));
        };
        let (_, role, _) = p.codes.remove(pos);
        p.failures = 0;
        drop(p);
        let token =
            base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, rand::random::<[u8; 32]>());
        let id = util::new_id();
        let role_s = if role == DeviceRole::Reader { "reader" } else { "controller" };
        self.db.add_device(&id, role_s, &token_hash(&token)).map_err(|e| err(ErrorCode::Internal, e.to_string()))?;
        self.db.audit(&format!("device:{id}"), "web_paired", role_s);
        Ok((token, role))
    }

    pub fn device_role(&self, token: &str) -> Option<(String, DeviceRole)> {
        let d = self.db.device_by_token_hash(&token_hash(token)).ok()??;
        let role = if d.role == "reader" { DeviceRole::Reader } else { DeviceRole::Controller };
        Some((d.id, role))
    }

    // ───────────── 웹 공개 ─────────────

    async fn publish_status(&self) -> PublishState {
        let out = tailscale::read().await;
        let plan = tailscale::plan(out.as_ref(), self.web_port(), &self.cfg.web.publish_ports);
        let owned = self.db.meta("publish_port").ok().flatten().and_then(|p| p.parse().ok());
        if let Some(dns) = &plan.dns_name {
            let _ = self.db.set_meta("publish_dns", dns);
        }
        PublishState {
            tailscale: plan.state.into(),
            dns_name: plan.dns_name.clone(),
            url: plan.serving.as_ref().map(|(u, _)| u.clone()),
            owned_port: owned,
            web_port: self.web_port(),
            message: match plan.state {
                "missing" => Some("tailscale CLI가 없다. 웹은 127.0.0.1에서만 열린다".into()),
                "stopped" => Some("Tailscale이 꺼졌거나 로그인 전이다".into()),
                _ if plan.serving.is_none() => Some("아직 공개하지 않았다: aa web publish".into()),
                _ => None,
            },
        }
    }

    async fn publish(&self) -> Result<PublishState, ErrorBody> {
        if self.web_port() == 0 {
            return Err(err(ErrorCode::BadRequest, "host.toml의 web.listen이 꺼져 있다"));
        }
        let out = tailscale::read().await;
        let plan = tailscale::plan(out.as_ref(), self.web_port(), &self.cfg.web.publish_ports);
        if plan.state != "running" {
            return Ok(self.publish_status().await);
        }
        if plan.serving.is_none() {
            let Some(port) = plan.free_port else {
                return Err(err(ErrorCode::Unavailable, format!("후보 포트가 모두 쓰이고 있다: {:?}", plan.taken)));
            };
            let text = tailscale::serve(port, self.web_port())
                .await
                .map_err(|e| err(ErrorCode::Unavailable, format!("{e:#}")))?;
            tracing::info!("tailscale serve: {}", text.trim());
            let _ = self.db.set_meta("publish_port", &port.to_string());
        }
        Ok(self.publish_status().await)
    }

    async fn publish_off(&self) -> Result<PublishState, ErrorBody> {
        let owned: Option<u16> = self.db.meta("publish_port").ok().flatten().and_then(|p| p.parse().ok());
        let Some(port) = owned else {
            return Err(err(ErrorCode::NotFound, "aa가 만든 공개가 없다(다른 서비스의 매핑은 건드리지 않는다)"));
        };
        tailscale::serve_off(port).await.map_err(|e| err(ErrorCode::Unavailable, format!("{e:#}")))?;
        let _ = self.db.del_meta("publish_port");
        Ok(self.publish_status().await)
    }

    /// 웹 요청의 Host 헤더가 이 host를 가리키는가(DNS rebinding 방어)
    pub fn web_port(&self) -> u16 {
        self.web_port.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn host_allowed(&self, host_header: &str) -> bool {
        let loopback = [format!("127.0.0.1:{}", self.web_port()), format!("localhost:{}", self.web_port())];
        if loopback.iter().any(|l| l == host_header) {
            return true;
        }
        let name = host_header.rsplit_once(':').map(|(h, _)| h).unwrap_or(host_header);
        self.db.meta("publish_dns").ok().flatten().is_some_and(|d| d.eq_ignore_ascii_case(name))
    }

    pub fn local_machine_connected(&self) -> bool {
        self.inner.lock().unwrap().machines.values().any(|m| m.local && m.conn.is_some())
    }
}

/// `aa host` 실행
pub async fn run(cfg: HostConfig, state_root: PathBuf) -> Result<()> {
    let host = Host::open(cfg, state_root.join("host"))?;
    tracing::info!(host = %host.host_id, fp = %&host.identity.fingerprint[..16], "host 시작");
    link::start(&host).await?;
    front::start(&host).await?;
    web::start(&host).await?;
    if host.cfg.local_agent.mode == LocalAgentMode::Child {
        let h = host.clone();
        let root = state_root.clone();
        // 개발 모드 감시: 로컬 agent가 없으면 띄운다(운영 모드에서는 LaunchAgent KeepAlive가 한다).
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(1500)).await;
            let mut missing_since: Option<Instant> = Some(Instant::now() - Duration::from_secs(10));
            loop {
                if h.local_machine_connected() {
                    missing_since = None;
                } else {
                    let since = *missing_since.get_or_insert_with(Instant::now);
                    if since.elapsed() >= Duration::from_secs(3) {
                        if let Err(e) = spawn_local_agent(&root) {
                            tracing::error!("로컬 agent 기동 실패: {e:#}");
                        }
                        missing_since = Some(Instant::now() + Duration::from_secs(5));
                    }
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        });
    }
    shutdown_signal().await;
    tracing::info!("host 종료(세션은 agent·session host가 계속 유지한다)");
    Ok(())
}

/// 개발 모드: 로컬 agent backend를 분리된 프로세스로 띄운다(결정 3 MVP 허용). 운영 모드에서는 LaunchAgent가 띄운다.
fn spawn_local_agent(state_root: &std::path::Path) -> Result<()> {
    let dir = state_root.join("agent");
    util::ensure_private_dir(&dir)?;
    let log = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("agent.log"))?;
    let mut cmd = std::process::Command::new(std::env::current_exe()?);
    cmd.args(["agent", "--host", "local", "--dev-mode"]);
    cmd.stdin(std::process::Stdio::null()).stdout(log.try_clone()?).stderr(log);
    #[cfg(unix)]
    unsafe {
        use std::os::unix::process::CommandExt;
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let mut child = cmd.spawn()?;
    tracing::info!(pid = child.id(), "개발 모드: 로컬 agent를 띄웠다");
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

pub async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut term = signal(SignalKind::terminate()).expect("SIGTERM");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

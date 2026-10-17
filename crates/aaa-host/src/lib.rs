//! `aaa host`: host backend. 정본 목록·권한·Front API·웹을 갖고, 각 머신의 agent backend와 Agent link로 통신한다.
//!
//! - host는 자식 프로세스를 만들지 않는다. 예외: tailscale CLI, 개발 모드의 로컬 agent 기동(§4.2).
//! - pane 화면은 agent(session host)가 정본이다. host는 마지막 화면 mirror를 가져 agent가 끊겨도 읽기 전용으로 보여 준다.

mod attach;
mod coord;
pub mod db;
mod evaluator;
pub mod front;
mod harness;
mod health;
mod http;
mod inbox;
mod library;
pub mod link;
mod merge;
pub mod migrate;
mod notify;
mod orch;
mod output;
mod push;
mod routine;
mod seen;
pub mod tailscale;
mod tasks;
mod text;
pub mod usage;
pub mod web;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use aaa_config::{HostConfig, LocalAgentMode};
use aaa_proto::{
    AgentRequest, AgentResponse, Connection, DeviceRole, ErrorBody, ErrorCode, FRONT_API, FrontRequest, FrontResponse,
    HostStatus, LayoutChange, LayoutNode, LeaseView, LinkMsg, MachineView, Outcome, PaneView, PendingEnroll,
    PublishState, ScreenText, SessionInfo, SessionState, SpawnSpec, Tab, TerminalChunk, Workspace,
};
use anyhow::{Context, Result};
use sha2::Digest;
use tokio::sync::{broadcast, oneshot, watch};

use aaa_common::conn::FrameTx;
use aaa_common::util;
use aaa_vt::Vt;

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
        self.request_timeout(body, REQUEST_TIMEOUT).await
    }

    /// 오래 걸리는 요청(routine 명령 등)
    pub async fn request_timeout(&self, body: AgentRequest, wait: Duration) -> Result<AgentResponse, ErrorBody> {
        let id = format!("c{}", self.seq.fetch_add(1, Ordering::Relaxed));
        let (otx, orx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id.clone(), otx);
        let msg = LinkMsg::Req { id: id.clone(), deadline_ms: Some(wait.as_millis() as u64), body };
        if self.tx.json(&msg).await.is_err() {
            self.pending.lock().unwrap().remove(&id);
            return Err(ErrorBody::new(ErrorCode::AgentDisconnected, "agent 연결이 끊겼다"));
        }
        match tokio::time::timeout(wait, orx).await {
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
    /// 마지막 건강 보고와 그때의 시계 차이(agent - host, ms)
    health: Option<(aaa_proto::MachineHealth, i64)>,
    /// agent가 hello에서 알린 기능(`cwd.v1` 등). 모르는 요청을 받은 옛 agent는 연결을 끊으므로 보내기 전에 본다.
    features: Vec<String>,
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

/// 웹 터미널 attach 결과: 사건 구독, replay와 그 앞 기록이 더 있는지, replay 끝 offset, 크기
pub struct Attach {
    pub rx: broadcast::Receiver<PaneEvent>,
    pub replay: Vec<u8>,
    pub more: bool,
    pub offset: u64,
    pub cols: u16,
    pub rows: u16,
}

struct PaneRt {
    machine_id: String,
    info: SessionInfo,
    mirror: Option<Mirror>,
    events: broadcast::Sender<PaneEvent>,
    /// 지금 쓰는 대화형 front(lease, 연결 하나). PTY 크기를 그 화면이 정한다. 다른 front가 입력·크기·그림을 보내면
    /// 그 front로 바뀌고(마지막 입력이 이긴다), 연결이 끝나거나 놓으면 비운다.
    lease: Option<FrontRef>,
    /// harness 상태(hook 사건으로만 바뀐다)
    agent: Option<aaa_proto::AgentStatus>,
    /// Claude 대화 기록 파일(그 머신의 경로, §8.27). 웹·로그로 내보내지 않는다.
    transcript: Option<String>,
}

/// 대화형 front 연결(TUI의 UDS 연결, 웹 터미널 WebSocket 하나)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrontRef {
    pub id: String,
    pub label: String,
}

impl FrontRef {
    /// 대화형 front(TUI·웹 터미널). 한 번 보내는 CLI는 아니다.
    pub fn interactive(&self) -> bool {
        self.label != aaa_common::front::CLI_CLIENT
    }
}

pub use aaa_proto::DEFAULT_WORKSPACE;

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
    pub identity: aaa_common::tls::Identity,
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
    /// 이 host 실행의 boot ID(흐름 사건에 같이 적는다)
    pub boot_id: String,
    /// 새 흐름 사건(연결된 front에 알린다)
    pub flow_events: broadcast::Sender<aaa_proto::FlowEvent>,
    /// thread 기록이 바뀌었다
    pub thread_events: broadcast::Sender<aaa_proto::ThreadView>,
    /// 보고 사본은 한 번에 하나씩 한다
    collect_lock: tokio::sync::Mutex<()>,
    /// 확인창 질문이 열리거나 바뀌었다
    pub question_events: broadcast::Sender<aaa_proto::QuestionView>,
    /// pane별 harness 사건 대기자
    waiters: Mutex<harness::Waiters>,
    /// 메시지 전달은 한 번에 하나씩 한다
    deliver_lock: tokio::sync::Mutex<()>,
    /// pane 등록 전에 온 harness 사건
    held: Mutex<harness::Held>,
    /// decisions.md 덧붙이기는 한 번에 하나씩
    decision_lock: Mutex<()>,
    /// ticker를 바로 깨운다(흐름 사건·harness 사건)
    tick_wake: tokio::sync::Notify,
    /// 머신별 끊김: 이 host 프로세스가 끊김을 처음 본 때와 알렸는지(inbox::Outage)
    outages: Mutex<HashMap<String, inbox::Outage>>,
    /// 머신별 마지막으로 알린 건강 이상(이어 붙인 글, 정상이면 빈 글)
    health_marks: Mutex<HashMap<String, String>>,
    /// hook 없는 coordinator pane: 마지막 화면 hash와 그 화면이 된 때
    nudge_screens: Mutex<HashMap<String, (String, Instant)>>,
    /// 프로젝트별로 nudge가 기다리는 coordinator pane과 그 입력창이 비어 보인 때(herdr-projects `box_pane`·`box_empty_since`)
    nudge_box: Mutex<HashMap<String, (String, Instant)>>,
    /// task.md Follow-ups 덧붙이기는 한 번에 하나씩
    follow_up_lock: Mutex<()>,
    /// 지금 도는 routine 명령(`<project>/<name>`)
    routine_running: Mutex<HashSet<String>>,
    /// 결과물 보기 경로(`/v/<cap>/`) → 대상 thread
    view_caps: Mutex<HashMap<String, library::ViewCap>>,
    /// Web Push 서명 키(설정이 켜져 있을 때)
    vapid: Option<push::Vapid>,
    /// 구독 사용량(웹이 물을 때만 읽는다)
    usage: usage::UsageService,
    /// 알림 webhook 대기열(설정이 켜져 있을 때)
    notify_tx: Mutex<Option<tokio::sync::mpsc::UnboundedSender<aaa_proto::InboxItem>>>,
    /// 지금 쓰는(보이고 focus가 있는) 웹 화면 수(events WS `presence`). 하나라도 있으면 Web Push를 보내지 않는다
    pub(crate) in_use: std::sync::atomic::AtomicUsize,
    /// 지금 pane을 보는 웹 화면(「끝남(안 봄)」, seen.rs)
    viewing: Mutex<seen::Viewing>,
    /// 사후 평가자: 프로젝트별 다음 묶음 확인 때
    eval_due: Mutex<HashMap<String, Instant>>,
    /// 사후 평가자 묶음은 한 번에 하나씩 만든다
    eval_lock: tokio::sync::Mutex<()>,
    /// 평가 기록 채택·보류는 한 번에 하나씩
    eval_decide_lock: Mutex<()>,
    /// thread 기록 다시 읽어 고치기(`update_rec`)는 한 번에 하나씩
    rec_lock: Mutex<()>,
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
        let identity = aaa_common::tls::load_or_create(&state, "aaa-host")?;
        // Web Push 키가 깨졌으면 host는 뜨고 push만 끈다(로그에 남긴다)
        let vapid = if cfg.notify.web_push {
            match push::Vapid::load_or_create(&state) {
                Ok(v) => Some(v),
                Err(e) => {
                    tracing::error!("Web Push를 끈다: {e:#}");
                    None
                }
            }
        } else {
            None
        };
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
                    health: None,
                    features: vec![],
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
            boot_id: util::boot_id(),
            flow_events: broadcast::channel(256).0,
            thread_events: broadcast::channel(256).0,
            collect_lock: tokio::sync::Mutex::new(()),
            question_events: broadcast::channel(256).0,
            waiters: Mutex::new(HashMap::new()),
            deliver_lock: tokio::sync::Mutex::new(()),
            held: Mutex::new(HashMap::new()),
            decision_lock: Mutex::new(()),
            tick_wake: tokio::sync::Notify::new(),
            outages: Mutex::new(HashMap::new()),
            health_marks: Mutex::new(HashMap::new()),
            nudge_screens: Mutex::new(HashMap::new()),
            nudge_box: Mutex::new(HashMap::new()),
            follow_up_lock: Mutex::new(()),
            routine_running: Mutex::new(HashSet::new()),
            view_caps: Mutex::new(HashMap::new()),
            vapid,
            usage: usage::UsageService::new(usage::UsageContext::system()),
            notify_tx: Mutex::new(None),
            in_use: std::sync::atomic::AtomicUsize::new(0),
            viewing: Mutex::new(seen::Viewing::default()),
            eval_due: Mutex::new(HashMap::new()),
            eval_lock: tokio::sync::Mutex::new(()),
            eval_decide_lock: Mutex::new(()),
            rec_lock: Mutex::new(()),
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
    pub fn agent_connected(self: &Arc<Self>, hello: &aaa_proto::AgentHello, local: bool, conn: AgentConn) {
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
                health: None,
                features: vec![],
            });
            m.os = Some(hello.os.clone());
            m.boot_id = Some(hello.boot_id.clone());
            m.dev_mode = hello.dev_mode;
            m.features = hello.features.clone();
            m.conn = Some(conn);
            // 다시 붙은 agent는 설정이 바뀌었을 수 있다. 새 보고를 기다린다.
            m.health = None;
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
                    agent: None,
                    transcript: None,
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
            self.replay_harness(&sid);
            self.spawn_subscribe(hello.machine_id.clone(), sid, from);
        }
        self.rewatch(hello.machine_id.clone());
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
    pub fn on_subscribed(&self, session: &str, snapshot: Option<aaa_proto::Snapshot>, offset: u64) {
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
                agent: None,
                transcript: None,
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
        self.replay_harness(&info.id);
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
                health: m.health.as_ref().map(|h| h.0.clone()),
                clock_skew_ms: m.health.as_ref().map(|h| h.1),
                problems: health::problems(m),
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
            role: p.info.role,
            agent: p.agent.clone().map(Box::new),
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
        let (replay, more, offset, rows, cols) = match &p.mirror {
            Some(m) => {
                let (rows, cols) = m.vt.size();
                let (replay, more) = m.vt.replay_more();
                (replay, more, m.offset, rows, cols)
            }
            None => (Vec::new(), false, 0, p.info.rows, p.info.cols),
        };
        Some(Attach { rx, replay, more, offset, cols, rows })
    }

    /// 웹이 위로 굴려 받은 기록 끝에 닿았다: host mirror 기록에서 anchor 바로 앞 덩어리(plan §8.67)
    pub fn older_history(&self, id: &str, anchor: &[String], depth: usize) -> Option<aaa_vt::Older> {
        let i = self.inner.lock().unwrap();
        i.panes.get(id)?.mirror.as_ref()?.vt.older(anchor, depth, aaa_vt::REPLAY_HISTORY)
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
            FrontRequest::Panes { role: want } => {
                // 기본은 평가자 세션을 빼고, 역할을 주면 그 역할만(§8.13)
                let panes = self
                    .pane_views(role)
                    .into_iter()
                    .filter(|p| match want {
                        None => p.role != Some(aaa_proto::Role::Evaluator),
                        Some(r) => p.role == Some(r),
                    })
                    .collect();
                Ok(FrontResponse::Panes { panes })
            }
            FrontRequest::ProjectCreate { name } => self.project_create(&name, actor),
            FrontRequest::Projects => Ok(FrontResponse::Projects { projects: self.projects() }),
            FrontRequest::Profiles { machine } => self.profiles(machine.as_deref()).await,
            FrontRequest::ThreadStart { project, title, machine, profile, kind, repo, base, task, from_task } => {
                let mut a = orch::StartArgs {
                    project,
                    title,
                    machine,
                    profile,
                    kind,
                    repo,
                    base,
                    task,
                    role: aaa_proto::Role::Worker,
                };
                if let Some(t) = from_task {
                    self.apply_from_task(&mut a, &t).await?;
                }
                self.thread_start(a, actor).await.map(FrontResponse::Thread)
            }
            FrontRequest::ThreadNext { project, thread, line, add } => {
                self.thread_next(&project, &thread, line, add, actor).await
            }
            FrontRequest::Context { project, peek } => self.context(&project, peek).await.map(FrontResponse::Context),
            FrontRequest::Assignable { project, check } => {
                self.project_dir(&project)?;
                let owners = self.assignable().await;
                let valid = match check {
                    Some(c) => Some(self.check_owner(&owners, &c)?),
                    None => None,
                };
                Ok(FrontResponse::Assignable { owners, valid })
            }
            FrontRequest::Threads { project, role: want } => {
                self.threads_of(project.as_deref(), want).map(|threads| FrontResponse::Threads { threads })
            }
            FrontRequest::ThreadShow { project, thread } => {
                self.thread_show(&project, &thread).map(FrontResponse::Thread)
            }
            FrontRequest::ThreadLibrary { project, thread } => {
                self.thread_library(&project, &thread).map(FrontResponse::Library)
            }
            FrontRequest::ThreadAck { project, thread } => {
                self.thread_ack(&project, &thread, actor).map(FrontResponse::Thread)
            }
            FrontRequest::ThreadResolve { project, thread, keep_worktree } => {
                self.thread_resolve(&project, &thread, keep_worktree, actor).await.map(FrontResponse::Thread)
            }
            FrontRequest::ThreadStop { project, thread } => {
                self.thread_stop(&project, &thread, actor).await.map(FrontResponse::Thread)
            }
            FrontRequest::ThreadRestart { project, thread, resume } => {
                self.thread_restart(&project, &thread, resume, actor).await.map(FrontResponse::Thread)
            }
            FrontRequest::ThreadBrief { project, thread } => {
                self.thread_brief(&project, &thread, actor).await.map(FrontResponse::Thread)
            }
            FrontRequest::Questions { all } => Ok(FrontResponse::Questions { questions: self.questions(all) }),
            FrontRequest::QuestionAnswer { id, choice } => {
                self.answer(&id, &choice, actor).await.map(FrontResponse::Question)
            }
            FrontRequest::ThreadMessage { project, thread, text, mode } => {
                self.thread_message(&project, &thread, text, mode, actor).await.map(FrontResponse::Message)
            }
            FrontRequest::DecisionAdd { project, text, replaces } => {
                self.decision_add(&project, &text, replaces, actor).map(FrontResponse::Decision)
            }
            FrontRequest::Decisions { project } => {
                self.decisions(&project).map(|decisions| FrontResponse::Decisions { decisions })
            }
            FrontRequest::Inbox { project, all, peek } => {
                self.inbox(&project, all, peek).map(|items| FrontResponse::Inbox { items })
            }
            FrontRequest::InboxDone { project, ids, all } => {
                self.inbox_done(&project, &ids, all, actor).map(|items| FrontResponse::Inbox { items })
            }
            FrontRequest::MergeEnqueue { project, thread } => {
                self.merge_enqueue(&project, &thread, actor).map(|queue| FrontResponse::Merge { queue })
            }
            FrontRequest::MergeDrop { project, thread, landed } => {
                self.merge_drop(&project, &thread, landed, actor).map(|queue| FrontResponse::Merge { queue })
            }
            FrontRequest::MergeLand { project, thread } => {
                self.merge_land(&project, thread.as_deref(), actor).await.map(|v| FrontResponse::MergeLand(Box::new(v)))
            }
            FrontRequest::MergeList { project } => {
                self.merge_queue(&project).map(|queue| FrontResponse::Merge { queue })
            }
            FrontRequest::Routines { project } => {
                self.routines(&project).map(|routines| FrontResponse::Routines { routines })
            }
            FrontRequest::RoutineApprove { project, name, command_sha256 } => self
                .routine_approve(&project, &name, &command_sha256, actor)
                .map(|routines| FrontResponse::Routines { routines }),
            FrontRequest::Events { project, after, limit } => {
                self.events(project.as_deref(), after, limit).map(|events| FrontResponse::Events { events })
            }
            FrontRequest::EvaluatorAssign { project, profile, machine, repo, note } => self
                .evaluator_assign(&project, profile, machine, repo, note, actor)
                .await
                .map(|v| FrontResponse::Evaluators { evaluators: vec![v] }),
            FrontRequest::EvaluatorStatus { project } => {
                self.evaluator_views(project.as_deref()).map(|evaluators| FrontResponse::Evaluators { evaluators })
            }
            FrontRequest::EvaluatorUnassign { project } => self
                .evaluator_unassign(&project, actor)
                .await
                .map(|v| FrontResponse::Evaluators { evaluators: vec![v] }),
            FrontRequest::Evaluations { project, all } => {
                self.evaluations(&project, all).map(|evaluations| FrontResponse::Evaluations { evaluations })
            }
            FrontRequest::EvaluationShow { project, id } => {
                self.evaluation(&project, &id).map(|v| FrontResponse::Evaluation(Box::new(v)))
            }
            FrontRequest::EvaluationDecide { project, id, proposals, adopt, note } => self
                .evaluation_decide(&project, &id, &proposals, adopt, note, actor)
                .map(|v| FrontResponse::Evaluation(Box::new(v))),
            FrontRequest::PaneRun { machine, name, argv, cwd, cols, rows } => {
                let mid = self.resolve_machine(machine.as_deref())?;
                let spec = SpawnSpec {
                    name: name.unwrap_or_default(),
                    argv,
                    cwd,
                    env: vec![],
                    cols: cols.unwrap_or(120),
                    rows: rows.unwrap_or(32),
                    role: None,
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
            FrontRequest::PaneChat { pane } => {
                let (id, mid) = self.resolve_pane(&pane)?;
                // 보기 범위(§8.25)는 pane 보기와 같다
                if self.pane_view_of(&id, role).is_none() {
                    return Err(err(ErrorCode::NotFound, format!("pane이 없다: {pane}")));
                }
                let (harness, transcript, harness_session) = {
                    let i = self.inner.lock().unwrap();
                    let p = i.panes.get(&id).ok_or_else(|| err(ErrorCode::NotFound, "pane이 없다"))?;
                    let a = p.agent.as_ref();
                    (
                        a.map(|a| a.harness.clone()).unwrap_or_default(),
                        p.transcript.clone(),
                        a.and_then(|a| a.harness_session.clone()),
                    )
                };
                if !matches!(harness.as_str(), "claude" | "codex") {
                    return Err(err(ErrorCode::NotFound, "대화 기록은 Claude·Codex pane만 볼 수 있다"));
                }
                let conn = self.live_conn(&mid)?;
                match conn.request(AgentRequest::Chat { harness: harness.clone(), transcript, harness_session }).await?
                {
                    AgentResponse::Chat { messages, truncated } => {
                        Ok(FrontResponse::Chat(aaa_proto::ChatView { pane: id, harness, messages, truncated }))
                    }
                    other => Err(err(ErrorCode::Internal, format!("예상하지 못한 응답: {other:?}"))),
                }
            }
            FrontRequest::PaneReadText { pane, source, lines, format } => {
                self.pane_read_text(&pane, source, lines, format).await
            }
            FrontRequest::PaneSendText { pane, text } => self.pane_send_text(&pane, text, actor).await,
            FrontRequest::PaneSendKeys { pane, keys } => self.pane_send_keys(&pane, &keys, actor).await,
            FrontRequest::PaneSubmit { pane, text } => self.pane_submit(&pane, &text, actor).await,
            FrontRequest::PaneWaitOutput { pane, matcher, source, lines, timeout_ms } => {
                self.pane_wait_output(&pane, matcher, source, lines, timeout_ms).await
            }
            FrontRequest::PaneGet { pane } => {
                let (id, _) = self.resolve_pane(&pane)?;
                self.pane_view_of(&id, role)
                    .map(FrontResponse::Pane)
                    .ok_or_else(|| err(ErrorCode::NotFound, format!("pane이 없다: {pane}")))
            }
            FrontRequest::PaneClose { pane } => self.pane_close(&pane, actor).await,
            FrontRequest::Workspaces => Ok(FrontResponse::Workspaces { workspaces: self.workspace_summaries() }),
            FrontRequest::WorkspaceClose { name } => self.workspace_close(&name, actor).await,
            FrontRequest::WorkspaceRename { name, to } => self.workspace_rename(&name, &to, actor),
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
                // 대화형 front가 크기를 걸면 그 화면이 지금 쓰는 화면이 된다. 한 번 보내는 CLI는 아무도 쓰지 않을 때만 바꾼다.
                let interactive = match front {
                    Some(f) if f.interactive() => {
                        self.lease_use(&id, f);
                        true
                    }
                    _ => {
                        self.lease_free(&id)?;
                        false
                    }
                };
                let conn = self.live_conn(&mid)?;
                if interactive {
                    return conn
                        .request(AgentRequest::Resize { session: id, cols, rows })
                        .await
                        .map(|_| FrontResponse::Done);
                }
                // 한 번 보내는 CLI(`pane pty-size`): 새 크기가 host에 닿은 뒤 돌아온다. 바로 뒤 읽기가 새 크기의 화면을 본다.
                let rx = self.pane_events(&id)?;
                conn.request(AgentRequest::Resize { session: id.clone(), cols, rows }).await?;
                self.wait_pane(
                    &id,
                    rx,
                    |p| (p.info.cols, p.info.rows) == (cols, rows),
                    "크기를 보냈지만 새 크기가 오지 않았다",
                )
                .await
                .map(|_| FrontResponse::Done)
            }
            FrontRequest::PaneInput { pane, data } => {
                let f = front.ok_or_else(|| {
                    err(ErrorCode::BadRequest, "대화형 front 연결에서만 쓴다(한 번 보내기는 pane_send)")
                })?;
                let (id, mid) = self.resolve_pane(&pane)?;
                self.check_running(&id)?;
                let conn = self.live_conn(&mid)?;
                self.lease_use(&id, f);
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
                Err(err(ErrorCode::BadRequest, "화면 구독은 같은 머신 front 연결(aaa attach)에서만 쓴다"))
            }
            // 받는 중인 첨부는 연결마다 하나라 front 연결 처리기(`front.rs`)가 직접 받는다
            FrontRequest::PaneAttach { .. } => Err(err(
                ErrorCode::BadRequest,
                "그림 붙여넣기는 같은 머신 front 연결(aaa attach·aaa pane attach)에서만 쓴다",
            )),
            FrontRequest::Workspace { name } => {
                Ok(FrontResponse::Workspace(self.workspace(name.as_deref().unwrap_or(DEFAULT_WORKSPACE))))
            }
            FrontRequest::LayoutApply { workspace, change } => {
                // workspace를 주지 않으면 그 pane·탭이 든 workspace(없으면 main)
                let found = match &change {
                    LayoutChange::Split { pane, .. }
                    | LayoutChange::Close { pane, .. }
                    | LayoutChange::Resize { pane, .. }
                    | LayoutChange::MoveToNewTab { pane, .. }
                    | LayoutChange::Zoom { pane, .. }
                    | LayoutChange::Swap { pane, .. } => self.workspace_of_pane(pane),
                    LayoutChange::RenameTab { tab, .. } | LayoutChange::CloseTab { tab } => self.workspace_of_tab(tab),
                    LayoutChange::NewTab { .. } | LayoutChange::AddTab { .. } => None,
                };
                let name = workspace.or(found).unwrap_or_else(|| DEFAULT_WORKSPACE.to_string());
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
        self.check_health(mid)?;
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
        // TUI가 그 pane을 쓰기 시작했다: 봤다(herdr `mark_active_tab_seen`). 웹 화면은 presence로 센다(seen.rs)
        if self.inner.lock().unwrap().panes.get(id).is_some_and(|p| self.someone_sees(id, p.lease.as_ref())) {
            self.mark_seen(&[id.to_string()]);
        }
    }

    /// 이 front가 지금 쓰는 화면이 된다(herdr-web-ui처럼 명시적 가져오기 없이, 마지막 입력이 이긴다).
    /// 바뀌면 pane 상태 사건이 나가고, 각 front는 자기 규칙으로 크기를 다시 건다.
    pub(crate) fn lease_use(&self, id: &str, f: &FrontRef) {
        self.lease_set(id, Some(f.clone()));
    }

    fn lease_free(&self, id: &str) -> Result<(), ErrorBody> {
        match self.inner.lock().unwrap().panes.get(id).and_then(|p| p.lease.clone()) {
            Some(h) => Err(err(ErrorCode::LeaseHeld, format!("{}이 쓰는 중이라 크기를 바꾸지 않았다", h.label))),
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
            role: info.role,
        };
        let new_id = self.spawn_on(&mid, spec, actor).await?;
        self.db.audit(actor, "pane_resume", &format!("{id} -> {new_id}"));
        {
            let mut ws = self.workspaces.lock().unwrap();
            let changed: Vec<Workspace> = ws
                .values_mut()
                .filter_map(|w| aaa_proto::layout::ws_replace(w, &id, &new_id).then(|| w.clone()))
                .collect();
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
            ws.values_mut().filter_map(|w| aaa_proto::layout::ws_remove(w, id).then(|| w.clone())).collect()
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
            LayoutChange::NewTab { machine, name: pname, argv, cwd, cols, rows, label, from } => {
                let mid = self.resolve_machine(machine.as_deref())?;
                let cwd = self.new_pane_cwd(cwd, from.as_deref(), &mid).await;
                let spec = SpawnSpec {
                    name: pname.unwrap_or_default(),
                    argv,
                    cwd,
                    env: vec![],
                    cols: cols.unwrap_or(120),
                    rows: rows.unwrap_or(32),
                    role: None,
                };
                let id = self.spawn_on(&mid, spec, actor).await?;
                // spawn 사이에 다른 변경이 있었을 수 있다: 최신을 다시 읽어 붙인다
                w = self.workspace(name);
                let label = label.map(|l| l.trim().to_string()).unwrap_or_default();
                w.tabs.push(Tab {
                    id: util::new_id(),
                    name: label,
                    root: LayoutNode::Pane { pane: id },
                    zoomed: false,
                });
            }
            LayoutChange::Split { pane, dir, machine, argv, cwd, ratio } => {
                let (target, target_mid) = self.resolve_pane(&pane)?;
                if !aaa_proto::layout::ws_contains(&w, &target) {
                    return Err(err(ErrorCode::NotFound, format!("workspace {name}에 pane {pane}이 없다")));
                }
                let mid = match machine {
                    Some(m) => self.resolve_machine(Some(&m))?,
                    None => pane_machine(&target).unwrap_or(target_mid),
                };
                let cwd = self.new_pane_cwd(cwd, Some(&target), &mid).await;
                let spec = SpawnSpec { name: String::new(), argv, cwd, env: vec![], cols: 80, rows: 24, role: None };
                let id = self.spawn_on(&mid, spec, actor).await?;
                w = self.workspace(name);
                let done = w.tabs.iter_mut().any(|t| {
                    // herdr: 나누면 그 탭의 zoom을 푼다
                    let hit = aaa_proto::layout::split(&mut t.root, &target, &id, dir);
                    t.zoomed &= !hit;
                    hit
                });
                if let Some(r) = ratio {
                    // herdr `pane split --ratio`: 새로 생긴 나눔의 a 쪽 비율
                    for t in &mut w.tabs {
                        aaa_proto::layout::set_split_ratio(&mut t.root, &id, r.clamp(100, 900));
                    }
                }
                if !done {
                    return Err(err(ErrorCode::NotFound, "나누는 사이에 pane이 layout에서 빠졌다"));
                }
            }
            LayoutChange::AddTab { pane } => {
                let (id, _) = self.resolve_pane(&pane)?;
                if aaa_proto::layout::ws_contains(&w, &id) {
                    return Err(err(ErrorCode::BadRequest, "이미 이 workspace에 있다"));
                }
                w.tabs.push(Tab {
                    id: util::new_id(),
                    name: String::new(),
                    root: LayoutNode::Pane { pane: id },
                    zoomed: false,
                });
            }
            LayoutChange::Close { pane, kill } => {
                let (id, mid) = self.resolve_pane(&pane)?;
                if kill && self.inner.lock().unwrap().panes.get(&id).is_some_and(|p| p.info.state.is_running()) {
                    let conn = self.live_conn(&mid)?;
                    self.db.audit(actor, "pane_kill", &id);
                    conn.request(AgentRequest::Kill { session: id.clone() }).await?;
                }
                if !aaa_proto::layout::ws_remove(&mut w, &id) {
                    return Err(err(ErrorCode::NotFound, format!("workspace {name}에 pane {pane}이 없다")));
                }
            }
            LayoutChange::Resize { pane, direction, amount } => {
                let (id, _) = self.resolve_pane(&pane)?;
                // herdr `handle_pane_resize`: 기본 0.05, 최대 0.5
                let amount = amount.unwrap_or(50).min(500);
                let mut found = false;
                for t in &mut w.tabs {
                    if aaa_proto::layout::leaves(&t.root).contains(&id) {
                        found = true;
                        aaa_proto::layout::resize(&mut t.root, &id, direction, amount);
                    }
                }
                if !found {
                    return Err(err(ErrorCode::NotFound, format!("workspace {name}에 pane {pane}이 없다")));
                }
            }
            LayoutChange::RenameTab { tab, name: label } => {
                let t = w
                    .tabs
                    .iter_mut()
                    .find(|t| t.id == tab)
                    .ok_or_else(|| err(ErrorCode::NotFound, format!("workspace {name}에 탭 {tab}이 없다")))?;
                t.name = label;
            }
            LayoutChange::CloseTab { tab } => {
                let at = w
                    .tabs
                    .iter()
                    .position(|t| t.id == tab)
                    .ok_or_else(|| err(ErrorCode::NotFound, format!("workspace {name}에 탭 {tab}이 없다")))?;
                let t = w.tabs.remove(at);
                self.close_panes(&aaa_proto::layout::leaves(&t.root), name, actor).await;
            }
            LayoutChange::MoveToNewTab { pane, name: label } => {
                let (id, _) = self.resolve_pane(&pane)?;
                // 옮긴다: 다른 workspace에 있던 자리도 뺀다(herdr `pane move`는 pane이 한 곳에만 있다)
                let others: Vec<Workspace> = {
                    let mut ws = self.workspaces.lock().unwrap();
                    ws.values_mut()
                        .filter(|o| o.name != name)
                        .filter_map(|o| aaa_proto::layout::ws_remove(o, &id).then(|| o.clone()))
                        .collect()
                };
                for o in others {
                    self.save_workspace(&o);
                }
                aaa_proto::layout::ws_remove(&mut w, &id);
                w.tabs.push(Tab {
                    id: util::new_id(),
                    name: label.unwrap_or_default(),
                    root: LayoutNode::Pane { pane: id },
                    zoomed: false,
                });
            }
            LayoutChange::Zoom { pane, zoomed } => {
                let (id, _) = self.resolve_pane(&pane)?;
                let t = w
                    .tabs
                    .iter_mut()
                    .find(|t| aaa_proto::layout::leaves(&t.root).contains(&id))
                    .ok_or_else(|| err(ErrorCode::NotFound, format!("workspace {name}에 pane {pane}이 없다")))?;
                // herdr `apply_pane_zoom`: pane이 하나뿐인 탭은 그대로
                if aaa_proto::layout::leaves(&t.root).len() > 1 {
                    t.zoomed = zoomed.unwrap_or(!t.zoomed);
                }
            }
            LayoutChange::Swap { pane, with } => {
                let (a, _) = self.resolve_pane(&pane)?;
                let (b, _) = self.resolve_pane(&with)?;
                let t = w
                    .tabs
                    .iter_mut()
                    .find(|t| {
                        let l = aaa_proto::layout::leaves(&t.root);
                        l.contains(&a) && l.contains(&b)
                    })
                    .ok_or_else(|| {
                        err(ErrorCode::BadRequest, format!("workspace {name}의 한 탭에 {pane}·{with}가 함께 없다"))
                    })?;
                aaa_proto::layout::swap(&mut t.root, &a, &b);
            }
        }
        self.db.audit(actor, "layout", name);
        self.save_workspace(&w);
        Ok(w)
    }

    /// 탭·workspace를 닫을 때 그 안 pane을 끝내고 기록에서 지운다(herdr `tab close`·`workspace close`: pane이 사라진다).
    /// 역할이 있는 pane(thread·coordinator·평가자)은 끝내지 않는다: 그 수명은 thread 명령(resolve·stop)이 정한다.
    /// `closing` 밖 다른 workspace에도 있는 pane은 자리만 빠진다. agent가 끊긴 pane은 끝낼 수 없어 기록을 둔다.
    async fn close_panes(self: &Arc<Self>, panes: &[String], closing: &str, actor: &str) {
        for id in panes {
            let target = {
                let i = self.inner.lock().unwrap();
                i.panes.get(id).map(|p| (p.machine_id.clone(), p.info.state.is_running(), p.info.role.is_some()))
            };
            let Some((mid, running, has_role)) = target else { continue };
            let elsewhere = self
                .workspaces
                .lock()
                .unwrap()
                .values()
                .any(|w| w.name != closing && aaa_proto::layout::ws_contains(w, id));
            if has_role || elsewhere {
                continue;
            }
            let Ok(conn) = self.live_conn(&mid) else { continue };
            if running {
                let Ok(rx) = self.pane_events(id) else { continue };
                self.db.audit(actor, "pane_kill", id);
                if let Err(e) = conn.request(AgentRequest::Kill { session: id.clone() }).await {
                    tracing::warn!(pane = %id, "닫으며 끝내지 못했다: {}", e.message);
                    continue;
                }
                if let Err(e) = self
                    .wait_pane(id, rx, |p| !p.info.state.is_running(), "끝내기를 보냈지만 세션이 끝나지 않았다")
                    .await
                {
                    tracing::warn!(pane = %id, "{}", e.message);
                    continue;
                }
            }
            if let Err(e) = self.forget(id, &mid, actor).await {
                tracing::warn!(pane = %id, "닫으며 기록을 지우지 못했다: {}", e.message);
            }
        }
    }

    /// pane이 들어 있는 workspace(main을 먼저). layout 요청에 workspace를 주지 않으면 이것을 쓴다(herdr는 pane ID가 workspace를 정한다).
    pub fn workspace_of_pane(&self, pane: &str) -> Option<String> {
        let (id, _) = self.resolve_pane(pane).ok()?;
        let ws = self.workspaces.lock().unwrap();
        if ws.get(DEFAULT_WORKSPACE).is_some_and(|w| aaa_proto::layout::ws_contains(w, &id)) {
            return Some(DEFAULT_WORKSPACE.to_string());
        }
        let mut names: Vec<_> =
            ws.values().filter(|w| aaa_proto::layout::ws_contains(w, &id)).map(|w| w.name.clone()).collect();
        names.sort();
        names.into_iter().next()
    }

    /// 탭이 들어 있는 workspace
    pub fn workspace_of_tab(&self, tab: &str) -> Option<String> {
        let ws = self.workspaces.lock().unwrap();
        ws.values().find(|w| w.tabs.iter().any(|t| t.id == tab)).map(|w| w.name.clone())
    }

    fn workspace_summaries(&self) -> Vec<aaa_proto::WorkspaceSummary> {
        let ws = self.workspaces.lock().unwrap();
        let mut out: Vec<_> = ws
            .values()
            .map(|w| aaa_proto::WorkspaceSummary {
                name: w.name.clone(),
                tabs: w.tabs.len() as u32,
                panes: w.tabs.iter().map(|t| aaa_proto::layout::leaves(&t.root).len() as u32).sum(),
            })
            .collect();
        if !ws.contains_key(DEFAULT_WORKSPACE) {
            out.push(aaa_proto::WorkspaceSummary { name: DEFAULT_WORKSPACE.to_string(), tabs: 0, panes: 0 });
        }
        out.sort_by(|a, b| (a.name != DEFAULT_WORKSPACE).cmp(&(b.name != DEFAULT_WORKSPACE)).then(a.name.cmp(&b.name)));
        out
    }

    /// workspace를 닫는다: 그 안 pane을 끝내고(역할 있는 pane은 두고) layout을 지운다. 붙은 TUI에는 빈 workspace로 알린다.
    async fn workspace_close(self: &Arc<Self>, name: &str, actor: &str) -> Result<FrontResponse, ErrorBody> {
        let w = self
            .workspaces
            .lock()
            .unwrap()
            .remove(name)
            .ok_or_else(|| err(ErrorCode::NotFound, format!("workspace가 없다: {name}")))?;
        if let Err(e) = self.db.delete_workspace(name) {
            tracing::warn!(workspace = %name, "workspace 지우기 실패: {e}");
        }
        let panes: Vec<String> = w.tabs.iter().flat_map(|t| aaa_proto::layout::leaves(&t.root)).collect();
        self.close_panes(&panes, name, actor).await;
        self.db.audit(actor, "workspace_close", name);
        let _ = self.layout_events.send(Workspace { name: name.to_string(), tabs: vec![] });
        Ok(FrontResponse::Workspaces { workspaces: self.workspace_summaries() })
    }

    fn workspace_rename(&self, name: &str, to: &str, actor: &str) -> Result<FrontResponse, ErrorBody> {
        let to = to.trim();
        if to.is_empty() {
            return Err(err(ErrorCode::BadRequest, "새 이름이 비었다"));
        }
        let mut w = {
            let mut ws = self.workspaces.lock().unwrap();
            if ws.contains_key(to) {
                return Err(err(ErrorCode::NameTaken, format!("이미 있는 workspace: {to}")));
            }
            ws.remove(name).ok_or_else(|| err(ErrorCode::NotFound, format!("workspace가 없다: {name}")))?
        };
        if let Err(e) = self.db.delete_workspace(name) {
            tracing::warn!(workspace = %name, "workspace 지우기 실패: {e}");
        }
        let _ = self.layout_events.send(Workspace { name: name.to_string(), tabs: vec![] });
        w.name = to.to_string();
        self.save_workspace(&w);
        self.db.audit(actor, "workspace_rename", &format!("{name} -> {to}"));
        Ok(FrontResponse::Workspace(w))
    }

    /// 새 pane의 cwd(herdr `resolve_new_terminal_cwd`): 준 값이 먼저, 없으면 `[terminal] new_cwd`.
    /// follow면 원래 pane(`from`)이 같은 머신에 있을 때 그 pane이 지금 있는 폴더(agent `cwd.v1`),
    /// 읽지 못하면 그 pane을 띄운 폴더(herdr `launch_cwd_for_terminal`). 다른 머신이면 그 머신 기본 위치.
    async fn new_pane_cwd(&self, given: Option<String>, from: Option<&str>, mid: &str) -> Option<String> {
        if given.is_some() {
            return given;
        }
        match self.cfg.terminal.new_cwd.as_str() {
            "follow" => {}
            "home" => return None,
            path => return Some(path.to_string()),
        }
        let (src, _) = self.resolve_pane(from?).ok()?;
        let (conn, launch) = {
            let i = self.inner.lock().unwrap();
            let p = i.panes.get(&src)?;
            if p.machine_id != mid {
                return None;
            }
            let m = i.machines.get(mid);
            let can_ask = p.info.state.is_running() && m.is_some_and(|m| m.features.iter().any(|f| f == "cwd.v1"));
            (m.and_then(|m| m.conn.clone()).filter(|_| can_ask), p.info.cwd.clone())
        };
        if let Some(conn) = conn
            && let Ok(AgentResponse::Cwd { cwd: Some(cwd) }) = conn.request(AgentRequest::Cwd { session: src }).await
        {
            return Some(cwd);
        }
        launch
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
                "host에서 `aaa machine enroll --open 10m`으로 등록 창을 먼저 연다",
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
                health: None,
                features: vec![],
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
                _ if plan.serving.is_none() => Some("아직 공개하지 않았다: aaa web publish".into()),
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
            return Err(err(ErrorCode::NotFound, "aaa가 만든 공개가 없다(다른 서비스의 매핑은 건드리지 않는다)"));
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

/// `aaa host` 실행
pub async fn run(cfg: HostConfig, state_root: PathBuf) -> Result<()> {
    let host = Host::open(cfg, state_root.join("host"))?;
    tracing::info!(host = %host.host_id, fp = %&host.identity.fingerprint[..16], "host 시작");
    link::start(&host).await?;
    front::start(&host).await?;
    web::start(&host).await?;
    host.start_ticker();
    host.start_notifier();
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

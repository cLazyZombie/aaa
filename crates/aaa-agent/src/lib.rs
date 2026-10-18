//! `aaa agent`: agent backend. 이 머신의 세션(session host)을 만들고 감독하며, host와 Agent link로 통신한다.
//!
//! - 외부에 아무것도 listen하지 않는다. host로 나가는 연결만 연다(§4.2 5).
//! - 세션은 session host 프로세스가 소유하므로 agent가 재시작해도 이어진다. 시작할 때 세션 폴더를 다시 읽어 붙는다.
//! - host의 세션 요청은 세션별 pump task가 session host로 중계한다. 응답과 출력이 같은 task를 지나므로 순서가 지켜진다.
//! - Claude·Codex 세션은 pump가 화면을 300ms마다 읽어 상태(작업 중·idle·질문)를 정한다(herdr 0.9.5 감지, plan §8.110).

mod attach;
mod chat;
mod command;
mod cwd;
pub mod detect;
mod files;
mod health;
pub mod hooks;
pub mod identity;
pub mod link;
mod merge;
mod page;
mod pr;
pub mod threads;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use aaa_proto::{
    AgentEvent, AgentRequest, AgentResponse, B64, ErrorBody, ErrorCode, LinkMsg, MAX_FRAME, Outcome, SESSION_IPC,
    ScreenText, SessionInfo, SessionMsg, SessionRequest, SessionState, Snapshot, SpawnSpec,
};
use anyhow::{Context, Result, bail};
use tokio::sync::mpsc;

use aaa_common::conn::{FrameRx, FrameTx, Incoming, spawn_writer};
use aaa_common::util;
use aaa_session::files::{ExitFile, MetaFile, SessionPaths, SpecFile};

enum PumpCmd {
    /// host 요청을 session host로 보낸다. 응답은 pump가 host에 보낸다.
    Host { host_id: String, body: SessionRequest },
    /// host 연결이 끊겼다: 구독을 멈춘다.
    HostGone,
}

struct Entry {
    info: SessionInfo,
    pump: Option<mpsc::Sender<PumpCmd>>,
}

/// 그린 HTML 페이지: 경로·크기·고친 시각과 bytes
type DrawnPage = (String, u64, std::time::SystemTime, Arc<Vec<u8>>);

pub struct Agent {
    pub state: PathBuf,
    pub machine_id: String,
    pub name: String,
    pub boot_id: String,
    pub dev_mode: bool,
    pub default_cwd: Option<String>,
    exe: PathBuf,
    sessions: Mutex<HashMap<String, Entry>>,
    host: Mutex<Option<FrameTx>>,
    pub selfcheck: Mutex<Option<serde_json::Value>>,
    threads_cfg: Mutex<threads::ThreadsConfig>,
    /// 파일 보기가 여는 폴더(plan §8.125)
    file_roots: Mutex<files::Roots>,
    /// 마지막으로 그린 HTML 페이지(경로·크기·고친 시각): host가 조각으로 읽는 동안 다시 그리지 않는다
    page_cache: Mutex<Option<DrawnPage>>,
    /// report.md 감시 task(thread ID별)
    watches: Mutex<HashMap<String, tokio::task::JoinHandle<()>>>,
    /// 세션별로 다시 알릴 harness 사건(host가 다시 붙으면 다시 보낸다). 칸마다(`Slot`) 마지막 하나다.
    /// 세션 폴더 `harness.json`에도 적어 agent가 다시 떠도 남는다.
    harness_last: Mutex<HashMap<String, Vec<aaa_proto::HarnessEvent>>>,
    /// 세션마다 SessionStart 때 적은 harness 세션 ID(다른 세션의 사건을 거른다)
    harness_started: Mutex<HashMap<String, String>>,
    /// 마지막 머신 건강(host가 다시 붙으면 다시 보낸다)
    health: Mutex<Option<aaa_proto::MachineHealth>>,
    /// 지금 host가 건강 보고(`health.v1`)를 받는가
    host_health: std::sync::atomic::AtomicBool,
    /// 세션마다 사람이 붙인 이름(`spec.json`의 `label`). session host는 띄울 때 이름만 알므로(이전 판은 label을 모른다)
    /// 그가 보내는 정보의 이름은 여기서 덮는다.
    labels: Mutex<HashMap<String, String>>,
}

impl Agent {
    pub fn new(
        state: PathBuf,
        machine_id: String,
        name: String,
        dev_mode: bool,
        default_cwd: Option<String>,
    ) -> Result<Arc<Self>> {
        util::ensure_private_dir(&state)?;
        util::ensure_private_dir(&state.join("sessions"))?;
        util::ensure_private_dir(&state.join("run"))?;
        Ok(Arc::new(Agent {
            state,
            machine_id,
            name,
            boot_id: util::boot_id(),
            dev_mode,
            default_cwd,
            exe: std::env::current_exe()?,
            sessions: Mutex::new(HashMap::new()),
            host: Mutex::new(None),
            selfcheck: Mutex::new(None),
            threads_cfg: Mutex::new(threads::ThreadsConfig::default()),
            file_roots: Mutex::new(files::Roots::default()),
            page_cache: Mutex::new(None),
            watches: Mutex::new(HashMap::new()),
            harness_last: Mutex::new(HashMap::new()),
            labels: Mutex::new(HashMap::new()),
            harness_started: Mutex::new(HashMap::new()),
            health: Mutex::new(None),
            host_health: std::sync::atomic::AtomicBool::new(false),
        }))
    }

    /// 세션 안의 `aaa hook`·`aaa report`가 보내는 사건을 받는다(로컬 socket, 같은 사용자만)
    pub fn start_hooks(self: &Arc<Self>) -> Result<()> {
        let listener = aaa_common::ipc::LocalListener::bind(&hooks::socket(&self.state)).context("hook socket")?;
        let me = self.clone();
        tokio::spawn(async move {
            while let Ok(mut s) = listener.accept().await {
                let me = me.clone();
                tokio::spawn(async move {
                    let reply = match me.on_hook(&mut s).await {
                        Ok(()) => "ok\n".to_string(),
                        Err(e) => format!("error: {e:#}\n"),
                    };
                    let _ = tokio::io::AsyncWriteExt::write_all(&mut s, reply.as_bytes()).await;
                    let _ = tokio::io::AsyncWriteExt::shutdown(&mut s).await;
                });
            }
        });
        Ok(())
    }

    async fn on_hook(self: &Arc<Self>, s: &mut aaa_common::ipc::LocalStream) -> Result<()> {
        let msg = hooks::read_msg(s).await?;
        if msg.session.is_empty() || !msg.session.chars().all(|c| c.is_ascii_alphanumeric()) {
            bail!("세션 ID 형식이 아니다");
        }
        let spec: SpecFile = util::read_json(&SessionPaths::new(&self.state, &msg.session).spec())?;
        if hooks::hook_token(&spec.token) != msg.token {
            bail!("hook token이 맞지 않는다");
        }
        let Some(ev) = hooks::normalize(&msg.session, &msg.harness, &msg.payload, util::now_ms()) else {
            return Ok(());
        };
        self.harness_event(ev).await;
        Ok(())
    }

    /// harness 사건을 기억하고(다시 알릴 것이면) host에 보낸다
    async fn harness_event(self: &Arc<Self>, ev: aaa_proto::HarnessEvent) {
        // 이 pane이 시작한 harness 세션과 다른 세션(중첩 Codex thread 등)의 사건은 버린다(herdr-projects `foreign_session`)
        if ev.harness != "report" {
            let mut started = self.harness_started.lock().unwrap();
            if ev.kind == "session_start" {
                if let Some(id) = &ev.harness_session {
                    started.insert(ev.session.clone(), id.clone());
                }
            } else if hooks::foreign_session(started.get(&ev.session).map(String::as_str), &ev) {
                return;
            }
        }
        let ev = if Slot::of(&ev).is_some() {
            let me = self.clone();
            let kept = ev.clone();
            tokio::task::spawn_blocking(move || me.remember_harness(kept)).await.unwrap_or(ev)
        } else {
            ev
        };
        self.send_host(&LinkMsg::Event(AgentEvent::Harness(ev))).await;
    }

    /// 화면 감지가 알린 상태(pump에서)
    async fn detected(self: &Arc<Self>, session: &str, harness: detect::Harness, r: detect::Report) {
        tracing::debug!(%session, kind = r.kind, rule = ?r.rule, "harness 상태(화면 감지)");
        self.harness_event(aaa_proto::HarnessEvent {
            session: session.to_string(),
            harness: harness.label().to_string(),
            kind: r.kind.to_string(),
            harness_session: None,
            message: r.message,
            question_kind: r.question_kind.map(str::to_string),
            percent: None,
            at_ms: util::now_ms(),
            transcript: None,
        })
        .await;
    }

    /// 다시 알릴 사건을 바꾸고 세션 폴더에 적는다. 같은 칸의 앞 사건을 바꾸되, 이번 사건에 없는 기록 위치·
    /// harness 세션 ID는 앞 사건 것을 이어받는다. 돌려준 사건을 host에 보낸다: agent가 다시 떠 같은 상태를 다시
    /// 감지했으면 앞 사건(그 상태가 된 때)을 그대로 보낸다.
    fn remember_harness(&self, mut ev: aaa_proto::HarnessEvent) -> aaa_proto::HarnessEvent {
        let Some(slot) = Slot::of(&ev) else { return ev };
        let mut map = self.harness_last.lock().unwrap();
        let list = map.entry(ev.session.clone()).or_default();
        let send = ev.clone();
        if let Some(i) = list.iter().position(|e| Slot::of(e) == Some(slot)) {
            let same_state = |o: &aaa_proto::HarnessEvent| {
                slot == Slot::State && (&o.harness, &o.kind, &o.message) == (&ev.harness, &ev.kind, &ev.message)
            };
            if same_state(&list[i]) {
                return list[i].clone();
            }
            let old = list.remove(i);
            if old.harness == ev.harness {
                ev.transcript = ev.transcript.or(old.transcript);
                ev.harness_session = ev.harness_session.or(old.harness_session);
            }
        }
        list.push(ev.clone());
        list.sort_by_key(|e| e.at_ms);
        // 파일 쓰기도 lock 안에서 해 순서가 뒤바뀌지 않게 한다
        if let Err(e) = util::write_json(&SessionPaths::new(&self.state, &ev.session).harness(), list) {
            tracing::warn!(session = %ev.session, "harness.json을 쓰지 못했다: {e}");
        }
        send
    }

    /// agent.toml의 thread 설정(작업 root·profile)
    pub fn configure_threads(&self, cfg: threads::ThreadsConfig) {
        *self.threads_cfg.lock().unwrap() = cfg;
    }

    /// 파일 보기가 열 수 있는 폴더(§6.5 허용 폴더, plan §8.125). 없는 폴더는 빠진다.
    pub fn configure_file_roots(&self, roots: Vec<PathBuf>) {
        *self.file_roots.lock().unwrap() = files::Roots::new(roots);
    }

    /// 페이지로 그린 HTML 파일의 한 조각. 파일이 그대로면 마지막으로 그린 것을 다시 쓴다(조각마다 다시 그리지 않는다)
    fn page_chunk(&self, path: &str, offset: u64, len: u32, roots: &files::Roots) -> anyhow::Result<(Vec<u8>, bool)> {
        let (size, modified) = files::page_key(path, roots)?;
        let cached = self
            .page_cache
            .lock()
            .unwrap()
            .as_ref()
            .filter(|c| c.0 == path && c.1 == size && c.2 == modified)
            .map(|c| c.3.clone());
        let bytes = match cached {
            Some(b) => b,
            None => {
                let b = Arc::new(page::render(Path::new(path), roots)?);
                *self.page_cache.lock().unwrap() = Some((path.to_string(), size, modified, b.clone()));
                b
            }
        };
        let start = (offset as usize).min(bytes.len());
        let end = start.saturating_add(len.min(files::MAX_CHUNK) as usize).min(bytes.len());
        Ok((bytes[start..end].to_vec(), end >= bytes.len()))
    }

    /// 파일 보기: 세션이 지금 있는 폴더(없으면 시작 폴더) 기준으로 경로를 찾는다
    async fn file_locate(&self, session: &str, path: String) -> Result<AgentResponse, ErrorBody> {
        let started = {
            let s = self.sessions.lock().unwrap();
            s.get(session).ok_or_else(|| ErrorBody::new(ErrorCode::NotFound, "세션이 없다"))?.info.cwd.clone()
        };
        // 지금 폴더를 못 읽어도 찾는다(참고 `paneContext` 실패: 절대·~ 경로는 그대로 열린다)
        let cwd = match self.session_cwd(session).await {
            Ok(AgentResponse::Cwd { cwd: Some(c) }) => Some(c),
            _ => started,
        };
        let roots = self.file_roots.lock().unwrap().clone();
        let home = aaa_config::paths::home();
        let found =
            tokio::task::spawn_blocking(move || files::locate(&path, cwd.as_deref().map(Path::new), &home, &roots))
                .await
                .map_err(|e| ErrorBody::new(ErrorCode::Internal, e.to_string()))?;
        match found {
            files::Located::Found(info) => Ok(AgentResponse::FileLocated { info: Some(info), candidates: Vec::new() }),
            files::Located::Candidates(candidates) => Ok(AgentResponse::FileLocated { info: None, candidates }),
            files::Located::Missing => Err(ErrorBody::new(ErrorCode::NotFound, "이 경로에 읽을 수 있는 파일이 없다")),
            files::Located::Outside => Err(ErrorBody::new(
                ErrorCode::Forbidden,
                "허용한 폴더 밖의 파일이라 열지 않는다(그 머신 agent.toml [agent] file_roots)",
            )),
        }
    }

    /// report.md hash를 1초마다 보고 바뀌면 host에 알린다. 등록하면 지금 값을 한 번 보낸다.
    fn watch_report(self: &Arc<Self>, thread: String, work_dir: PathBuf) {
        let me = self.clone();
        let t = thread.clone();
        let task = tokio::spawn(async move {
            let mut last: Option<Option<String>> = None;
            loop {
                let wd = work_dir.clone();
                let hash = tokio::task::spawn_blocking(move || threads::report_hash(&wd)).await.unwrap_or(None);
                if last.as_ref() != Some(&hash) {
                    me.send_host(&LinkMsg::Event(AgentEvent::ReportChanged { thread: t.clone(), hash: hash.clone() }))
                        .await;
                    last = Some(hash);
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });
        if let Some(old) = self.watches.lock().unwrap().insert(thread, task) {
            old.abort();
        }
    }

    /// 실행 중인 세션이 지금 있는 폴더(새 탭·나누기 cwd follow). 끝났거나 읽지 못하면 None.
    async fn session_cwd(&self, session: &str) -> Result<AgentResponse, ErrorBody> {
        let pid = {
            let s = self.sessions.lock().unwrap();
            let e = s.get(session).ok_or_else(|| ErrorBody::new(ErrorCode::NotFound, "세션이 없다"))?;
            e.info.state.is_running().then_some(e.info.pid).flatten()
        };
        let cwd = match pid {
            Some(pid) => tokio::task::spawn_blocking(move || cwd::follow_cwd(pid))
                .await
                .map_err(|e| ErrorBody::new(ErrorCode::Internal, e.to_string()))?,
            None => None,
        };
        Ok(AgentResponse::Cwd { cwd: cwd.map(|p| p.to_string_lossy().into_owned()) })
    }

    pub fn sessions(&self) -> Vec<SessionInfo> {
        let mut v: Vec<_> = self.sessions.lock().unwrap().values().map(|e| e.info.clone()).collect();
        v.sort_by(|a, b| a.started_at_ms.cmp(&b.started_at_ms).then(a.id.cmp(&b.id)));
        v
    }

    fn host_tx(&self) -> Option<FrameTx> {
        self.host.lock().unwrap().clone()
    }

    /// 세션별 마지막 harness 사건(시각 차례). hello에 싣거나 옛 host에 다시 보낸다.
    pub fn harness_events(&self) -> Vec<aaa_proto::HarnessEvent> {
        let mut last: Vec<_> = self.harness_last.lock().unwrap().values().flatten().cloned().collect();
        last.sort_by_key(|e| e.at_ms);
        last
    }

    /// `replay`: hello에 실은 harness 사건을 연결 뒤 다시 보낸다(hello의 것을 읽지 않는 옛 host)
    pub fn set_host(&self, tx: Option<FrameTx>, replay: bool) {
        let gone = tx.is_none();
        if let Some(tx) = tx.as_ref().filter(|_| replay) {
            // host가 다시 붙으면 세션별 마지막 harness 상태를 다시 알린다
            let last = self.harness_events();
            let tx = tx.clone();
            tokio::spawn(async move {
                for ev in last {
                    let _ = tx.json(&LinkMsg::Event(AgentEvent::Harness(ev))).await;
                }
            });
        }
        *self.host.lock().unwrap() = tx;
        if gone {
            let pumps: Vec<_> = self.sessions.lock().unwrap().values().filter_map(|e| e.pump.clone()).collect();
            for p in pumps {
                let _ = p.try_send(PumpCmd::HostGone);
            }
        }
    }

    async fn send_host(&self, msg: &LinkMsg) {
        if let Some(tx) = self.host_tx() {
            let _ = tx.json(msg).await;
        }
    }

    /// 시작할 때: 세션 폴더를 읽어 살아 있는 session host에 다시 붙고, 나머지는 종료·중단·유실로 분류한다.
    pub async fn recover(self: &Arc<Self>) -> Result<()> {
        let dir = self.state.join("sessions");
        let mut ids: Vec<String> = std::fs::read_dir(&dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().join("spec.json").exists())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        ids.sort();
        for id in ids {
            let paths = SessionPaths::new(&self.state, &id);
            let spec: SpecFile = match util::read_json(&paths.spec()) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(%id, "spec.json 읽기 실패: {e:#}");
                    continue;
                }
            };
            if let Some(label) = &spec.label {
                self.labels.lock().unwrap().insert(id.clone(), label.clone());
            }
            if let Ok(list) = util::read_json::<Vec<aaa_proto::HarnessEvent>>(&paths.harness()) {
                self.harness_last
                    .lock()
                    .unwrap()
                    .insert(id.clone(), list.into_iter().filter(|e| e.session == id).collect());
            }
            if paths.exit().exists() {
                let info = self.offline_info(&paths, &spec);
                self.sessions.lock().unwrap().insert(id, Entry { info, pump: None });
                continue;
            }
            match self.attach(&paths, &spec).await {
                Ok(_) => tracing::info!(%id, "세션에 다시 붙었다"),
                Err(e) => {
                    let info = self.offline_info(&paths, &spec);
                    tracing::info!(%id, state = ?info.state, "붙지 못했다: {e:#}");
                    self.sessions.lock().unwrap().insert(id, Entry { info, pump: None });
                }
            }
        }
        Ok(())
    }

    /// session host가 없는 세션의 정보(파일에서)
    fn offline_info(&self, paths: &SessionPaths, spec: &SpecFile) -> SessionInfo {
        let meta: Option<MetaFile> = util::read_json(&paths.meta()).ok();
        let exit: Option<ExitFile> = util::read_json(&paths.exit()).ok();
        let state = match &exit {
            Some(x) => SessionState::Exited { code: x.code },
            None if spec.boot_id != self.boot_id => SessionState::Interrupted,
            None => SessionState::Lost,
        };
        self.labeled(SessionInfo {
            id: spec.id.clone(),
            name: spec.spec.name.clone(),
            argv: spec.spec.argv.clone(),
            cwd: spec.spec.cwd.clone(),
            pid: meta.as_ref().and_then(|m| m.child_pid),
            started_at_ms: meta.map(|m| m.started_at_ms).unwrap_or(spec.created_at_ms),
            boot_id: spec.boot_id.clone(),
            cols: exit.as_ref().map(|x| x.cols).unwrap_or(spec.spec.cols),
            rows: exit.as_ref().map(|x| x.rows).unwrap_or(spec.spec.rows),
            state,
            role: spec.spec.role,
            label: None,
            harness: spec.spec.harness.clone(),
        })
    }

    /// session host나 옛 spec이 준 정보에 사람이 붙인 이름을 덮는다
    fn labeled(&self, mut info: SessionInfo) -> SessionInfo {
        if let Some(label) = self.labels.lock().unwrap().get(&info.id) {
            info.name = label.clone();
            info.label = Some(label.clone());
        }
        info
    }

    /// session host에 연결하고 pump를 띄운다.
    async fn attach(self: &Arc<Self>, paths: &SessionPaths, spec: &SpecFile) -> Result<SessionInfo> {
        let stream = aaa_common::ipc::connect(&paths.sock).await.context("session socket 연결")?;
        let (r, w) = tokio::io::split(stream);
        let tx = spawn_writer(w, MAX_FRAME);
        let mut rx = FrameRx::new(r, MAX_FRAME);
        tx.json(&SessionMsg::Hello { proto: SESSION_IPC, token: spec.token.clone() }).await?;
        let welcome: SessionMsg =
            tokio::time::timeout(Duration::from_secs(5), rx.control()).await.context("welcome 대기")??;
        let (proto, info) = match welcome {
            SessionMsg::Welcome { proto, info } => (proto, self.labeled(info)),
            SessionMsg::Reject(e) => bail!("session host가 거부했다: {e}"),
            other => bail!("예상하지 못한 응답: {other:?}"),
        };
        let watch = watch_of(spec, &info, proto);
        let (ptx, prx) = mpsc::channel(256);
        self.sessions.lock().unwrap().insert(info.id.clone(), Entry { info: info.clone(), pump: Some(ptx) });
        let me = self.clone();
        let paths = paths.clone();
        let spec = spec.clone();
        tokio::spawn(async move { me.pump(paths, spec, tx, rx, prx, watch).await });
        Ok(info)
    }

    async fn pump(
        self: Arc<Self>,
        paths: SessionPaths,
        spec: SpecFile,
        tx: FrameTx,
        mut rx: FrameRx<tokio::io::ReadHalf<aaa_common::ipc::LocalStream>>,
        mut cmds: mpsc::Receiver<PumpCmd>,
        mut watch: Option<detect::Watch>,
    ) {
        let id = spec.id.clone();
        let mut pending: HashMap<String, (String, bool)> = HashMap::new();
        let mut forwarding = false;
        let mut seq = 0u64;
        loop {
            let detect_at = watch.as_ref().and_then(detect::Watch::next_at);
            tokio::select! {
                _ = sleep_until(detect_at) => {
                    if let Some((sid, body)) = watch.as_mut().and_then(|w| w.due(std::time::Instant::now()))
                        && tx.json(&SessionMsg::Req { id: sid, body }).await.is_err()
                    {
                        break;
                    }
                }
                cmd = cmds.recv() => match cmd {
                    Some(PumpCmd::Host { host_id, body }) => {
                        seq += 1;
                        let sid = format!("h{seq}");
                        let is_sub = matches!(body, SessionRequest::Subscribe { .. });
                        if matches!(body, SessionRequest::Unsubscribe) { forwarding = false; }
                        pending.insert(sid.clone(), (host_id, is_sub));
                        if tx.json(&SessionMsg::Req { id: sid, body }).await.is_err() { break; }
                    }
                    Some(PumpCmd::HostGone) => {
                        if forwarding {
                            forwarding = false;
                            seq += 1;
                            let _ = tx.json(&SessionMsg::Req { id: format!("u{seq}"), body: SessionRequest::Unsubscribe }).await;
                        }
                    }
                    None => break,
                },
                incoming = rx.next() => match incoming {
                    Ok(Some(Incoming::Terminal(chunk))) => {
                        if forwarding {
                            match self.host_tx() {
                                Some(h) => { if h.terminal(&chunk).await.is_err() { forwarding = false; } }
                                None => forwarding = false,
                            }
                        }
                    }
                    Ok(Some(Incoming::Control(b))) => match serde_json::from_slice::<SessionMsg>(&b) {
                        Ok(SessionMsg::Res { id: sid, result }) => {
                            if let Some(w) = watch.as_mut().filter(|w| w.owns(&sid)) {
                                let harness = w.harness();
                                if let Some(r) = w.answer(std::time::Instant::now(), result) {
                                    self.detected(&id, harness, r).await;
                                }
                            } else if let Some((host_id, is_sub)) = pending.remove(&sid) {
                                if is_sub && matches!(result, Outcome::Ok(_)) { forwarding = true; }
                                self.send_host(&LinkMsg::Res { id: host_id, result }).await;
                            }
                        }
                        Ok(SessionMsg::Updated(info)) => {
                            let info = self.labeled(info);
                            if let Some(e) = self.sessions.lock().unwrap().get_mut(&id) { e.info = info.clone(); }
                            self.send_host(&LinkMsg::Event(AgentEvent::SessionUpdated(info))).await;
                        }
                        Ok(SessionMsg::Resync) => {
                            forwarding = false;
                            self.send_host(&LinkMsg::Event(AgentEvent::Resync { session: id.clone() })).await;
                        }
                        Ok(other) => tracing::debug!(%id, "무시한 session 메시지: {other:?}"),
                        Err(e) => { tracing::warn!(%id, "session 메시지 해석 실패: {e}"); break; }
                    },
                    Ok(None) | Err(_) => break,
                },
            }
        }
        // session host가 사라졌다: 종료 기록을 기다렸다 읽는다.
        for _ in 0..20 {
            if paths.exit().exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let info = self.offline_info(&paths, &spec);
        // harness가 끝났다(herdr는 프로세스가 사라지면 agent를 놓는다. aaa 세션은 harness 하나라 세션 끝이 그것이다)
        if let Some(w) = &watch
            && !info.state.is_running()
        {
            let r = detect::Report { kind: "exited", message: None, question_kind: None, rule: None };
            self.detected(&id, w.harness(), r).await;
        }
        if let Some(e) = self.sessions.lock().unwrap().get_mut(&id) {
            e.info = info.clone();
            e.pump = None;
        }
        for (_, (host_id, _)) in pending {
            let err = ErrorBody::new(ErrorCode::SessionExited, "세션이 끝났다");
            self.send_host(&LinkMsg::Res { id: host_id, result: Outcome::Err(err) }).await;
        }
        self.send_host(&LinkMsg::Event(AgentEvent::SessionUpdated(info))).await;
    }

    /// 새 세션: spec을 쓰고 session host를 분리된 프로세스로 띄운 뒤 붙는다.
    pub async fn spawn_session(self: &Arc<Self>, mut spec: SpawnSpec) -> Result<SessionInfo> {
        let id = util::new_id();
        let paths = SessionPaths::new(&self.state, &id);
        util::ensure_private_dir(&paths.dir)?;
        if spec.cwd.is_none() {
            spec.cwd = self.default_cwd.clone();
        }
        // 웹 새 세션 창은 `~/…`도 받는다(herdr-web-ui 「absolute path or ~/…」): 이 머신 사용자의 집으로 푼다
        if let Some(rest) = spec.cwd.as_deref().and_then(|c| c.strip_prefix('~'))
            && (rest.is_empty() || rest.starts_with('/'))
            && let Some(home) = std::env::var_os("HOME")
        {
            spec.cwd = Some(format!("{}{rest}", home.to_string_lossy()));
        }
        if let Some(cwd) = &spec.cwd
            && !Path::new(cwd).is_dir()
        {
            bail!("작업 폴더가 없다: {cwd}");
        }
        // profile로 띄우기(웹 새 세션 창의 agent 고르기): thread처럼 profile을 argv·env로 풀고 상태 hook을 붙인다.
        // hook 설정 파일은 thread 작업 폴더 대신 이 세션 폴더에 둔다.
        if let Some(name) = spec.profile.clone() {
            let cfg = self.threads_cfg.lock().unwrap().clone();
            let launch =
                threads::with_hooks(&cfg, threads::resolve(&name, threads::find_profile(&cfg, &name)?)?, &paths.dir)?;
            // thread처럼 profile harness의 화면으로 상태를 본다(host orch `harness: Some(launch.harness)`, plan §8.110)
            spec.harness.get_or_insert(launch.harness);
            spec.argv = launch.argv;
            spec.env.extend(launch.env);
            if spec.name.is_empty() {
                spec.name = name;
            }
        }
        if spec.name.is_empty() {
            spec.name = spec
                .argv
                .first()
                .map(|a| a.rsplit(['/', '\\']).next().unwrap_or(a).to_string())
                .unwrap_or_else(|| "shell".into());
        }
        let token = hex::encode(rand::random::<[u8; 16]>());
        // 세션 안의 `aaa hook`·`aaa report`가 이 agent에 사건을 보낼 곳
        spec.env.push((hooks::ENV_SOCK.into(), hooks::socket(&self.state).to_string_lossy().to_string()));
        spec.env.push((hooks::ENV_TOKEN.into(), hooks::hook_token(&token)));
        let file = SpecFile {
            id: id.clone(),
            token,
            boot_id: self.boot_id.clone(),
            created_at_ms: util::now_ms(),
            spec,
            label: None,
        };
        util::write_json(&paths.spec(), &file)?;
        let log = std::fs::File::create(paths.log())?;
        let mut cmd = std::process::Command::new(&self.exe);
        cmd.arg("session-host").arg("--dir").arg(&paths.dir);
        cmd.stdin(std::process::Stdio::null()).stdout(log.try_clone()?).stderr(log);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const DETACHED_PROCESS: u32 = 0x0000_0008;
            const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
            cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
        }
        let mut child = cmd.spawn().context("session host 실행")?;
        let pid = child.id();
        // 좀비가 남지 않게 기다리는 thread. agent가 먼저 끝나면 OS 감독자가 거둔다.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            match self.attach(&paths, &file).await {
                Ok(info) => {
                    self.send_host(&LinkMsg::Event(AgentEvent::SessionStarted(info.clone()))).await;
                    return Ok(info);
                }
                Err(_) if paths.exit().exists() => {
                    // 붙기 전에 이미 끝났다: 종료한 세션으로 등록한다(마지막 화면은 파일에 있다).
                    let info = self.offline_info(&paths, &file);
                    self.sessions.lock().unwrap().insert(id.clone(), Entry { info: info.clone(), pump: None });
                    self.send_host(&LinkMsg::Event(AgentEvent::SessionStarted(info.clone()))).await;
                    return Ok(info);
                }
                Err(e) if tokio::time::Instant::now() < deadline && util::pid_alive(pid) => {
                    tracing::trace!("session host 대기: {e:#}");
                    tokio::time::sleep(Duration::from_millis(40)).await;
                }
                Err(e) => {
                    let log = std::fs::read_to_string(paths.log()).unwrap_or_default();
                    bail!("session host가 준비되지 않았다: {e:#}\n{log}");
                }
            }
        }
    }

    /// host 요청 하나. 세션 요청은 pump로 넘기고(응답은 pump가 보낸다), 나머지는 여기서 답한다.
    pub async fn handle(self: Arc<Self>, id: String, body: AgentRequest) {
        let direct: Result<AgentResponse, ErrorBody> = match body {
            AgentRequest::Spawn(spec) => self
                .spawn_session(spec)
                .await
                .map(AgentResponse::Session)
                .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}"))),
            AgentRequest::List => Ok(AgentResponse::Sessions { sessions: self.sessions() }),
            AgentRequest::Subscribe { session, from } => {
                return self.to_session(id, &session, SessionRequest::Subscribe { from }).await;
            }
            AgentRequest::Unsubscribe { session } => {
                return self.to_session(id, &session, SessionRequest::Unsubscribe).await;
            }
            AgentRequest::Input { session, data } => {
                return self.to_session(id, &session, SessionRequest::Input { data }).await;
            }
            AgentRequest::Resize { session, cols, rows } => {
                return self.to_session(id, &session, SessionRequest::Resize { cols, rows }).await;
            }
            AgentRequest::Kill { session } => return self.to_session(id, &session, SessionRequest::Kill).await,
            AgentRequest::Screen { session } => return self.to_session(id, &session, SessionRequest::Screen).await,
            AgentRequest::History { session, anchor, depth } => {
                return self.to_session(id, &session, SessionRequest::History { anchor, depth }).await;
            }
            AgentRequest::Forget { session } => self.forget(&session).await,
            AgentRequest::Rename { session, label } => self.rename(&session, label).await,
            AgentRequest::Cwd { session } => self.session_cwd(&session).await,
            AgentRequest::FileLocate { session, path } => self.file_locate(&session, path).await,
            AgentRequest::FileView { path, offset, len, page } => {
                let roots = self.file_roots.lock().unwrap().clone();
                let me = self.clone();
                tokio::task::spawn_blocking(move || {
                    if page {
                        me.page_chunk(&path, offset, len, &roots)
                    } else {
                        files::read(&path, offset, len, &roots)
                    }
                })
                .await
                .map_err(|e| ErrorBody::new(ErrorCode::Internal, e.to_string()))
                .and_then(|r| r.map_err(|e| ErrorBody::new(ErrorCode::NotFound, format!("{e:#}"))))
                .map(|(data, eof)| AgentResponse::FileChunk { data: B64(data), eof })
            }
            AgentRequest::Chat { harness, transcript, harness_session, thinking } => {
                tokio::task::spawn_blocking(move || {
                    chat::read(&harness, transcript.as_deref(), harness_session.as_deref(), thinking)
                })
                .await
                .map_err(|e| ErrorBody::new(ErrorCode::Internal, e.to_string()))
                .and_then(|r| {
                    r.map(|(messages, truncated)| AgentResponse::Chat { messages, truncated })
                        .map_err(|e| ErrorBody::new(ErrorCode::NotFound, e))
                })
            }
            AgentRequest::Profiles => {
                Ok(AgentResponse::Profiles { profiles: threads::profile_views(&self.threads_cfg.lock().unwrap()) })
            }
            AgentRequest::ThreadPrepare(p) => {
                let cfg = self.threads_cfg.lock().unwrap().clone();
                threads::prepare(&cfg, p)
                    .await
                    .map(AgentResponse::ThreadReady)
                    .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}")))
            }
            AgentRequest::ThreadWatch { thread, work_dir } => {
                // 감시 key는 `<project>/<thread>`다(thread 번호는 프로젝트마다 다시 센다)
                let ok = thread
                    .split_once('/')
                    .is_some_and(|(p, t)| aaa_proto::valid_project(p) && aaa_proto::valid_thread(t));
                if ok {
                    self.watch_report(thread, PathBuf::from(work_dir));
                    Ok(AgentResponse::Done)
                } else {
                    Err(ErrorBody::new(ErrorCode::BadRequest, "감시 key는 <project>/<thread> 형식이다"))
                }
            }
            AgentRequest::ThreadUnwatch { thread } => {
                if let Some(t) = self.watches.lock().unwrap().remove(&thread) {
                    t.abort();
                }
                Ok(AgentResponse::Done)
            }
            AgentRequest::ReportCollect { work_dir, max_library_bytes } => {
                tokio::task::spawn_blocking(move || threads::collect(&work_dir, max_library_bytes))
                    .await
                    .map_err(|e| ErrorBody::new(ErrorCode::Internal, e.to_string()))
                    .and_then(|r| r.map_err(|e| ErrorBody::new(ErrorCode::NotFound, format!("{e:#}"))))
                    .map(AgentResponse::Report)
            }
            AgentRequest::FileRead { work_dir, path, offset, len } => {
                tokio::task::spawn_blocking(move || threads::read_chunk(&work_dir, &path, offset, len))
                    .await
                    .map_err(|e| ErrorBody::new(ErrorCode::Internal, e.to_string()))
                    .and_then(|r| r.map_err(|e| ErrorBody::new(ErrorCode::NotFound, format!("{e:#}"))))
                    .map(|(data, eof)| AgentResponse::FileChunk { data: B64(data), eof })
            }
            AgentRequest::ThreadRebrief { work_dir, brief, launch, profile } => {
                let cfg = self.threads_cfg.lock().unwrap().clone();
                threads::rebrief(&cfg, &work_dir, &brief, launch, profile.as_deref())
                    .map(AgentResponse::Relaunch)
                    .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}")))
            }
            AgentRequest::RunCommand { project, cwd, command, timeout_ms } => {
                let root = self.threads_cfg.lock().unwrap().work_root.clone();
                command::run(&root, &project, cwd.as_deref(), &command, Duration::from_millis(timeout_ms))
                    .await
                    .map(AgentResponse::CommandRan)
                    .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}")))
            }
            AgentRequest::WorkFileWrite { work_dir, path, data } => {
                tokio::task::spawn_blocking(move || threads::write_inbox(&work_dir, &path, &data.0))
                    .await
                    .map_err(|e| ErrorBody::new(ErrorCode::Internal, e.to_string()))
                    .and_then(|r| r.map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}"))))
                    .map(|()| AgentResponse::Done)
            }
            AgentRequest::AttachWrite { session, att, offset, data, eof } => {
                self.attach_write(&session, &att, offset, data.0, eof).await
            }
            AgentRequest::MergeLand { repo, cwd, branch, remote, target, message } => {
                merge::land(&repo, &cwd, &branch, &remote, &target, &message)
                    .await
                    .map(AgentResponse::Landed)
                    .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}")))
            }
            AgentRequest::ThreadCleanup { repo, cwd, branch, merged_head, keep_worktree } => {
                threads::cleanup(repo.as_deref(), &cwd, branch.as_deref(), merged_head.as_deref(), keep_worktree)
                    .await
                    .map(AgentResponse::Cleanup)
                    .map_err(|e| ErrorBody::new(ErrorCode::Internal, format!("{e:#}")))
            }
            AgentRequest::PrQuery { cwd, query } => pr::query(&cwd, &query)
                .await
                .map(AgentResponse::Pr)
                .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}"))),
        };
        self.send_host(&LinkMsg::Res { id, result: direct.into() }).await;
    }

    /// 실행 중인 세션 폴더에 첨부 한 조각을 쓴다
    async fn attach_write(
        &self,
        session: &str,
        att: &str,
        offset: u64,
        data: Vec<u8>,
        eof: bool,
    ) -> Result<AgentResponse, ErrorBody> {
        if session.is_empty() || !session.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(ErrorBody::new(ErrorCode::BadRequest, "세션 ID 형식이 아니다"));
        }
        if !self.sessions.lock().unwrap().get(session).is_some_and(|e| e.info.state.is_running()) {
            return Err(ErrorBody::new(ErrorCode::NotFound, format!("실행 중인 세션이 아니다: {session}")));
        }
        let dir = SessionPaths::new(&self.state, session).dir;
        let att = att.to_string();
        tokio::task::spawn_blocking(move || attach::write(&dir, &att, offset, &data, eof))
            .await
            .map_err(|e| ErrorBody::new(ErrorCode::Internal, e.to_string()))?
            .map(|r| match r {
                Some((path, sha256)) => AgentResponse::Attached { path, sha256 },
                None => AgentResponse::Done,
            })
            .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}")))
    }

    /// 끝난 세션을 지울 때 session host가 닫히기를 기다리는 한도(session host는 종료 뒤 2초 더 응답한다)
    const FORGET_WAIT: Duration = Duration::from_secs(5);

    /// 끝난 세션 기록(폴더)을 지운다. 실행 중이면 거부한다.
    /// session host는 종료를 알린 뒤에도 2초 더 응답한다(바로 끝난 명령도 마지막 화면을 받게). 그동안은 pump가
    /// 붙어 있으니 떨어질 때까지 기다렸다 지운다. 먼저 지우면 pump가 끝나며 보내는 종료 알림이 기록을 되살린다.
    /// 사람이 붙인 이름을 `spec.json`에 적고(지우면 뺀다) 바뀐 정보를 host에 알린다. 끝난 세션에도 붙인다(다시 열면 그 이름).
    async fn rename(&self, session: &str, label: Option<String>) -> Result<AgentResponse, ErrorBody> {
        if session.is_empty() || !session.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(ErrorBody::new(ErrorCode::BadRequest, "세션 ID 형식이 아니다"));
        }
        let label = label.map(|l| l.trim().to_string()).filter(|l| !l.is_empty());
        if label
            .as_ref()
            .is_some_and(|l| l.chars().count() > aaa_proto::MAX_PANE_LABEL_CHARS || l.chars().any(char::is_control))
        {
            return Err(ErrorBody::new(
                ErrorCode::BadRequest,
                format!("이름은 제어 문자 없이 {}자 안이어야 한다", aaa_proto::MAX_PANE_LABEL_CHARS),
            ));
        }
        if !self.sessions.lock().unwrap().contains_key(session) {
            return Err(ErrorBody::new(ErrorCode::NotFound, format!("세션이 없다: {session}")));
        }
        let paths = SessionPaths::new(&self.state, session);
        let mut spec: SpecFile = util::read_json(&paths.spec())
            .map_err(|e| ErrorBody::new(ErrorCode::Internal, format!("spec.json 읽기: {e:#}")))?;
        spec.label = label.clone();
        util::write_json(&paths.spec(), &spec)
            .map_err(|e| ErrorBody::new(ErrorCode::Internal, format!("spec.json 쓰기: {e:#}")))?;
        match &label {
            Some(l) => self.labels.lock().unwrap().insert(session.to_string(), l.clone()),
            None => self.labels.lock().unwrap().remove(session),
        };
        let info = {
            let mut map = self.sessions.lock().unwrap();
            let Some(e) = map.get_mut(session) else {
                return Err(ErrorBody::new(ErrorCode::NotFound, format!("세션이 없다: {session}")));
            };
            e.info.name = spec.name().to_string();
            e.info.label = label;
            e.info.clone()
        };
        self.send_host(&LinkMsg::Event(AgentEvent::SessionUpdated(info.clone()))).await;
        Ok(AgentResponse::Session(info))
    }

    async fn forget(&self, session: &str) -> Result<AgentResponse, ErrorBody> {
        if session.is_empty() || !session.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(ErrorBody::new(ErrorCode::BadRequest, "세션 ID 형식이 아니다"));
        }
        let deadline = tokio::time::Instant::now() + Self::FORGET_WAIT;
        loop {
            {
                let mut map = self.sessions.lock().unwrap();
                match map.get(session) {
                    None => return Err(ErrorBody::new(ErrorCode::NotFound, format!("세션이 없다: {session}"))),
                    Some(e) if e.info.state.is_running() => {
                        return Err(ErrorBody::new(ErrorCode::BadRequest, "실행 중인 세션은 지울 수 없다"));
                    }
                    Some(e) if e.pump.is_none() => {
                        map.remove(session);
                        break;
                    }
                    Some(_) => {}
                }
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(ErrorBody::new(ErrorCode::BadRequest, "session host가 아직 닫히지 않았다"));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        self.harness_last.lock().unwrap().remove(session);
        self.labels.lock().unwrap().remove(session);
        let paths = SessionPaths::new(&self.state, session);
        if let Err(e) = std::fs::remove_dir_all(&paths.dir) {
            tracing::warn!(%session, "세션 폴더 삭제 실패: {e}");
        }
        Ok(AgentResponse::Done)
    }

    async fn to_session(&self, id: String, session: &str, body: SessionRequest) {
        let (pump, info) = {
            let map = self.sessions.lock().unwrap();
            match map.get(session) {
                Some(e) => (e.pump.clone(), Some(e.info.clone())),
                None => (None, None),
            }
        };
        let result = match (pump, info) {
            (Some(p), _) => {
                if p.send(PumpCmd::Host { host_id: id.clone(), body }).await.is_ok() {
                    return;
                }
                Err(ErrorBody::new(ErrorCode::SessionExited, "세션이 끝났다"))
            }
            (None, Some(info)) => self.offline_request(&info, body),
            (None, None) => Err(ErrorBody::new(ErrorCode::NotFound, format!("세션이 없다: {session}"))),
        };
        self.send_host(&LinkMsg::Res { id, result: result.into() }).await;
    }

    /// 끝난 세션: 마지막 화면 파일로 답한다.
    fn offline_request(&self, info: &SessionInfo, body: SessionRequest) -> Result<AgentResponse, ErrorBody> {
        let paths = SessionPaths::new(&self.state, &info.id);
        match body {
            SessionRequest::Subscribe { .. } => {
                let replay = std::fs::read(paths.final_replay()).unwrap_or_default();
                let end = util::read_json::<ExitFile>(&paths.exit()).map(|x| x.end_offset).unwrap_or(0);
                Ok(AgentResponse::Subscribed {
                    session: info.id.clone(),
                    snapshot: Some(Snapshot {
                        session: info.id.clone(),
                        offset: end,
                        cols: info.cols,
                        rows: info.rows,
                        replay: B64(replay),
                        history_more: false,
                    }),
                    offset: end,
                })
            }
            SessionRequest::Unsubscribe => Ok(AgentResponse::Done),
            SessionRequest::Screen => util::read_json::<ScreenText>(&paths.final_screen())
                .map(AgentResponse::Screen)
                .map_err(|_| ErrorBody::new(ErrorCode::SessionExited, "마지막 화면 기록이 없다")),
            _ => Err(ErrorBody::new(ErrorCode::SessionExited, "세션이 끝났다")),
        }
    }
}

/// 다시 알릴 harness 사건의 칸. 칸마다 마지막 하나를 남긴다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    /// 진행 보고(`aaa report`)
    Report,
    /// harness 세션 ID·기록 위치(SessionStart hook, Codex notify)
    Session,
    /// 상태(화면 감지, 끝남). 세션 ID 사건이 이것을 덮으면 다시 붙은 host가 상태를 잃는다.
    State,
}

impl Slot {
    /// 다시 알리지 않는 사건(입력 제출 같은 한때 신호)은 None
    fn of(e: &aaa_proto::HarnessEvent) -> Option<Slot> {
        match e.kind.as_str() {
            _ if e.harness == "report" => Some(Slot::Report),
            "session_start" | "session" => Some(Slot::Session),
            "prompt_submit" => None,
            _ => Some(Slot::State),
        }
    }
}

/// 화면으로 상태를 볼 세션이면 감지 차례. session host가 SESSION_IPC 1.1 이상일 때만 `Detect`를 보낸다
/// (그 전 session host는 모르는 요청을 받으면 연결을 끊는다).
fn watch_of(spec: &SpecFile, info: &SessionInfo, proto: aaa_proto::Version) -> Option<detect::Watch> {
    if !info.state.is_running() {
        return None;
    }
    let harness = detect::Harness::of_session(spec.spec.harness.as_deref(), &spec.spec.argv)?;
    let now = std::time::Instant::now();
    // 띄운 지 오래된 세션(agent가 다시 떠 붙었다)은 시작 유예가 이미 끝났다
    let age = Duration::from_millis(util::now_ms().saturating_sub(info.started_at_ms));
    let started = now.checked_sub(age).unwrap_or(now);
    let detect_ipc = proto.major == SESSION_IPC.major && proto.minor >= aaa_proto::SESSION_IPC_DETECT;
    Some(detect::Watch::new(harness, started, detect_ipc, now))
}

/// `at`까지 잔다(None이면 깨지 않는다)
async fn sleep_until(at: Option<std::time::Instant>) {
    match at {
        Some(t) => tokio::time::sleep_until(tokio::time::Instant::from_std(t)).await,
        None => std::future::pending().await,
    }
}

/// 시작 때 자가 점검(§10.4 6): 계보·키체인·FDA·첫 실행 지연. 결과는 상태 폴더 selfcheck.json과 다음 hello에 담는다.
/// macOS에서는 App Nap·자동 종료를 막는 활동도 이때 잡는다.
pub fn start_selfcheck(agent: &Arc<Agent>, cfg: &aaa_config::SelfcheckSection) {
    #[cfg(target_os = "macos")]
    let held = aaa_doctor::macos::hold_activity("aaa agent backend: agent 세션 실행");
    #[cfg(not(target_os = "macos"))]
    let held = false;
    let opts = aaa_doctor::ProbeOptions {
        samples: cfg.first_exec_samples,
        keychain_service: Some(cfg.keychain_service.clone()).filter(|s| !s.is_empty()),
        volume_dir: Some(cfg.volume_dir.clone()).filter(|s| !s.is_empty()),
        label: Some("agent-selfcheck".into()),
        throughput: false,
        activity_held: held,
        fda_check: cfg!(target_os = "macos"),
    };
    let agent = agent.clone();
    tokio::task::spawn_blocking(move || {
        let r = aaa_doctor::probe(&opts);
        let _ = util::write_json(&agent.state.join("selfcheck.json"), &r);
        let summary = serde_json::json!({
            "pass": r.pass,
            "reasons": r.reasons,
            "responsible_path": r.lineage.responsible_path,
            "first_exec": r.first_exec,
            "keychain_unlocked": r.keychain.default_unlocked,
            "fda": r.fda,
            "activity_held": r.activity_held,
            "dev_mode": agent.dev_mode,
        });
        if !r.pass {
            tracing::warn!("자가 점검 실패: {:?}", r.reasons);
        }
        *agent.selfcheck.lock().unwrap() = Some(summary);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(harness: Option<&str>, argv: &[&str], state: SessionState) -> (SpecFile, SessionInfo) {
        let spec = SpawnSpec {
            name: "s".into(),
            argv: argv.iter().map(|a| a.to_string()).collect(),
            cwd: None,
            env: vec![],
            cols: 80,
            rows: 24,
            role: None,
            harness: harness.map(str::to_string),
            profile: None,
        };
        let info = SessionInfo {
            id: "s".into(),
            name: "s".into(),
            argv: spec.argv.clone(),
            cwd: None,
            pid: Some(1),
            // 오래전에 띄운 세션(시작 유예가 끝났다)
            started_at_ms: 1,
            boot_id: "b".into(),
            cols: 80,
            rows: 24,
            state,
            role: None,
            label: None,
            harness: spec.harness.clone(),
        };
        (SpecFile { id: "s".into(), token: "t".into(), boot_id: "b".into(), created_at_ms: 1, spec, label: None }, info)
    }

    /// 감지 요청: 유예가 끝난 차례 다음의 첫 요청
    fn first_request(w: &mut detect::Watch) -> SessionRequest {
        let now = std::time::Instant::now();
        assert!(w.due(now).is_none());
        w.due(w.next_at().unwrap()).unwrap().1
    }

    #[test]
    fn detection_asks_old_session_hosts_only_for_the_screen() {
        let v = |minor| aaa_proto::Version { major: SESSION_IPC.major, minor };
        let running = SessionState::Running;
        let (spec, info) = session(Some("codex"), &["/opt/bin/codex", "-c", "x"], running.clone());
        // SESSION_IPC 1.0 session host는 `Detect`를 모르고 연결을 끊는다: 보이는 화면만 묻는다
        let mut old = watch_of(&spec, &info, v(0)).unwrap();
        assert_eq!(old.harness(), detect::Harness::Codex);
        assert_eq!(first_request(&mut old), SessionRequest::Screen);
        let mut new = watch_of(&spec, &info, v(aaa_proto::SESSION_IPC_DETECT)).unwrap();
        assert_eq!(first_request(&mut new), SessionRequest::Detect { unchanged_since: None });
        // profile harness가 shell·custom이면 실행 파일 이름으로, 그것도 모르면 보지 않는다
        let (spec, info) = session(Some("custom"), &["/x/claude", "--settings", "f"], running.clone());
        assert_eq!(watch_of(&spec, &info, v(1)).map(|w| w.harness()), Some(detect::Harness::Claude));
        let (spec, info) = session(None, &["/bin/sh"], running);
        assert!(watch_of(&spec, &info, v(1)).is_none());
        let (spec, info) = session(Some("claude"), &["claude"], SessionState::Exited { code: Some(0) });
        assert!(watch_of(&spec, &info, v(1)).is_none(), "끝난 세션은 보지 않는다");
    }
}

//! `aaa session-host`: 세션 하나의 PTY master·자식 handle·VT 상태·출력 보존을 소유한다.
//!
//! agent backend가 띄우지만 자기 세션(setsid)으로 분리되어 agent·host가 재시작해도 살아 있다.
//! agent는 세션별 IPC로 다시 붙는다(token 확인). 자식이 끝나면 마지막 화면을 파일로 남기고 끝난다.

use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::Duration;

use aaa_proto::{
    AgentResponse, B64, DetectScreen, ErrorBody, ErrorCode, HistoryChunk, MAX_FRAME, Outcome, SESSION_IPC, SessionInfo,
    SessionMsg, SessionRequest, SessionState, Snapshot, TerminalChunk, negotiate,
};
use anyhow::{Context, Result};
use portable_pty::{CommandBuilder, PtySize};
use tokio::sync::mpsc;

use super::files::{ExitFile, MetaFile, SessionPaths, SpecFile};
use crate::ring::Ring;
use aaa_common::conn::{FrameRx, FrameTx, Incoming, spawn_writer};
use aaa_common::util;
use aaa_vt::{DsrScanner, Vt};

/// 출력 보존 상한(재접속 때 이 범위 안이면 bytes만 이어 보낸다)
pub const RING_BYTES: usize = 2 << 20;
/// 한 터미널 frame에 담는 최대 출력
const CHUNK: usize = 32 << 10;

/// 자식 종료. Windows는 portable-pty 0.9의 `WinChildKiller::kill`이 `TerminateProcess` 성공을
/// 실패로 돌려주므로(반환값 판정이 뒤집힘) spawn 직후 연 handle로 직접 끝낸다.
struct Killer {
    #[cfg_attr(windows, allow(dead_code))]
    pty: Box<dyn portable_pty::ChildKiller + Send + Sync>,
    #[cfg(windows)]
    handle: Option<win::ProcHandle>,
}

impl Killer {
    fn new(pty: Box<dyn portable_pty::ChildKiller + Send + Sync>, child_pid: Option<u32>) -> Killer {
        #[cfg(windows)]
        {
            Killer { pty, handle: child_pid.and_then(win::ProcHandle::open) }
        }
        #[cfg(not(windows))]
        {
            let _ = child_pid;
            Killer { pty }
        }
    }

    fn kill(&mut self) -> std::io::Result<()> {
        #[cfg(windows)]
        if let Some(h) = &self.handle {
            return h.terminate();
        }
        self.pty.kill()
    }
}

#[cfg(windows)]
mod win {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_TERMINATE, TerminateProcess};

    pub struct ProcHandle(HANDLE);
    // HANDLE은 이 구조체만 갖고 닫으므로 thread 사이에 옮겨도 된다
    unsafe impl Send for ProcHandle {}
    unsafe impl Sync for ProcHandle {}

    impl ProcHandle {
        /// 자식이 살아 있는 동안(spawn 직후) 열어야 PID 재사용과 섞이지 않는다
        pub fn open(pid: u32) -> Option<ProcHandle> {
            let h = unsafe { OpenProcess(PROCESS_TERMINATE, 0, pid) };
            (!h.is_null()).then_some(ProcHandle(h))
        }

        pub fn terminate(&self) -> std::io::Result<()> {
            if unsafe { TerminateProcess(self.0, 1) } != 0 { Ok(()) } else { Err(std::io::Error::last_os_error()) }
        }
    }

    impl Drop for ProcHandle {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.0) };
        }
    }
}

enum Cmd {
    Connected { conn: u64, tx: FrameTx, proto: aaa_proto::Version },
    Disconnected { conn: u64 },
    Req { conn: u64, id: String, body: SessionRequest },
}

struct State {
    id: String,
    spec: SpecFile,
    child_pid: Option<u32>,
    started_at_ms: u64,
    vt: Vt,
    /// 화면 변화 순번(출력·크기 변경). agent의 상태 감지가 그대로인 화면을 다시 읽지 않게 한다(herdr `detection_content_seq`).
    screen_seq: u64,
    dsr: DsrScanner,
    ring: Ring,
    conns: HashMap<u64, FrameTx>,
    subs: HashSet<u64>,
    exit: Option<Option<i32>>,
    master: Box<dyn portable_pty::MasterPty + Send>,
    killer: Killer,
    input: std::sync::mpsc::Sender<Vec<u8>>,
}

impl State {
    fn info(&self) -> SessionInfo {
        let (rows, cols) = self.vt.size();
        SessionInfo {
            id: self.id.clone(),
            name: self.spec.name().to_string(),
            argv: self.spec.spec.argv.clone(),
            cwd: self.spec.spec.cwd.clone(),
            pid: self.child_pid,
            started_at_ms: self.started_at_ms,
            boot_id: self.spec.boot_id.clone(),
            cols,
            rows,
            state: match self.exit {
                None => SessionState::Running,
                Some(code) => SessionState::Exited { code },
            },
            role: self.spec.spec.role,
            label: self.spec.label.clone(),
            harness: self.spec.spec.harness.clone(),
        }
    }

    fn snapshot(&self) -> Snapshot {
        let (rows, cols) = self.vt.size();
        let (replay, history_more) = self.vt.replay_more();
        Snapshot { session: self.id.clone(), offset: self.ring.end(), cols, rows, replay: B64(replay), history_more }
    }

    fn output(&mut self, data: &[u8]) {
        if !data.is_empty() {
            self.screen_seq += 1;
        }
        let offset = self.ring.end();
        self.ring.push(data);
        // 커서 위치 질의에는 질의 시점의 커서로 답한다(front가 아니라 session host가 터미널이다)
        let mut start = 0;
        for end in self.dsr.scan(data) {
            self.vt.feed(&data[start..end]);
            let _ = self.input.send(self.vt.cursor_report());
            start = end;
        }
        self.vt.feed(&data[start..]);
        let mut lagging = Vec::new();
        for conn in &self.subs {
            let Some(tx) = self.conns.get(conn) else { continue };
            for (i, part) in data.chunks(CHUNK).enumerate() {
                let c = TerminalChunk {
                    session: self.id.clone(),
                    offset: offset + (i * CHUNK) as u64,
                    data: part.to_vec(),
                };
                match tx.try_terminal(&c) {
                    Ok(true) => {}
                    Ok(false) | Err(_) => {
                        lagging.push(*conn);
                        break;
                    }
                }
            }
        }
        for conn in lagging {
            self.subs.remove(&conn);
            if let Some(tx) = self.conns.get(&conn).cloned() {
                tokio::spawn(async move {
                    let _ = tx.json(&SessionMsg::Resync).await;
                });
            }
        }
    }

    async fn handle(&mut self, conn: u64, id: String, body: SessionRequest) {
        let Some(tx) = self.conns.get(&conn).cloned() else { return };
        let result: Result<AgentResponse, ErrorBody> = match body {
            SessionRequest::Subscribe { from } => {
                self.subs.insert(conn);
                let backlog = from.and_then(|f| self.ring.since(f).map(|b| (f, b)));
                match backlog {
                    Some((f, bytes)) => {
                        let res = SessionMsg::Res {
                            id,
                            result: Outcome::Ok(AgentResponse::Subscribed {
                                session: self.id.clone(),
                                snapshot: None,
                                offset: f,
                            }),
                        };
                        let _ = tx.json(&res).await;
                        for (i, part) in bytes.chunks(CHUNK).enumerate() {
                            let c = TerminalChunk {
                                session: self.id.clone(),
                                offset: f + (i * CHUNK) as u64,
                                data: part.to_vec(),
                            };
                            if tx.terminal(&c).await.is_err() {
                                break;
                            }
                        }
                        return;
                    }
                    None => {
                        let snap = self.snapshot();
                        let offset = snap.offset;
                        Ok(AgentResponse::Subscribed { session: self.id.clone(), snapshot: Some(snap), offset })
                    }
                }
            }
            SessionRequest::Unsubscribe => {
                self.subs.remove(&conn);
                Ok(AgentResponse::Done)
            }
            SessionRequest::Input { data } => {
                if self.exit.is_some() {
                    Err(ErrorBody::new(ErrorCode::SessionExited, "세션이 끝났다"))
                } else {
                    self.input
                        .send(data.0)
                        .map(|_| AgentResponse::Done)
                        .map_err(|_| ErrorBody::new(ErrorCode::SessionExited, "입력 통로가 닫혔다"))
                }
            }
            SessionRequest::Resize { cols, rows } => {
                let size = PtySize { rows: rows.max(1), cols: cols.max(1), pixel_width: 0, pixel_height: 0 };
                match self.master.resize(size) {
                    Ok(()) => {
                        self.vt.resize(rows, cols);
                        self.screen_seq += 1;
                        let info = self.info();
                        self.broadcast(SessionMsg::Updated(info.clone())).await;
                        Ok(AgentResponse::Session(info))
                    }
                    Err(e) => Err(ErrorBody::new(ErrorCode::Internal, format!("resize 실패: {e}"))),
                }
            }
            SessionRequest::Kill => match self.killer.kill() {
                Ok(()) => Ok(AgentResponse::Done),
                Err(e) => Err(ErrorBody::new(ErrorCode::Internal, format!("종료 실패: {e}"))),
            },
            SessionRequest::Screen => Ok(AgentResponse::Screen(self.vt.text(&self.id))),
            SessionRequest::Detect { unchanged_since } => Ok(AgentResponse::Detect(DetectScreen {
                seq: self.screen_seq,
                text: (unchanged_since != Some(self.screen_seq)).then(|| self.vt.detection_text()),
                osc_title: self.vt.osc_title().to_string(),
                osc_progress: self.vt.osc_progress().to_string(),
            })),
            SessionRequest::History { anchor, depth } => {
                let chunk = match self.vt.older(&anchor, depth as usize, aaa_vt::REPLAY_HISTORY) {
                    Some(o) => HistoryChunk { data: B64(o.data), lines: o.lines as u32, more: o.more, found: true },
                    None => HistoryChunk { data: B64(Vec::new()), lines: 0, more: false, found: false },
                };
                Ok(AgentResponse::History(chunk))
            }
        };
        let _ = tx.json(&SessionMsg::Res { id, result: result.into() }).await;
    }

    async fn broadcast(&self, msg: SessionMsg) {
        for tx in self.conns.values() {
            let _ = tx.json(&msg).await;
        }
    }
}

pub fn main(dir: PathBuf) -> Result<()> {
    // agent·ssh 세션과 분리한다(SIGHUP을 받지 않고, agent가 죽어도 남는다).
    #[cfg(unix)]
    unsafe {
        libc::setsid();
    }
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build()?;
    rt.block_on(run(dir))
}

/// 세션에 넘기지 않는, 부모 harness 세션의 표지 env
const INHERITED_SESSION_MARKERS: &[&str] = &[
    "CLAUDECODE",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_SESSION_ATTENDED",
    "CLAUDE_CODE_MESSAGING_SOCKET",
    "CLAUDE_CODE_MESSAGING_TOKEN",
    "CLAUDE_CODE_EXECPATH",
    "CLAUDE_PID",
    "CLAUDE_EFFORT",
];

async fn run(dir: PathBuf) -> Result<()> {
    let paths = SessionPaths::from_dir(&dir);
    let spec: SpecFile = util::read_json(&paths.spec()).context("spec.json")?;
    let s = &spec.spec;

    let pty = portable_pty::native_pty_system();
    let pair = pty
        .openpty(PtySize { rows: s.rows.max(1), cols: s.cols.max(1), pixel_width: 0, pixel_height: 0 })
        .context("PTY 열기")?;
    let mut cmd = if s.argv.is_empty() {
        CommandBuilder::new_default_prog()
    } else {
        let mut c = CommandBuilder::new(&s.argv[0]);
        c.args(&s.argv[1..]);
        c
    };
    if let Some(cwd) = &s.cwd {
        cmd.cwd(cwd);
    }
    // agent를 Claude Code 세션 안에서 띄웠으면 그 세션 표지가 이어진다. 남기면 세션 안 Claude가 자기를 자식 세션으로 보고
    // 기록 저장(resume)을 끄고, 메시징 token도 새어 나간다. 사용자가 준 설정 env(CLAUDE_CODE_USE_BEDROCK 등)는 남긴다.
    for k in INHERITED_SESSION_MARKERS {
        cmd.env_remove(k);
    }
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    // fish는 다시 접는(reflow) 터미널이면 크기가 바뀔 때 스스로 다시 그리지 않는다. aaa-vt도 다시 접어 겹쳐 그리면 프롬프트가
    // 겹친다(herdr src/pane.rs `apply_pane_terminal_env`). 물려받은 값이 있으면 두고, spec env가 이긴다. 이름은 소문자 그대로만
    // 본다(MSYS·Cygwin fish는 소문자만 읽는다).
    if !cmd.iter_full_env_as_str().any(|(k, _)| k == "fish_handle_reflow") {
        cmd.env("fish_handle_reflow", "0");
    }
    cmd.env("AAA_SESSION_ID", &spec.id);
    for (k, v) in &s.env {
        cmd.env(k, v);
    }
    let mut child = pair.slave.spawn_command(cmd).context("자식 실행")?;
    drop(pair.slave);
    let child_pid = child.process_id();
    let killer = Killer::new(child.clone_killer(), child_pid);
    let mut reader = pair.master.try_clone_reader()?;
    let mut writer = pair.master.take_writer()?;

    let started_at_ms = util::now_ms();
    util::write_json(&paths.meta(), &MetaFile { host_pid: std::process::id(), child_pid, started_at_ms })?;

    // PTY 입출력은 blocking이라 runtime 밖 thread에서 한다(ConPTY 문서 권고와 같은 구조).
    let (out_tx, mut out_rx) = mpsc::channel::<Vec<u8>>(256);
    std::thread::spawn(move || {
        let mut buf = vec![0u8; 16 << 10];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if out_tx.blocking_send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    let (in_tx, in_rx) = std::sync::mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        while let Ok(data) = in_rx.recv() {
            if writer.write_all(&data).and_then(|_| writer.flush()).is_err() {
                break;
            }
        }
    });
    let (exit_tx, mut exit_rx) = mpsc::channel::<Option<i32>>(1);
    std::thread::spawn(move || {
        let code = child.wait().ok().map(|st| st.exit_code() as i32);
        let _ = exit_tx.blocking_send(code);
    });

    let listener = aaa_common::ipc::LocalListener::bind(&paths.sock).context("session socket")?;
    let (cmd_tx, mut cmd_rx) = mpsc::channel::<Cmd>(256);
    let token = spec.token.clone();
    {
        let cmd_tx = cmd_tx.clone();
        tokio::spawn(async move {
            let mut next = 1u64;
            while let Ok(stream) = listener.accept().await {
                let conn = next;
                next += 1;
                tokio::spawn(serve_conn(conn, stream, token.clone(), cmd_tx.clone()));
            }
        });
    }

    let (rows, cols) = (s.rows, s.cols);
    let mut st = State {
        id: spec.id.clone(),
        spec: spec.clone(),
        child_pid,
        started_at_ms,
        vt: Vt::new(rows, cols),
        screen_seq: 0,
        dsr: DsrScanner::default(),
        ring: Ring::new(RING_BYTES),
        conns: HashMap::new(),
        subs: HashSet::new(),
        exit: None,
        master: pair.master,
        killer,
        input: in_tx,
    };

    let mut reader_open = true;
    let mut exit_deadline: Option<tokio::time::Instant> = None;
    loop {
        let drain = async {
            match exit_deadline {
                Some(d) => tokio::time::sleep_until(d).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            data = out_rx.recv(), if reader_open => match data {
                Some(d) => st.output(&d),
                None => {
                    reader_open = false;
                    if st.exit.is_some() { break; }
                }
            },
            code = exit_rx.recv(), if st.exit.is_none() && exit_deadline.is_none() => {
                st.exit = Some(code.flatten());
                // 자식이 끝나도 PTY에 남은 출력을 잠깐 더 받는다(손자 프로세스가 PTY를 쥐고 있으면 기다리지 않는다).
                exit_deadline = Some(tokio::time::Instant::now() + Duration::from_millis(300));
                if !reader_open { break; }
            }
            _ = drain => break,
            Some(cmd) = cmd_rx.recv() => match cmd {
                Cmd::Connected { conn, tx, proto } => {
                    let _ = tx.json(&SessionMsg::Welcome { proto, info: st.info() }).await;
                    st.conns.insert(conn, tx);
                }
                Cmd::Disconnected { conn } => { st.conns.remove(&conn); st.subs.remove(&conn); }
                Cmd::Req { conn, id, body } => st.handle(conn, id, body).await,
            },
        }
    }

    // 종료 기록: 마지막 화면을 남겨 agent가 세션 종료 뒤에도 보여 줄 수 있게 한다.
    let (rows, cols) = st.vt.size();
    util::write_private(&paths.final_replay(), &st.vt.replay())?;
    util::write_json(&paths.final_screen(), &st.vt.text(&st.id))?;
    util::write_json(
        &paths.exit(),
        &ExitFile { code: st.exit.flatten(), ended_at_ms: util::now_ms(), end_offset: st.ring.end(), cols, rows },
    )?;
    let info = st.info();
    st.broadcast(SessionMsg::Updated(info)).await;
    // 바로 끝난 명령도 agent가 붙어 종료 상태·마지막 화면을 받을 수 있게 잠깐 더 응답한다.
    let until = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        tokio::select! {
            _ = tokio::time::sleep_until(until) => break,
            Some(cmd) = cmd_rx.recv() => match cmd {
                Cmd::Connected { conn, tx, proto } => {
                    let _ = tx.json(&SessionMsg::Welcome { proto, info: st.info() }).await;
                    st.conns.insert(conn, tx);
                }
                Cmd::Disconnected { conn } => { st.conns.remove(&conn); st.subs.remove(&conn); }
                Cmd::Req { conn, id, body } => st.handle(conn, id, body).await,
            },
        }
    }
    Ok(())
}

async fn serve_conn(conn: u64, stream: aaa_common::ipc::LocalStream, token: String, cmd_tx: mpsc::Sender<Cmd>) {
    let (r, w) = tokio::io::split(stream);
    let tx = spawn_writer(w, MAX_FRAME);
    let mut rx = FrameRx::new(r, MAX_FRAME);
    let hello: Result<SessionMsg> = rx.control().await;
    match hello {
        Ok(SessionMsg::Hello { proto, token: t }) if t == token => {
            let agreed = match negotiate(SESSION_IPC, proto) {
                Ok(v) => v,
                Err(e) => {
                    let _ = tx
                        .json(&SessionMsg::Reject(ErrorBody::new(ErrorCode::UnsupportedVersion, e.to_string())))
                        .await;
                    return;
                }
            };
            // Welcome(info)은 상태를 가진 main loop가 보낸다.
            if cmd_tx.send(Cmd::Connected { conn, tx: tx.clone(), proto: agreed }).await.is_err() {
                return;
            }
        }
        _ => {
            let _ = tx.json(&SessionMsg::Reject(ErrorBody::new(ErrorCode::Unauthorized, "token이 다르다"))).await;
            return;
        }
    }
    while let Ok(Some(Incoming::Control(b))) = rx.next().await {
        let Ok(SessionMsg::Req { id, body }) = serde_json::from_slice::<SessionMsg>(&b) else { break };
        if cmd_tx.send(Cmd::Req { conn, id, body }).await.is_err() {
            break;
        }
    }
    let _ = cmd_tx.send(Cmd::Disconnected { conn }).await;
}

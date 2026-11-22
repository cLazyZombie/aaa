//! `aaa agent`: agent backend. 이 머신의 세션(session host)을 만들고 감독하며, host와 Agent link로 통신한다.
//!
//! - 외부에 아무것도 listen하지 않는다. host로 나가는 연결만 연다(§4.2 5).
//! - 세션은 session host 프로세스가 소유하므로 agent가 재시작해도 이어진다. 시작할 때 세션 폴더를 다시 읽어 붙는다.
//! - host의 세션 요청은 세션별 pump task가 session host로 중계한다. 응답과 출력이 같은 task를 지나므로 순서가 지켜진다.
//! - Claude·Codex 세션은 pump가 화면을 300ms마다 읽어 상태(작업 중·idle·질문)를 정한다(herdr 0.9.5 감지, plan §8.110).

mod session_pump;
use session_pump::*;
mod harness_memory;
mod hook_intake;
use harness_memory::*;
mod chat_watch;
mod dispatch;
mod spawn;
use aaa_common::child_process::StdCommandExt as _;
mod account_spawn;
mod agy;
pub use agy::check_agy_model;
mod attach;
mod chat;
mod ci;
mod command;
mod cwd;
pub mod deploy;
mod design;
pub use deploy::guard::worker as deployment_guard;
pub mod detect;
mod diff;
mod files;
mod gh;
mod git_watch;
mod github;
mod health;
pub mod hooks;
pub mod identity;
pub mod image;
pub mod integration;
mod journal;
pub mod link;
mod model_catalog;
mod page;
mod plan;
pub mod post;
mod provider_error;
mod quota;
#[cfg(target_os = "macos")]
mod report_watch;
mod responsibility;
mod selfcheck;
mod settings_jobs;
mod shell_chat;
mod turn_reply;
pub use selfcheck::start_selfcheck;
mod slash;
pub mod thread_read;
pub mod threads;
pub use workspace_metrics::verify;
pub mod work_root;
mod workspace_metrics;

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

#[cfg(test)]
mod chat_paths_tests;

/// 그린 HTML 페이지: 경로·크기·고친 시각과 bytes
type DrawnPage = (String, u64, std::time::SystemTime, Arc<Vec<u8>>);

pub struct Agent {
    pub state: PathBuf,
    pub machine_id: String,
    pub name: Mutex<String>,
    pub boot_id: String,
    pub dev_mode: bool,
    pub default_cwd: Option<String>,
    exe: PathBuf,
    deployments: deploy::Manager,
    guard_gate: tokio::sync::Mutex<()>,
    sessions: Mutex<HashMap<String, Entry>>,
    /// 연결 변경은 옛 전송 큐에서 기다리는 send_host도 깨운다.
    host: tokio::sync::watch::Sender<Option<FrameTx>>,
    pub selfcheck: Mutex<Option<serde_json::Value>>,
    threads_cfg: Mutex<threads::ThreadsConfig>,
    /// 파일 보기가 여는 폴더(plan §8.125)
    file_roots: Mutex<files::Roots>,
    /// 마지막으로 그린 HTML 페이지(경로·크기·고친 시각): host가 조각으로 읽는 동안 다시 그리지 않는다
    page_cache: Mutex<Option<DrawnPage>>,
    /// report.md 감시 task(thread ID별)
    watches: Mutex<HashMap<String, tokio::task::JoinHandle<()>>>,
    /// 대화 기록 감시 task(host가 정한 key별, 차이 목록 C3). host 연결이 바뀌면 모두 끝낸다: 새 host가 청한 것만 다시 본다
    chat_watches: Mutex<HashMap<String, tokio::task::JoinHandle<()>>>,
    /// 기록 상태를 `ChatChanged.signature`로 감추는 이 프로세스의 무작위 열쇠(참고 conversation-monitor.ts `salt`)
    chat_salt: std::hash::RandomState,
    /// 세션별로 다시 알릴 harness 사건(host가 다시 붙으면 다시 보낸다). 칸마다(`Slot`) 마지막 하나다.
    /// 세션 폴더 `harness.json`에도 적어 agent가 다시 떠도 남는다.
    harness_last: Mutex<HashMap<String, Vec<aaa_proto::HarnessEvent>>>,
    /// 연결 시 변경분 전달과 새 harness 사건의 기억·전송 순서를 함께 지킨다.
    harness_delivery: tokio::sync::Mutex<()>,
    /// 세션마다 SessionStart 때 적은 harness 세션 ID(다른 세션의 사건을 거른다)
    harness_started: Mutex<HashMap<String, String>>,
    /// 세션마다 계획·배경 작업 수 추적(plan.rs, 차이 목록 S5)
    plans: Mutex<HashMap<String, plan::Track>>,
    /// 세션마다 지금 harness 프로세스가 시작한 때(Claude SessionStart startup·resume, ms)
    plan_since: Mutex<HashMap<String, u64>>,
    /// 세션마다 Claude가 hook·상태줄 입력으로 알린 생각 수준(plan.rs `ClaudeEffort`)
    claude_effort: Mutex<HashMap<String, plan::ClaudeEffort>>,
    /// 세션마다 harness가 알린 처리 속도(Claude 상태줄 `fast_mode`, Codex OSC 제목, plan.rs `HarnessSpeed`)
    harness_speed: Mutex<HashMap<String, plan::HarnessSpeed>>,
    /// 세션마다 진행 알림 기록(Claude·Codex hook 안내, herdr-projects progress.rs `Record`). 기억만 한다: agent가 다시 뜨면 알림이
    /// 한 번 일찍 갈 수 있을 뿐이다.
    progress: Mutex<HashMap<String, hooks::Progress>>,
    /// pane 등록보다 먼저 온 Codex hook을 등록 사건으로 깨운다.
    session_ready: tokio::sync::Notify,
    /// 마지막 머신 건강(host가 다시 붙으면 다시 보낸다)
    health: Mutex<Option<aaa_proto::MachineHealth>>,
    /// 지금 host가 건강 보고(`health.v1`)를 받는가
    host_health: std::sync::atomic::AtomicBool,
    host_recovery: std::sync::atomic::AtomicBool,
    /// 지금 host가 정리 사건(`compacting.v1`, plan §8.309)을 아는가. 모르는 host에는 정리 중을 `working`으로 보낸다
    host_compacting: std::sync::atomic::AtomicBool,
    host_agy_quota: std::sync::atomic::AtomicBool,
    agy_quota: Mutex<HashMap<String, aaa_proto::AgyQuotaSample>>,
    host_post: Mutex<Option<bool>>,
    host_automation_results: Mutex<bool>,
    host_thread_read: std::sync::atomic::AtomicBool,
    host_design_attach: std::sync::atomic::AtomicBool,
    host_design_detach: std::sync::atomic::AtomicBool,
    read_waiters: Mutex<HashMap<String, tokio::sync::oneshot::Sender<aaa_proto::Outcome<aaa_proto::FrontResponse>>>>,
    host_image: std::sync::atomic::AtomicBool,
    image_waiters: Mutex<HashMap<String, tokio::sync::oneshot::Sender<aaa_proto::Outcome<aaa_proto::ImageResponse>>>>,
    host_mission: Mutex<bool>,
    host_work_files: Mutex<bool>,
    post_gate: Mutex<()>,
    post_waiters: Mutex<HashMap<String, tokio::sync::oneshot::Sender<aaa_proto::PostResult>>>,
    /// 세션마다 사람이 붙인 이름(`spec.json`의 `label`). session host는 띄울 때 이름만 알므로(이전 판은 label을 모른다)
    /// 그가 보내는 정보의 이름은 여기서 덮는다.
    labels: Mutex<HashMap<String, String>>,
    /// host가 알려 준 화면 색(plan §8.187). 새 세션의 spec에 싣고, 다시 붙은 session host에도 보낸다
    term_colors: Mutex<Option<aaa_proto::TermColors>>,
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
        let name = match std::fs::read_to_string(state.join("machine-label")) {
            Ok(label) if util::machine_name_ok(label.trim()) => label.trim().to_string(),
            Ok(_) => anyhow::bail!("agent machine-label 이름 형식 오류"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => name,
            Err(e) => return Err(e.into()),
        };
        let deployments = deploy::Manager::new(&state, &std::env::current_exe()?)?;
        Ok(Arc::new(Agent {
            deployments,
            guard_gate: tokio::sync::Mutex::new(()),
            state,
            machine_id,
            name: Mutex::new(name),
            boot_id: util::boot_id(),
            dev_mode,
            default_cwd,
            exe: std::env::current_exe()?,
            sessions: Mutex::new(HashMap::new()),
            host: tokio::sync::watch::channel(None).0,
            selfcheck: Mutex::new(None),
            threads_cfg: Mutex::new(threads::ThreadsConfig::default()),
            file_roots: Mutex::new(files::Roots::default()),
            page_cache: Mutex::new(None),
            watches: Mutex::new(HashMap::new()),
            chat_watches: Mutex::new(HashMap::new()),
            chat_salt: std::hash::RandomState::new(),
            harness_last: Mutex::new(HashMap::new()),
            harness_delivery: tokio::sync::Mutex::new(()),
            labels: Mutex::new(HashMap::new()),
            harness_started: Mutex::new(HashMap::new()),
            plans: Mutex::new(HashMap::new()),
            plan_since: Mutex::new(HashMap::new()),
            claude_effort: Mutex::new(HashMap::new()),
            harness_speed: Mutex::new(HashMap::new()),
            progress: Mutex::new(HashMap::new()),
            session_ready: tokio::sync::Notify::new(),
            health: Mutex::new(None),
            host_health: std::sync::atomic::AtomicBool::new(false),
            host_recovery: std::sync::atomic::AtomicBool::new(false),
            host_compacting: std::sync::atomic::AtomicBool::new(false),
            host_agy_quota: std::sync::atomic::AtomicBool::new(false),
            agy_quota: Mutex::new(HashMap::new()),
            term_colors: Mutex::new(None),
            host_post: Mutex::new(None),
            host_automation_results: Mutex::new(false),
            host_thread_read: std::sync::atomic::AtomicBool::new(false),
            host_design_attach: std::sync::atomic::AtomicBool::new(false),
            host_design_detach: std::sync::atomic::AtomicBool::new(false),
            read_waiters: Mutex::new(HashMap::new()),
            host_image: std::sync::atomic::AtomicBool::new(false),
            image_waiters: Mutex::new(HashMap::new()),
            host_mission: Mutex::new(false),
            host_work_files: Mutex::new(false),
            post_gate: Mutex::new(()),
            post_waiters: Mutex::new(HashMap::new()),
        }))
    }

    /// agent.toml의 thread 설정(작업 root·profile)
    pub fn configure_threads(&self, mut cfg: threads::ThreadsConfig) {
        cfg.state_dir = Some(self.state.clone());
        *self.threads_cfg.lock().unwrap() = cfg;
    }

    /// 파일 보기가 열 수 있는 폴더(§6.5 허용 폴더, plan §8.125). 없는 폴더는 빠진다.
    pub fn configure_file_roots(&self, roots: Vec<PathBuf>) {
        *self.file_roots.lock().unwrap() = files::Roots::new(roots);
    }

    /// report.md 변경을 host에 알린다. macOS vnode 사건과 기존 1초 점검을 함께 쓰며 등록 즉시 한 번 보낸다.
    fn watch_report(self: &Arc<Self>, thread: String, work_dir: PathBuf) {
        let me = self.clone();
        let t = thread.clone();
        let task = tokio::spawn(async move {
            #[cfg(target_os = "macos")]
            let mut wake = report_watch::Wake::new(&work_dir);
            let mut last: Option<Option<String>> = None;
            loop {
                // hash 읽기 전에 등록한다. 원자적 rename과 제자리 쓰기를 모두 관측한다.
                #[cfg(target_os = "macos")]
                wake.prepare();
                let wd = work_dir.clone();
                let hash = tokio::task::spawn_blocking(move || threads::report_hash(&wd))
                    .await
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "보고 감시 task 실패 뒤 다음 사건 대기")
                    })
                    .unwrap_or(None);
                if last.as_ref() != Some(&hash) {
                    me.send_host(&LinkMsg::Event(AgentEvent::ReportChanged { thread: t.clone(), hash: hash.clone() }))
                        .await;
                    last = Some(hash);
                }
                #[cfg(target_os = "macos")]
                wake.wait().await;
                #[cfg(not(target_os = "macos"))]
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });
        if let Some(old) = self.watches.lock().unwrap().insert(thread, task) {
            old.abort();
        }
    }

    pub fn sessions(&self) -> Vec<SessionInfo> {
        let mut v: Vec<_> = self.sessions.lock().unwrap().values().map(|e| e.info.clone()).collect();
        for info in &mut v {
            *info = self.labeled(info.clone());
        }
        v.sort_by(|a, b| a.started_at_ms.cmp(&b.started_at_ms).then(a.id.cmp(&b.id)));
        v
    }

    fn host_tx(&self) -> Option<FrameTx> {
        self.host.borrow().clone()
    }

    /// host 연결을 바꾼다. 연결 시 harness 변경분은 link가 live 사건과 같은 울타리 안에서 먼저 보낸다.
    pub fn set_host(&self, tx: Option<FrameTx>) {
        let gone = tx.is_none();
        self.host.send_replace(tx);
        for (_, t) in self.chat_watches.lock().unwrap().drain() {
            t.abort();
        }
        if gone {
            self.read_waiters.lock().unwrap().clear();
            let pumps: Vec<_> = self.sessions.lock().unwrap().values().filter_map(|e| e.pump.clone()).collect();
            for p in pumps {
                aaa_common::diagnostic::ignored(
                    p.try_send(PumpCmd::HostGone),
                    "수신자가 끝난 뒤에도 현재 작업은 마친다",
                );
            }
        }
    }

    async fn send_host(&self, msg: &LinkMsg) {
        let mut host = self.host.subscribe();
        let tx = host.borrow_and_update().clone();
        if let Some(tx) = tx {
            // 실행 환경은 실행 입력이다. ThreadReady/Relaunch는 기록 경계에서만 가린다.
            // 인증 갱신 요청은 인증 전달이다: 가리면 요청 본문 JSON이 깨진다(t-1688). host가 받는 곳에서도 가리지 않는다.
            let clean = match msg {
                LinkMsg::Res {
                    result: Outcome::Ok(AgentResponse::ThreadReady(_) | AgentResponse::Relaunch(_)),
                    ..
                }
                | LinkMsg::Event(AgentEvent::AccountRefresh(_)) => msg.clone(),
                // 그림 조각은 Git에 저장할 원본 바이트다.
                LinkMsg::Event(AgentEvent::ThreadRead { request, .. })
                    if matches!(&**request, aaa_proto::FrontRequest::DesignAttach { .. }) =>
                {
                    msg.clone()
                }
                _ => match aaa_common::redact::json(msg.clone()) {
                    Ok(clean) => clean,
                    Err(_) => return, // 가림 실패는 기록한 뒤 메시지를 거부한다. 원문은 보내지 않는다.
                },
            };
            // 읽기를 멈춘 옛 host의 큐가 차 있어도 새 연결의 replay 울타리를 붙잡지 않는다.
            // 기억한 harness 상태는 다음 연결이 최신 snapshot으로 전달한다.
            tokio::select! {
                _ = host.changed() => {}
                _ = tx.link(&clean) => {}
            }
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod mission_feature_tests {
    use super::*;
    use aaa_common::conn::{FrameRx, spawn_writer};
    #[tokio::test]
    async fn old_host_cannot_receive_new_mission_responses() {
        let state = std::env::temp_dir().join(format!("aaa-mission-feature-{}", util::new_id()));
        let a = Agent::new(state.clone(), "m".into(), "mac-a".into(), true, None).unwrap();
        let work = state.join("work/.aaa/demo-t-0001");
        std::fs::create_dir_all(&work).unwrap();
        let (s, peer) = tokio::io::duplex(1 << 20);
        let (_, w) = tokio::io::split(s);
        a.set_host(Some(spawn_writer(w, aaa_proto::MAX_FRAME)));
        let mut rx = FrameRx::new(peer, aaa_proto::MAX_FRAME);
        a.clone()
            .handle("old".into(), AgentRequest::MissionStateList { work_dir: work.to_string_lossy().into() })
            .await;
        assert!(
            matches!(rx.control::<LinkMsg>().await.unwrap(),LinkMsg::Res{id,result:aaa_proto::Outcome::Err(e)} if id=="old" && e.code==ErrorCode::Forbidden)
        );
        *a.host_mission.lock().unwrap() = true;
        a.clone()
            .handle("new".into(), AgentRequest::MissionStateList { work_dir: work.to_string_lossy().into() })
            .await;
        assert!(
            matches!(rx.control::<LinkMsg>().await.unwrap(),LinkMsg::Res{id,result:aaa_proto::Outcome::Ok(AgentResponse::MissionFiles{files})} if id=="new" && files.is_empty())
        );
        std::fs::remove_dir_all(state).unwrap();
    }
}

#[cfg(test)]
mod machine_label_tests {
    use super::*;
    #[tokio::test]
    async fn machine_label_is_persistent_without_restarting_or_touching_sessions() {
        let state = std::env::temp_dir().join(format!("aaa-label-{}", util::new_id()));
        let a = Agent::new(state.clone(), "m-label".into(), "mac-b".into(), false, None).unwrap();
        assert_eq!(a.name.lock().unwrap().as_str(), "mac-b");
        assert!(matches!(a.machine_label("mac-c").unwrap(), AgentResponse::Done));
        assert_eq!(a.name.lock().unwrap().as_str(), "mac-c");
        assert_eq!(a.machine_id, "m-label");
        assert!(a.sessions.lock().unwrap().is_empty());
        assert!(a.machine_label("bad name").is_err());
        drop(a);
        let a = Agent::new(state.clone(), "m-label".into(), "mac-b".into(), false, None).unwrap();
        assert_eq!(a.name.lock().unwrap().as_str(), "mac-c", "설정의 옛 이름보다 host 이름표가 우선한다");
        assert_eq!(a.machine_id, "m-label");
        drop(a);
        std::fs::remove_dir_all(state).unwrap();
    }
}

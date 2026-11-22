//! agent 연결 하나: 요청을 보내고 응답·작업 파일 조각을 기다린다.

use super::*;

pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

pub(super) type Pending = Arc<Mutex<HashMap<String, PendingResponse>>>;

pub(super) enum WorkFileReply {
    List,
    Read,
}

pub(crate) struct PendingResponse {
    pub(super) tx: oneshot::Sender<Outcome<AgentResponse>>,
    pub(super) work_file: Option<WorkFileReply>,
    pub(super) input_pane: Option<String>,
}

pub(super) type InputWatch = Arc<tokio::sync::watch::Sender<bool>>;

pub(super) type InputWatches = Arc<Mutex<HashMap<String, std::sync::Weak<tokio::sync::watch::Sender<bool>>>>>;

impl PendingResponse {
    /// 같은 FileChunk라도 작업 상태 전송만 원본 hash를 검증한다. 파일 보기는 계속 가린다.
    pub(super) fn preserves(&self, response: &AgentResponse) -> bool {
        matches!(
            (&self.work_file, response),
            (Some(WorkFileReply::List), AgentResponse::WorkFiles { .. } | AgentResponse::MissionFiles { .. })
                | (Some(WorkFileReply::Read), AgentResponse::FileChunk { .. })
        )
    }

    pub(super) fn send(self, result: Outcome<AgentResponse>) -> Result<(), Box<Outcome<AgentResponse>>> {
        self.tx.send(result).map_err(Box::new)
    }
}

/// 취소·전송 실패·시한 종료 뒤에는 원본 전송 예외도 남기지 않는다.
pub(super) struct PendingRequest {
    pub(super) pending: Pending,
    pub(super) id: String,
}

impl Drop for PendingRequest {
    fn drop(&mut self) {
        self.pending.lock().unwrap().remove(&self.id);
    }
}

/// 연결된 agent 하나. 같은 머신이 다시 붙으면 generation이 바뀌고 이전 연결의 사건은 버린다.
#[derive(Clone)]
pub struct AgentConn {
    pub tx: FrameTx,
    pub(crate) pending: Pending,
    pub generation: u64,
    pub(super) seq: Arc<AtomicU64>,
    /// pending 잠금 아래에서 검사·변경한다. 종료 뒤에도 남는 clone은 새 요청을 받지 않는다.
    pub(super) closed: Arc<AtomicBool>,
    pub(super) input_watches: InputWatches,
    pub(super) machine_id: String,
    pub(super) timeouts: aaa_config::HostSection,
    pub(super) health_host: std::sync::Weak<Host>,
}

impl AgentConn {
    pub fn new(tx: FrameTx, generation: u64) -> Self {
        AgentConn {
            tx,
            pending: Arc::new(Mutex::new(HashMap::new())),
            generation,
            seq: Arc::new(AtomicU64::new(0)),
            closed: Default::default(),
            input_watches: Default::default(),
            machine_id: String::new(),
            timeouts: Default::default(),
            health_host: Default::default(),
        }
    }

    pub async fn request(&self, body: AgentRequest) -> Result<AgentResponse, ErrorBody> {
        let wait = match &body {
            AgentRequest::ThreadPrepare(_) | AgentRequest::ThreadPlacement { .. } => {
                Duration::from_secs(self.timeouts.thread_prepare_timeout_secs)
            }
            AgentRequest::ThreadCleanup { .. } => Duration::from_secs(self.timeouts.thread_cleanup_timeout_secs),
            AgentRequest::FileRead { .. }
            | AgentRequest::ReportCollect { .. }
            | AgentRequest::ReportHash { .. }
            | AgentRequest::ReportFileRead { .. }
            | AgentRequest::ReportTransfer { .. } => Duration::from_secs(self.timeouts.file_request_timeout_secs),
            _ => REQUEST_TIMEOUT,
        };
        self.request_timeout(body, wait).await
    }

    /// 오래 걸리는 요청(routine 명령 등)
    pub async fn request_timeout(&self, body: AgentRequest, wait: Duration) -> Result<AgentResponse, ErrorBody> {
        self.request_watched(body, wait, None, false)
            .await?
            .ok_or_else(|| err(ErrorCode::Internal, "입력 감시 없는 요청이 취소됐다"))
    }

    /// 같은 pane의 다른 입력이 끼면 이 전송의 대화 영수증은 판정에 쓰지 않는다.
    pub(super) fn watch_input(&self, pane: &str, yield_to_pending: bool) -> InputWatch {
        let pending = self.pending.lock().unwrap();
        let mut watches = self.input_watches.lock().unwrap();
        watches.retain(|_, watch| watch.strong_count() > 0);
        let in_flight = pending.values().any(|r| r.input_pane.as_deref() == Some(pane));
        if yield_to_pending
            && (in_flight || watches.get(pane).and_then(std::sync::Weak::upgrade).is_some_and(|w| *w.borrow()))
        {
            return Arc::new(tokio::sync::watch::Sender::new(false));
        }
        let watch = Arc::new(tokio::sync::watch::Sender::new(!in_flight && !self.closed.load(Ordering::Relaxed)));
        if let Some(old) = watches.insert(pane.into(), Arc::downgrade(&watch)).and_then(|w| w.upgrade()) {
            old.send_replace(false);
        }
        watch
    }

    pub(super) async fn request_watched(
        &self,
        body: AgentRequest,
        wait: Duration,
        watch: Option<&InputWatch>,
        require_current: bool,
    ) -> Result<Option<AgentResponse>, ErrorBody> {
        let id = format!("c{}", self.seq.fetch_add(1, Ordering::Relaxed));
        let input = match &body {
            AgentRequest::Input { session, data } => {
                Some((session.clone(), data.0.len(), data.0.starts_with(b"\x1b[<")))
            }
            _ => None,
        };
        if let Some((session, bytes, true)) = &input {
            tracing::debug!(request = %id, %session, bytes, "pane 입력: host 전달");
        } else if let Some((session, bytes, _)) = &input {
            tracing::info!(request = %id, %session, bytes, "pane 입력: host 전달");
        }
        let disconnected = || ErrorBody::new(ErrorCode::AgentDisconnected, "agent 연결이 끊겼다");
        let (otx, mut orx) = oneshot::channel();
        let work_file = match &body {
            AgentRequest::WorkFileList { .. } | AgentRequest::MissionStateList { .. } => Some(WorkFileReply::List),
            AgentRequest::WorkFileRead { .. } | AgentRequest::MissionStateRead { .. } => Some(WorkFileReply::Read),
            _ => None,
        };
        let msg = LinkMsg::Req { id: id.clone(), deadline_ms: Some(wait.as_millis() as u64), body };
        {
            let mut pending = self.pending.lock().unwrap();
            if self.closed.load(Ordering::Relaxed) {
                return Err(disconnected());
            }
            let input_pane = input.as_ref().map(|(pane, _, _)| pane.clone());
            if let Some(pane) = &input_pane {
                let mut watches = self.input_watches.lock().unwrap();
                // 같은 pending 잠금 안에서 확인하고 등록해야, 사용자의 글이 끼어든 뒤 알림 Enter가 예약되지 않는다.
                if require_current {
                    if watch.is_none_or(|w| !*w.borrow()) {
                        return Ok(None);
                    }
                    // 사용자 입력의 등록과 wire 큐의 순서가 뒤집히지 않게 여기서 바로 넣는다.
                    // 대기열이 찼으면 잠금을 놓고 나중의 사용자 글 위에 입력하는 대신 보류한다.
                    if !self.tx.try_json(&msg).map_err(|_| disconnected())? {
                        return Ok(None);
                    }
                }
                if let Some(active) = watches.get(pane).and_then(std::sync::Weak::upgrade)
                    && !watch.is_some_and(|watch| Arc::ptr_eq(&active, watch))
                {
                    active.send_replace(false);
                    watches.remove(pane);
                }
            }
            pending.insert(id.clone(), PendingResponse { tx: otx, work_file, input_pane });
        }
        let _pending = PendingRequest { pending: self.pending.clone(), id: id.clone() };
        let mut timed_out = false;
        let mut responded = false;
        let receive = async {
            // 전송 대기열이 찬 동안에도 연결 종료가 pending을 풀면 즉시 끝낸다.
            // 시한은 대기열에 넣는 시간까지 포함한다.
            if !require_current {
                tokio::select! {
                    biased;
                    result = &mut orx => return result.map_err(|_| disconnected()),
                    sent = self.tx.json(&msg) => sent.map_err(|_| disconnected())?,
                }
            }
            orx.await.map_err(|_| disconnected())
        };
        let result = match tokio::time::timeout(wait, receive).await {
            Ok(Ok(o)) => {
                responded = true;
                o.into_result()
            }
            Ok(Err(e)) => Err(e),
            Err(source_error) => {
                aaa_common::diagnostic::observe(&source_error, "오류 뒤 기존 복구·거부 분기를 따른다");
                timed_out = true;
                self.pending.lock().unwrap().remove(&id);
                // 무슨 요청이었는지 남긴다(op과 세션만: 입력 바이트 같은 내용은 남기지 않는다)
                let what = match &msg {
                    LinkMsg::Req { body, .. } => serde_json::to_value(body)
                        .inspect_err(|error| {
                            aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                        })
                        .ok(),
                    _ => None,
                };
                let field =
                    |k: &str| what.as_ref().and_then(|v| v.get(k)?.as_str().map(str::to_string)).unwrap_or_default();
                tracing::warn!(
                    id,
                    machine = %self.machine_id,
                    generation = self.generation,
                    op = field("op"),
                    session = field("session"),
                    wait_ms = wait.as_millis() as u64,
                    "agent 응답 시간 초과"
                );
                Err(ErrorBody::new(ErrorCode::Timeout, "agent 응답 시간 초과"))
            }
        };
        self.pending.lock().unwrap().remove(&id);
        if let Some(host) = self.health_host.upgrade() {
            host.agent_request_finished(timed_out, responded);
        }
        if let Some((session, bytes, true)) = &input {
            tracing::debug!(request = %id, %session, bytes, ok = result.as_ref().inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")).is_ok(), "pane 입력: host 확인");
        } else if let Some((session, bytes, _)) = &input {
            tracing::info!(request = %id, %session, bytes, ok = result.as_ref().inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")).is_ok(), "pane 입력: host 확인");
        }
        result.map(Some)
    }

    /// 연결이 끝났을 때 대기 중인 요청을 모두 실패로 끝낸다(다시 보내지 않는다: §8.5 a).
    pub fn fail_all(&self) {
        let drained: Vec<_> = {
            let mut pending = self.pending.lock().unwrap();
            self.closed.store(true, Ordering::Relaxed);
            for watch in self.input_watches.lock().unwrap().drain().filter_map(|(_, w)| w.upgrade()) {
                watch.send_replace(false);
            }
            pending.drain().collect()
        };
        for (_, tx) in drained {
            aaa_common::diagnostic::ignored(
                tx.send(Outcome::Err(ErrorBody::new(ErrorCode::AgentDisconnected, "agent 연결이 끊겼다"))),
                "수신자가 끝난 뒤에도 현재 작업은 마친다",
            );
        }
    }
}

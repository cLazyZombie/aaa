//! host 상태를 사건으로 보는 시험 도구: Front 연결 하나가 받는 Workspace·State 사건을 쌓고, 조건이 될 때까지 다음
//! 사건을 기다린다(polling하지 않는다). CLI와 웹 시험에서 함께 쓴다.

use std::time::{Duration, Instant};

use aaa_proto::{FrontEvent, FrontMsg, FrontRequest, FrontResponse, MachineView, PaneView, Workspace};

use super::Node;

pub struct HostWatch {
    rt: tokio::runtime::Runtime,
    rx: tokio::sync::mpsc::UnboundedReceiver<Result<FrontMsg, String>>,
    timeout: Duration,
    pub ws: Option<Workspace>,
    pub panes: Vec<PaneView>,
    pub machines: Vec<MachineView>,
}

impl HostWatch {
    /// `timeout`은 오지 않을 때 멈추는 안전 상한이다
    pub fn new(node: &Node, timeout: Duration) -> HostWatch {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
        let rx = rt.block_on(async {
            let mut c = aaa_common::front::FrontClient::connect_as(&node.state, "test-watch").await.unwrap();
            c.send(FrontRequest::Workspace { name: None }).await.unwrap();
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            tokio::spawn(async move {
                loop {
                    let message = match c.next().await {
                        Ok(Some(message)) => Ok(message),
                        Ok(None) => break,
                        Err(error) => Err(format!("{error:#}")),
                    };
                    let failed = message.is_err();
                    if tx.send(message).is_err() || failed {
                        break;
                    }
                }
            });
            rx
        });
        HostWatch { rt, rx, timeout, ws: None, panes: vec![], machines: vec![] }
    }

    fn apply(&mut self, m: FrontMsg) {
        match m {
            FrontMsg::Event(FrontEvent::Workspace(w)) if w.name == aaa_proto::DEFAULT_WORKSPACE => self.ws = Some(w),
            FrontMsg::Event(FrontEvent::State { machines, panes }) => {
                self.machines = machines;
                self.panes = panes;
            }
            FrontMsg::Res { result, .. } => {
                if let Ok(FrontResponse::Workspace(w)) = result.into_result() {
                    self.ws = Some(w);
                }
            }
            _ => {}
        }
    }

    /// 조건이 될 때까지 host 사건을 받는다
    pub fn wait<R>(&mut self, what: &str, mut f: impl FnMut(&HostWatch) -> Option<R>) -> R {
        self.wait_diag(what, &mut f, String::new)
    }

    /// 실패한 상태 사건과 호출자가 가진 로그를 남긴다. 진단 때문에 CLI를 다시 실행하지 않는다.
    pub fn wait_diag<R>(
        &mut self,
        what: &str,
        mut f: impl FnMut(&HostWatch) -> Option<R>,
        diag: impl FnOnce() -> String,
    ) -> R {
        let deadline = Instant::now() + self.timeout;
        loop {
            if let Some(v) = f(self) {
                return v;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            let rx = &mut self.rx;
            let m = self.rt.block_on(async move { tokio::time::timeout(left, rx.recv()).await });
            match m {
                Ok(Some(Ok(m))) => self.apply(m),
                Ok(Some(Err(error))) => panic!("{what}: Front 상태 수신 실패: {error}\n{}", diag()),
                Ok(None) => panic!("host 연결이 끊겼다({what})\n{}", diag()),
                Err(_) => panic!(
                    "{:?} 안에 {what}이 되지 않았다\nmachines: {:?}\nworkspace: {:?}\npanes: {:?}\n{}",
                    self.timeout,
                    self.machines.iter().map(|m| (&m.id, &m.name, &m.connection)).collect::<Vec<_>>(),
                    self.ws,
                    self.panes.iter().map(|p| (&p.id, &p.name, &p.state, &p.restore_error)).collect::<Vec<_>>(),
                    diag(),
                ),
            }
        }
    }

    /// 모든 탭의 pane(layout 차례)
    pub fn leaves(&self) -> Vec<String> {
        self.ws.iter().flat_map(|w| w.tabs.iter().flat_map(|t| aaa_proto::layout::leaves(&t.root))).collect()
    }

    pub fn pane(&self, id: &str) -> Option<&PaneView> {
        self.panes.iter().find(|p| p.id == id)
    }

    /// 이름이 `name`인 pane(여럿이면 실행 중인 것)
    pub fn named(&self, name: &str) -> Option<&PaneView> {
        let mut v: Vec<_> = self.panes.iter().filter(|p| p.name == name).collect();
        v.sort_by_key(|p| !p.state.is_running());
        v.first().copied()
    }

    pub fn connected(&self, name: &str) -> bool {
        self.machines.iter().any(|m| m.name == name && m.connection == aaa_proto::Connection::Connected)
    }
}

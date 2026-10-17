//! host 상태를 사건으로 보는 시험 도구: Front 연결 하나가 받는 Workspace·State 사건을 쌓고, 조건이 될 때까지 다음
//! 사건을 기다린다(polling하지 않는다). tui.rs의 `HostWatch`와 같은 꼴이다.

use std::time::{Duration, Instant};

use aaa_proto::{FrontEvent, FrontMsg, FrontRequest, FrontResponse, MachineView, PaneView, Workspace};

use super::Node;

pub struct HostWatch {
    rt: tokio::runtime::Runtime,
    rx: tokio::sync::mpsc::UnboundedReceiver<FrontMsg>,
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
            let c = aaa_common::front::FrontClient::connect_as(&node.state, "test-watch").await.unwrap();
            let (mut tx, rx) = c.split();
            tx.send(FrontRequest::Workspace { name: None }).await.unwrap();
            // 보내는 쪽을 닫지 않게 task에 둔다(연결이 살아 있어야 사건이 온다)
            tokio::spawn(async move {
                let _keep = tx;
                std::future::pending::<()>().await
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
        let deadline = Instant::now() + self.timeout;
        loop {
            if let Some(v) = f(self) {
                return v;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            let rx = &mut self.rx;
            let m = self.rt.block_on(async move { tokio::time::timeout(left, rx.recv()).await });
            match m {
                Ok(Some(m)) => self.apply(m),
                Ok(None) => panic!("host 연결이 끊겼다({what})"),
                Err(_) => panic!(
                    "{:?} 안에 {what}이 되지 않았다\nworkspace: {:?}\npanes: {:?}",
                    self.timeout,
                    self.ws,
                    self.panes.iter().map(|p| (&p.id, &p.name, &p.state, &p.restore_error)).collect::<Vec<_>>()
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

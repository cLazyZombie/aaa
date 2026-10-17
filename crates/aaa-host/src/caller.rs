//! front 연결을 연 프로세스가 어느 aaa pane 안에서 돌았나(plan §8.116). env(`AAA_SESSION_ID`) 자기 신고 대신 host가 판정한다.
//!
//! - 연결한 프로세스 pid(front socket peer credential, [`aaa_common::ipc::LocalListener::accept_peer`])에서, host 머신(로컬
//!   agent) pane의 child pid가 조상에 있거나 제어 터미널이 그 pane의 PTY면 그 pane 안이다. 변수를 지우거나(`env -u`) 다른
//!   셸·`nohup`으로 띄워도 조상이나 제어 터미널이 남는다. `setsid`로 터미널을 버려도 조상이 남는다.
//! - 부모가 끝나 init(1)에 붙고 제어 터미널도 없는 프로세스(일부러 떼어 낸 백그라운드)는 어디서 왔는지 모르니 보기만 준다.
//! - 참고: herdr·herdr-projects는 `HERDR_PANE_ID` env를 그대로 믿고, herdr는 peer pid를 로그에만 쓴다(`local_stream_peer_description`).
//!   pane 범위를 host가 정하는 선례는 t3code의 thread 범위 token(fail-closed, `McpSessionRegistry.ts`)이다.
//! - 남는 한계: 같은 사용자 권한이라 OS 경계는 아니다. 떼어 낸 프로세스가 부모(중간 프로세스)가 끝나기 전에 연결하면 조상 사슬이
//!   pane에 닿지 않는다. 원격 머신 pane은 그 머신에 front socket이 없어 해당이 없다.

use aaa_common::process;

/// 연결한 프로세스가 있는 곳
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Caller {
    /// 이 aaa pane 안
    Pane(String),
    /// 부모가 끝나 init에 붙고 제어 터미널이 없다(어디서 왔는지 모른다)
    Detached,
    /// pane 밖(사람의 터미널), 또는 pid를 모른다
    Outside,
}

impl crate::Host {
    pub(crate) fn caller_of(&self, pid: Option<u32>) -> Caller {
        let Some(pid) = pid else { return Caller::Outside };
        let panes: Vec<(String, u32)> = {
            let i = self.inner.lock().unwrap();
            i.panes
                .iter()
                .filter(|(_, p)| p.info.state.is_running() && i.machines.get(&p.machine_id).is_some_and(|m| m.local))
                .filter_map(|(id, p)| p.info.pid.map(|c| (id.clone(), c)))
                .collect()
        };
        let up = process::ancestors(pid);
        if let Some((id, _)) = panes.iter().find(|(_, c)| *c == pid || up.contains(c)) {
            return Caller::Pane(id.clone());
        }
        let tty = process::tty(pid);
        if let Some(t) = tty
            && let Some((id, _)) = panes.iter().find(|(_, c)| process::tty(*c) == Some(t))
        {
            return Caller::Pane(id.clone());
        }
        if cfg!(unix) && tty.is_none() && up.first() == Some(&1) {
            return Caller::Detached;
        }
        Caller::Outside
    }
}

//! peer PID의 pane 판정. host Front와 agent post가 같은 규칙을 쓴다.
use crate::process;

/// 연결한 프로세스가 있는 곳
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Caller {
    /// 이 aaa pane 안
    Pane(String),
    /// 떼어 낸 프로세스이거나 pid를 못 읽었다(어디서 왔는지 모른다)
    Detached,
    /// pane 밖(사람의 터미널, aaa가 띄운 명령)
    Outside,
}

/// `panes`는 (pane ID, pane child pid), `trusted`는 host·로컬 agent의 세션
pub fn judge(pid: u32, panes: &[(String, u32)], trusted: &[u32]) -> Caller {
    let pane_of = |p: u32| {
        let up = process::ancestors(p);
        panes.iter().find(|(_, c)| *c == p || up.contains(c)).map(|(id, _)| Caller::Pane(id.clone()))
    };
    if let Some(c) = pane_of(pid) {
        return c;
    }
    let Some(sid) = process::session(pid) else {
        return if cfg!(unix) { Caller::Detached } else { Caller::Outside };
    };
    // 세션 leader가 살아 있나(그 pid가 여전히 자기 세션의 leader인가: pid를 다시 쓴 남의 프로세스가 아니다)
    let leader_alive = sid == pid || process::session(sid) == Some(sid);
    if sid != pid
        && leader_alive
        && let Some(c) = pane_of(sid)
    {
        return c;
    }
    if trusted.contains(&sid) {
        return Caller::Outside;
    }
    let no_tty = process::tty(pid).is_none();
    if !leader_alive || (no_tty && (sid == pid || process::parent(sid) == Some(1))) {
        return Caller::Detached;
    }
    Caller::Outside
}

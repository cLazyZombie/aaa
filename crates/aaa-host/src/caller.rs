//! front 연결을 연 프로세스가 어느 aaa pane 안에서 돌았나(plan §8.116). env(`AAA_SESSION_ID`) 자기 신고 대신 host가 판정한다.
//!
//! - 연결한 프로세스 pid(front socket peer credential, [`aaa_common::ipc::LocalListener::accept_peer`])에서, host 머신(로컬
//!   agent) pane child가 그 프로세스이거나 조상에 있으면 그 pane 안이다. 변수를 지우거나(`env -u`) 다른 셸로 띄우거나
//!   `setsid`로 터미널을 버려도 조상이 남는다.
//! - 조상이 끊겼으면(`nohup … &`로 부모가 끝나 init에 붙었다) 세션 leader를 본다. pane child는 PTY 세션의 leader이고(portable-pty
//!   `setsid`), Claude Code Bash 도구의 셸은 따로 세션을 열지만 pane 안 agent의 자식이다. leader가 살아 있고 pane 안이면 그 pane이다.
//! - 떼어 낸 프로세스는 어디서 왔는지 모르니 보기만 준다(unix): 세션 leader가 끝났거나(double fork), 터미널 없이 스스로 세션
//!   leader이거나(`setsid -f`), 터미널 없는 세션의 leader가 init의 자식이다. host·로컬 agent와 같은 세션(시험·CI가 띄운 CLI,
//!   agent가 띄운 routine 명령)은 뺀다.
//! - pid를 못 읽으면 닫힌 쪽(보기만)이다.
//! - 참고: herdr·herdr-projects는 `HERDR_PANE_ID` env를 그대로 믿고, herdr는 peer pid를 로그에만 쓴다(`local_stream_peer_description`).
//!   pane 범위를 host가 정하는 선례는 t3code의 thread 범위 token(fail-closed, `McpSessionRegistry.ts`)이다.
//! - 남는 한계: 같은 사용자 권한이라 OS 경계는 아니다(pane 밖 데몬에 띄워 달라고 하면 — 새 터미널을 여는 tmux·ssh 등 — 사람의
//!   터미널과 가를 수 없다). Linux에서 init 대신 subreaper(systemd --user)에 붙으면 「leader가 init의 자식」을 놓친다. Windows는
//!   세션·제어 터미널이 없어 조상만 본다. 원격 머신 pane은 그 머신에 front socket이 없어 해당이 없다.

use aaa_common::process;

/// 연결한 프로세스가 있는 곳
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Caller {
    /// 이 aaa pane 안
    Pane(String),
    /// 떼어 낸 프로세스이거나 pid를 못 읽었다(어디서 왔는지 모른다)
    Detached,
    /// pane 밖(사람의 터미널, aaa가 띄운 명령)
    Outside,
}

impl crate::Host {
    pub(crate) fn caller_of(&self, pid: Option<u32>) -> Caller {
        // 끝났거나 못 읽는 프로세스: 닫힌 쪽
        let Some(pid) = pid.filter(|p| process::parent(*p).is_some()) else { return Caller::Detached };
        let (panes, agents) = {
            let i = self.inner.lock().unwrap();
            let panes: Vec<(String, u32)> = i
                .panes
                .iter()
                .filter(|(_, p)| p.info.state.is_running() && i.machines.get(&p.machine_id).is_some_and(|m| m.local))
                .filter_map(|(id, p)| p.info.pid.map(|c| (id.clone(), c)))
                .collect();
            let agents: Vec<u32> =
                i.machines.values().filter(|m| m.local && m.conn.is_some()).filter_map(|m| m.agent_pid).collect();
            (panes, agents)
        };
        let mut trusted: Vec<u32> =
            agents.into_iter().chain([std::process::id()]).filter_map(process::session).collect();
        trusted.dedup();
        judge(pid, &panes, &trusted)
    }

    /// 로컬 agent 프로세스를 적어 둔다(로컬 socket peer credential)
    pub(crate) fn note_agent_pid(&self, machine_id: &str, pid: Option<u32>) {
        if let Some(m) = self.inner.lock().unwrap().machines.get_mut(machine_id) {
            m.agent_pid = pid;
        }
    }
}

/// `panes`는 (pane ID, pane child pid), `trusted`는 host·로컬 agent의 세션
fn judge(pid: u32, panes: &[(String, u32)], trusted: &[u32]) -> Caller {
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

#[cfg(all(test, unix))]
mod tests {
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};

    use super::*;

    /// 떼어 낸 프로세스(터미널 없는 세션 leader, init의 자식)는 보기만, host·로컬 agent 세션이면(routine 명령) 밖이다.
    /// pane child의 자손은 그 pane이다.
    #[test]
    fn detached_trusted_and_pane() {
        // 중간 프로세스가 끝나 init에 붙은 뒤 새 세션을 열고, pid를 알린 뒤 표준 입력이 닫힐 때까지 기다린다
        let script = "use POSIX (); my $k = fork // die; if ($k) { waitpid($k, 0); exit 0 } \
                      my $kp = $$; my $x = fork // die; exit 0 if $x; \
                      select(undef, undef, undef, 0.005) while getppid() == $kp; \
                      POSIX::setsid() or die; $| = 1; print \"$$\\n\"; 1 while sysread(STDIN, my $b, 1);";
        let mut top =
            Command::new("perl").args(["-e", script]).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
        let mut line = String::new();
        BufReader::new(top.stdout.take().unwrap()).read_line(&mut line).unwrap();
        let x: u32 = line.trim().parse().unwrap();
        assert_eq!(process::parent(x), Some(1));
        assert_eq!(judge(x, &[], &[]), Caller::Detached);
        assert_eq!(judge(x, &[], &[x]), Caller::Outside, "host·agent 세션");
        assert_eq!(judge(x, &[("p".into(), 1)], &[]), Caller::Pane("p".into()), "조상에 pane child");
        drop(top.stdin.take());
        top.wait().unwrap();
        // 시험 프로세스의 자식: 같은 세션이 host 세션이면 밖이다
        let me = std::process::id();
        let mut child = Command::new("sleep").arg("30").spawn().unwrap();
        let sid = process::session(me).unwrap();
        assert_eq!(judge(child.id(), &[], &[sid]), Caller::Outside);
        assert_eq!(judge(child.id(), &[("q".into(), me)], &[]), Caller::Pane("q".into()));
        child.kill().unwrap();
        child.wait().unwrap();
    }
}

//! 셸 pane 앞에서 도는 agent 찾기(QA t-0337 Q21, 결정 D8 「참고대로」). herdr 0.9.5는 pane이 늘 셸이라 셸의 제어 터미널
//! 앞 프로세스 그룹을 보고 그 이름으로 agent를 정한 뒤(잡기), 셸이 다시 앞에 오면 놓는다.
//!
//! - 이름 판정은 herdr `src/detect/mod.rs`의 `identify_agent_process_in_job`·`normalized_process_name`·
//!   `wrapped_agent_name_from_runtime_argv`·`script_arg_agent_name`·`agent_name_from_path_token`을 Claude·Codex만 옮겼다
//!   (aaa 규칙 판은 둘뿐이다).
//! - 프로세스 조사는 herdr `src/platform/{macos,linux}.rs`의 `foreground_group_leader_job`·`foreground_job`이다. Linux는
//!   앞 그룹 leader와 셸의 자손을 너비 우선으로 훑는다(herdr `process_tree_pids`, 훑는 수에 상한).
//! - 잡기·놓기는 herdr `src/pane.rs` `spawn_basic_detection_task`의 프로세스 조사 부분(`should_probe_foreground_job`·
//!   `foreground_shell_agent_action`·`AgentDetectionPresence`)이다. 백그라운드로 보낸 agent 붙잡기(`AgentJobTracker`),
//!   lifecycle hook 권한, Herdr 환경 표지(`process_agent_hint`)는 옮기지 않았다(plan §8.110).
//!
//! herdr(https://github.com/herdrdev/herdr, Apache License 2.0)의 코드를 옮겨 aaa에 맞게 고쳤다(`LICENSE-herdr.txt`).

use std::time::{Duration, Instant};

use super::{Harness, POLL};

/// 셸에 agent가 있는데 다시 볼 간격(herdr `PROCESS_RECHECK_IDENTIFIED`)
const RECHECK_IDENTIFIED: Duration = Duration::from_secs(5);
/// 앞 그룹을 읽지 못할 때 다시 볼 간격(herdr `PROCESS_RECHECK_MISSING_FOREGROUND_GROUP`)
const RECHECK_MISSING_GROUP: Duration = Duration::from_secs(30);
/// 앞 그룹이 바뀐 뒤 이름이 바뀔 수 있는 동안 더 자주 본다(herdr `PROCESS_ACQUISITION_*`: exec·`process.title`)
const ACQUISITION_WINDOW: Duration = Duration::from_secs(8);
const ACQUISITION_FAST_WINDOW: Duration = Duration::from_millis(1500);
const ACQUISITION_FAST_RECHECK: Duration = Duration::from_millis(500);
const ACQUISITION_SLOW_RECHECK: Duration = Duration::from_secs(2);
/// 셸이 아닌 것이 앞에 있는데 agent로 보이지 않으면 이만큼 연달아 놓친 뒤 놓는다(herdr `AGENT_MISS_CONFIRMATION_ATTEMPTS`)
const MISS_CONFIRMATIONS: u8 = 6;

/// 앞 프로세스 그룹의 프로세스 하나(herdr `ForegroundProcess`)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Process {
    pub pid: u32,
    /// 커널이 아는 이름(macOS `pbi_comm`, Linux `comm`)
    pub name: String,
    /// `argv[0]`의 끝 이름(macOS `KERN_PROCARGS2`, `process.title`이 바꾼 이름). Linux는 None이다(herdr와 같다).
    pub argv0: Option<String>,
    pub argv: Option<Vec<String>>,
}

/// 앞 프로세스 그룹(herdr `ForegroundJob`)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub pgid: u32,
    pub processes: Vec<Process>,
}

/// 한 번 조사한 결과(herdr `ProcessProbeResult`)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Probe {
    pub agent: Option<Harness>,
    /// 셸 자신이 앞 그룹에 있다(프롬프트로 돌아왔다)
    pub shell_in_front: bool,
}

/// 잡기·놓기
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// agent가 앞에 왔다(또는 끝난 뒤 같은 agent가 다시 떴다): 이 harness로 화면을 본다
    Acquired(Harness),
    /// 셸이 다시 앞에 왔다: 앞 agent의 끝(idle)을 먼저 알린다(herdr `ReportProcessExit`)
    Exited(Harness),
    /// pane은 다시 agent 없는 셸이다(herdr `ClearAgent`). 놓은 agent다.
    Released(Harness),
}

/// 셸 pane 하나의 앞 agent 상태(herdr `AgentDetectionPresence`와 프로세스 조사 차례)
#[derive(Debug)]
pub struct Presence {
    current: Option<Harness>,
    misses: u8,
    last_pgid: Option<u32>,
    probed: bool,
    last_check: Instant,
    acquisition_started: Option<Instant>,
    /// 끝(idle)을 알렸고 놓기를 기다린다(herdr `pending_foreground_shell_clear`·`foreground_shell_exit_reported`)
    exit_reported: bool,
    next: Instant,
}

impl Presence {
    pub fn new(now: Instant) -> Presence {
        Presence {
            current: None,
            misses: 0,
            last_pgid: None,
            probed: false,
            last_check: now,
            acquisition_started: None,
            exit_reported: false,
            next: now,
        }
    }

    pub fn next_at(&self) -> Instant {
        self.next
    }

    /// 300ms마다(herdr 감지 차례): 셸의 앞 그룹(`pgid`)을 보고, 볼 차례면 앞 job을 조사한다(`probe`). 앞 그룹을 읽지
    /// 못하면 agent가 보이지 않은 것이다(herdr `probe_foreground_process`도 그렇다).
    pub fn tick(&mut self, now: Instant, pgid: Option<u32>, probe: impl FnOnce(u32) -> Probe) -> Option<Change> {
        self.next = now + POLL;
        let changed = pgid != self.last_pgid && (pgid.is_some() || self.last_pgid.is_some());
        if !self.due(now, pgid, changed) {
            return None;
        }
        self.last_check = now;
        let had_probe = self.probed;
        self.probed = true;
        let p = pgid.map_or(Probe { agent: None, shell_in_front: false }, probe);
        let change = self.observe(p);
        self.last_pgid = pgid;
        if p.agent.is_some() {
            self.acquisition_started = None;
        } else if self.current.is_none() && had_probe && changed {
            self.acquisition_started = Some(now);
        }
        change
    }

    /// herdr `should_probe_foreground_job`(lifecycle hook 권한·붙잡은 agent는 없다)
    fn due(&self, now: Instant, pgid: Option<u32>, changed: bool) -> bool {
        if self.exit_reported {
            return true;
        }
        let elapsed = now.duration_since(self.last_check);
        if let Some(age) = self.acquisition_started.map(|s| now.duration_since(s)) {
            let interval =
                if age <= ACQUISITION_FAST_WINDOW { ACQUISITION_FAST_RECHECK } else { ACQUISITION_SLOW_RECHECK };
            if age <= ACQUISITION_WINDOW && elapsed >= interval {
                return true;
            }
        }
        if self.current.is_none() {
            return !self.probed || changed || (pgid.is_none() && elapsed >= RECHECK_MISSING_GROUP);
        }
        changed || elapsed >= RECHECK_IDENTIFIED
    }

    /// herdr `foreground_shell_agent_action`·`apply_foreground_shell_agent_action`
    fn observe(&mut self, p: Probe) -> Option<Change> {
        let Some(prev) = self.current else { return self.observe_probe(p.agent) };
        if self.exit_reported {
            self.exit_reported = false;
            return match p.agent {
                // 끝난 뒤 같은 agent가 다시 떴다(herdr `ReportReplacementProcess`)
                Some(a) if a == prev => {
                    self.misses = 0;
                    Some(Change::Acquired(a))
                }
                None => {
                    self.current = None;
                    self.misses = 0;
                    Some(Change::Released(prev))
                }
                other => self.observe_probe(other),
            };
        }
        if p.agent.is_some() {
            return self.observe_probe(p.agent);
        }
        if p.shell_in_front {
            // 바로 놓지 않는다: 앞 agent의 끝(idle)을 먼저 알려 알림·기다리는 쪽이 끝남을 본 뒤 pane이 셸이 된다
            self.exit_reported = true;
            return Some(Change::Exited(prev));
        }
        self.observe_probe(None)
    }

    /// herdr `AgentDetectionPresence::observe_process_probe`
    fn observe_probe(&mut self, agent: Option<Harness>) -> Option<Change> {
        match agent {
            Some(a) => {
                self.misses = 0;
                (self.current != Some(a)).then(|| {
                    self.current = Some(a);
                    Change::Acquired(a)
                })
            }
            None if self.current.is_none() => {
                self.misses = 0;
                None
            }
            None => {
                self.misses = self.misses.saturating_add(1);
                if self.misses < MISS_CONFIRMATIONS {
                    return None;
                }
                self.misses = 0;
                self.current.take().map(Change::Released)
            }
        }
    }
}

/// 셸(`shell_pid`)의 앞 그룹 `pgid`를 조사한다(herdr `probe_foreground_process_from_jobs`): leader 하나를 먼저 보고,
/// 모르면 그룹 전체를 본다.
#[cfg(unix)]
pub fn probe(shell_pid: u32, pgid: u32) -> Probe {
    if let Some(leader) = platform::group_leader(pgid) {
        let job = Job { pgid, processes: vec![leader] };
        if let Some((agent, _)) = identify(&job) {
            return Probe { agent: Some(agent), shell_in_front: pgid == shell_pid };
        }
    }
    match platform::job(shell_pid, pgid) {
        Some(job) => match identify(&job) {
            Some((agent, _)) => Probe { agent: Some(agent), shell_in_front: false },
            None => Probe { agent: None, shell_in_front: job.processes.iter().any(|p| p.pid == shell_pid) },
        },
        None => Probe { agent: None, shell_in_front: false },
    }
}

/// 앞 그룹에서 agent 프로세스(herdr `identify_agent_process_in_job`): 그룹 leader가 먼저, 아니면 이름이 가장 분명한 것
pub fn identify(job: &Job) -> Option<(Harness, u32)> {
    if let Some(p) = job.processes.iter().find(|p| p.pid == job.pgid)
        && let Some(agent) = Harness::from_label(&normalized_name(p))
    {
        return Some((agent, p.pid));
    }
    let mut best: Option<(u8, Harness, u32)> = None;
    for p in &job.processes {
        let name = normalized_name(p);
        let Some(agent) = Harness::from_label(&name) else { continue };
        let score = priority(p, &name);
        if best.is_none_or(|(s, ..)| score > s) {
            best = Some((score, agent, p.pid));
        }
    }
    best.map(|(_, agent, pid)| (agent, pid))
}

/// herdr `normalized_process_name`: `argv[0]`(없으면 커널 이름)을 보되, 셸·런타임이면 그것이 돌리는 스크립트 이름
fn normalized_name(p: &Process) -> String {
    let effective = p.argv0.as_deref().unwrap_or(&p.name);
    let lower = effective.to_lowercase();
    if generic_runtime_or_shell(&lower)
        && let Some(wrapped) = wrapped_name(&lower, p.argv.as_deref())
    {
        return wrapped;
    }
    if Harness::from_label(effective).is_some() {
        return effective.to_string();
    }
    if let Some(name) = p.argv.as_deref().and_then(|a| a.first()).and_then(|t| name_from_path_token(t)) {
        return name;
    }
    effective.to_string()
}

/// herdr `wrapped_agent_name_from_runtime_argv`(Windows cmd·powershell, Cursor·Hermes 전용 길은 뺐다)
fn wrapped_name(runtime: &str, argv: Option<&[String]>) -> Option<String> {
    let argv = argv?;
    match lookup_name(basename(runtime)).as_str() {
        "node" | "bun" => script_arg_name(argv, &["-e", "--eval", "-p", "--print"], &[]),
        name if python_runtime(name) => script_arg_name(argv, &["-c"], &["-m"]),
        "sh" | "bash" | "zsh" | "fish" => script_arg_name(argv, &["-c"], &[]),
        _ => None,
    }
}

/// herdr `script_arg_agent_name`: 선택지를 건너뛴 첫 인자(스크립트)의 이름. 글로 준 코드(`-c`·`-e`)는 보지 않는다.
fn script_arg_name(argv: &[String], eval_flags: &[&str], module_flags: &[&str]) -> Option<String> {
    let mut args = argv.iter().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--" {
            return args.next().and_then(|t| name_from_path_token(t));
        }
        if flag_matches(arg, eval_flags) || flag_matches(arg, module_flags) {
            return None;
        }
        if arg.starts_with('-') {
            if option_takes_value(arg) {
                args.next();
            }
            continue;
        }
        return name_from_path_token(arg);
    }
    None
}

fn flag_matches(arg: &str, flags: &[&str]) -> bool {
    flags.iter().any(|flag| {
        arg == *flag
            || (flag.starts_with('-') && !flag.starts_with("--") && arg.starts_with(flag) && arg.len() > flag.len())
            || (flag.starts_with("--") && arg.strip_prefix(flag).is_some_and(|rest| rest.starts_with('=')))
    })
}

fn option_takes_value(arg: &str) -> bool {
    matches!(
        arg,
        "-r" | "--require"
            | "--loader"
            | "--import"
            | "--experimental-loader"
            | "--inspect-port"
            | "-W"
            | "-X"
            | "-S"
            | "-L"
            | "-o"
    )
}

/// herdr `agent_name_from_path_token`: 끝 이름, 아니면 symlink를 따라간 끝 이름(Claude 설치판 `~/.local/bin/claude` 등)
fn name_from_path_token(token: &str) -> Option<String> {
    let trimmed = token.trim_matches(|c| matches!(c, '"' | '\''));
    if trimmed.is_empty() || trimmed.starts_with('-') {
        return None;
    }
    let label = |name: &str| Harness::from_label(name).map(|h| h.label().to_string());
    label(basename(trimmed)).or_else(|| {
        let path = std::path::Path::new(trimmed);
        if path.components().count() < 2 {
            return None;
        }
        label(std::fs::canonicalize(path).ok()?.file_name()?.to_str()?)
    })
}

/// herdr `process_priority`: 스크립트 이름으로 찾은 것 > 셸·런타임이 아닌 이름 > 셸·런타임
fn priority(p: &Process, normalized: &str) -> u8 {
    let lower = normalized.to_lowercase();
    if lower != p.name.to_lowercase() {
        3
    } else if !generic_runtime_or_shell(&lower) {
        2
    } else {
        1
    }
}

fn generic_runtime_or_shell(name: &str) -> bool {
    let name = lookup_name(basename(name));
    python_runtime(&name)
        || matches!(
            name.as_str(),
            "sh" | "bash" | "zsh" | "fish" | "tmux" | "node" | "bun" | "cmd" | "powershell" | "pwsh"
        )
}

fn python_runtime(name: &str) -> bool {
    name == "python"
        || name
            .strip_prefix("python")
            .is_some_and(|v| !v.is_empty() && v.chars().all(|c| c.is_ascii_digit() || c == '.'))
}

/// herdr `normalized_agent_lookup_name`
fn lookup_name(name: &str) -> String {
    let mut name = name.trim().to_lowercase();
    for suffix in [".exe", ".cmd", ".bat", ".ps1", ".js"] {
        if name.ends_with(suffix) {
            name.truncate(name.len() - suffix.len());
            break;
        }
    }
    name
}

fn basename(path: &str) -> &str {
    path.rsplit(['/', '\\']).find(|c| !c.is_empty()).unwrap_or(path)
}

/// herdr `src/platform/macos.rs`: `proc_listpids(PROC_PGRP_ONLY)`·`proc_pidinfo(PROC_PIDTBSDINFO)`·`sysctl(KERN_PROCARGS2)`
#[cfg(target_os = "macos")]
mod platform {
    use super::{Job, Process};

    const PROC_PGRP_ONLY: u32 = 2;

    pub fn group_leader(pgid: u32) -> Option<Process> {
        let info = bsdinfo(pgid)?;
        (info.pbi_pgid == pgid).then_some(())?;
        process(pgid, &info)
    }

    pub fn job(_shell_pid: u32, pgid: u32) -> Option<Job> {
        let processes: Vec<Process> = group_pids(pgid)
            .into_iter()
            .filter_map(|pid| {
                let info = bsdinfo(pid)?;
                (info.pbi_pgid == pgid).then_some(())?;
                process(pid, &info)
            })
            .collect();
        (!processes.is_empty()).then_some(Job { pgid, processes })
    }

    fn process(pid: u32, info: &libc::proc_bsdinfo) -> Option<Process> {
        let end = info.pbi_comm.iter().position(|&b| b == 0).unwrap_or(info.pbi_comm.len());
        let bytes: Vec<u8> = info.pbi_comm[..end].iter().map(|&b| b as u8).collect();
        let name = String::from_utf8(bytes).ok().filter(|n| !n.is_empty())?;
        let args = procargs2(pid);
        let argv = args.as_deref().and_then(argv_of);
        // 로그인 셸은 「-zsh」로 보인다(herdr `process_argv0_name`)
        let argv0 = argv.as_ref().and_then(|a| a.first()).and_then(|a0| {
            let base = std::path::Path::new(a0).file_name()?.to_str()?;
            let base = base.strip_prefix('-').unwrap_or(base);
            (!base.is_empty()).then(|| base.to_string())
        });
        Some(Process { pid, name, argv0, argv })
    }

    fn group_pids(pgid: u32) -> Vec<u32> {
        let mut capacity = 16usize;
        for _ in 0..8 {
            let mut pids = vec![0 as libc::pid_t; capacity];
            let bytes = std::mem::size_of_val(pids.as_slice());
            // SAFETY: pids는 bytes 크기의 쓸 수 있는 버퍼다
            let got = unsafe {
                libc::proc_listpids(PROC_PGRP_ONLY, pgid, pids.as_mut_ptr() as *mut libc::c_void, bytes as libc::c_int)
            };
            if got <= 0 {
                return Vec::new();
            }
            let got = got as usize;
            if got < bytes {
                let n = got / std::mem::size_of::<libc::pid_t>();
                return pids[..n].iter().filter(|&&p| p > 0).map(|&p| p as u32).collect();
            }
            capacity = capacity.saturating_mul(2);
        }
        Vec::new()
    }

    fn bsdinfo(pid: u32) -> Option<libc::proc_bsdinfo> {
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
        // SAFETY: info는 쓸 수 있는 proc_bsdinfo이고 size가 그 크기다
        let ret = unsafe {
            libc::proc_pidinfo(
                pid as libc::c_int,
                libc::PROC_PIDTBSDINFO,
                0,
                &mut info as *mut _ as *mut libc::c_void,
                size,
            )
        };
        (ret == size).then_some(info)
    }

    fn procargs2(pid: u32) -> Option<Vec<u8>> {
        let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid as libc::c_int];
        let mut size: libc::size_t = 0;
        // SAFETY: 첫 호출은 크기만 묻는다
        let ret =
            unsafe { libc::sysctl(mib.as_mut_ptr(), 3, std::ptr::null_mut(), &mut size, std::ptr::null_mut(), 0) };
        if ret != 0 || size == 0 {
            return None;
        }
        let mut buf = vec![0u8; size];
        // SAFETY: buf는 size 크기의 쓸 수 있는 버퍼다
        let ret = unsafe {
            libc::sysctl(mib.as_mut_ptr(), 3, buf.as_mut_ptr() as *mut libc::c_void, &mut size, std::ptr::null_mut(), 0)
        };
        if ret != 0 {
            return None;
        }
        buf.truncate(size);
        Some(buf)
    }

    /// `[argc: i32] [exec_path\0] [\0…] [argv[0]\0] [argv[1]\0] …`(herdr `procargs2_argv`)
    fn argv_of(buf: &[u8]) -> Option<Vec<String>> {
        let argc = i32::from_ne_bytes(buf.get(..4)?.try_into().ok()?);
        if argc < 1 {
            return None;
        }
        let rest = &buf[4..];
        let mut at = rest.iter().position(|&b| b == 0)?;
        while at < rest.len() && rest[at] == 0 {
            at += 1;
        }
        let mut argv = Vec::with_capacity(argc as usize);
        for _ in 0..argc {
            if at >= rest.len() {
                return None;
            }
            let end = rest[at..].iter().position(|&b| b == 0).map_or(rest.len(), |o| at + o);
            if end == at {
                return None;
            }
            argv.push(String::from_utf8_lossy(&rest[at..end]).into_owned());
            at = end + 1;
        }
        Some(argv)
    }
}

/// herdr `src/platform/linux.rs`: `/proc/<pid>/stat`의 pgrp, `comm`, `cmdline`. 그룹 식구는 앞 그룹 leader와 셸의
/// 자손을 `/proc/<pid>/task/<tid>/children`으로 훑어 찾는다(herdr `process_tree_pids`, 훑는 수 상한).
#[cfg(all(unix, not(target_os = "macos")))]
mod platform {
    use std::collections::{HashSet, VecDeque};

    use super::{Job, Process};

    /// 한 번에 훑는 프로세스 수 상한
    const SCAN_MAX: usize = 512;

    pub fn group_leader(pgid: u32) -> Option<Process> {
        let (pgrp, name) = stat(pgid)?;
        (pgrp == pgid).then(|| Process { pid: pgid, name, argv0: None, argv: argv(pgid) })
    }

    pub fn job(shell_pid: u32, pgid: u32) -> Option<Job> {
        let mut seen = HashSet::new();
        let mut queue = VecDeque::from([pgid, shell_pid]);
        let mut processes = Vec::new();
        while let Some(pid) = queue.pop_front() {
            if pid == 0 || seen.len() >= SCAN_MAX || !seen.insert(pid) {
                continue;
            }
            if let Some((pgrp, name)) = stat(pid)
                && pgrp == pgid
            {
                processes.push(Process { pid, name, argv0: None, argv: argv(pid) });
            }
            queue.extend(children(pid));
        }
        processes.sort_by_key(|p| p.pid);
        (!processes.is_empty()).then_some(Job { pgid, processes })
    }

    /// (pgrp, comm). comm에 빈칸·괄호가 있을 수 있어 마지막 ')' 뒤부터 센다: state ppid pgrp …
    fn stat(pid: u32) -> Option<(u32, String)> {
        let s = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let open = s.find('(')?;
        let close = s.rfind(')')?;
        let name = s.get(open + 1..close)?.to_string();
        let pgrp: i64 = s.get(close + 2..)?.split_whitespace().nth(2)?.parse().ok()?;
        (pgrp > 0).then_some((pgrp as u32, name))
    }

    fn argv(pid: u32) -> Option<Vec<String>> {
        let bytes = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
        let parts: Vec<String> = bytes
            .split(|&b| b == 0)
            .filter(|p| !p.is_empty())
            .map(|p| String::from_utf8_lossy(p).into_owned())
            .collect();
        (!parts.is_empty()).then_some(parts)
    }

    fn children(pid: u32) -> Vec<u32> {
        let Ok(tasks) = std::fs::read_dir(format!("/proc/{pid}/task")) else { return Vec::new() };
        tasks
            .flatten()
            .filter_map(|t| std::fs::read_to_string(t.path().join("children")).ok())
            .flat_map(|s| s.split_whitespace().filter_map(|p| p.parse().ok()).collect::<Vec<u32>>())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(pid: u32, name: &str, argv0: Option<&str>, argv: &[&str]) -> Process {
        Process {
            pid,
            name: name.into(),
            argv0: argv0.map(str::to_string),
            argv: (!argv.is_empty()).then(|| argv.iter().map(|a| a.to_string()).collect()),
        }
    }

    fn one(p: Process) -> Job {
        Job { pgid: p.pid, processes: vec![p] }
    }

    /// herdr `src/detect/mod.rs` 시험의 Claude·Codex 몫
    #[test]
    fn names_like_herdr() {
        let cases = [
            ("설치판 claude", proc(7, "claude", None, &["claude"]), Some(Harness::Claude)),
            // 설치판은 판 번호 이름의 실행 파일을 symlink로 띄워 커널 이름이 판 번호다
            ("판 번호 실행 파일", proc(7, "2.1.302", Some("claude"), &["claude", "--resume"]), Some(Harness::Claude)),
            // npm판은 node가 `process.title = "claude"`로 argv[0]을 바꾼다
            ("node의 process.title", proc(7, "node", Some("claude"), &["claude"]), Some(Harness::Claude)),
            (
                "셸 스크립트",
                proc(7, "bash", Some("sh"), &["/bin/sh", "/opt/bin/claude", "--ask"]),
                Some(Harness::Claude),
            ),
            (
                "node 스크립트",
                proc(7, "node", None, &["node", "--require", "x.js", "/opt/codex.js"]),
                Some(Harness::Codex),
            ),
            ("codex", proc(7, "codex", None, &["codex", "resume"]), Some(Harness::Codex)),
            ("셸 -c 글", proc(7, "bash", None, &["bash", "-c", "claude"]), None),
            ("python -c 뒤 인자", proc(7, "python3", None, &["python3", "-c", "import time", "/tmp/codex"]), None),
            ("닮은 이름", proc(7, "my-codex-helper", None, &["/tmp/my-codex-helper"]), None),
            ("프롬프트의 로그인 셸", proc(7, "zsh", Some("zsh"), &["-zsh"]), None),
        ];
        for (what, p, want) in cases {
            assert_eq!(identify(&one(p)).map(|(h, _)| h), want, "{what}");
        }
        // 그룹 leader가 아니면 이름이 가장 분명한 것(스크립트 이름 > 다른 이름 > 셸)
        let job = Job {
            pgid: 1,
            processes: vec![
                proc(1, "bash", None, &["bash", "-c", "x"]),
                proc(2, "codex", None, &["codex"]),
                proc(3, "bash", None, &["/bin/sh", "/opt/claude"]),
            ],
        };
        assert_eq!(identify(&job), Some((Harness::Claude, 3)));
    }

    const SHELL: Probe = Probe { agent: None, shell_in_front: true };
    const CLAUDE: Probe = Probe { agent: Some(Harness::Claude), shell_in_front: false };
    const OTHER: Probe = Probe { agent: None, shell_in_front: false };

    /// herdr 잡기·놓기: 앞 그룹이 바뀌면 조사해 잡고, 셸이 다시 앞에 오면 끝(idle)을 알린 다음 차례에 놓는다
    #[test]
    fn acquires_on_a_new_foreground_group_and_releases_after_reporting_the_exit() {
        let t0 = Instant::now();
        let mut p = Presence::new(t0);
        assert_eq!(p.tick(t0, Some(10), |_| SHELL), None, "프롬프트의 셸");
        // 그룹이 그대로면 조사하지 않는다
        let t1 = t0 + POLL;
        assert_eq!(p.tick(t1, Some(10), |_| panic!("조사했다")), None);
        assert_eq!(p.next_at(), t1 + POLL);
        let t2 = t1 + POLL;
        assert_eq!(
            p.tick(t2, Some(20), |g| if g == 20 { CLAUDE } else { SHELL }),
            Some(Change::Acquired(Harness::Claude))
        );
        // 잡은 뒤로는 그룹이 바뀌거나 5초마다만 본다
        assert_eq!(p.tick(t2 + POLL, Some(20), |_| panic!("조사했다")), None);
        assert_eq!(p.tick(t2 + RECHECK_IDENTIFIED, Some(20), |_| CLAUDE), None);
        let t3 = t2 + RECHECK_IDENTIFIED + POLL;
        assert_eq!(p.tick(t3, Some(10), |_| SHELL), Some(Change::Exited(Harness::Claude)));
        // 끝을 알린 다음 차례에는 그룹이 그대로여도 보고 놓는다
        assert_eq!(p.tick(t3 + POLL, Some(10), |_| SHELL), Some(Change::Released(Harness::Claude)));
        assert_eq!(p.tick(t3 + POLL * 2, Some(10), |_| panic!("조사했다")), None);
    }

    /// 끝을 알린 뒤 같은 agent가 다시 앞에 오면 새로 잡는다(herdr `ReportReplacementProcess`). 셸이 아닌 것이 앞에 있는데
    /// agent로 보이지 않으면 6번 연달아 놓친 뒤 놓는다.
    #[test]
    fn a_relaunched_agent_is_acquired_again_and_an_unknown_front_releases_after_misses() {
        let t0 = Instant::now();
        let mut p = Presence::new(t0);
        assert_eq!(p.tick(t0, Some(20), |_| CLAUDE), Some(Change::Acquired(Harness::Claude)));
        assert_eq!(p.tick(t0 + POLL, Some(10), |_| SHELL), Some(Change::Exited(Harness::Claude)));
        assert_eq!(p.tick(t0 + POLL * 2, Some(30), |_| CLAUDE), Some(Change::Acquired(Harness::Claude)));
        // 다른 프로그램(less 등)이 앞에 왔다: 5초마다 보며 여섯 번째에 놓는다
        let mut t = t0 + POLL * 3;
        assert_eq!(p.tick(t, Some(40), |_| OTHER), None);
        for _ in 0..4 {
            t += RECHECK_IDENTIFIED;
            assert_eq!(p.tick(t, Some(40), |_| OTHER), None);
        }
        t += RECHECK_IDENTIFIED;
        assert_eq!(p.tick(t, Some(40), |_| OTHER), Some(Change::Released(Harness::Claude)));
    }

    /// 앞 그룹이 바뀐 뒤 이름을 모르면 1.5초까지 0.5초마다, 8초까지 2초마다 다시 본다(exec·`process.title` 뒤에 이름이 바뀐다)
    #[test]
    fn a_new_unknown_group_is_rechecked_while_its_name_may_change() {
        let t0 = Instant::now();
        let mut p = Presence::new(t0);
        assert_eq!(p.tick(t0, Some(10), |_| SHELL), None);
        let t1 = t0 + POLL;
        assert_eq!(p.tick(t1, Some(20), |_| OTHER), None);
        assert_eq!(p.tick(t1 + POLL, Some(20), |_| panic!("0.5초 전에 봤다")), None);
        assert_eq!(p.tick(t1 + ACQUISITION_FAST_RECHECK, Some(20), |_| OTHER), None);
        let t2 = t1 + ACQUISITION_FAST_WINDOW + POLL;
        assert_eq!(p.tick(t2, Some(20), |_| OTHER), None);
        assert_eq!(p.tick(t2 + ACQUISITION_FAST_RECHECK, Some(20), |_| panic!("느린 차례는 2초")), None);
        assert_eq!(
            p.tick(t2 + ACQUISITION_SLOW_RECHECK, Some(20), |_| CLAUDE),
            Some(Change::Acquired(Harness::Claude))
        );
    }

    /// 실제 프로세스: 스크립트 `claude`를 제 그룹의 leader로 띄우면 셸 인자로 알아본다. 자식은 입력이 닫히면 스스로 끝난다.
    #[cfg(unix)]
    #[test]
    fn a_running_claude_script_is_identified_from_the_process_table() {
        use std::io::BufRead;
        use std::os::unix::process::CommandExt;
        let dir = std::env::temp_dir().join(format!("aaa-fg-{}", aaa_common::util::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("claude");
        std::fs::write(&script, "#!/bin/sh\necho ready\nread x\n").unwrap();
        std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let mut child = std::process::Command::new(&script)
            .process_group(0)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        // exec가 끝나 스크립트가 돈다
        let mut line = String::new();
        std::io::BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).unwrap();
        let pid = child.id();
        let leader = platform::group_leader(pid);
        let job = platform::job(pid, pid);
        drop(child.stdin.take());
        child.wait().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        let leader = leader.expect("그룹 leader");
        assert_eq!(identify(&one(leader.clone())), Some((Harness::Claude, pid)), "{leader:?}");
        assert!(job.expect("그룹").processes.iter().any(|p| p.pid == pid));
        assert_eq!(probe(pid + 1, pid).agent, None, "끝난 그룹에는 agent가 없다");
    }
}

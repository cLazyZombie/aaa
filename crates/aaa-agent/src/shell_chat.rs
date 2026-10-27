//! 셸에서 직접 실행한 harness의 기록 연결. cwd·최근 파일로 세션을 추측하지 않는다.
//! Codex는 앞 프로세스의 열린 rollout, Claude는 해당 PID의 sessions 등록(2.1.309)을 쓴다.
//! agent의 허용 기록 root·일반 파일 검사도 그대로 적용한다. 연결은 hook과 같은 사건으로 보낸다.

use std::io::{BufRead, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::{chat, detect::Harness};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Binding {
    harness: Harness,
    id: String,
    path: PathBuf,
}

pub(crate) struct Tracker {
    pub next: Instant,
    targets: Vec<(Harness, u32)>,
    binding: Option<Binding>,
}

impl Tracker {
    pub fn new() -> Self {
        Self { next: Instant::now(), targets: Vec::new(), binding: None }
    }

    /// 같은 프로세스의 일시적인 stat/부분 쓰기 실패는 연결을 지우지 않는다. PID 교체·셸 복귀는 옛 연결을 끊는다.
    fn update(&mut self, targets: Vec<(Harness, u32)>, found: Option<Binding>) -> Option<(Harness, Option<Binding>)> {
        let replaced = !self.targets.is_empty() && self.targets != targets;
        let old = self.binding.clone();
        self.targets = targets;
        if replaced {
            self.binding = None;
        }
        if let Some(b) = found {
            self.binding = Some(b);
        }
        // 셸 복귀는 foreground 감시의 released가 알린다. 여기서 session_start(None)을 보내면
        // 더 빠른 released 뒤 host의 agent를 되살릴 수 있다. 실제 교체 프로세스가 있을 때만 ID를 비운다.
        if self.targets.is_empty() || old == self.binding {
            return None;
        }
        let harness = self.binding.as_ref().or(old.as_ref())?.harness;
        Some((harness, self.binding.clone()))
    }

    pub fn poll(&mut self, shell: u32, codex: &Path, claude: &Path) -> Option<(Harness, Option<Binding>)> {
        self.next = Instant::now() + Duration::from_secs(2);
        #[cfg(unix)]
        let mut targets = crate::detect::foreground::agents(shell);
        #[cfg(not(unix))]
        let mut targets = {
            let _ = shell;
            Vec::new()
        };
        targets.sort_by_key(|(_, pid)| *pid);
        let mut bindings = targets.iter().filter_map(|&(h, pid)| resolve(h, pid, codex, claude)).collect::<Vec<_>>();
        bindings.sort_by(|a, b| a.path.cmp(&b.path));
        bindings.dedup();
        let found = (bindings.len() == 1).then(|| bindings.remove(0));
        let changed = self.update(targets, found);
        // 뒤늦은 hook이 같은 ID의 경로를 비워도 보완한다. 실제 재전송 여부는 저장한 사건과 대조한다.
        changed.or_else(|| self.binding.clone().map(|b| (b.harness, Some(b))))
    }
}

impl crate::Agent {
    pub(crate) async fn shell_chat_event(self: &std::sync::Arc<Self>, session: &str, h: Harness, b: Option<Binding>) {
        let previous =
            self.harness_last.lock().unwrap().get(session).and_then(|events| {
                events.iter().find(|e| matches!(e.kind.as_str(), "session_start" | "session")).cloned()
            });
        let same_id = b.as_ref().is_some_and(|b| {
            previous
                .as_ref()
                .is_some_and(|e| e.harness == h.label() && e.harness_session.as_deref() == Some(b.id.as_str()))
        });
        if same_id
            && previous.as_ref().and_then(|e| e.transcript.as_deref()) == b.as_ref().and_then(|b| b.path.to_str())
        {
            return;
        }
        // 같은 세션의 경로 보완은 session: 진행 보고를 다시 초기화하지 않는다.
        let kind = if same_id { "session" } else { "session_start" };
        // 새 프로세스는 옛 hook ID와 다를 수 있다. session_start가 foreign_session 문도 갱신한다.
        if b.is_none() {
            self.harness_started.lock().unwrap().remove(session);
        }
        self.harness_event(aaa_proto::HarnessEvent {
            session: session.into(),
            harness: h.label().into(),
            kind: kind.into(),
            harness_session: b.as_ref().map(|b| b.id.clone()),
            transcript: b.map(|b| b.path.to_string_lossy().into_owned()),
            at_ms: aaa_common::util::now_ms(),
            message: None,
            question_kind: None,
            percent: None,
            plan: None,
            background: None,
            context: None,
            model_observation: None,
            turn_reply: None,
        })
        .await;
    }
}

fn read_json(path: &Path) -> Option<serde_json::Value> {
    // symlink·FIFO와 거대 등록 파일을 읽지 않는다. 대화 본문은 기존 chat reader만 읽는다.
    let meta = std::fs::symlink_metadata(path)
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
        .ok()?;
    if !meta.is_file() || meta.len() > 256 * 1024 {
        return None;
    }
    serde_json::from_reader(
        std::fs::File::open(path)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()?
            .take(256 * 1024),
    )
    .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
    .ok()
}

fn resolve(harness: Harness, pid: u32, codex: &Path, claude: &Path) -> Option<Binding> {
    let root = match harness {
        Harness::Codex => codex,
        Harness::Claude => claude,
    };
    let mut found = Vec::new();
    for path in open_files(pid) {
        let Ok(path) = chat::inside(root, &path)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다"))
        else {
            continue;
        };
        let id = match harness {
            Harness::Codex => {
                let mut bytes = Vec::new();
                std::io::BufReader::new(
                    std::fs::File::open(&path)
                        .inspect_err(|error| {
                            aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                        })
                        .ok()?
                        .take(256 * 1024),
                )
                .read_until(b'\n', &mut bytes)
                .inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                })
                .ok()?;
                let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes).inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다")
                }) else {
                    continue;
                };
                if v["type"] != "session_meta" || v["payload"]["source"] != "cli" {
                    continue;
                }
                let Some(id) = v["payload"]["id"].as_str() else { continue };
                if !chat::codex_record_name(&path, id) {
                    continue;
                }
                id.to_string()
            }
            Harness::Claude => {
                let Some(id) = path.file_stem().and_then(|n| n.to_str()) else { continue };
                if path.components().any(|c| c.as_os_str() == "subagents") {
                    continue;
                }
                id.to_string()
            }
        };
        if aaa_common::util::harness_session_ok(&id) {
            found.push(Binding { harness, id, path });
        }
    }
    // Claude는 기록을 매번 열고 닫는다. interactive PID 등록으로 쉬는 중에도 정확히 연결한다.
    if harness == Harness::Claude
        && let Some(b) = claude_registry(pid, claude)
    {
        return Some(b);
    }
    found.sort_by(|a, b| a.path.cmp(&b.path));
    found.dedup();
    (found.len() == 1).then(|| found.remove(0))
}

fn claude_registry(pid: u32, root: &Path) -> Option<Binding> {
    let home = root.parent()?;
    let v = read_json(&home.join("sessions").join(format!("{pid}.json")))?;
    let id = v["sessionId"].as_str()?;
    if v["pid"].as_u64()? != u64::from(pid)
        || v["kind"] != "interactive"
        || !aaa_common::util::harness_session_ok(id)
        || v["startedAt"].as_u64()? < process_started_ms(pid)?
        || v["startedAt"].as_u64()? > aaa_common::util::now_ms()
    {
        return None;
    }
    // 프로젝트 디렉터리 하나 아래의 정확한 UUID만 찾는다(하위 agent·symlink 폴더 제외).
    let mut found = Vec::new();
    for dir in std::fs::read_dir(root)
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
        .ok()?
        .flatten()
    {
        if !dir
            .file_type()
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()?
            .is_dir()
        {
            continue;
        }
        if let Ok(path) = chat::inside(root, &dir.path().join(format!("{id}.jsonl"))) {
            found.push(path);
        }
    }
    (found.len() == 1).then(|| Binding { harness: Harness::Claude, id: id.into(), path: found.remove(0) })
}

#[cfg(target_os = "macos")]
fn process_started_ms(pid: u32) -> Option<u64> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of_val(&info) as i32;
    // SAFETY: info는 size 크기의 쓰기 가능한 버퍼다.
    let got = unsafe {
        libc::proc_pidinfo(pid as i32, libc::PROC_PIDTBSDINFO, 0, (&mut info as *mut libc::proc_bsdinfo).cast(), size)
    };
    (got == size).then_some(info.pbi_start_tvsec * 1000 + info.pbi_start_tvusec / 1000)
}

#[cfg(target_os = "linux")]
fn process_started_ms(pid: u32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
        .ok()?;
    let ticks: u64 = stat
        .get(stat.rfind(')')? + 2..)?
        .split_whitespace()
        .nth(19)?
        .parse()
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
        .ok()?;
    let boot: u64 = std::fs::read_to_string("/proc/stat")
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
        .ok()?
        .lines()
        .find_map(|l| l.strip_prefix("btime "))?
        .parse()
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
        .ok()?;
    // SAFETY: sysconf는 메모리를 넘기지 않는 조회다.
    let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    (hz > 0).then(|| boot * 1000 + ticks * 1000 / hz as u64)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn process_started_ms(_: u32) -> Option<u64> {
    None
}

#[cfg(target_os = "linux")]
fn open_files(pid: u32) -> Vec<PathBuf> {
    std::fs::read_dir(format!("/proc/{pid}/fd"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let info = std::fs::read_to_string(format!("/proc/{pid}/fdinfo/{}", e.file_name().to_str()?))
                .inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                })
                .ok()?;
            let flags = info.lines().find_map(|l| l.strip_prefix("flags:"))?.trim();
            let flags = u32::from_str_radix(flags, 8)
                .inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                })
                .ok()?;
            (flags & libc::O_ACCMODE as u32 != libc::O_RDONLY as u32).then_some(())?;
            std::fs::read_link(e.path())
                .inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                })
                .ok()
        })
        .collect()
}

#[cfg(target_os = "macos")]
fn open_files(pid: u32) -> Vec<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    // sys/proc_info.h의 vnode_fdinfowithpath. libc는 vnode_info_path까지만 제공한다.
    #[repr(C)]
    struct FileInfo {
        flags: u32,
        status: u32,
        offset: i64,
        kind: i32,
        guard: u32,
    }
    #[repr(C)]
    struct VnodePath {
        file: FileInfo,
        vnode: libc::vnode_info_path,
    }
    // 상한 있는 한 번의 목록 조회. 경합으로 늘어난 fd는 다음 차례에 본다.
    let mut fds = vec![libc::proc_fdinfo { proc_fd: 0, proc_fdtype: 0 }; 4096];
    let size = std::mem::size_of_val(fds.as_slice()) as i32;
    // SAFETY: fds는 size 크기의 쓰기 가능한 버퍼다.
    let got = unsafe { libc::proc_pidinfo(pid as i32, libc::PROC_PIDLISTFDS, 0, fds.as_mut_ptr().cast(), size) };
    if got <= 0 || got > size {
        return Vec::new();
    }
    let mut paths = Vec::new();
    for fd in fds.iter().take(got as usize / std::mem::size_of::<libc::proc_fdinfo>()) {
        if fd.proc_fdtype != libc::PROX_FDTYPE_VNODE as u32 {
            continue;
        }
        let mut info: VnodePath = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of_val(&info) as i32;
        // SAFETY: info는 size 크기의 버퍼, flavor 2는 PROC_PIDFDVNODEPATHINFO다.
        let got =
            unsafe { libc::proc_pidfdinfo(pid as i32, fd.proc_fd, 2, (&mut info as *mut VnodePath).cast(), size) };
        if got != size {
            continue;
        }
        // fi_openflags는 커널 FREAD/FWRITE 비트다. 읽기 위해 잠깐 연 다른 대화를 연결하지 않는다.
        if info.file.flags & 2 == 0 {
            continue;
        }
        // SAFETY: vip_path는 MAXPATHLEN 바이트의 char 배열이다.
        let bytes =
            unsafe { std::slice::from_raw_parts(info.vnode.vip_path.as_ptr().cast::<u8>(), libc::MAXPATHLEN as usize) };
        if let Some(nul) = bytes.iter().position(|&b| b == 0) {
            paths.push(PathBuf::from(std::ffi::OsStr::from_bytes(&bytes[..nul])));
        }
    }
    paths
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn open_files(_: u32) -> Vec<PathBuf> {
    Vec::new()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::io::Write;

    const ID: &str = "019a4c5e-1f2b-7c3d-8e4f-a5b6c7d8e9f0";
    const OTHER: &str = "019a4c5e-1f2b-7c3d-8e4f-a5b6c7d8e9f1";
    struct Lab(PathBuf);
    impl Lab {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!("aaa-shell-chat-{}", aaa_common::util::new_id()));
            std::fs::create_dir_all(&p).unwrap();
            Self(p.canonicalize().unwrap())
        }
        fn codex(&self, id: &str, source: serde_json::Value) -> std::fs::File {
            let mut f = std::fs::File::create(self.0.join(format!("rollout-test-{id}.jsonl"))).unwrap();
            writeln!(f, "{}", serde_json::json!({"type":"session_meta","payload":{"id":id,"source":source}})).unwrap();
            f
        }
    }
    impl Drop for Lab {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn codex_resumed_versioned_rollout_binds_to_thread_id_instead_of_rollout_id() {
        let lab = Lab::new();
        let _file = lab.codex(ID, serde_json::json!("cli"));
        let path = lab.0.join(format!("rollout-test-{ID}_{OTHER}.jsonl"));
        std::fs::rename(lab.0.join(format!("rollout-test-{ID}.jsonl")), &path).unwrap();
        let b = resolve(Harness::Codex, std::process::id(), &lab.0, &lab.0).expect("재개 기록 연결");
        assert_eq!(b.id, ID);
        assert_eq!(b.path, path);
        assert_eq!(chat::codex_hint(&lab.0, ID, path.to_str().unwrap()), Some(path));
    }

    #[test]
    fn codex_uses_the_process_open_file_and_rejects_ambiguous_nested_and_outside_records() {
        let lab = Lab::new();
        // 같은 cwd의 더 새 파일도 열지 않은 기록이면 연결하지 않는다.
        drop(lab.codex(OTHER, serde_json::json!("cli")));
        let file = lab.codex(ID, serde_json::json!("cli"));
        let pid = std::process::id();
        assert!(open_files(pid).contains(&lab.0.join(format!("rollout-test-{ID}.jsonl"))));
        assert_eq!(resolve(Harness::Codex, pid, &lab.0, &lab.0).unwrap().id, ID);
        let second = lab.codex(OTHER, serde_json::json!("cli"));
        assert!(resolve(Harness::Codex, pid, &lab.0, &lab.0).is_none());
        drop(second);
        let nested = lab.codex(OTHER, serde_json::json!({"subagent":"review"}));
        assert_eq!(resolve(Harness::Codex, pid, &lab.0, &lab.0).unwrap().id, ID);
        drop(file);
        assert!(resolve(Harness::Codex, pid, &lab.0, &lab.0).is_none());
        drop(nested);
        let outside = Lab::new();
        let _file = outside.codex(ID, serde_json::json!("cli"));
        std::os::unix::fs::symlink(outside.0.join(format!("rollout-test-{ID}.jsonl")), lab.0.join("escape.jsonl"))
            .unwrap();
        assert!(resolve(Harness::Codex, pid, &lab.0, &lab.0).is_none());
    }

    #[test]
    fn claude_uses_exact_live_interactive_pid_registry_and_rejects_stale_or_ambiguous_records() {
        let lab = Lab::new();
        let root = lab.0.join("projects");
        std::fs::create_dir_all(root.join("project-a")).unwrap();
        std::fs::create_dir_all(lab.0.join("sessions")).unwrap();
        std::fs::write(root.join("project-a").join(format!("{ID}.jsonl")), "{}\n").unwrap();
        let pid = std::process::id();
        let registry = lab.0.join("sessions").join(format!("{pid}.json"));
        let mut v =
            serde_json::json!({"pid":pid,"sessionId":ID,"kind":"interactive","startedAt":aaa_common::util::now_ms()});
        let save = |v: &serde_json::Value| std::fs::write(&registry, serde_json::to_vec(v).unwrap()).unwrap();
        save(&v);
        assert_eq!(claude_registry(pid, &root).unwrap().id, ID);
        v["pid"] = serde_json::json!(pid + 1);
        save(&v);
        assert!(claude_registry(pid, &root).is_none());
        v["pid"] = serde_json::json!(pid);
        v["startedAt"] = serde_json::json!(0);
        save(&v);
        assert!(claude_registry(pid, &root).is_none());
        v["startedAt"] = serde_json::json!(aaa_common::util::now_ms());
        v["kind"] = serde_json::json!("bg");
        save(&v);
        assert!(claude_registry(pid, &root).is_none());
        v["kind"] = serde_json::json!("interactive");
        v["sessionId"] = serde_json::json!("../bad");
        save(&v);
        assert!(claude_registry(pid, &root).is_none());
        v["sessionId"] = serde_json::json!(ID);
        save(&v);
        std::fs::create_dir_all(root.join("project-b")).unwrap();
        std::fs::write(root.join("project-b").join(format!("{ID}.jsonl")), "{}\n").unwrap();
        assert!(claude_registry(pid, &root).is_none());
    }

    #[test]
    fn tracker_keeps_transient_failures_but_clears_replaced_and_released_processes() {
        let mut t = Tracker::new();
        let b = Binding { harness: Harness::Codex, id: ID.into(), path: PathBuf::from("/example/session.jsonl") };
        let first = vec![(Harness::Codex, 100)];
        assert_eq!(t.update(first.clone(), Some(b.clone())), Some((Harness::Codex, Some(b.clone()))));
        assert_eq!(t.update(first, None), None);
        assert_eq!(t.update(vec![(Harness::Codex, 101)], None), Some((Harness::Codex, None)));
        assert_eq!(t.update(vec![(Harness::Codex, 101)], Some(b.clone())), Some((Harness::Codex, Some(b))));
        assert_eq!(t.update(Vec::new(), None), None);
        assert!(t.binding.is_none(), "셸 복귀는 사건 없이 내부 연결만 지운다");
        assert_eq!(t.update(Vec::new(), None), None);
    }

    #[tokio::test]
    async fn shell_return_cannot_restart_released_harness_for_either_poll_order() {
        for harness in [Harness::Codex, Harness::Claude] {
            for released_first in [false, true] {
                let lab = Lab::new();
                let a = crate::Agent::new(lab.0.clone(), "m".into(), "mac-a".into(), true, None).unwrap();
                let mut tracker = Tracker::new();
                let binding = Binding { harness, id: ID.into(), path: lab.0.join("session.jsonl") };
                let (h, b) = tracker.update(vec![(harness, 100)], Some(binding.clone())).unwrap();
                a.shell_chat_event("s", h, b).await;
                let release =
                    || crate::detect::Report { kind: "released", message: None, question_kind: None, rule: None };
                // foreground 감시(300ms)와 기록 감시(2초)는 어느 쪽이 먼저 셸 복귀를 보아도 된다.
                if released_first {
                    a.detected("s", harness, release()).await;
                }
                if let Some((h, b)) = tracker.update(Vec::new(), None) {
                    a.shell_chat_event("s", h, b).await;
                }
                if !released_first {
                    a.detected("s", harness, release()).await;
                }
                let events = a.harness_events();
                assert_eq!(events.len(), 1, "{harness:?}, released_first={released_first}: {events:?}");
                assert_eq!(events[0].kind, "released", "다시 붙는 host에도 해제만 재전송해야 한다");
                assert!(tracker.binding.is_none());
                assert!(!a.harness_started.lock().unwrap().contains_key("s"));
                // 실제 다음 프로세스가 생기면 같은 대화 resume도 다시 연결할 수 있다.
                let (h, b) = tracker.update(vec![(harness, 101)], Some(binding)).unwrap();
                a.shell_chat_event("s", h, b).await;
                assert!(
                    a.harness_events()
                        .iter()
                        .any(|e| e.kind == "session_start" && e.harness_session.as_deref() == Some(ID))
                );
            }
        }
    }

    #[tokio::test]
    async fn replacement_and_release_cannot_replay_old_identity_or_duplicate_the_hook() {
        let lab = Lab::new();
        let a = crate::Agent::new(lab.0.clone(), "m".into(), "mac-a".into(), true, None).unwrap();
        let b = Binding { harness: Harness::Codex, id: ID.into(), path: lab.0.join("session.jsonl") };
        a.shell_chat_event("s", Harness::Codex, Some(b.clone())).await;
        let first = a.harness_events().into_iter().find(|e| e.kind == "session_start").unwrap();
        a.shell_chat_event("s", Harness::Codex, Some(b.clone())).await;
        assert_eq!(a.harness_events()[0], first, "이미 연결한 hook을 다시 보내지 않는다");
        a.harness_event(aaa_proto::HarnessEvent { transcript: None, ..first.clone() }).await;
        a.shell_chat_event("s", Harness::Codex, Some(b)).await;
        let repaired = a.harness_events().into_iter().find(|e| e.kind == "session").expect("경로 보완 사건");
        assert!(repaired.transcript.is_some());
        a.shell_chat_event("s", Harness::Codex, None).await;
        let cleared = a.harness_events().into_iter().find(|e| e.kind == "session_start").unwrap();
        assert_eq!(cleared.harness_session, None, "이전 ID를 기억에서 이어받았다");
        assert_eq!(cleared.transcript, None);
        a.harness_event(aaa_proto::HarnessEvent { kind: "released".into(), ..first }).await;
        assert!(a.harness_events().iter().all(|e| e.kind == "released"));
        assert!(!a.harness_started.lock().unwrap().contains_key("s"));
    }
}

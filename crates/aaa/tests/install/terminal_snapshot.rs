//! 실패 확정 뒤 자기 CLI와 같은 이미지의 직계 자식 스택을 수집한다. 입력/경로 원문은 내보내지 않는다.
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Instant;

#[cfg(target_arch = "x86_64")]
#[path = "snapshot_native.rs"]
mod native;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Request {
    process_id: u32,
    owner_id: u32,
    exe: std::path::PathBuf,
    children: Vec<u32>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Stage {
    phase: String,
    elapsed_ms: u64,
}

struct Journal<'a> {
    root: &'a Path,
    started: Instant,
    stages: Vec<Stage>,
}
impl Journal<'_> {
    fn mark(&mut self, phase: &'static str) -> std::io::Result<()> {
        self.stages.push(Stage { phase: phase.into(), elapsed_ms: self.started.elapsed().as_millis() as u64 });
        let tmp = self.root.join("progress.tmp");
        std::fs::write(&tmp, serde_json::to_vec(&self.stages)?)?;
        std::fs::rename(tmp, self.root.join("progress.json"))
    }
}

// 현재 시험 실행 파일의 별도 프로세스 입구다. 일반 시험 실행에는 참여하지 않는다.
#[test]
#[ignore = "capture가 실행하는 격리 helper 입구"]
fn snapshot_helper_process() {
    let Some(root) = std::env::var_os("AAA_TEST_CONSOLE_SNAPSHOT_DIR") else { return };
    let root = std::path::PathBuf::from(root);
    let mut journal = Journal { root: &root, started: Instant::now(), stages: Vec::new() };
    let result = (|| -> Result<Snapshot, serde_json::Value> {
        let io_error = |phase, error: std::io::Error| serde_json::json!({"phase": phase, "error_type": "IoError", "os_code": error.raw_os_error()});
        journal.mark("read_request").map_err(|error| io_error("write_progress", error))?;
        let raw = std::fs::read(root.join("request.json")).map_err(|error| io_error("read_request", error))?;
        let request: Request = serde_json::from_slice(&raw)
            .map_err(|_| serde_json::json!({"phase": "read_request", "error_type": "SchemaError", "os_code": 0}))?;
        #[cfg(target_arch = "x86_64")]
        {
            native::capture(request.process_id, request.owner_id, &request.exe, &request.children, |phase| {
                journal.mark(phase)
            })
            .map_err(|error| serde_json::to_value(error).unwrap())
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            let _ = request;
            Err(serde_json::json!({"phase": "architecture", "error_type": "Unsupported", "os_code": 0}))
        }
    })();
    let success = result.is_ok();
    let value = match result {
        Ok(snapshot) => serde_json::to_value(snapshot).unwrap(),
        Err(failure) => {
            // 수집기가 지정한 정적 단계만 journal에 옮긴다. 오류 원문은 출력하지 않는다.
            if let Some(phase) = failure["phase"].as_str() {
                journal
                    .stages
                    .push(Stage { phase: phase.into(), elapsed_ms: journal.started.elapsed().as_millis() as u64 });
            }
            failure
        }
    };
    if journal.mark("write_response").is_err()
        || std::fs::write(root.join("frames.json"), serde_json::to_vec(&value).unwrap()).is_err()
        || journal.mark("response_ready").is_err()
    {
        std::process::exit(2);
    }
    std::process::exit(if success { 0 } else { 1 });
}

fn progress(root: &Path) -> String {
    let stages = std::fs::read(root.join("progress.json"))
        .ok()
        .filter(|raw| raw.len() <= 32 * 1024)
        .and_then(|raw| serde_json::from_slice::<Vec<Stage>>(&raw).ok())
        .filter(|stages| {
            stages.len() <= 360
                && stages.iter().all(|stage| {
                    !stage.phase.is_empty()
                        && stage.phase.len() < 80
                        && stage.phase.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
                })
        })
        .unwrap_or_default();
    let phase = stages.last().map(|stage| stage.phase.as_str()).unwrap_or("helper_start");
    format!("phase={phase}, stages={}", serde_json::to_string(&stages).unwrap())
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Snapshot {
    format: u32,
    process_id: u32,
    threads: Vec<Thread>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    console_host: Option<ConsoleHost>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    console_host_error: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    console_host_code: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    console_input: Option<ConsoleInput>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    console_input_error: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    console_input_code: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    children: Vec<ChildSnapshot>,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ChildSnapshot {
    process_id: u32,
    threads: Vec<Thread>,
    error: String,
    os_code: u32,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ConsoleHost {
    process_id: u32,
    kernel_100ns: u64,
    user_100ns: u64,
    threads: Vec<HostThread>,
    file_version: Measurement<[u16; 4]>,
}
// OS 오류도 원문·경로 없이 독립 보존한다. 부가 조회 실패가 입력 큐 관측을 지우지 않는다.
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
enum Measurement<T> {
    Ok { value: T },
    Error { phase: String, os_code: u32 },
}
impl<T> Measurement<T> {
    fn valid(&self) -> bool {
        match self {
            Self::Ok { .. } => true,
            Self::Error { phase, .. } => {
                !phase.is_empty() && phase.len() < 80 && phase.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
            }
        }
    }
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct InputWait {
    result: u32,
    os_code: Option<u32>,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ScreenSize {
    buffer_cols: i16,
    buffer_rows: i16,
    window_cols: i32,
    window_rows: i32,
}
// 앞의 최대 128개 사건에서 본 존재 여부다. false로 전체 큐의 키 부재를 단정하지 않는다.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ConsoleInput {
    input_mode: u32,
    records_present: bool,
    prefix_key_down: bool,
    prefix_enter_down: bool,
    prefix_cancel_down: bool,
    prefix_eof_down: bool,
    // helper가 새로 연 CONIN$의 상태다. 대상 CLI의 stdin handle 조회로 부르지 않는다.
    conin_wait: InputWait,
    selection_flags: Measurement<u32>,
    screen_size: Measurement<ScreenSize>,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct HostThread {
    id: u32,
    nodes: Vec<WaitNode>,
    cycle: bool,
    os_code: u32,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WaitNode {
    kind: i32,
    status: i32,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Thread {
    id: u32,
    frames: Vec<Frame>,
    error: String,
    os_code: i32,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Frame {
    module: String,
    offset: String,
    symbol: String,
}
impl Snapshot {
    fn validate(&self, pid: u32) -> bool {
        self.format == 1
            && self.process_id == pid
            && self.threads.len() <= 32
            && self.console_host_error.len() < 80
            && self.console_host_error.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
            && self.console_input_error.len() < 80
            && self.console_input_error.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
            && self
                .console_input
                .as_ref()
                .is_none_or(|input| input.selection_flags.valid() && input.screen_size.valid())
            && self.children.len() <= 4
            && self.children.iter().all(|child| {
                child.process_id != 0
                    && child.process_id != pid
                    && child.error.len() < 80
                    && child.error.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
                    && valid_threads(&child.threads)
            })
            && self.console_host.as_ref().is_none_or(|host| {
                host.process_id != pid
                    && host.process_id != 0
                    && host.threads.len() <= 32
                    && host.file_version.valid()
                    && host.threads.iter().all(|thread| {
                        thread.nodes.len() <= 16
                            && thread
                                .nodes
                                .iter()
                                .all(|node| (0..=32).contains(&node.kind) && (0..=32).contains(&node.status))
                    })
            })
            && valid_threads(&self.threads)
    }
    #[cfg(target_arch = "x86_64")]
    pub(super) fn has_child_stack(&self, pid: u32) -> bool {
        self.children.iter().any(|child| {
            child.process_id == pid
                && child.error.is_empty()
                && child.threads.iter().any(|thread| {
                    thread.error.is_empty()
                        && thread.frames.len() > 1
                        && thread.frames.iter().any(|frame| frame.module.eq_ignore_ascii_case("aaa.exe"))
                })
        })
    }
    #[cfg(target_arch = "x86_64")]
    pub(super) fn has_reader_stack(&self, thread_id: u32) -> bool {
        self.threads.iter().any(|thread| {
            thread.id == thread_id
                && thread.error.is_empty()
                && thread.frames.len() > 1
                && thread.frames.iter().any(|frame| frame.module.eq_ignore_ascii_case("aaa.exe"))
        })
    }
    #[cfg(target_arch = "x86_64")]
    pub(super) fn has_console_host_view(&self) -> bool {
        self.console_host.as_ref().is_some_and(|host| {
            host.process_id != self.process_id
                && !host.threads.is_empty()
                && matches!(host.file_version, Measurement::Ok { value } if value[0] > 0)
        })
    }
    #[cfg(target_arch = "x86_64")]
    pub(super) fn has_console_input_view(&self) -> bool {
        self.console_input.as_ref().is_some_and(|input| {
            matches!(input.conin_wait.result, 0 | 258)
                && input.conin_wait.os_code.is_none()
                && matches!(input.selection_flags, Measurement::Ok { .. })
                && matches!(input.screen_size, Measurement::Ok { ref value } if value.buffer_cols == 120 && value.buffer_rows == 36)
        }) && self.console_input_error.is_empty()
            && self.console_input_code.is_none()
    }
}

fn valid_threads(threads: &[Thread]) -> bool {
    threads.len() <= 32
        && threads.iter().all(|thread| {
            thread.frames.len() <= 33
                && thread.error.len() < 40
                && thread.error.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
                && thread.frames.iter().all(|frame| {
                    frame.module.len() <= 80
                        && frame.module.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                        && frame.offset.starts_with("0x")
                        && frame.offset.len() <= 18
                        && frame.offset[2..].bytes().all(|b| b.is_ascii_hexdigit())
                        && frame.symbol.len() <= 256
                        && frame.symbol.bytes().all(|b| b.is_ascii_alphanumeric() || b"._:$<>[], &()*+-!".contains(&b))
                })
        })
}

pub(super) fn capture(pid: u32, exe: &Path) -> Result<Snapshot, String> {
    capture_until(pid, exe, None)
}

fn capture_until(pid: u32, exe: &Path, deadline: Option<Instant>) -> Result<Snapshot, String> {
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    use windows_sys::Win32::{
        Foundation::*,
        System::Threading::{CREATE_NO_WINDOW, WaitForSingleObject},
    };
    let root = std::env::temp_dir().join(format!("aaa-console-snapshot-{}", aaa_common::util::new_id()));
    aaa_common::util::ensure_private_dir(&root).map_err(|error| io_stage("snapshot_directory", error).to_owned())?;
    #[cfg(target_arch = "x86_64")]
    let children = native::owned_children(pid, std::process::id(), exe).unwrap_or_else(|error| {
        eprintln!("PTY snapshot children unavailable: {}", serde_json::to_string(&error).unwrap());
        Vec::new()
    });
    #[cfg(target_arch = "x86_64")]
    let child_ids = children.iter().map(|child| child.pid).collect();
    #[cfg(not(target_arch = "x86_64"))]
    let child_ids = Vec::new();
    let result = (|| -> Result<Snapshot, String> {
        let out = root.join("frames.json");
        let request = Request { process_id: pid, owner_id: std::process::id(), exe: exe.into(), children: child_ids };
        let request = serde_json::to_vec(&request).map_err(|_| "snapshot_request_schema")?;
        std::fs::write(root.join("request.json"), request).map_err(|error| io_stage("snapshot_request", error))?;
        let helper = std::env::current_exe().map_err(|error| io_stage("snapshot_executable", error))?;
        let started = Instant::now();
        let mut child = Command::new(helper)
            .args(["--exact", "authorization::terminal_snapshot::snapshot_helper_process", "--ignored", "--nocapture"])
            .creation_flags(CREATE_NO_WINDOW)
            .env("AAA_TEST_CONSOLE_SNAPSHOT_DIR", &root)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| io_stage("snapshot_spawn", error))?;
        // 이미 정해진 60초 실패 뒤의 증거 수집만 제한한다. 성공/응답을 더 기다리지 않는다.
        let remaining = deadline.map_or(5000, |deadline| {
            u32::try_from(deadline.saturating_duration_since(Instant::now()).as_millis()).unwrap_or(5000).min(5000)
        });
        let wait = unsafe { WaitForSingleObject(child.as_raw_handle(), remaining) };
        let wait_os = (wait == WAIT_FAILED).then(std::io::Error::last_os_error).and_then(|error| error.raw_os_error());
        if wait != WAIT_OBJECT_0 {
            let exit_before = child.try_wait().ok().flatten().and_then(|status| status.code());
            let kill_os = child.kill().err().and_then(|error| error.raw_os_error());
            let exit_after = child.wait().ok().and_then(|status| status.code());
            let stage = if wait == WAIT_TIMEOUT { "snapshot_collection_timeout" } else { "snapshot_wait" };
            return Err(format!(
                "{stage}: wait_os={wait_os:?}, exit_before={exit_before:?}, kill_os={kill_os:?}, exit_after={exit_after:?}, target={}, {}",
                target_state(pid),
                progress(&root)
            ));
        }
        let status = child.wait().map_err(|error| io_stage("snapshot_exit", error))?;
        eprintln!(
            "PTY snapshot helper: collection_ms={}, exit={:?}, {}",
            started.elapsed().as_millis(),
            status.code(),
            progress(&root)
        );
        if !status.success() {
            return Err(helper_failure(status.code(), &out));
        }
        let raw = std::fs::read(&out).map_err(|error| io_stage("snapshot_read", error))?;
        let raw = raw.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&raw);
        if raw.len() > 256 * 1024 {
            return Err("snapshot_size".into());
        }
        let snapshot: Snapshot = serde_json::from_slice(raw).map_err(|_| "snapshot_schema")?;
        if !snapshot.validate(pid) {
            return Err("snapshot_fields".into());
        }
        Ok(snapshot)
    })();
    #[cfg(target_arch = "x86_64")]
    for child in children {
        let cleanup = match &result {
            Err(_) => true,
            Ok(snapshot) => snapshot.children.iter().any(|view| {
                view.process_id == child.pid && view.threads.iter().any(|thread| thread.error == "resume_thread")
            }),
        };
        if cleanup {
            child.terminate();
        }
    }
    // 요청 경로와 helper 원문은 panic 출력으로 옮기지 않는다.
    let _ = std::fs::remove_dir_all(&root);
    result
}

fn target_state(pid: u32) -> String {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::{Foundation::*, System::Threading::*};
    let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    if handle.is_null() {
        return format!("unavailable(os_code={})", unsafe { GetLastError() });
    }
    let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
    match unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) } {
        WAIT_TIMEOUT => "alive".into(),
        WAIT_OBJECT_0 => "exited".into(),
        _ => format!("wait_failed(os_code={})", unsafe { GetLastError() }),
    }
}

fn helper_failure(exit: Option<i32>, out: &Path) -> String {
    let value = match std::fs::read(out) {
        Ok(raw) => {
            let raw = raw.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&raw);
            serde_json::from_slice::<serde_json::Value>(raw).unwrap_or_default()
        }
        Err(error) => {
            io_stage("snapshot_helper_diagnostic", error);
            serde_json::Value::Null
        }
    };
    // helper 원문·경로·예외 문구는 오류로 옮기지 않는다. 실패 종료도 항상 분류를 보존한다.
    let word = |key: &str| {
        value[key]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() < 80 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
            .unwrap_or("unknown")
    };
    format!(
        "snapshot_helper_failed: exit={exit:?}, phase={}, type={}, os_code={:?}",
        word("phase"),
        word("error_type"),
        value["os_code"].as_i64()
    )
}

#[test]
fn helper_failure_keeps_exit_and_os_code_without_arbitrary_text() {
    let lab = super::Lab::new();
    let out = lab.0.join("failure.json");
    std::fs::write(
        &out,
        serde_json::to_vec(&serde_json::json!({
            "phase": lab.0.to_string_lossy(),
            "error_type": format!("Exception: {}", super::CANARY),
            "os_code": 5,
            "message": super::CANARY,
        }))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        helper_failure(Some(1), &out),
        "snapshot_helper_failed: exit=Some(1), phase=unknown, type=unknown, os_code=Some(5)"
    );
    std::fs::write(&out, b"invalid JSON").unwrap();
    assert_eq!(
        helper_failure(Some(2), &out),
        "snapshot_helper_failed: exit=Some(2), phase=unknown, type=unknown, os_code=None"
    );
    let journal = lab.0.join("progress.json");
    std::fs::write(
        &journal,
        serde_json::to_vec(&serde_json::json!([{"phase": super::CANARY, "elapsed_ms": 1}])).unwrap(),
    )
    .unwrap();
    assert_eq!(progress(&lab.0), "phase=helper_start, stages=[]");
    std::fs::write(&journal, br#"[{"phase":"walk_thread","elapsed_ms":3}]"#).unwrap();
    assert_eq!(progress(&lab.0), r#"phase=walk_thread, stages=[{"phase":"walk_thread","elapsed_ms":3}]"#);
}

fn io_stage(stage: &'static str, error: std::io::Error) -> &'static str {
    eprintln!("PTY snapshot I/O error: stage={stage}, kind={:?}, os_code={:?}", error.kind(), error.raw_os_error());
    stage
}

pub(super) fn record(pid: u32, exe: &Path, probe: &super::read_state_probe::Probe, failure_unix_us: u64) {
    let start = Instant::now();
    let deadline = start + std::time::Duration::from_secs(5);
    // 실패 확정 뒤 기존 5초 안에서 두 수집을 함께 진행한다. 성공·입력 대기는 다시 열지 않는다.
    let (snapshot, read_state) = std::thread::scope(|scope| {
        let read_state =
            scope.spawn(|| probe.capture(pid, super::read_state_probe::Cause::PtyDeadline, failure_unix_us, deadline));
        let snapshot = capture_until(pid, exe, Some(deadline));
        (snapshot, read_state.join().unwrap_or_else(|_| Err("read_state_collector_panicked".into())))
    });
    match read_state {
        Ok(state) => eprintln!(
            "PTY original stdin after failure: collection_ms={} state={}",
            start.elapsed().as_millis(),
            serde_json::to_string(&state).unwrap()
        ),
        Err(stage) => {
            eprintln!("PTY original stdin after failure: collection_ms={} error={stage}", start.elapsed().as_millis())
        }
    }
    match snapshot {
        Ok(snapshot) => eprintln!(
            "PTY stack after failure: collection_ms={} snapshot={}",
            start.elapsed().as_millis(),
            serde_json::to_string(&snapshot).unwrap()
        ),
        Err(stage) => eprintln!("PTY stack after failure: collection_ms={} error={stage}", start.elapsed().as_millis()),
    }
}

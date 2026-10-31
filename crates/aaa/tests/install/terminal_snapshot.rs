//! 실패가 확정된 뒤 자기 CLI 자식의 함수 스택만 수집한다. 메모리/입력/경로 원문은 내보내지 않는다.
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Instant;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Snapshot {
    format: u32,
    process_id: u32,
    threads: Vec<Thread>,
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
            && self.threads.iter().all(|thread| {
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
                            && frame
                                .symbol
                                .bytes()
                                .all(|b| b.is_ascii_alphanumeric() || b"._:$<>[], &()*+-!".contains(&b))
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
}

pub(super) fn capture(pid: u32, exe: &Path) -> Result<Snapshot, String> {
    use std::os::windows::io::AsRawHandle;
    use std::process::{Command, Stdio};
    use windows_sys::Win32::{Foundation::*, System::Threading::WaitForSingleObject};
    let root = std::env::temp_dir().join(format!("aaa-console-snapshot-{}", aaa_common::util::new_id()));
    aaa_common::util::ensure_private_dir(&root).map_err(|error| io_stage("snapshot_directory", error).to_owned())?;
    let result = (|| -> Result<Snapshot, String> {
        let out = root.join("frames.json");
        let stderr = std::fs::File::create(root.join("helper.log")).map_err(|error| io_stage("helper_log", error))?;
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/windows-console-snapshot.ps1");
        let mut child = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(script)
            .arg("-TargetPid")
            .arg(pid.to_string())
            .arg("-OwnerPid")
            .arg(std::process::id().to_string())
            .arg("-ExpectedExe")
            .arg(exe)
            .arg("-OutFile")
            .arg(&out)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(stderr)
            .spawn()
            .map_err(|error| io_stage("snapshot_spawn", error))?;
        // 이미 정해진 60초 실패 뒤의 증거 수집만 제한한다. 성공/응답을 더 기다리지 않는다.
        let wait = unsafe { WaitForSingleObject(child.as_raw_handle(), 5000) };
        if wait == WAIT_FAILED {
            io_stage("snapshot_wait", std::io::Error::last_os_error());
        }
        if wait != WAIT_OBJECT_0 {
            let _ = child.kill();
            let _ = child.wait();
            return Err(if wait == WAIT_TIMEOUT { "snapshot_collection_timeout" } else { "snapshot_wait" }.into());
        }
        let status = child.wait().map_err(|error| io_stage("snapshot_exit", error))?;
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
    // helper 원문은 panic 출력으로 옮기지 않는다. 허용한 typed 값만 기존 test.log에 남는다.
    let _ = std::fs::remove_dir_all(&root);
    result
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
}

fn io_stage(stage: &'static str, error: std::io::Error) -> &'static str {
    eprintln!("PTY snapshot I/O error: stage={stage}, kind={:?}, os_code={:?}", error.kind(), error.raw_os_error());
    stage
}

pub(super) fn record(pid: u32, exe: &Path) {
    let start = Instant::now();
    match capture(pid, exe) {
        Ok(snapshot) => eprintln!(
            "PTY stack after failure: collection_ms={} snapshot={}",
            start.elapsed().as_millis(),
            serde_json::to_string(&snapshot).unwrap()
        ),
        Err(stage) => eprintln!("PTY stack after failure: collection_ms={} error={stage}", start.elapsed().as_millis()),
    }
}

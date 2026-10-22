//! payload·환경을 읽지 않는 panic 진단. 콜스택은 함수 이름과 허용한 상대 경로만 남긴다.
//! 알림 소비자는 `latest()`의 안전한 필드만 읽는다. hook 안에서는 전송·tracing을 하지 않는다.
use std::collections::HashMap;
use std::io::Write as _;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU8, Ordering};

include!(concat!(env!("OUT_DIR"), "/panic_build.rs"));

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
#[repr(u8)]
pub enum ProcessKind {
    Cli,
    Host,
    Agent,
    SessionHost,
}

static PROCESS: AtomicU8 = AtomicU8::new(ProcessKind::Cli as u8);

pub fn set_process(kind: ProcessKind) {
    PROCESS.store(kind as u8, Ordering::Relaxed);
}

fn process() -> ProcessKind {
    match PROCESS.load(Ordering::Relaxed) {
        1 => ProcessKind::Host,
        2 => ProcessKind::Agent,
        3 => ProcessKind::SessionHost,
        _ => ProcessKind::Cli,
    }
}

/// 본문·원본 경로·임의 thread 이름은 보관하지 않는다. 함수 이름은 실행 파일 심볼에서만 얻는다.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Snapshot {
    pub file: &'static str,
    pub line: u32,
    pub column: u32,
    pub thread: &'static str,
    pub thread_id: String,
    pub task_id: Option<String>,
    pub process: ProcessKind,
    pub build_sha: &'static str,
    /// 프로세스 시작부터 같은 원본 위치에서 panic한 횟수(처음은 1).
    pub count: u64,
    pub stack: Vec<StackFrame>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct StackFrame {
    pub function: String,
    pub file: &'static str,
    pub line: Option<u32>,
    /// 별도 심볼이 필요한 플랫폼에서 같은 판의 PDB로 해석할 module 상대 명령 주소.
    pub module_offset: Option<usize>,
}

// 이름은 실행 파일의 심볼에서만 얻는다. 원본 경로·payload·지역 변수는 포맷하지 않는다.
// 크래시 로그가 무한 재귀/거대한 제네릭 이름으로 커지지 않도록 프레임과 이름 길이를 제한한다.
fn call_stack() -> Vec<StackFrame> {
    let mut frames = Vec::new();
    let mut count = 0;
    backtrace::trace(|frame| {
        count += 1;
        let module_offset =
            frame.module_base_address().and_then(|base| (frame.ip() as usize).checked_sub(base as usize));
        let before = frames.len();
        backtrace::resolve_frame(frame, |symbol| {
            if frames.len() >= 64 {
                return;
            }
            let function = symbol.name().map(|name| format!("{name:#}")).unwrap_or_else(|| "unknown".into());
            let function = if function.contains(['/', '\\']) || function.chars().any(char::is_control) {
                "external".into()
            } else {
                function.chars().take(512).collect()
            };
            let file = symbol.filename().and_then(|p| p.to_str()).map(safe_file).unwrap_or("external");
            frames.push(StackFrame { function, file, line: symbol.lineno(), module_offset });
        });
        if frames.len() == before && frames.len() < 64 {
            frames.push(StackFrame { function: "unknown".into(), file: "external", line: None, module_offset });
        }
        count < 64 && frames.len() < 64
    });
    frames
}

#[derive(Default)]
struct State {
    counts: HashMap<(&'static str, u32, u32), u64>,
    latest: Option<Snapshot>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

pub fn latest() -> Option<Snapshot> {
    STATE.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|s| s.latest.clone())
}

// 출력값은 허용 목록에서 고른다. 절대 경로 앞부분에는 사용자 홈·빌드 환경이 있을 수 있다.
fn safe_file(file: &str) -> &'static str {
    let normalized = file.replace('\\', "/");
    SOURCE_FILES
        .iter()
        .copied()
        .find(|known| normalized == *known || normalized.strip_suffix(*known).is_some_and(|p| p.ends_with('/')))
        .unwrap_or("external")
}

pub fn install() {
    std::panic::set_hook(Box::new(|info| {
        let location = info.location();
        let (raw_file, line, column) = location.map(|l| (l.file(), l.line(), l.column())).unwrap_or(("", 0, 0));
        let thread = std::thread::current();
        let name = match thread.name() {
            Some("main") => "main",
            Some("aaa-cli") => "aaa-cli",
            Some("tokio-rt-worker" | "tokio-runtime-worker") => "tokio-runtime-worker",
            _ => "other",
        };
        let snapshot = Snapshot {
            file: safe_file(raw_file),
            line,
            column,
            thread: name,
            thread_id: format!("{:?}", thread.id()),
            task_id: tokio::task::try_id().map(|id| id.to_string()),
            process: process(),
            build_sha: BUILD_SHA,
            count: 0,
            stack: call_stack(),
        };
        let snapshot = {
            let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
            let state = state.get_or_insert_with(State::default);
            let count = state.counts.entry((raw_file, line, column)).or_default();
            *count = count.saturating_add(1);
            let snapshot = Snapshot { count: *count, ..snapshot };
            state.latest = Some(snapshot.clone());
            snapshot
        };
        // stderr가 막혀도 latest()는 읽힌다. 외부 I/O 중 상태 잠금을 잡으면 잠금 순서가 뒤집힐 수 있다.
        // stderr 실패가 hook의 추가 panic을 만들지 않게 한다.
        let mut stderr = std::io::stderr().lock();
        let _ = stderr.write_all(b"aaa panic: ");
        let _ = serde_json::to_writer(&mut stderr, &snapshot);
        let _ = stderr.write_all(b"\n");
    }));
}

#[cfg(test)]
mod tests;

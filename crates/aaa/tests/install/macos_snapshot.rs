//! 설치 PTY의 사건 상한 판정 뒤 소유한 프로세스와 러너의 상태를 보존한다.
use portable_pty::CommandBuilder;
use std::mem::{MaybeUninit, size_of, size_of_val};
use std::path::{Path, PathBuf};

pub(crate) fn enabled() -> bool {
    std::env::var_os("AAA_CI_MACOS_INSTALL_PTY_DIAGNOSTICS").as_deref() == Some("1".as_ref())
}

pub(super) struct Probe {
    lab: PathBuf,
    events: Option<PathBuf>,
    cpu_at_start: Option<[u32; 4]>,
}

impl Probe {
    pub(super) fn new(command: &CommandBuilder) -> Option<Self> {
        if !enabled() {
            return None;
        }
        let lab = PathBuf::from(command.get_env("HOME")?);
        // 셸 fixture의 ambient 환경을 따라 다른 실행의 상태 폴더를 읽지 않는다.
        let prefix = format!("aaa-install-{}-", std::process::id());
        if !lab.file_name()?.to_str()?.starts_with(&prefix)
            || command.get_env("AAA_STATE_DIR")? != lab.join("state").as_os_str()
        {
            return None;
        }
        let events = command
            .get_argv()
            .windows(2)
            .find(|pair| pair[0] == "--events-file")
            .map(|pair| PathBuf::from(&pair[1]))
            .filter(|path| path.parent() == Some(lab.as_path()));
        Some(Self { lab, events, cpu_at_start: cpu_ticks() })
    }

    pub(super) fn record(self, cli_pid: u32, cli_live: bool) {
        eprintln!("PTY macOS failure utc_ms={} cli_live={cli_live}", aaa_common::util::now_ms());
        let cpu_delta: Option<[u32; 4]> = self
            .cpu_at_start
            .zip(cpu_ticks())
            .map(|(start, end)| std::array::from_fn(|index| end[index].wrapping_sub(start[index])));
        if cli_live && record_task("cli", cli_pid as i32, std::process::id()) {
            record_children(cli_pid);
        }
        let mut load = [0.0; 3];
        // SAFETY: load는 3개의 double을 받을 수 있다.
        let load_count = unsafe { libc::getloadavg(load.as_mut_ptr(), load.len() as i32) }.max(0) as usize;
        eprintln!("PTY macOS runner load={:?} cpu_ticks(user,system,idle,nice)={cpu_delta:?}", &load[..load_count]);
        record_timebase();
        record_memory();
        record_disk_space(&self.lab);
        record_executions(&self.lab.join("state"));
        if let Some(events) = &self.events {
            super::super::recovery::record_public_events("PTY macOS", events);
        }
    }
}

#[allow(deprecated)] // 기존 libc의 Mach API를 사용한다.
fn record_timebase() {
    let mut timebase = MaybeUninit::<libc::mach_timebase_info>::uninit();
    // SAFETY: mach_timebase_info는 성공 시 구조체 전체를 채운다.
    let result = unsafe { libc::mach_timebase_info(timebase.as_mut_ptr()) };
    if result == libc::KERN_SUCCESS {
        let timebase = unsafe { timebase.assume_init() };
        eprintln!("PTY macOS process_timebase_ns(mach_ticks*numer/denom)=({},{})", timebase.numer, timebase.denom);
    } else {
        eprintln!("PTY macOS process_timebase unavailable: kern_result={result}");
    }
}

fn record_children(parent: u32) {
    let mut children = [0; 8];
    // SAFETY: 배열의 전체 바이트 길이를 주며 반환된 PID만 확인한다.
    let count =
        unsafe { libc::proc_listchildpids(parent as i32, children.as_mut_ptr().cast(), size_of_val(&children) as i32) };
    if count < 0 {
        eprintln!(
            "PTY macOS direct_children unavailable: os_code={:?}",
            std::io::Error::last_os_error().raw_os_error()
        );
        return;
    }
    eprintln!("PTY macOS direct_children={count} capacity={}", children.len());
    for pid in children.into_iter().take(count as usize).filter(|pid| *pid > 0) {
        record_task("cli_child", pid, parent);
    }
}

#[allow(deprecated)] // aaa-agent의 macOS 자원 측정과 같은 Mach API다.
fn host_port() -> libc::mach_port_t {
    static PORT: std::sync::OnceLock<libc::mach_port_t> = std::sync::OnceLock::new();
    *PORT.get_or_init(|| unsafe { libc::mach_host_self() })
}

fn cpu_ticks() -> Option<[u32; 4]> {
    let mut stats = MaybeUninit::<libc::host_cpu_load_info>::uninit();
    let mut count = libc::HOST_CPU_LOAD_INFO_COUNT;
    // SAFETY: HOST_CPU_LOAD_INFO의 ABI와 같은 버퍼를 제공한다.
    let result =
        unsafe { libc::host_statistics(host_port(), libc::HOST_CPU_LOAD_INFO, stats.as_mut_ptr().cast(), &mut count) };
    (result == libc::KERN_SUCCESS && count == libc::HOST_CPU_LOAD_INFO_COUNT)
        .then(|| unsafe { stats.assume_init().cpu_ticks })
}

fn record_memory() {
    let mut stats = MaybeUninit::<libc::vm_statistics64>::zeroed();
    let mut count = libc::HOST_VM_INFO64_COUNT;
    // SAFETY: HOST_VM_INFO64의 ABI와 같은 버퍼를 제공한다. 이전 커널이 채우지 않은 뒷부분은 0이다.
    let result =
        unsafe { libc::host_statistics64(host_port(), libc::HOST_VM_INFO64, stats.as_mut_ptr().cast(), &mut count) };
    if result == libc::KERN_SUCCESS {
        let s = unsafe { stats.assume_init() };
        // libc의 VM 구조체는 packed라 로그 매크로에서 필드를 참조하기 전에 복사한다.
        let pages = (s.free_count, s.speculative_count, s.compressor_page_count, s.pageouts, s.swapouts);
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        eprintln!(
            "PTY macOS memory page_bytes={page_size} pages(free,speculative,compressed,pageouts_total,swapouts_total)={pages:?}"
        );
    } else {
        eprintln!("PTY macOS memory unavailable: kern_result={result}");
    }
}

fn record_disk_space(path: &Path) {
    use std::os::unix::ffi::OsStrExt;
    let Ok(path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else { return };
    let mut stats = MaybeUninit::<libc::statfs>::uninit();
    // SAFETY: 경로와 struct statfs 버퍼는 호출이 끝날 때까지 유효하다.
    if unsafe { libc::statfs(path.as_ptr(), stats.as_mut_ptr()) } == 0 {
        let s = unsafe { stats.assume_init() };
        eprintln!("PTY macOS disk available_bytes={}", s.f_bavail.saturating_mul(u64::from(s.f_bsize)));
    } else {
        eprintln!("PTY macOS disk unavailable: os_code={:?}", std::io::Error::last_os_error().raw_os_error());
    }
}

fn record_task(role: &str, pid: i32, parent: u32) -> bool {
    let mut info = MaybeUninit::<libc::proc_taskallinfo>::uninit();
    let size = size_of::<libc::proc_taskallinfo>() as i32;
    // SAFETY: PROC_PIDTASKALLINFO의 ABI와 같은 버퍼를 제공한다.
    let got = unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTASKALLINFO, 0, info.as_mut_ptr().cast(), size) };
    if got != size {
        eprintln!(
            "PTY macOS process role={role} pid={pid} unavailable: os_code={:?}",
            std::io::Error::last_os_error().raw_os_error()
        );
        return false;
    }
    let info = unsafe { info.assume_init() };
    if info.pbsd.pbi_ppid != parent || info.pbsd.pbi_uid != unsafe { libc::geteuid() } {
        eprintln!("PTY macOS process role={role} pid={pid} no_longer_owned");
        return false;
    }
    let task = info.ptinfo;
    let mut usage = MaybeUninit::<libc::rusage_info_v2>::uninit();
    // SAFETY: RUSAGE_INFO_V2가 요구하는 구조체를 포인터로 넘긴다.
    let disk_bytes = (unsafe { libc::proc_pid_rusage(pid, libc::RUSAGE_INFO_V2, usage.as_mut_ptr().cast()) } == 0)
        .then(|| {
            let usage = unsafe { usage.assume_init() };
            (usage.ri_diskio_bytesread, usage.ri_diskio_byteswritten)
        });
    eprintln!(
        "PTY macOS process role={role} pid={pid} ppid={parent} bsd_state={} threads={} running={} cpu_mach_ticks(user,system)=({}, {}) rss_bytes={} pageins={} disk_bytes(read,write)={disk_bytes:?}",
        info.pbsd.pbi_status,
        task.pti_threadnum,
        task.pti_numrunning,
        task.pti_total_user,
        task.pti_total_system,
        task.pti_resident_size,
        task.pti_pageins,
    );
    true
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum Stage {
    Intent,
    FilesPrepared,
    FilesApplied,
}

#[derive(serde::Deserialize)]
struct Execution {
    worker_pid: u32,
    stage: Stage,
    last_exit: Option<u8>,
}

fn record_executions(state: &Path) {
    let entries = match std::fs::read_dir(state.join("install/executions")) {
        Ok(entries) => entries,
        Err(error) => {
            eprintln!("PTY macOS executions unavailable: kind={:?} os_code={:?}", error.kind(), error.raw_os_error());
            return;
        }
    };
    for entry in entries.take(8) {
        let bytes = entry.and_then(|entry| aaa_common::private_file::read(&entry.path(), 128 * 1024));
        match bytes {
            Ok(bytes) => match serde_json::from_slice::<Execution>(&bytes) {
                Ok(execution) => eprintln!(
                    "PTY macOS execution worker_pid={} stage={:?} last_exit={:?}",
                    execution.worker_pid, execution.stage, execution.last_exit
                ),
                Err(error) => eprintln!("PTY macOS execution parse failed: category={:?}", error.classify()),
            },
            Err(error) => {
                eprintln!("PTY macOS execution unavailable: kind={:?} os_code={:?}", error.kind(), error.raw_os_error())
            }
        }
    }
}

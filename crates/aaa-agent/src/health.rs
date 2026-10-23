//! 머신 건강(§8.10). 주기적으로 디스크·부하·전원·toolchain을 재서 host에 보낸다.
//!
//! - CPU %·메모리(§8.245)는 점검보다 자주(15초) 재되 명령을 돌리지 않는다. 사이에는 마지막 점검을 복사해 그 칸만 바꿔 보낸다.
//! - 이상(problems)은 새 세션을 거부할 이유다: 작업 root 디스크 여유 부족, toolchain 명령 실패.
//! - 주의(warnings)는 보이기만 한다: 배터리 전원.
//! - 시계는 host가 보고의 `at_ms`와 자기 시계를 비교해 본다.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use aaa_config::HealthSection;
use aaa_proto::{AgentEvent, HealthProblem, LinkMsg, MachineHealth, ToolCheck};

use crate::Agent;

const TOOL_TIMEOUT: Duration = Duration::from_secs(10);
/// CPU·메모리 표본 주기(점검이 이보다 드물 때 사이에 잰다)
const STATS_EVERY: Duration = Duration::from_secs(15);
/// CPU %를 낼 가장 짧은 구간: 점검과 표본이 잇달아 오면 앞 값을 그대로 둔다
const MIN_CPU_WINDOW: Duration = Duration::from_secs(1);

/// (여유, 전체) 바이트
#[cfg(unix)]
fn disk(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: c는 NUL로 끝나는 경로이고 st는 쓸 수 있는 statvfs다
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return None;
    }
    let unit = st.f_frsize as u64;
    Some((st.f_bavail as u64 * unit, st.f_blocks as u64 * unit))
}

#[cfg(not(unix))]
fn disk(_: &Path) -> Option<(u64, u64)> {
    None
}

#[cfg(unix)]
fn load_x100() -> Option<u32> {
    let mut l = [0f64; 3];
    // SAFETY: 길이 3 배열에 1개만 쓴다
    (unsafe { libc::getloadavg(l.as_mut_ptr(), 1) } == 1).then(|| (l[0] * 100.0).round() as u32)
}

#[cfg(not(unix))]
fn load_x100() -> Option<u32> {
    None
}

/// CPU 누적 시간(쉼, 전체). 단위는 OS마다 다르고 차이만 쓴다
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CpuTicks {
    idle: u64,
    total: u64,
}

/// 앞 표본부터의 CPU 사용률(0~100, 모든 코어 평균). 같은 표본·되감긴 counter는 None
fn cpu_pct(prev: CpuTicks, now: CpuTicks) -> Option<u8> {
    let total = now.total.checked_sub(prev.total)?;
    let idle = now.idle.checked_sub(prev.idle)?;
    if total == 0 || idle > total {
        return None;
    }
    Some(((total - idle) as f64 * 100.0 / total as f64).round() as u8)
}

#[cfg(target_os = "macos")]
#[allow(deprecated)] // libc는 mach 함수를 mach2 crate로 보내지만 새 의존성 없이 이 둘만 쓴다
fn host_port() -> libc::mach_port_t {
    // mach_host_self()는 부를 때마다 send right를 하나 더 얻는다: 한 번만 얻어 둔다
    static PORT: std::sync::OnceLock<libc::mach_port_t> = std::sync::OnceLock::new();
    *PORT.get_or_init(|| unsafe { libc::mach_host_self() })
}

#[cfg(target_os = "macos")]
fn cpu_ticks() -> Option<CpuTicks> {
    let mut info: libc::host_cpu_load_info = unsafe { std::mem::zeroed() };
    let mut n = libc::HOST_CPU_LOAD_INFO_COUNT;
    // SAFETY: info는 HOST_CPU_LOAD_INFO_COUNT개의 natural_t를 담는 구조다
    let r =
        unsafe { libc::host_statistics(host_port(), libc::HOST_CPU_LOAD_INFO, &mut info as *mut _ as *mut _, &mut n) };
    if r != libc::KERN_SUCCESS {
        return None;
    }
    let t = info.cpu_ticks.map(u64::from);
    Some(CpuTicks { idle: t[libc::CPU_STATE_IDLE as usize], total: t.iter().sum() })
}

/// (사용, 전체) 바이트. 사용 = Activity Monitor 「사용된 메모리」(앱 = internal − purgeable, wired, 압축). 파일 캐시는 가용이다
#[cfg(target_os = "macos")]
fn memory() -> Option<(u64, u64)> {
    let mut total = 0u64;
    let mut len = std::mem::size_of::<u64>();
    // SAFETY: hw.memsize는 u64다
    if unsafe {
        libc::sysctlbyname(c"hw.memsize".as_ptr(), &mut total as *mut u64 as *mut _, &mut len, std::ptr::null_mut(), 0)
    } != 0
    {
        return None;
    }
    let mut v: libc::vm_statistics64 = unsafe { std::mem::zeroed() };
    let mut n = libc::HOST_VM_INFO64_COUNT;
    // SAFETY: v는 HOST_VM_INFO64_COUNT개의 integer_t를 담는 구조다
    let r = unsafe { libc::host_statistics64(host_port(), libc::HOST_VM_INFO64, &mut v as *mut _ as *mut _, &mut n) };
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if r != libc::KERN_SUCCESS || page <= 0 || total == 0 {
        return None;
    }
    let pages = u64::from(v.internal_page_count).saturating_sub(v.purgeable_count.into())
        + u64::from(v.wire_count)
        + u64::from(v.compressor_page_count);
    Some(((pages * page as u64).min(total), total))
}

/// `/proc/stat` 첫 줄(user nice system idle iowait irq softirq steal …). iowait도 쉼이고 guest는 user에 이미 들었다
#[cfg(any(target_os = "linux", all(test, unix)))]
fn parse_proc_stat(text: &str) -> Option<CpuTicks> {
    let line = text.lines().find(|l| l.starts_with("cpu "))?;
    let v: Vec<u64> = line.split_whitespace().skip(1).take(8).map(|x| x.parse().ok()).collect::<Option<_>>()?;
    (v.len() >= 5).then(|| CpuTicks { idle: v[3] + v[4], total: v.iter().sum() })
}

/// `/proc/meminfo`의 (MemTotal − MemAvailable, MemTotal) 바이트
#[cfg(any(target_os = "linux", all(test, unix)))]
fn parse_meminfo(text: &str) -> Option<(u64, u64)> {
    let kb = |key: &str| -> Option<u64> {
        let l = text.lines().find(|l| l.starts_with(key))?;
        l[key.len()..].trim().trim_end_matches("kB").trim().parse::<u64>().ok().map(|k| k * 1024)
    };
    let (total, avail) = (kb("MemTotal:")?, kb("MemAvailable:")?);
    (total > 0).then(|| (total.saturating_sub(avail), total))
}

#[cfg(target_os = "linux")]
fn cpu_ticks() -> Option<CpuTicks> {
    parse_proc_stat(&std::fs::read_to_string("/proc/stat").ok()?)
}

#[cfg(target_os = "linux")]
fn memory() -> Option<(u64, u64)> {
    parse_meminfo(&std::fs::read_to_string("/proc/meminfo").ok()?)
}

#[cfg(windows)]
fn cpu_ticks() -> Option<CpuTicks> {
    use windows_sys::Win32::Foundation::FILETIME;
    let ft = |f: FILETIME| (u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime);
    let (mut idle, mut kernel, mut user) = unsafe { std::mem::zeroed::<(FILETIME, FILETIME, FILETIME)>() };
    // SAFETY: 세 FILETIME에 쓴다. kernel 시간은 idle을 포함한다
    if unsafe { windows_sys::Win32::System::Threading::GetSystemTimes(&mut idle, &mut kernel, &mut user) } == 0 {
        return None;
    }
    Some(CpuTicks { idle: ft(idle), total: ft(kernel) + ft(user) })
}

/// (전체 − ullAvailPhys, 전체). 가용에는 standby(캐시)가 든다
#[cfg(windows)]
fn memory() -> Option<(u64, u64)> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut m: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    m.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    // SAFETY: dwLength를 채운 MEMORYSTATUSEX에 쓴다
    if unsafe { GlobalMemoryStatusEx(&mut m) } == 0 || m.ullTotalPhys == 0 {
        return None;
    }
    Some((m.ullTotalPhys.saturating_sub(m.ullAvailPhys), m.ullTotalPhys))
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn cpu_ticks() -> Option<CpuTicks> {
    None
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn memory() -> Option<(u64, u64)> {
    None
}

/// CPU·메모리 표본기. CPU %는 앞 표본부터의 평균이라 첫 표본은 None이다
#[derive(Default)]
struct Stats {
    prev: Option<(Instant, CpuTicks)>,
    last: Option<u8>,
}

impl Stats {
    /// 지금 CPU %. 앞 표본과 MIN_CPU_WINDOW보다 가까우면 다시 읽지 않고 앞 값을 둔다
    fn cpu_at(&mut self, now: Instant, read: impl FnOnce() -> Option<CpuTicks>) -> Option<u8> {
        if self.prev.is_none_or(|(at, _)| now.duration_since(at) >= MIN_CPU_WINDOW)
            && let Some(ticks) = read()
        {
            self.last = self.prev.and_then(|(_, p)| cpu_pct(p, ticks));
            self.prev = Some((now, ticks));
        }
        self.last
    }

    /// 보고에 CPU %·메모리·부하와 잰 때를 채운다
    fn fill(&mut self, h: &mut MachineHealth) {
        let mem = memory();
        h.at_ms = aaa_common::util::now_ms();
        h.cpu_pct = self.cpu_at(Instant::now(), cpu_ticks);
        h.mem_used_bytes = mem.map(|m| m.0);
        h.mem_total_bytes = mem.map(|m| m.1);
        h.load_x100 = load_x100();
    }
}

/// 전원 연결 여부. 배터리가 없는 Mac은 `pmset`이 AC Power를 알린다.
async fn on_ac() -> Option<bool> {
    if cfg!(target_os = "macos") {
        let out = run(&["pmset".into(), "-g".into(), "batt".into()]).await.ok()?;
        if out.contains("'AC Power'") {
            Some(true)
        } else if out.contains("'Battery Power'") {
            Some(false)
        } else {
            None
        }
    } else if cfg!(target_os = "linux") {
        let mut any = false;
        for e in std::fs::read_dir("/sys/class/power_supply").ok()?.flatten() {
            let p = e.path();
            if std::fs::read_to_string(p.join("type")).is_ok_and(|t| t.trim() == "Mains") {
                any = true;
                if std::fs::read_to_string(p.join("online")).is_ok_and(|o| o.trim() == "1") {
                    return Some(true);
                }
            }
        }
        any.then_some(false)
    } else {
        None
    }
}

/// 명령 하나(시간 상한). 성공하면 stdout+stderr를 돌려준다.
async fn run(argv: &[String]) -> Result<String, String> {
    let mut c = tokio::process::Command::new(&argv[0]);
    c.args(&argv[1..]).stdin(std::process::Stdio::null()).kill_on_drop(true);
    let out = tokio::time::timeout(TOOL_TIMEOUT, c.output())
        .await
        .map_err(|_| format!("{}초 안에 끝나지 않았다", TOOL_TIMEOUT.as_secs()))?
        .map_err(|e| format!("실행하지 못했다: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    if out.status.success() { Ok(text) } else { Err(format!("{}: {}", out.status, first_line(&text))) }
}

fn first_line(s: &str) -> String {
    s.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").chars().take(200).collect()
}

pub async fn collect(cfg: &HealthSection, work_root: &Path, work_mount: Option<&Path>) -> MachineHealth {
    let mut problems = Vec::new();
    let real = match crate::work_root::check(work_root, work_mount) {
        Ok(real) => Some(real),
        Err(e) => {
            problems.push(HealthProblem { code: "work_root".into(), text: format!("{e:#}") });
            None
        }
    };
    // mount 검사가 통과한 디스크만 잰다. 없는 외장의 내장 상위 폴더를 대신 재지 않는다.
    let probe = real.as_deref().and_then(|r| r.ancestors().find(|p| p.exists()));
    let d = probe.and_then(disk);
    let mut warnings = Vec::new();
    if let Some((free, _)) = d
        && cfg.min_free_gb > 0
        && free < cfg.min_free_gb.saturating_mul(1 << 30)
    {
        problems.push(HealthProblem {
            code: "disk".into(),
            text: format!(
                "디스크 여유 {:.1}GB < {}GB({})",
                free as f64 / (1u64 << 30) as f64,
                cfg.min_free_gb,
                probe.unwrap().display()
            ),
        });
    }
    let mut toolchain = Vec::new();
    for (name, argv) in &cfg.toolchain {
        let check = match run(argv).await {
            Ok(out) => ToolCheck { name: name.clone(), ok: true, version: first_line(&out) },
            Err(e) => {
                problems.push(HealthProblem {
                    code: format!("toolchain:{name}"),
                    text: format!("toolchain {name} 실패: {e}"),
                });
                ToolCheck { name: name.clone(), ok: false, version: e }
            }
        };
        toolchain.push(check);
    }
    let ac = on_ac().await;
    if ac == Some(false) {
        warnings.push("배터리 전원".into());
    }
    MachineHealth {
        info: None,
        at_ms: aaa_common::util::now_ms(),
        disk_free_bytes: d.map(|x| x.0),
        disk_total_bytes: d.map(|x| x.1),
        load_x100: load_x100(),
        cpus: std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(1),
        // Stats::fill이 채운다
        cpu_pct: None,
        mem_used_bytes: None,
        mem_total_bytes: None,
        on_ac: ac,
        toolchain,
        problems,
        warnings,
    }
}

/// 표시용 명령은 실패 이유를 값으로 노출하지 않는다.
async fn info_value(program: &str, args: &[&str]) -> Option<String> {
    let argv: Vec<String> = std::iter::once(program).chain(args.iter().copied()).map(str::to_owned).collect();
    run(&argv).await.ok().map(|s| first_line(&s)).filter(|s| !s.is_empty())
}

async fn machine_info(root: &Path) -> aaa_proto::MachineInfo {
    let (claude, codex) = tokio::join!(info_value("claude", &["--version"]), info_value("codex", &["--version"]));
    #[cfg(target_os = "macos")]
    let (os_version, cpu_model) = tokio::join!(
        info_value("/usr/bin/sw_vers", &["-productVersion"]),
        info_value("/usr/sbin/sysctl", &["-n", "machdep.cpu.brand_string"])
    );
    #[cfg(target_os = "linux")]
    let (os_version, cpu_model) = (
        std::fs::read_to_string("/etc/os-release").ok().and_then(|s| {
            s.lines().find_map(|l| l.strip_prefix("PRETTY_NAME=").map(|v| v.trim_matches('"').to_owned()))
        }),
        std::fs::read_to_string("/proc/cpuinfo").ok().and_then(|s| {
            s.lines().find_map(|l| {
                l.strip_prefix("model name").and_then(|v| v.split_once(':')).map(|(_, v)| v.trim().to_owned())
            })
        }),
    );
    #[cfg(windows)]
    let (os_version, cpu_model) = (
        info_value("cmd.exe", &["/d", "/c", "ver"]).await,
        std::env::var("PROCESSOR_IDENTIFIER").ok().filter(|v| !v.is_empty()),
    );
    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    let (os_version, cpu_model) = (None, None);
    // 상속된 XPC_SERVICE_NAME만으로는 pane에서 시작한 시험 agent도 launchd로 오인한다.
    // 직접 부모가 등록된 launcher인 경우만 launchd로 표시한다.
    let installation = if cfg!(windows) && std::env::args_os().any(|a| a == "service-run") {
        Some("Windows 서비스".to_owned())
    } else {
        aaa_common::process::parent(std::process::id())
            .and_then(aaa_common::process::image)
            .filter(|p| aaa_common::process::is_launcher(p))
            .map(|_| "launchd".to_owned())
    };
    aaa_proto::MachineInfo {
        at_ms: aaa_common::util::now_ms(),
        os_version,
        cpu_model,
        claude,
        codex,
        installation,
        work_root: root.to_string_lossy().into_owned(),
    }
}

impl Agent {
    /// 주기 점검. 결과는 기억해 두었다가 host가 다시 붙을 때도 보낸다.
    pub fn start_health(self: &Arc<Self>, cfg: HealthSection) {
        let stats = Arc::new(Mutex::new(Stats::default()));
        let every = Duration::from_secs(cfg.every_s);
        let (me, s) = (self.clone(), stats.clone());
        tokio::spawn(async move {
            let mut info = None;
            let mut observed = None;
            loop {
                let threads = me.threads_cfg.lock().unwrap().clone();
                let mut h = collect(&cfg, &threads.work_root, threads.work_mount.as_deref()).await;
                if observed.is_none_or(|at: Instant| at.elapsed() >= Duration::from_secs(600)) {
                    info = Some(machine_info(&threads.work_root).await);
                    observed = Some(Instant::now());
                }
                h.info = info.clone();
                s.lock().unwrap().fill(&mut h);
                *me.health.lock().unwrap() = Some(h.clone());
                me.send_health(h).await;
                tokio::time::sleep(every).await;
            }
        });
        // 점검이 표본보다 드물면 사이에 CPU·메모리만 다시 잰다(마지막 점검의 이상·toolchain은 그대로)
        if every > STATS_EVERY {
            let me = self.clone();
            tokio::spawn(async move {
                let mut tick = tokio::time::interval_at(tokio::time::Instant::now() + STATS_EVERY, STATS_EVERY);
                // 잠들었다 깬 머신에서 밀린 표본을 몰아 재지 않는다
                tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                loop {
                    tick.tick().await;
                    let h = me.health.lock().unwrap().as_mut().map(|h| {
                        stats.lock().unwrap().fill(h);
                        h.clone()
                    });
                    if let Some(h) = h {
                        me.send_health(h).await;
                    }
                }
            });
        }
    }

    pub(crate) async fn send_health(&self, h: MachineHealth) {
        if self.host_health.load(std::sync::atomic::Ordering::Relaxed) {
            self.send_host(&LinkMsg::Event(AgentEvent::Health(h))).await;
        }
    }
}

// 시험 명령이 /bin/sh라 unix에서만 돈다
#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn cpu_pct_is_the_busy_share_between_samples() {
        let t = |idle, total| CpuTicks { idle, total };
        assert_eq!(cpu_pct(t(100, 400), t(175, 500)), Some(25));
        assert_eq!(cpu_pct(t(0, 0), t(1, 3)), Some(67));
        assert_eq!(cpu_pct(t(100, 400), t(100, 400)), None, "같은 표본");
        assert_eq!(cpu_pct(t(100, 400), t(50, 300)), None, "되감긴 counter");
        assert_eq!(cpu_pct(t(0, 0), t(5, 3)), None, "쉼이 전체보다 크다");
    }

    #[test]
    fn cpu_keeps_the_last_value_inside_the_window() {
        let t0 = Instant::now();
        let mut s = Stats::default();
        assert_eq!(s.cpu_at(t0, || Some(CpuTicks { idle: 0, total: 0 })), None, "첫 표본은 구간이 없다");
        // 점검과 표본이 잇달아 오면 다시 읽지 않는다
        assert_eq!(s.cpu_at(t0 + Duration::from_millis(500), || panic!("창 안에서 읽었다")), None);
        assert_eq!(s.cpu_at(t0 + MIN_CPU_WINDOW, || Some(CpuTicks { idle: 30, total: 100 })), Some(70));
        assert_eq!(s.cpu_at(t0 + MIN_CPU_WINDOW + Duration::from_millis(10), || panic!("창 안에서 읽었다")), Some(70));
        // 읽기에 실패하면 앞 표본·값을 그대로 둔다
        assert_eq!(s.cpu_at(t0 + MIN_CPU_WINDOW * 2, || None), Some(70));
        assert_eq!(s.cpu_at(t0 + MIN_CPU_WINDOW * 3, || Some(CpuTicks { idle: 40, total: 200 })), Some(90));
    }

    #[test]
    fn linux_proc_files() {
        let stat = "cpu  100 5 50 800 40 3 2 0 7 0\ncpu0 1 2 3 4 5 6 7 8 9 0\n";
        assert_eq!(parse_proc_stat(stat), Some(CpuTicks { idle: 840, total: 1000 }));
        assert_eq!(parse_proc_stat("intr 1 2\n"), None);
        let mem = "MemTotal:       16000000 kB\nMemFree:          500000 kB\nMemAvailable:    4000000 kB\n";
        assert_eq!(parse_meminfo(mem), Some((12_000_000 * 1024, 16_000_000 * 1024)));
        assert_eq!(parse_meminfo("MemTotal: 10 kB\n"), None, "MemAvailable이 없는 옛 kernel");
    }

    /// 이 OS의 실제 값: 누적 시간은 줄지 않고 사용 메모리는 전체를 넘지 않는다
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn this_machine_reports_cpu_and_memory() {
        let (a, b) = (cpu_ticks().unwrap(), cpu_ticks().unwrap());
        assert!(b.total >= a.total && b.idle >= a.idle && a.idle <= a.total, "{a:?} {b:?}");
        let (used, total) = memory().unwrap();
        assert!(used > 0 && used <= total, "{used}/{total}");
        let mut h = MachineHealth {
            info: None,
            at_ms: 0,
            disk_free_bytes: None,
            disk_total_bytes: None,
            load_x100: None,
            cpus: 1,
            cpu_pct: None,
            mem_used_bytes: None,
            mem_total_bytes: None,
            on_ac: None,
            toolchain: vec![],
            problems: vec![],
            warnings: vec![],
        };
        Stats::default().fill(&mut h);
        assert!(h.at_ms > 0 && h.mem_total_bytes == Some(total) && h.cpu_pct.is_none(), "{h:?}");
    }

    #[tokio::test]
    async fn display_probe_failure_is_unknown_and_old_health_has_no_info() {
        assert_eq!(info_value("/bin/sh", &["-c", "exit 3"]).await, None);
        assert_eq!(info_value("/bin/sh", &["-c", "printf 'tool 1\\nsecond'"]).await.as_deref(), Some("tool 1"));
        let cfg = HealthSection { min_free_gb: 0, ..HealthSection::default() };
        let h = collect(&cfg, Path::new("/tmp"), None).await;
        let mut wire = serde_json::to_value(&h).unwrap();
        wire.as_object_mut().unwrap().remove("info");
        assert!(serde_json::from_value::<MachineHealth>(wire).unwrap().info.is_none());
    }

    #[tokio::test]
    async fn problems_from_disk_and_toolchain() {
        let mut cfg = HealthSection { every_s: 60, min_free_gb: 0, toolchain: Default::default() };
        cfg.toolchain.insert("ok".into(), vec!["/bin/sh".into(), "-c".into(), "echo 'tool 1.2'".into()]);
        let h = collect(&cfg, Path::new("/nonexistent-aaa/work"), None).await;
        assert!(h.problems.is_empty(), "{:?}", h.problems);
        assert_eq!(h.toolchain[0].version, "tool 1.2");
        assert!(h.disk_free_bytes.is_some() && h.load_x100.is_some());
        cfg.min_free_gb = 1 << 30; // 1EB
        cfg.toolchain.insert("bad".into(), vec!["/bin/sh".into(), "-c".into(), "echo nope >&2; exit 3".into()]);
        let h = collect(&cfg, Path::new("/"), None).await;
        let all = h.problems.iter().map(|p| format!("{}={}", p.code, p.text)).collect::<Vec<_>>().join(" | ");
        assert!(all.contains("toolchain:bad=toolchain bad 실패") && all.contains("nope"), "{all}");
        assert!(all.contains("disk=디스크 여유"), "{all}");
    }
}

#[cfg(all(test, windows))]
mod windows_info_tests {
    #[tokio::test]
    async fn windows_system_information_is_display_only() {
        let info = super::machine_info(std::path::Path::new("C:\\aaa-test")).await;
        assert!(info.os_version.as_deref().is_some_and(|v| v.contains("Windows")));
        assert!(info.cpu_model.as_deref().is_some_and(|v| !v.is_empty()));
        assert!(info.at_ms > 0);
    }
}

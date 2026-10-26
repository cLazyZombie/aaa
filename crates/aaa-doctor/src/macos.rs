//! macOS 계보·키체인·첫 실행 측정. 비공개 API는 dlsym으로 찾고 없으면 None으로 둔다.

use aaa_common::child_process::StdCommandExt as _;
use std::ffi::{CString, c_int, c_void};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use anyhow::{Context, Result, bail};

use super::{Keychain, Lineage, Sample};

/// 빌드 때 지금 소스로 만든 작은 Mach-O(probe/target.c)
static PROBE_TARGET: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/probe-target"));
const SALT_MARKER: &[u8] = b"AAA_PROBE_SALT_MARKER_0123456789AB";

#[link(name = "Security", kind = "framework")]
unsafe extern "C" {
    fn SessionGetInfo(session: u32, session_id: *mut u32, attributes: *mut u32) -> i32;
    fn SecKeychainCopyDefault(keychain: *mut *mut c_void) -> i32;
    fn SecKeychainGetStatus(keychain: *mut c_void, status: *mut u32) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(cf: *const c_void);
}

unsafe extern "C" {
    fn proc_pidpath(pid: c_int, buf: *mut c_void, size: u32) -> c_int;
    fn qos_class_self() -> u32;
}

const CALLER_SECURITY_SESSION: u32 = 0xFFFF_FFFF;
const SESSION_HAS_GRAPHIC_ACCESS: u32 = 0x0010;
const SESSION_HAS_TTY: u32 = 0x0020;
const SESSION_IS_REMOTE: u32 = 0x1000;
const K_SEC_UNLOCK_STATE_STATUS: u32 = 1;

fn dlsym(name: &str) -> *mut c_void {
    let c = CString::new(name).unwrap();
    unsafe { libc::dlsym(libc::RTLD_DEFAULT, c.as_ptr()) }
}

pub fn pid_path(pid: i32) -> Option<String> {
    let mut buf = vec![0u8; 4096];
    let n = unsafe { proc_pidpath(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
    if n <= 0 {
        return None;
    }
    buf.truncate(n as usize);
    String::from_utf8(buf)
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
        .ok()
}

/// 다른 프로세스(같은 사용자)의 책임 프로세스
pub fn responsible_for(pid: i32) -> Option<i32> {
    let f = dlsym("responsibility_get_pid_responsible_for_pid");
    if f.is_null() {
        return None;
    }
    let f: extern "C" fn(c_int) -> c_int = unsafe { std::mem::transmute(f) };
    let r = f(pid);
    (r > 0).then_some(r)
}

pub fn fill_lineage(l: &mut Lineage) {
    l.responsible_pid = responsible_for(l.pid as i32);
    l.responsible_path = l.responsible_pid.and_then(pid_path);
    let (mut sid, mut attrs) = (0u32, 0u32);
    if unsafe { SessionGetInfo(CALLER_SECURITY_SESSION, &mut sid, &mut attrs) } == 0 {
        l.session_id = Some(sid);
        l.session_attrs = Some(attrs);
        l.session_has_gui = Some(attrs & SESSION_HAS_GRAPHIC_ACCESS != 0);
        l.session_has_tty = Some(attrs & SESSION_HAS_TTY != 0);
        l.session_is_remote = Some(attrs & SESSION_IS_REMOTE != 0);
    }
    l.tcc_devtool_preflight_raw = tcc_preflight();
}

fn tcc_preflight() -> Option<i32> {
    use core_foundation::base::TCFType;
    use core_foundation::string::CFString;
    let path = CString::new("/System/Library/PrivateFrameworks/TCC.framework/TCC").unwrap();
    let h = unsafe { libc::dlopen(path.as_ptr(), libc::RTLD_LAZY) };
    if h.is_null() {
        return None;
    }
    let name = CString::new("TCCAccessPreflight").unwrap();
    let f = unsafe { libc::dlsym(h, name.as_ptr()) };
    if f.is_null() {
        return None;
    }
    let f: extern "C" fn(*const c_void, *const c_void) -> c_int = unsafe { std::mem::transmute(f) };
    let service = CFString::new("kTCCServiceDeveloperTool");
    Some(f(service.as_concrete_TypeRef().cast(), std::ptr::null()))
}

pub fn qos_name() -> String {
    match unsafe { qos_class_self() } {
        0x21 => "user_interactive".into(),
        0x19 => "user_initiated".into(),
        0x15 => "default".into(),
        0x11 => "utility".into(),
        0x09 => "background".into(),
        0x00 => "unspecified".into(),
        v => format!("0x{v:x}"),
    }
}

pub fn keychain(service: Option<&str>) -> Keychain {
    let mut k = Keychain { item: "skipped".into(), item_service: service.map(String::from), ..Default::default() };
    unsafe {
        let mut kc: *mut c_void = std::ptr::null_mut();
        let e = SecKeychainCopyDefault(&mut kc);
        if e == 0 && !kc.is_null() {
            let mut st = 0u32;
            let e2 = SecKeychainGetStatus(kc, &mut st);
            CFRelease(kc);
            if e2 == 0 {
                k.default_unlocked = Some(st & K_SEC_UNLOCK_STATE_STATUS != 0);
            } else {
                k.status_error = Some(e2);
            }
        } else {
            k.status_error = Some(e);
        }
    }
    if let Some(service) = service {
        k.item = read_item(service);
    }
    k
}

/// 실제 generic password 항목을 읽는다. 인증 UI는 띄우지 않는다(띄우면 무인 실행이 멈춘다).
fn read_item(service: &str) -> String {
    use security_framework::item::{ItemClass, ItemSearchOptions, Limit};
    let service = service.to_string();
    // UI를 건너뛰게 해도 OS가 대화창을 띄우는 경우를 대비해 따로 기다린다.
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let r = ItemSearchOptions::new()
            .class(ItemClass::generic_password())
            .service(&service)
            .load_data(true)
            .skip_authenticated_items(true)
            .limit(Limit::Max(1))
            .search();
        aaa_common::diagnostic::ignored(
            tx.send(match r {
                Ok(v) if !v.is_empty() => "ok".to_string(),
                Ok(_) => "not_found".to_string(),
                Err(e) => match e.code() {
                    -25300 => "not_found".into(),
                    -25308 => "interaction_not_allowed".into(),
                    -25293 => "auth_failed".into(),
                    c => format!("error({c})"),
                },
            }),
            "수신자가 끝난 뒤에도 현재 작업은 마친다",
        );
    });
    rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap_or_else(|_| "timeout(대화창 대기 의심)".into())
}

pub fn codesign_summary(exe: &str) -> Vec<String> {
    let out = Command::new("/usr/bin/codesign").args(["-dv", "--verbose=2", exe]).capture_output();
    let Ok(out) =
        out.inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다"))
    else {
        return vec!["codesign 실행 실패".into()];
    };
    let text = String::from_utf8_lossy(&out.stderr);
    text.lines()
        .filter(|l| {
            ["Identifier=", "Authority=", "TeamIdentifier=", "Signature=", "CDHash=", "Format="]
                .iter()
                .any(|k| l.starts_with(k))
        })
        .map(String::from)
        .collect()
}

/// 새 실행 파일 n개를 만들어 각각 첫 실행과 바로 이은 두 번째 실행 시간을 잰다.
pub fn first_exec_samples(n: usize) -> Result<Vec<Sample>> {
    let dir = probe_dir()?;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let exe = dir.join(format!("t{i}"));
        make_fresh(&exe).with_context(|| format!("표본 {i} 생성"))?;
        let mut pair = [0f64; 2];
        for slot in &mut pair {
            let start = Instant::now();
            let st =
                Command::new(&exe).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status_guarded()?;
            *slot = start.elapsed().as_secs_f64() * 1000.0;
            if !st.success() {
                bail!("표본 {i} 실행 실패: {st}");
            }
        }
        out.push(Sample { first_ms: pair[0], warm_ms: pair[1], extra_ms: pair[0] - pair[1] });
    }
    aaa_common::diagnostic::ignored(std::fs::remove_dir_all(&dir), "정리 실패가 본 작업의 결과를 덮지 않게 한다");
    Ok(out)
}

fn probe_dir() -> Result<PathBuf> {
    let base = aaa_config::paths::state_dir().join("probe");
    let dir = base.join(format!("run-{}-{}", std::process::id(), aaa_common::util::now_ms()));
    aaa_common::util::ensure_private_dir(&dir)?;
    Ok(dir)
}

fn make_fresh(path: &Path) -> Result<()> {
    if PROBE_TARGET.is_empty() {
        bail!("probe target이 빌드되지 않았다");
    }
    let mut bytes = PROBE_TARGET.to_vec();
    let at = bytes.windows(SALT_MARKER.len()).position(|w| w == SALT_MARKER).context("salt 표식을 찾지 못했다")?;
    let salt: String =
        (0..SALT_MARKER.len()).map(|_| char::from(b"0123456789abcdef"[rand::random_range(0..16)])).collect();
    bytes[at..at + SALT_MARKER.len()].copy_from_slice(salt.as_bytes());
    std::fs::write(path, &bytes)?;
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    }
    // 내용이 바뀌어 링커 adhoc 서명이 깨졌으므로 다시 adhoc 서명한다(arm64는 서명 없이 실행 불가).
    let st = Command::new("/usr/bin/codesign")
        .args(["-f", "-s", "-"])
        .arg(path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status_guarded()?;
    if !st.success() {
        bail!("codesign 실패: {st}");
    }
    Ok(())
}

/// pane 밖에서 agent 일을 하는 장수 daemon의 계보(E6). ps로 찾고 책임 프로세스를 붙인다.
pub fn daemons(patterns: &[String]) -> Vec<(i32, String, Option<i32>, Option<String>)> {
    let out = Command::new("/bin/ps").args(["-axo", "pid=,command="]).capture_output();
    let Ok(out) =
        out.inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다"))
    else {
        return vec![];
    };
    let me = std::process::id() as i32;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let l = l.trim_start();
            let (pid, cmd) = l.split_once(' ')?;
            let pid: i32 = pid
                .parse()
                .inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                })
                .ok()?;
            (pid != me && patterns.iter().any(|p| cmd.contains(p.as_str()))).then(|| {
                let r = responsible_for(pid);
                (pid, cmd.to_string(), r, r.and_then(pid_path))
            })
        })
        .collect()
}

/// App Nap·자동 종료를 막는 활동을 프로세스 수명 동안 잡는다(agent backend 시작 때).
pub fn hold_activity(reason: &str) -> bool {
    use objc2_foundation::{NSActivityOptions, NSProcessInfo, NSString};
    let info = NSProcessInfo::processInfo();
    let opts = NSActivityOptions::UserInitiatedAllowingIdleSystemSleep | NSActivityOptions::LatencyCritical;
    let token = info.beginActivityWithOptions_reason(opts, &NSString::from_str(reason));
    // 끝내지 않는다: 프로세스가 끝날 때까지 유지
    std::mem::forget(token);
    true
}

/// 음성 대조: 자기 자신을 `responsibility_spawnattrs_setdisclaim(true)`로 다시 띄운다.
/// 자식은 미등록 실행 파일이 자기 책임자가 되므로 첫 실행이 느려야 한다(실험 기록 disclaim1 +224ms).
/// 측정 도구가 느린 계보를 실제로 잡아내는지 확인하는 용도다.
pub fn rerun_disclaimed(args: &[String]) -> Result<i32> {
    let f = dlsym("responsibility_spawnattrs_setdisclaim");
    if f.is_null() {
        bail!("responsibility_spawnattrs_setdisclaim을 찾지 못했다");
    }
    let f: extern "C" fn(*mut libc::posix_spawnattr_t, bool) -> c_int = unsafe { std::mem::transmute(f) };
    let exe = CString::new(std::env::current_exe()?.to_string_lossy().as_bytes())?;
    let cargs: Vec<CString> = args.iter().map(|a| CString::new(a.as_bytes()).unwrap()).collect();
    let mut argv: Vec<*mut libc::c_char> = cargs.iter().map(|c| c.as_ptr() as *mut _).collect();
    argv.push(std::ptr::null_mut());
    unsafe extern "C" {
        static environ: *const *mut libc::c_char;
    }
    unsafe {
        let mut attr: libc::posix_spawnattr_t = std::mem::zeroed();
        libc::posix_spawnattr_init(&mut attr);
        let e = f(&mut attr, true);
        if e != 0 {
            bail!("disclaim 설정 실패: {e}");
        }
        let mut pid: libc::pid_t = 0;
        let e = aaa_common::child_process::with_spawn_lock(|| {
            libc::posix_spawn(&mut pid, exe.as_ptr(), std::ptr::null(), &attr, argv.as_ptr(), environ)
        });
        libc::posix_spawnattr_destroy(&mut attr);
        if e != 0 {
            bail!("spawn 실패: {}", std::io::Error::from_raw_os_error(e));
        }
        let mut status = 0;
        if libc::waitpid(pid, &mut status, 0) != pid {
            bail!("waitpid 실패");
        }
        Ok(if libc::WIFEXITED(status) { libc::WEXITSTATUS(status) } else { 128 })
    }
}

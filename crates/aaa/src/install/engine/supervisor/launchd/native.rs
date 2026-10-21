//! launchctl과 macOS 커널 관찰만 사용한다. 임의 executor/쉘/권한 승격은 없다.
use super::*;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::MetadataExt;
use std::time::{Duration, Instant};

pub(super) struct Launchd;
fn call(args: &[String]) -> Result<(bool, String)> {
    let args: Vec<_> = args.iter().map(String::as_str).collect();
    crate::update::run_limited(Path::new("/bin/launchctl"), &args)
        .map_err(|_| failure("service_manager_unavailable", 3))
}
fn domain() -> String {
    format!("gui/{}", unsafe { libc::geteuid() })
}
fn target(spec: &Spec) -> String {
    format!("{}/{}", domain(), label(spec))
}
pub(super) fn available() -> Result<()> {
    if !call(&["print".into(), domain()])?.0 {
        return Err(failure("service_gui_session_required", 3));
    }
    Ok(())
}
fn loaded(spec: &Spec) -> Result<Option<u32>> {
    let (ok, text) = call(&["print".into(), target(spec)])?;
    if !ok {
        // 다른 오류/권한 부족을 미등록으로 취급해 덮어쓰지 않는다.
        if text.contains("Could not find service") && text.contains(&label(spec)) {
            return Ok(None);
        }
        return Err(failure("service_manager_unavailable", 3));
    }
    let field = |key: &str| text.lines().map(str::trim).find_map(|line| line.strip_prefix(key));
    let arguments: Vec<_> = text
        .lines()
        .map(str::trim)
        .skip_while(|line| *line != "arguments = {")
        .skip(1)
        .take_while(|line| *line != "}")
        .collect();
    if field("path = ") != path(spec).to_str()
        || field("program = ") != spec.launchd.as_ref().and_then(|c| c.launcher.to_str())
        || arguments != argv(spec).iter().map(String::as_str).collect::<Vec<_>>()
    {
        return Err(Fault::conflict());
    }
    let pid = field("pid = ").map(str::parse::<u32>).transpose().map_err(|_| Fault::conflict())?.unwrap_or(0);
    Ok(Some(pid))
}
pub(super) fn identity(pid: u32) -> Result<(libc::proc_bsdinfo, PathBuf)> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    // SAFETY: 고정 ABI의 쓰기 대상 크기를 넘긴다. PID는 읽기 대상뿐이다.
    let count = unsafe {
        libc::proc_pidinfo(
            pid as i32,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            std::mem::size_of_val(&info) as i32,
        )
    };
    if count as usize != std::mem::size_of_val(&info) || info.pbi_pid != pid || info.pbi_status == 5 {
        return Err(failure("service_process_unverified", 3));
    }
    let image = aaa_common::process::image(pid).ok_or_else(|| failure("service_process_unverified", 3))?;
    Ok((info, image))
}
fn created(info: &libc::proc_bsdinfo) -> u64 {
    info.pbi_start_tvsec.saturating_mul(1_000_000).saturating_add(info.pbi_start_tvusec)
}
pub(super) fn process(spec: &Spec, pid: u32, launcher: u32) -> Result<Process> {
    let (info, image) = identity(pid)?;
    let (parent, parent_image) = identity(launcher)?;
    let context = spec.launchd.as_ref().ok_or_else(Fault::conflict)?;
    if info.pbi_uid != unsafe { libc::geteuid() }
        || parent.pbi_uid != info.pbi_uid
        || info.pbi_ppid != launcher
        || parent.pbi_ppid != 1
        || image.canonicalize().map_err(Fault::io)? != spec.executable
        || parent_image.canonicalize().map_err(Fault::io)? != context.launcher
        || hash(&std::fs::read(&image).map_err(Fault::io)?) != spec.binary_sha256
        || hash(&std::fs::read(&parent_image).map_err(Fault::io)?) != context.launcher_sha256
    {
        return Err(failure("service_process_unverified", 3));
    }
    Ok(Process { pid, created: created(&info), elevated: info.pbi_uid == 0 })
}
pub(super) fn own_process(spec: &Spec) -> Result<(u32, Process)> {
    let pid = std::process::id();
    let (info, _) = identity(pid)?;
    let process = process(spec, pid, info.pbi_ppid)?;
    if loaded(spec)? != Some(info.pbi_ppid) {
        return Err(failure("service_controller_required", 3));
    }
    let session = unsafe { libc::getsid(pid as i32) };
    if session < 0 {
        return Err(failure("service_process_unverified", 3));
    }
    Ok((session as u32, process))
}
pub(super) fn verify_launcher(spec: &Spec) -> Result<()> {
    let context = spec.launchd.as_ref().ok_or_else(Fault::conflict)?;
    context.guard()?;
    let m = context.launcher.symlink_metadata().map_err(Fault::io)?;
    if !m.is_file()
        || m.uid() != unsafe { libc::geteuid() }
        || m.nlink() != 1
        || m.mode() & 0o022 != 0
        || m.mode() & 0o111 == 0
    {
        return Err(failure("service_launcher_unverified", 3));
    }
    let bundle = context.launcher.parent().and_then(Path::parent).and_then(Path::parent);
    let signed = bundle
        .filter(|root| {
            root.extension().is_some_and(|ext| ext == "app")
                && root.join("Contents/MacOS/aaa-launcher") == context.launcher
        })
        .unwrap_or(&context.launcher);
    let (ok, _) = crate::update::run_limited(
        Path::new("/usr/bin/codesign"),
        &["--verify", "--strict", &signed.to_string_lossy()],
    )
    .map_err(|_| failure("service_launcher_unverified", 3))?;
    if !ok {
        return Err(failure("service_launcher_unverified", 3));
    }
    Ok(())
}
fn proof(spec: &Spec) -> Result<Ready> {
    let bytes = match private::read(&spec.directory().join("ready.json"), 128 * 1024) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(failure("service_ready_pending", 3)),
        Err(_) => return Err(failure("service_ready_unverified", 3)),
    };
    serde_json::from_slice(&bytes).map_err(|_| Fault::conflict())
}
fn discard_stopped_proof(spec: &Spec) -> Result<()> {
    let ready = match proof(spec) {
        Ok(ready) => ready,
        Err(fault) if fault.code == "service_ready_pending" => return Ok(()),
        Err(fault) => return Err(fault),
    };
    if ready.schema != 1
        || ready.context_sha256 != spec.sha()
        || ready.account_sha256 != hash(spec.account.as_bytes())
        || ready.profile_sha256 != hash(spec.profile.to_string_lossy().as_bytes())
        || ready.binary_sha256 != spec.binary_sha256
    {
        return Err(Fault::conflict());
    }
    if let Ok((info, image)) = identity(ready.pid)
        && created(&info) == ready.process_created
    {
        if loaded(spec)? == Some(info.pbi_ppid) {
            return Ok(());
        }
        // bootout을 마친 이전 소유 자식만 종료 사건까지 기다린다. 외부 process는 기다려 채택하지 않는다.
        // launcher 종료 뒤에는 자식이 reparent될 수 있다. 원래 proof의 생성 시각·UID·image를
        // 재확인해 그 자식의 종료만 기다리며 새로운 ready로 채택하지 않는다.
        if info.pbi_uid != unsafe { libc::geteuid() }
            || image.canonicalize().map_err(Fault::io)? != spec.executable
            || hash(&std::fs::read(&image).map_err(Fault::io)?) != spec.binary_sha256
        {
            return Err(failure("service_process_unverified", 3));
        }
        if let Some(mut event) = Events::process(ready.pid)? {
            event.wait(Duration::from_secs(30))?;
        }
    }
    let path = spec.directory().join("ready.json");
    match proof(spec) {
        Ok(current) if current.pid == ready.pid && current.context_sha256 == ready.context_sha256 => {
            std::fs::remove_file(&path).map_err(Fault::io)?;
            private::sync_parent(&path).map_err(Fault::io)
        }
        Err(fault) if fault.code == "service_ready_pending" => Ok(()),
        _ => Err(Fault::conflict()),
    }
}

impl Manager for Launchd {
    fn query(&mut self, spec: &Spec) -> Result<Option<Observation>> {
        let path = path(spec);
        let bytes = match private::read(&path, 128 * 1024) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if loaded(spec)?.is_some() {
                    return Err(Fault::conflict());
                }
                return Ok(None);
            }
            Err(_) => return Err(Fault::conflict()),
        };
        let desired = Configuration::desired(spec);
        if bytes != desired.command.as_bytes() {
            return Err(Fault::conflict());
        }
        let launcher = loaded(spec)?.unwrap_or(0);
        let child = proof(spec).ok().filter(|p| process(spec, p.pid, launcher).is_ok()).map(|p| p.pid);
        Ok(Some(Observation { configuration: desired, running: launcher > 0, pid: child.unwrap_or(launcher) }))
    }
    fn create(&mut self, spec: &Spec, password: Option<&Password>) -> Result<()> {
        if password.is_some() {
            return Err(Fault::conflict());
        }
        install_plist(spec)
    }
    fn start(&mut self, spec: &Spec) -> Result<Observation> {
        let mut events = Events::directory(&spec.directory())?;
        discard_stopped_proof(spec)?;
        if loaded(spec)?.is_none() {
            let (ok, _) = call(&["bootstrap".into(), domain(), path(spec).to_string_lossy().into_owned()])?;
            if !ok {
                return Err(failure("service_bootstrap_failed", 3));
            }
        }
        let (ok, _) = call(&["kickstart".into(), target(spec)])?;
        if !ok {
            return Err(failure("service_start_failed", 3));
        }
        let deadline = Instant::now() + Duration::from_secs(u64::from(spec.wait_timeout_s));
        loop {
            match proof(spec) {
                Ok(ready) => {
                    if ready.context_sha256 != spec.sha() {
                        return Err(failure("service_ready_unverified", 3));
                    }
                    let observed = self.query(spec)?.ok_or_else(Fault::conflict)?;
                    if ready.connected && verify_ready(spec, self, &observed).is_ok() {
                        return Ok(observed);
                    }
                    // 시작 중인 connected=false만 기다린다. 다른 프로세스의 proof는 채택하지 않는다.
                    if ready.connected {
                        return Err(failure("service_ready_unverified", 3));
                    }
                }
                Err(fault) if fault.code == "service_ready_pending" => {}
                Err(fault) => return Err(fault),
            }
            events.wait(deadline.saturating_duration_since(Instant::now()))?;
        }
    }
    fn stop(&mut self, spec: &Spec) -> Result<()> {
        let mut waits = Vec::new();
        let Some(pid) = loaded(spec)? else { return Ok(()) };
        // PID가 0인 등록도 bootout한다. 자식이 없다는 사실은 등록 해제와 다르다.
        if pid > 0 {
            if let Ok(ready) = proof(spec)
                && process(spec, ready.pid, pid).is_ok()
            {
                waits.extend(Events::process(ready.pid)?);
            }
            waits.extend(Events::process(pid)?);
        }
        if !call(&["bootout".into(), target(spec)])?.0 {
            return Err(failure("service_stop_failed", 3));
        }
        for mut event in waits {
            event.wait(Duration::from_secs(30))?;
        }
        Ok(())
    }
    fn remove(&mut self, spec: &Spec) -> Result<()> {
        let current = self.query(spec)?.ok_or_else(Fault::conflict)?;
        if current.configuration != Configuration::desired(spec) {
            return Err(Fault::conflict());
        }
        self.stop(spec)?;
        if private::read(&path(spec), 128 * 1024).map_err(Fault::io)? != plist(spec).as_bytes() {
            return Err(Fault::conflict());
        }
        std::fs::remove_file(path(spec)).map_err(Fault::io)?;
        private::sync_parent(&path(spec)).map_err(Fault::io)
    }
    fn credentials(&mut self, _: &Spec, _: &Password) -> Result<()> {
        Err(failure("service_account_auth_unavailable", 3))
    }
    fn process(&mut self, spec: &Spec, pid: u32) -> Result<Option<Process>> {
        let Some(launcher) = loaded(spec)? else {
            return Ok(None);
        };
        let ready = proof(spec)?;
        if ready.pid != pid || unsafe { libc::getsid(pid as i32) } != ready.session as i32 {
            return Ok(None);
        }
        match process(spec, pid, launcher) {
            Ok(process) => Ok(Some(process)),
            Err(e) if e.exit == 3 => Ok(None),
            Err(e) => Err(e),
        }
    }
}

/// create-new와 같은 원자 등록. 외부/다른 instance의 plist를 대체하지 않는다.
pub(super) fn install_plist(spec: &Spec) -> Result<()> {
    use std::io::Write;
    use std::os::unix::ffi::OsStrExt;
    let dest = path(spec);
    let parent = transaction::real_directory(dest.parent().ok_or_else(Fault::conflict)?)?;
    std::fs::create_dir_all(&parent).map_err(Fault::io)?;
    let stage = parent.join(format!(".aaa-{}-{}.stage", spec.id, spec.role()));
    let bytes = plist(spec).into_bytes();
    match private::new_file(&stage) {
        Ok(mut file) => {
            file.write_all(&bytes).map_err(Fault::io)?;
            file.sync_all().map_err(Fault::io)?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            if private::read(&stage, 128 * 1024).map_err(Fault::io)? != bytes {
                return Err(Fault::conflict());
            }
        }
        Err(e) => return Err(Fault::io(e)),
    }
    let src = std::ffi::CString::new(stage.as_os_str().as_bytes()).map_err(|_| Fault::conflict())?;
    let dst = std::ffi::CString::new(dest.as_os_str().as_bytes()).map_err(|_| Fault::conflict())?;
    // SAFETY: 두 NUL 종료 경로가 살아 있으며 RENAME_EXCL로 기존 경로를 대체하지 않는다.
    if unsafe { libc::renameatx_np(libc::AT_FDCWD, src.as_ptr(), libc::AT_FDCWD, dst.as_ptr(), libc::RENAME_EXCL) } != 0
    {
        return Err(Fault::conflict());
    }
    private::sync_parent(&dest).map_err(Fault::io)
}

struct Events {
    queue: OwnedFd,
    _directory: Option<File>,
}
impl Events {
    fn register(fd: usize, filter: i16, flags: u16, fflags: u32, directory: Option<File>) -> std::io::Result<Self> {
        let raw = unsafe { libc::kqueue() };
        if raw < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let queue = unsafe { OwnedFd::from_raw_fd(raw) };
        let event = libc::kevent {
            ident: fd,
            filter,
            flags: libc::EV_ADD | libc::EV_CLEAR | flags,
            fflags,
            data: 0,
            udata: std::ptr::null_mut(),
        };
        if unsafe { libc::kevent(queue.as_raw_fd(), &event, 1, std::ptr::null_mut(), 0, std::ptr::null()) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Self { queue, _directory: directory })
    }
    fn directory(path: &Path) -> Result<Self> {
        let directory = File::open(path).map_err(Fault::io)?;
        Self::register(
            directory.as_raw_fd() as usize,
            libc::EVFILT_VNODE,
            0,
            libc::NOTE_WRITE | libc::NOTE_RENAME | libc::NOTE_DELETE,
            Some(directory),
        )
        .map_err(Fault::io)
    }
    fn process(pid: u32) -> Result<Option<Self>> {
        match Self::register(pid as usize, libc::EVFILT_PROC, libc::EV_ONESHOT, libc::NOTE_EXIT, None) {
            Ok(event) => Ok(Some(event)),
            // 신원 검증과 구독 사이에 이미 끝난 process는 종료 사건과 같다.
            Err(e) if e.raw_os_error() == Some(libc::ESRCH) => Ok(None),
            Err(_) => Err(failure("service_ready_unavailable", 3)),
        }
    }
    fn wait(&mut self, remaining: Duration) -> Result<()> {
        let mut result: libc::kevent = unsafe { std::mem::zeroed() };
        let deadline = Instant::now() + remaining;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let time = libc::timespec {
                tv_sec: remaining.as_secs().min(i64::MAX as u64) as _,
                tv_nsec: remaining.subsec_nanos() as _,
            };
            let count = unsafe { libc::kevent(self.queue.as_raw_fd(), std::ptr::null(), 0, &mut result, 1, &time) };
            if count > 0 && result.flags & libc::EV_ERROR == 0 {
                return Ok(());
            }
            if count < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(failure("service_ready_wait_expired", 3));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn owned_process_exit_is_an_event_even_if_it_ends_between_observation_and_subscription() {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let mut child = Command::new("/bin/sh")
            .args(["-c", "read answer"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id();
        let mut event = Events::process(pid).unwrap().expect("쓰기 전 stdin을 기다리는 소유 자식");
        child.stdin.take().unwrap().write_all(b"finish\n").unwrap();
        event.wait(Duration::from_secs(30)).unwrap();
        assert!(child.wait().unwrap().success());
        assert!(Events::process(pid).unwrap().is_none(), "이미 거둔 자식도 완료로 읽는다");
    }
}

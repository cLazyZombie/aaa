//! 고정 worker. 승인/EOF·현재 실행 SHA를 확인하며 구독자 연결은 쓰기 권한이 아니다.
use super::*;
use crate::install::output::{Progress, Reporter};
use aaa_common::{conn::FrameRx, ipc::LocalListener};
use aaa_proto::frame::{FrameKind, write_frame};
use authorization::Authority;
use execution::Execution;
use input::Input;
use std::process::{Child, Command, Stdio};
use std::time::Duration;
pub(super) mod private_display;
use private_display::Subscriber;

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Reply {
    Started { installation_id: String },
    Progress { progress: Progress },
    Private { request: private_display::Request },
    Result { result: Box<InstallResult> },
}
fn failed(code: &'static str, exit: u8) -> Fault {
    Fault { code, exit, cause: None }
}
fn executor_sha() -> Result<String> {
    let exe = std::env::current_exe().map_err(Fault::io)?;
    crate::update::sha256(&exe).map_err(|source_error| {
        aaa_common::diagnostic::context(source_error, failed("installation_executor_unverified", 4))
    })
}
fn socket(state: &Path, id: &str) -> Result<PathBuf> {
    #[cfg(unix)]
    {
        let key = hash(format!("{}:{id}", state.display()).as_bytes());
        Ok(PathBuf::from("/tmp")
            .canonicalize()
            .map_err(Fault::io)?
            .join(format!("aaa-i-{}-{}", unsafe { libc::getuid() }, &key[..20]))
            .join("b.sock"))
    }
    #[cfg(windows)]
    {
        Ok(state.join("install/authorization").join(format!("{id}.sock")))
    }
}
struct Bootstrap {
    path: PathBuf,
    listener: Option<LocalListener>,
}
impl Bootstrap {
    fn bind(path: PathBuf) -> Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::{FileTypeExt, MetadataExt};
            private::directory(path.parent().ok_or_else(Fault::conflict)?).map_err(Fault::io)?;
            match path.symlink_metadata() {
                Ok(metadata)
                    if metadata.file_type().is_socket()
                        && metadata.uid() == unsafe { libc::getuid() }
                        && metadata.mode() & 0o777 == 0o600
                        && metadata.nlink() == 1 => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                _ => return Err(Fault::conflict()),
            }
        }
        let mut this = Self { path, listener: None };
        this.listener =
            Some(LocalListener::bind(&this.path).map_err(|source_error| {
                aaa_common::diagnostic::context(source_error, failed("installation_busy", 4))
            })?);
        Ok(this)
    }
}
impl Drop for Bootstrap {
    fn drop(&mut self) {
        // 먼저 자신이 bind한 socket만 닫고 지운다. 다른 파일이 있는 디렉터리는 제거하지 않는다.
        drop(self.listener.take());
        #[cfg(unix)]
        if let Some(parent) = self.path.parent() {
            aaa_common::diagnostic::ignored(std::fs::remove_dir(parent), "정리 실패가 본 작업의 결과를 덮지 않게 한다");
        }
    }
}
fn detach(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // fork 이후 async-signal-safe setsid만 호출한다. SSH 제어 세션/stdio를 물려받지 않는다.
        unsafe {
            command.pre_exec(|| if libc::setsid() < 0 { Err(std::io::Error::last_os_error()) } else { Ok(()) });
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::System::Threading::{
            CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_PROCESS_GROUP, DETACHED_PROCESS,
        };
        // 부모 Job이 breakaway를 금지하면 spawn이 실패한다. 재시도/다른 flag로 우회하지 않는다.
        command.creation_flags(CREATE_BREAKAWAY_FROM_JOB | CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS);
    }
}
fn spawn(state: &Path, id: &str, sha: &str, resume: bool) -> Result<Child> {
    let mut command = Command::new(std::env::current_exe().map_err(Fault::io)?);
    command
        .arg("--state-dir")
        .arg(state)
        .args(["install-worker", "--id", id, "--executor-sha256", sha])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if resume {
        command.arg("--resume");
    }
    detach(&mut command);
    command.spawn().map_err(|error| {
        #[cfg(windows)]
        {
            use windows_sys::Win32::System::{JobObjects::*, Threading::*};
            let mut in_job = 0;
            let membership = unsafe { IsProcessInJob(GetCurrentProcess(), std::ptr::null_mut(), &mut in_job) };
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
            let queried = unsafe {
                QueryInformationJobObject(
                    std::ptr::null_mut(),
                    JobObjectExtendedLimitInformation,
                    (&mut limits as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                    std::mem::size_of_val(&limits) as u32,
                    std::ptr::null_mut(),
                )
            };
            // OS 오류·Job 제한 비트만 남긴다. 실행 경로·인자·환경·비밀 입력은 기록하지 않는다.
            eprintln!(
                "install worker spawn failed: os_error={:?}, in_job={:?}, job_limits={:?}",
                error.raw_os_error(),
                (membership != 0).then_some(in_job != 0),
                (queried != 0).then_some(limits.BasicLimitInformation.LimitFlags)
            );
        }
        #[cfg(not(windows))]
        tracing::info!(error = %aaa_common::diagnostic::Cause::capture(&error), "설치 worker 시작 실패");
        aaa_common::diagnostic::WithCause::with_cause(
            failed("installation_worker_unavailable", 3),
            aaa_common::diagnostic::Cause::capture(&error),
        )
    })
}

pub(super) async fn launch(
    state: &Path,
    input: Input,
    resume: bool,
    reporter: &mut Reporter,
    private: bool,
) -> Result<InstallResult> {
    let authority = Authority::open(state)?;
    let id = input.authorization.claims.installation_id.clone();
    let issued = authority.issued(&id)?;
    authority.validate(
        &input.authorization,
        &issued.plan,
        &input.authorization.claims.permissions,
        aaa_common::util::now_ms(),
    )?;
    if !resume {
        execution::ensure_not_started(state, &id)?;
    }
    let sha = executor_sha()?;
    if sha != input.authorization.claims.binary_sha256 {
        return Err(failed("installation_executor_unverified", 4));
    }
    let state = transaction::real_directory(state)?;
    let state = state.as_path();
    let bootstrap = Bootstrap::bind(socket(state, &id)?)?;
    let listener = bootstrap.listener.as_ref().ok_or_else(Fault::conflict)?;
    let mut child = spawn(state, &id, &sha, resume)?;
    let pid = child.id();
    let stdin = child.stdin.take().ok_or_else(|| failed("installation_worker_unavailable", 3))?;
    // 표준 thread는 CLI 종료를 붙잡지 않는다. Child handle을 버려도 worker를 종료하지 않는다.
    let completion = std::thread::spawn(move || child.wait());
    // send 뒤 stdin을 닫아야 EOF 뒤 실제 적용이 시작된다.
    input.send(stdin)?;
    drop(input);
    let connection = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let (stream, peer) = listener.accept_peer().await.map_err(Fault::io)?;
            if peer == Some(pid) {
                return Ok::<_, Fault>(stream);
            }
        }
    })
    .await
    .map_err(|source_error| {
        aaa_common::diagnostic::context(source_error, failed("installation_worker_unavailable", 3))
    })??;
    let (reader, mut writer) = tokio::io::split(connection);
    let mut rx = FrameRx::new(reader, 128 * 1024);
    let mut started = false;
    let result = loop {
        let reply = rx.control::<Reply>().await.map_err(|source_error| {
            aaa_common::diagnostic::context(source_error, failed("installation_worker_disconnected", 3))
        })?;
        match reply {
            Reply::Started { installation_id } if !started && installation_id == id => {
                execution::verify_started(state, &id, pid)?;
                started = true;
                reporter.started(&id).map_err(|source_error| {
                    aaa_common::diagnostic::context(source_error, failed("installation_progress_unavailable", 3))
                })?;
            }
            Reply::Progress { progress } if started => reporter.progress(progress).map_err(|source_error| {
                aaa_common::diagnostic::context(source_error, failed("installation_progress_unavailable", 3))
            })?,
            Reply::Private { request } if started => {
                let answer = private_display::answer(request, state, private).await;
                let bytes = serde_json::to_vec(&answer)
                    .map_err(|source_error| aaa_common::diagnostic::context(source_error, Fault::conflict()))?;
                write_frame(&mut writer, FrameKind::Control, &bytes, 4096).await.map_err(|source_error| {
                    aaa_common::diagnostic::context(source_error, failed("installation_private_display_lost", 3))
                })?;
            }
            Reply::Result { result } if started && result.installation_id.as_deref() == Some(id.as_str()) => {
                break *result;
            }
            Reply::Result { result } if !started && result.installation_id.is_none() && result.exit_code != 0 => {
                break *result;
            }
            _ => return Err(failed("installation_worker_unverified", 4)),
        }
    };
    let status = completion
        .join()
        .map_err(|source_error| {
            aaa_common::diagnostic::context(source_error, failed("installation_worker_unavailable", 3))
        })?
        .map_err(|source_error| {
            aaa_common::diagnostic::context(source_error, failed("installation_worker_unavailable", 3))
        })?;
    if status.code() != Some(i32::from(result.exit_code)) {
        return Err(failed("installation_worker_unverified", 4));
    }
    let mut result = result;
    crate::install::context(&issued.plan, &mut result);
    result.installation_id = Some(id.clone());
    result.plan_sha256 = Some(aaa_config::install::plan_sha256(&issued.plan));
    result.resume = (result.exit_code != 0).then_some(InstallResume { installation_id: id });
    Ok(result)
}

pub(super) fn dispatch(state: &Path, id: &str, sha: &str, resume: bool) -> std::process::ExitCode {
    let result = (|| {
        input::outside_session()?;
        if !id_ok(id) || !digest_ok(sha) || executor_sha()? != sha {
            return Err(failed("installation_executor_unverified", 4));
        }
        // 비밀/승인 입력을 전부 받고 EOF를 확인하기 전에는 socket/journal/nonce를 건드리지 않는다.
        let mut input = Input::framed(std::io::stdin().lock(), None)?;
        if input.authorization.claims.installation_id != id {
            return Err(Fault::conflict());
        }
        let authority = Authority::open(state)?;
        let issued = authority.issued(id)?;
        if input.password.is_some() {
            if issued.plan.choices.service != Some(InstallService::WindowsScm) {
                return Err(failed("installation_unexpected_secret_input", 2));
            }
            #[cfg(windows)]
            if !supervisor::requires_password(&issued.plan)? {
                return Err(failed("installation_unexpected_secret_input", 2));
            }
        }
        #[cfg(windows)]
        if issued.plan.choices.service == Some(InstallService::WindowsScm)
            && input.password.is_none()
            && supervisor::requires_password(&issued.plan)?
        {
            return Err(failed("service_account_auth_unavailable", 3));
        }
        if Path::new(&issued.plan.context.state_dir) != state {
            return Err(Fault::conflict());
        }
        authority.validate(
            &input.authorization,
            &issued.plan,
            &input.authorization.claims.permissions,
            aaa_common::util::now_ms(),
        )?;
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(Fault::io)?;
        runtime.block_on(async {
            let mut subscriber = Subscriber::connect(state, id)
                .await
                .inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                })
                .ok();
            let result = match Execution::begin(issued.plan, &input.authorization, aaa_common::util::now_ms(), resume) {
                Ok(mut execution) => {
                    if let Some(stream) = &mut subscriber {
                        let reply = Reply::Started { installation_id: id.to_owned() };
                        if !stream.send(&reply).await {
                            subscriber = None;
                        }
                    }
                    progress(&mut subscriber, Progress::StageRunning).await;
                    let mut result = match execution.files() {
                        Ok(result) => result,
                        Err(e) => *e.public(),
                    };
                    if result.exit_code == 0 {
                        progress(
                            &mut subscriber,
                            if result.status == InstallStatus::Unchanged {
                                Progress::FilesUnchanged
                            } else {
                                Progress::FilesConfigured
                            },
                        )
                        .await;
                    }
                    let issued_plan = authority.issued(id)?.plan;
                    let mut enrollment_changed = false;
                    if result.exit_code == 0 {
                        if !scope::enrollment(&issued_plan).is_empty() {
                            progress(&mut subscriber, Progress::EnrollRunning).await;
                        }
                        match execution.enroll(&input.authorization, &mut subscriber).await {
                            Ok(changed) => enrollment_changed = changed,
                            Err(fault) => result = stage_failure(fault, "enrollment"),
                        }
                    }
                    if result.exit_code == 0 && !scope::enrollment(&issued_plan).is_empty() {
                        progress(&mut subscriber, Progress::EnrollCompleted).await;
                    }
                    let mut path_changed = false;
                    if result.exit_code == 0 {
                        match execution.user_path() {
                            Ok(changed) => path_changed = changed,
                            Err(fault) => result = stage_failure(fault, "path"),
                        }
                    }
                    let mut service_changed = false;
                    if result.exit_code == 0 {
                        if issued_plan.choices.service != Some(InstallService::None) {
                            progress(&mut subscriber, Progress::ServiceRunning).await;
                        }
                        match execution.services(input.password.as_ref()) {
                            Ok(changed) => service_changed = changed,
                            Err(fault) => {
                                result = *fault.public();
                                if fault.exit == 3 {
                                    result.status = InstallStatus::NeedsUser;
                                    result.error = None;
                                    result.needs_user.clear();
                                    result.needs_user.push(InstallNeedsUser {
                                        code: fault.code.into(),
                                        target: "service".into(),
                                        message: "이 계정의 OS 권한·보호 암호 입력을 준비한 뒤 같은 ID로 재개하세요"
                                            .into(),
                                    });
                                }
                            }
                        }
                        if result.exit_code == 0 && issued_plan.choices.service != Some(InstallService::None) {
                            progress(&mut subscriber, Progress::ServiceReady).await;
                        }
                    }
                    drop(input.password.take());
                    let mut publication_changed = false;
                    if result.exit_code == 0 && issued_plan.choices.tailscale_serve == Some(InstallServe::On) {
                        progress(&mut subscriber, Progress::WebRunning).await;
                        match execution.publish().await {
                            Ok(changed) => {
                                publication_changed = changed;
                                progress(&mut subscriber, Progress::WebCompleted).await;
                            }
                            Err(fault) => result = stage_failure(fault, "web"),
                        }
                    }
                    let mut pairing_changed = false;
                    if result.exit_code == 0 && issued_plan.choices.pairing == Some(InstallPairing::Controller) {
                        progress(&mut subscriber, Progress::PairingRunning).await;
                        match execution.pairing(&mut subscriber).await {
                            Ok(changed) => {
                                pairing_changed = changed;
                                progress(&mut subscriber, Progress::PairingCompleted).await;
                            }
                            Err(fault) => result = stage_failure(fault, "pairing"),
                        }
                    }
                    if result.exit_code == 0 {
                        let unchanged = result.status == InstallStatus::Unchanged
                            && !service_changed
                            && !enrollment_changed
                            && !pairing_changed
                            && !publication_changed
                            && !path_changed;
                        result = match execution.verify().await {
                            Ok(result) => result,
                            Err(fault) => stage_failure(fault, "verify"),
                        };
                        if result.exit_code == 0 {
                            if unchanged {
                                result.status = InstallStatus::Unchanged;
                                for c in &mut result.changes {
                                    c.state = InstallChangeState::Unchanged;
                                }
                            }
                            readiness::final_status(&issued_plan, &mut result);
                            progress(&mut subscriber, Progress::VerifyCompleted).await;
                        }
                    }
                    crate::install::context(&issued_plan, &mut result);
                    result.installation_id = Some(id.to_owned());
                    result.plan_sha256 = Some(input.authorization.claims.plan_sha256.clone());
                    result.resume = (result.exit_code != 0).then(|| InstallResume { installation_id: id.to_owned() });
                    execution.finish(&result)?;
                    result
                }
                Err(fault) => *fault.public(),
            };
            if let Some(stream) = &mut subscriber {
                stream.send(&Reply::Result { result: Box::new(result.clone()) }).await;
            }
            Ok(result.exit_code)
        })
    })();
    std::process::ExitCode::from(result.unwrap_or_else(|fault| fault.exit))
}

fn stage_failure(fault: Fault, target: &str) -> InstallResult {
    let mut result = *fault.public();
    if fault.exit == 3 {
        result.status = InstallStatus::NeedsUser;
        result.error = None;
        result.needs_user.clear();
        result.needs_user.push(InstallNeedsUser {
            code: fault.code.into(),
            target: target.into(),
            message: crate::install::need_action(fault.code)
                .unwrap_or("보호된 승인·입력과 실제 상태를 확인한 뒤 같은 ID로 재개하세요")
                .into(),
        });
    }
    result
}

async fn progress(subscriber: &mut Option<Subscriber>, progress: Progress) {
    if let Some(stream) = subscriber
        && stream.send(&Reply::Progress { progress }).await
    {
        return;
    }
    *subscriber = None;
}

//! 공식 harness 로그인만 격리된 PTY에서 실행한다. 원본 출력은 어떠한 공개 경로에도 전달하지 않는다.
use std::io::Read;
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use aaa_proto::{AccountLoginView, ErrorBody, ErrorCode, FrontResponse};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use tokio::sync::{oneshot, watch};

use crate::account_meta::{Identity, claude_identity, codex_identity};
use crate::{Host, util};

const MAX_OUTPUT: u64 = 128 * 1024;

fn fail() -> ErrorBody {
    ErrorBody::new(
        ErrorCode::BadRequest,
        "host 로그인을 끝내지 못했다. 공식 CLI 설치·브라우저 인증·저장 권한을 확인한다",
    )
}

// Unix PTY 자식은 spawn_command가 만든 독립 session/group의 leader다.
// clone_killer의 SIGHUP은 무시할 수 있으므로 이 로그인만의 group을 강제로 끝낸다.
fn kill_login(killer: &mut dyn portable_pty::ChildKiller, pid: Option<u32>) {
    #[cfg(unix)]
    if let Some(pid) = pid {
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
        return;
    }
    let _ = pid;
    aaa_common::diagnostic::ignored(killer.kill(), "정리 실패가 본 작업의 결과를 덮지 않게 한다");
}

pub(crate) struct LoginJob {
    pub(super) view: Arc<Mutex<AccountLoginView>>,
    cancel: Option<oneshot::Sender<()>>,
    finished: watch::Receiver<bool>,
    /// 다시 로그인에 다른 메일이 들어온 새 인증(`mismatch`). 금고에 넣지 않고 메모리에만 두며, 고르거나 시한이 지나면 버린다
    pending: Arc<Mutex<Option<Pending>>>,
}

struct Pending {
    secret: String,
    identity: Identity,
    deadline: std::time::Instant,
}

/// 다른 메일 확인을 기다리는 시한(로그인 시한과 같다)
const CONFIRM_FOR: Duration = Duration::from_secs(300);

/// 확인 시한이 지난 새 인증을 버린다(옛 인증은 그대로다). 바꿨으면 참: 부른 쪽이 화면에 알린다(`accounts_bump`)
fn expire_parts(view: &Mutex<AccountLoginView>, pending: &Mutex<Option<Pending>>) -> bool {
    let mut v = view.lock().unwrap();
    let mut p = pending.lock().unwrap();
    if v.state == "mismatch" && p.as_ref().is_none_or(|p| std::time::Instant::now() >= p.deadline) {
        *p = None;
        v.state = "cancelled".into();
        return true;
    }
    false
}

fn expire(job: &LoginJob) -> bool {
    expire_parts(&job.view, &job.pending)
}

fn busy(state: &str) -> bool {
    state == "waiting" || state == "mismatch"
}

struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        aaa_common::diagnostic::ignored(
            std::fs::remove_dir_all(&self.0),
            "정리 실패가 본 작업의 결과를 덮지 않게 한다",
        );
    }
}

impl Host {
    pub(crate) fn account_login(
        self: &Arc<Self>,
        name: &str,
        harness: &str,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        if !crate::accounts::valid_name(name) || !matches!(harness, "claude" | "codex") {
            return Err(fail());
        }
        if self.accounts.list()?.iter().any(|a| a.name == name && a.harness != harness) {
            return Err(fail());
        }
        // 다시 로그인이면 그 별칭에 기록된 메일로 들어와야 한다(「이 메일로 로그인하세요」)
        let expect_email =
            if self.accounts.list()?.iter().any(|a| a.name == name) { self.account_meta(name).email } else { None };
        let mut jobs = self.accounts.logins.lock().unwrap();
        if jobs.values().map(expire).fold(false, |a, b| a | b) {
            self.accounts_bump();
        }
        if jobs.values().any(|j| {
            let v = j.view.lock().unwrap();
            v.account == name && busy(&v.state)
        }) {
            return Err(ErrorBody::new(ErrorCode::BadRequest, "그 계정의 로그인이 진행 중이다"));
        }
        // 진행 중·확인 대기 작업은 버리지 않는다. 오래된 완료 상태만 정리한다.
        if jobs.len() >= 16 {
            jobs.retain(|_, j| busy(&j.view.lock().unwrap().state));
        }
        if jobs.len() >= 16 {
            return Err(fail());
        }
        let id = util::new_id();
        let root = self.state.join("account-logins");
        if root
            .symlink_metadata()
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "성공 여부만 쓰는 분기의 하위 오류"))
            .is_ok_and(|m| m.file_type().is_symlink())
        {
            return Err(fail());
        }
        util::ensure_private_dir(&root)
            .map_err(|source_error| aaa_common::diagnostic::context(source_error, fail()))?;
        let temp = Temp(root.join(&id));
        util::ensure_private_dir(&temp.0)
            .map_err(|source_error| aaa_common::diagnostic::context(source_error, fail()))?;
        let home = temp.0.join(harness);
        util::ensure_private_dir(&home)
            .map_err(|source_error| aaa_common::diagnostic::context(source_error, fail()))?;
        let mut command = CommandBuilder::new(harness);
        command.env_clear();
        for key in [
            "PATH",
            "SYSTEMROOT",
            "WINDIR",
            "PATHEXT",
            "TMPDIR",
            "TMP",
            "TEMP",
            "LANG",
            "LC_ALL",
            "DISPLAY",
            "WAYLAND_DISPLAY",
            "DBUS_SESSION_BUS_ADDRESS",
        ] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        command.env("HOME", &temp.0);
        command.env("USERPROFILE", &temp.0);
        command.env("XDG_CONFIG_HOME", &temp.0);
        command.env("XDG_DATA_HOME", &temp.0);
        command.env("XDG_STATE_HOME", &temp.0);
        command.env("XDG_CACHE_HOME", &temp.0);
        command.env("TERM", "xterm-256color");
        command.cwd(&temp.0);
        #[cfg(unix)]
        command.umask(Some(0o077));
        if harness == "codex" {
            command.env("CODEX_HOME", &home);
            command.args(["-c", "cli_auth_credentials_store=\"file\"", "login"]);
        } else {
            command.env("CLAUDE_CONFIG_DIR", &home);
            command.arg("setup-token");
        }
        let pair = native_pty_system()
            .openpty(PtySize { rows: 32, cols: 512, pixel_width: 0, pixel_height: 0 })
            .map_err(|source_error| aaa_common::diagnostic::context(source_error, fail()))?;
        #[cfg(unix)]
        let output_fd = pair.master.as_raw_fd().ok_or_else(fail)?;
        let mut child = pair
            .slave
            .spawn_command(command)
            .map_err(|source_error| aaa_common::diagnostic::context(source_error, fail()))?;
        let pid = child.process_id();
        let mut killer = child.clone_killer();
        let reader = match pair.master.try_clone_reader() {
            Ok(reader) => reader,
            Err(source_error) => {
                aaa_common::diagnostic::observe_type(&source_error, "오류 뒤 기존 복구·거부 분기를 따른다");
                kill_login(&mut *child.clone_killer(), pid);
                aaa_common::diagnostic::ignored(child.wait(), "정리 실패가 본 작업의 결과를 덮지 않게 한다");
                return Err(fail());
            }
        };
        drop(pair.slave);
        let view = AccountLoginView {
            id: id.clone(),
            account: name.into(),
            harness: harness.into(),
            state: "waiting".into(),
            expect_email,
            email: None,
        };
        let state = Arc::new(Mutex::new(view.clone()));
        let pending = Arc::new(Mutex::new(None));
        let (cancel, mut cancelled) = oneshot::channel();
        let (done, finished) = watch::channel(false);
        jobs.insert(
            id.clone(),
            LoginJob { view: state.clone(), cancel: Some(cancel), finished, pending: pending.clone() },
        );
        drop(jobs);
        self.accounts_bump();
        tracing::info!(login = %id, account = %name, %harness, child_pid = pid, browser = "provider_managed_unconfirmed", "계정 로그인 자식 시작");
        self.db.audit(actor, "account_login", name);
        let host = self.clone();
        tokio::spawn(async move {
            // PTY의 출력/오류는 메모리에만 모은다. 상한을 넘으면 자식을 끝내고 실패한다.
            let mut output_killer = killer.clone_killer();
            let output_done = Arc::new(AtomicBool::new(false));
            let reader_done = output_done.clone();
            let output = tokio::task::spawn_blocking(move || {
                #[cfg(not(unix))]
                let _ = reader_done;
                let mut bytes = Vec::new();
                let mut reader = reader;
                let mut chunk = [0; 4096];
                loop {
                    // 자식 종료 뒤에도 자손이 PTY를 보유할 수 있다. 출력이 더 없으면 EOF를 기다리지 않는다.
                    #[cfg(unix)]
                    {
                        let mut event = libc::pollfd { fd: output_fd, events: libc::POLLIN, revents: 0 };
                        let ready = unsafe { libc::poll(&mut event, 1, 20) };
                        if ready == 0 {
                            if reader_done.load(Ordering::Acquire) {
                                break;
                            }
                            continue;
                        }
                        if ready < 0 {
                            break;
                        }
                    }
                    match reader.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => bytes.extend_from_slice(&chunk[..n]),
                    }
                    if bytes.len() as u64 > MAX_OUTPUT {
                        kill_login(&mut *output_killer, pid);
                        break;
                    }
                }
                bytes
            });
            let mut wait = tokio::task::spawn_blocking(move || child.wait());
            let mut exit_code = None;
            let end = tokio::select! {
                status = &mut wait => {
                    if let Ok(Ok(ref s)) = status { exit_code = Some(s.exit_code()); }
                    if matches!(status, Ok(Ok(ref s)) if s.success()) { "succeeded" } else { "failed" }
                },
                _ = &mut cancelled => "cancelled",
                _ = tokio::time::sleep(Duration::from_secs(300)) => "failed",
            };
            if !wait.is_finished() {
                kill_login(&mut *killer, pid);
                if let Ok(Ok(status)) = wait.await {
                    exit_code = Some(status.exit_code());
                }
            }
            tracing::info!(login = %id, child_pid = pid, ?exit_code, outcome = end, "계정 로그인 자식 종료");
            output_done.store(true, Ordering::Release);
            let bytes = output.await.unwrap_or_default();
            drop(pair.master);
            // 취소·금고 확정·상태 확정을 같은 상태 lock 아래 직렬화한다.
            let mut v = state.lock().unwrap();
            let was_cancelled = end == "cancelled"
                || cancelled
                    .try_recv()
                    .as_ref()
                    .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
                    .is_ok();
            let secret = if end == "succeeded" && !was_cancelled && bytes.len() as u64 <= MAX_OUTPUT {
                login_secret(&v.harness, &home, &bytes)
            } else {
                Err(fail())
            };
            // 메일·만료는 임시 폴더를 지우기 전에 읽는다(Claude `.claude.json`)
            let identity = match &secret {
                Ok(secret) => login_identity(&v.harness, &temp.0, &home, &bytes, secret),
                Err(source_error) => {
                    aaa_common::diagnostic::observe_type(&source_error, "오류 뒤 기존 복구·거부 분기를 따른다");
                    Identity::default()
                }
            };
            // 평문 정리에 실패하면 기존 금고 값을 바꾸지 않는다.
            let cleaned = std::fs::remove_dir_all(&temp.0)
                .as_ref()
                .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
                .is_ok();
            drop(temp);
            let other_email =
                matches!((&v.expect_email, &identity.email), (Some(a), Some(b)) if !a.eq_ignore_ascii_case(b));
            let mut result = Err(fail());
            if cleaned && let Ok(secret) = secret {
                if other_email {
                    // 다른 메일: 옛 인증을 그대로 두고 사람이 고르기를 기다린다
                    aaa_common::redact::remember_auth(&v.harness, &secret);
                    v.email = identity.email.clone();
                    *pending.lock().unwrap() =
                        Some(Pending { secret, identity, deadline: std::time::Instant::now() + CONFIRM_FOR });
                    result = Ok(true);
                    // 아무도 묻지 않아도 시한에 새 인증을 버리고 화면에 알린다
                    // host를 붙잡지 않는다(약한 참조): 그 전에 host가 끝나면 할 일이 없다
                    let (view, pending, host) = (state.clone(), pending.clone(), Arc::downgrade(&host));
                    tokio::spawn(async move {
                        tokio::time::sleep(CONFIRM_FOR).await;
                        if expire_parts(&view, &pending) {
                            tracing::info!(login = %view.lock().unwrap().id, "다른 메일 확인 시한: 새 인증을 버렸다");
                            if let Some(host) = host.upgrade() {
                                host.accounts_bump();
                            }
                        }
                    });
                } else {
                    result = host.accounts.import(&v.account, &v.harness, &secret).map(|_| false);
                    if result
                        .as_ref()
                        .inspect_err(|error| {
                            aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")
                        })
                        .is_ok()
                    {
                        v.email = identity.email.clone();
                        host.account_logged_in(&v.account, &identity, false);
                    }
                }
            }
            v.state = if was_cancelled {
                "cancelled"
            } else {
                match result {
                    Ok(true) => "mismatch",
                    Ok(false) => "succeeded",
                    Err(source_error) => {
                        aaa_common::diagnostic::observe_type(&source_error, "오류 뒤 기존 복구·거부 분기를 따른다");
                        "failed"
                    }
                }
            }
            .into();
            tracing::info!(login = %id, state = %v.state, temporary_cleaned = cleaned, "계정 로그인 완료");
            drop(v);
            host.accounts_bump();
            done.send_replace(true);
        });
        Ok(FrontResponse::AccountLogin { login: view })
    }

    /// host 정상 종료는 로그인 자식의 종료·임시 자료 정리까지 기다린다.
    pub(crate) async fn cancel_account_logins(&self) {
        let finished = {
            let mut jobs = self.accounts.logins.lock().unwrap();
            jobs.values_mut()
                .filter_map(|job| {
                    if job.view.lock().unwrap().state != "waiting" {
                        return None;
                    }
                    if let Some(cancel) = job.cancel.take() {
                        aaa_common::diagnostic::ignored(cancel.send(()), "수신자가 끝난 뒤에도 현재 작업은 마친다");
                    }
                    Some(job.finished.clone())
                })
                .collect::<Vec<_>>()
        };
        for done in finished {
            aaa_common::diagnostic::ignored(login_finished(done).await, "보조 작업 실패 뒤 기존 처리를 계속한다");
        }
    }

    pub(crate) fn account_login_status(&self, id: &str) -> Result<FrontResponse, ErrorBody> {
        let jobs = self.accounts.logins.lock().unwrap();
        let job = jobs.get(id).ok_or_else(fail)?;
        if expire(job) {
            self.accounts_bump();
        }
        Ok(FrontResponse::AccountLogin { login: job.view.lock().unwrap().clone() })
    }

    /// 다시 로그인에 다른 메일이 들어온 뒤의 선택(FRONT_API 1.79): 바꾸면 이 별칭이 새 메일 인증이 되고(최초 인증 시각도 새로),
    /// 아니면 새 인증을 버린다. 감사 기록에는 별칭과 선택만 남긴다
    pub(crate) fn account_login_confirm(
        &self,
        id: &str,
        switch: bool,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        let (view, pending) = {
            let jobs = self.accounts.logins.lock().unwrap();
            let job = jobs.get(id).ok_or_else(fail)?;
            if expire(job) {
                self.accounts_bump();
            }
            (job.view.clone(), job.pending.clone())
        };
        let mut v = view.lock().unwrap();
        if v.state != "mismatch" {
            return Err(ErrorBody::new(
                ErrorCode::BadRequest,
                if v.state == "cancelled" {
                    "확인 시한이 지나 새 인증을 버렸다. 다시 로그인한다"
                } else {
                    "다른 메일 확인을 기다리는 로그인이 아니다"
                },
            ));
        }
        // 금고에 넣은 뒤에만 비운다: 쓰기가 실패하면 그대로 다시 고를 수 있다
        let mut slot = pending.lock().unwrap();
        let p = slot.as_ref().ok_or_else(fail)?;
        if switch {
            self.accounts.import(&v.account, &v.harness, &p.secret)?;
            self.account_logged_in(&v.account, &p.identity, true);
            v.state = "succeeded".into();
        } else {
            v.state = "kept".into();
        }
        *slot = None;
        drop(slot);
        self.db.audit(
            actor,
            "account_login_confirm",
            &format!("{} {}", v.account, if switch { "switch" } else { "keep" }),
        );
        tracing::info!(login = %id, state = %v.state, "계정 로그인 메일 확인");
        let out = v.clone();
        drop(v);
        self.accounts_bump();
        Ok(FrontResponse::AccountLogin { login: out })
    }

    pub(crate) async fn account_login_cancel(&self, id: &str) -> Result<FrontResponse, ErrorBody> {
        let (view, finished) = {
            let mut jobs = self.accounts.logins.lock().unwrap();
            let job = jobs.get_mut(id).ok_or_else(fail)?;
            {
                // 확인을 기다리던 새 인증은 버린다(옛 인증 그대로)
                let mut v = job.view.lock().unwrap();
                if v.state == "mismatch" {
                    *job.pending.lock().unwrap() = None;
                    v.state = "cancelled".into();
                    drop(v);
                    self.accounts_bump();
                }
            }
            if job.view.lock().unwrap().state == "waiting"
                && let Some(cancel) = job.cancel.take()
            {
                tracing::info!(login = %id, "계정 로그인 취소 요청");
                aaa_common::diagnostic::ignored(cancel.send(()), "수신자가 끝난 뒤에도 현재 작업은 마친다");
            }
            (job.view.clone(), job.finished.clone())
        };
        // jobs/view 잠금을 놓고 기다린다. watch는 같은 ID의 여러 취소자·host 종료자 모두를 깨운다.
        login_finished(finished).await?;
        Ok(FrontResponse::AccountLogin { login: view.lock().unwrap().clone() })
    }
}

async fn login_finished(mut done: watch::Receiver<bool>) -> Result<(), ErrorBody> {
    while !*done.borrow_and_update() {
        done.changed().await.map_err(|source_error| aaa_common::diagnostic::context(source_error, fail()))?;
    }
    Ok(())
}

/// PTY 출력의 ANSI 제어열을 뺀 글
fn plain(output: &[u8]) -> Result<String, ErrorBody> {
    let text = String::from_utf8_lossy(output);
    let ansi = regex::Regex::new(r"\x1b\[[0-?]*[ -/]*[@-~]")
        .map_err(|source_error| aaa_common::diagnostic::context(source_error, fail()))?;
    Ok(ansi.replace_all(&text, "").into_owned())
}

/// 로그인 결과의 메일·plan·만료(인증값은 돌려주지 않는다). Codex는 auth.json의 id_token, Claude는 출력의 유효 기간과 `.claude.json`
fn login_identity(
    harness: &str,
    root: &std::path::Path,
    home: &std::path::Path,
    output: &[u8],
    secret: &str,
) -> Identity {
    if harness == "codex" {
        return codex_identity(secret);
    }
    plain(output).map(|text| claude_identity(&text, &[home, root], util::now_ms())).unwrap_or_default()
}

fn login_secret(harness: &str, home: &std::path::Path, output: &[u8]) -> Result<String, ErrorBody> {
    if harness == "claude" {
        let text = plain(output)?;
        let token = regex::Regex::new(r"sk-ant-oat01-[A-Za-z0-9_-]+")
            .map_err(|source_error| aaa_common::diagnostic::context(source_error, fail()))?;
        let values: std::collections::BTreeSet<_> = token.find_iter(&text).map(|m| m.as_str()).collect();
        if values.len() != 1 {
            return Err(fail());
        }
        Ok(values.into_iter().next().ok_or_else(fail)?.into())
    } else {
        let path = home.join("auth.json");
        let m =
            path.symlink_metadata().map_err(|source_error| aaa_common::diagnostic::context(source_error, fail()))?;
        if !m.is_file() || m.len() > MAX_OUTPUT {
            return Err(fail());
        }
        let value = std::fs::read_to_string(path)
            .map_err(|source_error| aaa_common::diagnostic::context(source_error, fail()))?;
        Ok(value)
    }
}

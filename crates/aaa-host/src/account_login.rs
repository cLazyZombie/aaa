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
use tokio::sync::oneshot;

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
    let _ = killer.kill();
}

pub(crate) struct LoginJob {
    view: Arc<Mutex<AccountLoginView>>,
    cancel: Option<oneshot::Sender<()>>,
    finished: Arc<tokio::sync::Notify>,
}

struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
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
        let mut jobs = self.accounts.logins.lock().unwrap();
        if jobs.values().any(|j| {
            let v = j.view.lock().unwrap();
            v.account == name && v.state == "waiting"
        }) {
            return Err(ErrorBody::new(ErrorCode::BadRequest, "그 계정의 로그인이 진행 중이다"));
        }
        // 진행 중 작업은 버리지 않는다. 오래된 완료 상태만 정리한다.
        if jobs.len() >= 16 {
            jobs.retain(|_, j| j.view.lock().unwrap().state == "waiting");
        }
        if jobs.len() >= 16 {
            return Err(fail());
        }
        let id = util::new_id();
        let root = self.state.join("account-logins");
        if root.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(fail());
        }
        util::ensure_private_dir(&root).map_err(|_| fail())?;
        let temp = Temp(root.join(&id));
        util::ensure_private_dir(&temp.0).map_err(|_| fail())?;
        let home = temp.0.join(harness);
        util::ensure_private_dir(&home).map_err(|_| fail())?;
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
            .map_err(|_| fail())?;
        #[cfg(unix)]
        let output_fd = pair.master.as_raw_fd().ok_or_else(fail)?;
        let mut child = pair.slave.spawn_command(command).map_err(|_| fail())?;
        let pid = child.process_id();
        let mut killer = child.clone_killer();
        let reader = match pair.master.try_clone_reader() {
            Ok(reader) => reader,
            Err(_) => {
                kill_login(&mut *child.clone_killer(), pid);
                let _ = child.wait();
                return Err(fail());
            }
        };
        drop(pair.slave);
        let view =
            AccountLoginView { id: id.clone(), account: name.into(), harness: harness.into(), state: "waiting".into() };
        let state = Arc::new(Mutex::new(view.clone()));
        let (cancel, mut cancelled) = oneshot::channel();
        let finished = Arc::new(tokio::sync::Notify::new());
        jobs.insert(id, LoginJob { view: state.clone(), cancel: Some(cancel), finished: finished.clone() });
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
            let end = tokio::select! {
                status = &mut wait => if matches!(status, Ok(Ok(ref s)) if s.success()) { "succeeded" } else { "failed" },
                _ = &mut cancelled => "cancelled",
                _ = tokio::time::sleep(Duration::from_secs(300)) => "failed",
            };
            if !wait.is_finished() {
                kill_login(&mut *killer, pid);
                let _ = wait.await;
            }
            output_done.store(true, Ordering::Release);
            let bytes = output.await.unwrap_or_default();
            drop(pair.master);
            // 취소·금고 확정·상태 확정을 같은 상태 lock 아래 직렬화한다.
            let mut v = state.lock().unwrap();
            let was_cancelled = end == "cancelled" || cancelled.try_recv().is_ok();
            let secret = if end == "succeeded" && !was_cancelled && bytes.len() as u64 <= MAX_OUTPUT {
                login_secret(&v.harness, &home, &bytes)
            } else {
                Err(fail())
            };
            // 평문 정리에 실패하면 기존 금고 값을 바꾸지 않는다.
            let cleaned = std::fs::remove_dir_all(&temp.0).is_ok();
            drop(temp);
            let result = if cleaned {
                secret.and_then(|secret| host.accounts.import(&v.account, &v.harness, &secret).map(|_| ()))
            } else {
                Err(fail())
            };
            v.state = if was_cancelled {
                "cancelled"
            } else if result.is_ok() {
                "succeeded"
            } else {
                "failed"
            }
            .into();
            finished.notify_one();
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
                        let _ = cancel.send(());
                    }
                    Some(job.finished.clone())
                })
                .collect::<Vec<_>>()
        };
        for done in finished {
            done.notified().await;
        }
    }

    pub(crate) fn account_login_status(&self, id: &str) -> Result<FrontResponse, ErrorBody> {
        let jobs = self.accounts.logins.lock().unwrap();
        let job = jobs.get(id).ok_or_else(fail)?;
        Ok(FrontResponse::AccountLogin { login: job.view.lock().unwrap().clone() })
    }

    pub(crate) fn account_login_cancel(&self, id: &str) -> Result<FrontResponse, ErrorBody> {
        let mut jobs = self.accounts.logins.lock().unwrap();
        let job = jobs.get_mut(id).ok_or_else(fail)?;
        if job.view.lock().unwrap().state == "waiting"
            && let Some(cancel) = job.cancel.take()
        {
            let _ = cancel.send(());
        }
        Ok(FrontResponse::AccountLogin { login: job.view.lock().unwrap().clone() })
    }
}

fn login_secret(harness: &str, home: &std::path::Path, output: &[u8]) -> Result<String, ErrorBody> {
    if harness == "claude" {
        let text = String::from_utf8_lossy(output);
        let ansi = regex::Regex::new(r"\x1b\[[0-?]*[ -/]*[@-~]").map_err(|_| fail())?;
        let text = ansi.replace_all(&text, "");
        let token = regex::Regex::new(r"sk-ant-oat01-[A-Za-z0-9_-]+").map_err(|_| fail())?;
        let values: std::collections::BTreeSet<_> = token.find_iter(&text).map(|m| m.as_str()).collect();
        if values.len() != 1 {
            return Err(fail());
        }
        Ok(values.into_iter().next().ok_or_else(fail)?.into())
    } else {
        let path = home.join("auth.json");
        let m = path.symlink_metadata().map_err(|_| fail())?;
        if !m.is_file() || m.len() > MAX_OUTPUT {
            return Err(fail());
        }
        let value = std::fs::read_to_string(path).map_err(|_| fail())?;
        Ok(value)
    }
}

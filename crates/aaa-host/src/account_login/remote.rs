//! 공식 CLI와 요청 기기 사이의 한 번짜리 인증. 원본 출력·토큰은 Front에 보내지 않는다.
use super::*;
use aaa_common::child_process::TokioCommandExt as _;
use aaa_proto::{AccountLoginPrompt, SecretText};
use std::io::Write;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

pub(super) struct Remote {
    owner: String,
    prompt: Option<AccountLoginPrompt>,
    code: Option<mpsc::UnboundedSender<SecretText>>,
    expires_ms: u64,
    deadline: std::time::Instant,
    reaped: Option<watch::Receiver<bool>>,
}

fn error(message: &str) -> ErrorBody {
    ErrorBody::new(ErrorCode::BadRequest, message)
}

pub(super) fn check_owner(job: &LoginJob, actor: &str) -> Result<(), ErrorBody> {
    if let Some(remote) = &job.remote
        && remote.lock().unwrap().owner != actor
    {
        return Err(ErrorBody::new(ErrorCode::Forbidden, "인증을 시작한 기기에서만 조작할 수 있습니다"));
    }
    Ok(())
}

impl Host {
    pub(crate) fn account_login_remote(
        self: &Arc<Self>,
        name: &str,
        harness: &str,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        self.start_remote(name, harness, actor, std::path::PathBuf::from(harness))
    }

    fn start_remote(
        self: &Arc<Self>,
        name: &str,
        harness: &str,
        actor: &str,
        program: std::path::PathBuf,
    ) -> Result<FrontResponse, ErrorBody> {
        if !actor.starts_with("device:") || self.is_revoked(actor.strip_prefix("device:").unwrap_or("")) {
            return Err(ErrorBody::new(ErrorCode::Forbidden, "원격 인증은 웹 조작 기기에서 시작합니다"));
        }
        if !crate::accounts::valid_name(name) || !matches!(harness, "claude" | "codex") {
            return Err(fail());
        }
        // Claude의 BROWSER 억제 helper는 POSIX 전용이다. 미검증 Windows 경로로 브라우저를 열지 않는다.
        if harness == "claude" && !cfg!(unix) {
            return Err(error("이 host에서는 Claude 원격 인증을 지원하지 않습니다"));
        }
        let existing = self.accounts.list()?;
        if existing.iter().any(|a| a.name == name && a.harness != harness) {
            return Err(fail());
        }
        let mut jobs = self.accounts.logins.lock().unwrap();
        for job in jobs.values() {
            expire(job);
            let view = job.view.lock().unwrap();
            if busy(&view.state)
                && view.account != name
                && job.remote.as_ref().is_some_and(|r| r.lock().unwrap().owner == actor)
            {
                return Err(error("진행 중인 인증을 먼저 마치거나 취소하세요"));
            }
            if view.account == name && busy(&view.state) {
                check_owner(job, actor)?;
                if job.remote.is_some() {
                    return Ok(FrontResponse::AccountLogin { login: view.clone() });
                }
                return Err(error("그 계정의 로그인이 진행 중입니다"));
            }
        }
        if jobs.len() >= 16 {
            jobs.retain(|_, job| busy(&job.view.lock().unwrap().state));
        }
        if jobs.len() >= 16 {
            return Err(error("진행 중인 인증이 너무 많습니다"));
        }
        let id = util::new_id();
        let root = self.state.join("account-logins");
        if root.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(fail());
        }
        util::ensure_private_dir(&root).map_err(|e| aaa_common::diagnostic::context(e, fail()))?;
        let temp = Temp(root.join(&id));
        util::ensure_private_dir(&temp.0).map_err(|e| aaa_common::diagnostic::context(e, fail()))?;
        let home = temp.0.join(harness);
        util::ensure_private_dir(&home).map_err(|e| aaa_common::diagnostic::context(e, fail()))?;
        let view = AccountLoginView {
            id: id.clone(),
            account: name.into(),
            harness: harness.into(),
            state: "waiting".into(),
            expect_email: self.account_meta(name).email,
            email: None,
            error: None,
        };
        let state = Arc::new(Mutex::new(view.clone()));
        let pending = Arc::new(Mutex::new(None));
        let (code, mut input) = mpsc::unbounded_channel();
        let remote = Arc::new(Mutex::new(Remote {
            owner: actor.into(),
            prompt: None,
            code: Some(code),
            expires_ms: util::now_ms() + 300_000,
            deadline: std::time::Instant::now() + Duration::from_secs(300),
            reaped: None,
        }));
        let (cancel, mut cancelled) = oneshot::channel();
        let (done, finished) = watch::channel(false);
        jobs.insert(
            id.clone(),
            LoginJob {
                remote: Some(remote.clone()),
                view: state.clone(),
                pending: pending.clone(),
                cancel: Some(cancel),
                finished,
            },
        );
        drop(jobs);
        self.db.audit(actor, "account_login", name);
        self.accounts_bump();
        let host = self.clone();
        let harness = harness.to_string();
        let is_existing = existing.iter().any(|a| a.name == name);
        let mut revoked = host.device_revoked.subscribe();
        tokio::spawn(async move {
            let run = async {
                if harness == "codex" {
                    codex(&host, &remote, &home, &program).await
                } else {
                    claude(&host, &remote, &temp.0, &home, &mut input, &program).await
                }
            };
            let owner_device = remote.lock().unwrap().owner.strip_prefix("device:").unwrap_or("").to_string();
            let revoked_owner = async {
                loop {
                    match revoked.recv().await {
                        Ok(id) if id == owner_device => break,
                        Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(_) => break,
                    }
                }
            };
            let (result, was_cancelled) = tokio::select! {
                result = run => (result, false),
                _ = &mut cancelled => (Err(error("인증을 취소했습니다")), true),
                _ = revoked_owner => (Err(error("기기 권한이 해제됐습니다")), true),
                _ = tokio::time::sleep(Duration::from_secs(300)) => (Err(error("인증 시간이 만료됐습니다")), false),
            };
            // 어댑터 Drop이 자기 자식을 종료한다. reader/wait의 완료까지 기다린 후 임시 자료를 정리한다.
            let cleanup = reap(&remote).await;
            {
                let mut r = remote.lock().unwrap();
                r.prompt = None;
                r.code = None;
            }
            let output = result.as_ref().map(Vec::as_slice).unwrap_or_default();
            let secret =
                if result.is_ok() && !was_cancelled { login_secret(&harness, &home, output) } else { Err(fail()) };
            let identity =
                secret.as_ref().map(|s| login_identity(&harness, &temp.0, &home, output, s)).unwrap_or_default();
            if let Err(e) = &result {
                tracing::info!(login = %id, reason = %e.message, "원격 인증 종료");
            }
            let cleaned = cleanup
                && std::fs::remove_dir_all(&temp.0)
                    .inspect_err(|e| aaa_common::diagnostic::observe(e, "원격 인증 임시 자료 정리"))
                    .is_ok();
            // 취소와 저장은 같은 view 잠금으로 직렬화한다.
            let mut v = state.lock().unwrap();
            let was_cancelled = was_cancelled
                || cancelled.try_recv().is_ok()
                || host.revoked_devices.lock().unwrap().contains(&owner_device);
            // 메일을 모르는 기존 계정은 사람의 명시적 교체 확인을 거친다.
            let unknown = is_existing && identity.email.is_none() && secret.is_ok() && cleaned && !was_cancelled;
            if unknown {
                let secret = secret.unwrap();
                aaa_common::redact::remember_auth(&harness, &secret);
                *pending.lock().unwrap() =
                    Some(Pending { secret, identity, deadline: std::time::Instant::now() + CONFIRM_FOR });
                v.state = "mismatch".into();
                let (state, pending, host) = (state.clone(), pending.clone(), Arc::downgrade(&host));
                tokio::spawn(async move {
                    tokio::time::sleep(CONFIRM_FOR).await;
                    if expire_parts(&state, &pending)
                        && let Some(host) = host.upgrade()
                    {
                        host.accounts_bump();
                    }
                });
            } else {
                complete(&host, &mut v, &state, &pending, secret, identity, cleaned, was_cancelled);
            }
            if v.state == "failed" {
                v.error =
                    Some(result.err().map(|e| e.message).unwrap_or_else(|| "인증 결과를 저장하지 못했습니다".into()));
            }
            drop(v);
            host.accounts_bump();
            done.send_replace(true);
        });
        Ok(FrontResponse::AccountLogin { login: view })
    }

    pub(crate) fn revoke_account_logins(&self, device: &str) {
        let actor = format!("device:{device}");
        let mut jobs = self.accounts.logins.lock().unwrap();
        for job in jobs.values_mut() {
            if job.remote.as_ref().is_none_or(|r| r.lock().unwrap().owner != actor) {
                continue;
            }
            let mut view = job.view.lock().unwrap();
            if view.state == "mismatch" {
                *job.pending.lock().unwrap() = None;
                view.state = "cancelled".into();
            }
            if let Some(remote) = &job.remote {
                let mut r = remote.lock().unwrap();
                r.prompt = None;
                r.code = None;
            }
            if let Some(cancel) = job.cancel.take() {
                let _ = cancel.send(());
            }
        }
        drop(jobs);
        self.accounts_bump();
    }

    pub(crate) fn account_login_interaction(&self, id: Option<&str>, actor: &str) -> Result<FrontResponse, ErrorBody> {
        let jobs = self.accounts.logins.lock().unwrap();
        let job = if let Some(id) = id {
            Some(jobs.get(id).ok_or_else(fail)?)
        } else {
            jobs.iter()
                .filter(|(_, j)| {
                    j.remote.as_ref().is_some_and(|r| r.lock().unwrap().owner == actor)
                        && busy(&j.view.lock().unwrap().state)
                })
                .max_by_key(|(id, _)| *id)
                .map(|(_, j)| j)
        };
        let Some(job) = job else {
            return Ok(FrontResponse::AccountLoginInteraction { login: None, prompt: None, expires_ms: 0 });
        };
        check_owner(job, actor)?;
        if expire(job) {
            self.accounts_bump();
        }
        let view = job.view.lock().unwrap().clone();
        let r = job.remote.as_ref().ok_or_else(fail)?.lock().unwrap();
        Ok(FrontResponse::AccountLoginInteraction {
            prompt: if view.state == "waiting" && std::time::Instant::now() < r.deadline {
                r.prompt.clone()
            } else {
                None
            },
            login: Some(view),
            expires_ms: r.expires_ms,
        })
    }

    pub(crate) fn account_login_code(
        &self,
        id: &str,
        code: SecretText,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        if code.0.is_empty() || code.0.len() > 4096 || !code.0.bytes().all(|b| b.is_ascii_graphic()) {
            return Err(error("인증 코드 형식이 올바르지 않습니다"));
        }
        let jobs = self.accounts.logins.lock().unwrap();
        let job = jobs.get(id).ok_or_else(fail)?;
        check_owner(job, actor)?;
        let view = job.view.lock().unwrap();
        let mut r = job.remote.as_ref().ok_or_else(fail)?.lock().unwrap();
        if view.state != "waiting"
            || std::time::Instant::now() >= r.deadline
            || r.prompt.as_ref().is_none_or(|p| p.kind != "paste_code")
        {
            return Err(error("인증 코드를 기다리는 상태가 아닙니다"));
        }
        let tx = r.code.take().ok_or_else(|| error("이미 코드를 보냈습니다"))?;
        tx.send(code).map_err(|_| error("인증 입력이 종료됐습니다"))?;
        r.prompt = None;
        drop(r);
        drop(view);
        drop(jobs);
        self.accounts_bump();
        Ok(FrontResponse::Done)
    }
}

// 어댑터가 취소되더라도 비동기 wait 완료를 외부 소유자가 기다린다.
// 작업별 handle은 Remote에 두어 다른 로그인과 정리 경합을 만들지 않는다.
async fn reap(remote: &Arc<Mutex<Remote>>) -> bool {
    let reaped = remote.lock().unwrap().reaped.clone();
    let Some(mut reaped) = reaped else {
        return true;
    };
    while !*reaped.borrow_and_update() {
        if reaped.changed().await.is_err() {
            return false;
        }
    }
    true
}

struct Process {
    child: Option<tokio::process::Child>,
    done: Option<watch::Sender<bool>>,
}
impl std::ops::Deref for Process {
    type Target = tokio::process::Child;
    fn deref(&self) -> &Self::Target {
        self.child.as_ref().unwrap()
    }
}
impl std::ops::DerefMut for Process {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.child.as_mut().unwrap()
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        let mut child = self.child.take().unwrap();
        let done = self.done.take().unwrap();
        tokio::spawn(async move {
            if child.try_wait().ok().flatten().is_none() {
                let _ = child.kill().await;
            }
            match child.wait().await {
                Ok(status) => {
                    tracing::info!(exit_code = status.code(), "원격 인증 자식 회수");
                    done.send_replace(true);
                }
                Err(e) => aaa_common::diagnostic::observe(&e, "원격 인증 자식 회수 실패"),
            }
        });
    }
}

fn publish(host: &Host, remote: &Mutex<Remote>, prompt: AccountLoginPrompt) {
    remote.lock().unwrap().prompt = Some(prompt);
    host.accounts_bump();
}

fn command(home: &std::path::Path, program: &std::path::Path) -> tokio::process::Command {
    let mut c = tokio::process::Command::new(program);
    c.env_clear();
    for k in ["PATH", "SystemRoot", "SYSTEMROOT", "WINDIR", "TMPDIR", "TMP", "TEMP"] {
        if let Some(v) = std::env::var_os(k) {
            c.env(k, v);
        }
    }
    c.env("HOME", home)
        .env("USERPROFILE", home)
        .env("CODEX_HOME", home)
        .env("XDG_CONFIG_HOME", home)
        .env("XDG_DATA_HOME", home)
        .env("XDG_CACHE_HOME", home)
        .env("XDG_STATE_HOME", home)
        .current_dir(home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    c
}

async fn codex(
    host: &Host,
    remote: &Arc<Mutex<Remote>>,
    home: &std::path::Path,
    program: &std::path::Path,
) -> Result<Vec<u8>, ErrorBody> {
    let child = command(home, program)
        .args(["-c", "cli_auth_credentials_store=\"file\"", "app-server"])
        .spawn_guarded()
        .map_err(|e| aaa_common::diagnostic::context(e, error("Codex를 시작하지 못했습니다")))?;
    let (done, reaped) = watch::channel(false);
    remote.lock().unwrap().reaped = Some(reaped);
    let mut child = Process { child: Some(child), done: Some(done) };
    let pid = child.id();
    tracing::info!(child_pid = pid, "Codex 원격 인증 시작");
    let mut tx = child.stdin.take().ok_or_else(fail)?;
    let mut rx = BufReader::new(child.stdout.take().ok_or_else(fail)?);
    let init = serde_json::json!({"id":1,"method":"initialize","params":{"clientInfo":{"name":"aaa-login","title":"aaa","version":env!("CARGO_PKG_VERSION")}}});
    tx.write_all(format!("{init}\n").as_bytes()).await.map_err(|e| aaa_common::diagnostic::context(e, fail()))?;
    let mut total = 0usize;
    let mut login_id = None;
    loop {
        let mut line = Vec::new();
        let n = (&mut rx)
            .take(MAX_OUTPUT + 1)
            .read_until(b'\n', &mut line)
            .await
            .map_err(|e| aaa_common::diagnostic::context(e, fail()))?;
        total += n;
        if n == 0 || total > MAX_OUTPUT as usize {
            return Err(error("Codex 인증 응답이 끝났거나 상한을 넘었습니다"));
        }
        let value: serde_json::Value =
            serde_json::from_slice(&line).map_err(|_| error("Codex 인증 응답 형식이 올바르지 않습니다"))?;
        if value.get("error").is_some() {
            tracing::info!(rpc_code = value["error"]["code"].as_i64(), "Codex 인증 RPC 거부");
            return Err(error("Codex 인증을 시작하지 못했습니다. 계정의 기기 코드 허용 설정을 확인하세요"));
        }
        if value["id"] == 1 {
            tx.write_all(b"{\"method\":\"initialized\",\"params\":{}}\n{\"id\":2,\"method\":\"account/login/start\",\"params\":{\"type\":\"chatgptDeviceCode\"}}\n").await.map_err(|e| aaa_common::diagnostic::context(e, fail()))?;
        } else if value["id"] == 2 {
            let v = &value["result"];
            let url = v["verificationUrl"].as_str().ok_or_else(fail)?;
            let code = v["userCode"].as_str().ok_or_else(fail)?;
            if url != "https://auth.openai.com/codex/device"
                || code.is_empty()
                || code.len() > 64
                || !code.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            {
                return Err(error("Codex 인증 주소·코드가 올바르지 않습니다"));
            }
            login_id = Some(v["loginId"].as_str().ok_or_else(fail)?.to_owned());
            publish(
                host,
                remote,
                AccountLoginPrompt {
                    kind: "device_code".into(),
                    url: SecretText(url.into()),
                    user_code: Some(SecretText(code.into())),
                },
            );
        } else if value["method"] == "account/login/completed"
            && login_id.as_deref().is_some_and(|id| value["params"]["loginId"] == id)
        {
            let success = value["params"]["success"] == true;
            child.kill().await.map_err(|e| aaa_common::diagnostic::context(e, fail()))?;
            let status = child.wait().await.map_err(|e| aaa_common::diagnostic::context(e, fail()))?;
            tracing::info!(child_pid = pid, exit_code = status.code(), success, "Codex 원격 인증 종료");
            return if success {
                Ok(Vec::new())
            } else {
                Err(error("Codex 인증이 거부됐거나 만료됐습니다"))
            };
        }
    }
}

struct PtyGuard {
    killer: Box<dyn portable_pty::ChildKiller + Send + Sync>,
    pid: Option<u32>,
    ended: Arc<AtomicBool>,
}
impl Drop for PtyGuard {
    fn drop(&mut self) {
        if !self.ended.load(Ordering::Acquire) {
            kill_login(&mut *self.killer, self.pid);
        }
    }
}

async fn claude(
    host: &Host,
    remote: &Arc<Mutex<Remote>>,
    root: &std::path::Path,
    home: &std::path::Path,
    input: &mut mpsc::UnboundedReceiver<SecretText>,
    program: &std::path::Path,
) -> Result<Vec<u8>, ErrorBody> {
    let browser = root.join("no-browser");
    util::write_private(&browser, b"#!/bin/sh\nexit 0\n").map_err(|e| aaa_common::diagnostic::context(e, fail()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&browser, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| aaa_common::diagnostic::context(e, fail()))?;
    }
    let mut c = CommandBuilder::new(program);
    c.arg("setup-token");
    c.env_clear();
    for key in ["PATH", "TMPDIR", "TMP", "TEMP", "LANG"] {
        if let Some(v) = std::env::var_os(key) {
            c.env(key, v);
        }
    }
    for key in ["HOME", "USERPROFILE", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "XDG_STATE_HOME"] {
        c.env(key, root);
    }
    c.env("CLAUDE_CONFIG_DIR", home);
    c.env("BROWSER", &browser);
    c.env("TERM", "dumb");
    c.cwd(root);
    #[cfg(unix)]
    c.umask(Some(0o077));
    let pair = aaa_common::child_process::with_spawn_lock(|| {
        native_pty_system().openpty(PtySize { rows: 32, cols: 512, pixel_width: 0, pixel_height: 0 })
    })
    .map_err(|e| aaa_common::diagnostic::context(e, fail()))?;
    #[cfg(unix)]
    let output_fd = pair.master.as_raw_fd().ok_or_else(fail)?;
    let mut reader = pair.master.try_clone_reader().map_err(|e| aaa_common::diagnostic::context(e, fail()))?;
    let mut writer = pair.master.take_writer().map_err(|e| aaa_common::diagnostic::context(e, fail()))?;
    let mut child = aaa_common::child_process::with_spawn_lock(|| pair.slave.spawn_command(c))
        .map_err(|e| aaa_common::diagnostic::context(e, fail()))?;
    let pid = child.process_id();
    let ended = Arc::new(AtomicBool::new(false));
    let _guard = PtyGuard { killer: child.clone_killer(), pid, ended: ended.clone() };
    drop(pair.slave);
    let (tx, mut rx) = mpsc::unbounded_channel();
    let reader_ended = ended.clone();
    let reader_task = tokio::task::spawn_blocking(move || {
        let mut count = 0usize;
        let mut buf = [0; 4096];
        loop {
            #[cfg(unix)]
            {
                let mut event = libc::pollfd { fd: output_fd, events: libc::POLLIN, revents: 0 };
                let ready = unsafe { libc::poll(&mut event, 1, 20) };
                if ready == 0 {
                    if reader_ended.load(Ordering::Acquire) {
                        break;
                    }
                    continue;
                }
                if ready < 0 || event.revents & libc::POLLNVAL != 0 {
                    break;
                }
            }
            #[cfg(not(unix))]
            let _ = &reader_ended;
            let Ok(n) = reader.read(&mut buf) else {
                break;
            };
            if n == 0 {
                break;
            }
            count += n;
            if tx.send(buf[..n].to_vec()).is_err() || count > MAX_OUTPUT as usize {
                break;
            }
        }
    });
    let (done, reaped) = watch::channel(false);
    remote.lock().unwrap().reaped = Some(reaped);
    let (status_tx, mut status_rx) = oneshot::channel();
    tokio::spawn(async move {
        let status = tokio::task::spawn_blocking(move || child.wait()).await;
        ended.store(true, Ordering::Release);
        let _ = reader_task.await;
        let success = matches!(&status, Ok(Ok(s)) if s.success());
        let exit_code = status.as_ref().ok().and_then(|s| s.as_ref().ok()).map(|s| s.exit_code());
        if let Ok(Err(e)) = &status {
            aaa_common::diagnostic::observe(e, "Claude 원격 인증 자식 회수 실패");
        }
        tracing::info!(child_pid = pid, exit_code, success, "Claude 원격 인증 자식 회수");
        let _ = status_tx.send(success);
        done.send_replace(true);
    });
    let mut bytes = Vec::new();
    let mut prompted = false;
    let mut submitted = false;
    let result = loop {
        tokio::select! {
            chunk = rx.recv() => if let Some(chunk) = chunk {
                bytes.extend_from_slice(&chunk);
                if bytes.len() > MAX_OUTPUT as usize { break Err(error("Claude 인증 출력이 상한을 넘었습니다")); }
                if !prompted && let Some(url) = claude_url(&bytes) {
                    publish(host, remote, AccountLoginPrompt { kind: "paste_code".into(), url: SecretText(url), user_code: None }); prompted = true;
                }
            } else { let status = (&mut status_rx).await; break if matches!(status, Ok(true)) { Ok(()) } else { Err(error("Claude 인증을 끝내지 못했습니다")) }; },
            code = input.recv(), if !submitted => if let Some(code) = code {
                writer.write_all(format!("{}\r", code.0).as_bytes()).map_err(|e| aaa_common::diagnostic::context(e, fail()))?;
                writer.flush().map_err(|e| aaa_common::diagnostic::context(e, fail()))?; submitted = true;
            },
            status = &mut status_rx => {
                while let Ok(chunk) = rx.try_recv() { bytes.extend_from_slice(&chunk); }
                tracing::info!(child_pid = pid, "Claude 원격 인증 자식 종료");
                break if matches!(status, Ok(true)) && bytes.len() <= MAX_OUTPUT as usize { Ok(()) } else { Err(error("Claude 인증이 거부됐거나 종료됐습니다")) };
            }
        }
    };
    result.map(|_| bytes)
}

fn claude_url(bytes: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(bytes).ok()?;
    // OSC 8의 링크 대상은 완전한 주소지만 화면에 보이는 글은 줄바꿈·말줄임될 수 있다.
    // ESC/BEL도 경계로 삼아 다음 링크나 제어 시퀀스가 주소에 붙지 않게 한다.
    let re = regex::Regex::new(r"https://[^\s\x00-\x1f]+[\s\x00-\x1f]").ok()?;
    for m in re.find_iter(text) {
        let raw = m.as_str().trim_end_matches(|c: char| c.is_whitespace() || c.is_control());
        let Ok(uri) = raw.parse::<axum::http::Uri>() else { continue };
        if uri.scheme_str() != Some("https")
            || !matches!(
                (uri.host(), uri.path()),
                (Some("claude.com"), "/cai/oauth/authorize") | (Some("claude.com" | "claude.ai"), "/oauth/authorize")
            )
            || uri.authority().is_none_or(|a| a.as_str().contains('@'))
            || uri.port().is_some()
        {
            continue;
        }
        let Some(query) = uri.query() else { continue };
        let mut fields = std::collections::HashMap::new();
        let unique = query.split('&').all(|p| p.split_once('=').is_some_and(|(k, v)| fields.insert(k, v).is_none()));
        if !unique
            || fields.get("code") != Some(&"true")
            || fields.get("state").is_none_or(|v| v.is_empty())
            || fields.get("code_challenge").is_none_or(|v| v.is_empty())
            || fields.get("code_challenge_method") != Some(&"S256")
            || fields.get("response_type") != Some(&"code")
            || !matches!(
                fields.get("redirect_uri").copied(),
                Some(
                    "https%3A%2F%2Fplatform.claude.com%2Foauth%2Fcode%2Fcallback"
                        | "https%3A%2F%2Fconsole.anthropic.com%2Foauth%2Fcode%2Fcallback"
                )
            )
        {
            continue;
        }
        return Some(raw.into());
    }
    None
}

#[cfg(test)]
mod tests;

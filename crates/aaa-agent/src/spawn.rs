//! 새 세션(pane)을 띄운다.

use super::*;

impl Agent {
    /// 새 세션: spec을 쓰고 session host를 분리된 프로세스로 띄운 뒤 붙는다.
    pub async fn spawn_session(self: &Arc<Self>, spec: SpawnSpec) -> Result<SessionInfo> {
        self.spawn_session_inner(spec, None).await
    }

    pub(super) async fn spawn_session_inner(
        self: &Arc<Self>,
        mut spec: SpawnSpec,
        credential: Option<aaa_proto::AccountCredential>,
    ) -> Result<SessionInfo> {
        if let Some(c) = &credential {
            aaa_common::redact::remember_auth(&c.harness, &c.secret.0);
        }

        let id = util::new_id();
        let paths = SessionPaths::new(&self.state, &id);
        util::ensure_private_dir(&paths.dir)
            .with_context(|| format!("pane 상태 폴더 준비: {}", paths.dir.display()))?;
        if spec.cwd.is_none() {
            spec.cwd = self.default_cwd.clone();
        }
        // 웹 새 세션 창은 `~/…`도 받는다(herdr-web-ui 「absolute path or ~/…」): 이 머신 사용자의 집으로 푼다
        if let Some(rest) = spec.cwd.as_deref().and_then(|c| c.strip_prefix('~'))
            && (rest.is_empty() || rest.starts_with('/'))
            && let Some(home) = std::env::var_os("HOME")
        {
            spec.cwd = Some(format!("{}{rest}", home.to_string_lossy()));
        }
        if let Some(cwd) = &spec.cwd
            && !Path::new(cwd).is_dir()
        {
            bail!("작업 폴더가 없다: {cwd}");
        }
        if let Some(cwd) = &spec.cwd {
            let cfg = self.threads_cfg.lock().unwrap().clone();
            cfg.check_path(Path::new(cwd))?;
        }
        // profile로 띄우기(웹 새 세션 창의 agent 고르기): thread처럼 profile을 argv·env로 풀고 상태 hook을 붙인다.
        // hook 설정 파일은 thread 작업 폴더 대신 이 세션 폴더에 둔다.
        if let Some(name) = spec.profile.clone() {
            // 새 host 정의 + 머신 덮어쓰기. 옛 host의 이름 기반 요청도 전환 기간에는 받는다.
            let cfg = self.threads_cfg.lock().unwrap().clone().fresh()?;
            let profile = match &spec.resolved_profile {
                Some(resolved) => threads::local_profile(&cfg, resolved)?,
                None => threads::find_profile(&cfg, &name)?.0,
            };
            let launch = threads::with_hooks(
                &cfg,
                threads::resolve(&name, &profile)?,
                &paths.dir,
                profile.command.len().max(1),
            )?;
            // thread처럼 profile harness의 화면으로 상태를 본다(host orch `harness: Some(launch.harness)`, plan §8.110)
            spec.harness.get_or_insert(launch.harness);
            spec.argv = launch.argv;
            spec.env.extend(launch.env);
            if spec.name.is_empty() {
                spec.name = name;
            }
        }
        if spec.profile.is_none()
            && let Some(resolved) = &spec.resolved_profile
        {
            // 저장 때 가린 env만 복원한다. 일반 env와 재개 argv는 종전 값이 유지된다.
            if credential.is_some() || spec.env.iter().any(|(_, value)| value.contains("********")) {
                let cfg = self.threads_cfg.lock().unwrap().clone().fresh()?;
                for (key, value) in threads::local_env(&cfg, resolved) {
                    if let Some((_, old)) =
                        spec.env.iter_mut().find(|(k, v)| k == &key && (credential.is_some() || v.contains("********")))
                    {
                        *old = value;
                    } else if credential.is_some() && !spec.env.iter().any(|(k, _)| k == &key) {
                        spec.env.push((key, value));
                    }
                }
            }
        }
        if spec.harness.as_deref() == Some("agy") && spec.role.is_some() {
            anyhow::ensure!(
                spec.role == Some(aaa_proto::Role::Worker) && credential.is_none(),
                "agy는 실행 머신의 개인 로그인으로 worker를 시작한다"
            );
            let home = aaa_config::paths::home();
            agy::check_local_settings(&home)?;
            anyhow::ensure!(
                self.threads_cfg.lock().unwrap().agy.is_some()
                    && agy::hooks_ready(&home, spec.cwd.as_deref().map(Path::new), &self.exe),
                "agy의 구독 신원이나 aaa hook 구성이 없다"
            );
            agy::check_model(&spec.argv).await?;
            #[cfg(target_os = "macos")]
            spec.env.push((agy::sandbox_progress::ENV.into(), "1".into()));
        }
        for (key, value) in &spec.env {
            aaa_common::redact::remember_env(key, value);
        }
        let version = if let Some(c) = &credential {
            let mut command = tokio::process::Command::new(spec.argv.first().map(String::as_str).unwrap_or(&c.harness));
            command.envs(spec.env.iter().cloned());
            if let Some(cwd) = &spec.cwd {
                command.current_dir(cwd);
            }
            Some(aaa_common::harness_version::observe(command, &c.harness).await)
        } else {
            None
        };
        // profile 없는 tab create와 저장된 Codex resume도 같은 pane 안에서 실행한다.
        if detect::Harness::of_session(None, &spec.argv) == Some(detect::Harness::Codex) {
            threads::ensure_codex_local(&mut spec.argv, 1);
        }
        if spec.name.is_empty() {
            spec.name = spec
                .argv
                .first()
                .map(|a| a.rsplit(['/', '\\']).next().unwrap_or(a).to_string())
                .unwrap_or_else(|| "shell".into());
        }
        if credential.as_ref().is_some_and(|c| c.harness == "claude" && c.auth_generation.is_some()) {
            let mut settings = hooks::claude_settings(&self.exe);
            // session의 계정 격리가 CLAUDE_CONFIG_DIR을 바꾸기 전에 로컬 사용자 홈을 읽는다.
            // 머신/profile의 덮어쓰기를 우선하며 권한·hooks·인증 등은 가져오지 않는다.
            let home = spec.env.iter().rev().find(|(key, _)| key == "CLAUDE_CONFIG_DIR").map_or_else(
                chat::claude_home,
                |(_, value)| {
                    if value.is_empty() { aaa_config::paths::home().join(".claude") } else { PathBuf::from(value) }
                },
            );
            match hooks::claude_advisor_model(&home) {
                Ok(Some(model)) => settings["advisorModel"] = model.into(),
                Ok(None) => {}
                Err(e) => tracing::warn!("Claude advisorModel 전달 실패: {e:#}"),
            }
            settings["statusLine"] = serde_json::json!({"type":"command","command":format!("'{}' hook --agent quota", self.exe.to_string_lossy().replace('\'', "'\\''"))});
            let path = paths.dir.join("quota-settings.json");
            util::write_json(&path, &settings)?;
            let path = path.to_string_lossy().to_string();
            if let Some(i) = spec.argv.iter().rposition(|a| a == "--settings").filter(|i| i + 1 < spec.argv.len()) {
                spec.argv[i + 1] = path;
            } else {
                spec.argv.splice(1..1, ["--settings".into(), path]);
            }
        }
        let token = hex::encode(rand::random::<[u8; 16]>());
        // 세션 안의 `aaa hook`·`aaa report`가 이 agent에 사건을 보낼 곳
        spec.env.push((hooks::ENV_SOCK.into(), hooks::socket(&self.state).to_string_lossy().to_string()));
        spec.env.push((hooks::ENV_TOKEN.into(), hooks::hook_token(&token)));
        let file = SpecFile {
            id: id.clone(),
            token,
            boot_id: self.boot_id.clone(),
            created_at_ms: util::now_ms(),
            spec,
            label: None,
            colors: *self.term_colors.lock().unwrap(),
            account_run: credential.as_ref().map(|c| c.run.clone()),
            account_generation: credential.as_ref().and_then(|c| c.auth_generation.clone()),
        };
        util::write_json(&paths.spec(), &file)
            .with_context(|| format!("pane 실행 명세 쓰기: {}", paths.spec().display()))?;
        let mut log = std::fs::File::create(paths.log())
            .with_context(|| format!("session host 로그 열기: {}", paths.log().display()))?;
        if let Some(version) = version {
            use std::io::Write;
            // thread의 pane ID로 찾는 시작 기록. 판 조회나 기록 실패가 실행을 막지 않는다.
            let _ = writeln!(log, "harness_version session={id} run={:?} {version}", file.account_run);
        }
        let mut cmd = std::process::Command::new(&self.exe);
        cmd.arg("session-host").arg("--dir").arg(&paths.dir);
        cmd.arg("--startup-stdin").stdin(std::process::Stdio::piped());
        cmd.stdout(log.try_clone()?).stderr(log);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const DETACHED_PROCESS: u32 = 0x0000_0008;
            const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
            cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
        }
        let mut child = cmd.spawn_guarded().context("session host 실행")?;
        {
            use std::io::Write;
            let startup = aaa_session::files::Startup { spec: file.spec.clone(), credential };
            let mut pipe = child.stdin.take().context("실행 전달 pipe가 없다")?;
            pipe.write_all(&serde_json::to_vec(&startup).map_err(|_| anyhow::anyhow!("실행 전달 형식 오류"))?)
                .context("실행 전달 pipe 오류")?;
        }
        let pid = child.id();
        // 좀비가 남지 않게 기다리는 thread. agent가 먼저 끝나면 OS 감독자가 거둔다.
        let (exit_tx, mut exit_rx) = tokio::sync::oneshot::channel();
        std::thread::spawn(move || {
            let _ = exit_tx.send(child.wait());
        });
        let mut startup_exit = None;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            if startup_exit.is_none() {
                startup_exit = exit_rx.try_recv().ok();
            }
            match self.attach(&paths, &file).await {
                Ok(info) => {
                    self.send_host(&LinkMsg::Event(AgentEvent::SessionStarted(info.clone()))).await;
                    return Ok(info);
                }
                Err(_) if paths.exit().exists() => {
                    // 붙기 전에 이미 끝났다: 종료한 세션으로 등록한다(마지막 화면은 파일에 있다).
                    let info = self.offline_info(&paths, &file);
                    self.sessions.lock().unwrap().insert(id.clone(), Entry { info: info.clone(), pump: None });
                    self.send_host(&LinkMsg::Event(AgentEvent::SessionStarted(info.clone()))).await;
                    return Ok(info);
                }
                Err(e) if tokio::time::Instant::now() < deadline && startup_exit.is_none() && util::pid_alive(pid) => {
                    tracing::trace!("session host 대기: {e:#}");
                    tokio::time::sleep(Duration::from_millis(40)).await;
                }
                Err(e) => {
                    if startup_exit.is_none() && !util::pid_alive(pid) {
                        startup_exit = exit_rx.await.ok();
                    }
                    let log = std::fs::read_to_string(paths.log()).unwrap_or_default();
                    bail!(
                        "session host가 준비되지 않았다: pid={pid} 종료={startup_exit:?} 상태 폴더={} 연결={e:#}\n{log}",
                        paths.dir.display()
                    );
                }
            }
        }
    }
}

//! 대화 기록 파일을 찾아 따라 읽는다.

use super::*;

/// 대화 기록 감시 간격(참고 conversation-monitor.ts `STAT_INTERVAL_MS`·`RESOLVE_INTERVAL_MS`·`MISSING_RESOLVE_INTERVAL_MS`)
pub(super) const CHAT_STAT_EVERY: Duration = Duration::from_millis(250);

pub(super) const CHAT_RESOLVE_EVERY: Duration = Duration::from_secs(5);

pub(super) const CHAT_MISSING_RESOLVE_EVERY: Duration = Duration::from_secs(2);

impl Agent {
    pub(super) fn chat_spec(&self, session: Option<&str>) -> Option<SpecFile> {
        session.filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric())).and_then(|s| {
            util::read_json::<SpecFile>(&SessionPaths::new(&self.state, s).spec())
                .inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                })
                .ok()
        })
    }

    pub(crate) fn codex_root_for(&self, session: Option<&str>) -> PathBuf {
        let spec = self.chat_spec(session);
        spec.as_ref()
            .and_then(|s| s.account_run.as_deref())
            .and_then(|r| {
                aaa_session::files::account_home(&self.state, r)
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
            })
            .or_else(|| {
                spec.as_ref()?
                    .spec
                    .env
                    .iter()
                    .rev()
                    .find_map(|(k, v)| (k == "CODEX_HOME" && Path::new(v).is_absolute()).then(|| PathBuf::from(v)))
            })
            .map(|h| h.join("sessions"))
            .unwrap_or_else(chat::codex_root)
    }

    pub(crate) fn claude_root_for(&self, session: Option<&str>) -> PathBuf {
        let run = session
            .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric()))
            .and_then(|s| {
                util::read_json::<SpecFile>(&SessionPaths::new(&self.state, s).spec())
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
            })
            .filter(|s| s.spec.harness.as_deref() == Some("claude"))
            .and_then(|s| s.account_run)
            .filter(|r| aaa_proto::account_run_parts(r).is_some_and(|(_, task)| aaa_proto::valid_thread(task)));
        run.and_then(|r| {
            aaa_session::files::account_home(&self.state, &r)
                .inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                })
                .ok()
        })
        .map(|h| h.with_file_name("claude-home").join("projects"))
        .filter(|h| h.parent().is_some_and(Path::is_dir))
        .unwrap_or_else(chat::claude_root)
    }

    pub(super) fn chat_path(
        &self,
        session: Option<&str>,
        harness: &str,
        transcript: Option<&str>,
        harness_session: Option<&str>,
    ) -> Result<PathBuf, chat::Error> {
        if harness == "codex" {
            let id = harness_session.ok_or("Codex thread-id가 없다")?;
            let root = self.codex_root_for(session);
            if let Some(path) = transcript.and_then(|p| chat::codex_hint(&root, id, p)) {
                return Ok(path);
            }
            if let Some(spec) = self.chat_spec(session)
                && spec.spec.harness.as_deref() == Some("codex")
                && spec
                    .account_run
                    .as_deref()
                    .and_then(aaa_proto::account_run_parts)
                    .is_some_and(|(_, task)| aaa_proto::valid_thread(task))
                && let Some(cwd) = spec.spec.cwd.as_deref()
            {
                let resume = spec.spec.argv.windows(2).any(|a| a[0] == "resume" && util::harness_session_ok(&a[1]));
                let started = if resume { 0 } else { spec.created_at_ms };
                chat::codex_account_file(&root, id, Path::new(cwd), started)
            } else {
                chat::codex_file(&root, id)
            }
        } else if harness == "claude" {
            let transcript = transcript.ok_or("Claude가 아직 기록 위치를 알리지 않았다(첫 hook 사건 전)")?;
            chat::inside(&self.claude_root_for(session), Path::new(transcript))
        } else {
            chat::path(harness, transcript, harness_session)
        }
    }

    /// 대화 기록 파일을 `CHAT_STAT_EVERY`마다 보고 바뀌면 host에 알린다(차이 목록 C3, herdr-web-ui `server/conversation-monitor.ts`
    /// `ConversationMonitor`). 켜면 지금 값을 한 번 보낸다. 경로는 찾았으면 `CHAT_RESOLVE_EVERY`, 아직 없으면
    /// `CHAT_MISSING_RESOLVE_EVERY`마다 다시 푼다(Codex 기록은 첫 턴 뒤에 생긴다). 파일이 사라지면 바로 다시 푼다
    pub(super) fn watch_chat(
        self: &Arc<Self>,
        key: String,
        harness: String,
        transcript: Option<String>,
        harness_session: Option<String>,
    ) {
        let me = self.clone();
        let k = key.clone();
        let task = tokio::spawn(async move {
            let mut observed: Option<String> = None;
            let mut path: Option<Result<PathBuf, chat::Error>> = None;
            let mut resolve_at = tokio::time::Instant::now();
            loop {
                let now = tokio::time::Instant::now();
                let resolve = now >= resolve_at;
                let (h, t, s, known) = (harness.clone(), transcript.clone(), harness_session.clone(), path.clone());
                let m = me.clone();
                let session = k.clone();
                let (found, seen) = tokio::task::spawn_blocking(move || {
                    let found = match known {
                        Some(p) if !resolve => p,
                        _ => m.chat_path(Some(&session), &h, t.as_deref(), s.as_deref()),
                    };
                    let seen = match &found {
                        Ok(p) => format!("{}\0{}", p.display(), chat::identity(p)),
                        Err(e) => format!("unavailable:{e}"),
                    };
                    (found, seen)
                })
                .await
                .unwrap_or_else(|e| (Err(e.to_string().into()), "unavailable".into()));
                if resolve {
                    resolve_at = now
                        + if found
                            .as_ref()
                            .inspect_err(|error| {
                                aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")
                            })
                            .is_ok()
                        {
                            CHAT_RESOLVE_EVERY
                        } else {
                            CHAT_MISSING_RESOLVE_EVERY
                        };
                }
                let missing = found
                    .as_ref()
                    .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
                    .is_ok()
                    && seen.ends_with("\0missing");
                path = Some(found);
                if observed.as_ref() != Some(&seen) {
                    if missing {
                        resolve_at = now;
                    }
                    let signature = format!("{:016x}", std::hash::BuildHasher::hash_one(&me.chat_salt, (&k, &seen)));
                    me.send_host(&LinkMsg::Event(AgentEvent::ChatChanged { key: k.clone(), signature })).await;
                    observed = Some(seen);
                }
                tokio::time::sleep(CHAT_STAT_EVERY).await;
            }
        });
        if let Some(old) = self.chat_watches.lock().unwrap().insert(key, task) {
            old.abort();
        }
    }
}

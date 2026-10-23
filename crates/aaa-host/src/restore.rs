//! 재부팅 뒤 자동 복원(herdr 0.9.5 `src/persist/restore.rs`·`src/app/agent_resume.rs`·`src/agent_resume.rs`, 차이 목록 4장 A).
//!
//! agent가 다른 boot ID로 붙으면 그 머신의 세션은 모두 `Interrupted`다(session host가 기록 없이 사라짐). host가 연결
//! 직후 그 pane들을 layout 차례로 되살린다. 한 host 실행에서 pane마다 한 번만 해 본다.
//! - **셸 pane**(argv가 비었거나 셸 하나): 저장한 cwd에서 같은 셸을 새로 띄워 layout 자리를 바꾼다(herdr: pane마다 저장한
//!   cwd에서 새 셸).
//! - 그 셸 안에서 Claude·Codex가 돌았고 harness 세션 ID를 알면 새 셸에 이어가기 명령을 친다(herdr
//!   `start_pending_agent_resume`: `claude --resume <id>`·`codex resume <id>` + Enter). harness를 바로 띄운 pane(argv 첫
//!   낱말이 claude·codex)은 원래 argv에서 대화 인자만 바꿔 띄운다(`resume_in_place`). aaa pane은 셸 아래가 아닐 수 있다.
//! - **thread pane**: `aaa thread restart --resume`과 같은 길(저장한 대화를 잇고 brief 안내는 없다). 못 하면 흐름 사건
//!   `restore_failed`와 inbox `session` 항목을 남긴다. 데려온 thread(`thread adopt`)는 다시 띄울 launch가 없어 사람이
//!   띄운 pane과 같은 길로 잇고, thread가 새 pane을 따라간다(`replace_pane`).
//! - **그 밖의 프로그램 pane**은 그대로 둔다(`aaa pane resume`으로 다시 띄운다, 까닭은 `restore_error`에 남겨 목록·열린 화면이 「다시 열기」와 함께 보인다, QA Q22). 명령을 다시 실행하지 않는다: 배포 같은
//!   명령이 재부팅마다 다시 돌면 안 된다. herdr pane은 늘 셸이라 이 경우가 없다.
//! - `[session] resume_agents_on_restore`(기본 켬, herdr와 같다. 결정 8-5 「꺼짐」을 결정 20 D12가 바꿨다)가 꺼지면
//!   이어가기만 하지 않는다: 셸은 되살리고 그 안의 agent는 잇지 않으며, harness pane과 thread는 중단으로 둔다.
//!   이어가기 사이에는 `startup_per_agent_delay_ms`(기본 100)를 둔다. 같은 harness 세션은 한 번만 잇는다(herdr `dedupe_key`).
//! - 다시 띄우지 못하면(저장한 폴더가 없어졌다 등) pane은 중단으로 남고 그 까닭을 `PaneView.restore_error`로 보인다
//!   (herdr `restore_error`).

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use aaa_proto::{AgentRequest, B64, BriefState, SessionState, SpawnSpec, ThreadKind, ThreadStatus};

use crate::Host;

/// 복원이 남기는 감사 기록의 행위자
const ACTOR: &str = "restore";

/// herdr `agent_resume::plan`(src/agent_resume.rs): harness 세션 ID로 대화를 이어 띄우는 명령. aaa harness는 둘이다.
/// 셸에 치는 줄이 되므로 UUID 꼴이 아닌 ID는 받지 않는다(herdr `validate_resume_argv`).
pub(crate) fn resume_argv(harness: &str, session: &str) -> Option<Vec<String>> {
    if !plain_id(session) {
        return None;
    }
    match harness {
        "claude" => Some(vec!["claude".into(), "--resume".into(), session.into()]),
        "codex" => Some(vec!["codex".into(), "resume".into(), session.into()]),
        _ => None,
    }
}

/// 셸에 치거나 인자로 넘겨도 안전한 harness 세션 ID: UUID 꼴만(herdr `validate_resume_argv`보다 좁다, 보안 감사 t-0340)
fn plain_id(session: &str) -> bool {
    aaa_common::util::harness_session_ok(session)
}

/// harness를 바로 띄운 pane: 원래 argv(실행 파일 경로, aaa hook 설정 `--settings`·`-c notify=…`, model 등)를 그대로 두고
/// 대화 인자만 이어가기로 바꾼다. herdr는 pane이 늘 셸이라 맨 명령(`resume_argv`)을 치지만, aaa pane은 harness 자체일
/// 수 있어 맨 명령으로 띄우면 hook 설정을 잃는다(상태가 보이지 않는다).
/// - claude: `--session-id <x>`·`--resume <x>`·`-r <x>`의 값을, `--continue`·`-c`는 `--resume <id>`로 바꾸고 없으면 붙인다.
/// - codex: `resume <x>`의 값을 바꾸고, 없으면 실행 파일 바로 뒤에 `resume <id>`를 넣는다(agent threads.rs `resolve`와 같은 꼴).
pub(crate) fn resume_in_place(argv: &[String], harness: &str, session: &str) -> Option<Vec<String>> {
    if !plain_id(session) || argv.is_empty() {
        return None;
    }
    let mut out = argv.to_vec();
    match harness {
        "claude" => {
            if let Some(i) = out.iter().position(|a| matches!(a.as_str(), "--session-id" | "--resume" | "-r")) {
                out[i] = "--resume".into();
                match out.get_mut(i + 1) {
                    Some(v) => *v = session.into(),
                    None => out.push(session.into()),
                }
            } else if let Some(i) = out.iter().position(|a| matches!(a.as_str(), "--continue" | "-c")) {
                out.splice(i..=i, ["--resume".to_string(), session.to_string()]);
            } else {
                out.extend(["--resume".to_string(), session.to_string()]);
            }
        }
        "codex" => match out.iter().position(|a| a == "resume") {
            Some(i) => match out.get_mut(i + 1) {
                Some(v) if !v.starts_with('-') => *v = session.into(),
                _ => out.insert(i + 1, session.into()),
            },
            None => out.splice(1..1, ["resume".to_string(), session.to_string()]).for_each(drop),
        },
        _ => return None,
    }
    Some(out)
}

/// 실행 파일 이름(경로·`.exe` 뺌, 소문자)
fn program(argv: &[String]) -> Option<String> {
    let first = argv.first()?;
    let name = first.rsplit(['/', '\\']).next().unwrap_or(first).to_ascii_lowercase();
    Some(name.strip_suffix(".exe").map(str::to_string).unwrap_or(name))
}

/// 셸 하나를 띄운 pane인가: argv가 비었거나(그 머신 기본 셸) 셸 이름 뒤에 선택지(`-l` 등)만 있다. `-c`는 명령이다.
pub(crate) fn is_shell(argv: &[String]) -> bool {
    const SHELLS: &[&str] = &[
        "sh",
        "bash",
        "zsh",
        "fish",
        "dash",
        "ksh",
        "mksh",
        "tcsh",
        "csh",
        "nu",
        "elvish",
        "xonsh",
        "pwsh",
        "powershell",
        "cmd",
    ];
    match program(argv) {
        None => true,
        Some(p) => SHELLS.contains(&p.as_str()) && argv[1..].iter().all(|a| a.starts_with('-') && a != "-c"),
    }
}

impl Host {
    /// agent가 연결된 직후(agent_connected): 이 머신의 중단된(재부팅) pane을 layout 차례로 되살린다. 한 host 실행에서
    /// pane마다 한 번만 한다. 도는 동안 ticker는 이 머신 thread의 묶음을 세지 않는다(사라졌다가 돌아온 것으로 보지 않게).
    pub(crate) fn restore_after_reboot(self: &Arc<Self>, mid: &str, generation: u64) {
        let ids = self.restore_order(mid);
        if ids.is_empty() {
            self.restoring.lock().unwrap().remove(mid);
            return;
        }
        self.restoring.lock().unwrap().insert(mid.to_string());
        let me = self.clone();
        let mid = mid.to_string();
        tokio::spawn(async move {
            me.restore_all(&mid, generation, ids).await;
            me.restoring.lock().unwrap().remove(&mid);
            me.bump();
            me.tick_wake.notify_one();
        });
    }

    /// 되살릴 pane: layout 차례(workspace·탭·나눈 차례), 그다음 layout 밖(thread·역할 pane)은 시작한 차례
    fn restore_order(&self, mid: &str) -> Vec<String> {
        let mut tried = self.restore_tried.lock().unwrap();
        let mut ids: Vec<(u64, String)> = {
            let i = self.inner.lock().unwrap();
            i.panes
                .iter()
                .filter(|(id, p)| {
                    p.machine_id == mid && p.info.state == SessionState::Interrupted && !tried.contains(*id)
                })
                .map(|(id, p)| (p.info.started_at_ms, id.clone()))
                .collect()
        };
        ids.sort();
        let laid: Vec<String> = {
            let ws = self.workspaces.lock().unwrap();
            ws.values().flat_map(|w| w.tabs.iter().flat_map(|t| aaa_proto::layout::leaves(&t.root))).collect()
        };
        let mut order: Vec<String> = laid.into_iter().filter(|l| ids.iter().any(|(_, id)| id == l)).collect();
        for (_, id) in ids {
            if !order.contains(&id) {
                order.push(id);
            }
        }
        tried.extend(order.iter().cloned());
        order
    }

    async fn restore_all(self: &Arc<Self>, mid: &str, generation: u64, ids: Vec<String>) {
        let resume_agents = self.cfg.session.resume_agents_on_restore;
        let spacing = Duration::from_millis(self.cfg.session.startup_per_agent_delay_ms.into());
        // 이은 harness 세션(herdr `dedupe_key`): 두 pane이 같은 대화를 가리키면 앞의 것만 잇는다
        let mut resumed: HashSet<(String, String)> = HashSet::new();
        for id in ids {
            // 연결이 바뀌었다(끊겼다 다시 붙음): 남은 것은 다음 연결이 이어 한다
            if self.conn_of(mid).map(|c| c.generation) != Some(generation) {
                self.restore_tried.lock().unwrap().remove(&id);
                continue;
            }
            // herdr `start_pending_agent_resumes`: 이어가기를 하나 시작하면 다음 것까지 간격을 둔다
            if self.restore_one(mid, &id, resume_agents, &mut resumed).await && !spacing.is_zero() {
                tokio::time::sleep(spacing).await;
            }
        }
    }

    /// pane 하나. 대화를 이었으면 true.
    async fn restore_one(
        self: &Arc<Self>,
        mid: &str,
        id: &str,
        resume_agents: bool,
        resumed: &mut HashSet<(String, String)>,
    ) -> bool {
        let Some((info, agent)) = ({
            let i = self.inner.lock().unwrap();
            i.panes.get(id).map(|p| (p.info.clone(), p.agent.clone()))
        }) else {
            return false;
        };
        // 그사이 사람이 다시 띄웠거나 지웠다
        if info.state != SessionState::Interrupted {
            return false;
        }
        let key = agent
            .as_ref()
            .filter(|_| resume_agents)
            .and_then(|a| Some((a.harness.clone(), a.harness_session.clone()?)))
            .filter(|k| !resumed.contains(k));
        // 데려온 thread(`thread adopt`)는 다시 띄울 수 없다: 그 pane은 사람이 띄운 pane처럼 잇고, thread가 새 pane을 따라간다
        if let Some(rec) =
            self.thread_of_pane(id).filter(|r| r.status == ThreadStatus::Open && r.kind != ThreadKind::Adopted)
        {
            if !resume_agents {
                self.leave_stopped(
                    id,
                    &format!(
                        "재부팅 뒤 이어가기를 꺼 두어 다시 띄우지 않았다(host.toml [session] resume_agents_on_restore). `aaa thread restart {} {}`가 띄운다",
                        rec.project, rec.id
                    ),
                );
                return false;
            }
            // brief를 받기 전이었으면 이을 대화가 없다: 새로 띄우고 brief를 다시 보낸다(`thread restart`와 같다).
            // 받은 뒤라면 대화를 잇는다. 대화 ID로 잇지 못하는 harness(custom 등)는 같은 명령을 다시 돌리면 일하던 것을
            // 잃은 채 기다리므로 사람에게 넘긴다.
            let resume = rec.brief == BriefState::Delivered;
            let resumable = rec.launch.as_ref().is_some_and(|l| l.resume_argv.iter().any(|a| a.contains("{session}")));
            let restarted = if resume && !resumable {
                Err(aaa_proto::ErrorBody::new(
                    aaa_proto::ErrorCode::BadRequest,
                    "이 harness는 대화를 이어 띄우지 못한다",
                ))
            } else {
                self.thread_restart(&rec.project, &rec.id, resume, None, ACTOR).await
            };
            return match restarted {
                Ok(v) => {
                    let new = v.pane.clone().unwrap_or_default();
                    let rec = self.load(&rec.project, &rec.id).unwrap_or(rec);
                    let how = if resume { "대화를 이어 띄웠다" } else { "brief 전이라 새로 띄웠다" };
                    self.flow(&rec, "session_restored", None, format!("재부팅 뒤 {how}: {id} → {new}"));
                    if let Some(k) = key {
                        resumed.insert(k);
                    }
                    true
                }
                Err(e) => {
                    let why = format!("재부팅 뒤 대화를 이어 띄우지 못했다: {}", e.message);
                    self.set_restore_error(id, &why);
                    self.flow(&rec, "restore_failed", None, e.message.clone());
                    self.inbox_add(
                        &rec.project,
                        "session",
                        &self.machine_name(mid),
                        "restore failed",
                        &format!(
                            "{}: {why}. `aaa thread restart {} {}`가 새 대화로 띄운다",
                            rec.id, rec.project, rec.id
                        ),
                    );
                    false
                }
            };
        }
        // 셸이면 같은 셸을 띄우고 이어가기 명령을 친다. harness를 바로 띄운 pane은 원래 argv에서 대화 인자만 바꿔 띄운다.
        let shell = is_shell(&info.argv);
        let plan = key.as_ref().and_then(|(h, s)| {
            if shell {
                resume_argv(h, s)
            } else if program(&info.argv).as_deref() == Some(h.as_str())
                || self
                    .pane_account(id)
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
                    .flatten()
                    .is_some_and(|r| r.harness == *h)
            {
                resume_in_place(&info.argv, h, s)
            } else {
                None
            }
        });
        let (argv, typed) = match (&plan, shell) {
            (_, true) => (info.argv.clone(), plan.clone()),
            (Some(p), false) => (p.clone(), None),
            (None, false) => {
                // 일부러 남긴 pane도 까닭을 보인다(QA Q22): 목록·열린 화면이 「다시 열기」와 함께 보인다
                let harness = agent.is_some() || matches!(program(&info.argv).as_deref(), Some("claude" | "codex"));
                let why = if harness && !resume_agents {
                    "재부팅 뒤 이어가기를 꺼 두어 다시 띄우지 않았다(host.toml [session] resume_agents_on_restore). 다시 열면 같은 명령으로 띄운다"
                } else if harness {
                    "이을 대화 ID를 몰라 다시 띄우지 않았다. 다시 열면 같은 명령으로 새 대화를 띄운다"
                } else {
                    "재부팅 뒤 다시 실행하지 않았다(배포 같은 명령이 재부팅마다 다시 돌지 않게). 다시 열면 같은 명령으로 띄운다"
                };
                self.leave_stopped(id, why);
                return false;
            }
        };
        let spec = SpawnSpec {
            name: crate::respawn_name(&info),
            argv,
            cwd: info.cwd.clone(),
            env: vec![],
            cols: info.cols,
            rows: info.rows,
            role: info.role,
            harness: info.harness.clone(),
            // argv는 처음 띄울 때 profile을 풀어 hook까지 붙인 그 명령이다: 다시 풀지 않는다
            profile: None,
            resolved_profile: None,
        };
        let new = match self.respawn(id, mid, spec, ACTOR).await {
            Ok(new) => new,
            Err(e) => {
                self.set_restore_error(id, &format!("다시 띄우지 못했다: {}", e.message));
                return false;
            }
        };
        if let Some(line) = typed {
            // herdr: 셸에 명령 한 줄과 CR을 친다(셸이 읽기 전이면 PTY가 담아 둔다)
            let mut bytes = shell_line(&line).into_bytes();
            bytes.push(b'\r');
            if let Ok(conn) = self.live_conn(mid) {
                aaa_common::diagnostic::ignored(
                    conn.request(AgentRequest::Input { session: new.clone(), data: B64(bytes) }).await,
                    "보조 작업 실패 뒤 기존 처리를 계속한다",
                );
            }
        }
        match (plan, key) {
            (Some(_), Some(k)) => {
                resumed.insert(k);
                true
            }
            _ => false,
        }
    }

    fn set_restore_error(&self, id: &str, why: &str) {
        self.note_restore(id, why);
        tracing::warn!(pane = %id, "{why}");
    }

    /// 일부러 다시 띄우지 않은 pane(실패가 아니다): 까닭은 같은 자리(`restore_error`)에 둔다
    fn leave_stopped(&self, id: &str, why: &str) {
        self.note_restore(id, why);
        tracing::info!(pane = %id, "{why}");
    }

    fn note_restore(&self, id: &str, why: &str) {
        if let Some(p) = self.inner.lock().unwrap().panes.get_mut(id) {
            p.restore_error = Some(why.to_string());
            aaa_common::diagnostic::ignored(
                p.events.send(crate::PaneEvent::State),
                "수신자가 끝난 뒤에도 현재 작업은 마친다",
            );
        }
        self.bump();
    }
}

/// herdr `shell_command_from_argv`: 셸에 칠 한 줄. 이어가기 argv는 `resume_argv`가 거른 낱말이라 따옴표가 필요 없다.
fn shell_line(argv: &[String]) -> String {
    argv.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "0f5b2c1e-7a3d-4c8e-9b21-5d6e7f809a1b";
    const B: &str = "019a4c5e-1f2b-7c3d-8e4f-a5b6c7d8e9f0";

    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn resume_command_like_herdr_plan() {
        assert_eq!(resume_argv("claude", A), Some(v(&["claude", "--resume", A])));
        assert_eq!(resume_argv("codex", B), Some(v(&["codex", "resume", B])));
        // 셸에 칠 줄이 되는 ID는 거른다(UUID 꼴만, herdr `validate_resume_argv`보다 좁다)
        for bad in ["", "-x", "abc-1", "a b", "a;rm", "$(id)", "--dangerously-skip-permissions"] {
            assert_eq!(resume_argv("claude", bad), None, "{bad:?}");
        }
        assert_eq!(resume_argv("custom", A), None);
    }

    #[test]
    fn shell_is_a_bare_shell_with_flags() {
        assert!(is_shell(&[]));
        assert!(is_shell(&v(&["/bin/zsh"])));
        assert!(is_shell(&v(&["bash", "-l"])));
        assert!(is_shell(&v(&["C:\\Windows\\System32\\cmd.exe"])));
        assert!(!is_shell(&v(&["/bin/sh", "-c", "make deploy"])));
        assert!(!is_shell(&v(&["bash", "script.sh"])));
        assert!(!is_shell(&v(&["claude", "--resume", "x"])));
        assert_eq!(program(&v(&["/opt/bin/Claude.exe"])).as_deref(), Some("claude"));
    }

    #[test]
    fn direct_harness_keeps_its_argv_and_swaps_the_conversation() {
        let r = |a: &[&str], h: &str| resume_in_place(&v(a), h, A);
        // 경로·hook 설정·model은 그대로, 대화 인자만
        assert_eq!(
            r(&["/opt/claude", "--settings", "/w/s.json", "--model", "m", "--session-id", "k0"], "claude"),
            Some(v(&["/opt/claude", "--settings", "/w/s.json", "--model", "m", "--resume", A]))
        );
        assert_eq!(r(&["claude", "-r", "old"], "claude"), Some(v(&["claude", "--resume", A])));
        assert_eq!(
            r(&["claude", "--continue", "--model", "m"], "claude"),
            Some(v(&["claude", "--resume", A, "--model", "m"]))
        );
        assert_eq!(r(&["claude", "--model", "m"], "claude"), Some(v(&["claude", "--model", "m", "--resume", A])));
        assert_eq!(
            r(&["codex", "-c", "notify=x", "-m", "g"], "codex"),
            Some(v(&["codex", "resume", A, "-c", "notify=x", "-m", "g"]))
        );
        assert_eq!(r(&["codex", "resume", "old", "-m", "g"], "codex"), Some(v(&["codex", "resume", A, "-m", "g"])));
        assert_eq!(resume_in_place(&v(&["claude"]), "claude", "--dangerously-skip-permissions"), None);
        assert_eq!(r(&["sh"], "custom"), None);
    }
}

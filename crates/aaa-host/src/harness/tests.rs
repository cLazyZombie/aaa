use super::*;

fn lines(t: &str) -> Vec<String> {
    t.lines().map(|l| l.to_string()).collect()
}

#[test]
fn limit_wait_screen_requires_active_wait_controls() {
    let banner = "Claude Code will continue automatically at 12:30pm.\nPress esc to cancel the wait.";
    assert!(limit_wait_screen(&lines(banner)));
    assert!(!limit_wait_screen(&lines(&format!("{banner}\n────────────────\n❯ \n────────────────"))));
    assert!(!limit_wait_screen(&lines("Do you want to continue?\n❯ Yes\n  No\nEsc to cancel")));
    assert!(!limit_wait_screen(&lines("You've hit your usage limit.\n› ")));
}

/// Claude Code 2.1의 실제 폴더 신뢰 창(경로만 바꿨다). 기본 선택이 「No, exit」다.
const CLAUDE_TRUST: &str = "\
 Accessing workspace:

 /work/demo/t-0001

 Quick safety check: Is this a project you created or one you trust? (Like your own code, a well-known open source
 project, or work from your team). If not, take a moment to review what's in this folder first.

 Claude Code'll be able to read, edit, and execute files here.

 Security guide

 ❯ No, exit
   Yes, I trust this folder

 Enter to confirm · Esc to cancel";

#[test]
fn claude_trust_screen() {
    let l = lines(CLAUDE_TRUST);
    let (kind, text) = modal_of(&l).unwrap();
    assert_eq!(kind, "trust");
    assert!(text.starts_with("Quick safety check: Is this a project"), "{text}");
    assert_eq!(answer_keys(&l, "yes").unwrap(), b"\x1b[B\r");
    assert_eq!(answer_keys(&l, "no").unwrap(), b"\x1b");
}

/// Codex 0.163.0 폴더 신뢰 창. 의미 답은 현재 커서에서 실제 선택지로 옮겨야 한다.
#[test]
fn codex_trust_screen() {
    let screen = include_str!("../../../aaa-agent/src/detect/fixtures/codex-trust.txt");
    for (screen, yes, no) in [
        (screen.to_string(), b"\r".as_slice(), b"\x1b[B\r".as_slice()),
        (screen.replace("› 1.", "  1.").replace("  2.", "› 2."), b"\x1b[A\r".as_slice(), b"\r".as_slice()),
    ] {
        let l = lines(&screen);
        assert_eq!(modal_of(&l).unwrap().0, "trust");
        assert_eq!(answer_keys(&l, "yes").unwrap(), yes);
        assert_eq!(answer_keys(&l, "no").unwrap(), no);
    }
}

/// Claude Code 2.1의 실제 권한 창(기본 선택 Yes, 번호 붙음, 「Esc to cancel」만 있다)
#[test]
fn claude_permission_screen() {
    let l = lines(
        "\
 Do you want to create report.md?
 ❯ 1. Yes
   2. Yes, and switch to accept edits (auto-approve file edits and common file commands) for this session (shift+tab)
   3. No
 Esc to cancel · Tab to amend",
    );
    assert_eq!(modal_of(&l).unwrap(), ("confirm", "Do you want to create report.md?".to_string()));
    assert_eq!(answer_keys(&l, "yes").unwrap(), b"\r");
}

/// 위쪽 출력(소스 cat 등)에 선택 창 글이 있어도, 맨 아래가 평소 입력 상자면 선택 창이 아니다
#[test]
fn dialog_text_in_scrollback_is_not_a_modal() {
    let l = lines(
        "\
⏺ Bash(cat harness.rs)
  const MODAL_MARKS = [\"Enter to confirm\", \"Esc to cancel\"];
   ❯ 1. Yes
     2. No
 Esc to cancel · Tab to amend
⏺ 위 파일을 읽었다.
✻ Brewed for 2s
────────────────────────────────
❯
────────────────────────────────
  Haiku 5.5 medium | ctx 95% left
  ⏸ manual mode on

",
    );
    assert!(modal_of(&l).is_none());
    assert!(answer_keys(&l, "yes").is_err());
}

/// Codex 0.163의 실제 명령 승인 창(경로만 바꿨다). 선택 커서는 `›`, 맨 아래 줄은 소문자 「enter」·「esc」다.
#[test]
fn codex_approval_screen() {
    let l = lines(
        "\
› .aaa/demo-t-0001/brief.md를 읽고 그대로 하라.

• Failed (exit 1) touch probe.txt
  └ touch: probe.txt: Operation not permitted

• Running touch probe.txt

  Would you like to run the following command?

  Environment: local

  $ touch probe.txt

› 1. Yes, proceed (y)
  2. Yes, and don't ask again for commands that start with `touch probe.txt` (p)
  3. No, and tell Codex what to do differently (esc)

  Press enter to confirm or esc to cancel
",
    );
    assert_eq!(modal_of(&l).unwrap(), ("confirm", "Would you like to run the following command?".to_string()));
    assert_eq!(answer_keys(&l, "yes").unwrap(), b"\r");
    assert_eq!(answer_keys(&l, "no").unwrap(), b"\x1b");
}

#[test]
fn numbered_menu_and_missing_menu() {
    let l = lines(
        "Do you want to proceed?\n  1. Yes\n  2. Yes, and don't ask again\n❯ 3. No, and tell Claude\nEsc to cancel\n",
    );
    assert_eq!(answer_keys(&l, "yes").unwrap(), b"\x1b[A\x1b[A\r");
    let l = lines("Do you want to proceed?\n❯ 1. Yes\n  2. No\nEsc to cancel\n");
    assert_eq!(answer_keys(&l, "yes").unwrap(), b"\r");
    // 선택 창이 없으면 Enter를 보내지 않는다
    assert!(answer_keys(&lines("fake> \nNo idea"), "yes").is_err());
    assert!(modal_of(&lines("fake> got: x")).is_none());
    assert!(answer_keys(&[], "maybe").is_err());
}

/// agent가 다시 붙으며 보낸 옛 기억(옛 agent의 `turn_start`)이 먼저 도착한 화면 감지 idle을 덮지 않는다(t-0334 결함 1)
#[test]
fn older_events_do_not_change_the_activity() {
    let ev = |kind: &str, at_ms| HarnessEvent {
        session: "s".into(),
        harness: "claude".into(),
        kind: kind.into(),
        harness_session: None,
        message: None,
        question_kind: None,
        percent: None,
        at_ms,
        transcript: None,
        plan: None,
        background: None,
        context: None,
        model_observation: None,
        codex_speed: None,
        provider_turn: None,
        turn_reply: None,
    };
    let idle = AgentStatus {
        activity: AgentActivity::Idle,
        since_ms: 2_000,
        harness: "claude".into(),
        message: None,
        percent: None,
        progress: None,
        progress_at_ms: None,
        progress_since_ms: None,
        harness_session: None,
        seen: true,
        plan: None,
        background: None,
        background_wait: false,
        compacting: false,
        context: None,
        model_observation: None,
        provider_turn: None,
    };
    assert_eq!(next_activity(Some(&idle), &ev("turn_start", 1_000)), None);
    assert_eq!(next_activity(Some(&idle), &ev("working", 2_000)), Some(AgentActivity::Working));
    // host가 처음 듣는 pane(host 재시작)은 옛 기억으로 상태를 정한다
    assert_eq!(next_activity(None, &ev("turn_start", 1_000)), Some(AgentActivity::Working));
    assert_eq!(next_activity(None, &ev("session_start", 1_000)), Some(AgentActivity::Started));
    assert_eq!(next_activity(Some(&idle), &ev("session_start", 3_000)), None);
}

#[test]
fn discovered_session_uses_the_agents_execution_generation_not_discovery_time() {
    let (h, _tmp) = host_with_pane();
    let id = "00000000-0000-4000-8000-000000000001";
    let native = aaa_proto::ProviderTurn {
        generation_ms: 100,
        harness_session: id.into(),
        started_ms: 150,
        at_ms: 180,
        finished_ms: Some(180),
        error: Some(aaa_proto::ProviderFailure {
            kind: aaa_proto::ProviderFailureKind::Authentication,
            code: Some("authentication_failed".into()),
            http_status: None,
        }),
    };
    let mut event = codex_event("session_start", 200);
    event.harness_session = Some(id.into());
    event.provider_turn =
        Some(aaa_proto::ProviderTurn { started_ms: 100, at_ms: 200, finished_ms: None, error: None, ..native.clone() });
    h.on_harness("m", event.clone());
    event.kind = "provider_turn".into();
    event.at_ms = 180;
    event.provider_turn = Some(native.clone());
    h.on_harness("m", event);
    assert_eq!(h.pane_agent("s").unwrap().provider_turn, Some(native));
}

#[test]
fn native_errors_are_bound_to_machine_session_generation_and_latest_turn() {
    let (h, _tmp) = host_with_pane();
    let id = "00000000-0000-4000-8000-000000000001";
    let mut event = codex_event("session_start", 100);
    event.harness = "claude".into();
    event.harness_session = Some(id.into());
    h.on_harness("m", event.clone());
    let original = h.pane_agent("s").unwrap();
    let native = |generation, start, at, error: bool| aaa_proto::ProviderTurn {
        generation_ms: generation,
        harness_session: id.into(),
        started_ms: start,
        at_ms: at,
        finished_ms: error.then_some(at),
        error: error.then_some(aaa_proto::ProviderFailure {
            kind: aaa_proto::ProviderFailureKind::Overloaded,
            code: Some("overloaded".into()),
            http_status: None,
        }),
    };
    event.kind = "provider_turn".into();
    event.at_ms = 200;
    event.provider_turn = Some(native(100, 150, 200, true));
    h.on_harness("other-machine", event.clone());
    assert_eq!(h.pane_agent("s").unwrap(), original);
    let mut wrong = event.clone();
    wrong.provider_turn.as_mut().unwrap().harness_session = "00000000-0000-4000-8000-000000000002".into();
    h.on_harness("m", wrong);
    assert_eq!(h.pane_agent("s").unwrap(), original);
    h.on_harness("m", event.clone());
    let failed = h.pane_agent("s").unwrap();
    assert_eq!(failed.activity, original.activity);
    assert_eq!(failed.message, original.message);
    assert!(failed.provider_turn.as_ref().unwrap().error.is_some());
    let mut next = event.clone();
    next.at_ms = 300;
    next.provider_turn = Some(native(100, 300, 300, false));
    h.on_harness("m", next.clone());
    h.on_harness("m", event.clone()); // 옛 완료의 재전송
    assert_eq!(h.pane_agent("s").unwrap().provider_turn, next.provider_turn);
    next.kind = "prompt_submit".into();
    next.at_ms = 400;
    next.provider_turn = None;
    h.on_harness("m", next.clone());
    event.at_ms = 350;
    event.provider_turn = Some(native(100, 300, 350, true));
    h.on_harness("m", event.clone());
    assert!(h.pane_agent("s").unwrap().provider_turn.unwrap().error.is_none());
    event.at_ms = 400;
    event.provider_turn = Some(native(100, 300, 400, true));
    h.on_harness("m", event.clone()); // 제출과 같은 ms의 과거 완료도 버린다.
    assert!(h.pane_agent("s").unwrap().provider_turn.unwrap().error.is_none());
    // native 시작이 hook 제출보다 약간 빨라도 완료가 새것이면 받는다.
    event.at_ms = 450;
    event.provider_turn = Some(native(100, 390, 450, true));
    h.on_harness("m", event.clone());
    assert!(h.pane_agent("s").unwrap().provider_turn.unwrap().error.is_some());
    next.kind = "session_start".into();
    next.at_ms = 500;
    h.on_harness("m", next);
    event.at_ms = 600;
    event.provider_turn = Some(native(100, 550, 600, true));
    h.on_harness("m", event); // 같은 ID로 재개한 옛 실행도 거른다.
    assert!(h.pane_agent("s").unwrap().provider_turn.unwrap().error.is_none());
}

struct Tmp(std::path::PathBuf);
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 머신 `m`의 Claude pane `s` 하나가 있는 host(agent 연결 없음)
fn host_with_pane() -> (Arc<Host>, Tmp) {
    let dir = std::env::temp_dir().join(format!("aaa-harness-test-{}", aaa_common::util::new_id()));
    let mut cfg = aaa_config::HostConfig::default();
    cfg.notify.web_push = false;
    cfg.projects.root = dir.join("projects").display().to_string();
    let h = Host::open(cfg, dir.clone()).unwrap();
    let info = aaa_proto::SessionInfo {
        responsibility: None,
        id: "s".into(),
        name: "claude".into(),
        argv: vec!["claude".into()],
        cwd: None,
        pid: None,
        started_at_ms: 0,
        boot_id: "b".into(),
        cols: 80,
        rows: 24,
        state: aaa_proto::SessionState::Running,
        role: None,
        label: None,
        harness: Some("claude".into()),
    };
    h.on_session("m", info, false);
    (h, Tmp(dir))
}

fn codex_event(kind: &str, at_ms: u64) -> HarnessEvent {
    HarnessEvent {
        session: "s".into(),
        harness: "codex".into(),
        kind: kind.into(),
        at_ms,
        harness_session: None,
        message: None,
        question_kind: None,
        percent: None,
        transcript: None,
        plan: None,
        background: None,
        context: None,
        model_observation: None,
        codex_speed: None,
        provider_turn: None,
        turn_reply: None,
    }
}

#[test]
fn agy_brief_receipt_requires_current_pane_conversation_and_matching_input() {
    let (h, _tmp) = host_with_pane();
    let mut info = h.inner.lock().unwrap().panes["s"].info.clone();
    info.harness = Some("agy".into());
    info.started_at_ms = 100;
    h.on_session("m", info, false);
    let root = "00000000-0000-4000-8000-000000000001";
    let start =
        HarnessEvent { harness: "agy".into(), harness_session: Some(root.into()), ..codex_event("session_start", 100) };
    h.on_harness("m", start);
    let mut rec =
        ThreadRec::starting("demo", "agy", "m", "gemini", aaa_proto::Role::Worker, aaa_proto::ThreadKind::Dir, 100);
    rec.id = "t-0001".into();
    rec.pane = Some("s".into());
    rec.launch = Some(aaa_proto::Launch {
        profile: "gemini".into(),
        harness: "agy".into(),
        argv: Vec::new(),
        env: Vec::new(),
        resume_argv: Vec::new(),
        session_key: None,
    });
    rec.status = ThreadStatus::Open;
    rec.brief = BriefState::Unknown;
    rec.brief_attempts = 1;
    h.db.thread_import("demo", &rec.id, "open", &serde_json::to_string(&rec).unwrap()).unwrap();
    let saved = || h.load("demo", &rec.id).unwrap();
    let receipt = HarnessEvent {
        harness: "agy".into(),
        harness_session: Some(root.into()),
        message: Some(crate::prompt_receipt::receipt_hash(".aaa/demo-t-0001/brief.md를 읽고 그대로 하라.")),
        ..codex_event("agy_receipt", 110)
    };
    h.on_harness("other", receipt.clone());
    h.on_harness("m", HarnessEvent { at_ms: 99, ..receipt.clone() });
    h.on_harness("m", HarnessEvent { harness_session: None, ..receipt.clone() });
    h.on_harness("m", HarnessEvent { message: Some("draft".into()), ..receipt.clone() });
    assert_eq!(saved().brief, BriefState::Unknown);
    h.on_harness("m", receipt.clone());
    assert_eq!(saved().brief, BriefState::Delivered);
    let delivery = saved().delivered_at_ms;
    assert!(delivery.is_some());
    h.on_harness("m", receipt);
    assert_eq!(saved().delivered_at_ms, delivery);
    assert_eq!(saved().brief_attempts, 1);
}

#[test]
fn codex_thread_speed_uses_launch_and_updates_without_model_observation() {
    use aaa_proto::CodexSpeed;
    let (h, _tmp) = host_with_pane();
    let mut info = h.inner.lock().unwrap().panes["s"].info.clone();
    info.harness = Some("codex".into());
    info.argv = [
        "codex",
        "-c",
        "service_tier=priority",
        "resume",
        "example-session",
        "--config=service_tier='ultrafast'",
        "--",
        "-cservice_tier=default",
    ]
    .map(str::to_owned)
    .into();
    h.on_session("m", info.clone(), false);
    let speed = || h.inner.lock().unwrap().panes["s"].thread_speed();
    let observe = |tier, at_ms| {
        h.on_harness("m", HarnessEvent { codex_speed: Some(tier), ..codex_event("codex_speed", at_ms) });
    };
    assert_eq!(speed().as_deref(), Some("ultrafast"));
    observe(CodexSpeed::Standard, 2_000);
    observe(CodexSpeed::Fast, 1_000);
    assert_eq!(speed(), None);
    observe(CodexSpeed::Fast, 3_000);
    assert_eq!(speed().as_deref(), Some("fast"));
    assert!(h.pane_agent("s").is_none());
    info.state = aaa_proto::SessionState::Lost;
    h.on_session("m", info, false);
    assert_eq!(speed(), None);
}

fn submission_machine(h: &Arc<Host>, conn: crate::AgentConn, os: &str, features: Vec<String>) {
    h.inner.lock().unwrap().machines.insert(
        "m".into(),
        crate::MachineRt {
            name: "mac-a".into(),
            local: true,
            os: Some(os.into()),
            boot_id: None,
            dev_mode: true,
            conn: Some(conn),
            last_seen_ms: None,
            health: None,
            health_sent: None,
            features,
            identity: None,
            agent_pid: None,
            colors: None,
            build: Default::default(),
        },
    );
}

#[tokio::test(start_paused = true)]
async fn card_answers_wait_for_the_same_report_and_pane_and_start_input_once() {
    use aaa_proto::{AgentResponse, AnswerDelivery, LinkMsg, Outcome};

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Case {
        Resume,
        Unconfirmed,
        NewReport,
        ChangedOnAgent,
        Closed,
        Replaced,
        NewExecution,
        ClaimedBeforeRestart,
        Partial,
    }

    for case in [
        Case::Resume,
        Case::Unconfirmed,
        Case::NewReport,
        Case::ChangedOnAgent,
        Case::Closed,
        Case::Replaced,
        Case::NewExecution,
        Case::ClaimedBeforeRestart,
        Case::Partial,
    ] {
        let (h, _lab) = host_with_pane();
        h.project_create("demo", Default::default(), "test").unwrap();
        let mut rec = ThreadRec::starting(
            "demo",
            "한도 대기",
            "m",
            "claude",
            aaa_proto::Role::Worker,
            aaa_proto::ThreadKind::Dir,
            1,
        );
        rec.id = "t-0001".into();
        rec.pane = Some("s".into());
        rec.work_dir = Some("fixture".into());
        rec.brief = BriefState::Delivered;
        rec.status = ThreadStatus::Open;
        rec.report_hash = Some("report-a".into());
        rec.questions = aaa_proto::report_questions("## Questions\n### 1. 자리\n- A 유지\n- B 이동\n");
        h.db.thread_import("demo", &rec.id, "open", &serde_json::to_string(&rec).unwrap()).unwrap();
        h.on_harness(
            "m",
            HarnessEvent {
                harness: "claude".into(),
                ..codex_event(if case == Case::Partial { "idle" } else { "question" }, 1)
            },
        );

        let (outgoing, mut frames) = aaa_common::conn::frame_channel();
        let conn = crate::AgentConn::new(outgoing, 1);
        submission_machine(&h, conn.clone(), "macos", vec!["report_hash.v1".into()]);
        let (running, resumed) = tokio::sync::watch::channel(case == Case::Partial);
        let writes = Arc::new(std::sync::Mutex::new(Vec::<Vec<u8>>::new()));
        let captured = writes.clone();
        let fake_host = h.clone();
        let fake = tokio::spawn(async move {
            while let Some((_, bytes)) = frames.recv().await {
                let LinkMsg::Req { id, body, .. } = serde_json::from_slice(&bytes).unwrap() else {
                    panic!("잘못된 요청")
                };
                let reply = match body {
                    AgentRequest::Screen { .. } => {
                        let writes = captured.lock().unwrap();
                        let screen = match (*resumed.borrow(), writes.as_slice()) {
                            (false, _) => "You've hit your limit · resets 12:30pm\n❯ Wait here, then continue automatically at 12:30pm\n  Stop\nEnter to select · Esc to cancel".to_string(),
                            (true, [_]) if case == Case::Partial =>
                                "Claude Code will continue automatically at 12:30pm.\nPress esc to cancel the wait.".to_string(),
                            (true, [paste]) => format!("────────────────\n❯ {}\n────────────────", String::from_utf8_lossy(paste)),
                            (true, _) => "────────────────\n❯ \n────────────────\n  ? for shortcuts".to_string(),
                        };
                        AgentResponse::Screen(aaa_proto::ScreenText {
                            session: "s".into(),
                            cols: 80,
                            rows: 24,
                            lines: lines(&screen),
                            cursor_row: 0,
                            cursor_col: 0,
                            ansi: screen,
                        })
                    }
                    AgentRequest::ReportHash { .. } => AgentResponse::ReportHash {
                        hash: if case == Case::ChangedOnAgent { "report-b" } else { "report-a" }.into(),
                    },
                    AgentRequest::Input { session, data } => {
                        assert_eq!(session, "s");
                        if data.0 == b"\r" && case != Case::Unconfirmed {
                            fake_host.on_harness(
                                "m",
                                HarnessEvent { harness: "claude".into(), ..codex_event("working", 3) },
                            );
                        }
                        captured.lock().unwrap().push(data.0);
                        AgentResponse::Done
                    }
                    _ => AgentResponse::Done,
                };
                if let Some(tx) = conn.pending.lock().unwrap().remove(&id) {
                    tx.send(Outcome::Ok(reply)).unwrap();
                }
            }
        });
        let answer = || h.thread_answer("demo", &rec.id, "report-a", vec![Some(0)], None, vec![], "device:fixture");
        let view = answer().await.unwrap();
        let answer_delivery = |view: aaa_proto::ThreadView| view.questions.unwrap().answer.unwrap().delivery;
        let pending = || h.db.pending_answers("demo").unwrap();
        let notices = || h.inbox("demo", true, true).unwrap().items;
        let messages = || {
            h.db.rec_list("messages", None)
                .unwrap()
                .iter()
                .map(|row| serde_json::from_str::<MessageView>(row).unwrap())
                .collect::<Vec<_>>()
        };
        if case == Case::Partial {
            assert_eq!(answer_delivery(view), None);
            h.pending_answer_pass("demo").await;
            assert_eq!(writes.lock().unwrap().len(), 1);
            assert!(pending().is_empty());
            assert_eq!(messages()[0].input, Some(MessageInput::DraftWritten));
            assert!(notices().iter().any(|i| i.event == "message not delivered"));
            fake.abort();
            continue;
        }

        assert_eq!(answer_delivery(view), Some(AnswerDelivery::Waiting), "{case:?}");
        assert!(answer().await.is_err(), "같은 카드의 두 번째 답");
        let notice = notices().into_iter().find(|i| i.event == "question answer waiting").unwrap();
        let waiting = messages().pop().unwrap();
        h.pending_answer_pass("demo").await;
        h.forget_queued_messages();
        assert_eq!(pending().len(), 1, "대기는 host 재시작 정리 뒤에도 남아야 한다");
        assert!(writes.lock().unwrap().is_empty(), "{case:?}: 한도 대기 중 입력");
        if case == Case::ClaimedBeforeRestart {
            let claimed = serde_json::from_str::<PendingAnswer>(&pending()[0]).unwrap();
            assert!(h.claim_pending_answer(&claimed).unwrap());
            assert!(!h.claim_pending_answer(&claimed).unwrap());
            h.forget_queued_messages();
            running.send_replace(true);
            h.pending_answer_pass("demo").await;
            assert!(writes.lock().unwrap().is_empty());
            assert_eq!(messages()[0].state, DeliveryState::Unknown);
            assert_eq!(messages()[0].input, None);
            assert!(notices().iter().any(|i| i.id == notice.id && i.event == "message not delivered"));
            fake.abort();
            continue;
        }
        match case {
            Case::NewReport => {
                h.update_rec("demo", &rec.id, |r| {
                    r.report_hash = Some("report-b".into());
                    r.question_answer = None;
                })
                .unwrap();
            }
            Case::Closed => {
                h.update_rec("demo", &rec.id, |r| r.status = ThreadStatus::Resolved).unwrap();
            }
            Case::Replaced => {
                h.update_rec("demo", &rec.id, |r| r.pane = Some("replacement".into())).unwrap();
            }
            Case::NewExecution => {
                h.inner.lock().unwrap().panes.get_mut("s").unwrap().info.started_at_ms += 1;
            }
            _ => {}
        }
        running.send_replace(true);
        h.on_harness("m", HarnessEvent { harness: "claude".into(), ..codex_event("idle", 2) });
        h.pending_answer_pass("demo").await;
        h.pending_answer_pass("demo").await;
        assert!(pending().is_empty(), "{case:?}");
        let updated = notices().into_iter().find(|i| i.id == notice.id).unwrap();
        assert_eq!(updated.body, notice.body);
        let values = writes.lock().unwrap();
        if matches!(case, Case::Resume | Case::Unconfirmed) {
            assert_eq!(values.as_slice(), &[waiting.text.as_bytes().to_vec(), b"\r".to_vec()]);
            let sent = messages().pop().unwrap();
            assert_eq!(sent.id, waiting.id);
            assert_eq!(sent.input, Some(MessageInput::Submitted));
            assert_eq!(answer_delivery(h.thread_show("demo", &rec.id).unwrap()), None);
            assert_eq!(h.db.ask_list("demo").unwrap().len(), 1);
            if case == Case::Resume {
                assert_eq!(sent.state, DeliveryState::Accepted);
                assert_eq!(updated.event, "question answered");
            } else {
                assert_eq!(sent.state, DeliveryState::Unknown);
                assert_eq!(updated.event, "message not delivered");
                assert_eq!(notices().iter().filter(|i| i.summary.contains(&sent.id)).count(), 1);
                h.flow(&rec, "message_accepted_late", Some(sent.id.clone()), &sent.id);
                assert_eq!(messages()[0].state, DeliveryState::Accepted);
                assert!(notices().iter().any(|i| i.id == notice.id && i.event == "message accepted late"));
            }
        } else {
            assert!(values.is_empty(), "{case:?}: 옛 카드 답 입력");
            assert_eq!(updated.event, "question answer held", "{case:?}");
            assert!(updated.summary.contains("전달 보류"));
            assert!(h.db.ask_list("demo").unwrap().is_empty());
        }
        fake.abort();
    }
}

/// 실제 ConPTY 재현의 버퍼 경계와 Enter 한 번, 경계 중 뜬 승인 창을 지킨다.
#[tokio::test(start_paused = true)]
async fn windows_codex_flushes_the_paste_before_one_enter_and_rechecks_a_modal() {
    use aaa_proto::{AgentResponse, LinkMsg, Outcome};
    for (os, modal) in [("windows", false), ("windows", true), ("macos", false)] {
        let (h, _lab) = host_with_pane();
        h.on_harness("m", codex_event("idle", 1));
        let (outgoing, mut frames) = aaa_common::conn::frame_channel();
        let conn = crate::AgentConn::new(outgoing, 1);
        submission_machine(&h, conn.clone(), os, vec![]);
        let writes = Arc::new(std::sync::Mutex::new(Vec::<Vec<u8>>::new()));
        let captured = writes.clone();
        let fake_host = h.clone();
        let windows = os == "windows";
        let fake = tokio::spawn(async move {
            let mut pending = false;
            let mut typed = false;
            let mut boundary = false;
            while let Some((_, bytes)) = frames.recv().await {
                let LinkMsg::Req { id, body, .. } = serde_json::from_slice(&bytes).unwrap() else { panic!("요청") };
                let reply = match body {
                    AgentRequest::Screen { .. } => {
                        let text = if modal && boundary {
                            "Press enter to confirm or esc to cancel"
                        } else if typed {
                            "› [Pasted Content 1046 chars]"
                        } else {
                            "›"
                        };
                        AgentResponse::Screen(aaa_proto::ScreenText {
                            session: "s".into(),
                            cols: 80,
                            rows: 24,
                            lines: lines(text),
                            cursor_row: 0,
                            cursor_col: 0,
                            ansi: text.into(),
                        })
                    }
                    AgentRequest::Input { data, .. } => {
                        captured.lock().unwrap().push(data.0.clone());
                        match data.0.as_slice() {
                            b"\x1b[C" => {
                                pending = false;
                                boundary = true;
                            }
                            b"\r" if !windows || !pending => {
                                fake_host.on_harness("m", codex_event("working", 2));
                            }
                            b"\r" => {}
                            _ => {
                                typed = true;
                                pending = true;
                            }
                        }
                        AgentResponse::Done
                    }
                    _ => panic!("입력과 무관한 요청"),
                };
                conn.pending.lock().unwrap().remove(&id).unwrap().send(Outcome::Ok(reply)).unwrap();
            }
        });
        let body = "BODY-PRIVATE-CANARY".repeat(60);
        let result = h.submit_checked("m", "s", Some(&body), None).await;
        if modal {
            assert!(result.unwrap_err().typed);
        } else {
            assert!(result.unwrap());
        }
        let writes = writes.lock().unwrap().clone();
        assert_eq!(writes[0], body.as_bytes());
        assert_eq!(writes.iter().filter(|b| b.as_slice() == b"\r").count(), usize::from(!modal));
        assert_eq!(writes.iter().filter(|b| b.as_slice() == b"\x1b[C").count(), usize::from(windows));
        let events = h.db.input_events(Some("s"), None, 0).unwrap();
        assert!(events.iter().all(|e| !e.detail.contains("BODY-PRIVATE-CANARY")));
        assert!(events.iter().any(|e| e.event == "paste_written"));
        if windows {
            assert!(events.iter().any(|e| e.event == "paste_boundary"));
        }
        fake.abort();
    }
}

/// 같은 본문의 과거 기록·기존 Working은 거부하고 새 기록 영수증만 수락한다.
#[tokio::test(start_paused = true)]
async fn a_busy_submission_requires_its_new_receipt_instead_of_the_previous_working_state() {
    use aaa_proto::{AgentResponse, ChatMessage, ChatReceipt, ChatRole, InputReceiptMethod, LinkMsg, Outcome};
    use sha2::Digest;
    for (harness, new_receipt) in [("claude", false), ("claude", true), ("codex", false), ("codex", true)] {
        let (h, _lab) = host_with_pane();
        h.on_harness(
            "m",
            HarnessEvent {
                harness: harness.into(),
                harness_session: Some("11111111-2222-4333-8444-555555555555".into()),
                ..codex_event("working", 1)
            },
        );
        let (outgoing, mut frames) = aaa_common::conn::frame_channel();
        let conn = crate::AgentConn::new(outgoing, 1);
        submission_machine(&h, conn.clone(), "macos", vec!["chat_pages.v1".into()]);
        let fake = tokio::spawn(async move {
            let mut typed = false;
            let mut entered = false;
            while let Some((_, bytes)) = frames.recv().await {
                let LinkMsg::Req { id, body, .. } = serde_json::from_slice(&bytes).unwrap() else { panic!("요청") };
                let reply = match body {
                    AgentRequest::Screen { .. } => {
                        let text = if typed { "› 새 본문" } else { "›" };
                        AgentResponse::Screen(aaa_proto::ScreenText {
                            session: "s".into(),
                            cols: 80,
                            rows: 24,
                            lines: lines(text),
                            cursor_row: 0,
                            cursor_col: 0,
                            ansi: text.into(),
                        })
                    }
                    AgentRequest::Input { data, .. } => {
                        if data.0 == b"\r" {
                            entered = true;
                        } else {
                            typed = true;
                        }
                        AgentResponse::Done
                    }
                    AgentRequest::Chat { .. } => {
                        let message = |id: &str| ChatMessage {
                            id: Some(id.into()),
                            role: ChatRole::User,
                            text: "새 본문".into(),
                            pending: harness == "claude",
                            receipt: Some(ChatReceipt {
                                sha256: format!("{:x}", sha2::Sha256::digest("새 본문".as_bytes())),
                                method: InputReceiptMethod::Normal,
                            }),
                        };
                        let mut messages = vec![message("file:0001")];
                        if entered && new_receipt {
                            messages.push(message("file:0002"));
                        }
                        AgentResponse::Chat { messages, history: Some("file".into()), truncated: false, before: None }
                    }
                    _ => panic!("입력과 무관한 요청"),
                };
                conn.pending.lock().unwrap().remove(&id).unwrap().send(Outcome::Ok(reply)).unwrap();
            }
        });
        assert_eq!(h.submit_checked("m", "s", Some("새 본문"), None).await.unwrap(), new_receipt);
        let events = h.db.input_events(Some("s"), None, 0).unwrap();
        assert_eq!(events.iter().any(|e| e.event == "receipt"), new_receipt);
        assert_eq!(events.iter().any(|e| e.event == "unknown"), !new_receipt);
        fake.abort();
    }
}

#[tokio::test(start_paused = true)]
async fn late_message_receipts_update_the_original_alert_without_writing_again() {
    use aaa_proto::{AgentResponse, ChatMessage, ChatReceipt, ChatRole, InputReceiptMethod, LinkMsg, Outcome};

    for (queued, competing_input) in [(false, false), (true, false), (false, true)] {
        let (h, _lab) = host_with_pane();
        h.project_create("demo", Default::default(), "test").unwrap();
        let mut rec =
            ThreadRec::starting("demo", "영수증", "m", "codex", aaa_proto::Role::Worker, aaa_proto::ThreadKind::Dir, 1);
        rec.id = "t-0001".into();
        rec.pane = Some("s".into());
        rec.brief = BriefState::Delivered;
        rec.status = ThreadStatus::Open;
        h.db.thread_import("demo", &rec.id, "open", &serde_json::to_string(&rec).unwrap()).unwrap();
        let mut idle = codex_event("idle", 1);
        idle.harness_session = Some("11111111-2222-4333-8444-555555555555".into());
        h.on_harness("m", idle);

        let (outgoing, mut frames) = aaa_common::conn::frame_channel();
        let conn = crate::AgentConn::new(outgoing, 1);
        submission_machine(&h, conn.clone(), "macos", vec!["chat_pages.v1".into(), "chat_watch.v1".into()]);
        let (watch_tx, mut watch_rx) = tokio::sync::mpsc::unbounded_channel();
        let (read_tx, mut read_rx) = tokio::sync::mpsc::unbounded_channel();
        let (record, observed) = tokio::sync::watch::channel(0);
        let agent = conn.clone();
        let fake = tokio::spawn(async move {
            let mut writes = Vec::new();
            while let Some((_, bytes)) = frames.recv().await {
                let LinkMsg::Req { id, body, .. } = serde_json::from_slice(&bytes).unwrap() else {
                    panic!("agent 요청")
                };
                let reply = match body {
                    AgentRequest::Screen { .. } => {
                        let text = if writes.is_empty() { "›" } else { "› 보낼 말" };
                        AgentResponse::Screen(aaa_proto::ScreenText {
                            session: "s".into(),
                            cols: 80,
                            rows: 24,
                            lines: lines(text),
                            cursor_row: 0,
                            cursor_col: 0,
                            ansi: text.into(),
                        })
                    }
                    AgentRequest::Input { data, .. } => {
                        writes.push(data.0);
                        AgentResponse::Done
                    }
                    AgentRequest::Chat { .. } => {
                        let message = |id: &str, text: &str, pending| ChatMessage {
                            id: Some(id.into()),
                            role: ChatRole::User,
                            text: text.into(),
                            pending,
                            receipt: Some(ChatReceipt {
                                sha256: crate::prompt_receipt::receipt_hash(text),
                                method: InputReceiptMethod::Normal,
                            }),
                        };
                        let state = *observed.borrow();
                        let mut messages = vec![message("file:0001", "보낼 말", false)];
                        if state >= 1 {
                            messages.push(message("file:0002", "다른 입력", false));
                        }
                        if state == 2 {
                            messages.push(message("file:0003", "보낼 말", queued));
                        }
                        read_tx.send(state).unwrap();
                        AgentResponse::Chat { messages, history: Some("file".into()), before: None, truncated: false }
                    }
                    AgentRequest::ChatWatch { enabled, .. } => {
                        let snapshot = (!enabled).then(|| writes.clone());
                        watch_tx.send(snapshot).unwrap();
                        AgentResponse::Done
                    }
                    _ => panic!("입력과 무관한 요청"),
                };
                if let Some(pending) = agent.pending.lock().unwrap().remove(&id) {
                    pending.send(Outcome::Ok(reply)).unwrap();
                }
            }
        });
        let mut flow = h.flow_events.subscribe();
        let message = h.thread_message("demo", &rec.id, "보낼 말".into(), MessageMode::Queue, "test").await.unwrap();
        assert_eq!((message.state, message.input), (DeliveryState::Unknown, Some(MessageInput::Submitted)));
        let mut second = message.clone();
        second.id = "m-other".into();
        second.text = "다른 입력".into();
        h.save_message(&second);
        h.flow(&rec, "message_unknown", Some(second.id.clone()), format!("{} Enter 입력됨", second.id));
        let page = h.inbox("demo", false, false).unwrap();
        let alert = page.items.iter().find(|i| i.summary.contains(&message.id)).unwrap();
        assert_eq!(alert.event, "message not delivered");
        assert!(alert.summary.contains("Enter 입력됨"));
        if queued {
            h.inbox_done("demo", std::slice::from_ref(&alert.id), false, "test").unwrap();
        }
        let limit = Duration::from_secs(1);
        assert_eq!(tokio::time::timeout(limit, watch_rx.recv()).await.unwrap(), Some(None));
        record.send_replace(1);
        h.on_chat_changed("m", "s".into(), "unrelated".into());
        tokio::time::timeout(limit, async { while read_rx.recv().await.unwrap() != 1 {} }).await.unwrap();
        if competing_input {
            conn.request(AgentRequest::Input { session: "s".into(), data: B64("보낼 말".as_bytes().to_vec()) })
                .await
                .unwrap();
        }
        record.send_replace(2);
        h.on_chat_changed("m", "s".into(), "receipt".into());
        let mut writes = vec!["보낼 말".as_bytes().to_vec(), b"\r".to_vec()];
        if competing_input {
            writes.push("보낼 말".as_bytes().to_vec());
        }
        assert_eq!(tokio::time::timeout(limit, watch_rx.recv()).await.unwrap(), Some(Some(writes)));
        let page = h.inbox("demo", true, true).unwrap();
        let updated = page.items.iter().find(|i| i.id == alert.id).unwrap();
        assert_eq!(updated.event, if competing_input { "message not delivered" } else { "message accepted late" });
        assert!(updated.seen);
        assert_eq!(updated.done, queued);
        if !competing_input {
            assert!(updated.summary.contains("늦게 수락 확인"));
            let mut path = h.projects_root().join("demo/inbox");
            if queued {
                path.push("done");
            }
            let file = std::fs::read_to_string(path.join(format!("{}.md", alert.id))).unwrap();
            assert!(file.contains("message accepted late"), "{file}");
        }
        assert_eq!(
            page.items.iter().filter(|i| i.event == "message not delivered" && i.summary.contains("m-other")).count(),
            1
        );
        let accepted = h.db.rec_list("messages", Some("accepted")).unwrap();
        assert_eq!(accepted.len(), usize::from(!competing_input));
        let events: Vec<_> =
            std::iter::from_fn(|| flow.try_recv().ok()).filter(|e| e.kind == "message_accepted_late").collect();
        assert_eq!(events.len(), usize::from(!competing_input));
        if let Some(e) = events.first() {
            assert_eq!(e.evidence.as_deref(), Some(message.id.as_str()));
        }
        fake.abort();
        assert!(fake.await.unwrap_err().is_cancelled());
    }
}

#[test]
fn an_earlier_same_body_can_arrive_in_the_transcript_late() {
    let (h, _lab) = host_with_pane();
    let mut earlier = MessageView {
        id: "m-earlier".into(),
        project: "demo".into(),
        thread: "t-0001".into(),
        mode: MessageMode::Queue,
        state: DeliveryState::Accepted,
        input: Some(MessageInput::Submitted),
        text: "같은\r\n본문".into(),
        created_at_ms: 1,
    };
    h.save_message(&earlier);
    let current = MessageView { id: "m-current".into(), text: "같은\n본문".into(), ..earlier.clone() };
    assert!(h.db.previous_message(&current).unwrap() == PreviousMessage::Accepted);
    earlier.state = DeliveryState::Unknown;
    earlier.input = Some(MessageInput::NotWritten);
    h.save_message(&earlier);
    assert!(h.db.previous_message(&current).unwrap() == PreviousMessage::None);
    earlier.input = Some(MessageInput::EnterUnconfirmed);
    h.save_message(&earlier);
    assert!(h.db.previous_message(&current).unwrap() == PreviousMessage::Unconfirmed);
}

/// 출력 알림만으로 수락하지 않고 실제 화면 변화에 반응해야 한다. 정지 시계로
/// 250ms 관측 간격을 기다리지 않는 것과 원래 paste 간격을 함께 검사한다.
#[tokio::test(start_paused = true)]
async fn unhooked_submission_rechecks_output_without_accepting_an_unchanged_screen() {
    use aaa_proto::{AgentResponse, LinkMsg, Outcome};
    let (h, _tmp) = host_with_pane();
    let (outgoing, mut frames) = aaa_common::conn::frame_channel();
    let conn = crate::AgentConn::new(outgoing, 1);
    h.inner.lock().unwrap().machines.insert(
        "m".into(),
        crate::MachineRt {
            name: "mac-a".into(),
            local: true,
            os: None,
            boot_id: None,
            dev_mode: true,
            conn: Some(conn.clone()),
            last_seen_ms: None,
            health: None,
            health_sent: None,
            features: vec![],
            identity: None,
            agent_pid: None,
            colors: None,
            build: Default::default(),
        },
    );
    let (observed, mut reads) = tokio::sync::mpsc::unbounded_channel();
    let changed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let screen_changed = changed.clone();
    let fake = tokio::spawn(async move {
        while let Some((_, bytes)) = frames.recv().await {
            let LinkMsg::Req { id, body, .. } = serde_json::from_slice(&bytes).unwrap() else { panic!("요청") };
            let (reply, label) = match body {
                AgentRequest::Screen { .. } => {
                    let screen =
                        if screen_changed.load(std::sync::atomic::Ordering::SeqCst) { "received" } else { "ready>" };
                    (
                        AgentResponse::Screen(aaa_proto::ScreenText {
                            session: "s".into(),
                            cols: 80,
                            rows: 24,
                            lines: lines(screen),
                            cursor_row: 0,
                            cursor_col: 0,
                            ansi: screen.into(),
                        }),
                        "screen",
                    )
                }
                AgentRequest::Input { data, .. } => {
                    (AgentResponse::Done, if data.0 == b"\r" { "enter" } else { "text" })
                }
                _ => panic!("입력과 무관한 요청"),
            };
            conn.pending.lock().unwrap().remove(&id).unwrap().send(Outcome::Ok(reply)).unwrap();
            observed.send((label, tokio::time::Instant::now())).unwrap();
        }
    });
    let submitter = h.clone();
    let submit = tokio::spawn(async move { submitter.submit_checked("m", "s", Some("hello"), None).await });
    let mut typed_at = None;
    let enter_at = loop {
        let (label, at) = reads.recv().await.unwrap();
        if label == "text" {
            typed_at = Some(at);
        }
        if label == "enter" {
            break at;
        }
    };
    assert_eq!(enter_at - typed_at.unwrap(), Duration::from_millis(300));
    let sender = h.inner.lock().unwrap().panes["s"].events.clone();
    // 앞서 읽은 typed 화면 다음에는 Enter 뒤의 동일 화면도 확인한다.
    while reads.recv().await.unwrap().0 != "screen" {}
    sender.send(crate::PaneEvent::State).unwrap();
    assert_eq!(reads.recv().await.unwrap().0, "screen");
    assert!(!submit.is_finished(), "화면이 그대로인 알림을 수락으로 셌다");
    changed.store(true, std::sync::atomic::Ordering::SeqCst);
    sender.send(crate::PaneEvent::Output(0, Arc::new(b"received".to_vec()))).unwrap();
    assert!(submit.await.unwrap().unwrap());
    assert_eq!(tokio::time::Instant::now(), enter_at, "출력 뒤 관측 간격을 기다렸다");
    fake.abort();
}

/// m13의 무반응 PTY는 두 번의 실제 10초 시한을 반복했다. 같은 Host/DB/AgentConn
/// owner에서 원래 시한과 입력·Enter·unknown·수동 재전송·조기 메시지 거부를 검사한다.
#[tokio::test(start_paused = true)]
async fn unknown_brief_keeps_the_accept_deadline_and_never_retypes_without_a_person() {
    use aaa_proto::{AgentResponse, LinkMsg, Outcome, Role, ThreadGroup, ThreadKind};
    let (h, _tmp) = host_with_pane();
    assert_eq!(h.cfg.projects.accept_wait_ms, 10_000);
    h.project_create("demo", Default::default(), "test").unwrap();
    let mut rec = ThreadRec::starting("demo", "mute", "m", "mute", Role::Worker, ThreadKind::Dir, 1);
    rec.id = "t-0001".into();
    rec.pane = Some("s".into());
    rec.status = ThreadStatus::Open;
    h.db.thread_import("demo", &rec.id, "open", &serde_json::to_string(&rec).unwrap()).unwrap();
    let (outgoing, mut frames) = aaa_common::conn::frame_channel();
    let conn = crate::AgentConn::new(outgoing, 1);
    h.inner.lock().unwrap().machines.insert(
        "m".into(),
        crate::MachineRt {
            name: "mac-a".into(),
            local: true,
            os: None,
            boot_id: None,
            dev_mode: true,
            conn: Some(conn.clone()),
            last_seen_ms: None,
            health: None,
            health_sent: None,
            features: vec![],
            identity: None,
            agent_pid: None,
            colors: None,
            build: Default::default(),
        },
    );
    let writes = Arc::new(std::sync::Mutex::new(Vec::new()));
    let captured = writes.clone();
    let (inputs, mut observed) = tokio::sync::mpsc::unbounded_channel();
    let fake = tokio::spawn(async move {
        while let Some((_, bytes)) = frames.recv().await {
            let LinkMsg::Req { id, body, .. } = serde_json::from_slice(&bytes).unwrap() else { panic!("요청") };
            let reply = match body {
                AgentRequest::Screen { .. } => AgentResponse::Screen(aaa_proto::ScreenText {
                    session: "s".into(),
                    cols: 80,
                    rows: 24,
                    lines: lines("mute>"),
                    cursor_row: 0,
                    cursor_col: 5,
                    ansi: "mute>".into(),
                }),
                AgentRequest::Input { data, .. } => {
                    captured.lock().unwrap().push(data.0.clone());
                    inputs.send((tokio::time::Instant::now(), data.0)).unwrap();
                    AgentResponse::Done
                }
                _ => panic!("brief와 무관한 요청"),
            };
            conn.pending.lock().unwrap().remove(&id).unwrap().send(Outcome::Ok(reply)).unwrap();
        }
    });
    let mut flow = h.flow_events.subscribe();
    h.spawn_brief("demo", &rec.id);
    let (typed_at, text) = observed.recv().await.unwrap();
    let (_, enter) = observed.recv().await.unwrap();
    assert_eq!(text, ".aaa/demo-t-0001/brief.md를 읽고 그대로 하라.".as_bytes());
    assert_eq!(enter, b"\r");
    let deadline = typed_at + Duration::from_millis(h.cfg.projects.accept_wait_ms);
    tokio::time::advance(deadline.duration_since(tokio::time::Instant::now()) - Duration::from_nanos(1)).await;
    tokio::task::yield_now().await;
    assert_eq!(h.load("demo", &rec.id).unwrap().brief, BriefState::Pending, "받음 시한 전에 unknown으로 끝났다");
    tokio::time::advance(Duration::from_nanos(1)).await;
    let event = flow.recv().await.unwrap();
    assert!(tokio::time::Instant::now() <= deadline + Duration::from_millis(500), "받음 시한과 두 관측 간격을 넘겼다");
    assert_eq!(event.kind, "brief_unknown");
    assert!(event.detail.contains("입력창을 읽을 수 없어"));
    let view = h.thread_show("demo", &rec.id).unwrap();
    assert_eq!((view.brief, view.group), (BriefState::Unknown, ThreadGroup::WaitingOnYou));
    tokio::time::advance(Duration::from_secs(60)).await;
    tokio::task::yield_now().await;
    assert_eq!(writes.lock().unwrap().as_slice(), &[text.clone(), enter.clone()]);
    let error = h.thread_brief("demo", &rec.id, "local:cli").await.unwrap_err();
    assert!(error.message.contains("아직 보내지 못했다"));
    assert_eq!(writes.lock().unwrap().as_slice(), &[text.clone(), enter.clone(), text, enter]);
    let error =
        h.thread_message("demo", &rec.id, "MSG-EARLY-1".into(), MessageMode::Queue, "local:cli").await.unwrap_err();
    assert!(error.message.contains("아직 brief를 받지 않았다"));
    assert_eq!(writes.lock().unwrap().len(), 4, "brief 전 메시지가 전송됐다");
    assert!(matches!(flow.try_recv(), Err(tokio::sync::broadcast::error::TryRecvError::Empty)));
    fake.abort();
}

#[tokio::test]
async fn shell_harness_replacement_clears_the_previous_chat_identity() {
    let (h, _tmp) = host_with_pane();
    let id = "019a4c5e-1f2b-7c3d-8e4f-a5b6c7d8e9f0";
    let mut first = codex_event("session_start", 1_000);
    first.harness = "claude".into();
    first.harness_session = Some(id.into());
    first.transcript = Some("/example/projects/session.jsonl".into());
    h.on_harness("m", first);
    assert_eq!(h.pane_agent("s").unwrap().harness_session.as_deref(), Some(id));
    h.on_harness("m", codex_event("session_start", 2_000));
    assert_eq!(h.pane_agent("s").unwrap().harness_session, None);
    assert_eq!(h.inner.lock().unwrap().panes["s"].transcript, None);
    let mut second = codex_event("session_start", 3_000);
    second.harness_session = Some(id.into());
    h.on_harness("m", second);
    h.on_harness("m", codex_event("released", 4_000));
    assert!(h.pane_agent("s").is_none());
    assert_eq!(h.inner.lock().unwrap().panes["s"].transcript, None);
}

#[tokio::test]
async fn progress_activity_clock_survives_heartbeats_and_resets_with_activity_and_session() {
    let (h, _tmp) = host_with_pane();
    let report = |at_ms, percent, text: &str| {
        let mut ev = codex_event("progress", at_ms);
        ev.percent = percent;
        ev.message = Some(text.into());
        h.on_harness("m", ev);
        h.inner.lock().unwrap().panes["s"].agent.clone().unwrap()
    };
    assert_eq!(report(1000, Some(20), "구현 중").progress_since_ms, Some(1000));
    let a = report(2000, Some(30), "구현 중");
    assert_eq!((a.progress_since_ms, a.progress_at_ms, a.percent), (Some(1000), Some(2000), Some(30)));
    let a = report(1500, Some(25), "늦은 옛 활동");
    assert_eq!(
        (a.progress.as_deref(), a.progress_since_ms, a.progress_at_ms),
        (Some("구현 중"), Some(1000), Some(2000))
    );
    assert_eq!(report(3000, None, "검증 중").progress_since_ms, Some(3000));
    report(4000, Some(50), crate::orch::WAITING);
    h.on_harness("m", codex_event("prompt_submit", 5000));
    assert_eq!(h.inner.lock().unwrap().panes["s"].agent.as_ref().unwrap().progress_since_ms, None);
    report(6000, Some(60), "수리 중");
    h.on_harness("m", codex_event("session_start", 7000));
    let a = h.inner.lock().unwrap().panes["s"].agent.clone().unwrap();
    assert_eq!((a.progress, a.progress_since_ms, a.progress_at_ms), (None, None, None));
}

/// `thread read`는 입력창의 흐린 글(Claude 다음 입력 예측)에 표지를 단다(t-1940). host 화면 사본(replay)에서 읽는 실제 길이다.
#[tokio::test]
async fn thread_read_marks_a_predicted_prompt_in_the_box() {
    use aaa_proto::{Role, Snapshot, ThreadKind};
    let rule = format!("\x1b[38;2;136;136;136m{}\x1b[m", "─".repeat(60));
    for (boxed, want) in [
        ("\u{a0}\x1b[2m바뀐 것을 main에 올려\x1b[m", "❯ [흐린 글·입력 아님] 바뀐 것을 main에 올려"),
        ("\u{a0}바뀐 것을 main에 올려", "❯\u{a0}바뀐 것을 main에 올려"),
    ] {
        let (h, _tmp) = host_with_pane();
        h.project_create("demo", Default::default(), "test").unwrap();
        let mut r = ThreadRec::starting("demo", "읽기", "m", "claude", Role::Worker, ThreadKind::Dir, 1);
        r.id = "t-0001".into();
        r.pane = Some("s".into());
        r.status = ThreadStatus::Open;
        r.brief = BriefState::Delivered;
        h.db.thread_import("demo", &r.id, "open", &serde_json::to_string(&r).unwrap()).unwrap();
        let screen = format!("⏺ 끝났습니다.\r\n\r\n{rule}\r\n❯{boxed}\r\n{rule}\r\n  status");
        h.on_subscribed(
            "s",
            Some(Snapshot {
                compression: None,
                session: "s".into(),
                offset: 0,
                cols: 80,
                rows: 24,
                replay: B64(screen.into_bytes()),
                history_more: false,
            }),
            0,
        );
        for lines in [None, Some(20)] {
            let aaa_proto::FrontResponse::ThreadPane(v) = h.thread_read("demo", "t-0001", lines).await.unwrap() else {
                panic!("ThreadPane 응답이 아니다")
            };
            let text = v.read.unwrap().text;
            assert!(text.lines().any(|l| l == want), "{lines:?}\n{text}");
        }
    }
}

/// thread keys의 글+Enter는 Codex의 paste 판정 창을 넘긴 뒤 제출한다.
/// 가상 시계와 실제 Agent link 경계로 바이트·순서를 본다(실제 시간을 재지 않는다).
#[tokio::test(start_paused = true)]
async fn thread_keys_separates_paste_from_enter() {
    use aaa_common::conn::{FrameRx, spawn_writer};
    use aaa_proto::{AgentResponse, LinkMsg, Role, Snapshot, ThreadKind};
    for (paste, trust_after_text) in [(false, false), (true, false), (true, true)] {
        let (h, _tmp) = host_with_pane();
        h.project_create("demo", Default::default(), "test").unwrap();
        let mut r = ThreadRec::starting("demo", "입력", "m", "codex", Role::Worker, ThreadKind::Dir, 1);
        r.id = "t-0001".into();
        r.pane = Some("s".into());
        r.status = ThreadStatus::Open;
        r.brief = BriefState::Delivered;
        h.db.thread_import("demo", &r.id, "open", &serde_json::to_string(&r).unwrap()).unwrap();
        h.on_subscribed(
            "s",
            Some(Snapshot {
                compression: None,
                session: "s".into(),
                offset: 0,
                cols: 80,
                rows: 24,
                replay: B64(if paste { b"\x1b[?2004h".to_vec() } else { vec![] }),
                history_more: false,
            }),
            0,
        );
        let (writer, reader) = tokio::io::duplex(65536);
        let conn = crate::AgentConn::new(spawn_writer(writer, aaa_proto::MAX_FRAME), 1);
        h.inner.lock().unwrap().machines.insert(
            "m".into(),
            crate::MachineRt {
                name: "mac-a".into(),
                local: true,
                os: None,
                boot_id: None,
                dev_mode: true,
                conn: Some(conn.clone()),
                last_seen_ms: None,
                health: None,
                health_sent: None,
                features: vec![],
                identity: None,
                agent_pid: None,
                colors: None,
                build: Default::default(),
            },
        );
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured = seen.clone();
        let fake = tokio::spawn(async move {
            let mut rx = FrameRx::new(reader, aaa_proto::MAX_FRAME);
            while let Ok(LinkMsg::Req { id, body, .. }) = rx.control::<LinkMsg>().await {
                let reply = match body {
                    AgentRequest::Screen { .. } => {
                        let screen = if trust_after_text && !captured.lock().unwrap().is_empty() {
                            "Trust this folder?\n› 1. Trust and continue\n  2. Open restricted"
                        } else {
                            "› Ready"
                        };
                        AgentResponse::Screen(aaa_proto::ScreenText {
                            session: "s".into(),
                            cols: 80,
                            rows: 24,
                            lines: lines(screen),
                            cursor_row: 0,
                            cursor_col: 0,
                            ansi: screen.into(),
                        })
                    }
                    AgentRequest::Input { data, .. } => {
                        captured.lock().unwrap().push((tokio::time::Instant::now(), data.0));
                        AgentResponse::Done
                    }
                    _ => AgentResponse::Done,
                };
                if let Some(tx) = conn.pending.lock().unwrap().remove(&id) {
                    tx.send(aaa_proto::Outcome::Ok(reply)).unwrap();
                }
            }
        });
        let result = h.thread_keys("demo", &r.id, &["enter".into()], Some("이어서 계속하라".into()), "test").await;
        fake.abort();
        let got = seen.lock().unwrap();
        if trust_after_text {
            assert!(result.unwrap_err().message.contains("trust_screen"));
            assert_eq!(got.len(), 1, "치는 동안 뜬 신뢰 창에 Enter를 보내면 안 된다");
            continue;
        }
        result.unwrap();
        assert_eq!(got.len(), 2);
        assert!(got[1].0.duration_since(got[0].0) >= Duration::from_millis(300), "Enter가 paste 판정 창 안에 갔다");
        let expected = if paste { "\x1b[200~이어서 계속하라\x1b[201~" } else { "이어서 계속하라" };
        assert_eq!(got[0].1, expected.as_bytes());
        assert_eq!(got[1].1, b"\r");
    }
}

/// 실제 메시지·Next 전송 owner를 통과한다. 가짜 Codex는 입력 받음 뒤 잠깐 idle 화면을 낸다.
/// hook이 없어도 이미 답한 기다림이 되살아나지 않으며, 새 기다림·실제 질문은 숨기지 않는다.
#[tokio::test]
async fn codex_message_and_next_do_not_revive_previous_waiting_report() {
    use aaa_common::conn::{FrameRx, spawn_writer};
    use aaa_proto::{AgentResponse, LinkMsg, Role, ThreadGroup, ThreadKind};
    for hooked in [false, true] {
        for next in [false, true] {
            let (h, _tmp) = host_with_pane();
            h.project_create("demo", Default::default(), "test").unwrap();
            let mut r = ThreadRec::starting("demo", "시험", "m", "codex", Role::Worker, ThreadKind::Dir, 1);
            r.id = "t-0001".into();
            r.pane = Some("s".into());
            r.status = ThreadStatus::Open;
            r.brief = BriefState::Delivered;
            r.next = vec!["다음 작업".into()];
            h.db.thread_import("demo", &r.id, "open", &serde_json::to_string(&r).unwrap()).unwrap();
            h.on_harness("m", codex_event("idle", 1));
            let mut waiting = codex_event("progress", 2);
            waiting.message = Some(crate::orch::WAITING.into());
            h.on_harness("m", waiting.clone());
            assert_eq!(h.thread_view(&r).group, ThreadGroup::WaitingOnYou);

            let (writer, reader) = tokio::io::duplex(65536);
            let conn = crate::AgentConn::new(spawn_writer(writer, aaa_proto::MAX_FRAME), 1);
            h.inner.lock().unwrap().machines.insert(
                "m".into(),
                crate::MachineRt {
                    name: "mac-a".into(),
                    local: true,
                    os: None,
                    boot_id: None,
                    dev_mode: true,
                    conn: Some(conn.clone()),
                    last_seen_ms: None,
                    health: None,
                    health_sent: None,
                    features: vec![],
                    identity: None,
                    agent_pid: None,
                    colors: None,
                    build: Default::default(),
                },
            );
            let fake_host = h.clone();
            let fake = tokio::spawn(async move {
                let mut rx = FrameRx::new(reader, aaa_proto::MAX_FRAME);
                let mut screen = "› Ask Codex to do anything\n? for shortcuts".to_string();
                while let Ok(LinkMsg::Req { id, body, .. }) = rx.control::<LinkMsg>().await {
                    let reply = match body {
                        AgentRequest::Screen { .. } => AgentResponse::Screen(aaa_proto::ScreenText {
                            session: "s".into(),
                            cols: 80,
                            rows: 24,
                            lines: lines(&screen),
                            cursor_row: 0,
                            cursor_col: 0,
                            ansi: screen.clone(),
                        }),
                        AgentRequest::Input { data, .. } => {
                            if data.0 == b"\r" {
                                if hooked {
                                    fake_host.on_harness("m", codex_event("prompt_submit", 3));
                                    assert!(
                                        fake_host.pane_agent("s").unwrap().progress.is_none(),
                                        "hook이 옛 기다림을 지운다"
                                    );
                                }
                                // 감지의 working이 받음 신호다. 곧바로 idle로 돌아온 중간 화면도 처리한다.
                                fake_host.on_harness("m", codex_event("working", 4));
                                fake_host.on_harness("m", codex_event("idle", 5));
                            } else {
                                screen = format!("› {}\n? for shortcuts", String::from_utf8_lossy(&data.0));
                            }
                            AgentResponse::Done
                        }
                        _ => AgentResponse::Done,
                    };
                    if let Some(tx) = conn.pending.lock().unwrap().remove(&id) {
                        tx.send(aaa_proto::Outcome::Ok(reply)).unwrap();
                    }
                }
            });
            if next {
                let response = h.thread_next("demo", &r.id, Some(1), None, "test").await.unwrap();
                assert!(
                    matches!(response, aaa_proto::FrontResponse::Next { message: Some(m), .. } if m.state == DeliveryState::Accepted)
                );
            } else {
                let message = h.thread_message("demo", &r.id, "계속".into(), MessageMode::Queue, "test").await.unwrap();
                assert_eq!(message.state, DeliveryState::Accepted);
            }
            assert_ne!(h.thread_view(&r).group, ThreadGroup::WaitingOnYou, "hook={hooked}, Next={next}: 중간 idle");
            // 보고 확인 task가 끝나야 thread_pass가 이 thread를 셈한다. 완료 알림을 기다린다.
            tokio::time::timeout(Duration::from_secs(5), async {
                while h.report_refresh.lock().unwrap().contains_key("demo/t-0001") {
                    h.tick_wake.notified().await;
                }
            })
            .await
            .expect("보고 확인 완료");
            h.thread_pass("demo");
            assert_eq!(h.load("demo", &r.id).unwrap().last_group, Some(ThreadGroup::Idle));
            assert!(
                !h.db.inbox_list(Some("demo"), false).unwrap().iter().any(|i| i.summary.contains("waiting on you"))
            );
            h.on_harness("m", codex_event("working", 6));
            assert_eq!(h.thread_view(&r).group, ThreadGroup::Working);
            // 새 턴의 새 기다림은 다시 보인다.
            waiting.at_ms = 7;
            h.on_harness("m", waiting);
            h.on_harness("m", codex_event("idle", 8));
            assert_eq!(h.thread_view(&r).group, ThreadGroup::WaitingOnYou);
            h.on_harness("m", codex_event("question", 9));
            assert!(h.open_question_of("s").is_some());
            fake.abort();
        }
    }
}

/// 계획 사건(agent plan.rs, 차이 목록 S5)은 계획·배경 수만 바꾸고 상태는 그대로 둔다. harness가 끝나면 배경 수는 지운다
#[tokio::test]
async fn plan_events_set_counts_without_changing_the_activity() {
    let (h, _t) = host_with_pane();
    let ev = |kind: &str, at_ms, plan: Option<aaa_proto::PlanSummary>, background| HarnessEvent {
        session: "s".into(),
        harness: "claude".into(),
        kind: kind.into(),
        harness_session: None,
        message: None,
        question_kind: None,
        percent: None,
        at_ms,
        transcript: None,
        plan,
        background,
        context: None,
        model_observation: None,
        codex_speed: None,
        provider_turn: None,
        turn_reply: None,
    };
    h.on_harness("m", ev("working", 1_000, None, None));
    let plan = aaa_proto::PlanSummary { done: 1, total: 3, current: Some("고치는 중".into()) };
    // 컨텍스트 사용량도 같은 사건으로 온다(plan §8.321)
    let context = aaa_proto::ContextUsage { used: 43_922, window: Some(200_000), percent: Some(22), compaction: None };
    let model = aaa_proto::ModelObservation {
        model: "gpt-6-astra".into(),
        effort: Some("high".into()),
        at_ms: 2_000,
        speed: None,
    };
    h.on_harness(
        "m",
        HarnessEvent {
            context: Some(context.clone()),
            model_observation: Some(model.clone()),
            codex_speed: None,
            provider_turn: None,
            turn_reply: None,
            ..ev("plan", 2_000, Some(plan.clone()), Some(2))
        },
    );
    let a = h.pane_agent("s").unwrap();
    assert_eq!(
        (a.activity, a.since_ms, a.plan, a.background, a.context),
        (AgentActivity::Working, 1_000, Some(plan.clone()), Some(2), Some(context.clone()))
    );
    // 다른 상태 사건은 계획·컨텍스트를 지우지 않는다
    h.on_harness("m", ev("idle", 3_000, None, None));
    let a = h.pane_agent("s").unwrap();
    assert_eq!(a.model_observation, Some(model.clone()));
    assert_eq!(
        (a.activity, a.plan.as_ref(), a.background, a.context.as_ref()),
        (AgentActivity::Idle, Some(&plan), Some(2), Some(&context))
    );
    // harness가 끝나면 배경 작업과 그 세션의 컨텍스트가 지워진다
    h.on_harness("m", ev("exited", 4_000, None, None));
    let a = h.pane_agent("s").unwrap();
    assert_eq!((a.plan.as_ref(), a.background, a.context), (Some(&plan), None, None));
    // 계획이 없어졌다는 사건
    h.on_harness("m", ev("plan", 5_000, None, None));
    assert_eq!(h.pane_agent("s").unwrap().plan, None);
    assert_eq!(h.pane_agent("s").unwrap().model_observation, None);
    h.on_harness("m", HarnessEvent { model_observation: Some(model), ..ev("plan", 6_000, None, None) });
    h.on_harness("m", ev("session_start", 7_000, None, None));
    assert_eq!(h.pane_agent("s").unwrap().model_observation, None);
}

/// 답한 확인창 뒤 늦게 온 질문 사건은 그 질문을 다시 열지 않는다(t-0333 m13 사건 16·plan §8.99: 부하에서
/// PermissionRequest → 답 → 1.3초 뒤 Notification `permission_prompt`). 막힘이 끝난 뒤(작업 중: 화면 감지나 옛 agent의
/// PostToolUse·PreToolUse)의 질문 사건은 다음 확인창의 새 질문이다.
#[tokio::test]
async fn a_late_question_event_does_not_reopen_the_answered_question() {
    let (h, _t) = host_with_pane();
    let ev = |kind: &str, at_ms| HarnessEvent {
        session: "s".into(),
        harness: "claude".into(),
        kind: kind.into(),
        harness_session: None,
        message: Some("Bash make test".into()),
        question_kind: Some("permission".into()),
        percent: None,
        at_ms,
        transcript: None,
        plan: None,
        background: None,
        context: None,
        model_observation: None,
        codex_speed: None,
        provider_turn: None,
        turn_reply: None,
    };
    h.on_harness("m", ev("question", 1_000));
    let mut q = h.open_question_of("s").expect("질문");
    // 답의 마지막 걸음(`answer`: 키를 보낸 뒤 Answered로 적는다)
    q.state = QuestionState::Answered;
    q.answer = Some("yes".into());
    h.save_question(&q);
    h.on_harness("m", ev("question", 2_300));
    assert_eq!(h.open_question_of("s").map(|x| x.id), None, "답한 질문을 다시 열었다");
    assert_eq!(h.questions(true).len(), 1);
    h.on_harness("m", ev("working", 3_000));
    h.on_harness("m", ev("question", 4_000));
    let again = h.open_question_of("s").expect("다음 확인창의 질문");
    assert_ne!(again.id, q.id);
}

/// brief 화면 읽기 → 실제 답 저장 → 첫 감지 Question → Idle을 직접 순서대로 처리한다.
/// 감지 사건이 먼저 온 기존 회귀와 달리, 답할 때 host 상태는 아직 Idle이다.
#[tokio::test]
async fn a_late_first_trust_observation_keeps_the_answered_brief_question() {
    use aaa_proto::{AgentResponse, LinkMsg, Role, ThreadKind};

    for anew in [false, true] {
        let (h, _tmp) = host_with_pane();
        h.project_create("demo", Default::default(), "test").unwrap();
        let mut r = ThreadRec::starting("demo", "시험", "m", "claude", Role::Worker, ThreadKind::Dir, 1);
        r.id = "t-0001".into();
        r.pane = Some("s".into());
        r.status = ThreadStatus::Open;
        h.db.thread_import("demo", &r.id, "open", &serde_json::to_string(&r).unwrap()).unwrap();
        h.on_harness("m", codex_event("idle", 1_000));

        let screen =
            "Quick safety check: Do you trust the files in this folder?\n❯ 1. Yes\n  2. No, exit\nEnter to confirm";
        let (outgoing, mut frames) = aaa_common::conn::frame_channel();
        let conn = crate::AgentConn::new(outgoing, 1);
        h.inner.lock().unwrap().machines.insert(
            "m".into(),
            crate::MachineRt {
                name: "mac-a".into(),
                local: true,
                os: None,
                boot_id: None,
                dev_mode: true,
                conn: Some(conn.clone()),
                last_seen_ms: None,
                health: None,
                health_sent: None,
                features: if anew { vec!["ask_anew.v1".into()] } else { vec![] },
                identity: None,
                agent_pid: None,
                colors: None,
                build: Default::default(),
            },
        );
        let fake = tokio::spawn(async move {
            while let Some((_, bytes)) = frames.recv().await {
                let LinkMsg::Req { id, body, .. } = serde_json::from_slice(&bytes).unwrap() else { panic!("요청") };
                let reply = match body {
                    AgentRequest::Screen { .. } => AgentResponse::Screen(aaa_proto::ScreenText {
                        session: "s".into(),
                        cols: 80,
                        rows: 24,
                        lines: lines(screen),
                        cursor_row: 0,
                        cursor_col: 0,
                        ansi: screen.into(),
                    }),
                    AgentRequest::Input { data, .. } => {
                        assert_eq!(data.0, b"\r");
                        AgentResponse::Done
                    }
                    other => panic!("예상 밖 요청: {other:?}"),
                };
                conn.pending.lock().unwrap().remove(&id).unwrap().send(aaa_proto::Outcome::Ok(reply)).unwrap();
            }
        });

        let error = h.thread_brief("demo", &r.id, "test").await.unwrap_err();
        assert!(error.message.contains("확인창에 답을 기다린다"), "{error:?}");
        let q = h.open_question_of("s").expect("brief가 연 신뢰 질문");
        assert_eq!(q.question_kind, "trust");
        let answered = h.answer(&q.id, "yes", "test").await.unwrap();
        assert_eq!(answered.state, QuestionState::Answered);
        assert_eq!(h.pane_agent("s").unwrap().activity, AgentActivity::Idle);

        let observed = HarnessEvent {
            harness: "claude".into(),
            question_kind: Some(q.question_kind.clone()),
            message: Some(q.text.clone()),
            ..codex_event("question", 2_000)
        };
        h.on_harness("m", observed.clone());
        assert!(h.open_question_of("s").is_none(), "첫 관측이 답한 질문을 다시 열었다: anew={anew}");
        h.on_harness("m", codex_event("idle", 3_000));
        let saved = h.questions(true);
        assert_eq!(saved.len(), 1, "Idle이 중복 질문을 Gone으로 숨기면 안 된다: {saved:?}");
        assert_eq!(
            (saved[0].id.as_str(), saved[0].state, saved[0].answer.as_deref()),
            (q.id.as_str(), QuestionState::Answered, Some("yes"))
        );

        // 같은 본문도 막힘이 끝난 뒤 다시 나타나면 다음 질문이다.
        h.on_harness("m", HarnessEvent { at_ms: 4_000, ..observed.clone() });
        let next = h.open_question_of("s").unwrap();
        assert_ne!(next.id, q.id);
        h.answer(&next.id, "yes", "test").await.unwrap();
        // 이미 관측한 막힘은 원래 계약대로: 새 agent의 재질문은 열고 옛 중복 사건은 묶는다.
        h.on_harness("m", HarnessEvent { at_ms: 5_000, ..observed });
        assert_eq!(h.open_question_of("s").is_some(), anew);
        fake.abort();
        assert!(fake.await.unwrap_err().is_cancelled());
    }
}

/// 첫 감지가 다른 창이면 앞 신뢰 답으로 가리지 않는다. 그 새 질문의 답 전 중복도 열린 질문 하나다.
#[tokio::test]
async fn a_different_first_observation_is_not_an_answered_screen_question() {
    for (kind, text) in [("permission", "Do you want to proceed?"), ("trust", "Do you trust another folder?")] {
        let (h, _tmp) = host_with_pane();
        h.on_harness("m", codex_event("idle", 1_000));
        let mut trust = h.ask("s", "trust", "Do you trust the files in this folder?".into());
        trust.state = QuestionState::Answered;
        trust.answer = Some("yes".into());
        h.save_question(&trust);
        let ev = HarnessEvent {
            question_kind: Some(kind.into()),
            message: Some(text.into()),
            ..codex_event("question", 2_000)
        };
        h.on_harness("m", ev.clone());
        let next = h.open_question_of("s").expect("다른 첫 질문은 열린다");
        assert_ne!(next.id, trust.id);
        assert_eq!((next.question_kind.as_str(), next.text.as_str()), (kind, text));
        h.on_harness("m", HarnessEvent { at_ms: 3_000, ..ev });
        assert_eq!(h.open_question_of("s").unwrap().id, next.id);
        assert_eq!(h.questions(true).len(), 2);
    }
}

/// 대화 정리(plan §8.309): 화면 감지의 `compacting`은 작업 중이고 정리 표지를 단다. 같은 작업 중이라 상태가 된 때(목록 차례)는
/// 그대로다. 다음 상태 사건(작업 중·쉼)이 오면 표지를 지운다. 정리 중에는 대기 메시지 「지금 보내기」를 하지 않는다
#[tokio::test]
async fn compacting_is_working_with_a_marker_until_the_next_state() {
    let (h, _t) = host_with_pane();
    let state = |h: &Arc<Host>| h.pane_agent("s").map(|a| (a.activity, a.compacting, a.since_ms));
    h.on_harness("m", codex_event("working", 1_000));
    h.on_harness("m", codex_event("compacting", 2_000));
    assert_eq!(state(&h), Some((AgentActivity::Working, true, 1_000)));
    assert_eq!(h.steer_queued_data("s", "t"), Err(aaa_proto::SteerRefusal::Compacting));
    h.on_harness("m", codex_event("working", 3_000));
    assert_eq!(state(&h), Some((AgentActivity::Working, false, 1_000)));
    h.on_harness("m", codex_event("compacting", 4_000));
    h.on_harness("m", codex_event("idle", 5_000));
    assert_eq!(state(&h), Some((AgentActivity::Idle, false, 5_000)));
}

/// 같은 ms에 잇달아 만든 ID도 다르다(전에는 ULID 시각 부분이라 같아져 둘째 질문·메시지가 첫째 기록을 덮었다)
#[test]
fn ids_made_in_the_same_millisecond_differ() {
    let ids: std::collections::HashSet<String> = (0..1000).map(|_| short_id("q")).collect();
    assert_eq!(ids.len(), 1000);
}
/// host는 파일을 읽지 않는다. Reader의 같은 pane 권한/커서 중계와 구 agent 기능 경계를 소유한다.
#[tokio::test(start_paused = true)]
async fn chat_pages_relay_boundaries_without_reinterpreting_them_and_gate_old_agents() {
    use aaa_common::conn::{FrameRx, spawn_writer};
    use aaa_proto::{AgentResponse, ChatMessage, ChatRole, FrontRequest, FrontResponse, LinkMsg};
    let (h, _tmp) = host_with_pane();
    let (writer, reader) = tokio::io::duplex(65536);
    let conn = crate::AgentConn::new(spawn_writer(writer, aaa_proto::MAX_FRAME), 1);
    h.inner.lock().unwrap().machines.insert(
        "m".into(),
        crate::MachineRt {
            name: "mac-a".into(),
            local: true,
            os: None,
            boot_id: None,
            dev_mode: true,
            conn: Some(conn.clone()),
            last_seen_ms: None,
            health: None,
            health_sent: None,
            features: vec![],
            identity: None,
            agent_pid: None,
            colors: None,
            build: Default::default(),
        },
    );
    h.on_harness("m", codex_event("idle", 1));
    let denied = h
        .call(
            crate::DeviceRole::Reader,
            "test",
            FrontRequest::PaneChat {
                pane: "s".into(),
                thinking: false,
                compact: false,
                commands: false,
                before: Some("older".into()),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(denied.code, ErrorCode::UnsupportedVersion);
    let fake = tokio::spawn(async move {
        let mut rx = FrameRx::new(reader, aaa_proto::MAX_FRAME);
        for (asked, cursor, history, text) in [
            (None, None, None, "옛 agent"),
            (None, Some("boundary"), Some("file"), "최신 쪽"),
            (Some("boundary"), None, Some("file"), "첫 쪽"),
            (None, Some("grown-boundary"), Some("file"), "파일이 자람"),
        ] {
            let LinkMsg::Req { id, body: AgentRequest::Chat { before, harness, compact, commands, .. }, .. } =
                rx.control::<LinkMsg>().await.unwrap()
            else {
                panic!("chat 요청이 아니다")
            };
            assert_eq!(harness, "codex");
            // 정리된 자리는 그것을 청한 front에만 싣는다(옛 front는 모르는 role로 답을 읽지 못한다)
            assert!(compact);
            // 명령 줄(plan §8.323)도 청한 front에만
            assert!(commands);
            assert_eq!(before.as_deref(), asked);
            let reply = AgentResponse::Chat {
                messages: vec![ChatMessage {
                    id: history.map(|_| text.into()),
                    role: ChatRole::Assistant,
                    text: text.into(),
                    pending: false,
                    receipt: None,
                }],
                truncated: cursor.is_some(),
                before: cursor.map(str::to_string),
                history: history.map(str::to_string),
            };
            conn.pending.lock().unwrap().remove(&id).unwrap().send(aaa_proto::Outcome::Ok(reply)).unwrap();
        }
    });
    for (index, (before, expected_cursor, expected_text)) in [
        (None, None, "옛 agent"),
        (None, Some("boundary"), "최신 쪽"),
        (Some("boundary"), None, "첫 쪽"),
        (None, Some("grown-boundary"), "파일이 자람"),
    ]
    .into_iter()
    .enumerate()
    {
        if index == 1 {
            h.inner.lock().unwrap().machines.get_mut("m").unwrap().features.push("chat_pages.v1".into());
        }
        let response = h
            .call(
                crate::DeviceRole::Reader,
                "test",
                FrontRequest::PaneChat {
                    pane: "s".into(),
                    thinking: false,
                    compact: true,
                    commands: true,
                    before: before.map(str::to_string),
                },
            )
            .await
            .unwrap();
        let FrontResponse::Chat(view) = response else { panic!("chat 응답이 아니다") };
        assert_eq!(view.before.as_deref(), expected_cursor);
        assert_eq!(view.messages[0].text, expected_text);
        assert_eq!(view.truncated, expected_cursor.is_some());
        assert_eq!(view.history.as_deref(), if index == 0 { None } else { Some("file") });
    }
    fake.await.unwrap();
}

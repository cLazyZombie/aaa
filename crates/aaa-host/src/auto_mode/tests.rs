use super::*;
use aaa_proto::{DeviceRole, FrontRequest, PostInput, PostResult, PostType, Role};
struct Lab(std::path::PathBuf);
impl Drop for Lab {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn coordinator_host() -> (Arc<Host>, Lab) {
    let path = std::env::temp_dir().join(format!("aaa-auto-{}", aaa_common::util::new_id()));
    let mut cfg = aaa_config::HostConfig::default();
    cfg.projects.root = path.join("projects").to_string_lossy().into();
    cfg.notify.web_push = false;
    let h = Host::open(cfg, path.clone()).unwrap();
    let _lab = Lab(path);
    h.project_create("demo", Default::default(), "test").unwrap();
    h.agent_connected(
        &aaa_proto::AgentHello {
            proto: aaa_proto::AGENT_LINK,
            machine_id: "m".into(),
            machine_name: "mac-a".into(),
            boot_id: "boot".into(),
            os: "test".into(),
            arch: "test".into(),
            software: "test".into(),
            features: vec![],
            dev_mode: true,
            sha256: None,
            selfcheck: None,
            sessions: vec![],
            identity: None,
            harness: vec![],
        },
        true,
        crate::AgentConn::new(aaa_common::conn::spawn_writer(tokio::io::sink(), aaa_proto::MAX_FRAME), 1),
    );
    h.on_session(
        "m",
        aaa_proto::SessionInfo {
            responsibility: None,
            id: "coord".into(),
            name: "coordinator".into(),
            argv: vec!["codex".into()],
            cwd: Some(h.project_dir("demo").unwrap().to_string_lossy().into()),
            pid: Some(std::process::id()),
            started_at_ms: 0,
            boot_id: "boot".into(),
            cols: 80,
            rows: 24,
            state: aaa_proto::SessionState::Running,
            role: Some(Role::Coordinator),
            label: None,
            harness: Some("codex".into()),
        },
        false,
    );
    (h, _lab)
}

const COORD: &str = "local:cli@coord";
fn input(n: u32, body: &str) -> PostInput {
    PostInput {
        post_id: format!("{n:032x}"),
        to: "user".into(),
        post_type: PostType::Request,
        topic: "user-action".into(),
        title: format!("질문 {n}"),
        body: body.into(),
        needs_user: true,
        refs: vec![],
    }
}
fn question(h: &Host, n: u32, body: &str) -> Vec<String> {
    assert!(matches!(h.front_post("demo", input(n, body), COORD).unwrap(), PostResult::Accepted { .. }));
    h.user_requests("demo")
        .unwrap()
        .into_iter()
        .last()
        .unwrap()
        .questions
        .unwrap()
        .items
        .into_iter()
        .map(|q| q.id.unwrap())
        .collect()
}
async fn answer(h: &Arc<Host>, id: &str, actor: &str) -> Result<FrontResponse, ErrorBody> {
    h.call(
        DeviceRole::Local,
        actor,
        FrontRequest::QuestionAutoAnswer {
            project: "demo".into(),
            id: id.into(),
            choice: Some("B".into()),
            text: None,
            reason: "실측 근거로 추천과 다른 안을 고름".into(),
        },
    )
    .await
}
#[tokio::test]
async fn only_humans_toggle_and_only_the_projects_coordinator_answers() {
    let (h, _lab) = coordinator_host();
    let id = question(&h, 1, "## Questions\n### 1. 배치\n- A 아래 (추천)\n- B 위\n").remove(0);
    assert!(answer(&h, &id, COORD).await.unwrap_err().message.contains("꺼져"));
    for (role, actor) in [
        (DeviceRole::Local, COORD),
        (DeviceRole::Reader, "device:reader"),
        (DeviceRole::Local, "worker:w"),
        (DeviceRole::Local, "detached:w"),
    ] {
        assert!(h.call(role, actor, FrontRequest::AutoModeSet { project: "demo".into(), on: true }).await.is_err());
    }
    let mut events = h.asks_changed.subscribe();
    h.call(DeviceRole::Controller, "device:phone", FrontRequest::AutoModeSet { project: "demo".into(), on: true })
        .await
        .unwrap();
    assert!(events.has_changed().unwrap());
    events.borrow_and_update();
    for actor in ["local:cli", "local:cli@other", "worker:w", "device:phone"] {
        assert!(answer(&h, &id, actor).await.is_err());
    }
    answer(&h, &id, COORD).await.unwrap();
    let records = h.auto_decisions("demo").unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].decided_by, "coordinator(auto)");
    assert_eq!(records[0].pick, Some(1));
    assert!(records[0].question.choices[0].recommended);
    assert_eq!(records[0].outcome, "answered");
    assert_eq!(h.auto_mode("demo").unwrap().decisions, 1);
    assert!(answer(&h, &id, COORD).await.is_err());
    assert_eq!(h.auto_decisions("demo").unwrap().len(), 1);
    assert!(events.has_changed().unwrap());
    h.auto_mode_set("demo", false, "local:cli").unwrap();
    let mode = h.auto_mode("demo").unwrap();
    assert!(!mode.on);
    assert!(mode.ended_ms >= mode.since_ms);
    assert_eq!(mode.decisions, 1);
    let db = crate::db::Db::open(&_lab.0.join("copy.sqlite")).unwrap();
    db.set_meta(&key("demo"), &h.db.meta(&key("demo")).unwrap().unwrap()).unwrap();
    drop(db);
    let db = crate::db::Db::open(&_lab.0.join("copy.sqlite")).unwrap();
    assert_eq!(decode(db.meta(&key("demo")).unwrap()).unwrap().mode, mode);
    h.auto_mode_set("demo", true, "local:cli").unwrap();
    assert_eq!(h.auto_mode("demo").unwrap().decisions, 0);
    assert_eq!(h.auto_decisions("demo").unwrap().len(), 1);
    assert!(h.inbox("demo", true, true).unwrap().items.iter().any(|i| i.event == "auto-mode off"));
}

/// 자동 답도 글자 B를 위치 1로 가정하지 않고 원문의 B를 고른다.
#[tokio::test]
async fn automatic_reply_keeps_the_source_letter_on_reordered_choices() {
    let (h, _lab) = coordinator_host();
    h.auto_mode_set("demo", true, "local:cli").unwrap();
    let id = question(&h, 1, "## Questions\n### 1. 배치\n- B 위 (추천)\n- A 아래\n").remove(0);
    answer(&h, &id, COORD).await.unwrap();
    let records = h.auto_decisions("demo").unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].pick, Some(0));
    assert_eq!(records[0].choice, "B");
    assert_eq!(records[0].question.choices[0].letter.as_deref(), Some("B"));
    assert_eq!(records[0].outcome, "answered");
}
#[tokio::test]
async fn mixed_questions_keep_human_exceptions_open_and_harness_aliases_cannot_be_auto_answered() {
    let (h, _lab) = coordinator_host();
    h.auto_mode_set("demo", true, "local:cli").unwrap();
    let ids = question(
        &h,
        2,
        "## Questions\n### 1. 배치\n- A 아래 (추천)\n- B 위\n### 2. [사람] 권한 확인\n- A 허용\n- B 거부\n",
    );
    answer(&h, &ids[0], COORD).await.unwrap();
    assert!(answer(&h, &ids[1], COORD).await.unwrap_err().message.contains("사용자 전용"));
    assert_eq!(h.auto_exception_count("demo").unwrap(), 1);
    let pending = h.user_requests("demo").unwrap();
    assert_eq!(pending.len(), 1);
    assert!(pending[0].questions.as_ref().unwrap().draft.is_some());
    let harness = h.db.question_id("demo", "harness", "permission", "", 0).unwrap();
    assert!(answer(&h, &harness, COORD).await.unwrap_err().message.contains("확인창"));
    assert!(h.db.question_target(Some("demo"), &harness).unwrap().unwrap().draft.is_none());
    h.reply_by_id("demo", &ids[1], Some("A"), None, "device:phone").await.unwrap();
    assert!(h.user_requests("demo").unwrap().is_empty());
    assert_eq!(h.auto_decisions("demo").unwrap().len(), 1);
    let text = std::fs::read_to_string(h.ask_file("demo", &format!("auto-{}", ids[0]), "report.md").unwrap()).unwrap();
    assert!(text.contains("[사람]"));
}
#[tokio::test]
async fn auto_post_is_durable_not_pushed_and_reconsideration_only_notifies_coordinator() {
    let (h, _lab) = coordinator_host();
    h.auto_mode_set("demo", true, "local:cli").unwrap();
    let (tx, mut queue) = tokio::sync::mpsc::unbounded_channel();
    *h.notify_tx.lock().unwrap() = Some(tx);
    let mut post = input(
        3,
        r#"{"question":"색을 고르기","answer":"청록","reason":"시인성","action":"배치 지시함","reversible":true}"#,
    );
    post.topic = "auto-decision".into();
    post.post_type = PostType::Data;
    post.needs_user = false;
    assert!(matches!(h.front_post("demo", post.clone(), "local:cli").unwrap(), PostResult::Refused { .. }));
    post.post_id = "4".repeat(32);
    let first = h.front_post("demo", post.clone(), COORD).unwrap();
    let PostResult::Accepted { id } = &first else { panic!("{first:?}") };
    assert_eq!(h.front_post("demo", post, COORD).unwrap(), first);
    assert!(queue.try_recv().is_err());
    assert_eq!(h.auto_decisions("demo").unwrap().len(), 1);
    assert_eq!(h.auto_mode("demo").unwrap().decisions, 1);
    h.call(
        DeviceRole::Controller,
        "device:phone",
        FrontRequest::AutoDecisionReview {
            project: "demo".into(),
            id: id.clone(),
            answer: Some("보라로 바꿔라".into()),
        },
    )
    .await
    .unwrap();
    assert_eq!(h.auto_decisions("demo").unwrap()[0].choice, "청록");
    assert_eq!(h.auto_decisions("demo").unwrap()[0].reconsideration.as_deref(), Some("보라로 바꿔라"));
    assert!(h.inbox("demo", false, true).unwrap().items.iter().any(|i| i.event == "auto decision reconsider"));
    assert!(queue.try_recv().is_err());
    assert!(
        h.call(
            DeviceRole::Local,
            COORD,
            FrontRequest::AutoDecisionReview { project: "demo".into(), id: id.clone(), answer: None }
        )
        .await
        .is_err()
    );
}
#[tokio::test]
async fn mode_survives_host_reopen_and_project_rename() {
    let (h, lab) = coordinator_host();
    h.auto_mode_set("demo", true, "local:cli").unwrap();
    let mode = h.auto_mode("demo").unwrap();
    let cfg = h.cfg.clone();
    drop(h);
    let h = Host::open(cfg, lab.0.clone()).unwrap();
    assert_eq!(h.auto_mode("demo").unwrap(), mode);
    h.db.project_rename("demo", "next", |s| s.into()).unwrap();
    assert_eq!(h.auto_mode("next").unwrap(), mode);
    assert!(!h.auto_mode("demo").unwrap().on);
}

#[tokio::test]
async fn long_running_mode_pages_records_and_can_review_an_older_decision() {
    let (h, _lab) = coordinator_host();
    let d = Document {
        mode: AutoMode { on: true, run: 1, decisions: 75, ..Default::default() },
        decisions: (0..75)
            .map(|n| AutoDecision {
                id: format!("auto-q-{n:04}"),
                run: 1,
                subject: "coordinator".into(),
                question: ThreadQuestion {
                    source_number: None,
                    id: Some(format!("q-{n:04}")),
                    text: "질문".into(),
                    body: Some("자료".repeat(4000)),
                    choices: vec![],
                    after: None,
                },
                choice: "A".into(),
                pick: None,
                reason: "근거".into(),
                action: "기록".into(),
                reversible: Some(true),
                decided_by: "coordinator(auto)".into(),
                at_ms: n,
                outcome: "answered".into(),
                error: None,
                reviewed: false,
                reconsideration: None,
                files_base: None,
            })
            .collect(),
        forgotten: vec![],
    };
    h.db.set_meta(&key("demo"), &serde_json::to_string(&d).unwrap()).unwrap();
    let mut before = None;
    let mut seen = std::collections::BTreeSet::new();
    loop {
        let (page, next) = h.auto_decision_page("demo", before.as_deref(), None).unwrap();
        assert!(serde_json::to_vec(&page).unwrap().len() < 130 * 1024);
        for d in page {
            assert!(seen.insert(d.id));
        }
        if next.is_none() {
            break;
        }
        before = next;
    }
    assert_eq!(seen.len(), 75);
    assert!(serde_json::to_vec(&h.asks_view("demo").unwrap()).unwrap().len() < aaa_proto::MAX_FRAME);
    let r = h.auto_decision_review("demo", "auto-q-0000", Some("B".into()), "device:phone").unwrap();
    let FrontResponse::AutoDecisions { decisions, .. } = r else { panic!() };
    assert_eq!(decisions.len(), 1);
    assert_eq!(decisions[0].reconsideration.as_deref(), Some("B"));
    assert_eq!(h.auto_mode("demo").unwrap().decisions, 75);
}

/// 지우기는 원장·질문 사본과 자동 답만으로 된 「답한 것」 기록을 함께 지운다. 전달을 모르는 기록은 지워도 다시 자동 답하지 않는다.
#[tokio::test]
async fn forgetting_decisions_clears_every_list_but_never_reopens_an_auto_resend() {
    let (h, _lab) = coordinator_host();
    h.auto_mode_set("demo", true, "local:cli").unwrap();
    let first = question(&h, 1, "## Questions\n### 1. 배치\n- A 아래 (추천)\n- B 위\n").remove(0);
    let second = question(&h, 2, "## Questions\n### 1. 색\n- A 파랑\n- B 빨강\n").remove(0);
    answer(&h, &first, COORD).await.unwrap();
    answer(&h, &second, COORD).await.unwrap();
    let dir = h.project_dir("demo").unwrap().join("asks");
    let auto_only = |h: &Host| {
        h.db.ask_list("demo")
            .unwrap()
            .into_iter()
            .filter(|a| a.items.iter().any(|q| q.id.as_deref() == Some(first.as_str())))
            .count()
    };
    assert_eq!(auto_only(&h), 1);
    assert!(dir.join(format!("auto-{first}")).is_dir());
    let forget = |ids: Vec<String>, all: bool| FrontRequest::AutoDecisionForget { project: "demo".into(), ids, all };
    for (role, actor) in
        [(DeviceRole::Reader, "device:reader"), (DeviceRole::Local, COORD), (DeviceRole::Local, "worker:w")]
    {
        assert!(h.call(role, actor, forget(vec![format!("auto-{first}")], false)).await.is_err());
    }
    assert!(h.call(DeviceRole::Controller, "device:phone", forget(vec![], false)).await.is_err());
    let events = h.asks_changed.subscribe();
    let r = h.call(DeviceRole::Controller, "device:phone", forget(vec![format!("auto-{first}")], false)).await.unwrap();
    let FrontResponse::Asks { auto_decisions, done, .. } = r else { panic!() };
    assert_eq!(
        auto_decisions.iter().map(|d| d.question.id.clone().unwrap()).collect::<Vec<_>>(),
        std::slice::from_ref(&second)
    );
    assert!(done.iter().all(|a| a.items.iter().all(|q| q.id.as_deref() != Some(first.as_str()))));
    assert!(events.has_changed().unwrap());
    assert!(!dir.join(format!("auto-{first}")).exists());
    assert_eq!(auto_only(&h), 0);
    // 답한 질문은 지운 뒤에도 「이미 답한 질문」으로 막힌다.
    assert!(answer(&h, &first, COORD).await.is_err());
    // 전달 여부를 모르는 기록을 지워도 같은 ID 예약은 계속 막힌다.
    let pending = |id: &str| AutoDecision {
        id: id.into(),
        run: 0,
        subject: "coordinator".into(),
        question: ThreadQuestion {
            source_number: None,
            id: None,
            text: "q".into(),
            body: None,
            after: None,
            choices: vec![],
        },
        choice: "A".into(),
        pick: None,
        reason: "r".into(),
        action: "a".into(),
        reversible: None,
        decided_by: "coordinator(auto)".into(),
        at_ms: 1,
        outcome: "pending".into(),
        error: None,
        reviewed: false,
        reconsideration: None,
        files_base: None,
    };
    h.auto_reserve("demo", pending("auto-q-9999")).unwrap();
    h.call(DeviceRole::Controller, "device:phone", forget(vec![], true)).await.unwrap();
    assert!(h.auto_decisions("demo").unwrap().is_empty());
    assert!(h.auto_reserve("demo", pending("auto-q-9999")).is_err());
    assert!(
        h.db.ask_list("demo").unwrap().iter().all(|a| a.items.iter().all(|q| q.id.as_deref() != Some(second.as_str())))
    );
}

/// 자동 결정은 「답한 것」과 같은 14일이 지나면 ticker가 지운다. 전달을 모르는 결정은 질문이 열려 있는 동안 계속 자동 답을 막는다.
#[tokio::test]
async fn old_decisions_expire_like_answered_records_but_never_reopen_an_auto_resend() {
    let (h, _lab) = coordinator_host();
    h.auto_mode_set("demo", true, "local:cli").unwrap();
    let old = question(&h, 1, "## Questions\n### 1. 배치\n- A 아래 (추천)\n- B 위\n").remove(0);
    let recent = question(&h, 2, "## Questions\n### 1. 색\n- A 파랑\n- B 빨강\n").remove(0);
    let open = question(&h, 3, "## Questions\n### 1. 폭\n- A 좁게\n- B 넓게\n").remove(0);
    answer(&h, &old, COORD).await.unwrap();
    answer(&h, &recent, COORD).await.unwrap();
    let now = now_ms();
    let record = |id: &str, outcome: &str| {
        let mut r = h.auto_decisions("demo").unwrap()[0].clone();
        r.question.id = id.strip_prefix("auto-").map(Into::into);
        AutoDecision { id: id.into(), outcome: outcome.into(), at_ms: 1, ..r }
    };
    let mut d = h.auto_document("demo").unwrap();
    d.decisions.iter_mut().filter(|r| r.id == format!("auto-{old}")).for_each(|r| r.at_ms = 1);
    d.decisions.push(record(&format!("auto-{open}"), "failed"));
    d.decisions.push(record("auto-q-9999", "pending"));
    d.forgotten.push("p-00000001".into());
    h.db.set_meta(&key("demo"), &serde_json::to_string(&d).unwrap()).unwrap();
    let dir = h.project_dir("demo").unwrap().join("asks");
    let asked = |h: &Host, q: &str| {
        h.db.ask_list("demo").unwrap().iter().filter(|a| a.items.iter().any(|i| i.id.as_deref() == Some(q))).count()
    };
    assert_eq!((asked(&h, &old), asked(&h, &recent)), (1, 1));
    let events = h.asks_changed.subscribe();
    h.ask_prune_pass(now);
    h.auto_prune_pass(now);
    let d = h.auto_document("demo").unwrap();
    assert_eq!(d.decisions.iter().map(|r| r.id.clone()).collect::<Vec<_>>(), [format!("auto-{recent}")]);
    // 닫힌 질문(q-9999)과 post 번호 ID는 빠지고, 열린 질문의 실패 기록만 남는다.
    assert_eq!(d.forgotten, [format!("auto-{open}")]);
    assert!(events.has_changed().unwrap());
    assert!(!dir.join(format!("auto-{old}")).exists() && dir.join(format!("auto-{recent}")).is_dir());
    assert_eq!((asked(&h, &old), asked(&h, &recent)), (0, 1));
    assert!(answer(&h, &open, COORD).await.unwrap_err().message.contains("이미 자동 답"));
    // 사람이 답해 질문이 닫히면 다음 정리에서 빠진다.
    h.call(
        DeviceRole::Controller,
        "device:phone",
        FrontRequest::QuestionReply { project: "demo".into(), id: open.clone(), choice: Some("A".into()), text: None },
    )
    .await
    .unwrap();
    h.auto_prune_pass(now);
    assert!(h.auto_document("demo").unwrap().forgotten.is_empty());
    assert!(answer(&h, &open, COORD).await.is_err());
}

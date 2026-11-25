//! herdr-projects src/thread.rs `group()` 시험을 옮겼다(현재 보고 상태 판정에 맞췄다).
use super::*;

const NOW: u64 = 10_000_000;

#[test]
fn copy_notes_in_flow_keep_a_bounded_preview_and_the_total_count() {
    let notes = vec!["첨부 경로/한글.log: 복사 상한 초과".into(); 20000];
    let summary = copy_notes_summary(&notes);
    assert!(summary.len() < 9 * 1024);
    assert!(summary.starts_with(&notes[0]));
    assert!(summary.contains("총 20000건"));
    assert!(summary.contains("copy_notes"));
    assert_eq!(copy_notes_summary(&["one".into(), "two".into()]), "one; two");
}

fn open_thread() -> ThreadRec {
    ThreadRec {
        account_allocation: None,
        legacy: Default::default(),
        account: None,
        account_label: None,
        holds: vec![],
        started_by: None,
        over_limit: None,
        park: None,
        brief_line: None,
        left_places: Vec::new(),
        design: None,
        automation: None,
        automation_run: None,
        project: "p".into(),
        id: "t-0001".into(),
        title: "t".into(),
        machine_id: "m".into(),
        profile: "claude".into(),
        role: Role::Worker,
        kind: ThreadKind::default(),
        repo: None,
        base: None,
        branch: None,
        cwd: None,
        work_dir: None,
        pane: Some("pane".into()),
        launch_sent: false,
        launch_existing_slot: false,
        status: ThreadStatus::Open,
        brief: BriefState::Delivered,
        brief_attempts: 0,
        delivered_at_ms: None,
        delivered_report: None,
        launch: None,
        report_hash: None,
        pending_report_hash: None,
        report_copy_hash: None,
        report_stage: None,
        acked_hash: None,
        next: Vec::new(),
        questions: Vec::new(),
        question_answer: None,
        question_warning: None,
        question_warning_sent: None,
        question_evidence: None,
        question_missing: Vec::new(),
        copy: None,
        copy_failures: 0,
        copy_retry_at_ms: 0,
        copy_notes: Vec::new(),
        last_group: None,
        review_item_hash: None,
        group_changed_ms: None,
        report_changed_ms: None,
        error: None,
        resolve_pending: None,
        task_clean: TaskClean::default(),
        task_id: None,
        attachments: Vec::new(),
        prepare_started_ms: None,
        leftover: None,
        created_at_ms: NOW - 3_600_000,
        updated_at_ms: NOW,
    }
}

// 옛 저장 자료에 PR 상태가 있어도 현재 분류·출력의 근거가 되지 않는다.
#[test]
fn legacy_pr_metadata_does_not_override_report_groups() {
    let mut value = serde_json::to_value(with_report(open_thread())).unwrap();
    value["pr"] = serde_json::json!("https://github.com/o/app/pull/1");
    value["pr_state"] = serde_json::json!("OPEN");
    value["pr_review"] = serde_json::json!("APPROVED");
    value["last_group"] = serde_json::json!("landing");
    value["retired_mission"] = serde_json::json!("보존");
    let mut rec: ThreadRec = serde_json::from_value(value).unwrap();
    assert_eq!(rec.last_group, Some(ThreadGroup::ReadyForReview));
    assert_eq!(g(&rec, Some(agent(aaa_proto::AgentActivity::Idle, 0))), ThreadGroup::ReadyForReview);
    rec.acked_hash = rec.report_hash.clone();
    assert_eq!(g(&rec, Some(agent(aaa_proto::AgentActivity::Idle, 0))), ThreadGroup::Idle);
    let saved = serde_json::to_value(rec).unwrap();
    assert!(saved.get("pr").is_none());
    assert_eq!(saved["retired_mission"], "보존");
}

fn agent(activity: AgentActivity, secs: u64) -> AgentStatus {
    AgentStatus {
        activity,
        since_ms: NOW - secs * 1000,
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
    }
}

fn g(r: &ThreadRec, a: Option<AgentStatus>) -> ThreadGroup {
    thread_group(r, true, a.as_ref(), NOW)
}

fn with_report(r: ThreadRec) -> ThreadRec {
    ThreadRec { report_hash: Some("h".into()), ..r }
}

use AgentActivity::*;

#[test]
fn row1_resolved_wins_over_everything() {
    let t = ThreadRec { status: ThreadStatus::Resolved, brief: BriefState::Pending, ..open_thread() };
    assert_eq!(g(&t, Some(agent(Question, 999))), ThreadGroup::Resolved);
}

#[test]
fn row2_starting_is_working_for_five_minutes() {
    let young = ThreadRec { status: ThreadStatus::Starting, created_at_ms: NOW - 10_000, ..open_thread() };
    assert_eq!(g(&young, None), ThreadGroup::Working);
    let old = ThreadRec { status: ThreadStatus::Starting, created_at_ms: NOW - 301_000, ..open_thread() };
    assert_eq!(g(&old, None), ThreadGroup::WaitingOnYou);
}

/// herdr-projects threads.rs `restart_case_*`: 아무것도 못 만든 실패는 자리부터(a), 자리가 있으면 다시 띄우고(e),
/// 실행 중(d)·시작 중·끝낸 thread는 거부한다. 이을 대화가 없으면 --resume도 거부한다.
#[test]
fn restart_plan_follows_herdr_projects() {
    let launch = aaa_proto::Launch {
        profile: "claude".into(),
        harness: "claude".into(),
        argv: vec!["claude".into()],
        env: Vec::new(),
        resume_argv: Vec::new(),
        session_key: None,
    };
    let failed = ThreadRec { status: ThreadStatus::Failed, pane: None, ..open_thread() };
    assert_eq!(restart_plan(&failed, false, false, false), Ok(RestartPlan::Create));
    let e = restart_plan(&failed, true, false, false).unwrap_err();
    assert!(e.message.contains("이을 대화가 없다"), "{e:?}");
    let launched = ThreadRec { launch: Some(launch), ..failed.clone() };
    assert_eq!(restart_plan(&launched, false, false, false), Ok(RestartPlan::Reopen));
    let ended = ThreadRec { status: ThreadStatus::Open, ..launched.clone() };
    assert_eq!(restart_plan(&ended, true, false, false), Ok(RestartPlan::Reopen));
    assert!(restart_plan(&ended, false, true, false).unwrap_err().message.contains("aaa pane kill"));
    let starting = ThreadRec { status: ThreadStatus::Starting, ..failed.clone() };
    assert!(restart_plan(&starting, false, false, false).unwrap_err().message.contains("시작 중"));
    let resolved = ThreadRec { status: ThreadStatus::Resolved, ..launched };
    assert!(restart_plan(&resolved, false, false, false).unwrap_err().message.contains("--reopen"));
    // 보류는 이어 하기(`thread unpark`)만 다시 띄운다
    let parked = ThreadRec { status: ThreadStatus::Parked, ..ended };
    assert!(restart_plan(&parked, true, false, false).unwrap_err().message.contains("thread unpark"));
    assert_eq!(restart_plan(&parked, true, false, true), Ok(RestartPlan::Reopen));
}

#[test]
fn row3_waiting_on_you() {
    let failed = ThreadRec { status: ThreadStatus::Failed, ..open_thread() };
    assert_eq!(g(&failed, Some(agent(Working, 0))), ThreadGroup::WaitingOnYou);

    let pending = ThreadRec { brief: BriefState::Pending, ..open_thread() };
    assert_eq!(g(&pending, Some(agent(Question, 60))), ThreadGroup::WaitingOnYou);
    assert_eq!(g(&pending, Some(agent(Error, 60))), ThreadGroup::WaitingOnYou);
    let stuck = ThreadRec { brief: BriefState::Unknown, ..open_thread() };
    assert_eq!(g(&stuck, Some(agent(Idle, 0))), ThreadGroup::WaitingOnYou);

    assert_eq!(thread_group(&open_thread(), false, None, NOW), ThreadGroup::WaitingOnYou);
    assert_eq!(g(&open_thread(), Some(agent(Question, 30))), ThreadGroup::WaitingOnYou);

    let copy = ThreadRec { copy: Some("failed".into()), ..open_thread() };
    assert_eq!(g(&copy, Some(agent(Working, 0))), ThreadGroup::Working);
    let copy = ThreadRec { last_group: Some(ThreadGroup::Working), ..copy };
    assert_eq!(g(&copy, Some(agent(Idle, 100))), ThreadGroup::Working);
    assert_eq!(g(&copy, Some(agent(Question, 30))), ThreadGroup::WaitingOnYou);
    let legacy = ThreadRec { last_group: Some(ThreadGroup::WaitingOnYou), ..copy };
    assert_eq!(g(&legacy, Some(agent(Working, 0))), ThreadGroup::Working);
}

#[test]
fn row4_working_including_a_launch_in_progress() {
    assert_eq!(g(&open_thread(), Some(agent(Working, 0))), ThreadGroup::Working);
    // 금방 답한 확인창은 확인 필요로 보이지 않는다
    assert_eq!(g(&open_thread(), Some(agent(Question, 29))), ThreadGroup::Working);
    // 새 thread는 입력을 받을 상태가 아닌 채 60초가 지나기 전까지 작업 중이다
    let pending = ThreadRec { brief: BriefState::Pending, ..open_thread() };
    assert_eq!(g(&pending, None), ThreadGroup::Working);
    assert_eq!(g(&pending, Some(agent(Error, 59))), ThreadGroup::Working);
    assert_eq!(g(&pending, Some(agent(Idle, 500))), ThreadGroup::Working);
    assert_eq!(g(&pending, Some(agent(Started, 500))), ThreadGroup::Working);
}

#[test]
fn ready_for_review_until_ack() {
    let t = with_report(open_thread());
    assert_eq!(g(&t, Some(agent(Idle, 0))), ThreadGroup::ReadyForReview);
    let acked = ThreadRec { acked_hash: Some("h".into()), ..t.clone() };
    assert_eq!(g(&acked, Some(agent(Idle, 0))), ThreadGroup::Idle);
    // brief를 아직 못 보냈으면 보고가 있어도 작업 중
    let pending = ThreadRec { brief: BriefState::Pending, ..t };
    assert_eq!(g(&pending, Some(agent(Idle, 0))), ThreadGroup::Working);
}

#[test]
fn row7_idle_and_precedence() {
    assert_eq!(g(&open_thread(), Some(agent(Idle, 0))), ThreadGroup::Idle);
    // 작업 중이 새 보고를 이긴다
    let t = with_report(open_thread());
    assert_eq!(g(&t, Some(agent(Working, 0))), ThreadGroup::Working);
    // 오래 뜬 확인창이 새 보고를 이긴다
    assert_eq!(g(&t, Some(agent(Question, 31))), ThreadGroup::WaitingOnYou);
}

#[test]
fn a_just_delivered_brief_is_working_until_a_report_or_a_tick() {
    let t = ThreadRec { delivered_at_ms: Some(NOW - 5_000), ..open_thread() };
    assert_eq!(g(&t, None), ThreadGroup::Working);
    assert_eq!(g(&t, Some(agent(Idle, 0))), ThreadGroup::Working);
    let later = ThreadRec { delivered_at_ms: Some(NOW - 16_000), ..open_thread() };
    assert_eq!(g(&later, None), ThreadGroup::Idle);
    // 창 길이는 설정을 따른다(host.toml `[projects] delivered_working_secs`)
    assert_eq!(thread_group_with(&t, true, None, NOW, 4_000, STARTING_TIMEOUT_MS), ThreadGroup::Idle);
    // 새 보고가 오면 기다리지 않는다: 검토 대기, 확인하면 쉼
    let reported = with_report(t);
    assert_eq!(g(&reported, None), ThreadGroup::ReadyForReview);
    let acked = ThreadRec { acked_hash: Some("h".into()), ..reported };
    assert_eq!(g(&acked, None), ThreadGroup::Idle);
}

#[test]
fn a_closed_pane_needs_you_only_without_a_report() {
    let t = with_report(open_thread());
    assert_eq!(thread_group(&t, false, None, NOW), ThreadGroup::ReadyForReview);
    let acked = ThreadRec { acked_hash: Some("h".into()), ..t };
    assert_eq!(thread_group(&acked, false, None, NOW), ThreadGroup::Idle);
    // 전송 전 보고밖에 없어도 종료된 pane은 유예 창 동안 일하지 않는다.
    let recent = ThreadRec { delivered_at_ms: Some(NOW), delivered_report: Some("h".into()), ..acked };
    assert_eq!(thread_group(&recent, true, None, NOW), ThreadGroup::Working);
    assert_eq!(thread_group(&recent, false, None, NOW), ThreadGroup::Idle);
    assert_eq!(thread_group(&open_thread(), false, None, NOW), ThreadGroup::WaitingOnYou);
}

fn reported(activity: &str, percent: Option<u8>, age: u64, state: AgentActivity) -> Option<AgentStatus> {
    Some(AgentStatus {
        progress: Some(activity.into()),
        percent,
        progress_at_ms: Some(NOW - age * 1000),
        progress_since_ms: None,
        ..agent(state, 0)
    })
}

#[test]
fn self_reports_feed_the_group() {
    // 사람을 기다린다고 알렸는데 harness는 쉰다: 확인 필요
    assert_eq!(g(&open_thread(), reported(WAITING, Some(40), 5, Idle)), ThreadGroup::WaitingOnYou);
    // 기다림이 새 보고를 이기지만, 다시 일하는 harness는 이기지 못한다
    let t = with_report(open_thread());
    assert_eq!(g(&t, reported(WAITING, None, 5, Idle)), ThreadGroup::WaitingOnYou);
    assert_eq!(g(&t, reported(WAITING, None, 5, Working)), ThreadGroup::Working);
    // 100% 아래이고 새것이면 도구 호출 사이에도 작업 중
    assert_eq!(g(&open_thread(), reported("시험 중", Some(55), 30, Idle)), ThreadGroup::Working);
    // 5분이 지나면 묵은 것이고, 100%는 끝난 것
    assert_eq!(g(&open_thread(), reported("시험 중", Some(55), 400, Idle)), ThreadGroup::Idle);
    assert_eq!(g(&open_thread(), reported("끝", Some(100), 5, Idle)), ThreadGroup::Idle);
    // 진행을 보고하는 agent의 새 보고는 `Done`일 때만 검토 대기다(plan §8.85, 참고는 새 보고가 이긴다):
    // 배경 명령을 기다리며 턴을 끝낸 중간 보고는 작업 중, 묵으면 쉼이다.
    assert_eq!(g(&t, reported("CI 기다림", Some(90), 5, Idle)), ThreadGroup::Working);
    assert_eq!(g(&t, reported("CI 기다림", None, 5, Idle)), ThreadGroup::Working);
    assert_eq!(g(&t, reported("CI 기다림", Some(90), 400, Idle)), ThreadGroup::Idle);
    assert_eq!(g(&t, reported("Done", Some(100), 5, Idle)), ThreadGroup::ReadyForReview);
    // pane이 닫혔으면 더 보고할 수 없다: 참고대로 검토 대기
    assert_eq!(
        thread_group(&t, false, reported("CI 기다림", Some(90), 5, Idle).as_ref(), NOW),
        ThreadGroup::ReadyForReview
    );
    // 진행을 보고하지 않는 harness는 참고대로
    assert_eq!(g(&t, Some(agent(Idle, 0))), ThreadGroup::ReadyForReview);
}

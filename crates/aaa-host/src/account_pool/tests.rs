use super::*;
use std::sync::Barrier;

const NOW: u64 = 10_000_000;

struct Lab {
    h: Arc<Host>,
    _root: Cleanup,
}
struct Cleanup(std::path::PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
impl Lab {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("aaa-pool-{}", util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = root.join("projects").display().to_string();
        cfg.notify.web_push = false;
        let h = Host::open(cfg, root.join("host")).unwrap();
        h.project_create("demo", Default::default(), "local:cli").unwrap();
        Self { h, _root: Cleanup(root) }
    }
    fn configure(&self, candidates: &[Candidate]) -> AccountPoolView {
        let pool = AccountPoolView {
            revision: "revision-a".into(),
            projects: vec!["demo".into()],
            members: candidates.iter().map(|c| c.member.clone()).collect(),
        };
        self.h.db.set_meta(CONFIG, &serde_json::to_string(&pool).unwrap()).unwrap();
        pool
    }
    fn seed(&self, account: &str, allocation: Allocation, status: ThreadStatus, sent: bool) -> String {
        self.h
            .db
            .thread_insert_next("demo", |id| {
                let mut r = ThreadRec::starting("demo", "fixture", "m", "claude", Role::Worker, ThreadKind::Dir, NOW);
                r.id = id.into();
                r.account = Some(account.into());
                r.account_allocation = Some(allocation);
                r.status = status;
                r.launch_sent = sent;
                serde_json::to_string(&r).unwrap()
            })
            .unwrap()
    }
}

fn candidate(name: &str, five: f64, week: f64) -> Candidate {
    let windows = [(300, five), (10080, week)]
        .into_iter()
        .map(|(duration, used)| AccountQuotaWindow {
            window: QuotaWindow {
                limit_id: "claude".into(),
                window_id: format!("w-{duration}"),
                scope: None,
                duration_mins: Some(duration),
                used_percent: used,
                observed_at_ms: NOW,
                resets_at_ms: Some(NOW + u64::from(duration) * 60000),
            },
            machine: "mac-a".into(),
            source: QuotaSource::ClaudeStatusline,
            received_at_ms: NOW,
            freshness: QuotaFreshness::Fresh,
            problem: None,
            conflict: false,
        })
        .collect();
    Candidate {
        member: AccountPoolMember { account: name.into(), group: name.into(), models: vec![] },
        generation: format!("generation-{name}"),
        quota: Some(AccountQuota {
            account: name.into(),
            harness: "claude".into(),
            windows,
            problem: None,
            retry_at_ms: None,
        }),
    }
}
fn unknown(name: &str) -> Candidate {
    let mut c = candidate(name, 0.0, 0.0);
    let q = c.quota.as_mut().unwrap();
    q.windows.clear();
    q.problem = Some(QuotaProblem::Unknown);
    c
}
fn input(lab: &Lab, candidates: Vec<Candidate>) -> SelectionInput {
    SelectionInput {
        pool: lab.configure(&candidates),
        project: "demo".into(),
        model: Some("model-a".into()),
        auto: true,
        size: AccountTaskSize::Medium,
        candidates,
        excluded: vec![],
    }
}
fn choose(lab: &Lab, input: &SelectionInput) -> anyhow::Result<(String, Allocation)> {
    input.select(&lab.h.db.conn_for_pool_test(), NOW)
}

#[test]
fn bottleneck_reservations_ties_and_model_windows_choose_explainable_accounts() {
    let l = Lab::new();
    let p = input(&l, vec![candidate("a", 30.0, 60.0), candidate("b", 50.0, 40.0)]);
    let (account, allocation) = choose(&l, &p).unwrap();
    assert_eq!(account, "b");
    assert!(!allocation.selection.unobserved);
    l.seed(&account, allocation, ThreadStatus::Open, true);
    assert_eq!(choose(&l, &p).unwrap().0, "a", "예약을 가장 작은 여유 창에서도 뺀다");
    let mut p = input(&l, vec![candidate("b", 50.0, 50.0), candidate("a", 50.0, 50.0)]);
    // 다른 모델 전용 숫자를 더 큰 공용 잔량으로 합치지 않는다.
    for w in &mut p.candidates[1].quota.as_mut().unwrap().windows {
        w.window.scope = Some("other-model".into());
    }
    assert_eq!(choose(&l, &p).unwrap().0, "b");
}

#[test]
fn fresh_candidates_precede_blind_first_runs_and_first_runs_are_not_repeated() {
    let l = Lab::new();
    let mut p = input(&l, vec![unknown("cold"), candidate("known", 20.0, 30.0)]);
    assert_eq!(choose(&l, &p).unwrap().0, "known");
    for w in &mut p.candidates[1].quota.as_mut().unwrap().windows {
        w.window.used_percent = 99.0;
    }
    let (name, allocation) = choose(&l, &p).unwrap();
    assert_eq!(name, "cold");
    assert!(allocation.selection.unobserved);
    let id = l.seed(&name, allocation, ThreadStatus::Starting, true);
    assert!(choose(&l, &p).unwrap_err().to_string().contains("첫 실행 관측 대기"));
    // host 재시작처럼 cache가 없어도 영속 실행 기록으로 첫 실행을 중복하지 않는다.
    let stored = l.h.load("demo", &id).unwrap();
    assert!(stored.account_allocation.unwrap().selection.unobserved);
    l.h.update_rec("demo", &id, |r| r.status = ThreadStatus::Resolved).unwrap();
    assert!(choose(&l, &p).is_err(), "관측 없는 첫 실행을 종료로 되풀이하지 않는다");
}

#[test]
fn stale_failed_reset_missing_and_invalid_readings_do_not_become_blind_capacity() {
    for mode in ["stale", "failed", "reset", "partial", "nan", "clock", "no-reset"] {
        let l = Lab::new();
        let mut c = candidate("work", 20.0, 20.0);
        let q = c.quota.as_mut().unwrap();
        match mode {
            "stale" => q.windows[0].window.observed_at_ms = NOW - 120001,
            "failed" => q.problem = Some(QuotaProblem::Failed),
            "reset" => q.windows[0].window.resets_at_ms = Some(NOW),
            "partial" => {
                q.windows.pop();
            }
            "nan" => q.windows[0].window.used_percent = f64::NAN,
            "clock" => q.windows[0].window.observed_at_ms = NOW + 30001,
            "no-reset" => q.windows[0].window.resets_at_ms = None,
            _ => unreachable!(),
        }
        let p = input(&l, vec![c]);
        assert!(choose(&l, &p).is_err(), "{mode}");
    }
    let l = Lab::new();
    let mut c = unknown("work");
    c.quota.as_mut().unwrap().problem = Some(QuotaProblem::RateLimited);
    let p = input(&l, vec![c]);
    assert!(choose(&l, &p).is_err());
}

#[test]
fn shared_groups_cannot_bootstrap_around_observed_limits() {
    let l = Lab::new();
    let mut cold = unknown("cold");
    cold.member.group = "shared".into();
    let mut full = candidate("full", 95.0, 20.0);
    full.member.group = "shared".into();
    let p = input(&l, vec![cold, full]);
    assert!(choose(&l, &p).is_err());
    let mut loose = candidate("loose", 10.0, 10.0);
    loose.member.group = "shared".into();
    let mut tight = candidate("tight", 95.0, 20.0);
    tight.member.group = "shared".into();
    let p = input(&l, vec![loose, tight]);
    assert!(choose(&l, &p).is_err(), "같은 그룹의 다른 별칭 숫자도 제약");
}

#[test]
fn concurrent_shared_budget_and_receipt_are_atomic_across_projects() {
    // 지금 host는 프로젝트 하나만 연다. 옛 프로젝트의 DB 기록도 잔량 셈에서 빠지면 안 된다.
    let l = Lab::new();
    let mut a = candidate("a", 50.0, 50.0);
    a.member.group = "shared".into();
    let mut b = candidate("b", 50.0, 50.0);
    b.member.group = "shared".into();
    let mut p = input(&l, vec![a, b]);
    p.pool.projects.push("other".into());
    l.h.db.set_meta(CONFIG, &serde_json::to_string(&p.pool).unwrap()).unwrap();
    let p = Arc::new(p);
    let gate = Arc::new(Barrier::new(4));
    let jobs: Vec<_> = (0..4)
        .map(|n| {
            let h = l.h.clone();
            let gate = gate.clone();
            let p = p.clone();
            std::thread::spawn(move || {
                gate.wait();
                let project = if n % 2 == 0 { "demo" } else { "other" };
                h.db.thread_insert_transaction(project, Some(&format!("once-{n}")), None, None, None, |tx, id| {
                    let (name, allocation) = p.select(tx, NOW)?;
                    let mut r = ThreadRec::starting(project, "job", "m", "claude", Role::Worker, ThreadKind::Dir, NOW);
                    r.id = id.into();
                    r.account = Some(name);
                    r.account_allocation = Some(allocation);
                    r.launch_sent = true;
                    Ok(serde_json::to_string(&r)?)
                })
            })
        })
        .collect();
    let results: Vec<_> = jobs.into_iter().map(|j| j.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 2, "공유 잔량 50 - 안전 여유10에 예약15 두 개만 허용");
    assert_eq!(l.h.db.threads(None).unwrap().len(), 2);
    let (n, _) = results.iter().enumerate().find(|(_, r)| r.is_ok()).unwrap();
    let key = format!("once-{n}");
    let project = if n % 2 == 0 { "demo" } else { "other" };
    let replay =
        l.h.db
            .thread_insert_transaction(project, Some(&key), None, None, None, |_, _| {
                panic!("같은 영수증에서 재선택하면 안 됨")
            })
            .unwrap();
    assert!(!replay.1);
    assert_eq!(l.h.db.threads(None).unwrap().len(), 2);
}

#[test]
fn definite_preparation_failure_releases_but_unknown_spawn_keeps_budget() {
    let l = Lab::new();
    let p = input(&l, vec![candidate("a", 60.0, 60.0)]);
    let (name, a) = choose(&l, &p).unwrap();
    let id = l.seed(&name, a.clone(), ThreadStatus::Failed, false);
    assert!(choose(&l, &p).is_ok(), "headroom 0 경계의 두 번째 예약은 허용한다");
    let other = l.seed(&name, a, ThreadStatus::Failed, false);
    assert!(choose(&l, &p).is_err());
    l.h.pool_reconcile(&AccountQuotaReport::default()).unwrap();
    assert!(choose(&l, &p).is_ok(), "spawn 전 실패는 즉시 해제");
    for id in [id, other] {
        l.h.update_rec("demo", &id, |r| {
            r.status = ThreadStatus::Starting;
            r.launch_sent = true;
            r.account_allocation.as_mut().unwrap().released = false;
        })
        .unwrap();
    }
    l.h.pool_reconcile(&AccountQuotaReport::default()).unwrap();
    assert!(choose(&l, &p).is_err(), "응답 유실은 해제하지 않음");
}

#[tokio::test]
async fn pool_changes_require_human_control_and_compare_revisions() {
    let l = Lab::new();
    l.h.accounts.import("work", "claude", "fixture-pool-token-01234567890123456789").unwrap();
    let request = FrontRequest::AccountPoolMember {
        revision: "".into(),
        account: "work".into(),
        member: Some(AccountPoolMember {
            account: "work".into(),
            group: "shared".into(),
            models: vec!["model-a".into()],
        }),
    };
    for (role, actor) in [
        (DeviceRole::Reader, "device:reader"),
        (DeviceRole::Local, "local:cli@pane"),
        (DeviceRole::Local, "worker:demo/t-0001"),
        (DeviceRole::Local, "detached:cli"),
    ] {
        assert_eq!(l.h.call(role, actor, request.clone()).await.unwrap_err().code, ErrorCode::Forbidden);
    }
    l.h.call(DeviceRole::Controller, "device:control", request.clone()).await.unwrap();
    assert_eq!(
        l.h.call(DeviceRole::Controller, "device:control", request).await.unwrap_err().code,
        ErrorCode::Conflict
    );
    let pool = l.h.account_pool().unwrap();
    l.h.pool_project_set(&pool.revision, "demo", true, "local:cli").unwrap();
    assert!(l.h.pool_project_enabled("demo").unwrap());
    l.h.pool_forget("work").unwrap();
    assert!(l.h.account_pool().unwrap().members.is_empty());
}

#[test]
fn ended_reservations_wait_for_observations_after_exit_and_survive_relogin() {
    let l = Lab::new();
    l.h.accounts.import("work", "claude", "fixture-end-canary-01234567890123456789").unwrap();
    let mut p = input(&l, vec![candidate("work", 60.0, 60.0)]);
    p.candidates[0].generation = l.h.accounts.generation("work").unwrap();
    let (name, mut allocation) = choose(&l, &p).unwrap();
    let now = util::now_ms();
    allocation.ended_at_ms = Some(now - 1);
    let id = l.seed(&name, allocation, ThreadStatus::Resolved, true);
    let mut q = p.candidates[0].quota.clone().unwrap();
    for w in &mut q.windows {
        w.window.observed_at_ms = now - 2;
        w.window.resets_at_ms = Some(now + 3_600_000);
    }
    l.h.pool_reconcile(&AccountQuotaReport { accounts: vec![q.clone()] }).unwrap();
    assert!(
        !l.h.load("demo", &id).unwrap().account_allocation.unwrap().released,
        "종료 전 관측은 예약을 대체하지 않는다"
    );
    l.h.accounts.import("work", "claude", "fixture-end-renewed-01234567890123456789").unwrap();
    for w in &mut q.windows {
        w.window.observed_at_ms = now;
    }
    l.h.pool_reconcile(&AccountQuotaReport { accounts: vec![q] }).unwrap();
    assert!(
        l.h.load("demo", &id).unwrap().account_allocation.unwrap().released,
        "끝난 실행의 예약은 다시 로그인 뒤에도 종료 이후의 관측으로 교체한다"
    );
}

#[tokio::test]
async fn host_filters_permitted_models_before_allowing_an_unobserved_first_run() {
    let l = Lab::new();
    for name in ["one", "two"] {
        l.h.accounts.import(name, "claude", &format!("fixture-model-{name}-01234567890123456789")).unwrap();
    }
    let mut one = unknown("one");
    one.member.models = vec!["model-a".into()];
    let mut two = unknown("two");
    two.member.models = vec!["model-b".into()];
    l.configure(&[one, two]);
    let profile = ResolvedProfile {
        harness: "claude".into(),
        model: "model-a".into(),
        effort: "high".into(),
        args: vec![],
        command: vec![],
        env: Default::default(),
    };
    let prepared = l.h.account_selection_input("demo", &profile, None, AccountTaskSize::Medium).await.unwrap().unwrap();
    assert_eq!(choose(&l, &prepared).unwrap().0, "one");
    assert!(prepared.excluded.iter().any(|s| s.contains("two: 허용 모델")));
}

#[test]
fn project_rename_and_delete_move_jobs_without_touching_global_pool_or_observations() {
    let l = Lab::new();
    l.configure(&[unknown("demo")]);
    queued_job(&l, "pending");
    l.h.db.set_meta(&receipt("demo", "pending"), "t-0001").unwrap();
    l.h.db.set_meta("account-quota-v1/demo", "observation").unwrap();
    l.h.db.project_rename("demo", "v1", str::to_string).unwrap();
    assert_eq!(l.h.account_pool().unwrap().projects, ["v1"]);
    let moved = l.h.pool_job("v1", "pending").unwrap();
    assert_eq!(moved.args.project, "v1");
    assert_eq!(moved.view.project, "v1");
    assert_eq!(l.h.db.meta(&receipt("v1", "pending")).unwrap().as_deref(), Some("t-0001"));
    l.h.db.project_forget("v1").unwrap();
    assert!(l.h.account_pool().unwrap().projects.is_empty());
    assert_eq!(l.h.account_pool().unwrap().members.len(), 1);
    assert!(l.h.pool_job("v1", "pending").is_err());
    assert!(l.h.db.meta(&receipt("v1", "pending")).unwrap().is_none());
    assert_eq!(l.h.db.meta("account-quota-v1/demo").unwrap().as_deref(), Some("observation"));
}

#[test]
fn regrouping_an_alias_does_not_erase_its_existing_reservation() {
    let l = Lab::new();
    let p = input(&l, vec![candidate("a", 60.0, 60.0)]);
    let (name, allocation) = choose(&l, &p).unwrap();
    l.seed(&name, allocation, ThreadStatus::Open, true);
    let mut a = candidate("a", 60.0, 60.0);
    a.member.group = "shared".into();
    let mut b = candidate("b", 60.0, 60.0);
    b.member.group = "shared".into();
    let p = input(&l, vec![a, b]);
    let (name, allocation) = choose(&l, &p).unwrap();
    l.seed(&name, allocation, ThreadStatus::Open, true);
    assert!(choose(&l, &p).is_err(), "기존 그룹에서 새 그룹으로 옮겨도 두 예약이 함께 남는다");
}

// 사용량 조회의 실제 await를 보류해 pool_run·웹 요청 owner의 겹침을 고정한다.
#[tokio::test]
async fn cancel_waits_for_allocation_and_cannot_be_overwritten_or_restarted() {
    let l = Lab::new();
    l.configure(&[]);
    l.h.agent_connected(
        &AgentHello {
            proto: AGENT_LINK,
            machine_id: "m".into(),
            machine_name: "mac-a".into(),
            boot_id: "boot".into(),
            os: "test".into(),
            arch: "test".into(),
            software: "test".into(),
            features: vec!["host_profiles.v1".into(), "account_auth.v1".into()],
            dev_mode: true,
            sha256: None,
            selfcheck: None,
            sessions: vec![],
            identity: None,
            harness: vec![],
        },
        true,
        crate::AgentConn::new(aaa_common::conn::spawn_writer(tokio::io::sink(), MAX_FRAME), 1),
    );
    let mut job = queued_job(&l, "cancel-during-pass");
    job.profile_revision = l.h.profile_revision(&job.args).unwrap();
    l.h.db.set_meta(&job_key("demo", &job.view.id), &serde_json::to_string(&job).unwrap()).unwrap();
    let quota = l.h.account_quota.lock().await;
    let pass = l.h.pool_run("demo", &job.view.id, true);
    tokio::pin!(pass);
    assert!(futures_util::poll!(&mut pass).is_pending());
    assert!(l.h.claim_thread("demo", "account-cancel-during-pass").is_err(), "실제 배정 경로가 잠금을 잡았다");
    let cancel = l.h.call(
        DeviceRole::Controller,
        "device:control",
        FrontRequest::AccountStartCancel { project: "demo".into(), id: job.view.id.clone() },
    );
    tokio::pin!(cancel);
    let first = futures_util::poll!(&mut cancel);
    assert!(first.is_pending(), "배정 중 취소는 잠금을 기다려야 한다: {first:?}");
    drop(quota);
    // 실제 pool_run이 취소보다 먼저 읽은 사본으로 Queued를 저장한다.
    let result = pass.await.unwrap();
    assert!(matches!(result, FrontResponse::AccountStart { start } if start.state == AccountStartState::Queued));
    let result = cancel.await.unwrap();
    assert!(matches!(result, FrontResponse::AccountStart { start } if start.state == AccountStartState::Cancelled));
    l.h.pool_pass("demo").await.unwrap();
    let replay = l.h.pool_run("demo", &job.view.id, true).await.unwrap();
    assert!(matches!(replay, FrontResponse::AccountStart { start } if start.state == AccountStartState::Cancelled));
    assert!(l.h.db.threads(None).unwrap().is_empty());
    // cache가 아닌 DB를 다시 열어 취소의 내구성도 확인한다.
    let reopened = Host::open(l.h.cfg.clone(), l._root.0.join("host")).unwrap();
    assert_eq!(reopened.pool_job("demo", &job.view.id).unwrap().view.state, AccountStartState::Cancelled);
}

fn queued_job(l: &Lab, id: &str) -> Job {
    let job = Job {
        args: StartArgs {
            pool: Some(PoolRequest { id: id.into(), size: AccountTaskSize::Medium }),
            over_limit: None,
            reservation: None,
            account: None,
            holds: vec![],
            project: "demo".into(),
            title: "대기".into(),
            machine: Some("m".into()),
            profile: "claude".into(),
            kind: ThreadKind::Dir,
            repo: None,
            base: None,
            task: "task".into(),
            role: Role::Worker,
            attachments: vec![],
        },
        actor: "local:cli".into(),
        picked: None,
        profile_revision: "fixture".into(),
        view: AccountStartView {
            id: id.into(),
            project: "demo".into(),
            title: "대기".into(),
            size: AccountTaskSize::Medium,
            state: AccountStartState::Queued,
            thread: None,
            reason: "대기".into(),
            created_at_ms: NOW,
        },
    };
    l.h.db.set_meta(&job_key("demo", id), &serde_json::to_string(&job).unwrap()).unwrap();
    job
}

#[tokio::test]
async fn cancel_rechecks_a_receipt_committed_by_the_inflight_allocation() {
    let l = Lab::new();
    let job = queued_job(&l, "allocated-during-cancel");
    let guard = l.h.claim_thread("demo", "account-allocated-during-cancel").unwrap();
    let cancel = l.h.pool_cancel("demo", &job.view.id, "local:cli");
    tokio::pin!(cancel);
    assert!(futures_util::poll!(&mut cancel).is_pending());
    l.h.db.set_meta(&receipt("demo", &job.view.id), "t-0001").unwrap();
    drop(guard);
    let error = cancel.await.unwrap_err();
    assert_eq!(error.code, ErrorCode::Conflict);
    assert_eq!(error.message, "이미 시작한 thread는 thread에서 끝낸다");
    assert_eq!(l.h.pool_job("demo", &job.view.id).unwrap().view.state, AccountStartState::Queued);
    assert_eq!(l.h.db.meta(&receipt("demo", &job.view.id)).unwrap().as_deref(), Some("t-0001"));
}

#[tokio::test]
async fn cancel_preserves_reader_denial_missing_jobs_and_repeated_cancellation() {
    let l = Lab::new();
    let job = queued_job(&l, "human-cancel");
    let request = FrontRequest::AccountStartCancel { project: "demo".into(), id: job.view.id.clone() };
    // Reader의 쓰기 거부는 배정 잠금이 잡혀 있어도 기다리지 않고 끝나야 한다.
    let guard = l.h.claim_thread("demo", "account-human-cancel").unwrap();
    assert_eq!(
        l.h.call(DeviceRole::Reader, "device:reader", request.clone()).await.unwrap_err().code,
        ErrorCode::Forbidden
    );
    drop(guard);
    assert_eq!(l.h.pool_cancel("demo", "missing", "local:cli").await.unwrap_err().code, ErrorCode::NotFound);
    for _ in 0..2 {
        let result = l.h.call(DeviceRole::Controller, "device:control", request.clone()).await.unwrap();
        assert!(matches!(result, FrontResponse::AccountStart { start } if start.state == AccountStartState::Cancelled));
    }
}

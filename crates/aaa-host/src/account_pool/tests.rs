use super::*;
use std::sync::Barrier;

const NOW: u64 = 10_000_000;

struct Lab {
    h: Arc<Host>,
    _root: Cleanup,
}
struct Cleanup(std::path::PathBuf, std::sync::Weak<Host>);
impl Drop for Cleanup {
    fn drop(&mut self) {
        assert!(self.1.upgrade().is_none(), "임시 폴더를 지우기 전에 Host와 DB가 닫혀야 한다");
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
        let weak = Arc::downgrade(&h);
        Self { h, _root: Cleanup(root, weak) }
    }
    fn configure(&self, candidates: &[Candidate]) -> AccountPoolView {
        let pool = AccountPoolView {
            revision: "revision-a".into(),
            projects: vec![],
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
        model: Some("model-a".into()),
        auto: true,
        size: AccountTaskSize::Medium,
        candidates,
        excluded: vec![],
        required_account: None,
        minimum: [None, None],
    }
}

fn image_job(account: String, allocation: Allocation, id: &str) -> crate::image::Job {
    crate::image::Job {
        begin: ImageBegin {
            id: id.into(),
            prompt: "fixture".into(),
            profile: "image".into(),
            account: None,
            transparent: false,
            inputs: vec![],
        },
        machine: "m".into(),
        session: "s".into(),
        project: "demo".into(),
        owner: "thread:t-0001".into(),
        model: "model-a".into(),
        effort: "high".into(),
        view: ImageView {
            id: id.into(),
            state: ImageState::Running,
            account: Some(account),
            reason: "fixture".into(),
            outputs: vec![],
            total_tokens: None,
        },
        allocation: Some(allocation),
        launched: true,
    }
}

fn choose(lab: &Lab, input: &SelectionInput) -> anyhow::Result<(String, Allocation)> {
    input.select(&lab.h.db.conn_for_test(), NOW)
}

// 필요: 관측 수집과 무관하게 시작한다는 사용자 계약과 알려진 한도의 경계를 지킨다.
#[test]
fn unknown_stale_failed_partial_and_invalid_usage_never_waits_or_requires_a_first_run() {
    let l = Lab::new();
    for mode in [
        "unknown",
        "missing",
        "stale",
        "failed",
        "reset",
        "partial",
        "nan",
        "clock",
        "no-reset",
        "conflict",
        "rate-unknown",
    ] {
        // Host/DB 준비만 공유한다. 각 입력은 이력 없는 새 계정으로 처음·두 번째·종료 뒤를 모두 검사한다.
        let account = format!("work-{mode}");
        let mut c = candidate(&account, 20.0, 20.0);
        if mode == "missing" {
            c.quota = None;
        }
        if let Some(q) = &mut c.quota {
            match mode {
                "unknown" => {
                    q.windows.clear();
                    q.problem = Some(QuotaProblem::Unknown);
                }
                "stale" => q.windows[0].window.observed_at_ms = NOW - 120001,
                "failed" => q.problem = Some(QuotaProblem::Failed),
                "reset" => q.windows[0].window.resets_at_ms = Some(NOW),
                "partial" => {
                    q.windows.remove(0);
                    q.windows[0].window.used_percent = 10.0;
                }
                "nan" => q.windows[0].window.used_percent = f64::NAN,
                "clock" => q.windows[0].window.observed_at_ms = NOW + 30001,
                "no-reset" => q.windows[0].window.resets_at_ms = None,
                "conflict" => q.windows[0].conflict = true,
                "rate-unknown" => q.problem = Some(QuotaProblem::RateLimited),
                _ => {}
            }
        }
        let p = input(&l, vec![c]);
        let (name, allocation) = choose(&l, &p).unwrap();
        assert_eq!(allocation.selection.unobserved, mode != "partial", "{mode}: 들어온 주간 창만으로 판단한다");
        if mode != "partial" {
            assert!(allocation.selection.reason.contains("판단"));
        }
        let id = l.seed(&name, allocation, ThreadStatus::Starting, true);
        assert_eq!(choose(&l, &p).unwrap().0, account, "{mode}: 두 번째도 시작한다");
        l.h.update_rec("demo", &id, |r| r.status = ThreadStatus::Resolved).unwrap();
        assert!(choose(&l, &p).is_ok(), "{mode}: 종료 뒤 관측이 없어도 시작한다");
    }
}

#[test]
fn only_confirmed_limits_and_broken_auth_exclude_an_account() {
    let l = Lab::new();
    let mut full = candidate("full", 100.0, 20.0);
    full.quota.as_mut().unwrap().windows.pop(); // 필수 창 누락으로 알려진 고갈을 지우지 않는다.
    let mut p = input(&l, vec![full, candidate("almost", 99.0, 99.0)]);
    assert_eq!(choose(&l, &p).unwrap().0, "almost", "예상 %와 안전 여유로 차단하지 않는다");
    p.candidates[1].quota.as_mut().unwrap().windows[0].window.used_percent = 100.0;
    assert!(choose(&l, &p).unwrap_err().to_string().contains("사용 한도 도달"));
    p.candidates[0].quota.as_mut().unwrap().windows[0].window.scope = Some("other-model".into());
    assert_eq!(choose(&l, &p).unwrap().0, "full");
    p.candidates[0].quota.as_mut().unwrap().windows[0].window.scope = None;
    p.candidates[0].quota.as_mut().unwrap().windows[0].window.observed_at_ms = NOW - 120001;
    assert_eq!(choose(&l, &p).unwrap().0, "full", "오래된 100%는 현재 한도로 단정하지 않는다");
    for problem in [QuotaProblem::Expired, QuotaProblem::Locked, QuotaProblem::RateLimited] {
        let mut c = unknown("broken");
        let q = c.quota.as_mut().unwrap();
        q.problem = Some(problem);
        q.retry_at_ms = Some(NOW + 1000);
        let p = input(&l, vec![c]);
        assert!(choose(&l, &p).is_err(), "{problem:?}");
    }
}

#[test]
fn active_jobs_then_recent_assignment_balance_accounts_without_legacy_groups_or_models() {
    let l = Lab::new();
    let mut a = unknown("a");
    a.member.group = "shared".into();
    a.member.models = vec!["unused".into()];
    let mut b = candidate("b", 99.0, 99.0);
    b.member.group = "shared".into();
    let p = input(&l, vec![b, a]);
    let (name, mut allocation) = choose(&l, &p).unwrap();
    assert_eq!(name, "a", "미관측과 작은 잔량도 같은 차례로 분산한다");
    allocation.selection.selected_at_ms = NOW - 20;
    let a = l.seed(&name, allocation, ThreadStatus::Open, true);
    let (name, mut allocation) = choose(&l, &p).unwrap();
    assert_eq!(name, "b");
    allocation.selection.selected_at_ms = NOW - 10;
    let b = l.seed(&name, allocation, ThreadStatus::Open, true);
    assert_eq!(choose(&l, &p).unwrap().0, "a", "작업 수가 같으면 오래전에 배정한 계정");
    l.h.update_rec("demo", &b, |r| r.status = ThreadStatus::Resolved).unwrap();
    assert_eq!(choose(&l, &p).unwrap().0, "b", "종료 후 관측 지연은 진행 중 작업이 아니다");
    let (name, allocation) = choose(&l, &p).unwrap();
    let id = util::new_id();
    l.h.image_save(&image_job(name, allocation, &id)).unwrap();
    assert_eq!(choose(&l, &p).unwrap().0, "a", "진행 중 이미지는 같은 계정 작업 수에 포함한다");
    l.h.update_rec("demo", &a, |r| r.account_allocation.as_mut().unwrap().released = true).unwrap();
    assert_eq!(choose(&l, &p).unwrap().0, "a", "예약 해제 여부와 실제 진행 상태는 다르다");
}

#[test]
fn concurrent_selection_and_receipts_are_atomic_across_projects() {
    let l = Lab::new();
    let p = Arc::new(input(&l, vec![unknown("a"), unknown("b")]));
    let gate = Arc::new(Barrier::new(8));
    let jobs: Vec<_> = (0..8)
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
                .unwrap()
            })
        })
        .collect();
    for job in jobs {
        job.join().unwrap();
    }
    let records: Vec<ThreadRec> =
        l.h.db.threads(None).unwrap().iter().map(|s| serde_json::from_str(s).unwrap()).collect();
    assert_eq!(records.len(), 8);
    assert_eq!(records.iter().filter(|r| r.account.as_deref() == Some("a")).count(), 4);
    assert!(
        !l.h.db
            .thread_insert_transaction("demo", Some("once-0"), None, None, None, |_, _| {
                panic!("같은 영수증은 다시 선택하지 않는다")
            })
            .unwrap()
            .1
    );
    assert_eq!(l.h.db.threads(None).unwrap().len(), 8);
}

#[tokio::test]
async fn pool_changes_require_human_control_compare_revisions_and_ignore_legacy_fields() {
    let l = Lab::new();
    l.h.accounts.import("work", "claude", &aaa_test_support::token("fixture-pool-token-01234567890123456789")).unwrap();
    let request = FrontRequest::AccountPoolMember {
        revision: l.h.account_pool().unwrap().revision,
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
    assert_eq!(pool.members[0].group, "work");
    assert!(pool.members[0].models.is_empty());
    assert!(pool.projects.is_empty());
    assert!(
        l.h.pool_project_set(&pool.revision, "demo", true, "local:cli")
            .unwrap_err()
            .message
            .contains("프로젝트 참여 설정은 없어졌다")
    );
    l.h.pool_forget("work").unwrap();
    assert!(l.h.account_pool().unwrap().members.is_empty());
}

#[tokio::test]
async fn host_uses_only_same_harness_and_never_waits_for_usage_refresh() {
    let l = Lab::new();
    l.h.register_account("one", "claude", &aaa_test_support::token("fixture-model-one-01234567890123456789")).unwrap();
    // 옛 실행의 인증 파일이 없어져도 다른 정상 계정의 시작을 막지 않는다.
    let mut pool = l.h.account_pool().unwrap();
    pool.members.push(AccountPoolMember { account: "missing".into(), group: "legacy".into(), models: vec![] });
    l.h.db.set_meta(CONFIG, &serde_json::to_string(&pool).unwrap()).unwrap();
    l.h.db
        .thread_insert_next("demo", |id| {
            let mut rec = ThreadRec::starting("demo", "옛 실행", "m", "claude", Role::Worker, ThreadKind::Dir, NOW);
            rec.id = id.into();
            rec.account = Some("missing".into());
            rec.status = ThreadStatus::Open;
            serde_json::to_string(&rec).unwrap()
        })
        .unwrap();

    let profile = ResolvedProfile {
        harness: "claude".into(),
        model: "any-model".into(),
        effort: "high".into(),
        args: vec![],
        command: vec![],
        env: Default::default(),
    };
    let lock = l.h.account_quota.lock().await;
    let prepared = l.h.account_selection_input("demo", &profile, None, AccountTaskSize::Medium);
    tokio::pin!(prepared);
    let std::task::Poll::Ready(result) = futures_util::poll!(&mut prepared) else {
        panic!("관측 RPC 잠금을 기다리지 않는다");
    };
    let p = result.unwrap().unwrap();
    assert_eq!(choose(&l, &p).unwrap().0, "one");
    drop(lock);
    l.h.pool_forget("missing").unwrap();
    let profile = ResolvedProfile { harness: "codex".into(), ..profile.clone() };
    assert!(
        l.h.account_selection_input("demo", &profile, None, AccountTaskSize::Medium).await.unwrap().is_none(),
        "다른 harness만 등록돼도 기본 시작으로 돌아간다"
    );
    let revision = l.h.account_pool().unwrap().revision;
    l.h.pool_member_set(&revision, "one", None, "local:cli").unwrap();
    l.h.register_account("one", "claude", &aaa_test_support::token("fixture-renewed-one-01234567890123456789"))
        .unwrap();
    assert!(l.h.account_pool().unwrap().members.is_empty(), "다시 로그인은 사용 해제를 되돌리지 않는다");
}

#[test]
fn migration_preserves_the_original_and_membership_without_rewriting_running_threads() {
    let l = Lab::new();
    let p = input(&l, vec![unknown("work")]);
    let (name, allocation) = choose(&l, &p).unwrap();
    let id = l.seed(&name, allocation, ThreadStatus::Open, true);
    let before = l.h.load("demo", &id).unwrap();
    let original = r#"{"revision":"legacy","members":[{"account":"work","group":"claude","models":["old-model"]}],"projects":["old-project"]}"#;
    l.h.db.set_meta("account-pool-v1", original).unwrap();
    l.h.db.conn_for_test().execute("DELETE FROM meta WHERE key=?1", [CONFIG]).unwrap();
    l.h.pool_migrate().unwrap();
    let pool = l.h.account_pool().unwrap();
    assert_eq!(pool.members[0].account, "work");
    assert_eq!(pool.members[0].group, "work");
    assert!(pool.members[0].models.is_empty());
    assert!(pool.projects.is_empty());
    assert_eq!(l.h.db.meta("account-pool-v1").unwrap().as_deref(), Some(original));
    assert_eq!(serde_json::to_value(l.h.load("demo", &id).unwrap()).unwrap(), serde_json::to_value(before).unwrap());
    l.h.pool_member_set(&pool.revision, "work", None, "local:cli").unwrap();
    l.h.pool_migrate().unwrap();
    assert!(l.h.account_pool().unwrap().members.is_empty(), "이전을 다시 실행해 사용 해제를 되돌리지 않는다");
}
#[test]
fn ended_reservations_wait_for_observations_after_exit_and_survive_relogin() {
    let l = Lab::new();
    l.h.accounts.import("work", "claude", &aaa_test_support::token("fixture-end-canary-01234567890123456789")).unwrap();
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
    l.h.accounts
        .import("work", "claude", &aaa_test_support::token("fixture-end-renewed-01234567890123456789"))
        .unwrap();
    for w in &mut q.windows {
        w.window.observed_at_ms = now;
    }
    l.h.pool_reconcile(&AccountQuotaReport { accounts: vec![q] }).unwrap();
    assert!(
        l.h.load("demo", &id).unwrap().account_allocation.unwrap().released,
        "끝난 실행의 예약은 다시 로그인 뒤에도 종료 이후의 관측으로 교체한다"
    );
}

#[test]
fn image_restart_recovers_auth_without_replaying_or_releasing_unknown_history() {
    let l = Lab::new();
    let selected = input(&l, vec![candidate("work", 85.0, 85.0)]);
    let (account, allocation) =
        l.h.db.image_transaction(|tx| selected.select_with_percent(tx, NOW, crate::image::RESERVE)).unwrap();
    let id = util::new_id();
    let job = image_job(account.clone(), allocation.clone(), &id);
    l.h.image_save(&job).unwrap();
    let dir = l.h.image_dir(&id).unwrap();
    let home = dir.join("rpc-home");
    util::ensure_private_dir(&home).unwrap();
    util::write_private(&home.join("auth.json"), aaa_test_support::token("auth-to-recover").as_bytes()).unwrap();
    util::write_private(&home.join("output.png"), b"preserved-result").unwrap();
    let queued = util::new_id();
    let mut q = image_job(account, allocation, &queued);
    q.launched = false;
    q.view.state = ImageState::Queued;
    l.h.image_save(&q).unwrap();
    l.h.image_recover().unwrap();
    let read = |id: &str| {
        serde_json::from_str::<crate::image::Job>(&l.h.db.meta(&format!("image-job:{id}")).unwrap().unwrap()).unwrap()
    };
    let unknown = read(&id);
    assert_eq!(unknown.view.state, ImageState::Unknown);
    assert!(!unknown.allocation.unwrap().released);
    assert_eq!(read(&queued).view.state, ImageState::Waiting);
    assert!(read(&queued).allocation.unwrap().released);
    assert!(!home.join("auth.json").exists());
    assert_eq!(std::fs::read(home.join("output.png")).unwrap(), b"preserved-result");
    l.h.image_recover().unwrap();
    assert_eq!(read(&id).view.state, ImageState::Unknown);
    // 잔량 15%p에서 불명 이미지 5%p와 안전 여유 10%p를 유지한다.
    assert!(
        l.h.db.image_transaction(|tx| selected.select_with_percent(tx, NOW, crate::image::RESERVE)).is_ok(),
        "불명 실행 이력은 보존하되 예상 소비량으로 새 시작을 막지 않는다"
    );
}

#[test]
fn concurrent_image_claims_keep_distinct_history_and_end_after_new_observations() {
    let l = Lab::new();
    let selected = input(&l, vec![candidate("work", 85.0, 85.0)]);
    let barrier = Barrier::new(2);
    let results = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..2)
            .map(|_| {
                scope.spawn(|| {
                    barrier.wait();
                    l.h.db.image_transaction(|tx| {
                        let (account, allocation) = selected.select_with_percent(tx, NOW, crate::image::RESERVE)?;
                        let id = util::new_id();
                        let job = image_job(account, allocation, &id);
                        tx.execute(
                            "INSERT INTO meta(key,value) VALUES(?1,?2)",
                            [format!("image-job:{id}"), serde_json::to_string(&job)?],
                        )?;
                        Ok(id)
                    })
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect::<Vec<_>>()
    });
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 2);
    let id = results.into_iter().find_map(Result::ok).unwrap();
    let mut job: crate::image::Job =
        serde_json::from_str(&l.h.db.meta(&format!("image-job:{id}")).unwrap().unwrap()).unwrap();
    let now = util::now_ms();
    let ended = now - 100;
    job.view.state = ImageState::Succeeded;
    job.allocation.as_mut().unwrap().ended_at_ms = Some(ended);
    l.h.image_save(&job).unwrap();
    let mut q = candidate("work", 0.0, 0.0).quota.unwrap();
    for w in &mut q.windows {
        w.window.observed_at_ms = ended - 1;
        w.window.resets_at_ms = Some(now + 60_000);
    }
    let mut report = AccountQuotaReport { accounts: vec![q] };
    l.h.image_reconcile(&report).unwrap();
    let read = || {
        serde_json::from_str::<crate::image::Job>(&l.h.db.meta(&format!("image-job:{id}")).unwrap().unwrap()).unwrap()
    };
    assert!(!read().allocation.unwrap().released);
    for w in &mut report.accounts[0].windows {
        w.window.observed_at_ms = now;
    }
    l.h.image_reconcile(&report).unwrap();
    assert!(read().allocation.unwrap().released);
}

// 통신 조회가 없는 가짜 연결이다. 기동 전에 이미 해석한 profile·머신을 다시 RPC로 묻지 않아야 한다.
fn connect_pool_machine(l: &Lab) {
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
}

#[test]
fn waiting_direct_start_keeps_one_task_across_passes_and_request_replays() {
    let l = Lab::new();
    // 연결 때 예약한 rewatch도 Host를 쥔다. runtime을 Lab보다 먼저 닫아 DB 수명을 끝낸다.
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    runtime.block_on(async {
        l.h.accounts
            .import("blocked", "claude", &aaa_test_support::token("fixture-blocked-01234567890123456789"))
            .unwrap();
        let mut meta = l.h.account_meta("blocked");
        meta.rejected_ms = Some(util::now_ms());
        l.h.db.set_meta("account-meta:blocked", &serde_json::to_string(&meta).unwrap()).unwrap();
        l.configure(&[unknown("blocked")]);
        connect_pool_machine(&l);
        let mut job = queued_job(&l, "one-task");
        job.profile_revision = l.h.profile_revision(&job.args).unwrap();
        l.h.db.set_meta(&job_key("demo", &job.view.id), &serde_json::to_string(&job).unwrap()).unwrap();
        for _ in 0..2 {
            let response = l.h.pool_run("demo", &job.view.id, true).await.unwrap();
            assert!(
                matches!(response, FrontResponse::AccountStart { start } if start.state == AccountStartState::Queued)
            );
        }
        let response = l.h.pool_submit(job.args, &job.actor, None, true).await.unwrap();
        assert!(matches!(response, FrontResponse::AccountStart { start } if start.state == AccountStartState::Queued));
        let board = l.h.task_board("demo").unwrap();
        assert_eq!(board.tasks.len(), 1, "대기 pass마다 새 할 일을 만들지 않는다");
        assert_eq!(board.tasks[0].machine.as_deref(), Some("mac-a"));
        assert!(board.tasks[0].thread.is_none());
        let id = board.tasks[0].id.clone();
        assert_eq!(l.h.pool_job("demo", &job.view.id).unwrap().args.start_task.unwrap().id, id);
        let reopened = Host::open(l.h.cfg.clone(), l._root.0.join("host")).unwrap();
        assert_eq!(reopened.pool_job("demo", &job.view.id).unwrap().args.start_task.unwrap().id, id);
        assert!(reopened.db.threads(None).unwrap().is_empty());
    });
    drop(runtime);
}

fn queued_job(l: &Lab, id: &str) -> Job {
    let job = Job {
        args: StartArgs {
            automatic_workspace: None,
            start_task: None,
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

#[test]
fn automation_thresholds_filter_the_selected_account_and_ignore_missing_windows() {
    let l = Lab::new();
    let mut p = input(&l, vec![candidate("a", 96.0, 20.0), candidate("b", 20.0, 20.0)]);
    p.minimum = [Some(5.0), Some(10.0)];
    assert_eq!(choose(&l, &p).unwrap().0, "b", "작업 수 동률이어도 문턱 아래 계정은 고르지 않는다");
    p.candidates[1].quota.as_mut().unwrap().windows[1].window.used_percent = 91.0;
    assert!(choose(&l, &p).unwrap_err().to_string().contains("자동화 사용량 문턱"));
    p.candidates[0].quota.as_mut().unwrap().windows.remove(0);
    assert_eq!(choose(&l, &p).unwrap().0, "a", "5시간 창이 없으면 들어온 주간 창만 판정한다");
}

use super::*;

struct Lab(std::path::PathBuf);
impl Drop for Lab {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

fn job(state: ReviewState) -> Job {
    let start = ReviewStart {
        id: util::new_id(),
        profile: "review-model".into(),
        cwd: "fixture".into(),
        target: ReviewTarget { base: "a".repeat(40), commit: "b".repeat(40), tree: "c".repeat(40) },
    };
    Job {
        view: ReviewView {
            id: start.id.clone(),
            profile: start.profile.clone(),
            target: start.target.clone(),
            state,
            account: (state == ReviewState::Running).then(|| "review-account".into()),
            reason: "fixture".into(),
        },
        start,
        project: "demo".into(),
        machine: "machine".into(),
        session: "pane".into(),
        thread: "t-0001".into(),
        harness: "codex".into(),
        model: "model-a".into(),
        effort: "high".into(),
        required_account: None,
        allocation: (state == ReviewState::Running).then(|| Allocation {
            selection: AccountSelection {
                size: AccountTaskSize::Small,
                group: "review-account".into(),
                unobserved: false,
                reason: "fixture".into(),
                selected_at_ms: 123,
            },
            reserve_percent: Some(5),
            generation: "generation".into(),
            ended_at_ms: None,
            released: false,
        }),
    }
}

#[test]
fn host_restart_keeps_sent_reservation_and_resumes_only_pre_dispatch_waiting() {
    let lab = Lab(std::env::temp_dir().join(format!("aaa-review-recovery-{}", util::new_id())));
    let mut cfg = aaa_config::HostConfig::default();
    cfg.projects.root = lab.0.join("projects").display().to_string();
    cfg.notify.web_push = false;
    let state = lab.0.join("host");
    let running = job(ReviewState::Running);
    let queued = job(ReviewState::Queued);
    let h = Host::open(cfg.clone(), state.clone()).unwrap();
    h.review_save(&running, None).unwrap();
    h.review_save(&queued, None).unwrap();
    drop(h);

    let h = Host::open(cfg, state).unwrap();
    let load = |job: &Job| h.review_load(&job.start.id).unwrap().unwrap();
    assert_eq!(load(&running).view.state, ReviewState::Unknown);
    assert_eq!(load(&queued).view.state, ReviewState::Waiting);
    h.review_recover().unwrap();
    assert_eq!(load(&running).view.state, ReviewState::Unknown);
    let reservations = h.db.image_transaction(pool_reservations).unwrap();
    assert_eq!(reservations.len(), 1);
    assert_eq!(reservations[0].account, "review-account");
    assert_eq!(reservations[0].allocation.reserve_percent, Some(5));
    assert!(reservations[0].allocation.ended_at_ms.is_none());
}

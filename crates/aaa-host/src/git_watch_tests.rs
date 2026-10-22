//! 실제 host DB·Git 조회 frame·내구 outbox로 자기 착지의 입력 거르기를 확인한다.
use super::*;
use aaa_common::conn::{FrameRx, spawn_writer};
use aaa_proto::*;
use std::sync::Mutex;

struct Lab {
    h: Arc<Host>,
    root: std::path::PathBuf,
    answer: Arc<Mutex<GitAnswer>>,
    worker: tokio::task::JoinHandle<()>,
}
impl Lab {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("aaa-git-own-{}", util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = root.join("projects").to_string_lossy().into();
        cfg.notify.web_push = false;
        let h = Host::open(cfg, root.join("host")).unwrap();
        h.project_create("demo", Default::default(), "local:cli").unwrap();
        let dir = h.project_dir("demo").unwrap();
        std::fs::write(dir.join("project.toml"), "[[repos]]\npath='/work/app'\n").unwrap();
        std::fs::create_dir_all(dir.join("automations")).unwrap();
        std::fs::write(
            dir.join("automations/check.toml"),
            "[on.git]\nrepo='app'\nbranches=['main']\n[do.start]\ntitle='점검'\ntext='자료 확인'\nprofiles=['claude']\nmachines=['mac-a']\n",
        )
        .unwrap();
        let answer = Arc::new(Mutex::new(GitAnswer {
            origin_hash: "origin".into(),
            heads: BTreeMap::from([("main".into(), Some("a".repeat(40)))]),
            change: None,
            merged_commit: None,
        }));
        let (a, b) = tokio::io::duplex(2 << 20);
        let (ar, aw) = tokio::io::split(a);
        drop(ar);
        let conn = AgentConn::new(spawn_writer(aw, MAX_FRAME), 1);
        let pending = conn.pending.clone();
        let current = answer.clone();
        let worker = tokio::spawn(async move {
            let mut rx = FrameRx::new(b, MAX_FRAME);
            while let Ok(LinkMsg::Req { id, body, .. }) = rx.control::<LinkMsg>().await {
                let AgentRequest::GitQuery { query, .. } = body else { panic!("Git 조회만 허용한다") };
                let mut reply = current.lock().unwrap().clone();
                if matches!(query, GitQuery::Heads { .. }) {
                    reply.change = None;
                }
                if let Some(tx) = pending.lock().unwrap().remove(&id) {
                    let _ = tx.send(Outcome::Ok(AgentResponse::Git(reply)));
                }
            }
        });
        h.agent_connected(
            &AgentHello {
                proto: AGENT_LINK,
                machine_id: "m".into(),
                machine_name: "mac-a".into(),
                boot_id: "boot".into(),
                os: "test".into(),
                arch: "test".into(),
                software: "test".into(),
                identity: None,
                features: vec!["git.v1".into(), "host_profiles.v1".into()],
                dev_mode: true,
                sha256: None,
                selfcheck: None,
                sessions: vec![],
                harness: vec![],
            },
            true,
            conn,
        );
        for (id, source) in
            [("t-0001", Some("check")), ("t-0002", Some("other")), ("t-0003", None), ("t-0004", Some("check"))]
        {
            let mut t = crate::orch::ThreadRec::starting("demo", id, "m", "claude", Role::Worker, ThreadKind::Dir, 1);
            t.id = id.into();
            t.status = ThreadStatus::Resolved;
            t.automation = source.map(str::to_owned);
            assert!(h.db.thread_import("demo", id, "resolved", &serde_json::to_string(&t).unwrap()).unwrap());
        }
        Self { h, root, answer, worker }
    }
    fn rule(&self) -> Routine {
        crate::routine::load_all(&self.h.project_dir("demo").unwrap())
            .0
            .into_iter()
            .find(|r| r.name == "check")
            .unwrap()
    }
    fn push(&self, change: GitChange) {
        let mut answer = self.answer.lock().unwrap();
        answer.heads.insert("main".into(), change.after.clone());
        answer.change = Some(change);
    }
    async fn poll(&self) {
        self.h.git_repo("demo", "m", "/work/app", &[self.rule()]).await.unwrap();
    }
}
impl Drop for Lab {
    fn drop(&mut self) {
        self.worker.abort();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn change(subjects: &[&str]) -> GitChange {
    GitChange {
        branch: "main".into(),
        before: Some("a".repeat(40)),
        after: Some("b".repeat(40)),
        kind: "pushed".into(),
        commits: subjects
            .iter()
            .map(|subject| GitCommit {
                sha: "b".repeat(40),
                subject: aaa_common::pr::sanitize(subject),
                owners: aaa_common::ci::owners(subject),
            })
            .collect(),
        changed_files: 1,
        truncated: false,
    }
}

#[tokio::test]
async fn own_landings_skip_but_mixed_and_other_rules_enter_the_durable_queue() {
    let cases: &[(&[&str], bool)] = &[
        (&["병합: 첫 수리 (t-0001)", "내부 구현", "병합: 다음 수리 (t-0004)"], true),
        (&["병합: 수리 (t-0001)", "병합: 일반 thread (t-0003)"], false),
        (&["병합: 다른 규칙 (t-0002)"], false),
        (&["병합: 수리 (t-0001)", "병합: 다른 규칙 (t-0002)"], false),
    ];
    for &(subjects, skipped) in cases {
        let l = Lab::new();
        let r = l.rule();
        let (sha, approved) = l.h.automation_start_approval("demo", &r).unwrap().unwrap();
        assert!(!approved);
        l.h.automation_approve("demo", &r.name, &sha, "local:cli").unwrap();
        l.poll().await; // 첫 등록은 기준만 저장한다.
        l.push(change(subjects));
        l.poll().await;
        assert_eq!(l.h.automation_queue("demo", "check").unwrap().pending, u32::from(!skipped), "{subjects:?}");
        let runs = l.h.routine_runs("demo", "check").unwrap();
        let view = l.h.routines("demo").unwrap().into_iter().find(|r| r.name == "check").unwrap();
        if skipped {
            assert_eq!(runs.len(), 1);
            assert_eq!(runs[0].status, "skipped");
            assert_eq!(runs[0].detail, "자기 착지");
            assert_eq!(view.last_skip.unwrap().detail, "자기 착지");
        } else {
            assert!(runs.is_empty(), "아직 큐에 있는 입력을 실행 완료로 기록하지 않는다");
            assert!(view.last_result.is_none());
        }
        assert_eq!(view.today, 0, "건너뜀과 큐 인계는 실행 완료 횟수가 아니다");
        assert_eq!(view.start_approval_sha256, Some(sha.clone()));
        assert_eq!(view.start_approved, Some(true));
        assert_eq!(l.h.automation_start_approval("demo", &r).unwrap(), Some((sha, true)));
        l.poll().await;
        assert_eq!(l.h.routine_runs("demo", "check").unwrap(), runs, "같은 head 재조회는 실행 기록을 바꾸지 않는다");
        assert_eq!(l.h.automation_queue("demo", "check").unwrap().pending, u32::from(!skipped));
        assert_eq!(
            l.h.db
                .flow_after(Some("demo"), 0, 100)
                .unwrap()
                .iter()
                .filter(|e| e.kind == "git_pushed" && e.thread.is_none())
                .count(),
            1,
            "시작을 건너뛰어도 Git 사건 이력은 한 번 남는다"
        );
    }
}

#[tokio::test]
async fn incomplete_or_unknown_landings_and_non_start_actions_are_not_suppressed() {
    let l = Lab::new();
    let r = l.rule();
    let long = format!("병합: {} (t-0001)", "제목".repeat(80));
    let long_change = change(&[&long]);
    assert!(!long_change.commits[0].subject.contains("t-0001"));
    assert!(l.h.git_own_landing("demo", &r, &long_change), "표시용 제목이 잘려도 원문에서 뽑은 출처를 쓴다");
    for subjects in
        [vec![], vec!["일반 커밋"], vec!["병합: 출처 없음 (t-9999)"], vec!["병합: 자기 (t-0001)", "병합: 번호 없음"]]
    {
        assert!(!l.h.git_own_landing("demo", &r, &change(&subjects)), "{subjects:?}");
    }
    let mut c = change(&["병합: 자기 (t-0001)"]);
    c.truncated = true;
    assert!(!l.h.git_own_landing("demo", &r, &c));
    c.truncated = false;
    for kind in ["forced", "deleted"] {
        c.kind = kind.into();
        assert!(!l.h.git_own_landing("demo", &r, &c));
    }
    c.kind = "pushed".into();
    for action in [crate::automation::Action::Deploy, crate::automation::Action::Inbox(Default::default())] {
        let mut other = r.clone();
        other.action = action;
        assert!(!l.h.git_own_landing("demo", &other, &c));
    }
    assert!(!l.h.git_own_landing("other-project", &r, &c), "출처는 사건 프로젝트에서만 읽는다");
}

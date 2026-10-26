//! frame 상한 회귀: 실제 DB의 수천 건을 필터·커서·바이트 한도로 읽는다.
use super::*;
use aaa_proto::{MAX_FRAME, ThreadListQuery};

struct Lab {
    host: Arc<Host>,
    root: PathBuf,
}
impl Lab {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("aaa-thread-list-{}", aaa_common::util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = root.join("projects").display().to_string();
        cfg.notify.web_push = false;
        let host = Host::open(cfg, root.join("host")).unwrap();
        host.project_create("p", Default::default(), "test").unwrap();
        // 다중 프로젝트는 옛 DB 호환 경로다. 신규 생성은 host당 하나만 허용한다.
        std::fs::create_dir_all(root.join("projects/z")).unwrap();
        Self { host, root }
    }
    fn insert(&self, project: &str, n: u32, status: ThreadStatus, role: Role, next: Vec<String>) {
        let mut r = ThreadRec::starting(project, "목록 회귀", "m", "codex", role, ThreadKind::Dir, 0);
        r.id = format!("t-{n:04}");
        r.status = status;
        r.next = next;
        self.host.db.thread_import(project, &r.id, status.as_str(), &serde_json::to_string(&r).unwrap()).unwrap();
    }
    fn page(
        &self,
        project: Option<&str>,
        role: Option<Role>,
        query: &ThreadListQuery,
    ) -> (Vec<ThreadView>, Option<String>) {
        let response = self.host.threads_page(project, role, query).unwrap();
        // 실제 IPC 포장까지 상한 안이다. 한 frame으로 모두 돌려주면 이 단언을 지키지 못한다.
        let frame = aaa_proto::FrontMsg::Res { id: "1".into(), result: Ok::<_, ErrorBody>(response.clone()).into() };
        assert!(serde_json::to_vec(&frame).unwrap().len() < MAX_FRAME);
        match response {
            FrontResponse::Threads { threads, next_after } => (threads, next_after),
            _ => unreachable!(),
        }
    }
}
impl Drop for Lab {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}

#[tokio::test]
async fn thousands_of_resolved_threads_page_without_losing_projects_roles_or_live_states() {
    let lab = Lab::new();
    for n in 1..=2500 {
        // 상세 100건만으로도 frame 한도를 넘는다. 목록은 이 본문을 읽어 보내지 않는다.
        let next = if n <= 60 { vec!["상세 다음 작업".repeat(1500)] } else { vec![] };
        lab.insert("p", n, ThreadStatus::Resolved, Role::Worker, next);
    }
    for (i, status) in
        [ThreadStatus::Starting, ThreadStatus::Open, ThreadStatus::Failed, ThreadStatus::Parked].into_iter().enumerate()
    {
        lab.insert("p", 2501 + i as u32, status, Role::Worker, vec![]);
    }
    for n in [9999, 10000, 10001] {
        lab.insert(
            "z",
            n,
            ThreadStatus::Resolved,
            if n == 10001 { Role::Evaluator } else { Role::Coordinator },
            vec![],
        );
    }
    let mut query = ThreadListQuery { limit: Some(u32::MAX), ..Default::default() };
    let mut all = Vec::new();
    loop {
        let (threads, next) = lab.page(None, None, &query);
        assert!(threads.len() <= 200);
        assert!(threads.iter().all(|t| t.next.is_empty() && t.questions.is_none()));
        all.extend(threads.into_iter().map(|t| (t.project, t.id)));
        if next.is_none() {
            break;
        }
        assert_ne!(next, query.after);
        query.after = next;
    }
    assert_eq!(all.len(), 2507);
    assert_eq!(all.iter().collect::<std::collections::BTreeSet<_>>().len(), 2507);
    assert_eq!(
        &all[2504..],
        &[("z".into(), "t-9999".into()), ("z".into(), "t-10000".into()), ("z".into(), "t-10001".into())]
    );
    let (open, next) = lab.page(Some("p"), Some(Role::Worker), &ThreadListQuery { open: true, ..Default::default() });
    assert!(next.is_none());
    assert_eq!(
        open.iter().map(|t| t.status).collect::<Vec<_>>(),
        [ThreadStatus::Starting, ThreadStatus::Open, ThreadStatus::Failed, ThreadStatus::Parked]
    );
    let (failed, _) =
        lab.page(None, None, &ThreadListQuery { status: Some(ThreadStatus::Failed), ..Default::default() });
    assert_eq!(failed[0].id, "t-2503");
    let (evaluators, _) = lab.page(None, Some(Role::Evaluator), &Default::default());
    assert_eq!(evaluators.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["t-10001"]);
    let (detailed, next) = lab.page(Some("p"), None, &ThreadListQuery { details: true, ..Default::default() });
    assert!(detailed.len() < 60 && !detailed.is_empty());
    assert!(next.is_some());
    assert_eq!(detailed[0].next, lab.host.thread_show("p", "t-0001").unwrap().next);
    let (continued, _) = lab.page(
        Some("p"),
        None,
        &ThreadListQuery { details: true, after: next, limit: Some(1), ..Default::default() },
    );
    assert_eq!(continued[0].id, format!("t-{:04}", detailed.len() + 1));
}

#[tokio::test]
async fn invalid_page_requests_fail_without_silently_changing_the_filter() {
    let lab = Lab::new();
    for query in [
        ThreadListQuery { limit: Some(0), ..Default::default() },
        ThreadListQuery { open: true, status: Some(ThreadStatus::Resolved), ..Default::default() },
        ThreadListQuery { after: Some("broken".into()), ..Default::default() },
        ThreadListQuery { after: Some(hex::encode(br#"["z","t-0001"]"#)), ..Default::default() },
    ] {
        assert_eq!(lab.host.threads_page(Some("p"), None, &query).unwrap_err().code, ErrorCode::BadRequest);
    }
}

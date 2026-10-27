use super::*;
use crate::db::LibraryReportRow;
use aaa_proto::{AskKind, AskRecord, DeviceRole, FrontRequest, FrontResponse, QuestionAnswerView};
use std::{path::PathBuf, sync::Arc};

struct Lab {
    host: Arc<Host>,
    root: PathBuf,
}
impl Lab {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("aaa-record-{}", aaa_common::util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = root.join("projects").to_string_lossy().into();
        cfg.notify.web_push = false;
        let host = Host::open(cfg, root.join("state")).unwrap();
        host.project_create("demo", Default::default(), "test").unwrap();
        // 새 프로젝트 생성은 단일 host 정책이다. 이관된 기존 프로젝트 경계도 검사한다.
        let other = root.join("projects/other");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::copy(root.join("projects/demo/PROJECT.md"), other.join("PROJECT.md")).unwrap();
        std::fs::create_dir_all(other.join("threads")).unwrap();
        Self { host, root }
    }
    fn search(&self, query: &str, kind: Option<RecordKind>) -> RecordSearchPage {
        self.host.records_search(RecordSearchQuery { query: query.into(), kind, ..Default::default() }).unwrap()
    }
    fn report(&self, project: &str, source: &str, body: &str, at: u64) -> String {
        let root = self.root.join("report-source");
        std::fs::create_dir_all(&root).unwrap();
        self.host
            .snapshot_report_version(
                project,
                source,
                &format!("rev-{at}"),
                &root,
                &[],
                Some(body.as_bytes()),
                &LibraryReportRow { project: project.into(), name: source.into(), updated: at, ..Default::default() },
            )
            .unwrap()
    }
    fn ask(&self) -> AskRecord {
        let mut q = aaa_proto::report_questions(
            "## Questions\n### 1. 검색 정책\n한글 질문 본문\n\n- A 과거판 (추천) — 원문 보존\n- B 최신\n",
        );
        q[0].id = Some("q-0007".into());
        AskRecord {
            id: "answered".into(),
            report_id: None,
            question_id: Some("q-0007".into()),
            kind: AskKind::Question,
            subject: "t-0001".into(),
            title: "검색 정책".into(),
            asked_ms: 100,
            done_ms: 200,
            by: "CLI".into(),
            summary: "1A 직접".into(),
            items: q,
            answer: Some(QuestionAnswerView {
                picks: vec![Some(0)],
                text: Some("예전 공통 답".into()),
                comments: vec![Some("내가 직접 쓴 결정".into())],
                summary: "1A 직접".into(),
                by: "CLI".into(),
                at_ms: 200,
            }),
            body: None,
            files_base: None,
        }
    }
}
impl Drop for Lab {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[tokio::test]
async fn retained_versions_answers_and_tasks_share_search_and_reader_opens_the_matched_body() {
    let lab = Lab::new();
    let h = &lab.host;
    let old = "[완료] 검색 정책 이전\n\n한글 공통근거 SQLite\n";
    let old_id = lab.report("demo", "t-0001", old, 10);
    lab.report("demo", "t-0001", "[완료] 새 판\n한글 공통근거 파일\n", 20);
    lab.report("demo", "t-0002", "[진행 중] 공통근거 진행\n", 30);
    h.db.ask_put("demo", &lab.ask()).unwrap();
    let path = h.project_dir("demo").unwrap().join("TASKS.md");
    std::fs::write(
        &path,
        "## Backlog\n- [ ] 한글 공통근거 <!-- aaa-task:01ARZ3NDEKTSV4RRFFQ69G5FAV -->\n  결정의 본문 SQLite\n",
    )
    .unwrap();
    assert_eq!(lab.search("공통근거", None).hits.len(), 3);
    let page = lab.search("sqlite", None);
    assert_eq!(page.hits.len(), 2);
    let hit = page.hits.iter().find(|r| r.kind == RecordKind::Report).unwrap();
    assert_eq!(hit.report_id.as_deref(), Some(old_id.as_str()));
    let FrontResponse::RecordDocument(doc) = h
        .call(
            DeviceRole::Reader,
            "device:test",
            FrontRequest::RecordOpen { project: hit.project.clone(), key: hit.key.clone(), offset: None },
        )
        .await
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(doc.body, old);
    let q = lab.search("직접 쓴 결정", Some(RecordKind::Question));
    assert_eq!(q.hits.len(), 1);
    let doc = h.records_open("demo", &q.hits[0].key).unwrap();
    for expected in ["한글 질문 본문", "A 과거판", "원문 보존", "내가 직접 쓴 결정", "예전 공통 답"]
    {
        assert!(doc.body.contains(expected), "{expected}: {}", doc.body);
    }
    assert_eq!(lab.search("q-00007", None).hits[0].id, "q-0007");
    assert!(lab.search("진행", Some(RecordKind::Report)).hits.is_empty());
}

#[test]
fn task_before_after_and_current_are_deduplicated_and_open_never_substitutes_the_new_body() {
    let lab = Lab::new();
    let h = &lab.host;
    let path = h.project_dir("demo").unwrap().join("TASKS.md");
    let before = "- [ ] 계약 <!-- aaa-task:01ARZ3NDEKTSV4RRFFQ69G5FAV -->\n  옛본문 고유검색\n";
    let after = before.replace("옛본문", "새본문");
    std::fs::write(&path, before).unwrap();
    let found = lab.search("고유검색", Some(RecordKind::Task));
    assert_eq!(found.hits.len(), 1);
    std::fs::write(&path, &after).unwrap();
    // 조회 직후 직접 파일을 고쳐도 이미 찾은 판을 열 수 있다.
    assert_eq!(h.records_open("demo", &found.hits[0].key).unwrap().body, before);
    let record = crate::task_edit::Record {
        action: aaa_proto::TaskAction::Update,
        list: "Backlog".into(),
        from_list: None,
        title: "계약".into(),
        before: Some(before.into()),
        after: Some(after.clone()),
        thread: None,
        restored: None,
    };
    h.db.task_log_add("demo", 100, "file", &record).unwrap();
    let found = lab.search("고유검색", Some(RecordKind::Task));
    assert_eq!(found.hits.len(), 2);
    assert!(found.hits.iter().all(|h| !h.excerpt.contains("aaa-task:") && !h.excerpt.contains("01ARZ3")));
    let short = found.hits[0].id.clone();
    assert!(short.starts_with("d-"));
    assert_eq!(lab.search(&short.replace("d-0", "d-00"), None).hits.len(), 2);
    h.db.task_log_prune("demo", 101).unwrap();
    assert!(lab.search("옛본문", Some(RecordKind::Task)).hits.is_empty());
}

#[test]
fn filtering_precedes_paging_literal_unicode_search_and_project_boundaries_survive_restart() {
    let lab = Lab::new();
    let h = &lab.host;
    for i in 1..=8 {
        lab.report("demo", &format!("t-{i:04}"), &format!("[완료] 판 {i}\n공통검색 한글 100% literal_\n"), i);
    }
    lab.report("other", "t-0001", "[완료] 공통검색\n다른 프로젝트\n", 50);
    let mut q = RecordSearchQuery {
        query: "공통검색".into(),
        project: Some("demo".into()),
        kind: Some(RecordKind::Report),
        limit: Some(3),
        ..Default::default()
    };
    let mut keys = std::collections::HashSet::new();
    loop {
        let p = h.records_search(q.clone()).unwrap();
        for hit in p.hits {
            assert_eq!(hit.project, "demo");
            assert!(keys.insert(hit.key));
        }
        q.after = p.next;
        if q.after.is_none() {
            break;
        }
    }
    assert_eq!(keys.len(), 8);
    assert_eq!(lab.search("100% literal_", None).hits.len(), 8);
    assert_eq!(lab.search("글", None).hits.len(), 8);
    assert!(lab.search("missing", None).hits.is_empty());
    let page =
        h.records_search(RecordSearchQuery { query: "공통검색".into(), limit: Some(1), ..Default::default() }).unwrap();
    let bad = RecordSearchQuery { query: "다른 검색".into(), after: page.next, ..Default::default() };
    assert!(h.records_search(bad).is_err());
    let hit = &lab.search("다른 프로젝트", None).hits[0];
    assert!(h.records_open("demo", &hit.key).is_err());
    let reopened = Host::open(h.cfg.clone(), lab.root.join("state")).unwrap();
    assert_eq!(reopened.records_open("other", &hit.key).unwrap().hit.project, "other");
    h.db.project_forget("other").unwrap();
    assert!(h.records_open("other", &hit.key).is_err());
}

#[test]
fn identical_report_aliases_retention_and_large_utf8_originals_are_preserved() {
    let lab = Lab::new();
    let h = &lab.host;
    let body = format!("[완료] 긴 원문\n{}끝", "한글🙂\"\\\n".repeat(100000));
    let first = lab.report("demo", "t-0001", &body, 1);
    let second = lab.report("demo", "t-0001", &body, 2);
    let hits = lab.search("긴 원문", None).hits;
    assert_eq!(hits.len(), 1);
    for id in [&first, &second] {
        assert_eq!(lab.search(id, None).hits.len(), 1);
    }
    let key = &hits[0].key;
    let mut restored = String::new();
    let mut offset = None;
    loop {
        let part = h.records_chunk("demo", key, offset).unwrap();
        assert!(
            serde_json::to_vec(&aaa_proto::FrontResponse::RecordDocument(part.clone())).unwrap().len()
                < aaa_proto::MAX_FRAME
        );
        restored.push_str(&part.body);
        offset = part.next;
        if offset.is_none() {
            break;
        }
    }
    assert_eq!(restored, body);
    assert!(h.records_chunk("demo", key, Some(2)).is_err());
    h.db.ask_put("demo", &lab.ask()).unwrap();
    let question = lab.search("내가 직접", None).hits[0].clone();
    let stale_question = h.records_open("demo", &question.key).unwrap();
    h.db.ask_delete("demo", None).unwrap();
    assert!(h.records_open("demo", &question.key).is_err(), "명시적으로 지운 질문은 새 검색 전에도 열리지 않는다");
    h.db.report_deleted("demo", &second).unwrap();
    assert_eq!(h.records_open("demo", key).unwrap().hit.report_id.as_deref(), Some(first.as_str()));
    h.db.report_deleted("demo", &first).unwrap();
    assert!(h.records_open("demo", key).is_err());
    assert!(lab.search("긴 원문", None).hits.is_empty());
    // 목록을 읽은 직후 원장이 삭제됐어도 늦은 색인 갱신이 되살리지 않는다.
    h.db.records_replace("demo", &[(stale_question.hit.source.clone(), stale_question)]).unwrap();
    assert!(h.records_open("demo", &question.key).is_err());
}

use super::*;
use aaa_proto::{DeviceRole, FrontRequest, FrontResponse, LibraryRef, Role, ThreadKind};
use std::{path::PathBuf, sync::Arc};
struct Lab {
    h: Arc<Host>,
    root: PathBuf,
}
impl Lab {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("aaa-rids-{}", aaa_common::util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = root.join("projects").to_string_lossy().into();
        cfg.notify.web_push = false;
        let h = Host::open(cfg, root.join("state")).unwrap();
        h.project_create("demo", Default::default(), "test").unwrap();
        Self { h, root }
    }
    fn publish(&self, name: &str, text: &str, live: bool) -> String {
        let source = self.root.join("source");
        std::fs::create_dir_all(&source).unwrap();
        for path in ["index.html", "picture.svg"] {
            std::fs::write(source.join(path), text).unwrap();
        }
        let FrontResponse::LibraryLink(v) =
            self.h.library_publish("demo", name, source.to_str().unwrap(), live, None, None, "test").unwrap()
        else {
            panic!()
        };
        v.report_id.unwrap()
    }
    async fn file(&self, name: &str, path: &str) -> Vec<u8> {
        use futures_util::TryStreamExt;
        let v = self.h.report_library("demo", name, None, false).unwrap();
        let cap = v.base.trim_start_matches("/v/").trim_end_matches('/');
        let source = self.h.view_source(cap, path).await.unwrap().source;
        let size = source.size();
        source.open(0, size).await.unwrap().try_concat().await.unwrap()
    }
}
impl Drop for Lab {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[tokio::test]
async fn republish_keeps_exact_old_files_restart_aliases_and_deletion_tombstones() {
    let lab = Lab::new();
    let h = &lab.h;
    let first = lab.publish("week", "first", false);
    assert_eq!(first, "r-0001");
    assert!(
        h.library_publish("demo", &first, lab.root.join("source").to_str().unwrap(), false, None, None, "test")
            .is_err(),
        "발급한 판은 publish로 덮어쓰지 못한다"
    );
    let second = lab.publish("week", "second", false);
    assert_eq!(second, "r-0002");
    assert_eq!(lab.file(&first, "index.html").await, b"first");
    assert_eq!(lab.file(&first, "picture.svg").await, b"first");
    assert_eq!(lab.file("week", "index.html").await, b"second");
    let rows = h.library_reports(None, true).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].report_id.as_deref(), Some(second.as_str()));
    assert_eq!(rows[0].versions, vec![second.clone(), first.clone()]);
    assert_eq!(rows[0].history_size, 22);
    let reopened = Host::open(h.cfg.clone(), lab.root.join("state")).unwrap();
    assert_eq!(reopened.report_version_names("demo", "week"), vec![second, first.clone()]);
    assert!(reopened.report_library("demo", &first, None, false).is_ok());
    let link = h.library_link("demo", &first, None, "test").unwrap();
    assert!(
        h.call(
            DeviceRole::Reader,
            "device:r",
            FrontRequest::LibraryDelete { reports: vec![LibraryRef { project: "demo".into(), name: first.clone() }] }
        )
        .await
        .is_err()
    );
    assert!(h.report_library("demo", &first, None, false).is_ok());
    h.library_delete(&[LibraryRef { project: "demo".into(), name: first.clone() }], "test").unwrap();
    assert!(h.report_library("demo", &first, None, false).is_err());
    assert!(
        h.signed_library_at(link.base.trim_start_matches("/v/").trim_end_matches('/'), aaa_common::util::now_ms())
            .is_err()
    );
    assert_eq!(lab.publish("week", "third", false), "r-0003");
    assert!(h.report_library("demo", &first, None, false).is_err());
}

#[tokio::test]
async fn live_name_remains_live_while_its_r_id_is_frozen_and_accounted() {
    let lab = Lab::new();
    let first = lab.publish("live", "before", true);
    std::fs::write(lab.root.join("source/index.html"), b"after").unwrap();
    assert_eq!(lab.file("live", "index.html").await, b"after");
    assert_eq!(lab.file(&first, "index.html").await, b"before");
    let FrontResponse::LibraryUsage { size, protected_size, .. } = lab.h.library_usage().unwrap() else { panic!() };
    assert_eq!((size, protected_size), (12, 12));
}

#[test]
fn allocator_is_project_local_atomic_and_preserves_q_and_r_on_project_rename() {
    let lab = Lab::new();
    let gate = Arc::new(std::sync::Barrier::new(8));
    let joins = (0..8)
        .map(|_| {
            let h = lab.h.clone();
            let gate = gate.clone();
            std::thread::spawn(move || {
                gate.wait();
                h.db.report_reserve("demo", "week", "rev", 0).unwrap()
            })
        })
        .collect::<Vec<_>>();
    let mut ids = joins.into_iter().map(|t| t.join().unwrap()).collect::<Vec<_>>();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 8);
    assert_eq!(lab.h.db.report_reserve("other", "week", "rev", 0).unwrap(), "r-0001");
    assert_eq!(lab.h.db.question_id("demo", "thread", "t-0001", "rev", 0).unwrap(), "q-1000");
    let export = lab.h.db.project_export("demo").unwrap();
    assert_eq!(export["report_ids"].as_array().unwrap().len(), 8);
    assert_eq!(export["question_ids"].as_array().unwrap().len(), 1);
    lab.h.db.project_rename("demo", "renamed", str::to_string).unwrap();
    assert_eq!(lab.h.db.report_reserve("renamed", "week", "rev", 0).unwrap(), "r-0009");
    assert_eq!(lab.h.db.question_id("renamed", "thread", "t-0001", "rev2", 0).unwrap(), "q-1001");
}

#[tokio::test]
async fn thread_versions_keep_markdown_and_uncopied_files_never_read_latest_agent_bytes() {
    let lab = Lab::new();
    let h = &lab.h;
    let dir = h.project_dir("demo").unwrap();
    let root = lab.root.join("thread-files");
    std::fs::create_dir(&root).unwrap();
    let mut rec = crate::orch::ThreadRec::starting("demo", "원문", "mac-a", "fake", Role::Worker, ThreadKind::Dir, 1);
    rec.id = "t-0001".into();
    h.db.thread_import("demo", &rec.id, "open", &serde_json::to_string(&rec).unwrap()).unwrap();
    let mut ids = vec![];
    for (revision, text) in [("first", "첫 보고"), ("second", "둘째 보고")] {
        std::fs::write(root.join("picture.svg"), text).unwrap();
        let files = vec![
            LibraryFile {
                path: "picture.svg".into(),
                size: text.len() as u64,
                on_agent: false,
                mime: String::new(),
                kind: Default::default(),
            },
            LibraryFile {
                path: "large.bin".into(),
                size: 999,
                on_agent: true,
                mime: String::new(),
                kind: Default::default(),
            },
        ];
        let meta = LibraryReportRow {
            project: "demo".into(),
            name: rec.id.clone(),
            from_thread: Some(rec.id.clone()),
            ..Default::default()
        };
        ids.push(
            h.snapshot_report_version("demo", &rec.id, revision, &root, &files, Some(text.as_bytes()), &meta).unwrap(),
        );
        rec.report_hash = Some(revision.into());
        h.db.thread_save("demo", &rec.id, "open", &serde_json::to_string(&rec).unwrap()).unwrap();
        std::fs::write(dir.join("threads/t-0001.md"), text).unwrap();
    }
    assert_eq!(h.thread_show("demo", &rec.id).unwrap().report_id.as_deref(), Some(ids[1].as_str()));
    rec.status = aaa_proto::ThreadStatus::Open;
    rec.last_group = Some(aaa_proto::ThreadGroup::ReadyForReview);
    h.db.thread_save("demo", &rec.id, "open", &serde_json::to_string(&rec).unwrap()).unwrap();
    assert!(h.new_report_item("demo", &rec));
    assert!(
        h.db.inbox_list(Some("demo"), true)
            .unwrap()
            .iter()
            .any(|i| i.event == "new report" && i.summary.contains(&ids[1]))
    );
    let old = h.thread_show("demo", &ids[0]).unwrap();
    assert_eq!(old.report_hash.as_deref(), Some("first"));
    assert_eq!(old.report_id.as_deref(), Some(ids[0].as_str()));
    let view = h.thread_library("demo", &ids[0], None, false).unwrap();
    assert_eq!(view.report.as_deref(), Some("첫 보고"));
    assert_eq!(lab.file(&ids[0], "picture.svg").await, "첫 보고".as_bytes());
    let cap = view.base.trim_start_matches("/v/").trim_end_matches('/');
    assert!(h.view_source(cap, "large.bin").await.is_err());
}

#[test]
fn legacy_names_and_latest_thread_copy_get_ids_once_at_startup() {
    let lab = Lab::new();
    let h = &lab.h;
    let dir = h.project_dir("demo").unwrap();
    let legacy = "r-0042";
    std::fs::create_dir_all(dir.join("library").join(legacy)).unwrap();
    std::fs::write(dir.join("library").join(legacy).join("index.html"), "legacy").unwrap();
    std::fs::write(
        manifest_path(&dir, legacy),
        serde_json::to_vec(&vec![LibraryFile {
            path: "index.html".into(),
            size: 6,
            on_agent: false,
            mime: String::new(),
            kind: Default::default(),
        }])
        .unwrap(),
    )
    .unwrap();
    h.db.library_report_put(&LibraryReportRow { project: "demo".into(), name: legacy.into(), ..Default::default() })
        .unwrap();
    let reopened = Host::open(h.cfg.clone(), lab.root.join("state")).unwrap();
    assert!(reopened.report_id("demo", legacy).is_none(), "일반 Host::open은 프로젝트 파일을 등록하지 않는다");
    reopened.backfill_report_ids().unwrap();
    assert_eq!(reopened.report_id("demo", legacy).as_deref(), Some("r-0043"));
    let again = Host::open(h.cfg.clone(), lab.root.join("state")).unwrap();
    again.backfill_report_ids().unwrap();
    assert_eq!(again.report_version_names("demo", legacy), vec!["r-0043"]);
}

#[test]
fn snapshot_keeps_path_and_size_checks_and_does_not_limit_directory_depth() {
    let lab = Lab::new();
    let root = lab.root.join("files");
    let path = "one/two/three/four/five/six/seven/eight/test.txt";
    std::fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
    std::fs::write(root.join(path), "current").unwrap();
    let file =
        LibraryFile { path: path.into(), size: 7, on_agent: false, mime: String::new(), kind: Default::default() };
    let meta = LibraryReportRow { project: "demo".into(), name: "thread".into(), ..Default::default() };
    for (path, size, reason) in [
        ("lab/.claude/commands/deploy.md", 7, "보존할 보고 파일 경로가 안전하지 않다"),
        ("capture/.reports/view.json", 7, "보존할 보고 파일 경로가 안전하지 않다"),
        (path, 3, "보고 판 보존 중 파일 크기가 바뀌었다"),
    ] {
        let files = [LibraryFile { path: path.into(), size, ..file.clone() }];
        let error =
            lab.h.snapshot_report_version("demo", "thread", "old", &root, &files, Some(b"body"), &meta).unwrap_err();
        assert_eq!(error.message, reason);
        assert!(lab.h.report_id("demo", "thread").is_none());
    }
    let id = lab.h.snapshot_report_version("demo", "thread", "new", &root, &[file], Some(b"body"), &meta).unwrap();
    assert_eq!(
        std::fs::read(lab.h.project_dir("demo").unwrap().join("library").join(id).join(path)).unwrap(),
        b"current"
    );
}

#[cfg(unix)]
#[test]
fn snapshot_rejects_symlink_components_separately_from_hidden_paths() {
    let lab = Lab::new();
    let root = lab.root.join("files");
    std::fs::create_dir_all(root.join("plain")).unwrap();
    std::fs::write(root.join("plain/test.txt"), b"data").unwrap();
    std::os::unix::fs::symlink(root.join("plain"), root.join("linked")).unwrap();
    let file = LibraryFile {
        path: "linked/test.txt".into(),
        size: 4,
        on_agent: false,
        mime: String::new(),
        kind: Default::default(),
    };
    let meta = LibraryReportRow { project: "demo".into(), name: "thread".into(), ..Default::default() };
    let error =
        lab.h.snapshot_report_version("demo", "thread", "one", &root, &[file], Some(b"body"), &meta).unwrap_err();
    assert!(error.message.contains("symlink"));
    assert!(lab.h.report_id("demo", "thread").is_none());
}

#[test]
fn failed_snapshot_reservation_is_hidden_and_never_reissued() {
    let lab = Lab::new();
    let root = lab.root.join("broken");
    std::fs::create_dir(&root).unwrap();
    let files = vec![LibraryFile {
        path: "missing.svg".into(),
        size: 5,
        on_agent: false,
        mime: String::new(),
        kind: Default::default(),
    }];
    let meta = LibraryReportRow { project: "demo".into(), name: "broken".into(), ..Default::default() };
    assert!(lab.h.snapshot_report_version("demo", "broken", "one", &root, &files, None, &meta).is_err());
    assert!(lab.h.report_id("demo", "broken").is_none());
    assert!(lab.h.db.library_report("demo", "r-0001").unwrap().is_none());
    assert!(lab.h.library_reports(None, false).unwrap().is_empty());
    assert_eq!(lab.publish("fixed", "good", false), "r-0002");
}

#[test]
fn backfill_does_not_adopt_files_owned_by_a_different_state_database() {
    let lab = Lab::new();
    let dir = lab.h.project_dir("demo").unwrap();
    std::fs::create_dir_all(dir.join("library/foreign")).unwrap();
    std::fs::write(dir.join("library/foreign/index.html"), "다른 state의 원문").unwrap();
    std::fs::write(manifest_path(&dir, "foreign"), b"[]").unwrap();
    let before = lab.h.db.project_export("demo").unwrap();
    lab.h.backfill_report_ids().unwrap();
    assert_eq!(lab.h.db.project_export("demo").unwrap(), before);
    assert!(lab.h.report_id("demo", "foreign").is_none());
    assert!(!dir.join("library/r-0001").exists());
}

#[tokio::test]
async fn existing_review_gets_one_question_and_feedback_without_text_is_rejected_without_claiming_it() {
    use sha2::{Digest, Sha256};
    let lab = Lab::new();
    let h = &lab.h;
    let dir = h.project_dir("demo").unwrap();
    let body = "[검수 대기] 시안\n\n## Report\n검토할 결과\n";
    let mut rec = crate::orch::ThreadRec::starting("demo", "시안", "mac-a", "fake", Role::Worker, ThreadKind::Dir, 1);
    rec.id = "t-0001".into();
    rec.status = aaa_proto::ThreadStatus::Open;
    rec.report_hash = Some(hex::encode(Sha256::digest(body)));
    h.db.thread_import("demo", &rec.id, "open", &serde_json::to_string(&rec).unwrap()).unwrap();
    std::fs::write(dir.join("threads/t-0001.md"), body).unwrap();
    let frozen = lab.root.join("frozen-review");
    std::fs::create_dir(&frozen).unwrap();
    h.snapshot_report_version(
        "demo",
        &rec.id,
        rec.report_hash.as_deref().unwrap(),
        &frozen,
        &[],
        Some(body.as_bytes()),
        &LibraryReportRow {
            project: "demo".into(),
            name: rec.id.clone(),
            from_thread: Some(rec.id.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    h.backfill_review_questions().unwrap();
    h.backfill_review_questions().unwrap();
    let q = h.thread_show("demo", &rec.id).unwrap().questions.unwrap();
    assert!(q.review);
    assert_eq!(q.items.len(), 1);
    assert_eq!(q.items[0].id.as_deref(), Some("q-1000"));
    // 새 파일 사본과 rec 갱신 사이에도 지금 답하는 불변 판의 검수 조건을 적용한다.
    std::fs::write(dir.join("threads/t-0001.md"), "[완료] 다음 판을 받는 중").unwrap();
    assert_eq!(
        h.reply_by_id("demo", "q-1000", Some("B"), None, "test").await.unwrap_err().message,
        "고칠 점을 글로 적어 답한다"
    );
    assert!(h.question_draft("demo", &q.items).is_none(), "거절한 선택은 질문을 선점하지 않는다");
    assert!(h.load("demo", &rec.id).unwrap().question_answer.is_none());
}

#[tokio::test]
async fn archived_r_attachment_protects_its_entire_report_family_from_delete_and_prune() {
    let lab = Lab::new();
    let first = lab.publish("week", "first", false);
    let second = lab.publish("week", "next", false);
    let row:aaa_proto::ArchivedTask=serde_json::from_value(serde_json::json!({
        "task_id":"fixture-task","task":{"id":"fixture-task","list":"Backlog","title":"완료","owner":null,"thread":null,"notes":[],"done":true},
        "body":format!("- [x] 완료\n  [결과](library/{first}/index.html)"),"reason":"완료","report":null,"completed_ms":1,"restored":false,"committed":true,"removed_sha256":"","history":[]
    })).unwrap();
    lab.h.db.archive_save("demo", &row).unwrap();
    let report = lab.h.library_reports(None, false).unwrap().into_iter().find(|r| r.name == "week").unwrap();
    assert!(report.busy, "완료 보관의 r 첨부가 가리키는 원래 이름과 모든 판을 보호한다");
    assert!(lab.h.library_delete(&[LibraryRef { project: "demo".into(), name: first.clone() }], "test").is_err());
    let FrontResponse::LibraryPruned { removed } = lab.h.library_prune_at(Some("demo"), 1, u64::MAX, "test").unwrap()
    else {
        panic!()
    };
    assert!(removed.is_empty());
    assert_eq!(lab.file(&first, "index.html").await, b"first");
    assert_eq!(lab.file(&second, "index.html").await, b"next");
    // manifest가 없는 옛 잔여 폴더도 완료 할 일의 참조 보호를 유지한다.
    std::fs::remove_file(manifest_path(&lab.h.project_dir("demo").unwrap(), "week")).unwrap();
    assert!(lab.h.library_prune_preview(None, Some(1), u64::MAX).unwrap().candidates.is_empty());
}

#[tokio::test]
async fn archive_pins_one_version_outside_capacity_and_survives_prune_restart_and_explicit_delete() {
    let lab = Lab::new();
    let first = lab.publish("week", "first", false);
    lab.h.library_archive(&[LibraryRef { project: "demo".into(), name: "week".into() }], true, "test").unwrap();
    let second = lab.publish("week", "second", false);
    let rows = lab.h.library_reports(None, false).unwrap();
    assert_eq!(rows.iter().filter(|r| r.archived).map(|r| r.name.as_str()).collect::<Vec<_>>(), vec![first.as_str()]);
    let FrontResponse::LibraryUsage { size, archived_size, .. } = lab.h.library_usage().unwrap() else { panic!() };
    assert_eq!((size, archived_size), (24, 10), "선택한 판만 상한 밖이다. 새 판을 자동으로 보관하지 않는다");
    let preview = lab.h.library_prune_preview(None, Some(1), u64::MAX).unwrap();
    assert_eq!(preview.archived_size, 10);
    assert!(preview.candidates.iter().any(|c| c.name == second));
    assert!(!preview.candidates.iter().any(|c| c.name == first));
    lab.h.library_prune_at(None, 1, u64::MAX, "test").unwrap();
    assert_eq!(lab.file(&first, "index.html").await, b"first");
    assert!(lab.h.report_library("demo", &second, None, false).is_err());
    let reopened = Host::open(lab.h.cfg.clone(), lab.root.join("state")).unwrap();
    assert_eq!(reopened.library_reports(None, false).unwrap().iter().filter(|r| r.archived).count(), 1);
    assert!(reopened.library_prune_preview(None, Some(1), u64::MAX).unwrap().candidates.is_empty());
    // 최신 이름이 정리된 뒤에도 보관 판을 사용자가 명시 삭제할 수 있다.
    reopened.library_delete(&[LibraryRef { project: "demo".into(), name: first.clone() }], "test").unwrap();
    assert!(reopened.report_library("demo", &first, None, false).is_err());
    assert!(!reopened.db.library_archived("demo", &first).unwrap());
}

#[tokio::test]
async fn archive_validates_batch_permissions_and_invalidates_prune_preview() {
    let lab = Lab::new();
    let first = lab.publish("week", "first", false);
    let refs = vec![LibraryRef { project: "demo".into(), name: first.clone() }];
    let request = FrontRequest::LibraryArchive { reports: refs.clone(), archived: true };
    assert_eq!(
        lab.h.call(DeviceRole::Reader, "device:r", request).await.unwrap_err().code,
        aaa_proto::ErrorCode::Forbidden
    );
    let mut invalid = refs.clone();
    invalid.push(LibraryRef { project: "demo".into(), name: "missing".into() });
    assert!(lab.h.library_archive(&invalid, true, "test").is_err());
    assert!(!lab.h.db.library_archived("demo", &first).unwrap());
    let plan = lab.h.library_prune_preview(None, Some(1), u64::MAX).unwrap();
    let expected = plan.candidates.iter().map(|r| format!("{}/{}", r.project, r.name)).collect::<Vec<_>>();
    lab.h
        .call(
            DeviceRole::Controller,
            "device:c",
            FrontRequest::LibraryArchive { reports: refs.clone(), archived: true },
        )
        .await
        .unwrap();
    assert_eq!(
        lab.h.library_prune_apply(None, Some(1), &expected, u64::MAX, "test").unwrap_err().code,
        aaa_proto::ErrorCode::Conflict
    );
    assert_eq!(lab.file(&first, "index.html").await, b"first");
    lab.h.library_archive(&refs, false, "test").unwrap();
    assert!(lab.h.library_prune_preview(None, Some(1), u64::MAX).unwrap().candidates.iter().any(|c| c.name == first));
}

#[test]
fn actual_inventory_includes_unlisted_and_orphan_bytes_without_following_symlinks() {
    let lab = Lab::new();
    lab.publish("week", "first", false);
    let dir = lab.h.project_dir("demo").unwrap();
    std::fs::write(dir.join("library/week/unlisted.bin"), b"leftover").unwrap();
    std::fs::create_dir(dir.join("library/orphan")).unwrap();
    std::fs::write(dir.join("library/orphan/file.bin"), b"orphan").unwrap();
    std::fs::create_dir(dir.join("library/.collecting")).unwrap();
    std::fs::write(dir.join("library/.collecting/file.bin"), b"active").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&lab.root, dir.join("library/week/outside")).unwrap();
    let FrontResponse::LibraryUsage { size, protected_size, .. } = lab.h.library_usage().unwrap() else { panic!() };
    assert_eq!((size, protected_size), (40, 6));
    let plan = lab.h.library_prune_preview(None, Some(1), u64::MAX).unwrap();
    assert!(plan.candidates.iter().any(|c| c.name == "week" && c.size == 18));
    assert!(plan.candidates.iter().any(|c| c.name == "orphan" && c.size == 6));
    assert!(!plan.candidates.iter().any(|c| c.name == ".collecting"));
    lab.h.library_prune_at(None, 1, u64::MAX, "test").unwrap();
    assert!(!dir.join("library/orphan").exists());
    assert!(dir.join("library/.collecting/file.bin").exists());
    assert!(lab.root.join("source/index.html").exists());
}

#[test]
fn archive_batch_and_completion_ledgers_follow_project_export_and_rename() {
    let lab = Lab::new();
    let first = lab.publish("one", "first", false);
    let second = lab.publish("two", "second", false);
    let refs = [first.clone(), second.clone()].map(|name| LibraryRef { project: "demo".into(), name });
    lab.h.library_archive(&refs, true, "test").unwrap();
    assert_eq!(lab.h.library_reports(None, false).unwrap().iter().filter(|r| r.archived).count(), 2);
    lab.h.db.library_complete("demo", "one", "revision", "제목", 10).unwrap();
    let exported = lab.h.db.project_export("demo").unwrap();
    assert_eq!(exported["library_archives"].as_array().unwrap().len(), 2);
    assert_eq!(exported["library_completion"].as_array().unwrap().len(), 1);
    lab.h.db.project_rename("demo", "renamed", str::to_string).unwrap();
    assert!(lab.h.db.library_archived("renamed", &first).unwrap());
    assert!(!lab.h.db.library_archived("demo", &first).unwrap());
    assert!(!lab.h.db.library_complete("renamed", "one", "revision", "제목", 20).unwrap());
    lab.h.db.library_archive_set(&[("renamed".into(), first), ("renamed".into(), second.clone())], false).unwrap();
    assert!(!lab.h.db.library_archived("renamed", &second).unwrap());
}

use super::*;
use aaa_proto::FrontRequest;
use std::sync::Arc;

struct Lab {
    h: Arc<Host>,
    _root: Cleanup,
}
struct Cleanup(PathBuf, std::sync::Weak<Host>);
impl Drop for Cleanup {
    fn drop(&mut self) {
        assert!(self.1.upgrade().is_none(), "Host와 DB를 닫은 뒤 지운다");
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn lab() -> Lab {
    let root = std::env::temp_dir().join(format!("aaa-library-upload-{}", aaa_common::util::new_id()));
    let mut cfg = aaa_config::HostConfig::default();
    cfg.projects.root = root.join("projects").to_string_lossy().into();
    cfg.notify.web_push = false;
    let h = Host::open(cfg, root.join("host")).unwrap();
    h.project_create("demo", Default::default(), "local:cli").unwrap();
    let weak = Arc::downgrade(&h);
    Lab { h, _root: Cleanup(root, weak) }
}
// 다른 시험이 등록한 가짜 비밀(`…0123456789`)의 조각과 겹치지 않는 값: 요청 가림(redact::json)이 그 조각을 지운다
const UP: &str = "a7c3e9b1d5f2084c6e1a9b3d7f5c2e80";

#[test]
fn uploads_check_role_path_and_size_before_writing() {
    let lab = lab();
    let h = &lab.h;
    let put = |role, path: &str, bytes: &[u8]| h.library_upload_put(role, "demo", UP, path, bytes);
    assert_eq!(put(DeviceRole::Reader, "a.md", b"x").unwrap_err().code, ErrorCode::Forbidden);
    for bad in ["../a.md", ".hidden", "a/.git/x", "/abs", "a\\b"] {
        assert_eq!(put(DeviceRole::Controller, bad, b"x").unwrap_err().code, ErrorCode::BadRequest, "{bad}");
    }
    assert_eq!(
        h.library_upload_put(DeviceRole::Controller, "demo", "not-an-id", "a.md", b"x").unwrap_err().code,
        ErrorCode::BadRequest
    );
    assert!(h.library_upload_put(DeviceRole::Controller, "nope", UP, "a.md", b"x").is_err(), "없는 프로젝트");
    // 합계 64MiB까지: 16MiB 넷은 되고 1바이트 더는 거부, 같은 경로를 다시 올리면 그 파일 크기를 빼고 센다
    let big = vec![b'x'; FILE_MAX as usize];
    for i in 0..4 {
        assert_eq!(
            put(DeviceRole::Controller, &format!("big/{i}.bin"), &big).unwrap(),
            (i + 1, (i as u64 + 1) * FILE_MAX)
        );
    }
    assert_eq!(put(DeviceRole::Controller, "one.md", b"x").unwrap_err().code, ErrorCode::TooLarge);
    assert_eq!(put(DeviceRole::Controller, "big/3.bin", b"small").unwrap(), (4, 3 * FILE_MAX + 5));
}

#[tokio::test]
async fn an_upload_becomes_one_report_and_its_folder_goes_away() {
    let lab = lab();
    let h = &lab.h;
    h.library_upload_put(DeviceRole::Controller, "demo", UP, "notes.md", b"# notes\n").unwrap();
    let dir = h.state.join("library-uploads").join(UP);
    // 사용자 보고서는 맨 위 index.html이 있어야 한다: 실패하면 올린 것을 두어 고쳐 다시 등록한다
    let req = |to_user, files| FrontRequest::LibraryUploadPublish {
        project: "demo".into(),
        upload: UP.into(),
        name: "plan-report".into(),
        files,
        to_user,
        title: Some("계획 보고서".into()),
    };
    let e = h.call(DeviceRole::Controller, "device:phone", req(true, 1)).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::BadRequest, "{e:?}");
    assert!(dir.join("notes.md").is_file());
    h.library_upload_put(DeviceRole::Controller, "demo", UP, "index.html", b"<h1>plan</h1>").unwrap();
    h.library_upload_put(DeviceRole::Controller, "demo", UP, "img/a.png", b"png").unwrap();
    // 웹이 올린 수와 올린 자리가 다르면(치워진 뒤 나머지만 온 경우) 등록하지 않는다
    let e = h.call(DeviceRole::Controller, "device:phone", req(true, 4)).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Conflict, "{e:?}");
    // 통째로 치워진 올리기도 같은 답(웹이 새 자리에 처음부터 올린다)
    const GONE: &str = "b7c3e9b1d5f2084c6e1a9b3d7f5c2e80";
    let gone = FrontRequest::LibraryUploadPublish {
        project: "demo".into(),
        upload: GONE.into(),
        name: "gone".into(),
        files: 2,
        to_user: false,
        title: None,
    };
    assert_eq!(h.call(DeviceRole::Controller, "device:phone", gone).await.unwrap_err().code, ErrorCode::Conflict);
    h.call(DeviceRole::Controller, "device:phone", req(true, 3)).await.unwrap();
    assert!(
        !dir.exists() && !mark(&h.state.join("library-uploads"), UP).exists(),
        "등록한 올리기 폴더와 그 표지는 지운다"
    );
    let report = h.project_dir("demo").unwrap().join("library/plan-report");
    for f in ["index.html", "notes.md", "img/a.png"] {
        assert!(report.join(f).is_file(), "{f}");
    }
    // 세션 안 agent는 웹 올리기를 등록하지 못한다(사람만)
    h.library_upload_put(DeviceRole::Controller, "demo", UP, "x.md", b"x").unwrap();
    let e = h.call(DeviceRole::Local, "local:cli@coordinator", req(false, 1)).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Forbidden);
    assert_eq!(h.call(DeviceRole::Reader, "device:tv", req(false, 1)).await.unwrap_err().code, ErrorCode::Forbidden);
}

#[test]
fn a_stale_upload_goes_away_but_the_one_being_written_stays() {
    let lab = lab();
    let h = &lab.h;
    const OLD: &str = "ffffffffffffffffffffffffffffffff";
    h.library_upload_put(DeviceRole::Controller, "demo", OLD, "a.md", b"a").unwrap();
    // 하위 폴더를 미리 만든다: 그 안에 더 써도 맨 위 폴더 항목은 그대로라 OS가 시각을 바꾸지 않는다
    h.library_upload_put(DeviceRole::Controller, "demo", UP, "sub/a.md", b"a").unwrap();
    let root = h.state.join("library-uploads");
    // 둘 다 하루 넘게 손대지 않은 것으로 한다(표지의 시각). 폴더 시각은 OS마다 달라 쓰지 않는다(Windows는 폴더 시각을 열어 바꾸지 못한다)
    let day_ago = aaa_common::util::now_ms() - 25 * 3600 * 1000;
    for id in [OLD, UP] {
        assert!(
            last_put_ms(&root, id).is_some_and(|ms| ms + 3600 * 1000 > aaa_common::util::now_ms()),
            "쓸 때 표지를 남긴다"
        );
        std::fs::write(mark(&root, id), day_ago.to_string()).unwrap();
    }
    // 하루 넘게 둔 올리기에 이어 쓰면 그것은 두고(다른 것만 치운다) 시각을 새로 한다
    assert_eq!(h.library_upload_put(DeviceRole::Controller, "demo", UP, "sub/b.md", b"b").unwrap().0, 2);
    assert!(!root.join(OLD).exists() && !mark(&root, OLD).exists(), "버려진 올리기와 그 표지는 치운다");
    assert!(root.join(UP).join("sub/a.md").is_file(), "쓰는 올리기는 그대로");
    assert!(
        last_put_ms(&root, UP).is_some_and(|ms| ms > day_ago + 3600 * 1000),
        "하위 폴더에 써도 올리기 시각이 새로워진다"
    );
    // 다음 올리기도 방금 쓴 올리기는 치우지 않는다(표지가 새롭다)
    const NEXT: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
    h.library_upload_put(DeviceRole::Controller, "demo", NEXT, "a.md", b"a").unwrap();
    assert!(root.join(UP).join("sub/b.md").is_file(), "새로 쓴 올리기는 다른 올리기의 치우기에 남는다");
}

#[test]
fn an_upload_under_a_thread_numbered_state_path_has_no_origin_thread() {
    // host 상태 폴더 경로에 그 프로젝트 thread 번호가 들어 있어도 웹 올리기의 근원으로 읽지 않는다
    let root = std::env::temp_dir().join(format!("aaa-upload-origin-{}", aaa_common::util::new_id()));
    let mut cfg = aaa_config::HostConfig::default();
    cfg.projects.root = root.join("projects").to_string_lossy().into();
    cfg.notify.web_push = false;
    let h = Host::open(cfg, root.join("projects-t-0001").join("host")).unwrap();
    h.project_create("demo", Default::default(), "local:cli").unwrap();
    let mut r = crate::orch::ThreadRec::starting(
        "demo",
        "보고",
        "mac-a",
        "fake",
        aaa_proto::Role::Worker,
        aaa_proto::ThreadKind::Dir,
        1,
    );
    r.id = "t-0001".into();
    h.db.thread_import("demo", "t-0001", "open", &serde_json::to_string(&r).unwrap()).unwrap();
    h.library_upload_put(DeviceRole::Controller, "demo", UP, "a.md", b"a").unwrap();
    h.library_upload_publish("demo", UP, "notes", 1, false, None, "device:phone").unwrap();
    let row = h.db.library_report("demo", "notes").unwrap().unwrap();
    assert_eq!(row.from_thread, None);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn usage_cache_keeps_limits_replacements_and_failed_writes_correct() {
    let lab = lab();
    let h = &lab.h;
    let put = |path: &str, bytes: &[u8]| h.library_upload_put(DeviceRole::Controller, "demo", UP, path, bytes);
    assert_eq!(put("empty", b"").unwrap(), (1, 0));
    assert_eq!(put("empty", b"changed").unwrap(), (1, 7), "0바이트도 기존 파일이다");
    // 실패 뒤 디스크 기준으로 재계산하고 완료된 다른 파일은 보존한다.
    assert_eq!(put("empty/child", b"x").unwrap_err().code, ErrorCode::Internal);
    assert_eq!(put("next", b"ok").unwrap(), (2, 9));
    const OTHER: &str = "dc3a9f2b7e5081d4c6a3b9f1e5d70286";
    assert_eq!(h.library_upload_put(DeviceRole::Controller, "demo", OTHER, "x", b"z").unwrap(), (1, 1));
    assert_eq!(put("empty", b"").unwrap(), (2, 2), "다른 올리기 뒤 다시 디스크를 센다");
    // 많은 기존 파일은 디스크에 준비하고, 상한을 채우는 마지막 추가는 실제 owner를 통과한다.
    let dir = h.uploads_root().join(UP);
    // 경계는 실제 1000개 경로다. 빈 준비 파일의 inode를 매번 새로 만들 필요는 없다.
    let empty = dir.join("file-2");
    std::fs::write(&empty, b"").unwrap();
    for i in 3..FILES_MAX - 1 {
        std::fs::hard_link(&empty, dir.join(format!("file-{i}"))).unwrap();
    }
    h.library_upload_put(DeviceRole::Controller, "demo", OTHER, "x", b"z").unwrap();
    assert_eq!(put(&format!("file-{}", FILES_MAX - 1), b"").unwrap(), (FILES_MAX, 2));
    assert_eq!(put("extra", b"").unwrap_err().code, ErrorCode::TooLarge);
    assert_eq!(put("file-2", b"replace").unwrap(), (FILES_MAX, 9), "상한에서 빈 파일 교체 가능");
    assert_eq!(std::fs::read(dir.join("file-2")).unwrap(), b"replace", "교체한 내용도 실제 저장한다");
    assert_eq!(std::fs::read(dir.join("file-3")).unwrap(), b"", "교체는 다른 준비 파일을 바꾸지 않는다");
    assert_eq!(put("large", &vec![0; FILE_MAX as usize + 1]).unwrap_err().code, ErrorCode::TooLarge);
}

#[test]
fn independent_upload_states_do_not_block_each_other() {
    let first = lab();
    let second = lab();
    let held = first.h.upload_gate.lock().unwrap();
    let h = second.h.clone();
    let (sent, received) = std::sync::mpsc::channel();
    let writer = std::thread::spawn(move || {
        sent.send(h.library_upload_put(DeviceRole::Controller, "demo", UP, "independent.md", b"ok")).unwrap();
    });
    let result = received.recv_timeout(std::time::Duration::from_secs(2));
    // 실패할 때도 먼저 잠금을 풀고 직접 만든 writer를 회수한다.
    drop(held);
    writer.join().unwrap();
    assert_eq!(result.expect("다른 상태 폴더의 쓰기는 이 잠금을 기다리지 않는다").unwrap(), (1, 2));
    assert_eq!(std::fs::read(second.h.uploads_root().join(UP).join("independent.md")).unwrap(), b"ok");
    drop(first);
}

#[test]
fn upload_state_aliases_share_cached_counts() {
    let lab = lab();
    let h = &lab.h;
    let alias = Host::open(h.cfg.clone(), h.state.join(".")).unwrap();
    assert_eq!(h.library_upload_put(DeviceRole::Controller, "demo", UP, "first", b"a").unwrap(), (1, 1));
    assert_eq!(alias.library_upload_put(DeviceRole::Controller, "demo", UP, "second", b"bb").unwrap(), (2, 3));
    assert_eq!(h.library_upload_put(DeviceRole::Controller, "demo", UP, "third", b"ccc").unwrap(), (3, 6));
    assert_eq!(usage(&h.uploads_root().join(UP)), (3, 6));
}

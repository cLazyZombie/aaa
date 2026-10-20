use super::*;
use aaa_proto::FrontRequest;
use std::sync::Arc;

struct Lab {
    root: PathBuf,
    h: Arc<Host>,
}
impl Drop for Lab {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn lab() -> Lab {
    let root = std::env::temp_dir().join(format!("aaa-library-upload-{}", aaa_common::util::new_id()));
    let mut cfg = aaa_config::HostConfig::default();
    cfg.projects.root = root.join("projects").to_string_lossy().into();
    cfg.notify.web_push = false;
    let h = Host::open(cfg, root.join("host")).unwrap();
    h.project_create("demo", Default::default(), "local:cli").unwrap();
    Lab { root, h }
}
const UP: &str = "0123456789abcdef0123456789abcdef";

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
    let req = |to_user| FrontRequest::LibraryUploadPublish {
        project: "demo".into(),
        upload: UP.into(),
        name: "plan-report".into(),
        to_user,
        title: Some("계획 보고서".into()),
    };
    let e = h.call(DeviceRole::Controller, "device:phone", req(true)).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::BadRequest, "{e:?}");
    assert!(dir.join("notes.md").is_file());
    h.library_upload_put(DeviceRole::Controller, "demo", UP, "index.html", b"<h1>plan</h1>").unwrap();
    h.library_upload_put(DeviceRole::Controller, "demo", UP, "img/a.png", b"png").unwrap();
    h.call(DeviceRole::Controller, "device:phone", req(true)).await.unwrap();
    assert!(!dir.exists(), "등록한 올리기 폴더는 지운다");
    let report = h.project_dir("demo").unwrap().join("library/plan-report");
    for f in ["index.html", "notes.md", "img/a.png"] {
        assert!(report.join(f).is_file(), "{f}");
    }
    // 세션 안 agent는 웹 올리기를 등록하지 못한다(사람만)
    h.library_upload_put(DeviceRole::Controller, "demo", UP, "x.md", b"x").unwrap();
    let e = h.call(DeviceRole::Local, "local:cli@coordinator", req(false)).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Forbidden);
    assert_eq!(h.call(DeviceRole::Reader, "device:tv", req(false)).await.unwrap_err().code, ErrorCode::Forbidden);
}

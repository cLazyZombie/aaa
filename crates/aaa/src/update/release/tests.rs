//! 실제 curl·임시 HTTP 서버·임시 실행 파일 자리로 본다. 운영 설정·키체인·신호·고정 sleep은 쓰지 않는다.

use std::io::{BufRead as _, Write as _};
use std::net::TcpListener;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;

use sha2::Digest as _;

use super::*;

fn digest(bytes: &[u8]) -> String {
    hex::encode(sha2::Sha256::digest(bytes))
}

// Content-Length를 실제 바이트보다 크게 주면 서버가 닫힐 때 전송 중단이다. 시간으로 끊지 않는다.
fn serve(body: Vec<u8>, declared: Option<usize>) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/asset", listener.local_addr().unwrap());
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream.set_read_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
        let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
        let mut request = String::new();
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                break;
            }
            request.push_str(&line);
        }
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            declared.unwrap_or(body.len())
        )
        .unwrap();
        stream.write_all(&body).unwrap();
        request
    });
    (url, handle)
}

fn manifest(version: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "version": version,
        "notes": "시험 release",
        "assets": {
            "macos-aarch64": {"url": format!("{RELEASES}/download/v{version}/aaa-macos-aarch64"), "sha256": digest(b"mac")},
            "windows-x86_64": {"url": format!("{RELEASES}/download/v{version}/aaa-windows-x86_64.exe"), "sha256": digest(b"win")}
        }
    })).unwrap()
}

struct Lab(Place);

impl Lab {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "aaa-release-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&dir).unwrap();
        let exe = dir.join(if cfg!(windows) { "aaa.exe" } else { "aaa" });
        std::fs::write(&exe, b"original build").unwrap();
        Self(Place { exe, dir })
    }

    fn untouched(&self) {
        assert_eq!(std::fs::read(&self.0.exe).unwrap(), b"original build");
        assert!(!self.0.old().exists(), "실패·검사만에서는 보관도 하지 않는다");
        assert!(
            !std::fs::read_dir(&self.0.dir)
                .unwrap()
                .flatten()
                .any(|e| e.file_name().to_string_lossy().starts_with(".aaa-update-")),
            "staging 잔여"
        );
    }
}

impl Drop for Lab {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0.dir).unwrap();
    }
}

#[test]
fn manifest_selects_platform_from_http() {
    let (url, server) = serve(manifest("0.2.0"), None);
    let m = fetch_manifest(&url).unwrap();
    let mac = m.select("macos", "aarch64", "0.1.0", None).unwrap().unwrap();
    let win = m.select("windows", "x86_64", "0.1.0", Some("0.2.0")).unwrap().unwrap();
    assert_eq!(m.select("macos", "aarch64", "0.1.0+abc123", None).unwrap().unwrap().version, "0.2.0");
    assert_eq!(m.select("macos", "aarch64", "0.3.0+abc123", Some("0.2.0")).unwrap().unwrap().version, "0.2.0");
    assert!(mac.asset.url.ends_with("/aaa-macos-aarch64"));
    assert_eq!(mac.asset.sha256, digest(b"mac"));
    assert!(win.asset.url.ends_with("/aaa-windows-x86_64.exe"));
    assert_eq!(win.asset.sha256, digest(b"win"));
    let request = server.join().unwrap();
    assert!(request.starts_with("GET /asset HTTP/1.1"));
    assert!(!request.to_ascii_lowercase().contains("authorization:"));
    assert!(
        m.select("macos", "aarch64", "0.1.0", Some("0.3.0")).err().unwrap().to_string().contains("요청한 release 판")
    );
}

#[test]
fn checksum_mismatch_leaves_installation_untouched() {
    let lab = Lab::new();
    let (url, server) = serve(b"corrupt build".to_vec(), None);
    let error = download(&url, &digest(b"expected build"), &lab.0).err().expect("checksum 거부");
    assert!(error.to_string().contains("SHA-256 불일치"), "{error:#}");
    server.join().unwrap();
    lab.untouched();
}

#[test]
fn unsupported_platform_is_rejected_from_http_manifest() {
    let (url, server) = serve(manifest("0.2.0"), None);
    let m = fetch_manifest(&url).unwrap();
    let error = m.select("windows", "aarch64", "0.1.0", None).err().expect("없는 OS/arch 거부");
    assert!(error.to_string().contains("windows-aarch64 실행 파일이 없다"), "{error:#}");
    server.join().unwrap();
}

#[test]
fn interrupted_download_is_not_installed_even_if_partial_hash_matches() {
    let lab = Lab::new();
    let partial = b"partial build";
    let (url, server) = serve(partial.to_vec(), Some(partial.len() + 100));
    let error = download(&url, &digest(partial), &lab.0).err().expect("전송 중단 거부");
    assert!(error.to_string().contains("내려받기 실패"), "{error:#}");
    server.join().unwrap();
    lab.untouched();
}

#[test]
fn same_version_needs_no_asset_or_staging() {
    let lab = Lab::new();
    let (url, server) = serve(br#"{"version":"0.1.0","assets":{}}"#.to_vec(), None);
    let m = fetch_manifest(&url).unwrap();
    assert!(m.select("macos", "aarch64", "0.1.0", None).unwrap().is_none());
    assert!(m.select("macos", "aarch64", "0.1.0", Some("0.1.0")).unwrap().is_none());
    assert!(m.select("macos", "aarch64", "0.2.0", None).unwrap().is_none(), "latest가 옛 판이면 내리지 않는다");
    server.join().unwrap();
    lab.untouched();
}

#[test]
fn downloaded_build_keeps_previous_and_rolls_back() {
    let lab = Lab::new();
    for (version, body) in [("0.1.0", b"build one".as_slice()), ("0.2.0", b"build two".as_slice())] {
        let (url, server) = serve(body.to_vec(), None);
        let staged = download(&url, &digest(body), &lab.0).unwrap();
        let checked_sha = sha256(staged.path()).unwrap();
        staged.install(&lab.0, version, &checked_sha).unwrap();
        server.join().unwrap();
        assert_eq!(std::fs::read(&lab.0.exe).unwrap(), body);
    }
    let chosen = lab.0.rollback_source(None).unwrap();
    assert_eq!(std::fs::read(&chosen).unwrap(), b"build one", "가장 새 보관으로 돌아간다");
    let chosen_by_name = lab.0.rollback_source(chosen.file_name().unwrap().to_str()).unwrap();
    assert_eq!(chosen, chosen_by_name);
    let saved =
        Staged::copy_from(&chosen, &lab.0).unwrap().install(&lab.0, "0.3.0", &sha256(&chosen).unwrap()).unwrap();
    assert_eq!(std::fs::read(&lab.0.exe).unwrap(), b"build one");
    assert_eq!(std::fs::read(saved).unwrap(), b"build two", "되돌리기 전 것도 보관한다");
    assert_eq!(std::fs::read(lab.0.rollback_source(None).unwrap()).unwrap(), b"build two");
}

#[test]
fn rollback_order_survives_version_decrease_within_one_clock_tick() {
    let lab = Lab::new();
    std::fs::create_dir_all(lab.0.old()).unwrap();
    let now = 1_800_000_000_123;
    let first = lab.0.backup_path(now, "1.0.0");
    std::fs::write(&first, b"first").unwrap();
    let second = lab.0.backup_path(now, "0.1.0");
    std::fs::write(&second, b"second").unwrap();
    assert_eq!(lab.0.rollback_source(None).unwrap(), second, "판 문자열이 아니라 생성 순서로 되돌린다");
    super::super::prune(&lab.0, 1);
    let third = lab.0.backup_path(now, "2.0.0");
    std::fs::write(&third, b"third").unwrap();
    assert_eq!(lab.0.rollback_source(None).unwrap(), third, "보관 정리 뒤에도 순서를 이어 간다");
}

#[test]
fn release_urls_and_checksum_are_bound_to_requested_version() {
    assert_eq!(requested_version(Some("v1.2.3")).unwrap().as_deref(), Some("1.2.3"));
    for bad in ["../main", "1.2", "01.2.3", "1.2.3?token=x"] {
        assert!(requested_version(Some(bad)).is_err(), "{bad}");
    }
    assert_eq!(manifest_url(None), format!("{RELEASES}/latest/download/manifest.json"));
    assert_eq!(manifest_url(Some("1.2.3")), format!("{RELEASES}/download/v1.2.3/manifest.json"));
    for bad in [
        "https://example.com/aaa",
        "https://github.com/other/aaa/releases/download/v0.2.0/aaa",
        "https://github.com/cLazyZombie/aaa/releases/download/v0.1.0/aaa",
        "https://github.com/cLazyZombie/aaa/releases/download/v0.2.0/aaa?token=x",
    ] {
        let mut m: Manifest = serde_json::from_slice(&manifest("0.2.0")).unwrap();
        m.assets.get_mut("macos-aarch64").unwrap().url = bad.into();
        assert!(m.select("macos", "aarch64", "0.1.0", None).err().unwrap().to_string().contains("공개 GitHub release"));
    }
    let mut m: Manifest = serde_json::from_slice(&manifest("0.2.0")).unwrap();
    m.assets.get_mut("macos-aarch64").unwrap().sha256 = "bad".into();
    assert!(m.select("macos", "aarch64", "0.1.0", None).err().unwrap().to_string().contains("64자리"));
}

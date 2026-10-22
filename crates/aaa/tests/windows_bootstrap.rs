//! Windows PowerShell 5.1/7·PE·CreateProcess argv 실제 경계. SCM/사용자 설정은 바꾸지 않는다.
#![cfg(windows)]
use std::path::Path;
use std::process::{Command, Stdio};

#[test]
fn pinned_bootstrap_preserves_native_argv_exit_and_existing_files() {
    let root = std::env::temp_dir().join(format!("aaa-bootstrap-owner-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    struct Cleanup<'a>(&'a Path);
    impl Drop for Cleanup<'_> {
        fn drop(&mut self) {
            std::fs::remove_dir_all(self.0).unwrap();
        }
    }
    let _cleanup = Cleanup(&root);
    let source = root.join("fixture.rs");
    let fixture = root.join("fixture.exe");
    // Only an OS/native executable stand-in: no install decisions, ready or approval.
    std::fs::write(
        &source,
        r#"use std::io::Read;
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--version"] {
        println!("aaa {}", std::env::var("AAA_BOOTSTRAP_VERSION").unwrap());
        return;
    }
    let mut text = String::new();
    for arg in &args {
        for byte in arg.as_bytes() { text.push_str(&format!("{byte:02x}")); }
        text.push('\n');
    }
    if args.iter().any(|a| a == "--read-stdin") {
        let mut input = String::new();
        std::io::stdin().read_to_string(&mut input).unwrap();
        assert_eq!(input, "external-console-input\n");
        println!("inherited-stdio-ok");
    }
    std::fs::write(std::env::var_os("AAA_BOOTSTRAP_ARGV").unwrap(), text).unwrap();
    std::process::exit(23);
}"#,
    )
    .unwrap();
    let build = Command::new("rustc")
        .arg(&source)
        .args(["--edition=2024", "-C", "target-feature=+crt-static", "-o"])
        .arg(&fixture)
        .output()
        .unwrap();
    assert!(build.status.success(), "native argv fixture: {build:?}");
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap();
    let entry = root.join("install.ps1");
    use sha2::Digest;
    let digest = hex::encode(sha2::Sha256::digest(std::fs::read(&fixture).unwrap()));
    let rendered = Command::new("python")
        .current_dir(repository)
        .args(["-c", "from pathlib import Path; import sys; from scripts.release import render_windows_entry; Path(sys.argv[3]).write_bytes(render_windows_entry(sys.argv[1], sys.argv[2]))"])
        .arg(env!("CARGO_PKG_VERSION"))
        .arg(&digest)
        .arg(&entry)
        .output()
        .unwrap();
    assert!(rendered.status.success(), "published entry rendering: {rendered:?}");
    for shell in ["powershell.exe", "pwsh.exe"] {
        let lab = root.join(shell);
        std::fs::create_dir(&lab).unwrap();
        // Exercise irm's octet-stream decoding and the actual pipe into iex, not only a local string.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/install.ps1", listener.local_addr().unwrap());
        let body = std::fs::read(&entry).unwrap();
        let server = std::thread::spawn(move || {
            use std::io::{BufRead, Write};
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
            let mut request = String::new();
            reader.read_line(&mut request).unwrap();
            assert!(request.starts_with("GET /install.ps1 "));
            loop {
                let mut line = String::new();
                assert!(reader.read_line(&mut line).unwrap() > 0);
                if line == "\r\n" {
                    break;
                }
            }
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).unwrap();
            stream.write_all(&body).unwrap();
        });
        let mut child = Command::new(shell)
            .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-File"])
            .arg(repository.join("scripts/bootstrap/windows-owner.ps1"))
            .arg("-Bootstrap")
            .arg(repository.join("scripts/bootstrap/install-windows.ps1"))
            .arg("-EntryUrl")
            .arg(&url)
            .arg("-Entry")
            .arg(&entry)
            .arg("-Fixture")
            .arg(&fixture)
            .arg("-Aaa")
            .arg(env!("CARGO_BIN_EXE_aaa"))
            .arg("-AaaVersion")
            .arg(env!("CARGO_PKG_VERSION"))
            .arg("-Lab")
            .arg(&lab)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap_or_else(|e| panic!("required Windows shell {shell}: {e}"));
        use std::io::Write;
        child.stdin.take().unwrap().write_all(b"external-console-input\n").unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "bootstrap owner {shell}: status={} stdout={} stderr={}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        server.join().unwrap();
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(text.contains("inherited-stdio-ok"), "console inheritance {shell}: {text}");
        assert!(text.contains("bootstrap-owner-ok"), "owner completion {shell}: {text}");
    }
}

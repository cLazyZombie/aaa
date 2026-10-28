use super::authorization::{redact_private, terminal_steps};
use super::*;

// 원래 CI에는 제목 OSC만 남았다. 공개 사건의 타입·순서만 출력해 승인 파일이나
// private 입력을 노출하지 않고 worker 시작/파일 적용/등록 중 마지막 경계를 구분한다.
struct RecoveryDiagnostic {
    phase: &'static str,
    started: std::time::Instant,
    events: Option<PathBuf>,
}
impl RecoveryDiagnostic {
    fn mark(&mut self, phase: &'static str, events: Option<PathBuf>) {
        eprintln!("closed-window phase={} elapsed={:?}", self.phase, self.started.elapsed());
        self.phase = phase;
        self.started = std::time::Instant::now();
        self.events = events;
        eprintln!("closed-window phase={phase} started");
    }
}
impl Drop for RecoveryDiagnostic {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            return;
        }
        eprintln!("closed-window failed phase={} elapsed={:?}", self.phase, self.started.elapsed());
        let Some(path) = &self.events else { return };
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) => {
                eprintln!("closed-window public events unavailable: {error}");
                return;
            }
        };
        for line in text.lines() {
            match serde_json::from_str::<aaa_proto::install::InstallStreamItem>(line) {
                Ok(aaa_proto::install::InstallStreamItem::Event { seq, phase, state, .. }) => {
                    eprintln!("closed-window public event seq={seq} phase={phase:?} state={state:?}");
                }
                Ok(aaa_proto::install::InstallStreamItem::Result { seq, result, .. }) => {
                    eprintln!("closed-window public result seq={seq} exit={}", result.exit_code);
                }
                // 파싱 오류도 원문 줄·payload를 출력하지 않는다.
                Err(error) => eprintln!(
                    "closed-window public event parse failed: category={:?} line={} column={}",
                    error.classify(),
                    error.line(),
                    error.column()
                ),
            }
        }
    }
}

#[test]
fn closed_enrollment_window_keeps_the_id_and_action_after_files_are_applied() {
    use aaa_proto::DeviceRole;
    let lab = std::mem::ManuallyDrop::new(Lab::new());
    eprintln!("closed-window owner lab: {}", lab.0.display());
    let mut diagnostic = RecoveryDiagnostic { phase: "prepare", started: std::time::Instant::now(), events: None };
    let codex = lab.0.join(".codex");
    std::fs::create_dir(&codex).unwrap();
    std::fs::write(codex.join("config.toml"), "[features]\nhooks = true\n").unwrap();
    std::fs::write(
        codex.join("hooks.json"),
        r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"another-tool --event"}]}]}}"#,
    )
    .unwrap();
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let host = rt.block_on(async {
        let mut cfg = aaa_config::HostConfig::default();
        cfg.notify.web_push = false;
        cfg.host.agent_listen = "127.0.0.1:0".into();
        let host = aaa_host::Host::open(cfg, lab.0.join("closed-host")).unwrap();
        aaa_host::link::start(&host).await.unwrap();
        host
    });
    let mut base = lab.args();
    base.retain(|arg| arg != "--dry-run");
    let harness = base.iter().position(|arg| arg == "--harness").unwrap();
    base[harness + 1] = "codex".into();
    base.extend([
        "--codex-path".into(),
        BIN.into(),
        "--codex-home".into(),
        codex.to_string_lossy().into_owned(),
        "--hooks".into(),
        "install".into(),
        "--login".into(),
        "defer".into(),
    ]);

    let at = base.iter().position(|arg| arg == "--host").unwrap();
    base[at + 1] = host.agent_listen.lock().unwrap().clone().unwrap();
    let at = base.iter().position(|arg| arg == "--enroll").unwrap();
    base[at + 1] = "wait".into();
    let approval = lab.0.join("closed-approval.json");
    let mut issue = base.clone();
    issue.extend([
        "--prompt".into(),
        "plain".into(),
        "authorize".into(),
        "--out".into(),
        approval.to_string_lossy().into_owned(),
    ]);
    diagnostic.mark("authorize", None);
    assert_eq!(terminal_steps(&lab, &issue, &[("정확히 yes", "yes\r")], false).0, 0);
    let issued: Value = serde_json::from_slice(&std::fs::read(&approval).unwrap()).unwrap();
    let id = issued["claims"]["installation_id"].as_str().unwrap();
    let mut interactive = base.clone();
    let resume_events = lab.0.join("resume-events.jsonl");
    interactive.extend([
        "--prompt".into(),
        "plain".into(),
        "--events-file".into(),
        resume_events.to_string_lossy().into_owned(),
    ]);
    let apply_events = lab.0.join("apply-events.jsonl");
    base.extend([
        "--authorization-file".into(),
        approval.to_string_lossy().into_owned(),
        "--prompt".into(),
        "plain".into(),
        "--events-file".into(),
        apply_events.to_string_lossy().into_owned(),
    ]);
    diagnostic.mark("apply", Some(apply_events.clone()));
    let (code, text) = terminal_steps(&lab, &base, &[], false);
    assert_eq!(code, 3, "{}", redact_private(&text));
    diagnostic.mark("observe-applied", Some(apply_events));
    assert!(lab.0.join("config/agent.toml").is_file(), "부분 파일 적용을 삭제하면 안 된다");
    let before = snapshot(&lab.0);
    let observed = json(&lab.cmd(&["install", "--status", id, "--json"]));
    assert_eq!(observed["installation_id"], id, "{observed}");
    assert!(
        observed["needs_user"]
            .as_array()
            .unwrap()
            .iter()
            .any(|need| need["code"] == "installation_enrollment_window_closed"),
        "{observed}"
    );
    assert!(snapshot(&lab.0) == before, "status는 기록을 변경하지 않는다");
    let aaa_proto::FrontResponse::Pending { pending, .. } =
        rt.block_on(host.call(DeviceRole::Local, "local:cli", aaa_proto::FrontRequest::EnrollPending)).unwrap()
    else {
        panic!("pending")
    };
    assert!(pending.is_empty());
    assert!(
        text.contains("installation_enrollment_window_closed") && text.contains("등록 창"),
        "{}",
        redact_private(&text)
    );

    let nonce = issued["claims"]["nonce"].clone();
    diagnostic.mark("resume", Some(resume_events.clone()));
    let (code, text) =
        terminal_steps(&lab, &interactive, &[("남은 설치 처리", "resume\r"), ("정확히 yes", "yes\r")], false);
    assert_eq!(code, 3, "{}", redact_private(&text));
    diagnostic.mark("observe-resumed", Some(resume_events));
    assert!(text.contains(id) && text.contains("등록 창"), "{}", redact_private(&text));
    let observed = json(&lab.cmd(&["install", "--status", id, "--json"]));
    assert_eq!(observed["installation_id"], id, "{observed}");
    let input: Value = serde_json::from_slice(
        &aaa_common::private_file::read(
            &lab.0.join("state/install/authorization").join(format!("{id}.input.json")),
            1024 * 1024,
        )
        .unwrap(),
    )
    .unwrap();
    assert_ne!(input["authorization"]["claims"]["nonce"], nonce, "새 동의는 같은 ID의 새 nonce여야 한다");
    let jobs = std::fs::read_dir(lab.0.join("state/install/jobs")).unwrap().count();
    assert_eq!(jobs, 1, "재개는 파일 transaction을 하나 더 만들지 않는다");

    // Listener tasks retain Host/SQLite handles. Runtime drop joins their shutdown;
    // release the last Host before removing its lab on Windows too.
    diagnostic.mark("cleanup", None);
    drop(rt);
    drop(host);
    drop(std::mem::ManuallyDrop::into_inner(lab));
    diagnostic.mark("completed", None);
}

#[test]
fn status_failure_retains_the_requested_id_and_safe_phase_without_writes() {
    let lab = Lab::new();
    let id = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
    let before = snapshot(&lab.0);
    let observed = json(&lab.cmd(&["install", "--status", id, "--json"]));
    assert_eq!(observed["error"]["code"], "installation_not_found", "{observed}");
    assert_eq!(observed["installation_id"], id, "{observed}");
    assert_eq!(observed["error"]["details"], serde_json::json!(["phase=file_journal"]), "{observed}");
    assert!(snapshot(&lab.0) == before, "오류 진단을 위해 state/nonce/lock을 만들면 안 된다");
}

/// PS 5.1/7의 실제 irm | iex → PE → setup 콘솔/취소 경계다.
/// HTTP transport만 격리하며 실제 SCM/계정/LSA는 변경하지 않는다.
#[cfg(windows)]
#[test]
fn one_line_entry_enters_setup_and_cancels_without_installation_changes() {
    use sha2::Digest;
    use std::io::{BufRead, Read, Write};
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap();
    let digest = hex::encode(sha2::Sha256::digest(std::fs::read(BIN).unwrap()));
    for shell in ["powershell.exe", "pwsh.exe"] {
        let lab = Lab::new();
        // 차가운 명령 탐색이 러너의 AWS/Graph 등 무관한 모듈을 분석하지 않게 한다.
        // 같은 이름의 ambient 명령이 있어도 실제 내장 IRM으로 HTTP를 받아야 한다.
        let ambient = lab.0.join("ambient-modules/OutsideInstaller");
        std::fs::create_dir_all(&ambient).unwrap();
        std::fs::write(
            ambient.join("OutsideInstaller.psd1"),
            "@{ ModuleVersion='1.0'; RootModule='OutsideInstaller.psm1'; FunctionsToExport=@('Invoke-RestMethod'); CmdletsToExport=@(); AliasesToExport=@() }",
        )
        .unwrap();
        std::fs::write(
            ambient.join("OutsideInstaller.psm1"),
            "function Invoke-RestMethod { throw 'unrelated module must not serve the installation entry' }\nExport-ModuleMember -Function Invoke-RestMethod\n",
        )
        .unwrap();
        let old_binary = lab.0.join(".local/bin/aaa.exe");
        std::fs::create_dir_all(old_binary.parent().unwrap()).unwrap();
        std::fs::write(&old_binary, b"old aaa executor preserved").unwrap();
        // HTTP fixture는 직렬화한 release 바이트를 제공한다. 중첩 PowerShell
        // pipeline으로 JSON을 만드는 일은 이 설치 owner의 계약이 아니다.
        let manifest = lab.0.join("manifest.json");
        let base = format!("https://github.com/cLazyZombie/aaa/releases/download/v{}", env!("CARGO_PKG_VERSION"));
        std::fs::write(
            &manifest,
            serde_json::to_vec(&serde_json::json!({
                "version": env!("CARGO_PKG_VERSION"),
                "assets": { "windows-x86_64": {
                    "url": format!("{base}/aaa-windows-x86_64.exe"),
                    "sha256": digest,
                } },
            }))
            .unwrap(),
        )
        .unwrap();
        let entry = lab.0.join("install.ps1");
        let rendered = Command::new("python").current_dir(repository)
            .args(["-c", "from pathlib import Path; import sys; from scripts.release import render_windows_entry; Path(sys.argv[3]).write_bytes(render_windows_entry(sys.argv[1], sys.argv[2]))"])
            .arg(env!("CARGO_PKG_VERSION")).arg(&digest).arg(&entry).output().unwrap();
        assert!(rendered.status.success(), "published entry: {rendered:?}");
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/install.ps1", listener.local_addr().unwrap());
        let body = std::fs::read(&entry).unwrap();
        let http_phase = std::sync::Arc::new(AtomicU32::new(0));
        let server_phase = http_phase.clone();
        let server = std::thread::spawn(move || {
            server_phase.store(1, Ordering::Relaxed);
            let (mut stream, _) = listener.accept().unwrap();
            server_phase.store(2, Ordering::Relaxed);
            let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
            loop {
                let mut line = String::new();
                assert!(reader.read_line(&mut line).unwrap() > 0);
                if line == "\r\n" {
                    break;
                }
            }
            server_phase.store(3, Ordering::Relaxed);
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).unwrap();
            stream.write_all(&body).unwrap();
            // 완전한 응답을 FIN으로 닫는다. Windows에서 write 직후 socket을 버리면
            // IRM이 아직 읽는 동안 reset으로 관측할 수 있다. peer EOF까지 수신도 비운다.
            stream.shutdown(std::net::Shutdown::Write).unwrap();
            server_phase.store(4, Ordering::Relaxed);
            stream.set_read_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
            let mut trailing = Vec::new();
            reader.read_to_end(&mut trailing).unwrap();
            assert!(trailing.is_empty(), "entry GET 뒤 예상하지 않은 요청 bytes");
            server_phase.store(5, Ordering::Relaxed);
        });
        let trace = lab.0.join("entry-stage.txt");
        let runner = lab.0.join("entry-owner.ps1");
        std::fs::write(
            &runner,
            r#"[IO.File]::WriteAllText($env:AAA_TEST_TRACE, 'powershell_started')
$env:PSModulePath = [IO.Path]::Combine($PSHOME, 'Modules')
function Invoke-WebRequest {
    param($Uri, $OutFile, [switch]$UseBasicParsing)
    $base = 'https://github.com/cLazyZombie/aaa/releases/download/v' + $env:AAA_TEST_VERSION
    if ($Uri -ceq ($base + '/manifest.json')) {
        [IO.File]::WriteAllText($env:AAA_TEST_TRACE, 'manifest_request')
        [IO.File]::Copy($env:AAA_TEST_MANIFEST, $OutFile, $false)
        [IO.File]::WriteAllText($env:AAA_TEST_TRACE, 'manifest_written')
    } elseif ($Uri -ceq ($base + '/aaa-windows-x86_64.exe')) {
        [IO.File]::WriteAllText($env:AAA_TEST_TRACE, 'payload_request')
        [IO.File]::Copy($env:AAA_TEST_PAYLOAD, $OutFile, $false)
        [IO.File]::WriteAllText($env:AAA_TEST_TRACE, 'payload_written')
    } else { throw 'unexpected fixture transport' }
}
[IO.File]::WriteAllText($env:AAA_TEST_TRACE, 'irm_entry')
irm $env:AAA_TEST_ENTRY_URL | iex
[IO.File]::WriteAllText($env:AAA_TEST_TRACE, 'entry_returned')
exit $global:LASTEXITCODE
"#,
        )
        .unwrap();
        // 공개 입구는 v0.1.6부터 setup이다. 계정/서비스 조작 전 첫 질문에서
        // 실제 콘솔 EOF(Ctrl+Z Enter)로 취소한다. install의 서명 적용은 별도 owner다.
        let steps = [("host 주소 (이름:포트)", "\x1a\r")];
        let inherited = lab.command(&[]);
        let mut command = Command::new(shell);
        command.env_clear();
        for (key, value) in inherited.get_envs() {
            if let Some(value) = value {
                command.env(key, value);
            }
        }
        command
            .env("TEMP", &lab.0)
            .env("TMP", &lab.0)
            // 비관리자 실행도 UAC 창을 열지 않고 실패하도록 제한한다.
            .env("SSH_CONNECTION", "fixture")
            .env("PSModulePath", ambient.parent().unwrap())
            // LOCALAPPDATA 환경만 바꿔도 Windows 폴더 API의 캐시 위치는 바뀌지 않는다.
            .env("PSModuleAnalysisCachePath", lab.0.join("module-analysis-cache"))
            .env("AAA_TEST_VERSION", env!("CARGO_PKG_VERSION"))
            .env("AAA_TEST_MANIFEST", &manifest)
            .env("AAA_TEST_PAYLOAD", BIN)
            .env("AAA_TEST_ENTRY_URL", &url)
            .env("AAA_TEST_TRACE", &trace)
            .args(["-NoLogo", "-NoProfile", "-File"])
            .arg(&runner);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            super::authorization::terminal_program(&command, &steps)
        }));
        let (code, text) = result.unwrap_or_else(|panic| {
            let stage = std::fs::read_to_string(&trace).unwrap_or_else(|error| {
                format!("trace_unreadable:{:?}:{:?}", error.kind(), error.raw_os_error())
            });
            let executor = lab.0.join(".local/share/aaa/installers").join(&digest[..16]).join("aaa.exe");
            eprintln!(
                "one-line owner stalled: shell={shell}, stage={stage}, http_phase={}, executor={}, config={}, active={}",
                http_phase.load(Ordering::Relaxed),
                executor.is_file(),
                lab.0.join("config/agent.toml").is_file(),
                lab.0.join("state/install/active.json").is_file(),
            );
            std::panic::resume_unwind(panic)
        });
        eprintln!(
            "one-line owner returned: shell={shell}, stage={}, http_phase={}",
            std::fs::read_to_string(&trace).unwrap(),
            http_phase.load(Ordering::Relaxed),
        );
        assert_eq!(code, 130, "{}: {}", shell, redact_private(&text));
        assert!(text.contains("취소했습니다. 아무것도 바꾸지 않았습니다."), "{}", redact_private(&text));
        assert_eq!(std::fs::read_to_string(&trace).unwrap(), "entry_returned");
        server.join().unwrap();
        assert_eq!(std::fs::read(&old_binary).unwrap(), b"old aaa executor preserved");
        let executor = lab.0.join(".local/share/aaa/installers").join(&digest[..16]).join("aaa.exe");
        assert_eq!(hex::encode(sha2::Sha256::digest(std::fs::read(executor).unwrap())), digest);
        assert!(!lab.0.join("config").exists(), "setup 취소가 설정을 만들면 안 된다");
        assert!(!lab.0.join("state").exists(), "setup 취소가 설치 기록을 만들면 안 된다");
    }
}

/// 공개 setup 입구와 독립된 기존 install의 전체 질문·검토·서명 파일 적용 계약.
#[cfg(windows)]
#[test]
fn external_console_reviews_and_installs_signed_files_without_service_changes() {
    let lab = Lab::new();
    let old_binary = lab.0.join(".local/bin/aaa.exe");
    std::fs::create_dir_all(old_binary.parent().unwrap()).unwrap();
    std::fs::write(&old_binary, b"old aaa executor preserved").unwrap();
    // 설치는 binary 옆에 소유 기록도 쓴다. Cargo 산출물을 설치 대상으로 삼으면
    // 다음 빌드와 충돌하므로 공개 bootstrap과 같이 Lab 안의 별도 사본을 쓴다.
    let executor = lab.0.join("executor/aaa.exe");
    std::fs::create_dir(executor.parent().unwrap()).unwrap();
    std::fs::copy(BIN, &executor).unwrap();
    let work = format!("{}\r", lab.0.join("work").display());
    let steps = [
        ("설치 방식", "fresh\r"),
        ("이 머신의 역할", "agent\r"),
        ("머신 표시 이름", "windows-a\r"),
        ("작업 root 절대 경로", work.as_str()),
        ("저장소(internal", "internal\r"),
        ("남겨 둘 여유 GB", "0\r"),
        ("사용할 harness", "none\r"),
        ("harness 로그인 검사", "defer\r"),
        ("서비스 관리자", "none\r"),
        ("지금 시작할까요", "deferred\r"),
        ("사용자 PATH", "preserve\r"),
        ("연결할 host:port", "example.ts.net:7430\r"),
        ("네트워크", "direct\r"),
        ("host 등록 방식", "skip\r"),
        ("검토 후 승인할까요", "review\r"),
        ("정확히 yes", "yes\r"),
    ];
    let args = ["install".into(), "--bin-path".into(), executor.to_string_lossy().into_owned()];
    let (code, text) = terminal_steps(&lab, &args, &steps, false);
    assert_eq!(code, 0, "{}", redact_private(&text));
    assert_eq!(std::fs::read(&old_binary).unwrap(), b"old aaa executor preserved");
    assert!(lab.0.join("config/agent.toml").is_file());
    let id: String = serde_json::from_slice(
        &aaa_common::private_file::read(&lab.0.join("state/install/active.json"), 1024).unwrap(),
    )
    .unwrap();
    let status = json(&lab.cmd(&["install", "--status", &id, "--json"]));
    assert_eq!(status["installation_id"], id, "{status}");
    assert_eq!(status["exit_code"], 0, "{status}");
    assert_eq!(std::fs::read_dir(lab.0.join("state/install/jobs")).unwrap().count(), 1);
}

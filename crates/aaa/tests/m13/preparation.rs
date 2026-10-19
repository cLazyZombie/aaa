//! 소유 경계: 실제 host·agent·CLI와 실제 Git. Git 응답만 socket으로 붙잡아 old 15초 경계·연결 유실을 재현한다.
//! 회귀: 공통 15초, journal 누락, resolve의 조기 반환, 변경 파일을 덮는 회수는 각각 아래 관찰에서 실패한다.
use super::*;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};

struct GitGate {
    listener: TcpListener,
}
impl GitGate {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        Self { listener }
    }
    fn world(&self) -> World {
        let addr = self.listener.local_addr().unwrap().to_string();
        world_setup("", false, |root| {
            let real = Command::new("which").arg("git").output().unwrap();
            assert!(real.status.success());
            let real = String::from_utf8(real.stdout).unwrap();
            let bin = root.join("git-bin");
            std::fs::create_dir(&bin).unwrap();
            let source = root.join("git-source.py");
            std::fs::write(&source, r#"#!/usr/bin/env python3
import os, pathlib, socket, subprocess, sys
args = sys.argv[1:]
rc = subprocess.call([os.environ['REAL_GIT'], *args])
if rc == 0 and 'worktree' in args:
    index = args.index('worktree')
    action = args[index + 1]
    gate = pathlib.Path(os.environ['GIT_GATE_ROOT']) / ('gate-' + action)
    if gate.exists():
        with socket.create_connection(tuple([os.environ['GIT_GATE_HOST'], int(os.environ['GIT_GATE_PORT'])]), timeout=60) as s:
            s.sendall((action + '\n').encode())
            if s.recv(1) != b'!':
                sys.exit(42)
sys.exit(rc)
"#).unwrap();
            // 실행 파일은 별도 복사 자식이 쓴다(docs/testing.md, ETXTBSY 쓰기 fd 상속 경계).
            assert!(Command::new("cp").arg(&source).arg(bin.join("git")).status().unwrap().success());
            std::fs::set_permissions(bin.join("git"), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
            let (host, port) = addr.rsplit_once(':').unwrap();
            vec![
                ("PATH".into(), format!("{}:{}", bin.display(), std::env::var("PATH").unwrap())),
                ("REAL_GIT".into(), real.trim().into()),
                ("GIT_GATE_ROOT".into(), root.to_string_lossy().into()),
                ("GIT_GATE_HOST".into(), host.into()),
                ("GIT_GATE_PORT".into(), port.into()),
            ]
        })
    }
    fn take(&self, action: &str) -> TcpStream {
        let (mut stream, _) = wait_for("Git 진입 사건", T, || self.listener.accept().ok());
        stream.set_nonblocking(false).unwrap();
        stream.set_read_timeout(Some(T)).unwrap();
        let mut text = vec![0; action.len() + 1];
        stream.read_exact(&mut text).unwrap();
        assert_eq!(text, format!("{action}\n").as_bytes());
        stream
    }
    fn enable(w: &World, action: &str) {
        std::fs::write(w.lab.root.join(format!("gate-{action}")), "").unwrap();
    }
    fn release(w: &World, action: &str, mut stream: TcpStream) {
        std::fs::remove_file(w.lab.root.join(format!("gate-{action}"))).unwrap();
        stream.write_all(b"!").unwrap();
    }
    fn past_old_deadline(stream: &mut TcpStream) {
        // fixed wait: 예전 host 요청 15초를 실제로 넘긴 뒤에도 준비·정리가 계속됨을 검증하는 시간 규칙 자체다.
        // socket은 해제 사건을 기다리는 Git만 갖고 있으므로 이 read는 시간 창을 증명하고 별도 sleep이 필요 없다.
        stream.set_nonblocking(false).unwrap();
        let started = Instant::now();
        stream.set_read_timeout(Some(Duration::from_secs(16))).unwrap();
        let mut b = [0];
        let e = stream.read(&mut b).unwrap_err();
        assert!(started.elapsed() >= Duration::from_secs(15), "15초 경계를 실제로 넘어야 한다");
        assert!(matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut), "{e}");
    }
}
fn start_gated(w: &World) -> Value {
    w.start(&[
        "--background",
        "--title",
        "slow",
        "--profile",
        "fake",
        "--repo",
        path_str(&w.repo),
        "--task-file",
        &w.task("slow.txt", "noreport"),
    ])
}
fn restart_host(w: &mut World, tag: &str, next: &str) {
    w.lab.kill(tag);
    w.spawn_host(next);
    wait_for("새 host 연결", T, || w.h.connected("labA").then_some(()));
}

#[test]
fn slow_prepare_and_cleanup_cross_old_fifteen_second_deadline() {
    let gate = GitGate::new();
    let mut w = gate.world();
    GitGate::enable(&w, "add");
    let started = Instant::now();
    let v = start_gated(&w);
    assert!(started.elapsed() < Duration::from_secs(10), "배경 시작은 Git 해제를 기다리지 않는다");
    assert_eq!(v["status"], "starting", "{v}");
    let id = v["id"].as_str().unwrap().to_string();
    let mut pending = gate.take("add");
    GitGate::past_old_deadline(&mut pending);
    assert_eq!(w.show(&id)["status"], "starting");
    assert_eq!(w.show(&id)["group"], "working");
    GitGate::release(&w, "add", pending);
    let v = w.wait_show("준비·brief 완료", &id, |v| v["status"] == "open" && v["brief"] == "delivered");
    let cwd = PathBuf::from(v["cwd"].as_str().unwrap());
    GitGate::enable(&w, "remove");
    let (_, mut output) = w.lab.spawn_piped("resolve", &w.h, &["--json", "thread", "resolve", "demo", &id]);
    let mut pending = gate.take("remove");
    GitGate::past_old_deadline(&mut pending);
    assert!(w.show(&id)["error"].as_str().is_some_and(|s| s.contains("정리 중")));
    assert_eq!(w.show(&id)["cleanup_pending"], true);
    assert_eq!(w.show(&id)["group"], "working");
    GitGate::release(&w, "remove", pending);
    let result: Value = serde_json::from_str(&read_line_containing(&mut output, "resolved")).unwrap();
    assert_eq!(result["status"], "resolved", "{result}");
    assert!(!cwd.exists());
    assert!(w.lab.wait_exit("resolve", T).success());
    assert!(w.show(&id)["error"].is_null());
}

#[test]
fn interrupted_placement_and_resolve_resume_from_agent_records() {
    let gate = GitGate::new();
    let mut w = gate.world();
    GitGate::enable(&w, "add");
    let v = start_gated(&w);
    let id = v["id"].as_str().unwrap().to_string();
    let pending = gate.take("add");
    let cwd = w.lab.root.join("workA/repo/demo-t-0001");
    assert!(cwd.exists(), "응답 전에 worktree가 만들어졌다");
    restart_host(&mut w, "host", "host2");
    let (_, mut output) = w.lab.spawn_piped("restart", &w.h, &["--json", "thread", "restart", "demo", &id]);
    GitGate::release(&w, "add", pending);
    let result: Value = serde_json::from_str(&read_line_containing(&mut output, "open")).unwrap();
    assert_eq!(result["cwd"], path_str(&cwd), "{result}");
    assert!(w.lab.wait_exit("restart", T).success());
    w.wait_show("회수한 자리의 brief", &id, |v| v["brief"] == "delivered");
    let registered = git(&w.repo, &["worktree", "list", "--porcelain"]);
    assert_eq!(registered.matches(path_str(&cwd)).count(), 1, "중복 자리 생성 금지: {registered}");
    let receipt: Value =
        serde_json::from_slice(&std::fs::read(w.lab.root.join("workA/.preparations/demo-t-0001.json")).unwrap())
            .unwrap();
    assert_eq!(receipt["ready"]["cwd"], path_str(&cwd));

    GitGate::enable(&w, "remove");
    let (_, _old_output) = w.lab.spawn_piped("resolve-old", &w.h, &["--json", "thread", "resolve", "demo", &id]);
    let pending = gate.take("remove");
    assert!(!cwd.exists(), "Git 정리는 끝났지만 응답은 아직 안 왔다");
    restart_host(&mut w, "host2", "host3");
    let (_, mut output) = w.lab.spawn_piped("resolve-new", &w.h, &["--json", "thread", "resolve", "demo", &id]);
    GitGate::release(&w, "remove", pending);
    let result: Value = serde_json::from_str(&read_line_containing(&mut output, "resolved")).unwrap();
    assert!(result["error"].is_null(), "{result}");
    assert!(w.lab.wait_exit("resolve-new", T).success());
    let events = w.h.ok(&["events", "demo"]);
    assert_eq!(
        events["events"].as_array().unwrap().iter().filter(|e| e["kind"] == "thread_resolved").count(),
        1,
        "{events}"
    );
    let record: Value = serde_json::from_slice(
        &std::fs::read(
            std::fs::read_dir(w.h.state.join("agent/thread-cleanup")).unwrap().next().unwrap().unwrap().path(),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(record["result"].is_object(), "agent도 정리 결과를 기록했다: {record}");
}

/// 1초 host 설정이 실제 요청·Git deadline까지 전달된다. 응답을 잃은 완성 자리도 변경이 있으면 덮지 않는다.
#[test]
fn configured_prepare_timeout_recovers_only_unchanged_worktree() {
    let gate = GitGate::new();
    let mut w = gate.world();
    let config = w.h.cfg.join("host.toml");
    let text = std::fs::read_to_string(&config).unwrap().replace("[host]", "[host]\nthread_prepare_timeout_secs = 1");
    std::fs::write(config, text).unwrap();
    restart_host(&mut w, "host", "short-host");
    GitGate::enable(&w, "add");
    let v = start_gated(&w);
    let id = v["id"].as_str().unwrap();
    let mut pending = gate.take("add");
    let failed = w.wait_show("설정한 준비 상한", id, |v| v["status"] == "failed");
    assert!(failed["error"].as_str().unwrap().contains("시간 초과"), "{failed}");
    // kill_on_drop가 자기 Git 자식을 끝냈다. EOF 사건 뒤에만 같은 자리를 다시 본다.
    let mut b = [0];
    assert_eq!(pending.read(&mut b).unwrap(), 0);
    std::fs::remove_file(w.lab.root.join("gate-add")).unwrap();
    let cwd = w.lab.root.join("workA/repo/demo-t-0001");
    let tracked = cwd.join("README.md");
    let original = std::fs::read(&tracked).unwrap();
    std::fs::write(&tracked, "보존할 변경\n").unwrap();
    let error = w.h.json(&["thread", "restart", "demo", id]).expect_err("변경한 준비 자리는 덮지 않는다");
    assert!(error.contains("변경이 있다"), "{error}");
    assert_eq!(std::fs::read_to_string(&tracked).unwrap(), "보존할 변경\n");
    assert_eq!(w.show(id)["status"], "failed");
    std::fs::write(tracked, original).unwrap();
    let v = w.h.ok(&["thread", "restart", "demo", id]);
    assert_eq!(v["cwd"], path_str(&cwd));
    assert_eq!(v["status"], "open");
}

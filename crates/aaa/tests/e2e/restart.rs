//! 재시작 중인 agent 대신 별도 FIFO로 진행시키는 PTY harness.
//! 실제 pane의 입력·신호·크기·PID를 기록하므로 연결 상태만 보고 턴 보존으로 간주하지 않는다.

use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use serde_json::Value;

use super::{Lab, T, WITH_LINK, read_line_containing, wait_for};

const HARNESS: &str = r#"import json, os, selectors, signal, sys, tty
root = sys.argv[1]
tty.setraw(0)
os.mkfifo(root + '/control')
control = os.open(root + '/control', os.O_RDWR | os.O_NONBLOCK)
inputs, signals = [], []
working = True
def note(sig, frame):
    global working
    signals.append(sig)
    working = False
for sig in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP, signal.SIGWINCH):
    signal.signal(sig, note)
def checkpoint(stage):
    size = os.get_terminal_size(0)
    record = dict(stage=stage, pid=os.getpid(), inputs=inputs, signals=signals,
                  size=[size.columns, size.lines], working=working)
    with open(root + '/audit.part', 'w') as f:
        json.dump(record, f)
    os.replace(root + '/audit.part', root + '/audit.json')
    os.write(1, ('\r\n' + ('Working' if working else 'Idle') + ' ' + stage + '\r\n').encode())
sel = selectors.DefaultSelector()
sel.register(0, selectors.EVENT_READ)
sel.register(control, selectors.EVENT_READ)
checkpoint('ready')
pending = b''
while True:
    for key, _ in sel.select():
        if key.fd == 0:
            data = os.read(0, 4096)
            if not data:
                sys.exit(0)
            inputs.extend(data)
            if any(byte != 112 for byte in data):
                working = False
            checkpoint('input')
        else:
            pending += os.read(control, 4096)
            while b'\n' in pending:
                stage, pending = pending.split(b'\n', 1)
                checkpoint(stage.decode())
"#;

fn read_audit(root: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(root.join("audit.json")).ok()?).ok()
}

fn audit(root: &Path, stage: &str) -> Value {
    wait_for("harness checkpoint", T, || {
        let v = read_audit(root)?;
        (v["stage"] == stage).then_some(v)
    })
}

fn checkpoint(root: &Path, stage: &str, before: &Value) {
    // 자식이 사라졌으면 ENXIO로 실패해야 한다. reader 없는 FIFO open에서 시험이 멈추면 안 된다.
    let mut control =
        std::fs::OpenOptions::new().write(true).custom_flags(libc::O_NONBLOCK).open(root.join("control")).unwrap();
    writeln!(control, "{stage}").unwrap();
    let after = audit(root, stage);
    for field in ["pid", "inputs", "signals", "size", "working"] {
        assert_eq!(after[field], before[field], "{stage}: {field}: {after}");
    }
}

/// 로컬 UDS와 원격 TLS가 같은 PTY 보존 계약을 지킨다. host → 로컬 → 원격 순서로 SIGTERM 재시작한다.
/// agent가 없는 동안도 FIFO checkpoint로 실제 harness가 계속 일할 수 있음을 확인한다.
#[test]
fn restart_preserves_working_pty_without_input_signal_or_resize() {
    let mut lab = Lab::new();
    let h = lab.node("h", Some(WITH_LINK));
    let remote = lab.node("b", None);
    lab.spawn("host", &h, &["host"]);
    let status = wait_for("host 준비", T, || h.json(&["status"]).ok());
    let addr = status["agent_listen"].as_str().unwrap().to_string();
    // host도 같은 포트로 돌아오게 한다.
    let config = std::fs::read_to_string(h.cfg.join("host.toml")).unwrap();
    std::fs::write(h.cfg.join("host.toml"), config.replacen("127.0.0.1:0", &addr, 1)).unwrap();
    lab.spawn("local", &h, &["agent", "--host", "local", "--name", "labA"]);
    wait_for("로컬 연결", T, || h.connected("labA").then_some(()));
    h.ok(&["machine", "enroll", "--open", "5m"]);
    let (_, mut out) = lab.spawn_piped("enroll", &remote, &["agent", "enroll", &addr, "--name", "labB"]);
    let (code_tx, code_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = code_tx.send(read_line_containing(&mut out, "--code"));
        // 승인을 받은 뒤 CLI가 남은 안내를 쓴다. EOF까지 읽어 stdout을 먼저 닫지 않는다.
        let _ = std::io::copy(&mut out, &mut std::io::sink());
    });
    let line = code_rx.recv_timeout(T).expect("등록 코드 출력이 오지 않았다");
    let code = line.split("--code").nth(1).unwrap().trim();
    h.ok(&["machine", "approve", "labB", "--code", code]);
    assert!(lab.wait_exit("enroll", T).success());
    lab.spawn("remote", &remote, &["agent", "--host", &addr, "--name", "labB"]);
    wait_for("원격 연결", T, || h.connected("labB").then_some(()));
    let script = lab.root.join("working.py");
    std::fs::write(&script, HARNESS).unwrap();
    let mut panes: Vec<_> = [("labA", &h), ("labB", &remote)]
        .into_iter()
        .map(|(name, node)| {
            let root = node.cfg.join("harness");
            std::fs::create_dir(&root).unwrap();
            let id = h.new_pane(None, name, None, &["python3", script.to_str().unwrap(), root.to_str().unwrap()], None);
            let before = audit(&root, "ready");
            assert_eq!(before["working"], true);
            let host_pid = node.session_host_pid(&id).unwrap();
            (id, root, before, node, host_pid)
        })
        .collect();
    for (tag, node, target, name) in
        [("host", &h, "", ""), ("local", &h, "local", "labA"), ("remote", &remote, addr.as_str(), "labB")]
    {
        lab.signal(tag, libc::SIGTERM);
        assert!(lab.wait_exit(tag, T).success(), "{tag}: {}", lab.log(tag));
        for (_, root, before, _, _) in &panes {
            checkpoint(root, &format!("{tag}-down"), before);
        }
        if tag == "host" {
            lab.spawn(tag, node, &["host"]);
        } else {
            wait_for("agent 끊김", T, || (!h.connected(name)).then_some(()));
            lab.spawn(tag, node, &["agent", "--host", target, "--name", name]);
        }
        wait_for("두 agent 재연결", T, || (h.connected("labA") && h.connected("labB")).then_some(()));
        for (id, root, before, node, host_pid) in &mut panes {
            assert!(h.screen(id).contains("Working"));
            assert_eq!(node.session_host_pid(id), Some(*host_pid));
            assert_eq!(h.pane(id)["state"], "running");
            // PTY writer는 session 요청 처리와 비동기다. 정상 probe('p')를 실제로 읽은 뒤에 검사해야
            // 앞서 큐에 든 예상 밖 키까지 포함한다. Screen 응답만으로는 이 울타리가 되지 않는다.
            before["inputs"].as_array_mut().unwrap().push(serde_json::json!(112));
            let expected_len = before["inputs"].as_array().unwrap().len();
            h.ok(&["pane", "send-text", id, "p"]);
            wait_for("PTY probe 수신", T, || {
                let record = read_audit(root)?;
                (record["stage"] == "input" && record["inputs"].as_array().unwrap().len() >= expected_len).then_some(())
            });
            checkpoint(root, &format!("{tag}-back"), before);
        }
    }
    // 양성 대조군: 실제 Esc가 들어오면 같은 관측 경계에서 Idle과 정확한 바이트를 본다.
    for (id, root, before, _, _) in &panes {
        h.ok(&["pane", "send-keys", id, "escape"]);
        let interrupted = wait_for("Esc 수신", T, || {
            let record = read_audit(root)?;
            (record["stage"] == "input" && record["working"] == false).then_some(record)
        });
        assert_eq!(interrupted["working"], false);
        let mut expected = before["inputs"].as_array().unwrap().clone();
        expected.push(serde_json::json!(27));
        assert_eq!(interrupted["inputs"], serde_json::json!(expected));
    }
}

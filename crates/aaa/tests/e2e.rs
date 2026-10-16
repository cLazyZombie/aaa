//! M1-1 동작 E2E(Unix). 한 머신에 host와 agent backend 여러 개를 띄워 실제 실행 파일로 확인한다.
//! 실행: `cargo test -p aaa --test e2e`

#![cfg(unix)]

mod lab;

use std::time::Duration;

use lab::{Lab, Node, read_line_containing, wait_for};

const T: Duration = Duration::from_secs(20);

/// 원격을 받지 않고 로컬 agent는 시험이 직접 띄우는 host
const LOCAL_ONLY: &str = r#"schema = 1
[host]
agent_listen = ""
[web]
listen = ""
[local_agent]
mode = "none"
"#;

/// 원격 agent를 받는 host(포트는 OS가 고른다)
const WITH_LINK: &str = r#"schema = 1
[host]
agent_listen = "127.0.0.1:0"
[web]
listen = ""
[local_agent]
mode = "none"
"#;

fn connected(node: &Node, name: &str) -> bool {
    node.machine(name).is_some_and(|m| m["connection"] == "connected")
}

fn screen_has(node: &Node, pane: &str, needle: &str) -> Option<()> {
    node.json(&["pane", "read", pane]).ok().filter(|v| v.to_string().contains(needle)).map(|_| ())
}

/// host를 죽였다 살려도 세션이 남고, agent가 끊기면 읽기 전용이 되며, agent가 돌아오면 다시 입력된다.
#[test]
fn host_restart_keeps_sessions_and_disconnect_is_readonly() {
    let mut lab = Lab::new();
    let h = lab.node("h", Some(LOCAL_ONLY));
    lab.spawn("host", &h, &["host"]);
    wait_for("host 준비", T, || h.json(&["status"]).ok());
    lab.spawn("agent", &h, &["agent", "--host", "local", "--name", "labA"]);
    wait_for("labA 연결", T, || connected(&h, "labA").then_some(()));

    let p = h.ok(&["pane", "run", "--machine", "labA", "--name", "t1", "--", "/bin/sh"]);
    let id = p["id"].as_str().unwrap().to_string();
    h.ok(&["pane", "send", &id, "--text", "echo mark-$((6*7))", "--enter"]);
    wait_for("첫 출력", T, || screen_has(&h, &id, "mark-42"));

    // host 갑작스러운 종료 → 재시작: agent가 다시 붙고 같은 세션이 그대로 있다
    lab.kill("host");
    lab.spawn("host2", &h, &["host"]);
    wait_for("host 재시작 뒤 labA 재연결", Duration::from_secs(40), || connected(&h, "labA").then_some(()));
    assert_eq!(h.pane(&id)["state"], "running");
    wait_for("재시작 뒤 화면", T, || screen_has(&h, &id, "mark-42"));
    h.ok(&["pane", "send", &id, "--text", "echo after-$((1+1))", "--enter"]);
    wait_for("재시작 뒤 입력", T, || screen_has(&h, &id, "after-2"));

    // agent 종료 → pane은 「실행·끊김」 읽기 전용, 입력은 거부, 마지막 화면은 읽힌다
    lab.kill("agent");
    wait_for("끊김 표시", T, || {
        let p = h.pane(&id);
        (p["connection"] == "disconnected" && p["readonly"] == true).then_some(())
    });
    assert_eq!(h.pane(&id)["state"], "running", "끊긴 것은 종료가 아니다");
    let err = h.json(&["pane", "send", &id, "--text", "x"]).expect_err("끊긴 pane 입력은 실패해야 한다");
    assert!(err.contains("agent_disconnected"), "{err}");
    assert!(h.screen(&id).contains("after-2"), "끊긴 뒤에도 마지막 화면을 읽는다");

    // agent가 돌아오면 session host가 살아 있어 다시 붙는다
    lab.spawn("agent2", &h, &["agent", "--host", "local", "--name", "labA"]);
    wait_for("agent 재연결", T, || {
        let p = h.pane(&id);
        (p["connection"] == "connected" && p["readonly"] == false).then_some(())
    });
    h.ok(&["pane", "send", &id, "--text", "echo back-$((2+2))", "--enter"]);
    wait_for("재부착 뒤 입력", T, || screen_has(&h, &id, "back-4"));

    // 세션이 끝나면 종료 상태와 마지막 화면이 남는다
    h.ok(&["pane", "send", &id, "--text", "exit 3", "--enter"]);
    wait_for("종료 반영", T, || {
        let p = h.pane(&id);
        (p["state"] == "exited" && p["code"] == 3).then_some(())
    });
    assert!(h.screen(&id).contains("back-4"));
}

/// 원격 agent 등록(TLS 1.3 + 확인 코드)과 원격 pane. 틀린 코드는 거부되고 곧바로 끝나는 명령도 결과가 남는다.
#[test]
fn enroll_remote_agent_and_run_short_command() {
    let mut lab = Lab::new();
    let h = lab.node("h", Some(WITH_LINK));
    let b = lab.node("b", None);
    lab.spawn("host", &h, &["host"]);
    let st = wait_for("host 준비", T, || h.json(&["status"]).ok());
    let addr = st["agent_listen"].as_str().expect("agent_listen").to_string();

    // 등록 창이 닫혀 있으면 등록할 수 없다
    let closed = b.run(&["agent", "enroll", &addr, "--name", "labB"]);
    assert!(!closed.status.success(), "등록 창이 닫혀 있는데 등록됐다");

    h.ok(&["machine", "enroll", "--open", "5m"]);
    let (_, mut out) = lab.spawn_piped("enroll", &b, &["agent", "enroll", &addr, "--name", "labB"]);
    let line = read_line_containing(&mut out, "--code");
    let code = line.split("--code").nth(1).unwrap().trim().to_string();
    assert_eq!(code.len(), 6, "{line}");
    let pending = h.ok(&["machine", "pending"]);
    assert_eq!(pending["pending"][0]["name"], "labB", "{pending}");

    let wrong = if code == "000000" { "111111" } else { "000000" };
    let e = h.json(&["machine", "approve", "labB", "--code", wrong]).expect_err("틀린 코드가 승인됐다");
    assert!(e.contains("enroll_code_mismatch"), "{e}");
    h.ok(&["machine", "approve", "labB", "--code", &code]);
    assert!(lab.wait_exit("enroll", T).success(), "enroll 실패: {}", lab.log("enroll"));

    lab.spawn("agentB", &b, &["agent", "--host", &addr, "--name", "labB"]);
    wait_for("labB 연결", T, || connected(&h, "labB").then_some(()));

    // 곧바로 끝나는 명령도 종료 코드와 출력이 남는다
    let p = h.ok(&["pane", "run", "--machine", "labB", "--", "/bin/sh", "-c", "echo remote-$((2+3)); exit 7"]);
    let id = p["id"].as_str().unwrap().to_string();
    wait_for("짧은 명령 종료", T, || {
        let p = h.pane(&id);
        (p["state"] == "exited" && p["code"] == 7).then_some(())
    });
    assert!(h.screen(&id).contains("remote-5"), "{}", h.screen(&id));

    // 원격 셸에 입력
    let p = h.ok(&["pane", "run", "--machine", "labB", "--name", "rsh", "--", "/bin/sh"]);
    let rid = p["id"].as_str().unwrap().to_string();
    h.ok(&["pane", "send", "rsh", "--text", "echo 원격-$((3*3))", "--enter"]);
    wait_for("원격 입력", T, || screen_has(&h, &rid, "원격-9"));

    // 등록을 취소하면 그 장치는 다시 붙지 못한다
    h.ok(&["machine", "revoke", "labB"]);
    wait_for("취소 뒤 끊김", T, || (!connected(&h, "labB")).then_some(()));
}

//! 모든 OS(macOS·Linux·Windows)에서 도는 PTY·Agent link smoke. CI가 실행한다.
//! - 로컬 agent(UDS 또는 Named Pipe)로 대화형 셸 입력·크기 바꾸기·종료, 곧바로 끝나는 명령의 종료 코드와 출력
//! - 원격 agent 등록(TCP+TLS 1.3, 확인 코드)과 원격 pane

mod lab;

use std::time::Duration;

use lab::{Lab, Node, read_line_containing, tree, wait_for, wait_for_diag};

const T: Duration = Duration::from_secs(30);

fn host_toml(link: bool) -> String {
    let listen = if link { "127.0.0.1:0" } else { "" };
    format!("schema = 1\n[host]\nagent_listen = \"{listen}\"\n[web]\nlisten = \"\"\n[local_agent]\nmode = \"none\"\n")
}

/// (곧바로 끝나며 출력 뒤 종료 코드 5를 내는 명령, 대화형 셸, 셸에 보낼 계산식). 계산 결과 1042는 입력 echo에 없다.
fn commands() -> (Vec<&'static str>, Vec<&'static str>, &'static str) {
    if cfg!(windows) {
        (vec!["cmd.exe", "/d", "/c", "echo smoke-ok& exit /b 5"], vec!["cmd.exe", "/d"], "set /a 6*7+1000")
    } else {
        (vec!["/bin/sh", "-c", "echo smoke-ok; exit 5"], vec!["/bin/sh"], "echo $((6*7+1000))")
    }
}

fn connected(node: &Node, name: &str) -> bool {
    node.machine(name).is_some_and(|m| m["connection"] == "connected")
}

fn run(node: &Node, machine: &str, name: &str, argv: &[&str]) -> String {
    let mut args = vec!["pane", "run", "--machine", machine, "--name", name, "--"];
    args.extend_from_slice(argv);
    node.ok(&args)["id"].as_str().unwrap().to_string()
}

fn diag(h: &Node, id: &str, root: &std::path::Path) -> String {
    let screen = h.json(&["pane", "read", id]).map(|v| v.to_string()).unwrap_or_else(|e| e);
    format!("pane: {}\nscreen: {screen}\nfiles:\n{}", h.pane(id), tree(root))
}

fn check_panes(lab: &Lab, h: &Node, machine: &str) {
    let (short, shell, calc) = commands();

    let id = run(h, machine, "short", &short);
    wait_for_diag(
        "짧은 명령 종료",
        T,
        || {
            let p = h.pane(&id);
            (p["state"] == "exited").then_some(())
        },
        || diag(h, &id, &lab.root),
    );
    assert_eq!(h.pane(&id)["code"], 5, "{}", h.pane(&id));
    assert!(h.screen(&id).contains("smoke-ok"), "짧은 명령 출력이 없다:\n{}", h.screen(&id));

    let sh = run(h, machine, "sh", &shell);
    h.ok(&["pane", "send", &sh, "--text", calc, "--enter"]);
    wait_for_diag("대화형 입력 결과", T, || h.screen(&sh).contains("1042").then_some(()), || diag(h, &sh, &lab.root));
    h.ok(&["pane", "resize", &sh, "100", "30"]);
    wait_for("크기 반영", T, || {
        let p = h.pane(&sh);
        (p["cols"] == 100 && p["rows"] == 30).then_some(())
    });
    h.ok(&["pane", "kill", &sh]);
    wait_for("kill 뒤 종료", T, || (h.pane(&sh)["state"] == "exited").then_some(()));
}

#[test]
fn pty_smoke_local_agent() {
    let mut lab = Lab::new();
    let h = lab.node("h", Some(&host_toml(false)));
    lab.spawn("host", &h, &["host"]);
    wait_for("host 준비", T, || h.json(&["status"]).ok());
    lab.spawn("agent", &h, &["agent", "--host", "local", "--name", "local1"]);
    wait_for("로컬 agent 연결", T, || connected(&h, "local1").then_some(()));
    check_panes(&lab, &h, "local1");
}

#[test]
fn pty_smoke_remote_agent_over_tls() {
    let mut lab = Lab::new();
    let h = lab.node("h", Some(&host_toml(true)));
    let b = lab.node("b", None);
    lab.spawn("host", &h, &["host"]);
    let st = wait_for("host 준비", T, || h.json(&["status"]).ok());
    let addr = st["agent_listen"].as_str().expect("agent_listen").to_string();

    h.ok(&["machine", "enroll", "--open", "5m"]);
    let (_, mut out) = lab.spawn_piped("enroll", &b, &["agent", "enroll", &addr, "--name", "remote1"]);
    let line = read_line_containing(&mut out, "--code");
    let code = line.split("--code").nth(1).unwrap().trim().to_string();
    h.ok(&["machine", "approve", "remote1", "--code", &code]);
    assert!(lab.wait_exit("enroll", T).success(), "enroll 실패: {}", lab.log("enroll"));

    lab.spawn("agent", &b, &["agent", "--host", &addr, "--name", "remote1"]);
    wait_for("원격 agent 연결", T, || connected(&h, "remote1").then_some(()));
    check_panes(&lab, &h, "remote1");
}

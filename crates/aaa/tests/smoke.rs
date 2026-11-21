//! 모든 OS(macOS·Linux·Windows)에서 도는 PTY·Agent link smoke. CI가 실행한다.
//! - 로컬 agent(UDS 또는 Named Pipe)로 대화형 셸 입력·크기 바꾸기·종료, 곧바로 끝나는 명령의 종료 코드와 출력
//! - 원격 agent 등록(TCP+TLS 1.3, 확인 코드)과 원격 pane

mod lab;

use std::time::Duration;

use lab::{Lab, Node, read_line_containing, tree, wait_for, wait_for_diag};

const T: Duration = Duration::from_secs(30);

#[cfg(unix)]
#[test]
fn agy_config_and_profiles_use_a_person_terminal_and_the_model_catalog() {
    use aaa_common::child_process::StdCommandExt as _;
    use sha2::{Digest as _, Sha256};
    use std::fs;
    use std::os::unix::fs::PermissionsExt as _;
    use std::path::Path;

    let lab = Lab::new();
    let node = lab.node("gemini", Some("schema = 1\n# host profile\n"));
    let agent_file = node.cfg.join("agent.toml");
    let host_file = node.cfg.join("host.toml");
    let own_agent = "schema = 1\n# 내 설정\n[health]\nmin_free_gb = 5\n";
    fs::write(&agent_file, own_agent).unwrap();
    let agy = lab.root.join("agy");
    let script = lab.root.join("agy-source");
    fs::write(
        &script,
        "#!/bin/sh\n[ \"$1\" = models ] || exit 2\nprintf 'sample-low\\tSample Low\\nsample-high\\tSample High\\n'\n",
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(std::process::Command::new("cp").args([&script, &agy]).status_guarded().unwrap().success());
    let setup = ["config", "agy", "--command", agy.to_str().unwrap(), "--subscription", "gemini-personal"];
    let terminal = |args: &[&str], answers: &[(&str, &str)]| {
        let mut command = lab::terminal_command(Path::new(lab::BIN));
        command.args(args);
        command.env("AAA_CONFIG_DIR", &node.cfg);
        command.env("AAA_STATE_DIR", &node.state);
        command.env("TERM", "xterm-256color");
        command.env_remove("AAA_SESSION_ID");
        command.env_remove("AAA_HOOK_SOCK");
        lab::Terminal::spawn(command).read_raw(args, answers, T)
    };
    let denied = node.cmd(&setup).env_remove("AAA_SESSION_ID").env_remove("AAA_HOOK_SOCK").capture_output().unwrap();
    assert!(!denied.status.success());
    assert!(String::from_utf8_lossy(&denied.stderr).contains("표준 입력이 터미널이 아니다"));
    assert_eq!(fs::read_to_string(&agent_file).unwrap(), own_agent);

    let login = " Operator@example.com ";
    let output = terminal(&setup, &[("agy Google 로그인 주소", &format!("{login}\r"))]);
    assert!(output.contains("agy 머신 설정을 저장했다"), "{output}");
    let saved = fs::read_to_string(&agent_file).unwrap();
    let cfg = aaa_config::load_agent(&node.cfg).unwrap().config;
    let account = cfg.agy.unwrap();
    assert_eq!(account.subscription, "gemini-personal");
    assert_eq!(account.login_sha256, hex::encode(Sha256::digest(b"operator@example.com")));
    assert_eq!(cfg.harnesses["agy"].command, [agy.to_str().unwrap()]);
    assert!(saved.starts_with(own_agent));
    assert!(!saved.contains(login.trim()));
    assert!(!output.contains(login.trim()) && !output.contains(&account.login_sha256));

    let mut overwrite = setup;
    overwrite[5] = "gemini-next";
    let output = terminal(&overwrite, &[("기존 agy 설정을 바꿀까요?", "\r")]);
    assert!(output.contains("현재 구독 별칭") && output.contains("gemini-personal"), "{output}");
    assert!(output.contains("새 구독 별칭") && output.contains("gemini-next"), "{output}");
    assert!(!output.contains("agy Google 로그인 주소") && !output.contains(&account.login_sha256));
    assert_eq!(fs::read_to_string(&agent_file).unwrap(), saved);
    let output = terminal(
        &overwrite,
        &[("기존 agy 설정을 바꿀까요?", "y\r"), ("agy Google 로그인 주소", &format!("{login}\r"))],
    );
    assert!(output.contains("agy 머신 설정을 저장했다") && !output.contains(login.trim()), "{output}");
    assert_eq!(aaa_config::load_agent(&node.cfg).unwrap().config.agy.unwrap().subscription, "gemini-next");

    let mut profile = ["profile", "add", "gemini", "--agent", "agy", "--model", "sample-low", "--effort", "low"];
    let before = fs::read_to_string(&host_file).unwrap();
    let denied = node.cmd(&profile).env("AAA_SESSION_ID", "session-example").capture_output().unwrap();
    assert!(!denied.status.success());
    assert!(String::from_utf8_lossy(&denied.stderr).contains("aaa 세션 안에서는"));
    assert_eq!(fs::read_to_string(&host_file).unwrap(), before);
    let output = terminal(&profile, &[]);
    assert!(output.contains("profile `gemini`를 더했다"), "{output}");
    let mut saved = fs::read_to_string(&host_file).unwrap();
    assert!(saved.contains("agent = \"agy\"") && saved.contains("model = \"sample-low\""));
    profile[1] = "edit";
    profile[8] = "high";
    let output = terminal(&profile, &[]);
    assert!(output.contains("지정한 모델과 effort가 없다"), "{output}");
    assert_eq!(fs::read_to_string(&host_file).unwrap(), saved);
    profile[6] = "sample-high";
    let output = terminal(&profile, &[]);
    assert!(output.contains("profile `gemini`를 고쳤다"), "{output}");
    saved = fs::read_to_string(&host_file).unwrap();
    assert!(saved.contains("model = \"sample-high\"") && saved.contains("effort = \"high\""));
}

fn host_toml(link: bool) -> String {
    let listen = if link { "127.0.0.1:0" } else { "" };
    format!("schema = 1\n[host]\nagent_listen = \"{listen}\"\n[web]\nlisten = \"\"\n[local_agent]\nmode = \"none\"\n")
}

/// (곧바로 끝나며 출력 뒤 종료 코드 5를 내는 명령, 대화형 셸, 셸에 보낼 계산식). 계산 결과 1042는 입력 echo에 없다.
fn commands() -> (Vec<&'static str>, Vec<&'static str>, &'static str) {
    if cfg!(windows) {
        (vec!["cmd.exe", "/d", "/c", "echo smoke-ok& exit 5"], vec!["cmd.exe", "/d"], "set /a 6*7+1000")
    } else {
        (vec!["/bin/sh", "-c", "echo smoke-ok; exit 5"], vec!["/bin/sh"], "echo $((6*7+1000))")
    }
}

fn run(node: &Node, machine: &str, name: &str, argv: &[&str]) -> String {
    node.new_pane(None, machine, Some(name), argv, None)
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
    h.ok(&["pane", "run", &sh, calc]);
    h.wait_output(&sh, "1042", T);
    h.ok(&["pane", "pty-size", &sh, "100", "30"]);
    wait_for("크기 반영", T, || {
        let p = h.pane(&sh);
        (p["cols"] == 100 && p["rows"] == 30).then_some(())
    });
    h.ok(&["pane", "kill", &sh]);
    wait_for("kill 뒤 종료", T, || (h.pane(&sh)["state"] == "exited").then_some(()));

    // 커서 위치 질의(DSR)에 session host가 답한다(Windows ConPTY는 이 답이 없으면 시작하지 못한다)
    if cfg!(unix) {
        let q = run(
            h,
            machine,
            "dsr",
            &["/bin/bash", "-c", r#"printf 'ab\033[6n'; IFS= read -rs -d R -t 5 r; echo "dsr=${r#*[}""#],
        );
        wait_for_diag("DSR 답", T, || h.screen(&q).contains("dsr=1;3").then_some(()), || diag(h, &q, &lab.root));
    }
}

#[test]
fn pty_smoke_local_agent() {
    let mut lab = Lab::new();
    let h = lab.node("h", Some(&host_toml(false)));
    lab.spawn("host", &h, &["host"]);
    wait_for("host 준비", T, || h.json(&["status"]).ok());
    lab.spawn("agent", &h, &["agent", "--host", "local", "--name", "local1"]);
    wait_for("로컬 agent 연결", T, || h.connected("local1").then_some(()));
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
    wait_for("원격 agent 연결", T, || h.connected("remote1").then_some(()));
    check_panes(&lab, &h, "remote1");
}

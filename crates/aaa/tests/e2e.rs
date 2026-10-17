//! M1-1 동작 E2E(Unix). 한 머신에 host와 agent backend 여러 개를 띄워 실제 실행 파일로 확인한다.
//! 실행: `cargo test -p aaa --test e2e`

#![cfg(unix)]

mod lab;

use std::time::Duration;

use lab::{Lab, read_line_containing, wait_for};

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

/// 쉬고 있는 가짜 Claude: SessionStart hook(대화 기록 위치)과 진행 보고를 내고 입력 상자를 그린 채 기다린다.
/// `$1`은 aaa 실행 파일, `$2`는 hook JSON이다.
const IDLE_CLAUDE: &str = r#"#!/bin/sh
"$1" hook --agent claude "$2"
"$1" report --percent 40 --activity 정리
printf '────────────────\n\342\235\257 \n────────────────\n  ? for shortcuts'
exec cat
"#;

/// host를 죽였다 살려도 세션이 남고, agent가 끊기면 읽기 전용이 되며, agent가 돌아오면 다시 입력된다.
/// host와 agent를 함께 다시 띄워도 harness 상태·진행·대화 기록 위치가 되살아난다(다음 hook을 기다리지 않는다).
#[test]
fn host_restart_keeps_sessions_and_disconnect_is_readonly() {
    let mut lab = Lab::new();
    let h = lab.node("h", Some(LOCAL_ONLY));
    lab.spawn("host", &h, &["host"]);
    wait_for("host 준비", T, || h.json(&["status"]).ok());
    let claude = lab.root.join("claude");
    let claude_s = claude.to_str().unwrap().to_string();
    lab.spawn_env("agent", &h, &["agent", "--host", "local", "--name", "labA"], &[("CLAUDE_CONFIG_DIR", &claude_s)]);
    wait_for("labA 연결", T, || h.connected("labA").then_some(()));

    let id = h.new_pane(None, "labA", Some("t1"), &["/bin/sh"], None);
    h.ok(&["pane", "run", &id, "echo mark-$((6*7))"]);
    h.wait_output(&id, "mark-42", T);

    // 쉬고 있는 Claude pane(SessionStart hook과 진행 보고 뒤 입력 상자에서 기다린다): 다시 떠도 상태·진행·대화 보기가
    // 남아야 한다. 상태는 화면(입력 상자, herdr claude.toml `live_prompt_box`)으로 안다.
    let dir = claude.join("projects").join("-lab");
    std::fs::create_dir_all(&dir).unwrap();
    let tr = dir.join("s1.jsonl");
    std::fs::write(
        &tr,
        concat!(
            r#"{"type":"user","message":{"role":"user","content":"이름 뜻을 적어 줘"}}"#,
            "\n",
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"적었다"}]}}"#,
            "\n"
        ),
    )
    .unwrap();
    let hook =
        serde_json::json!({ "hook_event_name": "SessionStart", "session_id": "s1", "transcript_path": tr }).to_string();
    // 실행 파일 이름이 claude라 agent가 Claude 화면 규칙으로 본다
    let fake = lab.root.join("bin").join("claude");
    std::fs::create_dir_all(fake.parent().unwrap()).unwrap();
    std::fs::write(&fake, IDLE_CLAUDE).unwrap();
    std::fs::set_permissions(&fake, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let mut ev = lab::Events::connect(&h);
    let cid = h.new_pane(None, "labA", Some("c1"), &[fake.to_str().unwrap(), lab::BIN, &hook], None);
    let chat_len = || h.json(&["pane", "chat", &cid]).ok().and_then(|v| v["messages"].as_array().map(|m| m.len()));
    ev.wait("쉬는 Claude 상태·진행", T, |e| {
        match e {
            aaa_proto::FrontEvent::State { panes, .. } => panes
                .iter()
                .find(|p| p.id == cid)
                .and_then(|p| p.agent.as_ref())
                .filter(|a| a.activity == aaa_proto::AgentActivity::Idle && a.percent == Some(40)),
            _ => None,
        }
        .map(|_| ())
    });
    assert_eq!(chat_len(), Some(2));
    let before = h.pane(&cid)["agent"].clone();
    assert_eq!((before["harness"].as_str(), before["activity"].as_str()), (Some("claude"), Some("idle")), "{before}");
    let restored = |what: &str| {
        assert_eq!(h.pane(&cid)["agent"], before, "{what}: harness 상태가 되살아나지 않았다");
        assert_eq!(chat_len(), Some(2), "{what}: 대화 기록 위치가 되살아나지 않았다");
    };

    // host 갑작스러운 종료 → 재시작: agent가 다시 붙고 같은 세션이 그대로 있다
    lab.kill("host");
    lab.spawn("host2", &h, &["host"]);
    wait_for("host 재시작 뒤 labA 재연결", Duration::from_secs(40), || h.connected("labA").then_some(()));
    assert_eq!(h.pane(&id)["state"], "running");
    // 새 hook 없이 같은 harness 상태(때 포함)와 대화 보기(agent 메모리에서 다시 알린다)
    wait_for("host 재시작 뒤 harness 상태", T, || (h.pane(&cid)["agent"] == before).then_some(()));
    restored("host만 다시 띄움");
    h.wait_output(&id, "mark-42", T);
    h.ok(&["pane", "run", &id, "echo after-$((1+1))"]);
    h.wait_output(&id, "after-2", T);

    // agent 종료 → pane은 「실행·끊김」 읽기 전용, 입력은 거부, 마지막 화면은 읽힌다
    lab.kill("agent");
    wait_for("끊김 표시", T, || {
        let p = h.pane(&id);
        (p["connection"] == "disconnected" && p["readonly"] == true).then_some(())
    });
    assert_eq!(h.pane(&id)["state"], "running", "끊긴 것은 종료가 아니다");
    let err = h.json(&["pane", "send-text", &id, "x"]).expect_err("끊긴 pane 입력은 실패해야 한다");
    assert!(err.contains("agent_disconnected"), "{err}");
    assert!(h.screen(&id).contains("after-2"), "끊긴 뒤에도 마지막 화면을 읽는다");

    // agent가 내려간 동안 host도 다시 띄운다(운영 업그레이드처럼 둘 다 메모리가 빈다)
    lab.kill("host2");
    lab.spawn("host3", &h, &["host"]);
    wait_for("host3 준비", T, || h.json(&["status"]).ok());

    // agent가 돌아오면 session host가 살아 있어 다시 붙는다(새 host는 agent가 붙어야 pane을 안다)
    lab.spawn_env("agent2", &h, &["agent", "--host", "local", "--name", "labA"], &[("CLAUDE_CONFIG_DIR", &claude_s)]);
    wait_for("agent 재연결", T, || {
        let v = h.json(&["pane", "list"]).ok()?;
        let p = v["panes"].as_array()?.iter().find(|p| p["id"] == id.as_str())?.clone();
        (p["connection"] == "connected" && p["readonly"] == false).then_some(())
    });
    // agent가 세션 폴더(harness.json)에서 읽어 다시 알린다
    wait_for("둘 다 다시 띄운 뒤 harness 상태", T, || (h.pane(&cid)["agent"] == before).then_some(()));
    restored("host·agent 함께 다시 띄움");
    h.ok(&["pane", "run", &id, "echo back-$((2+2))"]);
    h.wait_output(&id, "back-4", T);

    // 세션이 끝나면 종료 상태와 마지막 화면이 남는다
    h.ok(&["pane", "run", &id, "exit 3"]);
    wait_for("종료 반영", T, || {
        let p = h.pane(&id);
        (p["state"] == "exited" && p["code"] == 3).then_some(())
    });
    assert!(h.screen(&id).contains("back-4"));
}

/// 운영 pane(shadow/t-0002)을 읽은 화면: 턴이 끝난 뒤 보내지 않은 글이 입력 상자에 남아 있다
const T0002_SCREEN: &str = include_str!("../../aaa-agent/src/detect/fixtures/claude-idle-typed.txt");

/// pane `id`가 `from` 뒤에 idle이 됐다는 상태 사건을 기다린다
fn idle_after(ev: &mut lab::Events, what: &str, id: &str, from: u64) {
    ev.wait(what, T, |e| match e {
        aaa_proto::FrontEvent::State { panes, .. } => panes
            .iter()
            .find(|p| p.id == id)
            .and_then(|p| p.agent.as_ref())
            .filter(|a| a.activity == aaa_proto::AgentActivity::Idle && a.since_ms >= from)
            .map(|_| ()),
        _ => None,
    });
}

/// 옛 agent가 남긴 「작업 중」을 새 agent가 화면으로 바로잡는다(t-0334 결함 1, plan §8.95).
/// 운영 shadow/t-0002는 보낸 글이 답 없이 입력 상자로 돌아와(응답 전 interrupt로 보인다. Claude는 사용자 interrupt에
/// Stop hook을 내지 않는다) UserPromptSubmit의 `turn_start`만 남아 10시간 넘게 작업 중이었다. 그 화면과 harness.json을
/// 그대로 두고 binary를 바꿀 때처럼 host와 agent를 함께 다시 띄운다.
#[test]
fn stale_working_left_by_hooks_turns_idle_from_the_screen() {
    let mut lab = Lab::new();
    let h = lab.node("h", Some(LOCAL_ONLY));
    lab.spawn("host", &h, &["host"]);
    wait_for("host 준비", T, || h.json(&["status"]).ok());
    lab.spawn("agent", &h, &["agent", "--host", "local", "--name", "labA"]);
    wait_for("labA 연결", T, || h.connected("labA").then_some(()));

    // 실행 파일 이름이 claude인 가짜가 운영 pane 화면을 그린 채 기다린다(같은 66×42)
    let screen = lab.root.join("t-0002.txt");
    std::fs::write(&screen, T0002_SCREEN).unwrap();
    let fake = lab.root.join("bin").join("claude");
    std::fs::create_dir_all(fake.parent().unwrap()).unwrap();
    std::fs::write(&fake, "#!/bin/sh\ncat \"$1\"\nexec cat\n").unwrap();
    std::fs::set_permissions(&fake, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let mut ev = lab::Events::connect(&h);
    let argv = [fake.to_str().unwrap(), screen.to_str().unwrap()];
    let w = h.ok(&[&["tab", "create", "--machine", "labA", "--cols", "66", "--rows", "42", "--"][..], &argv].concat());
    let id = w["tabs"].as_array().and_then(|t| t.last()).unwrap()["root"]["pane"].as_str().unwrap().to_string();
    idle_after(&mut ev, "운영 pane 화면으로 idle", &id, 0);

    // 옛 agent가 둔 것처럼: 마지막 사건이 `turn_start`이고 턴 끝은 오지 않았다. 새 host는 이 기억만 듣는다.
    lab.kill("agent");
    lab.kill("host");
    let file = h.state.join("agent").join("sessions").join(&id).join("harness.json");
    let old = serde_json::json!([{
        "session": id, "harness": "claude", "kind": "turn_start", "harness_session": "s1",
        "message": null, "question_kind": null, "percent": null, "at_ms": lab::now_ms() - 10 * 3600 * 1000,
    }]);
    std::fs::write(&file, old.to_string()).unwrap();
    lab.spawn("host2", &h, &["host"]);
    wait_for("host2 준비", T, || h.json(&["status"]).ok());
    let mut ev = lab::Events::connect(&h);
    let t1 = lab::now_ms();
    lab.spawn("agent2", &h, &["agent", "--host", "local", "--name", "labA"]);
    idle_after(&mut ev, "새 agent가 화면으로 idle", &id, t1);
    let a = h.pane(&id)["agent"].clone();
    assert_eq!((a["harness"].as_str(), a["activity"].as_str()), (Some("claude"), Some("idle")), "{a}");
    // 바로잡은 상태를 기억한다: 다시 떠도 옛 「작업 중」이 돌아오지 않는다
    let kept = std::fs::read_to_string(&file).unwrap();
    assert!(kept.contains("\"idle\"") && !kept.contains("turn_start"), "{kept}");
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
    wait_for("labB 연결", T, || h.connected("labB").then_some(()));

    // 곧바로 끝나는 명령도 종료 코드와 출력이 남는다
    let id = h.new_pane(None, "labB", None, &["/bin/sh", "-c", "echo remote-$((2+3)); exit 7"], None);
    wait_for("짧은 명령 종료", T, || {
        let p = h.pane(&id);
        (p["state"] == "exited" && p["code"] == 7).then_some(())
    });
    assert!(h.screen(&id).contains("remote-5"), "{}", h.screen(&id));
    // 종료가 보이자마자 지울 수 있다(session host가 종료 뒤 2초 더 응답하는 동안에도)
    let qid = h.new_pane(None, "labB", None, &["/bin/sh", "-c", "exit 0"], None);
    wait_for("곧바로 끝남", T, || (h.pane(&qid)["state"] == "exited").then_some(()));
    h.ok(&["pane", "forget", &qid]);
    std::thread::sleep(Duration::from_millis(2500));
    assert!(!h.ok(&["pane", "list"]).to_string().contains(&qid), "지운 pane이 되살아났다");

    // 원격 셸에 입력
    let rid = h.new_pane(None, "labB", Some("rsh"), &["/bin/sh"], None);
    h.ok(&["pane", "run", "rsh", "echo 원격-$((3*3))"]);
    h.wait_output(&rid, "원격-9", T);

    // 등록을 취소하면 그 장치는 다시 붙지 못한다
    h.ok(&["machine", "revoke", "labB"]);
    wait_for("취소 뒤 끊김", T, || (!h.connected("labB")).then_some(()));
}

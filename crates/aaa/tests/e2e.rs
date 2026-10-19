//! M1-1 동작 E2E(Unix). 한 머신에 host와 agent backend 여러 개를 띄워 실제 실행 파일로 확인한다.
//! 실행: `cargo test -p aaa --test e2e`

#![cfg(unix)]

mod lab;

use std::time::Duration;

use lab::watch::HostWatch;
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
        serde_json::json!({ "hook_event_name": "SessionStart", "session_id": "3c2b1a09-8f7e-4d6c-9b5a-4f3e2d1c0b9a", "transcript_path": tr }).to_string();
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
    // thread가 아닌 pane의 Claude hook에는 진행 안내를 넣지 않는다(aaa가 hook 설정을 주는 세션만, herdr-projects
    // `managed`). 이 가짜는 hook 출력을 화면에 낸다
    let read = h.ok(&["pane", "read", &cid, "--lines", "1000"]);
    assert!(!read["text"].as_str().unwrap().contains("additionalContext"), "{read}");
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

    // 셸이 스스로 끝나면 그 pane을 닫는다(plan §8.143): host를 다시 띄운 뒤 다시 붙어 실행 중을 본 셸도 그렇다.
    // 프로그램 pane의 종료 코드·마지막 화면은 remote 시험이 본다
    h.ok(&["pane", "run", &id, "exit 3"]);
    wait_for("닫힘", T, || (!h.ok(&["pane", "list"]).to_string().contains(&id)).then_some(()));
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

/// 옛 agent가 남긴 「작업 중」을 새 agent가 화면으로 바로잡는다(t-0334 결함 1, plan §8.110).
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
        "session": id, "harness": "claude", "kind": "turn_start", "harness_session": "3c2b1a09-8f7e-4d6c-9b5a-4f3e2d1c0b9a",
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

/// pane 안 Codex가 낸 것처럼 notify 사건 셋을 보내고, 하나 보낼 때마다 진행 보고로 끝을 알린다. `$1`은 aaa 실행 파일이다.
/// 둘째는 그 Codex 안에서 돈 중첩 thread다(물려받은 `CODEX_THREAD_ID`가 사건의 thread-id와 다르다).
/// 진행 보고 뒤에는 Enter를 기다린다: 시험이 그 단계를 본 뒤에 다음 사건을 보낸다.
const NESTED_CODEX: &str = r#"#!/bin/sh
A=0a0a0a0a-0000-4000-8000-00000000000a
B=0b0b0b0b-0000-4000-8000-00000000000b
C=0c0c0c0c-0000-4000-8000-00000000000c
"$1" hook --agent codex "{\"type\":\"agent-turn-complete\",\"thread-id\":\"$A\"}"
"$1" report --percent 10 --activity 하나
read _
CODEX_THREAD_ID=$A "$1" hook --agent codex "{\"type\":\"agent-turn-complete\",\"thread-id\":\"$B\"}"
"$1" report --percent 20 --activity 둘
read _
CODEX_THREAD_ID=$C "$1" hook --agent codex "{\"type\":\"agent-turn-complete\",\"thread-id\":\"$C\"}"
"$1" report --percent 30 --activity 셋
exec cat
"#;

/// Codex 중첩 thread의 사건은 pane의 harness 세션 ID를 바꾸지 않는다(herdr `herdr-agent-state.sh`, herdr-projects
/// progress.rs `hook`: 물려받은 `CODEX_THREAD_ID`와 다른 세션의 사건은 버린다). 바깥 Codex 안에서 띄운 agent의
/// `CODEX_THREAD_ID`는 세션에 넘기지 않는다(herdr `apply_pane_launch_env`). 넘기면 pane 안 Codex 사건이 모두 버려진다.
#[test]
fn nested_codex_events_do_not_replace_the_pane_session() {
    let mut lab = Lab::new();
    let h = lab.node("h", Some(LOCAL_ONLY));
    lab.spawn("host", &h, &["host"]);
    wait_for("host 준비", T, || h.json(&["status"]).ok());
    let outer = "0e0e0e0e-0000-4000-8000-00000000000e";
    lab.spawn_env("agent", &h, &["agent", "--host", "local", "--name", "labA"], &[("CODEX_THREAD_ID", outer)]);
    wait_for("labA 연결", T, || h.connected("labA").then_some(()));
    let script = lab.root.join("nested-codex.sh");
    std::fs::write(&script, NESTED_CODEX).unwrap();
    let mut ev = lab::Events::connect(&h);
    let id = h.new_pane(None, "labA", Some("x1"), &["/bin/sh", script.to_str().unwrap(), lab::BIN], None);
    // hook은 agent가 host에 보낸 뒤에 답하므로 사건은 보낸 차례로 온다: 그 단계의 진행 보고가 실린 상태 사건의 세션 ID를 본다.
    // host는 상태 사건을 그때의 최신 하나로 합쳐 보낸다(front.rs `state_task`, watch). 스크립트가 쉬지 않고 셋을 보내면
    // 빠른 머신에서는 한 단계의 상태 사건이 오지 않거나 다음 단계의 세션이 실린다(GitHub CI ubuntu 287c1d1은 세션 시작부터
    // 셋째 보고까지 35ms). 그래서 단계를 본 뒤에 Enter로 다음 단계를 보낸다
    let mut session_at = |step: &str| {
        ev.wait(step, T, |e| match e {
            aaa_proto::FrontEvent::State { panes, .. } => panes
                .iter()
                .find(|p| p.id == id)
                .and_then(|p| p.agent.as_ref())
                .filter(|a| a.progress.as_deref() == Some(step))
                .map(|a| a.harness_session.clone()),
            _ => None,
        })
    };
    assert_eq!(session_at("하나").as_deref(), Some("0a0a0a0a-0000-4000-8000-00000000000a"), "바깥 표지를 물려받았다");
    h.ok(&["pane", "send-keys", &id, "enter"]);
    assert_eq!(
        session_at("둘").as_deref(),
        Some("0a0a0a0a-0000-4000-8000-00000000000a"),
        "중첩 thread가 세션을 바꿨다"
    );
    h.ok(&["pane", "send-keys", &id, "enter"]);
    assert_eq!(session_at("셋").as_deref(), Some("0c0c0c0c-0000-4000-8000-00000000000c"));
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
    let qhost = b.session_host_pid(&qid).expect("session host PID");
    h.ok(&["pane", "forget", &qid]);
    // 지운 pane은 되살아나지 않는다(agent `forget`: 먼저 잊으면 session host가 닫힐 때 보내는 종료 알림이 되살린다).
    // 그 session host가 끝나기를 기다리고, 그 뒤 같은 agent 연결로 온 새 셸의 출력까지 host가 처리한 다음에 본다
    wait_for("지운 pane의 session host 끝남", T, || b.session_host_gone(qhost, &qid).then_some(()));

    // 원격 셸에 입력
    let rid = h.new_pane(None, "labB", Some("rsh"), &["/bin/sh"], None);
    h.ok(&["pane", "run", "rsh", "echo 원격-$((3*3))"]);
    h.wait_output(&rid, "원격-9", T);
    assert!(!h.ok(&["pane", "list"]).to_string().contains(&qid), "지운 pane이 되살아났다");

    // 이미 등록된 머신이 다시 등록하면 그렇다고 하고 다시 등록하는 길을 알린다(QA t-0337 Q17, 전에는 「hello가 와야
    // 한다」만 나왔다)
    let again = b.run(&["agent", "enroll", &addr, "--name", "labB"]);
    let err = String::from_utf8_lossy(&again.stderr).to_string();
    assert!(!again.status.success(), "이미 등록된 머신이 다시 등록됐다");
    assert!(
        err.contains("이 머신(labB)은 이미 이 host에 등록돼 있다") && err.contains("aaa machine revoke labB"),
        "{err}"
    );
    assert!(!err.contains("hello가 와야 한다"), "{err}");

    // 등록을 취소하면 그 장치는 다시 붙지 못한다
    h.ok(&["machine", "revoke", "labB"]);
    wait_for("취소 뒤 끊김", T, || (!h.connected("labB")).then_some(()));
    lab.kill("agentB");
    let mut hw = HostWatch::new(&h, T);

    // 알린 길대로 다시 등록한다: 등록 창을 열고 같은 머신이 다시 등록해 승인받으면 다시 붙는다
    h.ok(&["machine", "enroll", "--open", "5m"]);
    let (_, mut out) = lab.spawn_piped("enroll2", &b, &["agent", "enroll", &addr, "--name", "labB"]);
    let line = read_line_containing(&mut out, "--code");
    let code = line.split("--code").nth(1).unwrap().trim().to_string();
    h.ok(&["machine", "approve", "labB", "--code", &code]);
    assert!(lab.wait_exit("enroll2", T).success(), "다시 등록 실패: {}", lab.log("enroll2"));
    lab.spawn("agentB2", &b, &["agent", "--host", &addr, "--name", "labB"]);
    hw.wait("다시 등록한 labB 연결", |w| w.connected("labB").then_some(()));
}

/// 감독자를 흉내 내는 `launchctl`: 받은 인자를 `$LAUNCHCTL_LOG`에 적고, `$LAUNCHCTL_UP` 파일(안에 pid)로 올라가 있음을
/// 흉내 낸다. 실제 launchd는 건드리지 않는다.
#[cfg(target_os = "macos")]
const FAKE_LAUNCHCTL: &str = r#"#!/bin/sh
echo "$*" >> "$LAUNCHCTL_LOG"
case "$1" in
  print) [ -f "$LAUNCHCTL_UP" ] || { echo "Could not find service" >&2; exit 113; }
         printf '\tstate = running\n\tpid = %s\n' "$(cat "$LAUNCHCTL_UP")" ;;
  bootout) [ -f "$LAUNCHCTL_UP" ] || exit 3; rm -f "$LAUNCHCTL_UP" ;;
  bootstrap) echo 100 > "$LAUNCHCTL_UP" ;;
  kickstart) n=$(cat "$LAUNCHCTL_UP"); echo $((n + 1)) > "$LAUNCHCTL_UP" ;;
  *) exit 1 ;;
esac
"#;

/// 감독자 멈추기·띄우기·다시 띄우기(herdr `server stop`, 4장 A #497)는 launchctl `bootout`·`bootstrap`·`kickstart -k`를
/// 감싸고, 설치·해제처럼 사람이 터미널에서 `yes`를 쳐야 한다(aaa 세션 안·터미널 아님·`yes` 아님은 launchctl을 바꾸지
/// 않는다). 설치 전·멈춘 때·올라간 때 맞지 않는 명령은 까닭과 다음 명령을 알린다. launchctl은 가짜, HOME은 실험실이다.
#[cfg(target_os = "macos")]
#[test]
fn service_stop_start_restart_wrap_launchctl() {
    let lab = Lab::new();
    let h = lab.node("h", None);
    let home = lab.root.join("home");
    let bin = lab.root.join("fakebin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(bin.join("launchctl"), FAKE_LAUNCHCTL).unwrap();
    std::fs::set_permissions(bin.join("launchctl"), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let (log, up) = (lab.root.join("launchctl.log"), lab.root.join("launchctl.up"));
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let env = [
        ("PATH", path.as_str()),
        ("HOME", home.to_str().unwrap()),
        ("LAUNCHCTL_LOG", log.to_str().unwrap()),
        ("LAUNCHCTL_UP", up.to_str().unwrap()),
    ];
    let run = |args: &[&str], answers: &[(&str, &str)]| h.in_terminal_env(args, &env, answers, T);
    let changes =
        || std::fs::read_to_string(&log).unwrap_or_default().lines().filter(|l| !l.starts_with("print ")).count();
    let uid = String::from_utf8(std::process::Command::new("id").arg("-u").output().unwrap().stdout).unwrap();
    let target = format!("gui/{}/dev.aaa.agent", uid.trim());

    let out = run(&["service", "restart", "--role", "agent"], &[]);
    assert!(out.contains("감독자가 설치되지 않았다") && out.contains("aaa service install --role agent"), "{out}");
    let plist = home.join("Library/LaunchAgents/dev.aaa.agent.plist");
    std::fs::create_dir_all(plist.parent().unwrap()).unwrap();
    std::fs::write(&plist, "<plist/>").unwrap();
    let out = run(&["service", "restart", "--role", "agent"], &[]);
    assert!(out.contains("멈춰 있다") && out.contains("aaa service start --role agent"), "{out}");
    let out = run(&["service", "stop", "--role", "agent"], &[]);
    assert!(out.contains("이미 멈춰 있다"), "{out}");
    // 사람 문: 터미널 아님·aaa 세션 안·yes 아님
    let o = h.cmd(&["service", "start", "--role", "agent"]).envs(env).output().unwrap();
    assert!(String::from_utf8_lossy(&o.stderr).contains("터미널에서 실행해야 한다"), "{o:?}");
    let mut in_session = env.to_vec();
    in_session.push(("AAA_SESSION_ID", "01EXAMPLE"));
    let out = h.in_terminal_env(&["service", "start", "--role", "agent"], &in_session, &[], T);
    assert!(out.contains("aaa 세션 안에서는 하지 않는다"), "{out}");
    let out = run(&["service", "start", "--role", "agent"], &[("yes를 입력", "no")]);
    assert!(out.contains("그만뒀다"), "{out}");
    assert_eq!(changes(), 0, "{}", std::fs::read_to_string(&log).unwrap_or_default());

    let out = run(&["service", "start", "--role", "agent"], &[("yes를 입력", "yes")]);
    assert!(out.contains("띄웠다: dev.aaa.agent · state = running · pid = 100"), "{out}");
    let out = run(&["service", "start", "--role", "agent"], &[]);
    assert!(out.contains("이미 올라가 있다") && out.contains("aaa service restart --role agent"), "{out}");
    let out = run(&["service", "restart", "--role", "agent"], &[("yes를 입력", "yes")]);
    assert!(out.contains("다시 띄웠다: dev.aaa.agent · state = running · pid = 101"), "{out}");
    let out = run(&["service", "stop", "--role", "agent"], &[("yes를 입력", "yes")]);
    assert!(out.contains("멈췄다: dev.aaa.agent") && !up.exists(), "{out}");
    assert!(plist.exists(), "멈추기는 plist를 남긴다");
    let calls: Vec<String> =
        std::fs::read_to_string(&log).unwrap().lines().filter(|l| !l.starts_with("print ")).map(String::from).collect();
    assert_eq!(
        calls,
        [
            format!("bootstrap gui/{} {}", uid.trim(), plist.display()),
            format!("kickstart -k {target}"),
            format!("bootout {target}"),
        ]
    );
}

/// `aaa update`(plan §8.180, herdr `update`): 실험실에 둔 실행 파일 사본을 띄워 그것을 바꾼다(시험 binary는 건드리지 않는다).
/// 「다른 빌드」는 시험 binary 뒤에 바이트를 덧붙인 사본이다(같은 version, 다른 sha256, 그대로 돈다).
#[cfg(unix)]
#[test]
fn update_replaces_keeps_and_rolls_back() {
    use std::os::unix::fs::PermissionsExt as _;
    let lab = Lab::new();
    let h = lab.node("h", None);
    let bin = lab.root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let exe = bin.join("aaa");
    std::fs::copy(lab::BIN, &exe).unwrap();
    let original = std::fs::read(lab::BIN).unwrap();
    let version = env!("CARGO_PKG_VERSION");
    let build = |name: &str| {
        let p = lab.root.join(name);
        let mut b = original.clone();
        b.extend_from_slice(format!("aaa-test-{name}").as_bytes());
        std::fs::write(&p, b).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    };
    let script = |name: &str, body: &str| {
        let p = lab.root.join(name);
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    };
    let (new1, new2) = (build("new1"), build("new2"));
    let s = |p: &std::path::Path| p.to_str().unwrap().to_string();
    let run = |args: &[&str], answers: &[(&str, &str)]| h.in_terminal_bin(&exe, args, &[], answers, T);
    let is = |want: &[u8]| std::fs::read(&exe).unwrap() == want;
    let kept = || {
        let mut v: Vec<String> = std::fs::read_dir(bin.join(".aaa-old"))
            .map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default();
        v.sort();
        v
    };
    let staging_left = || {
        std::fs::read_dir(&bin).unwrap().flatten().any(|e| e.file_name().to_string_lossy().starts_with(".aaa-update-"))
    };

    // 사람 문: 터미널 아님·aaa 세션 안. 잘못된 release 판은 네트워크에 닿기 전에 거부한다
    let o = std::process::Command::new(&exe)
        .args(["update", "--from", &s(&new1)])
        .env("AAA_CONFIG_DIR", &h.cfg)
        .env("AAA_STATE_DIR", &h.state)
        .env_remove("AAA_SESSION_ID")
        .env_remove("AAA_HOOK_SOCK")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&o.stderr).contains("터미널에서 실행해야 한다"), "{o:?}");
    let out = h.in_terminal_bin(&exe, &["update", "--from", &s(&new1)], &[("AAA_SESSION_ID", "01EXAMPLE")], &[], T);
    assert!(out.contains("aaa 세션 안에서는 하지 않는다"), "{out}");
    assert!(run(&["update", "--version", "invalid"], &[]).contains("release 판은 x.y.z"));
    // self-check: 돌지 않는 파일, version을 말하지 않는 파일, 이 Mac 설정을 받지 않는 파일은 바꾸지 않고 staging도 남기지 않는다
    for (name, body, want) in [
        ("broken", "exit 1", "`--version`이 `aaa x.y.z`가 아니다"),
        ("other", "echo other 1.0.0", "`--version`이 `aaa x.y.z`가 아니다"),
        (
            "strict",
            "case \"$1\" in --version) echo aaa 0.1.0;; *) echo 'schema 9'; exit 2;; esac",
            "이 Mac 설정을 받지 않는다",
        ),
    ] {
        let out = run(&["update", "--from", &s(&script(name, body))], &[]);
        assert!(out.contains("self-check 실패") && out.contains(want), "{name}: {out}");
    }
    // version 고정: 고정한 것과 다르면, 고정 없이 major가 바뀌면 거부
    let major = script("major", "case \"$1\" in --version) echo aaa 9.0.0;; *) exit 0;; esac");
    assert!(run(&["update", "--from", &s(&major)], &[]).contains(&format!("major가 바뀐다({version} → 9.0.0)")));
    std::fs::write(h.cfg.join("agent.toml"), "schema = 1\n[update]\nversion = \"0.0.9\"\n").unwrap();
    let out = run(&["update", "--from", &s(&new1)], &[]);
    assert!(out.contains("agent.toml `[update] version = \"0.0.9\"`로 고정돼 있다"), "{out}");
    std::fs::remove_file(h.cfg.join("agent.toml")).unwrap();
    assert!(is(&original) && kept().is_empty() && !staging_left(), "거부는 아무것도 바꾸지 않는다");

    // 검사만(누구나), no면 그만, yes면 바꾸고 옛것을 보관
    let o = std::process::Command::new(&exe)
        .args(["update", "--from", &s(&new1), "--dry-run"])
        .env("AAA_CONFIG_DIR", &h.cfg)
        .env("AAA_STATE_DIR", &h.state)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&o.stdout).contains("검사만 했다"), "{o:?}");
    assert!(run(&["update", "--from", &s(&new1)], &[("yes를 입력", "no")]).contains("그만뒀다"));
    assert!(is(&original) && kept().is_empty() && !staging_left());
    let out = run(&["update", "--from", &s(&new1)], &[("yes를 입력", "yes")]);
    assert!(
        out.contains(&format!("바꿨다: aaa {version} → aaa {version}"))
            && out.contains("aaa service restart --role agent"),
        "{out}"
    );
    assert!(is(&std::fs::read(&new1).unwrap()) && !staging_left());
    let k = kept();
    assert!(k.len() == 1 && k[0].starts_with("aaa-") && k[0].ends_with(&format!("-{version}")), "{k:?}");
    assert_eq!(std::fs::read(bin.join(".aaa-old").join(&k[0])).unwrap(), original, "보관한 것은 바꾸기 전 실행 파일");
    assert!(run(&["update", "--from", &s(&new1)], &[]).contains("이미 같은 실행 파일이다"));

    // 되돌리기: 이름이 없으면 지금과 다른 가장 새 것(new2 → new1, 그 밑 원본이 아니다), 다시 하면 되돌린 것을 되돌린다
    let out = run(&["update", "--from", &s(&new2)], &[("yes를 입력", "yes")]);
    assert!(out.contains("바꿨다") && is(&std::fs::read(&new2).unwrap()), "{out}");
    let out = run(&["update", "rollback"], &[("yes를 입력", "yes")]);
    assert!(out.contains("바꿨다") && is(&std::fs::read(&new1).unwrap()), "가장 새 보관(new1)으로: {out}");
    assert_eq!(kept().len(), 3);
    let out = run(&["update", "rollback"], &[("yes를 입력", "yes")]);
    assert!(is(&std::fs::read(&new2).unwrap()), "두 번째 되돌리기는 new2로: {out}");
    assert!(out.contains("보관 수(3)를 넘어 지웠다"), "{out}");
    let list = h.in_terminal_bin(&exe, &["--json", "update", "list"], &[], &[], T);
    let v: serde_json::Value = serde_json::from_str(list.trim()).unwrap_or_else(|e| panic!("{e}: {list}"));
    assert_eq!(v["kept"].as_array().unwrap().len(), 3, "{v}");

    // 보관 수: keep = 1이면 바꾼 뒤 가장 새 것 하나만 남고, 그 이름으로 되돌린다
    std::fs::write(h.cfg.join("agent.toml"), "schema = 1\n[update]\nkeep = 1\n").unwrap();
    let out = run(&["update", "--from", &s(&new1)], &[("yes를 입력", "yes")]);
    assert!(out.contains("보관 수(1)를 넘어 지웠다") && is(&std::fs::read(&new1).unwrap()), "{out}");
    let k = kept();
    assert_eq!(k.len(), 1, "{k:?}");
    assert_eq!(std::fs::read(bin.join(".aaa-old").join(&k[0])).unwrap(), std::fs::read(&new2).unwrap());
    let out = run(&["update", "rollback", &k[0]], &[("yes를 입력", "yes")]);
    assert!(out.contains("바꿨다") && is(&std::fs::read(&new2).unwrap()), "{out}");
    assert!(run(&["update", "rollback", "aaa-none"], &[]).contains("보관한 것 가운데 `aaa-none`가 없다"));
}

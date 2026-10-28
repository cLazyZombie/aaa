//! 배포하지 않는 시작 실패 회귀: 실제 host·agent·CLI, 끊긴 소스와 시작 전 거절·같은 ID 조회.
#![cfg(unix)]

mod lab;

use lab::{Lab, wait_for};
use std::time::{Duration, Instant};

#[test]
fn disconnected_start_keeps_a_stopped_record_across_host_restart_without_installing() {
    let began = Instant::now();
    let limit = Duration::from_secs(5);
    let mut lab = Lab::new();
    let h = lab.node(
        "h",
        Some("schema = 1\n[host]\nagent_listen = \"\"\n[web]\nlisten = \"\"\n[local_agent]\nmode = \"none\"\n"),
    );
    std::fs::create_dir_all(h.state.join("host")).unwrap();
    let db = aaa_host::db::Db::open(&h.state.join("host/host.db")).unwrap();
    db.upsert_machine("offline", "mac-b", None, false).unwrap();
    drop(db);
    lab.spawn("host", &h, &["host"]);
    wait_for("시험 host 준비", limit, || h.json(&["status"]).ok());
    h.ok(&["project", "create", "demo"]);
    let mut events = lab::watch::HostWatch::new(&h, limit);
    lab.spawn("agent", &h, &["agent", "--host", "local", "--name", "mac-a"]);
    events.wait("로컬 시험 agent 연결", |e| e.connected("mac-a").then_some(()));
    // 사람이 연 터미널의 권한을 그대로 쓰며 자식 종료 코드를 별도 파일에 보존한다.
    let wrapper = lab.root.join("person-cli.sh");
    let result_path = lab.root.join("person-cli.exit");
    std::fs::write(&wrapper, "#!/bin/sh\n\"$AAA_TEST_CLI\" \"$@\"\naaa_test_status=$?\nprintf '%s' \"$aaa_test_status\" > \"$AAA_TEST_EXIT\"\nexit \"$aaa_test_status\"\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    let person = |args: &[&str], answers: &[(&str, &str)]| {
        let out = h.in_terminal_bin(
            &wrapper,
            args,
            &[("AAA_TEST_CLI", lab::BIN), ("AAA_TEST_EXIT", result_path.to_str().unwrap())],
            answers,
            limit,
        );
        let code = std::fs::read_to_string(&result_path).unwrap();
        (out, code)
    };
    let sha = "a".repeat(64);
    let start = |sha: &str| {
        person(
            &[
                "--json",
                "deploy",
                "start",
                "--file",
                "mac-b:/opt/aaa-candidate/aaa",
                "--sha256",
                sha,
                "--machines",
                "all",
            ],
            &[("yes를 입력한다:", "yes")],
        )
    };
    let (out, code) = start(&sha);
    assert_eq!(code, "1", "{out}");
    assert!(out.contains("아직 접수 확인 전"), "{out}");
    assert!(
        out.contains("배포 중단") && out.contains("빌드 머신 mac-b") && out.contains("agent_disconnected"),
        "{out}"
    );
    let record: serde_json::Value = out.lines().find_map(|l| serde_json::from_str(l).ok()).expect(&out);
    let r = &record["rollout"];
    assert_eq!(r["state"], "stopped", "{out}");
    assert!(r["steps"].as_array().unwrap().iter().all(|s| s["started_ms"] == 0 && s["completed"] == false));
    let id = r["id"].as_str().unwrap();
    let status = || {
        let (out, code) = person(&["--json", "deploy", "status", id, "--wait"], &[]);
        assert_eq!(code, "1", "{out}");
        let record: serde_json::Value = out.lines().find_map(|l| serde_json::from_str(l).ok()).expect(&out);
        assert_eq!(record["rollout"], *r);
    };
    status();
    // 형식 오류는 시작 전에 거절한다. 요청 ID와 접수 완료를 구분하는 안내가 있어야 한다.
    let (refused, code) = start("not-a-sha");
    assert_eq!(code, "1", "{refused}");
    assert!(refused.contains("접수 완료를 뜻하지 않는다") && refused.contains("bad_request"), "{refused}");
    assert!(!refused.contains("· 접수됨"), "{refused}");
    drop(events);
    lab.kill("host");
    // agent 연결을 보류해 host 준비와 agent 준비가 다른 경계임을 매번 검사한다.
    lab.kill("agent");
    lab.spawn("host-reopened", &h, &["host"]);
    wait_for("재시작 시험 host 준비", limit, || h.json(&["status"]).ok());
    // 옛 host의 Connected 사건을 재사용하지 않고 새 host의 상태부터 구독한다.
    let mut reopened = lab::watch::HostWatch::new(&h, limit);
    reopened.wait("재시작 뒤 agent 연결 전 상태", |e| {
        e.machines
            .iter()
            .any(|m| m.name == "mac-a" && m.connection == aaa_proto::Connection::Disconnected)
            .then_some(())
    });
    lab.spawn("agent-reopened", &h, &["agent", "--host", "local", "--name", "mac-a"]);
    reopened.wait_diag(
        "재시작 뒤 로컬 시험 agent 연결",
        |e| e.connected("mac-a").then_some(()),
        || format!("host:\n{}\nagent:\n{}", lab.log("host-reopened"), lab.log("agent-reopened")),
    );
    status();
    assert!(lab.log("host").contains("rollout 시작 사전 검사 실패: 설치하지 않음"));
    eprintln!("시작 거절·host 재시작 owner: {:?}", began.elapsed());
}

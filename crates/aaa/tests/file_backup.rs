//! 실제 CLI·host 저장과 별도 백업 프로세스·복원 host의 링크를 연결한다.
mod lab;

use aaa_common::private_file;
use std::fs;

#[test]
fn cli_restores_a_live_hosts_idea_into_a_new_host_and_obeys_the_cross_process_write_lock() {
    let mut lab = lab::Lab::new();
    let node = lab.node("files", Some("schema=1\n[host]\nagent_listen=''\n[web]\nlisten=''\n[local_agent]\nmode='none'\n[notify]\nweb_push=false\n"));
    let key = lab.root.join("identity");
    let public = aaa_host::backup::keygen(&key).unwrap();
    let config = node.cfg.join("host.toml");
    let mut text = fs::read_to_string(&config).unwrap();
    text.push_str(&format!(
        "\n[backup]\nenabled=false\ndirectory={}\nrecipient={}\n",
        serde_json::to_string(&lab.root.to_string_lossy()).unwrap(),
        serde_json::to_string(&public).unwrap()
    ));
    fs::write(config, text).unwrap();
    lab.spawn("source", &node, &["host"]);
    lab.wait_host_ready("source", &node);
    node.ok(&["project", "create", "demo"]);
    node.ok(&["idea", "new", "복원 시험"]);
    let png = lab.root.join("fixture.png");
    let bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";
    fs::write(&png, bytes).unwrap();
    node.ok(&["idea", "attach", "n-0001", png.to_str().unwrap()]);
    let before = node.ok(&["idea", "show", "n-0001"]);
    let writing = private_file::coordinated_lock(&node.state.join("host/idea-files.lock"), true).unwrap();
    assert!(node.json(&["file-backup", "run"]).is_err(), "별도 CLI도 저장 중간에 들어갈 수 없다");
    writing.unlock().unwrap();
    drop(writing);
    let result = node.ok(&["file-backup", "run"]);
    assert!(node.ok(&["file-backup", "status"])["last_attempt_ms"].is_null(), "수동 실행은 정기 기록을 바꾸지 않는다");
    let output = lab.root.join("recovery");
    node.ok(&[
        "file-backup",
        "restore",
        result["backup"].as_str().unwrap(),
        "--identity",
        key.to_str().unwrap(),
        "--output",
        output.to_str().unwrap(),
    ]);
    let recovered = lab.node("recovered", Some(&format!("schema=1\n[host]\nagent_listen=''\n[web]\nlisten=''\n[local_agent]\nmode='none'\n[notify]\nweb_push=false\n[projects]\nroot={}\n", serde_json::to_string(&output.join("projects").to_string_lossy()).unwrap())));
    // 복원 명령의 결과 root 자체가 --state-dir로 쓰인다.
    let mut command = recovered.cmd(&["host"]);
    command.env("AAA_STATE_DIR", &output);
    lab.spawn_command("restored", command);
    let recovered = lab::Node { state: output.clone(), ..recovered };
    lab.wait_host_ready("restored", &recovered);
    assert_eq!(recovered.ok(&["idea", "show", "n-0001"]), before);
    assert!(recovered.ok(&["db-backup", "status"])["last_attempt_ms"].is_null());
    lab.kill("restored");
    lab.kill("source");
}

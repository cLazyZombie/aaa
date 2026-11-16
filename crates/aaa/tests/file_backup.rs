//! 실제 CLI·host 저장과 별도 백업 프로세스·복원 host의 링크를 연결한다.
mod lab;

use aaa_common::private_file;
use std::fs;

#[test]
fn cli_restores_a_live_hosts_idea_into_a_new_host_and_obeys_the_cross_process_write_lock() {
    let mut lab = lab::Lab::new();
    let projects = lab.root.join("projects");
    let node = lab.node("files", Some(&format!("schema=1\n[host]\nagent_listen=''\n[web]\nlisten=''\n[local_agent]\nmode='none'\n[notify]\nweb_push=false\n[projects]\nroot={}\n", serde_json::to_string(&projects.to_string_lossy()).unwrap())));
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
    // 아이디어 쓰기 명령은 거뒀다(t-1894 4단계): 옛 판이 만든 노트 폴더를 놓고 읽기·복원만 본다
    let refused = node.json(&["idea", "new", "복원 시험"]).unwrap_err();
    assert!(refused.contains("옛 아이디어는 읽기 전용이다") && refused.contains("aaa plan"), "{refused}");
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/e2e/fixtures/ideas");
    copy_dir(&fixtures, &projects.join("demo/ideas"));
    let ideas = node.ok(&["idea", "list"]);
    let camp = ideas["ideas"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["title"] == "캠프 게시판 의뢰")
        .and_then(|i| i["short_id"].as_str())
        .unwrap()
        .to_string();
    let before = node.ok(&["idea", "show", &camp]);
    assert_eq!(before["note"]["rev"], 2);
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
    assert_eq!(recovered.ok(&["idea", "show", &camp]), before);
    assert!(recovered.ok(&["db-backup", "status"])["last_attempt_ms"].is_null());
    lab.kill("restored");
    lab.kill("source");
}

fn copy_dir(from: &std::path::Path, to: &std::path::Path) {
    fs::create_dir_all(to).unwrap();
    for e in fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &to.join(e.file_name()));
        } else {
            fs::copy(e.path(), to.join(e.file_name())).unwrap();
        }
    }
}

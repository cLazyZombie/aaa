//! 실제 CLI·host 저장과 별도 백업 프로세스·새 폴더 복원을 연결한다.
mod lab;

use aaa_common::private_file;
use std::fs;

#[test]
fn cli_restores_a_live_hosts_uploads_into_a_new_folder_and_obeys_the_cross_process_write_lock() {
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
    // 옛 아이디어 명령은 지웠다(t-1959)
    assert!(node.json(&["idea", "list"]).is_err());
    let png = b"\x89PNG\r\n\x1a\nlive upload".to_vec();
    fs::create_dir_all(projects.join("demo/uploads")).unwrap();
    fs::write(projects.join("demo/uploads/a6d715956dab435e.png"), &png).unwrap();
    let writing = private_file::coordinated_lock(&node.state.join("host/files.lock"), true).unwrap();
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
    assert_eq!(fs::read(output.join("projects/demo/uploads/a6d715956dab435e.png")).unwrap(), png);
    assert!(!output.join("projects/demo/ideas").exists());
    lab.kill("source");
}

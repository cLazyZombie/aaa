//! CLI 프로세스에서 생성·백업·복원을 연결한다. WAL 원본은 부모가 계속 연 상태다.
mod lab;

use aaa_host::{Host, db::Db};
use std::fs;

#[test]
fn encrypted_db_backup_cli_restores_live_host_without_overwrite() {
    let lab = lab::Lab::new();
    let node = lab.node("backup", None);
    let state = node.state.join("host");
    let mut isolated = aaa_config::HostConfig::default();
    isolated.projects.root = lab.root.join("projects").to_string_lossy().into();
    let host = Host::open(isolated, state.clone()).unwrap();
    let value = aaa_test_support::token("backup-cli-wal");
    host.db.set_meta("backup-fixture", &value).unwrap();
    let identity = lab.root.join("recovery-identity");
    let key = node.json(&["db-backup", "keygen", "--identity", identity.to_str().unwrap()]).unwrap();
    let cfg = aaa_config::HostConfig {
        backup: aaa_config::BackupSection {
            enabled: true,
            directory: lab.root.to_string_lossy().into(),
            recipient: key["recipient"].as_str().unwrap().into(),
            ..Default::default()
        },
        ..Default::default()
    };
    // 경로의 역슬래시는 JSON과 TOML의 기본 문자열에서 같은 escape를 쓴다.
    fs::write(
        node.cfg.join("host.toml"),
        format!(
            "schema = 1\n[backup]\nenabled=true\ndirectory={}\nrecipient={}\n",
            serde_json::to_string(&cfg.backup.directory).unwrap(),
            serde_json::to_string(&cfg.backup.recipient).unwrap()
        ),
    )
    .unwrap();
    let result = node.json(&["db-backup", "run"]).unwrap();
    let backup = result["backup"].as_str().unwrap();
    let restored = lab.root.join("restored.db");
    node.json(&[
        "db-backup",
        "restore",
        backup,
        "--identity",
        identity.to_str().unwrap(),
        "--output",
        restored.to_str().unwrap(),
    ])
    .unwrap();
    let db = Db::open(&restored).unwrap();
    assert_eq!(db.meta("backup-fixture").unwrap().as_deref(), Some(value.as_str()));
    assert_eq!(db.meta("host_id").unwrap().as_deref(), Some(host.host_id.as_str()));
    assert!(
        node.json(&[
            "db-backup",
            "restore",
            backup,
            "--identity",
            identity.to_str().unwrap(),
            "--output",
            state.join("host.db").to_str().unwrap()
        ])
        .is_err()
    );
    assert_eq!(host.db.meta("backup-fixture").unwrap().as_deref(), Some(value.as_str()));
}

#[test]
fn host_ticker_backs_up_once_and_remembers_attempt_after_restart() {
    use std::time::Duration;
    let mut lab = lab::Lab::new();
    let node = lab.node("scheduled", None);
    let identity = lab.root.join("identity");
    let key = node.json(&["db-backup", "keygen", "--identity", identity.to_str().unwrap()]).unwrap();
    let directory = serde_json::to_string(&lab.root.to_string_lossy()).unwrap();
    fs::write(node.cfg.join("host.toml"), format!(
        "schema=1\n[host]\nagent_listen=''\n[web]\nlisten=''\n[local_agent]\nmode='none'\n[notify]\nweb_push=false\n[projects]\nroot={}\ntick_secs=1\n[backup]\nenabled=true\ndirectory={directory}\nrecipient={}\ndaily_utc='00:00'\n",
        serde_json::to_string(&lab.root.join("projects").to_string_lossy()).unwrap(), key["recipient"]
    )).unwrap();
    lab.spawn("host", &node, &["host"]);
    let first = lab::wait_for("정기 백업 성공 기록", Duration::from_secs(5), || {
        let status = node.json(&["db-backup", "status"]).ok()?;
        status["last_success_ms"].as_u64().map(|_| status)
    });
    lab.kill("host");
    lab.spawn("restarted", &node, &["host"]);
    lab::wait_for("다시 뜬 host의 tick", Duration::from_secs(5), || node.json(&["ticker", "tick"]).ok());
    assert_eq!(node.json(&["db-backup", "status"]).unwrap(), first);
}

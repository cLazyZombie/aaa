//! 기존 역할 로그를 열어 둔 감독자와 같은 fd로 실제 host·agent를 실행한다.
#![cfg(unix)]
mod lab;

use aaa_common::child_process::StdCommandExt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Child;

struct Role(Child);

impl Role {
    fn stop(&mut self) {
        // SAFETY: 이 시험이 직접 시작했고 아직 회수하지 않은 자식에게만 보낸다.
        assert_eq!(unsafe { libc::kill(self.0.id() as i32, libc::SIGTERM) }, 0);
        let result = lab::wait_for("역할 종료", lab::START_TIMEOUT, || self.0.try_wait().unwrap());
        assert!(result.success(), "{result}");
    }
}

impl Drop for Role {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(None)) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn output_path(node: &lab::Node, home: &Path, role: &str) -> PathBuf {
    #[cfg(target_os = "macos")]
    let dir = {
        let _ = node;
        home.join("Library/Logs/aaa-log-lab")
    };
    #[cfg(not(target_os = "macos"))]
    let dir = {
        let _ = home;
        node.state.clone()
    };
    fs::create_dir_all(&dir).unwrap();
    dir.join(format!("{role}.log"))
}

fn launch(node: &lab::Node, home: &Path, role: &str, path: &Path) -> Role {
    let fd = OpenOptions::new().append(true).open(path).unwrap();
    let child = node
        .cmd(&[role])
        .env("HOME", home)
        .env("AAA_INSTANCE", "log-lab")
        .env("AAA_LOG", "info")
        .stdout(fd.try_clone().unwrap())
        .stderr(fd)
        .spawn_guarded()
        .unwrap();
    Role(child)
}

#[test]
fn host_and_agent_keep_legacy_logs_and_write_to_the_same_active_names_after_rotation() {
    let lab = lab::Lab::new();
    let home = lab.root.join("home");
    fs::create_dir(&home).unwrap();
    let node = lab.node(
        "logs",
        Some("schema=1\n[host]\nagent_listen=''\n[web]\nlisten=''\n[notify]\nweb_push=false\n[logs]\nmax_file_mib=1\nkeep_files=1\n"),
    );
    fs::write(
        node.cfg.join("agent.toml"),
        format!(
            "schema=1\n[agent]\nhost='local'\nname='log-agent'\nwork_root={:?}\n[logs]\nmax_file_mib=1\nkeep_files=1\n",
            lab.root.join("work"),
        ),
    )
    .unwrap();
    let mut roles = Vec::new();
    for role in ["host", "agent"] {
        let path = output_path(&node, &home, role);
        let mut old = fs::File::create(&path).unwrap();
        old.write_all(b"legacy log\n").unwrap();
        old.set_len(1_048_577).unwrap();
        drop(old);
        let process = launch(&node, &home, role, &path);
        lab::wait_for_diag(
            "새 활성 로그에 서비스 시작 기록",
            lab::START_TIMEOUT,
            || fs::read_to_string(&path).ok().filter(|text| text.contains(&format!("{role} 시작"))),
            || fs::read_to_string(&path).unwrap_or_default().chars().take(1200).collect(),
        );
        let prefix = format!("{role}.log.rotated-");
        let archives: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap())
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))
            .collect();
        assert_eq!(archives.len(), 1);
        let legacy = fs::read(archives[0].path()).unwrap();
        assert_eq!(legacy.len(), 1_048_577);
        assert!(legacy.starts_with(b"legacy log\n"));
        roles.push((process, path, role));
    }
    let mut watch = lab::watch::HostWatch::new(&node, lab::START_TIMEOUT);
    watch.wait_diag(
        "agent가 host와 연결됨",
        |state| state.connected("log-agent").then_some(()),
        || String::from("역할별 시작 로그는 확인했다"),
    );
    for (mut process, path, role) in roles.into_iter().rev() {
        process.stop();
        assert!(fs::read_to_string(path).unwrap().contains(&format!("{role} 종료")));
    }
}

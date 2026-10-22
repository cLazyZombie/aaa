//! macOS 실험실: 고정 launcher 아래 실제 agent 이미지만 교체한다. launchd·TCC·운영 서비스는 만지지 않는다.
#![cfg(target_os = "macos")]
mod lab;

use aaa_common::{process, util};
use lab::{Lab, Node, wait_for};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

const T: Duration = Duration::from_secs(30);
// launcher를 새 책임자로 만드는 실험 입구. 실제 launcher 소스·등록·환경은 바꾸지 않는다.
const SPAWNER: &str = r#"
#include <dlfcn.h>
#include <errno.h>
#include <signal.h>
#include <spawn.h>
#include <stdio.h>
#include <sys/wait.h>
#include <unistd.h>
extern char **environ;
static volatile sig_atomic_t child;
static void forward(int sig) { if (child > 0) kill(child, sig); }
int main(int argc, char **argv) {
    if (argc < 4) return 64;
    void (*disclaim)(posix_spawnattr_t *, int) = dlsym(RTLD_DEFAULT, "responsibility_spawnattrs_setdisclaim");
    if (!disclaim) return 65;
    struct sigaction sa = {0}; sa.sa_handler = forward; sigemptyset(&sa.sa_mask);
    sigaction(SIGTERM, &sa, NULL); sigaction(SIGINT, &sa, NULL);
    posix_spawnattr_t attr; if (posix_spawnattr_init(&attr)) return 66;
    disclaim(&attr, 1);
    pid_t pid; int rc = posix_spawn(&pid, argv[2], NULL, &attr, &argv[2], environ);
    posix_spawnattr_destroy(&attr); if (rc) return 67;
    child = pid;
    FILE *file = fopen(argv[1], "w"); if (!file) return 68;
    fprintf(file, "%d", pid); fclose(file);
    int status;
    while (waitpid(pid, &status, 0) < 0) { if (errno != EINTR) return 69; }
    return WIFEXITED(status) ? WEXITSTATUS(status) : 70;
}
"#;

struct Launcher {
    pid: u32,
    image: PathBuf,
}
impl Drop for Launcher {
    fn drop(&mut self) {
        if let Ok(expected) = self.image.canonicalize()
            && process::image(self.pid).and_then(|path| path.canonicalize().ok()).as_ref() == Some(&expected)
        {
            unsafe {
                libc::kill(self.pid as i32, libc::SIGTERM);
            }
        }
    }
}
fn record(n: &Node) -> Value {
    serde_json::from_slice(&std::fs::read(n.state.join("service-restart/agent.json")).unwrap()).unwrap()
}
fn pane(n: &Node, id: &str) -> Value {
    n.pane(id)
}
struct Probes {
    root: PathBuf,
    archive: PathBuf,
}
impl Probes {
    fn new(repo: &Path) -> Self {
        let id = util::new_id();
        // 새 책임자는 저장소가 있는 보호 볼륨을 읽을 권한이 없다. 등록·권한을 바꾸지 않고
        // 자료와 cwd를 비공유 캐시에 둔다. Cargo target도 OS 임시 폴더에는 만들지 않는다.
        let cache = aaa_config::paths::home().join("Library/Caches");
        std::fs::create_dir_all(&cache).unwrap();
        let root = cache.join(format!("aaa-restart-{id}"));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("probe.py"), include_str!("../../../scripts/pane-responsibility-probe.py")).unwrap();
        let archive = repo.join("target/restart-probes").join(id);
        std::fs::create_dir_all(&archive).unwrap();
        Self { root, archive }
    }
}
impl Drop for Probes {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("책임 진단 자료를 남겼다: {}", self.root.display());
        } else {
            std::fs::remove_dir_all(&self.root).unwrap();
        }
    }
}
fn probe(n: &Node, pane: &str, probes: &Probes, owner: u32, stage: &str) {
    let output = probes.root.join(stage);
    let command = format!(
        "python3 {} --output-dir {} --expected-responsible {}; code=$?; printf 'probe-%s-exit:%d\\n' {} \"$code\"",
        lab::shell_word(&probes.root.join("probe.py").to_string_lossy()),
        lab::shell_word(&output.to_string_lossy()),
        owner,
        stage
    );
    n.ok(&["pane", "run", pane, &command]);
    let done = n.wait_output(pane, &format!("probe-{stage}-exit:"), T);
    assert_eq!(done["matched_line"], format!("probe-{stage}-exit:0"), "{done}");
    let result: Value = util::read_json(&output.join("result.json")).unwrap();
    assert_eq!(result["records"].as_array().unwrap().len(), 6);
    for record in result["records"].as_array().unwrap() {
        assert_eq!(record["responsible"], owner);
    }
    // 보존 사본은 원래 시험 책임자가 읽고 쓴다. 새 launcher는 저장소를 열지 않는다.
    assert!(Command::new("cp").arg("-R").arg(&output).arg(probes.archive.join(stage)).status().unwrap().success());
}

#[test]
fn worker_exec_preserves_launcher_existing_pane_and_new_pane_responsibility() {
    let mut lab = Lab::new();
    let node = lab.node("n", Some("schema = 1\n[host]\nagent_listen = '127.0.0.1:0'\n[web]\nlisten = '127.0.0.1:0'\n[local_agent]\nmode = 'none'\n"));
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let probes = Probes::new(&repo);
    let existing = std::env::var_os("AAA_RESTART_TEST_LAUNCHER").map(PathBuf::from);
    let image = existing.clone().unwrap_or_else(|| lab.root.join("aaa-launcher.app/Contents/MacOS/aaa-launcher"));
    if existing.is_none() {
        std::fs::create_dir_all(image.parent().unwrap()).unwrap();
        let built = Command::new("cc")
            .args(["-O2", "-Wall", "-Wextra", "-o"])
            .arg(&image)
            .arg(repo.join("p0/launcher/aaa-launcher.c"))
            .output()
            .unwrap();
        assert!(built.status.success(), "{}", String::from_utf8_lossy(&built.stderr));
        // 실행 파일만 든 가짜 .app은 번들 검사에 실패한다. 설치와 같은 정상 번들을 만들되
        // 별도 시험 신원을 쓰며 LaunchServices·TCC에는 등록하지 않는다.
        let bundle = image.parent().unwrap().parent().unwrap().parent().unwrap();
        std::fs::write(
            bundle.join("Contents/Info.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>dev.aaa.restart-fixture</string>
<key>CFBundleExecutable</key><string>aaa-launcher</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleVersion</key><string>1</string>
</dict></plist>"#,
        )
        .unwrap();
        for args in [vec!["--force", "--sign", "-", "--timestamp=none"], vec!["--verify", "--strict"]] {
            let out = Command::new("codesign").args(args).arg(bundle).output().unwrap();
            assert!(out.status.success(), "시험 launcher 번들 서명: {}", String::from_utf8_lossy(&out.stderr));
        }
    }
    let launcher_before = std::fs::read(&image).unwrap();
    let source = lab.root.join("spawn.c");
    std::fs::write(&source, SPAWNER).unwrap();
    let spawn = lab.root.join("spawn");
    assert!(Command::new("cc").args(["-Wall", "-Wextra", "-o"]).arg(&spawn).arg(source).status().unwrap().success());
    let binary = lab.root.join("aaa");
    lab::variant_binary(Path::new(lab::BIN), &binary, "aaa-restart-first");
    lab.spawn("host", &node, &["host"]);
    wait_for("host 준비", T, || node.json(&["machine", "list"]).ok());
    let pidfile = lab.root.join("launcher.pid");
    let cmd = node.cmd(&[]);
    // Node가 가리는 실험실 환경을 그대로 helper로 넘긴다.
    let envs: Vec<_> = cmd.get_envs().map(|(k, v)| (k.to_owned(), v.map(ToOwned::to_owned))).collect();
    let mut command = Command::new(&spawn);
    command.current_dir(&probes.root);
    // kache도 보호 볼륨의 사용자 store를 열 수 있다. wrapper는 유지하고 이 fixture의 store만 격리한다.
    command.env("KACHE_CACHE_DIR", probes.root.join("kache"));
    for (k, v) in envs {
        if let Some(v) = v {
            command.env(k, v);
        } else {
            command.env_remove(k);
        }
    }
    command.arg(&pidfile).arg(&image).arg(&binary).args(["agent", "--name", "labA"]);
    lab.spawn_command("spawn", command);
    let launcher_pid = wait_for("launcher PID", T, || std::fs::read_to_string(&pidfile).ok()?.parse::<u32>().ok());
    let launcher = Launcher { pid: launcher_pid, image: image.clone() };
    wait_for("agent 재시작 준비", T, || {
        assert!(process::image(launcher.pid).is_some(), "준비 전에 시험 launcher가 종료됐다");
        node.state.join("service-restart/agent.json").exists().then_some(())
    });
    let first = record(&node);
    let worker = first["pid"].as_u64().unwrap() as u32;
    assert_eq!(first["launcher"], launcher_pid);
    assert_eq!(process::responsible(worker), Some(launcher_pid));
    let mut watch = lab::watch::HostWatch::new(&node, T);
    watch.wait("agent 연결", |w| w.connected("labA").then_some(()));
    let old = node.new_pane(None, "labA", Some("existing"), &[], probes.root.to_str());
    node.ok(&["pane", "run", &old, "printf 'old-%s\\n' ready"]);
    node.wait_output(&old, "old-ready", T);
    let old_host = node.session_host_pid(&old).unwrap();
    assert_eq!(pane(&node, &old)["responsibility"]["state"], "launcher");
    probe(&node, &old, &probes, launcher.pid, "before");
    let replacement = lab.root.join("next");
    lab::variant_binary(Path::new(lab::BIN), &replacement, "aaa-restart-second");
    assert_ne!(util::sha256_file(&binary).ok(), util::sha256_file(&replacement).ok());
    assert!(Command::new("mv").arg(&replacement).arg(&binary).status().unwrap().success());
    assert_eq!(process::parent(worker), Some(launcher.pid));
    // 실제 service restart 입구를 쓰되 launchctl만 이 실험실 launcher 하나에 연결한다.
    let control = lab.root.join("control");
    std::fs::create_dir(&control).unwrap();
    let launchctl = control.join("launchctl");
    let calls = lab.root.join("restart.calls");
    let script = format!(
        "#!/bin/sh\ncase \"$1\" in\nprint) printf 'state = running\\npid = {}\\n' ;;\nkill) [ \"$2\" = SIGUSR2 ] || exit 9; echo \"$@\" >> {}; kill -USR2 {} ;;\n*) exit 8 ;;\nesac\n",
        launcher.pid,
        lab::shell_word(&calls.to_string_lossy()),
        launcher.pid
    );
    std::fs::write(&launchctl, script).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&launchctl, std::fs::Permissions::from_mode(0o755)).unwrap();
    let home = lab.root.join("home");
    let plist = home.join("Library/LaunchAgents/dev.aaa.agent.plist");
    std::fs::create_dir_all(plist.parent().unwrap()).unwrap();
    std::fs::write(plist, "<plist/>").unwrap();
    let path = format!("{}:{}", control.display(), std::env::var("PATH").unwrap());
    let ready_path = node.state.join("service-restart/agent.json");
    let mut stale = first.clone();
    stale["created"] = serde_json::json!(first["created"].as_u64().unwrap() + 1);
    std::fs::write(&ready_path, serde_json::to_vec(&stale).unwrap()).unwrap();
    let refused = node.in_terminal_env(
        &["service", "restart", "--role", "agent"],
        &[("HOME", home.to_str().unwrap()), ("PATH", &path)],
        &[("yes를 입력", "yes")],
        T,
    );
    assert!(refused.contains("실제 프로세스가 다르다"), "{refused}");
    assert!(!calls.exists(), "옛 PID 신원에 신호를 보냈다");
    let mut pinned = first.clone();
    pinned["approved_executable"] = serde_json::json!([binary, "0".repeat(64)]);
    std::fs::write(&ready_path, serde_json::to_vec(&pinned).unwrap()).unwrap();
    let refused = node.in_terminal_env(
        &["service", "restart", "--role", "agent"],
        &[("HOME", home.to_str().unwrap()), ("PATH", &path)],
        &[("yes를 입력", "yes")],
        T,
    );
    assert!(refused.contains("새 install 승인"), "{refused}");
    assert!(!calls.exists(), "승인과 다른 실행 파일에 재시작 신호를 보냈다");
    std::fs::write(&ready_path, serde_json::to_vec(&first).unwrap()).unwrap();
    let out = node.in_terminal_env(
        &["service", "restart", "--role", "agent"],
        &[("HOME", home.to_str().unwrap()), ("PATH", &path)],
        &[("yes를 입력", "yes")],
        T,
    );
    assert!(out.contains("다시 띄웠다"), "{out}");
    assert_eq!(std::fs::read_to_string(calls).unwrap().lines().count(), 1);

    let ready = std::panic::catch_unwind(|| {
        wait_for("새 worker 준비 기록", T, || {
            let next = record(&node);
            (next["nonce"] != first["nonce"]).then_some(next)
        });
    });
    if let Err(panic) = ready {
        // 시한·판정은 그대로 실패시킨다. 정리 전에 아직 멈춘 자기 worker의 스택을 보존한다.
        let paths = [("worker", worker, &binary), ("launcher", launcher.pid, &image)];
        for (label, pid, expected) in paths {
            let observed = process::image(pid);
            eprintln!("restart {label} pid={pid} image={observed:?} parent={:?}", process::parent(pid));
            if observed.and_then(|p| p.canonicalize().ok()) == expected.canonicalize().ok() {
                let out = Command::new("sample")
                    .arg(pid.to_string())
                    .args(["1", "1", "-file"])
                    .arg(probes.archive.join(format!("{label}.sample.txt")))
                    .output();
                eprintln!("restart {label} sample: {out:?}");
            }
        }
        std::panic::resume_unwind(panic);
    }
    let next = record(&node);
    assert_eq!(next["pid"], worker);
    assert_eq!(next["launcher"], launcher.pid);
    wait_for("기존 pane 재연결", T, || {
        let p = node.json(&["pane", "list"]).ok()?;
        p["panes"].as_array()?.iter().any(|p| p["id"] == old && p["connection"] == "connected").then_some(())
    });
    assert_eq!(node.session_host_pid(&old), Some(old_host));
    assert_eq!(process::responsible(old_host), Some(launcher.pid));
    node.ok(&["pane", "run", &old, "printf 'old-pane-alive\\n'"]);
    node.wait_output(&old, "old-pane-alive", T);
    let new = node.new_pane(None, "labA", Some("new"), &[], probes.root.to_str());
    node.ok(&["pane", "run", &new, "printf 'new-%s\\n' ready"]);
    node.wait_output(&new, "new-ready", T);
    assert_eq!(pane(&node, &old)["responsibility"]["state"], "launcher");
    assert_eq!(pane(&node, &new)["responsibility"]["state"], "launcher");
    assert_eq!(process::responsible(node.session_host_pid(&new).unwrap()), Some(launcher.pid));
    assert_eq!(std::fs::read(&image).unwrap(), launcher_before);
    probe(&node, &old, &probes, launcher.pid, "after-existing");
    probe(&node, &new, &probes, launcher.pid, "after-new");
    // watcher를 먼저 놓고 실험실이 아는 launcher 하나만 종료한다.
    drop(watch);
    drop(launcher);
    assert!(lab.wait_exit("spawn", T).success());
    // 옛 kickstart처럼 launcher가 사라진 상황: pane은 보존하고 새 agent가 실제 경로를 다시 관찰한다.
    lab.spawn("plain-agent", &node, &["agent", "--name", "labA"]);
    wait_for("끊긴 기존 pane 경고", T, || {
        let p = node.json(&["pane", "list"]).ok()?;
        let entries = p["panes"].as_array()?;
        [old.as_str(), new.as_str()]
            .iter()
            .all(|id| {
                entries.iter().any(|p| {
                    p["id"] == *id && p["connection"] == "connected" && p["responsibility"]["state"] == "detached"
                })
            })
            .then_some(())
    });
    assert_eq!(node.session_host_pid(&old), Some(old_host));
    let listed = node.run(&["pane", "list"]);
    assert!(listed.status.success(), "{listed:?}");
    // pane 명령은 --json 생략 때도 JSON 한 줄이다. 기존 계약을 지키며 책임 관측값을 싣는다.
    let listed: Value = serde_json::from_slice(&listed.stdout).unwrap();
    assert!(
        listed["panes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["id"] == old && p["responsibility"]["state"] == "detached")
    );
    node.ok(&["pane", "run", &old, "printf 'detached-pane-alive\\n'"]);
    node.wait_output(&old, "detached-pane-alive", T);
}

//! L1 실제 CLI·TLS agent·사본 실행 파일. launchctl만 기록하는 모형이며 운영 서비스에 닿지 않는다.
#![cfg(target_os = "macos")]
use super::*;

// 실제 TLS agent 앞의 시험 소유 byte proxy. 두 번째 조각 응답을 붙잡고 다른 link/입력을 관찰한다.
struct TransferProxy {
    addr: String,
    reached: std::sync::mpsc::Receiver<()>,
    release: std::sync::Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
impl TransferProxy {
    fn new(upstream: String) -> Self {
        use std::io::{Read, Write};
        use std::net::{TcpListener, TcpStream};
        use std::sync::{
            Arc, Condvar, Mutex,
            atomic::{AtomicBool, Ordering},
            mpsc,
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let used = Arc::new(AtomicBool::new(false));
        let (notify, reached) = mpsc::channel();
        let (thread_release, thread_stop) = (release.clone(), stop.clone());
        std::thread::spawn(move || {
            for client in listener.incoming() {
                if thread_stop.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(mut client) = client else { break };
                let Ok(mut server) = TcpStream::connect(&upstream) else { continue };
                let _ = client.set_nodelay(true);
                let _ = server.set_nodelay(true);
                let (mut read_server, mut write_client) = (server.try_clone().unwrap(), client.try_clone().unwrap());
                std::thread::spawn(move || {
                    let _ = std::io::copy(&mut read_server, &mut write_client);
                    let _ = write_client.shutdown(std::net::Shutdown::Both);
                });
                let (release, used, notify) = (thread_release.clone(), used.clone(), notify.clone());
                std::thread::spawn(move || {
                    let mut forwarded = 0;
                    let mut bytes = [0; 16 * 1024];
                    while let Ok(n) = client.read(&mut bytes) {
                        if n == 0 {
                            break;
                        }
                        if forwarded >= 2 * aaa_proto::DEPLOY_CHUNK && !used.swap(true, Ordering::SeqCst) {
                            let _ = notify.send(());
                            let (lock, changed) = &*release;
                            let mut open = lock.lock().unwrap();
                            while !*open {
                                open = changed.wait(open).unwrap();
                            }
                        }
                        if server.write_all(&bytes[..n]).is_err() {
                            break;
                        }
                        forwarded += n;
                    }
                    let _ = server.shutdown(std::net::Shutdown::Both);
                });
            }
        });
        Self { addr, reached, release, stop }
    }
    fn release(&self) {
        let (lock, changed) = &*self.release;
        *lock.lock().unwrap() = true;
        changed.notify_all();
    }
}
impl Drop for TransferProxy {
    fn drop(&mut self) {
        self.release();
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        let _ = std::net::TcpStream::connect(&self.addr);
    }
}

#[test]
#[cfg(target_os = "macos")]
fn rollout_preflights_all_targets_and_resumes_same_jobs_after_host_restart() {
    use sha2::{Digest, Sha256};
    let mut w = world();
    w.add_remote();
    let b = w.lab.node("b", None);
    let proxy = TransferProxy::new(w.addr.clone());
    let dir = w.lab.root.join("rollout-bin");
    std::fs::create_dir_all(dir.join("a")).unwrap();
    std::fs::create_dir_all(dir.join("b")).unwrap();
    let a_exe = dir.join("a/aaa");
    let b_exe = dir.join("b/aaa");
    let candidate_dir = dir.join("build");
    std::fs::create_dir_all(&candidate_dir).unwrap();
    let candidate = candidate_dir.join("aaa");
    for path in [&a_exe, &b_exe, &candidate] {
        assert!(Command::new("cp").args([lab::BIN, path_str(path)]).status().unwrap().success());
    }
    // 실행 파일을 쓰는 FD는 이 자식에서만 열어 병렬 시험의 fork에 물려주지 않는다.
    assert!(
        Command::new("python3")
            .args(["-c", "import sys; open(sys.argv[1], 'ab').write(b'rollout-candidate')", path_str(&candidate)])
            .status()
            .unwrap()
            .success()
    );
    let original = hex::encode(Sha256::digest(std::fs::read(&a_exe).unwrap()));
    let sha = hex::encode(Sha256::digest(std::fs::read(&candidate).unwrap()));
    std::fs::write(candidate_dir.join("provenance.json"), serde_json::json!({"sha256":sha}).to_string()).unwrap();
    let service_home = dir.join("home");
    let service_bin = dir.join("bin");
    let service_log = dir.join("calls");
    std::fs::create_dir_all(service_home.join("Library/LaunchAgents")).unwrap();
    std::fs::create_dir_all(&service_bin).unwrap();
    for role in ["host", "agent"] {
        std::fs::write(service_home.join(format!("Library/LaunchAgents/dev.aaa.{role}.plist")), "<plist/>").unwrap();
    }
    let launchctl = service_bin.join("launchctl");
    std::fs::write(
        &launchctl,
        r#"#!/bin/sh
case "$1" in
print) printf 'state = running\npid = 100\n' ;;
kickstart) printf '%s %s\n' "$ROLLOUT_MACHINE" "$3" >> "$ROLLOUT_CALLS" ;;
*) exit 1 ;;
esac
"#,
    )
    .unwrap();
    std::fs::set_permissions(&launchctl, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let path = format!("{}:{}", service_bin.display(), std::env::var("PATH").unwrap());
    let agent_cmd = |node: &Node, exe: &Path, machine: &str, host: &str, name: &str| {
        let mut c = node.cmd_bin(exe, &["agent", "--host", host, "--name", name]);
        c.env("HOME", &service_home)
            .env("PATH", &path)
            .env("ROLLOUT_CALLS", &service_log)
            .env("ROLLOUT_MACHINE", machine);
        c
    };
    let config = w.h.cfg.join("host.toml");
    std::fs::write(&config, std::fs::read_to_string(&config).unwrap().replace("127.0.0.1:0", &w.addr)).unwrap();
    w.lab.kill("host");
    w.lab.kill("agentA");
    w.lab.kill("agentB");
    w.lab.spawn_command("rollout-host", w.h.cmd_bin(&a_exe, &["host"]));
    wait_for("사본 host 준비", T, || w.h.json(&["status"]).ok());
    w.lab.spawn_command("rollout-a", agent_cmd(&w.h, &a_exe, "a", "local", "labA"));
    w.lab.spawn_command("rollout-b", agent_cmd(&b, &b_exe, "b", &proxy.addr, "labB"));
    wait_for("사본 agent 둘 연결", T, || {
        (w.lab.log("rollout-a").contains("host에 연결했다")
            && w.lab.log("rollout-b").contains("host에 연결했다")
            && w.h.connected("labA")
            && w.h.connected("labB"))
        .then_some(())
    });
    let coordinator = w.h.ok(&["project", "open", "demo", "--profile", "fake"]);
    let pane = coordinator["pane"]["id"].as_str().unwrap().to_string();
    w.wait_screen(&pane, "fake>");
    let pid = w.h.pane(&pane)["pid"].clone();
    let mut n = 0;
    let mut probe = |w: &World, args: &str| {
        n += 1;
        let prefix = dir.join(format!("probe-{n}"));
        let script = w.task(&format!("rollout-probe-{n}.sh"), &format!("'{}' --json {args} > '{}.out' 2> '{}.err'\nprintf '%s' \"$?\" > '{}.rc'\necho rollout-probe-{n}-done\n", lab::BIN, prefix.display(), prefix.display(), prefix.display()));
        w.h.ok(&["pane", "run", &pane, &format!("post-probe {script}")]);
        w.h.wait_output(&pane, &format!("rollout-probe-{n}-done"), T);
        let read = |suffix| std::fs::read_to_string(prefix.with_extension(suffix)).unwrap();
        (read("rc"), read("out"), read("err"))
    };
    let start = |sha: &str| format!("deploy start --from '{}' --sha256 {sha}", candidate.display());
    let (rc, _, err) = probe(&w, &start(&sha));
    assert_eq!(rc, "1");
    assert!(err.contains("coordinator_deploy on"), "{err}");
    // pane 밖 CLI·위장한 coordinator는 이번 L1에서 실행 권한을 얻지 않는다.
    let spoof =
        w.h.cmd(&["deploy", "start", "--from", path_str(&candidate), "--sha256", &sha])
            .env("AAA_SESSION_ID", &pane)
            .output()
            .unwrap();
    assert!(!spoof.status.success(), "{spoof:?}");
    let enabled = w.h.in_terminal(&["safety", "set", "demo", "coordinator_deploy", "on"], &[("바꿀까?", "y")], T);
    assert!(enabled.contains("coordinator_deploy = on"), "{enabled}");
    let (rc, out, err) = probe(&w, &start(&"0".repeat(64)));
    assert_eq!(rc, "0", "{out} {err}");
    let r: Value = serde_json::from_str(&out).unwrap();
    let bad_id = r["rollout"]["id"].as_str().unwrap();
    let (rc, out, err) = probe(&w, &format!("deploy status {bad_id} --wait"));
    assert_eq!(rc, "1", "{out} {err}");
    let stopped: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(stopped["rollout"]["state"], "stopped");
    assert!(stopped["rollout"]["error"].as_str().unwrap().contains("SHA-256"));
    for exe in [&a_exe, &b_exe] {
        assert_eq!(hex::encode(Sha256::digest(std::fs::read(exe).unwrap())), original);
    }
    assert!(!service_log.exists(), "사전 검사 실패인데 재시작했다");

    // dry-run은 SHA를 확인한 staging만 실행해야 한다. 후보가 자신을 검사하는 동안 원본을 바꿔도 재실행하지 않는다.
    let shim = dir.join("mutable-candidate");
    let unverified = dir.join("unverified-executed");
    let payload = format!("#!/bin/sh\necho executed > '{}'\nprintf 'aaa 0.1.2\\n'\n", unverified.display());
    let body = format!(
        "#!/bin/sh\nif [ \"$1\" = --version ]; then\ncat > '{}' <<'PAYLOAD'\n{payload}PAYLOAD\nprintf 'aaa 0.1.2\\n'\nfi\nexit 0\n",
        shim.display()
    );
    std::fs::write(&shim, &body).unwrap();
    std::fs::set_permissions(&shim, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let shim_sha = hex::encode(Sha256::digest(body.as_bytes()));
    let (rc, out, err) = probe(&w, &format!("deploy start --from '{}' --sha256 {shim_sha}", shim.display()));
    assert_eq!(rc, "0", "{out} {err}");
    let started: Value = serde_json::from_str(&out).unwrap();
    let shim_id = started["rollout"]["id"].as_str().unwrap();
    let (rc, out, err) = probe(&w, &format!("deploy status {shim_id} --wait"));
    assert_eq!(rc, "1", "{out} {err}");
    assert!(!unverified.exists(), "SHA를 검증하지 않은 원본 후보를 실행했다");
    for exe in [&a_exe, &b_exe] {
        assert_eq!(hex::encode(Sha256::digest(std::fs::read(exe).unwrap())), original);
    }

    // L2: 빌드 머신 한 곳의 provenance 후보를 보낸다. 대상에는 미리 복사하지 않는다.
    let (rc, out, err) = probe(&w, &format!("deploy start --file 'labB:{}' --sha256 {sha}", candidate.display()));
    assert_eq!(rc, "0", "{out} {err}");
    let started: Value = serde_json::from_str(&out).unwrap();
    let r = &started["rollout"];
    let id = r["id"].as_str().unwrap().to_string();
    let before_ids: Vec<_> = r["steps"].as_array().unwrap().iter().map(|s| s["job_id"].clone()).collect();
    let transfer_started = Instant::now();
    let partial = w.h.state.join(format!("agent/deployments/{id}.part"));
    let total = std::fs::metadata(&candidate).unwrap().len();
    proxy.reached.recv_timeout(T).expect("시험 소켓이 두 번째 후보 조각을 붙잡았다");
    let offset = std::fs::metadata(&partial).unwrap().len();
    assert!(offset > 0 && offset < total);
    // 같은 link의 세션 입력/출력도 조각 사이에 처리된다.
    let control = Instant::now();
    w.h.ok(&["pane", "run", &pane, "전송중제어"]);
    w.h.wait_output(&pane, "got: 전송중제어", T);
    let control_ms = control.elapsed().as_millis();
    w.lab.kill("rollout-host");
    assert_eq!(std::fs::metadata(&partial).unwrap().len(), offset, "잡아 둔 소켓 사건까지 저장 길이는 고정이다");
    proxy.release();
    w.lab.spawn_command("rollout-host-transfer-resume", w.h.cmd_bin(&a_exe, &["host"]));
    wait_for("전송 재개 host의 agent 연결", T, || {
        (w.h.connected("labA") && w.h.connected("labB")).then_some(())
    });
    eprintln!("L2 transfer resume bytes={offset}/{total}, pane input/output={control_ms}ms");
    let mut transfer_events = Events::connect(&w.h);
    let next_call = |index: usize| {
        wait_for("typed 서비스 재시작 기록", T, || {
            std::fs::read_to_string(&service_log).ok()?.lines().nth(index).map(str::to_string)
        })
    };
    let wait_out = dir.join("wait.out");
    let wait_err = dir.join("wait.err");
    let wait_rc = dir.join("wait.rc");
    let wait_script = w.task(
        "rollout-wait.sh",
        &format!(
            "'{}' --json deploy status {id} --wait > '{}' 2> '{}'\nprintf '%s' \"$?\" > '{}'\necho rollout-wait-done\n",
            lab::BIN,
            wait_out.display(),
            wait_err.display(),
            wait_rc.display()
        ),
    );
    w.h.ok(&["pane", "run", &pane, &format!("post-probe {wait_script}")]);
    wait_for("대기 CLI가 재시작 전 host 응답을 받음", T, || {
        std::fs::read_to_string(&wait_err).ok().filter(|s| s.contains(&format!("rollout {id}:")))
    });
    // 전송은 실제 진행 사건으로 기다리고, 그 뒤 서비스 재시작의 기존 대기 한도를 적용한다.
    // 같은 사건이나 바이트가 늘지 않은 사건으로 대기 시간을 새로 시작하지 않는다.
    let transfer_progress = || {
        let stages: Vec<Value> = [&w.h, &b]
            .into_iter()
            .filter_map(|node| {
                serde_json::from_slice(
                    &std::fs::read(node.state.join(format!("agent/deployments/{id}.stage.json"))).ok()?,
                )
                .ok()
            })
            .collect();
        // 중간 offset은 실제 파일 길이가 정본이고, metadata는 EOF 검증 때 완료로 바뀐다.
        let bytes: u64 = [&w.h, &b]
            .into_iter()
            .filter_map(|node| {
                ["part", "bin"].into_iter().find_map(|suffix| {
                    std::fs::metadata(node.state.join(format!("agent/deployments/{id}.{suffix}"))).ok().map(|m| m.len())
                })
            })
            .sum();
        let complete = stages.len() == 2 && stages.iter().all(|s| s["complete"] == true && s["offset"] == total);
        (bytes, complete)
    };
    let mut from = 0;
    let mut progress = transfer_progress();
    while !progress.1 {
        let (index, next) =
            transfer_events.wait_from(from, "후보 전송 진행 또는 검증 완료", T, |event| match event {
                FrontEvent::Flow(f) if f.kind == "rollout_changed" && f.detail == id => {
                    let next = transfer_progress();
                    (next.0 > progress.0 || next.1).then_some(next)
                }
                _ => None,
            });
        from = index + 1;
        progress = next;
    }
    assert!(next_call(0).ends_with("/dev.aaa.host"));
    let transfer_ms = transfer_started.elapsed().as_millis();
    eprintln!(
        "L2 all target transfer/preflight to host restart={transfer_ms}ms, bytes={total}, chunk={}",
        aaa_proto::DEPLOY_CHUNK
    );
    // 교체는 agent 상태 폴더의 검증 후보만 쓴다. 원본을 치워도 두 머신 교체가 끝나야 한다.
    let saved = candidate_dir.join("saved-aaa");
    std::fs::rename(&candidate, &saved).unwrap();
    // 첫 교체 전 둘 모두 검사 완료. host 재시작 전 remote 파일은 아직 그대로다.
    assert_eq!(hex::encode(Sha256::digest(std::fs::read(&b_exe).unwrap())), original);
    let host_job = w.h.state.join(format!("agent/deployments/{}.json", before_ids[5].as_str().unwrap()));
    wait_for("host restart 작업 영수증 완료", T, || {
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&host_job).ok()?).ok()?;
        (v["state"] == "completed").then_some(())
    });
    w.lab.kill("rollout-host-transfer-resume");
    w.lab.kill("rollout-a");
    w.lab.spawn_command("rollout-host-new", w.h.cmd_bin(&a_exe, &["host"]));
    // CLI가 새 host에 먼저 붙어도, peer PID 신원을 local hello 이전에 고정하면 안 된다.
    wait_for("wait handshake가 local 자료를 기다림", T, || {
        w.lab.log("rollout-host-new").contains("배포 상태 조회: 로컬 agent 자료 기다림").then_some(())
    });
    w.lab.spawn_command("rollout-a-hello", agent_cmd(&w.h, &a_exe, "a", "local", "labA"));
    wait_for("새 host와 agent 둘 재연결", T, || (w.h.connected("labA") && w.h.connected("labB")).then_some(()));
    let restart = next_call(1);
    assert!(
        restart.starts_with("a ") && restart.ends_with("/dev.aaa.agent"),
        "host 다음에는 로컬 agent를 한 번만 다시 띄운다: {restart}"
    );
    w.lab.kill("rollout-a-hello");
    w.lab.spawn_command("rollout-a-new", agent_cmd(&w.h, &a_exe, "a", "local", "labA"));
    let restart = next_call(2);
    assert!(restart.starts_with("b ") && restart.ends_with("/dev.aaa.agent"), "{restart}");
    w.lab.kill("rollout-b");
    w.lab.spawn_command("rollout-b-new", agent_cmd(&b, &b_exe, "b", &proxy.addr, "labB"));
    wait_for("새 agent 둘 연결", T, || {
        (w.lab.log("rollout-a-new").contains("host에 연결했다")
            && w.lab.log("rollout-b-new").contains("host에 연결했다")
            && w.h.connected("labA")
            && w.h.connected("labB"))
        .then_some(())
    });
    w.h.wait_output(&pane, "rollout-wait-done", T);
    assert_eq!(std::fs::read_to_string(&wait_rc).unwrap(), "0", "{}", std::fs::read_to_string(&wait_err).unwrap());
    let done: Value = serde_json::from_str(&std::fs::read_to_string(&wait_out).unwrap()).unwrap();
    let r = &done["rollout"];
    assert_eq!(r["state"], "done");
    assert_eq!(r["cursor"], 9);
    for transfer in &r["steps"].as_array().unwrap()[..2] {
        assert_eq!(transfer["phase"], "transfer");
        assert_eq!(transfer["transferred_bytes"], total);
        assert_eq!(transfer["total_bytes"], total);
    }
    std::fs::rename(&saved, &candidate).unwrap();
    let after_ids: Vec<_> = r["steps"].as_array().unwrap().iter().map(|s| s["job_id"].clone()).collect();
    assert_eq!(before_ids, after_ids, "재개 시 job ID가 바뀌었다");
    assert!(r["steps"].as_array().unwrap().iter().all(|s| s["completed"] == true));
    assert!(r["finish"].as_str().unwrap().contains("project_files"));
    assert_eq!(w.h.pane(&pane)["pid"], pid, "coordinator pane을 바꾸지 않는다");
    assert_eq!(std::fs::read_to_string(&service_log).unwrap().lines().count(), 3, "재시작을 중복 실행했다");
    for exe in [&a_exe, &b_exe] {
        assert_eq!(hex::encode(Sha256::digest(std::fs::read(exe).unwrap())), sha);
    }
    // 사전 검사 뒤 후보가 바뀌어도 실제 교체가 다시 SHA를 대조한다.
    let (rc, out, err) = probe(&w, &start(&sha));
    assert_eq!(rc, "0", "{out} {err}");
    let second: Value = serde_json::from_str(&out).unwrap();
    let second_id = second["rollout"]["id"].as_str().unwrap();
    assert!(next_call(3).ends_with("/dev.aaa.host"));
    w.lab.kill("rollout-host-new");
    w.lab.spawn_command("rollout-host-second", w.h.cmd_bin(&a_exe, &["host"]));
    let restart = next_call(4);
    assert!(restart.starts_with("a ") && restart.ends_with("/dev.aaa.agent"), "{restart}");
    assert!(
        Command::new("python3")
            .args(["-c", "import sys; open(sys.argv[1], 'ab').write(b'changed-after-preflight')", path_str(&candidate)])
            .status()
            .unwrap()
            .success()
    );
    w.lab.kill("rollout-a-new");
    w.lab.spawn_command("rollout-a-second", agent_cmd(&w.h, &a_exe, "a", "local", "labA"));
    wait_for("새 로컬 agent 연결", T, || {
        (w.lab.log("rollout-a-second").contains("host에 연결했다") && w.h.connected("labA")).then_some(())
    });
    let (rc, out, err) = probe(&w, &format!("deploy status {second_id} --wait"));
    assert_eq!(rc, "1", "{out} {err}");
    let stopped: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(stopped["rollout"]["state"], "stopped");
    assert_eq!(stopped["rollout"]["cursor"], 5, "둘째 머신 교체 실패에서 멈춘다");
    assert!(stopped["rollout"]["error"].as_str().unwrap().contains("SHA-256"));
    assert_eq!(
        std::fs::read_to_string(&service_log).unwrap().lines().count(),
        5,
        "실패한 remote를 다시 띄우면 안 된다"
    );
    for exe in [&a_exe, &b_exe] {
        assert_eq!(hex::encode(Sha256::digest(std::fs::read(exe).unwrap())), sha, "성공한 앞 머신을 보존한다");
    }
}

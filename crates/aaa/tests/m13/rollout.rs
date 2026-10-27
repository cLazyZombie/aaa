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

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scenario {
    Transfer,
    RemoteRecovery,
    SharedRecovery,
}

#[test]
fn rollout_transfer_preflight_and_resume_keep_the_same_jobs() {
    rollout_scenario(Scenario::Transfer);
}

#[test]
fn rollout_rejected_update_and_remote_recovery_keep_the_same_jobs() {
    rollout_scenario(Scenario::RemoteRecovery);
}

#[test]
fn rollout_shared_host_agent_recovery_and_inflight_cancel_preserve_baselines() {
    rollout_scenario(Scenario::SharedRecovery);
}

fn rollout_scenario(scenario: Scenario) {
    use sha2::{Digest, Sha256};
    // 웹을 연 host: 새 host 감시가 실행 신원과 함께 웹 응답(`/api/v1/me`)까지 본다(W6c)
    // 이 keeper는 실제 사본으로 시작한다. 곧바로 버릴 원본 host/agent 셋과 쓰지 않는 Git repo는 준비하지 않는다.
    let (mut w, _) = world_files("", true, Some(BRIEF_SETTLE_MS), false, |_| Vec::new());
    let dir = w.lab.root.join("rollout-bin");
    std::fs::create_dir_all(dir.join("a")).unwrap();
    std::fs::create_dir_all(dir.join("b")).unwrap();
    let a_exe = dir.join("a/aaa");
    let b_exe = dir.join("b/aaa");
    let candidate_dir = dir.join("build");
    std::fs::create_dir_all(&candidate_dir).unwrap();
    let candidate = candidate_dir.join("aaa");
    // 앞 시험의 배포를 다시 실행하지 않고, 각 시나리오 시작 때 필요한 실제 이미지로 준비한다.
    let initial = match scenario {
        Scenario::Transfer => [None, None],
        Scenario::RemoteRecovery => [Some("rollout-candidate"), Some("rollout-candidate")],
        Scenario::SharedRecovery => [Some("third-candidate"), Some("rollout-candidate")],
    };
    for (path, variant) in [(&a_exe, initial[0]), (&b_exe, initial[1])] {
        if let Some(variant) = variant {
            lab::variant_binary(Path::new(lab::BIN), path, variant);
        } else {
            assert!(Command::new("cp").args([lab::BIN, path_str(path)]).status().unwrap().success());
        }
    }
    lab::variant_binary(Path::new(lab::BIN), &candidate, "rollout-candidate");
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
    let (host_tag, a_tag, b_tag) = match scenario {
        Scenario::Transfer => ("rollout-host", "rollout-a", "rollout-b"),
        Scenario::RemoteRecovery => ("rollout-host-new", "rollout-a-new", "rollout-b-new"),
        Scenario::SharedRecovery => ("rollout-host-recovery-resume", "rollout-a-third", "rollout-b-restored"),
    };
    w.lab.spawn_command(host_tag, w.h.cmd_bin(&a_exe, &["host"]));
    let status = wait_for("사본 host 준비", T, || w.h.json(&["status"]).ok());
    w.addr = status["agent_listen"].as_str().unwrap().into();
    // 처음 실제 사본이 고른 agent port를 뒤의 모든 재시작에도 유지한다.
    let config = w.h.cfg.join("host.toml");
    let pinned = std::fs::read_to_string(&config)
        .unwrap()
        .replace("agent_listen = \"127.0.0.1:0\"", &format!("agent_listen = {:?}", w.addr));
    std::fs::write(&config, pinned).unwrap();
    w.enroll_remote();
    // 앞선 keeper와 같은 사본 agent 설정이다. 등록 신원은 상태 폴더에 그대로 있다.
    let b = w.lab.node("b", None);
    use std::io::Write;
    std::fs::OpenOptions::new()
        .append(true)
        .open(b.cfg.join("agent.toml"))
        .unwrap()
        .write_all(b"\n[reconnect]\nmax_backoff_s = 1\n")
        .unwrap();
    let proxy = TransferProxy::new(w.addr.clone());
    if scenario != Scenario::Transfer {
        proxy.release();
    }
    w.lab.spawn_command(a_tag, agent_cmd(&w.h, &a_exe, "a", "local", "labA"));
    w.lab.spawn_command(b_tag, agent_cmd(&b, &b_exe, "b", &proxy.addr, "labB"));
    // t-0578 부하 실패의 사본 프로세스 시작만 여유를 둔다. 이후 배포 단계별 30초는 유지한다.
    wait_for("사본 agent 둘 연결", lab::LOAD_TIMEOUT, || {
        (w.lab.log(a_tag).contains("host에 연결했다")
            && w.lab.log(b_tag).contains("host에 연결했다")
            && w.h.connected("labA")
            && w.h.connected("labB"))
        .then_some(())
    });
    w.h.ok(&["project", "create", "demo"]);
    w.h.ok(&["machine", "workspaces", "labA", "--allow", "작업 폴더"]);
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
    let db = aaa_host::db::Db::open(&w.h.state.join("host/host.db")).unwrap();
    // 기존 전체 흐름의 논리적 번호를 유지하되 이전 시험의 실제 서비스 실행은 반복하지 않는다.
    let prior_calls = match scenario {
        Scenario::Transfer => 0,
        Scenario::RemoteRecovery => 3,
        Scenario::SharedRecovery => 9,
    };
    let service_count = || std::fs::read_to_string(&service_log).unwrap().lines().count() + prior_calls;
    let next_call = |index: usize| {
        let index = index.checked_sub(prior_calls).expect("이번 시나리오의 서비스 호출");
        wait_for_diag(
            &format!("typed 서비스 재시작 기록 {index}"),
            T,
            || std::fs::read_to_string(&service_log).ok()?.lines().nth(index).map(str::to_string),
            || format!("서비스 기록: {:?}", std::fs::read_to_string(&service_log)),
        )
    };
    // 복구도 설치와 서비스 재시작을 나눈다. 옛 프로세스 기동부터 파일 대조·복구·서비스를 한 상한으로 재면
    // 큰 실행 파일을 복구 중인데 서비스가 멈췄다고 오판한다. 같은 표지의 반복은 상한을 다시 시작하지 않는다.
    let restored_file = |file: &Path| {
        let load = || -> Option<Value> { serde_json::from_slice(&std::fs::read(file).ok()?).ok() };
        let check = |v: &Value| {
            assert_ne!(v["phase"], "failed", "파일 복구 전에 감시 실패: {v}");
            assert_ne!(v["phase"], "cancelled", "파일 복구 전에 감시 취소: {v}");
        };
        wait_for_diag(
            "독립 감시의 파일 복구 시작",
            T,
            || {
                let v = load()?;
                check(&v);
                (v["restore_started"] == true).then_some(())
            },
            || format!("감시 기록: {:?}", load()),
        );
        wait_for_diag(
            "독립 감시의 파일 복구 완료 뒤 서비스 단계 진입",
            T,
            || {
                let v = load()?;
                check(&v);
                matches!(v["phase"].as_str(), Some("restarting" | "awaiting_old" | "restored")).then_some(())
            },
            || format!("감시 기록: {:?}", load()),
        );
    };
    // 서비스 대기 상한에 앞선 사전 검사·설치 시간을 섞지 않는다. 실제 단계 진행만 다음 대기로 넘어간다.
    // 같은 rollout의 같은 cursor/시작 표지를 되풀이하는 사건은 한도를 새로 시작하지 않는다.
    let job_started = |id: &str, job: &str| {
        // 재기동한 host가 아직 listen하기 전에도 이 helper를 부른다.
        // Front 준비와 그 뒤의 job 진행은 각각 기존 상한으로 기다린다.
        wait_for("rollout host의 Front 준비", T, || w.h.json(&["status"]).ok());
        let mut events = Events::connect(&w.h);
        let load = || -> aaa_proto::RolloutView {
            serde_json::from_str(&db.meta(&format!("rollout:{id}")).unwrap().unwrap()).unwrap()
        };
        let mut view = load();
        let target = view.steps.iter().position(|s| s.job_id == job).expect("기다릴 rollout job");
        let phase = view.steps[target].phase;
        loop {
            eprintln!(
                "rollout {id}: cursor={} started={} state={:?}, 기다릴 단계={phase:?} job={job}",
                view.cursor,
                view.steps.get(view.cursor as usize).is_some_and(|s| s.started_ms != 0),
                view.state
            );
            if view.steps[target].started_ms != 0 {
                return;
            }
            assert_eq!(view.state, aaa_proto::RolloutState::Running, "단계 진입 전에 멈췄다: {view:?}");
            let progress = (view.cursor, view.steps[view.cursor as usize].started_ms != 0);
            view = events.wait(&format!("rollout {id}의 {phase:?} 전 단계 진행"), T, |event| {
                let FrontEvent::Flow(f) = event else { return None };
                if f.kind != "rollout_changed" || f.detail != id {
                    return None;
                }
                let next = load();
                let advanced = next.cursor > progress.0
                    || (next.cursor == progress.0 && !progress.1 && next.steps[next.cursor as usize].started_ms != 0);
                (advanced || next.state != aaa_proto::RolloutState::Running).then_some(next)
            });
        }
    };
    let load_rollout = |id: &str| -> aaa_proto::RolloutView {
        serde_json::from_str(&db.meta(&format!("rollout:{id}")).unwrap().unwrap()).unwrap()
    };
    let phase_started = |id: &str, phase: aaa_proto::RolloutPhase| {
        let view = load_rollout(id);
        let job = &view.steps.iter().rfind(|s| s.phase == phase).expect("기다릴 rollout 단계").job_id;
        job_started(id, job);
    };
    let local_restart_started = |id: &str| {
        let view = load_rollout(id);
        let host = &view.steps.iter().find(|s| s.phase == aaa_proto::RolloutPhase::HostRestart).unwrap().machine;
        let mut steps =
            view.steps.iter().filter(|s| s.phase == aaa_proto::RolloutPhase::AgentRestart && &s.machine == host);
        let job = &steps.next().expect("로컬 agent 재시작 단계").job_id;
        assert!(steps.next().is_none(), "로컬 agent 재시작 단계는 하나다");
        job_started(id, job);
    };
    let digest = |p: &Path| hex::encode(Sha256::digest(std::fs::read(p).unwrap()));
    let copy = |from: &Path, to: &Path| {
        assert!(Command::new("cp").args([path_str(from), path_str(to)]).status().unwrap().success());
    };
    let variant = |p: &Path, suffix: &str| lab::variant_binary(Path::new(lab::BIN), p, suffix);
    if scenario == Scenario::Transfer {
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
        let cancelled = w.h.in_terminal(
            &["--json", "deploy", "start", "--from", path_str(&candidate), "--sha256", &sha],
            &[("yes를 입력한다:", "no")],
            T,
        );
        assert!(cancelled.contains("취소"), "{cancelled}");
        assert_eq!(db.meta("rollout:active").unwrap(), None, "no 확인으로 배포를 만들지 않는다");
        let non_tty = w.h.cmd(&["deploy", "start", "--from", path_str(&candidate), "--sha256", &sha]).output().unwrap();
        assert!(!non_tty.status.success(), "터미널 없는 CLI가 시작했다");
        assert!(String::from_utf8_lossy(&non_tty.stderr).contains("표준 입력이 터미널"), "{non_tty:?}");
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
        let off = w.h.in_terminal(&["safety", "set", "demo", "coordinator_deploy", "off"], &[("바꿀까?", "y")], T);
        assert!(off.contains("coordinator_deploy = off"), "{off}");
        let from = format!("labB:{}", candidate.display());
        let out = w.h.in_terminal(
            &["--json", "deploy", "start", "--file", &from, "--sha256", &sha],
            &[("yes를 입력한다:", "yes")],
            T,
        );
        let started: Value = serde_json::from_str(out[out.find('{').expect("사람 시작의 JSON 응답")..].trim()).unwrap();
        assert!(started["rollout"]["actor"].as_str().unwrap().starts_with("person:"), "{out}");
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
        // 전송은 실제 진행 사건으로 기다리고, 사전 검사·설치 뒤 서비스 단계에 기존 대기 한도를 적용한다.
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
                        std::fs::metadata(node.state.join(format!("agent/deployments/{id}.{suffix}")))
                            .ok()
                            .map(|m| m.len())
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
        phase_started(&id, aaa_proto::RolloutPhase::HostRestart);
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
        // 실행 신원은 맞지만 웹이 답하지 않는 새 host(W6c): 감시는 그 웹 주소를 기다리고 rollout은 다음 단계로 가지 않는다
        let mut silent = w.h.cmd_bin(&a_exe, &["host"]);
        silent.env("AAA_TEST_WEB_HOLD_ME", "1");
        w.lab.spawn_command("rollout-host-web-silent", silent);
        // CLI가 새 host에 먼저 붙어도, peer PID 신원을 local hello 이전에 고정하면 안 된다.
        wait_for("wait handshake가 local 자료를 기다림", T, || {
            w.lab.log("rollout-host-web-silent").contains("배포 상태 조회: 로컬 agent 자료 기다림").then_some(())
        });
        w.lab.spawn_command("rollout-a-hello", agent_cmd(&w.h, &a_exe, "a", "local", "labA"));
        let host_guard_file =
            w.h.state.join(format!("agent/deployments/{}.guard.json", before_ids[5].as_str().unwrap()));
        let silent_web = w.h.ok(&["status"])["web_listen"].as_str().unwrap().to_string();
        let waiting = wait_for("감시가 새 host의 웹 응답을 기다림", T, || {
            let v: Value = serde_json::from_slice(&std::fs::read(&host_guard_file).ok()?).ok()?;
            (v["pending_web"] == silent_web.as_str()).then_some(v)
        });
        assert_eq!(waiting["phase"], "awaiting_new", "웹이 답하지 않는데 새 판 준비로 넘겼다");
        // 웹이 답하는 같은 판으로 다시 띄우면 그 주소를 확인하고 이어 간다(되돌림은 시한 뒤: 유닛 `a_new_host_is_ready_only_after_its_web_answers…`)
        w.lab.kill("rollout-host-web-silent");
        w.lab.spawn_command("rollout-host-new", w.h.cmd_bin(&a_exe, &["host"]));
        wait_for("새 host와 agent 둘 재연결", T, || {
            (w.h.connected("labA") && w.h.connected("labB")).then_some(())
        });
        local_restart_started(&id);
        let restart = next_call(1);
        assert!(
            restart.starts_with("a ") && restart.ends_with("/dev.aaa.agent"),
            "host 다음에는 로컬 agent를 한 번만 다시 띄운다: {restart}"
        );
        w.lab.kill("rollout-a-hello");
        w.lab.spawn_command("rollout-a-new", agent_cmd(&w.h, &a_exe, "a", "local", "labA"));
        phase_started(&id, aaa_proto::RolloutPhase::AgentRestart);
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
        assert_eq!(service_count(), 3, "재시작을 중복 실행했다");
        for exe in [&a_exe, &b_exe] {
            assert_eq!(hex::encode(Sha256::digest(std::fs::read(exe).unwrap())), sha);
        }
        assert!(r["actor"].as_str().unwrap().starts_with("person:"), "사람 승인을 host 재시작 뒤에도 보존한다");
        let enabled = w.h.in_terminal(&["safety", "set", "demo", "coordinator_deploy", "on"], &[("바꿀까?", "y")], T);
        assert!(enabled.contains("coordinator_deploy = on"), "{enabled}");
        let person_status = w.h.in_terminal(&["--json", "deploy", "status", &id], &[], T);
        let person_view: Value =
            serde_json::from_str(person_status[person_status.find('{').unwrap()..].trim()).unwrap();
        assert_eq!(person_view["rollout"]["id"], id);
        assert_eq!(person_view["rollout"]["state"], "done", "{person_status}");
        return;
    }
    let enabled = w.h.in_terminal(&["safety", "set", "demo", "coordinator_deploy", "on"], &[("바꿀까?", "y")], T);
    assert!(enabled.contains("coordinator_deploy = on"), "{enabled}");
    if scenario == Scenario::RemoteRecovery {
        // 사전 검사 뒤 후보가 바뀌어도 실제 교체가 다시 SHA를 대조한다.
        let (rc, out, err) = probe(&w, &start(&sha));
        assert_eq!(rc, "0", "{out} {err}");
        let second: Value = serde_json::from_str(&out).unwrap();
        let second_id = second["rollout"]["id"].as_str().unwrap();
        phase_started(second_id, aaa_proto::RolloutPhase::HostRestart);
        assert!(next_call(3).ends_with("/dev.aaa.host"));
        w.lab.kill("rollout-host-new");
        w.lab.spawn_command("rollout-host-second", w.h.cmd_bin(&a_exe, &["host"]));
        local_restart_started(second_id);
        let restart = next_call(4);
        assert!(restart.starts_with("a ") && restart.ends_with("/dev.aaa.agent"), "{restart}");
        assert!(
            Command::new("python3")
                .args([
                    "-c",
                    "import sys; open(sys.argv[1], 'ab').write(b'changed-after-preflight')",
                    path_str(&candidate)
                ])
                .status()
                .unwrap()
                .success()
        );
        w.lab.kill("rollout-a-new");
        w.lab.spawn_command("rollout-a-second", agent_cmd(&w.h, &a_exe, "a", "local", "labA"));
        wait_for("새 로컬 agent 연결", T, || {
            (w.lab.log("rollout-a-second").contains("host에 연결했다") && w.h.connected("labA")).then_some(())
        });
        // 연결 뒤에도 로컬 재시작의 실행 신원 대조가 남는다. 원격 SHA 거부 대기에 그 시간을 섞지 않는다.
        phase_started(second_id, aaa_proto::RolloutPhase::AgentUpdate);
        let (rc, out, err) = probe(&w, &format!("deploy status {second_id} --wait"));
        assert_eq!(rc, "1", "{out} {err}");
        let stopped: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(stopped["rollout"]["state"], "stopped");
        assert_eq!(stopped["rollout"]["cursor"], 5, "둘째 머신 교체 실패에서 멈춘다");
        assert!(stopped["rollout"]["error"].as_str().unwrap().contains("SHA-256"));
        let rejected_guard: Value = serde_json::from_slice(
            &std::fs::read(b.state.join(format!(
                "agent/deployments/{}.guard.json",
                second["rollout"]["steps"][6]["job_id"].as_str().unwrap()
            )))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(rejected_guard["install_started"], true, "실행 전 BeginInstall도 통과해야 한다");
        assert_eq!(rejected_guard["activated"], false);
        assert_eq!(rejected_guard["phase"], "cancelled", "교체 lease 뒤 unchanged면 감시를 끝낸다");
        assert_eq!(rejected_guard["installed_sha256"], rejected_guard["spec"]["baseline"]["sha256"]);
        assert_eq!(rejected_guard["restart_started"], false);
        assert_eq!(service_count(), 5, "실패한 remote를 다시 띄우면 안 된다");
        for exe in [&a_exe, &b_exe] {
            assert_eq!(hex::encode(Sha256::digest(std::fs::read(exe).unwrap())), sha, "성공한 앞 머신을 보존한다");
        }

        eprintln!("L3 unchanged failed update stopped without rollback");
        // L3: 앞 머신은 새 판을 유지하고, 실패한 remote만 배포 전 SHA로 복구한다.
        let old_b = dir.join("old-b");
        copy(&b_exe, &old_b);
        variant(&candidate, "third-candidate");
        let third_sha = digest(&candidate);
        let (rc, out, err) = probe(&w, &start(&third_sha));
        assert_eq!(rc, "0", "{out} {err}");
        let third: Value = serde_json::from_str(&out).unwrap();
        let third_id = third["rollout"]["id"].as_str().unwrap();
        phase_started(third_id, aaa_proto::RolloutPhase::HostRestart);
        assert!(next_call(5).ends_with("/dev.aaa.host"));
        w.lab.kill("rollout-host-second");
        w.lab.spawn_command("rollout-host-third", w.h.cmd_bin(&a_exe, &["host"]));
        local_restart_started(third_id);
        assert!(next_call(6).starts_with("a "));
        w.lab.kill("rollout-a-second");
        w.lab.spawn_command("rollout-a-third", agent_cmd(&w.h, &a_exe, "a", "local", "labA"));
        phase_started(third_id, aaa_proto::RolloutPhase::AgentRestart);
        assert!(next_call(7).starts_with("b "));
        let guard_id = third["rollout"]["steps"][6]["job_id"].as_str().unwrap().to_string();
        let guard_file = b.state.join(format!("agent/deployments/{guard_id}.guard.json"));
        let guard = || -> Option<Value> { serde_json::from_slice(&std::fs::read(&guard_file).ok()?).ok() };
        let armed = guard().unwrap();
        // 같은 사용자라도 agent peer PID가 아니면 private token을 알아도 제어할 수 없다.
        let auth: Value =
            serde_json::from_slice(&std::fs::read(b.state.join("agent/deployments/executor.identity.json")).unwrap())
                .unwrap();
        let mut spoof =
            std::os::unix::net::UnixStream::connect(b.state.join(format!("agent/run/g{guard_id}.sock"))).unwrap();
        spoof.set_read_timeout(Some(T)).unwrap();
        {
            use std::io::{Read, Write};
            let payload = serde_json::to_vec(
                &serde_json::json!({"token":auth["token"],"control":{"op":"recover","reason":"spoof"}}),
            )
            .unwrap();
            let mut frame = ((payload.len() + 1) as u32).to_be_bytes().to_vec();
            frame.push(1);
            frame.extend(payload);
            let _ = spoof.write_all(&frame);
            let mut reply = [0; 1];
            match spoof.read(&mut reply) {
                Ok(0) => {}
                Err(e) if matches!(e.kind(), std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::BrokenPipe) => {}
                result => panic!("위장 peer가 guard 응답을 받았다: {result:?}"),
            }
        }
        assert_eq!(guard().unwrap()["phase"], "awaiting_new");
        let direct = b.cmd_bin(&b_exe, &["deployment-guard", "--id", &guard_id]).output().unwrap();
        assert!(!direct.status.success(), "agent bootstrap 없이 내부 감시 CLI가 실행됐다");
        let recovery_ids = armed["spec"].clone();
        let guard_pid = armed["worker_pid"].clone();
        assert_eq!(digest(&b_exe), third_sha);
        w.lab.kill("rollout-b-new");
        // launchctl이 옛 실행 파일을 잘못 다시 띄운 사건. 감시는 원래 agent 밖에서 살아 있어야 한다.
        w.lab.spawn_command("rollout-b-wrong", agent_cmd(&b, &old_b, "b", &proxy.addr, "labB"));
        restored_file(&guard_file);
        let restart = next_call(8);
        assert!(restart.starts_with("b ") && restart.ends_with("/dev.aaa.agent"), "{restart}");
        let restoring =
            wait_for("독립 감시가 remote 복구 재시작 뒤 옛 프로세스를 기다림", T, || {
                let v = guard()?;
                (v["phase"] == "awaiting_old").then_some(v)
            });
        assert_eq!(restoring["worker_pid"], guard_pid, "agent가 죽어도 독립 감시는 같은 프로세스다");
        assert_eq!(restoring["spec"], recovery_ids, "복구 작업 ID를 새로 만들지 않는다");
        assert_eq!(digest(&a_exe), third_sha, "성공한 앞 머신을 되돌리지 않는다");
        assert_eq!(digest(&b_exe), sha, "실패 머신은 배포 전 SHA만 복구한다");
        let (rc, out, err) = probe(&w, &format!("deploy status {third_id}"));
        assert_eq!(rc, "1", "{out} {err}");
        let failed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(failed["rollout"]["state"], "recovering");
        assert_eq!(failed["rollout"]["cursor"], 6);
        // 복구 중 host 재시작도 같은 실패 머신/복구 ID에서 재개한다.
        w.lab.kill("rollout-host-third");
        w.lab.spawn_command("rollout-host-recovery-resume", w.h.cmd_bin(&a_exe, &["host"]));
        wait_for("복구 중 host와 두 agent 재접속", T, || {
            (w.h.connected("labA") && w.h.connected("labB")).then_some(())
        });
        let (rc, out, err) = probe(&w, &format!("deploy status {third_id}"));
        assert_eq!(rc, "1", "{out} {err}");
        let resumed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(resumed["rollout"]["state"], "recovering", "복구 요청 전 뜬 옛 프로세스를 복구 성공으로 셀 수 없다");
        assert_eq!(resumed["rollout"]["recovery"]["spec"], recovery_ids);
        w.lab.kill("rollout-b-wrong");
        w.lab.spawn_command("rollout-b-restored", agent_cmd(&b, &b_exe, "b", &proxy.addr, "labB"));
        let (rc, out, err) = probe(&w, &format!("deploy status {third_id} --wait"));
        assert_eq!(rc, "1", "{out} {err}");
        let rolled: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(rolled["rollout"]["state"], "stopped_rolled_back");
        assert_eq!(rolled["rollout"]["cursor"], 6, "실패 뒤 다음 단계로 진행하지 않는다");
        assert_eq!(rolled["rollout"]["recovery"]["phase"], "restored");
        assert_eq!(rolled["rollout"]["recovery"]["spec"], recovery_ids);
        assert_eq!(service_count(), 9, "복구 재시작은 한 번만 실행한다");

        eprintln!("L3 remote rollback survived agent death and host restart with the same recovery IDs");
        return;
    }
    let third_sha = digest(&a_exe);
    // host가 옛 판으로 돌아온 실패도 host 밖 감시가 복구한다. remote 파일/서비스는 건드리지 않는다.
    let old_a = dir.join("old-a");
    copy(&a_exe, &old_a);
    variant(&candidate, "fourth-candidate");
    let fourth_sha = digest(&candidate);
    let (rc, out, err) = probe(&w, &start(&fourth_sha));
    assert_eq!(rc, "0", "{out} {err}");
    let fourth: Value = serde_json::from_str(&out).unwrap();
    let fourth_id = fourth["rollout"]["id"].as_str().unwrap();
    phase_started(fourth_id, aaa_proto::RolloutPhase::HostRestart);
    assert!(next_call(9).ends_with("/dev.aaa.host"));
    w.lab.kill("rollout-host-recovery-resume");
    w.lab.spawn_command("rollout-host-wrong", w.h.cmd_bin(&old_a, &["host"]));
    let host_guard_id = fourth["rollout"]["steps"][3]["job_id"].as_str().unwrap();
    let host_guard_file = w.h.state.join(format!("agent/deployments/{host_guard_id}.guard.json"));
    restored_file(&host_guard_file);
    let restart = next_call(10);
    assert!(restart.starts_with("a ") && restart.ends_with("/dev.aaa.host"), "{restart}");
    wait_for("host 복구 후 새 옛판 프로세스 대기", T, || {
        let v: Value = serde_json::from_slice(&std::fs::read(&host_guard_file).ok()?).ok()?;
        (v["phase"] == "awaiting_old").then_some(())
    });
    assert_eq!(digest(&a_exe), third_sha);
    assert_eq!(digest(&b_exe), sha);
    w.lab.kill("rollout-host-wrong");
    w.lab.spawn_command("rollout-host-restored", w.h.cmd_bin(&a_exe, &["host"]));
    wait_for("복구 host의 front와 두 agent 준비", T, || {
        (w.h.connected("labA") && w.h.connected("labB")).then_some(())
    });
    let (rc, out, err) = probe(&w, &format!("deploy status {fourth_id} --wait"));
    assert_eq!(rc, "1", "{out} {err}");
    let rolled: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(rolled["rollout"]["state"], "stopped_rolled_back");
    assert_eq!(rolled["rollout"]["cursor"], 3);
    assert_eq!(service_count(), 11, "host 실패 뒤 agent 교체를 시작하면 안 된다");
    assert_eq!(digest(&b_exe), sha);

    eprintln!("L3 wrong host restored without touching the remote agent");

    // 로컬 agent 실패는 공유 실행 파일을 복구하고 host/agent 둘의 새 옛판 프로세스를 확인한다.
    variant(&candidate, "fifth-candidate");
    let fifth_sha = digest(&candidate);
    let (rc, out, err) = probe(&w, &start(&fifth_sha));
    assert_eq!(rc, "0", "{out} {err}");
    let fifth: Value = serde_json::from_str(&out).unwrap();
    let fifth_id = fifth["rollout"]["id"].as_str().unwrap();
    phase_started(fifth_id, aaa_proto::RolloutPhase::HostRestart);
    assert!(next_call(11).ends_with("/dev.aaa.host"));
    w.lab.kill("rollout-host-restored");
    w.lab.spawn_command("rollout-host-fifth", w.h.cmd_bin(&a_exe, &["host"]));
    local_restart_started(fifth_id);
    assert!(next_call(12).ends_with("/dev.aaa.agent"));
    w.lab.kill("rollout-a-third");
    w.lab.spawn_command("rollout-a-wrong", agent_cmd(&w.h, &old_a, "a", "local", "labA"));
    let local_guard_id = fifth["rollout"]["steps"][4]["job_id"].as_str().unwrap();
    let local_guard_file = w.h.state.join(format!("agent/deployments/{local_guard_id}.guard.json"));
    restored_file(&local_guard_file);
    assert!(next_call(13).ends_with("/dev.aaa.host"));
    assert!(next_call(14).ends_with("/dev.aaa.agent"));
    wait_for("로컬 복구가 host/agent 둘의 새 프로세스를 기다림", T, || {
        let v: Value = serde_json::from_slice(&std::fs::read(&local_guard_file).ok()?).ok()?;
        (v["phase"] == "awaiting_old").then_some(())
    });
    assert_eq!(digest(&a_exe), third_sha);
    w.lab.kill("rollout-host-fifth");
    w.lab.kill("rollout-a-wrong");
    w.lab.spawn_command("rollout-host-local-restored", w.h.cmd_bin(&a_exe, &["host"]));
    w.lab.spawn_command("rollout-a-local-restored", agent_cmd(&w.h, &a_exe, "a", "local", "labA"));
    wait_for("복구 host의 front와 두 agent 준비", T, || {
        (w.h.connected("labA") && w.h.connected("labB")).then_some(())
    });
    let (rc, out, err) = probe(&w, &format!("deploy status {fifth_id} --wait"));
    assert_eq!(rc, "1", "{out} {err}");
    let rolled: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(rolled["rollout"]["state"], "stopped_rolled_back");
    assert_eq!(rolled["rollout"]["cursor"], 4);
    assert_eq!(digest(&b_exe), sha);
    assert_eq!(service_count(), 15);
    eprintln!("L3 local agent rollback verified both restored host and agent processes");

    // 실제 설치 self-check를 socket 사건으로 붙잡는다. 승인 철회 뒤 늦은 rename이 복구를 덮으면 안 된다.
    let race_socket = w.lab.root.join("race.sock");
    let listener = std::os::unix::net::UnixListener::bind(&race_socket).unwrap();
    let (reached, blocked) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let _ = reached.send(stream);
    });
    let shim = dir.join("blocked-install");
    let script = format!(
        r#"#!/usr/bin/env python3
import os, pathlib, socket, sys
if sys.argv[1:] == ['--version']:
    print('aaa 0.1.2')
elif sys.argv[1:] == ['config', 'check']:
    counter = pathlib.Path(os.environ['AAA_STATE_DIR']) / 'install-check-count'
    count = int(counter.read_text()) + 1 if counter.exists() else 1
    counter.write_text(str(count))
    if os.environ.get('ROLLOUT_MACHINE') == 'a' and count == 2:
        with socket.socket(socket.AF_UNIX) as s:
            s.connect({socket:?})
            assert s.recv(1) == b'!'
else:
    sys.exit(1)
"#,
        socket = path_str(&race_socket)
    );
    std::fs::write(&shim, script).unwrap();
    std::fs::set_permissions(&shim, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let shim_sha = digest(&shim);
    let (rc, out, err) = probe(&w, &format!("deploy start --from '{}' --sha256 {shim_sha}", shim.display()));
    assert_eq!(rc, "0", "{out} {err}");
    let race: Value = serde_json::from_str(&out).unwrap();
    let race_id = race["rollout"]["id"].as_str().unwrap();
    let mut gate = blocked.recv_timeout(T).expect("실제 설치의 config check 도달 사건");
    let disabled = w.h.in_terminal(&["safety", "set", "demo", "coordinator_deploy", "off"], &[("바꿀까?", "y")], T);
    assert!(disabled.contains("coordinator_deploy = off"), "{disabled}");
    let race_guard_id = race["rollout"]["steps"][3]["job_id"].as_str().unwrap();
    let race_guard_file = w.h.state.join(format!("agent/deployments/{race_guard_id}.guard.json"));
    wait_for("진행 중 설치 철회는 감시를 끝내지 않고 복구로 전환", T, || {
        let v: Value = serde_json::from_slice(&std::fs::read(&race_guard_file).ok()?).ok()?;
        (v["phase"] == "restoring").then_some(())
    });
    let (rc, out, err) = probe(&w, &format!("deploy status {race_id}"));
    assert_eq!(rc, "1", "{out} {err}");
    let recovering: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(recovering["rollout"]["state"], "recovering", "진행 중 설치가 남았는데 중단 완료로 기록했다");
    assert_eq!(digest(&a_exe), third_sha);
    {
        use std::io::Write;
        gate.write_all(b"!").unwrap();
    }
    restored_file(&race_guard_file);
    assert!(next_call(15).ends_with("/dev.aaa.host"));
    wait_for("늦은 설치 rename 뒤 baseline 복구 완료", T, || (digest(&a_exe) == third_sha).then_some(()));
    w.lab.kill("rollout-host-local-restored");
    w.lab.spawn_command("rollout-host-cancel-restored", w.h.cmd_bin(&a_exe, &["host"]));
    wait_for("복구 host의 front와 두 agent 준비", T, || {
        (w.h.connected("labA") && w.h.connected("labB")).then_some(())
    });
    let (rc, out, err) = probe(&w, &format!("deploy status {race_id} --wait"));
    assert_eq!(rc, "1", "{out} {err}");
    let rolled: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(rolled["rollout"]["state"], "stopped_rolled_back");
    assert_eq!(rolled["rollout"]["cursor"], 2);
    assert_eq!(digest(&a_exe), third_sha, "복구 뒤 늦은 설치가 덮어썼다");
    assert_eq!(digest(&b_exe), sha);
    assert_eq!(service_count(), 16);
    eprintln!("L3 in-flight install cancellation restored after the final candidate rename");

    // W6c(답 1A): coordinator의 `aaa service restart host`는 재시작만 하는 rollout이다. 독립 감시가 새 host의 실행 신원과
    // 웹 응답(`/api/v1/me`)을 확인해야 끝나고, 실행 파일은 그대로다
    let enabled = w.h.in_terminal(&["safety", "set", "demo", "coordinator_deploy", "on"], &[("바꿀까?", "y")], T);
    assert!(enabled.contains("coordinator_deploy = on"), "{enabled}");
    let (rc, out, err) = probe(&w, "service restart --role host");
    assert_eq!(rc, "0", "{out} {err}");
    let restart: Value = serde_json::from_str(&out).unwrap();
    let restart_id = restart["rollout"]["id"].as_str().unwrap().to_string();
    assert_eq!(restart["rollout"]["spec"]["restart"], "host", "{out}");
    let restart_guard = w
        .h
        .state
        .join(format!("agent/deployments/{}.guard.json", restart["rollout"]["steps"][0]["job_id"].as_str().unwrap()));
    assert!(next_call(16).ends_with("/dev.aaa.host"));
    w.lab.kill("rollout-host-cancel-restored");
    w.lab.spawn_command("rollout-host-restart", w.h.cmd_bin(&a_exe, &["host"]));
    wait_for("재시작한 host의 front와 두 agent 준비", T, || {
        (w.h.connected("labA") && w.h.connected("labB")).then_some(())
    });
    let (rc, out, err) = probe(&w, &format!("deploy status {restart_id} --wait"));
    assert_eq!(rc, "0", "{out} {err}");
    let restarted: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(restarted["rollout"]["state"], "done", "{out}");
    let guard: Value = serde_json::from_slice(&std::fs::read(&restart_guard).unwrap()).unwrap();
    assert_eq!(guard["phase"], "verified_new", "{guard}");
    assert_eq!(digest(&a_exe), third_sha, "재시작은 실행 파일을 바꾸지 않는다");
    assert_eq!(service_count(), 17, "재시작을 한 번만 했다");
    eprintln!("W6c host restart rollout confirmed the new host and its web");
}

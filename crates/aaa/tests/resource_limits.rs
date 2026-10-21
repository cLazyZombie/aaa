//! 낮은 launchd 상속 한도를 자식에서 흉내 낸다. 운영 프로세스·부모 시험의 rlimit는 바꾸지 않는다.
#![cfg(unix)]
mod lab;
use lab::{Lab, Node, wait_for};
use std::io::{BufReader, Write};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::time::Duration;
const T: Duration = Duration::from_secs(15);
const BURST_CONNECTIONS: usize = 384;
// ticker·Front 조회가 잠깐 여는 fd의 여유를 회수 대기와 연결 수 단언에 똑같이 적용한다.
const FD_SLACK: usize = 2;

fn burst_fd_drop_matches(open: usize, after: usize) -> bool {
    open + FD_SLACK >= after + BURST_CONNECTIONS
}

#[test]
fn burst_fd_assertion_accepts_the_same_two_fd_slack_as_the_wait() {
    for after in 16..=18 {
        assert!(burst_fd_drop_matches(400, after), "닫은 뒤 {after}");
    }
    assert!(!burst_fd_drop_matches(400, 19), "여유 3개는 허용하지 않는다");
    assert!(!burst_fd_drop_matches(16 + 256, 16), "예전 256개 상한은 검출한다");
}

#[test]
fn cli_parses_and_dispatches_with_a_small_main_stack() {
    let lab = Lab::new();
    let node = lab.node("client", None);
    for (args, code, message) in [
        (vec!["--help"], 0, "work-root"),
        (vec!["attach"], 1, "TUI는 삭제됐다"),
        (vec!["completion", "bash"], 0, "_aaa"),
    ] {
        // macOS의 pre_exec는 시험 worker에서 fork한 자식이라 stack rlimit 변경을
        // 거부한다. 새 셸의 main에서 한도를 낮춘 뒤 실제 aaa로 exec한다.
        let mut cmd = node.cmd_bin(
            std::path::Path::new("/bin/sh"),
            &["-c", "ulimit -c 0 && ulimit -s 768 && exec \"$@\"", "aaa-small-stack", lab::BIN],
        );
        let out = cmd.args(&args).output().unwrap();
        assert_eq!(out.status.code(), Some(code), "{args:?}: {out:?}");
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        assert!(text.contains(message), "{args:?}: {text}");
    }
    assert!(std::fs::read_dir(&node.state).unwrap().next().is_none());
}

fn limited(mut cmd: Command, hard: libc::rlim_t) -> Command {
    // SAFETY: fork 뒤 이 자식의 rlimit만 바꾼다. 부모·다른 시험에는 영향이 없다.
    unsafe {
        cmd.pre_exec(move || {
            let limit = libc::rlimit { rlim_cur: 64, rlim_max: hard };
            if libc::setrlimit(libc::RLIMIT_NOFILE, &limit) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    cmd
}
fn setup(hard: libc::rlim_t) -> (Lab, Node, u32, String) {
    let mut lab = Lab::new();
    let node = lab.node("host", Some(&format!(
        "schema=1\n[host]\nagent_listen=\"\"\n[web]\nlisten=\"127.0.0.1:0\"\n[local_agent]\nmode=\"none\"\n[projects]\nroot={:?}\ntick_secs=1\n[notify]\nweb_push=false\n", lab.root.join("projects").display().to_string()
    )));
    let pid = lab.spawn_command("host", limited(node.cmd(&["host"]), hard));
    let status = wait_for("낮은 한도의 개발 host", T, || node.json(&["status"]).ok());
    node.ok(&["project", "create", "demo"]);
    (lab, node, pid, status["web_listen"].as_str().unwrap().to_string())
}

struct Burst {
    child: Child,
    out: BufReader<std::process::ChildStdout>,
}
impl Burst {
    fn new(addr: &str, count: usize, http: bool) -> Self {
        let script = r#"
import sys, socket, resource, http.client
soft, hard = resource.getrlimit(resource.RLIMIT_NOFILE)
resource.setrlimit(resource.RLIMIT_NOFILE, (min(8192, hard), hard))
host, port = sys.argv[1].split(':')
sockets = []
for _ in range(int(sys.argv[2])):
    if sys.argv[3] == 'http':
        c = http.client.HTTPConnection(host, int(port), timeout=10)
        c.request('GET', '/')
        r = c.getresponse()
        assert r.status == 200, r.status
        r.read()
    else:
        c = socket.create_connection((host, int(port)), timeout=10)
    sockets.append(c)
print('READY', flush=True)
sys.stdin.readline()
for c in sockets: c.close()
print('CLOSED', flush=True)
"#;
        let mut child = Command::new("python3")
            .args(["-u", "-c", script, addr, &count.to_string(), if http { "http" } else { "raw" }])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let out = BufReader::new(child.stdout.take().unwrap());
        Self { child, out }
    }
    fn ready(&mut self) {
        lab::read_line_containing(&mut self.out, "READY");
    }
    fn close(&mut self) {
        self.child.stdin.as_mut().unwrap().write_all(b"close\n").unwrap();
        lab::read_line_containing(&mut self.out, "CLOSED");
        assert!(self.child.wait().unwrap().success());
    }
}
impl Drop for Burst {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// OS가 실제로 가진 numeric fd를 센다. 제품 내부 counter만으로 누수가 없다고 단언하지 않는다.
fn fd_count(pid: u32) -> usize {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_dir(format!("/proc/{pid}/fd")).unwrap().count()
    }
    #[cfg(not(target_os = "linux"))]
    {
        let out = Command::new("lsof").args(["-n", "-P", "-a", "-p", &pid.to_string(), "-Ff"]).output().unwrap();
        assert!(out.status.success());
        String::from_utf8(out.stdout)
            .unwrap()
            .lines()
            .filter(|l| l.strip_prefix('f').is_some_and(|s| s.parse::<u32>().is_ok()))
            .count()
    }
}

/// 시간 상한·판정은 그대로 두고, 실패한 그 host와 단발 Front 요청의 상태를 보존한다.
/// 웹 EMFILE과 Front 접속 사이에 fd가 풀렸는지, 요청이 성공했는지/연결 전 실패했는지 구분할 근거다.
fn wait_emfile(lab: &Lab, pid: u32, source: &str, pending: Option<u32>) {
    fn command_state(name: &str, result: std::io::Result<std::process::Output>) {
        match result {
            Ok(out) => eprintln!(
                "{name}: {}\n{}{}",
                out.status,
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ),
            Err(e) => eprintln!("{name} 조회 실패: {e}"),
        }
    }
    let marker = format!("source=\"{source}\"");
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        wait_for(&format!("{source} 실제 EMFILE"), T, || lab.log("host").contains(&marker).then_some(()));
    }));
    if let Err(failure) = result {
        eprintln!("EMFILE 진단: source={source}, host_pid={pid}, pending_front_pid={pending:?}");
        eprintln!("단발 Front 요청 stdout·stderr:\n{}", lab.log("pending-front"));
        eprintln!("host 로그:\n{}", lab.log("host"));
        #[cfg(target_os = "linux")]
        match std::fs::read_dir(format!("/proc/{pid}/fd")) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    eprintln!("host fd {:?}: {:?}", entry.file_name(), std::fs::read_link(entry.path()));
                }
            }
            Err(e) => eprintln!("host fd 조회 실패: {e}"),
        }
        #[cfg(not(target_os = "linux"))]
        command_state("host OS fd 목록", Command::new("lsof").args(["-nP", "-p", &pid.to_string()]).output());
        let pids = pending.map_or_else(|| pid.to_string(), |p| format!("{pid},{p}"));
        command_state(
            "관측한 host·Front PID 상태",
            Command::new("ps").args(["-o", "pid,ppid,stat,etime,command", "-p", &pids]).output(),
        );
        std::panic::resume_unwind(failure);
    }
}

#[test]
fn low_soft_limit_is_raised_for_host_and_agent_and_web_bursts_release_fds() {
    let (mut lab, node, pid, addr) = setup(8192);
    assert!(lab.log("host").contains("before=64 soft=8192 hard=8192"));
    let agent = lab.node("agent", None);
    // agent는 host와 같은 로컬 socket을 쓰도록 이 시험 host의 state root로 붙인다.
    let mut cmd = node.cmd(&["agent", "--host", "local", "--name", "mac-a"]);
    cmd.env("AAA_CONFIG_DIR", &agent.cfg);
    lab.spawn_command("agent", limited(cmd, 512));
    wait_for("agent 한도·연결", T, || {
        let log = lab.log("agent");
        (log.contains("before=64 soft=512 hard=512") && log.contains("host에 연결했다")).then_some(())
    });
    let baseline = fd_count(pid);
    for _ in 0..2 {
        let mut burst = Burst::new(&addr, BURST_CONNECTIONS, true);
        burst.ready();
        // 연결 수는 닫은 뒤 가라앉은 수와 견준다: 시작 직후 첫 ticker 같은 일이 잠깐 연 파일이 기준에 섞이면
        // 기준과 견주는 셈이 모자라 보인다. 회수 대기에서 허용한 여유는 연결 수 단언에도 적용한다.
        let open = fd_count(pid);
        burst.close();
        let after = wait_for("연결 종료 뒤 OS fd 회수", T, || {
            let n = fd_count(pid);
            (n <= baseline + FD_SLACK).then_some(n)
        });
        assert!(
            burst_fd_drop_matches(open, after),
            "실제로 열린 연결로 256개 상한을 넘겨야 한다: 열었을 때 {open}, 닫은 뒤 {after} (여유 {FD_SLACK})"
        );
    }
    node.ticks(1);
    assert!(!lab.log("host").contains("host 파일 한도 소진"));
}

#[test]
fn exhausted_hard_limit_warns_once_and_acceptors_recover() {
    let (mut lab, node, pid, addr) = setup(64);
    assert!(lab.log("host").contains("before=64 soft=64 hard=64"));
    let baseline = fd_count(pid);
    let mut burst = Burst::new(&addr, 80, false);
    burst.ready();
    wait_for("실제 EMFILE", T, || lab.log("host").contains("host 파일 한도 소진").then_some(()));
    // EMFILE 중 Front 접속도 시도한다. 예전 while-let accept는 이 오류에 listener를 영구히 끝냈다.
    let pending_front = lab.spawn_command("pending-front", node.cmd(&["--json", "status"]));
    wait_emfile(&lab, pid, "front accept", Some(pending_front));
    burst.close();
    // macOS accept의 EMFILE은 그때 대기하던 socket을 닫을 수 있다. 그 요청의 성공을 약속하지 않고,
    // 종료를 울타리로 삼아 이후 새 Front 요청이 처리되는지를 아래에서 확인한다.
    let _ = lab.wait_exit("pending-front", T);
    wait_for("EMFILE 뒤 fd 회수", T, || (fd_count(pid) <= baseline + 2).then_some(()));
    let items = wait_for("시스템 경고 저장", T, || {
        let v = node.json(&["inbox", "list", "demo", "--all", "--peek"]).ok()?;
        v["items"].as_array()?.iter().any(|i| i["event"] == "file descriptors exhausted").then_some(v)
    });
    assert_eq!(
        items["items"].as_array().unwrap().iter().filter(|i| i["event"] == "file descriptors exhausted").count(),
        1
    );
    node.ticks(2);
    let items = node.ok(&["inbox", "list", "demo", "--all", "--peek"]);
    assert_eq!(
        items["items"].as_array().unwrap().iter().filter(|i| i["event"] == "file descriptors exhausted").count(),
        1
    );
    // 웹 accept도 영구히 끝나지 않았다. HTTP 응답을 받은 뒤 연결을 닫는다.
    let mut resumed = Burst::new(&addr, 1, true);
    resumed.ready();
    resumed.close();
}

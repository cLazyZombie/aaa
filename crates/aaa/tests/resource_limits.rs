//! 낮은 launchd 상속 한도를 자식에서 흉내 낸다. 운영 프로세스·부모 시험의 rlimit는 바꾸지 않는다.
#![cfg(unix)]
mod lab;
use lab::{Lab, Node, wait_for};
use std::io::{BufReader, Write};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::time::Duration;
const T: Duration = Duration::from_secs(15);

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
        let mut burst = Burst::new(&addr, 384, true);
        burst.ready();
        assert!(fd_count(pid) >= baseline + 384, "실제로 열린 연결로 256개 상한을 넘겨야 한다");
        burst.close();
        wait_for("연결 종료 뒤 OS fd 회수", T, || (fd_count(pid) <= baseline + 2).then_some(()));
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
    let (_, pending_front) = lab.spawn_piped("pending-front", &node, &["--json", "status"]);
    wait_for("Front accept도 EMFILE", T, || lab.log("host").contains("source=\"front accept\"").then_some(()));
    burst.close();
    // macOS accept의 EMFILE은 그때 대기하던 socket을 닫을 수 있다. 그 요청의 성공을 약속하지 않고,
    // 종료를 울타리로 삼아 이후 새 Front 요청이 처리되는지를 아래에서 확인한다.
    drop(pending_front);
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

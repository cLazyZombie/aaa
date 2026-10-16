//! M1-3a E2E: 프로젝트·thread 시작(그 머신 agent가 worktree·작업 폴더를 만든다)·brief 안내·보고 회수·ack·resolve·흐름 사건.
//! agent harness는 시험용 가짜 agent(셸 스크립트)다. brief 안내 한 줄을 받으면 brief를 읽고 report.md·library를 쓴다.
//! 실행: `cargo test -p aa --test m13`

#![cfg(unix)]

mod lab;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use lab::{Lab, Node, path_str, read_line_containing, wait_for, wait_for_diag};
use serde_json::Value;

const T: Duration = Duration::from_secs(30);

/// brief 안내를 받으면 할 일대로 보고를 쓰는 가짜 agent
const FAKE_AGENT: &str = r#"#!/bin/sh
printf 'fake> '
while IFS= read -r line; do
  case "$line" in
    *brief.md*)
      path=$(printf '%s' "$line" | sed 's/를 읽고.*//')
      dir=$(dirname "$path")
      echo "brief 받음: $path"
      task=$(sed -n '/^# 할 일/,$p' "$path" | tail -n +3)
      echo "결과물" > "$dir/library/out.txt"
      mkdir -p "$dir/library/sub" && echo deep > "$dir/library/sub/d.txt"
      ln -s /etc/hosts "$dir/library/link"
      printf '[완료]\n\n## Report\n%s\n\n## Next\n- 결과 확인\n- 다음 단계\n' "$task" > "$dir/report.md.tmp"
      mv "$dir/report.md.tmp" "$dir/report.md"
      case "$task" in
        *commit*) echo change > change.txt && git add change.txt && git -c user.name=t -c user.email=t@example.com commit -qm change ;;
      esac
      case "$task" in
        *dirty*) echo dirty > dirty.txt ;;
      esac
      echo "done"
      ;;
    *) echo "got: $line" ;;
  esac
  printf 'fake> '
done
"#;

/// 안내를 받아도 화면이 바뀌지 않는 agent(echo 없음). 받은 줄은 파일에 남긴다.
const MUTE_AGENT: &str = "#!/bin/sh\nstty -echo\nexec cat >> \"$MUTE_LOG\"\n";

fn host_toml(projects: &Path) -> String {
    format!(
        "schema = 1\n[host]\nagent_listen = \"127.0.0.1:0\"\n[web]\nlisten = \"\"\n[local_agent]\nmode = \"none\"\n\
         [projects]\nroot = \"{}\"\nallow_profiles = [\"fake\", \"mute\"]\nlibrary_copy_mb = 1\n",
        path_str(projects)
    )
}

fn agent_toml(work_root: &Path, scripts: &Path) -> String {
    format!(
        "schema = 1\n[agent]\nwork_root = \"{}\"\n\
         [profiles.fake]\nharness = \"custom\"\ncommand = [\"/bin/sh\", \"{}\"]\n\
         [profiles.mute]\nharness = \"custom\"\ncommand = [\"/bin/sh\", \"{}\"]\nenv = {{ MUTE_LOG = \"{}\" }}\n\
         [profiles.other]\nharness = \"custom\"\ncommand = [\"/bin/sh\"]\n",
        path_str(work_root),
        path_str(&scripts.join("fake.sh")),
        path_str(&scripts.join("mute.sh")),
        path_str(&scripts.join("mute.log")),
    )
}

fn connected(node: &Node, name: &str) -> bool {
    node.machine(name).is_some_and(|m| m["connection"] == "connected")
}

fn git(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(args)
        .output()
        .unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

/// origin(bare)과 그 clone(origin/HEAD 있음)
fn make_repo(root: &Path) -> PathBuf {
    let seed = root.join("seed");
    std::fs::create_dir_all(&seed).unwrap();
    git(&seed, &["init", "-q", "-b", "main"]);
    std::fs::write(seed.join("README.md"), "hi\n").unwrap();
    git(&seed, &["add", "."]);
    git(&seed, &["commit", "-qm", "init"]);
    let origin = root.join("origin.git");
    git(root, &["clone", "-q", "--bare", path_str(&seed), path_str(&origin)]);
    let repo = root.join("repo");
    git(root, &["clone", "-q", path_str(&origin), path_str(&repo)]);
    repo
}

struct World {
    lab: Lab,
    h: Node,
    projects: PathBuf,
    repo: PathBuf,
    scripts: PathBuf,
    addr: String,
}

fn world() -> World {
    let lab = Lab::new();
    let projects = lab.root.join("projects");
    let scripts = lab.root.join("scripts");
    std::fs::create_dir_all(&scripts).unwrap();
    std::fs::write(scripts.join("fake.sh"), FAKE_AGENT).unwrap();
    std::fs::write(scripts.join("mute.sh"), MUTE_AGENT).unwrap();
    let repo = make_repo(&lab.root);
    let h = lab.node("h", Some(&host_toml(&projects)));
    std::fs::write(h.cfg.join("agent.toml"), agent_toml(&lab.root.join("workA"), &scripts)).unwrap();
    let mut w = World { lab, h, projects, repo, scripts, addr: String::new() };
    w.lab.spawn("host", &w.h, &["host"]);
    let st = wait_for("host 준비", T, || w.h.json(&["status"]).ok());
    w.addr = st["agent_listen"].as_str().unwrap().to_string();
    w.lab.spawn("agentA", &w.h, &["agent", "--host", "local", "--name", "labA"]);
    wait_for("labA 연결", T, || connected(&w.h, "labA").then_some(()));
    w.h.ok(&["project", "create", "demo"]);
    w
}

impl World {
    fn task(&self, name: &str, text: &str) -> String {
        let p = self.lab.root.join(name);
        std::fs::write(&p, text).unwrap();
        path_str(&p).to_string()
    }

    fn start(&self, args: &[&str]) -> Value {
        let mut a = vec!["thread", "start", "demo"];
        a.extend_from_slice(args);
        self.h.ok(&a)
    }

    fn show(&self, id: &str) -> Value {
        self.h.ok(&["thread", "show", "demo", id])
    }

    fn wait_show(&self, what: &str, id: &str, f: impl Fn(&Value) -> bool) -> Value {
        wait_for_diag(
            what,
            T,
            || Some(self.show(id)).filter(|v| f(v)),
            || format!("thread: {}\nevents: {}", self.show(id), self.h.ok(&["events", "--project", "demo"])),
        )
    }

    fn kinds(&self, after: u64) -> Vec<String> {
        let v = self.h.ok(&["events", "--project", "demo", "--after", &after.to_string()]);
        v["events"].as_array().unwrap().iter().map(|e| e["kind"].as_str().unwrap().to_string()).collect()
    }

    /// 원격(TLS) agent labB를 등록하고 띄운다
    fn add_remote(&mut self) {
        let b = self.lab.node("b", None);
        std::fs::write(b.cfg.join("agent.toml"), agent_toml(&self.lab.root.join("workB"), &self.scripts)).unwrap();
        self.h.ok(&["machine", "enroll", "--open", "5m"]);
        let (_, mut out) = self.lab.spawn_piped("enroll", &b, &["agent", "enroll", &self.addr, "--name", "labB"]);
        let line = read_line_containing(&mut out, "--code");
        let code = line.split("--code").nth(1).unwrap().trim().to_string();
        self.h.ok(&["machine", "approve", "labB", "--code", &code]);
        assert!(self.lab.wait_exit("enroll", T).success(), "{}", self.lab.log("enroll"));
        self.lab.spawn("agentB", &b, &["agent", "--host", &self.addr, "--name", "labB"]);
        wait_for("labB 연결", T, || connected(&self.h, "labB").then_some(()));
    }
}

/// worktree thread 한 바퀴: 시작 → brief 안내 → 보고·library 사본 → ack → resolve(병합된 branch 삭제)
#[test]
fn worktree_thread_round_trip() {
    let w = world();
    let task = w.task("task1.txt", "보고에 X-42를 적는다");
    let v = w.start(&[
        "--title",
        "Fix the bug",
        "--machine",
        "labA",
        "--profile",
        "fake",
        "--repo",
        path_str(&w.repo),
        "--task-file",
        &task,
    ]);
    assert_eq!((v["id"].as_str(), v["status"].as_str()), (Some("t-0001"), Some("open")), "{v}");
    assert_eq!(v["branch"], "aa/demo/t-0001-fix-the-bug");
    let cwd = PathBuf::from(v["cwd"].as_str().unwrap());
    assert_eq!(cwd, w.lab.root.join("workA").join("repo").join("demo-t-0001"));
    assert!(cwd.join(".aa/demo-t-0001/brief.md").is_file());
    let brief = std::fs::read_to_string(cwd.join(".aa/demo-t-0001/brief.md")).unwrap();
    assert!(brief.contains("X-42") && brief.contains("# thread 규칙") && brief.contains("# 프로젝트 지침"), "{brief}");
    assert!(w.projects.join("demo/threads/t-0001.task.md").is_file());

    let v = w.wait_show("보고 사본", "t-0001", |v| v["report_hash"].is_string());
    assert_eq!(v["brief"], "delivered", "{v}");
    assert_eq!(v["report_state"], "완료");
    assert_eq!(v["next"], serde_json::json!(["결과 확인", "다음 단계"]));
    assert_eq!(v["group"], "ready_for_review");
    // symlink는 복사하지 않아 사본은 partial이다
    assert_eq!(v["copy"], "partial");
    let copied = std::fs::read_to_string(w.projects.join("demo/threads/t-0001.md")).unwrap();
    assert!(copied.contains("X-42"), "{copied}");
    let lib = w.projects.join("demo/library/t-0001");
    assert_eq!(std::fs::read_to_string(lib.join("out.txt")).unwrap(), "결과물\n");
    assert_eq!(std::fs::read_to_string(lib.join("sub/d.txt")).unwrap(), "deep\n");
    assert!(std::fs::symlink_metadata(lib.join("link")).is_err(), "symlink가 복사됐다");
    // 작업 폴더는 git 상태에 보이지 않는다(info/exclude)
    assert_eq!(git(&cwd, &["status", "--porcelain"]), "");

    let v = w.h.ok(&["thread", "ack", "demo", "t-0001"]);
    assert_eq!(v["group"], "working");

    let v = w.h.ok(&["thread", "resolve", "demo", "t-0001"]);
    assert_eq!(v["status"], "resolved");
    assert!(!cwd.exists(), "고치지 않은 worktree는 지워져야 한다");
    assert_eq!(git(&w.repo, &["branch", "--list", "aa/demo/*"]), "", "병합된(새 commit 없는) branch는 지운다");
    let pane = v["pane"].as_str().unwrap();
    wait_for("세션 종료", T, || (w.h.pane(pane)["state"] != "running").then_some(()));
    assert_eq!(w.kinds(0), ["thread_started", "brief_delivered", "report_ready", "review_acked", "thread_resolved"]);
    // cursor 뒤만 읽는다
    assert_eq!(w.kinds(3), ["review_acked", "thread_resolved"]);
}

/// 고친 worktree와 병합되지 않은 branch는 남긴다(force 없음). 허용되지 않은 profile은 거부한다.
#[test]
fn resolve_keeps_dirty_worktree_and_unmerged_branch() {
    let w = world();
    let e =
        w.h.json(&[
            "thread",
            "start",
            "demo",
            "--title",
            "x",
            "--profile",
            "other",
            "--repo",
            path_str(&w.repo),
            "--task-file",
            &w.task("t0.txt", "x"),
        ])
        .expect_err("허용되지 않은 profile");
    assert!(e.contains("forbidden"), "{e}");

    let task = w.task("task2.txt", "commit 하고 dirty 파일을 남긴다");
    let v = w.start(&["--title", "keep", "--profile", "fake", "--repo", path_str(&w.repo), "--task-file", &task]);
    let id = v["id"].as_str().unwrap().to_string();
    let cwd = PathBuf::from(v["cwd"].as_str().unwrap());
    w.wait_show("보고", &id, |v| v["report_hash"].is_string());
    let v = w.h.ok(&["thread", "resolve", "demo", &id]);
    assert_eq!(v["status"], "resolved");
    assert!(cwd.join("dirty.txt").exists(), "고친 worktree는 남아야 한다");
    assert_eq!(git(&w.repo, &["branch", "--list", "aa/demo/*", "--format=%(refname:short)"]), "aa/demo/t-0001-keep");
    let ev = w.h.ok(&["events", "--project", "demo"]);
    let last = ev["events"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(last["kind"], "thread_resolved");
    assert!(last["detail"].as_str().unwrap().contains("worktree 유지"), "{last}");
}

/// 원격(TLS) 머신의 작업 폴더 thread, host 재시작 뒤 보고 감시 재개, 평가자 역할 필터
#[test]
fn remote_dir_thread_survives_host_restart() {
    let mut w = world();
    w.add_remote();
    let v = w.start(&[
        "--title",
        "원격 일",
        "--machine",
        "labB",
        "--profile",
        "fake",
        "--kind",
        "dir",
        "--task-file",
        &w.task("t3.txt", "원격 R-7"),
    ]);
    let id = v["id"].as_str().unwrap().to_string();
    assert_eq!(PathBuf::from(v["cwd"].as_str().unwrap()), w.lab.root.join("workB").join("demo").join(&id));
    assert_eq!(v["branch"], Value::Null);
    let v = w.wait_show("원격 보고", &id, |v| v["report_hash"].is_string());
    assert_eq!(v["machine"], "labB");
    assert!(std::fs::read_to_string(w.projects.join("demo/threads").join(format!("{id}.md"))).unwrap().contains("R-7"));
    let pane = v["pane"].as_str().unwrap().to_string();
    // thread 세션은 worker 역할이다
    let workers = w.h.ok(&["pane", "list", "--role", "worker"]);
    assert!(workers.to_string().contains(&pane));
    assert_eq!(w.h.ok(&["pane", "list", "--role", "evaluator"])["panes"], serde_json::json!([]));
    let seen = w.h.ok(&["events", "--project", "demo"])["events"].as_array().unwrap().len() as u64;

    // host 재시작 중에 보고가 바뀌어도, 다시 붙은 agent가 지금 hash를 알려 사본을 다시 받는다
    // 원격 agent가 같은 주소로 다시 붙게 처음 받은 포트를 고정한다
    let toml = std::fs::read_to_string(w.h.cfg.join("host.toml")).unwrap();
    std::fs::write(w.h.cfg.join("host.toml"), toml.replace("127.0.0.1:0", &w.addr)).unwrap();
    w.lab.kill("host");
    // host가 없는 동안 agent 머신에서 보고가 바뀐다
    let work_dir = PathBuf::from(v["work_dir"].as_str().unwrap());
    std::fs::write(work_dir.join("report.md"), "[사용자 작업 필요]\n\n## Report\n갱신\n\n## Next\n- 사용자 확인\n")
        .unwrap();
    w.lab.spawn("host2", &w.h, &["host"]);
    wait_for("labB 재연결", Duration::from_secs(60), || connected(&w.h, "labB").then_some(()));
    let v = w.wait_show("갱신된 보고", &id, |v| v["report_state"] == "사용자 작업 필요");
    assert_eq!(v["group"], "waiting_on_you");
    assert_eq!(v["next"], serde_json::json!(["사용자 확인"]));
    assert_eq!(w.kinds(seen), ["report_ready"]);
}

/// 안내를 보내도 화면이 바뀌지 않으면 unknown으로 남기고 자동으로 다시 보내지 않는다(§8.5 a)
#[test]
fn unknown_brief_is_not_resent() {
    let w = world();
    let v = w.start(&["--title", "mute", "--profile", "mute", "--kind", "dir", "--task-file", &w.task("t4.txt", "x")]);
    let id = v["id"].as_str().unwrap().to_string();
    let v = w.wait_show("unknown", &id, |v| v["brief"] == "unknown");
    assert_eq!(v["group"], "waiting_on_you");
    let log = w.scripts.join("mute.log");
    // 더 기다려도 한 번뿐이다
    std::thread::sleep(Duration::from_secs(5));
    let got = std::fs::read_to_string(&log).unwrap();
    assert_eq!(got.matches("brief.md").count(), 1, "{got}");
    assert_eq!(w.kinds(0), ["thread_started", "brief_unknown"]);
    // 사람이 명시적으로 다시 보내면 그때만 한 번 더 간다
    w.h.ok(&["thread", "brief", "demo", &id]);
    w.wait_show("다시 unknown", &id, |v| v["brief"] == "unknown");
    wait_for("두 번째 안내", T, || {
        (std::fs::read_to_string(&log).unwrap_or_default().matches("brief.md").count() == 2).then_some(())
    });
}

//! M1-3 E2E: 프로젝트·thread 시작(그 머신 agent가 worktree·작업 폴더를 만든다)·brief 안내·보고 회수·ack·resolve·흐름 사건,
//! harness 상태(hook)·확인창 질문 원격 응답·메시지(queue·interrupt).
//! agent harness는 시험용 가짜 agent(셸 스크립트)다. brief 안내 한 줄을 받으면 brief를 읽고 report.md·library를 쓴다.
//! 가짜 Claude(`hooked`)는 Claude Code처럼 hook 사건을 `aa hook --agent claude`로 보내고 확인창을 띄운다.
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

/// 가짜 Claude Code: 폴더 신뢰 창 → SessionStart → 입력마다 UserPromptSubmit … Stop.
/// 선택 창은 Claude처럼 그린다(❯ 커서, 화살표로 옮기고 Enter로 고름, Esc는 취소). 폴더 신뢰 창의 기본 선택은
/// 실제 Claude Code 2.1처럼 「No, exit」다. 권한을 거절하거나 `long` 중에 Esc가 오면 Claude처럼 「Interrupted」만 보이고
/// Stop hook은 내지 않는다.
const HOOKED_AGENT: &str = r#"#!/bin/sh
ESC=$(printf '\033')
hook() { "$AA" hook --agent claude "$1"; }
key() { stty -icanon -echo min 1 time 0; k=$(dd bs=1 count=1 2>/dev/null); stty icanon echo; }
keyt() { stty -icanon -echo min 0 time "$1"; k=$(dd bs=1 count=1 2>/dev/null); stty icanon echo; }
menu() {
  q=$1; sel=$2
  while :; do
    printf '\033[2J\033[H%s\n' "$q"
    if [ "$sel" = 0 ]; then printf '\342\235\257 %s\n  %s\n' "$3" "$4"; else printf '  %s\n\342\235\257 %s\n' "$3" "$4"; fi
    printf 'Enter to confirm \302\267 Esc to cancel\n'
    key
    if [ "$k" = "$ESC" ]; then
      keyt 2
      if [ "$k" = "[" ]; then keyt 2; case "$k" in A) sel=0;; B) sel=1;; esac; continue; fi
      pick=cancel; break
    fi
    if [ -z "$k" ]; then if [ "$sel" = 0 ]; then pick=$3; else pick=$4; fi; break; fi
  done
  printf '\033[2J\033[H'
}
perm() {
  hook '{"hook_event_name":"PermissionRequest","session_id":"fake-1","tool_name":"Bash","tool_input":{"command":"make test"}}'
  hook '{"hook_event_name":"Notification","session_id":"fake-1","notification_type":"permission_prompt","message":"Claude needs your permission to use Bash"}'
  menu 'Allow Bash(make test)?' 0 'Yes' 'No, and tell Claude what to do differently (esc)'
  if [ "$pick" = Yes ]; then echo "allowed"; hook '{"hook_event_name":"PostToolUse","session_id":"fake-1","tool_name":"Bash"}'
  else echo "denied"; printf 'Interrupted \302\267 What should Claude do instead?\n'; quiet=1; fi
}
menu 'Quick safety check: Do you trust the files in this folder?' 0 'No, exit' 'Yes, I trust this folder'
[ "$pick" = 'Yes, I trust this folder' ] || { echo "trust: $pick"; exit 1; }
hook '{"hook_event_name":"SessionStart","session_id":"fake-1","source":"startup"}'
echo "marker: ${CLAUDE_CODE_CHILD_SESSION:-none}"
printf 'fake> '
while IFS= read -r line; do
  quiet=0
  hook '{"hook_event_name":"UserPromptSubmit","session_id":"fake-1"}'
  case "$line" in
    *brief.md*)
      path=$(printf '%s' "$line" | sed 's/를 읽고.*//')
      dir=$(dirname "$path")
      "$AA" report --percent 40 --activity '권한 확인 중'
      perm
      printf '[완료]\n\n## Report\n%s\n\n## Next\n- 확인\n' "$pick" > "$dir/report.md"
      ;;
    *ask*) perm ;;
    *slow*) echo "slow start"; sleep 3; echo "slow done" ;;
    *long*)
      echo "long start"; keyt 250
      if [ "$k" = "$ESC" ]; then
        # 멈추는 데 시간이 걸리고, 그동안 들어온 입력은 버린다(멈춤을 확인하기 전에 보낸 글은 사라진다)
        sleep 1; while keyt 1; [ -n "$k" ]; do :; done
        echo "Interrupted"; quiet=1
      else echo "long done"; fi ;;
    *) echo "got: $line" ;;
  esac
  [ "$quiet" = 1 ] || hook '{"hook_event_name":"Stop","session_id":"fake-1","last_assistant_message":"끝"}'
  printf 'fake> '
done
"#;

/// 안내를 받아도 화면이 바뀌지 않는 agent(echo 없음). 받은 줄은 파일에 남긴다.
const MUTE_AGENT: &str = "#!/bin/sh\nstty -echo\nexec cat >> \"$MUTE_LOG\"\n";

fn host_toml(projects: &Path) -> String {
    format!(
        "schema = 1\n[host]\nagent_listen = \"127.0.0.1:0\"\n[web]\nlisten = \"\"\n[local_agent]\nmode = \"none\"\n\
         [projects]\nroot = \"{}\"\nallow_profiles = [\"fake\", \"mute\", \"hooked\"]\nlibrary_copy_mb = 1\n",
        path_str(projects)
    )
}

fn agent_toml(work_root: &Path, scripts: &Path) -> String {
    format!(
        "schema = 1\n[agent]\nwork_root = \"{}\"\n\
         [profiles.fake]\nharness = \"custom\"\ncommand = [\"/bin/sh\", \"{}\"]\n\
         [profiles.mute]\nharness = \"custom\"\ncommand = [\"/bin/sh\", \"{}\"]\nenv = {{ MUTE_LOG = \"{}\" }}\n\
         [profiles.hooked]\nharness = \"custom\"\ncommand = [\"/bin/sh\", \"{}\"]\nenv = {{ AA = \"{}\" }}\n\
         [profiles.other]\nharness = \"custom\"\ncommand = [\"/bin/sh\"]\n",
        path_str(work_root),
        path_str(&scripts.join("fake.sh")),
        path_str(&scripts.join("mute.sh")),
        path_str(&scripts.join("mute.log")),
        path_str(&scripts.join("hooked.sh")),
        lab::BIN,
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
    std::fs::write(scripts.join("hooked.sh"), HOOKED_AGENT).unwrap();
    let repo = make_repo(&lab.root);
    let h = lab.node("h", Some(&host_toml(&projects)));
    std::fs::write(h.cfg.join("agent.toml"), agent_toml(&lab.root.join("workA"), &scripts)).unwrap();
    let mut w = World { lab, h, projects, repo, scripts, addr: String::new() };
    w.lab.spawn("host", &w.h, &["host"]);
    let st = wait_for("host 준비", T, || w.h.json(&["status"]).ok());
    w.addr = st["agent_listen"].as_str().unwrap().to_string();
    // agent를 Claude Code 세션 안에서 띄운 경우: 그 세션 표지는 aa 세션에 넘기지 않는다
    w.lab.spawn_env(
        "agentA",
        &w.h,
        &["agent", "--host", "local", "--name", "labA"],
        &[("CLAUDE_CODE_CHILD_SESSION", "1"), ("CLAUDECODE", "1")],
    );
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

    fn questions(&self, all: bool) -> Vec<Value> {
        let a: &[&str] = if all { &["question", "list", "--all"] } else { &["question", "list"] };
        self.h.ok(a)["questions"].as_array().unwrap().clone()
    }

    /// 이 thread에 열린 질문 하나(종류 `kind`)를 기다린다
    fn wait_question(&self, id: &str, kind: &str) -> Value {
        wait_for_diag(
            &format!("{kind} 질문"),
            T,
            || {
                let open: Vec<_> = self.questions(false).into_iter().filter(|q| q["thread"] == id).collect();
                (open.len() == 1 && open[0]["question_kind"] == kind).then(|| open[0].clone())
            },
            || format!("questions: {:?}\nthread: {}", self.questions(true), self.show(id)),
        )
    }

    fn message(&self, id: &str, text: &str, interrupt: bool) -> Value {
        let mut a = vec!["thread", "message", "demo", id, "--text", text];
        if interrupt {
            a.push("--interrupt");
        }
        self.h.ok(&a)
    }

    fn wait_screen(&self, pane: &str, needle: &str) -> String {
        wait_for_diag(
            &format!("화면 {needle:?}"),
            T,
            || Some(self.h.screen(pane)).filter(|s| s.contains(needle)),
            || self.h.screen(pane),
        )
    }

    fn wait_for_count(&self, pane: &str, needle: &str, n: usize) {
        wait_for_diag(
            &format!("화면 {needle:?} {n}번"),
            T,
            || (self.h.screen(pane).matches(needle).count() >= n).then_some(()),
            || self.h.screen(pane),
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
    // 메시지도 받았다는 신호(hook 사건·화면 변화)가 없으면 unknown이고 다시 보내지 않는다
    let m = w.message(&id, "MSG-UNKNOWN-1", false);
    assert_eq!(m["state"], "unknown", "{m}");
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(std::fs::read_to_string(&log).unwrap().matches("MSG-UNKNOWN-1").count(), 1);
    assert_eq!(
        w.kinds(0).iter().filter(|k| k.starts_with("message_")).collect::<Vec<_>>(),
        ["message_queued", "message_unknown"]
    );
}

/// hook으로 아는 harness 상태, 확인창 질문의 원격 응답, 메시지 queue·interrupt(§8.3~8.5)
#[test]
fn hooked_thread_questions_and_messages() {
    let w = world();
    let v =
        w.start(&["--title", "hooked", "--profile", "hooked", "--kind", "dir", "--task-file", &w.task("t5.txt", "x")]);
    let id = v["id"].as_str().unwrap().to_string();
    let pane = v["pane"].as_str().unwrap().to_string();

    // 처음 실행 확인창(폴더 신뢰)은 입력하지 않고 질문으로 올린다
    let q = w.wait_question(&id, "trust");
    assert_eq!(q["text"], "Quick safety check: Do you trust the files in this folder?");
    assert_eq!(w.show(&id)["brief"], "pending");
    // 고를 수 없는 답은 거부한다
    let e = w.h.json(&["question", "answer", q["id"].as_str().unwrap(), "maybe"]).expect_err("없는 답");
    assert!(e.contains("yes, no"), "{e}");
    let a = w.h.ok(&["question", "answer", q["id"].as_str().unwrap(), "yes"]);
    assert_eq!((a["state"].as_str(), a["answer"].as_str()), (Some("answered"), Some("yes")));
    // 기본 선택(No, exit)이 아니라 Yes까지 옮겨 골랐다
    w.wait_screen(&pane, "marker: none");

    // 답한 뒤 brief 안내가 가고, harness가 받았다고(UserPromptSubmit) 알린다. 이어 권한 확인창이 뜬다.
    let q = w.wait_question(&id, "permission");
    let v = w.show(&id);
    assert_eq!(v["brief"], "delivered", "{v}");
    let ag = &v["agent"];
    assert_eq!((ag["activity"].as_str(), ag["harness"].as_str()), (Some("question"), Some("claude")), "{v}");
    assert_eq!((ag["percent"].as_u64(), ag["progress"].as_str()), (Some(40), Some("권한 확인 중")), "{v}");
    assert_eq!(ag["harness_session"], "fake-1");
    // PermissionRequest와 Notification이 함께 와도 질문은 하나다
    assert_eq!(w.questions(false).len(), 1);
    // pane 목록에도 상태가 보인다(polling 없이 hook으로만 바뀐다)
    assert_eq!(w.h.pane(&pane)["agent"]["activity"], "question");
    let qid = q["id"].as_str().unwrap().to_string();
    w.h.ok(&["question", "answer", &qid, "yes"]);
    let e = w.h.json(&["question", "answer", &qid, "yes"]).expect_err("두 번 답하기");
    assert!(e.contains("닫힌"), "{e}");
    let v = w.wait_show("보고", &id, |v| v["report_hash"].is_string() && v["agent"]["activity"] == "idle");
    assert_eq!(v["agent"]["message"], "끝");
    assert!(w.h.screen(&pane).contains("allowed"));

    // queue: 지금 기다리는 중이면 바로 받는다. 일하는 중에 온 것은 턴이 끝난 뒤 보낸다.
    let m1 = w.message(&id, "slow one", false);
    assert_eq!(m1["state"], "accepted", "{m1}");
    let m2 = w.message(&id, "second", false);
    assert_eq!(m2["state"], "queued", "{m2}");
    let screen = w.wait_screen(&pane, "got: second");
    assert!(screen.find("slow done").unwrap() < screen.find("got: second").unwrap(), "{screen}");
    // 화면 출력이 Stop hook보다 먼저 보일 수 있다: 입력 대기(idle)를 확인한 뒤 다음을 보낸다
    w.wait_show("idle", &id, |v| v["agent"]["activity"] == "idle");

    // 거절(Esc)
    let m3 = w.message(&id, "ask again", false);
    assert_eq!(m3["state"], "accepted", "{m3}");
    let q = w.wait_question(&id, "permission");
    // 확인창이 열려 있으면 interrupt는 거부하고 기록도 남기지 않는다(Esc가 확인창을 닫기 때문)
    let e =
        w.h.json(&["thread", "message", "demo", &id, "--interrupt", "--text", "NEVER-SENT"])
            .expect_err("질문 중 interrupt");
    assert!(e.contains("확인창"), "{e}");
    w.h.ok(&["question", "answer", q["id"].as_str().unwrap(), "no"]);
    w.wait_screen(&pane, "denied");
    // 거절 뒤 Stop hook이 없어도 입력 대기로 본다
    let v = w.wait_show("idle", &id, |v| v["agent"]["activity"] == "idle");
    assert_eq!(v["agent"]["message"], "거절: 입력을 기다린다");

    // interrupt: 일하는 중이면 Esc로 멈춘 뒤 보낸다. 멈춤은 Stop hook이 없어 화면(「Interrupted」)으로 안다.
    assert_eq!(w.message(&id, "long run", false)["state"], "accepted");
    w.wait_screen(&pane, "long start");
    let t0 = std::time::Instant::now();
    let m4 = w.message(&id, "urgent", true);
    assert_eq!(m4["state"], "accepted", "{m4}");
    assert!(t0.elapsed() < Duration::from_secs(10), "멈춤을 화면으로 알아야 한다: {:?}", t0.elapsed());
    let screen = w.wait_screen(&pane, "got: urgent");
    assert!(screen.contains("Interrupted") && !screen.contains("long done"), "{screen}");
    // 두 번째 interrupt: 지난 「Interrupted」가 화면에 남아 있어도 새로 멈출 때까지 기다린다
    w.wait_show("idle", &id, |v| v["agent"]["activity"] == "idle");
    assert_eq!(w.message(&id, "long again", false)["state"], "accepted");
    w.wait_for_count(&pane, "long start", 2);
    let t0 = std::time::Instant::now();
    let m5 = w.message(&id, "urgent2", true);
    assert_eq!(m5["state"], "accepted", "{m5}");
    assert!(t0.elapsed() < Duration::from_secs(10), "{:?}", t0.elapsed());
    let screen = w.wait_screen(&pane, "got: urgent2");
    assert!(!screen.contains("long done"), "{screen}");
    assert_eq!(screen.matches("Interrupted").count(), 3, "{screen}");
    // thread stop: Esc 한 번(세션은 그대로)
    w.wait_show("idle", &id, |v| v["agent"]["activity"] == "idle");
    assert_eq!(w.message(&id, "long three", false)["state"], "accepted");
    w.wait_for_count(&pane, "long start", 3);
    w.h.ok(&["thread", "stop", "demo", &id]);
    w.wait_for_count(&pane, "Interrupted", 4);
    assert_eq!(w.h.pane(&pane)["state"], "running");
    assert!(!screen.contains("NEVER-SENT"), "{screen}");

    let all = w.questions(true);
    let got: Vec<_> =
        all.iter().map(|q| (q["question_kind"].as_str().unwrap(), q["answer"].as_str().unwrap_or("-"))).collect();
    assert_eq!(got, [("trust", "yes"), ("permission", "yes"), ("permission", "no")]);
    let kinds = w.kinds(0);
    let msg: Vec<_> = kinds.iter().filter(|k| k.starts_with("message_") || k.starts_with("question_")).collect();
    assert_eq!(
        msg,
        [
            "question_shown",
            "question_answered",
            "question_shown",
            "question_answered",
            "message_queued",
            "message_accepted",
            "message_queued",
            "message_accepted",
            "message_queued",
            "message_accepted",
            "question_shown",
            "question_answered",
            "message_queued",
            "message_accepted",
            "message_queued",
            "message_accepted",
            "message_queued",
            "message_accepted",
            "message_queued",
            "message_accepted",
            "message_queued",
            "message_accepted",
        ]
    );
}

/// 결정 기록이 brief에 들어가고, 옛 결정으로 일하는 thread가 보인다. 끝난 세션은 다시 시작한다(새 brief·resume).
#[test]
fn decisions_and_restart() {
    let w = world();
    let d = w.h.ok(&["decision", "add", "demo", "--text", "API는 v2로 한다"]);
    assert_eq!(d["num"], 1);
    let e = w.h.json(&["decision", "add", "demo", "--text", "x", "--replaces", "9"]).expect_err("없는 번호 대체");
    assert!(e.contains("대체할 결정이 없다"), "{e}");

    let v = w.start(&[
        "--title",
        "dec",
        "--profile",
        "fake",
        "--kind",
        "dir",
        "--task-file",
        &w.task("t6.txt", "결정 확인"),
    ]);
    let id = v["id"].as_str().unwrap().to_string();
    let pane = v["pane"].as_str().unwrap().to_string();
    let brief_path = PathBuf::from(v["work_dir"].as_str().unwrap()).join("brief.md");
    assert!(std::fs::read_to_string(&brief_path).unwrap().contains("API는 v2로 한다"));
    let v = w.wait_show("보고", &id, |v| v["report_hash"].is_string());
    assert_eq!((v["decisions_rev"].as_u64(), v["decisions_now"].as_u64()), (Some(1), Some(1)));

    // 결정이 바뀌면 옛 결정으로 일하는 thread가 보인다
    w.h.ok(&["decision", "add", "demo", "--text", "API는 v3로 바꾼다", "--replaces", "1"]);
    let list = w.h.ok(&["decision", "list", "demo"]);
    assert_eq!(list["decisions"][0]["replaced_by"], 2);
    let text = String::from_utf8(w.h.cmd(&["thread", "show", "demo", &id]).output().unwrap().stdout).unwrap();
    assert!(text.contains("결정 1번까지 보고 시작했다(지금 2번)"), "{text}");

    // 실행 중이면 다시 시작하지 않는다
    let e = w.h.json(&["thread", "restart", "demo", &id]).expect_err("실행 중 restart");
    assert!(e.contains("실행 중"), "{e}");
    // 새 보고는 「검토 대기」가 먼저다. 확인한 뒤 세션이 끝나면 「사람 차례」다.
    w.h.ok(&["thread", "ack", "demo", &id]);
    w.h.ok(&["pane", "kill", &pane]);
    w.wait_show("세션 끝남 → 사람 차례", &id, |v| v["group"] == "waiting_on_you");

    // 새로 시작: task·결정을 다시 읽어 brief를 새로 쓰고 안내한다
    let v = w.h.ok(&["thread", "restart", "demo", &id]);
    let pane2 = v["pane"].as_str().unwrap().to_string();
    assert_ne!(pane2, pane);
    let brief = std::fs::read_to_string(&brief_path).unwrap();
    assert!(brief.starts_with("> 이 thread는 다시 시작했다") && brief.contains("API는 v3로 바꾼다"), "{brief}");
    let v = w.wait_show("다시 안내", &id, |v| v["brief"] == "delivered" && v["decisions_rev"] == 2);
    assert_eq!(v["group"], "working");
    let seen = w.h.ok(&["events", "--project", "demo"])["events"].as_array().unwrap().len() as u64;

    // resume: 같은 대화를 잇는다(custom harness는 같은 명령). brief 안내는 하지 않는다.
    w.h.ok(&["pane", "kill", &pane2]);
    wait_for("세션 끝남", T, || (w.h.pane(&pane2)["state"] != "running").then_some(()));
    let v = w.h.ok(&["thread", "restart", "demo", &id, "--resume"]);
    let pane3 = v["pane"].as_str().unwrap().to_string();
    wait_for("resume 세션", T, || (w.h.pane(&pane3)["state"] == "running").then_some(()));
    std::thread::sleep(Duration::from_secs(4));
    assert_eq!(w.kinds(seen), ["thread_restarted"]);
    let all = w.kinds(0);
    assert!(all.contains(&"decision_added".to_string()), "{all:?}");
}

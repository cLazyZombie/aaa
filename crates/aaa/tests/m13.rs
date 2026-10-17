//! M1-3 E2E: 프로젝트·thread 시작(그 머신 agent가 worktree·작업 폴더를 만든다)·brief 안내·보고 회수·ack·resolve·흐름 사건,
//! harness 상태(hook)·확인창 질문 원격 응답·메시지(queue·interrupt).
//! agent harness는 시험용 가짜 agent(셸 스크립트)다. brief 안내 한 줄을 받으면 brief를 읽고 report.md·library를 쓴다.
//! 가짜 Claude(`hooked`)는 Claude Code처럼 hook 사건을 `aaa hook --agent claude`로 보내고 확인창을 띄운다.
//! 실행: `cargo test -p aaa --test m13`

#![cfg(unix)]

mod lab;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

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
      task=$(awk '/^# 할 일/{f=1;next} /^# 경로/{f=0} f' "$path" | sed '/^$/d')
      echo "결과물" > "$dir/library/out.txt"
      mkdir -p "$dir/library/sub" && echo deep > "$dir/library/sub/d.txt"
      ln -s /etc/hosts "$dir/library/link"
      printf '## Report\n%s\n\n## Next\n- 결과 확인\n- 다음 단계\n' "$task" > "$dir/report.md.tmp"
      mv "$dir/report.md.tmp" "$dir/report.md"
      case "$task" in
        *commit*) echo change > change.txt && git add change.txt && git -c user.name=t -c user.email=t@example.com commit -qm change ;;
      esac
      case "$task" in
        *dirty*) echo dirty > dirty.txt ;;
      esac
      case "$task" in
        *big*)
          head -c 2097152 /dev/urandom > "$dir/library/big.bin"
          printf '<!doctype html><p>보고</p><img src="sub/d.txt">' > "$dir/library/index.html" ;;
      esac
      echo "done"
      ;;
    *inbox/*.json*)
      # 사후 평가자 역할: 사건 묶음을 읽고 평가 기록을 쓴 뒤 report.md를 다시 쓴다
      f=$(printf '%s' "$line" | tr ' ' '\n' | grep 'inbox/.*[.]json$' | head -n 1)
      dir=$(dirname "$(dirname "$f")")
      name=$(basename "$f" .json)
      mkdir -p "$dir/library/evaluations"
      printf '{"targets":["t-0002"],"batches":["%s"],"summary":"묶음 %s를 봤다","stages":[{"thread":"t-0002","stage":"전체","minutes":1.4}],"proposals":[{"id":"p1","text":"brief 확인을 자동으로"},{"id":"p2","text":"보고 형식 검사"}]}' "$name" "$name" > "$dir/library/evaluations/e-$name.json"
      [ -f "$dir/library/evaluations/bad.json" ] || printf '{"proposals":[]}' > "$dir/library/evaluations/bad.json"
      echo "$name" >> "$dir/processed.txt"
      printf '## Report\n평가 %s\n\n## Next\n- 다음 묶음\n' "$name" > "$dir/report.md.tmp"
      mv "$dir/report.md.tmp" "$dir/report.md"
      echo "evaluated $name"
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
      printf '## Report\n%s\n\n## Next\n- 확인\n' "$pick" > "$dir/report.md"
      ;;
    *ask*) perm ;;
    *slow*)
      echo "slow start"
      # 중첩 세션(다른 session_id)과 하위 agent의 Stop: 이 pane의 턴은 끝나지 않았다(버려야 한다)
      hook '{"hook_event_name":"Stop","session_id":"fake-nested"}'
      hook '{"hook_event_name":"Stop","session_id":"fake-1","agent_id":"sub-1"}'
      echo "nested sent"; sleep 3; echo "slow done" ;;
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

/// Claude Code처럼 입력창(위 테두리·`❯ `·아래 테두리)을 그리는 harness(hook 없음, profile harness는 claude). 첫 Enter는
/// 무시해 brief 줄이 입력창에 남는다. 다음 Enter부터 받아 대화에 `❯ <줄>`·`got: <줄>`을 남기고 입력창을 비운다.
/// 화면은 바뀔 때마다 통째로 다시 그린다(받은 조각을 한 번에 그려 느린 머신에서도 그리는 중에 Enter가 오지 않는다).
const STICKY_AGENT: &str = r#"#!/bin/sh
stty -icanon -echo min 1 time 0
R=$(printf '%.0s\342\224\200' 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30)
log=''; buf=''; n=0
draw() { printf '\033[2J\033[H%s%s\n\342\235\257 %s\n%s\n' "$log" "$R" "$buf" "$R"; }
draw
while :; do
  c=$(dd bs=4096 count=1 2>/dev/null)
  if [ -z "$c" ]; then
    n=$((n+1))
    [ "$n" -lt 2 ] && continue
    log="$log$(printf '\342\235\257 %s\ngot: %s' "$buf" "$buf")
"
    buf=''
  else
    buf="$buf$c"
  fi
  draw
done
"#;

/// 프로젝트 폴더를 cwd로 쓰는 coordinator(hook 없음). 입력창은 Claude처럼 「❯ 」다.
const COORD_AGENT: &str = "#!/bin/sh\nprintf '\\342\\235\\257 '\nwhile IFS= read -r line; do\n  echo \"got: $line\"\n  printf '\\342\\235\\257 '\ndone\n";

/// `extra`는 `[projects]`에 덧붙인다. `web`이면 웹을 loopback 임의 port로 연다.
fn host_toml(projects: &Path, extra: &str, web: bool) -> String {
    let listen = if web { "127.0.0.1:0" } else { "" };
    format!(
        "schema = 1\n[host]\nagent_listen = \"127.0.0.1:0\"\n[web]\nlisten = \"{listen}\"\n[local_agent]\nmode = \"none\"\n\
         [projects]\nroot = \"{}\"\nallow_profiles = [\"fake\", \"mute\", \"hooked\", \"sticky\"]\nlibrary_copy_mb = 1\n{extra}",
        path_str(projects)
    )
}

fn agent_toml(work_root: &Path, scripts: &Path) -> String {
    format!(
        "schema = 1\n[agent]\nwork_root = \"{}\"\n\
         [profiles.fake]\nharness = \"custom\"\ncommand = [\"/bin/sh\", \"{}\"]\n\
         [profiles.mute]\nharness = \"custom\"\ncommand = [\"/bin/sh\", \"{}\"]\nenv = {{ MUTE_LOG = \"{}\" }}\n\
         [profiles.hooked]\nharness = \"custom\"\ncommand = [\"/bin/sh\", \"{}\"]\nenv = {{ AA = \"{}\" }}\n\
         [profiles.sticky]\nharness = \"claude\"\ncommand = [\"{}\"]\n\
         [profiles.other]\nharness = \"custom\"\ncommand = [\"/bin/sh\"]\n",
        path_str(work_root),
        path_str(&scripts.join("fake.sh")),
        path_str(&scripts.join("mute.sh")),
        path_str(&scripts.join("mute.log")),
        path_str(&scripts.join("hooked.sh")),
        lab::BIN,
        path_str(&scripts.join("sticky.sh")),
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
    world_with("")
}

fn world_with(extra: &str) -> World {
    world_opts(extra, false)
}

fn world_opts(extra: &str, web: bool) -> World {
    let lab = Lab::new();
    let projects = lab.root.join("projects");
    let scripts = lab.root.join("scripts");
    std::fs::create_dir_all(&scripts).unwrap();
    std::fs::write(scripts.join("fake.sh"), FAKE_AGENT).unwrap();
    std::fs::write(scripts.join("mute.sh"), MUTE_AGENT).unwrap();
    std::fs::write(scripts.join("hooked.sh"), HOOKED_AGENT).unwrap();
    std::fs::write(scripts.join("sticky.sh"), STICKY_AGENT).unwrap();
    // harness claude는 실행 파일 뒤에 `--settings …`·`--session-id …`를 붙이므로 sh가 아니라 스크립트를 바로 띄운다
    std::fs::set_permissions(scripts.join("sticky.sh"), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    std::fs::write(scripts.join("coord.sh"), COORD_AGENT).unwrap();
    let repo = make_repo(&lab.root);
    let h = lab.node("h", Some(&host_toml(&projects, extra, web)));
    std::fs::write(h.cfg.join("agent.toml"), agent_toml(&lab.root.join("workA"), &scripts)).unwrap();
    let mut w = World { lab, h, projects, repo, scripts, addr: String::new() };
    w.lab.spawn("host", &w.h, &["host"]);
    let st = wait_for("host 준비", T, || w.h.json(&["status"]).ok());
    w.addr = st["agent_listen"].as_str().unwrap().to_string();
    // agent를 Claude Code 세션 안에서 띄운 경우: 그 세션 표지는 aaa 세션에 넘기지 않는다
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
            || format!("thread: {}\nevents: {}", self.show(id), self.h.ok(&["events", "demo"])),
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
        let v = self.h.ok(&["events", "demo", "--after", &after.to_string()]);
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
    assert_eq!(v["branch"], "aaa/demo/t-0001-fix-the-bug");
    let cwd = PathBuf::from(v["cwd"].as_str().unwrap());
    assert_eq!(cwd, w.lab.root.join("workA").join("repo").join("demo-t-0001"));
    assert!(cwd.join(".aaa/demo-t-0001/brief.md").is_file());
    let brief = std::fs::read_to_string(cwd.join(".aaa/demo-t-0001/brief.md")).unwrap();
    assert!(brief.contains("X-42") && brief.contains("# thread brief") && brief.contains("# 프로젝트 지침"), "{brief}");
    // 작업 폴더·aaa 실행 파일 자리는 agent가 그 머신 값으로 채웠다(plan §8.78)
    let work = cwd.join(".aaa/demo-t-0001");
    assert!(brief.contains(&format!("- 보고: `{}/report.md`", work.display())), "{brief}");
    assert!(brief.contains(&format!("`{} report --percent 25", lab::BIN)), "{brief}");
    assert!(!brief.contains("{{aaa:") && !brief.contains("[진행 중]"), "{brief}");
    assert!(w.projects.join("demo/threads/t-0001.task.md").is_file());

    let v = w.wait_show("보고 사본", "t-0001", |v| v["report_hash"].is_string());
    assert_eq!(v["brief"], "delivered", "{v}");
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

    // 확인한 뒤 harness가 일하지 않으면 「쉼」이다(herdr-projects `group()` 7번 줄)
    let v = w.h.ok(&["thread", "ack", "demo", "t-0001"]);
    assert_eq!(v["group"], "idle");

    let v = w.h.ok(&["thread", "resolve", "demo", "t-0001"]);
    assert_eq!(v["status"], "resolved");
    // 끝난 thread도 목록에 남는다(herdr-projects `thread list <slug>`, `--all` 없음). 프로젝트는 위치 인자다.
    let listed = w.h.ok(&["thread", "list", "demo"]);
    assert_eq!(listed["threads"][0]["group"], "resolved", "{listed}");
    let text = String::from_utf8_lossy(&w.h.run(&["thread", "list", "demo"]).stdout).to_string();
    assert!(text.starts_with("demo/t-0001\t끝남\t"), "{text}");
    assert!(!cwd.exists(), "고치지 않은 worktree는 지워져야 한다");
    assert_eq!(git(&w.repo, &["branch", "--list", "aaa/demo/*"]), "", "병합된(새 commit 없는) branch는 지운다");
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
    assert_eq!(git(&w.repo, &["branch", "--list", "aaa/demo/*", "--format=%(refname:short)"]), "aaa/demo/t-0001-keep");
    let ev = w.h.ok(&["events", "demo"]);
    let last = ev["events"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(last["kind"], "thread_resolved");
    assert!(last["detail"].as_str().unwrap().contains("worktree 유지"), "{last}");
}

/// 원격(TLS) 머신의 작업 폴더 thread, host 재시작 뒤 보고 감시 재개, 평가자 역할 필터
#[test]
fn remote_dir_thread_survives_host_restart() {
    let mut w = world();
    w.add_remote();
    let connected_at = Instant::now();
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
    let seen = w.h.ok(&["events", "demo"])["events"].as_array().unwrap().len() as u64;

    // host 재시작 중에 보고가 바뀌어도, 다시 붙은 agent가 지금 hash를 알려 사본을 다시 받는다
    // 원격 agent가 같은 주소로 다시 붙게 처음 받은 포트를 고정한다
    // 끊김은 새 host가 처음 본 때부터 잰다(herdr-projects `Outage`): 연결된 지 기준(5초)보다 오래된 머신도
    // 다시 붙는 동안 「오래 끊김」이 되지 않는다. 매초 검사해 붙기 전 검사가 반드시 있게 한다.
    let toml = std::fs::read_to_string(w.h.cfg.join("host.toml")).unwrap();
    let toml = toml
        .replace("127.0.0.1:0", &w.addr)
        .replace("library_copy_mb = 1\n", "library_copy_mb = 1\noutage_secs = 5\ntick_secs = 1\n");
    assert!(toml.contains("outage_secs = 5"), "{toml}");
    std::fs::write(w.h.cfg.join("host.toml"), toml).unwrap();
    std::thread::sleep(Duration::from_secs(7).saturating_sub(connected_at.elapsed()));
    w.lab.kill("host");
    // host가 없는 동안 agent 머신에서 보고가 바뀐다
    let work_dir = PathBuf::from(v["work_dir"].as_str().unwrap());
    std::fs::write(work_dir.join("report.md"), "## Report\n갱신\n\n## Next\n- 사용자 확인\n").unwrap();
    w.lab.spawn("host2", &w.h, &["host"]);
    wait_for("labB 재연결", Duration::from_secs(60), || connected(&w.h, "labB").then_some(()));
    let v = w.wait_show("갱신된 보고", &id, |v| v["next"] == serde_json::json!(["사용자 확인"]));
    // 보고 첫 줄 태그로 사람 차례를 정하지 않는다(D9): 새 보고는 검토 대기
    assert_eq!(v["group"], "ready_for_review");
    assert_eq!(w.kinds(seen), ["report_ready"]);
    std::thread::sleep(Duration::from_secs(2));
    let items = w.h.ok(&["inbox", "list", "demo", "--all", "--peek"])["items"].as_array().unwrap().clone();
    assert!(!items.iter().any(|i| i["item_kind"] == "outage"), "host 재시작에 outage가 생겼다: {items:?}");

    // 머신이 끊긴 채 host를 다시 띄워도 기준(5초) 전에는 알리지 않는다. 옛 코드는 마지막 연결 시각(7초 넘게 전)부터
    // 재서 첫 검사에 바로 알렸다. 기준 뒤 한 번, 돌아오면 한 번 알린다.
    w.lab.kill("agentB");
    w.lab.kill("host2");
    w.lab.spawn("host3", &w.h, &["host"]);
    let started = Instant::now();
    let outage = |event: &str| {
        let items = w.h.ok(&["inbox", "list", "demo", "--all", "--peek"])["items"].as_array().unwrap().clone();
        items.iter().filter(|i| i["item_kind"] == "outage" && i["event"] == event).count()
    };
    wait_for("host3 응답", T, || w.h.json(&["inbox", "list", "demo", "--peek"]).ok());
    std::thread::sleep(Duration::from_millis(2500));
    assert_eq!(outage("down"), 0, "host가 끊김을 본 지 기준 전인데 outage가 생겼다");
    assert!(started.elapsed() < Duration::from_secs(5), "기준 전 확인이 늦었다: {:?}", started.elapsed());
    wait_for("outage down", T, || (outage("down") == 1).then_some(()));
    let b = w.lab.node("b", None);
    w.lab.spawn("agentB2", &b, &["agent", "--host", &w.addr, "--name", "labB"]);
    wait_for("outage recovered", T, || (outage("recovered") == 1).then_some(()));
    assert_eq!(outage("down"), 1);
}

/// 안내를 보내도 화면이 바뀌지 않으면 unknown으로 남기고 자동으로 다시 보내지 않는다(§8.5 a)
/// 입력창을 읽을 수 없는 harness(`mute`: 화면이 바뀌지 않는다): ticker는 한 번 보내고, 다시 보내기 전에 화면으로 앞
/// 시도를 확인할 수 없어 멈춘다(herdr-projects brief.rs `give_up`). 사람이 `thread brief`로 보내면 그때만 한 번 더 간다.
#[test]
fn unknown_brief_is_not_resent() {
    let mut w = world();
    let mut ev = w.lab.follow("events", &w.h, "demo");
    let v = w.start(&["--title", "mute", "--profile", "mute", "--kind", "dir", "--task-file", &w.task("t4.txt", "x")]);
    let id = v["id"].as_str().unwrap().to_string();
    let e = ev.kind("brief_unknown", &id, T);
    assert!(e["detail"].as_str().unwrap().contains("입력창을 읽을 수 없어"), "{e}");
    // 배경 task는 멈췄다(brief_unknown 뒤에는 보내는 쪽이 없다): 한 번뿐이다
    let log = w.scripts.join("mute.log");
    let got = std::fs::read_to_string(&log).unwrap();
    assert_eq!(got.matches("brief.md").count(), 1, "{got}");
    assert_eq!(ev.kinds(), ["thread_started", "brief_unknown"]);
    let v = w.show(&id);
    assert_eq!((v["brief"].as_str(), v["group"].as_str()), (Some("unknown"), Some("waiting_on_you")));
    // 사람이 다시 보내면 한 번 더 간다(받았다는 표지가 없어 여전히 모른다)
    let e = w.h.json(&["thread", "brief", "demo", &id]).expect_err("받았는지 모른다");
    assert!(e.contains("아직 보내지 못했다"), "{e}");
    let got = std::fs::read_to_string(&log).unwrap();
    assert_eq!(got.matches("brief.md").count(), 2, "{got}");
    // brief를 받기 전의 메시지는 입력하기 전에 거부한다(brief보다 먼저 가지 않게, herdr-projects threads.rs `prompt`)
    let e = w.h.json(&["thread", "message", "demo", &id, "--text", "MSG-EARLY-1"]).expect_err("brief 전 메시지");
    assert!(e.contains("아직 brief를 받지 않았다"), "{e}");
    assert!(!std::fs::read_to_string(&log).unwrap().contains("MSG-EARLY-1"));
    assert!(!w.kinds(0).iter().any(|k| k.starts_with("message_")), "{:?}", w.kinds(0));
}

/// brief 줄이 입력창에 남으면 ticker는 다시 치지 않고 Enter만 보낸다(herdr-projects brief.rs: 입력창에 남은 줄은
/// Enter, 대화에 보이고 입력창이 비면 받은 것). 받은 thread에는 `thread brief`를 다시 보내지 않는다.
#[test]
fn brief_left_in_box_is_submitted_not_retyped() {
    let mut w = world();
    let mut ev = w.lab.follow("events", &w.h, "demo");
    let v =
        w.start(&["--title", "sticky", "--profile", "sticky", "--kind", "dir", "--task-file", &w.task("t9.txt", "x")]);
    let id = v["id"].as_str().unwrap().to_string();
    let pane = v["pane"].as_str().unwrap().to_string();
    let e = ev.kind("brief_delivered", &id, T);
    assert!(e["detail"].as_str().unwrap().starts_with("입력창에 남은 줄을 Enter로 보냈다(시도 2번)"), "{e}");
    let screen = w.h.screen(&pane);
    let line = format!(".aaa/demo-{id}/brief.md를 읽고 그대로 하라.");
    // 대화의 `❯ <줄>`과 `got: <줄>`에 한 번씩: 다시 쳤으면 입력창에 두 번 이어 붙어 `got:` 줄이 두 배가 된다
    assert!(screen.contains(&format!("got: {line}\n")), "{screen}");
    assert_eq!(screen.matches("brief.md를 읽고").count(), 2, "{screen}");
    let e = w.h.json(&["thread", "brief", "demo", &id]).expect_err("받은 뒤 brief");
    assert!(e.contains("이미 brief를 받았다"), "{e}");
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
    // 중첩 세션·하위 agent의 Stop은 버린다(herdr-projects progress.rs eligible·foreign_session)
    w.wait_screen(&pane, "nested sent");
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(w.h.pane(&pane)["agent"]["activity"], "working", "다른 세션의 Stop으로 턴이 끝난 것으로 보였다");
    let screen = w.wait_screen(&pane, "got: second");
    assert!(screen.find("slow done").unwrap() < screen.find("got: second").unwrap(), "{screen}");
    // 화면 출력이 Stop hook보다 먼저 보일 수 있다: 입력 대기(idle)를 확인한 뒤 다음을 보낸다
    w.wait_show("idle", &id, |v| v["agent"]["activity"] == "idle");
    // 입력한 메시지는 task.md Follow-ups에 남는다(다시 시작하면 brief와 함께 읽는다, herdr-projects append_follow_up)
    let task = std::fs::read_to_string(w.projects.join("demo/threads").join(format!("{id}.task.md"))).unwrap();
    let follow = task.split("\n## Follow-ups\n").nth(1).unwrap_or_else(|| panic!("Follow-ups가 없다: {task}"));
    assert!(follow.contains("\nslow one\n") && follow.find("slow one") < follow.find("second"), "{follow}");

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
    // 닫힌 질문의 inbox 항목은 끝난 것으로 남는다(nudge에 섞이지 않는다)
    let items = w.h.ok(&["inbox", "list", "demo", "--all", "--peek"])["items"].as_array().unwrap().clone();
    let qs: Vec<_> = items.iter().filter(|i| i["item_kind"] == "question").collect();
    assert_eq!(qs.len(), 3, "{items:?}");
    assert!(qs.iter().all(|i| i["done"] == true), "{qs:?}");
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
    // 보고를 쓴 뒤 닫힌 세션은 끝난 일이다: 사람 차례가 아니다(herdr-projects `group()` 3번 줄)
    w.h.ok(&["thread", "ack", "demo", &id]);
    w.h.ok(&["pane", "kill", &pane]);
    wait_for("세션 끝남", T, || (w.h.pane(&pane)["state"] != "running").then_some(()));
    assert_eq!(w.show(&id)["group"], "idle");

    // 새로 시작: task·결정을 다시 읽어 brief를 새로 쓰고 안내한다
    let v = w.h.ok(&["thread", "restart", "demo", &id]);
    let pane2 = v["pane"].as_str().unwrap().to_string();
    assert_ne!(pane2, pane);
    let brief = std::fs::read_to_string(&brief_path).unwrap();
    // 다시 시작 안내는 thread 규칙 다음, 프로젝트 지침 앞이다(herdr-projects `compose_brief`)
    let at = |n: &str| brief.find(n).unwrap_or_else(|| panic!("{n} 없음: {brief}"));
    assert!(
        at("# thread brief") < at("**이 일을 앞서 시도한 것이")
            && at("**이 일을 앞서 시도한 것이") < at("# 프로젝트 지침")
    );
    assert!(brief.contains("API는 v3로 바꾼다"), "{brief}");
    let v = w.wait_show("다시 안내", &id, |v| v["brief"] == "delivered" && v["decisions_rev"] == 2);
    // hook이 없는 harness는 일하는지 알 수 없다: 안내를 보낸 뒤에는 「쉼」이다(herdr-projects `group()` 7번 줄)
    assert_eq!(v["group"], "idle");
    let seen = w.h.ok(&["events", "demo"])["events"].as_array().unwrap().len() as u64;

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

/// inbox: 흐름 사건·세션 끝남·설정 오류가 항목이 되고, coordinator가 입력을 기다릴 때 처음 보는 묶음을 한 번만 알린다(§8.17)
#[test]
fn inbox_and_nudge() {
    let w = world_with("tick_secs = 1\nnudge_idle_secs = 2\nnudge_quiet_secs = 1\n");
    let dir = w.projects.join("demo");
    let coord =
        w.h.new_pane(None, "labA", None, &["/bin/sh", path_str(&w.scripts.join("coord.sh"))], Some(path_str(&dir)));
    w.wait_screen(&coord, "❯");
    // 사람이 입력창에 쓰던 글이 있으면 알리지 않는다
    w.h.ok(&["pane", "send-text", &coord, "draft-text"]);
    w.wait_screen(&coord, "❯ draft-text");

    let v = w.start(&["--title", "inbox", "--profile", "fake", "--kind", "dir", "--task-file", &w.task("t7.txt", "x")]);
    let id = v["id"].as_str().unwrap().to_string();
    let pane = v["pane"].as_str().unwrap().to_string();
    w.wait_show("보고", &id, |v| v["report_hash"].is_string());
    let items = w.h.ok(&["inbox", "list", "demo", "--peek"])["items"].as_array().unwrap().clone();
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!((items[0]["item_kind"].as_str(), items[0]["event"].as_str()), (Some("report"), Some("report ready")));
    std::thread::sleep(Duration::from_secs(5));
    assert!(!w.h.screen(&coord).contains("[aaa inbox]"), "{}", w.h.screen(&coord));

    // 쓰던 글을 보내고 나면 알린다
    w.h.ok(&["pane", "send-keys", &coord, "enter"]);
    w.wait_screen(&coord, "got: draft-text");
    let screen = w.wait_screen(&coord, "got: [aaa inbox]");
    assert!(screen.contains(&format!("got: [aaa inbox] {id} report ready (aaa inbox list demo)")), "{screen}");
    // 같은 묶음은 다시 보내지 않는다
    std::thread::sleep(Duration::from_secs(4));
    assert_eq!(w.h.screen(&coord).matches("got: [aaa inbox]").count(), 1, "{}", w.h.screen(&coord));

    // context가 보인 항목은 「봤다」가 되어 다음 알림에 다시 나오지 않는다(herdr-projects `context`).
    // 세션이 끝나면 새 항목만 알린다.
    let c = w.h.ok(&["context", "demo", "--peek"]);
    assert_eq!(c["inbox"].as_array().unwrap().len(), 1, "{c}");
    assert_eq!(c["inbox"][0]["seen"], false);
    w.h.ok(&["context", "demo"]);
    assert_eq!(w.h.ok(&["inbox", "list", "demo", "--peek"])["items"][0]["seen"], true);
    w.h.ok(&["pane", "kill", &pane]);
    w.wait_for_count(&coord, "got: [aaa inbox]", 2);
    let screen = w.h.screen(&coord);
    assert!(screen.contains(&format!("got: [aaa inbox] {id} session ended (aaa inbox list demo)")), "{screen}");

    // 보면(peek 아님) 「봤다」가 되고, 더 알리지 않는다
    let items = w.h.ok(&["inbox", "list", "demo"])["items"].as_array().unwrap().clone();
    assert_eq!(items.len(), 2);
    let items = w.h.ok(&["inbox", "list", "demo", "--peek"])["items"].as_array().unwrap().clone();
    assert!(items.iter().all(|i| i["seen"] == true), "{items:?}");
    let inbox = dir.join("inbox");
    let files = |d: &Path| -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(d)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        v.sort();
        v
    };
    assert_eq!(files(&inbox).len(), 2);
    let report_id = items.iter().find(|i| i["item_kind"] == "report").unwrap()["id"].as_str().unwrap().to_string();
    let md = std::fs::read_to_string(inbox.join(format!("{report_id}.md"))).unwrap();
    assert!(
        md.starts_with("+++\n") && md.contains("kind = \"report\"") && md.contains("event = \"report ready\""),
        "{md}"
    );

    // 끝내기: 파일은 inbox/done으로
    let e = w.h.json(&["inbox", "done", "demo", "no-such-item"]).expect_err("없는 항목");
    assert!(e.contains("no-such-item"), "{e}");
    let done = w.h.ok(&["inbox", "done", "demo", &report_id]);
    assert_eq!((done["done"].as_u64(), done["items"].as_array().unwrap().len()), (Some(1), 1), "{done}");
    // 끝낸 수를 말한다(herdr-projects 「N item(s) moved to inbox/done」)
    let out = w.h.run(&["inbox", "done", "demo", "--all"]);
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1개 항목을 끝냈다(inbox/done으로 옮김)\n");
    assert!(w.h.ok(&["inbox", "list", "demo"])["items"].as_array().unwrap().is_empty());
    assert_eq!(w.h.ok(&["inbox", "list", "demo", "--all"])["items"].as_array().unwrap().len(), 2);
    assert!(files(&inbox).is_empty());
    assert_eq!(files(&inbox.join("done")).len(), 2);

    // project.toml을 읽지 못하면 내용당 한 번 알린다
    std::fs::write(dir.join("project.toml"), "schema = [\n").unwrap();
    let screen = w.wait_screen(&coord, "got: [aaa inbox] project.toml not usable");
    assert!(screen.contains("got: [aaa inbox] project.toml not usable (aaa inbox list demo)"), "{screen}");
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(w.h.ok(&["inbox", "list", "demo", "--peek"])["items"].as_array().unwrap().len(), 1);
    let nudges = w.kinds(0).iter().filter(|k| *k == "nudge_sent").count();
    assert_eq!(nudges, 3);
    assert_eq!(w.h.screen(&coord).matches("got: [aaa inbox]").count(), 3);
}

/// 병합 대기열 순번·사건(§8.8)과 머신별 동시 thread 한도(§8.10)
/// thread 범위(§8.25): 일반 thread 세션 안의 CLI는 보기만 되고, thread가 아닌 pane(사람의 셸) 안의 CLI는 그대로 조작한다
#[test]
fn worker_session_cli_is_read_only() {
    let w = world();
    let t = w.start(&["--title", "w", "--profile", "fake", "--kind", "dir", "--task-file", &w.task("w.txt", "x")]);
    let (id, pane) = (t["id"].as_str().unwrap(), t["pane"].as_str().unwrap());
    let shell = w.h.new_pane(None, "labA", Some("sh"), &["/bin/sh"], None);
    let as_session = |s: &str, args: &[&str]| w.h.cmd(args).env("AAA_SESSION_ID", s).output().unwrap();
    for args in [
        &["decision", "add", "demo", "--text", "thread가 정한 결정"][..],
        &["thread", "message", "demo", id, "--text", "x"],
        &["pane", "send-text", &shell, "echo hi"],
        &["merge", "enqueue", "demo", id],
    ] {
        let o = as_session(pane, args);
        assert_eq!(o.status.code(), Some(1), "{args:?}: {}", String::from_utf8_lossy(&o.stdout));
        assert!(String::from_utf8_lossy(&o.stderr).contains("보기 권한"), "{}", String::from_utf8_lossy(&o.stderr));
    }
    let o = as_session(pane, &["--json", "thread", "show", "demo", id]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    // 사람의 셸(thread가 아닌 pane) 안에서는 조작이 된다
    let o = as_session(&shell, &["--json", "decision", "add", "demo", "--text", "사람이 정한 결정"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let d = w.h.ok(&["decision", "list", "demo"]).to_string();
    assert!(d.contains("사람이 정한 결정") && !d.contains("thread가 정한 결정"), "{d}");
}

/// 병합 단계 자동 실행(§8.24): 순번, 커밋하지 않은 변경, 원격 일치, 병합·tree 같음·push, 사용자 checkout ff-only, 다른 thread 알림
#[test]
fn merge_land_steps() {
    let w = world();
    // agent가 임시 worktree에서 만드는 병합 commit의 이름(사용자 repo 설정을 따른다)
    git(&w.repo, &["config", "user.name", "t"]);
    git(&w.repo, &["config", "user.email", "t@example.com"]);
    let start = |title: &str| {
        w.start(&[
            "--title",
            title,
            "--machine",
            "labA",
            "--profile",
            "fake",
            "--repo",
            path_str(&w.repo),
            "--task-file",
            &w.task(&format!("{title}.txt"), "x"),
        ])
    };
    let t1 = start("land one");
    let t2 = start("land two");
    let (id1, id2) = (t1["id"].as_str().unwrap(), t2["id"].as_str().unwrap());
    let branch = t1["branch"].as_str().unwrap();
    let cwd1 = PathBuf::from(t1["cwd"].as_str().unwrap());
    w.wait_show("보고", id1, |v| v["report_hash"].is_string());
    std::fs::write(cwd1.join("feature.txt"), "one\n").unwrap();
    git(&cwd1, &["add", "feature.txt"]);
    git(&cwd1, &["commit", "-qm", "기능 하나"]);
    w.h.ok(&["merge", "enqueue", "demo", id1]);
    w.h.ok(&["merge", "enqueue", "demo", id2]);
    let fail = |args: &[&str]| -> Value {
        let e = w.h.json(args).expect_err("병합이 멈춰야 한다");
        serde_json::from_str(&e).unwrap_or_else(|_| panic!("{e}"))
    };
    let last_step = |v: &Value| v["steps"].as_array().unwrap().last().unwrap().clone();

    // 순번을 지킨다
    let e = w.h.json(&["merge", "land", "demo", id2]).expect_err("맨 앞이 아니다");
    assert!(e.contains("맨 앞이 아니다"), "{e}");
    // 커밋하지 않은 변경이 있으면 확인 단계에서 멈춘다
    std::fs::write(cwd1.join("README.md"), "changed\n").unwrap();
    let v = fail(&["merge", "land", "demo"]);
    let st = last_step(&v);
    assert_eq!(
        (v["ok"].as_bool(), st["name"].as_str(), st["ok"].as_bool()),
        (Some(false), Some("확인"), Some(false)),
        "{v}"
    );
    git(&cwd1, &["checkout", "--", "README.md"]);
    // 그 사이 main이 앞서 나갔으면(다른 착지) 원격 일치에서 멈추고 대기열은 그대로다
    std::fs::write(w.repo.join("other.txt"), "x\n").unwrap();
    git(&w.repo, &["add", "other.txt"]);
    git(&w.repo, &["commit", "-qm", "다른 착지"]);
    git(&w.repo, &["push", "-q", "origin", "main"]);
    let v = fail(&["merge", "land", "demo", id1]);
    let st = last_step(&v);
    assert_eq!(st["name"], "원격 일치", "{v}");
    assert!(st["note"].as_str().unwrap().contains("포함하지 않는다"), "{v}");
    assert_eq!(v["queue"].as_array().unwrap().len(), 2, "{v}");

    // thread가 main을 합치면 병합된다
    git(&cwd1, &["fetch", "-q", "origin"]);
    git(&cwd1, &["merge", "-q", "--no-edit", "origin/main"]);
    let seen = w.h.ok(&["events", "demo"])["events"].as_array().unwrap().len() as u64;
    let v = w.h.ok(&["merge", "land", "demo"]);
    let steps: Vec<(&str, bool)> = v["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| (s["name"].as_str().unwrap(), s["ok"].as_bool().unwrap()))
        .collect();
    assert_eq!(
        steps,
        [
            ("확인", true),
            ("fetch", true),
            ("원격 일치", true),
            ("병합", true),
            ("tree 같음", true),
            ("push", true),
            ("사용자 checkout", true),
            ("정리", true)
        ],
        "{v}"
    );
    let commit = v["commit"].as_str().unwrap();
    let origin = w.lab.root.join("origin.git");
    assert_eq!(git(&origin, &["rev-parse", "main"]), commit);
    assert_eq!(
        git(&origin, &["rev-list", "--parents", "-n", "1", "main"]).split(' ').count(),
        3,
        "병합 commit(부모 둘)"
    );
    assert_eq!(git(&origin, &["log", "-1", "--format=%s", "main"]), format!("병합: land one (demo/{id1})"));
    assert_eq!(
        git(&origin, &["rev-parse", "main^{tree}"]),
        git(&w.repo, &["rev-parse", &format!("{branch}^{{tree}}")])
    );
    // 사용자 checkout(깨끗한 main)은 ff-only로 따라온다
    assert_eq!(git(&w.repo, &["rev-parse", "main"]), commit);
    assert!(v["steps"][6]["note"].as_str().unwrap().contains("ff-only"), "{v}");
    assert!(!git(&w.repo, &["worktree", "list"]).contains("aaa-land-"), "임시 worktree가 남았다");
    // 대기열에서 빠지고, 같은 기준의 다른 열린 thread에 「main 이동」을 알린다
    assert_eq!(v["notified"], serde_json::json!([id2]));
    let q = v["queue"].as_array().unwrap();
    assert_eq!((q.len(), q[0]["thread"].as_str()), (1, Some(id2)));
    let kinds = w.kinds(seen);
    assert_eq!(kinds.iter().filter(|k| *k == "merge_step").count(), 9, "{kinds:?}");
    assert!(kinds.contains(&"merge_landed".to_string()) && kinds.contains(&"merge_head".to_string()), "{kinds:?}");
    // 새 커밋이 없는 thread는 「포함하지 않는다」(main이 움직였다)로 멈춘다
    let v = fail(&["merge", "land", "demo"]);
    assert_eq!(last_step(&v)["name"], "원격 일치", "{v}");
}

#[test]
fn merge_queue_and_capacity() {
    let w = world_with("max_threads_per_machine = 2\n");
    let start = |n: &str| {
        w.h.json(&[
            "thread",
            "start",
            "demo",
            "--title",
            n,
            "--profile",
            "fake",
            "--kind",
            "dir",
            "--task-file",
            &w.task(&format!("{n}.txt"), "x"),
        ])
    };
    let t1 = start("m1").unwrap()["id"].as_str().unwrap().to_string();
    let t2 = start("m2").unwrap()["id"].as_str().unwrap().to_string();
    let e = start("m3").expect_err("한도");
    assert!(e.contains("한도 2") && e.contains(&format!("demo/{t1}")), "{e}");
    w.wait_show("보고", &t1, |v| v["report_hash"].is_string());
    w.wait_show("보고", &t2, |v| v["report_hash"].is_string());
    let seen = w.h.ok(&["events", "demo"])["events"].as_array().unwrap().len() as u64;

    w.h.ok(&["merge", "enqueue", "demo", &t1]);
    let q = w.h.ok(&["merge", "enqueue", "demo", &t2])["queue"].as_array().unwrap().clone();
    let order: Vec<_> = q.iter().map(|e| (e["position"].as_u64().unwrap(), e["thread"].as_str().unwrap())).collect();
    assert_eq!(order, [(1, t1.as_str()), (2, t2.as_str())]);
    // 이미 있으면 그 자리
    assert_eq!(w.h.ok(&["merge", "enqueue", "demo", &t1])["queue"].as_array().unwrap().len(), 2);
    // 병합을 마치면 다음이 맨 앞
    let q = w.h.ok(&["merge", "done", "demo", &t1])["queue"].as_array().unwrap().clone();
    assert_eq!((q.len(), q[0]["thread"].as_str(), q[0]["position"].as_u64()), (1, Some(t2.as_str()), Some(1)));
    let e = w.h.json(&["merge", "drop", "demo", &t1]).expect_err("없는 항목");
    assert!(e.contains("대기열에 없다"), "{e}");
    // resolve하면 대기열에서 빠지고, 한도도 풀린다
    w.h.ok(&["thread", "resolve", "demo", &t2]);
    assert!(w.h.ok(&["merge", "list", "demo"])["queue"].as_array().unwrap().is_empty());
    let e = w.h.json(&["merge", "enqueue", "demo", &t2]).expect_err("끝난 thread");
    assert!(e.contains("열린 thread만"), "{e}");
    start("m3").expect("한도 안");

    let ev: Vec<(String, String)> = w.h.ok(&["events", "demo", "--after", &seen.to_string()])["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["kind"].as_str().unwrap().starts_with("merge_"))
        .map(|e| (e["kind"].as_str().unwrap().to_string(), e["thread"].as_str().unwrap().to_string()))
        .collect();
    let want: Vec<(String, String)> = [
        ("merge_enqueued", &t1),
        ("merge_head", &t1),
        ("merge_enqueued", &t2),
        ("merge_landed", &t1),
        ("merge_head", &t2),
        ("merge_dropped", &t2),
    ]
    .iter()
    .map(|(k, t)| (k.to_string(), t.to_string()))
    .collect();
    assert_eq!(ev, want);
}

/// routine(§8.9): coordinator가 없으면 건너뛰고, 안내는 처리 전까지 하나만, 명령은 host.toml 허용과 사람의 승인이 있어야
/// 대상 머신 agent가 실행한다. 출력이 바뀔 때만 항목이 되고, 명령을 고치면 다시 승인해야 한다.
#[test]
fn routines() {
    let mut w = world_with("tick_secs = 1\nroutine_commands = [\"demo\"]\n");
    w.add_remote();
    let dir = w.projects.join("demo");
    let put = |name: &str, body: &str| std::fs::write(dir.join("routines").join(format!("{name}.md")), body).unwrap();
    std::fs::write(dir.join("value.txt"), "v1\n").unwrap();
    put("ping", "+++\nschedule = \"every 2s\"\n+++\n상태를 확인한다\n");
    put("cmd", "+++\nschedule = \"every 2s\"\ncommand = \"cat value.txt\"\n+++\n값을 본다\n");
    put("far", "+++\nschedule = \"every 2s\"\ncommand = \"pwd\"\nmachine = \"labB\"\n+++\n");
    put("pr", "+++\non = \"pr\"\n+++\n");
    let items = |kind: &str, subject: &str| -> Vec<Value> {
        w.h.ok(&["inbox", "list", "demo", "--all", "--peek"])["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|i| i["item_kind"] == kind && i["subject"] == subject)
            .cloned()
            .collect()
    };
    let routine = |name: &str| -> Value {
        w.h.ok(&["routine", "list", "demo"])["routines"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["name"] == name)
            .unwrap()
            .clone()
    };

    // coordinator가 없으면 돌지 않고 센다(밀린 실행도 쌓지 않는다)
    wait_for("coordinator 없음", T, || (routine("ping")["no_coordinator"].as_u64() >= Some(2)).then_some(()));
    assert!(items("routine", "ping").is_empty() && items("routine-approval", "cmd").is_empty());
    assert_eq!(items("config-error", "pr").len(), 1);
    assert!(routine("pr")["error"].as_str().unwrap().contains("M3"));

    let coord =
        w.h.new_pane(None, "labA", None, &["/bin/sh", path_str(&w.scripts.join("coord.sh"))], Some(path_str(&dir)));
    w.wait_screen(&coord, "❯");
    // 안내 routine: 처리하지 않은 항목이 있으면 새로 만들지 않고 건너뛴다
    let ping = wait_for("ping 항목", T, || items("routine", "ping").into_iter().next());
    assert_eq!((ping["event"].as_str(), ping["body"].as_str()), (Some("due"), Some("상태를 확인한다")));
    wait_for("ping 건너뜀", T, || (routine("ping")["skipped"].as_u64() >= Some(2)).then_some(()));
    assert_eq!(items("routine", "ping").len(), 1);
    // 승인 전에는 실행하지 않고, 같은 명령에는 「승인 필요」를 한 번만 남긴다
    wait_for("승인 필요", T, || items("routine-approval", "cmd").into_iter().next());
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(items("routine-approval", "cmd").len(), 1);
    assert!(items("routine", "cmd").is_empty());

    // 승인: aaa 세션 안이거나 터미널이 아니면 거부한다
    let mut c = w.h.cmd(&["routine", "approve", "demo", "cmd"]);
    let o = c.env("AAA_SESSION_ID", "x").output().unwrap();
    assert!(String::from_utf8_lossy(&o.stderr).contains("aaa 세션 안에서는"), "{o:?}");
    let o = w.h.cmd(&["routine", "approve", "demo", "cmd"]).env_remove("AAA_SESSION_ID").output().unwrap();
    assert!(String::from_utf8_lossy(&o.stderr).contains("터미널"), "{o:?}");
    // 사람이 터미널에서 이름을 치면 승인된다(시험은 script로 PTY를 만든다)
    let approve = |name: &str, typed: &str| -> String {
        let line = format!("{} routine approve demo {name}", lab::BIN);
        let mut c = Command::new("script");
        if cfg!(target_os = "macos") {
            c.args(["-q", "/dev/null", lab::BIN, "routine", "approve", "demo", name]);
        } else {
            c.args(["-qec", &line, "/dev/null"]);
        }
        c.env("AAA_CONFIG_DIR", &w.h.cfg)
            .env("AAA_STATE_DIR", &w.h.state)
            .env_remove("AAA_SESSION_ID")
            .env_remove("AAA_HOOK_SOCK")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let mut child = c.spawn().unwrap();
        // 프롬프트가 보인 뒤에 친다(script는 입력이 끝나면 바로 EOF를 넣는다)
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
        let mut out = child.stdout.take().unwrap();
        let s2 = seen.clone();
        let reader = std::thread::spawn(move || {
            let mut b = [0u8; 256];
            while let Ok(n) = std::io::Read::read(&mut out, &mut b) {
                if n == 0 {
                    break;
                }
                s2.lock().unwrap().extend_from_slice(&b[..n]);
            }
        });
        let text = || String::from_utf8_lossy(&seen.lock().unwrap()).to_string();
        wait_for_diag("승인 프롬프트", T, || text().contains("이름을 친다").then_some(()), text);
        let mut stdin = child.stdin.take().unwrap();
        std::io::Write::write_all(&mut stdin, format!("{typed}\n").as_bytes()).unwrap();
        let o = child.wait_with_output().unwrap();
        drop(stdin);
        reader.join().unwrap();
        format!("{}{}", text(), String::from_utf8_lossy(&o.stderr))
    };
    let out = approve("cmd", "wrong");
    assert!(out.contains("cat value.txt") && out.contains("이름이 다르다"), "{out}");
    assert_eq!(routine("cmd")["approved"], false);
    let out = approve("cmd", "cmd");
    assert!(!out.contains("aaa:"), "{out}");
    assert_eq!(routine("cmd")["approved"], true);

    // 승인 뒤 실행: 출력이 바뀔 때만 항목이 된다. 출력은 자료로 감싼다.
    let first = wait_for("출력 항목", T, || items("routine", "cmd").into_iter().next());
    assert_eq!(first["event"], "output changed");
    assert!(first["body"].as_str().unwrap().contains("값을 본다\n\n신뢰할 수 없는 명령 출력(종료 코드 0)"), "{first}");
    assert!(first["body"].as_str().unwrap().contains("```text\nv1\n```"), "{first}");
    let md = std::fs::read_to_string(dir.join("inbox").join(format!("{}.md", first["id"].as_str().unwrap()))).unwrap();
    assert!(md.contains("```text\nv1\n```"), "{md}");
    std::thread::sleep(Duration::from_secs(4));
    assert_eq!(items("routine", "cmd").len(), 1);
    std::fs::write(dir.join("value.txt"), "v2\n").unwrap();
    wait_for("두 번째 출력", T, || (items("routine", "cmd").len() == 2).then_some(()));
    // 명령을 고치면 승인이 풀린다
    put("cmd", "+++\nschedule = \"every 2s\"\ncommand = \"cat value.txt; echo more\"\n+++\n값을 본다\n");
    wait_for("다시 승인 필요", T, || (items("routine-approval", "cmd").len() == 2).then_some(()));
    assert_eq!(routine("cmd")["approved"], false);

    // 다른 머신: 그 머신 agent가 자기 작업 폴더(<work_root>/<project>/routines)에서 실행한다
    approve("far", "far");
    let far = wait_for("원격 출력", T, || items("routine", "far").into_iter().next());
    assert!(far["body"].as_str().unwrap().contains("/workB/demo/routines\n"), "{far}");
    let ran = w.kinds(0).iter().filter(|k| *k == "routine_ran").count();
    assert!(ran >= 3, "{:?}", w.kinds(0));
    assert!(w.kinds(0).contains(&"routine_approved".to_string()));
}

/// HTTP/1.1 GET 한 번(Connection: close). (상태, 머리글 소문자, 본문)
fn http_get(addr: &str, path: &str) -> (u16, String, Vec<u8>) {
    use std::io::{Read, Write};
    let mut c = std::net::TcpStream::connect(addr).unwrap();
    c.set_read_timeout(Some(T)).unwrap();
    write!(c, "GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n").unwrap();
    let mut raw = Vec::new();
    c.read_to_end(&mut raw).unwrap();
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").expect("머리글 끝");
    let head = String::from_utf8_lossy(&raw[..split]).to_lowercase();
    let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    (status, head, raw[split + 4..].to_vec())
}

/// 결과물 보기(§8.7): host 사본은 바로, 복사 상한을 넘어 agent에 남긴 파일은 agent에서 스트리밍한다.
/// 보기 경로는 그 thread 결과물만 읽는 짧은 권한이고, 결과물 HTML은 sandbox로 간다.
#[test]
fn library_view() {
    let w = world_opts("", true);
    let web = w.h.ok(&["status"])["web_listen"].as_str().unwrap().to_string();
    let v = w.start(&["--title", "big", "--profile", "fake", "--kind", "dir", "--task-file", &w.task("t9.txt", "big")]);
    let id = v["id"].as_str().unwrap().to_string();
    let agent_lib = PathBuf::from(v["work_dir"].as_str().unwrap()).join("library");
    let v = w.wait_show("보고", &id, |v| v["report_hash"].is_string());
    assert_eq!(v["copy"], "partial", "{v}");

    let lib = w.h.ok(&["thread", "library", "demo", &id]);
    assert!(lib["report"].as_str().unwrap().contains("## Report"), "{lib}");
    let files: Vec<(String, u64, bool)> = lib["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| (f["path"].as_str().unwrap().into(), f["size"].as_u64().unwrap(), f["on_agent"].as_bool().unwrap()))
        .collect();
    assert!(files.contains(&("big.bin".into(), 2_097_152, true)), "{files:?}");
    assert!(files.contains(&("out.txt".into(), 10, false)), "{files:?}");
    // symlink는 목록에 없다
    assert!(!files.iter().any(|f| f.0 == "link"), "{files:?}");
    let base = lib["base"].as_str().unwrap().to_string();

    // host 사본: sandbox CSP, 같은 host 웹 화면 안 frame만
    let (st, head, body) = http_get(&web, &format!("{base}index.html"));
    assert_eq!(st, 200, "{head}");
    assert!(head.contains("content-security-policy: sandbox allow-scripts"), "{head}");
    assert!(head.contains("x-frame-options: sameorigin") && head.contains("text/html"), "{head}");
    assert!(String::from_utf8_lossy(&body).contains("<p>보고</p>"));
    let (st, _, body) = http_get(&web, &format!("{base}sub/d.txt"));
    assert_eq!((st, body.as_slice()), (200, b"deep\n".as_slice()));
    // agent에 남긴 파일: 조각으로 스트리밍한 바이트가 원본과 같다
    let (st, head, body) = http_get(&web, &format!("{base}big.bin"));
    assert_eq!(st, 200, "{head}");
    assert!(head.contains("content-length: 2097152"), "{head}");
    assert!(body == std::fs::read(agent_lib.join("big.bin")).unwrap(), "본문 {}바이트가 원본과 다르다", body.len());

    // 경로 밖·목록 밖·틀린 권한
    let (st, _, _) = http_get(&web, &format!("{base}..%2F..%2Fbrief.md"));
    assert_eq!(st, 400);
    assert_eq!(http_get(&web, &format!("{base}link")).0, 404);
    let (st, _, body) = http_get(&web, "/v/0000/out.txt");
    assert_eq!(st, 401, "{}", String::from_utf8_lossy(&body));
    // agent 쪽 파일이 사라졌으면 기다리지 않고 바로 알린다
    std::fs::remove_file(agent_lib.join("big.bin")).unwrap();
    let t0 = std::time::Instant::now();
    let (st, _, body) = http_get(&web, &format!("{base}big.bin"));
    assert_eq!(st, 502, "{}", String::from_utf8_lossy(&body));
    assert!(String::from_utf8_lossy(&body).contains("agent에 남긴 결과물을 읽지 못한다"));
    assert!(t0.elapsed() < Duration::from_secs(5));
}

/// 알림 webhook(§8.7): 사람이 볼 항목을 debounce 동안 모아 프로젝트마다 한 통 보내고, 실패하면 다시 보내지 않는다.
#[test]
fn notify_webhook() {
    use std::io::{BufRead, BufReader, Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = std::sync::mpsc::channel::<(String, String)>();
    std::thread::spawn(move || {
        for (n, conn) in listener.incoming().enumerate() {
            let Ok(mut c) = conn else { continue };
            let mut r = BufReader::new(c.try_clone().unwrap());
            let mut line = String::new();
            r.read_line(&mut line).unwrap();
            let mut len = 0;
            loop {
                let mut h = String::new();
                r.read_line(&mut h).unwrap();
                if h == "\r\n" {
                    break;
                }
                if let Some(v) = h.to_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap();
                }
            }
            let mut body = vec![0u8; len];
            r.read_exact(&mut body).unwrap();
            // 두 번째 요청부터는 실패를 돌려준다
            let status = if n == 0 { "200 OK" } else { "500 Internal Server Error" };
            write!(c, "HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
            tx.send((line.trim().to_string(), String::from_utf8(body).unwrap())).unwrap();
        }
    });
    let w = world_with(&format!(
        "[notify]\nwebhook = \"http://127.0.0.1:{port}/hook\"\ndebounce_secs = 2\nkinds = [\"report\", \"session\"]\n"
    ));
    let ids: Vec<String> = ["n1", "n2"]
        .iter()
        .map(|n| {
            w.start(&[
                "--title",
                n,
                "--profile",
                "fake",
                "--kind",
                "dir",
                "--task-file",
                &w.task(&format!("{n}.txt"), "x"),
            ])["id"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    // 두 보고가 debounce 안에 오면 한 통이다
    let (line, body) = rx.recv_timeout(T).expect("알림");
    assert_eq!(line, "POST /hook HTTP/1.1");
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["project"], "demo");
    let got: Vec<_> = v["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| (i["subject"].as_str().unwrap(), i["event"].as_str().unwrap()))
        .collect();
    assert_eq!(got, [(ids[0].as_str(), "report ready"), (ids[1].as_str(), "report ready")], "{body}");
    assert!(v["text"].as_str().unwrap().starts_with("[aaa] demo\n- t-0001 report ready"), "{body}");
    wait_for("notify_sent", T, || w.kinds(0).contains(&"notify_sent".to_string()).then_some(()));
    // 세션이 끝나면(session) 알린다. 받는 쪽이 실패하면 기록만 하고 다시 보내지 않는다.
    w.h.ok(&["pane", "kill", w.show(&ids[0])["pane"].as_str().unwrap()]);
    let (_, body) = rx.recv_timeout(T).expect("세션 알림");
    assert!(body.contains(&format!("- {} session ended: 세션이 끝났다", ids[0])), "{body}");
    wait_for("notify_failed", T, || w.kinds(0).contains(&"notify_failed".to_string()).then_some(()));
    assert!(rx.recv_timeout(Duration::from_secs(5)).is_err(), "실패한 알림을 다시 보냈다");
    let failed = w.h.ok(&["events", "demo"])["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["kind"] == "notify_failed")
        .unwrap()
        .clone();
    assert_eq!(failed["detail"], "HTTP 500");
    // 주소(비밀값이 들어갈 수 있다)는 기록에 남지 않는다
    assert!(!w.h.ok(&["events", "demo"]).to_string().contains("/hook"));
}

/// 머신 건강(§8.10): 디스크 부족·toolchain 실패면 새 세션을 거부하고 inbox에 남긴다. 고치고 다시 띄우면 풀린다.
#[test]
fn machine_health() {
    let mut w = world();
    let t = w.start(&["--title", "h", "--profile", "fake", "--kind", "dir", "--task-file", &w.task("h.txt", "x")]);
    let id = t["id"].as_str().unwrap().to_string();
    w.wait_show("보고", &id, |v| v["report_hash"].is_string());
    let base = agent_toml(&w.lab.root.join("workA"), &w.scripts);
    let restart = |w: &mut World, health: &str| {
        w.lab.kill("agentA");
        std::fs::write(w.h.cfg.join("agent.toml"), format!("{base}{health}")).unwrap();
        w.lab.spawn("agentA", &w.h.clone(), &["agent", "--host", "local", "--name", "labA"]);
    };
    // 1EB가 필요하다고 하고, 실패하는 toolchain 명령을 둔다
    restart(
        &mut w,
        "[health]\nevery_s = 1\nmin_free_gb = 1073741824\n[health.toolchain]\n\
         ok = [\"/bin/sh\", \"-c\", \"echo tool 9.9\"]\nbad = [\"/bin/sh\", \"-c\", \"echo broken >&2; exit 2\"]\n",
    );
    let m =
        wait_for("건강 이상", T, || w.h.machine("labA").filter(|m| !m["problems"].as_array().unwrap().is_empty()));
    let problems = m["problems"].to_string();
    assert!(
        problems.contains("디스크 여유") && problems.contains("toolchain bad 실패") && problems.contains("broken"),
        "{m}"
    );
    let tools = &m["health"]["toolchain"];
    assert_eq!((tools[1]["name"].as_str(), tools[1]["version"].as_str()), (Some("ok"), Some("tool 9.9")), "{m}");
    assert!(m["clock_skew_ms"].as_i64().unwrap().abs() < 5_000, "{m}");
    // 새 세션·thread는 거부한다
    let e = w.h.json(&["tab", "create", "--machine", "labA", "--", "/bin/sh"]).expect_err("점검 이상");
    assert!(e.contains("점검 이상") && e.contains("toolchain bad"), "{e}");
    let e =
        w.h.json(&[
            "thread",
            "start",
            "demo",
            "--title",
            "x",
            "--profile",
            "fake",
            "--kind",
            "dir",
            "--task-file",
            &w.task("x.txt", "x"),
        ])
        .expect_err("점검 이상");
    assert!(e.contains("점검 이상"), "{e}");
    let st = String::from_utf8_lossy(&w.h.run(&["status"]).stdout).to_string();
    assert!(st.contains("이상: 디스크 여유"), "{st}");
    // 열린 thread가 있는 프로젝트 inbox에 한 번
    let health = |w: &World| -> Vec<(String, String)> {
        w.h.ok(&["inbox", "list", "demo", "--all", "--peek"])["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|i| i["item_kind"] == "health")
            .map(|i| (i["event"].as_str().unwrap().to_string(), i["summary"].as_str().unwrap().to_string()))
            .collect()
    };
    let items = wait_for("inbox 이상 항목", T, || Some(health(&w)).filter(|v| !v.is_empty()));
    assert!(items.len() == 1 && items[0].0 == "problem" && items[0].1.contains("labA"), "{items:?}");
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(health(&w).len(), 1, "같은 이상은 다시 남기지 않는다");

    // 고치고 다시 띄우면 풀린다
    restart(&mut w, "[health]\nevery_s = 1\nmin_free_gb = 0\n");
    wait_for("건강 회복", T, || {
        w.h.machine("labA").filter(|m| m["health"].is_object() && m["problems"].as_array().unwrap().is_empty())
    });
    let items = wait_for("inbox 회복 항목", T, || Some(health(&w)).filter(|v| v.len() == 2));
    assert_eq!(items[1].0, "ok", "{items:?}");
    w.h.ok(&["tab", "create", "--machine", "labA", "--", "/bin/sh"]);
}

/// coordinator 일 흐름(부록 A): TASKS.md owner로 thread 시작, assignable 확인, Next 목록·보내기, context 요약.
#[test]
fn coordinator_flow() {
    let w = world();
    let pdir = w.projects.join("demo");
    std::fs::write(
        pdir.join("TASKS.md"),
        "# Tasks\n\n## Backlog\n- [ ] 로그인 고치기 (fake@labA)\n  - 메모: 테스트 먼저\n- [ ] 배포 (me)\n\
         - [ ] 사람 일 (Priya Rao)\n- [ ] 미할당 일\n",
    )
    .unwrap();
    // 맡길 수 있는 owner: host 허용 목록 ∩ 그 머신 profile
    let a = w.h.ok(&["assignable", "demo"]);
    let la = a["owners"].as_array().unwrap().iter().find(|o| o["machine"] == "labA").unwrap().clone();
    assert_eq!(la["profiles"], serde_json::json!(["fake", "hooked", "mute", "sticky"]), "{a}");
    w.h.ok(&["assignable", "demo", "--check", "fake@labA"]);
    for (bad, why) in
        [("other@labA", "쓸 수 있는 profile이 아니다"), ("Priya Rao", "사람"), ("@nowhere", "모르는 머신")]
    {
        let e = w.h.json(&["assignable", "demo", "--check", bad]).expect_err(bad);
        assert!(e.contains(why), "{bad}: {e}");
    }
    // TASKS.md task로 시작: owner가 profile·machine, 메모가 할 일에 붙는다
    for (title, why) in
        [("배포", "me"), ("사람 일", "사람"), ("미할당 일", "--profile"), ("없는 일", "TASKS.md에 없는")]
    {
        let e = w.h.json(&["thread", "start", "demo", "--from-task", title, "--kind", "dir"]).expect_err(title);
        assert!(e.contains(why), "{title}: {e}");
    }
    let t = w.h.ok(&["thread", "start", "demo", "--from-task", "로그인 고치기", "--kind", "dir"]);
    let id = t["id"].as_str().unwrap().to_string();
    assert_eq!(
        (t["title"].as_str(), t["profile"].as_str(), t["machine"].as_str()),
        (Some("로그인 고치기"), Some("fake"), Some("labA"))
    );
    let task = std::fs::read_to_string(pdir.join("threads").join(format!("{id}.task.md"))).unwrap();
    assert!(task.contains("## Notes from the task list") && task.contains("메모: 테스트 먼저"), "{task}");
    // coordinator가 ` · t-…`를 붙이면 같은 task를 다시 맡기지 않는다
    let md =
        std::fs::read_to_string(pdir.join("TASKS.md")).unwrap().replace("(fake@labA)", &format!("(fake@labA) · {id}"));
    std::fs::write(pdir.join("TASKS.md"), md).unwrap();
    let e = w.h.json(&["thread", "start", "demo", "--from-task", "로그인 고치기", "--kind", "dir"]).expect_err("중복");
    assert!(e.contains(&format!("이미 {id}")), "{e}");

    // Next: 보고서 `## Next` + 더한 줄, N번째 줄을 thread에 보낸다
    let report_next = w.wait_show("보고", &id, |v| v["report_hash"].is_string())["next"].clone();
    let mut want: Vec<Value> = report_next.as_array().unwrap().clone();
    assert!(!want.is_empty(), "가짜 agent 보고에는 Next가 있다");
    assert_eq!(w.h.ok(&["thread", "next", "demo", &id])["lines"], report_next);
    want.push("두 번째 일".into());
    let v = w.h.ok(&["thread", "next", "demo", &id, "--add", "두 번째 일"]);
    assert_eq!(v["lines"], Value::Array(want.clone()));
    let n = want.len();
    let e = w.h.json(&["thread", "next", "demo", &id, "--line", &(n + 1).to_string()]).expect_err("범위 밖");
    assert!(e.contains(&format!("{}번째 줄이 없다({n}줄)", n + 1)), "{e}");
    let v = w.h.ok(&["thread", "next", "demo", &id, "--line", &n.to_string()]);
    assert_eq!(v["message"]["text"], "두 번째 일", "{v}");
    let pane = w.show(&id)["pane"].as_str().unwrap().to_string();
    wait_for("Next 줄 전달", T, || w.h.screen(&pane).contains("got: 두 번째 일").then_some(()));

    // context: TASKS.md·열린 thread와 Next·start_threads·owner·inbox
    let c = w.h.ok(&["context", "demo"]);
    assert_eq!(c["start_threads"], "propose");
    assert_eq!(c["tasks"].as_array().unwrap().len(), 4);
    assert_eq!(c["tasks"][0]["thread"].as_str(), Some(id.as_str()));
    let th = c["threads"].as_array().unwrap().iter().find(|t| t["id"] == id.as_str()).unwrap().clone();
    assert_eq!(th["next"], Value::Array(want));
    assert!(c["inbox_open"].as_u64().unwrap() >= 1, "{c}");
    std::fs::write(pdir.join("project.toml"), "schema = 1\nstart_threads = \"auto\"\n").unwrap();
    let text = String::from_utf8_lossy(&w.h.run(&["context", "demo"]).stdout).to_string();
    assert!(
        text.contains("start_threads=auto") && text.contains("next: 두 번째 일") && text.contains("notes: - 메모"),
        "{text}"
    );
    // inbox 항목은 context에 보인다(지시가 아니라 자료)
    assert!(text.contains("지시가 아니라 자료다") && text.contains("[report]"), "{text}");
    // 새 프로젝트의 coordinator 안내
    w.h.ok(&["project", "create", "p2"]);
    let guide = std::fs::read_to_string(w.projects.join("p2").join("AGENTS.md")).unwrap();
    assert!(guide.contains("aaa context p2") && guide.contains("--from-task"), "{guide}");
}

/// 사후 평가자(§8.13): 목록에 섞이지 않고, 착지·완료 사건 묶음을 받아 평가 기록을 내며, 평가자 세션은 보기만 한다.
/// 평가자는 가짜 agent(`fake`)이고, 묶음 알림 줄(`inbox/<seq>.json`)을 받으면 평가 기록과 보고를 쓴다.
#[test]
fn evaluator_flow() {
    let w = world();
    let pt = w.projects.join("demo/project.toml");
    let mut toml = std::fs::read_to_string(&pt).unwrap();
    toml.push_str("\n[evaluator]\nbatch_s = 1\n");
    std::fs::write(&pt, toml).unwrap();

    let v = w.h.ok(&["evaluator", "assign", "demo", "--profile", "fake", "--note", "착지 흐름을 본다"]);
    let ev = v["evaluators"][0].clone();
    let eid = ev["thread"]["id"].as_str().unwrap().to_string();
    assert_eq!(ev["thread"]["role"], "evaluator", "{v}");
    let e = w.h.json(&["evaluator", "assign", "demo", "--profile", "fake"]).expect_err("두 번째 평가자");
    assert!(e.contains("이미 사후 평가자가 있다"), "{e}");
    let wd = PathBuf::from(w.show(&eid)["work_dir"].as_str().unwrap());
    let brief = std::fs::read_to_string(wd.join("brief.md")).unwrap();
    assert!(brief.contains("# 사후 평가자 규칙") && !brief.contains("# thread brief"), "{brief}");
    assert!(brief.contains("착지 흐름을 본다"), "{brief}");
    w.wait_show("평가자 brief", &eid, |v| v["brief"] == "delivered");

    // 목록에 섞이지 않는다: thread·pane 목록과 context에는 없고 role로만 보인다
    let ids = |v: &Value, key: &str| -> Vec<String> {
        v[key].as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap().to_string()).collect()
    };
    assert!(!ids(&w.h.ok(&["thread", "list", "demo"]), "threads").contains(&eid));
    assert_eq!(ids(&w.h.ok(&["thread", "list", "--role", "evaluator"]), "threads"), std::slice::from_ref(&eid));
    let pane = w.show(&eid)["pane"].as_str().unwrap().to_string();
    assert!(!ids(&w.h.ok(&["pane", "list"]), "panes").contains(&pane));
    assert_eq!(ids(&w.h.ok(&["pane", "list", "--role", "evaluator"]), "panes"), std::slice::from_ref(&pane));
    let c = w.h.ok(&["context", "demo"]);
    assert!(c["threads"].as_array().unwrap().is_empty(), "{c}");
    assert_eq!(c["evaluator"]["thread"]["id"], eid.as_str());

    // 평가자 세션 안의 CLI는 보기만 한다: 다른 thread에 지시·채택이 거부되고 보기는 된다
    let as_eval = |args: &[&str]| w.h.cmd(args).env("AAA_SESSION_ID", &pane).output().unwrap();
    for args in [
        &["thread", "message", "demo", "t-0001", "--text", "x"][..],
        &["evaluator", "unassign", "demo"],
        &["thread", "start", "demo", "--title", "x", "--profile", "fake", "--kind", "dir", "--task-file", "/dev/null"],
    ] {
        let o = as_eval(args);
        assert_eq!(o.status.code(), Some(1), "{args:?}: {}", String::from_utf8_lossy(&o.stdout));
        assert!(String::from_utf8_lossy(&o.stderr).contains("보기 권한"), "{}", String::from_utf8_lossy(&o.stderr));
    }
    let o = as_eval(&["--json", "evaluator", "status", "demo"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));

    // 일반 thread가 착지·완료되면 묶음이 평가자 작업 폴더 inbox/에 생긴다
    let t =
        w.start(&["--title", "work", "--profile", "fake", "--kind", "dir", "--task-file", &w.task("e1.txt", "X-77")]);
    let tid = t["id"].as_str().unwrap().to_string();
    w.wait_show("보고", &tid, |v| v["report_hash"].is_string());
    w.h.ok(&["merge", "enqueue", "demo", &tid]);
    w.h.ok(&["merge", "done", "demo", &tid]);
    w.h.ok(&["thread", "resolve", "demo", &tid]);
    let inbox = wd.join("inbox");
    let batches = |n: usize| {
        wait_for_diag(
            &format!("사건 묶음 {n}개"),
            T,
            || {
                let mut v: Vec<PathBuf> = std::fs::read_dir(&inbox)
                    .ok()?
                    .filter_map(|e| e.ok().map(|e| e.path()))
                    .filter(|p| p.extension().is_some_and(|x| x == "json"))
                    .collect();
                v.sort();
                (v.len() >= n).then_some(v)
            },
            || format!("evaluator: {}\nevents: {}", w.h.ok(&["evaluator", "status", "demo"]), w.kinds(0).join(",")),
        )
    };
    // 착지는 바로, 완료는 1초 뒤 묶음: 둘이 한 묶음이거나 두 묶음이다. 완료까지 넘어갈 때까지 기다린다.
    let all_kinds = |files: &[PathBuf]| -> (Vec<String>, Vec<Value>) {
        let mut kinds = Vec::new();
        let mut docs = Vec::new();
        for f in files {
            let b: Value = serde_json::from_slice(&std::fs::read(f).unwrap()).unwrap();
            for e in b["events"].as_array().unwrap() {
                assert_ne!(e["role"], "evaluator", "평가자 자신의 사건이 들어갔다: {e}");
                kinds.push(e["kind"].as_str().unwrap().to_string());
            }
            docs.push(b);
        }
        (kinds, docs)
    };
    let files = wait_for_diag(
        "완료 사건까지 넘김",
        T,
        || {
            let f = batches(1);
            all_kinds(&f).0.contains(&"thread_resolved".to_string()).then_some(f)
        },
        || w.h.ok(&["evaluator", "status", "demo"]).to_string(),
    );
    let (kinds, docs) = all_kinds(&files);
    for k in ["thread_started", "report_ready", "merge_landed", "thread_resolved"] {
        assert!(kinds.contains(&k.to_string()), "{k} 없음: {kinds:?}");
    }
    // 묶음은 seq가 겹치지 않고, thread 이력·시간 요약·보고 본문을 담는다
    for pair in docs.windows(2) {
        assert!(pair[0]["to_seq"].as_u64() < pair[1]["from_seq"].as_u64(), "묶음이 겹친다");
    }
    let last = docs.last().unwrap();
    let th = last["threads"].as_array().unwrap().iter().find(|t| t["id"] == tid.as_str()).unwrap();
    assert!(th["summary"]["started_at_ms"].is_u64() && th["summary"]["resolved_ms"].is_u64(), "{th}");
    assert!(th["report"].as_str().unwrap().contains("X-77"), "{th}");

    // 평가 기록을 받는다: 목록·inbox 항목·잘못된 기록은 한 번만 알림
    let first = files[0].file_stem().unwrap().to_str().unwrap().to_string();
    let eval_id = format!("e-{first}");
    let got = wait_for_diag(
        "평가 기록",
        T,
        || {
            let v = w.h.ok(&["evaluation", "list", "demo"]);
            v["evaluations"].as_array().unwrap().iter().find(|e| e["id"] == eval_id.as_str()).cloned()
        },
        || format!("inbox: {}\nevents: {}", w.h.ok(&["inbox", "list", "demo", "--peek"]), w.kinds(0).join(",")),
    );
    assert_eq!(got["evaluator"], eid.as_str());
    assert_eq!(got["doc"]["stages"][0]["minutes"], 1, "소수 분은 반올림");
    assert!(w.projects.join("demo").join(got["file"].as_str().unwrap()).is_file());
    let inbox_items = w.h.ok(&["inbox", "list", "demo", "--peek"])["items"].as_array().unwrap().clone();
    let evals: Vec<&Value> = inbox_items.iter().filter(|i| i["item_kind"] == "evaluation").collect();
    assert!(evals.iter().any(|i| i["subject"] == eval_id.as_str() && i["event"] == "evaluation"), "{inbox_items:?}");
    assert_eq!(evals.iter().filter(|i| i["event"] == "invalid").count(), 1, "{inbox_items:?}");
    assert!(
        !inbox_items.iter().any(|i| i["item_kind"] == "report" && i["subject"] == eid.as_str()),
        "평가자 보고가 report 항목이 됐다"
    );

    // 채택은 결정 기록에 남고, 보류까지 정하면 inbox 항목이 끝난다
    let a = w.h.ok(&["evaluation", "adopt", "demo", &eval_id, "p1", "--note", "다음 주"]);
    let num = a["doc"]["proposals"][0]["decision"]["decision_num"].as_u64().unwrap();
    let d = w.h.ok(&["decision", "list", "demo"]);
    assert!(
        d["decisions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["num"] == num && x["text"].as_str().unwrap().contains("p1")),
        "{d}"
    );
    let e = w.h.json(&["evaluation", "adopt", "demo", &eval_id, "p1"]).expect_err("두 번 채택");
    assert!(e.contains("이미 채택"), "{e}");
    let e = w.h.json(&["evaluation", "hold", "demo", &eval_id, "p9"]).expect_err("없는 개선안");
    assert!(e.contains("없는 개선안"), "{e}");
    w.h.ok(&["evaluation", "hold", "demo", &eval_id, "p2"]);
    let left = w.h.ok(&["evaluation", "list", "demo"]);
    assert!(!left["evaluations"].as_array().unwrap().iter().any(|e| e["id"] == eval_id.as_str()), "{left}");
    let open = w.h.ok(&["inbox", "list", "demo", "--peek"])["items"].as_array().unwrap().clone();
    assert!(!open.iter().any(|i| i["subject"] == eval_id.as_str() && i["event"] == "evaluation"), "{open:?}");

    // 평가자를 다시 띄워도 넘긴 사건은 다시 넘기지 않는다: 다음 묶음은 그 뒤 seq부터
    let before = w.h.ok(&["evaluator", "status", "demo"])["evaluators"][0].clone();
    w.h.ok(&["pane", "kill", &pane]);
    let eval_pane = || -> Value {
        w.h.ok(&["pane", "list", "--role", "evaluator"])["panes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == pane.as_str())
            .cloned()
            .unwrap()
    };
    wait_for("평가자 종료", T, || (eval_pane()["state"] != "running").then_some(()));
    let st = w.h.ok(&["evaluator", "status", "demo"])["evaluators"][0].clone();
    assert_eq!(st["running"], false, "{st}");
    w.h.ok(&["thread", "restart", "demo", &eid]);
    w.wait_show("평가자 다시 brief", &eid, |v| v["brief"] == "delivered");
    let t2 =
        w.start(&["--title", "work2", "--profile", "fake", "--kind", "dir", "--task-file", &w.task("e2.txt", "y")]);
    let t2id = t2["id"].as_str().unwrap().to_string();
    w.wait_show("보고", &t2id, |v| v["report_hash"].is_string());
    w.h.ok(&["merge", "enqueue", "demo", &t2id]);
    w.h.ok(&["merge", "done", "demo", &t2id]);
    let files2 = batches(files.len() + 1);
    let (_, docs2) = all_kinds(&files2);
    let newest = docs2.last().unwrap();
    assert!(newest["from_seq"].as_u64().unwrap() > before["cursor"].as_u64().unwrap(), "{newest}");
    assert!(
        newest["events"].as_array().unwrap().iter().all(|e| e["thread"] != tid.as_str()),
        "앞 thread 사건이 다시 왔다"
    );

    // 끝내기: 상태에서 사라진다
    w.h.ok(&["evaluator", "unassign", "demo"]);
    assert!(w.h.ok(&["evaluator", "status", "demo"])["evaluators"].as_array().unwrap().is_empty());
}

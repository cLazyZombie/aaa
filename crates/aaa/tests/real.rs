//! 실제 harness E2E(plan §8.70): 시험 host에서 실제 Claude Code·Codex를 thread로 띄워 화면 감지 상태가
//! 작업 중 → 질문(명령 허락) → idle로 바뀌고, 처음 실행 확인창은 질문으로 올라오며, brief가 받아졌는지(작업 중·질문이
//! 보이면 받았다)를 본다. 구독 사용량을 쓰므로 평소에는 돌지 않는다(CI 제외).
//!
//! 실행: `cargo test -p aaa --test real -- --ignored --test-threads 1`
//! `AAA_FIXTURE_DIR=<폴더>`를 주면 단계마다 pane 화면을 그 폴더에 남긴다(감지 fixture 후보. 개인 경로를 지운 뒤 쓴다).

#![cfg(unix)]

mod lab;

use std::path::Path;
use std::time::Duration;

use aaa_proto::{AgentActivity, FrontEvent, QuestionState, QuestionView};
use lab::{Events, Lab, Node, now_ms, path_str, wait_for};
use serde_json::Value;

/// 실제 모델의 한 단계(시작·응답)를 기다리는 한도
const STEP: Duration = Duration::from_secs(180);

/// 짧은 일: 허락이 필요한 명령 하나를 돌리고 한 낱말로 답한다
const TASK: &str = "Run exactly this shell command once: touch probe.txt\nThen reply with the single word: done\n";

fn host_toml(projects: &Path) -> String {
    format!(
        "schema = 1\n[host]\nagent_listen = \"127.0.0.1:0\"\n[web]\nlisten = \"\"\n[local_agent]\nmode = \"none\"\n\
         [projects]\nroot = \"{}\"\nallow_profiles = [\"rclaude\", \"rcodex\"]\n",
        path_str(projects)
    )
}

/// Claude는 작은 모델·기본 권한 방식(사용자 설정의 bypass를 덮는다), Codex는 낮은 effort·읽기 전용 sandbox·
/// on-request 승인(쓰기 명령은 sandbox 밖 실행 허락을 묻는다. Codex 0.163은 untrusted를 받지 않는다)
fn agent_toml(work_root: &Path) -> String {
    format!(
        "schema = 1\n[agent]\nwork_root = \"{}\"\n\
         [profiles.rclaude]\nharness = \"claude\"\nmodel = \"haiku\"\nargs = [\"--permission-mode\", \"default\"]\n\
         [profiles.rcodex]\nharness = \"codex\"\neffort = \"low\"\n\
         args = [\"-c\", \"approval_policy=\\\"on-request\\\"\", \"-c\", \"sandbox_mode=\\\"read-only\\\"\"]\n",
        path_str(work_root)
    )
}

struct World {
    _lab: Lab,
    h: Node,
    work: std::path::PathBuf,
}

fn world() -> World {
    let mut lab = Lab::new();
    let projects = lab.root.join("projects");
    let work = lab.root.join("work");
    let h = lab.node("h", Some(&host_toml(&projects)));
    std::fs::write(h.cfg.join("agent.toml"), agent_toml(&work)).unwrap();
    lab.spawn("host", &h, &["host"]);
    wait_for("host 준비", Duration::from_secs(30), || h.json(&["status"]).ok());
    // 이 시험을 Claude Code 세션 안에서 돌려도 그 세션 표지는 aaa 세션에 넘기지 않는다(agent가 지운다)
    lab.spawn("agent", &h, &["agent", "--host", "local", "--name", "labA"]);
    wait_for("labA 연결", Duration::from_secs(30), || {
        h.machine("labA").is_some_and(|m| m["connection"] == "connected").then_some(())
    });
    h.ok(&["project", "create", "demo"]);
    World { _lab: lab, h, work }
}

/// 단계 화면을 fixture 후보로 남긴다
fn keep_screen(w: &World, pane: &str, name: &str) {
    if let Ok(dir) = std::env::var("AAA_FIXTURE_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(Path::new(&dir).join(format!("{name}.txt")), w.h.screen(pane)).unwrap();
    }
}

enum Seen {
    Question(Box<QuestionView>),
    Agent(AgentActivity),
    Flow(String),
}

/// 이 thread의 다음 사건 하나(`cursor`번째 받은 사건부터): 새 질문, `at` 뒤의 상태 변화, 흐름 사건
fn next(ev: &mut Events, cursor: &mut usize, id: &str, at: u64, answered: &[String], what: &str) -> Seen {
    let (i, seen) = ev.wait_from(*cursor, what, STEP, |e| match e {
        FrontEvent::Question(q)
            if q.thread.as_deref() == Some(id) && q.state == QuestionState::Open && !answered.contains(&q.id) =>
        {
            Some(Seen::Question(Box::new(q.clone())))
        }
        FrontEvent::Thread(t) if t.id == id => {
            t.agent.as_ref().filter(|a| a.since_ms >= at).map(|a| Seen::Agent(a.activity))
        }
        FrontEvent::Flow(f) if f.thread.as_deref() == Some(id) && f.at_ms >= at => Some(Seen::Flow(f.kind.clone())),
        _ => None,
    });
    *cursor = i + 1;
    seen
}

/// thread를 띄워 처음 실행 확인창(신뢰)에 답하고 brief가 받아질 때까지, 이어 작업 중 → 허락 질문 → idle을 본다
fn run(profile: &str, harness: &str) {
    let w = world();
    let mut ev = Events::connect(&w.h);
    let task = w.work.with_file_name("task.txt");
    std::fs::write(&task, TASK).unwrap();
    let t0 = now_ms();
    let v = w.h.ok(&[
        "thread",
        "start",
        "demo",
        "--title",
        "real",
        "--profile",
        profile,
        "--kind",
        "dir",
        "--task-file",
        path_str(&task),
    ]);
    let id = v["id"].as_str().unwrap().to_string();
    let pane = v["pane"].as_str().unwrap().to_string();
    let mut answered: Vec<String> = Vec::new();
    let mut kinds: Vec<String> = Vec::new();
    let mut cursor = 0;

    // 처음 실행 확인창은 입력하지 않고 질문으로 올린다. 답하면 brief가 간다.
    loop {
        match next(&mut ev, &mut cursor, &id, t0, &answered, "확인창 질문이나 brief 전달") {
            Seen::Question(q) => {
                keep_screen(&w, &pane, &format!("{harness}-start-{}", q.question_kind));
                kinds.push(q.question_kind.clone());
                w.h.ok(&["question", "answer", &q.id, "yes"]);
                answered.push(q.id);
            }
            Seen::Flow(k) if k == "brief_delivered" => break,
            Seen::Flow(k) if k == "brief_unknown" => panic!("brief가 받아지지 않았다: {}", w.h.screen(&pane)),
            _ => {}
        }
    }
    let v = w.h.ok(&["thread", "show", "demo", &id]);
    let a = &v["agent"];
    assert_eq!(a["harness"], harness, "{v}");
    // brief를 받으면 harness가 일하거나(spinner) 질문한다: 그것으로 받았다고 본다
    assert!(matches!(a["activity"].as_str(), Some("working" | "question")), "{v}");
    keep_screen(&w, &pane, &format!("{harness}-working"));

    // 명령 허락 질문에 답하고, 턴이 끝나 입력 상자가 보이면 idle이다
    let mut at = now_ms();
    let mut seen_working = a["activity"] == "working";
    // brief 뒤 질문(명령·파일 허락). 종류는 맞은 감지 규칙으로 정한다(Codex 승인 창은 OSC 제목 「Action Required」로
    // 먼저 잡혀 confirm이다)
    let mut asked = false;
    loop {
        match next(&mut ev, &mut cursor, &id, at, &answered, "작업 중·질문·idle") {
            Seen::Question(q) => {
                keep_screen(&w, &pane, &format!("{harness}-question-{}", answered.len()));
                assert_ne!(q.question_kind, "trust", "brief 뒤에 신뢰 창: {q:?}");
                asked = true;
                kinds.push(q.question_kind.clone());
                at = now_ms();
                w.h.ok(&["question", "answer", &q.id, "yes"]);
                answered.push(q.id);
            }
            Seen::Agent(AgentActivity::Working) => {
                if !seen_working {
                    keep_screen(&w, &pane, &format!("{harness}-working-after"));
                }
                seen_working = true;
            }
            Seen::Agent(AgentActivity::Idle) if asked => break,
            Seen::Agent(AgentActivity::Exited | AgentActivity::Error) => {
                panic!("harness가 끝났다: {}", w.h.screen(&pane))
            }
            _ => {}
        }
    }
    keep_screen(&w, &pane, &format!("{harness}-idle"));
    assert!(seen_working, "작업 중을 한 번도 보지 못했다");
    let cwd = v["cwd"].as_str().unwrap();
    assert!(Path::new(cwd).join("probe.txt").exists(), "허락한 명령이 돌지 않았다: {}", w.h.screen(&pane));
    eprintln!("{harness}: 질문 {kinds:?}");
    let msg: Vec<Value> = w.h.ok(&["events", "--project", "demo"])["events"].as_array().unwrap().clone();
    let shown = msg.iter().filter(|e| e["kind"] == "question_shown").count();
    assert_eq!(shown, answered.len(), "질문이 하나씩만 열려야 한다");
}

#[test]
#[ignore = "실제 Claude Code를 띄운다(구독 사용). --ignored로 돈다"]
fn real_claude_working_question_idle() {
    run("rclaude", "claude");
}

#[test]
#[ignore = "실제 Codex를 띄운다(구독 사용). --ignored로 돈다"]
fn real_codex_working_question_idle() {
    run("rcodex", "codex");
}

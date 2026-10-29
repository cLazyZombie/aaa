//! CLI holds → 실제 두 agent → 승인/대기/시작 → host 재시작의 L5 전송 경계.
use super::*;
pub(super) fn automatic(w: &World) -> Vec<Value> {
    w.h.ok(&["events", "demo"])["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["kind"] == "thread_started" && e["actor"] == "routine:check")
        .cloned()
        .collect()
}
#[test]
fn automation_start_holds_approval_two_agents_and_restart_receipt() {
    let mut w = world_with("tick_secs=1\n");
    w.add_remote();
    let config = std::fs::read_to_string(w.h.cfg.join("host.toml")).unwrap().replace("127.0.0.1:0", &w.addr);
    std::fs::write(w.h.cfg.join("host.toml"), config).unwrap();
    w.h.in_terminal(&["safety", "set", "start_threads", "auto"], &[("[y/N]", "y")], T);
    w.h.ok(&["project", "set", "demo", "repos.add", path_str(&w.repo)]);
    w.h.ok(&["project", "set", "demo", "repos.add", &format!("{}@labB", path_str(&w.repo))]);
    let task = w.task("hold.md", "noreport");
    let mut holders = vec![];
    for machine in ["labA", "labB"] {
        let row = w.start(&[
            "--title",
            "측정 중",
            "--profile",
            "fake",
            "--machine",
            machine,
            "--kind",
            "dir",
            "--holds",
            "timing",
            "--task-file",
            &task,
        ]);
        let id = row["id"].as_str().unwrap().to_string();
        let shown = w.wait_show("측정 thread brief", &id, |v| v["brief"] == "delivered");
        assert_eq!(shown["holds"], serde_json::json!(["timing"]));
        holders.push(id);
    }
    let dir = w.projects.join("demo/automations");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("check.toml"),"[on.thread]\nevents=['report_ready']\nrepos=['repo']\n[do.start]\ntitle='자동 점검'\nrepo='repo'\ntext='noreport'\nprofiles=['fake','mute']\nprofile_pick='week-left'\nmachines=['labA','labB']\nwait=['timing']\n[limits]\nmax_running=2\n").unwrap();
    // 기존 thread 사건은 등록 시 cursor의 기준일 뿐이다.
    w.h.ticks(2);
    let listed = w.h.ok(&["routine", "list", "demo"]);
    let r = listed["routines"].as_array().unwrap().iter().find(|r| r["name"] == "check").unwrap();
    assert_eq!(r["start_approved"], false);
    let source_task = w.task("source.md", "원인 사건의 실제 보고");
    let source = w.start(&[
        "--title",
        "원인 사건",
        "--profile",
        "fake",
        "--repo",
        path_str(&w.repo),
        "--task-file",
        &source_task,
    ]);
    w.wait_show("원인 thread 준비", source["id"].as_str().unwrap(), |v| v["brief"] == "delivered");
    wait_for("승인 대기 기록", T, || {
        w.h.ticks(1);
        w.h.ok(&["routine", "runs", "demo", "check"])["runs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["detail"].as_str().is_some_and(|s| s.contains("범위 승인 필요")))
            .then_some(())
    });
    assert!(automatic(&w).is_empty());
    // 사람 CLI가 앞서 검토한 hash를 승인한다. 세션 안 agent의 대리 승인은 쓰지 않는다.
    w.h.in_terminal(&["automation", "approve", "check", r["start_approval_sha256"].as_str().unwrap()], &[], T);
    wait_for("holds 대기 기록", T, || {
        w.h.ticks(1);
        w.h.ok(&["routine", "runs", "demo", "check"])["runs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["detail"].as_str().is_some_and(|s| s.contains("timing")))
            .then_some(())
    });
    assert!(automatic(&w).is_empty());
    w.h.ok(&["thread", "resolve", "demo", &holders[1], "--skip-copy"]);
    let event = wait_for("표지 해제 뒤 자동 시작", T, || automatic(&w).first().cloned());
    let id = event["thread"].as_str().unwrap();
    let shown = w.wait_show("자동 worker brief", id, |v| v["brief"] == "delivered");
    assert_eq!(shown["machine"], "labB");
    assert_eq!(shown["role"], "worker");
    assert_eq!(shown["repo"], path_str(&w.repo));
    assert_eq!(shown["thread_kind"], "worktree");
    assert_eq!(shown["profile"], "fake");
    let board = w.h.ok(&["task", "list"]);
    assert!(
        board["tasks"].as_array().unwrap().iter().all(|t| t["thread"] != id && t["title"] != "자동 점검"),
        "자동화는 할 일을 만들지 않는다: {board}"
    );

    w.lab.kill("host");
    w.spawn_host("host2");
    wait_for("두 agent 재연결", T, || (w.h.connected("labA") && w.h.connected("labB")).then_some(()));
    w.h.ticks(2);
    assert_eq!(automatic(&w).len(), 1, "같은 사건은 재시작 뒤 다시 시작하지 않는다");
    // 다른 사건은 앞의 자동 thread가 열려 있어도 명시 동시 상한 2개까지 실행한다.
    let second_task = w.task("second.md", "다른 원인 사건의 실제 보고");
    let second = w.start(&[
        "--title",
        "다른 원인",
        "--profile",
        "fake",
        "--repo",
        path_str(&w.repo),
        "--task-file",
        &second_task,
    ]);
    w.wait_show("다른 원인 준비", second["id"].as_str().unwrap(), |v| v["brief"] == "delivered");
    wait_for("서로 다른 사건 두 번째 시작", T, || (automatic(&w).len() == 2).then_some(()));
}

/// 자기 사건 거르기(plan t-0475 A1 §3.5): `on.thread`+`do.start` 규칙이 띄운 thread가 보고(`report_ready`)해도 그 규칙은 다시
/// thread를 띄우지 않는다. 사람이 띄운 thread의 보고는 그대로 받는다. 끝은 시간이 아니라 사건이다: 자동 thread가 보고한 뒤 사람이
/// 띄운 둘째 원인의 보고로 둘째 시작이 오면, 규칙은 그 앞의 자동 thread 보고를 이미 지나갔다(흐름 사건은 순서대로 고른다).
/// 결함 되심기: runtime.rs `automation_flow_events`의 자동화 thread 거르기를 빼면 자동 thread 보고가 새 시작을 만들어 실패한다.
#[test]
fn automation_thread_reports_do_not_wake_their_own_rule() {
    let w = world();
    w.h.in_terminal(&["safety", "set", "start_threads", "auto"], &[("[y/N]", "y")], T);
    w.h.ok(&["project", "set", "demo", "repos.add", path_str(&w.repo)]);
    let dir = w.projects.join("demo/automations");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("check.toml"),
        "title='보고 점검'\n[on.thread]\nevents=['report_ready']\nrepos=['repo']\n[do.start]\ntitle='자동 점검'\nrepo='repo'\ntext='자동 점검 보고'\nprofiles=['fake']\nmachines=['labA']\n[limits]\nmax_running=2\n",
    )
    .unwrap();
    w.h.ticks(2);
    let listed = w.h.ok(&["routine", "list", "demo"]);
    let r = listed["routines"].as_array().unwrap().iter().find(|r| r["name"] == "check").unwrap().clone();
    w.h.in_terminal(&["automation", "approve", "check", r["start_approval_sha256"].as_str().unwrap()], &[], T);
    let human = |title: &str| {
        let task = w.task(&format!("{title}.md"), &format!("{title} 보고"));
        let v = w.start(&["--title", title, "--profile", "fake", "--repo", path_str(&w.repo), "--task-file", &task]);
        let id = v["id"].as_str().unwrap().to_string();
        w.wait_show(&format!("{title} 보고"), &id, |v| v["report_hash"].is_string());
        id
    };
    // 자동 thread의 할 일에는 원인 사건(조건 자료)이 들어 있다: 어느 thread의 보고로 시작했는지 그것으로 안다
    let cause = |id: &str| {
        // 막 시작한 thread는 아직 작업 폴더가 없다(다음 판정에서 다시 본다)
        let shown = w.show(id);
        let brief = shown["work_dir"]
            .as_str()
            .and_then(|d| std::fs::read_to_string(std::path::Path::new(d).join("brief.md")).ok())
            .unwrap_or_default();
        ["t-0001", "t-0002", "t-0003", "t-0004", "t-0005"]
            .into_iter()
            .filter(|t| brief.contains(&format!("\"thread\":\"{t}\"")))
            .map(str::to_string)
            .collect::<Vec<_>>()
    };
    let started = || automatic(&w).iter().map(|e| e["thread"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    let first = human("첫 원인");
    let auto = wait_for("첫 원인 보고로 자동 시작", T, || {
        w.h.ticks(1);
        started().first().cloned()
    });
    let shown = w.wait_show("자동 thread 보고", &auto, |v| v["report_hash"].is_string());
    assert_eq!((shown["automation"].as_str(), shown["automation_title"].as_str()), (Some("check"), Some("보고 점검")));
    assert!(shown["next"].as_array().is_some_and(|v| !v.is_empty()), "보고의 Next 표시는 유지한다");
    assert!(shown["followups"].as_array().is_none_or(|v| v.is_empty()), "do.start 출처는 후속 원장에서 제외한다");
    let human_shown = w.show(&first);
    assert!(
        human_shown["followups"].as_array().is_some_and(|v| !v.is_empty()),
        "일반 시작은 같은 fake 완료 Next도 보호한다"
    );
    assert_eq!(cause(&auto), std::slice::from_ref(&first));
    assert!(human_shown["automation"].is_null(), "사람이 띄운 thread에는 출처가 없다");
    let board = w.h.ok(&["task", "list"]);
    let unlinked: Vec<_> =
        board["unlinked_threads"].as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap()).collect();
    assert!(!unlinked.contains(&first.as_str()) && !unlinked.contains(&auto.as_str()), "{unlinked:?}");
    let tasks = board["tasks"].as_array().unwrap();
    assert!(tasks.iter().any(|t| t["thread"] == first), "일반 기동의 자동 연결: {board}");
    assert!(tasks.iter().all(|t| t["thread"] != auto && t["title"] != "자동 점검"), "자동화 예외: {board}");
    let second = human("둘째 원인");
    let next = wait_for("둘째 원인 보고로 자동 시작", T, || {
        w.h.ticks(1);
        started().into_iter().find(|id| *id != auto && cause(id).contains(&second))
    });
    let all = started();
    assert_eq!(all, [auto.clone(), next.clone()], "자동 thread 보고는 규칙을 다시 깨우지 않는다");
    for id in &all {
        assert!(!cause(id).contains(&auto), "{id}가 자동 thread {auto}의 보고로 시작했다");
    }
}

/// 실제 자동화가 일반 자리가 꽉 차도 시작하며, 사용자 예약·출력·재시작에서도 같은 분류를 쓴다.
#[test]
fn automation_threads_leave_user_capacity_and_queue_slots_available() {
    let mut w = world();
    w.h.in_terminal(&["safety", "set", "start_threads", "auto"], &[("[y/N]", "y")], T);
    w.h.ok(&["project", "set", "demo", "max_parallel_threads", "1"]);
    w.h.ok(&["project", "set", "demo", "thread_profile", "fake"]);
    let dir = w.projects.join("demo/automations");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("check.toml"), "title='완료 점검'\n[on.thread]\nevents=['report_ready']\n[do.start]\ntitle='자동 점검'\ntext='noreport'\nprofiles=['fake']\nmachines=['labA']\n").unwrap();
    w.h.ticks(2);
    let r = w.h.ok(&["routine", "list"])["routines"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == "check")
        .unwrap()
        .clone();
    w.h.in_terminal(&["automation", "approve", "check", r["start_approval_sha256"].as_str().unwrap()], &[], T);
    let task = w.task("cause.md", "원인 보고");
    let first = w.start(&["--title", "사용자 원인", "--profile", "fake", "--kind", "dir", "--task-file", &task]);
    let first_id = first["id"].as_str().unwrap();
    w.wait_show("원인 보고", first_id, |v| v["report_hash"].is_string());
    let auto = wait_for("사용자 한도가 찬 상태의 자동화 기동", T, || {
        w.h.ticks(1);
        automatic(&w).first().and_then(|e| e["thread"].as_str()).map(str::to_string)
    });
    let shown = w.wait_show("자동화 thread 준비", &auto, |v| v["brief"] == "delivered");
    assert_eq!(shown["automation"], "check");
    let trigger = shown["automation_trigger"].as_str().unwrap().to_string();
    assert!(trigger.contains("events") && trigger.contains("report_ready") && trigger.contains(first_id), "{trigger}");
    for args in [vec!["thread", "list"], vec!["thread", "show", &auto], vec!["context", "--peek"]] {
        let out = w.h.run(&args);
        assert!(out.status.success());
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(
            text.contains("자동화 thread") && text.contains("완료 점검 (check)") && text.contains(&trigger),
            "{text}"
        );
    }
    std::fs::write(
        w.projects.join("demo/TASKS.md"),
        "## Backlog\n- [ ] 사용자 예약 (fake@labA) [작업 폴더]\n  noreport\n",
    )
    .unwrap();
    w.h.ok(&["task", "reserve", "사용자 예약"]);
    w.h.ticks(1);
    assert!(w.h.ok(&["task", "queue"])["entries"][0]["thread"].is_null());
    let denied = w
        .h
        .json(&["thread", "start", "--title", "한도 검사", "--profile", "fake", "--kind", "dir", "--task-file", &task])
        .unwrap_err();
    assert!(denied.contains("프로젝트 실행 한도"), "{denied}");
    // 사용자 자리를 돌려주면 자동화가 열려 있어도 다음 예약이 기동한다.
    w.h.ok(&["thread", "resolve", first_id]);
    let next = wait_for("자동화와 독립된 사용자 예약 기동", T, || {
        w.h.json(&["task", "queue"]).ok()?["entries"][0]["thread"].as_str().map(str::to_string)
    });
    w.wait_show("예약 pane 준비", &next, |v| v["status"] == "open" && v["brief"] == "delivered");
    assert_eq!(w.show(&auto)["status"], "open");
    // 재시작 시에도 출처를 그대로 읽고 자동화 재기동은 사용자 자리와 별개다.
    w.lab.kill("host");
    w.spawn_host("host-restored");
    wait_for("agent 재연결", T, || w.h.connected("labA").then_some(()));
    assert_eq!(w.show(&auto)["automation_trigger"], trigger);
    let pane = w.show(&auto)["pane"].as_str().unwrap().to_string();
    w.h.ok(&["pane", "kill", &pane]);
    wait_for("자동화 pane 종료 사건", T, || (w.h.pane(&pane)["state"] == "exited").then_some(()));
    w.h.ok(&["thread", "restart", &auto]);
    let restarted = w.wait_show("자동화 재시작", &auto, |v| v["status"] == "open" && v["pane"] != pane);
    assert_eq!(restarted["automation_trigger"], trigger);
    assert_eq!(w.show(&next)["status"], "open");
}

/// fake harness가 압축 때 기억을 버린 뒤 실제 hook 응답의 경로를 열어 최신 파일을 읽는다.
/// 모델의 지시 이행 자체를 보장하지 않으며 host→세션 명세→CLI→agent 응답의 계약을 검사한다.
#[test]
fn automation_memory_reentry_claude_after_compact_and_resume() {
    memory_reentry("claude");
}

#[test]
fn automation_memory_reentry_codex_after_compact_and_resume() {
    memory_reentry("codex");
}

fn memory_reentry(harness: &str) {
    let mut w = world();
    w.h.in_terminal(&["profile", "edit", "claude", "--agent", harness], &[], T);
    w.h.in_terminal(&["safety", "set", "start_threads", "auto"], &[("[y/N]", "y")], T);
    let idle = if harness == "claude" {
        include_str!("../../../aaa-host/tests/fixtures/prompt_box/claude-empty.ansi")
    } else {
        include_str!("../../../aaa-host/tests/fixtures/prompt_box/codex-empty.ansi")
    };
    let script = r#"#!/usr/bin/env python3
import json, os, pathlib, subprocess, sys, tempfile
harness = HARNESS
idle = IDLE
work = pathlib.Path('.aaa') / (os.environ['AAA_PROJECT'] + '-' + os.environ['AAA_THREAD'])
sid = '019a4c5e-1f2b-7c3d-8e4f-a5b6c7d8e9f0'
def publish_probe(source, value):
    # 같은 폴더에서 원자 교체하여 읽는 쪽에는 완성된 JSON만 보인다.
    target = work / ('probe-' + source + '.json')
    with tempfile.NamedTemporaryFile(mode='w', encoding='utf-8', dir=work,
                                     prefix=target.name + '.', suffix='.tmp', delete=False) as pending:
        json.dump(value, pending)
        pending.flush()
    try:
        os.replace(pending.name, target)
    finally:
        pathlib.Path(pending.name).unlink(missing_ok=True)
def hook(source, **extra):
    event = dict(hook_event_name='SessionStart', session_id=sid, source=source, **extra)
    result = subprocess.run([os.environ['AA'], 'hook', '--agent', harness, json.dumps(event)],
                            check=True, capture_output=True, text=True)
    if extra:
        assert not result.stdout, result.stdout
        return
    context = json.loads(result.stdout)['hookSpecificOutput']['additionalContext']
    # 압축으로 사라진 문맥을 대신해 hook이 돌려준 절대 경로만 따른다.
    paths = [pathlib.Path(p) for p in context.split('`')[1::2] if p.endswith('/memory/memory.md')]
    if source == 'startup' and not paths:
        # 최초 brief의 기억 안내로 시작한다. 압축·재개에서는 이 fallback을 쓰지 않는다.
        memory = (work / 'memory/memory.md').read_text()
    elif len(paths) == 1:
        memory = paths[0].read_text()
        assert str(paths[0].parent.parent / 'brief.md') in context, context
        assert '4KiB' in context and 'processed.txt' in context, context
    else:
        memory = None
    publish_probe(source, dict(memory=memory, context=context))
hook('startup')
print('\x1b[2J\x1b[H' + idle, end='', flush=True)
for line in sys.stdin:
    line = line.strip()
    if line == 'probe-compact':
        hook('compact')
    elif line == 'probe-resume':
        hook('resume')
    elif line == 'probe-child':
        hook('compact', agent_id='child')
        publish_probe('child', {})
    print('\x1b[2J\x1b[Hgot: ' + line + '\n' + idle, end='', flush=True)
"#
    .replace("HARNESS", &serde_json::to_string(harness).unwrap())
    .replace("IDLE", &serde_json::to_string(idle).unwrap());
    std::fs::write(w.scripts.join("claude.sh"), script).unwrap();
    let dir = w.projects.join("demo/automations");
    std::fs::create_dir_all(dir.join("check.memory")).unwrap();
    std::fs::write(dir.join("check.memory/memory.md"), "처음 기억\n").unwrap();
    std::fs::write(dir.join("check.toml"), "[on.schedule]\nevery='1h'\n[do.start]\ntitle='기억 재독'\ntext='기억을 읽는다'\nprofiles=['claude']\nmachines=['labA']\nreuse='open'\nmemory=true\n").unwrap();
    let rule = wait_for("자동화 승인 범위", T, || {
        w.h.ok(&["routine", "list", "demo"])["routines"]
            .as_array()?
            .iter()
            .find(|r| r["name"] == "check" && r["start_approval_sha256"].is_string())
            .cloned()
    });
    w.h.in_terminal(&["automation", "approve", "check", rule["start_approval_sha256"].as_str().unwrap()], &[], T);
    w.h.ok(&["automation", "run", "check"]);
    let id = wait_for("자동화 thread 시작", T, || {
        w.h.ticks(1);
        automatic(&w).first()?.get("thread")?.as_str().map(str::to_owned)
    });
    let row = w.wait_show("자동화 brief 수신", &id, |v| v["brief"] == "delivered");
    let pane = row["pane"].as_str().unwrap();
    let work = PathBuf::from(row["work_dir"].as_str().unwrap());
    let probe = |source: &str| {
        wait_for("기억 파일 재독 완료", T, || {
            std::fs::read(work.join(format!("probe-{source}.json")))
                .ok()
                .map(|bytes| serde_json::from_slice::<Value>(&bytes).unwrap())
        })
    };
    assert_eq!(probe("startup")["memory"], "처음 기억\n", "{harness}");
    for (source, memory) in [("compact", "압축 뒤 최신 기억\n"), ("resume", "재개 뒤 최신 기억\n")] {
        if source == "resume" {
            let mut watch = HostWatch::new(&w.h, T);
            watch.wait("재시작 전 연결 상태", |s| s.connected("labA").then_some(()));
            w.lab.kill("agentA");
            watch.wait("agent 연결 종료", |s| (!s.connected("labA")).then_some(()));
            w.lab.spawn("agentA-reconnected", &w.h, &["agent", "--host", "local", "--name", "labA"]);
            watch.wait("agent 재연결", |s| s.connected("labA").then_some(()));
        }
        std::fs::write(work.join("memory/memory.md"), memory).unwrap();
        w.h.ok(&["pane", "run", pane, &format!("probe-{source}")]);
        assert_eq!(probe(source)["memory"], memory, "{harness}/{source}");
    }
    w.h.ok(&["pane", "run", pane, "probe-child"]);
    probe("child");
    w.h.ok(&["thread", "resolve", "demo", &id]);
}

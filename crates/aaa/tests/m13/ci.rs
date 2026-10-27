//! 실제 host↔agent↔가짜 gh와 임무 inbox 경계. 고정 sleep 대신 flow·gh 호출 완료를 기다린다.
use super::*;
use aaa_common::child_process::StdCommandExt;

fn fake_ci(root: &Path) -> Vec<(String, String)> {
    let dir = root.join("ci");
    std::fs::create_dir_all(dir.join("bin")).unwrap();
    let source = dir.join("gh.py");
    std::fs::write(&source,r#"#!/usr/bin/env python3
import os,sys,json,pathlib,urllib.parse
root=pathlib.Path(os.environ['CI_STUB']); args=sys.argv[1:]
phase=(root/'phase').read_text().strip()
endpoint=args[-1]; result='200'; code=0
query=urllib.parse.parse_qs(urllib.parse.urlsplit(endpoint).query)
page=query.get('page',['1'])[0]
if args[:2]==['run','view']:
    # 실패 시험 줄은 agent가 남기는 32KiB 꼬리 밖(앞쪽)에 둔다. run 4만 같은 job·단계의 다른 시험이 실패한다.
    test='a::second' if args[2]=='4' else 'a::first'
    print('linux\ttest\t2026-10-19T00:00:00.0000000Z test '+test+' ... FAILED\n'+'x'*40000+'\ntest failure tail')
elif 'actions/runs?' in endpoint:
    etag='If-None-Match: "'+phase+'"'
    if phase=='auth':
        code=4;result='auth'
    elif phase=='outage':
        print('HTTP/2.0 403 Forbidden\n\n{}');code=1;result='403'
    elif etag in args and 'created=' not in endpoint:
        print('HTTP/2.0 304 Not Modified\nETag: "'+phase+'"\n\n');code=1;result='304'
    else:
        filename='catchup-2.json' if 'created=' in endpoint and page=='2' else phase+'.json'
        body=(root/filename).read_text()
        print('HTTP/2.0 200 OK\nETag: "'+phase+'"')
        if 'created=' in endpoint and page=='1': print('Link: <ignored>; rel="next"')
        print('\n'+body)
elif '/jobs?' in endpoint:
    print('HTTP/2.0 200 OK\n\n'+json.dumps({'jobs':[{'id':77,'name':('linux-'+phase if phase.startswith('a2-') else 'linux'),'conclusion':'failure','steps':[{'name':'test','conclusion':'failure'}]}]}))
elif '/compare/' in endpoint:
    print('HTTP/2.0 200 OK\n\n'+json.dumps({'commits':[{'sha':'f'*40,'commit':{'message':'병합: 두번째 착지 (t-0043)'}}]}))
else:
    print('HTTP/2.0 404 Not Found\n\n{}');code=1
with (root/'calls').open('a') as f: f.write(json.dumps({'phase':phase,'args':args,'result':result})+'\n')
sys.exit(code)
"#).unwrap();
    assert!(Command::new("cp").arg(source).arg(dir.join("bin/gh")).status_guarded().unwrap().success());
    std::fs::set_permissions(dir.join("bin/gh"), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    std::fs::write(dir.join("phase"), "base").unwrap();
    let mut old = run(9, 'f', "failure");
    old["created_at"] = "2026-10-18T23:59:00Z".into();
    write_runs(&dir, "base", vec![run(1, 'a', "success"), old]);
    std::fs::write(dir.join("catchup-2.json"), "{\"workflow_runs\":[]}").unwrap();
    vec![
        ("AAA_TEST_WATCH_POLL_MS".into(), "10".into()),
        ("PATH".into(), format!("{}:{}", dir.join("bin").display(), std::env::var("PATH").unwrap())),
        ("CI_STUB".into(), dir.to_string_lossy().into()),
    ]
}
fn run(id: u64, sha: char, result: &str) -> Value {
    serde_json::json!({"id":id,"workflow_id":1,"name":"ci","run_number":id,"run_attempt":1,"event":"push","head_sha":sha.to_string().repeat(40),"head_branch":"main","status":"completed","conclusion":result,"created_at":format!("2026-10-19T00:{:02}:00Z",id),"head_commit":{"message":"병합: 착지 (t-0042)"}})
}
fn write_runs(dir: &Path, phase: &str, runs: Vec<Value>) {
    std::fs::write(dir.join(format!("{phase}.json")), serde_json::json!({"workflow_runs":runs}).to_string()).unwrap();
}
fn phase(dir: &Path, name: &str) {
    std::fs::write(dir.join("next"), name).unwrap();
    std::fs::rename(dir.join("next"), dir.join("phase")).unwrap();
}
fn calls(dir: &Path) -> Vec<Value> {
    std::fs::read_to_string(dir.join("calls"))
        .unwrap_or_default()
        .lines()
        .filter_map(|s| serde_json::from_str(s).ok())
        .collect()
}
fn quiet_barrier(w: &World, dir: &Path, name: &str) {
    let before = calls(dir).iter().filter(|c| c["phase"] == name && c["result"] == "304").count();
    wait_for("뒤이은 304 조회(앞 판정 완료)", T, || {
        (calls(dir).iter().filter(|c| c["phase"] == name && c["result"] == "304").count() >= before + 2).then_some(())
    });
    w.h.ticks(2);
}
fn ci_items(w: &World, all: bool) -> Vec<Value> {
    let mut args = vec!["inbox", "list", "demo", "--peek"];
    if all {
        args.push("--all");
    }
    w.h.ok(&args)["items"].as_array().unwrap().iter().filter(|i| i["item_kind"] == "ci").cloned().collect()
}
fn ci_events(w: &World) -> Vec<Value> {
    w.h.ok(&["events", "demo"])["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["kind"].as_str().is_some_and(|k| k.starts_with("ci_")))
        .cloned()
        .collect()
}

#[test]
fn ci_conditional_failure_restart_catchup_and_outage() {
    let mut w = world_setup("tick_secs=1\nci_poll_secs=1\noutage_secs=0\n", false, fake_ci);
    w.add_remote();
    // 원격 agent가 재시작 뒤에도 같은 host 주소로 붙는다.
    let config = std::fs::read_to_string(w.h.cfg.join("host.toml")).unwrap().replace("127.0.0.1:0", &w.addr);
    std::fs::write(w.h.cfg.join("host.toml"), config).unwrap();
    let stub = w.lab.root.join("ci");
    let project = w.projects.join("demo");
    // default 조회 머신은 host의 로컬 agent. 임무는 repo 경로 선택과 독립이다.
    w.h.ok(&["project", "set", "demo", "repos.add", path_str(&w.repo)]);
    assert!(
        Command::new("git")
            .args(["-C", path_str(&w.repo), "remote", "set-url", "origin", "https://github.com/o/app.git"])
            .status_guarded()
            .unwrap()
            .success()
    );
    let mut flow = w.lab.follow("ci-flow", &w.h, "demo");
    std::fs::write(
        project.join("routines/ci-main.md"),
        format!(
            "+++\non=\"ci\"\nrepo={:?}\nbranches=[\"main\"]\n+++\nCI 실패를 분류한다.\n",
            w.repo.file_name().unwrap().to_str().unwrap()
        ),
    )
    .unwrap();
    flow.until("CI 기준 판정", T, |e| e["kind"] == "ci_passed");
    quiet_barrier(&w, &stub, "base");
    assert!(ci_items(&w, true).is_empty());
    let events = ci_events(&w);
    let count = events.len();
    quiet_barrier(&w, &stub, "base");
    assert_eq!(ci_events(&w).len(), count, "304는 흐름 사건도 만들지 않는다");
    write_runs(&stub, "red", vec![run(2, 'b', "failure"), run(1, 'a', "success")]);
    phase(&stub, "red");
    flow.until("CI 실패 사건", T, |e| e["kind"] == "ci_failed");
    quiet_barrier(&w, &stub, "red");
    let items = ci_items(&w, true);
    assert_eq!(items.len(), 1);
    let body = items[0]["body"].as_str().unwrap();
    assert!(body.contains("t-0042") && body.contains("t-0043") && body.contains("test failure tail"));
    assert!(body.contains("\"failures\":[\"a::first\"]"), "꼬리 밖의 실패 시험도 job에 남는다");
    assert!(body.contains("\"truncated\":true"));
    let delivery_runs = |w: &World| -> Vec<Value> {
        w.h.ok(&["routine", "runs", "demo", "ci-main"])["runs"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["id"].as_str().unwrap().starts_with("ci-"))
            .cloned()
            .collect()
    };
    let history = wait_for("첫 CI 실패의 자동화 실행 완료", T, || {
        let runs = delivery_runs(&w);
        (runs.first().is_some_and(|r| r["status"] == "done" && r["detail"] == "coordinator 안내")).then_some(runs)
    });
    // host 재시작: 같은 SHA는 항목·깨움을 늘리지 않고 created 창을 쪽 넘겨 따라잡는다.
    w.lab.kill("host");
    w.spawn_host("host2");
    wait_for("재연결", T, || (w.h.connected("labA") && w.h.connected("labB")).then_some(()));
    quiet_barrier(&w, &stub, "red");
    assert_eq!(ci_items(&w, true).len(), 1);
    assert!(calls(&stub).iter().any(|c| {
        c["args"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a.as_str().is_some_and(|s| s.contains("created=") && s.contains("page=2")))
    }));
    assert_eq!(delivery_runs(&w), history, "재시작 뒤 같은 전달 기록도 늘지 않는다");
    // 같은 실패 집합은 앞 항목에 추가되며 새 wake는 없다.
    write_runs(&stub, "same", vec![run(3, 'c', "failure"), run(2, 'b', "failure")]);
    phase(&stub, "same");
    quiet_barrier(&w, &stub, "same");
    assert_eq!(ci_items(&w, true).len(), 1);
    assert!(ci_items(&w, true)[0]["body"].as_str().unwrap().contains(&"c".repeat(40)));
    assert_eq!(w.h.ok(&["routine", "runs", "demo", "ci-main"])["runs"][0]["detail"], "같은 실패 집합");
    // 같은 job·단계라도 실패 시험이 바뀌면 같은 항목에 덧붙이고 다시 깨운다(0a8c294f windows).
    write_runs(&stub, "other", vec![run(4, 'd', "failure"), run(3, 'c', "failure")]);
    phase(&stub, "other");
    quiet_barrier(&w, &stub, "other");
    assert_eq!(ci_items(&w, true).len(), 1);
    assert!(ci_items(&w, true)[0]["body"].as_str().unwrap().contains("a::second"));
    let latest = wait_for("바뀐 실패 시험의 새 자동화 실행 완료", T, || {
        delivery_runs(&w).into_iter().find(|r| !history.iter().any(|old| old["id"] == r["id"]) && r["status"] == "done")
    });
    assert_eq!((&latest["status"], &latest["detail"]), (&"done".into(), &history[0]["detail"]), "첫 빨강처럼 깨운다");
    // 꺼진 사이 실패→회복을 함께 받으면 새 깨움 없이 앞 CI 항목을 닫는다.
    w.lab.kill("host2");
    write_runs(&stub, "fixed", vec![run(6, '0', "success"), run(5, 'e', "failure"), run(4, 'd', "failure")]);
    phase(&stub, "fixed");
    w.spawn_host("host3");
    wait_for("재연결", T, || (w.h.connected("labA") && w.h.connected("labB")).then_some(()));
    quiet_barrier(&w, &stub, "fixed");
    assert!(ci_items(&w, false).is_empty());
    assert!(ci_items(&w, true).iter().all(|i| i["event"] == "CI fixed"));
    assert!(ci_events(&w).iter().any(|e| e["kind"] == "ci_fixed"));
    phase(&stub, "outage");
    wait_for("CI 조회 outage", T, || {
        let items = w.h.ok(&["inbox", "list", "demo", "--peek"]);
        items["items"].as_array()?.iter().find(|i| i["item_kind"] == "outage" && i["subject"] == "ci:ci-main").cloned()
    });
    let count = calls(&stub).len();
    wait_for("두번째 실패 조회", T, || (calls(&stub).len() > count + 1).then_some(()));
    w.h.ticks(2);
    let items = w.h.ok(&["inbox", "list", "demo", "--peek"]);
    assert_eq!(
        items["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|i| i["item_kind"] == "outage" && i["subject"] == "ci:ci-main")
            .count(),
        1
    );
    // PR의 조용한 생략과 달리 CI 조회 머신의 인증 실패는 계속 알린다.
    phase(&stub, "fixed");
    quiet_barrier(&w, &stub, "fixed");
    phase(&stub, "auth");
    wait_for("CI 인증 실패 알림", T, || {
        let items = w.h.ok(&["inbox", "list", "demo", "--peek"]);
        items["items"]
            .as_array()?
            .iter()
            .find(|i| {
                i["item_kind"] == "outage"
                    && i["subject"] == "ci:ci-main"
                    && i["summary"].as_str().is_some_and(|s| s.contains("인증 실패"))
            })
            .cloned()
    });
    // CI 작업 때문에 새로운 thread를 만들거나 기존 착지 thread를 되살리지 않는다.
    assert_eq!(w.h.ok(&["thread", "list", "demo"])["threads"].as_array().unwrap().len(), 0);
}

/// A2: CI 둘은 첫 inbox 하나, 셋째는 같은 thread에 Queue, 옛 완료는 닫지 않고 새 완료 뒤 기억을 다음 thread로 옮긴다.
#[test]
fn automation_ci_reuses_open_thread_and_closes_only_after_new_report_with_memory() {
    let w = world_setup("tick_secs=1\nci_poll_secs=1\n", false, fake_ci);
    let script = format!(
        r#"#!/bin/sh
AA={:?}
hook() {{ "$AA" hook --agent claude "$1" >/dev/null; }}
hook '{{"hook_event_name":"SessionStart","session_id":"0f5b2c1e-7a3d-4c8e-9b21-5d6e7f809a1b"}}'
printf '────────────────────────────────────────\n❯ \n────────────────────────────────────────\n'
work=".aaa/$AAA_PROJECT-$AAA_THREAD"
mkfifo "$work/turn"
drain() {{ find "$work/inbox" -name '*.json' -type f -exec basename '{{}}' \; > "$work/processed.txt"; }}
while IFS= read -r line; do
  if [ "$silent" = 1 ]; then printf '%s\n' "$line" >> "$work/silent-input"; continue; fi
  hook '{{"hook_event_name":"UserPromptSubmit","session_id":"0f5b2c1e-7a3d-4c8e-9b21-5d6e7f809a1b"}}'
  printf '\033[2J\033[H✻ Working… (1s · esc to interrupt)\n────────────────────────────────────────\n❯ \n────────────────────────────────────────\n'
  # 시험은 host가 작업 화면을 관측한 뒤 FIFO로 다음 화면을 허용한다.
  read proceed < "$work/turn"
  case "$line" in
    a2-silent) silent=1; echo a2-silent-armed; stty -echo ;;
    *brief.md*) work=$(dirname "$(printf '%s' "$line" | sed 's/를 읽고.*//')"); drain; printf '[완료] 첫 사건 처리\n\n## Report\n초기\n' > "$work/report.md" ;;
    a2-drain) drain; echo a2-drained ;;
    a2-complete) mkdir -p "$work/memory/history"; cp "$work/memory/memory.md" "$work/memory/history/before.md"; printf '다음에 볼 것: 같은 실패를 대조한다\n교훈: 입력 뒤 보고한다\n' > "$work/memory/memory.md"; printf '[완료] 이어 받은 사건 처리\n\n## Report\n새 완료\n' > "$work/report.md" ;;
    *) echo "got: $line" ;;
  esac
  hook '{{"hook_event_name":"Stop","session_id":"0f5b2c1e-7a3d-4c8e-9b21-5d6e7f809a1b"}}'
  printf '\033[2J\033[H────────────────────────────────────────\n❯ \n────────────────────────────────────────\n'
done
"#,
        lab::BIN
    );
    std::fs::write(w.scripts.join("claude.sh"), script).unwrap();
    w.h.in_terminal(&["safety", "set", "start_threads", "auto"], &[("[y/N]", "y")], T);
    w.h.ok(&["project", "set", "demo", "repos.add", path_str(&w.repo)]);
    assert!(
        Command::new("git")
            .args(["-C", path_str(&w.repo), "remote", "set-url", "origin", "https://github.com/o/app.git"])
            .status_guarded()
            .unwrap()
            .success()
    );
    let dir = w.projects.join("demo/automations");
    std::fs::create_dir_all(&dir).unwrap();
    let memory_root = dir.join("check.memory");
    std::fs::create_dir_all(&memory_root).unwrap();
    let original_memory = format!("이전 판단\n{}\n", "x".repeat(3000));
    std::fs::write(memory_root.join("memory.md"), &original_memory).unwrap();
    std::fs::write(dir.join("check.toml"),"title='CI 분류'\n[on.ci]\nrepo='repo'\nbranches=['main']\nevents=['failed']\n[do.start]\ntitle='자동 분류'\ntext='입력을 분류한다'\nprofiles=['claude']\nmachines=['labA']\nreuse='open'\nmemory=true\nclose_idle='30m'\n[limits]\nbatch='5m'\n").unwrap();
    let rule = wait_for("규칙 승인 범위", T, || {
        w.h.ticks(1);
        w.h.ok(&["routine", "list"])["routines"]
            .as_array()?
            .iter()
            .find(|r| r["name"] == "check" && r["start_approval_sha256"].is_string())
            .cloned()
    });
    w.h.in_terminal(&["automation", "approve", "check", rule["start_approval_sha256"].as_str().unwrap()], &[], T);
    let stub = w.lab.root.join("ci");
    quiet_barrier(&w, &stub, "base");
    write_runs(&stub, "a2-one", vec![run(2, 'b', "failure"), run(1, 'a', "success")]);
    phase(&stub, "a2-one");
    wait_for("첫 CI 실패가 batch에 대기", T, || {
        w.h.ticks(1);
        w.h.ok(&["routine", "list"])["routines"]
            .as_array()?
            .iter()
            .find(|r| r["name"] == "check" && r["queue"]["pending"] == 1)
            .cloned()
    });
    write_runs(&stub, "a2-two", vec![run(3, 'c', "failure"), run(2, 'b', "failure")]);
    phase(&stub, "a2-two");
    wait_for("둘째 CI 실패도 같은 batch에 대기", T, || {
        w.h.ticks(1);
        w.h.ok(&["routine", "list"])["routines"]
            .as_array()?
            .iter()
            .find(|r| r["name"] == "check" && r["queue"]["pending"] == 2)
            .cloned()
    });
    std::fs::write(w.h.cfg.join("automation-clock"), (lab::now_ms() + 6 * 60 * 1000).to_string()).unwrap();
    let id = wait_for("CI 두 사건의 첫 thread", T, || {
        w.h.ticks(1);
        super::automation_start::automatic(&w).first().and_then(|e| e["thread"].as_str().map(str::to_owned))
    });
    let working = w.wait_show("첫 brief 작업 화면", &id, |v| v["agent"]["activity"] == "working");
    let gate = Path::new(working["work_dir"].as_str().unwrap()).join("turn");
    std::fs::write(&gate, "go\n").unwrap();
    // 보고 hash 저장과 자동 ack는 별도 기록이다. Idle만으로 ack 완료를 추정하지 않는다.
    let first = w.wait_show("첫 완료의 자동 확인과 Idle", &id, |v| {
        v["report_hash"].is_string() && v["agent"]["activity"] == "idle" && v["acked_hash"] == v["report_hash"]
    });
    let work = PathBuf::from(first["work_dir"].as_str().unwrap());
    let initial: Vec<_> = std::fs::read_dir(work.join("inbox")).unwrap().map(|e| e.unwrap().path()).collect();
    assert_eq!(initial.len(), 1);
    let packet: Value = serde_json::from_slice(&std::fs::read(&initial[0]).unwrap()).unwrap();
    assert_eq!(packet["inputs"].as_array().unwrap().len(), 2, "두 실패를 하나로 묶는다");
    assert_eq!(first["acked_hash"], first["report_hash"], "자동 완료는 저절로 확인한다");
    write_runs(&stub, "a2-third", vec![run(4, 'd', "failure"), run(3, 'c', "failure"), run(2, 'b', "failure")]);
    phase(&stub, "a2-third");
    wait_for("같은 thread의 이어 보내기 전달 기록", T, || {
        w.h.ticks(1);
        w.h.ok(&["routine", "runs", "check"])["runs"]
            .as_array()?
            .iter()
            .any(|r| r["detail"].as_str().is_some_and(|s| s.contains("이어 보냄")) && r["thread"] == id)
            .then_some(())
    });
    assert_eq!(super::automation_start::automatic(&w).len(), 1);
    std::fs::write(&gate, "go\n").unwrap();
    w.wait_show("이어 보내기 뒤 Idle", &id, |v| v["agent"]["activity"] == "idle");
    w.message(&id, "a2-drain", false);
    std::fs::write(&gate, "go\n").unwrap();
    wait_for("처리 기록 두 파일", T, || {
        std::fs::read_to_string(work.join("processed.txt")).ok().filter(|s| s.lines().count() == 2)
    });
    w.wait_show("처리 뒤 Idle", &id, |v| v["agent"]["activity"] == "idle");
    // 처리 기록은 비었지만 완료 보고는 마지막 입력보다 옛것이다. 종료 조건 2를 빼면 여기서 잘못 끝난다.
    std::fs::write(w.h.cfg.join("automation-clock"), (lab::now_ms() + 31 * 60 * 1000).to_string()).unwrap();
    wait_for("입력 처리 상태 판정", T, || {
        w.h.ticks(1);
        let raw = std::fs::read_to_string(work.join("processed.txt")).ok()?;
        (raw.lines().count() == 2).then_some(())
    });
    w.h.ticks(2);
    assert_eq!(w.show(&id)["status"], "open", "옛 완료 보고로 끝내면 안 된다");
    w.message(&id, "a2-complete", false);
    // Working 사건 뒤 FIFO를 열기 전에 host 원문을 고쳐, 마지막 회수와의 순서를 고정한다.
    let host_memory = format!("{original_memory}coordinator가 열린 동안 덧붙인 줄\n");
    std::fs::write(memory_root.join("memory.md"), &host_memory).unwrap();
    std::fs::write(&gate, "go\n").unwrap();
    w.wait_show("새 완료 뒤 짧은 끝내기", &id, |v| v["status"] == "resolved");
    let memory = std::fs::read_to_string(dir.join("check.memory/memory.md")).unwrap();
    assert_eq!(memory, host_memory, "마지막 회수로 host에서 쓴 줄이 사라졌다");
    assert!(memory.len() <= 4096);
    assert_eq!(std::fs::read_to_string(memory_root.join("history/before.md")).unwrap(), original_memory);
    let conflicts = dir.join("check.memory-conflicts").join(&id);
    let snapshots: Vec<_> = std::fs::read_dir(&conflicts).unwrap().map(|e| e.unwrap().path()).collect();
    assert_eq!(snapshots.len(), 1, "보고 회수와 마지막 회수는 같은 충돌 사본을 재사용한다");
    let thread_memory = std::fs::read_to_string(snapshots[0].join("thread/memory.md")).unwrap();
    assert!(thread_memory.contains("입력 뒤 보고"));
    assert!(thread_memory.len() <= 4096);
    assert_eq!(std::fs::read_to_string(snapshots[0].join("host/memory.md")).unwrap(), host_memory);
    assert!(w.h.ok(&["inbox", "list", "demo", "--peek"])["items"].as_array().unwrap().iter().any(|i| {
        i["item_kind"] == "automation-memory" && i["summary"].as_str().is_some_and(|s| s.contains("기억 충돌"))
    }));
    std::fs::write(w.h.cfg.join("automation-clock"), (lab::now_ms() + 6 * 60 * 1000).to_string()).unwrap();
    write_runs(&stub, "a2-next", vec![run(5, 'e', "failure"), run(4, 'd', "failure")]);
    phase(&stub, "a2-next");
    let next = wait_for("다음 사건의 새 thread", T, || {
        w.h.ticks(1);
        super::automation_start::automatic(&w)
            .iter()
            .find_map(|e| e["thread"].as_str().filter(|t| *t != id).map(str::to_owned))
    });
    let starting = w.wait_show("새 thread 작업 화면", &next, |v| v["agent"]["activity"] == "working");
    let next_gate = Path::new(starting["work_dir"].as_str().unwrap()).join("turn");
    std::fs::write(&next_gate, "go\n").unwrap();
    let shown = w.wait_show("새 thread 기억 주입", &next, |v| v["brief"] == "delivered");
    assert_eq!(
        std::fs::read_to_string(Path::new(shown["work_dir"].as_str().unwrap()).join("memory/memory.md")).unwrap(),
        memory
    );
    w.wait_show("다음 thread Idle", &next, |v| v["agent"]["activity"] == "idle");
    w.message(&next, "a2-silent", false);
    std::fs::write(&next_gate, "go\n").unwrap();
    w.wait_show("조용한 입력창", &next, |v| v["agent"]["activity"] == "idle");
    write_runs(&stub, "a2-unknown", vec![run(6, '1', "failure"), run(5, 'e', "failure")]);
    phase(&stub, "a2-unknown");
    let blocked = wait_for("전달 모름은 막힘", T, || {
        w.h.ticks(1);
        w.h.ok(&["routine", "list"])["routines"]
            .as_array()?
            .iter()
            .find(|r| r["name"] == "check" && r["queue"]["inflight"]["unknown"] == true)
            .cloned()
    });
    assert_eq!(blocked["queue"]["inflight"]["thread"], next);
    let log = Path::new(shown["work_dir"].as_str().unwrap()).join("silent-input");
    let before = wait_for("조용히 받은 입력 파일", T, || std::fs::read_to_string(&log).ok());
    w.h.ticks(3);
    assert_eq!(std::fs::read_to_string(log).unwrap(), before, "모름 묶음을 자동 재전송하지 않는다");
    assert_eq!(super::automation_start::automatic(&w).len(), 2);
}

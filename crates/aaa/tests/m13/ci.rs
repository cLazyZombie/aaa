//! 실제 host↔agent↔가짜 gh와 임무 inbox 경계. 고정 sleep 대신 flow·gh 호출 완료를 기다린다.
use super::*;

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
    print('x'*40000+'\ntest failure tail')
elif 'actions/runs?' in endpoint:
    etag='If-None-Match: "'+phase+'"'
    if phase=='outage':
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
    print('HTTP/2.0 200 OK\n\n'+json.dumps({'jobs':[{'id':77,'name':'linux','conclusion':'failure','steps':[{'name':'test','conclusion':'failure'}]}]}))
elif '/compare/' in endpoint:
    print('HTTP/2.0 200 OK\n\n'+json.dumps({'commits':[{'sha':'f'*40,'commit':{'message':'병합: 두번째 착지 (t-0043)'}}]}))
else:
    print('HTTP/2.0 404 Not Found\n\n{}');code=1
with (root/'calls').open('a') as f: f.write(json.dumps({'phase':phase,'args':args,'result':result})+'\n')
sys.exit(code)
"#).unwrap();
    assert!(Command::new("cp").arg(source).arg(dir.join("bin/gh")).status().unwrap().success());
    std::fs::set_permissions(dir.join("bin/gh"), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    std::fs::write(dir.join("phase"), "base").unwrap();
    let mut old = run(9, 'f', "failure");
    old["created_at"] = "2026-10-18T23:59:00Z".into();
    write_runs(&dir, "base", vec![run(1, 'a', "success"), old]);
    std::fs::write(dir.join("catchup-2.json"), "{\"workflow_runs\":[]}").unwrap();
    vec![
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
fn ci_conditional_failure_mission_restart_catchup_and_outage() {
    let mut w = world_setup("tick_secs=1\nci_poll_secs=1\npr_poll_secs=3600\noutage_secs=0\n", false, fake_ci);
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
            .status()
            .unwrap()
            .success()
    );
    std::fs::create_dir_all(project.join("missions")).unwrap();
    std::fs::write(project.join("missions/ci-watch.md"),"+++\nprofile=\"resumable\"\nmachine=\"labB\"\nbatch_s=0\nmin_gap=\"1s\"\nmax_wakes=20\nmax_minutes=15\nreset_after_wakes=3\nwithdrawal=\"none\"\n+++\nCI 입력을 읽고 분류한다.\n").unwrap();
    let approved = w.h.in_terminal(&["mission", "approve", "demo", "ci-watch"], &[("그대로 입력한다:", "ci-watch")], T);
    assert!(!approved.contains("일치하지"), "{approved}");
    w.h.ok(&["mission", "start", "demo", "ci-watch"]);
    let mission = wait_for("임무 brief 전달", T, || {
        let m = w.h.ok(&["mission", "show", "demo", "ci-watch"]);
        (m["mission"]["thread"]["brief"] == "delivered").then_some(m)
    });
    let work = PathBuf::from(mission["mission"]["thread"]["work_dir"].as_str().unwrap());
    let mut flow = w.lab.follow("ci-flow", &w.h, "demo");
    std::fs::write(
        project.join("routines/ci-main.md"),
        format!(
            "+++\non=\"ci\"\nrepo={:?}\nbranches=[\"main\"]\nmission=\"ci-watch\"\n+++\nCI 실패를 분류한다.\n",
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
    flow.until("CI 실패 임무 깨움", T, |e| e["kind"] == "mission_wake");
    wait_for("임무 CI 입력 파일", T, || {
        std::fs::read_dir(work.join("inbox")).ok()?.filter_map(Result::ok).find_map(|e| {
            let v: Value = serde_json::from_slice(&std::fs::read(e.path()).ok()?).ok()?;
            (v["source"] == "ci").then_some(v)
        })
    });
    w.h.wait_output(mission["mission"]["thread"]["pane"].as_str().unwrap(), "got: [aaa 임무 ci-watch]", T);
    quiet_barrier(&w, &stub, "red");
    let items = ci_items(&w, true);
    assert_eq!(items.len(), 1);
    let body = items[0]["body"].as_str().unwrap();
    assert!(body.contains("t-0042") && body.contains("t-0043") && body.contains("test failure tail"));
    assert!(body.contains("\"truncated\":true"));
    assert_eq!(w.h.ok(&["mission", "show", "demo", "ci-watch"])["mission"]["wakes_today"], 1);
    assert_eq!(
        w.h.ok(&["mission", "show", "demo", "ci-watch"])["mission"]["pending"],
        1,
        "처리하지 않은 최초 입력 한 건 외의 새 CI 입력이 없어야 한다"
    );
    // host 재시작: 같은 SHA는 항목·깨움을 늘리지 않고 created 창을 쪽 넘겨 따라잡는다.
    w.lab.kill("host");
    w.spawn_host("host2");
    wait_for("재연결", T, || (w.h.connected("labA") && w.h.connected("labB")).then_some(()));
    quiet_barrier(&w, &stub, "red");
    assert_eq!(ci_items(&w, true).len(), 1);
    assert_eq!(w.h.ok(&["mission", "show", "demo", "ci-watch"])["mission"]["wakes_today"], 1);
    assert_eq!(
        w.h.ok(&["mission", "show", "demo", "ci-watch"])["mission"]["pending"],
        1,
        "처리하지 않은 최초 입력 한 건 외의 새 CI 입력이 없어야 한다"
    );
    assert!(calls(&stub).iter().any(|c| {
        c["args"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a.as_str().is_some_and(|s| s.contains("created=") && s.contains("page=2")))
    }));
    // 같은 실패 집합은 앞 항목에 추가되며 새 wake는 없다.
    write_runs(&stub, "same", vec![run(3, 'c', "failure"), run(2, 'b', "failure")]);
    phase(&stub, "same");
    quiet_barrier(&w, &stub, "same");
    assert_eq!(ci_items(&w, true).len(), 1);
    assert!(ci_items(&w, true)[0]["body"].as_str().unwrap().contains(&"c".repeat(40)));
    assert_eq!(w.h.ok(&["mission", "show", "demo", "ci-watch"])["mission"]["wakes_today"], 1);
    assert_eq!(
        w.h.ok(&["mission", "show", "demo", "ci-watch"])["mission"]["pending"],
        1,
        "처리하지 않은 최초 입력 한 건 외의 새 CI 입력이 없어야 한다"
    );
    // 꺼진 사이 실패→회복을 함께 받으면 새 깨움 없이 앞 CI 항목을 닫는다.
    w.lab.kill("host2");
    write_runs(&stub, "fixed", vec![run(5, 'e', "success"), run(4, 'd', "failure"), run(3, 'c', "failure")]);
    phase(&stub, "fixed");
    w.spawn_host("host3");
    wait_for("재연결", T, || (w.h.connected("labA") && w.h.connected("labB")).then_some(()));
    quiet_barrier(&w, &stub, "fixed");
    assert!(ci_items(&w, false).is_empty());
    assert!(ci_items(&w, true).iter().all(|i| i["event"] == "CI fixed"));
    assert_eq!(w.h.ok(&["mission", "show", "demo", "ci-watch"])["mission"]["wakes_today"], 1);
    assert_eq!(
        w.h.ok(&["mission", "show", "demo", "ci-watch"])["mission"]["pending"],
        1,
        "처리하지 않은 최초 입력 한 건 외의 새 CI 입력이 없어야 한다"
    );
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
    // CI 작업 때문에 새로운 thread를 만들거나 기존 착지 thread를 되살리지 않는다.
    assert_eq!(w.h.ok(&["thread", "list", "demo", "--role", "evaluator"])["threads"].as_array().unwrap().len(), 1);
    assert_eq!(w.h.ok(&["thread", "list", "demo"])["threads"].as_array().unwrap().len(), 0);
}

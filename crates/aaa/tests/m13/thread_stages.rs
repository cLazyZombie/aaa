//! 실제 보고 회수 → DB 한도 → 예약 기동·CLI 표지·host 재시작을 같은 기록으로 검증한다.
use super::*;

#[test]
fn report_stages_release_queue_capacity_and_block_new_work_after_return() {
    let mut w = report_shell_world();
    w.h.ok(&["project", "set", "demo", "max_parallel_threads", "1"]);
    w.h.ok(&["project", "set", "demo", "thread_profile", "fake"]);
    let task = w.task("stage-work.md", "noreport");
    let first = w.start(&["--title", "전체 확인 완료", "--profile", "fake", "--kind", "dir", "--task-file", &task]);
    let first = first["id"].as_str().unwrap().to_string();
    w.wait_show("첫 작업 준비", &first, |v| v["brief"] == "delivered");
    std::fs::write(w.projects.join("demo/TASKS.md"), "## Backlog\n- [ ] 첫 예약 (fake@labA) [작업 폴더]\n  noreport\n- [ ] 둘째 예약 (fake@labA) [작업 폴더]\n  noreport\n").unwrap();
    for title in ["첫 예약", "둘째 예약"] {
        w.h.ok(&["task", "reserve", title]);
    }
    w.h.ticks(1);
    assert!(w.h.ok(&["task", "queue"])["entries"].as_array().unwrap().iter().all(|q| q["thread"].is_null()));
    let report = |id: &str, text: &str| {
        let v = w.show(id);
        let path = Path::new(v["work_dir"].as_str().unwrap()).join("report.md");
        std::fs::write(path, text).unwrap();
    };
    report(&first, "[병합 준비됨] 전체 확인 완료\n\n## Next\n- 착지 창 대기\n");
    w.wait_show("병합 대기 회수", &first, |v| v["stage"] == "merge_ready");
    let queued_thread = |title: &str| {
        wait_for("보고 회수 사건 뒤 예약 자동 기동", T, || {
            let q = w.h.ok(&["task", "queue"]);
            q["entries"].as_array()?.iter().find(|q| q["task"]["title"] == title)?["thread"]
                .as_str()
                .map(str::to_string)
        })
    };
    let next = queued_thread("첫 예약");
    w.wait_show("첫 예약 준비", &next, |v| v["brief"] == "delivered");
    report(&next, "[착지] 전체 검증 실패를 수리 중\n");
    w.wait_show("병합 중 회수", &next, |v| v["stage"] == "landing");
    let third = queued_thread("둘째 예약");
    w.wait_show("착지 자리 제외 뒤 둘째 예약", &third, |v| v["brief"] == "delivered");
    for args in [vec!["thread", "list"], vec!["task", "queue"], vec!["context", "--peek"]] {
        let out = w.h.run(&args);
        assert!(out.status.success());
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.contains("병합 중"), "{text}");
        if args[0] != "task" {
            assert!(text.contains("병합 대기"), "{text}");
        }
    }
    let q = w.h.ok(&["task", "queue"]);
    assert_eq!(q["entries"][0]["stage"], "landing");
    w.h.ok(&["thread", "ack", &first]);
    assert_eq!(w.show(&first)["stage"], "merge_ready", "ack는 작업 복귀가 아니다");
    report(&first, "[진행 중] 중간 단계 착지 뒤 다음 단계 구현\n");
    w.wait_show("다음 단계는 작업으로 복귀", &first, |v| v["stage"].is_null());
    let denied = w
        .h
        .json(&["thread", "start", "--title", "초과 기동", "--profile", "fake", "--kind", "dir", "--task-file", &task])
        .unwrap_err();
    assert!(denied.contains("실행 한도"), "{denied}");
    for id in [&first, &next, &third] {
        assert_eq!(w.show(id)["status"], "open");
    }
    w.lab.kill("host");
    w.spawn_host("host-stages-restored");
    wait_for("보고 단계 재연결", T, || w.h.connected("labA").then_some(()));
    assert_eq!(w.show(&next)["stage"], "landing");
    assert!(w.show(&first)["stage"].is_null());
    assert_eq!(w.h.ok(&["task", "queue"])["entries"][0]["stage"], "landing");
}

#[test]
fn explicit_over_limit_start_keeps_queue_waiting_and_requires_coordinator_user_request() {
    let mut w = world();
    w.h.ok(&["project", "set", "demo", "max_parallel_threads", "1"]);
    w.h.ok(&["project", "set", "demo", "thread_profile", "fake"]);
    let task = w.task("over-limit-busy.md", "noreport");
    let first = w.start(&["--title", "자리 보유", "--profile", "fake", "--kind", "dir", "--task-file", &task]);
    let first = first["id"].as_str().unwrap().to_string();
    w.wait_show("첫 자리 준비", &first, |v| v["brief"] == "delivered");
    for title in ["직접 강제", "채팅 강제", "예약 대기"] {
        w.h.ok(&[
            "task",
            "add",
            title,
            "--profile",
            "fake",
            "--machine",
            "labA",
            "--repo",
            "작업 폴더",
            "--note",
            "noreport",
        ]);
    }
    w.h.ok(&["task", "reserve", "예약 대기"]);
    let denied = w.h.json(&["thread", "start", "demo", "--from-task", "직접 강제"]).unwrap_err();
    assert!(denied.contains("실행 한도"), "{denied}");
    let forced = w.h.ok(&["thread", "start", "demo", "--from-task", "직접 강제", "--link-task", "--over-limit"]);
    let forced_id = forced["id"].as_str().unwrap().to_string();
    assert_eq!(forced["over_limit"]["actor"], "local:cli");
    assert!(forced["over_limit"]["at_ms"].as_u64().unwrap() >= w.show(&first)["created_at_ms"].as_u64().unwrap());
    let before = forced["over_limit"].clone();
    let wait_queue = |w: &World| {
        w.h.ticks(1);
        let q = w.h.ok(&["task", "queue"]);
        assert_eq!(q["entries"][0]["state"], "queued");
        assert!(q["entries"][0]["thread"].is_null());
    };
    wait_queue(&w);
    assert_eq!(w.h.ok(&["thread", "list", "demo"])["threads"].as_array().unwrap().len(), 2);
    let coord = w.h.ok(&["project", "open", "demo", "--profile", "fake"]);
    let pane = coord["pane"]["id"].as_str().unwrap();
    w.wait_screen(pane, "fake>");
    let mut number = 0;
    let mut probe = |command: &str| {
        number += 1;
        let prefix = w.lab.root.join(format!("limit-request-{number}"));
        let script = w.task(&format!("limit-request-{number}.sh"), &format!(
            "'{}' --json {command} > '{p}.out' 2> '{p}.err'\nprintf '%s' \"$?\" > '{p}.rc'\nprintf 'limit-request-{number}-finished\\n'\n",
            lab::BIN, p=prefix.display()
        ));
        w.h.ok(&["pane", "run", pane, &format!("post-probe {script}")]);
        w.h.wait_output(pane, &format!("limit-request-{number}-finished"), T);
        (
            std::fs::read_to_string(prefix.with_extension("rc")).unwrap(),
            std::fs::read_to_string(prefix.with_extension("err")).unwrap(),
            std::fs::read_to_string(prefix.with_extension("out")).unwrap(),
        )
    };
    for (command, message) in [
        ("thread start demo --from-task '채팅 강제' --over-limit", "사용자가 그 task"),
        ("thread start demo --from-task '채팅 강제' --over-limit --user-request ' '", "근거"),
        (
            "thread start demo --title '채팅 강제' --task-file /dev/null --over-limit --user-request '사용자가 강제 시작 지시'",
            "--from-task",
        ),
    ] {
        let (code, error, _) = probe(command);
        assert_ne!(code, "0", "{command}");
        assert!(error.contains(message), "{command}: {error}");
    }
    let reason = "사용자가 채팅에서 채팅 강제 task를 한도 무시하고 시작하라고 지시";
    let (code, error, out) =
        probe(&format!("thread start demo --from-task '채팅 강제' --link-task --over-limit --user-request '{reason}'"));
    assert_eq!(code, "0", "{error}");
    let approved: Value = serde_json::from_str(&out).unwrap();
    let approved_id = approved["id"].as_str().unwrap().to_string();
    assert!(approved["over_limit"]["actor"].as_str().unwrap().starts_with("local:cli@"));
    assert_eq!(approved["over_limit"]["user_request"], reason);
    wait_queue(&w);
    let plain = w.h.run(&["thread", "show", "demo", &approved_id]);
    assert!(String::from_utf8_lossy(&plain.stdout).contains("한도 초과로 시작"));
    let events = w.h.ok(&["events", "demo"]);
    assert!(events["events"].as_array().unwrap().iter().any(|e| e["thread"] == approved_id
        && e["kind"] == "thread_started"
        && e["actor"] == approved["over_limit"]["actor"]));
    w.lab.kill("host");
    w.spawn_host("host-over-limit-restored");
    wait_for("한도 초과 기록 재연결", T, || w.h.connected("labA").then_some(()));
    assert_eq!(w.show(&forced_id)["over_limit"], before);
    for id in [&first, &forced_id] {
        w.h.ok(&["thread", "resolve", "demo", id]);
        wait_queue(&w);
    }
    w.h.ok(&["thread", "resolve", "demo", &approved_id]);
    let queued = wait_for("모든 한도 점유 반환 뒤 예약 기동", T, || {
        w.h.ok(&["task", "queue"])["entries"][0]["thread"].as_str().map(str::to_string)
    });
    w.wait_show("예약의 실제 기동", &queued, |v| v["brief"] == "delivered");
    assert!(w.show(&queued)["over_limit"].is_null());
}

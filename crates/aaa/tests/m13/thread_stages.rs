//! 실제 보고 회수 → DB 한도 → 예약 기동·CLI 표지·host 재시작을 같은 기록으로 검증한다.
use super::*;

#[test]
fn report_stages_release_queue_capacity_and_block_new_work_after_return() {
    let mut w = world();
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
    w.wait_show("착지 중 회수", &next, |v| v["stage"] == "landing");
    let third = queued_thread("둘째 예약");
    w.wait_show("착지 자리 제외 뒤 둘째 예약", &third, |v| v["brief"] == "delivered");
    for args in [vec!["thread", "list"], vec!["task", "queue"], vec!["context", "--peek"]] {
        let out = w.h.run(&args);
        assert!(out.status.success());
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.contains("착지 중"), "{text}");
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

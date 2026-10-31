//! 실제 CLI 다중 입력 → 완료 보관 → 자리 제한 → 재시작·중복 tick에도 한 번만 기동.
use super::*;

#[test]
fn multiple_predecessors_start_once_after_all_archives_and_capacity_across_restart() {
    let mut w = world();
    w.h.ok(&["project", "set", "demo", "max_parallel_threads", "1"]);
    w.h.ok(&["project", "set", "demo", "thread_profile", "fake"]);
    let path = w.projects.join("demo/TASKS.md");
    std::fs::write(&path, "## Backlog\n- [ ] 첫째 (fake@labA) [작업 폴더]\n  noreport\n- [ ] 둘째 (fake@labA) [작업 폴더]\n  noreport\n- [ ] 합류 (fake@labA) [작업 폴더]\n  noreport\n- [ ] 독립 (fake@labA) [작업 폴더]\n  noreport\n").unwrap();
    let tasks = w.h.ok(&["task", "list"])["tasks"].as_array().unwrap().clone();
    let ids: Vec<_> = tasks.iter().map(|t| t["short_id"].as_str().unwrap()).collect();
    // 한 --after의 여러 값과 반복 옵션, 같은 ID 중복을 실제 CLI로 보낸다.
    w.h.ok(&["task", "reserve", ids[2], "--after", ids[0], ids[1], "--after", ids[0]]);
    w.h.ticks(1);
    let queue = || w.h.ok(&["task", "queue"])["entries"].as_array().unwrap().clone();
    assert_eq!(queue()[0]["after_all"].as_array().unwrap().len(), 2);
    assert!(w.h.ok(&["thread", "list"])["threads"].as_array().unwrap().is_empty());
    // 막힌 합류가 뒤의 독립 예약을 막으면 안 된다. 그 일은 이후 동시 한도도 채운다.
    w.h.ok(&["task", "reserve", ids[3]]);
    let independent = wait_for("독립 예약 기동", T, || queue()[1]["thread"].as_str().map(str::to_string));
    w.wait_show("독립 pane 준비", &independent, |v| v["brief"] == "delivered");
    let complete = |title: &str| {
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, text.replace(&format!("- [ ] {title}"), &format!("- [x] {title}"))).unwrap();
        w.h.ticks(1);
    };
    complete("첫째");
    wait_for("첫째 완료 보관", T, || {
        w.h.ok(&["task", "archive"])["entries"].as_array()?.iter().any(|a| a["task"]["title"] == "첫째").then_some(())
    });
    assert!(queue()[0]["thread"].is_null());
    assert!(queue()[0]["reason"].as_str().unwrap().contains(ids[1]));
    w.lab.kill("host");
    w.spawn_host("host-dependencies-restored");
    wait_for("재연결", T, || w.h.connected("labA").then_some(()));
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, text.replace("- [ ] 둘째", "- [x] 둘째")).unwrap();
    w.h.ticks(1);
    wait_for("둘째 완료 보관", T, || {
        w.h.ok(&["task", "archive"])["entries"].as_array()?.iter().any(|a| a["task"]["title"] == "둘째").then_some(())
    });
    w.h.ticks(1);
    let q = w.h.ok(&["task", "queue"]);
    assert_eq!(q["entries"][0]["reason"], "자리 대기");
    assert!(q["entries"][0]["thread"].is_null());
    w.h.ok(&["thread", "resolve", &independent, "--skip-copy"]);
    let started = wait_for("모든 선행과 자리 충족 뒤 기동", T, || {
        w.h.ok(&["task", "queue"])["entries"][0]["thread"].as_str().map(str::to_string)
    });
    let v = w.wait_show("합류 pane 준비", &started, |v| v["brief"] == "delivered");
    w.lab.kill("host-dependencies-restored");
    w.spawn_host("host-dependencies-started");
    wait_for("기동 뒤 재연결", T, || w.h.connected("labA").then_some(()));
    // 이미 완료된 체크와 tick을 반복해도 새 thread·pane이 생기지 않는다.
    w.h.ticks(2);
    let q = w.h.ok(&["task", "queue"]);
    assert_eq!(q["entries"][0]["thread"], started);
    assert_eq!(w.show(&started)["pane"], v["pane"]);
    assert_eq!(w.h.ok(&["thread", "list"])["threads"].as_array().unwrap().len(), 2);
}

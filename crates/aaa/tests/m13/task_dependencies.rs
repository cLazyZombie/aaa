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
    // 감시 중인 파일을 truncate한 빈 순간에 예약이 「할 일 없음」으로 바뀌지 않도록 제품 편집 경로로 저장한다.
    w.h.ok(&["task", "edit", ids[0], "--done"]);
    w.h.ticks(1);
    wait_for("첫째 완료 보관", T, || {
        w.h.ok(&["task", "archive"])["entries"].as_array()?.iter().any(|a| a["task"]["title"] == "첫째").then_some(())
    });
    assert!(queue()[0]["thread"].is_null());
    assert!(queue()[0]["reason"].as_str().unwrap().contains(ids[1]));
    w.lab.kill("host");
    w.spawn_host("host-dependencies-restored");
    wait_for("재연결", T, || w.h.connected("labA").then_some(()));
    w.h.ok(&["task", "edit", ids[1], "--done"]);
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

// 필요: 실제 CLI --account·--after가 재시작을 넘어 같은 인증으로 agent에 전달되는지 확인한다.
#[test]
fn reserved_account_survives_restart_waits_without_fallback_and_reaches_agent() {
    let mut w = world();
    let deadline = Duration::from_secs(5);
    let value = aaa_test_support::token("queue-account-chosen");
    let auth = w.scripts.join("chosen-auth");
    std::fs::write(&auth, &value).unwrap();
    w.h.ok(&["account", "import", "chosen", "--harness", "claude", "--file", path_str(&auth)]);
    let other_auth = w.scripts.join("other-auth");
    std::fs::write(&other_auth, aaa_test_support::token("queue-account-other")).unwrap();
    w.h.ok(&["account", "import", "other", "--harness", "claude", "--file", path_str(&other_auth)]);
    let proof = w.scripts.join("account-proof");
    // 외부 제공자를 부르지 않는 harness. 인증을 비교하고 성공 여부만 남긴다.
    std::fs::write(w.scripts.join("claude.sh"), format!(
        "#!/bin/sh\nif [ \"$(cat '{}')\" = \"$CLAUDE_CODE_OAUTH_TOKEN\" ]; then echo chosen > '{}'; fi\nexec /bin/sh '{}' \"$@\"\n",
        path_str(&auth), path_str(&proof), path_str(&w.scripts.join("fake.sh"))
    )).unwrap();
    let path = w.projects.join("demo/TASKS.md");
    std::fs::write(&path, "## Backlog\n- [ ] 선행 (claude@labA) [작업 폴더]\n  선행 범위\n- [ ] 지정 계정 (claude@labA) [작업 폴더]\n  noreport\n").unwrap();
    let tasks = w.h.ok(&["task", "list"])["tasks"].as_array().unwrap().clone();
    let first = tasks[0]["short_id"].as_str().unwrap();
    let second = tasks[1]["short_id"].as_str().unwrap();
    w.h.ok(&["task", "reserve", second, "--account", "chosen", "--after", first]);
    w.h.ticks(1);
    let row = w.h.ok(&["task", "queue"])["entries"][0].clone();
    assert_eq!(row["account"], "chosen");
    assert_eq!(row["after"], tasks[0]["id"]);
    assert!(row["thread"].is_null());
    w.lab.kill("host");
    // 인증 금고 키를 잠시 치워 실제 재시작에서 잠긴 계정을 흉내 낸다. 다른 계정이 있어도 시작하면 안 된다.
    std::fs::rename(w.h.state.join("host/secrets.key"), w.h.state.join("host/held.key")).unwrap();
    w.spawn_host("host-pinned-account");
    wait_for("지정 예약 재연결", deadline, || w.h.connected("labA").then_some(()));
    w.h.ok(&["task", "edit", first, "--done"]);
    wait_for("지정 계정 대기", deadline, || {
        w.h.ticks(1);
        let row = w.h.ok(&["task", "queue"])["entries"][0].clone();
        (row["reason"].as_str()?.contains("인증 잠김") && row["state"] == "queued").then_some(())
    });
    assert!(w.h.ok(&["thread", "list"])["threads"].as_array().unwrap().is_empty());
    assert!(!proof.exists());
    std::fs::rename(w.h.state.join("host/held.key"), w.h.state.join("host/secrets.key")).unwrap();
    let tid = wait_for("지정 계정 시작", deadline, || {
        w.h.ticks(1);
        w.h.ok(&["task", "queue"])["entries"][0]["thread"].as_str().map(str::to_string)
    });
    let v = w.wait_show("지정 계정 pane", &tid, |v| v["pane"].is_string());
    assert_eq!(v["account"], "chosen");
    wait_for("agent의 지정 인증 수신", deadline, || {
        std::fs::read_to_string(&proof).ok().filter(|s| s.trim() == "chosen")
    });
    assert_eq!(w.h.ok(&["task", "queue"])["entries"][0]["account"], "chosen");
}

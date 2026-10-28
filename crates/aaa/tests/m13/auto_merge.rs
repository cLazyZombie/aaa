//! 할 일·thread별 자동 병합(plan §8.530): coordinator는 사용자 지시 근거 없이 바꾸지 못하고, 근거를 주면 바꾼 기록에 남는다.
//! 할 일에서 띄운 thread는 그 값을 thread show·context·brief 머리에 보인다. 사용자(세션 밖 CLI)는 근거 없이 바꾸고 coordinator inbox에 남는다.
use super::*;

#[test]
fn coordinator_changes_auto_merge_only_with_user_request() {
    let w = world();
    w.h.ok(&[
        "task",
        "add",
        "자동 일",
        "--profile",
        "fake",
        "--machine",
        "labA",
        "--repo",
        "작업 폴더",
        "--note",
        "noreport",
    ]);
    let merged = |title: &str| {
        w.h.ok(&["task", "list"])["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["title"] == title && t["auto_merge"] == true)
    };
    let coord = w.h.ok(&["project", "open", "demo", "--profile", "fake"]);
    let cpane = coord["pane"]["id"].as_str().unwrap().to_string();
    w.wait_screen(&cpane, "fake>");
    let mut number = 0;
    let mut probe = |command: &str| {
        number += 1;
        let prefix = w.lab.root.join(format!("merge-request-{number}"));
        let script = w.task(
            &format!("merge-request-{number}.sh"),
            &format!(
                "'{}' --json {command} > '{p}.out' 2> '{p}.err'\nprintf '%s' \"$?\" > '{p}.rc'\nprintf 'merge-request-{number}-finished\\n'\n",
                lab::BIN,
                p = prefix.display()
            ),
        );
        w.h.ok(&["pane", "run", &cpane, &format!("post-probe {script}")]);
        w.h.wait_output(&cpane, &format!("merge-request-{number}-finished"), T);
        (
            std::fs::read_to_string(prefix.with_extension("rc")).unwrap(),
            std::fs::read_to_string(prefix.with_extension("err")).unwrap(),
            std::fs::read_to_string(prefix.with_extension("out")).unwrap(),
        )
    };
    // 근거 없는 coordinator 변경은 거부하고 아무것도 남기지 않는다
    let (code, error, _) = probe("task edit '자동 일' --auto-merge on");
    assert_ne!(code, "0");
    assert!(error.contains("--user-request"), "{error}");
    assert!(!merged("자동 일"), "거부한 요청이 값을 바꿨다");
    let reason = "사용자가 채팅에서 자동 일은 자동 병합하라고 지시";
    let (code, error, out) = probe(&format!("task edit '자동 일' --auto-merge on --user-request '{reason}'"));
    assert_eq!(code, "0", "{error}");
    let set: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(
        (set["kind"].as_str(), set["on"].as_bool(), set["changed"].as_bool()),
        (Some("auto_merge"), Some(true), Some(true))
    );
    assert!(merged("자동 일"));
    let log = w.h.ok(&["task", "log", "자동 일"]);
    let entry = log["entries"].as_array().unwrap().iter().find(|e| e["action"] == "auto_merge").unwrap().clone();
    assert!(entry["actor"].as_str().unwrap().contains(&format!("(사용자 지시: {reason})")), "{entry}");
    assert_eq!(entry["after"], "on");
    // coordinator 자신이 바꾼 것은 coordinator inbox에 다시 알리지 않는다
    let inbox = w.h.ok(&["inbox", "list", "demo", "--peek"]);
    assert!(!inbox["items"].as_array().unwrap().iter().any(|i| i["event"] == "auto-merge on"), "{inbox}");

    // 할 일에서 띄운 thread: show·context·brief 머리가 같은 값이다
    let v = w.start(&["--from-task", "자동 일", "--link-task"]);
    let id = v["id"].as_str().unwrap().to_string();
    let t = w.wait_show("brief 받음", &id, |v| v["brief"] == "delivered");
    assert_eq!(t["auto_merge"], true, "{t}");
    let brief = std::fs::read_to_string(PathBuf::from(t["work_dir"].as_str().unwrap()).join("brief.md")).unwrap();
    assert!(brief.contains("- 자동 병합: on(병합 준비되면 사용자에게 묻지 않고 착지 창)"), "{brief}");
    let context = w.h.ok(&["context", "demo", "--peek"]);
    let row = context["threads"].as_array().unwrap().iter().find(|x| x["id"] == id.as_str()).unwrap().clone();
    assert_eq!(row["auto_merge"], true, "{row}");

    // thread로 끄기도 같은 규칙이다
    let (code, error, _) = probe(&format!("thread auto-merge {id} off"));
    assert_ne!(code, "0");
    assert!(error.contains("--user-request"), "{error}");
    let (code, error, _) =
        probe(&format!("thread auto-merge {id} off --user-request '사용자가 이 thread는 직접 승인한다고 지시'"));
    assert_eq!(code, "0", "{error}");
    assert_eq!(w.show(&id)["auto_merge"], Value::Null, "off는 생략한다");
    assert!(!merged("자동 일"));

    // 사용자(세션 밖 CLI)는 근거 없이 바꾸고, coordinator가 다음 턴에 볼 inbox가 남는다
    let back = w.h.ok(&["thread", "auto-merge", &id, "on"]);
    assert_eq!(back["thread"], id.as_str());
    let inbox = w.h.ok(&["inbox", "list", "demo", "--peek"]);
    assert!(inbox["items"].as_array().unwrap().iter().any(|i| i["event"] == "auto-merge on"), "{inbox}");
}

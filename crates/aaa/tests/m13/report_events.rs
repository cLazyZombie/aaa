//! 보고 상태 저장과 완료 사건 발행은 서로 다른 경계다.
use super::*;

/// 기존 flow 제어 소켓으로 발행 순서 잠금을 잡아 실제 수집의 상태 저장→사건 발행 간격을 고정한다.
#[test]
fn report_state_precedes_completion_event() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::{UnixListener, UnixStream};

    fn signal(listener: &tokio::net::UnixListener, rt: &tokio::runtime::Runtime) -> (UnixStream, String) {
        rt.block_on(async {
            tokio::time::timeout(T, async {
                let (stream, _) = listener.accept().await.unwrap();
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                assert!(reader.read_line(&mut line).await.unwrap() > 0, "flow 제어 연결에 표지가 없다");
                (reader.into_inner(), line)
            })
            .await
            .expect("flow 제어 표지가 오지 않았다")
        })
    }

    let mut w = world();
    let mut initial = Events::connect(&w.h);
    let v = w.start(&[
        "--title",
        "보고 사건",
        "--profile",
        "fake",
        "--kind",
        "dir",
        "--task-file",
        &w.task("report.txt", "보고"),
    ]);
    let id = v["id"].as_str().unwrap().to_string();
    flow_event(&mut initial, "report_ready", "");
    flow_event(&mut initial, "brief_delivered", "");
    let before = w.kinds(0).len() as u64;

    // agent와 pane은 살려 둔 채 host만 새 제어 소켓으로 다시 띄운다.
    let control = w.lab.root.join("report-flow.sock");
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let listener = rt.block_on(async { UnixListener::bind(&control).unwrap() });
    w.lab.kill("host");
    w.lab.spawn_env("host2", &w.h, &["host"], &[("AAA_TEST_FLOW_HOLD", path_str(&control))]);
    wait_for("host2 준비", T, || w.h.json(&["status"]).ok());
    wait_for("labA 재연결", T, || w.h.connected("labA").then_some(()));
    let mut events = w.lab.follow("live-flow", &w.h, "demo");
    let routines = w.projects.join("demo/routines");
    std::fs::create_dir_all(&routines).unwrap();
    std::fs::write(routines.join("hold.md"), "+++\non = \"pr\"\nevents = [\"merged\"]\nenabled = false\n+++\n본문\n")
        .unwrap();
    w.lab.spawn("toggle", &w.h, &["routine", "toggle", "demo", "hold", "--on"]);
    let (mut gate, line) = signal(&listener, &rt);
    assert_eq!(line, "Hhold: 켬\n");
    let after = before + 1;
    let work_dir = PathBuf::from(v["work_dir"].as_str().unwrap());
    std::fs::write(work_dir.join("report.md"), "## Report\n갱신\n\n## Next\n- 사용자 확인\n").unwrap();
    let (_, line) = signal(&listener, &rt);
    assert_eq!(line, "W사본 complete\n", "수집 완료 사건이 발행 잠금을 기다린다");
    let v = w.show(&id);
    assert_eq!(v["next"], serde_json::json!(["사용자 확인"]), "{v}");
    let kinds = w.kinds(after);
    assert!(kinds.is_empty(), "완료 사건 전 실제 목록: {kinds:?}");
    // 구판 owner의 상태 조회 직후 단언은 이 지점에서 [] != [report_ready]로 실패한다.
    rt.block_on(gate.write_all(b"R")).unwrap();
    assert!(w.lab.wait_exit("toggle", T).success());
    let ready =
        |e: &Value| e["kind"] == "report_ready" && e["thread"] == id && e["seq"].as_u64().is_some_and(|s| s > after);
    events.until("보고 수집 완료", T, ready);
    let kinds = w.kinds(after);
    assert_eq!(kinds, ["report_ready"], "완료 뒤 실제 목록: {kinds:?}");
    // 구독 전에 발행된 완료도 다시 기다리지 않고 원장에서 찾는다.
    let mut replay = w.lab.follow("replayed-flow", &w.h, "demo");
    replay.until("구독 전 보고 수집 완료", T, ready);
}

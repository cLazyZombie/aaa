//! M1-2 host 기능 E2E: 여러 front의 조작 lease, workspace layout(host 정본), 화면(grid) 구독,
//! 재부팅 뒤 interrupted 표시와 같은 spec으로 재개.
//! 실행: `cargo test -p aa --test m12`

#![cfg(unix)]

mod lab;

use std::time::Duration;

use aa::host::front::FrontClient;
use aa_proto::{B64, ErrorCode, FrontEvent, FrontMsg, FrontRequest, FrontResponse};
use lab::{Lab, Node, wait_for};

const T: Duration = Duration::from_secs(20);

const LOCAL_ONLY: &str = r#"schema = 1
[host]
agent_listen = ""
[web]
listen = ""
[local_agent]
mode = "none"
"#;

fn connected(node: &Node, name: &str) -> bool {
    node.machine(name).is_some_and(|m| m["connection"] == "connected")
}

fn start(lab: &mut Lab) -> Node {
    let h = lab.node("h", Some(LOCAL_ONLY));
    lab.spawn("host", &h, &["host"]);
    wait_for("host 준비", T, || h.json(&["status"]).ok());
    lab.spawn("agent", &h, &["agent", "--host", "local", "--name", "labA"]);
    wait_for("labA 연결", T, || connected(&h, "labA").then_some(()));
    h
}

fn rt() -> tokio::runtime::Runtime {
    // 연결을 놓은 뒤 시험이 동기로 기다리는 동안에도 writer task가 돌아 socket을 닫아야 한다
    tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap()
}

fn input(pane: &str, text: &str) -> FrontRequest {
    FrontRequest::PaneInput { pane: pane.into(), data: B64(format!("{text}\r").into_bytes()) }
}

fn code(r: Result<FrontResponse, aa_proto::ErrorBody>) -> Option<ErrorCode> {
    r.err().map(|e| e.code)
}

/// 두 대화형 front(TUI 두 개를 흉내 낸 Front 연결)가 같은 pane을 조작한다.
#[test]
fn lease_between_fronts() {
    let mut lab = Lab::new();
    let h = start(&mut lab);
    let p = h.ok(&["pane", "run", "--machine", "labA", "--name", "t1", "--", "/bin/sh"]);
    let id = p["id"].as_str().unwrap().to_string();
    rt().block_on(async {
        let mut a = FrontClient::connect_as(&h.state, "tui-a").await.unwrap();
        let mut b = FrontClient::connect_as(&h.state, "tui-b").await.unwrap();
        assert_ne!(a.front, b.front);

        // 비어 있으면 처음 입력한 front가 갖는다
        assert_eq!(code(a.call(input(&id, "echo a-$((1+1))")).await.unwrap()), None);
        assert_eq!(h.pane(&id)["lease"]["front"], a.front.as_str());
        assert_eq!(h.pane(&id)["lease"]["label"], "tui-a");

        // 다른 front의 입력·크기 바꾸기는 거부된다
        assert_eq!(code(b.call(input(&id, "echo b")).await.unwrap()), Some(ErrorCode::LeaseHeld));
        let resize = FrontRequest::PaneResize { pane: id.clone(), cols: 90, rows: 20 };
        assert_eq!(code(b.call(resize).await.unwrap()), Some(ErrorCode::LeaseHeld));
        // 한 번 보내는 CLI 크기 바꾸기도 거부, 한 번 보내는 입력(API)은 lease 대상이 아니다
        let e = h.json(&["pane", "resize", &id, "90", "20"]).expect_err("lease 중 CLI resize");
        assert!(e.contains("lease_held"), "{e}");
        h.ok(&["pane", "send", &id, "--text", "echo api-$((2+2))", "--enter"]);

        // 가져오기: 이제 b가 갖고 a는 거부된다
        assert_eq!(code(b.call(FrontRequest::PaneTake { pane: id.clone() }).await.unwrap()), None);
        assert_eq!(h.pane(&id)["lease"]["front"], b.front.as_str());
        assert_eq!(code(a.call(input(&id, "echo a2")).await.unwrap()), Some(ErrorCode::LeaseHeld));
        let resize = FrontRequest::PaneResize { pane: id.clone(), cols: 90, rows: 20 };
        assert_eq!(code(b.call(resize).await.unwrap()), None);

        // b 연결이 끝나면 lease가 풀리고 a가 다시 입력한다
        drop(b);
        wait_for("lease 해제", T, || h.pane(&id)["lease"].is_null().then_some(()));
        assert_eq!(code(a.call(input(&id, "echo a-$((3+3))")).await.unwrap()), None);
    });
    wait_for("출력", T, || {
        let s = h.screen(&id);
        (s.contains("a-2") && s.contains("api-4") && s.contains("a-6")).then_some(())
    });
    assert_eq!((h.pane(&id)["cols"].as_u64(), h.pane(&id)["rows"].as_u64()), (Some(90), Some(20)));
}

/// workspace layout은 host가 저장하고(재시작 뒤에도 같다), 바뀌면 연결된 front에 알린다.
#[test]
fn layout_is_host_state_and_survives_restart() {
    let mut lab = Lab::new();
    let h = start(&mut lab);
    let w = h.ok(&["ws", "tab", "--machine", "labA", "--name", "first", "--", "/bin/sh"]);
    let first = w["tabs"][0]["root"]["pane"].as_str().unwrap().to_string();
    let rt = rt();
    let mut watcher = rt.block_on(FrontClient::connect_as(&h.state, "watcher")).unwrap();

    let w = h.ok(&["ws", "split", &first, "--dir", "h", "--", "/bin/sh"]);
    let second = w["tabs"][0]["root"]["b"]["pane"].as_str().unwrap().to_string();
    assert_eq!(w["tabs"][0]["root"]["dir"], "horizontal");
    let w = h.ok(&["ws", "split", &second, "--dir", "v"]);
    assert_eq!(w["tabs"][0]["root"]["b"]["dir"], "vertical");
    let third = w["tabs"][0]["root"]["b"]["b"]["pane"].as_str().unwrap().to_string();

    // 연결된 front는 바뀐 layout을 사건으로 받는다
    rt.block_on(async {
        let mut seen = 0;
        while seen < 2 {
            match tokio::time::timeout(T, watcher.next()).await.expect("layout 사건").unwrap() {
                Some(FrontMsg::Event(FrontEvent::Workspace(w))) => {
                    assert_eq!(w.name, "main");
                    seen += 1;
                }
                Some(_) => {}
                None => panic!("연결이 끊겼다"),
            }
        }
    });

    // 하나를 빼면 남은 쪽이 자리를 차지한다(세션도 끝낸다)
    let w = h.ok(&["ws", "close", &second, "--kill"]);
    assert_eq!(w["tabs"][0]["root"]["b"]["pane"], third.as_str());
    wait_for("kill 반영", T, || (h.pane(&second)["state"] == "exited").then_some(()));
    let before = h.ok(&["ws", "show"]);

    lab.kill("host");
    lab.spawn("host2", &h, &["host"]);
    wait_for("재시작 뒤 labA", Duration::from_secs(40), || connected(&h, "labA").then_some(()));
    assert_eq!(h.ok(&["ws", "show"]), before, "재시작 뒤 layout이 같아야 한다");

    // 없는 pane을 나누면 거부
    let e = h.json(&["ws", "split", "ZZZZZZZZ"]).expect_err("없는 pane");
    assert!(e.contains("not_found"), "{e}");
}

/// TUI용 화면 구독: 처음에는 전체, 그다음에는 바뀐 줄만 온다.
#[test]
fn grid_watch_sends_full_then_changed_rows() {
    let mut lab = Lab::new();
    let h = start(&mut lab);
    let p = h.ok(&["pane", "run", "--machine", "labA", "--cols", "40", "--rows", "6", "--", "/bin/sh"]);
    let id = p["id"].as_str().unwrap().to_string();
    h.ok(&["pane", "send", &id, "--text", "echo first-$((1+1))", "--enter"]);
    wait_for("첫 출력", T, || h.screen(&id).contains("first-2").then_some(()));
    rt().block_on(async {
        let mut c = FrontClient::connect_as(&h.state, "grid").await.unwrap();
        assert_eq!(code(c.call(FrontRequest::PaneWatch { pane: id.clone() }).await.unwrap()), None);
        let text = |f: &aa_proto::GridFrame| -> String {
            f.lines
                .iter()
                .map(|l| l.runs.iter().map(|r| r.t.as_str()).collect::<String>())
                .collect::<Vec<_>>()
                .join("\n")
        };
        let mut next_grid = async || loop {
            match tokio::time::timeout(T, c.next()).await.expect("grid frame").unwrap() {
                Some(FrontMsg::Event(FrontEvent::Grid(f))) => return f,
                Some(_) => continue,
                None => panic!("연결이 끊겼다"),
            }
        };
        let full = next_grid().await;
        assert!(full.full && full.cols == 40 && full.rows == 6, "{full:?}");
        assert_eq!(full.lines.len(), 6);
        assert!(text(&full).contains("first-2"));
        // 다른 경로(API)로 입력하면 바뀐 줄만 온다
        h.ok(&["pane", "send", &id, "--text", "echo second-$((2+2))", "--enter"]);
        let mut got = String::new();
        while !got.contains("second-4") {
            let f = next_grid().await;
            assert!(!f.full, "바뀐 줄만 와야 한다");
            assert!(f.lines.len() < 6, "모든 줄이 왔다: {f:?}");
            got.push_str(&text(&f));
        }
        assert!(!got.contains("first-2"), "바뀌지 않은 줄이 다시 왔다: {got}");
    });
}

/// 재부팅 흉내(session host가 사라지고 agent가 다른 boot ID로 시작) → interrupted → 같은 spec으로 재개.
#[test]
fn reboot_marks_interrupted_and_resume_replaces_layout_slot() {
    let mut lab = Lab::new();
    let h = start(&mut lab);
    let w = h.ok(&["ws", "tab", "--machine", "labA", "--name", "job", "--", "/bin/sh"]);
    let old = w["tabs"][0]["root"]["pane"].as_str().unwrap().to_string();
    h.ok(&["pane", "send", &old, "--text", "echo before-reboot", "--enter"]);
    wait_for("출력", T, || h.screen(&old).contains("before-reboot").then_some(()));

    lab.kill("agent");
    assert_eq!(lab.kill_session_hosts(), 1);
    lab.spawn_env("agent2", &h, &["agent", "--host", "local", "--name", "labA"], &[("AA_TEST_BOOT_ID", "boot-after")]);
    wait_for("interrupted 표시", T, || {
        let p = h.pane(&old);
        (p["state"] == "interrupted" && p["connection"] == "connected").then_some(())
    });
    assert!(h.screen(&old).contains("before-reboot"), "중단된 pane도 마지막 화면을 읽는다");
    let e = h.json(&["pane", "send", &old, "--text", "x"]).expect_err("중단된 pane 입력");
    assert!(e.contains("session_exited"), "{e}");

    let p = h.ok(&["pane", "resume", &old]);
    let new = p["id"].as_str().unwrap().to_string();
    assert_ne!(new, old);
    assert_eq!((p["name"].as_str(), p["state"].as_str()), (Some("job"), Some("running")));
    assert_eq!(p["argv"], serde_json::json!(["/bin/sh"]));
    let w = h.ok(&["ws", "show"]);
    assert_eq!(w["tabs"][0]["root"]["pane"], new.as_str(), "layout 자리가 새 pane으로 바뀐다");
    let panes = h.ok(&["pane", "list"]);
    assert!(!panes.to_string().contains(&old), "옛 기록은 지워진다");
    h.ok(&["pane", "send", &new, "--text", "echo resumed-$((5*5))", "--enter"]);
    wait_for("재개 뒤 입력", T, || h.screen(&new).contains("resumed-25").then_some(()));
}

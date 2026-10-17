//! M1-2 host 기능 E2E: 여러 front의 조작 lease, workspace layout(host 정본), 화면(grid) 구독,
//! 재부팅 뒤 interrupted 표시와 같은 spec으로 재개.
//! 실행: `cargo test -p aaa --test m12`

#![cfg(unix)]

mod lab;

use std::time::Duration;

use aaa_common::front::FrontClient;
use aaa_proto::{B64, ErrorCode, FrontEvent, FrontMsg, FrontRequest, FrontResponse};
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

fn start(lab: &mut Lab) -> Node {
    let h = lab.node("h", Some(LOCAL_ONLY));
    lab.spawn("host", &h, &["host"]);
    wait_for("host 준비", T, || h.json(&["status"]).ok());
    lab.spawn("agent", &h, &["agent", "--host", "local", "--name", "labA"]);
    wait_for("labA 연결", T, || h.connected("labA").then_some(()));
    h
}

fn rt() -> tokio::runtime::Runtime {
    // 연결을 놓은 뒤 시험이 동기로 기다리는 동안에도 writer task가 돌아 socket을 닫아야 한다
    tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap()
}

fn input(pane: &str, text: &str) -> FrontRequest {
    FrontRequest::PaneInput { pane: pane.into(), data: B64(format!("{text}\r").into_bytes()) }
}

fn code(r: Result<FrontResponse, aaa_proto::ErrorBody>) -> Option<ErrorCode> {
    r.err().map(|e| e.code)
}

/// 두 대화형 front(TUI 두 개를 흉내 낸 Front 연결)가 같은 pane을 쓴다(§8.33, herdr와 같은 모델).
/// 거부는 없다: 입력·크기를 보낸 front가 「지금 쓰는 화면」(lease)이 되고, 마지막 입력이 이긴다.
#[test]
fn lease_between_fronts() {
    let mut lab = Lab::new();
    let h = start(&mut lab);
    let id = h.new_pane(None, "labA", Some("t1"), &["/bin/sh"], None);
    rt().block_on(async {
        let mut a = FrontClient::connect_as(&h.state, "tui-a").await.unwrap();
        let mut b = FrontClient::connect_as(&h.state, "tui-b").await.unwrap();
        assert_ne!(a.front, b.front);

        // 처음 입력한 front가 지금 쓰는 화면이 된다
        assert_eq!(code(a.call(input(&id, "echo a-$((1+1))")).await.unwrap()), None);
        assert_eq!(h.pane(&id)["lease"]["front"], a.front.as_str());
        assert_eq!(h.pane(&id)["lease"]["label"], "tui-a");

        // 다른 front도 바로 입력된다(가져오기 없이). 그 front가 지금 쓰는 화면이 된다
        assert_eq!(code(b.call(input(&id, "echo b-$((4+4))")).await.unwrap()), None);
        assert_eq!(h.pane(&id)["lease"]["front"], b.front.as_str());
        // 한 번 보내는 CLI 크기 바꾸기는 누가 쓰는 동안 거부, 한 번 보내는 입력(API)은 대상이 아니다
        let e = h.json(&["pane", "pty-size", &id, "90", "20"]).expect_err("쓰는 중 CLI resize");
        assert!(e.contains("lease_held"), "{e}");
        h.ok(&["pane", "run", &id, "echo api-$((2+2))"]);
        assert_eq!(h.pane(&id)["lease"]["front"], b.front.as_str());

        // 크기를 건 front도 지금 쓰는 화면이 된다(focus를 얻은 화면이 크기를 다시 건다)
        let resize = FrontRequest::PaneResize { pane: id.clone(), cols: 90, rows: 20 };
        assert_eq!(code(a.call(resize).await.unwrap()), None);
        assert_eq!(h.pane(&id)["lease"]["front"], a.front.as_str());
        // 놓기(가려진 웹 탭): 자기 것만 놓는다
        assert_eq!(code(b.call(FrontRequest::PaneRelease { pane: id.clone() }).await.unwrap()), None);
        assert_eq!(h.pane(&id)["lease"]["front"], a.front.as_str());
        assert_eq!(code(a.call(FrontRequest::PaneRelease { pane: id.clone() }).await.unwrap()), None);
        assert!(h.pane(&id)["lease"].is_null());

        // 연결이 끝나면 그 front가 쓰던 것도 풀린다
        assert_eq!(code(b.call(input(&id, "echo b2")).await.unwrap()), None);
        assert_eq!(h.pane(&id)["lease"]["front"], b.front.as_str());
        drop(b);
        wait_for("lease 해제", T, || h.pane(&id)["lease"].is_null().then_some(()));
        assert_eq!(code(a.call(input(&id, "echo a-$((3+3))")).await.unwrap()), None);
    });
    for want in ["a-2", "b-8", "api-4", "a-6"] {
        h.wait_output(&id, want, T);
    }
    assert_eq!((h.pane(&id)["cols"].as_u64(), h.pane(&id)["rows"].as_u64()), (Some(90), Some(20)));
}

/// workspace layout은 host가 저장하고(재시작 뒤에도 같다), 바뀌면 연결된 front에 알린다.
#[test]
fn layout_is_host_state_and_survives_restart() {
    let mut lab = Lab::new();
    let h = start(&mut lab);
    let w = h.ok(&["tab", "create", "--machine", "labA", "--label", "first", "--", "/bin/sh"]);
    let first = w["tabs"][0]["root"]["pane"].as_str().unwrap().to_string();
    let rt = rt();
    let mut watcher = rt.block_on(FrontClient::connect_as(&h.state, "watcher")).unwrap();

    let w = h.ok(&["pane", "split", &first, "--direction", "right", "--", "/bin/sh"]);
    let second = w["tabs"][0]["root"]["b"]["pane"].as_str().unwrap().to_string();
    assert_eq!(w["tabs"][0]["root"]["dir"], "horizontal");
    let w = h.ok(&["pane", "split", &second, "--direction", "down"]);
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

    // 닫으면(herdr `pane close`) 세션이 끝나고 기록이 지워지며, 남은 쪽이 자리를 차지한다
    h.ok(&["pane", "close", &second]);
    let w = h.ok(&["workspace", "get"]);
    assert_eq!(w["tabs"][0]["root"]["b"]["pane"], third.as_str());
    assert!(!h.ok(&["pane", "list"]).to_string().contains(&second), "닫은 pane 기록은 지워진다");
    let before = h.ok(&["workspace", "get"]);

    lab.kill("host");
    lab.spawn("host2", &h, &["host"]);
    wait_for("재시작 뒤 labA", Duration::from_secs(40), || h.connected("labA").then_some(()));
    assert_eq!(h.ok(&["workspace", "get"]), before, "재시작 뒤 layout이 같아야 한다");

    // 없는 pane을 나누면 거부
    let e = h.json(&["pane", "split", "ZZZZZZZZ", "--direction", "right"]).expect_err("없는 pane");
    assert!(e.contains("not_found"), "{e}");
}

/// TUI용 화면 구독: 처음에는 전체, 그다음에는 바뀐 줄만 온다.
#[test]
fn grid_watch_sends_full_then_changed_rows() {
    let mut lab = Lab::new();
    let h = start(&mut lab);
    let id = h.new_pane(None, "labA", None, &["/bin/sh"], None);
    h.ok(&["pane", "pty-size", &id, "40", "6"]);
    h.ok(&["pane", "run", &id, "echo first-$((1+1))"]);
    h.wait_output(&id, "first-2", T);
    rt().block_on(async {
        let mut c = FrontClient::connect_as(&h.state, "grid").await.unwrap();
        assert_eq!(code(c.call(FrontRequest::PaneWatch { pane: id.clone() }).await.unwrap()), None);
        let text = |f: &aaa_proto::GridFrame| -> String {
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
        h.ok(&["pane", "run", &id, "echo second-$((2+2))"]);
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
    let old = h.new_pane(None, "labA", Some("job"), &["/bin/sh"], None);
    h.ok(&["pane", "run", &old, "echo before-reboot"]);
    h.wait_output(&old, "before-reboot", T);

    lab.kill("agent");
    assert_eq!(lab.kill_session_hosts(), 1);
    lab.spawn_env("agent2", &h, &["agent", "--host", "local", "--name", "labA"], &[("AAA_TEST_BOOT_ID", "boot-after")]);
    wait_for("interrupted 표시", T, || {
        let p = h.pane(&old);
        (p["state"] == "interrupted" && p["connection"] == "connected").then_some(())
    });
    assert!(h.screen(&old).contains("before-reboot"), "중단된 pane도 마지막 화면을 읽는다");
    let e = h.json(&["pane", "send-text", &old, "x"]).expect_err("중단된 pane 입력");
    assert!(e.contains("session_exited"), "{e}");

    let p = h.ok(&["pane", "resume", &old]);
    let new = p["id"].as_str().unwrap().to_string();
    assert_ne!(new, old);
    assert_eq!((p["name"].as_str(), p["state"].as_str()), (Some("job"), Some("running")));
    assert_eq!(p["argv"], serde_json::json!(["/bin/sh"]));
    let w = h.ok(&["workspace", "get"]);
    assert_eq!(w["tabs"][0]["root"]["pane"], new.as_str(), "layout 자리가 새 pane으로 바뀐다");
    let panes = h.ok(&["pane", "list"]);
    assert!(!panes.to_string().contains(&old), "옛 기록은 지워진다");
    h.ok(&["pane", "run", &new, "echo resumed-$((5*5))"]);
    h.wait_output(&new, "resumed-25", T);
}

/// pane 이름 바꾸기(herdr `pane rename <pane> <label>|--clear`, plan §8.106). 이름의 정본은 agent `spec.json`의 label이다:
/// 띄울 때 이름을 모르는 session host가 보내는 정보(크기 바뀜)·agent 재시작·세션 끝·재개를 지나도 남고, 지우면 띄울 때 이름으로 돌아간다.
#[test]
fn pane_rename_survives_agent_restart_exit_and_resume() {
    let mut lab = Lab::new();
    let h = start(&mut lab);
    let id = h.new_pane(None, "labA", Some("job"), &["/bin/sh"], None);
    let p = h.ok(&["pane", "rename", &id, " 빌드", "창 "]);
    assert_eq!(
        (p["name"].as_str(), p["label"].as_str()),
        (Some("빌드 창"), Some("빌드 창")),
        "앞뒤 공백을 떼고 낱말을 띄어 잇는다"
    );
    assert_eq!(h.pane("빌드 창")["id"], id.as_str(), "새 이름으로 찾는다");
    // session host는 띄울 때 이름만 안다: 그가 보낸 크기 바뀜에도 붙인 이름이 남는다
    h.ok(&["pane", "pty-size", &id, "100", "30"]);
    let p = h.pane(&id);
    assert_eq!((p["cols"].as_u64(), p["name"].as_str()), (Some(100), Some("빌드 창")));
    // agent만 다시 띄운다(session host는 살아 있다): spec.json에서 이름을 읽어 다시 붙은 정보에 덮는다
    lab.kill("agent");
    wait_for("끊김", T, || (h.pane(&id)["connection"] == "disconnected").then_some(()));
    lab.spawn("agent2", &h, &["agent", "--host", "local", "--name", "labA"]);
    wait_for("다시 붙음", T, || (h.pane(&id)["connection"] == "connected").then_some(()));
    assert_eq!(h.pane(&id)["name"], "빌드 창");
    // 지우면 띄울 때 이름
    let p = h.ok(&["pane", "rename", &id, "--clear"]);
    assert_eq!(p["name"], "job");
    assert!(p.get("label").is_none(), "{p}");
    let e = h.json(&["pane", "rename", &id, &"가".repeat(aaa_proto::MAX_PANE_LABEL_CHARS + 1)]).expect_err("긴 이름");
    assert!(e.contains("bad_request"), "{e}");
    // 끝난 pane에도 붙고, 재개한 pane이 그 이름을 이어받는다
    h.ok(&["pane", "rename", &id, "끝난 뒤"]);
    h.ok(&["pane", "kill", &id]);
    wait_for("끝남", T, || (h.pane(&id)["state"] == "exited").then_some(()));
    assert_eq!(h.pane(&id)["name"], "끝난 뒤", "세션이 끝나도 붙인 이름");
    let p = h.ok(&["pane", "resume", &id]);
    assert_eq!((p["name"].as_str(), p["state"].as_str()), (Some("끝난 뒤"), Some("running")));
}

/// 한 front 연결에서 응답을 기다리지 않고 보낸 키 입력은 보낸 순서대로 PTY에 들어간다(TUI가 키마다 보낸다).
#[test]
fn inputs_from_one_front_keep_order() {
    let mut lab = Lab::new();
    let h = start(&mut lab);
    let id = h.new_pane(None, "labA", None, &["/bin/sh"], None);
    let line = "echo ordered-abcdefghijklmnopqrstuvwxyz-0123456789-$((40+2))\r";
    rt().block_on(async {
        let c = FrontClient::connect_as(&h.state, "keys").await.unwrap();
        let (mut tx, mut rx) = c.split();
        let mut ids = Vec::new();
        for b in line.bytes() {
            ids.push(tx.send(FrontRequest::PaneInput { pane: id.clone(), data: B64(vec![b]) }).await.unwrap());
        }
        // 응답이 모두 올 때까지 기다린다(모두 성공해야 한다)
        let mut left: std::collections::HashSet<String> = ids.into_iter().collect();
        while !left.is_empty() {
            if let FrontMsg::Res { id, result } = tokio::time::timeout(T, rx.recv()).await.expect("응답").unwrap() {
                assert!(matches!(result, aaa_proto::Outcome::Ok(_)), "{result:?}");
                left.remove(&id);
            }
        }
    });
    h.wait_output(&id, "ordered-abcdefghijklmnopqrstuvwxyz-0123456789-42", T);
}

/// 받은 바이트를 16진수로 보이는 프로그램. bracketed paste와 DECCKM(커서 키 SS3)을 켜고, 받은 조각들을 이어 쓰다가
/// `.`(2e)를 받으면 줄을 끝낸다. 시험은 동작 뒤에 `send-text .`을 보내 그 동작이 쓴 바이트를 한 줄로 본다(조각 나뉨과 무관).
const HEX_ECHO: &str = r#"printf '\033[?2004h\033[?1h'
stty raw -echo
printf 'ready\r\nGOT:'
while c=$(dd bs=4096 count=1 2>/dev/null | od -An -tx1 | tr -d ' \n') && [ -n "$c" ]; do
  printf '%s' "$c"
  case $c in *2e) printf '\r\nGOT:' ;; esac
done
"#;

/// 4장 F: herdr 0.9.5의 pane 입력·읽기·기다리기 명령(src/cli/pane.rs, src/app/api_helpers.rs).
/// `pane run`은 글+Enter를 한 번에 쓰고 프로그램이 bracketed paste를 켰으면 글을 감싼다. `send-text`는 그대로,
/// `send-keys`는 키마다 터미널 모드(DECCKM)에 맞춰 쓰고 모르는 키가 있으면 아무것도 보내지 않는다.
/// 오류는 stderr JSON·종료 1, 사용법 오류는 종료 2다. 기다림은 모두 출력 사건(`pane wait-output`)으로 끝난다.
#[test]
fn herdr_pane_input_read_and_wait() {
    let mut lab = Lab::new();
    let h = start(&mut lab);
    let script = lab.root.join("hex.sh");
    std::fs::write(&script, HEX_ECHO).unwrap();
    let id = h.new_pane(None, "labA", Some("hex"), &["/bin/sh", lab::path_str(&script)], None);
    h.wait_output(&id, "ready", T);
    let got = |line: &str| {
        h.ok(&["pane", "send-text", &id, "."]);
        h.wait_output(&id, &format!("GOT:{line}2e"), T);
    };

    // pane run: ESC[200~a b ESC[201~ CR 한 번에
    h.ok(&["pane", "run", &id, "a", "b"]);
    got("1b5b3230307e6120621b5b3230317e0d");
    // send-text: 감싸지 않는다(이름으로도 부른다)
    h.ok(&["pane", "send-text", "hex", "x"]);
    got("78");
    // send-keys: DECCKM이 켜져 있으니 위 화살표는 SS3 A, ctrl+c는 0x03
    h.ok(&["pane", "send-keys", &id, "up", "ctrl+c", "enter"]);
    got("1b4f41030d");
    // 모르는 키가 하나라도 있으면 아무것도 보내지 않는다(앞의 enter도)
    let e = h.json(&["pane", "send-keys", &id, "enter", "nope"]).expect_err("모르는 키");
    assert!(e.contains("bad_request") && e.contains("nope"), "{e}");
    got("");

    // read: 기본은 글 그대로(JSON 아님), --json이면 ReadView
    let o = h.run(&["pane", "read", &id, "--source", "visible"]);
    assert!(o.status.success());
    let text = String::from_utf8_lossy(&o.stdout);
    assert!(text.starts_with("ready\n") && text.contains("GOT:782e\n"), "{text}");
    let v = h.ok(&["pane", "read", &id, "--lines", "2"]);
    assert_eq!(
        (v["kind"].as_str(), v["source"].as_str(), v["format"].as_str()),
        (Some("read"), Some("recent"), Some("text"))
    );
    assert_eq!(v["text"], "GOT:2e\nGOT:\n", "{v}");

    // wait-output: 정규식, 맞은 줄과 읽은 글을 준다
    let m = h.ok(&["pane", "wait-output", &id, "--regex", "^GOT:78", "--timeout", "5000"]);
    assert_eq!((m["kind"].as_str(), m["matched_line"].as_str()), (Some("output_matched"), Some("GOT:782e")), "{m}");
    // 시한(설정 값)이 지나면 timeout 오류
    let e = h.json(&["pane", "wait-output", &id, "--match", "never-printed", "--timeout", "200"]).expect_err("시한");
    assert!(e.contains("timeout"), "{e}");

    // 오류는 stderr의 JSON 한 줄, 종료 1(herdr). 표준 출력은 비어 있다
    let o = h.run(&["pane", "get", "NOPE"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(o.stdout.is_empty());
    let err: serde_json::Value = serde_json::from_slice(&o.stderr).expect("stderr JSON");
    assert_eq!(err["error"]["code"], "not_found", "{err}");
    // 사용법 오류는 종료 2(herdr usage)
    assert_eq!(h.run(&["pane", "send-keys", &id]).status.code(), Some(2));
}

/// 4장 F: herdr 0.9.5의 tab·workspace·나누기·크기 명령(src/cli/tab.rs·workspace.rs, src/layout.rs `resize_focused`)과 셸 완성.
#[test]
fn herdr_tab_workspace_and_layout() {
    let mut lab = Lab::new();
    let h = start(&mut lab);
    let first = h.new_pane(None, "labA", Some("one"), &["/bin/sh"], None);
    // split --ratio: 원래 pane 쪽 비율
    let w = h.ok(&["pane", "split", &first, "--direction", "right", "--ratio", "0.3"]);
    let root = &w["tabs"][0]["root"];
    assert_eq!((root["dir"].as_str(), root["ratio"].as_u64()), (Some("horizontal"), Some(300)), "{w}");
    let second = root["b"]["pane"].as_str().unwrap().to_string();
    // resize: 오른쪽이면 그 가장자리 나눔이 +0.05, 왼쪽 끝까지 가면 0.1에서 멈춘다
    let w = h.ok(&["pane", "resize", "--pane", &first, "--direction", "right"]);
    assert_eq!(w["tabs"][0]["root"]["ratio"], 350);
    let w = h.ok(&["pane", "resize", "--pane", &second, "--direction", "left", "--amount", "0.5"]);
    assert_eq!(w["tabs"][0]["root"]["ratio"], 100);

    // move --new-tab: 나눈 자리에서 빠져 새 탭으로
    let w = h.ok(&["pane", "move", &second, "--new-tab", "--label", "two"]);
    assert_eq!(w["tabs"][0]["root"]["pane"], first.as_str());
    assert_eq!(
        (w["tabs"][1]["name"].as_str(), w["tabs"][1]["root"]["pane"].as_str()),
        (Some("two"), Some(second.as_str()))
    );
    let tab = w["tabs"][1]["id"].as_str().unwrap().to_string();
    assert_eq!(h.ok(&["tab", "list"])["tabs"].as_array().map(|t| t.len()), Some(2));
    h.ok(&["tab", "rename", &tab, "renamed"]);
    let t = h.ok(&["tab", "get", &tab]);
    assert_eq!(
        (t["kind"].as_str(), t["workspace"].as_str(), t["tab"]["name"].as_str()),
        (Some("tab"), Some("main"), Some("renamed"))
    );
    // tab close: 그 안 pane이 끝나고 기록에서도 사라진다(herdr)
    let w = h.ok(&["tab", "close", &tab]);
    assert_eq!(w["tabs"].as_array().map(|t| t.len()), Some(1));
    assert!(!h.ok(&["pane", "list"]).to_string().contains(&second), "닫은 탭의 pane 기록은 지워진다");

    // workspace: 다른 이름의 layout
    let side = h.new_pane(Some("side"), "labA", None, &["/bin/sh"], None);
    let names = |v: serde_json::Value| -> Vec<(String, u64)> {
        v["workspaces"]
            .as_array()
            .unwrap()
            .iter()
            .map(|w| (w["name"].as_str().unwrap().into(), w["tabs"].as_u64().unwrap()))
            .collect()
    };
    assert_eq!(names(h.ok(&["workspace", "list"])), vec![("main".into(), 1), ("side".into(), 1)]);
    let e = h.json(&["workspace", "rename", "side", "main"]).expect_err("겹치는 이름");
    assert!(e.contains("name_taken"), "{e}");
    h.ok(&["workspace", "rename", "side", "side2"]);
    assert_eq!(h.ok(&["workspace", "get", "side2"])["tabs"][0]["root"]["pane"], side.as_str());
    let v = h.ok(&["workspace", "close", "side2"]);
    assert_eq!(names(v), vec![("main".into(), 1)]);
    assert!(!h.ok(&["pane", "list"]).to_string().contains(&side), "닫은 workspace의 pane 기록은 지워진다");
    assert_eq!(h.pane(&first)["state"], "running", "다른 workspace의 pane은 그대로");

    // 셸 완성(herdr `completion`): zsh는 `--opt 값`(빈칸) 꼴로 완성한다
    let o = h.run(&["completion", "zsh"]);
    let zsh = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success() && zsh.starts_with("#compdef aaa"), "{zsh}");
    assert!(zsh.contains("'--direction[") && !zsh.contains("'--direction=["), "빈칸 꼴이 아니다");
    let o = h.run(&["completion", "bash"]);
    assert!(o.status.success() && String::from_utf8_lossy(&o.stdout).contains("complete -F"));
}

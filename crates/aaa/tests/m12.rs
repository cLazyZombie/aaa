//! M1-2 host 기능 E2E: 여러 front의 조작 lease, workspace layout(host 정본), 화면(grid) 구독,
//! 재부팅 뒤 복원(셸 pane·대화 이어가기, 프로그램 pane은 중단으로 두고 같은 spec으로 재개).
//! 실행: `cargo test -p aaa --test m12`

#![cfg(unix)]

mod lab;

use std::time::Duration;

use aaa_common::front::FrontClient;
use aaa_proto::{B64, ErrorCode, FrontEvent, FrontMsg, FrontRequest, FrontResponse, SessionState};
use lab::watch::HostWatch;
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

/// 가짜 claude: 받은 인자를 보이고, SessionStart hook으로 harness 세션 ID(`--resume`·`--session-id` 뒤, 없으면
/// `$FAKE_SID`)를 알린 뒤 입력을 기다린다.
const FAKE_CLAUDE: &str = r#"#!/bin/sh
echo "fake-claude $*"
sid=$FAKE_SID
case "$1" in --resume|--session-id) sid=$2 ;; esac
[ -n "$sid" ] && "$AA" hook --agent claude "{\"hook_event_name\":\"SessionStart\",\"session_id\":\"$sid\",\"source\":\"startup\"}"
# `--ask`: 확인창을 띄운 채 기다린다(화면 감지가 질문으로 본다)
[ "$3" = --ask ] && printf '%s\n' ' Bash command' '' '   make test' '' ' Do you want to proceed?' ' \342\235\257 1. Yes' '   2. No' '' ' Esc to cancel'
exec cat
"#;

/// 시험 대화 ID(harness 세션 ID는 UUID 꼴만 받는다, `aaa_common::util::harness_session_ok`)
const SID_A: &str = "0f5b2c1e-7a3d-4c8e-9b21-5d6e7f809a1b";
const SID_C: &str = "7d0c9b8a-6f5e-4d3c-8b2a-1f0e9d8c7b6a";

/// 재부팅 흉내 준비: 가짜 claude를 PATH 앞에 둔 agent 환경
fn fake_claude_env(lab: &Lab) -> (String, Vec<(String, String)>) {
    let bin = lab.root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let claude = bin.join("claude");
    std::fs::write(&claude, FAKE_CLAUDE).unwrap();
    std::fs::set_permissions(&claude, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default());
    (claude.display().to_string(), vec![("PATH".into(), path), ("AA".into(), lab::BIN.into())])
}

/// 재부팅 흉내: agent와 session host를 끝내고 agent를 다른 boot ID로 다시 띄운다
fn reboot(lab: &mut Lab, h: &Node, env: &[(String, String)], hosts: usize) {
    lab.kill("agent");
    assert_eq!(lab.kill_session_hosts(), hosts);
    let mut e: Vec<(&str, &str)> = env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    e.push(("AAA_TEST_BOOT_ID", "boot-after"));
    lab.spawn_env("agent2", h, &["agent", "--host", "local", "--name", "labA"], &e);
}

/// host와 agent(labA, `env`를 더함)를 띄우고 사건을 볼 연결을 준다
fn start_watch(lab: &mut Lab, host_toml: &str, env: &[(String, String)]) -> (Node, HostWatch) {
    let h = lab.node("h", Some(host_toml));
    lab.spawn("host", &h, &["host"]);
    wait_for("host 준비", T, || h.json(&["status"]).ok());
    let mut hw = HostWatch::new(&h, T);
    let e: Vec<(&str, &str)> = env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    lab.spawn_env("agent", &h, &["agent", "--host", "local", "--name", "labA"], &e);
    hw.wait("labA 연결", |w| w.connected("labA").then_some(()));
    (h, hw)
}

/// 이 pane이 harness 세션 ID를 알렸나
fn has_session(w: &HostWatch, id: &str) -> bool {
    w.pane(id).and_then(|p| p.agent.as_ref()).is_some_and(|a| a.harness_session.is_some())
}

/// 4장 A 재부팅 뒤 복원(herdr src/persist/restore.rs·src/app/agent_resume.rs `start_pending_agent_resume`). session host가
/// 사라지고 agent가 다른 boot ID로 다시 붙으면 host가 셸 pane을 저장한 cwd에서 새로 띄워 layout 자리를 바꾸고, 셸에서
/// 돌던 Claude는 `claude --resume <id>`를 쳐서 잇는다. 같은 대화를 가리키는 두 번째 셸은 빈 셸로 되살린다(herdr
/// `dedupe_key`). Claude를 바로 띄운 pane은 그 명령으로 띄운다. 다른 프로그램 pane은 다시 실행하지 않아 중단으로 남고
/// `pane resume`이 같은 spec으로 그 자리에 띄운다. 저장한 폴더가 없어진 셸은 중단으로 남고 까닭을 보인다(herdr
/// `restore_error`). 사람이 붙인 pane 이름은 다시 띄운 pane이 잇는다. 기다림은 host 사건(layout·pane 상태)과 출력뿐이다.
#[test]
fn reboot_restores_shells_and_resumes_agents() {
    let mut lab = Lab::new();
    let (claude, env) = fake_claude_env(&lab);
    let (h, mut hw) = start_watch(&mut lab, LOCAL_ONLY, &env);
    let wa = lab.root.join("wa");
    let gone = lab.root.join("gone");
    std::fs::create_dir_all(&wa).unwrap();
    std::fs::create_dir_all(&gone).unwrap();
    let a = h.new_pane(None, "labA", Some("a"), &["/bin/sh"], Some(lab::path_str(&wa)));
    let b = h.new_pane(None, "labA", Some("b"), &["/bin/sh"], None);
    let c = h.new_pane(None, "labA", Some("c"), &[&claude, "--session-id", SID_C], None);
    let d = h.new_pane(None, "labA", Some("d"), &["/bin/sh", "-c", "echo job-ran-$((3*3)); exec cat"], None);
    let e = h.new_pane(None, "labA", Some("e"), &["/bin/sh"], Some(lab::path_str(&gone)));
    // a·b 셸에서 같은 대화(SID_A)의 Claude가 돈다
    for p in [&a, &b] {
        h.ok(&["pane", "run", p, &format!("FAKE_SID={SID_A} claude")]);
    }
    hw.wait("harness 세션 ID", |w| [&a, &b, &c].iter().all(|p| has_session(w, p)).then_some(()));
    h.wait_output(&d, "job-ran-9", T);
    // 사람이 붙인 이름은 다시 띄운 pane이 잇는다(herdr 복원 `set_manual_label`)
    h.ok(&["pane", "rename", &a, "내 셸"]);
    h.ok(&["pane", "rename", &d, "작업"]);
    std::fs::remove_dir(&gone).unwrap();
    reboot(&mut lab, &h, &env, 5);

    let l = hw.wait("복원", |w| {
        let l = w.leaves();
        let running = |i: usize| w.pane(&l[i]).is_some_and(|p| p.state.is_running());
        // 붙인 이름은 state 사건(bump)으로, layout은 따로 와서 차례가 정해지지 않는다: 이름까지 본다
        let named = l.first().and_then(|p| w.pane(p)).is_some_and(|p| p.label.as_deref() == Some("내 셸"));
        let restored = l.len() == 5 && l[0] != a && l[1] != b && l[2] != c && (0..3).all(running) && named;
        let left = l.get(3) == Some(&d) && w.pane(&d).is_some_and(|p| p.state == SessionState::Interrupted);
        let failed = l.get(4) == Some(&e) && w.pane(&e).is_some_and(|p| p.restore_error.is_some());
        (restored && left && failed).then(|| l.clone())
    });
    // 저장한 cwd에서 같은 셸, 그 셸에 이어가기 명령을 쳤다
    let pa = hw.pane(&l[0]).unwrap().clone();
    assert_eq!((pa.argv.as_slice(), pa.cwd.as_deref()), (&["/bin/sh".to_string()][..], Some(lab::path_str(&wa))));
    h.wait_output(&l[0], &format!("fake-claude --resume {SID_A}"), T);
    // 붙인 이름을 이었다. 지우면 띄운 명령 이름이다(처음 띄울 때 준 이름 `a`는 host가
    // 모른다: agent가 `name`을 붙인 이름으로 덮어 알린다). 이름이 없던 pane은 띄울 때 이름 그대로
    assert_eq!((pa.name.as_str(), pa.label.as_deref()), ("내 셸", Some("내 셸")));
    let p = h.ok(&["pane", "rename", &l[0], "--clear"]);
    assert_eq!(p["name"], "sh");
    assert!(p.get("label").is_none(), "{p}");
    assert_eq!(hw.pane(&l[1]).map(|p| (p.name.as_str(), p.label.is_none())), Some(("b", true)));
    // Claude를 바로 띄운 pane은 원래 argv(실행 파일 경로)에서 대화 인자만 바꿔 띄웠다
    assert_eq!(hw.pane(&l[2]).unwrap().argv, [claude.as_str(), "--resume", SID_C]);
    h.wait_output(&l[2], &format!("fake-claude --resume {SID_C}"), T);
    // 같은 대화는 한 번만 잇는다: b는 빈 셸(뒤에 친 줄보다 먼저 쳤을 이어가기 명령이 화면에 없다)
    h.ok(&["pane", "run", &l[1], "echo plain-$((2+2))"]);
    h.wait_output(&l[1], "plain-4", T);
    assert!(!h.screen(&l[1]).contains("fake-claude"), "{}", h.screen(&l[1]));
    // 폴더가 없어진 셸은 까닭과 함께 중단으로 남는다
    let why = hw.pane(&e).unwrap().restore_error.clone().unwrap();
    assert!(why.contains("작업 폴더가 없다"), "{why}");
    // 옛 기록은 지웠다
    assert!(
        [&a, &b, &c].iter().all(|p| hw.pane(p).is_none()),
        "{:?}",
        hw.panes.iter().map(|p| &p.id).collect::<Vec<_>>()
    );
    // 프로그램 pane은 사람이 다시 띄운다: 같은 spec, 같은 layout 자리
    let p = h.ok(&["pane", "resume", &d]);
    assert_eq!((p["name"].as_str(), p["label"].as_str()), (Some("작업"), Some("작업")));
    let nd = p["id"].as_str().unwrap().to_string();
    h.wait_output(&nd, "job-ran-9", T);
    hw.wait("재개 자리", |w| (w.leaves().get(3) == Some(&nd)).then_some(()));
}

/// QA t-0337 Q20: 확인창을 띄운 채 재부팅으로 끊긴 Claude pane. 다시 붙은 agent가 그 세션의 마지막 상태(질문)를
/// 알려도(`hello.harness`) 끊긴 pane에 질문을 다시 열지 않는다(전에는 닫은 뒤 옛 pane ID로 다시 열려 복원 뒤에도 남았다).
/// 질문은 agent가 다시 붙는 처리 안에서 열렸으므로, 복원한 pane이 보이면 이미 열렸을 때다.
#[test]
fn reboot_does_not_reopen_the_question_of_an_interrupted_pane() {
    let mut lab = Lab::new();
    let (claude, env) = fake_claude_env(&lab);
    let (h, mut hw) = start_watch(&mut lab, LOCAL_ONLY, &env);
    let c = h.new_pane(None, "labA", Some("c"), &[&claude, "--session-id", SID_C, "--ask"], None);
    let asks = |pane: &str| -> Vec<serde_json::Value> {
        let v = h.ok(&["question", "list"]);
        v["questions"].as_array().unwrap().iter().filter(|q| q["pane"] == pane).cloned().collect()
    };
    hw.wait("harness 세션 ID", |w| has_session(w, &c).then_some(()));
    wait_for("확인창 질문", T, || (!asks(&c).is_empty()).then_some(()));
    reboot(&mut lab, &h, &env, 1);
    let n = hw.wait("복원", |w| {
        let l = w.leaves();
        (l.len() == 1 && l[0] != c && w.pane(&l[0]).is_some_and(|p| p.state.is_running())).then(|| l[0].clone())
    });
    h.wait_output(&n, &format!("fake-claude --resume {SID_C}"), T);
    assert_eq!(asks(&c), Vec::<serde_json::Value>::new(), "끊긴 pane의 질문이 다시 열렸다");
}

/// 보안 감사 t-0340: hook이 알린 harness 세션 ID는 UUID 꼴만 받는다(`aaa_common::util::harness_session_ok`). 셸 pane의
/// 프로그램이 `x; touch pwned`를 세션 ID로 알려도 재부팅 복원이 셸에 그 줄을 치지 않는다(전에는 `claude --resume x;
/// touch pwned`가 돌 수 있었다). 결함 심기(검사를 늘 참으로): 세션 ID가 남아 첫 단언에서 실패하고, 그 단언을 빼면
/// 복원한 셸에 그 줄을 쳐서 뒤에 친 줄의 출력(`plain-4`)이 나오지 않는다.
#[test]
fn reboot_does_not_type_a_forged_session_id() {
    let mut lab = Lab::new();
    let (_, env) = fake_claude_env(&lab);
    let (h, mut hw) = start_watch(&mut lab, LOCAL_ONLY, &env);
    let dir = lab.root.join("wf");
    std::fs::create_dir_all(&dir).unwrap();
    let a = h.new_pane(None, "labA", Some("a"), &["/bin/sh"], Some(lab::path_str(&dir)));
    let hook = r#""$AA" hook --agent claude '{"hook_event_name":"SessionStart","session_id":"x; touch pwned"}'"#;
    h.ok(&["pane", "run", &a, hook]);
    // 사건은 받았다(harness claude), 세션 ID는 버렸다
    hw.wait("hook 받음", |w| w.pane(&a)?.agent.as_ref().filter(|g| g.harness == "claude").map(|_| ()));
    assert!(!has_session(&hw, &a), "{:?}", hw.pane(&a).unwrap().agent);
    reboot(&mut lab, &h, &env, 1);
    let n = hw.wait("복원", |w| {
        let l = w.leaves();
        (l.len() == 1 && l[0] != a && w.pane(&l[0]).is_some_and(|p| p.state.is_running())).then(|| l[0].clone())
    });
    // 셸이 앞에 친 줄을 다 읽은 뒤의 출력
    h.ok(&["pane", "run", &n, "echo plain-$((2+2))"]);
    h.wait_output(&n, "plain-4", T);
    assert!(!dir.join("pwned").exists(), "복원이 셸에 세션 ID를 쳤다");
    assert!(!h.screen(&n).contains("--resume"), "{}", h.screen(&n));
}

/// 보안 감사 t-0340: 탭을 닫는 동안(그 안 pane 끝내기를 기다림) 들어온 다른 layout 변경이 닫기가 끝날 때 사라지지 않는다.
/// 시험 agent를 SIGSTOP으로 세워 끝내기 응답을 붙잡고, 탭을 뺀 layout이 먼저 저장된 사건을 본 뒤 다른 탭 이름을 바꾼다.
/// 결함 심기: 맨 위 복사본을 끝에 저장하던 판이면 「탭을 뺀 layout」 사건이 끝내기 전에 오지 않고, 먼저 저장만 하고
/// 다시 읽지 않으면 바꾼 이름이 사라진다.
#[cfg(unix)]
#[test]
fn tab_close_keeps_layout_changes_made_while_closing() {
    let mut lab = Lab::new();
    let (h, mut hw) = start_watch(&mut lab, LOCAL_ONLY, &[]);
    let a = h.new_pane(None, "labA", Some("a"), &["/bin/sh"], None);
    let b = h.new_pane(None, "labA", Some("b"), &["/bin/sh"], None);
    let tabs = hw.wait("탭 둘", |w| {
        let ws = w.ws.as_ref()?;
        let up = [&a, &b].iter().all(|p| w.pane(p).is_some_and(|v| v.state.is_running()));
        (ws.tabs.len() == 2 && up).then(|| ws.tabs.iter().map(|t| t.id.clone()).collect::<Vec<_>>())
    });
    lab.signal("agent", libc::SIGSTOP);
    let node = h.clone();
    let tab_b = tabs[1].clone();
    let close = std::thread::spawn(move || node.ok(&["tab", "close", &tab_b]));
    hw.wait("탭을 뺀 layout", |w| (w.ws.as_ref()?.tabs.len() == 1).then_some(()));
    h.ok(&["tab", "rename", &tabs[0], "kept"]);
    lab.signal("agent", libc::SIGCONT);
    let w = close.join().unwrap();
    assert_eq!((w["tabs"].as_array().map(|t| t.len()), w["tabs"][0]["name"].as_str()), (Some(1), Some("kept")), "{w}");
    let t = h.ok(&["tab", "get", &tabs[0]]);
    assert_eq!(t["tab"]["name"], "kept", "{t}");
    // 닫은 탭의 pane은 끝나고 기록에서 지워졌다
    hw.wait("b 지움", |w| w.pane(&b).is_none().then_some(()));
}

/// `[session] resume_agents_on_restore = false`(herdr와 같은 이름): 셸은 되살리되 대화는 잇지 않는다. 셸에서 돌던
/// Claude는 빈 셸로, Claude를 바로 띄운 pane은 중단으로 남는다.
#[test]
fn reboot_without_agent_resume_restores_plain_shells() {
    let mut lab = Lab::new();
    let (claude, env) = fake_claude_env(&lab);
    let toml = format!("{LOCAL_ONLY}[session]\nresume_agents_on_restore = false\n");
    let (h, mut hw) = start_watch(&mut lab, &toml, &env);
    let a = h.new_pane(None, "labA", Some("a"), &["/bin/sh"], None);
    let c = h.new_pane(None, "labA", Some("c"), &[&claude, "--session-id", SID_C], None);
    h.ok(&["pane", "run", &a, &format!("FAKE_SID={SID_A} claude")]);
    hw.wait("harness 세션 ID", |w| [&a, &c].iter().all(|p| has_session(w, p)).then_some(()));
    reboot(&mut lab, &h, &env, 2);

    let l = hw.wait("셸만 복원", |w| {
        let l = w.leaves();
        let shell = l.first().is_some_and(|p| *p != a && w.pane(p).is_some_and(|v| v.state.is_running()));
        let left = l.get(1) == Some(&c) && w.pane(&c).is_some_and(|p| p.state == SessionState::Interrupted);
        (shell && left).then(|| l.clone())
    });
    h.ok(&["pane", "run", &l[0], "echo plain-$((2+2))"]);
    h.wait_output(&l[0], "plain-4", T);
    assert!(!h.screen(&l[0]).contains("fake-claude"), "{}", h.screen(&l[0]));
    assert!(hw.pane(&c).unwrap().restore_error.is_none());
}

/// pane 이름 바꾸기(herdr `pane rename <pane> <label>|--clear`, plan §8.109). 이름의 정본은 agent `spec.json`의 label이다:
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

/// 셸에서 `exit`하면(셸이 스스로 끝남) 그 pane을 닫는다(herdr 0.9.5 `handle_pane_died`·`Workspace::remove_pane`, plan §8.123):
/// 나눈 자리에서 빠지고, 탭의 마지막 pane이면 탭이, workspace의 마지막 탭이면 workspace가 없어진다. agent가 내려간 사이
/// 끝난 셸도 다시 붙을 때 닫는다. `pane kill`로 끝낸 셸과 프로그램 pane은 끝난 화면과 종료 코드를 남긴다.
#[test]
fn shell_exit_closes_its_pane_tab_and_workspace() {
    let mut lab = Lab::new();
    let (h, mut hw) = start_watch(&mut lab, LOCAL_ONLY, &[]);
    let a = h.new_pane(None, "labA", Some("a"), &["/bin/sh"], None);
    let w = h.ok(&["pane", "split", &a, "--direction", "right"]);
    let b = w["tabs"][0]["root"]["b"]["pane"].as_str().unwrap().to_string();
    let c = h.new_pane(None, "labA", Some("c"), &["/bin/sh"], None);
    let k = h.new_pane(None, "labA", Some("k"), &["/bin/sh"], None);
    let p = h.new_pane(None, "labA", Some("p"), &["/bin/sh", "-c", "read x; exit 4"], None);
    let up = [&a, &b, &c, &k, &p];
    hw.wait("모두 실행 중", |w| {
        up.iter().all(|id| w.pane(id).is_some_and(|v| v.state.is_running())).then_some(())
    });

    // 남기는 것 먼저 끝낸다: 사람이 끝낸 셸, 프로그램 pane
    h.ok(&["pane", "kill", &k]);
    h.ok(&["pane", "run", &p, "go"]);
    hw.wait("k·p 끝남", |w| {
        let k_done = w.pane(&k).is_some_and(|v| matches!(v.state, SessionState::Exited { .. }));
        (k_done && w.pane(&p)?.state == SessionState::Exited { code: Some(4) }).then_some(())
    });
    // 나눈 pane 하나: 그 자리에서 빠지고 옆 pane이 탭을 채운다
    h.ok(&["pane", "run", &b, "exit"]);
    let tabs = |w: &HostWatch| -> Vec<Vec<String>> {
        w.ws.iter().flat_map(|ws| ws.tabs.iter().map(|t| aaa_proto::layout::leaves(&t.root))).collect()
    };
    hw.wait("b 닫힘", |w| (w.pane(&b).is_none() && tabs(w)[0] == [a.clone()]).then_some(()));
    // 앞서 끝난 k·p는 닫는 일이 그 앞에 끝났을 것이다: 그대로 남았다
    assert_eq!(tabs(&hw), [vec![a.clone()], vec![c.clone()], vec![k.clone()], vec![p.clone()]]);
    assert!(matches!(hw.pane(&k).unwrap().state, SessionState::Exited { .. }));
    assert!(h.screen(&p).contains("go"), "프로그램 pane은 끝난 화면을 남긴다");
    // 탭의 마지막 pane이면 탭이 없어진다
    h.ok(&["pane", "run", &c, "exit 3"]);
    hw.wait("c의 탭 닫힘", |w| (w.pane(&c).is_none() && tabs(w).len() == 3).then_some(()));

    // workspace의 마지막 탭이면 workspace가 없어진다(main은 늘 보인다)
    let s = h.new_pane(Some("side"), "labA", None, &["/bin/sh"], None);
    hw.wait("s 실행 중", |w| w.pane(&s).is_some_and(|v| v.state.is_running()).then_some(()));
    h.ok(&["pane", "run", &s, "exit"]);
    hw.wait("s 닫힘", |w| w.pane(&s).is_none().then_some(()));
    let names = || -> Vec<String> {
        let v = h.ok(&["workspace", "list"]);
        v["workspaces"].as_array().unwrap().iter().map(|w| w["name"].as_str().unwrap().to_string()).collect()
    };
    wait_for("side 닫힘", T, || (names() == ["main"]).then_some(()));

    // agent가 내려간 사이 끝난 셸: 다시 붙을 때 닫는다(host는 그 전에 실행 중을 봤다)
    let d = h.new_pane(None, "labA", Some("d"), &["/bin/sh"], None);
    let fifo = lab.root.join("go");
    let cpath = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(cpath.as_ptr(), 0o600) }, 0);
    h.ok(&["pane", "run", &d, &format!("echo ready-$((1+1)); read x < {}; exit", fifo.display())]);
    h.wait_output(&d, "ready-2", T);
    lab.kill("agent");
    hw.wait("labA 끊김", |w| (!w.connected("labA")).then_some(()));
    std::fs::write(&fifo, "x\n").unwrap();
    let exit = h.state.join("agent").join("sessions").join(&d).join("exit.json");
    wait_for("d 종료 기록", T, || exit.exists().then_some(()));
    lab.spawn("agent2", &h, &["agent", "--host", "local", "--name", "labA"]);
    hw.wait("d 닫힘", |w| (w.connected("labA") && w.pane(&d).is_none() && !w.leaves().contains(&d)).then_some(()));
    assert_eq!(tabs(&hw), [vec![a], vec![k], vec![p]]);
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
    // 앞뒤 빈칸은 빼고, 빈칸뿐이면 이름 없음(보안 감사 t-0340: 병합에서 빠졌던 t-0330 처리)
    h.ok(&["tab", "rename", &tab, "   "]);
    assert_eq!(h.ok(&["tab", "get", &tab])["tab"]["name"], "");
    h.ok(&["tab", "rename", &tab, "  renamed  "]);
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

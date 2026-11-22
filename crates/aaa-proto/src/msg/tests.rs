use super::*;
use crate::QuestionAnswerView;
use serde_json::json;

/// 옛 공통 답 요청·기록을 계속 읽고 새 의견의 null 위치와 줄바꿈을 보존한다.
#[test]
fn question_comments_keep_legacy_requests_and_answer_records_readable() {
    let old = json!({"op":"thread_answer","thread":"t-0001","report_hash":"h","picks":[0,null],"text":"2 원문"});
    let FrontRequest::ThreadAnswer { comments, text, .. } = serde_json::from_value(old).unwrap() else { panic!() };
    assert!(comments.is_empty());
    assert_eq!(text.as_deref(), Some("2 원문"));
    let legacy = json!({"picks":[0,null],"text":"옛 공통 답","summary":"1A 직접","by":"CLI","at_ms":1});
    let mut answer: QuestionAnswerView = serde_json::from_value(legacy).unwrap();
    assert!(answer.comments.is_empty());
    answer.comments = vec![Some("선택과 의견\n다음 줄".into()), None];
    assert_eq!(serde_json::from_value::<QuestionAnswerView>(serde_json::to_value(&answer).unwrap()).unwrap(), answer);
}

#[test]
fn link_wire_shape_is_stable() {
    // wire 계약: 이 JSON 모양이 바뀌면 이전 agent와 호환이 깨진다.
    let msg = LinkMsg::Req {
        id: "c1".into(),
        deadline_ms: None,
        body: AgentRequest::Input { session: "s1".into(), data: B64(b"ls\r".to_vec()) },
    };
    assert_eq!(
        serde_json::to_value(&msg).unwrap(),
        json!({"t":"req","id":"c1","deadline_ms":null,"body":{"op":"input","session":"s1","data":"bHMN"}})
    );
    let res = LinkMsg::Res { id: "c1".into(), result: Outcome::Err(ErrorBody::new(ErrorCode::NotFound, "x")) };
    assert_eq!(
        serde_json::to_value(&res).unwrap(),
        json!({"t":"res","id":"c1","result":{"err":{"code":"not_found","message":"x"}}})
    );
}

#[test]
fn unknown_optional_fields_are_ignored_but_unknown_kinds_fail() {
    // 더 새 agent가 모르는 필드를 보내도 이전 host가 읽는다.
    let v = json!({"t":"hello","proto":{"major":1,"minor":4},"machine_id":"m","machine_name":"n",
        "boot_id":"b","os":"macos","arch":"aarch64","software":"aaa 9","future_field":{"x":1}});
    let LinkMsg::Hello(h) = serde_json::from_value::<LinkMsg>(v).unwrap() else { panic!() };
    assert!(h.sessions.is_empty() && !h.dev_mode && h.features.is_empty());
    assert!(serde_json::from_value::<LinkMsg>(json!({"t":"teleport"})).is_err());
    assert!(serde_json::from_value::<LinkMsg>(json!({"t":"hello"})).is_err());
}

#[test]
fn session_state_flattens_into_info() {
    let info = SessionInfo {
        responsibility: None,
        id: "s".into(),
        name: "n".into(),
        argv: vec![],
        cwd: None,
        pid: Some(3),
        started_at_ms: 1,
        boot_id: "b".into(),
        cols: 80,
        rows: 24,
        state: SessionState::Exited { code: Some(2) },
        role: None,
        label: None,
        harness: Some("codex".into()),
    };
    let v = serde_json::to_value(&info).unwrap();
    assert_eq!(v["state"], "exited");
    assert_eq!(v["code"], 2);
    assert_eq!(serde_json::from_value::<SessionInfo>(v).unwrap(), info);
}

#[test]
fn b64_rejects_garbage() {
    assert!(serde_json::from_value::<B64>(json!("%%%")).is_err());
}

#[test]
fn control_classification() {
    for req in [
        FrontRequest::LibraryLink { project: "p".into(), thread: "t-0001".into(), days: None },
        FrontRequest::LibraryLinks { project: None },
        FrontRequest::LibraryLinkRevoke { id: "id".into() },
    ] {
        assert!(req.needs_control());
    }
    // reader가 보낼 수 있는 요청은 읽기뿐이어야 한다.
    assert!(!FrontRequest::PaneRead { pane: "p".into() }.needs_control());
    // thread pane 읽기는 보기, 키로 답하기는 조작이다(세션 안 CLI는 보기만, §8.25)
    let (project, thread) = ("p".to_string(), "t-0001".to_string());
    assert!(
        !FrontRequest::ThreadRead { project: project.clone(), thread: thread.clone(), lines: None }.needs_control()
    );
    assert!(FrontRequest::ThreadKeys { project, thread, keys: vec!["enter".into()], text: None }.needs_control());
    assert!(
        !FrontRequest::PaneChat { pane: "p".into(), thinking: false, compact: true, commands: true, before: None }
            .needs_control()
    );
    // 파일 보기는 조작 기기만(herdr-web-ui도 보기 전용 기기는 fs/ 403)
    assert!(FrontRequest::PaneFile { pane: "p".into(), path: "a.txt".into() }.needs_control());
    // diff는 소스를 보인다: 보기 기기는 못 받는다(plan §8.305)
    assert!(
        FrontRequest::ThreadDiff { project: "p".into(), thread: "t-0001".into(), query: Default::default() }
            .needs_control()
    );
    assert!(
        FrontRequest::ThreadFile { project: "p".into(), thread: "t-0001".into(), path: "a.txt".into() }.needs_control()
    );
    assert!(FrontRequest::PaneDir { pane: "p".into(), path: "src".into(), hidden: false }.needs_control());
    assert!(FrontRequest::PanePaste { pane: "p".into(), text: "a".into() }.needs_control());
    assert!(FrontRequest::PaneSend { pane: "p".into(), data: B64(vec![]) }.needs_control());
    assert!(FrontRequest::PaneResize { pane: "p".into(), cols: 1, rows: 1 }.needs_control());
    assert!(
        FrontRequest::PaneAttach { pane: "p".into(), offset: 0, total: 1, sha256: String::new(), data: B64(vec![]) }
            .needs_control()
    );
    // 웹 설정에서도 코드를 만든다(D6). 조작 권한이 있어야 하고, 목록은 보기 권한도 본다
    assert!(!FrontRequest::WebPair { role: DeviceRole::Controller }.local_only());
    assert!(FrontRequest::WebPair { role: DeviceRole::Controller }.needs_control());
    assert!(!FrontRequest::Devices.needs_control());
    assert!(FrontRequest::DeviceRename { id: "d".into(), label: "x".into() }.needs_control());
    assert!(FrontRequest::DeviceRevoke { id: "d".into() }.needs_control());
    assert!(!FrontRequest::Panes { role: None }.local_only());
    // 「직접 설치」 안내는 host 주소·지문을 보이므로 조작 기기만(웹 허용, plan §8.383)
    assert!(FrontRequest::EnrollGuide.needs_control() && !FrontRequest::EnrollGuide.local_only());
    // SSH 머신 추가(plan §8.384): 목록은 Reader도, 시작·신뢰·고름·멈춤은 조작 기기
    assert!(!FrontRequest::MachineSetupList.needs_control());
    assert!(FrontRequest::MachineSetupStart { target: "mac-b".into() }.needs_control());
    assert!(FrontRequest::MachineSetupTrust { job: "j".into(), host_key_sha256: "SHA256:x".into() }.needs_control());
    assert!(!FrontRequest::MachineSetupStart { target: "mac-b".into() }.local_only());
    let apply =
        FrontRequest::MachineSetupApply { job: "j".into(), plan_sha256: "0".repeat(64), service_password: None };
    assert!(apply.needs_control() && !apply.local_only());
    assert!(!FrontRequest::Threads { project: None, role: None, query: Default::default() }.needs_control());
    // 사후 평가자 세션은 Reader로 다룬다: 평가 보기는 되고, 지정·채택·메시지는 안 된다(§8.13)
    assert!(!FrontRequest::EvaluatorStatus { project: None }.needs_control());
    assert!(!FrontRequest::Evaluations { project: "p".into(), all: false }.needs_control());
    assert!(
        FrontRequest::EvaluationDecide {
            project: "p".into(),
            id: "e".into(),
            proposals: vec![],
            adopt: true,
            note: None
        }
        .needs_control()
    );
    assert!(FrontRequest::EvaluatorUnassign { project: "p".into() }.needs_control());
    assert!(
        FrontRequest::ThreadMessage {
            project: "p".into(),
            thread: "t".into(),
            text: "x".into(),
            mode: MessageMode::Queue
        }
        .needs_control()
    );
    assert!(FrontRequest::ThreadAck { project: "p".into(), thread: "t".into() }.needs_control());
    // 설정 관리(W5c)는 읽기도 조작 기기만: host 설정 경로·profile 인자·머신 폴더를 보인다
    assert!(FrontRequest::ProfileAdmin.needs_control());
    assert!(FrontRequest::SafetyShow { project: "p".into(), scope: Default::default() }.needs_control());
    assert!(FrontRequest::WorkRootShow { machine: "m".into() }.needs_control());
    assert!(FrontRequest::HookStatus { machine: "m".into() }.needs_control());
    assert!(FrontRequest::ModelAdmin.needs_control());
    assert!(!FrontRequest::Questions { all: true }.needs_control());
    assert!(!FrontRequest::Asks { project: "p".into(), paged: true }.needs_control());
    assert!(!FrontRequest::AskHistory { project: "p".into(), before: None, id: Some("a".into()) }.needs_control());
    assert!(
        FrontRequest::QuestionAnswer { id: "q".into(), choice: "yes".into(), menu: None, pick: None }.needs_control()
    );
    assert!(
        FrontRequest::QuestionAnswer {
            id: "q".into(),
            choice: String::new(),
            menu: Some("m".into()),
            pick: Some(MenuPick::Custom { text: "x".into() })
        }
        .needs_control()
    );
}

#[test]
fn additive_fields_keep_old_messages() {
    // 1.1 front의 `panes`(필드 없음), 1.1 session host의 SessionInfo(role 없음)를 그대로 읽는다
    assert_eq!(
        serde_json::from_value::<FrontRequest>(json!({"op":"panes"})).unwrap(),
        FrontRequest::Panes { role: None }
    );
    // 1.2 front의 threads(role 없음)와 hello(session 없음)
    assert_eq!(
        serde_json::from_value::<FrontRequest>(json!({"op":"threads","project":null})).unwrap(),
        FrontRequest::Threads { project: None, role: None, query: Default::default() }
    );
    let v = json!({"t":"hello","proto":{"major":1,"minor":2},"client":"cli"});
    assert!(matches!(serde_json::from_value::<FrontMsg>(v), Ok(FrontMsg::Hello { session: None, .. })));
    let v = json!({"id":"s","name":"","argv":[],"cwd":null,"pid":null,"started_at_ms":1,"boot_id":"b",
        "cols":80,"rows":24,"state":"running"});
    assert_eq!(serde_json::from_value::<SessionInfo>(v).unwrap().role, None);
    let v = json!({"name":"","argv":[],"cwd":null,"cols":80,"rows":24});
    assert_eq!(serde_json::from_value::<SpawnSpec>(v).unwrap().role, None);
    // 1.2 host의 PaneView(agent 없음)와 1.2 front의 thread message(mode 없음 → queue)
    let v = json!({"id":"p","machine_id":"m","machine":"mac-a","name":"","argv":[],"cwd":null,"state":"running",
        "connection":"connected","readonly":false,"cols":80,"rows":24,"lease":null,"role":null});
    assert_eq!(serde_json::from_value::<PaneView>(v).unwrap().agent, None);
    let v = json!({"op":"thread_message","project":"p","thread":"t","text":"x"});
    assert!(matches!(
        serde_json::from_value::<FrontRequest>(v).unwrap(),
        FrontRequest::ThreadMessage { mode: MessageMode::Queue, .. }
    ));
    // 1.28 front의 예·아니오 답(선택 창 답 없음)과 1.28 host의 질문(선택 창 없음)
    assert_eq!(
        serde_json::from_value::<FrontRequest>(json!({"op":"question_answer","id":"q","choice":"yes"})).unwrap(),
        FrontRequest::QuestionAnswer { id: "q".into(), choice: "yes".into(), menu: None, pick: None }
    );
    let v = json!({"id":"q","pane":"p","machine":"mac-a","project":null,"thread":null,"question_kind":"permission",
        "text":"t","options":["yes","no"],"state":"open","answer":null,"created_at_ms":1});
    assert_eq!(serde_json::from_value::<QuestionView>(v).unwrap().menu, None);
    let v = json!({"op":"question_answer","id":"q","menu":"m","pick":{"by":"options","indices":[0,2]}});
    assert!(matches!(
        serde_json::from_value::<FrontRequest>(v).unwrap(),
        FrontRequest::QuestionAnswer { pick: Some(MenuPick::Options { .. }), .. }
    ));
}
#[test]
fn chat_pages_keep_legacy_requests_and_responses_readable() {
    #[derive(Deserialize)]
    #[serde(tag = "op", rename_all = "snake_case")]
    enum OldRequest {
        PaneChat { pane: String, thinking: bool },
    }
    let old = r#"{"op":"pane_chat","pane":"p","thinking":false}"#;
    let request: FrontRequest = serde_json::from_str(old).unwrap();
    assert!(matches!(request, FrontRequest::PaneChat { before: None, compact: false, commands: false, .. }));
    let new = FrontRequest::PaneChat {
        pane: "p".into(),
        thinking: true,
        compact: true,
        commands: true,
        before: Some("cursor".into()),
    };
    let OldRequest::PaneChat { pane, thinking } = serde_json::from_value(serde_json::to_value(new).unwrap()).unwrap();
    assert_eq!(pane, "p");
    assert!(thinking);
    let old_reply = r#"{"pane":"p","harness":"claude","messages":[{"role":"assistant","text":"答"}],"truncated":true}"#;
    let legacy: ChatView = serde_json::from_str(old_reply).unwrap();
    assert!(legacy.before.is_none() && legacy.history.is_none() && legacy.messages[0].id.is_none());
    #[derive(Deserialize)]
    struct OldView {
        pane: String,
        messages: Vec<OldMessage>,
        truncated: bool,
    }
    #[derive(Deserialize)]
    struct OldMessage {
        text: String,
    }
    let new = ChatView { before: Some("cursor".into()), history: Some("file".into()), ..legacy };
    let old: OldView = serde_json::from_value(serde_json::to_value(new).unwrap()).unwrap();
    assert_eq!(old.pane, "p");
    assert_eq!(old.messages[0].text, "答");
    assert!(old.truncated);
    let agent: AgentResponse = serde_json::from_str(r#"{"kind":"chat","messages":[],"truncated":true}"#).unwrap();
    assert!(matches!(agent, AgentResponse::Chat { before: None, history: None, .. }));
}

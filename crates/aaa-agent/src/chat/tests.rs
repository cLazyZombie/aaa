use super::*;

pub(super) fn dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("aaa-chat-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// 합성 기록(실제 기록은 쓰지 않는다): 큰 요청 원문 줄, 하위 agent, 생각, 시스템 글, 도구 결과가 섞여 있다
fn claude_fixture() -> String {
    let blob = format!(r#"{{"type":"api-request-blob","data":"{}"}}"#, "x".repeat(3 << 20));
    [
        r#"{"type":"user","message":{"role":"user","content":"첫 질문"}}"#.to_string(),
        blob,
        r#"{"type":"user","message":{"role":"user","content":"<system-reminder>끼운 글</system-reminder>"}}"#.into(),
        r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"meta 글"}}"#.into(),
        r#"{"type":"assistant","isSidechain":true,"message":{"content":[{"type":"text","text":"하위 agent 글"}]}}"#.into(),
        r#"{"type":"assistant","message":{"content":[{"type":"thinking","thinking":"속생각"},{"type":"text","text":"답을 쓴다"},{"type":"tool_use","name":"Bash","input":{"command":"cargo test\n  --all"}}]}}"#.into(),
        r#"{"type":"user","message":{"content":[{"type":"tool_result","content":"도구 결과 본문"}]}}"#.into(),
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"끝났다"}]}}"#.into(),
        r#"{"type":"queue-operation","operation":"enqueue","content":"작업 중 추가 요청"}"#.into(),
        r#"{"type":"queue-operation","operation":"remove","content":"작업 중 추가 요청","reason":"absorbed_mid_turn"}"#.into(),
        r#"{"type":"attachment","attachment":{"type":"queued_command","commandMode":"prompt","origin":{"kind":"human"},"prompt":"작업 중 추가 요청"}}"#.into(),
        r#"{"type":"attachment","attachment":{"type":"queued_command","commandMode":"task-notification","prompt":"배경 도구 알림"}}"#.into(),
        r#"{"type":"attachment","attachment":{"type":"queued_command","commandMode":"prompt","origin":{"kind":"peer"},"prompt":"다른 agent 글"}}"#.into(),
    ]
    .join("\n")
        + "\n"
}

#[test]
fn claude_tail_filters() {
    let root = dir("claude");
    let f = root.join("s.jsonl");
    std::fs::write(&f, claude_fixture()).unwrap();
    let page = pages::read(&f, None, |v| claude_line(v, false)).unwrap();
    let truncated = page.before.is_some();
    let m = page.messages;
    let got: Vec<_> = m.iter().map(|x| (x.role, x.text.as_str())).collect();
    assert_eq!(
        got,
        [
            (ChatRole::User, "첫 질문"),
            (ChatRole::Assistant, "답을 쓴다"),
            (ChatRole::Tool, "Bash: cargo test --all"),
            (ChatRole::Assistant, "끝났다"),
            (ChatRole::User, "작업 중 추가 요청"),
        ]
    );
    assert!(!truncated);
    // 「생각 표시」를 켠 화면이 청하면 생각도 제자리에
    let m = pages::read(&f, None, |v| claude_line(v, true)).unwrap().messages;
    let got: Vec<_> = m.iter().map(|x| (x.role, x.text.as_str())).collect();
    assert_eq!(&got[1..3], [(ChatRole::Thinking, "속생각"), (ChatRole::Assistant, "답을 쓴다")]);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn claude_user_origin_separates_notifications_from_literal_receipts() {
    use serde_json::json;
    let notification = "<task-notification><task-id>task-1</task-id><status>completed</status><summary>작업 끝</summary></task-notification>";
    let reminder = "<system-reminder>사람이 인용한 예시</system-reminder>";
    for content in [json!(notification), json!([{"type":"text","text":notification}])] {
        for kind in ["task-notification", "peer"] {
            let record = json!({"type":"user","origin":{"kind":kind},"message":{"role":"user","content":content}});
            assert!(claude_line(&record, false).is_empty(), "내부 알림은 user receipt가 아니다: {kind}");
        }
    }
    for text in [notification, reminder] {
        let record = json!({"type":"user","origin":{"kind":"human"},"message":{"content":text}});
        let messages = claude_line(&record, false);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].text, text);
        assert_eq!(messages[0], msg(ChatRole::User, text).unwrap(), "사람의 원문과 수신 hash 보존");
        let queued = json!({"type":"attachment","attachment":{"type":"queued_command","commandMode":"prompt","origin":{"kind":"human"},"prompt":text}});
        assert_eq!(claude_line(&queued, false), messages);
    }
    for flag in ["isMeta", "isCompactSummary", "isSynthetic"] {
        for mut record in [
            serde_json::json!({"type":"user","origin":{"kind":"human"},"message":{"content":"내부 안내"}}),
            serde_json::json!({"type":"attachment","attachment":{"type":"queued_command","commandMode":"prompt","origin":{"kind":"human"},"prompt":"내부 안내"}}),
        ] {
            record[flag] = json!(true);
            assert!(claude_line(&record, false).is_empty(), "구조 필드가 글 모양보다 우선한다: {flag}");
        }
    }
    // 비사람 origin은 표지 없는 평문이어도 사람이 쓴 글이 아니다.
    assert!(
        claude_line(
            &json!({"type":"user","origin":{"kind":"task-notification"},"message":{"content":"작업 끝"}}),
            false
        )
        .is_empty()
    );
}

#[test]
fn claude_pending_queue() {
    let root = dir("pending");
    let f = root.join("s.jsonl");
    let op = |op: &str, content: Option<&str>| match content {
        Some(c) => format!(r#"{{"type":"queue-operation","operation":"{op}","content":{}}}"#, serde_json::json!(c)),
        None => format!(r#"{{"type":"queue-operation","operation":"{op}"}}"#),
    };
    let lines = [
        // 숨긴 알림도 remove가 같은 원문으로 찾아야 다음 dequeue가 사람 글을 꺼낸다.
        r#"{"type":"queue-operation","operation":"enqueue","origin":{"kind":"task-notification"},"content":"먼저 끝난 알림"}"#.into(),
        op("remove", Some("먼저 끝난 알림")),
        op("enqueue", Some("먼저 꺼낸 글")),
        op("dequeue", None),
        // 한가할 때 보낸 글: 넣자마자 꺼낸다
        op("enqueue", Some("바로 간 글")),
        op("dequeue", None),
        // 작업 중에 보내 흡수된 글
        op("enqueue", Some("흡수된 글")),
        op("remove", Some("흡수된 글")),
        // 아직 남은 글 둘과 배경 작업 알림
        op("enqueue", Some("<task-notification>\n<task-id>b1</task-id>")),
        op("enqueue", Some("기다리는 글")),
        op("enqueue", Some("기다리는 글 둘")),
        // dequeue는 맨 앞(알림)을 꺼낸다
        op("dequeue", None),
        r#"{"type":"queue-operation","operation":"enqueue","origin":{"kind":"human"},"content":"<system-reminder>사람의 예시</system-reminder>"}"#.into(),
        r#"{"type":"queue-operation","operation":"enqueue","origin":{"kind":"task-notification"},"content":"평문 내부 알림"}"#.into(),
    ];
    std::fs::write(&f, lines.join("\n") + "\n").unwrap();
    let got: Vec<_> = claude_pending(&f, "file").unwrap().into_iter().map(|m| (m.role, m.text, m.pending)).collect();
    assert_eq!(
        got,
        [
            (ChatRole::User, "기다리는 글".to_string(), true),
            (ChatRole::User, "기다리는 글 둘".to_string(), true),
            (ChatRole::User, "<system-reminder>사람의 예시</system-reminder>".to_string(), true)
        ]
    );
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn claude_queue_receipts_identify_each_enqueue_before_and_after_consumption() {
    use sha2::{Digest, Sha256};
    let root = dir("queue-receipts");
    let f = root.join("s.jsonl");
    let text = "같은 긴 본문".repeat(800);
    let enqueue =
        serde_json::json!({"type":"queue-operation", "operation":"enqueue", "content":text}).to_string() + "\n";
    std::fs::write(&f, &enqueue).unwrap();
    let first = read_path("claude", f.clone(), false, false, false, None).unwrap();
    let old = first.messages[0].clone();
    assert!(old.pending);
    assert!(old.id.is_some());
    assert_ne!(old.text, text, "표시 글은 축약돼야 한다");
    assert_eq!(old.receipt.unwrap().sha256, format!("{:x}", Sha256::digest(text.as_bytes())));
    std::fs::write(&f, format!("{enqueue}{enqueue}")).unwrap();
    let second = read_path("claude", f.clone(), false, false, false, None).unwrap();
    assert_eq!(second.history, first.history);
    assert_eq!(second.messages[0].id, old.id);
    assert!(second.messages[1].id > old.id, "같은 본문의 새 enqueue를 구별해야 한다");
    let dequeue = "{\"type\":\"queue-operation\",\"operation\":\"dequeue\"}\n";
    std::fs::write(&f, format!("{enqueue}{enqueue}{dequeue}")).unwrap();
    let consumed = read_path("claude", f, false, false, false, None).unwrap();
    assert_eq!(consumed.messages.len(), 1);
    assert_eq!(consumed.messages[0].id, second.messages[1].id, "앞 항목 소비가 새 ID를 만들면 안 된다");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn long_tail_is_truncated() {
    let root = dir("long");
    let f = root.join("s.jsonl");
    let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"줄"}]}}"#;
    std::fs::write(&f, format!("{line}\n").repeat(400)).unwrap();
    let page = pages::read(&f, None, |v| claude_line(v, false)).unwrap();
    let truncated = page.before.is_some();
    let m = page.messages;
    assert_eq!((m.len(), truncated), (150, true));
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn codex_lines() {
    let user = r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"고쳐 줘"}]}}"#;
    let env = r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<environment_context>cwd</environment_context>"}]}}"#;
    let dev = r#"{"type":"response_item","payload":{"type":"message","role":"developer","content":[{"type":"input_text","text":"규칙"}]}}"#;
    let call = r#"{"type":"response_item","payload":{"type":"function_call","name":"shell","arguments":"{\"command\":[\"bash\",\"-lc\",\"ls\"]}"}}"#;
    let think = r#"{"type":"response_item","payload":{"type":"reasoning","summary":[{"type":"summary_text","text":"목록부터 본다"}],"encrypted_content":"gAAA"}}"#;
    let out = r#"{"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"했다"}]}}"#;
    let interrupted = r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<turn_aborted>\nThe user interrupted the previous turn on purpose.\n</turn_aborted>"}]}}"#;
    let lines = |thinking: bool| -> Vec<_> {
        [user, env, interrupted, dev, think, call, out]
            .iter()
            .flat_map(|l| codex_line(&serde_json::from_str(l).unwrap(), thinking))
            .map(|m| (m.role, m.text))
            .collect()
    };
    assert_eq!(
        lines(false),
        [
            (ChatRole::User, "고쳐 줘".to_string()),
            (ChatRole::Tool, "shell: bash -lc ls".to_string()),
            (ChatRole::Assistant, "했다".to_string())
        ]
    );
    assert_eq!(lines(true)[1], (ChatRole::Thinking, "목록부터 본다".to_string()));
}

#[test]
fn paths_stay_inside() {
    let root = dir("root");
    let projects = root.join("projects");
    std::fs::create_dir_all(projects.join("p")).unwrap();
    let ok = projects.join("p/s.jsonl");
    std::fs::write(&ok, "").unwrap();
    assert!(inside(&projects, &ok).is_ok());
    // 밖의 파일, 상대 경로, 확장자, .. 로 빠져나가기
    let outside = root.join("secret.jsonl");
    std::fs::write(&outside, "").unwrap();
    assert!(inside(&projects, &outside).is_err());
    assert!(inside(&projects, Path::new("p/s.jsonl")).is_err());
    assert!(inside(&projects, &projects.join("p/../../secret.jsonl")).is_err());
    let txt = projects.join("p/s.txt");
    std::fs::write(&txt, "").unwrap();
    assert!(inside(&projects, &txt).is_err());
    #[cfg(unix)]
    {
        // 안에 둔 symlink가 밖을 가리키면 거부, 폴더도 거부
        std::os::unix::fs::symlink(&outside, projects.join("p/link.jsonl")).unwrap();
        assert!(inside(&projects, &projects.join("p/link.jsonl")).is_err());
        std::fs::create_dir_all(projects.join("p/d.jsonl")).unwrap();
        assert!(inside(&projects, &projects.join("p/d.jsonl")).is_err());
        // FIFO는 일반 파일이 아니다
        let fifo = projects.join("p/f.jsonl");
        let c = std::ffi::CString::new(fifo.to_string_lossy().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        assert!(inside(&projects, &fifo).is_err());
    }
    // Codex: thread-id 형식, 깊이 안의 rollout 파일만
    let sessions = root.join("sessions");
    // 지어낸 id다. 실제 세션 기록의 id를 시험 값으로 옮기지 않는다(공개 저장소, check-public 「실제 세션 id」)
    let id = "01900000-0000-7000-8000-00000000a001";
    std::fs::create_dir_all(sessions.join("2026/10/17")).unwrap();
    let roll = sessions.join(format!("2026/10/17/rollout-2026-10-17T07-00-00-{id}.jsonl"));
    std::fs::write(&roll, "").unwrap();
    assert_eq!(codex_file(&sessions, id).unwrap(), std::fs::canonicalize(&roll).unwrap());
    assert!(codex_file(&sessions, "../../etc").is_err());
    assert!(codex_file(&sessions, "01900000-0000-7000-8000-00000000a002").is_err());
    std::fs::remove_dir_all(&root).unwrap();
}

/// 정리된 자리(plan §8.309): Claude `isCompactSummary` 사람 줄(10-20 실측 「This session is being continued …」)과 Codex
/// `compacted`는 host가 청할 때만 「정리됨」 줄이다. 청하지 않으면 전처럼 아무것도 싣지 않는다(옛 host는 이 역할을 모른다).
/// 요약 글은 사람 말·받음 증거가 아니다
#[test]
fn compaction_rows_only_when_asked() {
    let root = dir("compaction");
    let path = root.join("s.jsonl");
    let summary = "This session is being continued from a previous conversation that ran out of context.";
    std::fs::write(
        &path,
        [
            serde_json::json!({"type":"user","message":{"content":"앞 질문"}}).to_string(),
            serde_json::json!({"type":"system","subtype":"compact_boundary","content":"Conversation compacted"}).to_string(),
            serde_json::json!({"type":"user","isCompactSummary":true,"message":{"content":[{"type":"text","text":summary}]}}).to_string(),
            serde_json::json!({"type":"user","message":{"content":"뒤 질문"}}).to_string(),
        ]
        .join("\n")
            + "\n",
    )
    .unwrap();
    let roles = |compact| -> Vec<(ChatRole, String)> {
        pages::read(&path, None, |v| line("claude", v, false, compact, false))
            .unwrap()
            .messages
            .into_iter()
            .map(|m| (m.role, m.text))
            .collect()
    };
    assert_eq!(
        roles(true),
        [(ChatRole::User, "앞 질문".into()), (ChatRole::Compact, summary.into()), (ChatRole::User, "뒤 질문".into())]
    );
    assert_eq!(roles(false), [(ChatRole::User, "앞 질문".into()), (ChatRole::User, "뒤 질문".into())]);
    let codex = serde_json::json!({"type":"compacted","payload":{"message":"","replacement_history":[{"type":"message","role":"user"}]}});
    let m = compaction("codex", &codex).unwrap();
    assert_eq!((m.role, m.text.as_str(), m.receipt.is_none()), (ChatRole::Compact, "", true));
    assert!(compaction("claude", &codex).is_none());
    assert!(
        compaction("codex", &serde_json::json!({"type":"user","isCompactSummary":true,"message":{"content":"x"}}))
            .is_none()
    );
    std::fs::remove_dir_all(&root).unwrap();
}

/// 실측(10-20 Claude 2.1.304): 웹에서 보낸 `/context`·`/model`·`/compact`가 남긴 줄과, 앞 판의 알림 줄
#[test]
fn claude_commands_and_their_output_only_when_asked() {
    let echo = |name: &str, args: &str| {
        format!(
            "<command-name>{name}</command-name>\n            <command-message>{}</command-message>\n            <command-args>{args}</command-args>",
            &name[1..]
        )
    };
    let lines = [
        serde_json::json!({"type":"user","message":{"role":"user","content":"Reply with exactly: OK"},"origin":{"kind":"human"}}),
        serde_json::json!({"type":"system","subtype":"local_command","content":echo("/context",""),"level":"info","isMeta":false}),
        serde_json::json!({"type":"system","subtype":"local_command","content":"<local-command-stdout> \u{1b}[1mContext Usage\u{1b}[22m\n\u{1b}[38;2;153;153;153m43.9k/1m tokens (4%)\u{1b}[39m</local-command-stdout>","level":"info","isMeta":false}),
        serde_json::json!({"type":"user","isMeta":true,"message":{"role":"user","content":"## Context Usage\n\n**Tokens:** 43.9k / 1m (4%)"}}),
        serde_json::json!({"type":"system","subtype":"local_command","content":echo("/model","sonnet"),"level":"info","isMeta":false}),
        serde_json::json!({"type":"system","subtype":"local_command","content":"<local-command-stdout>Set model to `Sonnet 5.5`</local-command-stdout>","level":"info","isMeta":false}),
        serde_json::json!({"type":"user","isMeta":true,"message":{"role":"user","content":"<local-command-caveat>The command below was run directly in Claude Code</local-command-caveat>"}}),
        serde_json::json!({"type":"user","message":{"role":"user","content":echo("/compact","")}}),
        serde_json::json!({"type":"user","message":{"role":"user","content":"<local-command-stdout>\u{1b}[2mCompacted (ctrl+o to see full summary)\u{1b}[22m</local-command-stdout>"}}),
        serde_json::json!({"type":"system","subtype":"informational","content":"Unknown command: /ultraplan","level":"info"}),
    ];
    // 실제 읽기 길(쪽 읽기의 칸 고르기까지)로 읽는다
    let root = std::env::temp_dir().join(format!("aaa-chat-commands-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("s.jsonl");
    std::fs::write(&path, lines.iter().map(|v| format!("{v}\n")).collect::<String>()).unwrap();
    let rows = |commands: bool| -> Vec<(ChatRole, String)> {
        read_path("claude", path.clone(), false, false, commands, None)
            .unwrap()
            .messages
            .into_iter()
            .map(|m| (m.role, m.text))
            .collect()
    };
    assert_eq!(
        rows(true),
        [
            (ChatRole::User, "Reply with exactly: OK".into()),
            (ChatRole::Command, "/context".into()),
            (ChatRole::CommandOutput, "Context Usage\n43.9k/1m tokens (4%)".into()),
            (ChatRole::Command, "/model sonnet".into()),
            (ChatRole::CommandOutput, "Set model to `Sonnet 5.5`".into()),
            (ChatRole::CommandOutput, "Unknown command: /ultraplan".into()),
        ]
    );
    // 청하지 않으면(옛 host·front) 지금처럼 사람 말만
    assert_eq!(rows(false), [(ChatRole::User, "Reply with exactly: OK".into())]);
    // Codex는 명령을 기록하지 않는다: 같은 꼴의 줄이 와도 명령 줄로 읽지 않는다
    assert!(line("codex", &lines[1], false, false, true).is_empty());
    assert_eq!(plain("a\u{1b}]8;;http://x\u{7}link\u{1b}]8;;\u{1b}\\b"), "alinkb");
    std::fs::remove_dir_all(&root).unwrap();
}

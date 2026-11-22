use super::*;

#[test]
fn advisor_reads_only_a_string_and_observes_changes_and_removal() {
    let home = std::env::temp_dir().join(format!("aaa-advisor-{}-{}", std::process::id(), rand::random::<u64>()));
    std::fs::create_dir(&home).unwrap();
    let path = home.join("settings.json");
    assert_eq!(claude_advisor_model(&home).unwrap(), None);
    for (input, expected) in [
        (r#"{"advisorModel":"fable","permissions":{"defaultMode":"bypassPermissions"},"hooks":{}}"#, Some("fable")),
        (r#"{"advisorModel":"off"}"#, Some("off")),
        (r#"{"permissions":{}}"#, None),
    ] {
        std::fs::write(&path, input).unwrap();
        assert_eq!(claude_advisor_model(&home).unwrap().as_deref(), expected);
    }
    for input in [r#"{"advisorModel":true}"#, r#"{"advisorModel":null}"#, "[]", "{"] {
        std::fs::write(&path, input).unwrap();
        assert!(claude_advisor_model(&home).is_err());
    }
    std::fs::remove_file(&path).unwrap();
    assert_eq!(claude_advisor_model(&home).unwrap(), None);
    std::fs::create_dir(&path).unwrap();
    assert!(claude_advisor_model(&home).is_err());
    std::fs::remove_dir(&path).unwrap();
    std::fs::remove_dir(&home).unwrap();
}

#[test]
fn image_reply_preserves_encoded_collisions_and_masks_actual_secret_bytes() {
    use aaa_proto::{B64, IMAGE_CHUNK, ImageResponse, Outcome};
    use base64::Engine as _;
    let secret = &*aaa_test_support::token("image-collision");
    aaa_common::redact::remember(secret);
    let collision = base64::engine::general_purpose::STANDARD.decode(format!("AAA{}A", &secret[..8])).unwrap();
    for size in [IMAGE_CHUNK - 1, IMAGE_CHUNK, IMAGE_CHUNK + 1] {
        let mut raw = vec![0; size];
        raw[2838..2838 + collision.len()].copy_from_slice(&collision);
        let original = Outcome::Ok(ImageResponse::Chunk { data: B64(raw.clone()), eof: true });
        let old = aaa_common::redact::text(&serde_json::to_string(&original).unwrap());
        assert!(
            serde_json::from_str::<Outcome<ImageResponse>>(&old)
                .unwrap_err()
                .to_string()
                .contains("Invalid symbol 42, offset 3787")
        );
        let encoded = reply(Ok(Some(Reply::Image(original))));
        let restored: Outcome<ImageResponse> =
            serde_json::from_str(encoded.strip_prefix("ok\nimagegen.v1 ").unwrap()).unwrap();
        let Outcome::Ok(ImageResponse::Chunk { data, eof }) = restored else { panic!("이미지 응답 형식") };
        assert_eq!(data.0, raw);
        assert!(eof);
    }
    // B64라고 가림을 생략하지 않는다. 실제 인증 bytes와 일반 글·오류는 계속 가린다.
    let encoded = reply(Ok(Some(Reply::Image(Outcome::Ok(ImageResponse::Chunk {
        data: B64(secret.as_bytes().to_vec()),
        eof: true,
    })))));
    let restored: Outcome<ImageResponse> =
        serde_json::from_str(encoded.strip_prefix("ok\nimagegen.v1 ").unwrap()).unwrap();
    let Outcome::Ok(ImageResponse::Chunk { data, .. }) = restored else { panic!("이미지 응답 형식") };
    assert_eq!(data.0, vec![b'*'; secret.len()]);
    for out in [
        Ok(Some(Reply::Text(secret.into()))),
        Err(anyhow::anyhow!("hook 실패: {secret}")),
        Ok(Some(Reply::Image(Outcome::Err(aaa_proto::ErrorBody::new(aaa_proto::ErrorCode::BadRequest, secret))))),
    ] {
        let encoded = reply(out);
        assert!(!encoded.contains("cG5nMDkx"));
        assert!(encoded.contains("********"));
    }
}
use serde_json::json;

/// JSON을 읽은 agent가 응답·종료한 뒤 클라이언트가 쓰기 반쪽을 닫는 실제 IPC 차례.
/// macOS는 이 shutdown을 ENOTCONN으로 거부하지만 읽지 않은 응답은 그대로 남긴다.
#[cfg(unix)]
#[tokio::test]
async fn closed_peer_reply_survives_write_shutdown() {
    use std::{
        pin::Pin,
        task::{Context, Poll},
    };
    use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

    struct AfterPeerClosed {
        stream: tokio::net::UnixStream,
        closed: tokio::sync::oneshot::Receiver<()>,
        ready: bool,
    }
    impl AsyncRead for AfterPeerClosed {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            Pin::new(&mut self.stream).poll_read(cx, buf)
        }
    }
    impl AsyncWrite for AfterPeerClosed {
        fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
            Pin::new(&mut self.stream).poll_write(cx, buf)
        }
        fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Pin::new(&mut self.stream).poll_flush(cx)
        }
        fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            if !self.ready {
                match Pin::new(&mut self.closed).poll(cx) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(r) => {
                        r.expect("agent 응답 task");
                        self.ready = true;
                    }
                }
            }
            Pin::new(&mut self.stream).poll_shutdown(cx)
        }
    }
    let msg = HookMsg {
        session: "s1".into(),
        token: aaa_test_support::token("test-hook-secret"),
        harness: "claude".into(),
        payload: json!({"hook_event_name":"UserPromptSubmit"}),
    };
    let guidance = output("UserPromptSubmit", "Waiting for you");
    for reply in [format!("ok\n{guidance}"), "error: 거부\n".into(), String::new()] {
        let (client, server) = tokio::net::UnixStream::pair().unwrap();
        let (closed, wait) = tokio::sync::oneshot::channel();
        let response = reply.clone();
        let peer = tokio::spawn(async move {
            let mut server: aaa_common::ipc::LocalStream = Box::new(server);
            assert_eq!(read_msg(&mut server).await.unwrap().session, "s1");
            server.write_all(response.as_bytes()).await.unwrap();
            drop(server);
            closed.send(()).unwrap();
        });
        let mut client: aaa_common::ipc::LocalStream =
            Box::new(AfterPeerClosed { stream: client, closed: wait, ready: false });
        let result = exchange(&mut client, &serde_json::to_vec(&msg).unwrap()).await;
        peer.await.unwrap();
        if reply.starts_with("ok") {
            assert_eq!(result.unwrap(), Some(guidance.clone()));
        } else {
            assert!(result.is_err(), "빈 응답·거부는 성공이 아니다");
        }
    }
}

#[tokio::test]
async fn hook_json_completion_and_errors_do_not_need_eof() {
    let payload = json!({"body": "한글\"\\\n".repeat(20)});
    let valid = serde_json::to_vec(&HookMsg {
        session: "s1".into(),
        token: aaa_test_support::token("test-hook-secret"),
        harness: "post".into(),
        payload: payload.clone(),
    })
    .unwrap();
    // 작은 버퍼가 JSON·UTF-8을 여러 read로 나눈다. writer는 응답을 기다리듯 열린 채로 남긴다.
    for (bytes, close, expected) in [
        (valid.clone(), false, "완성"),
        (valid, true, "완성"),
        (b"{!".to_vec(), false, "문법"),
        ([b"{\"payload\":\"".as_slice(), &vec![b'x'; MAX_RELAY_MSG]].concat(), false, "한도"),
        (b"{\"session\":".to_vec(), true, "미완성"),
    ] {
        let capacity = if expected == "완성" { 7 } else { MAX_RELAY_MSG + 64 };
        let (mut writer, reader) = tokio::io::duplex(capacity);
        let writing = tokio::spawn(async move {
            writer.write_all(&bytes).await.unwrap();
            if close {
                writer.shutdown().await.unwrap();
            }
            writer
        });
        let mut reader: aaa_common::ipc::LocalStream = Box::new(reader);
        let result = read_msg(&mut reader).await;
        // join 결과의 writer를 여기까지 보존하므로 EOF로 거짓 통과할 수 없다.
        let _writer = writing.await.unwrap();
        match expected {
            "완성" => {
                let msg = result.unwrap();
                assert_eq!((msg.session.as_str(), msg.harness.as_str(), msg.payload), ("s1", "post", payload.clone()));
            }
            "한도" => assert!(result.unwrap_err().to_string().contains("사건이 너무 크다")),
            _ => assert!(result.unwrap_err().downcast_ref::<serde_json::Error>().is_some(), "{expected}"),
        }
    }
}

/// 원격 전달은 기획 문서 저장(512KiB 글)을 싣고, 다른 hook 사건은 예전 한도(256KiB)를 지킨다
#[tokio::test]
async fn relay_messages_carry_design_documents_but_other_hooks_keep_the_old_limit() {
    for (harness, size, ok) in
        [("thread_read", 600 * 1024, true), ("post", 600 * 1024, false), ("thread_read", MAX_RELAY_MSG, false)]
    {
        let body = serde_json::to_vec(&HookMsg {
            session: "s1".into(),
            token: aaa_test_support::token("test-hook-secret"),
            harness: harness.into(),
            payload: json!({ "text": "가".repeat(size / 3) }),
        })
        .unwrap();
        let (mut writer, reader) = tokio::io::duplex(body.len() + 64);
        let writing = tokio::spawn(async move {
            writer.write_all(&body).await.unwrap();
            writer
        });
        let mut reader: aaa_common::ipc::LocalStream = Box::new(reader);
        let result = read_msg(&mut reader).await;
        let _writer = writing.await.unwrap();
        match result {
            Ok(msg) => assert!(ok && msg.harness == harness, "{harness} {size}"),
            Err(e) => assert!(!ok && e.to_string().contains("사건이 너무 크다"), "{harness} {size}: {e}"),
        }
    }
    assert!(aaa_proto::DESIGN_MAX_TEXT as usize * 3 / 2 < MAX_RELAY_MSG, "기획 글 한도와 JSON 감쌈이 들어간다");
}

#[test]
fn subagent_and_foreign_session_events_are_dropped() {
    // herdr-projects progress.rs eligible·foreign_session 시험과 같은 경우
    let n = |p: Value| normalize("S", "claude", &p, 1);
    assert!(n(json!({"hook_event_name":"UserPromptSubmit","session_id":"0f5b2c1e-7a3d-4c8e-9b21-5d6e7f809a1b","agent_id":"x"})).is_none());
    assert!(
        n(json!({"hook_event_name":"UserPromptSubmit","session_id":"0f5b2c1e-7a3d-4c8e-9b21-5d6e7f809a1b","transcript_path":"/p/subagents/x.jsonl"}))
            .is_none()
    );
    assert!(n(json!({"hook_event_name":"UserPromptSubmit","session_id":"0f5b2c1e-7a3d-4c8e-9b21-5d6e7f809a1b","agent_id":null})).is_some());
    let other =
        n(json!({"hook_event_name":"UserPromptSubmit","session_id":"7d0c9b8a-6f5e-4d3c-8b2a-1f0e9d8c7b6a"})).unwrap();
    assert!(foreign_session(Some("0f5b2c1e-7a3d-4c8e-9b21-5d6e7f809a1b"), &other));
    assert!(!foreign_session(Some("7d0c9b8a-6f5e-4d3c-8b2a-1f0e9d8c7b6a"), &other));
    assert!(!foreign_session(None, &other));
    let start =
        n(json!({"hook_event_name":"SessionStart","session_id":"7d0c9b8a-6f5e-4d3c-8b2a-1f0e9d8c7b6a"})).unwrap();
    assert!(!foreign_session(Some("0f5b2c1e-7a3d-4c8e-9b21-5d6e7f809a1b"), &start));
}

#[test]
fn hooks_only_link_sessions_and_never_set_state() {
    let n = |h: &str, p: Value| normalize("S", h, &p, 1);
    let e = n("claude", json!({"hook_event_name":"SessionStart","session_id":"0f5b2c1e-7a3d-4c8e-9b21-5d6e7f809a1b","transcript_path":"/t.jsonl"}))
        .unwrap();
    assert_eq!(
        (e.kind.as_str(), e.harness_session.as_deref()),
        ("session_start", Some("0f5b2c1e-7a3d-4c8e-9b21-5d6e7f809a1b"))
    );
    assert_eq!(e.transcript.as_deref(), Some("/t.jsonl"));
    let p = n(
        "claude",
        json!({"hook_event_name":"UserPromptSubmit","session_id":"0f5b2c1e-7a3d-4c8e-9b21-5d6e7f809a1b","prompt":"x"}),
    )
    .unwrap();
    assert_eq!(p.kind, "prompt_submit");
    // UUID 꼴이 아닌 세션 ID는 버린다(보안 감사 t-0340: `--resume` 인자가 된다). 사건은 그대로 받는다
    for bad in ["--dangerously-skip-permissions", "x; touch pwned", "abc"] {
        let e = n("claude", json!({"hook_event_name":"SessionStart","session_id":bad})).unwrap();
        assert_eq!((e.kind.as_str(), e.harness_session.as_deref()), ("session_start", None), "{bad}");
    }
    // 옛 agent가 띄운 세션의 상태 hook은 버린다: 상태는 화면 감지가 정한다
    for old in [
        json!({"hook_event_name":"Stop","session_id":"0f5b2c1e-7a3d-4c8e-9b21-5d6e7f809a1b"}),
        json!({"hook_event_name":"PermissionRequest","tool_name":"Bash","tool_input":{"command":"rm x"}}),
        json!({"hook_event_name":"Notification","notification_type":"permission_prompt","message":"m"}),
        json!({"hook_event_name":"Notification","notification_type":"idle_prompt"}),
        json!({"hook_event_name":"PostToolUse","session_id":"0f5b2c1e-7a3d-4c8e-9b21-5d6e7f809a1b"}),
        json!({"hook_event_name":"SessionEnd","session_id":"0f5b2c1e-7a3d-4c8e-9b21-5d6e7f809a1b"}),
        json!({"x":1}),
    ] {
        assert!(n("claude", old.clone()).is_none(), "{old}");
    }
    let c =
        n("codex", json!({"type":"agent-turn-complete","thread-id":"019a4c5e-1f2b-7c3d-8e4f-a5b6c7d8e9f0","last-assistant-message":"done"})).unwrap();
    assert_eq!(
        (c.kind.as_str(), c.harness_session.as_deref(), c.message),
        ("session", Some("019a4c5e-1f2b-7c3d-8e4f-a5b6c7d8e9f0"), None)
    );
    let r = n("report", json!({"percent":250,"activity":"테스트 중\n이상한 줄"})).unwrap();
    assert_eq!((r.kind.as_str(), r.percent), ("progress", Some(100)));
    assert_eq!(r.message.as_deref(), Some("테스트 중이상한 줄"));
}

/// herdr `tests/cli/hooks.rs`의 Codex 세션 시험과 같은 경우들
#[test]
fn codex_events_of_nested_threads_are_not_sent() {
    let notify = |id: &str| json!({"type":"agent-turn-complete","thread-id":id});
    assert!(!nested_codex(None, &notify("a")));
    assert!(!nested_codex(Some(""), &notify("a")));
    assert!(!nested_codex(Some("a"), &notify("a")));
    assert!(nested_codex(Some("parent"), &notify("nested")));
    assert!(!nested_codex(Some("s"), &json!({"hook_event_name":"SessionStart","session_id":"s"})));
    assert!(nested_codex(Some("s"), &json!({"type":"agent-turn-complete"})));
}

#[test]
fn settings_quote_exe_and_register_session_and_guidance_hooks() {
    let v = claude_settings(Path::new("/A b/it's/aaa"));
    let cmd = v["hooks"]["SessionStart"][0]["hooks"][0]["command"].as_str().unwrap();
    assert_eq!(cmd, "'/A b/it'\\''s/aaa' hook --agent claude");
    assert_eq!(v["hooks"]["SessionStart"][0]["matcher"], "^(startup|resume|clear|compact|fork)$");
    let mut events: Vec<&str> = v["hooks"].as_object().unwrap().keys().map(String::as_str).collect();
    events.sort();
    // 상태 hook(Stop·PermissionRequest·Notification)은 없다. PostToolUse는 진행 알림만 한다(herdr-projects setup.rs)
    assert_eq!(events, ["PostToolUse", "SessionStart", "UserPromptSubmit"]);
    // 다음 입력 예측은 끈다(q-4042 1A): 빠지거나 true면 Claude가 예측을 그린다
    assert_eq!(v["promptSuggestionEnabled"], serde_json::Value::Bool(false));
}

/// herdr-projects progress.rs `helpers_and_the_reporters_own_call_do_not_trigger`. PostToolUse는 안내만 하고 host 사건은
/// 아니다(상태는 화면 감지가 정한다).
#[test]
fn helpers_and_the_reporters_own_call_get_no_guidance() {
    assert_eq!(guide_kind(&json!({"hook_event_name":"PostToolUse","agent_id":"child"})), None);
    assert_eq!(guide_kind(&json!({"hook_event_name":"PostToolUse","transcript_path":"/x/subagents/y.jsonl"})), None);
    let own =
        json!({"hook_event_name":"PostToolUse","tool_input":{"command":"/p/aaa report --percent 5 --activity x"}});
    assert_eq!(guide_kind(&own), None);
    let other = json!({"hook_event_name":"PostToolUse","tool_input":{"command":"/p/aaa thread list demo"}});
    assert_eq!(guide_kind(&other), Some("PostToolUse"));
    assert!(normalize("S", "claude", &other, 1).is_none(), "PostToolUse가 host 사건이 됐다");
    assert_eq!(guide_kind(&json!({"hook_event_name":"SessionStart","source":"compact"})), Some("SessionStart"));
    assert_eq!(guide_kind(&json!({"hook_event_name":"Stop"})), None);
}

#[test]
fn automation_reentry_is_session_start_context_only() {
    let mut rec = Progress::default();
    let reentry = "자동화 기억 재독: /work/.aaa/demo-t-0001/memory/memory.md";
    let reader = Reader { thread: true, reentry: Some(reentry) };
    for kind in ["SessionStart", "UserPromptSubmit", "PostToolUse"] {
        let text = respond(&mut rec, kind, "aaa", 1_000_000, reader, false).unwrap_or_default();
        assert_eq!(text.contains(reentry), kind == "SessionStart", "{kind}: {text}");
    }
}

/// herdr-projects progress.rs `session_start_injects_instructions_and_resets_the_record`·
/// `reminders_are_throttled_to_once_a_minute_and_stop_at_done`(초를 ms로)
#[test]
fn session_start_guides_and_reminders_are_throttled_to_once_a_minute() {
    let mut rec = Progress { percent: Some(50), reported_at: 5, auto_mode: Some("on".into()), ..Progress::default() };
    let thread = Reader { thread: true, reentry: None };
    let text = respond(&mut rec, "SessionStart", "/p/aaa", 1_000_000, thread, false).unwrap();
    assert!(text.contains("`/p/aaa report --percent 25"), "{text}");
    assert!(!text.contains("자동화 기억"), "일반 thread에 기억 재독 안내가 섞였다: {text}");
    assert_eq!(rec, Progress { reminded_at: 1_000_000, ..Progress::default() });

    assert!(respond(&mut rec, "PostToolUse", "aaa", 1_030_000, thread, false).is_none());
    assert!(respond(&mut rec, "PostToolUse", "aaa", 1_061_000, thread, false).is_some());
    assert_eq!(rec.reminded_at, 1_061_000);
    // 새 보고도 1분 동안 알림을 멈춘다
    rec.reported_at = 1_100_000;
    assert!(respond(&mut rec, "PostToolUse", "aaa", 1_130_000, thread, false).is_none());
    assert!(respond(&mut rec, "PostToolUse", "aaa", 1_200_000, thread, false).is_some());
    rec.percent = Some(100);
    assert!(respond(&mut rec, "PostToolUse", "aaa", 9_000_000, thread, false).is_none());
    assert!(
        respond(&mut rec, "UserPromptSubmit", "aaa", 9_001_000, thread, false).unwrap().contains("Waiting for you")
    );
    assert!(respond(&mut rec, "Stop", "aaa", 9_002_000, thread, false).is_none());
    let out: Value = serde_json::from_str(&output("PostToolUse", "hi")).unwrap();
    assert_eq!(out, json!({"hookSpecificOutput":{"hookEventName":"PostToolUse","additionalContext":"hi"}}));
}

/// 진행 안내는 역할별이다: thread만 report.md `## 진행` 줄을 받고, 외부 푸시 정책은 둘 다 받지 않는다(coordinator
/// 안내에 있다). aaa가 친 입력(inbox 알림)에는 매 입력 알림을 넣지 않고 기록도 바꾸지 않는다.
#[test]
fn guidance_follows_the_reader_and_skips_prompts_typed_by_aaa() {
    let mut rec = Progress::default();
    let coordinator = respond(&mut rec, "SessionStart", "aaa", 1, Reader::default(), false).unwrap();
    let thread = respond(&mut rec, "SessionStart", "aaa", 1, Reader { thread: true, reentry: None }, false).unwrap();
    assert!(!coordinator.contains("`## 진행`") && thread.contains("`## 진행`"), "{coordinator}\n{thread}");
    assert!(!thread.contains("외부 푸시") && !coordinator.contains("외부 푸시"));
    let nudge =
        json!({"hook_event_name":"UserPromptSubmit","prompt":"[aaa inbox] t-0001 new report (aaa inbox list demo)"});
    let person = json!({"hook_event_name":"UserPromptSubmit","prompt":"배포해 줘"});
    assert!(
        typed_by_aaa(&nudge) && !typed_by_aaa(&person) && !typed_by_aaa(&json!({"hook_event_name":"UserPromptSubmit"}))
    );
    let before = rec.clone();
    assert_eq!(respond(&mut rec, "UserPromptSubmit", "aaa", 120_000, Reader::default(), true), None);
    assert_eq!(rec, before, "알림을 넣지 않은 입력은 다음 알림 시각을 바꾸지 않는다");
    let prompt = respond(&mut rec, "UserPromptSubmit", "aaa", 120_000, Reader::default(), false).unwrap();
    assert!(prompt.contains("새 일을") && prompt.contains("Waiting for you") && prompt.len() < 200, "{prompt}");
}

/// coordinator auto mode 안내는 SessionStart(compaction 포함)와 상태가 바뀐 뒤 첫 입력에만 들어간다. 같은 상태의 inbox
/// 알림에는 아무것도 넣지 않는다(E2E100은 compaction hook에 `auto_mode: on`이 있는지 본다)
#[test]
fn auto_mode_is_added_at_session_start_and_when_it_changes() {
    let mut rec = Progress::default();
    let reader = Reader::default();
    let mut hook = |kind: &str, by_aaa: bool, state: &str| {
        let progress = respond(&mut rec, kind, "aaa", 1, reader, by_aaa);
        with_auto_mode(&mut rec, progress, Some((state, &format!("aaa coordinator auto_mode: {state}"))))
    };
    let start = hook("SessionStart", false, "off").unwrap();
    assert!(start.contains("# 진행 보고(aaa)") && start.ends_with("aaa coordinator auto_mode: off"), "{start}");
    assert_eq!(hook("UserPromptSubmit", true, "off"), None, "같은 상태의 inbox 알림");
    assert!(!hook("UserPromptSubmit", false, "off").unwrap().contains("auto_mode"), "같은 상태의 사람 입력");
    assert_eq!(hook("UserPromptSubmit", true, "on").as_deref(), Some("aaa coordinator auto_mode: on"));
    assert_eq!(hook("UserPromptSubmit", true, "on"), None);
    assert!(hook("SessionStart", false, "on").unwrap().ends_with("aaa coordinator auto_mode: on"), "compaction 뒤");
}

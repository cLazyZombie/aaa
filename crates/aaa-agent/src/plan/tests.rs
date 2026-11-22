use super::*;
use serde_json::json;
use std::path::Path;

fn file(tag: &str, lines: &[Value]) -> PathBuf {
    let d = std::env::temp_dir().join(format!("aaa-plan-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let p = d.join("s.jsonl");
    std::fs::write(&p, lines.iter().map(|l| format!("{l}\n")).collect::<String>()).unwrap();
    p
}

fn append(p: &Path, lines: &[Value]) {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().append(true).open(p).unwrap();
    for l in lines {
        writeln!(f, "{l}").unwrap();
    }
}

#[test]
fn native_errors_wait_for_complete_jsonl_and_match_the_current_execution() {
    use std::io::Write;
    let id = "00000000-0000-4000-8000-000000000001";
    let p = file(
        "native-errors",
        &[
            json!({"timestamp":T,"type":"session_meta","payload":{"id":id}}),
            json!({"timestamp":T,"type":"event_msg","payload":{"type":"task_started","turn_id":"one"}}),
        ],
    );
    let mut r = Reader::new(Source::Codex, p.clone());
    poll(&mut r);
    let at = aaa_common::util::parse_iso(T).unwrap();
    assert_eq!(r.provider_turn(at, Some(id)).unwrap().error, None);
    let finish = json!({"timestamp":T,"type":"event_msg","payload":{"type":"task_complete","turn_id":"one","error":{"codex_error_info":{"http_connection_failed":{"http_status_code":401}}}}}).to_string();
    let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
    f.write_all(finish.as_bytes()).unwrap();
    poll(&mut r);
    assert_eq!(r.provider_turn(at, Some(id)), None, "완성된 줄까지 읽지 못했으면 관측을 보내지 않는다");
    f.write_all(b"\n").unwrap();
    poll(&mut r);
    let error = r.provider_turn(at, Some(id)).unwrap().error.unwrap();
    assert_eq!(error.kind, aaa_proto::ProviderFailureKind::Authentication);
    assert_eq!(error.http_status, Some(401));
    assert_eq!(r.provider_turn(at + 1, Some(id)), None, "지난 실행의 turn");
    assert_eq!(r.provider_turn(at, Some("00000000-0000-4000-8000-000000000002")), None, "다른 세션");
    drop(f);
    std::fs::remove_dir_all(p.parent().unwrap()).unwrap();
}

#[test]
fn turn_replies_follow_complete_appends_without_plan_changes() {
    use std::io::Write;
    for (tag, source, start, finish) in [
        (
            "reply-codex",
            Source::Codex,
            json!({"type":"event_msg","timestamp":T,"payload":{"type":"task_started"}}),
            json!({"type":"event_msg","timestamp":T,"payload":{"type":"task_complete","last_agent_message":"진행할까요?"}}),
        ),
        (
            "reply-claude",
            Source::Claude,
            json!({"type":"user","timestamp":T,"message":{"content":"시작"}}),
            json!({"type":"assistant","timestamp":T,"message":{"stop_reason":"end_turn","content":[{"type":"text","text":"진행할까요?"}]}}),
        ),
    ] {
        let p = file(tag, std::slice::from_ref(&start));
        let mut r = Reader::new(source, p.clone());
        assert_eq!(poll(&mut r), Snapshot::default());
        assert!(r.turns.completed.is_empty());
        let encoded = finish.to_string();
        let split = encoded.len() / 2;
        let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
        f.write_all(&encoded.as_bytes()[..split]).unwrap();
        assert_eq!(poll(&mut r), Snapshot::default());
        assert!(r.turns.completed.is_empty(), "미완성 JSONL은 완료가 아니다");
        f.write_all(&encoded.as_bytes()[split..]).unwrap();
        f.write_all(b"\n").unwrap();
        assert_eq!(poll(&mut r), Snapshot::default());
        if source == Source::Claude {
            assert!(r.turns.completed.is_empty(), "end_turn 뒤 Stop hook이 계속할 수 있다");
            append(&p, &[json!({"type":"system","timestamp":T,"subtype":"turn_duration"})]);
            poll(&mut r);
        }
        assert_eq!(r.turns.completed.remove(0).text, "진행할까요?");
        poll(&mut r);
        assert!(r.turns.completed.is_empty(), "같은 append를 두 번 읽지 않는다");
        drop(f);
        std::fs::remove_dir_all(p.parent().unwrap()).unwrap();
    }
}

const T: &str = "2026-10-18T09:00:00.000Z";

fn call(id: &str, name: &str, input: Value) -> Value {
    json!({"type":"assistant","timestamp":T,"message":{"content":[{"type":"tool_use","id":id,"name":name,"input":input}]}})
}

fn answer(id: &str, text: &str, result: Value, error: bool) -> Value {
    json!({"type":"user","timestamp":T,"toolUseResult":result,"message":{"content":[{"type":"tool_result","tool_use_id":id,"content":text,"is_error":error}]}})
}

fn poll(r: &mut Reader) -> Snapshot {
    r.poll(&mut POLL_BUDGET.clone(), 0).unwrap()
}

fn plan(done: u32, total: u32, current: Option<&str>) -> Option<PlanSummary> {
    Some(PlanSummary { done, total, current: current.map(str::to_string) })
}

#[test]
fn model_observation_follows_turn_changes_and_excludes_previous_processes() {
    let turn = |model: &str, effort: Option<&str>| json!({"type":"turn_context","timestamp":T,"payload":{"model":model,"effort":effort}});
    let p = file("model-codex", &[turn("gpt-6-sol", Some("high"))]);
    let mut r = Reader::new(Source::Codex, p.clone());
    assert_eq!(poll(&mut r).model_observation.unwrap().model, "gpt-6-sol");
    append(&p, &[turn("gpt-6-astra", Some("medium"))]);
    let m = poll(&mut r).model_observation.unwrap();
    assert_eq!((&*m.model, m.effort.as_deref()), ("gpt-6-astra", Some("medium")));
    // resume로 같은 기록을 읽어도 옛 프로세스의 선택을 현재 모델이라고 하지 않는다.
    assert_eq!(r.poll(&mut POLL_BUDGET.clone(), m.at_ms as i64 + 1).unwrap().model_observation, None);
    append(&p, &[turn("gpt-6-astra", None)]);
    assert_eq!(poll(&mut r).model_observation.unwrap().effort, None);
    append(&p, &[turn("", None)]);
    assert_eq!(poll(&mut r).model_observation, None);
}

#[test]
fn model_observation_uses_only_main_claude_answers_without_guessing_effort() {
    let answer =
        |model| json!({"type":"assistant","timestamp":T,"requestedModel":model,"message":{"usage":{"input_tokens":1}}});
    let p = file("model-claude", &[answer("claude-opus-5-5")]);
    let mut r = Reader::new(Source::Claude, p.clone());
    let original = poll(&mut r).model_observation.unwrap();
    assert_eq!((&*original.model, original.effort), ("claude-opus-5-5", None));
    let mut side = answer("claude-haiku-5-5");
    side["isSidechain"] = json!(true);
    append(&p, &[side]);
    assert_eq!(poll(&mut r).model_observation.unwrap().model, "claude-opus-5-5");
    append(&p, &[answer("claude-sonnet-5-5")]);
    assert_eq!(poll(&mut r).model_observation.unwrap().model, "claude-sonnet-5-5");
}

#[test]
fn model_observation_takes_claude_effort_from_documented_hook_input() {
    let dir = std::env::temp_dir().join(format!("aaa-plan-effort-{}", aaa_common::util::new_id()));
    let a = crate::Agent::new(dir.clone(), "m".into(), "mac-a".into(), true, None).unwrap();
    let main = "0f5b2c1e-7a3d-4c8e-9b21-5d6e7f809a1b";
    let hook = |id: &str, level: Option<&str>| {
        let mut p = json!({"hook_event_name":"PostToolUse","session_id":id});
        if let Some(l) = level {
            p["effort"] = json!({"level":l});
        }
        p
    };
    let seen = |a: &crate::Agent| a.claude_effort.lock().unwrap().get("s").cloned();
    a.harness_started.lock().unwrap().insert("s".into(), main.into());
    a.observe_claude_effort("s", &hook(main, Some("high")), 10);
    // 상태줄은 같은 값으로 자주 온다: 시각을 옮기지 않아 같은 관측을 다시 보내지 않는다
    a.observe_claude_effort("s", &hook(main, Some("high")), 20);
    assert_eq!(seen(&a), Some(ClaudeEffort { level: Some("high".into()), at_ms: 10 }));
    // 하위 agent·다른 harness 세션의 수준은 본 세션 것이 아니다
    let mut side = hook(main, Some("low"));
    side["agent_id"] = json!("x");
    a.observe_claude_effort("s", &side, 30);
    a.observe_claude_effort("s", &hook("7d0c9b8a-6f5e-4d3c-8b2a-1f0e9d8c7b6a", Some("low")), 30);
    a.observe_claude_effort("s", &json!({"session_id":main,"effort":{"level":"max"}}), 40);
    let mut m = ModelObservation { model: "claude-opus-5-5".into(), effort: None, at_ms: 5, speed: None };
    seen(&a).unwrap().apply(&mut m);
    assert_eq!((m.effort.as_deref(), m.at_ms), (Some("max"), 40));
    // effort를 받지 않는 모델로 바꾸면 칸이 없다: 옛 수준을 남기지 않는다
    a.observe_claude_effort("s", &hook(main, None), 50);
    seen(&a).unwrap().apply(&mut m);
    assert_eq!(m.effort, None);
    a.observe_claude_effort("s", &hook(main, Some("high")), 60);
    a.harness_process_started("s", 70);
    assert_eq!(seen(&a), None);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn model_observation_takes_speed_from_claude_statusline_and_codex_title() {
    let dir = std::env::temp_dir().join(format!("aaa-plan-speed-{}", aaa_common::util::new_id()));
    let a = crate::Agent::new(dir.clone(), "m".into(), "mac-a".into(), true, None).unwrap();
    let main = "0f5b2c1e-7a3d-4c8e-9b21-5d6e7f809a1b";
    let seen = |a: &crate::Agent, s: &str| a.harness_speed.lock().unwrap().get(s).cloned();
    let speed = |a: &crate::Agent, s: &str| {
        let mut m = ModelObservation { model: "m".into(), effort: None, at_ms: 5, speed: None };
        seen(a, s).unwrap().apply(&mut m);
        (m.speed, m.at_ms)
    };
    a.harness_started.lock().unwrap().insert("s".into(), main.into());
    // hook 입력에는 fast 칸이 없다(공식 공통 입력): 속도를 만들지 않는다
    a.observe_claude_effort(
        "s",
        &json!({"hook_event_name":"PostToolUse","session_id":main,"effort":{"level":"high"}}),
        10,
    );
    assert_eq!(seen(&a, "s"), None);
    a.observe_claude_effort("s", &json!({"session_id":main,"fast_mode":true}), 20);
    // 상태줄은 같은 값으로 자주 온다: 시각을 옮기지 않는다
    a.observe_claude_effort("s", &json!({"session_id":main,"fast_mode":true}), 30);
    assert_eq!(speed(&a, "s"), (Some("fast".into()), 20));
    // 하위 agent·다른 harness 세션의 입력과 칸 없는 hook은 본 세션 값을 바꾸지 않는다
    a.observe_claude_effort("s", &json!({"session_id":main,"agent_id":"x","fast_mode":false}), 40);
    a.observe_claude_effort("s", &json!({"session_id":"7d0c9b8a-6f5e-4d3c-8b2a-1f0e9d8c7b6a","fast_mode":false}), 40);
    a.observe_claude_effort("s", &json!({"hook_event_name":"PostToolUse","session_id":main}), 40);
    assert_eq!(speed(&a, "s"), (Some("fast".into()), 20));
    a.observe_claude_effort("s", &json!({"session_id":main,"fast_mode":false}), 50);
    assert_eq!(speed(&a, "s"), (None, 50));
    a.harness_process_started("s", 60);
    assert_eq!(seen(&a, "s"), None);

    // Codex 0.166 제목(aaa가 fast-mode를 끝에 둔다)
    use aaa_proto::CodexSpeed;
    assert_eq!(a.observe_codex_title("new", "Ready | shop | Standard", 90), Some(CodexSpeed::Standard));
    assert_eq!(a.observe_codex_title("c", "Working | ⠋ | shop | Fast", 100), Some(CodexSpeed::Fast));
    assert_eq!(a.observe_codex_title("c", "Ready | shop | Fast", 105), None);
    assert_eq!(speed(&a, "c"), (Some("fast".into()), 100));
    // 제목이 아직 없거나 속도 항목이 아니면 앞 값을 둔다
    assert_eq!(a.observe_codex_title("c", "Fast", 110), None);
    assert_eq!(a.observe_codex_title("c", "Ready | shop", 110), None);
    assert_eq!(speed(&a, "c"), (Some("fast".into()), 100));
    assert_eq!(a.observe_codex_title("c", "Ready | shop | Standard", 120), Some(CodexSpeed::Standard));
    assert_eq!(speed(&a, "c"), (None, 120));
    assert_eq!(a.observe_codex_title("c", "Ready | shop | Ultrafast", 130), Some(CodexSpeed::Ultrafast));
    assert_eq!(speed(&a, "c"), (Some("ultrafast".into()), 130));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn claude_tasks_make_a_plan_from_answered_calls_only() {
    let p = file(
        "claude",
        &[
            call("u1", "TaskCreate", json!({"subject":"읽기","activeForm":"읽는 중"})),
            answer("u1", "Task #1 created successfully: 읽기", json!({"task":{"id":"1"}}), false),
            call("u2", "TaskCreate", json!({"subject":"고치기"})),
            answer("u2", "Task #2 created successfully: 고치기", json!(null), false),
            // 오류로 끝난 호출은 단계가 아니다
            call("u3", "TaskCreate", json!({"subject":"실패"})),
            answer("u3", "error", json!(null), true),
            call("u4", "TaskUpdate", json!({"taskId":"1","status":"in_progress"})),
            answer("u4", "Updated", json!({"success":true}), false),
        ],
    );
    let mut r = Reader::new(Source::Claude, p.clone());
    assert_eq!(poll(&mut r).plan, plan(0, 2, Some("읽는 중")));
    // 답이 아직 없는 호출은 바꾸지 않는다
    append(&p, &[call("u5", "TaskUpdate", json!({"taskId":"1","status":"completed"}))]);
    assert_eq!(poll(&mut r).plan, plan(0, 2, Some("읽는 중")));
    append(&p, &[answer("u5", "Updated", json!({"success":true}), false)]);
    assert_eq!(poll(&mut r).plan, plan(1, 2, None));
    // 실패한 갱신·지운 단계
    append(
        &p,
        &[
            call("u6", "TaskUpdate", json!({"taskId":"2","status":"completed"})),
            answer("u6", "no", json!({"success":false}), false),
            call("u7", "TaskUpdate", json!({"taskId":"2","status":"deleted"})),
            answer("u7", "Updated", json!({"success":true}), false),
        ],
    );
    assert_eq!(poll(&mut r).plan, plan(1, 1, None));
    // 모두 끝난 뒤 처음 만든 단계는 새 계획이다
    append(
        &p,
        &[
            call("u8", "TaskCreate", json!({"subject":"다음"})),
            answer("u8", "Task #3 created successfully: 다음", json!(null), false),
        ],
    );
    assert_eq!(poll(&mut r).plan, plan(0, 1, None));
}

#[test]
fn codex_plan_is_the_last_update_plan() {
    let up = |items: Value| json!({"type":"response_item","payload":{"type":"function_call","name":"update_plan","arguments":json!({"plan":items}).to_string()}});
    let p = file(
        "codex",
        &[
            up(json!([{"step":"a","status":"in_progress"},{"step":"b","status":"pending"}])),
            // 명령이 계획을 인용한 것은 호출이 아니다
            json!({"type":"response_item","payload":{"type":"function_call","name":"exec","arguments":"update_plan"}}),
            up(
                json!([{"step":"a","status":"completed"},{"step":"b","status":"in_progress"},{"step":"c","status":"pending"}]),
            ),
        ],
    );
    let mut r = Reader::new(Source::Codex, p);
    assert_eq!(poll(&mut r).plan, plan(1, 3, Some("b")));
}

#[test]
fn background_counts_agents_and_commands_until_their_notice() {
    let note = |task: &str, tool: &str| {
        format!(
            "<task-notification>\n<task-id>{task}</task-id>\n<tool-use-id>{tool}</tool-use-id>\n<status>completed</status>\n<summary>끝</summary>\n</task-notification>"
        )
    };
    let p = file(
        "bg",
        &[
            // 배경 명령
            call("b1", "Bash", json!({"command":"cargo test","run_in_background":true})),
            answer("b1", "Command running in background with ID: bg1", json!({"backgroundTaskId":"bg1"}), false),
            // 배경 하위 agent
            call("a1", "Agent", json!({"description":"찾기","run_in_background":true})),
            answer(
                "a1",
                "Async agent launched successfully.",
                json!({"status":"async_launched","agentId":"ag1"}),
                false,
            ),
            // 동기 하위 agent(답이 오면 끝)
            call("a2", "Agent", json!({"description":"읽기"})),
        ],
    );
    let mut r = Reader::new(Source::Claude, p.clone());
    assert_eq!(poll(&mut r).background, Some(3));
    append(&p, &[answer("a2", "결과", json!({"status":"completed"}), false)]);
    assert_eq!(poll(&mut r).background, Some(2));
    // 알림은 세 자리에 실려 온다: queue-operation, 사람 쪽 글. 도구 답이 인용한 것은 알림이 아니다
    append(
        &p,
        &[
            answer("zz", &note("bg1", "b1"), json!(null), false),
            json!({"type":"queue-operation","operation":"enqueue","timestamp":T,"content":note("ag1", "a1")}),
        ],
    );
    assert_eq!(poll(&mut r).background, Some(1));
    append(&p, &[json!({"type":"user","timestamp":T,"message":{"content":note("bg1", "b1")}})]);
    assert_eq!(poll(&mut r).background, None);
    // TaskStop으로 멈춘 명령, 지금 프로세스보다 먼저 띄운 명령
    append(
        &p,
        &[
            call("b2", "Bash", json!({"command":"sleep 99"})),
            answer("b2", "running", json!({"backgroundTaskId":"bg2"}), false),
            call("s1", "TaskStop", json!({"task_id":"bg2"})),
            answer("s1", "stopped", json!({"task_id":"bg2","task_type":"local_bash"}), false),
            call("b3", "Bash", json!({"command":"sleep 99"})),
            answer("b3", "running", json!({"backgroundTaskId":"bg3"}), false),
        ],
    );
    assert_eq!(poll(&mut r).background, Some(1));
    let later = stamp_ms(T).unwrap() + 1;
    assert_eq!(r.poll(&mut POLL_BUDGET.clone(), later).unwrap().background, None);
}

#[test]
fn first_read_waits_for_the_end_and_long_lines_are_skipped() {
    let blob = json!({"type":"api-request-blob","data":"x".repeat((READ_BUDGET + 10) as usize)});
    let p = file(
        "long",
        &[
            blob,
            call("u1", "TaskCreate", json!({"subject":"한 단계"})),
            answer("u1", "Task #1 created successfully: x", json!(null), false),
        ],
    );
    let mut r = Reader::new(Source::Claude, p);
    // 한 번에 다 못 읽으면 말하지 않는다
    assert_eq!(r.poll(&mut 1024, 0), None);
    assert_eq!(poll(&mut r).plan, plan(0, 1, None));
}

#[test]
fn stamps_are_utc_ms() {
    assert_eq!(stamp_ms("1970-01-01T00:00:00Z"), Some(0));
    assert_eq!(stamp_ms("2026-10-18T09:17:01.123Z"), Some(1_792_315_021_123));
    assert_eq!(stamp_ms("2000-02-29T12:00:00.5Z"), Some(951_825_600_500));
    assert_eq!(stamp_ms("2026-10-18 09:17:01Z"), None);
}

/// 실측 꼴(10-20 Claude 2.1.304 /compact): 본 대화 응답의 usage 합, `requestedModel`의 창, 정리 끝의 전후 크기
#[test]
fn claude_context_is_the_last_main_answer_and_the_compaction_sizes() {
    let said = |model: &str, input: u64, created: u64, read: u64| {
        json!({"type":"assistant","requestedModel":model,"isSidechain":false,"message":{"model":"claude-haiku-5-5","content":[{"type":"text","text":"OK"}],
            "usage":{"input_tokens":input,"cache_creation_input_tokens":created,"cache_read_input_tokens":read,"output_tokens":4}}})
    };
    let p =
        file("claude-context", &[json!({"type":"user","message":{"role":"user","content":"Reply with exactly: OK"}})]);
    let mut r = Reader::new(Source::Claude, p.clone());
    // 사용자 설정이 없는 Claude(실제 ~/.claude는 읽지 않는다)
    r.claude_settings = p.with_extension("settings.json");
    // 아직 응답이 없다
    assert_eq!(poll(&mut r).context, None);
    // 실측(10-20 Claude 2.1.304 Haiku 5.5): `/context`가 「43.9k/1m tokens (4%)」였다. Claude 5 모델은 `[1m]` 없이도 1M이다
    append(&p, &[said("claude-haiku-5-5", 2, 11_748, 32_172)]);
    let c = poll(&mut r).context.unwrap();
    assert_eq!((c.used, c.window, c.percent, c.compaction), (43_922, Some(1_000_000), Some(4), None));
    // 하위 agent의 응답·합성 줄(usage 0)은 이 세션의 컨텍스트가 아니다
    let mut side = said("claude-haiku-5-5", 1, 0, 150_000);
    side["isSidechain"] = json!(true);
    let synthetic = json!({"type":"assistant","message":{"model":"<synthetic>","content":[],"usage":{"input_tokens":0,"output_tokens":0}}});
    append(&p, &[side, synthetic]);
    assert_eq!(poll(&mut r).context.unwrap().used, 43_922);
    // 정리가 끝나면 다음 응답 전에도 정리 뒤 크기다
    append(
        &p,
        &[json!({"type":"system","subtype":"compact_boundary","content":"Conversation compacted",
        "compactMetadata":{"trigger":"manual","preTokens":43_926,"postTokens":7_910,"durationMs":7_100}})],
    );
    let c = poll(&mut r).context.unwrap();
    assert_eq!(
        (c.used, c.percent, c.compaction),
        (7_910, Some(1), Some(ContextCompaction { before: 43_926, after: 7_910 }))
    );
    // `[1m]`, Claude 5 앞 모델의 200k, 창을 넘은 사용(창 추정이 틀린 경우: %가 없다), 창을 모르는 모델
    append(&p, &[said("claude-opus-5-5[1m]", 2, 986, 89_361)]);
    let c = poll(&mut r).context.unwrap();
    assert_eq!((c.used, c.window, c.percent), (90_349, Some(1_000_000), Some(9)));
    append(&p, &[said("claude-haiku-4-5", 2, 998, 75_000)]);
    let c = poll(&mut r).context.unwrap();
    assert_eq!((c.window, c.percent), (Some(200_000), Some(38)));
    append(&p, &[said("claude-haiku-4-5", 2, 30_000, 200_000)]);
    assert_eq!(poll(&mut r).context.unwrap().percent, None);
    let mut unnamed = said("x", 2, 0, 84_000);
    unnamed.as_object_mut().unwrap().remove("requestedModel");
    append(&p, &[unnamed]);
    let c = poll(&mut r).context.unwrap();
    assert_eq!((c.used, c.window, c.percent), (84_002, None, None));

    // 사용자 설정 `modelSettings.<모델>.autoCompactWindow`가 창을 줄인다(실측: 이 설정 500000이면 Opus 5.5는 `[1m]`이어도
    // `/context`가 500k를 창으로 보인다). 설정은 모델마다 처음 한 번 읽는다
    std::fs::write(&r.claude_settings, r#"{"modelSettings":{"claude-opus-5-5":{"autoCompactWindow":500000}}}"#)
        .unwrap();
    let mut capped = Reader::new(Source::Claude, p.clone());
    capped.claude_settings = r.claude_settings.clone();
    append(&p, &[said("claude-opus-5-5[1m]", 2, 11_748, 32_172)]);
    let c = poll(&mut capped).context.unwrap();
    assert_eq!((c.window, c.percent), (Some(500_000), Some(9)));
    append(&p, &[said("claude-haiku-5-5", 2, 11_748, 32_172)]);
    assert_eq!(poll(&mut capped).context.unwrap().window, Some(1_000_000));
}

#[test]
fn claude_window_names_and_setting_forms() {
    let w = |m: &str| claude_window(Some(m), None);
    assert_eq!(
        [
            w("claude-opus-5-5"),
            w("claude-fable-5"),
            w("claude-sonnet-5-5[1m]"),
            w("claude-haiku-4-5"),
            w("claude-3-5-sonnet-20241022")
        ],
        [Some(1_000_000), Some(1_000_000), Some(1_000_000), Some(200_000), Some(200_000)]
    );
    assert_eq!([claude_window(Some("gpt-6"), None), claude_window(None, None)], [None, None]);
    let forms = [json!(500_000), json!("500k"), json!("1m"), json!("200"), json!("auto"), json!(50_000), json!("2m")];
    assert_eq!(
        forms.iter().map(compact_window).collect::<Vec<_>>(),
        [Some(500_000), Some(500_000), Some(1_000_000), Some(200_000), None, None, None]
    );
}

/// 실측 꼴(10-20 Codex 0.163 /compact): token_count의 마지막 응답 크기와 창, 정리 앞뒤 token_count
#[test]
fn codex_context_follows_token_count_and_the_compaction() {
    let count = |total: u64, window: u64| {
        format!(
            r#"{{"timestamp":"2026-10-20T08:47:38.000Z","type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"total_tokens":{total}}},"last_token_usage":{{"input_tokens":{total},"cached_input_tokens":0,"output_tokens":0,"total_tokens":{total}}},"model_context_window":{window}}},"rate_limits":null}}}}"#
        )
    };
    let raw = |p: &Path, lines: &[String]| {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().append(true).open(p).unwrap();
        for l in lines {
            writeln!(f, "{l}").unwrap();
        }
    };
    let p = file("codex-context", &[]);
    raw(&p, &[count(18_861, 258_400)]);
    let mut r = Reader::new(Source::Codex, p.clone());
    let c = poll(&mut r).context.unwrap();
    // Codex 터미널: 기준 12k를 빼고 left = round((246400 − 6861) / 246400 × 100) = 97 → 사용 3%
    assert_eq!((c.used, c.window, c.percent, c.compaction), (18_861, Some(258_400), Some(3), None));
    // 사용량이 없는 줄(한도만)은 앞 값을 둔다
    raw(&p, &[r#"{"timestamp":"2026-10-20T08:47:39.000Z","type":"event_msg","payload":{"type":"token_count","info":null,"rate_limits":{"primary":null}}}"#.to_string()]);
    assert_eq!(poll(&mut r).context.unwrap().used, 18_861);
    // 정리 줄(남긴 기록을 실어 크다) 뒤 첫 token_count가 정리 뒤 크기다
    raw(&p, &[
        r#"{"timestamp":"2026-10-20T08:47:41.000Z","type":"compacted","payload":{"message":"","replacement_history":[{"type":"compaction","encrypted_content":"x"}]}}"#.to_string(),
        count(12_274, 258_400),
    ]);
    let c = poll(&mut r).context.unwrap();
    assert_eq!(
        (c.used, c.percent, c.compaction),
        (12_274, Some(0), Some(ContextCompaction { before: 18_861, after: 12_274 }))
    );
    // 창보다 큰 사용은 %가 없다
    raw(&p, &[count(480_000, 475_000)]);
    let c = poll(&mut r).context.unwrap();
    assert_eq!((c.used, c.window, c.percent), (480_000, Some(475_000), None));
    // 운영 rollout 실측: 350,245 / 475,000 → Codex 「27% left」 → 사용 73%
    raw(&p, &[count(350_245, 475_000)]);
    assert_eq!(poll(&mut r).context.unwrap().percent, Some(73));
}

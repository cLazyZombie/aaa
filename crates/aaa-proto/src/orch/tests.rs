use super::*;

/// env를 담는 형의 Debug(로그·오류 문구)는 이름만 찍고 값은 찍지 않는다. 이 형을 품은 메시지(AgentRequest·ThreadPrepare 등)도 같다.
#[test]
fn debug_shows_env_names_without_values() {
    let value = &*aaa_test_support::token("aaa-canary-debug-0123456789abcdef");
    let env = vec![("GH_TOKEN".to_string(), value.to_string()), ("LANG".to_string(), value.to_string())];
    let resolved = ResolvedProfile {
        harness: "claude".into(),
        model: String::new(),
        effort: String::new(),
        args: vec![],
        command: vec![],
        env: env.iter().cloned().collect(),
    };
    let launch = Launch {
        profile: "p".into(),
        harness: "claude".into(),
        argv: vec!["claude".into()],
        env: env.clone(),
        resume_argv: vec![],
        session_key: None,
    };
    let spec = crate::SpawnSpec {
        name: "n".into(),
        argv: vec![],
        cwd: None,
        env,
        cols: 80,
        rows: 24,
        role: None,
        harness: None,
        profile: Some("p".into()),
        resolved_profile: Some(resolved.clone()),
    };
    let prepare = ThreadPrepare {
        resolved_profile: Some(resolved),
        project: "demo".into(),
        thread: "t-0001".into(),
        title: String::new(),
        kind: ThreadKind::Dir,
        repo: None,
        base: None,
        profile: "p".into(),
        brief: String::new(),
        safety: None,
    };
    let request = crate::AgentRequest::Spawn(spec.clone());
    for text in [format!("{launch:?}"), format!("{spec:?}"), format!("{prepare:#?}"), format!("{request:?}")] {
        assert!(!text.contains(value) && !text.contains("0123456789"), "{text}");
        assert!(text.contains("«env 2개: GH_TOKEN, LANG»"), "{text}");
    }
}

/// N-1 host의 wire 응답에는 L3 기록 필드가 없다.
#[test]
fn routine_view_reads_previous_front_response() {
    let old = r#"{"name":"ping","schedule":"every 10m","command":"","machine":null,"enabled":true,"commands_allowed":false,"approved":false,"command_sha256":"","last_run_ms":1,"next_run_ms":600001,"skipped":2,"no_coordinator":0,"error":null}"#;
    let view: RoutineView = serde_json::from_str(old).unwrap();
    assert_eq!(view.name, "ping");
    assert_eq!(view.schedule, "every 10m");
    assert_eq!(view.next_run_ms, Some(600001));
    assert!(view.recent.is_empty() && view.last_result.is_none());
}

/// 응답·사건 enum 태그(`kind`·`ev`)와 안에 든 구조체 필드 이름이 겹치면 읽을 수 없다
#[test]
fn tagged_views_round_trip() {
    use crate::{FrontEvent, FrontResponse};
    let q = QuestionView {
        short_id: None,
        id: "q-1".into(),
        pane: "p".into(),
        machine: "mac-a".into(),
        project: Some("demo".into()),
        thread: Some("t-0001".into()),
        question_kind: "trust".into(),
        text: "Do you trust".into(),
        options: vec!["yes".into(), "no".into()],
        state: QuestionState::Open,
        answer: None,
        created_at_ms: 1,
        menu: None,
    };
    let m = MessageView {
        id: "m-1".into(),
        project: "demo".into(),
        thread: "t-0001".into(),
        mode: MessageMode::Interrupt,
        state: DeliveryState::Accepted,
        input: Some(MessageInput::Submitted),
        text: "x".into(),
        created_at_ms: 1,
    };
    for r in [FrontResponse::Question(q.clone()), FrontResponse::Message(m)] {
        let j = serde_json::to_string(&r).unwrap();
        assert_eq!(serde_json::from_str::<FrontResponse>(&j).unwrap(), r, "{j}");
    }
    let e = FrontEvent::Question(q);
    let j = serde_json::to_string(&e).unwrap();
    assert_eq!(serde_json::from_str::<FrontEvent>(&j).unwrap(), e, "{j}");
}

#[test]
fn names_and_slug() {
    assert!(valid_project("aaa-dev_1") && !valid_project("../x") && !valid_project("A") && !valid_project(""));
    assert!(valid_thread("t-0001") && valid_thread("t-12345") && !valid_thread("t-1") && !valid_thread("t-00a1"));
    assert_eq!(slug("Fix the Login bug!"), "fix-the-login-bug");
    assert_eq!(slug("한글 제목"), "");
    assert_eq!(slug("M1-3 Thread: 시작"), "m1-3-thread");
}

/// herdr-projects `next_lines_come_from_the_next_section_only`를 옮겼다
#[test]
fn next_lines_come_from_the_next_section_only() {
    let report = "[완료] 착지\n## Report\n- not this\n## Next\n- 확인한다\n* Fix CI\n3. Confirm assumption X\n\n## Remember\n- nor this\n";
    assert_eq!(next_lines(report), ["확인한다", "Fix CI", "Confirm assumption X"]);
    assert!(next_lines("## Report\nnothing\n").is_empty());
    assert!(next_lines("## Next\n").is_empty());
    // `#` 머리가 오면 Next가 끝난다
    assert_eq!(next_lines("## Next\n- 하나\n# 다른 절\n- 둘\n"), ["하나"]);
}

#[test]
fn section_lines_ignore_fenced_examples_and_keep_real_actions() {
    let report = "[완료] 끝\n## Report\n예전 보고:\n````markdown\n## Next\n- 이어 하기\n```\n## Decisions\n- 예시 결정\n````\n## Next\n- 결과 확인\n~~~sh\n## Remember\n- 예시 명령\n~~~\n- 다음 단계\n## Decisions\n- 실제 결정\n";
    assert_eq!(next_lines(report), ["결과 확인", "다음 단계"]);
    assert_eq!(section_lines(report, "Decisions"), ["실제 결정"]);
    for (open, close) in [("```md", "```"), ("~~~~", "~~~~~"), ("   ```", "  ````  ")] {
        let report = format!("## Report\n{open}\n## Next\n- 예시\n{close}\n## Next\n- 실제 행동\n");
        assert_eq!(next_lines(&report), ["실제 행동"], "{report}");
    }
    // 다른 문자·짧은 닫기·뒤에 글이 붙은 줄은 열린 울타리를 끝내지 않는다.
    for close in ["~~~", "``", "``` 설명"] {
        let report = format!("## Next\n- 실제 행동\n```\n{close}\n## Next\n- 코드 속 예시\n");
        assert_eq!(next_lines(&report), ["실제 행동"], "{report}");
    }
}

/// 번호 줄이 질문, 글자 줄이 선택지다. 선택지가 모자라도 질문 제목은 보존하고 진단한다.
#[test]
fn report_questions_read_numbered_questions_and_lettered_choices() {
    let report = "[결정 필요] 질문 2개: 배치\n\n## Report\n- 1. 이것은 질문이 아니다\n\n## Questions\n\
            1. 로그인 단추 자리\n   - A 오른쪽 아래 (추천) — 지금 화면과 같음\n   - B) 가운데 큰 단추 — 휴대폰에서 누르기 쉬움\n   - C. 입력칸 바로 아래\n\
            2. 선택지 하나뿐\n   - A 그대로\n\
            3. 비밀번호 보기\n   - A 넣는다(Recommended)\n   - B 넣지 않는다\n\
            - Z 질문 밖 줄은 앞 질문 것이다\n\n## Next\n- 1. 다음 절\n";
    let qs = report_questions(report);
    assert_eq!(qs.len(), 3, "선택지가 모자란 질문 제목도 남긴다: {qs:?}");
    assert_eq!(qs[1].text, "선택지 하나뿐");
    assert_eq!(qs[1].choices.len(), 1);
    assert!(parse_report_questions(report).warning.is_some());
    assert_eq!(qs[0].text, "로그인 단추 자리");
    let c = &qs[0].choices;
    assert_eq!(
        (c[0].label.as_str(), c[0].description.as_deref(), c[0].recommended),
        ("오른쪽 아래", Some("지금 화면과 같음"), true)
    );
    assert_eq!((c[1].label.as_str(), c[1].recommended), ("가운데 큰 단추", false));
    assert_eq!((c[2].label.as_str(), c[2].description.as_deref()), ("입력칸 바로 아래", None));
    assert_eq!(qs[2].text, "비밀번호 보기");
    assert_eq!((qs[2].choices[0].label.as_str(), qs[2].choices[0].recommended), ("넣는다", true));
    assert_eq!(qs[2].choices.len(), 3, "글자 줄은 그 앞 질문에 붙는다");
    assert!(report_questions("## Report\n1. 질문\n- A 하나\n- B 둘\n").is_empty());
}

#[test]
fn report_questions_keep_emphasized_letters_and_numbered_m3_questions() {
    // host가 보존한 t-0578 답변 당시 Questions 원문이다.
    let report = include_str!("../../tests/fixtures/m3-questions.md");
    let qs = report_questions(report);
    assert_eq!(qs.len(), 2, "{qs:?}");
    assert_eq!(qs[0].choices.len(), 3);
    assert_eq!(qs[1].choices.len(), 2);
    assert!(qs[0].choices[0].recommended);
    assert!(qs[0].choices[0].label.starts_with("install I3이 main에 올 때까지"));
    assert!(qs[0].choices[2].label.starts_with("install 순서를 바꿔"));
    assert!(qs[1].text.starts_with("M3 끝까지 시험할 곳:"));
}

#[test]
fn report_questions_do_not_count_korean_words_or_alphanumeric_names() {
    for report in [
        include_str!("../../tests/fixtures/r-1931-questions.md"),
        include_str!("../../tests/fixtures/r-1994-questions.md"),
    ] {
        let parsed = parse_report_questions(report);
        assert!(parsed.warning.is_none(), "{parsed:?}");
        assert_eq!(parsed.items.len(), 1);
        assert_eq!(parsed.items[0].choices.len(), 2);
        assert!(parsed.items[0].choices[0].recommended);
    }
    for word in ["A를 달성", "A안 추천", "B안", "t3code", "A1 항목"] {
        assert_eq!(choice_letter(&format!("- {word}")), None, "{word}");
    }
}

#[test]
fn report_questions_keep_deploy_thread_list_in_body() {
    // 수정해서 등록한 q-4321의 본문에 거절 당시 목록 표기를 되돌렸다.
    let report = format!(
        "{}- `d-2033` 후속 기록\n",
        include_str!("../../tests/fixtures/deploy-approval-questions.md")
            .replace("t-2030: ", "t-2030(질문 카드) ")
            .replace("t-2032: ", "r-2032 — ")
    );
    let parsed = parse_report_questions(&report);
    assert!(parsed.warning.is_none(), "{parsed:?}");
    assert_eq!(parsed.items.len(), 1);
    let question = &parsed.items[0];
    let body = question.body.as_deref().unwrap();
    for start in ["- t-2030(", "- t-2031: ", "- r-2032 — "] {
        assert!(body.lines().any(|line| line.starts_with(start)), "{body}");
    }
    assert_eq!(question.after.as_deref(), Some("- `d-2033` 후속 기록"));
    let choices: Vec<_> = question.choices.iter().map(|c| (c.letter.as_deref(), c.label.as_str())).collect();
    assert_eq!(choices, [(Some("A"), "지금 배포"), (Some("B"), "나중에")]);
}

#[test]
fn report_questions_locate_malformed_choices_next_to_deploy_thread_list() {
    let report = include_str!("../../tests/fixtures/deploy-approval-questions.md");
    for wrong in ["C: 중단", "C-2030: 중단", "**C 중단", "T-2030: 중단", "t-2030x: 중단"] {
        let broken = report.replace("\n세 건 모두", &format!("\n- {wrong}\n\n세 건 모두"));
        let parsed = parse_report_questions(&broken);
        let warning = parsed.warning.as_deref().unwrap();
        assert!(warning.contains("1번 질문 · 8번 줄"), "{wrong}: {warning}");
        let fix = if wrong.starts_with(['T', 't']) {
            "선택지 글자는 A부터 I까지다"
        } else {
            "형식으로 고친다"
        };
        assert!(warning.contains(fix), "{wrong}: {warning}");
    }
}

#[test]
fn report_questions_locate_short_duplicate_and_missing_choices() {
    let report = concat!(
        "[결정 필요] 질문 3개: 검토\r\n\r\n## Questions\r\n",
        "### 1. 진행\r\n- A 진행\r\n- B 보류\r\n",
        "### 2. 배포\r\n- t-2261(replay resume) 설명\r\n- A 배포\r\n",
        "### 3. 시점\r\n- A 지금\r\n- A 나중에\r\n- C 내일\r\n",
    );
    let parsed = parse_report_questions(report);
    let warning = parsed.warning.as_deref().unwrap();
    assert!(warning.contains("2번 질문 · 7번 줄. 선택지가 1개다"), "{warning}");
    assert!(warning.contains("3번 질문 · 12번 줄. A 선택지가 11번 줄과 중복된다"), "{warning}");
    assert!(warning.contains("3번 질문 · 13번 줄. B 선택지가 빠졌다"), "{warning}");
    assert_eq!(warning.lines().count(), 3, "{warning}");
    assert_eq!(parsed.items[1].body.as_deref(), Some("- t-2261(replay resume) 설명"));
    assert_eq!(parsed.items[1].choices.len(), 1);
    assert!(parsed.items[2].choices.is_empty());
}

#[test]
fn report_questions_bound_the_warning_for_many_invalid_lines() {
    let report = format!("## Questions\n### 1. 진행\n- A 진행\n- B 보류\n{}", "- Z 확인\n".repeat(100));
    let warning = parse_report_questions(&report).warning.unwrap();
    assert!(warning.starts_with("형식 확인 필요: 1번 질문 · 5번 줄."));
    assert_eq!(warning.lines().count(), 65);
    assert!(warning.ends_with("오류가 36개 더 있다. 위 줄을 고친 뒤 다시 검사한다."));
}

#[test]
fn report_questions_keep_a_direct_answer_card_when_no_heading_is_read() {
    let report = "[결정 필요] 질문 1개: 진행\n## Questions\n### 진행할까요\n- A 진행\n- B 보류\n";
    let parsed = parse_report_questions(report);
    assert!(parsed.warning.is_some());
    assert_eq!(parsed.items.len(), 1);
    assert_eq!(parsed.items[0].text, "직접 답");
    assert!(parsed.items[0].choices.is_empty());
    assert!(parsed.items[0].body.as_deref().unwrap().contains("### 진행할까요"));
}

#[test]
fn report_questions_accept_choice_emphasis_and_recommendation_spacing() {
    for a in ["**A(추천)**", "**A** (추천)", "*A* (추천)", "A(추천)", "A (추천)", "__A(추천)__", "_A_ (추천)"]
    {
        let report = format!("## Questions\n### 1. 진행\n- {a} 진행한다\n- B 멈춘다\n");
        let parsed = parse_report_questions(&report);
        assert!(parsed.warning.is_none(), "{a}: {parsed:?}");
        assert_eq!(parsed.items[0].choices[0].label, "진행한다", "{a}");
        assert!(parsed.items[0].choices[0].recommended, "{a}");
    }
}

#[test]
fn report_questions_warn_instead_of_silently_relabeling_or_dropping() {
    for report in [
        "## Questions\n### 1. 진행\n- **A 진행\n- B 보류\n- C 멈춤\n",
        "## Questions\n### 1. 진행\n- B 보류\n- C 멈춤\n",
        "## Questions\n### 1. 진행\n- A 진행\n- C 멈춤\n",
        "## Questions\n### 1. 진행\n- A 진행\n- A 보류\n",
        "## Questions\n### 1. 진행\n- A 진행\n### 2. 색\n- A 파랑\n- B 회색\n",
        "## Questions\n### 1. 진행\n- A 진행\n- B 보류\n### 둘째 질문\n- A 파랑\n- B 회색\n",
        "## Questions\n### 1. 진행\n- A 진행\n- B 보류\n#### 2. 잘못된 질문 머리\n- A 파랑\n- B 회색\n",
        "[결정 필요] 질문 2개: 색\n## Questions\n1. 색\n- A 파랑\n- B 회색\n",
        "## Questions\n질문 제목만\n- A 파랑\n- B 회색\n",
        "## Questions\n2. 색\n- A 파랑\n- B 회색\n",
    ] {
        let parsed = parse_report_questions(report);
        assert!(parsed.warning.as_deref().is_some_and(|w| w.contains("형식 확인 필요")), "{report}: {parsed:?}");
    }
    let too_many = (0..10).map(|i| format!("- {} 선택\n", (b'A' + i) as char)).collect::<String>();
    assert!(parse_report_questions(&format!("## Questions\n1. 많음\n{too_many}")).warning.is_some());
    let ten = (1..=10).map(|i| format!("### {i}. 질문\n- A 진행\n- B 보류\n")).collect::<String>();
    assert!(parse_report_questions(&format!("## Questions\n{ten}")).warning.is_some());
    let code = "## Questions\n### 1. 진행\n```md\n2. 코드 속 번호\n- X 코드 속 글자\n```\n- A 진행\n- B 보류\n";
    assert!(parse_report_questions(code).warning.is_none());
    assert!(parse_report_questions("## Report\n- A 본문\n- B 본문\n").warning.is_none());
    assert!(parse_report_questions(include_str!("../../tests/fixtures/m3-questions.md")).warning.is_none());
}

#[test]
fn report_questions_distinguish_question_limit_from_format_errors() {
    for heading in ["### ", ""] {
        let questions = (1..=9).map(|i| format!("{heading}{i}. 질문 {i}\n- A 진행\n- B 보류\n")).collect::<String>();
        let nine = parse_report_questions(&format!("[결정 필요] 질문 9개: 진행\n## Questions\n{questions}"));
        assert!(nine.warning.is_none(), "{nine:?}");
        assert_eq!(nine.items.len(), 9);
        let ten =
            format!("[결정 필요] 질문 10개: 진행\n## Questions\n{questions}{heading}10. 질문 10\n- A 진행\n- B 보류\n");
        let parsed = parse_report_questions(&ten);
        assert_eq!(parsed.items.len(), 9);
        assert_eq!(parsed.items[8].text, "질문 9");
        assert!(parsed.items.iter().all(|q| q.choices.len() == 2));
        let warning = parsed.warning.as_deref().unwrap();
        assert!(warning.contains("보고당 질문 최대 9개") && warning.contains("질문 10개 중 9개"), "{warning}");
        assert!(warning.contains("9개 이하로 나눠 남은 질문은 기록해 다음 보고로 이어 묻는다"), "{warning}");
        assert!(!warning.contains("형식 확인 필요"), "{warning}");
        // 상한 초과와 함께 생긴 형식 오류는 마지막(잘린) 질문에서도 놓치지 않는다.
        for broken in [ten.replacen("질문 10개:", "질문 11개:", 1), ten.replace("- B 보류", "- C 보류")] {
            let parsed = parse_report_questions(&broken);
            let warning = parsed.warning.as_deref().unwrap();
            assert!(warning.contains("보고당 질문 최대 9개") && warning.contains("형식 확인 필요"), "{warning}");
        }
        let broken_tail = ten.replace("10. 질문 10\n- A 진행\n- B 보류", "10. 질문 10\n- **A 진행\n- B 보류\n- C 중단");
        let parsed = parse_report_questions(&broken_tail);
        assert!(parsed.warning.as_deref().unwrap().contains("형식 확인 필요"), "{parsed:?}");
    }
    // 선택지 상한은 별도 계약이다. 질문 수 상한으로 잘못 진단하지 않는다.
    let choices = (b'A'..=b'J').map(|c| format!("- {} 선택\n", c as char)).collect::<String>();
    let parsed = parse_report_questions(&format!("## Questions\n### 1. 선택\n{choices}"));
    assert_eq!(parsed.items[0].choices.len(), 9);
    let warning = parsed.warning.as_deref().unwrap();
    assert!(warning.contains("형식 확인 필요") && !warning.contains("보고당 질문 최대"), "{warning}");
}

#[test]
fn reordered_choices_keep_source_letters_and_ambiguous_letters_are_diagnosed() {
    let report = "## Questions\n### 1. 진행\n- B 새 안 (추천)\n- A 원래\n- C 보류\n";
    let parsed = parse_report_questions(report);
    assert!(parsed.warning.is_none(), "{parsed:?}");
    let q = &parsed.items[0];
    assert_eq!(q.choices.iter().enumerate().map(|(i, c)| c.letter_at(i)).collect::<Vec<_>>(), ["B", "A", "C"]);
    assert_eq!(q.choice_index("b"), Some(0));
    assert_eq!(q.choice_index("A"), Some(1));
    assert_eq!(q.choice_index("D"), None);
    for broken in [report.replace("- C 보류", "- B 보류"), report.replace("- C 보류", "- 보류")] {
        assert!(parse_report_questions(&broken).warning.is_some());
    }
    let duplicate = parse_report_questions(&report.replace("- C 보류", "- B 보류"));
    assert!(duplicate.items[0].choices.is_empty());
    let legacy: QuestionChoice =
        serde_json::from_str(r#"{"label":"옛 자료","description":null,"recommended":false}"#).unwrap();
    assert_eq!(legacy.letter_at(1), "B");
}

#[test]
fn rich_question_keeps_numbered_body_lists_and_accepts_a_numbered_next_question() {
    let report = "## Questions\n### 1. 순서\n1. 먼저 읽는다\n2. 다음 확인한다\n\n- A 진행 (추천)\n- B 보류\n\n2. **시험할 곳**\n- A 여기\n- B 실험실\n";
    let parsed = parse_report_questions(report);
    assert!(parsed.warning.is_none(), "{parsed:?}");
    assert_eq!(parsed.items.len(), 2);
    assert_eq!(parsed.items[0].text, "순서");
    assert_eq!(parsed.items[0].body.as_deref(), Some("1. 먼저 읽는다\n2. 다음 확인한다"));
    assert_eq!(parsed.items[1].text, "시험할 곳");
}

#[test]
fn t0621_trailing_icon_notes_do_not_drop_the_first_question() {
    let report = include_str!("../../tests/fixtures/t0621-questions.md");
    let parsed = parse_report_questions(report);
    assert_eq!(parsed.items.len(), 2, "{parsed:?}");
    assert_eq!(parsed.items[0].choices.len(), 4);
    assert_eq!(parsed.items[1].choices.len(), 3);
    assert!(parsed.warning.is_none(), "{parsed:?}");
    let q = &parsed.items[0];
    assert_eq!(q.text, "단추 아이콘");
    assert!(q.after.as_deref().unwrap().contains("E·F안 그림:"));
    assert!(q.after.as_deref().unwrap().contains("![E안](library/icon-e.png)"));
    assert!(q.after.as_deref().unwrap().contains("![F안](library/icon-f.png)"));
    assert_eq!(q.choices[3].body.as_deref(), Some("![D안](library/icon-d.png)"));
    let broken = report.replace("- B BotMessageSquare", "- **B BotMessageSquare");
    assert!(parse_report_questions(&broken).warning.is_some(), "줄이 빠지면 경고한다");
}

#[test]
fn t0621_table_rows_and_indented_pictures_do_not_limit_choices_to_four() {
    let original = include_str!("../../tests/fixtures/t0621-questions.md");
    let start = original.find("\nE·F안 그림:").unwrap();
    let end = original[start..].find("\n### 2.").unwrap() + start;
    let no_after = format!("{}{}", &original[..start], &original[end..]);
    let four_rows =
        original.lines().filter(|l| !l.starts_with("| E |") && !l.starts_with("| F |")).collect::<Vec<_>>().join("\n");
    let no_choice_images = original.lines().filter(|l| !l.starts_with("  ![")).collect::<Vec<_>>().join("\n");
    let tail_text_only = original
        .lines()
        .filter(|l| !l.starts_with("![E안]") && !l.starts_with("![F안]"))
        .collect::<Vec<_>>()
        .join("\n");
    let six = no_after.replace(
        "\n### 2.",
        "\n- E TowerControl\n  ![E](library/icon-e.png)\n- F Headset\n  ![F](library/icon-f.png)\n\n### 2.",
    );
    for (report, count) in
        [(original, 4), (&four_rows, 4), (&no_choice_images, 4), (&tail_text_only, 4), (&no_after, 4), (&six, 6)]
    {
        let parsed = parse_report_questions(report);
        assert_eq!(parsed.items.len(), 2, "{parsed:?}");
        assert_eq!(parsed.items[0].choices.len(), count, "{parsed:?}");
        assert!(parsed.warning.is_none(), "{parsed:?}");
    }
    let bad = parse_report_questions("## Questions\n### 1. 단추 아이콘\n- A Bot\n");
    assert_eq!(bad.items[0].text, "단추 아이콘");
    assert_eq!(bad.items[0].choices.len(), 1);
    assert!(bad.warning.is_some());
    let ten = (b'A'..=b'J').map(|c| format!("- {} 아이콘\n", c as char)).collect::<String>();
    let over = parse_report_questions(&format!("## Questions\n### 1. 단추 아이콘\n{ten}"));
    assert_eq!(over.items[0].text, "단추 아이콘");
    assert!(over.warning.is_some(), "상한 초과도 제목과 경고를 남긴다");
}

#[test]
fn report_questions_read_rich_bodies_and_choice_pictures() {
    let report = "[결정 필요] 질문 2개: 시안\n\n## Questions\n### 1. 왼쪽 막대 시안\n지금 모양:\n\n![지금](library/now.png)\n\n\
            | 안 | 차이 |\n| --- | --- |\n| A | 한 화면 |\n\n- 보통 글머리\n- I think 이것도 본문\n\n```text\n- A 코드 안 줄\n```\n\n\
            - A 따로 된 화면 (추천) — 한 번에 답함\n  ![A안](library/A.png)\n\n  둘째 줄\n- B 사이드바 목록 — 목록이 옆에\n  ![B안](library/B.png)\n\n\
            ### 2. 정리 기간\n- A 14일 (추천)\n- B 30일\n\n## Next\n- 다음\n";
    let qs = report_questions(report);
    assert_eq!(qs.len(), 2, "{qs:?}");
    assert_eq!(qs[0].text, "왼쪽 막대 시안");
    let body = qs[0].body.as_deref().unwrap();
    assert!(
        body.starts_with("지금 모양:") && body.contains("![지금](library/now.png)") && body.contains("| A | 한 화면 |")
    );
    assert!(
        body.contains("- I think 이것도 본문") && body.ends_with("```"),
        "본문 글머리·code 안 줄은 본문이다: {body}"
    );
    let c = &qs[0].choices;
    assert_eq!(c.len(), 2);
    assert_eq!(
        (c[0].label.as_str(), c[0].recommended, c[0].description.as_deref()),
        ("따로 된 화면", true, Some("한 번에 답함"))
    );
    assert_eq!(c[0].body.as_deref(), Some("![A안](library/A.png)\n\n둘째 줄"));
    assert_eq!(c[1].body.as_deref(), Some("![B안](library/B.png)"));
    assert_eq!((qs[1].text.as_str(), qs[1].body.as_deref(), qs[1].choices.len()), ("정리 기간", None, 2));
    // 한도: 글자 경계에서 자르고 열린 fence를 닫는다
    let long =
        format!("## Questions\n### 1. 긴 본문\n```\n{}\n```\n- A 하나\n- B 둘\n", "가".repeat(QUESTION_BODY_MAX));
    let b = report_questions(&long)[0].body.clone().unwrap();
    assert!(b.len() < QUESTION_BODY_MAX + 100 && b.contains("```\n\n*(본문이 길어 잘렸다"), "{}", &b[b.len() - 80..]);
}

#[test]
fn fill_brief_puts_the_machine_paths() {
    let b = format!("`{BRIEF_WORK_DIR}/report.md` `{BRIEF_EXE} report --unknown`");
    assert_eq!(
        fill_brief(&b, "/w/.aaa/p-t-0001", "/opt/aaa"),
        "`/w/.aaa/p-t-0001/report.md` `/opt/aaa report --unknown`"
    );
    assert_eq!(fill_brief(BRIEF_EXE, "", r"C:\Program Files\aaa.exe"), r#""C:\Program Files\aaa.exe""#);
    assert_eq!(fill_brief(BRIEF_INBOX_LIST, "/work/it's a folder", ""), "ls -1 -- '/work/it'\\''s a folder/inbox'");
    assert_eq!(fill_brief(BRIEF_INBOX_LIST, r"C:\work dir\task", ""), "ls -1 -- 'C:/work dir/task/inbox'");
    let reads = fill_brief(BRIEF_GIT_READS, "/work/it's a folder/.aaa/demo-t-0001", "");
    assert!(reads.contains("git -C '/work/it'\\''s a folder' --no-pager --no-optional-locks"));
    assert!(reads.contains("--end-of-options") && reads.contains("status --short --"));
    assert!(work_git_read_commands("/work/other").is_empty());
    assert!(work_git_read_commands("/work/*/.aaa/demo-t-0001").is_empty());
}

use super::*;

/// t-1845의 다섯 발췌와 11-14 선택지 제목 사례. 주변 설명·주소는 재구성한다.
#[test]
fn observed_reply_sections_history_and_corrections_keep_decision_boundaries() {
    let cards = HashMap::from([
        ("q-3578".into(), CardState::Open),
        ("q-3619".into(), CardState::Open),
        ("q-3621".into(), CardState::Open),
        ("q-3622".into(), CardState::Open),
        ("q-3654".into(), CardState::Closed),
        ("q-3656".into(), CardState::Closed),
        ("q-3655".into(), CardState::Open),
        ("q-3657".into(), CardState::Open),
        ("q-3658".into(), CardState::Open),
        ("q-3706".into(), CardState::Open),
    ]);
    let mut failures = Vec::new();
    for (name, text, missing, stale) in [
        (
            "12:08 일반 안내 뒤 카드",
            "그림과 영상을 보시고 고칠 점과 질문 하나에 답해 주세요.\n\n**정할 것**\n[q-3578 질문](#question=q-3578)",
            false,
            false,
        ),
        (
            "16:00 앞 카드와 뒤 선택지",
            "[q-3619 질문](#question=q-3619)을 열었습니다.\n\n**고칠 방식**\n- A 지금 고침 (추천)\n- B 나중에 고침\n- C 보류",
            false,
            false,
        ),
        (
            "16:09 질문 둘과 카드 둘",
            "질문 2개에 답해 주세요.\n\n**첫 작업**\nq-3621에서 답합니다.\n\n**둘째 작업**\nq-3622에서 답합니다.",
            false,
            false,
        ),
        (
            "19:34 교체 이력 표",
            "아래 새 카드로 답해 주세요.\n\n**현재 카드**\n| 질문 | 교체 이력 |\n| --- | --- |\n| q-3657 | q-3654에서 바뀜 |\n| q-3655 | 유지 |\n| q-3658 | q-3656에서 바뀜 |",
            false,
            false,
        ),
        (
            "21:10 선택적 해석 정정",
            "기절 시간은 중첩 없이 이어 가는 것으로 맡겼습니다.\n\n겹쳐야 한다면 말씀해 주세요.",
            false,
            false,
        ),
        (
            "11-14 선택지 제목",
            "[q-3706 질문](#question=q-3706)을 열었습니다.\n\n**선택지**\n- A 지금 진행 (추천)\n- B 나중에 진행\n- C 보류\n\nA를 고르시면 작업을 이어 갑니다.",
            false,
            false,
        ),
        (
            "일반 안내 뒤 새 승인",
            "질문 하나에 답해 주세요.\n\n**정할 것**\nq-3578에서 답합니다.\n\n지금 배포할까요?",
            true,
            false,
        ),
        (
            "선택지 뒤 독립 질문",
            "q-3706에서 답합니다.\n\n**선택지**\n- A 진행 (추천)\n- B 보류\n\n승인 여부를 알려 주세요.",
            true,
            false,
        ),
        (
            "선택지 뒤 추가 승인",
            "q-3706에서 답합니다.\n\n**선택지**\n- A 진행 (추천)\n- B 보류\n\n추가로 배포할까요?",
            true,
            false,
        ),
        ("카드 없는 선택지", "**선택지**\n- A 진행 (추천)\n- B 보류\n- C 취소", true, false),
        ("질문 둘에 카드 하나", "질문 2개에 답해 주세요.\n\n**정할 것**\nq-3621에서 답합니다.", true, false),
        (
            "중복 링크도 카드 하나",
            "질문 두 개에 답해 주세요.\n\n**정할 것**\n[q-3621 질문](#question=q-03621)",
            true,
            false,
        ),
        (
            "모르는 카드는 세지 않음",
            "질문 2개에 답해 주세요.\n\n**정할 것**\nq-3621, q-9999에서 답합니다.",
            true,
            false,
        ),
        (
            "옛 번호를 실제 답할 곳으로 안내",
            "아래 새 카드로 답해 주세요.\n\n**현재 카드**\nq-3657\n\n**답할 곳**\nq-3654에서 답해 주세요.",
            false,
            true,
        ),
        ("이력과 실제 안내를 함께 적음", "q-3654에서 바뀜. q-3654에서 선택해 주세요.", false, true),
        ("옛 번호 표지", "아래 새 카드로 답해 주세요.\n\n**현재 카드**\nq-3657 (옛 번호: q-3654)", false, false),
        ("옛 번호 뒤 실제 답 요청", "옛 번호: q-3654에서 답해 주세요.", false, true),
        ("옛 번호 자체를 선택", "옛 번호: q-3654를 선택해 주세요.", false, true),
        ("옛 번호 괄호를 닫고 답 요청", "(옛 번호: q-3654)에서 답해 주세요.", false, true),
        ("옛 번호 괄호 뒤 실제 선택", "q-3654 (옛 번호)에서 선택해 주세요.", false, true),
        (
            "링크로 쓴 이력",
            "아래 새 카드로 답해 주세요.\n\n**현재 카드**\nq-3657 ([q-3654 질문](#question=q-3654)에서 바뀜)",
            false,
            false,
        ),
        (
            "옛 번호 링크",
            "아래 새 카드로 답해 주세요.\n\n**현재 카드**\nq-3657 (옛 번호: [q-3654 질문](#question=q-3654))",
            false,
            false,
        ),
        (
            "이력 번호는 질문 수에 안 셈",
            "질문 2개에 답해 주세요.\n\n**현재 카드**\nq-3657 (q-3654에서 바뀜)",
            true,
            false,
        ),
        (
            "같은 문장에 요약과 별개 승인",
            "배포 승인 여부를 알려 주세요, 질문 하나에 답해 주세요.\n\n**정할 것**\nq-3578에서 답합니다.",
            true,
            false,
        ),
        ("정정과 같은 문장 뒤 승인", "겹쳐야 한다면 말씀해 주세요, 배포 승인 여부를 알려 주세요.", true, false),
        ("정정과 같은 문장 앞 승인", "배포 승인 여부를 알려 주세요, 겹쳐야 한다면 말씀해 주세요.", true, false),
        ("조건문에 실제 선택", "겹쳐야 한다면 중첩과 연장 중 하나를 골라 주세요.", true, false),
        ("조건문에 승인", "배포해야 한다면 말씀해 주세요.", true, false),
        ("조건문에 승인 내용", "겹쳐야 한다면 배포 승인 여부를 말씀해 주세요.", true, false),
    ] {
        let result = inspect(text, &cards);
        if (result.missing.is_some(), result.stale.is_some()) != (missing, stale) {
            failures.push(format!("{name}: missing={:?}, stale={:?}", result.missing, result.stale));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn cards() -> HashMap<String, CardState> {
    HashMap::from([
        ("q-1001".into(), CardState::Open),
        ("q-1002".into(), CardState::Answered),
        ("q-1003".into(), CardState::Closed),
    ])
}

/// t-1809의 관측 발췌와 같은 구절 앞뒤의 별개 요청을 함께 보존한다.
#[test]
fn observed_negative_and_optional_feedback_do_not_hide_real_requests() {
    let cards = cards();
    let mut unexpected = Vec::new();
    for feedback in [
        "지금 정해 주실 것은 없습니다.",
        "지금 정해 주실 것은 없고 기록을 확인합니다.",
        "혹시 시험 자체를 지우라는 뜻이셨다면 말씀해 주세요.",
        "화면에 뜬 오류 문구가 있었다면 알려 주세요.",
    ] {
        let result = inspect(feedback, &cards);
        if result.missing.is_some() || result.stale.is_some() {
            unexpected.push(feedback);
        }
        for text in [
            format!("{feedback}\n\n지금 배포할까요?"),
            format!("{}, 배포 승인 여부를 알려 주세요.", feedback.trim_end_matches('.')),
            format!("승인 여부를 알려 주세요, {feedback}"),
            format!("{feedback}\n- A 배포 (추천)\n- B 보류"),
            format!("{feedback}\n\n정해 주실 것은 배포 방식입니다."),
        ] {
            assert!(inspect(&text, &cards).missing.is_some(), "{text}");
        }
    }
    for text in [
        "정해 주실 것은 없습니다만 배포 방식은 정해 주세요.",
        "혹시 시험 자체를 지우라는 뜻이셨다면 배포 승인 여부를 말씀해 주세요.",
        "오류 문구가 있었다면 복구와 삭제 중 하나를 골라 주세요.",
        "화면에 뜬 오류 문구가 있었다면 알려 주세요, 승인 여부도 알려 주세요.",
        "알려 주시면 좋은 것은 오류 문구입니다.",
    ] {
        assert!(inspect(text, &cards).missing.is_some(), "{text}");
    }
    assert!(unexpected.is_empty(), "불필요한 경고: {unexpected:?}");
    let evidence = inspect("지금 정해 주실 것은 없습니다.\n\n승인 여부를 알려 주세요.", &cards).missing.unwrap();
    assert_eq!(evidence.line, 3);
}

/// t-1809에서 실제 카드로 이어진 두 발췌는 정상 경고로 유지한다.
#[test]
fn observed_work_and_visual_choices_still_require_cards() {
    for text in ["만들려면 저에게 말씀해 주세요.", "다음 시안 때 함께 골라 주세요."] {
        assert_eq!(inspect(text, &cards()).missing.unwrap().rule, "request", "{text}");
    }
}

/// i-24655의 링크와 답변 방법 안내를 재구성한다. 주소·선택지 본문은 넣지 않는다.
#[test]
fn card_or_chat_reply_instructions_link_only_to_the_existing_topic() {
    let mut cards = cards();
    for state in [CardState::Open, CardState::Answered, CardState::Closed] {
        cards.insert("q-3563".into(), state);
        for text in [
            "[q-3563 질문](#question=q-3563)을 열었습니다. 카드나 채팅(「1B」 등)으로 답해 주세요.",
            "[q-3563 질문](#question=q-3563)을 열었습니다.\n\n카드나 채팅(「1B」 등)으로 답해 주세요.",
        ] {
            let result = inspect(text, &cards);
            assert!(result.missing.is_none(), "{text}");
            if state == CardState::Closed {
                assert_eq!(result.stale.unwrap().refs, [("q-3563".into(), state)], "{text}");
            } else {
                assert!(result.stale.is_none(), "{text}");
            }
        }
    }
    for text in [
        "[q-9999 질문](#question=q-9999)을 열었습니다. 카드나 채팅으로 답해 주세요.",
        "q-1001에서 고릅니다.\n\n## 다른 작업\n카드나 채팅으로 답해 주세요.",
        "q-1001에서 고릅니다. 추가로 카드나 채팅으로 답해 주세요.",
        "q-1001에서 고릅니다. 배포 승인 여부를 채팅으로 답해 주세요.",
        "q-1001에서 고릅니다. 카드나 채팅으로 배포 승인 여부를 답해 주세요.",
    ] {
        assert!(inspect(text, &cards).missing.is_some(), "{text}");
    }
}

/// 기록의 오탐 형태와 실제 승인·정보 요청을 함께 고정한다. 원문 개인정보는 넣지 않는다.
#[test]
fn decisions_are_distinct_from_instructions_optional_offers_and_completed_lists() {
    let cards = cards();
    for text in [
        "진행할까요?",
        "이 파일을 지울까요?",
        "진행이라고 해 주세요.",
        "지금 배포해도 되면 말씀해 주세요.",
        "A 진행 (추천)\nB 보류",
        "Which option do you prefer?",
        "원하는 방향을 알려 주세요.",
        "함께 알려 주시면 좋은 것은 셋입니다.\n- 바꿀 내용\n- 진행 방식\n- 쓸 모델",
        "q-9999에서 선택해 주세요.",
        "해 주실 것: 저장소를 만듭니다.\n\n만드셨다고 알려 주시면 진행하겠습니다.",
        "함께 정할 것 (카드는 없음)\n선택지는 두 가지입니다.\n\n채팅에 답해 주시면 전하겠습니다.",
    ] {
        assert!(inspect(text, &cards).missing.is_some(), "{text}");
    }
    for text in [
        "검증을 마쳤습니다.",
        "실제 화면에서 직접 확인해 주세요.",
        "질문 카드가 뜨면 답해 주세요.",
        "Let me know if you'd like to change it.",
        "시간이 지나면 브라우저에서 다시 요청하신 뒤 말씀해 주세요.",
        "웹에 대기 카드가 남지 않는지는 화면에서 확인해 주세요.",
        "- A 원격 연결: 이미 진행합니다.\n- B 도구 대화: 모델 선택이 적용됩니다.",
        "원하시면 진행하겠습니다.",
        "원하시면 말씀해 주세요.",
        "뜻이 다르면 말씀해 주세요.",
        "시작하려면 「시작해」라고 해 주세요.",
        "백업은 보관했습니다. 지울지는 정해 주시면 됩니다.",
        "끝나면 알려 드릴 테니, 그 뒤에 빼 주세요.",
        "A. 디스크 기준: 고쳤습니다.\nB. 메뉴 시험: 고쳤습니다.",
        "- A 원격 연결: 이미 진행합니다.\n- B 대화 도구: 시작했습니다.",
        "> 진행할까요?",
        "```md\n진행할까요?\n```",
        "~~~md\n진행할까요?\n~~~",
        "보고: 「진행할까요?」",
        "진행할까요?라고 물었다는 보고입니다.",
    ] {
        assert!(inspect(text, &cards).missing.is_none(), "{text}");
    }
}

/// 선택적인 피드백 한 구절이 같은 문장의 별개 승인·선택 요청을 가리지 않아야 한다.
#[test]
fn conditional_feedback_does_not_require_an_answer_or_hide_a_decision() {
    let cards = cards();
    for text in [
        "필요하면 말씀해 주세요.",
        "지우려면 따로 말씀해 주세요.",
        "수정을 마쳤습니다. 아직 안 움직이는 경우가 있으면 말씀해 주세요.",
        "필요하다면 알려 주십시오.",
        "오류가 있으면 따로 알려 주세요.",
        "말씀해 주세요, 필요하면.",
    ] {
        let result = inspect(text, &cards);
        assert!(result.missing.is_none() && result.stale.is_none(), "{text}");
    }
    for text in [
        "지금 배포해도 되면 말씀해 주세요.",
        "필요하면 지금 배포할지 알려 주세요.",
        "지우려면 승인 여부를 말씀해 주세요.",
        "아직 안 움직이는 경우가 있으면 복구와 삭제 중 하나를 골라 주세요.",
        "필요하면 말씀해 주세요, 지금 배포 여부를 알려 주세요.",
        "지금 배포 여부를 알려 주세요, 필요하면 말씀해 주세요.",
        "필요하면 말씀해 주세요. 지금 배포할까요?",
        "필요하면 말씀해 주세요.\n- A 배포 (추천)\n- B 보류",
        "필요하면 배포할까요?",
        "지우려면 어느 안으로 할까요?",
        "승인하셨으면 알려 주세요.",
        "승인 여부를 말씀해 주세요, 필요하면.",
    ] {
        assert!(inspect(text, &cards).missing.is_some(), "{text}");
    }
    let evidence = inspect("필요하면 말씀해 주세요.\n\n승인 여부를 알려 주세요.", &cards).missing.unwrap();
    assert_eq!(evidence.line, 3);
}

/// 뜻을 바로잡는 안내만 제외하며 같은 문장의 승인·선택 요청은 보존한다.
#[test]
fn meaning_corrections_are_optional_without_hiding_decisions() {
    let cards = cards();
    for feedback in ["다르게 생각하신 거라면 말씀해 주세요.", "다른 작업을 뜻하셨다면 말씀해 주세요."]
    {
        let result = inspect(feedback, &cards);
        assert!(result.missing.is_none() && result.stale.is_none(), "{feedback}");
        for text in [
            format!("{feedback}\n\n지금 배포할까요?"),
            format!("{}, 지금 배포 여부를 알려 주세요.", feedback.trim_end_matches('.')),
            format!("승인 여부를 알려 주세요, {feedback}"),
            format!("{feedback}\n- A 배포 (추천)\n- B 보류"),
        ] {
            assert!(inspect(&text, &cards).missing.is_some(), "{text}");
        }
    }
    for text in [
        "다르게 생각하신 거라면 배포 승인 여부를 말씀해 주세요.",
        "다른 작업을 뜻하셨다면 복구와 삭제 중 하나를 골라 주세요.",
    ] {
        assert!(inspect(text, &cards).missing.is_some(), "{text}");
    }
}

/// 완료 설명의 '별도로'는 뒤의 기존 카드 연결을 막지 않으며 새 요청은 계속 분리한다.
#[test]
fn completed_handoff_keeps_existing_card_choices_linked() {
    let mut cards = cards();
    cards.insert("q-3346".into(), CardState::Open);
    // 기록의 짧은 완료 문구와 q-ID를 쓰고, 공개되지 않은 선택지 본문은 재구성한다.
    let explanation = "별도로 정할 일로 넘겼습니다.\n- A 지금 구현 (추천)\n- B 할 일로 만들기\n- C 폐기";
    let linked = format!("{explanation}\n\n[질문](#question=q-3346)에서 답합니다.");
    let result = inspect(&linked, &cards);
    assert!(result.missing.is_none() && result.stale.is_none());
    for text in [
        explanation.to_owned(),
        linked.replace("q-3346", "q-9999"),
        format!("{explanation}\n\n**다른 작업**\nq-3346에서 답합니다."),
        format!("{linked}\n\n별도로 배포할까요?"),
        "q-3346에서 답해 주세요. 별도로 승인 여부를 알려 주세요.".to_owned(),
        "q-3346에서 답해 주세요. 별도로 배포할까요? 별도로 정할 일로 넘겼습니다.".to_owned(),
        "별도로 정할 일로 넘겼습니다. 별도로 승인 여부를 알려 주세요.\n\nq-3346에서 답합니다.".to_owned(),
    ] {
        assert!(inspect(&text, &cards).missing.is_some(), "{text}");
    }
    cards.insert("q-3346".into(), CardState::Closed);
    let result = inspect(&linked, &cards);
    assert!(result.missing.is_none());
    assert_eq!(result.stale.unwrap().refs, [("q-3346".into(), CardState::Closed)]);
}

/// 링크와 요청 사이의 빈 줄을 허용하되 별개 승인 요청까지 면제하지 않는다.
#[test]
fn card_links_follow_the_topic_and_preserve_separate_new_requests() {
    let cards = cards();
    for text in [
        "[질문](#question=q-01001)에서 선택해 주세요.",
        "어느 안으로 할까요?\n\n- A 진행\n- B 보류\n\nq-1001에서 답합니다.",
        "카드를 확인해 주세요.\n\n**작업 결과**\n내용입니다.\n\nq-1001에서 답합니다.",
        "q-1001에서 고릅니다.\n\n- A 진행 (추천)\n- B 보류",
        "q-1002에서 답해 주세요.",
        "추가로 q-1001에서 배포할까요?",
        "q-1001로 바뀌었습니다.\n\n넣을 안을 골라 직접 답에 적어 주세요.",
        "두 가지를 골라 주세요.\n\n**1. 배치**\n내용\n\n**2. 색**\n내용\n\nq-1001에서 답합니다.",
        "**질문 1. 적용할까요?**\n\n**질문 2. 배포할까요?**\n\nq-1001, q-1002에서 답합니다.",
    ] {
        let result = inspect(text, &cards);
        assert!(result.missing.is_none() && result.stale.is_none(), "{text}");
    }
    for text in [
        "q-1001에서 답해 주세요.\n\n추가로 배포할까요?",
        "q-1001에서 답해 주세요. 추가로 배포할까요?",
        "q-1002에서 다시 배포할까요?",
        "q-1001과 q-9999에서 선택해 주세요.",
        "**질문 1. 적용할까요?**\n\n**질문 2. 배포할까요?**\n\nq-1001에서 답합니다.",
        "q-1001에서 답해 주세요.\n\n배포할까요?",
        "배포할까요?\n\n**다른 작업**\nq-1001에서 답합니다.",
    ] {
        assert!(inspect(text, &cards).missing.is_some(), "{text}");
    }
    let stale = inspect("q-1003에서 선택해 주세요.", &cards);
    assert!(stale.missing.is_none());
    assert_eq!(stale.stale.unwrap().refs, [("q-1003".into(), CardState::Closed)]);
}

#[test]
fn diagnostics_keep_rule_and_source_line_without_copying_the_reply() {
    let result = inspect("상태 보고\n\n배포할까요? 비공개 본문", &cards());
    let evidence = result.missing.unwrap();
    assert_eq!(evidence.line, 3);
    let diagnostic = evidence.diagnostic(123);
    assert!(diagnostic.contains("rule=question line=3"));
    assert!(!diagnostic.contains("비공개"));
    assert!(evidence.summary(REMINDER).contains("답변 3행 · 질문 어미"));
}

/// 간접 인용과 같은 줄의 카드 안내를 분리해 열림·답함·교체 상태까지 전달한다.
#[test]
fn reported_explanations_keep_card_links_and_states() {
    let cards = cards();
    for (id, state) in [("q-1001", CardState::Open), ("q-1002", CardState::Answered), ("q-1003", CardState::Closed)] {
        for text in [
            format!(
                "그림은 아직 검수를 받지 않았다고 적었습니다. [질문](#question={id})에서 판단해 주세요.\nA 통과(추천)\nB 더 고침"
            ),
            format!(
                "그림은 아직 검수를 받지 않았다고 적었습니다, [질문](#question={id})에서 고릅니다.\n\nA 통과(추천)\nB 더 고침"
            ),
            format!(
                "[질문](#question={id})에서 고릅니다. 그림은 아직 검수를 받지 않았다고 적었습니다.\n\nA 통과(추천)\nB 더 고침"
            ),
            format!("그림은 아직 검수를 받지 않았다고 적었습니다. {id}에서 고릅니다. 카드에서 답해 주세요."),
            format!("별도로 정할지 물었다고 적었습니다. {id}에서 고릅니다.\nA 통과(추천)\nB 더 고침"),
        ] {
            let result = inspect(&text, &cards);
            assert!(result.missing.is_none(), "{text}");
            if state == CardState::Closed {
                assert_eq!(result.stale.unwrap().refs, [(id.into(), state)], "{text}");
            } else {
                assert!(result.stale.is_none(), "{text}");
            }
        }
    }
}

/// 설명의 인용·옛 번호·다른 주제는 실제 새 요청의 카드가 될 수 없다.
#[test]
fn reported_explanations_do_not_hide_unlinked_requests() {
    let cards = cards();
    let explanation = "그림은 아직 검수를 받지 않았다고 적었습니다.";
    for text in [
        format!("{explanation} 지금 배포할까요?"),
        format!("지금 배포할까요? {explanation}"),
        format!("{explanation}, 승인 여부를 알려 주세요."),
        format!("승인 여부를 알려 주세요, {explanation}"),
        format!("{explanation} q-1001에서 고릅니다. 추가로 배포할까요?"),
        format!("{explanation} q-1001에서 고릅니다. 별도로 승인 여부를 알려 주세요."),
        format!("{explanation} q-1001에서 고릅니다. 배포할까요?"),
        format!("q-1001에서 고릅니다. {explanation} 승인 여부를 알려 주세요."),
        format!("{explanation} q-1001 카드에서 답해 주세요. 배포 승인 여부를 알려 주세요."),
        format!("{explanation} q-1001에서 진행할까요? 배포할까요?"),
        format!("{explanation} q-9999에서 고릅니다.\nA 통과(추천)\nB 더 고침"),
        format!("{explanation} q-1001에서 고릅니다.\n\n## 배포\nA 배포(추천)\nB 보류"),
        "q-1001에서 고르라고 적었습니다.\nA 통과(추천)\nB 더 고침".to_owned(),
        "보고: 「q-1001에서 진행할까요?」\nA 통과(추천)\nB 더 고침".to_owned(),
        "예시: q-1001에서 진행할까요?\nA 통과(추천)\nB 더 고침".to_owned(),
    ] {
        assert!(inspect(&text, &cards).missing.is_some(), "{text}");
    }
    for text in [
        "진행할까요?라고 물었다는 보고입니다.",
        "어느 안으로 할까요?라고 적었습니다.",
        "배포해 달라고 적었습니다.",
        "보고: 「q-1001에서 진행할까요?」",
    ] {
        let result = inspect(text, &cards);
        assert!(result.missing.is_none() && result.stale.is_none(), "{text}");
    }
    let text = format!("{explanation}\n\n승인 여부를 알려 주세요.");
    assert_eq!(inspect(&text, &cards).missing.unwrap().line, 3);
}

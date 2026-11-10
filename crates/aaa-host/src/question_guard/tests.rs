use super::*;

fn cards() -> HashMap<String, CardState> {
    HashMap::from([
        ("q-1001".into(), CardState::Open),
        ("q-1002".into(), CardState::Answered),
        ("q-1003".into(), CardState::Closed),
    ])
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

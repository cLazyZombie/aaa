use super::*;

fn answer_ids(text: &str) -> Vec<String> {
    super::answer_ids(text, &HashMap::new())
}

fn inspect(text: &str, cards: &HashMap<String, CardState>) -> Inspection {
    super::inspect(text, cards, &HashMap::new())
}

// 11-26 감사 기록과 완료 답변을 대조했다. 경고 구절과 카드 배치를 보존하고 링크 주소와 무관한 설명을 줄였다.
const NOV_26_REVIEW: &str = concat!(
    "서리 거인 발 구르기 시안을 검수해 주세요 → [q-4913 질문](#question=q-4913)\n\n",
    "**바뀐 점**\n- 뛰는 동작을 바꿨습니다.\n\n",
    "**알아 둘 점**\n- 영상은 두 개입니다.\n\n",
    "**선택지**\n",
    "- A(추천): 좋음. 설계 문서, 테스트, 검증까지 마무리합니다.\n",
    "- B: 한 단계 더 높고 무겁게 다시 찍습니다.\n",
    "- C: 고칠 곳을 글로 적어 주시면 반영합니다.",
);
const NOV_26_SCOPE: &str = concat!(
    "수리 범위를 정해 주셔야 합니다 → [q-4915 질문](#question=q-4915)\n\n",
    "**원인**\n- macOS 영상 재생 엔진에서 생기는 멈춤입니다.\n\n",
    "**thread가 낸 선택지**\n",
    "- **A(추천):** 원본 재생을 그대로 두고 플랫폼 결함으로 기록합니다.\n",
    "- **B:** 기존 영상을 보존하면서 따로 호환 재생본을 만드는 방안을 검토합니다.\n\n",
    "**제 의견**\n- 이 경우 카드에 직접 적어 주시면 됩니다.",
);
const NOV_26_NUMBERED: &str = concat!(
    "**질문**\n",
    "1. [q-4944 질문](#question=q-4944): 시안 전체. 추천은 **A 이대로 확정**입니다.\n",
    "2. [q-4945 질문](#question=q-4945): 나올 때 전환.\n",
    "3. [q-4946 질문](#question=q-4946): 여우 자리.\n",
    "4. [q-4947 질문](#question=q-4947): 대사. 추천은 **A 이대로**입니다. ",
    "고칠 대사가 있으면 B를 고르고 번호와 고칠 말을 채팅으로 알려 주세요.\n",
    "5. [q-4948 질문](#question=q-4948): 전환 길이.\n\n",
    "끝날 때 대사는 결과 하나에 2가지뿐입니다. 결과마다 5가지 이상을 원하시면 4번에서 B를 고르고 말씀해 주세요.",
);
const NOV_26_CORRECTION: &str = "내려찍는 순간 주인공이 피해를 입는다는 뜻으로 읽었습니다. 서 있는 동안 주인공이 거인을 때릴 틈을 말씀하신 거라면 알려 주세요.";
const NOV_26_DEPLOY_STATUS: &str = concat!(
    "요약 사전 검사 명령이 aaa main에 들어갔습니다.\n\n",
    "**배포할 변경**\n어제 배포 이후 aaa에 들어간 변경이 두 건 있습니다. 둘 다 아직 배포되지 않았습니다.\n",
    "- macOS WebKit 영상 탐색 시험 예외\n- 요약 사전 검사 명령\n\n",
    "배포할지 말씀해 주세요.",
);
const NOV_26_DEPLOY_CHAT: &str = "배포하려면 채팅으로 「배포해」라고 말씀해 주세요.";
const NOV_26_DEPLOY_CHAT_SHORT: &str = "배포하려면 채팅으로 「배포해」라고 해 주세요.";

fn november_26_cards() -> HashMap<String, CardState> {
    ["q-4913", "q-4915", "q-4944", "q-4945", "q-4946", "q-4947", "q-4948"]
        .into_iter()
        .map(|id| (id.to_owned(), CardState::Open))
        .collect()
}

#[test]
fn november_26_observed_card_and_chat_guidance() {
    let cards = november_26_cards();
    let mut unexpected = Vec::new();
    for (time, text) in [
        ("06:36", NOV_26_CORRECTION),
        ("06:49", NOV_26_REVIEW),
        ("07:08", NOV_26_SCOPE),
        ("08:32", NOV_26_DEPLOY_STATUS),
        ("14:48", NOV_26_DEPLOY_CHAT),
        ("16:05", NOV_26_NUMBERED),
        ("21:02", NOV_26_DEPLOY_CHAT_SHORT),
    ] {
        let result = inspect(text, &cards);
        if result.missing.is_some() || result.stale.is_some() {
            unexpected.push(format!("{time}: {:?} {:?}", result.missing, result.stale));
        }
    }
    assert!(unexpected.is_empty(), "{}", unexpected.join("\n"));
}

#[test]
fn november_26_guidance_preserves_separate_approvals_and_unknown_cards() {
    let cards = november_26_cards();
    let approval = "배포 승인 여부를 알려 주세요.";
    for guidance in [
        NOV_26_CORRECTION,
        NOV_26_REVIEW,
        NOV_26_SCOPE,
        NOV_26_NUMBERED,
        NOV_26_DEPLOY_STATUS,
        NOV_26_DEPLOY_CHAT,
        NOV_26_DEPLOY_CHAT_SHORT,
    ] {
        for text in [format!("{guidance}\n\n{approval}"), format!("{}, {approval}", guidance.trim_end_matches('.'))] {
            assert_eq!(inspect(&text, &cards).missing.map(|e| e.rule), Some("request"), "{text}");
        }
    }
    for text in [
        "배포할지 말씀해 주세요.".to_owned(),
        NOV_26_DEPLOY_STATUS.replace("배포할지", "지금 배포할지"),
        NOV_26_DEPLOY_STATUS.replace("둘 다 아직 배포되지 않았습니다.", "배포 승인을 기다립니다."),
        NOV_26_DEPLOY_CHAT.replace("「배포해」", "「배포를 승인해」"),
        NOV_26_DEPLOY_CHAT.replace("채팅으로 「배포해」라고", "배포 승인 여부를"),
        NOV_26_CORRECTION.replace("알려 주세요", "배포 승인 여부를 알려 주세요"),
        NOV_26_REVIEW.replace("q-4913", "q-9999"),
        NOV_26_SCOPE.replace("**원인**", "**별개 작업**"),
        NOV_26_NUMBERED.replace("q-4947", "q-9999"),
        NOV_26_NUMBERED.replace("고칠 말을 채팅으로", "배포 승인 여부를"),
        NOV_26_NUMBERED.replace("B를 고르고 번호와 고칠 말을 채팅으로", "배포 승인 여부를"),
        format!("{NOV_26_REVIEW}\n\n**별개 작업**\n\n**선택지**\n- A 배포 (추천)\n- B 보류"),
    ] {
        assert!(inspect(&text, &cards).missing.is_some(), "{text}");
    }
}

#[test]
fn november_26_numbered_choice_resolves_only_the_displayed_item() {
    let text = concat!(
        "3. [q-4946 질문](#question=q-4946): 여우 자리.\n",
        "1. [q-4947 질문](#question=q-4947): 대사.\n",
        "1. [q-4948 질문](#question=q-4948): 전환 길이.\n\n",
        "4번에서 **B**를 고르고 번호와 고칠 말을 채팅으로 알려 주세요.",
    );
    let mut cards = november_26_cards();
    let result = inspect(text, &cards);
    assert!(result.missing.is_none() && result.stale.is_none(), "{:?} {:?}", result.missing, result.stale);
    for state in [CardState::Answered, CardState::Closed] {
        cards.insert("q-4947".into(), state);
        let result = inspect(text, &cards);
        assert!(result.missing.is_none());
        assert_eq!(result.stale.unwrap().refs, [("q-4947".into(), state)]);
    }
    cards.remove("q-4947");
    assert!(inspect(text, &cards).missing.is_some());
    for text in [
        text.replace("4번에서", "6번에서"),
        text.replace("4번에서", "**다른 작업**\n\n4번에서"),
        text.replace("고칠 말을 채팅으로", "배포 승인 여부를"),
    ] {
        assert!(inspect(&text, &november_26_cards()).missing.is_some(), "{text}");
    }
}

/// 11-18의 오탐 세 건과 실제 터미널 조작 요청. 주소와 주변 작업 설명은 예시로 바꿨다.
#[test]
fn observed_model_offer_and_existing_card_options_do_not_warn() {
    let samples = [
        ("17:40", "기획서에서 맡긴 작업은 기본 모델로 시작됐습니다. 모델을 바꾸려면 말씀해 주세요.", None),
        (
            "18:00",
            "**정하실 것:** [q-4272 질문](https://example.ts.net/#question=q-4272)에서 실제 게임 화면을 보고 골라 주세요.\n- **A(추천):** 이대로 문서를 반영하고 전체 검증을 진행합니다.\n- **B:** 위치나 표시에서 바꿀 곳을 알려 주세요.",
            Some(("q-4272", CardState::Answered)),
        ),
        (
            "22:13",
            "**정하실 것:** [q-4295 질문](https://example.ts.net/#question=q-4295)에서 [웹 검수본](library/t-2022/index.html)을 보고 골라 주세요.\n- **A:** 좋습니다. 착지합니다.\n- **B:** 고칠 점을 글로 적어 주세요.",
            Some(("q-4295", CardState::Open)),
        ),
        // 아래 둘은 완료 사건·DB 경계 시험에 있던 같은 가지의 줄인 꼴이다.
        ("줄인 꼴", "기본 모델로 시작했습니다. 모델을 바꾸려면 말씀해 주세요.", None),
        (
            "줄인 꼴",
            "[질문](#question=q-4295)에서 [검수본](library/index.html)을 보고 골라 주세요.\n- **A(추천):** 진행합니다.\n- **B:** 고칠 곳을 알려 주세요.",
            Some(("q-4295", CardState::Open)),
        ),
    ];
    let mut failures = Vec::new();
    for (time, text, card) in samples {
        let cards = card.into_iter().map(|(id, state)| (id.to_owned(), state)).collect();
        let result = inspect(text, &cards);
        if result.missing.is_some() || result.stale.is_some() {
            failures.push(format!("{time}: missing={:?}, stale={:?}", result.missing, result.stale));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn model_offer_keeps_real_requests_for_input_and_terminal_actions() {
    for text in [
        "모델을 바꾸려면 말씀해 주세요, 배포 승인 여부를 알려 주세요.",
        "배포 승인 여부를 알려 주세요, 모델을 바꾸려면 말씀해 주세요.",
        "모델을 바꾸려면 쓸 모델 이름을 알려 주세요.",
        "새 작업에 쓸 모델을 골라 주세요.",
        "aaa 밖 터미널에서 프로필을 추가해 주세요.\n\n- **주의:** 실행해서 오류가 나면 출력을 알려 주세요.",
        "터미널에서 프로필을 추가해 주세요.\n\n실행해서 오류가 나면 출력을 알려 주세요.",
    ] {
        assert_eq!(inspect(text, &cards()).missing.unwrap().rule, "request", "{text}");
    }
}

#[test]
fn card_options_and_markdown_keep_separate_requests_and_card_statuses() {
    let cards = cards();
    let linked = "[질문](https://example.ts.net/#question=q-1001)에서 [웹 검수본?](library/preview(a).html?view=1) 보고 골라 주세요.";
    let options = "- **A(추천):** 진행합니다.\n- **B:** 고칠 곳을 알려 주세요.";
    let closed = linked.replace("q-1001", "q-1003");
    let result = inspect(&format!("{closed}\n{options}"), &cards);
    assert!(result.missing.is_none());
    assert_eq!(result.stale.unwrap().refs, [("q-1003".into(), CardState::Closed)]);
    for text in [
        linked.replace("q-1001", "q-9999"),
        format!("{linked} 배포 승인 여부를 알려 주세요."),
        format!("{linked}\n{options} 배포 승인 여부를 알려 주세요."),
        format!("{linked}\n{options}\n배포 승인 여부를 알려 주세요."),
        format!("{linked}\n{options}\n\n**별개 작업**\n배포 승인 여부를 알려 주세요."),
        format!("{linked}\n- **B:** 배포 승인 여부를 알려 주세요."),
    ] {
        assert!(inspect(&text, &cards).missing.is_some(), "{text}");
    }
}

/// t-1983의 네 실제 오탐 구절. 최신 카드 안내와 이미 적용한 기준만 재구성한다.
#[test]
fn observed_preview_card_replacements_and_corrections_do_not_require_answers() {
    let cards = HashMap::from([
        ("q-4026".into(), CardState::Closed),
        ("q-4027".into(), CardState::Open),
        ("q-4053".into(), CardState::Closed),
        ("q-4054".into(), CardState::Open),
    ]);
    let mut failures = Vec::new();
    for text in [
        "제가 정해 둔 것(다르면 말씀해 주세요)\n돈과 마석 처리 기준으로 작업을 시작했습니다.",
        "q-4027에서 답해 주세요.\n앞 시안 q-4026은 이 시안이 대신합니다.",
        "q-4054에서 답해 주세요.\n앞 카드 q-4053은 이 카드가 대신합니다.",
        "적용한 기준입니다.\n\n다르게 생각하시면 말씀해 주세요.",
        "제가 정해 둔 것(다르면 말씀해 주세요)\n이미 적용한 기준입니다.",
        "이미 적용한 기준입니다.\n\n다르게 생각하시면 말씀해 주세요.",
    ] {
        let result = inspect(text, &cards);
        if result.missing.is_some() || result.stale.is_some() {
            failures.push(format!("{text}: missing={:?}, stale={:?}", result.missing, result.stale));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// 정정 구절만 제외한다. 미결 설정·선택과 같은 문장 앞뒤의 별개 승인은 남긴다.
#[test]
fn observed_corrections_preserve_pending_decisions_and_separate_approvals() {
    let cards = cards();
    for feedback in ["다르면 말씀해 주세요", "다르게 생각하시면 말씀해 주세요"] {
        for text in [
            format!("{feedback}, 배포 승인 여부를 알려 주세요."),
            format!("배포 승인 여부를 알려 주세요, {feedback}."),
            format!("{feedback}. 지금 배포할까요?"),
            format!("제가 정해 둔 것({feedback})\n- A 배포 (추천)\n- B 보류"),
            format!("{feedback}.\n\n승인 여부를 알려 주세요."),
        ] {
            assert!(inspect(&text, &cards).missing.is_some(), "{text}");
        }
        let text = format!("{feedback}.\n\n승인 여부를 알려 주세요.");
        assert_eq!(inspect(&text, &cards).missing.unwrap().line, 3);
    }
    // 점검에서 정상 경고로 남긴 두 사건: 원문 전체가 없어 결정 내용과 선택지는 재구성한다.
    for text in [
        "기획 폴더를 docs/design으로 바꾸려면 알려 주세요.",
        "보스 싸움 중 두루마리 허용 여부를 바꾸려면 알려 주세요.\n- A 허용 (추천)\n- B 금지",
        "다르면 배포 승인 여부를 말씀해 주세요.",
        "다르게 생각하시면 배포 승인 여부를 말씀해 주세요.",
        "다르게 생각하시면 복구와 삭제 중 하나를 골라 주세요.",
    ] {
        assert!(inspect(text, &cards).missing.is_some(), "{text}");
    }
}

/// t-1896에서 관측한 교체 설명. 링크는 재구성하고 교체 문장은 그대로 보존한다.
#[test]
fn observed_replacement_explanation_is_not_an_old_card_answer_request() {
    let cards = HashMap::from([("q-3834".into(), CardState::Open), ("q-3822".into(), CardState::Closed)]);
    let text = "[q-3834 질문](#question=q-3834)에서 답해 주세요.\n앞서 열려 있던 q-3822는 이 질문이 대신합니다.";
    let result = inspect(text, &cards);
    assert!(
        result.missing.is_none() && result.stale.is_none(),
        "missing={:?}, stale={:?}",
        result.missing,
        result.stale
    );
}

/// 11-21에 관측한 교체 문장. 링크 주소와 답변 안내는 예시다.
#[test]
fn observed_explicit_replacement_keeps_only_the_new_card_in_the_explanation() {
    let old = "q-4584";
    let new = "q-4591";
    let cards = HashMap::from([(old.into(), CardState::Closed), (new.into(), CardState::Open)]);
    let replacements = HashMap::from([(old.into(), new.into())]);
    let history = "q-4584에서 [q-4591 질문](#question=q-4591)으로 바뀌었습니다";
    let text = format!("아래 새 카드로 답해 주세요.\n\n{history}.");
    let result = super::inspect(&text, &cards, &replacements);
    assert!(result.missing.is_none() && result.stale.is_none(), "{:?}", result.stale);
    assert_eq!(super::answer_ids(history, &replacements), [new]);

    let unconfirmed = inspect(&text, &cards);
    assert_eq!(unconfirmed.stale.unwrap().refs, [(old.into(), CardState::Closed), (new.into(), CardState::Open)]);
    let closed_cards = HashMap::from([(old.into(), CardState::Closed), (new.into(), CardState::Closed)]);
    assert!(inspect(&text, &closed_cards).stale.is_some());

    for request in ["q-4584에서 답해 주세요.", "별도로 q-4584에서 배포 승인 여부를 알려 주세요."]
    {
        let result = super::inspect(&format!("{history}. {request}"), &cards, &replacements);
        assert!(result.missing.is_none(), "{request}");
        assert_eq!(result.stale.unwrap().refs, [(old.into(), CardState::Closed)], "{request}");
    }
    let text = format!("{history}. 별도로 배포 승인 여부를 알려 주세요.");
    let result = super::inspect(&text, &cards, &replacements);
    assert!(result.missing.is_some() && result.stale.is_none());
}

/// 교체 경위는 카드 수를 채우지 않고 실제 옛 카드 안내·별개 승인을 면제하지 않는다.
#[test]
fn replacement_explanations_keep_answer_targets_and_separate_requests() {
    let cards = cards();
    for history in [
        "앞서 열려 있던 q-1003는 이 질문이 대신합니다.",
        "앞서 열려 있던 [q-1003 질문](#question=q-1003)은 이 질문이 대신합니다.",
        "앞 시안 q-1003은 이 시안이 대신합니다.",
        "앞 시안 [q-1003 시안](#question=q-1003)은 이 시안이 대신합니다.",
        "앞 카드 q-1003은 이 카드가 대신합니다.",
        "앞 카드 [q-1003 카드](#question=q-1003)은 이 카드가 대신합니다.",
    ] {
        assert!(answer_ids(history).is_empty(), "{history}");
        let linked = format!("q-1001에서 답해 주세요.\n{history}");
        for text in [
            linked.clone(),
            format!("{history}\n\nq-1001에서 답합니다.\n- A 진행 (추천)\n- B 보류"),
            format!("아래 새 카드로 답해 주세요.\n\n**현재 카드**\nq-1001\n{history}"),
        ] {
            let result = inspect(&text, &cards);
            assert!(result.missing.is_none() && result.stale.is_none(), "{text}");
        }
        let text = format!("질문 2개에 답해 주세요.\n\nq-1001\n{history}");
        assert!(inspect(&text, &cards).missing.is_some(), "{text}");
        for text in [
            format!("{linked} q-1003에서 답해 주세요."),
            format!("q-1003에서 선택해 주세요. {history}"),
            format!("{linked}\n\nq-1003에서 판단해 주세요.\n- A 진행 (추천)\n- B 보류"),
        ] {
            let result = inspect(&text, &cards);
            assert!(result.missing.is_none(), "{text}");
            assert!(
                result.stale.as_ref().is_some_and(|e| e.refs.contains(&("q-1003".into(), CardState::Closed))),
                "{text}: stale={:?}",
                result.stale
            );
        }
        for request in ["지금 배포할까요?", "별도로 배포 승인 여부를 알려 주세요."] {
            for text in
                [format!("{linked} {request}"), format!("{linked}\n\n{request}"), format!("{request}\n\n{history}")]
            {
                let result = inspect(&text, &cards);
                assert!(result.missing.is_some() && result.stale.is_none(), "{text}");
            }
        }
    }
    // 대체 주체가 옛 카드이거나, 대체 여부를 묻거나, 설명이 아직 이어지면 제외하지 않는다.
    for text in [
        "q-1003가 이 질문을 대신합니다.",
        "q-1003는 이 질문이 대신할까요?",
        "q-1003는 이 질문이 대신합니다만 아직 답이 필요합니다.",
        "q-1003가 이 시안을 대신합니다.",
        "q-1003가 이 카드를 대신합니다.",
        "q-1003는 이 시안이 대신할까요?",
        "q-1003는 이 카드가 대신할까요?",
        "q-1003는 이 시안이 대신합니다만 아직 답이 필요합니다.",
        "q-1003는 이 카드가 대신합니다만 아직 답이 필요합니다.",
        "q-1003는 이 카드가 대신합니다에서 선택해 주세요.",
        "q-1003는 이 시안이 대신합니다에서 답해 주세요.",
    ] {
        assert_eq!(answer_ids(text), ["q-1003"], "{text}");
    }
}

/// t-1877의 배포 후 오탐 다섯 발췌. 카드 상태와 주변 선택지만 재구성한다.
#[test]
fn observed_discarded_cards_and_optional_corrections_are_not_answer_requests() {
    let cards = HashMap::from([
        ("q-3744".into(), CardState::Closed),
        ("q-3746".into(), CardState::Open),
        ("q-3747".into(), CardState::Open),
        ("q-3779".into(), CardState::Closed),
        ("q-3780".into(), CardState::Open),
    ]);
    let samples = [
        "q-3746·q-3747에서 답해 주세요.\n앞에 안내한 q-3744는 닫혔으니 답하지 않으셔도 됩니다.",
        "「3발부터」를 3발 이하일 때로 읽었습니다.\n\n다르게 뜻하셨으면 말씀해 주세요.",
        "제가 만든 q-3779는 닫았습니다. 이제 [q-3780 질문](#question=q-3780) 하나에만 답하시면 됩니다.\n- A 진행 (추천)\n- B 보류",
        "작업은 이미 시작했고 종을 찾아 시안 작성 중입니다.\n\n다른 종을 말씀하신 거라면 알려 주세요.",
        "앞 경고는 선택적 정정을 질문으로 잘못 센 것입니다. 다른 종이라면 그때 말씀해 주세요.",
    ];
    let mut failures = Vec::new();
    for (n, text) in samples.iter().enumerate() {
        let result = inspect(text, &cards);
        if result.missing.is_some() || result.stale.is_some() {
            failures.push(format!("발췌 {}: missing={:?}, stale={:?}", n + 1, result.missing, result.stale));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    // 같은 가지의 줄인 꼴(완료 사건·DB 경계 시험에 있던 것)
    for text in [
        "작업은 이미 시작했습니다.\n\n다른 종을 말씀하신 거라면 알려 주세요.",
        "앞 경고는 정정 안내를 잘못 센 것입니다. 다른 종이라면 그때 말씀해 주세요.",
    ] {
        let result = inspect(text, &cards);
        assert!(result.missing.is_none() && result.stale.is_none(), "{text}");
    }

    for text in samples {
        for request in ["지금 배포할까요?", "배포 승인 여부를 알려 주세요.", "추가로 진행할까요?"]
        {
            assert!(inspect(&format!("{text}\n\n{request}"), &cards).missing.is_some(), "{text}\n{request}");
        }
    }
    for feedback in [samples[1], samples[3], samples[4]] {
        let feedback = feedback.rsplit_once('\n').map_or(feedback, |(_, tail)| tail);
        for text in [
            format!("{}, 배포 승인 여부를 알려 주세요.", feedback.trim_end_matches('.')),
            format!("배포 승인 여부를 알려 주세요, {feedback}"),
            format!("{feedback}\n- A 진행 (추천)\n- B 보류"),
        ] {
            assert!(inspect(&text, &cards).missing.is_some(), "{text}");
        }
    }
    for text in [
        "다르게 뜻하셨으면 배포 승인 여부를 말씀해 주세요.",
        "다른 종을 말씀하신 거라면 복구와 삭제 중 하나를 골라 주세요.",
        "다른 종이라면 그때 배포 승인 여부를 알려 주세요.",
        "다른 배포 방식이라면 말씀해 주세요.",
    ] {
        assert!(inspect(text, &cards).missing.is_some(), "{text}");
    }
}

#[test]
fn discarded_card_references_do_not_cover_or_count_as_answer_targets() {
    let cards = cards();
    for discarded in [
        "q-1003는 닫혔으니 답하지 않으셔도 됩니다.",
        "제가 만든 q-1003는 닫았습니다.",
        "[q-1003 질문](#question=q-1003)은 닫았습니다.",
        "q-1003에는 답하지 않아도 됩니다.",
    ] {
        for text in [
            format!("q-1001에서 답해 주세요. {discarded}"),
            format!("{discarded}\n\nq-1001에서 답합니다.\n\n**선택지**\n- A 진행 (추천)\n- B 보류"),
            format!("아래 새 카드로 답해 주세요.\n\n**카드**\nq-1001\n{discarded}"),
        ] {
            let result = inspect(&text, &cards);
            assert!(result.missing.is_none() && result.stale.is_none(), "{text}");
        }
        let text = format!("질문 2개에 답해 주세요.\n\nq-1001\n{discarded}");
        assert!(inspect(&text, &cards).missing.is_some(), "{text}");
        for text in [
            format!("{discarded} q-1003에서 답해 주세요."),
            format!("q-1003에서 답해 주세요. {discarded}"),
            format!("{discarded}\n\nq-1003에서 선택해 주세요."),
        ] {
            assert_eq!(inspect(&text, &cards).stale.unwrap().refs, [("q-1003".into(), CardState::Closed)], "{text}");
        }
        assert!(inspect(&format!("{discarded}\n\n지금 배포할까요?"), &cards).missing.is_some());
    }
}

/// t-2184의 15:55 폐기 문장과 새 카드 안내를 재현한다. 링크와 선택지 내용은 예시다.
#[test]
fn observed_withdrawal_needs_a_confirmed_open_successor() {
    let old = "q-4753";
    let new = "q-4754";
    let cards = HashMap::from([(old.into(), CardState::Closed), (new.into(), CardState::Open)]);
    let history = "앞서 올린 순서 카드 q-4753은 상황이 바뀌어 닫았습니다.";
    let notice = "이제 새 카드 [q-4754](#question=q-4754)에서 답해 주세요.";
    let options = "- A 진행 (추천)\n- B 보류";
    for gap in [" ", "\n", "\n\n"] {
        let text = format!("{history}{gap}{notice}\n{options}");
        let result = inspect(&text, &cards);
        assert!(result.missing.is_none() && result.stale.is_none(), "{text}: {:?}", result.stale);
    }
    let text = format!("{history} 지금 답할 카드는 **[q-4754](#question=q-4754) 하나**입니다.\n{options}");
    let result = inspect(&text, &cards);
    assert!(result.missing.is_none() && result.stale.is_none(), "{text}");

    let text = format!("{history} {notice}\n{options}");
    let unconfirmed = HashMap::from([(old.into(), CardState::Closed), (new.into(), CardState::Answered)]);
    assert!(inspect(&text, &unconfirmed).stale.is_some());
    assert!(inspect(&text, &HashMap::from([(new.into(), CardState::Open)])).missing.is_some());

    for text in [
        format!("{history} 다른 작업의 q-4754도 열려 있습니다.\n{options}"),
        format!("{history} q-4754에서 별개 질문에 답해 주세요.\n{options}"),
        format!("{history} {notice} q-4753에서 답해 주세요."),
        format!("{history} q-4753에서 답해 주세요. {notice}"),
    ] {
        let result = inspect(&text, &cards);
        assert!(result.stale.unwrap().refs.contains(&(old.into(), CardState::Closed)), "{text}");
    }
    let text = format!("{history} {notice}\n{options}\n\n별도로 배포 승인 여부를 알려 주세요.");
    assert!(inspect(&text, &cards).missing.is_some(), "{text}");
    let text = format!("질문 2개에 답해 주세요.\n\n{history} {notice}\n{options}");
    assert!(inspect(&text, &cards).missing.is_some(), "{text}");
}

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
            "일반 안내와 카드 사이의 다른 절",
            "질문 하나에 답해 주세요.\n\n**작업 결과**\n내용입니다.\n\n**정할 것**\n[q-3578 질문](#question=q-3578)",
            false,
            false,
        ),
        (
            "앞 카드와 뒤 선택지 둘",
            "[q-3619 질문](#question=q-3619)을 열었습니다.\n\n**고칠 방식**\n- A 진행 (추천)\n- B 보류",
            false,
            false,
        ),
        (
            "선택지 제목과 선택지 둘",
            "[q-3706 질문](#question=q-3706)을 열었습니다.\n\n**선택지**\n- A 진행 (추천)\n- B 보류\n\nA를 고르시면 이어 갑니다.",
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
        "지금 정해 주실 것은 없습니다. 별도로 배포할까요?",
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

const RECORDING_CARD: &str =
    "- [q-4689 녹화 카드](#question=q-4689)에서 cold start 화면 녹화를 받을 수 있는지 답해 주세요.";
const RECORDING_DETAILS: &str = "Wi-Fi였는지 셀룰러였는지, 홈 화면 앱인지 Safari 탭인지도 함께 알려 주세요.";

#[test]
fn observed_recording_details_link_to_the_open_card_in_the_same_item() {
    let cards = HashMap::from([("q-4689".into(), CardState::Open), ("q-4690".into(), CardState::Open)]);
    let other = "- [q-4690 질문](#question=q-4690)에서 답해 주세요.";
    for text in [
        format!("{other}\n{RECORDING_CARD}\n  {RECORDING_DETAILS}"),
        format!("{RECORDING_CARD}\n  {RECORDING_DETAILS}\n{other}"),
        format!("{RECORDING_CARD}\n\n  {RECORDING_DETAILS}"),
        format!("10. {}\n    {RECORDING_DETAILS}", &RECORDING_CARD[2..]),
        format!("-\t{}\n\t{RECORDING_DETAILS}", &RECORDING_CARD[2..]),
        format!("{RECORDING_CARD}\n  기기 모델도 말씀해 주세요."),
    ] {
        let result = inspect(&text, &cards);
        assert!(result.missing.is_none() && result.stale.is_none(), "{text}");
    }
}

#[test]
fn recording_card_keeps_separate_requests_and_questions_visible() {
    let cards = HashMap::from([("q-4689".into(), CardState::Open)]);
    for (text, line) in [
        (format!("{RECORDING_CARD}\n\n{RECORDING_DETAILS}"), 3),
        (format!("{RECORDING_CARD}\n{RECORDING_DETAILS}"), 2),
        (format!("{RECORDING_CARD}\n {RECORDING_DETAILS}"), 2),
        (format!("{RECORDING_CARD}\n- {RECORDING_DETAILS}"), 2),
        (format!("{RECORDING_CARD}\n  - {RECORDING_DETAILS}"), 2),
        (format!("{RECORDING_CARD}\n- 녹화 파일입니다.\n  {RECORDING_DETAILS}"), 3),
        (format!("{RECORDING_CARD}\n  {RECORDING_DETAILS}\n  배포 승인 여부를 알려 주세요."), 3),
        (format!("{RECORDING_CARD}\n  배포할지도 함께 알려 주세요."), 2),
        (format!("{RECORDING_CARD}\n  배포해도 되는지도 함께 알려 주세요."), 2),
        (format!("{RECORDING_CARD}\n  승인 여부도 함께 알려 주세요."), 2),
        (format!("{RECORDING_CARD}\n  배포할까요?"), 2),
        (format!("10. {}\n    배포할까요?", &RECORDING_CARD[2..]), 2),
        (format!("{RECORDING_CARD}\n  {RECORDING_DETAILS} 배포 승인 여부를 알려 주세요."), 2),
        (format!("{RECORDING_CARD}\n  별도로 {RECORDING_DETAILS}"), 2),
    ] {
        assert_eq!(inspect(&text, &cards).missing.map(|e| e.line), Some(line), "{text}");
    }
}

#[test]
fn recording_details_require_a_single_open_card() {
    for state in [None, Some(CardState::Answered), Some(CardState::Closed)] {
        let cards = state.into_iter().map(|state| ("q-4689".into(), state)).collect();
        let text = format!("- [q-4689 녹화 카드](#question=q-4689)를 안내합니다.\n  {RECORDING_DETAILS}");
        assert_eq!(inspect(&text, &cards).missing.map(|e| e.line), Some(2), "{state:?}");
    }
    let cards = HashMap::from([("q-4689".into(), CardState::Open), ("q-4690".into(), CardState::Open)]);
    let text = format!(
        "- [q-4689 녹화 카드](#question=q-4689)와 [q-4690 질문](#question=q-4690)을 안내합니다.\n  {RECORDING_DETAILS}"
    );
    assert_eq!(inspect(&text, &cards).missing.map(|e| e.line), Some(2));
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

/// t-2064의 이름 의견은 앞에 확정 안내가 있을 때만 선택 사항이다.
#[test]
fn optional_feedback_on_a_decided_name_does_not_hide_requests() {
    let cards = cards();
    let decided = "이름은 제가 `docs/design/themes/`로 정했습니다.";
    let feedback = "다른 이름이 좋으시면 말씀해 주세요";
    for gap in [" ", "\n", "\n\n"] {
        let text = format!("{decided}{gap}{feedback}.");
        let result = inspect(&text, &cards);
        assert!(result.missing.is_none() && result.stale.is_none(), "{text}");
    }
    for text in [
        format!("{feedback}."),
        format!("이름은 아직 정하지 않았습니다. {feedback}."),
        format!("이름은 이쪽으로 정했습니다? {feedback}."),
        format!("{decided}\n\n## 다른 폴더\n{feedback}."),
        format!("{decided} 파일은 옮기지 않았습니다. {feedback}."),
        format!("{decided} 어느 이름으로 할까요?"),
        format!("{decided} 다른 이름이 좋으시면 새 이름을 정해 주세요."),
        format!("{decided} 다른 이름이 좋으시면 변경 승인 여부를 말씀해 주세요."),
        format!("{decided} {feedback}. 별도로 배포할까요?"),
        format!("{decided} {feedback}.\n- A 현재 이름 (추천)\n- B 새 이름"),
        "새 원격 저장소를 만들까요?".into(),
        "폴더 이름은 테마로 정했습니다. 다른 이름이 좋으시면 말씀해 주세요, 배포 승인 여부를 알려 주세요.".into(),
        "폴더 이름은 아직 정하지 않았습니다. 어느 이름으로 할까요?".into(),
    ] {
        assert!(inspect(&text, &cards).missing.is_some(), "{text}");
    }
    for text in [
        format!("{decided} {feedback}, 배포 승인 여부를 알려 주세요."),
        format!("{decided} 배포 승인 여부를 알려 주세요, {feedback}."),
    ] {
        assert!(inspect(&text, &cards).missing.is_some(), "{text}");
        let text = prose(&text);
        let detected = requests(&text, false, None).into_iter().map(|r| (r.rule, r.offset)).collect::<Vec<_>>();
        assert_eq!(detected, [("request", text.find("알려 주세요").unwrap())], "{text}");
    }
}

/// t-2184의 12:47·17:30 의견 요청. 앞 완료 설명과 인용되지 않은 문구는 점검 기록대로 재구성한다.
#[test]
fn optional_feedback_on_completed_results_keeps_decisions_visible() {
    let cards = cards();
    for (completed, feedback) in [
        ("시험 카드는 이미 처리했습니다.", "체감이 어땠는지 알려 주세요"),
        ("빛 반경은 그대로 두기로 확정했습니다.", "반경 변경도 하지 않으려던 것이면 말씀해 주세요"),
    ] {
        for gap in [" ", "\n", "\n\n"] {
            let text = format!("{completed}{gap}{feedback}.");
            let result = inspect(&text, &cards);
            assert!(result.missing.is_none() && result.stale.is_none(), "{text}");
        }
        for text in [
            format!("{feedback}."),
            format!("시험 카드는 아직 처리하지 않았습니다. {feedback}."),
            format!("{completed}\n\n## 다른 작업\n{feedback}."),
            format!("{completed} {feedback}, 임시 브랜치 삭제 승인 여부를 알려 주세요."),
            format!("{completed} 임시 브랜치 삭제 승인 여부를 알려 주세요, {feedback}."),
            format!("{completed} {feedback}. 지금 배포할까요?"),
            format!("{completed} {feedback}.\n- A 진행 (추천)\n- B 보류"),
        ] {
            assert!(inspect(&text, &cards).missing.is_some(), "{text}");
        }
    }
    for text in [
        "시험 카드를 처리했습니다. 체감이 어땠는지와 배포 승인 여부를 알려 주세요.",
        "빛 반경은 확정했습니다. 반경 변경도 하지 않으려던 것이면 배포 승인 여부를 말씀해 주세요.",
    ] {
        assert!(inspect(text, &cards).missing.is_some(), "{text}");
    }
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

#[test]
fn observed_open_card_guidance_follows_one_card_across_explanations() {
    let cards = cards();
    let card = "[검수 질문](https://example.ts.net/#question=q-1001)";
    for (announcement, guidance) in [
        (format!("질문은 {card}에 열려 있습니다. "), "보시고 하나를 골라 주세요."),
        (format!("{card}입니다. 화면에서 두 시안을 볼 수 있습니다.\n\n"), "화면을 보신 뒤 원하는 안을 선택해 주세요."),
        (format!("{card}\n\n[시안](library/a.b.html?view=1)도 있습니다.\n\n"), "확인하신 뒤 한 가지를 선택해 주세요."),
        (format!("- {card}를 열었습니다.\n\n  화면 두 장을 같이 올렸습니다.\n  "), "A/B 중 하나를 골라 주세요."),
    ] {
        let text = format!("{announcement}{guidance}");
        let result = inspect(&text, &cards);
        assert!(result.missing.is_none() && result.stale.is_none(), "{text}: {:?} {:?}", result.missing, result.stale);

        let markdown = markdown(&text);
        let blocks = blocks(&text, &markdown);
        let current = blocks.last().unwrap();
        let prose = request_prose(&current.text);
        let detected = requests(&prose, false, None);
        let [request] = detected.as_slice() else { panic!("선택 안내 한 건: {text}") };
        let links = CardLinks {
            blocks: &blocks,
            markdown: &markdown,
            replacements: &HashMap::new(),
            cards: &cards,
            reply_ids: Vec::new(),
        };
        let line = text.lines().count();
        let link = links.link(blocks.len() - 1, request, &prose, line, false);
        assert_eq!(link.evidence(request, line, &cards).refs, [("q-1001".into(), CardState::Open)]);
    }
}

#[test]
fn general_guidance_checks_card_state_and_topic_boundaries() {
    let mut cards = cards();
    cards.insert("q-1004".into(), CardState::Open);
    let guide = "보시고 하나를 골라 주세요.";
    let closed = inspect(&format!("q-1003에 카드가 있습니다.\n\n{guide}"), &cards);
    assert!(closed.missing.is_none());
    assert_eq!(closed.stale.unwrap().refs, [("q-1003".into(), CardState::Closed)]);
    let answered = inspect(&format!("q-1002에 카드가 있습니다.\n\n{guide}"), &cards);
    assert!(answered.missing.is_none());
    assert_eq!(answered.stale.unwrap().refs, [("q-1002".into(), CardState::Answered)]);

    for (announcement, line) in [
        ("", 1),
        ("q-9999에 카드가 있습니다. ", 1),
        ("q-1001과 q-1004가 열려 있습니다.\n\n", 3),
        ("q-1001과 q-9999가 열려 있습니다.\n\n", 3),
        ("q-1001 카드입니다.\n\n## 별개 작업\n", 4),
        ("- [질문](#question=q-1001)입니다.\n- 다른 시안입니다.\n  ", 3),
        ("q-1003은 닫았습니다.\n\n", 3),
    ] {
        let text = format!("{announcement}{guide}");
        let result = inspect(&text, &cards);
        assert_eq!(result.missing.map(|e| (e.rule, e.line, e.refs)), Some(("request", line, Vec::new())), "{text}");
        assert!(result.stale.is_none(), "{text}");
    }
}

#[test]
fn selection_guidance_keeps_the_branch_deletion_approval_and_new_questions_visible() {
    let cards = cards();
    let card = "[시안 검수](#question=q-1001)를 열었습니다.";
    let approval = "로컬 시안 브랜치 삭제 승인 여부를 알려 주세요.";
    for (text, line, rule) in [
        (approval.to_owned(), 1, "request"),
        (format!("{card}\n\n보시고 하나를 골라 주세요, {approval}"), 3, "request"),
        (format!("[시안 검수](#question=q-1001)에서 하나를 골라 주세요, {approval}"), 1, "request"),
        (format!("{card}\n\n{approval} 보시고 하나를 골라 주세요."), 3, "request"),
        (format!("{card}\n\n보시고 하나를 골라 주세요. 로컬 시안 브랜치를 지울까요?"), 3, "question"),
        (format!("{card}\n\n브랜치를 지울지 정해 주세요.\n\nq-1001에서 답합니다."), 3, "request"),
        (format!("{card}\n\n카드에서 브랜치 삭제 승인 여부를 알려 주세요."), 3, "request"),
        ("카드가 뜨면 답해 주세요, 로컬 시안 브랜치 삭제 승인 여부를 알려 주세요.".into(), 1, "request"),
    ] {
        let result = inspect(&text, &cards);
        assert_eq!(result.missing.map(|e| (e.rule, e.line, e.refs)), Some((rule, line, Vec::new())), "{text}");
        assert!(result.stale.is_none(), "{text}");
    }
}

#[test]
fn observed_two_card_guidance_names_the_numbered_cards() {
    let text = concat!(
        "어제 사용 점검에서 고칠 곳 두 가지를 찾았습니다.\n\n",
        "1. [q-4843 질문](#question=q-4843) **보고 첨부가 깨지는 문제:** 마지막 사본은 열립니다.\n",
        "2. [q-4844 질문](#question=q-4844) **질문 감시 문제:** 기존 q-4834 안내에서 잘못 경고했습니다.\n\n",
        "두 질문 모두 **A(할 일로 만들기)**를 추천합니다. 지금 당장 불편은 없습니다. ",
        "바로 고치려면 B, 넘기려면 C를 골라 주세요.",
    );
    let mut cards = HashMap::from([
        ("q-4843".into(), CardState::Open),
        ("q-4844".into(), CardState::Open),
        ("q-4834".into(), CardState::Answered),
    ]);
    let clean = inspect(text, &cards);
    assert!(clean.missing.is_none() && clean.stale.is_none(), "{:?} {:?}", clean.missing, clean.stale);
    for unrelated in [
        text.replace("두 질문 모두", "추천은"),
        text.replace("두 질문 모두", "세 질문 모두"),
        text.replace("1. [q-4843", "- [q-4843").replace("2. [q-4844", "- [q-4844"),
        text.replace("두 질문 모두", "## 다른 작업\n\n두 질문 모두"),
        text.replace("두 질문 모두", "다른 작업\n---------\n\n두 질문 모두"),
        format!("{text} 로컬 시안 브랜치 삭제 승인 여부를 알려 주세요."),
    ] {
        let result = inspect(&unrelated, &cards);
        assert!(result.missing.is_some() && result.stale.is_none(), "{unrelated}");
    }
    cards.insert("q-4844".into(), CardState::Closed);
    let stale = inspect(text, &cards);
    assert!(stale.missing.is_none());
    assert_eq!(stale.stale.unwrap().refs, [("q-4843".into(), CardState::Open), ("q-4844".into(), CardState::Closed)]);
    cards.remove("q-4844");
    assert!(inspect(text, &cards).missing.is_some());
}

use super::*;

/// 멈춤과 일꾼은 단계마다 하나만 이긴다: 쓰기 전 멈춤은 아무것도 쓰지 않고, 글을 쓰는 중·쓴 뒤 멈춤은 Enter를 막고,
/// Enter를 보내기 시작한 뒤 멈춤은 늦다(plan §8.247)
#[test]
fn cancel_races_each_write() {
    let stage = AtomicU8::new(SUBMIT_BEFORE);
    assert_eq!(cancel_submit(&stage), SubmitStage::NotSent);
    assert!(!advance_submit(&stage, SUBMIT_BEFORE, SUBMIT_TYPING), "멈춘 submit이 글을 썼다");
    assert_eq!(cancel_submit(&stage), SubmitStage::NotSent, "두 번째 멈춤은 처음 답");

    for typed in [SUBMIT_TYPING, SUBMIT_TYPED] {
        let stage = AtomicU8::new(SUBMIT_BEFORE);
        assert!(advance_submit(&stage, SUBMIT_BEFORE, SUBMIT_TYPING));
        if typed == SUBMIT_TYPED {
            assert!(advance_submit(&stage, SUBMIT_TYPING, SUBMIT_TYPED));
        }
        assert_eq!(cancel_submit(&stage), SubmitStage::NoEnter);
        assert!(!advance_submit(&stage, SUBMIT_TYPING, SUBMIT_TYPED));
        assert!(!advance_submit(&stage, SUBMIT_TYPED, SUBMIT_ENTERING), "멈춘 submit이 Enter를 보냈다");
    }

    let stage = AtomicU8::new(SUBMIT_BEFORE);
    for (from, to) in [(SUBMIT_BEFORE, SUBMIT_TYPING), (SUBMIT_TYPING, SUBMIT_TYPED), (SUBMIT_TYPED, SUBMIT_ENTERING)] {
        assert!(advance_submit(&stage, from, to));
    }
    assert_eq!(cancel_submit(&stage), SubmitStage::TooLate);
    assert_eq!(stage.load(Ordering::SeqCst), SUBMIT_ENTERING, "늦은 멈춤이 단계를 바꿨다");
}

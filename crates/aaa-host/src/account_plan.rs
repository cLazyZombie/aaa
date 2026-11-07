//! 요금제 정책 정본. 근거와 갱신 기준은 docs/account-plans.md에 둔다.
use aaa_proto::AccountPlanOption;

/// (harness, ID, 표시 이름, 5시간 창, 월 금액 달러). 금액은 자동 배정 배수(금액 ÷ 100)에도 쓴다.
type Plan = (&'static str, &'static str, &'static str, Option<bool>, Option<u32>);
const PLANS: [Plan; 17] = [
    ("claude", "free", "Free", Some(true), None),
    ("claude", "pro", "Pro · $20/월", Some(true), Some(20)),
    ("claude", "max", "Max · $100/월", Some(true), Some(100)),
    ("claude", "ultra", "Ultra · $200/월 · 인증됨", Some(false), Some(200)),
    ("claude", "ultra_pending", "Ultra · $200/월 · 임시 5시간 제한", Some(true), Some(200)),
    ("claude", "team_standard", "Team Standard", Some(true), None),
    ("claude", "team_premium", "Team Premium", Some(true), None),
    ("claude", "enterprise", "Enterprise", None, None),
    ("codex", "free", "Free", None, None),
    ("codex", "go", "Go · $8/월", None, Some(8)),
    ("codex", "plus", "Plus · $20/월", Some(true), Some(20)),
    ("codex", "pro_100", "Pro · $100/월", Some(false), Some(100)),
    ("codex", "pro_200", "Pro · $200/월", Some(false), Some(200)),
    ("codex", "pro_500", "Pro · $500/월", Some(false), Some(500)),
    ("codex", "business", "Business", Some(true), None),
    ("codex", "enterprise", "Enterprise", None, None),
    ("codex", "edu", "Edu", None, None),
];

pub(crate) fn options() -> Vec<AccountPlanOption> {
    PLANS
        .into_iter()
        .map(|(harness, id, label, session_limit, _)| AccountPlanOption {
            id: id.into(),
            harness: harness.into(),
            label: label.into(),
            session_limit,
        })
        .collect()
}

/// 금액·인증 여부가 없는 모호한 이름은 추측하지 않는다.
/// 옛 Max 20x는 Ultra로 이전됐지만 인증 상태를 모르므로 자동으로 창을 끄지 않는다.
pub(crate) fn resolve(harness: &str, selected: Option<&str>, detected: Option<&str>) -> Option<AccountPlanOption> {
    let id = resolved_id(harness, selected, detected)?;
    options().into_iter().find(|p| p.harness == harness && p.id == id)
}

fn resolved_id<'a>(harness: &str, selected: Option<&'a str>, detected: Option<&str>) -> Option<&'a str> {
    if selected.is_some() {
        return selected;
    }
    let detected = detected?.to_ascii_lowercase();
    let id = match (harness, detected.as_str()) {
        ("codex", "pro") => "pro_200", // 금액 표시는 기존 감지값 그대로 두며 창 정책만 공유한다.
        ("codex", "team") => "business",
        ("claude", "ultra" | "max" | "max_20x" | "max20x") => return None,
        (_, value) => PLANS.iter().find(|p| p.0 == harness && p.1 == value)?.1,
    };
    Some(id)
}

/// 자동 배정의 요금제 배수(월 금액 ÷ 100). 금액이 없는 요금제와 금액 단계를 모르는 감지값은 None(미확인)이다.
/// Codex 감지값 `pro`는 $100/$200/$500을 가르지 않으므로 사람이 고른 값이 있어야 배수를 안다.
pub(crate) fn weight(harness: &str, selected: Option<&str>, detected: Option<&str>) -> Option<f64> {
    if selected.is_none() && harness == "codex" && detected.is_some_and(|d| d.eq_ignore_ascii_case("pro")) {
        return None;
    }
    let id = resolved_id(harness, selected, detected)?;
    let price = PLANS.iter().find(|p| p.0 == harness && p.1 == id)?.4?;
    Some(f64::from(price) / 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    // 필요: 사용자가 정한 배수(100$=1, 200$=2, 500$=5)와 모호한 감지값을 추측하지 않는 계약.
    #[test]
    fn weight_follows_monthly_price_and_never_guesses_ambiguous_detection() {
        assert_eq!(weight("claude", Some("ultra"), None), Some(2.0));
        assert_eq!(weight("claude", Some("max"), Some("pro")), Some(1.0));
        assert_eq!(weight("claude", None, Some("pro")), Some(0.2));
        assert_eq!(weight("claude", None, Some("max")), None, "옛 max는 $100/$200을 모른다");
        assert_eq!(weight("codex", Some("pro_500"), Some("pro")), Some(5.0));
        assert_eq!(weight("codex", Some("go"), None), Some(0.08));
        assert_eq!(weight("codex", None, Some("pro")), None, "감지값 pro는 금액 단계를 모른다");
        assert_eq!(weight("codex", None, Some("Plus")), Some(0.2));
        assert_eq!(weight("codex", Some("business"), None), None);
        assert_eq!(weight("claude", None, None), None);
    }
}

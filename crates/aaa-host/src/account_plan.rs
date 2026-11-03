//! 요금제 정책 정본. 근거와 갱신 기준은 docs/account-plans.md에 둔다.
use aaa_proto::AccountPlanOption;

pub(crate) fn options() -> Vec<AccountPlanOption> {
    [
        ("claude", "free", "Free", Some(true)),
        ("claude", "pro", "Pro · $20/월", Some(true)),
        ("claude", "max", "Max · $100/월", Some(true)),
        ("claude", "ultra", "Ultra · $200/월 · 인증됨", Some(false)),
        ("claude", "ultra_pending", "Ultra · $200/월 · 임시 5시간 제한", Some(true)),
        ("claude", "team_standard", "Team Standard", Some(true)),
        ("claude", "team_premium", "Team Premium", Some(true)),
        ("claude", "enterprise", "Enterprise", None),
        ("codex", "free", "Free", None),
        ("codex", "go", "Go · $8/월", None),
        ("codex", "plus", "Plus · $20/월", Some(true)),
        ("codex", "pro_100", "Pro · $100/월", Some(false)),
        ("codex", "pro_200", "Pro · $200/월", Some(false)),
        ("codex", "pro_500", "Pro · $500/월", Some(false)),
        ("codex", "business", "Business", Some(true)),
        ("codex", "enterprise", "Enterprise", None),
        ("codex", "edu", "Edu", None),
    ]
    .into_iter()
    .map(|(harness, id, label, session_limit)| AccountPlanOption {
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
    let detected = detected.map(str::to_ascii_lowercase);
    let id = selected.or_else(|| match (harness, detected.as_deref()?) {
        ("codex", "pro") => Some("pro_200"), // 금액 표시는 기존 감지값 그대로 두며 창 정책만 공유한다.
        ("codex", "team") => Some("business"),
        ("claude", "ultra" | "max" | "max_20x" | "max20x") => None,
        (_, value) => Some(value),
    })?;
    options().into_iter().find(|p| p.harness == harness && p.id == id)
}

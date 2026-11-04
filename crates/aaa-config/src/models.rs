//! provider(harness)별 모델 켜기·끄기(t-1509, 참고 t3 code 설정 「Models」의 켜기 스위치·「Disable all」).
//!
//! - host.toml `[models.<harness>] disabled = [...]`에 끈 모델만 둔다. 목록에 없는 모델(새로 나온 모델 포함)은 켜진 것이다.
//! - 막는 곳은 aaa의 profile model 칸뿐이다(사용자 답 q-2643 2B): profile을 더하거나 model을 바꿀 때 끈 모델이면 거절한다.
//!   harness 안 `/model` 선택과 이미 그 모델을 쓰는 profile의 실행은 건드리지 않는다.
//! - 맞추기: Codex는 slug 그대로. Claude는 `[1m]` 같은 꼬리 `[…]`를 떼고 비교하며, 별칭(`opus` 등)을 끄면 그 계열 ID
//!   (`claude-opus-5-5`)도 막는다.

use toml_edit::{Array, DocumentMut, Item, Table};

use crate::HostConfig;

/// 모델을 켜고 끄는 harness
pub const HARNESSES: [&str; 2] = ["claude", "codex"];

/// Claude Code `/model` 기본 목록의 별칭과 보이는 이름. Codex처럼 머신에 목록 파일이 없어 host가 둔다
pub const CLAUDE_ALIASES: [(&str, &str); 4] =
    [("fable", "Fable"), ("opus", "Opus"), ("sonnet", "Sonnet"), ("haiku", "Haiku")];

/// `[models.<harness>]` 한 표
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProviderModels {
    /// 끈 모델 ID
    pub disabled: Vec<String>,
}

/// harness 이름이 모델 켜기·끄기를 받는가
pub fn validate_harness(harness: &str) -> Result<(), String> {
    if HARNESSES.contains(&harness) {
        Ok(())
    } else {
        Err(format!("모델 켜기·끄기는 claude·codex만 한다: {harness}"))
    }
}

/// `claude-opus-5-5[1m]` → `claude-opus-5-5`
fn bare(model: &str) -> &str {
    match model.find('[') {
        Some(i) if model.ends_with(']') => &model[..i],
        _ => model,
    }
}

/// Claude 모델 ID의 계열 별칭(`claude-opus-5-5` → `opus`). 별칭 자체면 그 별칭
pub fn claude_family(model: &str) -> Option<&'static str> {
    let m = bare(model);
    CLAUDE_ALIASES
        .iter()
        .map(|(a, _)| *a)
        .find(|a| m == *a || m.strip_prefix("claude-").is_some_and(|r| r.starts_with(&format!("{a}-"))))
}

/// `model`을 막는 끈 항목(없으면 `None`). 빈 model(harness 기본)은 막지 않는다
pub fn blocked<'a>(disabled: &'a [String], harness: &str, model: &str) -> Option<&'a str> {
    let model = model.trim();
    if model.is_empty() {
        return None;
    }
    disabled.iter().map(String::as_str).find(|d| match harness {
        "claude" => {
            bare(d) == bare(model) || (claude_family(d) == Some(bare(d)) && claude_family(model) == Some(bare(d)))
        }
        _ => *d == model,
    })
}

/// 설정의 `harness` 끈 목록
pub fn disabled_of<'a>(cfg: &'a HostConfig, harness: &str) -> &'a [String] {
    cfg.models.get(harness).map(|m| m.disabled.as_slice()).unwrap_or(&[])
}

/// host.toml 글의 `[models]`(형식이 틀리면 오류)
pub fn models_of(text: &str) -> Result<std::collections::BTreeMap<String, ProviderModels>, String> {
    if text.trim().is_empty() {
        return Ok(Default::default());
    }
    toml_edit::de::from_str::<HostConfig>(text).map(|c| c.models).map_err(|e| format!("host.toml을 읽지 못했다: {e}"))
}

/// profile이 `model`을 쓰려 할 때의 오류(끈 모델이면)
pub fn check_profile_model(
    models: &std::collections::BTreeMap<String, ProviderModels>,
    harness: &str,
    model: &str,
) -> Result<(), String> {
    let disabled = models.get(harness).map(|m| m.disabled.as_slice()).unwrap_or(&[]);
    match blocked(disabled, harness, model) {
        Some(d) => Err(format!(
            "{harness} 모델 `{model}`는 꺼져 있다(`{d}`): 설정 「모델」이나 `aaa model on {harness} {d}`로 켠 뒤 쓴다"
        )),
        None => Ok(()),
    }
}

/// host.toml 글에서 `harness`의 `ids`를 켜거나(`enabled`) 끈다. 나머지 글(주석·차례)은 그대로다.
/// 새 글과 실제로 바뀐 ID를 돌려준다. 빈 파일이면 `schema = 1`부터 쓴다
pub fn set_in(text: &str, harness: &str, ids: &[String], enabled: bool) -> Result<(String, Vec<String>), String> {
    validate_harness(harness)?;
    for id in ids {
        if !crate::profile_edit::is_model_name(id) {
            return Err(format!("`{id}`는 model 이름으로 보이지 않는다"));
        }
    }
    let base = if text.trim().is_empty() { format!("schema = {}\n", crate::SCHEMA) } else { text.to_string() };
    let mut doc = base.parse::<DocumentMut>().map_err(|e| format!("host.toml을 읽지 못했다: {e}"))?;
    let mut current = models_of(text)?.remove(harness).unwrap_or_default().disabled;
    let mut changed = Vec::new();
    for id in ids {
        let has = current.contains(id);
        if enabled && has {
            current.retain(|d| d != id);
            changed.push(id.clone());
        } else if !enabled && !has {
            current.push(id.clone());
            changed.push(id.clone());
        }
    }
    if changed.is_empty() {
        return Ok((text.to_string(), changed));
    }
    if !doc.contains_key("models") {
        let mut t = Table::new();
        t.set_implicit(true);
        doc["models"] = Item::Table(t);
    }
    let models = doc["models"].as_table_mut().ok_or("host.toml의 `models`가 표가 아니다")?;
    if current.is_empty() {
        models.remove(harness);
        if models.is_empty() {
            doc.remove("models");
        }
    } else {
        if !models.contains_key(harness) {
            models[harness] = Item::Table(Table::new());
        }
        let t = models[harness].as_table_mut().ok_or_else(|| format!("[models.{harness}]가 표가 아니다"))?;
        let mut a = Array::new();
        for d in &current {
            a.push(d.as_str());
        }
        t["disabled"] = toml_edit::value(a);
    }
    let out = doc.to_string();
    models_of(&out)?;
    Ok((out, changed))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn claude_alias_blocks_its_family_and_long_context_spelling() {
        let off = list(&["opus"]);
        assert_eq!(blocked(&off, "claude", "opus"), Some("opus"));
        assert_eq!(blocked(&off, "claude", "claude-opus-5-5"), Some("opus"));
        assert_eq!(blocked(&off, "claude", "claude-opus-5-5[1m]"), Some("opus"));
        assert_eq!(blocked(&off, "claude", "opus[1m]"), Some("opus"));
        assert_eq!(blocked(&off, "claude", "claude-sonnet-5-5"), None);
        assert_eq!(blocked(&off, "claude", ""), None);
        // ID 하나를 끄면 그 ID만 막는다(계열 전체가 아니다)
        let one = list(&["claude-opus-5-5"]);
        assert_eq!(blocked(&one, "claude", "claude-opus-5-5[1m]"), Some("claude-opus-5-5"));
        assert_eq!(blocked(&one, "claude", "opus"), None);
    }

    #[test]
    fn codex_matches_exact_slug_only() {
        let off = list(&["gpt-5.6-luna"]);
        assert_eq!(blocked(&off, "codex", "gpt-5.6-luna"), Some("gpt-5.6-luna"));
        assert_eq!(blocked(&off, "codex", "gpt-5.6-luna-mini"), None);
        assert_eq!(blocked(&off, "claude", "gpt-5.6-luna"), Some("gpt-5.6-luna"));
    }

    #[test]
    fn set_in_keeps_other_text_and_removes_empty_table() {
        let text = "schema = 1\n# 사람 주석\n[profiles.review]\nagent = \"codex\"\n";
        let (off, changed) = set_in(text, "codex", &list(&["gpt-6-luna", "gpt-5.6-luna"]), false).unwrap();
        assert_eq!(changed, list(&["gpt-6-luna", "gpt-5.6-luna"]));
        assert!(off.contains("# 사람 주석"));
        assert_eq!(models_of(&off).unwrap()["codex"].disabled, list(&["gpt-6-luna", "gpt-5.6-luna"]));
        // 이미 끈 것을 다시 끄면 그대로다
        let (same, none) = set_in(&off, "codex", &list(&["gpt-6-luna"]), false).unwrap();
        assert_eq!((same.as_str(), none.len()), (off.as_str(), 0));
        let (back, _) = set_in(&off, "codex", &list(&["gpt-6-luna", "gpt-5.6-luna"]), true).unwrap();
        assert!(!back.contains("models"), "{back}");
        assert!(back.contains("[profiles.review]"));
        assert!(set_in(text, "shell", &list(&["x"]), false).is_err());
        assert!(set_in(text, "codex", &list(&["--danger"]), false).is_err());
    }

    #[test]
    fn profile_cannot_take_disabled_model_but_keeps_other_edits() {
        use crate::Profile;
        use crate::profile_edit::{ProfileChange, apply_in};
        let text = "schema = 1\n[profiles.review]\nagent = \"codex\"\nmodel = \"gpt-5.6-luna\"\n\n[models.codex]\ndisabled = [\"gpt-5.6-luna\"]\n";
        let add = |model: &str| ProfileChange::Add {
            name: "fast".into(),
            profile: Profile { harness: "codex".into(), model: model.into(), ..Profile::default() },
        };
        assert!(apply_in(text, &add("gpt-5.6-luna")).unwrap_err().contains("꺼져 있다"));
        assert!(apply_in(text, &add("gpt-6-sol")).is_ok());
        assert!(apply_in(text, &add("")).is_ok());
        let edit = |model: Option<&str>| ProfileChange::Edit {
            name: "review".into(),
            harness: None,
            model: model.map(Into::into),
            effort: None,
            description: Some("검토".into()),
            args: None,
        };
        // 이미 끈 모델을 쓰는 profile도 model을 건드리지 않는 고치기는 된다
        assert!(apply_in(text, &edit(None)).is_ok());
        assert!(apply_in(text, &edit(Some("gpt-5.6-luna"))).is_err());
        assert!(apply_in(text, &edit(Some("gpt-6-sol"))).is_ok());
    }
}

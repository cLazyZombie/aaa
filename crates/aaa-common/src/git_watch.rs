//! Git 감시의 입력·출력 경계. 명령 실행은 repo 머신 agent만 한다.
use anyhow::{Result, bail};
use std::collections::BTreeMap;

pub const MAX_BRANCHES: usize = 32;
pub const MAX_COMMITS: usize = 100;
pub const QUERY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);
pub fn valid_sha(sha: &str) -> bool {
    matches!(sha.len(), 40 | 64) && sha.bytes().all(|b| b.is_ascii_hexdigit())
}
pub fn valid_branch(b: &str) -> bool {
    !b.is_empty()
        && b.len() <= 1024
        && !b.starts_with('-')
        && b != "@"
        && !b.ends_with('.')
        && !b.contains("..")
        && !b.contains("@{")
        && !b.chars().any(|c| c.is_control() || c.is_whitespace() || "~^:?*[\\".contains(c))
        && b.split('/').all(|part| !part.is_empty() && !part.starts_with('.') && !part.ends_with(".lock"))
}
pub fn validate_branches(branches: &[String]) -> Result<()> {
    if branches.is_empty() || branches.len() > MAX_BRANCHES || branches.iter().any(|b| !valid_branch(b)) {
        bail!("Git branches는 1~{MAX_BRANCHES}개의 유효한 branch 이름이어야 한다");
    }
    Ok(())
}
pub fn heads(text: &str, branches: &[String]) -> Result<BTreeMap<String, Option<String>>> {
    let mut heads: BTreeMap<_, _> = branches.iter().map(|b| (b.clone(), None)).collect();
    for line in text.lines() {
        let (sha, name) = line.split_once('\t').ok_or_else(|| anyhow::anyhow!("ls-remote 출력 형식이 아니다"))?;
        if !valid_sha(sha) {
            bail!("ls-remote SHA 형식이 아니다");
        }
        if let Some(value) = name.strip_prefix("refs/heads/").and_then(|b| heads.get_mut(b)) {
            if value.is_some() {
                bail!("ls-remote branch가 중복됐다");
            }
            *value = Some(sha.to_ascii_lowercase());
        }
    }
    Ok(heads)
}
pub fn kind(before: Option<&str>, after: Option<&str>, fast_forward: bool) -> Option<&'static str> {
    if before == after {
        None
    } else if after.is_none() {
        Some("deleted")
    } else if before.is_none() || fast_forward {
        Some("pushed")
    } else {
        Some("forced")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_heads_and_deleted_branches_do_not_accept_option_or_pattern_input() {
        for b in ["main", "release/one", "한글"] {
            assert!(valid_branch(b));
        }
        for b in ["-x", "main*", "a..b", "a@{1}", "a.lock", "a//b", "a b", "a\nb", "a\\b", "/main"] {
            assert!(!valid_branch(b), "{b}");
        }
        let branches = vec!["main".into(), "gone".into()];
        let got = heads(&format!("{}\trefs/heads/main\n", "a".repeat(40)), &branches).unwrap();
        assert_eq!(got["main"].as_deref(), Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"));
        assert_eq!(got["gone"], None);
        assert!(heads("oops\trefs/heads/main", &branches).is_err());
    }
    #[test]
    fn head_changes_distinguish_unchanged_creation_force_and_deletion() {
        assert_eq!(kind(Some("a"), Some("a"), true), None);
        assert_eq!(kind(None, None, true), None);
        assert_eq!(kind(None, Some("a"), true), Some("pushed"));
        assert_eq!(kind(Some("a"), Some("b"), true), Some("pushed"));
        assert_eq!(kind(Some("a"), Some("b"), false), Some("forced"));
        assert_eq!(kind(Some("a"), None, false), Some("deleted"));
    }
}

//! PR 추적에서 agent와 host가 함께 쓰는 순수 함수(plan §8.80). agent는 `gh` 인자를 만들고, host는 origin을 맞춰 본다.
//! GitHub에서 읽은 것은 모두 공격자가 고른 글이다: 정해진 꼴만 받고 이름은 거른다.
//!
//! herdr-projects 0.2.35 `src/pr.rs`(`valid_pr_url`·`parse_remote`·`normalize_origin`·`host_of`·`gh`·`sanitize`·
//! `view`·`find_by_branch`)에서 옮겼다. 그 코드의 고지:
//!
//! > MIT License
//! >
//! > Copyright (c) 2026 Elias Stravik
//! >
//! > Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated
//! > documentation files (the "Software"), to deal in the Software without restriction, including without limitation the
//! > rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to
//! > permit persons to whom the Software is furnished to do so, subject to the following conditions:
//! >
//! > The above copyright notice and this permission notice shall be included in all copies or substantial portions of
//! > the Software.
//! >
//! > THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE
//! > WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR
//! > COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR
//! > OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

use std::time::Duration;

use serde::Deserialize;

/// `gh` 한 번의 시간 한도(herdr-projects `GH_TIMEOUT`)
pub const GH_TIMEOUT: Duration = Duration::from_secs(10);
const NAME_LIMIT: usize = 80;

/// `gh pr view --json`으로 받는 필드. 본문·제목은 받지 않는다.
pub const VIEW_FIELDS: &str =
    "state,reviewDecision,statusCheckRollup,comments,reviews,headRefName,headRefOid,headRepository,headRepositoryOwner";

/// 정확히 `https://github.com/<owner>/<repo>/pull/<number>`인가
pub fn valid_pr_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://github.com/") else {
        return false;
    };
    let parts: Vec<&str> = rest.split('/').collect();
    let name_ok = |s: &str| {
        !s.is_empty()
            && s != "."
            && s != ".."
            && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    parts.len() == 4
        && name_ok(parts[0])
        && name_ok(parts[1])
        && parts[2] == "pull"
        && !parts[3].is_empty()
        && parts[3].len() <= 9
        && parts[3].chars().all(|c| c.is_ascii_digit())
}

/// git remote URL이 가리키는 GitHub 저장소: `gh`가 말할 host와 `owner/repo`(모두 소문자)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remote {
    pub host: String,
    pub repo: String,
}

/// git이 쓰는 꼴(`https://`·`http://`·`ssh://`·`git@host:`). host 이름에 `github` 마디가 있어야 한다
/// (`github.com`·`github.localhost`·`github.example.com`). 그래서 GitLab origin이면 `gh`를 부르지 않는다.
pub fn parse_remote(origin: &str) -> Option<Remote> {
    let origin = origin.trim();
    let (host, rest) = if let Some(rest) = origin.strip_prefix("https://").or_else(|| origin.strip_prefix("http://")) {
        let (authority, path) = rest.split_once('/')?;
        (authority.rsplit('@').next()?, path)
    } else if let Some(rest) = origin.strip_prefix("ssh://") {
        let (authority, path) = rest.split_once('/')?;
        (authority.rsplit('@').next()?.split(':').next()?, path)
    } else {
        let (authority, path) = origin.split_once(':')?;
        (authority.split_once('@')?.1, path)
    };
    let host = host.to_lowercase();
    if !host.split(['.', ':']).any(|label| label == "github") {
        return None;
    }
    let rest = rest.trim_end_matches('/');
    let rest = rest.strip_suffix(".git").unwrap_or(rest).trim_end_matches('/');
    let mut parts = rest.split('/');
    let (owner, repo) = (parts.next()?, parts.next()?);
    if owner.is_empty() || repo.is_empty() || parts.next().is_some() {
        return None;
    }
    Some(Remote { host, repo: format!("{owner}/{repo}").to_lowercase() })
}

/// GitHub remote URL의 `owner/repo`(소문자)
pub fn normalize_origin(origin: &str) -> Option<String> {
    parse_remote(origin).map(|r| r.repo)
}

/// remote URL의 GitHub host. 없으면 `github.com`
pub fn host_of(origin: &str) -> String {
    parse_remote(origin).map(|r| r.host).unwrap_or_else(|| "github.com".into())
}

/// `github.com`이 아니면 이 호출에만 `GH_HOST`를 준다(herdr-projects `gh`)
pub fn gh_host_env(host: &str) -> Option<String> {
    (!host.is_empty() && host != "github.com").then(|| host.to_string())
}

/// check 이름과 login은 공격자가 고른다: 제어 문자·줄바꿈을 빼고 80자로 자른다
pub fn sanitize(name: &str) -> String {
    name.chars().filter(|c| !c.is_control()).take(NAME_LIMIT).collect::<String>().trim().to_string()
}

/// `gh` 한 번: 이 호출에만 줄 `GH_HOST`와 인자
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GhCall {
    pub gh_host: Option<String>,
    pub args: Vec<String>,
}

fn call(host: &str, args: &[&str]) -> GhCall {
    GhCall { gh_host: gh_host_env(host), args: args.iter().map(|a| a.to_string()).collect() }
}

/// PR 주소의 `gh pr view`. `gh`는 주소를 그 주소의 host로 보내므로, `github.com` 밖에서는 번호와 `--repo`로 묻는다.
/// 주소는 늘 `--` 뒤에 둔다.
pub fn view_call(url: &str, origin: &str) -> Option<GhCall> {
    if !valid_pr_url(url) {
        return None;
    }
    let host = host_of(origin);
    Some(if host == "github.com" {
        call(&host, &["pr", "view", "--json", VIEW_FIELDS, "--", url])
    } else {
        let parts: Vec<&str> = url.trim_start_matches("https://github.com/").split('/').collect();
        let repo = format!("{}/{}", parts[0], parts[1]);
        call(&host, &["pr", "view", "--json", VIEW_FIELDS, "--repo", &repo, "--", parts[3]])
    })
}

/// `origin` 저장소에서 head가 `branch`인 PR 찾기(`gh pr list` 한 번). origin이 GitHub가 아니거나 branch가 없으면 묻지 않는다.
pub fn find_call(origin: &str, branch: &str) -> Option<GhCall> {
    let Remote { host, repo } = parse_remote(origin)?;
    if branch.is_empty() {
        return None;
    }
    let head = format!("--head={branch}");
    Some(call(
        &host,
        &["pr", "list", "--repo", &repo, &head, "--state", "all", "--limit", "20", "--json", "url,state,createdAt"],
    ))
}

/// `gh`가 쓰는 계정: 그 계정의 댓글은 thread 자기 답이다
pub fn login_call(origin: &str) -> GhCall {
    call(&host_of(origin), &["api", "user", "--jq", ".login"])
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct Listed {
    url: String,
    state: String,
    created_at: String,
}

/// `find_call` 출력에서 고른다: 열린 것, 병합된 것, 닫힌 것 차례로, 같으면 새것. 두 tick 사이에 열고 병합할 수 있어서
/// 병합·닫힌 것도 센다(herdr-projects `find_by_branch`).
pub fn pick_listed(json: &str) -> Result<Option<String>, serde_json::Error> {
    let mut listed: Vec<Listed> = serde_json::from_str(json)?;
    listed.retain(|p| valid_pr_url(&p.url));
    let rank = |state: &str| match state.to_ascii_uppercase().as_str() {
        "OPEN" => 0,
        "MERGED" => 1,
        _ => 2,
    };
    listed.sort_by(|a, b| rank(&a.state).cmp(&rank(&b.state)).then_with(|| b.created_at.cmp(&a.created_at)));
    Ok(listed.into_iter().next().map(|p| p.url))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pr_urls() {
        assert!(valid_pr_url("https://github.com/o/r/pull/12"));
        for bad in [
            "http://github.com/o/r/pull/1",
            "https://github.com/o/r/pull/1/files",
            "https://github.com/o/r/pull/abc",
            "https://github.com/o/r/issues/1",
            "https://evil.example/o/r/pull/1",
            "https://github.com/o/r/pull/1 --repo x",
            "--web",
            "https://github.com/../r/pull/1",
            "https://github.com/o/r/pull/",
            "https://github.com/o/r/pull/1234567890",
        ] {
            assert!(!valid_pr_url(bad), "{bad}");
        }
    }

    #[test]
    fn origin_forms() {
        for origin in [
            "https://github.com/Owner/Repo",
            "https://github.com/Owner/Repo.git",
            "https://github.com/Owner/Repo/",
            "git@github.com:Owner/Repo.git",
            "ssh://git@github.com/Owner/Repo.git",
        ] {
            assert_eq!(normalize_origin(origin).as_deref(), Some("owner/repo"), "{origin}");
        }
        for bad in
            ["", "https://gitlab.com/o/r", "git@github.com:o", "https://github.com/o/r/extra", "/srv/git/r.git", "../r"]
        {
            assert_eq!(normalize_origin(bad), None, "{bad}");
        }
        let remote = |host: &str| Some(Remote { host: host.into(), repo: "owner/repo".into() });
        assert_eq!(parse_remote("http://github.localhost/Owner/Repo.git"), remote("github.localhost"));
        assert_eq!(parse_remote("https://token@GitHub.example.com/Owner/Repo"), remote("github.example.com"));
        assert_eq!(parse_remote("ssh://git@github.example.com:2222/Owner/Repo.git"), remote("github.example.com"));
        assert_eq!(host_of(""), "github.com");
        assert_eq!(gh_host_env("github.com"), None);
        assert_eq!(gh_host_env("github.localhost").as_deref(), Some("github.localhost"));
    }

    #[test]
    fn gh_calls_put_the_url_after_a_double_dash_and_follow_the_origin_host() {
        let v = view_call("https://github.com/o/r/pull/7", "git@github.com:o/r.git").unwrap();
        assert_eq!(
            (v.gh_host, &v.args[v.args.len() - 2..]),
            (None, &["--".to_string(), "https://github.com/o/r/pull/7".into()][..])
        );
        assert!(view_call("--web", "").is_none());
        // github.com 밖: 주소를 주면 gh가 github.com으로 간다. 번호와 --repo로
        let off = view_call("https://github.com/up/r/pull/7", "http://github.localhost/o/r.git").unwrap();
        assert_eq!(off.gh_host.as_deref(), Some("github.localhost"));
        assert!(off.args.join(" ").ends_with("--repo up/r -- 7"), "{:?}", off.args);
        let f = find_call("git@github.com:O/R.git", "aaa/demo/t-0001-x").unwrap();
        assert!(f.args.join(" ").contains("--repo o/r --head=aaa/demo/t-0001-x --state all"), "{:?}", f.args);
        assert!(
            find_call("https://gitlab.com/o/r", "b").is_none() && find_call("git@github.com:o/r.git", "").is_none()
        );
        assert_eq!(login_call("http://github.localhost/o/r.git").gh_host.as_deref(), Some("github.localhost"));
    }

    #[test]
    fn branch_lookup_prefers_open_then_merged_then_closed_and_the_newest() {
        let found = pick_listed(
            r#"[
            {"url":"https://github.com/o/r/pull/1","state":"CLOSED","createdAt":"2026-01-03T00:00:00Z"},
            {"url":"https://github.com/o/r/pull/2","state":"MERGED","createdAt":"2026-01-01T00:00:00Z"},
            {"url":"https://github.com/o/r/pull/3","state":"MERGED","createdAt":"2026-01-02T00:00:00Z"},
            {"url":"--web","state":"OPEN","createdAt":"2026-01-04T00:00:00Z"}]"#,
        )
        .unwrap();
        assert_eq!(found.as_deref(), Some("https://github.com/o/r/pull/3"));
        assert_eq!(pick_listed("[]").unwrap(), None);
    }

    #[test]
    fn names_are_cut_and_cleaned() {
        assert_eq!(sanitize(&"x".repeat(200)).len(), 80);
        assert_eq!(sanitize("a\r\nb\tc"), "abc");
    }
}

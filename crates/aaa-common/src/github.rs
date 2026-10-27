//! CI의 GitHub remote 해석과 외부 이름 정리.
//! herdr-projects 0.2.35 `src/pr.rs`의 remote 해석·이름 정리를 옮겼다. 그 코드의 고지:
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

pub const GH_TIMEOUT: Duration = Duration::from_secs(10);
const NAME_LIMIT: usize = 80;

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

/// check 이름과 login은 공격자가 고른다: 제어 문자·줄바꿈을 빼고 80자로 자른다
pub fn sanitize(name: &str) -> String {
    name.chars().filter(|c| !c.is_control()).take(NAME_LIMIT).collect::<String>().trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
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
        // 사용자 정보·port는 떼고 host는 소문자로 본다
        assert_eq!(parse_remote("https://GitHub.example.com/Owner/Repo"), remote("github.example.com"));
        assert_eq!(parse_remote("https://git@github.com/Owner/Repo"), remote("github.com"));
        assert_eq!(parse_remote("ssh://git@github.com:2222/Owner/Repo.git"), remote("github.com"));
        assert_eq!(host_of(""), "github.com");
    }

    #[test]
    fn names_are_cut_and_cleaned() {
        assert_eq!(sanitize(&"x".repeat(200)).len(), 80);
        assert_eq!(sanitize("a\r\nb\tc"), "abc");
    }
}

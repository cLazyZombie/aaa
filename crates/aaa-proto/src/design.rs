//! 기획(t-1894, FRONT_API 1.170): 저장소 안 기획 폴더(`aaa.toml [[workspaces]]`의 `design`)의 Markdown 문서 트리.
//! 정본은 저장소 origin 기본 branch(`origin/main` 등)다. 1단계는 읽기만 한다: repo 머신 agent(`design.v1`)가 작업 파일·index를
//! 바꾸지 않고 git 객체에서 읽는다(fetch만 한다).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 트리에 싣는 문서 수 상한(넘으면 `truncated`)
pub const DESIGN_MAX_DOCS: usize = 2000;
/// 문서 한 개 글 상한(바이트, 넘으면 읽지 않는다). CLI front socket의 한 frame(1MiB)에 JSON 이스케이프까지 들어가게 둔다
pub const DESIGN_MAX_TEXT: u64 = 512 * 1024;
/// 그림·영상 한 개 상한(바이트, 아이디어 그림과 같다). 조각마다 blob을 처음부터 풀므로 크게 두지 않는다
pub const DESIGN_MAX_FILE: u64 = 16 * 1024 * 1024;

/// 기획 폴더 안 Markdown 문서 한 개
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct DesignDocEntry {
    /// 저장소 기준 경로(`docs/game-loop/README.md`)
    pub path: String,
    /// 첫 `# ` 제목(없으면 빈 글)
    pub title: String,
    #[ts(type = "number")]
    pub bytes: u64,
}

/// agent가 읽은 기획 폴더(한 commit 기준)
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct DesignTree {
    /// 읽은 commit(full SHA)
    pub commit: String,
    /// commit 시각(ms)
    #[ts(type = "number")]
    pub committed_ms: u64,
    /// 읽은 ref(`origin/main`, origin이 없으면 `HEAD`)
    pub base: String,
    /// 경로 차례
    pub docs: Vec<DesignDocEntry>,
    /// 문서가 상한보다 많아 뒤를 뺐다
    pub truncated: bool,
    /// origin을 받지 못해 가지고 있던 ref로 읽었다(까닭)
    pub fetch_error: Option<String>,
}

/// 프로젝트 저장소 하나의 기획 폴더
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct DesignRepoView {
    /// 저장소 이름(경로 끝 폴더 이름)
    pub repo: String,
    /// 저장소가 있는 머신 이름
    pub machine: String,
    /// 기획 폴더(저장소 기준, `docs`). 없으면 이 저장소는 기획 폴더를 정하지 않았다
    pub folder: Option<String>,
    /// 읽은 트리. 못 읽었으면 없고 `error`가 까닭이다
    pub tree: Option<DesignTree>,
    /// 못 읽은 까닭, 또는 마지막으로 읽은 트리를 보이는 까닭(머신 연결 끊김 등)
    pub error: Option<String>,
}

/// 문서 하나(읽기 전용)
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct DesignDoc {
    /// 프로젝트(그림 주소 `/api/v1/projects/<p>/design/<저장소>/<경로>`)
    pub project: String,
    pub repo: String,
    /// 저장소 기준 경로
    pub path: String,
    pub title: String,
    pub text: String,
    /// 파일 blob SHA(다음 단계 저장의 기준 판)
    pub blob: String,
    /// 읽은 commit(그림 주소에 싣는다)
    pub commit: String,
    #[ts(type = "number")]
    pub committed_ms: u64,
}

/// 첫 `# ` 제목. 앞 YAML front matter(`---`)는 건너뛴다
pub fn design_title(text: &str) -> String {
    let mut lines = text.lines();
    let mut first = true;
    let mut front = false;
    for line in lines.by_ref() {
        let line = line.trim_end();
        if first {
            first = false;
            if line == "---" {
                front = true;
                continue;
            }
        }
        if front {
            if line == "---" || line == "..." {
                front = false;
            }
            continue;
        }
        if let Some(t) = line.strip_prefix("# ") {
            return t.trim().trim_end_matches('#').trim().to_string();
        }
    }
    String::new()
}

/// 저장소 기준 상대 경로를 정리한다: `\`·빈 마디·`.`·`..`·절대 경로·NUL을 받지 않는다. 앞 `./`와 끝 `/`는 뗀다
pub fn design_clean_path(path: &str) -> Option<String> {
    let p = path.trim().trim_start_matches("./").trim_end_matches('/');
    if p.is_empty() || p.starts_with('/') || p.contains('\\') || p.contains('\0') {
        return None;
    }
    if p.split('/').any(|s| s.is_empty() || s == "." || s == "..") {
        return None;
    }
    Some(p.to_string())
}

/// `path`가 기획 폴더 `folder` 안인가(둘 다 `design_clean_path`를 거친 값)
pub fn design_inside(folder: &str, path: &str) -> bool {
    path.strip_prefix(folder).is_some_and(|rest| rest.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_is_first_h1_after_front_matter() {
        assert_eq!(design_title("# 게임 루프\n\n본문"), "게임 루프");
        assert_eq!(design_title("---\ntitle: x\n# 아님\n---\n\n## 둘\n# 하나 #\n"), "하나");
        assert_eq!(design_title("본문만\n## 절"), "");
        assert_eq!(design_title("#붙음\n# 띄움"), "띄움");
    }

    #[test]
    fn clean_path_rejects_escape() {
        assert_eq!(design_clean_path("./docs/"), Some("docs".into()));
        assert_eq!(design_clean_path("docs/game-loop/README.md"), Some("docs/game-loop/README.md".into()));
        for bad in ["", "/etc", "docs/../x", "docs//x", "docs/./x", "a\\b", ".."] {
            assert_eq!(design_clean_path(bad), None, "{bad}");
        }
        assert!(design_inside("docs", "docs/a.md"));
        assert!(!design_inside("docs", "docs2/a.md"));
        assert!(!design_inside("docs", "docs"));
    }
}

//! herdr-projects 0.2.x 프로젝트 폴더를 읽는다(읽기만 한다).
//!
//! 형식 근거(herdr-projects 1f05c00): thread 기록 `src/thread.rs` `Thread`(`threads/<id>.toml`), 보고 사본
//! `home_report_path`(`threads/<id>.md`), library `library/<id>/`, inbox `src/inbox.rs`(`inbox/<id>.md` +++ TOML,
//! 끝낸 것은 `inbox/done/`), 설정 `PROJECT.md` +++ TOML front matter, routine `routines/<name>.md`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// `threads/<id>.toml`(0.2.35 `Thread`). 모르는 필드는 무시하고, 없는 필드는 기본값이다.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HpThread {
    pub id: String,
    pub title: String,
    /// starting·open·failed·resolved
    pub status: String,
    pub error: String,
    pub prompt_pending: bool,
    pub launch_attempts: u32,
    pub brief_stuck: bool,
    /// worktree·tab·checkout·adopted
    pub kind: String,
    pub repo: String,
    pub branch: String,
    pub base: String,
    /// 비면 projects root가 있는 머신(로컬)
    pub machine: String,
    pub worktree_path: String,
    pub thread_dir: String,
    pub workspace_id: String,
    pub tab_id: String,
    pub pane_id: String,
    pub agent: String,
    pub profile: String,
    pub agent_args: Vec<String>,
    pub profile_args: Vec<String>,
    pub agent_name: String,
    pub cwd: String,
    pub created: String,
    pub updated: String,
    pub last_state: String,
    pub last_state_change: String,
    pub last_group: String,
    pub report_hash: String,
    pub acked_report_hash: String,
    pub pr: String,
    pub pr_state: String,
    pub pr_review: String,
    pub resolved_reason: String,
}

impl HpThread {
    pub fn is_remote(&self) -> bool {
        !self.machine.is_empty()
    }
}

/// `+++` TOML front matter와 본문으로 나눈다. front matter가 없으면 (None, 전체).
pub fn front_matter(text: &str) -> (Option<&str>, &str) {
    let Some(rest) = text.strip_prefix("+++\n").or_else(|| text.strip_prefix("+++\r\n")) else {
        return (None, text);
    };
    match rest.find("\n+++") {
        Some(end) => {
            let body = &rest[end + 4..];
            let body = body.strip_prefix('\n').or_else(|| body.strip_prefix("\r\n")).unwrap_or(body);
            (Some(&rest[..end + 1]), body)
        }
        None => (None, text),
    }
}

/// inbox 항목(`inbox/<id>.md`)
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HpInbox {
    pub id: String,
    pub kind: String,
    pub subject: String,
    pub created: String,
    pub summary: String,
    pub event: String,
    /// front matter 아래 본문
    #[serde(skip)]
    pub body: String,
    /// `inbox/done/`에 있다
    #[serde(skip)]
    pub done: bool,
}

/// 읽은 프로젝트
#[derive(Debug, Clone)]
pub struct HpProject {
    pub dir: PathBuf,
    pub slug: String,
    /// PROJECT.md front matter(설정)
    pub settings: toml::Table,
    /// PROJECT.md 본문(프로젝트 지침)
    pub instructions: String,
    pub threads: Vec<HpThread>,
    pub inbox: Vec<HpInbox>,
    /// 읽지 못한 기록(파일, 까닭)
    pub unreadable: Vec<(String, String)>,
}

impl HpProject {
    pub fn report_path(&self, id: &str) -> PathBuf {
        self.dir.join("threads").join(format!("{id}.md"))
    }

    pub fn library_dir(&self, id: &str) -> PathBuf {
        self.dir.join("library").join(id)
    }
}

fn sorted_md(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "md") && p.is_file())
        .collect();
    v.sort();
    v
}

/// `<root>/<slug>`를 읽는다
pub fn read(root: &Path, slug: &str) -> Result<HpProject> {
    let dir = root.join(slug);
    let project_md =
        std::fs::read_to_string(dir.join("PROJECT.md")).with_context(|| format!("{}/PROJECT.md", dir.display()))?;
    let (fm, body) = front_matter(&project_md);
    let settings: toml::Table = match fm {
        Some(t) => toml::from_str(t).context("PROJECT.md front matter")?,
        None => toml::Table::new(),
    };
    let mut unreadable = Vec::new();
    let mut threads = Vec::new();
    let mut tomls: Vec<PathBuf> = std::fs::read_dir(dir.join("threads"))
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "toml") && p.is_file())
        .collect();
    tomls.sort();
    for p in tomls {
        let name = format!("threads/{}", p.file_name().unwrap().to_string_lossy());
        match std::fs::read_to_string(&p)
            .map_err(|e| e.to_string())
            .and_then(|t| toml::from_str::<HpThread>(&t).map_err(|e| e.message().to_string()))
        {
            Ok(t) if !t.id.is_empty() => threads.push(t),
            Ok(_) => unreadable.push((name, "id가 없다".into())),
            Err(e) => unreadable.push((name, e)),
        }
    }
    let mut inbox = Vec::new();
    for (sub, done) in [("inbox", false), ("inbox/done", true)] {
        for p in sorted_md(&dir.join(sub)) {
            let name = format!("{sub}/{}", p.file_name().unwrap().to_string_lossy());
            let text = match std::fs::read_to_string(&p) {
                Ok(t) => t,
                Err(e) => {
                    unreadable.push((name, e.to_string()));
                    continue;
                }
            };
            let (fm, body) = front_matter(&text);
            match fm.map(toml::from_str::<HpInbox>) {
                Some(Ok(mut i)) => {
                    i.body = body.trim().to_string();
                    i.done = done;
                    inbox.push(i);
                }
                Some(Err(e)) => unreadable.push((name, e.message().to_string())),
                None => unreadable.push((name, "front matter가 없다".into())),
            }
        }
    }
    Ok(HpProject { dir, slug: slug.to_string(), settings, instructions: body.to_string(), threads, inbox, unreadable })
}

// ───────────────────────── herdr 세션 스냅샷 ─────────────────────────

/// `herdr api snapshot`의 agent 하나(herdr 0.9.5 `src/api/schema`). 쓰는 필드만 읽는다.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SnapAgent {
    pub agent: String,
    pub agent_session: Option<SnapSession>,
    /// idle·working·blocked·done·unknown
    pub agent_status: String,
    pub cwd: String,
    pub pane_id: String,
    pub terminal_id: String,
    pub name: String,
    pub workspace_id: String,
    pub tokens: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SnapSession {
    pub value: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SnapPane {
    pub pane_id: String,
    pub cwd: String,
    pub agent_status: String,
    pub terminal_id: String,
    pub workspace_id: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Snapshot {
    pub version: String,
    pub agents: Vec<SnapAgent>,
    pub panes: Vec<SnapPane>,
    pub workspaces: Vec<serde_json::Value>,
}

/// `herdr api snapshot` 출력(`{"result":{"snapshot":…}}`)이나 snapshot 객체 자체
pub fn parse_snapshot(text: &str) -> Result<Snapshot> {
    let v: serde_json::Value = serde_json::from_str(text).context("herdr snapshot JSON")?;
    let snap = v.pointer("/result/snapshot").cloned().unwrap_or(v);
    serde_json::from_value(snap).context("herdr snapshot 형식")
}

/// thread의 agent인가(herdr-projects `thread::agent_matches`를 옮겼다): pane ID와 작업 폴더가 같고, binary가 띄운
/// thread면 agent 종류가 같거나 비었고 이름이 우리 이름이거나 비었다(herdr 재시작 복원은 이름 없이 띄운다).
pub fn agent_matches(t: &HpThread, a: &SnapAgent) -> bool {
    let ids = a.pane_id == t.pane_id && a.cwd == t.cwd;
    if t.kind == "adopted" {
        return ids;
    }
    ids && (t.agent.is_empty() || a.agent.is_empty() || a.agent == t.agent)
        && (a.name.is_empty() || a.name == t.agent_name)
}

/// pane이 thread의 pane인가(`thread::pane_matches`)
pub fn pane_matches(t: &HpThread, p: &SnapPane) -> bool {
    p.pane_id == t.pane_id && p.cwd == t.cwd
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_wrapper_and_agent_match() {
        let s = parse_snapshot(
            r#"{"id":"x","result":{"snapshot":{"version":"0.9.5","agents":[{"agent":"claude","agent_status":"idle",
            "cwd":"/w","pane_id":"w1:p1","name":"hp-p-t-0001","agent_session":{"value":"s-1"}}],"panes":[]}}}"#,
        )
        .unwrap();
        assert_eq!(s.agents[0].agent_session.as_ref().unwrap().value, "s-1");
        let mut t = HpThread {
            pane_id: "w1:p1".into(),
            cwd: "/w".into(),
            agent: "claude".into(),
            agent_name: "hp-p-t-0001".into(),
            kind: "worktree".into(),
            ..HpThread::default()
        };
        assert!(agent_matches(&t, &s.agents[0]));
        // 복원된 agent는 이름이 없다: 여전히 우리 것
        let mut unnamed = s.agents[0].clone();
        unnamed.name.clear();
        assert!(agent_matches(&t, &unnamed));
        // 다른 이름이면 남의 agent다
        t.agent_name = "other".into();
        assert!(!agent_matches(&t, &s.agents[0]));
        t.kind = "adopted".into();
        assert!(agent_matches(&t, &s.agents[0]));
    }

    #[test]
    fn front_matter_splits_toml_and_body() {
        let (fm, body) = front_matter("+++\nname = \"x\"\n+++\n\n# 지침\n");
        assert_eq!(fm, Some("name = \"x\"\n"));
        assert_eq!(body, "\n# 지침\n");
        assert_eq!(front_matter("그냥 글"), (None, "그냥 글"));
        assert_eq!(front_matter("+++\n끝 없음"), (None, "+++\n끝 없음"));
    }
}

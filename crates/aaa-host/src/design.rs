//! 기획(t-1894, FRONT_API 1.170): 프로젝트 저장소마다 기획 폴더(`aaa.toml [[workspaces]]`의 `design`)의 Markdown 문서 트리와
//! 문서·그림 읽기. 정본은 저장소 origin 기본 branch이고 repo 머신 agent(`design.v1`)가 git 객체에서 읽는다. host는 저장소마다
//! 마지막으로 읽은 트리를 기억해 두었다가 머신 연결이 끊기면 그것을 보인다(읽기 전용).
//!
//! - 같은 저장소(경로 끝 이름)가 여러 머신에 있으면 `design`을 단 항목의 머신에서 읽는다.
//! - 문서·그림 경로는 저장소 기준이고 기획 폴더 안이어야 한다(agent도 다시 본다).

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use aaa_proto::{
    AgentRequest, AgentResponse, DESIGN_MAX_FILE, DesignDoc, DesignRepoView, DesignTree, ErrorBody, ErrorCode,
    FrontResponse, design_clean_path, design_inside,
};

use crate::{Host, err};

/// 트리 읽기(origin 받기 30초 + 목록)
const TREE_WAIT: Duration = Duration::from_secs(60);
const READ_WAIT: Duration = Duration::from_secs(30);
/// 그림 한 번에 받는 조각
const CHUNK: u32 = 512 * 1024;
pub(crate) const FEATURE: &str = "design.v1";

/// 저장소 하나의 읽을 자리
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Target {
    /// 경로 끝 이름
    pub(crate) name: String,
    pub(crate) path: String,
    pub(crate) machine: Option<String>,
    pub(crate) folder: Option<String>,
    /// 기획 branch를 올리기 전 검사(`design_check`)
    pub(crate) check: Vec<String>,
}

/// 프로젝트 저장소 목록에서 이름마다 한 자리를 고른다: `design`을 단 항목이 있으면 그것, 없으면 처음 항목. 설정 차례를 지킨다
fn targets(settings: &crate::project::ProjectSettings) -> Vec<Target> {
    let mut order: Vec<String> = Vec::new();
    let mut by_name: BTreeMap<String, Target> = BTreeMap::new();
    for repo in &settings.repos {
        let name = repo_name(&repo.path);
        if name.is_empty() {
            continue;
        }
        let design = settings.designs.iter().find(|d| d.repo.path == repo.path && d.repo.machine == repo.machine);
        let candidate = Target {
            name: name.clone(),
            path: repo.path.clone(),
            machine: repo.machine.clone(),
            folder: design.map(|d| d.folder.clone()),
            check: Vec::new(),
        };
        match by_name.get(&name) {
            None => {
                order.push(name.clone());
                by_name.insert(name, candidate);
            }
            Some(old) if old.folder.is_none() && candidate.folder.is_some() => {
                by_name.insert(name, candidate);
            }
            Some(_) => {}
        }
    }
    order.into_iter().filter_map(|n| by_name.remove(&n)).collect()
}

/// 경로 끝 폴더 이름(`/` 또는 `\`로 나눈다: Windows 머신 경로도 받는다)
fn repo_name(path: &str) -> String {
    path.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().unwrap_or_default().to_string()
}

impl Host {
    pub(crate) fn design_targets(&self, project: &str) -> Result<Vec<Target>, ErrorBody> {
        let dir = self.project_dir(project)?;
        let mut list = targets(&crate::project::ProjectSettings::load(&dir));
        // 올리기 전 검사는 host.toml에서만 읽는다(프로젝트 폴더는 agent가 고칠 수 있다)
        for t in &mut list {
            t.check = self.cfg.projects.design_check.get(&t.name).cloned().unwrap_or_default();
        }
        Ok(list)
    }

    /// 그 저장소의 기획 폴더 자리와 연결(기획 폴더가 없거나 머신이 꺼져 있으면 오류)
    pub(crate) fn design_target(&self, project: &str, repo: &str) -> Result<(Target, String), ErrorBody> {
        let target = self
            .design_targets(project)?
            .into_iter()
            .find(|t| t.name == repo)
            .ok_or_else(|| err(ErrorCode::NotFound, format!("프로젝트 저장소가 아니다: {repo}")))?;
        if target.folder.is_none() {
            return Err(err(ErrorCode::NotFound, format!("{repo}: 기획 폴더를 정하지 않았다(aaa.toml의 design)")));
        }
        let mid = self.resolve_machine(target.machine.as_deref())?;
        Ok((target, mid))
    }

    fn design_conn(&self, mid: &str) -> Result<crate::AgentConn, ErrorBody> {
        let conn = self.live_conn(mid)?;
        if !self.machine_feature(mid, FEATURE) {
            return Err(err(
                ErrorCode::Unavailable,
                "그 머신 agent가 기획 읽기를 아직 모른다(agent binary를 바꿔야 한다)",
            ));
        }
        Ok(conn)
    }

    /// 기획 요청(FRONT_API 1.170·1.171). 공용 dispatcher의 poll 스택이 함께 커지지 않게 따로 둔다(부르는 쪽이 heap에 둔다)
    pub(crate) async fn call_design(
        self: &Arc<Self>,
        role: aaa_proto::DeviceRole,
        actor: &str,
        req: aaa_proto::FrontRequest,
    ) -> Result<FrontResponse, ErrorBody> {
        use aaa_proto::FrontRequest as R;
        match req {
            R::DesignTree { project } => self.design_tree(&project).await,
            R::IdeaLinks { project } => Ok(FrontResponse::IdeaLinks { links: self.idea_links(&project)? }),
            R::DesignRead { project, repo, path, branch } => {
                self.design_read(&project, &repo, &path, branch.as_deref()).await
            }
            R::DesignBranchOpen { project, repo, slug, title, profile } => {
                self.design_branch_open(role, actor, &project, &repo, &slug, title, profile).await
            }
            R::DesignSave { project, repo, branch, path, base, text, summary } => {
                self.design_save(role, actor, &project, &repo, &branch, &path, base, text, summary).await
            }
            R::DesignSay { project, repo, branch, text } => {
                self.design_say(role, actor, &project, &repo, &branch, &text).await
            }
            R::DesignDiscard { project, repo, branch } => {
                self.design_discard(role, actor, &project, &repo, &branch).await
            }
            R::DesignApply { project, repo, branch } => self.design_apply(role, actor, &project, &repo, &branch).await,
            R::DesignHandoff { project, repo, branch, path, blob, blocks, title, profile, memo, reserve } => {
                let pick = crate::design_handoff::Handoff {
                    repo,
                    slug: branch,
                    path,
                    blob,
                    blocks,
                    title,
                    profile,
                    memo,
                    reserve,
                };
                // 할 일·저장·올리기·예약을 차례로 기다리는 큰 future다: 공용 dispatcher 스택에 얹지 않게 heap에 둔다
                Box::pin(self.design_handoff(role, actor, &project, pick)).await
            }
            _ => Err(err(ErrorCode::Internal, "기획 요청이 아니다")),
        }
    }

    /// 프로젝트 저장소마다 기획 트리(머신마다 동시에 읽는다)
    pub(crate) async fn design_tree(self: &Arc<Self>, project: &str) -> Result<FrontResponse, ErrorBody> {
        let targets = self.design_targets(project)?;
        let mut jobs = tokio::task::JoinSet::new();
        for (i, t) in targets.into_iter().enumerate() {
            let host = self.clone();
            let project = project.to_string();
            jobs.spawn(async move { (i, host.design_repo(&project, t).await) });
        }
        let mut repos = Vec::new();
        while let Some(done) = jobs.join_next().await {
            match done {
                Ok(v) => repos.push(v),
                Err(e) => return Err(crate::orch::internal(e)),
            }
        }
        repos.sort_by_key(|(i, _)| *i);
        Ok(FrontResponse::DesignTree { repos: repos.into_iter().map(|(_, v)| v).collect() })
    }

    /// 지운 옛 아이디어의 n-ID → 옮긴 기획 문서(`<저장소>/<경로>`, t-1959). 프로젝트 폴더 `idea-links.toml`의 한 단 표다.
    /// 없으면 빈 표다. 꼴이 틀리면 경고만 남기고 빈 표로 둔다(글 속 n-ID는 글자로 보인다). 기획 폴더가 있는 프로젝트 저장소의
    /// 그 폴더 안 문서만 싣는다(열 수 없는 문서로 잇지 않는다)
    pub(crate) fn idea_links(&self, project: &str) -> Result<std::collections::BTreeMap<String, String>, ErrorBody> {
        let dir = self.project_dir(project)?;
        let path = dir.join(IDEA_LINKS);
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Default::default()),
            Err(e) => {
                tracing::warn!(project, "{} 읽기 실패: {e}", path.display());
                return Ok(Default::default());
            }
        };
        let links = parse_idea_links(&text).unwrap_or_else(|e| {
            tracing::warn!(project, "{} 꼴이 틀렸다: {e}", path.display());
            Default::default()
        });
        let targets = self.design_targets(project)?;
        Ok(links
            .into_iter()
            .filter(|(_, doc)| {
                let (repo, path) = doc.split_once('/').unwrap_or_default();
                targets.iter().any(|t| {
                    t.name == repo
                        && t.folder.as_deref().is_some_and(|f| path.starts_with(&format!("{}/", f.trim_matches('/'))))
                })
            })
            .collect())
    }

    async fn design_repo(&self, project: &str, t: Target) -> DesignRepoView {
        let mut view = DesignRepoView {
            repo: t.name.clone(),
            machine: t.machine.clone().unwrap_or_default(),
            folder: t.folder.clone(),
            tree: None,
            error: None,
            branches: Vec::new(),
            handoffs: self.design_handoff_notes(project, &t.name),
        };
        let Some(folder) = t.folder.clone() else { return view };
        let mid = match self.resolve_machine(t.machine.as_deref()) {
            Ok(mid) => mid,
            Err(e) => {
                view.error = Some(e.message);
                return view;
            }
        };
        view.machine = self.machine_name(&mid);
        let key = format!("{project}/{}", t.name);
        let fresh = async {
            let conn = self.design_conn(&mid)?;
            let req = AgentRequest::DesignTree { repo: t.path.clone(), folder: folder.clone(), branch: None };
            match conn.request_timeout(req, TREE_WAIT).await? {
                AgentResponse::DesignTree(tree) => Ok(tree),
                _ => Err(err(ErrorCode::Internal, "예상하지 못한 응답")),
            }
        }
        .await;
        match fresh {
            Ok(tree) => {
                self.design_trees.lock().unwrap().insert(key, tree.clone());
                view.tree = Some(tree);
            }
            Err(e) => {
                // 마지막으로 읽은 트리를 보인다(머신이 꺼져 있어도 목록·문서 고르기는 된다)
                view.tree = self.design_trees.lock().unwrap().get(&key).cloned();
                view.error = Some(e.message);
            }
        }
        view.branches = self.design_branch_views(project, &t.name, &t.path, &folder, &mid).await;
        view
    }

    /// 마지막으로 읽은 그 저장소 트리의 commit(문서·그림을 같은 판으로 읽는다)
    fn design_commit(&self, project: &str, repo: &str) -> Option<String> {
        self.design_trees.lock().unwrap().get(&format!("{project}/{repo}")).map(|t: &DesignTree| t.commit.clone())
    }

    /// 문서 하나. `branch`(slug)면 host가 기억하는 그 기획 branch 끝에서 읽는다
    pub(crate) async fn design_read(
        &self,
        project: &str,
        repo: &str,
        path: &str,
        branch: Option<&str>,
    ) -> Result<FrontResponse, ErrorBody> {
        let (target, mid) = self.design_target(project, repo)?;
        let folder = target.folder.clone().unwrap_or_default();
        let path = checked_path(&folder, path)?;
        let (conn, commit) = match branch {
            Some(slug) => {
                self.design_known_branch(project, &target.name, slug)?;
                (self.design_write_conn(&mid)?, None)
            }
            None => (self.design_conn(&mid)?, self.design_commit(project, repo)),
        };
        let req = AgentRequest::DesignRead {
            repo: target.path.clone(),
            folder,
            commit,
            path: path.clone(),
            branch: branch.map(str::to_string),
        };
        let AgentResponse::DesignText { text, blob, commit, committed_ms } =
            conn.request_timeout(req, READ_WAIT).await?
        else {
            return Err(err(ErrorCode::Internal, "예상하지 못한 응답"));
        };
        let title = aaa_proto::design_title(&text);
        Ok(FrontResponse::DesignDoc {
            doc: DesignDoc {
                project: project.to_string(),
                repo: target.name,
                path,
                title,
                text,
                blob,
                commit,
                committed_ms,
                branch: branch.map(str::to_string),
            },
        })
    }

    /// host가 만든(기록이 있는) 기획 branch인가
    fn design_known_branch(&self, project: &str, repo: &str, slug: &str) -> Result<(), ErrorBody> {
        self.design_branch_load(project, repo, slug)?
            .map(|_| ())
            .ok_or_else(|| err(ErrorCode::NotFound, format!("host가 만든 기획 branch가 아니다: design/{slug}")))
    }

    /// 문서 안 그림·영상(웹 `GET /api/v1/projects/<p>/design/<저장소>/<경로>?c=<commit>`). 조각으로 받아 모은다. `range`는 HTTP
    /// `Range` 머리다(영상 넘기기·WebKit 영상): 먼저 크기만 묻고 그 범위만 받는다
    pub(crate) async fn design_file_read(
        &self,
        project: &str,
        repo: &str,
        path: &str,
        (commit, branch): (Option<&str>, Option<&str>),
        range: Option<&str>,
    ) -> Result<DesignBytes, ErrorBody> {
        let (target, mid) = self.design_target(project, repo)?;
        let folder = target.folder.clone().unwrap_or_default();
        let path = checked_path(&folder, path)?;
        let commit = match commit {
            Some(c) => c.to_string(),
            None => self
                .design_commit(project, repo)
                .ok_or_else(|| err(ErrorCode::NotFound, "기획 트리를 먼저 읽어야 한다"))?,
        };
        let conn = match branch {
            Some(slug) => {
                self.design_known_branch(project, &target.name, slug)?;
                self.design_write_conn(&mid)?
            }
            None => self.design_conn(&mid)?,
        };
        let file = FileAt { conn, repo: target.path, folder, commit, path, branch: branch.map(str::to_string) };
        let Some(range) = range else { return Ok(DesignBytes::Whole(file.read(0, u64::MAX).await?.0)) };
        let total = file.read(0, 0).await?.1;
        Ok(match crate::files::parse_range(Some(range), total) {
            Err(()) => DesignBytes::Unsatisfiable { total },
            Ok(None) => DesignBytes::Whole(file.read(0, u64::MAX).await?.0),
            Ok(Some((start, end))) => {
                let (bytes, total) = file.read(start, end - start + 1).await?;
                DesignBytes::Part { bytes, start, end, total }
            }
        })
    }
}

/// 웹에 보낼 기획 파일
pub(crate) enum DesignBytes {
    Whole(Vec<u8>),
    Part { bytes: Vec<u8>, start: u64, end: u64, total: u64 },
    Unsatisfiable { total: u64 },
}

/// 읽을 파일 하나(agent 연결·저장소·commit 고정)
struct FileAt {
    conn: crate::AgentConn,
    repo: String,
    folder: String,
    commit: String,
    path: String,
    branch: Option<String>,
}

impl FileAt {
    /// `offset`부터 `want`바이트(파일 끝까지)를 조각으로 받는다. 파일 전체 크기도 돌려준다. `want` 0은 크기만 묻는다.
    /// 끝까지 받지 못하면 오류다(잘린 그림을 commit 주소로 오래 두지 않는다)
    async fn read(&self, offset: u64, want: u64) -> Result<(Vec<u8>, u64), ErrorBody> {
        let mut bytes = Vec::new();
        loop {
            let len = (want - bytes.len() as u64).min(u64::from(CHUNK)) as u32;
            let req = AgentRequest::DesignFile {
                repo: self.repo.clone(),
                folder: self.folder.clone(),
                commit: self.commit.clone(),
                path: self.path.clone(),
                offset: offset + bytes.len() as u64,
                len,
                branch: self.branch.clone(),
            };
            let AgentResponse::DesignChunk { data, total } = self.conn.request_timeout(req, READ_WAIT).await? else {
                return Err(err(ErrorCode::Internal, "예상하지 못한 응답"));
            };
            if total > DESIGN_MAX_FILE {
                return Err(err(ErrorCode::TooLarge, "파일이 너무 크다"));
            }
            let got = data.0.len();
            bytes.extend_from_slice(&data.0);
            let end = want.min(total.saturating_sub(offset));
            if len == 0 || bytes.len() as u64 >= end {
                return Ok((bytes, total));
            }
            if got == 0 {
                return Err(err(ErrorCode::Internal, format!("파일을 다 받지 못했다({}/{end}바이트)", bytes.len())));
            }
        }
    }
}

/// 저장소 기준 경로를 정리하고 기획 폴더 안인지 본다
pub(crate) fn checked_path(folder: &str, path: &str) -> Result<String, ErrorBody> {
    let path = design_clean_path(path).ok_or_else(|| err(ErrorCode::BadRequest, "문서 경로가 잘못됐다"))?;
    if !design_inside(folder, &path) {
        return Err(err(ErrorCode::BadRequest, "기획 폴더 밖 경로다"));
    }
    Ok(path)
}

/// 확장자의 Content-Type. 모르는 것(HTML 포함)은 내려받기다. 응답은 sandbox CSP라 SVG의 script도 앱에 닿지 못한다
pub(crate) fn mime(path: &str) -> &'static str {
    let ext = Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "svg" => "image/svg+xml",
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "md" | "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// 프로젝트 폴더의 n-ID 링크 표 파일 이름
pub(crate) const IDEA_LINKS: &str = "idea-links.toml";

/// `"n-0010" = "pt/docs/design/game-loop.md"` 줄들. 키는 정규 n-ID, 값은 `<저장소>/<Markdown 경로>`(`..`·절대 경로 없음)만 받는다
pub(crate) fn parse_idea_links(text: &str) -> Result<std::collections::BTreeMap<String, String>, String> {
    let table: std::collections::BTreeMap<String, String> = toml::from_str(text).map_err(|e| e.to_string())?;
    let mut out = std::collections::BTreeMap::new();
    for (id, doc) in table {
        if aaa_proto::canonical_short_id(&id, 'n').as_deref() != Some(id.as_str()) {
            return Err(format!("n-ID가 아니다: {id}"));
        }
        let ok = doc.len() <= 512
            && doc.ends_with(".md")
            && doc.split_once('/').is_some_and(|(repo, path)| !repo.is_empty() && !path.is_empty())
            && doc.split('/').all(|p| !p.is_empty() && p != "." && p != ".." && !p.contains('\\'));
        if !ok {
            return Err(format!("{id}: 문서는 `<저장소>/<경로>.md`로 쓴다: {doc}"));
        }
        out.insert(id, doc);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// n-ID 링크 표(t-1959): 정규 n-ID와 `<저장소>/<경로>.md`만 받고, 틀린 줄이 있으면 표 전체를 버린다
    #[test]
    fn idea_links_accept_only_ids_and_repo_docs() {
        let ok =
            parse_idea_links("\"n-0010\" = \"pt/docs/design/game-loop.md\"\n\"n-0004\" = \"engine/doc/design/a.md\"\n")
                .unwrap();
        assert_eq!(ok.get("n-0010").map(String::as_str), Some("pt/docs/design/game-loop.md"));
        assert_eq!(ok.len(), 2);
        for bad in [
            "\"n-10\" = \"pt/a.md\"",
            "\"d-0010\" = \"pt/a.md\"",
            "\"n-0010\" = \"pt/../a.md\"",
            "\"n-0010\" = \"/abs/a.md\"",
            "\"n-0010\" = \"a.md\"",
            "\"n-0010\" = \"pt/a.png\"",
            "\"n-0010\" = 3",
        ] {
            assert!(parse_idea_links(bad).is_err(), "{bad}");
        }
        assert!(parse_idea_links("").unwrap().is_empty());
    }
    use crate::project::ProjectSettings;

    fn settings(toml: &str) -> ProjectSettings {
        crate::project::ProjectSettings::load_str(toml)
    }

    /// 기획 요청 future는 공용 dispatcher(`Host::call_from`)의 poll 스택에 임시 값으로 얹힌다(`Box::pin`이 heap에 옮기기 전).
    /// thread 끝내기·시작처럼 큰 future를 그 길 안에 그대로 품으면 모든 요청의 스택이 커진다: 2-2에서 버리기가 thread 끝내기
    /// (약 16KiB)를 품어 이 future가 3.9KiB → 20KiB가 되었고, 기기 폐기 시험이 Windows에서 스택을 넘쳤다(t-1894)
    #[test]
    fn design_requests_keep_the_dispatcher_stack_small() {
        let root = std::env::temp_dir().join(format!("aaa-design-size-{}", aaa_common::util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = root.join("projects").display().to_string();
        cfg.notify.web_push = false;
        let h = std::sync::Arc::new(crate::Host::open(cfg, root.join("host")).unwrap());
        let f = h.call_design(
            aaa_proto::DeviceRole::Local,
            "a",
            aaa_proto::FrontRequest::DesignTree { project: "p".into() },
        );
        let size = std::mem::size_of_val(&f);
        drop(f);
        drop(h);
        std::fs::remove_dir_all(&root).unwrap();
        assert!(size <= 8 * 1024, "기획 요청 future {size}바이트: 큰 future는 그 길 안에서 Box::pin 한다");
    }

    #[test]
    fn targets_pick_the_entry_with_design_and_keep_order() {
        let s = settings(
            "[[workspaces]]\npath = \"/w/lib\"\n\n[[workspaces]]\npath = \"/v/app\"\nmachine = \"mac-a\"\n\n\
             [[workspaces]]\npath = \"/x/app\"\nmachine = \"mac-b\"\ndesign = \"./docs/\"\n\n\
             [[workspaces]]\npath = \"/x/lib\"\nmachine = \"mac-b\"\n\n[[workspaces]]\npath = \"/y/tool\"\ndesign = \"../x\"\n",
        );
        let got: Vec<_> = targets(&s).into_iter().map(|t| (t.name, t.path, t.machine, t.folder)).collect::<Vec<_>>();
        assert_eq!(
            got,
            [
                ("lib".to_string(), "/w/lib".to_string(), None, None),
                ("app".into(), "/x/app".into(), Some("mac-b".into()), Some("docs".into())),
                ("tool".into(), "/y/tool".into(), None, None),
            ],
            "design 단 항목이 이기고, 잘못된 design은 없는 것"
        );
        assert_eq!(repo_name(r"C:\work\tool\"), "tool");
    }

    #[test]
    fn paths_stay_inside_the_folder() {
        assert_eq!(checked_path("docs", "./docs/a.md").unwrap(), "docs/a.md");
        assert_eq!(checked_path("docs", "README.md").unwrap_err().code, ErrorCode::BadRequest);
        assert_eq!(checked_path("docs", "docs/../x.md").unwrap_err().code, ErrorCode::BadRequest);
        assert_eq!((mime("docs/media/a.PNG"), mime("docs/x.html")), ("image/png", "application/octet-stream"));
    }
}

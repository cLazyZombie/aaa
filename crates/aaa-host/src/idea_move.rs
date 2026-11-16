//! 옛 아이디어 「기획으로 옮기기」(t-1894 4단계, 확정 계획 §6, 검수 q-3980~q-3985).
//!
//! - 사람이 저장소·위치·대화(새 대화면 `design/<주제>`)를 고르면 host가 그 기획 branch를 열고, 지금 판 노트가 쓰는 그림·영상을
//!   위치 폴더에 내용 hash 이름으로 commit한 뒤(2-3 붙이기와 같은 길) 노트 본문·그림 경로·옮기는 법을 그 대화에 보낸다.
//!   다른 머신의 대화 thread는 host의 노트 폴더를 읽지 못하므로 본문을 말에 싣는다(길면 자르고 `aaa idea show`로 읽게 한다).
//! - 아이디어는 그때부터 「옮기는 중」이고(`idea.json`의 `moved`), 옮길 때의 branch 끝 뒤 commit이 main에 들어가면 「옮김」이다.
//!   들어간 것은 「main에 반영」 thread가 확인한 착지 commit이나, 그 branch가 main보다 앞선 commit이 없어진 것으로 본다(ticker).
//!   branch를 버리면(들어가기 전) 다시 「다듬는 중」이 된다.

use std::sync::Arc;
use std::time::Duration;

use aaa_proto::{
    AgentRequest, AgentResponse, DesignBranchState, DesignTree, DeviceRole, ErrorBody, ErrorCode, FrontResponse,
    IdeaMoved, IdeaStatus,
};

use crate::ideas::{Folder, media_ext, media_refs};
use crate::orch::{bad, internal};
use crate::{Host, err};

/// 대화 말에 싣는 노트 본문 상한(바이트, 대화 말 상한 100,000 안). 넘으면 줄 경계에서 자르고 `aaa idea show`로 읽게 한다
const BODY_MAX: usize = 48 * 1024;
/// 같은 아이디어의 branch를 다시 보는 간격(ticker마다 agent에 묻지 않는다)
const CHECK_EVERY_MS: u64 = 60_000;
const TREE_WAIT: Duration = Duration::from_secs(120);

/// 옮기는 법(확정 계획 §6의 「구현」 칸 바꾸기)
const HOW: &str = "- 옮기는 법(노트 「구현」 칸):\n  - ✓ → 표지 없는 본문(지금 사실)\n  - △ 일부 → 된 만큼의 본문 + 남은 것의 \
                   `[!계획]` 블록\n  - ✗, 넘긴 할 일 있음 → `[!계획] d-NNNN`\n  - ✗, 할 일 없음 → `[!계획]`\n  - 미정 → 그 절의 \
                   `[!미정]`(이미 답한 것은 계획 글로)\n  - — → 머리말 본문\n  - 「바뀐 것」 절은 그대로, 목업 그림은 해당 블록 안 그림 + \
                   「시안」\n- main 코드와 견주어 이미 된 것은 본문으로 쓴다. 기존 문서와 겹치면 글을 겹쳐 쓰지 않고 링크로 잇는다.\n";

/// 대화에 보낼 옮기기 말
#[allow(clippy::too_many_arguments)]
fn move_message(
    short: &str,
    title: &str,
    rev: u32,
    tasks: &[String],
    place: &str,
    placed: &[(String, String)],
    text: &str,
) -> String {
    let mut out =
        format!("옛 아이디어 {short} 「{title}」(판 {rev})를 기획 폴더로 옮긴다. 노트 본문은 아래에 있다.\n\n");
    out.push_str(&format!("- 위치: `{place}/`(문서 이름과 나누기는 네가 정한다)\n"));
    if !placed.is_empty() {
        out.push_str(
            "- 그림·영상: host가 이 branch에 commit했다. 노트의 이름 → 저장소 경로(문서에서는 상대 경로로 잇는다):\n",
        );
        for (name, path) in placed {
            out.push_str(&format!("  - `{name}` → `{path}`\n"));
        }
    }
    if !tasks.is_empty() {
        out.push_str(&format!("- 이 아이디어에서 넘긴 할 일: {}\n", tasks.join(", ")));
    }
    out.push_str(HOW);
    out.push_str("- `aaa plan save`로 저장한다. 사람이 이 대화를 「main에 반영」하면 아이디어는 「옮김」이 된다.\n\n");
    let (body, cut) = cut_body(text);
    out.push_str(&format!("노트 {short} 판 {rev}:\n\n{}\n", fenced(body)));
    if cut {
        out.push_str(&format!(
            "\n노트가 길어 {}바이트 중 앞 {}바이트만 실었다. 나머지는 `aaa idea show {short}`로 읽는다.\n",
            text.len(),
            body.len()
        ));
    }
    out
}

/// 상한 안의 마지막 줄 경계까지(넘지 않으면 그대로)
fn cut_body(text: &str) -> (&str, bool) {
    if text.len() <= BODY_MAX {
        return (text, false);
    }
    let mut end = BODY_MAX;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let end = text[..end].rfind('\n').map_or(end, |i| i + 1);
    (&text[..end], true)
}

/// 글 안의 가장 긴 backtick 줄보다 긴 울타리로 감싼다
fn fenced(text: &str) -> String {
    let longest = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest.max(2) + 1);
    format!("{fence}markdown\n{}\n{fence}", text.trim_end_matches('\n'))
}

/// 옮긴 경로: 그 branch가 고친 문서(지운 것 빼고 Markdown) 하나면 그것, 여럿이면 공통 폴더, 없으면 None
fn moved_path(tree: &DesignTree) -> Option<String> {
    let docs: Vec<&str> = tree
        .changed
        .iter()
        .filter(|c| c.status != "D" && c.path.to_ascii_lowercase().ends_with(".md"))
        .map(|c| c.path.as_str())
        .collect();
    match docs.as_slice() {
        [] => None,
        [one] => Some(one.to_string()),
        [first, rest @ ..] => {
            let mut common: Vec<&str> = first.split('/').collect();
            common.pop();
            for p in rest {
                let parts: Vec<&str> = p.split('/').collect();
                let same = common.iter().zip(&parts).take_while(|(a, b)| a == b).count();
                common.truncate(same.min(parts.len().saturating_sub(1)));
            }
            Some(format!("{}/", common.join("/")))
        }
    }
}

impl Host {
    /// 「기획으로 옮기기」. 조작 권한의 사람만(thread·coordinator 세션은 `design_writer`가 거절한다)
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn idea_move(
        self: &Arc<Self>,
        role: DeviceRole,
        actor: &str,
        project: &str,
        idea: &str,
        rev: u32,
        repo: &str,
        slug: &str,
        title: Option<String>,
        place: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        self.design_writer(role, actor, project, None)?;
        let f = self.idea_folder(project, idea)?;
        // 한 아이디어는 한 번에 하나만 옮긴다(두 창·두 번 누름)
        let lock = self.thread_op_lock(project, &format!("idea-move:{}", f.id));
        let _one = lock.lock().await;
        let m = f.meta()?;
        let short = self.db.idea_short_id(project, &f.id).map_err(internal)?;
        if let Some(mv) = &m.moved {
            return Err(err(
                ErrorCode::Conflict,
                if mv.done {
                    format!("이미 옮겼다: {} {}", mv.repo, mv.path)
                } else {
                    format!("옮기는 중이다: {} design/{}", mv.repo, mv.branch)
                },
            ));
        }
        if m.status == IdeaStatus::Done {
            return Err(err(ErrorCode::Conflict, "완료한 아이디어는 옮기지 않는다(읽기 전용으로 보관한다)"));
        }
        if m.rev != rev {
            return Err(err(ErrorCode::Conflict, format!("노트가 바뀌었다(판 {rev} → {}): 다시 연다", m.rev)));
        }
        let text = f.text(None)?;
        let (target, mid) = self.design_target(project, repo)?;
        let folder = target.folder.clone().unwrap_or_default();
        let place = place.trim().trim_matches('/');
        // 위치는 기획 폴더 안(비우면 기획 폴더). 문서 경로처럼 검사한다
        let place_path = if place.is_empty() {
            folder.trim_end_matches('/').to_string()
        } else {
            let doc = crate::design::checked_path(&folder, &format!("{place}/x.md"))?;
            doc.trim_end_matches("/x.md").to_string()
        };
        // 그림·영상은 보내기 전에 모두 확인한다(하나라도 못 붙이면 아무것도 commit하지 않는다)
        let media = self.idea_move_media(&f, &text)?;
        if !media.is_empty() && !self.machine_feature(&mid, crate::design_branch::ATTACH_FEATURE) {
            return Err(err(
                ErrorCode::Unavailable,
                format!(
                    "{} agent가 기획 그림 붙이기를 모른다({} 없음). 그 머신 aaa를 갱신한다",
                    self.machine_name(&mid),
                    crate::design_branch::ATTACH_FEATURE
                ),
            ));
        }
        // branch 열기: 없으면 만들고, 쉬면 다시 연다
        let old = self.design_branch_load(project, &target.name, slug)?;
        if old.as_ref().is_none_or(|r| r.state == DesignBranchState::Rest) {
            let title = title.filter(|t| !t.trim().is_empty()).unwrap_or_else(|| format!("옮김: {}", m.title));
            Box::pin(self.design_branch_open_with(role, actor, project, repo, slug, Some(title), None, old.is_none()))
                .await?;
        }
        let rec = self
            .design_branch_load(project, &target.name, slug)?
            .ok_or_else(|| err(ErrorCode::NotFound, format!("기획 branch가 없다: design/{slug}")))?;
        // 옮길 때의 끝: 이 뒤의 commit이 main에 들어가야 옮긴 것이다(열 때 agent가 읽은 끝)
        let head = rec.head.clone();
        let mut placed = Vec::new();
        for (name, bytes) in &media {
            let saved = Box::pin(self.design_attach_quiet(role, actor, project, repo, slug, place, bytes)).await?;
            placed.push((name.clone(), saved));
        }
        let tasks = self.idea_summary(project, &f, &m)?.tasks;
        let message = move_message(&short, &m.title, rev, &tasks, &place_path, &placed, &text);
        Box::pin(self.design_say(role, actor, project, repo, slug, &message)).await?;
        {
            let _w = self.idea_writing()?;
            let mut m = f.meta()?;
            m.moved = Some(IdeaMoved {
                repo: target.name.clone(),
                branch: slug.to_string(),
                path: format!("{place_path}/"),
                done: false,
                at_ms: aaa_common::util::now_ms(),
                rev,
                head,
                branch_ms: rec.created_ms,
            });
            f.write_meta(&m)?;
        }
        self.ideas_bump(project);
        self.db.audit(actor, "idea_move", &format!("{project} {short} → {}/{slug} {place_path}", target.name));
        Ok(FrontResponse::Idea { note: self.idea_note(project, &f, None)? })
    }

    /// 지금 판이 쓰는 그림·영상(노트 이름, 내용). 없거나 형식·크기가 맞지 않으면 거절한다
    fn idea_move_media(&self, f: &Folder, text: &str) -> Result<Vec<(String, Vec<u8>)>, ErrorBody> {
        let mut out = Vec::new();
        for name in media_refs(text) {
            let bytes =
                std::fs::read(f.dir.join(&name)).map_err(|e| bad(format!("노트의 그림을 읽지 못했다: {name}: {e}")))?;
            if bytes.is_empty() || bytes.len() as u64 > aaa_proto::DESIGN_MAX_FILE {
                return Err(bad(format!("그림·영상은 1바이트부터 16MiB까지 옮긴다: {name}")));
            }
            if media_ext(&bytes).is_none() {
                return Err(bad(format!("그림(PNG·JPEG·GIF·WebP)·영상(MP4·WebM)만 옮긴다: {name}")));
            }
            out.push((name, bytes));
        }
        Ok(out)
    }

    /// 그 저장소 머신 agent가 읽은 기획 branch(main과 다른 파일·앞선 commit 수)
    async fn idea_move_tree(&self, project: &str, repo: &str, slug: &str) -> Result<DesignTree, ErrorBody> {
        let (target, mid) = self.design_target(project, repo)?;
        let conn = self.design_write_conn(&mid)?;
        let req = AgentRequest::DesignTree {
            repo: target.path.clone(),
            folder: target.folder.clone().unwrap_or_default(),
            branch: Some(slug.to_string()),
        };
        match conn.request_timeout(req, TREE_WAIT).await? {
            AgentResponse::DesignTree(tree) => Ok(tree),
            other => Err(internal(format!("기획 branch 응답이 아니다: {other:?}"))),
        }
    }

    /// 옮긴 뒤의 commit이 main에 들어갔나: 「main에 반영」 thread가 끝나며 확인한 착지 commit, 또는 그 branch가 main보다 앞선
    /// commit이 없고 끝이 옮길 때와 다르다(구현 맡기기로 코드와 함께 들어간 것도 이것이다). 묻지 못하면 None
    async fn idea_move_landed(
        &self,
        project: &str,
        mv: &IdeaMoved,
        apply: Option<&str>,
    ) -> Option<(bool, Option<String>)> {
        if let Some(id) = apply
            && let Ok(t) = self.load(project, id)
            && t.status == aaa_proto::ThreadStatus::Resolved
            && self.thread_main_merge(&t).await.is_some()
        {
            return Some((true, None));
        }
        let tree = match self.idea_move_tree(project, &mv.repo, &mv.branch).await {
            Ok(tree) => tree,
            Err(e) => {
                tracing::info!(project, repo = %mv.repo, slug = %mv.branch, "옮기는 branch를 읽지 못했다: {}", e.message);
                return None;
            }
        };
        let landed = tree.ahead == 0 && mv.head.as_deref() != Some(tree.commit.as_str());
        Some((landed, moved_path(&tree)))
    }

    /// ticker: 옮기는 중인 아이디어의 branch를 본다. 들어갔으면 「옮김」, branch 기록이 없어졌으면(버림) 다시 「다듬는 중」,
    /// 아니면 고친 문서로 경로를 잡아 둔다(들어간 뒤에는 main과 다른 파일이 없다)
    pub(crate) async fn idea_move_pass(self: &Arc<Self>, project: &str) {
        let Ok(list) = self.idea_list(project) else { return };
        let now = aaa_common::util::now_ms();
        for idea in list {
            let Some(mv) = idea.moved.filter(|m| !m.done) else { continue };
            {
                let mut seen = self.idea_move_checked.lock().unwrap();
                let key = format!("{project}/{}", idea.id);
                if seen.get(&key).is_some_and(|at| now.saturating_sub(*at) < CHECK_EVERY_MS) {
                    continue;
                }
                seen.insert(key, now);
            }
            self.idea_move_settle(project, &idea.id, &mv.repo, &mv.branch, false).await;
        }
    }

    /// 한 아이디어의 옮기기 상태를 맞춘다. `discarding`이면 그 branch를 곧 지운다: 들어가지 않았으면 풀어 준다
    pub(crate) async fn idea_move_settle(&self, project: &str, idea: &str, repo: &str, slug: &str, discarding: bool) {
        let Ok(f) = self.idea_folder(project, idea) else { return };
        let lock = self.thread_op_lock(project, &format!("idea-move:{}", f.id));
        let _one = lock.lock().await;
        let Some(mv) = f.meta().ok().and_then(|m| m.moved).filter(|m| !m.done && m.repo == repo && m.branch == slug)
        else {
            return;
        };
        let rec = self.design_branch_load(project, repo, slug).ok().flatten().filter(|r| r.created_ms == mv.branch_ms);
        let next = match rec {
            // 다른 길로 기록이 없어졌다(들어갔는지 물을 branch가 없다): 풀어 준다
            None => None,
            Some(rec) => match self.idea_move_landed(project, &mv, rec.apply.as_deref()).await {
                Some((true, path)) => {
                    Some(IdeaMoved { done: true, path: path.unwrap_or(mv.path.clone()), ..mv.clone() })
                }
                Some((false, _)) if discarding => None,
                Some((false, Some(path))) => Some(IdeaMoved { path, ..mv.clone() }),
                Some((false, None)) => return,
                // 묻지 못했다: 버리는 중이면 들어갔는지 모른 채 풀지 않는다(다음 셈이 기록 없음으로 푼다)
                None if discarding => return,
                None => return,
            },
        };
        if next.as_ref() == Some(&mv) {
            return;
        }
        let saved = (|| {
            let _w = self.idea_writing()?;
            let mut m = f.meta()?;
            m.moved = next.clone();
            f.write_meta(&m)
        })();
        match saved {
            Ok(()) => {
                let what = match &next {
                    Some(n) if n.done => format!("옮김 {}", n.path),
                    Some(n) => format!("경로 {}", n.path),
                    None => "풀어 줌".to_string(),
                };
                self.db.audit("host", "idea_move_settle", &format!("{project} {idea} {repo}/{slug} {what}"));
                self.ideas_bump(project);
            }
            Err(e) => tracing::warn!(project, idea, "옮기기 상태를 쓰지 못했다: {}", e.message),
        }
    }

    /// 그 branch로 옮기는 중인 아이디어(버리기 전에 맞춘다)
    pub(crate) fn ideas_moving_to(&self, project: &str, repo: &str, slug: &str) -> Vec<String> {
        self.idea_list(project)
            .unwrap_or_default()
            .into_iter()
            .filter(|i| i.moved.as_ref().is_some_and(|m| !m.done && m.repo == repo && m.branch == slug))
            .map(|i| i.id)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aaa_proto::DesignChange;

    fn tree(ahead: u32, changed: &[(&str, &str)]) -> DesignTree {
        DesignTree {
            ahead,
            changed: changed
                .iter()
                .map(|(p, s)| DesignChange { path: p.to_string(), status: s.to_string(), ..Default::default() })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn moved_path_is_the_one_doc_or_the_common_folder() {
        assert_eq!(moved_path(&tree(1, &[])), None);
        assert_eq!(moved_path(&tree(1, &[("docs/a/3f.png", "A")])), None, "그림만이면 아직 문서가 없다");
        assert_eq!(
            moved_path(&tree(1, &[("docs/a/README.md", "M"), ("docs/a/x.png", "A")])).as_deref(),
            Some("docs/a/README.md")
        );
        assert_eq!(
            moved_path(&tree(2, &[("docs/a/README.md", "M"), ("docs/a/b/skill.md", "A")])).as_deref(),
            Some("docs/a/")
        );
        assert_eq!(moved_path(&tree(2, &[("docs/a/x.md", "A"), ("docs/b/y.md", "A")])).as_deref(), Some("docs/"));
        assert_eq!(moved_path(&tree(1, &[("docs/a/x.md", "D"), ("docs/b/y.md", "A")])).as_deref(), Some("docs/b/y.md"));
    }

    #[test]
    fn message_carries_body_media_and_rules() {
        let text = "# 캠프\n\n```\ncode\n```\n![지도](map.png)\n";
        let msg = move_message(
            "n-0003",
            "캠프 게시판",
            2,
            &["d-0001".to_string()],
            "docs/camp",
            &[("map.png".to_string(), "docs/camp/0123456789abcdef.png".to_string())],
            text,
        );
        assert!(msg.starts_with("옛 아이디어 n-0003 「캠프 게시판」(판 2)를 기획 폴더로 옮긴다."), "{msg}");
        assert!(msg.contains("- 위치: `docs/camp/`"), "{msg}");
        assert!(msg.contains("  - `map.png` → `docs/camp/0123456789abcdef.png`"), "{msg}");
        assert!(msg.contains("넘긴 할 일: d-0001"), "{msg}");
        assert!(msg.contains("`[!계획] d-NNNN`") && msg.contains("「시안」"), "{msg}");
        assert!(
            msg.contains("````markdown\n# 캠프\n\n```\ncode\n```\n![지도](map.png)\n````"),
            "본문 울타리가 안의 울타리보다 길다: {msg}"
        );
        assert!(!msg.contains("aaa idea show"), "다 실었으면 따로 읽으라 하지 않는다");
    }

    #[test]
    fn long_body_is_cut_at_a_line() {
        let line = "가나다라마바사 아자차카타파하\n";
        let text = line.repeat(BODY_MAX / line.len() + 10);
        let (body, cut) = cut_body(&text);
        assert!(cut && body.len() <= BODY_MAX && body.ends_with('\n'));
        let msg = move_message("n-0001", "긴 것", 1, &[], "docs", &[], &text);
        assert!(msg.contains(&format!("{}바이트 중 앞 {}바이트만", text.len(), body.len())), "{msg}");
        assert!(msg.contains("`aaa idea show n-0001`로 읽는다"));
        assert!(msg.len() < 100_000, "대화 말 상한 안");
    }
}

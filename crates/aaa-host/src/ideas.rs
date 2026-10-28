//! 아이디어 노트(t-1260, 기획 t-1255 결정 28개): 아이디어는 한 번에 정리되지 않으므로 「중간 정리본(노트)」이 정본이고, 대화·
//! 직접 고치기·인용 의견으로 계속 고친다. host 프로젝트 폴더에 둔다.
//!
//! - `ideas/<ULID>/note.md` 지금 판, `idea.json` 메타(제목·상태·판·기록·넘긴 할 일·전용 thread), `revs/<판>.md` 판마다의 글,
//!   그림·영상 `<sha256 앞 16자>.<ext>`(노트는 `![그림](<이름>)`으로 같은 폴더를 가리킨다), `merge/` 겹친 사람 글.
//! - 짧은 ID `n-NNNN`은 host DB `idea_ids` 원장(할 일 `task_ids`와 같은 꼴)이다. 복사·완료 뒤에도 번호를 재사용하지 않는다.
//! - 저장은 기준 판을 준다(spec-kit `/clarify`처럼 본문과 「바뀐 것」을 함께 고치는 것은 thread 몫이다). 지금 판이 아니면 쓰지 않는다:
//!   thread는 두 판을 받아 합치고, 사람 글은 `merge/`에 두고 thread에 합치기를 맡긴다(결정 11).
//! - 할 일로 넘기면 완료로 잠근다(결정 3·23). 다시 다듬으려면 복사해서 새 아이디어로 한다.

use std::path::PathBuf;

use aaa_proto::{
    ErrorBody, ErrorCode, FrontResponse, IdeaAuthor, IdeaConflict, IdeaNote, IdeaRevView, IdeaStatus, IdeaSummary,
    IdeaTaskDraft, TaskOp,
};
use serde::{Deserialize, Serialize};

use crate::orch::{bad, internal, write_atomic};
use crate::{Host, err};

/// 노트 글 상한(TASKS.md와 같다)
pub(crate) const MAX_NOTE: usize = 512 * 1024;
/// 판 기록에 남기는 요약 한 줄 상한(바이트)
const MAX_SUMMARY: usize = 2000;
/// 노트 쓰기는 한 번에 하나(사람·thread가 함께 저장해도 판 번호가 겹치지 않게)
pub(crate) static WRITING: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// `idea.json`
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Meta {
    pub title: String,
    pub status: IdeaStatus,
    pub rev: u32,
    pub created_ms: u64,
    pub updated_ms: u64,
    /// 오래된 판부터
    pub history: Vec<IdeaRevView>,
    /// 넘긴 할 일 ULID
    pub tasks: Vec<String>,
    /// 전용 thread ID
    pub thread: Option<String>,
    /// 복사해 온 아이디어 ULID
    pub copied_from: Option<String>,
    /// 사람이 고친 판의 요약 중 thread에 아직 알리지 않은 것(다음 말 앞에 붙인다, 결정 16)
    pub edited: Vec<String>,
    /// 겹친 사람 글을 thread가 합치는 중이다: 그 글의 기준 판
    pub merging: Option<u32>,
    /// 전용 thread가 준비되기를 기다리는 말(idea_thread.rs)
    pub pending: Vec<String>,
    /// 전용 thread를 띄우지 못한 까닭(다음 말에 지운다)
    pub error: Option<String>,
}

/// 폴더 이름(ULID: 26자 Crockford base32 대문자)
pub(crate) fn valid_id(id: &str) -> bool {
    id.len() == 26 && id.bytes().all(|b| b.is_ascii_digit() || (b.is_ascii_uppercase() && !b"ILOU".contains(&b)))
}

/// 노트의 첫 `# 제목`
pub(crate) fn heading(text: &str) -> Option<String> {
    text.lines().filter_map(|l| l.trim().strip_prefix("# ")).map(str::trim).find(|t| !t.is_empty()).map(str::to_string)
}

fn one_line(what: &str, text: &str, max: usize) -> Result<String, ErrorBody> {
    let t = text.trim();
    if t.is_empty() || t.len() > max || t.chars().any(char::is_control) {
        return Err(bad(format!("{what}은 비지 않은 한 줄({max}바이트 이하)로 쓴다")));
    }
    Ok(t.to_string())
}

fn check_text(text: &str) -> Result<(), ErrorBody> {
    if text.len() > MAX_NOTE {
        return Err(err(ErrorCode::TooLarge, format!("노트는 {}KiB까지 쓴다", MAX_NOTE / 1024)));
    }
    if text.contains('\0') {
        return Err(bad("노트에 NUL 문자가 있다"));
    }
    Ok(())
}

/// `## 절` 이름(첫 절 앞은 빈 이름)과 그 줄들
fn sections(text: &str) -> Vec<(String, Vec<&str>)> {
    let mut out: Vec<(String, Vec<&str>)> = vec![(String::new(), Vec::new())];
    for line in text.lines() {
        if let Some(h) = line.strip_prefix("## ") {
            out.push((h.trim().to_string(), Vec::new()));
        } else if !line.starts_with("# ") && !line.trim().is_empty() {
            out.last_mut().unwrap().1.push(line.trim_end());
        }
    }
    out
}

/// 사람이 고친 판의 요약: 절마다 더하고 뺀 줄 수(같은 수면 「고침」). 절이 없는 노트는 「노트」로 센다
pub(crate) fn summarize(before: &str, after: &str) -> String {
    let (old, new) = (sections(before), sections(after));
    let mut names: Vec<&str> = new.iter().map(|(n, _)| n.as_str()).collect();
    for (n, _) in &old {
        if !names.contains(&n.as_str()) {
            names.push(n);
        }
    }
    let lines = |list: &[(String, Vec<&str>)], name: &str| -> Vec<String> {
        list.iter().filter(|(n, _)| n == name).flat_map(|(_, l)| l.iter().map(|s| s.to_string())).collect()
    };
    let mut parts = Vec::new();
    for name in names {
        let (a, b) = (lines(&old, name), lines(&new, name));
        let mut left = a.clone();
        let mut added = 0usize;
        for l in &b {
            if let Some(i) = left.iter().position(|x| x == l) {
                left.remove(i);
            } else {
                added += 1;
            }
        }
        let removed = left.len();
        let what = match (added, removed) {
            (0, 0) => continue,
            (a, r) if a == r => format!("{a}줄 고침"),
            (a, 0) => format!("{a}줄 더함"),
            (0, r) => format!("{r}줄 뺌"),
            (a, r) => format!("{a}줄 더함·{r}줄 뺌"),
        };
        let place = if name.is_empty() { "머리".to_string() } else { format!("「{name}」") };
        parts.push(format!("{place} {what}"));
    }
    if heading(before) != heading(after) {
        parts.insert(0, "제목 바꿈".into());
    }
    if parts.is_empty() { "줄 바꿈만".into() } else { parts.join(", ") }
}

/// 한 아이디어 폴더
#[derive(Debug)]
pub(crate) struct Folder {
    pub id: String,
    pub dir: PathBuf,
}

impl Folder {
    fn meta_path(&self) -> PathBuf {
        self.dir.join("idea.json")
    }

    pub(crate) fn note_path(&self) -> PathBuf {
        self.dir.join("note.md")
    }

    fn rev_path(&self, rev: u32) -> PathBuf {
        self.dir.join("revs").join(format!("{rev:04}.md"))
    }

    pub(crate) fn meta(&self) -> Result<Meta, ErrorBody> {
        let bytes =
            std::fs::read(self.meta_path()).map_err(|e| internal(format!("ideas/{}/idea.json: {e}", self.id)))?;
        serde_json::from_slice(&bytes).map_err(|e| internal(format!("ideas/{}/idea.json: {e}", self.id)))
    }

    pub(crate) fn write_meta(&self, meta: &Meta) -> Result<(), ErrorBody> {
        let bytes = serde_json::to_vec_pretty(meta).map_err(internal)?;
        write_atomic(&self.meta_path(), &bytes).map_err(|e| internal(format!("ideas/{}/idea.json: {e}", self.id)))
    }

    pub(crate) fn text(&self, rev: Option<u32>) -> Result<String, ErrorBody> {
        let path = rev.map_or_else(|| self.note_path(), |r| self.rev_path(r));
        match std::fs::read(&path) {
            Ok(b) => String::from_utf8(b).map_err(|_| internal(format!("{}: UTF-8 글이 아니다", path.display()))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(err(ErrorCode::NotFound, format!("{} 판이 없다", rev.map_or("지금".into(), |r| r.to_string()))))
            }
            Err(e) => Err(internal(format!("{}: {e}", path.display()))),
        }
    }

    /// 새 판을 쓴다(판 글 → 지금 노트 → 메타 차례: 메타가 늦으면 다음 저장이 같은 판 번호로 덮어 일관을 되찾는다)
    fn write_rev(
        &self,
        meta: &mut Meta,
        text: &str,
        by: IdeaAuthor,
        summary: Option<String>,
        now: u64,
    ) -> Result<(), ErrorBody> {
        let rev = meta.rev + 1;
        let io = |e: std::io::Error| internal(format!("ideas/{}: {e}", self.id));
        write_atomic(&self.rev_path(rev), text.as_bytes()).map_err(io)?;
        write_atomic(&self.note_path(), text.as_bytes()).map_err(io)?;
        meta.rev = rev;
        meta.updated_ms = now;
        if let Some(t) = heading(text) {
            meta.title = t;
        }
        meta.history.push(IdeaRevView { rev, at_ms: now, by, summary });
        self.write_meta(meta)
    }
}

impl Host {
    pub(crate) fn ideas_bump(&self, project: &str) {
        aaa_common::diagnostic::ignored(
            self.ideas_changed.send(project.to_string()),
            "웹 연결이 없으면 받을 곳이 없다",
        );
    }

    fn ideas_root(&self, project: &str) -> Result<PathBuf, ErrorBody> {
        Ok(self.project_dir(project)?.join("ideas"))
    }

    /// `n-NNNN`·ULID → 폴더. 없는 아이디어는 NotFound
    pub(crate) fn idea_folder(&self, project: &str, idea: &str) -> Result<Folder, ErrorBody> {
        let idea = idea.trim();
        let id = match self.db.idea_long_id(project, idea).map_err(internal)? {
            Some(id) => id,
            None if valid_id(idea) => idea.to_string(),
            None => return Err(err(ErrorCode::NotFound, format!("아이디어가 없다: {idea}"))),
        };
        let dir = self.ideas_root(project)?.join(&id);
        if !dir.join("idea.json").is_file() {
            return Err(err(ErrorCode::NotFound, format!("아이디어가 없다: {idea}")));
        }
        Ok(Folder { id, dir })
    }

    fn idea_summary(&self, project: &str, f: &Folder, m: &Meta) -> Result<IdeaSummary, ErrorBody> {
        let tasks = m
            .tasks
            .iter()
            .map(|t| self.db.task_short_id(project, t))
            .collect::<anyhow::Result<Vec<_>>>()
            .map_err(internal)?;
        let copied_from =
            m.copied_from.as_deref().map(|c| self.db.idea_short_id(project, c)).transpose().map_err(internal)?;
        Ok(IdeaSummary {
            id: f.id.clone(),
            short_id: self.db.idea_short_id(project, &f.id).map_err(internal)?,
            title: m.title.clone(),
            status: m.status,
            rev: m.rev,
            created_ms: m.created_ms,
            updated_ms: m.updated_ms,
            tasks,
            thread: m.thread.clone(),
            copied_from,
        })
    }

    pub(crate) fn idea_note(&self, project: &str, f: &Folder, rev: Option<u32>) -> Result<IdeaNote, ErrorBody> {
        let m = f.meta()?;
        let rev = rev.unwrap_or(m.rev);
        if rev == 0 || rev > m.rev {
            return Err(err(ErrorCode::NotFound, format!("{rev} 판이 없다(지금 {})", m.rev)));
        }
        let text = f.text((rev != m.rev).then_some(rev))?;
        Ok(IdeaNote {
            project: project.to_string(),
            idea: self.idea_summary(project, f, &m)?,
            rev,
            text,
            history: m.history.iter().rev().cloned().collect(),
            edited: (!m.edited.is_empty()).then(|| m.edited.join("; ")),
            merging: m.merging,
            pending: m.pending.clone(),
            error: m.error.clone(),
        })
    }

    pub(crate) fn idea_list(&self, project: &str) -> Result<Vec<IdeaSummary>, ErrorBody> {
        let root = self.ideas_root(project)?;
        let entries = match std::fs::read_dir(&root) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(internal(format!("{}: {e}", root.display()))),
        };
        let mut out = Vec::new();
        for e in entries {
            let e = e.map_err(|e| internal(format!("{}: {e}", root.display())))?;
            let Some(id) = e.file_name().to_str().map(str::to_string).filter(|n| valid_id(n)) else { continue };
            let f = Folder { id, dir: e.path() };
            if !f.meta_path().is_file() {
                continue;
            }
            // 메타 하나가 깨져도 다른 아이디어는 보인다. 깨진 것은 로그에 남긴다
            match f.meta() {
                Ok(m) => out.push(self.idea_summary(project, &f, &m)?),
                Err(e) => tracing::warn!(project, idea = %f.id, "아이디어 메타를 읽지 못했다: {}", e.message),
            }
        }
        // 다듬는 중을 먼저, 그 안에서 최근에 고친 것부터
        out.sort_by(|a, b| {
            (a.status, std::cmp::Reverse(a.updated_ms)).cmp(&(b.status, std::cmp::Reverse(b.updated_ms)))
        });
        Ok(out)
    }

    pub(crate) fn idea_create(
        &self,
        project: &str,
        title: &str,
        text: &str,
        copied_from: Option<String>,
        actor: &str,
    ) -> Result<IdeaNote, ErrorBody> {
        let title = one_line("제목", title, 400)?;
        check_text(text)?;
        let text = if text.trim().is_empty() { format!("# {title}\n") } else { text.to_string() };
        let id = aaa_common::util::new_id();
        let f = Folder { dir: self.ideas_root(project)?.join(&id), id };
        let now = aaa_common::util::now_ms();
        let mut meta = Meta { title, created_ms: now, copied_from, ..Default::default() };
        {
            let _w = WRITING.lock().unwrap_or_else(|e| e.into_inner());
            std::fs::create_dir_all(&f.dir).map_err(|e| internal(format!("{}: {e}", f.dir.display())))?;
            f.write_rev(&mut meta, &text, IdeaAuthor::User, Some("만듦".into()), now)?;
        }
        // 번호는 폴더를 쓴 뒤 예약한다(쓰다 실패한 아이디어가 번호를 먹지 않게)
        let short = self.db.idea_short_id(project, &f.id).map_err(internal)?;
        self.db.audit(actor, "idea_create", &format!("{project}/{short}"));
        self.ideas_bump(project);
        self.idea_note(project, &f, None)
    }

    /// 판을 주고 저장한다. `by`가 Thread면 겹칠 때 두 판을 돌려주고, User면 겹친 글을 `merge/`에 두고 `Err(merge 경로)`를 준다
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn idea_write(
        &self,
        project: &str,
        f: &Folder,
        base_rev: u32,
        text: &str,
        summary: Option<String>,
        by: IdeaAuthor,
        actor: &str,
    ) -> Result<Result<IdeaNote, Saved>, ErrorBody> {
        check_text(text)?;
        let summary = summary.map(|s| one_line("바뀐 곳 요약", &s, MAX_SUMMARY)).transpose()?;
        let short = self.db.idea_short_id(project, &f.id).map_err(internal)?;
        let outcome = {
            let _w = WRITING.lock().unwrap_or_else(|e| e.into_inner());
            let mut meta = f.meta()?;
            if meta.status == IdeaStatus::Done {
                return Err(bad(format!("{short}는 할 일로 넘겨 잠겼다. 다시 다듬으려면 복사해서 새 아이디어로 한다")));
            }
            if base_rev == 0 || base_rev > meta.rev {
                return Err(bad(format!("{base_rev} 판이 없다(지금 {})", meta.rev)));
            }
            let now = aaa_common::util::now_ms();
            if base_rev != meta.rev {
                let base_text = f.text(Some(base_rev))?;
                if by == IdeaAuthor::Thread {
                    Saved::Conflict(base_rev, base_text)
                } else {
                    // 사람 글은 버리지 않는다(결정 11): 따로 두고 thread가 합친다
                    let path = f.dir.join("merge").join(format!("{base_rev:04}-{now}.md"));
                    write_atomic(&path, text.as_bytes()).map_err(|e| internal(format!("{}: {e}", path.display())))?;
                    meta.merging = Some(base_rev);
                    f.write_meta(&meta)?;
                    Saved::Merge { base_rev, mine: path, current: meta.rev }
                }
            } else {
                let before = f.text(None)?;
                if before == text {
                    Saved::Same
                } else {
                    let summary = match by {
                        IdeaAuthor::User => {
                            let s = summarize(&before, text);
                            meta.edited.push(format!("판 {} {s}", meta.rev + 1));
                            Some(s)
                        }
                        IdeaAuthor::Thread => {
                            // thread가 합쳤다: 합치는 중 표지를 푼다
                            meta.merging = None;
                            summary
                        }
                    };
                    f.write_rev(&mut meta, text, by, summary, now)?;
                    Saved::Written
                }
            }
        };
        match outcome {
            Saved::Same => Ok(Ok(self.idea_note(project, f, None)?)),
            Saved::Written => {
                self.db.audit(actor, "idea_save", &format!("{project}/{short}"));
                self.ideas_bump(project);
                Ok(Ok(self.idea_note(project, f, None)?))
            }
            Saved::Merge { .. } => {
                self.db.audit(actor, "idea_save_merge", &format!("{project}/{short}"));
                self.ideas_bump(project);
                Ok(Err(outcome))
            }
            Saved::Conflict(..) => Ok(Err(outcome)),
        }
    }

    pub(crate) fn idea_conflict(
        &self,
        project: &str,
        f: &Folder,
        base_rev: u32,
        base_text: String,
    ) -> Result<FrontResponse, ErrorBody> {
        Ok(FrontResponse::IdeaConflict(IdeaConflict {
            base_rev,
            base_text,
            current: self.idea_note(project, f, None)?,
        }))
    }

    /// 할 일로 넘긴다: 초안마다 할 일을 더하고(메모 첫 줄에 아이디어 링크) 완료로 잠근다
    pub(crate) async fn idea_handoff(
        &self,
        project: &str,
        idea: &str,
        base_rev: u32,
        drafts: Vec<IdeaTaskDraft>,
        actor: &str,
    ) -> Result<IdeaNote, ErrorBody> {
        let f = self.idea_folder(project, idea)?;
        let meta = f.meta()?;
        let short = self.db.idea_short_id(project, &f.id).map_err(internal)?;
        if meta.status == IdeaStatus::Done {
            return Err(bad(format!("{short}는 이미 할 일로 넘겼다")));
        }
        if meta.rev != base_rev {
            return Err(err(
                ErrorCode::Conflict,
                format!("노트가 그사이 판 {}으로 바뀌었다(본 판 {base_rev}). 다시 보고 넘긴다", meta.rev),
            ));
        }
        if meta.merging.is_some() {
            return Err(bad("thread가 겹친 고침을 합치는 중이다. 합친 뒤 넘긴다"));
        }
        if drafts.is_empty() {
            return Err(bad("넘길 할 일이 없다"));
        }
        let board = self.task_board(project)?;
        let list = board
            .lists
            .iter()
            .find(|l| l.name == "Backlog")
            .or_else(|| board.lists.iter().find(|l| !l.name.is_empty()))
            .map_or_else(|| "Backlog".to_string(), |l| l.name.clone());
        let link = idea_link(&short);
        let mut ops = Vec::new();
        for d in &drafts {
            let title = one_line("할 일 제목", &d.title, 400)?;
            let mut notes: Vec<String> = vec![link.clone()];
            notes.extend(d.notes.lines().map(|l| l.trim_end().to_string()).filter(|l| l.trim() != link));
            while notes.last().is_some_and(|l| l.trim().is_empty()) {
                notes.pop();
            }
            ops.push(TaskOp::Add {
                list: list.clone(),
                title,
                owner: None,
                person: false,
                profile: None,
                machine: None,
                notes,
                parent: None,
                before: None,
                thread: None,
                repo: d.repo.clone().filter(|r| !r.trim().is_empty()),
            });
        }
        let before: std::collections::HashSet<String> = board.tasks.iter().filter_map(|t| t.id.clone()).collect();
        let edited = self.task_edit(project, ops, Some(board.sha256.clone()), None, actor).await?;
        let FrontResponse::TaskEdited { board: after, conflict, .. } = edited else {
            return Err(internal("할 일 편집 답이 아니다"));
        };
        if let Some(c) = conflict {
            return Err(err(ErrorCode::Conflict, format!("할 일 목록이 그사이 바뀌어 넘기지 못했다: {}", c.reason)));
        }
        let added: Vec<String> = after
            .tasks
            .iter()
            .filter(|t| t.id.as_ref().is_some_and(|id| !before.contains(id)) && t.notes.first() == Some(&link))
            .filter_map(|t| t.id.clone())
            .collect();
        {
            let _w = WRITING.lock().unwrap_or_else(|e| e.into_inner());
            let mut meta = f.meta()?;
            meta.status = IdeaStatus::Done;
            meta.tasks.extend(added.iter().cloned());
            meta.updated_ms = aaa_common::util::now_ms();
            f.write_meta(&meta)?;
        }
        let ids = added
            .iter()
            .map(|t| self.db.task_short_id(project, t))
            .collect::<anyhow::Result<Vec<_>>>()
            .map_err(internal)?;
        self.db.audit(actor, "idea_handoff", &format!("{project}/{short} → {}", ids.join(", ")));
        self.ideas_bump(project);
        self.idea_note(project, &f, None)
    }

    pub(crate) fn idea_copy(&self, project: &str, idea: &str, actor: &str) -> Result<IdeaNote, ErrorBody> {
        let f = self.idea_folder(project, idea)?;
        let meta = f.meta()?;
        let text = f.text(None)?;
        let note = self.idea_create(project, &meta.title, &text, Some(f.id.clone()), actor)?;
        // 노트가 가리키는 그림·영상도 같은 이름으로 옮긴다(이름이 내용 hash라 겹치지 않는다)
        let to = self.ideas_root(project)?.join(&note.idea.id);
        for name in media_refs(&text) {
            let src = f.dir.join(&name);
            if src.symlink_metadata().is_ok_and(|m| m.is_file()) {
                std::fs::copy(&src, to.join(&name)).map_err(|e| internal(format!("{}: {e}", src.display())))?;
            }
        }
        Ok(note)
    }

    /// 웹이 올린 그림·영상을 아이디어 폴더에 쓰고 노트에 적을 이름을 돌려준다(이름은 내용 hash)
    pub(crate) fn idea_file_save(&self, project: &str, idea: &str, bytes: &[u8]) -> Result<String, ErrorBody> {
        let f = self.idea_folder(project, idea)?;
        if f.meta()?.status == IdeaStatus::Done {
            return Err(bad("할 일로 넘겨 잠긴 아이디어다"));
        }
        if bytes.is_empty() || bytes.len() as u64 > crate::task_attach::MAX_BYTES {
            return Err(bad("파일은 1바이트부터 16MiB까지 올린다"));
        }
        let ext = media_ext(bytes).ok_or_else(|| bad("그림(PNG·JPEG·GIF·WebP)·영상(MP4·WebM)만 올린다"))?;
        use sha2::Digest as _;
        let name = format!("{}.{ext}", &hex::encode(sha2::Sha256::digest(bytes))[..16]);
        let path = f.dir.join(&name);
        let _w = WRITING.lock().unwrap_or_else(|e| e.into_inner());
        match path.symlink_metadata() {
            Ok(m) if m.is_file() => {
                if std::fs::read(&path).map_err(internal)? != bytes {
                    return Err(bad(format!("{name}의 기존 내용이 올린 파일과 다르다. 기존 파일을 보존했다")));
                }
            }
            Ok(_) => return Err(internal(format!("{name}가 일반 파일이 아니다"))),
            Err(_) => write_atomic(&path, bytes).map_err(|e| internal(format!("{}: {e}", path.display())))?,
        }
        Ok(name)
    }

    /// 노트의 그림·영상(보기 권한도). 이름 꼴만 받고 폴더 밖·symlink는 읽지 않는다
    pub(crate) fn idea_file_read(&self, project: &str, idea: &str, name: &str) -> Result<Vec<u8>, ErrorBody> {
        if !media_name(name) {
            return Err(bad("그림·영상 이름 꼴이 아니다"));
        }
        let f = self.idea_folder(project, idea)?;
        let path = f.dir.join(name);
        match path.symlink_metadata() {
            Ok(m) if m.is_file() && m.len() <= crate::task_attach::MAX_BYTES => std::fs::read(&path).map_err(internal),
            Ok(_) => Err(bad("일반 파일이 아니거나 너무 크다")),
            Err(_) => Err(err(ErrorCode::NotFound, format!("{name}가 없다"))),
        }
    }
}

/// 할 일 메모 첫 줄의 아이디어 링크(웹은 앱 안 아이디어 화면으로 연다, thread 시작 때는 노트를 넘긴다)
pub(crate) fn idea_link(short: &str) -> String {
    format!("[아이디어 {short}](ideas/{short})")
}

/// 저장 결과(겹침 처리 전)
pub(crate) enum Saved {
    Same,
    Written,
    /// thread 판이 겹쳤다: 기준 판과 그 글
    Conflict(u32, String),
    /// 사람 판이 겹쳤다: 따로 둔 글과 지금 판
    Merge {
        base_rev: u32,
        mine: PathBuf,
        current: u32,
    },
}

/// 아이디어 폴더 안 그림·영상 이름(`<16 hex>.<png|jpg|gif|webp|mp4|webm>`)
pub(crate) fn media_name(name: &str) -> bool {
    let Some((stem, ext)) = name.split_once('.') else { return false };
    stem.len() == 16
        && stem.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        && matches!(ext, "png" | "jpg" | "gif" | "webp" | "mp4" | "webm")
}

pub(crate) fn media_mime(name: &str) -> &'static str {
    match name.rsplit('.').next() {
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        _ => crate::task_attach::mime(name),
    }
}

/// 파일 머리로 종류를 정한다(그림은 task 첨부와 같은 판정)
fn media_ext(bytes: &[u8]) -> Option<&'static str> {
    if let Some(ext) = crate::attach::sniff(bytes) {
        return Some(ext);
    }
    if bytes.len() > 12 && &bytes[4..8] == b"ftyp" {
        return Some("mp4");
    }
    if bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) {
        return Some("webm");
    }
    None
}

/// 노트가 가리키는 폴더 안 그림·영상(`](<이름>)`, 처음 나온 차례)
pub(crate) fn media_refs(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (at, _) in text.match_indices("](") {
        let rest = &text[at + 2..];
        let Some(end) = rest.find(')') else { continue };
        let target = rest[..end].trim();
        if media_name(target) && !out.iter().any(|n| n == target) {
            out.push(target.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_counts_lines_per_section() {
        let before = "# 아이디어\n\n## 목표\n- 하나\n\n## 결정\n- 가\n- 나\n";
        let after = "# 아이디어\n\n## 목표\n- 하나\n\n## 결정\n- 가\n- 다\n\n## 미정\n- 새 줄\n";
        assert_eq!(summarize(before, after), "「결정」 1줄 고침, 「미정」 1줄 더함");
        assert_eq!(summarize(before, before), "줄 바꿈만");
        assert_eq!(summarize(before, "# 새 이름\n\n## 목표\n- 하나\n"), "제목 바꿈, 「결정」 2줄 뺌");
    }

    fn host() -> (std::sync::Arc<Host>, PathBuf) {
        let state = std::env::temp_dir().join(format!("aaa-ideas-{}", aaa_common::util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.notify.web_push = false;
        cfg.projects.root = state.join("projects").display().to_string();
        let h = Host::open(cfg, state.clone()).unwrap();
        h.project_create("demo", Default::default(), "test").unwrap();
        (h, state)
    }

    /// 판 계약: 지금 판으로만 쓰고 판마다 글이 남는다. 겹치면 thread는 두 판을 받고, 사람 글은 버리지 않고 따로 둔다(결정 11).
    /// 사람이 고친 요약은 다음 말까지 쌓이고(결정 16), 짧은 ID는 n-0001부터 프로젝트마다 이어진다
    #[test]
    fn revisions_conflicts_and_user_edits() {
        let (h, state) = host();
        let note = h.idea_create("demo", "알림 묶기", "", None, "test").unwrap();
        assert_eq!((note.idea.short_id.as_str(), note.rev, note.text.as_str()), ("n-0001", 1, "# 알림 묶기\n"));
        let f = h.idea_folder("demo", "n-1").unwrap();
        let save = |base, text: &str, by| {
            h.idea_write("demo", &f, base, text, Some("thread 고침".into()), by, "test").unwrap()
        };
        let v2 = "# 알림 묶기\n\n## 결정\n- 하루 한 번\n";
        assert!(matches!(save(1, v2, IdeaAuthor::Thread), Ok(n) if n.rev == 2 && n.edited.is_none()));
        let v3 = "# 알림 묶기\n\n## 결정\n- 하루 한 번\n- 밤에는 묶기\n";
        let n3 = save(2, v3, IdeaAuthor::User).unwrap_or_else(|_| panic!("사람 저장"));
        assert_eq!(n3.edited.as_deref(), Some("판 3 「결정」 1줄 더함"));
        assert_eq!(n3.history[0].summary.as_deref(), Some("「결정」 1줄 더함"), "사람 판은 host가 센다");
        // thread가 옛 판(2)으로 저장: 쓰지 않고 기준 판 글을 준다
        match save(2, "# 알림 묶기\n", IdeaAuthor::Thread) {
            Err(Saved::Conflict(2, base)) => assert_eq!(base, v2),
            _ => panic!("thread 겹침"),
        }
        // 사람이 옛 판으로 저장: 글을 merge/에 두고 합치는 중 표지를 남긴다
        match save(2, "# 알림 묶기\n사람 글\n", IdeaAuthor::User) {
            Err(Saved::Merge { base_rev: 2, mine, current: 3 }) => {
                assert_eq!(std::fs::read_to_string(mine).unwrap(), "# 알림 묶기\n사람 글\n")
            }
            _ => panic!("사람 겹침"),
        }
        assert_eq!(h.idea_note("demo", &f, None).unwrap().merging, Some(2));
        assert_eq!(
            (h.idea_note("demo", &f, None).unwrap().text, h.idea_note("demo", &f, Some(2)).unwrap().text),
            (v3.to_string(), v2.to_string())
        );
        // thread가 합쳐 지금 판으로 저장하면 표지가 풀린다
        assert!(
            matches!(save(3, "# 알림 묶기\n합침\n", IdeaAuthor::Thread), Ok(n) if n.rev == 4 && n.merging.is_none())
        );
        assert_eq!(h.idea_create("demo", "둘째", "", None, "test").unwrap().idea.short_id, "n-0002");
        assert_eq!(h.idea_folder("demo", "n-0003").unwrap_err().code, ErrorCode::NotFound);
        let _ = std::fs::remove_dir_all(&state);
    }

    /// 넘기기 계약(결정 3·23·27): 초안마다 할 일이 생기고 메모 첫 줄이 아이디어 링크다. 넘긴 아이디어는 잠기고, 복사한 새 아이디어는
    /// 노트와 그림을 가지고 다시 다듬을 수 있다. 본 판이 지난 넘기기는 거부한다
    #[tokio::test]
    async fn handoff_locks_and_copy_reopens() {
        let (h, state) = host();
        let n = h.idea_create("demo", "알림 묶기", "", None, "test").unwrap();
        let f = h.idea_folder("demo", &n.idea.short_id).unwrap();
        let png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        let img = h.idea_file_save("demo", "n-0001", &png).unwrap();
        let text = format!("# 알림 묶기\n![그림]({img})\n");
        assert!(h.idea_write("demo", &f, 1, &text, None, IdeaAuthor::User, "test").unwrap().is_ok());
        let draft = |t: &str| IdeaTaskDraft { title: t.into(), repo: None, notes: "정리본대로 만든다".into() };
        assert_eq!(
            h.idea_handoff("demo", "n-0001", 1, vec![draft("x")], "test").await.unwrap_err().code,
            ErrorCode::Conflict
        );
        let done =
            h.idea_handoff("demo", "n-0001", 2, vec![draft("알림 묶기 구현"), draft("문서")], "test").await.unwrap();
        assert_eq!((done.idea.status, done.idea.tasks.len()), (IdeaStatus::Done, 2));
        let board = h.task_board("demo").unwrap();
        let t = board.tasks.iter().find(|t| t.title == "알림 묶기 구현").unwrap();
        assert_eq!(t.notes, ["[아이디어 n-0001](ideas/n-0001)", "정리본대로 만든다"]);
        assert!(h.idea_write("demo", &f, 2, "# 바꿈\n", None, IdeaAuthor::User, "test").is_err(), "잠긴 노트");
        let copy = h.idea_copy("demo", "n-0001", "test").unwrap();
        assert_eq!(
            (copy.idea.short_id.as_str(), copy.idea.status, copy.idea.copied_from.as_deref()),
            ("n-0002", IdeaStatus::Open, Some("n-0001"))
        );
        assert_eq!(h.idea_file_read("demo", "n-0002", &img).unwrap(), png, "그림도 옮긴다");
        // 할 일 thread 시작 때 넘길 것: 노트와 그 그림
        let mut task = "[아이디어 n-0001](ideas/n-0001)\n정리본대로".to_string();
        let (files, warnings) = h.task_attachments("demo", &mut task);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(
            files.iter().map(|f| f.src.clone()).collect::<Vec<_>>(),
            [format!("ideas/{}/note.md", f.id), format!("ideas/{}/{img}", f.id)]
        );
        assert!(task.starts_with(&format!("[아이디어 n-0001](attachments/{})", files[0].name)), "{task}");
        let _ = std::fs::remove_dir_all(&state);
    }

    #[test]
    fn ids_and_media_names() {
        assert!(valid_id("01JD0000000000000000000000"));
        for bad in ["01jd0000000000000000000000", "01JD00000000000000000000", "01JD000000000000000000000I", "../x"] {
            assert!(!valid_id(bad), "{bad}");
        }
        let a = "0123456789abcdef.png";
        let v = "fedcba9876543210.mp4";
        let text =
            format!("![그림]({a})\n![영상]({v})\n![다시]({a})\n![밖](../{a})\n![큰]({}.png)\n", "0123456789ABCDEF");
        assert_eq!(media_refs(&text), [a, v]);
        assert_eq!(media_ext(b"\x00\x00\x00\x18ftypisom\x00\x00"), Some("mp4"));
        assert_eq!(heading("소개\n#  \n# 제목 \n## 절"), Some("제목".into()));
    }
}

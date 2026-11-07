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
    ErrorBody, ErrorCode, FrontResponse, IdeaAuthor, IdeaConflict, IdeaMediaSource, IdeaMediaView, IdeaNote,
    IdeaRevView, IdeaStatus, IdeaSummary, IdeaTaskDraft, TaskOp,
};
use serde::{Deserialize, Serialize};

use crate::orch::{bad, internal, write_atomic};
use crate::{Host, err};

/// 노트 글 상한(TASKS.md와 같다)
pub(crate) const MAX_NOTE: usize = 512 * 1024;
/// 판 기록에 남기는 요약 한 줄 상한(바이트)
const MAX_SUMMARY: usize = 2000;
/// 한 번에 붙이는 그림·영상 수
pub(crate) const MAX_ATTACH: usize = 20;
/// 그림 설명(alt) 상한(글자)
const MAX_ALT: usize = 200;
/// 노트 쓰기는 한 번에 하나(사람·thread가 함께 저장해도 판 번호가 겹치지 않게)
pub(crate) static WRITING: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 프로세스 안 순서와 프로세스 사이 snapshot 경계를 함께 잡는다.
pub(crate) struct Writing {
    _local: std::sync::MutexGuard<'static, ()>,
    _file: crate::file_backup::FilesLock,
}
impl Host {
    pub(crate) fn idea_writing(&self) -> Result<Writing, ErrorBody> {
        let local = WRITING.lock().unwrap_or_else(|e| e.into_inner());
        let file = crate::file_backup::files_lock(&self.state, true).map_err(internal)?;
        Ok(Writing { _local: local, _file: file })
    }
}

/// `idea.json`
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Meta {
    pub title: String,
    pub status: IdeaStatus,
    pub rev: u32,
    pub seen_rev: u32,
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
    /// 그림·영상 이름 → 출처(t-1575). 처음 들인 기록만 둔다(같은 내용은 같은 이름이다)
    pub media: std::collections::BTreeMap<String, MediaMeta>,
    /// 사람이 고른 진행 모델(t-1581). 없으면 프로젝트 기본 thread profile. 전용 thread의 profile과 다르면 다음 말 때 새 thread로 바꾼다
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
}

/// 그림·영상 하나의 출처(`idea.json` `media`)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct MediaMeta {
    pub source: IdeaMediaSource,
    /// `사람`·`coordinator`·`t-NNNN`
    pub by: String,
    /// 원래 파일 이름이나 경로
    pub file: Option<String>,
    pub at_ms: u64,
}

impl Meta {
    /// 처음 들인 출처만 남긴다
    pub(crate) fn note_media(&mut self, name: &str, source: IdeaMediaSource, by: &str, file: Option<String>) {
        self.media.entry(name.to_string()).or_insert_with(|| MediaMeta {
            source,
            by: by.to_string(),
            file,
            at_ms: aaa_common::util::now_ms(),
        });
    }
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
        if by == IdeaAuthor::User {
            meta.seen_rev = rev;
        }
        meta.updated_ms = now;
        if let Some(t) = heading(text) {
            meta.title = t;
        }
        meta.history.push(IdeaRevView { rev, at_ms: now, by, summary });
        self.write_meta(meta)
    }
}

impl Host {
    fn idea_updated(&self, note: &IdeaNote) {
        use aaa_proto::{InboxItem, NoticeKind, NoticeView};
        self.send_notice(NoticeView {
            kind: NoticeKind::IdeaUpdated,
            idea: Some(note.idea.short_id.clone()),
            pane: None,
            machine: String::new(),
            project: Some(note.project.clone()),
            thread: note.idea.thread.clone(),
            title: note.idea.title.clone(),
            agent: None,
            turn_ms: None,
            long: true,
            at_ms: note.idea.updated_ms,
        });
        if !self.settings(&note.project).mute
            && let Some(tx) = self.notify_tx.lock().unwrap().as_ref()
        {
            let item = InboxItem {
                id: format!("idea:{}:{}", note.idea.id, note.rev),
                short_id: None,
                project: note.project.clone(),
                item_kind: "idea".into(),
                subject: note.idea.short_id.clone(),
                event: "updated".into(),
                summary: note.idea.title.clone(),
                body: String::new(),
                done: false,
                seen: false,
                created_at_ms: note.idea.updated_ms,
                done_at_ms: None,
            };
            aaa_common::diagnostic::ignored(tx.send(item), "알림 수신자가 끝났다");
        }
    }

    pub(crate) fn idea_still_news(&self, item: &aaa_proto::InboxItem) -> bool {
        self.idea_folder(&item.project, &item.subject)
            .and_then(|f| {
                let m = f.meta()?;
                Ok(m.status == IdeaStatus::Open && m.rev > m.seen_rev && item.id == format!("idea:{}:{}", f.id, m.rev))
            })
            .inspect_err(|e| tracing::warn!(project = %item.project, idea = %item.subject, "아이디어 알림 재확인 실패: {}", e.message))
            .unwrap_or(false)
    }

    /// 화면이 실제 표시한 판만 확인한다. 그 사이 저장한 새 판은 읽지 않은 채 남는다.
    pub(crate) fn idea_seen(&self, project: &str, idea: &str, rev: u32) -> Result<(), ErrorBody> {
        let f = self.idea_folder(project, idea)?;
        let changed = {
            let _w = self.idea_writing()?;
            let mut m = f.meta()?;
            if rev == 0 || rev > m.rev {
                return Err(bad("읽은 판이 없다"));
            }
            if rev <= m.seen_rev {
                false
            } else {
                m.seen_rev = rev;
                f.write_meta(&m)?;
                true
            }
        };
        if changed {
            self.ideas_bump(project);
        }
        Ok(())
    }

    /// 기존 노트의 library 링크도 파일 사본으로 바꾼다. 주소만 이행하므로 판·시각·알림은 바꾸지 않는다.
    pub(crate) async fn idea_read_note(
        &self,
        project: &str,
        f: &Folder,
        rev: Option<u32>,
    ) -> Result<IdeaNote, ErrorBody> {
        let mut note = self.idea_note(project, f, rev)?;
        let text = match self.idea_import_media(project, f, &note.text, "").await {
            Ok(text) => text,
            Err(e) => {
                tracing::warn!(project, idea = %f.id, "아이디어 미디어 이행 실패: {}", e.message);
                note.media_error = Some(e.message);
                return Ok(note);
            }
        };
        if text != note.text {
            let _w = self.idea_writing()?;
            if f.text(Some(note.rev))? == note.text {
                write_atomic(&f.rev_path(note.rev), text.as_bytes()).map_err(internal)?;
            }
            if f.meta()?.rev == note.rev && f.text(None)? == note.text {
                write_atomic(&f.note_path(), text.as_bytes()).map_err(internal)?;
            }
        }
        self.idea_note(project, f, rev)
    }

    /// 노트가 가리키는 바깥 미디어를 아이디어 소유 파일로 보존하고 링크를 폴더 안 이름으로 바꾼다: 전용 thread `library/`는 agent를
    /// 통해 읽고, 프로젝트 `uploads/`(대화·할 일 첨부)는 host에서 복사한다. `by`는 출처 기록에 남길 들인 쪽
    pub(crate) async fn idea_import_media(
        &self,
        project: &str,
        f: &Folder,
        text: &str,
        by: &str,
    ) -> Result<String, ErrorBody> {
        use aaa_proto::{AgentRequest, AgentResponse};
        check_text(text)?;
        let targets = media_targets(text);
        let upload = |src: &str| {
            src.strip_prefix("uploads/")
                .filter(|n| crate::task_attach::valid_name(n) && !n.ends_with(".md") && !n.ends_with(".html"))
                .map(str::to_string)
        };
        if !targets.iter().any(|(_, src)| src.contains("library/") || upload(src).is_some()) {
            return Ok(text.to_string());
        }
        let thread = f.meta()?.thread;
        // 전용 thread 기록이 없으면 library 링크는 그대로 둔다(uploads는 thread 없이 옮긴다)
        let rec = match thread.as_deref().map(|t| self.load(project, t)).transpose() {
            Ok(rec) => rec,
            Err(e) => {
                tracing::warn!(project, idea = %f.id, "아이디어 thread 기록을 읽지 못해 library 링크는 두고 옮긴다: {}", e.message);
                None
            }
        };
        let work = rec.as_ref().and_then(|r| r.work_dir.clone());
        let mut imported: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        let mut sources: Vec<(String, IdeaMediaSource, String)> = Vec::new();
        let mut out = text.to_string();
        for (range, src) in targets.into_iter().rev() {
            let (key, source) = if let Some(name) = upload(&src) {
                (format!("uploads/{name}"), IdeaMediaSource::Uploads)
            } else if let Some(path) = work.as_deref().and_then(|w| thread_media_path(&src, w)) {
                (path, IdeaMediaSource::Library)
            } else {
                continue;
            };
            let name = if let Some(name) = imported.get(&key) {
                name.clone()
            } else {
                let bytes = if source == IdeaMediaSource::Uploads {
                    let name = &key["uploads/".len()..];
                    self.task_file_read(project, name).map_err(|e| err(e.code, format!("{key}: {}", e.message)))?
                } else {
                    let (Some(rec), Some(work)) = (rec.as_ref(), work.as_deref()) else { continue };
                    let conn = self.live_conn(&rec.machine_id)?;
                    let mut bytes = Vec::new();
                    loop {
                        let response = conn
                            .request(AgentRequest::FileRead {
                                work_dir: work.to_string(),
                                path: key["library/".len()..].to_string(),
                                offset: bytes.len() as u64,
                                len: 256 * 1024,
                            })
                            .await
                            .map_err(|e| err(e.code, format!("{key}: {}", e.message)))?;
                        let AgentResponse::FileChunk { data, eof } = response else {
                            return Err(internal("아이디어 미디어 읽기 응답이 아니다"));
                        };
                        if bytes.len() as u64 + data.0.len() as u64 > crate::task_attach::MAX_BYTES {
                            return Err(bad(format!("{key}: 미디어가 16MiB를 넘는다")));
                        }
                        if data.0.is_empty() && !eof {
                            return Err(internal(format!("{key}: 미디어 읽기가 진행되지 않는다")));
                        }
                        bytes.extend(data.0);
                        if eof {
                            break;
                        }
                    }
                    bytes
                };
                let name = self.idea_store_file(f, &bytes).map_err(|e| internal(format!("{key}: {}", e.message)))?;
                let who = if source == IdeaMediaSource::Library {
                    thread.clone().unwrap_or_default()
                } else {
                    by.to_string()
                };
                sources.push((name.clone(), source, who));
                imported.insert(key.clone(), name.clone());
                name
            };
            out.replace_range(range, &name);
        }
        if !sources.is_empty() {
            let _w = self.idea_writing()?;
            let mut meta = f.meta()?;
            for (name, source, who) in &sources {
                let file = imported.iter().find(|(_, n)| *n == name).map(|(k, _)| k.clone());
                meta.note_media(name, *source, who, file);
            }
            f.write_meta(&meta)?;
        }
        Ok(out)
    }

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

    /// 다음 말을 맡을 모델: 고른 것, 고르지 않았으면 이어 받을 전용 thread의 profile(끝났으면 프로젝트 기본). 고르지 않은 아이디어는
    /// 기본이 바뀌어도 열린 thread를 잇기 때문이다(`idea_thread::model_switch`)
    fn idea_profile(&self, project: &str, m: &Meta) -> String {
        if let Some(p) = &m.profile {
            return p.clone();
        }
        m.thread
            .as_deref()
            .and_then(|t| self.load(project, t).ok())
            .filter(|r| !matches!(r.status, aaa_proto::ThreadStatus::Resolved | aaa_proto::ThreadStatus::Failed))
            .map(|r| r.profile)
            .unwrap_or_else(|| self.default_thread_profile(project))
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
            unseen: m.history.iter().any(|r| r.by == IdeaAuthor::Thread && r.rev > m.seen_rev),
            created_ms: m.created_ms,
            updated_ms: m.updated_ms,
            tasks,
            thread: m.thread.clone(),
            copied_from,
            profile: Some(self.idea_profile(project, m)),
        })
    }

    pub(crate) fn idea_note(&self, project: &str, f: &Folder, rev: Option<u32>) -> Result<IdeaNote, ErrorBody> {
        let m = f.meta()?;
        let rev = rev.unwrap_or(m.rev);
        if rev == 0 || rev > m.rev {
            return Err(err(ErrorCode::NotFound, format!("{rev} 판이 없다(지금 {})", m.rev)));
        }
        let text = f.text((rev != m.rev).then_some(rev))?;
        let size =
            |name: &str| f.dir.join(name).symlink_metadata().map_or(0, |m| if m.is_file() { m.len() } else { 0 });
        let media = media_refs(&text)
            .into_iter()
            .map(|name| {
                let info = m.media.get(&name).cloned().unwrap_or_default();
                IdeaMediaView {
                    bytes: size(&name),
                    source: info.source,
                    by: info.by,
                    file: info.file,
                    at_ms: info.at_ms,
                    name,
                }
            })
            .collect();
        let media_bytes = std::fs::read_dir(&f.dir)
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .filter(|e| e.file_name().to_str().is_some_and(media_name))
                    .filter_map(|e| e.metadata().ok().filter(std::fs::Metadata::is_file).map(|m| m.len()))
                    .sum()
            })
            .unwrap_or(0);
        Ok(IdeaNote {
            media,
            media_bytes,
            media_error: None,
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
        let short;
        {
            let _w = self.idea_writing()?;
            std::fs::create_dir_all(&f.dir).map_err(|e| internal(format!("{}: {e}", f.dir.display())))?;
            // 복사 노트는 그림이 모두 준비된 뒤 처음 공개한다.
            if let Some(source) = &meta.copied_from {
                let from = Folder { id: source.clone(), dir: self.ideas_root(project)?.join(source) };
                let old = from.meta()?;
                let old_short = self.db.idea_short_id(project, source).map_err(internal)?;
                for name in media_refs(&text) {
                    let bytes = self.idea_file_read(project, source, &name)?;
                    write_atomic(&f.dir.join(&name), &bytes).map_err(internal)?;
                    let mut info = old.media.get(&name).cloned().unwrap_or_default();
                    info.source = IdeaMediaSource::Copy;
                    info.file = Some(match info.file {
                        Some(file) => format!("{old_short} {file}"),
                        None => old_short.clone(),
                    });
                    meta.media.insert(name, info);
                }
            }
            f.write_rev(&mut meta, &text, IdeaAuthor::User, Some("만듦".into()), now)?;
            short = self.db.idea_short_id(project, &f.id).map_err(internal)?;
        }
        // snapshot에는 파일과 번호 원장이 함께 잡힌다.
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
            let _w = self.idea_writing()?;
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
                let note = self.idea_note(project, f, None)?;
                if by == IdeaAuthor::Thread && note.idea.unseen {
                    self.idea_updated(&note);
                }
                Ok(Ok(note))
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
        // 노트의 그림은 할 일 첨부(`uploads/`, 이름이 같은 내용 hash)로도 둔다: 할 일 화면이 썸네일로 보이고 thread 시작 때
        // `attachments/`로 넘어간다. 영상은 할 일 첨부 형식이 아니라 노트와 함께만 넘어간다
        let images = self.idea_handoff_images(project, &f)?;
        let mut ops = Vec::new();
        for d in &drafts {
            let title = one_line("할 일 제목", &d.title, 400)?;
            let mut notes: Vec<String> = vec![link.clone()];
            notes.extend(d.notes.lines().map(|l| l.trim_end().to_string()).filter(|l| l.trim() != link));
            while notes.last().is_some_and(|l| l.trim().is_empty()) {
                notes.pop();
            }
            for line in &images {
                if !notes.iter().any(|l| l.contains(&line[line.find("](").unwrap_or(0)..])) {
                    notes.push(line.clone());
                }
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
            let _w = self.idea_writing()?;
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

    /// 넘길 때 할 일 메모에 더할 그림 줄(`![설명](uploads/<이름>)`): 노트의 그림(PNG·JPEG·GIF·WebP)을 프로젝트 `uploads/`에 복사한다
    fn idea_handoff_images(&self, project: &str, f: &Folder) -> Result<Vec<String>, ErrorBody> {
        let text = f.text(None)?;
        let dir = self.project_dir(project)?.join("uploads");
        let mut out = Vec::new();
        for (alt, name) in media_images(&text) {
            if !crate::task_attach::valid_name(&name) {
                continue;
            }
            let bytes = match self.idea_file_read(project, &f.id, &name) {
                Ok(b) => b,
                Err(e) => {
                    tracing::warn!(project, idea = %f.id, name, "넘길 그림을 읽지 못해 노트에만 둔다: {}", e.message);
                    continue;
                }
            };
            self.upload_store(&dir, &name, &bytes).map_err(|e| err(e.code, format!("{name}: {}", e.message)))?;
            out.push(format!("![{}](uploads/{name})", media_alt(Some(&alt), &name)));
        }
        Ok(out)
    }

    pub(crate) fn idea_copy(&self, project: &str, idea: &str, actor: &str) -> Result<IdeaNote, ErrorBody> {
        let f = self.idea_folder(project, idea)?;
        let meta = f.meta()?;
        let text = f.text(None)?;
        self.idea_create(project, &meta.title, &text, Some(f.id.clone()), actor)
    }

    /// 웹이 올린 그림·영상을 아이디어 폴더에 쓰고 노트에 적을 이름을 돌려준다(이름은 내용 hash)
    /// 웹이 올린 그림·영상(`file`은 브라우저가 알린 원래 이름)
    pub(crate) fn idea_file_save(
        &self,
        project: &str,
        idea: &str,
        bytes: &[u8],
        file: Option<String>,
    ) -> Result<String, ErrorBody> {
        let f = self.idea_folder(project, idea)?;
        if f.meta()?.status == IdeaStatus::Done {
            return Err(bad("할 일로 넘겨 잠긴 아이디어다"));
        }
        let name = self.idea_store_file(&f, bytes)?;
        let file =
            file.map(|n| n.chars().filter(|c| !c.is_control()).take(200).collect::<String>()).filter(|n| !n.is_empty());
        let _w = self.idea_writing()?;
        let mut meta = f.meta()?;
        meta.note_media(&name, IdeaMediaSource::Web, "사람", file);
        f.write_meta(&meta)?;
        Ok(name)
    }

    /// 그림·영상을 폴더에 넣고 노트 끝(또는 `section` 절 끝)에 한 줄씩 더해 새 판으로 쓴다(`store_only`면 폴더에만).
    /// `names`는 이미 올린 폴더 안 이름, `paths`는 host 파일. 경로 권한(`roots`: None이면 일반 파일 어디나)은 부르는 쪽이 정한다
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn idea_attach(
        &self,
        project: &str,
        f: &Folder,
        by: IdeaAuthor,
        by_label: &str,
        actor: &str,
        names: &[String],
        paths: &[String],
        roots: Option<&[PathBuf]>,
        alt: Option<&str>,
        section: Option<&str>,
        store_only: bool,
    ) -> Result<(Vec<String>, IdeaNote), ErrorBody> {
        let short = self.db.idea_short_id(project, &f.id).map_err(internal)?;
        let count = names.len() + paths.len();
        if count == 0 {
            return Err(bad("붙일 그림·영상이 없다"));
        }
        if count > MAX_ATTACH {
            return Err(bad(format!("한 번에 {MAX_ATTACH}개까지 붙인다")));
        }
        if f.meta()?.status == IdeaStatus::Done {
            return Err(bad(format!("{short}는 할 일로 넘겨 잠겼다. 다시 다듬으려면 복사해서 새 아이디어로 한다")));
        }
        let mut added: Vec<(String, IdeaMediaSource, Option<String>)> = Vec::new();
        for name in names {
            let name = name.trim();
            if !media_name(name) || !f.dir.join(name).symlink_metadata().is_ok_and(|m| m.is_file()) {
                return Err(bad(format!("{name}: 이 아이디어에 올린 그림·영상 이름이 아니다")));
            }
            added.push((name.to_string(), IdeaMediaSource::Web, None));
        }
        for path in paths {
            let bytes = read_attach_path(path, roots)?;
            let name = self.idea_store_file(f, &bytes).map_err(|e| err(e.code, format!("{path}: {}", e.message)))?;
            added.push((name, IdeaMediaSource::File, Some(path.clone())));
        }
        let lines: Vec<String> =
            added.iter().map(|(name, _, _)| format!("![{}]({name})", media_alt(alt, name))).collect();
        let n = added.len();
        let written = {
            let _w = self.idea_writing()?;
            let mut meta = f.meta()?;
            if meta.status == IdeaStatus::Done {
                return Err(bad(format!("{short}는 할 일로 넘겨 잠겼다")));
            }
            for (name, source, file) in &added {
                meta.note_media(name, *source, by_label, file.clone());
            }
            if store_only {
                f.write_meta(&meta)?;
                false
            } else {
                let text = place_media(&f.text(None)?, &lines, section)?;
                let what = if n == 1 { "그림 1개 더함".to_string() } else { format!("그림 {n}개 더함") };
                let summary = match section.map(str::trim).filter(|s| !s.is_empty()) {
                    Some(sec) => format!("「{}」 {what}", sec.trim_start_matches('#').trim()),
                    None => what,
                };
                if by == IdeaAuthor::User {
                    meta.edited.push(format!("판 {} {summary}", meta.rev + 1));
                }
                f.write_rev(&mut meta, &text, by, Some(summary), aaa_common::util::now_ms())?;
                true
            }
        };
        let out: Vec<String> = added.into_iter().map(|(name, _, _)| name).collect();
        self.db.audit(actor, "idea_attach", &format!("{project}/{short} {}", out.join(" ")));
        self.ideas_bump(project);
        let note = self.idea_note(project, f, None)?;
        if written && by == IdeaAuthor::Thread && note.idea.unseen {
            self.idea_updated(&note);
        }
        Ok((out, note))
    }

    fn idea_store_file(&self, f: &Folder, bytes: &[u8]) -> Result<String, ErrorBody> {
        if bytes.is_empty() || bytes.len() as u64 > crate::task_attach::MAX_BYTES {
            return Err(bad("파일은 1바이트부터 16MiB까지 올린다"));
        }
        let ext = media_ext(bytes).ok_or_else(|| bad("그림(PNG·JPEG·GIF·WebP)·영상(MP4·WebM)만 올린다"))?;
        use sha2::Digest as _;
        let name = format!("{}.{ext}", &hex::encode(sha2::Sha256::digest(bytes))[..16]);
        let path = f.dir.join(&name);
        let _w = self.idea_writing()?;
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

/// `idea attach`의 host 파일을 읽는다: 절대 경로의 일반 파일, 16MiB 이하. `roots`가 있으면 실제 경로(링크를 푼 것)가 그 안이어야 한다
fn read_attach_path(path: &str, roots: Option<&[PathBuf]>) -> Result<Vec<u8>, ErrorBody> {
    let p = std::path::Path::new(path);
    if !p.is_absolute() {
        return Err(bad(format!("{path}: 절대 경로로 준다")));
    }
    if let Some(roots) = roots {
        let real = p.canonicalize().map_err(|e| bad(format!("{path}: {e}")))?;
        let inside = roots.iter().filter_map(|r| r.canonicalize().ok()).any(|r| real.starts_with(&r));
        if !inside {
            return Err(err(
                ErrorCode::Forbidden,
                format!("{path}: thread는 자기 작업 폴더와 프로젝트 uploads/ 안의 파일만 붙인다"),
            ));
        }
    }
    let m = std::fs::metadata(p).map_err(|e| bad(format!("{path}: {e}")))?;
    if !m.is_file() {
        return Err(bad(format!("{path}: 일반 파일이 아니다")));
    }
    if m.len() == 0 || m.len() > crate::task_attach::MAX_BYTES {
        return Err(bad(format!("{path}: 파일은 1바이트부터 16MiB까지 붙인다({}바이트)", m.len())));
    }
    std::fs::read(p).map_err(|e| internal(format!("{path}: {e}")))
}

/// 아이디어 폴더 안 그림·영상 이름(`<16 hex>.<png|jpg|gif|webp|mp4|webm>`)
pub(crate) fn media_name(name: &str) -> bool {
    let Some((stem, ext)) = name.split_once('.') else { return false };
    stem.len() == 16
        && stem.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        && matches!(ext, "png" | "jpg" | "gif" | "webp" | "mp4" | "webm")
}

/// 코드 울타리(```·~~~) 밖의 줄과 그 시작 바이트 위치(줄 끝 개행 포함)
fn prose_lines(text: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut fence: Option<&str> = None;
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            let mark = &trimmed[..3];
            if fence == Some(mark) {
                fence = None;
            } else if fence.is_none() {
                fence = Some(mark);
            }
        } else if fence.is_none() {
            out.push((offset, line));
        }
        offset += line.len();
    }
    out
}

/// 줄의 이 위치가 `` ` `` 안인가(앞의 backtick 수가 홀수)
fn in_code(line: &str, at: usize) -> bool {
    line[..at].matches('`').count() % 2 == 1
}

/// Markdown 링크·그림의 대상 범위. 코드 안의 예시는 파일 요청으로 바꾸지 않는다.
pub(crate) fn media_targets(text: &str) -> Vec<(std::ops::Range<usize>, String)> {
    static TARGET: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r#"\]\(\s*(?:<([^>\n]+)>|([^\s)]+))(?:\s+["'][^\n]*?["'])?\s*\)"#).unwrap()
    });
    let mut out = Vec::new();
    for (offset, line) in prose_lines(text) {
        for c in TARGET.captures_iter(line) {
            let m = c.get(1).or_else(|| c.get(2)).unwrap();
            if in_code(line, c.get(0).unwrap().start()) {
                continue;
            }
            out.push((offset + m.start()..offset + m.end(), m.as_str().to_string()));
        }
    }
    out
}

/// 노트의 그림 `![설명](<폴더 안 이름>)`들(설명, 이름). 처음 나온 차례, 같은 이름은 처음 것만
pub(crate) fn media_images(text: &str) -> Vec<(String, String)> {
    static IMAGE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(
            r#"!\[([^\]\n]*)\]\(\s*<?([0-9a-f]{16}\.(?:png|jpg|gif|webp|mp4|webm))>?(?:\s+["'][^\n]*?["'])?\s*\)"#,
        )
        .unwrap()
    });
    let mut out: Vec<(String, String)> = Vec::new();
    for (_, line) in prose_lines(text) {
        for c in IMAGE.captures_iter(line) {
            if in_code(line, c.get(0).unwrap().start()) {
                continue;
            }
            let name = c[2].to_string();
            if !out.iter().any(|(_, n)| *n == name) {
                out.push((c[1].trim().to_string(), name));
            }
        }
    }
    out
}

/// 붙일 그림 줄의 설명: 한 줄로, `[`·`]`·제어 문자는 줄 꼴을 깨므로 빈칸으로. 비면 `그림`·`영상`
pub(crate) fn media_alt(alt: Option<&str>, name: &str) -> String {
    let cleaned: String =
        alt.unwrap_or("").chars().map(|c| if c == '[' || c == ']' || c.is_control() { ' ' } else { c }).collect();
    let words = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let alt: String = words.chars().take(MAX_ALT).collect();
    if !alt.is_empty() {
        alt
    } else if name.ends_with(".mp4") || name.ends_with(".webm") {
        "영상".into()
    } else {
        "그림".into()
    }
}

/// `#` 머리 줄의 (단계, 제목)
fn heading_of(line: &str) -> Option<(usize, String)> {
    let t = line.trim_end();
    let level = t.bytes().take_while(|&b| b == b'#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let rest = &t[level..];
    if !rest.is_empty() && !rest.starts_with(' ') {
        return None;
    }
    Some((level, rest.trim().trim_end_matches('#').trim().to_string()))
}

/// 그림 줄들을 노트 끝, 또는 `section` 절(그 머리 아래부터 같은·높은 단계 다음 머리 전까지) 끝에 넣는다. 줄마다 앞에 빈 줄을 두어
/// 문단에 붙지 않고 혼자 선 그림이 된다
pub(crate) fn place_media(text: &str, lines: &[String], section: Option<&str>) -> Result<String, ErrorBody> {
    let all: Vec<&str> = text.lines().collect();
    let (start, end) = match section.map(|s| s.trim().trim_start_matches('#').trim()).filter(|s| !s.is_empty()) {
        None => (0, all.len()),
        Some(want) => {
            let prose: std::collections::HashSet<usize> = {
                let mut starts = std::collections::HashSet::new();
                let mut index = std::collections::HashMap::new();
                let mut at = 0;
                for (i, l) in text.split_inclusive('\n').enumerate() {
                    index.insert(at, i);
                    at += l.len();
                }
                for (offset, _) in prose_lines(text) {
                    if let Some(i) = index.get(&offset) {
                        starts.insert(*i);
                    }
                }
                starts
            };
            let found = all
                .iter()
                .enumerate()
                .filter(|(i, _)| prose.contains(i))
                .find_map(|(i, l)| heading_of(l).filter(|(_, t)| t == want).map(|(level, _)| (i, level)));
            let Some((at, level)) = found else {
                return Err(bad(format!("노트에 「{want}」 절이 없다")));
            };
            let end = (at + 1..all.len())
                .find(|i| prose.contains(i) && heading_of(all[*i]).is_some_and(|(l, _)| l <= level))
                .unwrap_or(all.len());
            (at + 1, end)
        }
    };
    let mut at = end;
    while at > start && all[at - 1].trim().is_empty() {
        at -= 1;
    }
    let mut out: Vec<String> = all[..at].iter().map(|l| l.to_string()).collect();
    for l in lines {
        if !out.is_empty() {
            out.push(String::new());
        }
        out.push(l.clone());
    }
    if end < all.len() {
        out.push(String::new());
        out.extend(all[end..].iter().map(|l| l.to_string()));
    }
    let mut text = out.join("\n");
    text.push('\n');
    check_text(&text)?;
    Ok(text)
}

fn thread_media_path(src: &str, work: &str) -> Option<String> {
    let src = src.strip_prefix("file://").unwrap_or(src);
    let relative = src.strip_prefix(work).and_then(|s| s.strip_prefix('/')).unwrap_or(src);
    let relative = relative.strip_prefix("./").unwrap_or(relative);
    let relative = if let Some(name) = std::path::Path::new(work).file_name().and_then(|s| s.to_str()) {
        relative.strip_prefix(&format!(".aaa/{name}/")).unwrap_or(relative)
    } else {
        relative
    };
    if !relative.starts_with("library/") || !crate::library::safe_rel(relative) {
        return None;
    }
    let ext = relative.rsplit('.').next()?.to_ascii_lowercase();
    matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp" | "mp4" | "webm").then(|| relative.to_string())
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
    for (_, target) in media_targets(text) {
        if media_name(&target) && !out.contains(&target) {
            out.push(target);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use aaa_proto::DeviceRole;

    /// 재접속에도 새 판이 남고, 옛 판 읽음·중복 저장·사람 저장이 새 알림을 만들지 않는다.
    #[test]
    fn thread_updates_stay_unseen_until_the_displayed_revision_is_seen() {
        let (h, state) = host();
        let mut notices = h.notice_events.subscribe();
        let (tx, mut notifications) = tokio::sync::mpsc::unbounded_channel();
        *h.notify_tx.lock().unwrap() = Some(tx);
        let n = h.idea_create("demo", "아이디어", "", None, "test").unwrap();
        let f = h.idea_folder("demo", &n.idea.id).unwrap();
        assert!(!n.idea.unseen);
        let save = |base, text, by| {
            h.idea_write("demo", &f, base, text, None, by, "test").unwrap().unwrap_or_else(|_| panic!("저장"))
        };
        let n = save(1, "# 아이디어\n둘째\n", IdeaAuthor::Thread);
        assert!(n.idea.unseen);
        assert_eq!(notices.try_recv().unwrap().idea.as_deref(), Some("n-0001"));
        let item = notifications.try_recv().unwrap();
        assert!(h.idea_still_news(&item));
        let alert = crate::push::payload(
            &crate::notify::Place::Project("demo".into()),
            std::slice::from_ref(&item),
            &Default::default(),
            None,
        );
        assert_eq!(alert["url"], "/#idea=n-0001");
        assert_eq!(
            crate::push::for_device(crate::db::Alerts { input: true, done: false }, std::slice::from_ref(&item)).len(),
            0
        );
        h.idea_seen("demo", "n-0001", 1).unwrap();
        assert!(h.idea_list("demo").unwrap()[0].unseen);
        assert_eq!(save(2, "# 아이디어\n둘째\n", IdeaAuthor::Thread).rev, 2);
        assert!(notices.try_recv().is_err());
        save(2, "# 아이디어\n셋째\n", IdeaAuthor::Thread);
        h.idea_seen("demo", "n-0001", 2).unwrap();
        assert!(h.idea_list("demo").unwrap()[0].unseen, "화면에 없던 셋째 판은 남긴다");
        assert!(!h.idea_still_news(&item), "묶음 전 옛 알림은 보내지 않는다");
        assert_eq!(f.meta().unwrap().seen_rev, 2, "host 메타에 보존한다");
        assert!(h.idea_seen("demo", "n-0001", 4).is_err());
        h.idea_seen("demo", "n-0001", 3).unwrap();
        assert!(!h.idea_list("demo").unwrap()[0].unseen);
        assert!(!save(3, "# 아이디어\n사람\n", IdeaAuthor::User).idea.unseen);
        let _ = std::fs::remove_dir_all(state);
    }

    /// 임의 파일을 읽지 않고 자기 library만 가져온다. 코드의 예시도 읽기 요청이 되지 않는다.
    #[test]
    fn media_import_is_scoped_to_the_threads_library() {
        let work = "/work/demo/.aaa/demo-t-0001";
        for src in [
            "library/pic.png",
            "./library/pic.png",
            "/work/demo/.aaa/demo-t-0001/library/pic.png",
            ".aaa/demo-t-0001/library/pic.png",
        ] {
            assert_eq!(thread_media_path(src, work).as_deref(), Some("library/pic.png"), "{src}");
        }
        for src in [
            "library/../secret.png",
            "library/.hidden.png",
            "../library/pic.png",
            "/other/library/pic.png",
            "https://example.com/library/pic.png",
            "library/a.html",
            "library/a.svg",
        ] {
            assert!(thread_media_path(src, work).is_none(), "{src}");
        }
        let text = "![그림](library/pic.png)\n[링크](<library/a b.png>)\n`![예](library/code.png)`\n```md\n![예](library/fenced.png)\n```\n";
        assert_eq!(
            media_targets(text).iter().map(|(_, s)| s.as_str()).collect::<Vec<_>>(),
            ["library/pic.png", "library/a b.png"]
        );
    }

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
        let img = h.idea_file_save("demo", "n-0001", &png, None).unwrap();
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
        // 노트의 그림은 할 일 첨부 줄(uploads/, 같은 내용 hash 이름)로도 붙는다(t-1575)
        assert_eq!(
            t.notes,
            ["[아이디어 n-0001](ideas/n-0001)", "정리본대로 만든다", &format!("![그림](uploads/{img})")]
        );
        assert_eq!(h.task_file_read("demo", &img).unwrap(), png, "할 일 첨부로 보이는 사본");
        // 실제 메모로 thread를 시작하면: 노트·그림이 attachments/로 한 번씩 가고 메모의 그림 줄도 그 사본을 가리킨다
        let mut memo = t.notes.join("\n");
        let (files, warnings) = h.task_attachments("demo", &mut memo);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(files.iter().map(|f| f.name.as_str()).filter(|n| *n == img).count(), 1, "같은 그림은 한 번");
        assert!(memo.ends_with(&format!("![그림](attachments/{img})")), "{memo}");
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

    /// 붙이기 자리(t-1575): 노트 끝이나 고른 절 끝에 빈 줄을 두고 한 줄씩. 코드 안의 머리는 절로 보지 않고, 없는 절은 거부한다
    #[test]
    fn media_lines_go_to_the_end_or_the_section() {
        let lines = ["![가](a)".to_string(), "![나](b)".to_string()];
        assert_eq!(place_media("# 제목\n본문\n\n\n", &lines, None).unwrap(), "# 제목\n본문\n\n![가](a)\n\n![나](b)\n");
        let text = "# 제목\n\n## 시안\n- 첫\n\n### 작은 절\n- 안\n\n## 결정\n- 끝\n";
        assert_eq!(
            place_media(text, &lines[..1], Some("## 시안")).unwrap(),
            "# 제목\n\n## 시안\n- 첫\n\n### 작은 절\n- 안\n\n![가](a)\n\n## 결정\n- 끝\n"
        );
        let fenced = "# 제목\n```md\n## 결정\n```\n";
        assert_eq!(place_media(fenced, &lines[..1], Some("결정")).unwrap_err().code, ErrorCode::BadRequest);
        assert_eq!(media_alt(Some(" 간판 [시안]\n둘 "), "a.png"), "간판 시안 둘");
        assert_eq!((media_alt(None, "a.mp4"), media_alt(Some("  "), "a.png")), ("영상".into(), "그림".into()));
        assert_eq!(
            media_images("![가](0123456789abcdef.png)\n`![코드](fedcba9876543210.png)`\n![다시](0123456789abcdef.png)"),
            [("가".to_string(), "0123456789abcdef.png".to_string())]
        );
    }

    /// `idea attach` 계약(t-1575): host 파일·올린 이름을 붙여 새 판 하나로 쓰고 출처를 남긴다. 제한한 폴더 밖·그림 아닌 파일·
    /// 올리지 않은 이름·잠긴 아이디어는 거부하고, `store_only`는 노트를 바꾸지 않는다. 노트의 `uploads/` 링크는 저장 때 옮긴다
    #[tokio::test]
    async fn attach_adds_one_revision_and_guards_sources() {
        let (h, state) = host();
        let n = h.idea_create("demo", "간판", "# 간판\n\n## 시안\n- 첫\n\n## 결정\n", None, "test").unwrap();
        let f = h.idea_folder("demo", &n.idea.short_id).unwrap();
        let png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        let mp4 = b"\x00\x00\x00\x18ftypisom\x00\x00".to_vec();
        let work = state.join("work");
        std::fs::create_dir_all(work.join("library")).unwrap();
        let (pic, clip, text) = (work.join("library/a.png"), work.join("b.mp4"), work.join("c.txt"));
        std::fs::write(&pic, &png).unwrap();
        std::fs::write(&clip, &mp4).unwrap();
        std::fs::write(&text, "글").unwrap();
        let path = |p: &PathBuf| p.display().to_string();
        let attach = |names: &[String], paths: &[String], roots: Option<&[PathBuf]>, section, store_only| {
            h.idea_attach(
                "demo",
                &f,
                IdeaAuthor::User,
                "사람",
                "test",
                names,
                paths,
                roots,
                Some("가게 앞"),
                section,
                store_only,
            )
        };
        let (names, note) = attach(&[], &[path(&pic), path(&clip)], None, Some("시안"), false).unwrap();
        assert_eq!(note.rev, 2, "두 개를 한 판에");
        assert!(
            note.text.contains(&format!("- 첫\n\n![가게 앞]({})\n\n![가게 앞]({})\n\n## 결정", names[0], names[1])),
            "{}",
            note.text
        );
        assert_eq!(note.history[0].summary.as_deref(), Some("「시안」 그림 2개 더함"));
        assert_eq!(note.edited.as_deref(), Some("판 2 「시안」 그림 2개 더함"), "다음 말에 thread가 안다");
        assert_eq!((note.media[0].source, note.media[0].by.as_str()), (IdeaMediaSource::File, "사람"));
        assert_eq!(note.media[0].file.as_deref(), Some(path(&pic).as_str()));
        assert_eq!(note.media_bytes, (png.len() + mp4.len()) as u64);
        // 제한한 폴더 밖(전용 thread가 남의 파일을), 그림 아닌 파일, 올리지 않은 이름
        let outside = state.join("outside.png");
        std::fs::write(&outside, &png).unwrap();
        let roots = [work.clone()];
        assert_eq!(attach(&[], &[path(&outside)], Some(&roots), None, false).unwrap_err().code, ErrorCode::Forbidden);
        assert!(attach(&[], &[path(&pic)], Some(&roots), None, true).is_ok(), "제한 안의 파일");
        assert_eq!(attach(&[], &[path(&text)], None, None, false).unwrap_err().code, ErrorCode::BadRequest);
        assert_eq!(attach(&[], &["rel.png".into()], None, None, false).unwrap_err().code, ErrorCode::BadRequest);
        assert!(attach(&["0123456789abcdef.png".into()], &[], None, None, false).is_err(), "올리지 않은 이름");
        // store_only는 판을 만들지 않는다
        let up = h.idea_file_save("demo", "n-0001", &png, Some("IMG_0001.png".into())).unwrap();
        let (_, kept) = attach(std::slice::from_ref(&up), &[], None, None, true).unwrap();
        assert_eq!(kept.rev, 2);
        // uploads/ 링크(할 일·대화 첨부)는 저장 때 폴더 안 이름으로 옮긴다
        let gif = b"GIF89a\x01\x00\x01\x00".to_vec();
        let upload = h.task_file_save(DeviceRole::Controller, "test", "demo", None, &gif).unwrap();
        let moved =
            h.idea_import_media("demo", &f, &format!("{}\n![첨부]({upload})\n", kept.text), "사람").await.unwrap();
        let name = upload.trim_start_matches("uploads/");
        assert!(moved.ends_with(&format!("![첨부]({name})\n")), "{moved}");
        assert_eq!(f.meta().unwrap().media[name].source, IdeaMediaSource::Uploads);
        // 잠긴 아이디어
        h.idea_handoff("demo", "n-0001", 2, vec![IdeaTaskDraft { title: "x".into(), ..Default::default() }], "test")
            .await
            .unwrap();
        assert_eq!(attach(&[], &[path(&pic)], None, None, false).unwrap_err().code, ErrorCode::BadRequest);
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

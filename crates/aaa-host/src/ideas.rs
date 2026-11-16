//! 옛 아이디어 노트(t-1260, 기획 t-1255 결정 28개). 아이디어는 저장소 기획 문서로 옮겼다(t-1894 4단계, q-3982·q-3985): 노트는
//! 읽기 전용이고 남은 일은 읽기(목록·보기·읽음)와 「기획으로 옮기기」(idea_move.rs)다. host 프로젝트 폴더에 둔다.
//!
//! - `ideas/<ULID>/note.md` 지금 판, `idea.json` 메타(제목·상태·판·기록·넘긴 할 일·전용 thread·옮긴 곳), `revs/<판>.md` 판마다의 글,
//!   그림·영상 `<sha256 앞 16자>.<ext>`(노트는 `![그림](<이름>)`으로 같은 폴더를 가리킨다), `merge/` 겹친 사람 글(옛 판).
//! - 짧은 ID `n-NNNN`은 host DB `idea_ids` 원장(할 일 `task_ids`와 같은 꼴)이다. 번호를 재사용하지 않는다.
//! - 옛 판이 쓰던 칸(합치는 중·보낼 말·띄우지 못한 까닭·알리지 않은 고침)도 그대로 읽는다(`web/e2e/fixtures/ideas`가 그 꼴이다).

use std::path::PathBuf;

use aaa_proto::{
    ErrorBody, ErrorCode, IdeaAuthor, IdeaMediaSource, IdeaMediaView, IdeaNote, IdeaRevView, IdeaStatus, IdeaSummary,
};
use serde::{Deserialize, Serialize};

use crate::orch::{bad, internal, write_atomic};
use crate::{Host, err};

/// 노트 글 상한(TASKS.md와 같다)
pub(crate) const MAX_NOTE: usize = 512 * 1024;
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
    /// 기획으로 옮겼다(t-1894 4단계)
    pub moved: Option<aaa_proto::IdeaMoved>,
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

fn check_text(text: &str) -> Result<(), ErrorBody> {
    if text.len() > MAX_NOTE {
        return Err(err(ErrorCode::TooLarge, format!("노트는 {}KiB까지 쓴다", MAX_NOTE / 1024)));
    }
    if text.contains('\0') {
        return Err(bad("노트에 NUL 문자가 있다"));
    }
    Ok(())
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
}

impl Host {
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

    pub(crate) fn idea_summary(&self, project: &str, f: &Folder, m: &Meta) -> Result<IdeaSummary, ErrorBody> {
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
            moved: m.moved.clone(),
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
pub(crate) fn media_ext(bytes: &[u8]) -> Option<&'static str> {
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
pub(crate) mod tests {
    use super::*;

    fn host() -> (std::sync::Arc<Host>, PathBuf) {
        let state = std::env::temp_dir().join(format!("aaa-ideas-{}", aaa_common::util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.notify.web_push = false;
        cfg.projects.root = state.join("projects").display().to_string();
        let h = Host::open(cfg, state.clone()).unwrap();
        h.project_create("demo", Default::default(), "test").unwrap();
        (h, state)
    }

    fn copy_dir(from: &std::path::Path, to: &std::path::Path) {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap() {
            let e = e.unwrap();
            if e.file_type().unwrap().is_dir() {
                copy_dir(&e.path(), &to.join(e.file_name()));
            } else {
                std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
            }
        }
    }

    /// 옛 판이 만든 아이디어 셋(`web/e2e/fixtures/ideas`: 그림 있는 다듬는 중 판 2, 완료, 합치는 중·보낼 말·까닭이 남은 것)을 든
    /// 프로젝트 `demo`. 이름 차례로 번호를 매긴다(보스 연속 층 n-0001, 캠프 게시판 의뢰 n-0002, 초반 튜토리얼 n-0003)
    pub(crate) fn host_with_fixture_ideas() -> (std::sync::Arc<Host>, PathBuf) {
        let (h, state) = host();
        seed_fixture_ideas(&h, "demo");
        (h, state)
    }

    /// 옛 아이디어 견본을 그 프로젝트에 둔다(쓰기 요청을 거뒀으므로 시험은 폴더를 놓는다)
    pub(crate) fn seed_fixture_ideas(h: &Host, project: &str) {
        let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/e2e/fixtures/ideas");
        let root = h.ideas_root(project).unwrap();
        let mut ids: Vec<_> = std::fs::read_dir(&fixtures).unwrap().map(|e| e.unwrap().file_name()).collect();
        ids.sort();
        for id in ids {
            copy_dir(&fixtures.join(&id), &root.join(&id));
            h.db.idea_short_id(project, id.to_str().unwrap()).unwrap();
        }
    }

    /// 옛 판의 아이디어 폴더는 그대로 읽힌다: 상태·판·그림·넘긴 할 일과 옛 쓰기 칸. 읽음은 사람만 남긴다
    #[test]
    fn old_idea_folders_read_as_they_were() {
        let (h, state) = host_with_fixture_ideas();
        let list = h.idea_list("demo").unwrap();
        let by = |short: &str| list.iter().find(|i| i.short_id == short).unwrap().clone();
        assert_eq!(list.len(), 3);
        assert_eq!(
            (by("n-0002").title.as_str(), by("n-0002").status, by("n-0002").rev),
            ("캠프 게시판 의뢰", IdeaStatus::Open, 2)
        );
        assert_eq!(by("n-0003").status, IdeaStatus::Done);
        assert_eq!(list.last().unwrap().short_id, "n-0003", "완료는 뒤로");
        assert!(list.iter().all(|i| i.moved.is_none()));
        let camp = h.idea_note("demo", &h.idea_folder("demo", "n-0002").unwrap(), None).unwrap();
        assert_eq!(
            camp.media.iter().map(|m| (m.name.as_str(), m.bytes > 0)).collect::<Vec<_>>(),
            [("a6d715956dab435e.png", true)]
        );
        assert_eq!(camp.history.len(), 2);
        assert_eq!(
            h.idea_note("demo", &h.idea_folder("demo", "n-0002").unwrap(), Some(1)).unwrap().media.len(),
            0,
            "판 1에는 그림이 없다"
        );
        let left = h.idea_note("demo", &h.idea_folder("demo", "n-0001").unwrap(), None).unwrap();
        assert_eq!(
            (left.merging, left.pending.len(), left.error.is_some(), left.edited.is_some()),
            (Some(1), 1, true, true)
        );
        assert_eq!(left.idea.thread.as_deref(), Some("t-0009"), "없는 thread여도 읽힌다");
        h.idea_seen("demo", "n-0002", 2).unwrap();
        assert!(h.idea_seen("demo", "n-0002", 3).is_err());
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
    }
}

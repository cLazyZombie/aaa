//! 보고서·결과물 보기(§8.7, M1-4).
//!
//! - 회수할 때 결과물 목록(manifest)을 `threads/<id>.files.json`에 남긴다. 복사 상한을 넘어 agent에 남긴 파일도 적는다.
//! - 웹은 `ThreadLibrary`로 짧은 수명(2시간)의 읽기 전용 경로 `/v/<cap>/`를 받는다. 그 아래 파일은 host 사본에서,
//!   사본이 없으면 agent에서 조각으로 스트리밍한다(host에 캐시하지 않는다).
//! - 결과물 HTML은 CSP `sandbox`(opaque origin)로 보낸다. 그 문서의 하위 요청에는 SameSite=Strict cookie가 붙지 않으므로,
//!   장치 token 대신 이 경로 자체가 그 thread 결과물만 읽는 권한이다(§6.4 「URL에 token을 넣지 않는다」는 장치 token 이야기다).

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use aaa_proto::{
    AgentRequest, AgentResponse, ErrorBody, ErrorCode, FileKind, FrontResponse, LibraryFile, LibraryView, ReportSnapshot,
};

use crate::{Host, err};

/// 보기 경로 수명
pub(crate) const VIEW_TTL: Duration = Duration::from_secs(2 * 3600);
/// agent에서 한 번에 읽는 조각
const CHUNK: u32 = 512 * 1024;
/// 이름으로 모르는 파일이 글인지 볼 앞부분(agent 파일 보기와 같다)
const SNIFF_BYTES: u64 = 8 * 1024;

pub(crate) struct ViewCap {
    pub project: String,
    pub thread: String,
    pub until: Instant,
    /// 이 경로를 받은 웹 기기. 그 기기를 폐기하면 경로도 끝난다(CLI·TUI가 받은 것은 None)
    pub device: Option<String>,
}

pub(crate) fn manifest_path(dir: &Path, id: &str) -> PathBuf {
    dir.join("threads").join(format!("{id}.files.json"))
}

/// 회수한 보고의 결과물 목록을 남긴다(host 사본 + agent에 남긴 일반 파일)
pub(crate) fn write_manifest(dir: &Path, id: &str, snap: &ReportSnapshot) -> std::io::Result<()> {
    let file = |path: &str, size, on_agent| LibraryFile {
        path: path.to_string(),
        size,
        on_agent,
        mime: String::new(),
        kind: FileKind::Binary,
    };
    let mut files: Vec<LibraryFile> = snap.files.iter().map(|f| file(&f.path, f.size, false)).collect();
    files.extend(snap.skipped.iter().filter_map(|s| s.lazy_size.map(|size| file(&s.path, size, true))));
    files.sort_by(|a, b| a.path.cmp(&b.path));
    crate::orch::write_atomic(&manifest_path(dir, id), &serde_json::to_vec_pretty(&files).unwrap())
}

/// library 안 상대 경로인가(`..`·절대 경로·빈 경로 거부)
pub(crate) fn safe_rel(p: &str) -> bool {
    !p.is_empty()
        && !p.contains('\\')
        && !p.contains(':')
        && p.split('/').all(|c| !c.is_empty() && !c.starts_with('.'))
        && Path::new(p).components().all(|c| matches!(c, Component::Normal(_)))
}

/// 최종 파일뿐 아니라 library 아래의 모든 폴더도 symlink를 거부한다.
pub(crate) fn local_file(dir: &Path, id: &str, path: &str) -> Result<(PathBuf, u64), ErrorBody> {
    let mut local = dir.to_path_buf();
    for part in std::iter::once("library").chain(std::iter::once(id)).chain(path.split('/')) {
        local.push(part);
        let m = std::fs::symlink_metadata(&local).map_err(|_| err(ErrorCode::NotFound, "host 사본이 없다"))?;
        if m.file_type().is_symlink() {
            return Err(err(ErrorCode::NotFound, "symlink는 열지 않는다"));
        }
    }
    let m = std::fs::symlink_metadata(&local).map_err(|_| err(ErrorCode::NotFound, "host 사본이 없다"))?;
    if !m.is_file() {
        return Err(err(ErrorCode::NotFound, "일반 파일이 아니다"));
    }
    Ok((local, m.len()))
}

/// 파일 보기가 고를 MIME·종류(agent 파일 보기 `info`와 같다): 이름으로 정하고, 모르는 이름은 host 사본의 앞부분에
/// NUL이 없으면 글이다. agent에만 있는 것은 읽지 않고 binary로 둔다(목록을 줄 때마다 agent에 묻지 않는다)
fn file_type(path: &str, local: Option<&Path>) -> (String, FileKind) {
    let name = path.rsplit('/').next().unwrap_or(path);
    let text = || {
        use std::io::Read;
        let mut buf = Vec::new();
        local
            .and_then(|p| std::fs::File::open(p).ok())
            .and_then(|f| f.take(SNIFF_BYTES).read_to_end(&mut buf).ok())
            .is_some_and(|_| !buf.contains(&0))
    };
    match aaa_proto::mime_of(name) {
        Some(m) => (m.to_string(), FileKind::of_mime(m)),
        None if text() => ("text/plain".into(), FileKind::Text),
        None => ("application/octet-stream".into(), FileKind::Binary),
    }
}

/// 확장자로 정하는 Content-Type(nosniff를 쓰므로 정확히 준다)
pub(crate) fn content_type(path: &str) -> &'static str {
    let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "md" | "txt" | "log" | "csv" | "toml" | "yaml" | "yml" | "rs" | "ts" | "tsx" | "py" | "sh" => {
            "text/plain; charset=utf-8"
        }
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
}

/// 보낼 파일: host 사본이거나 agent에서 읽을 것
pub(crate) enum ViewSource {
    Local(std::fs::File, u64),
    Page(Vec<u8>),
    Agent { conn: crate::AgentConn, work_dir: String, path: String, size: u64 },
}

pub(crate) struct ViewFile {
    pub source: ViewSource,
    pub project: String,
    pub name: String,
}

pub(crate) type ByteStream = futures_util::stream::BoxStream<'static, Result<Vec<u8>, std::io::Error>>;

impl ViewSource {
    pub fn size(&self) -> u64 {
        match self {
            ViewSource::Local(_, n) => *n,
            ViewSource::Page(b) => b.len() as u64,
            ViewSource::Agent { size, .. } => *size,
        }
    }

    /// `start`부터 `len`바이트(파일 보기의 Range). agent 파일은 첫 조각을 먼저 받아, 못 읽으면 응답 전에 오류로 끝낸다.
    pub async fn open(self, start: u64, len: u64) -> Result<ByteStream, ErrorBody> {
        use futures_util::StreamExt;
        match self {
            ViewSource::Page(b) => {
                Ok(futures_util::stream::once(async move { Ok(b[start as usize..(start + len) as usize].to_vec()) })
                    .boxed())
            }
            ViewSource::Local(p, _) => {
                use tokio::io::{AsyncReadExt, AsyncSeekExt};
                let mut f = tokio::fs::File::from_std(p);
                f.seek(std::io::SeekFrom::Start(start))
                    .await
                    .map_err(|e| err(ErrorCode::NotFound, format!("host 사본을 열지 못했다: {e}")))?;
                Ok(futures_util::stream::unfold((f.take(len), len), |(mut f, left)| async move {
                    if left == 0 {
                        return None;
                    }
                    let mut buf = vec![0u8; left.min(256 * 1024) as usize];
                    match f.read(&mut buf).await {
                        Ok(0) => None,
                        Ok(n) => {
                            buf.truncate(n);
                            Some((Ok(buf), (f, left - n as u64)))
                        }
                        Err(e) => Some((Err(e), (f, 0))),
                    }
                })
                .boxed())
            }
            ViewSource::Agent { conn, work_dir, path, .. } => {
                let first = conn
                    .request(AgentRequest::FileRead {
                        work_dir: work_dir.clone(),
                        path: path.clone(),
                        offset: start,
                        len: CHUNK.min(len.try_into().unwrap_or(u32::MAX)),
                    })
                    .await
                    .map_err(|e| {
                        err(
                            ErrorCode::Unavailable,
                            format!("agent에 남긴 결과물을 읽지 못한다(정리됐거나 없다): {}", e.message),
                        )
                    })?;
                let AgentResponse::FileChunk { data, eof } = first else {
                    return Err(err(ErrorCode::Internal, "예상하지 못한 응답"));
                };
                Ok(agent_stream(conn, work_dir, path, start, len, data.0, eof).boxed())
            }
        }
    }
}

impl Host {
    pub(crate) fn manifest(&self, dir: &Path, id: &str) -> Vec<LibraryFile> {
        std::fs::read(manifest_path(dir, id)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub(crate) fn thread_library(
        &self,
        project: &str,
        id: &str,
        device: Option<&str>,
        reveal_link: bool,
    ) -> Result<LibraryView, ErrorBody> {
        self.load(project, id)?;
        self.report_library(project, id, device, reveal_link)
    }

    pub(crate) fn report_library(
        &self,
        project: &str,
        id: &str,
        device: Option<&str>,
        reveal_link: bool,
    ) -> Result<LibraryView, ErrorBody> {
        let dir = self.project_dir(project)?;
        self.report_manifest(&dir, project, id)?;
        let rec = match self.load(project, id) {
            Ok(r) => Some(r),
            Err(e) => {
                if self.db.library_report(project, id).map_err(super::orch::internal)?.is_none() {
                    return Err(e);
                }
                None
            }
        };
        let report = rec
            .as_ref()
            .and_then(|r| r.report_hash.as_ref())
            .and_then(|_| std::fs::read(dir.join("threads").join(format!("{id}.md"))).ok())
            .map(|b| String::from_utf8_lossy(&b).into_owned());
        let cap = hex::encode(rand::random::<[u8; 24]>());
        let until = Instant::now() + VIEW_TTL;
        {
            let mut caps = self.view_caps.lock().unwrap();
            caps.retain(|_, c| c.until > Instant::now());
            caps.insert(
                cap.clone(),
                ViewCap {
                    project: project.to_string(),
                    thread: id.to_string(),
                    until,
                    device: device.map(str::to_string),
                },
            );
        }
        let mut files = self.report_manifest(&dir, project, id)?;
        files.retain(|f| safe_rel(&f.path));
        for f in &mut files {
            let local = (!f.on_agent && safe_rel(&f.path)).then(|| dir.join("library").join(id).join(&f.path));
            (f.mime, f.kind) = file_type(&f.path, local.as_deref());
        }
        Ok(LibraryView {
            project: project.to_string(),
            thread: id.to_string(),
            report,
            files,
            base: format!("/v/{cap}/"),
            expires_at_ms: aaa_common::util::now_ms() + VIEW_TTL.as_millis() as u64,
            link: reveal_link.then(|| self.latest_library_link(project, id)).flatten(),
            from_thread: self.report_thread(
                project,
                id,
                self.db.library_report(project, id).map_err(super::orch::internal)?.as_ref(),
            ),
        })
    }

    /// 보기 경로의 cap이 가리키는 (프로젝트, 보고서): 앱이 준 짧은 경로(폐기한 기기 것은 아님), 아니면 서명한 공유 링크
    fn view_target(&self, cap: &str) -> Result<(String, String), ErrorBody> {
        let memory = {
            let caps = self.view_caps.lock().unwrap();
            match caps.get(cap) {
                // 경로는 이 host가 돈 뒤에 만든 것이라 그 뒤의 폐기는 `is_revoked`가 다 안다
                Some(c) if c.until > Instant::now() && !c.device.as_deref().is_some_and(|d| self.is_revoked(d)) => {
                    Some((c.project.clone(), c.thread.clone()))
                }
                _ => None,
            }
        };
        match memory {
            Some(target) => Ok(target),
            None => self.signed_library_at(cap, aaa_common::util::now_ms()),
        }
    }

    /// task 첨부에 붙인 보고서 보기 주소를 그 보고서로 바꾼다. 다른 프로젝트 보고서는 이 프로젝트 할 일에 링크하지 않는다
    pub(crate) fn library_resolve(&self, project: &str, cap: &str) -> Result<FrontResponse, ErrorBody> {
        let (at, name) = self.view_target(cap)?;
        if at != project {
            return Err(err(ErrorCode::BadRequest, format!("다른 프로젝트({at})의 보고서 주소다")));
        }
        Ok(FrontResponse::LibraryResolved { project: at, name })
    }

    /// host에 있는 보고서 문서를 `max` 바이트까지 읽는다(live 공개는 원본 폴더에서). task 첨부를 thread로 넘길 때 쓴다
    pub(crate) fn report_doc_bytes(&self, project: &str, id: &str, path: &str, max: u64) -> Result<Vec<u8>, ErrorBody> {
        use std::io::Read;
        let dir = self.project_dir(project)?;
        let live = self.db.library_report(project, id).map_err(super::orch::internal)?.and_then(|r| r.live);
        let (file, size) = match live {
            Some(root) => crate::library_publish::live_file(Path::new(&root), path)?,
            None => {
                let (local, size) = local_file(&dir, id, path)?;
                (std::fs::File::open(local).map_err(|_| err(ErrorCode::NotFound, "host 사본이 없다"))?, size)
            }
        };
        if size > max {
            return Err(err(ErrorCode::BadRequest, format!("{path}가 너무 크다")));
        }
        let mut buf = Vec::new();
        file.take(max + 1).read_to_end(&mut buf).map_err(|e| err(ErrorCode::NotFound, e.to_string()))?;
        if buf.len() as u64 > max {
            return Err(err(ErrorCode::BadRequest, format!("{path}가 너무 크다")));
        }
        Ok(buf)
    }

    /// 보기 경로의 파일을 찾는다. 읽기는 `ViewSource::open`이다.
    pub(crate) async fn view_source(self: &Arc<Self>, cap: &str, path: &str) -> Result<ViewFile, ErrorBody> {
        let signed = !self.view_caps.lock().unwrap().contains_key(cap);
        let (project, id) = self.view_target(cap)?;
        let view = |source| ViewFile { source, project: project.clone(), name: id.clone() };
        if path.split('/').any(|c| c.starts_with('.') && c != "." && c != "..") {
            return Err(err(ErrorCode::NotFound, "숨은 결과물은 열지 않는다"));
        }
        if !path.is_empty() && !safe_rel(path) {
            return Err(err(ErrorCode::BadRequest, "library 상대 경로가 아니다"));
        }
        let dir = self.project_dir(&project)?;
        let _ops = self.library_ops.lock().unwrap();
        let manifest = self.report_manifest(&dir, &project, &id)?;
        if path.is_empty() && !manifest.iter().any(|f| f.path == "index.html" && !f.on_agent) {
            return Ok(view(ViewSource::Page(library_listing(&manifest))));
        }
        let path = if path.is_empty() { "index.html" } else { path };
        let entry = manifest
            .into_iter()
            .find(|f| f.path == path)
            .ok_or_else(|| err(ErrorCode::NotFound, format!("결과물에 없다: {path}")))?;
        if !entry.on_agent {
            if let Some(r) = self.db.library_report(&project, &id).map_err(super::orch::internal)?
                && let Some(root) = r.live
            {
                let (file, size) = crate::library_publish::live_file(Path::new(&root), path)?;
                return Ok(view(ViewSource::Local(file, size)));
            }
            let (local, _) = local_file(&dir, &id, path)?;
            let file = std::fs::File::open(local).map_err(|_| err(ErrorCode::NotFound, "host 사본이 없다"))?;
            let size = file.metadata().map_err(|_| err(ErrorCode::NotFound, "host 사본이 없다"))?.len();
            return Ok(view(ViewSource::Local(file, size)));
        }
        if signed {
            return Ok(view(ViewSource::Page("<!doctype html><meta charset=utf-8><title>앱에서 열기</title><p>이 결과물은 agent에 남아 있다. pairing한 앱의 보고서 화면에서 연다.</p>".as_bytes().to_vec())));
        }
        let rec = self.load(&project, &id)?;
        let work_dir = rec.work_dir.clone().ok_or_else(|| err(ErrorCode::NotFound, "thread 작업 폴더 기록이 없다"))?;
        let conn = self.live_conn(&rec.machine_id)?;
        Ok(view(ViewSource::Agent { conn, work_dir, path: path.to_string(), size: entry.size }))
    }
}

fn library_listing(files: &[LibraryFile]) -> Vec<u8> {
    let mut s = "<!doctype html><html lang=ko><meta charset=utf-8><meta name=viewport content=\"width=device-width, initial-scale=1\"><title>보고서 결과물</title><h1>보고서 결과물</h1><ul>".to_string();
    for f in files.iter().filter(|f| safe_rel(&f.path)) {
        let name = f
            .path
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&#39;");
        let path = f.path.split('/').map(crate::files::pct).collect::<Vec<_>>().join("/");
        s.push_str(&format!(
            "<li><a href=\"./{path}\">{name}</a>{}</li>",
            if f.on_agent { " · 앱에서 열기" } else { "" }
        ));
    }
    s.push_str("</ul></html>");
    s.into_bytes()
}

/// agent 조각을 이어 읽는 stream(`start`부터 `len`바이트까지). 중간에 끊기면 stream 오류로 끝난다(받은 쪽은 길이가 모자라
/// 실패를 안다).
fn agent_stream(
    conn: crate::AgentConn,
    work_dir: String,
    path: String,
    start: u64,
    len: u64,
    first: Vec<u8>,
    eof: bool,
) -> impl futures_util::Stream<Item = Result<Vec<u8>, std::io::Error>> {
    let read = first.len() as u64;
    let left = len.saturating_sub(read);
    futures_util::stream::unfold(
        (Some(first), start + read, left, eof || left == 0),
        move |(pending, offset, left, done)| {
            let conn = conn.clone();
            let work_dir = work_dir.clone();
            let path = path.clone();
            async move {
                if let Some(b) = pending {
                    return Some((Ok(b), (None, offset, left, done)));
                }
                if done {
                    return None;
                }
                let ask = CHUNK.min(left.try_into().unwrap_or(u32::MAX));
                match conn.request(AgentRequest::FileRead { work_dir, path, offset, len: ask }).await {
                    Ok(AgentResponse::FileChunk { data, eof }) => {
                        let n = data.0.len() as u64;
                        let left = left.saturating_sub(n);
                        Some((Ok(data.0), (None, offset + n, left, eof || n == 0 || left == 0)))
                    }
                    Ok(_) => Some((Err(std::io::Error::other("예상하지 못한 응답")), (None, offset, left, true))),
                    Err(e) => Some((Err(std::io::Error::other(e.message)), (None, offset, left, true))),
                }
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_and_types() {
        assert!(safe_rel("index.html") && safe_rel("img/a.png"));
        for bad in ["", "../x", "/etc/hosts", "a/../../b", "./a"] {
            assert!(!safe_rel(bad), "{bad}");
        }
        assert_eq!(content_type("A.HTML"), "text/html; charset=utf-8");
        assert_eq!(content_type("big.bin"), "application/octet-stream");
        assert_eq!(content_type("noext"), "application/octet-stream");
    }

    #[test]
    fn file_types_come_from_the_name_or_the_host_copy() {
        let dir = std::env::temp_dir().join(format!("aaa-library-type-{}", aaa_common::util::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (text, bin) = (dir.join("NOTES"), dir.join("blob"));
        std::fs::write(&text, "그냥 글\n").unwrap();
        std::fs::write(&bin, b"ab\0cd").unwrap();
        let ty = file_type;
        assert_eq!(ty("shots/a.PNG", None), ("image/png".into(), FileKind::Image));
        assert_eq!(ty("notes.md", None), ("text/markdown".into(), FileKind::Text));
        assert_eq!(ty("NOTES", Some(&text)), ("text/plain".into(), FileKind::Text));
        assert_eq!(ty("blob", Some(&bin)), ("application/octet-stream".into(), FileKind::Binary));
        // agent에만 있는 모르는 이름은 읽지 않는다
        assert_eq!(ty("NOTES", None), ("application/octet-stream".into(), FileKind::Binary));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

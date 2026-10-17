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

use aaa_proto::{AgentRequest, AgentResponse, ErrorBody, ErrorCode, LibraryFile, LibraryView, ReportSnapshot};

use crate::{Host, err};

/// 보기 경로 수명
pub(crate) const VIEW_TTL: Duration = Duration::from_secs(2 * 3600);
/// agent에서 한 번에 읽는 조각
const CHUNK: u32 = 512 * 1024;

pub(crate) struct ViewCap {
    pub project: String,
    pub thread: String,
    pub until: Instant,
}

pub(crate) fn manifest_path(dir: &Path, id: &str) -> PathBuf {
    dir.join("threads").join(format!("{id}.files.json"))
}

/// 회수한 보고의 결과물 목록을 남긴다(host 사본 + agent에 남긴 일반 파일)
pub(crate) fn write_manifest(dir: &Path, id: &str, snap: &ReportSnapshot) -> std::io::Result<()> {
    let mut files: Vec<LibraryFile> =
        snap.files.iter().map(|f| LibraryFile { path: f.path.clone(), size: f.size, on_agent: false }).collect();
    files.extend(
        snap.skipped
            .iter()
            .filter_map(|s| s.lazy_size.map(|size| LibraryFile { path: s.path.clone(), size, on_agent: true })),
    );
    files.sort_by(|a, b| a.path.cmp(&b.path));
    crate::orch::write_atomic(&manifest_path(dir, id), &serde_json::to_vec_pretty(&files).unwrap())
}

/// library 안 상대 경로인가(`..`·절대 경로·빈 경로 거부)
pub(crate) fn safe_rel(p: &str) -> bool {
    !p.is_empty() && Path::new(p).components().all(|c| matches!(c, Component::Normal(_)))
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
    Local(PathBuf, u64),
    Agent { conn: crate::AgentConn, work_dir: String, path: String, size: u64, first: Vec<u8>, eof: bool },
}

impl ViewSource {
    pub fn size(&self) -> u64 {
        match self {
            ViewSource::Local(_, n) => *n,
            ViewSource::Agent { size, .. } => *size,
        }
    }
}

impl Host {
    fn manifest(&self, dir: &Path, id: &str) -> Vec<LibraryFile> {
        std::fs::read(manifest_path(dir, id)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub(crate) fn thread_library(&self, project: &str, id: &str) -> Result<LibraryView, ErrorBody> {
        let dir = self.project_dir(project)?;
        let rec = self.load(project, id)?;
        let report = rec
            .report_hash
            .as_ref()
            .and_then(|_| std::fs::read(dir.join("threads").join(format!("{id}.md"))).ok())
            .map(|b| String::from_utf8_lossy(&b).into_owned());
        let cap = hex::encode(rand::random::<[u8; 24]>());
        let until = Instant::now() + VIEW_TTL;
        {
            let mut caps = self.view_caps.lock().unwrap();
            caps.retain(|_, c| c.until > Instant::now());
            caps.insert(cap.clone(), ViewCap { project: project.to_string(), thread: id.to_string(), until });
        }
        Ok(LibraryView {
            project: project.to_string(),
            thread: id.to_string(),
            report,
            files: self.manifest(&dir, id),
            base: format!("/v/{cap}/"),
            expires_at_ms: aaa_common::util::now_ms() + VIEW_TTL.as_millis() as u64,
        })
    }

    /// 보기 경로의 파일을 찾는다. agent 파일이면 첫 조각을 먼저 받아, 못 읽으면 응답 전에 오류로 끝낸다.
    pub(crate) async fn view_source(self: &Arc<Self>, cap: &str, path: &str) -> Result<ViewSource, ErrorBody> {
        let (project, id) = {
            let caps = self.view_caps.lock().unwrap();
            match caps.get(cap) {
                Some(c) if c.until > Instant::now() => (c.project.clone(), c.thread.clone()),
                _ => return Err(err(ErrorCode::Unauthorized, "보기 경로가 없거나 만료됐다. 목록에서 다시 연다")),
            }
        };
        if !safe_rel(path) {
            return Err(err(ErrorCode::BadRequest, "library 상대 경로가 아니다"));
        }
        let dir = self.project_dir(&project)?;
        let entry = self
            .manifest(&dir, &id)
            .into_iter()
            .find(|f| f.path == path)
            .ok_or_else(|| err(ErrorCode::NotFound, format!("결과물에 없다: {path}")))?;
        if !entry.on_agent {
            let local = dir.join("library").join(&id).join(path);
            let meta = std::fs::symlink_metadata(&local)
                .map_err(|e| err(ErrorCode::NotFound, format!("host 사본이 없다: {path} ({e})")))?;
            if !meta.is_file() {
                return Err(err(ErrorCode::NotFound, format!("일반 파일이 아니다: {path}")));
            }
            return Ok(ViewSource::Local(local, meta.len()));
        }
        let rec = self.load(&project, &id)?;
        let work_dir = rec.work_dir.clone().ok_or_else(|| err(ErrorCode::NotFound, "thread 작업 폴더 기록이 없다"))?;
        let conn = self.live_conn(&rec.machine_id)?;
        let first = conn
            .request(AgentRequest::FileRead {
                work_dir: work_dir.clone(),
                path: path.to_string(),
                offset: 0,
                len: CHUNK,
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
        Ok(ViewSource::Agent { conn, work_dir, path: path.to_string(), size: entry.size, first: data.0, eof })
    }
}

/// agent 조각을 이어 읽는 stream. 중간에 끊기면 stream 오류로 끝난다(받은 쪽은 길이가 모자라 실패를 안다).
pub(crate) fn agent_stream(
    conn: crate::AgentConn,
    work_dir: String,
    path: String,
    first: Vec<u8>,
    eof: bool,
) -> impl futures_util::Stream<Item = Result<Vec<u8>, std::io::Error>> {
    let start = first.len() as u64;
    futures_util::stream::unfold((Some(first), start, eof), move |(pending, offset, done)| {
        let conn = conn.clone();
        let work_dir = work_dir.clone();
        let path = path.clone();
        async move {
            if let Some(b) = pending {
                return Some((Ok(b), (None, offset, done)));
            }
            if done {
                return None;
            }
            match conn.request(AgentRequest::FileRead { work_dir, path, offset, len: CHUNK }).await {
                Ok(AgentResponse::FileChunk { data, eof }) => {
                    let n = data.0.len() as u64;
                    let end = eof || n == 0;
                    Some((Ok(data.0), (None, offset + n, end)))
                }
                Ok(_) => Some((Err(std::io::Error::other("예상하지 못한 응답")), (None, offset, true))),
                Err(e) => Some((Err(std::io::Error::other(e.message)), (None, offset, true))),
            }
        }
    })
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
}

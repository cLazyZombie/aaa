//! 파일 보기(결정 20 D7, plan §8.109, herdr-web-ui `server/file-view.ts`).
//!
//! - 웹은 `PaneFile`로 경로를 묻는다. 그 pane이 도는 머신의 agent가 pane이 지금 있는 폴더 기준으로 찾고(허용 폴더 안만),
//!   host는 찾은 파일 하나에 짧은 수명(30분)의 읽기 경로 `/f/<cap>/<이름>`을 준다. cap은 그 파일 하나(실제 경로)만 읽는다.
//! - 그 경로는 새 탭·내려받기·그림이 그대로 쓴다. 경로만으로는 열리지 않고 짝지은 조작 기기의 cookie도 있어야 한다
//!   (참고 `/api/fs/file`도 기기 인증 뒤다). 경로를 받는 것도 조작 기기만이다(`needs_control`, 참고도 보기 전용 기기는 403).
//! - host에 모으지 않고 agent에서 조각으로 스트리밍한다. Range를 받는다(글은 앞부분만 읽는다).

use std::sync::Arc;
use std::time::{Duration, Instant};

use aaa_proto::{AgentRequest, AgentResponse, DeviceRole, ErrorBody, ErrorCode, FileInfo, FrontResponse};

use crate::{AgentConn, Host, err};

/// 읽기 경로 수명
pub(crate) const FILE_TTL: Duration = Duration::from_secs(30 * 60);
/// agent에서 한 번에 읽는 조각
const CHUNK: u64 = 512 * 1024;

pub(crate) struct FileCap {
    pub machine: String,
    pub info: FileInfo,
    pub until: Instant,
}

/// URL 경로 한 마디로 넣을 이름(RFC 3986 unreserved 밖은 모두 %XX)
pub(crate) fn pct(name: &str) -> String {
    let mut out = String::new();
    for b in name.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `Range: bytes=…` 하나를 [시작, 끝] 바이트로. 없거나 여러 구간이면 None(전체), 범위 밖이면 Err.
pub(crate) fn parse_range(value: Option<&str>, size: u64) -> Result<Option<(u64, u64)>, ()> {
    let Some(spec) = value.and_then(|v| v.trim().strip_prefix("bytes=")) else { return Ok(None) };
    if spec.contains(',') {
        return Ok(None);
    }
    let (a, b) = spec.split_once('-').ok_or(())?;
    let (a, b) = (a.trim(), b.trim());
    let range = if a.is_empty() {
        // 끝에서 n바이트
        let n: u64 = b.parse().map_err(|_| ())?;
        if n == 0 || size == 0 {
            return Err(());
        }
        (size.saturating_sub(n), size - 1)
    } else {
        let start: u64 = a.parse().map_err(|_| ())?;
        let end = if b.is_empty() {
            size.saturating_sub(1)
        } else {
            b.parse::<u64>().map_err(|_| ())?.min(size.saturating_sub(1))
        };
        if start >= size || end < start {
            return Err(());
        }
        (start, end)
    };
    Ok(Some(range))
}

impl Host {
    /// 경로를 찾아 읽기 경로를 준다. 그 머신 agent가 파일 보기(`file.v1`)를 모르면 binary를 바꾸라고 알린다.
    pub(crate) async fn pane_file(
        &self,
        role: DeviceRole,
        actor: &str,
        pane: &str,
        path: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        let (id, mid) = self.resolve_pane(pane)?;
        // 보기 범위(§8.25)는 pane 보기와 같다
        if self.pane_view_of(&id, role).is_none() {
            return Err(err(ErrorCode::NotFound, format!("pane이 없다: {pane}")));
        }
        let conn = self.live_conn(&mid)?;
        let knows =
            self.inner.lock().unwrap().machines.get(&mid).is_some_and(|m| m.features.iter().any(|f| f == "file.v1"));
        if !knows {
            return Err(err(
                ErrorCode::Unavailable,
                "그 머신 agent가 파일 보기를 아직 모른다(agent binary를 바꿔야 한다)",
            ));
        }
        let res = conn.request(AgentRequest::FileLocate { session: id.clone(), path: path.to_string() }).await?;
        let AgentResponse::FileLocated { info, candidates } = res else {
            return Err(err(ErrorCode::Internal, "예상하지 못한 응답"));
        };
        let Some(info) = info else {
            return Ok(FrontResponse::PaneFile { info: None, candidates, url: None });
        };
        let cap = hex::encode(rand::random::<[u8; 24]>());
        {
            let mut caps = self.file_caps.lock().unwrap();
            caps.retain(|_, c| c.until > Instant::now());
            caps.insert(cap.clone(), FileCap { machine: mid, info: info.clone(), until: Instant::now() + FILE_TTL });
        }
        self.db.audit(actor, "pane_file", &format!("{id} {} {} bytes", info.path, info.size));
        let url = format!("/f/{cap}/{}", pct(&info.name));
        Ok(FrontResponse::PaneFile { info: Some(info), candidates: Vec::new(), url: Some(url) })
    }

    /// 읽기 경로의 파일과 그 머신 연결
    pub(crate) fn file_cap(&self, cap: &str) -> Result<(AgentConn, FileInfo), ErrorBody> {
        let (mid, info) = {
            let caps = self.file_caps.lock().unwrap();
            match caps.get(cap) {
                Some(c) if c.until > Instant::now() => (c.machine.clone(), c.info.clone()),
                _ => return Err(err(ErrorCode::Unauthorized, "읽기 경로가 없거나 만료됐다. 파일을 다시 연다")),
            }
        };
        Ok((self.live_conn(&mid)?, info))
    }
}

/// 한 조각을 읽는다(`page`면 페이지로 그린 HTML). 끝에 닿았는지도 돌려준다
pub(crate) async fn read_at(
    conn: &AgentConn,
    path: &str,
    offset: u64,
    len: u64,
    page: bool,
) -> Result<(Vec<u8>, bool), ErrorBody> {
    let req = AgentRequest::FileView { path: path.to_string(), offset, len: len.min(CHUNK) as u32, page };
    match conn.request(req).await? {
        AgentResponse::FileChunk { data, eof } => Ok((data.0, eof)),
        _ => Err(err(ErrorCode::Internal, "예상하지 못한 응답")),
    }
}

/// `first`(이미 읽은 첫 조각) 뒤를 `len`까지(끝에 닿으면 거기까지) agent에서 이어 읽는 stream. 파일이 그 사이 줄면 짧게
/// 끝난다(받은 쪽은 Content-Length가 모자라 안다). 페이지는 길이를 모르고 보내 끝까지 읽는다.
pub(crate) fn file_stream(
    conn: AgentConn,
    path: Arc<str>,
    start: u64,
    len: u64,
    (first, eof): (Vec<u8>, bool),
    page: bool,
) -> impl futures_util::Stream<Item = Result<Vec<u8>, std::io::Error>> {
    let offset = start + first.len() as u64;
    let left = if eof { 0 } else { len.saturating_sub(first.len() as u64) };
    futures_util::stream::unfold((Some(first), offset, left), move |(pending, offset, left)| {
        let conn = conn.clone();
        let path = path.clone();
        async move {
            if let Some(b) = pending {
                return Some((Ok(b), (None, offset, left)));
            }
            if left == 0 {
                return None;
            }
            match read_at(&conn, &path, offset, left, page).await {
                Ok((b, _)) if b.is_empty() => None,
                Ok((b, eof)) => {
                    let n = b.len() as u64;
                    Some((Ok(b), (None, offset + n, if eof { 0 } else { left.saturating_sub(n) })))
                }
                Err(e) => Some((Err(std::io::Error::other(e.message)), (None, offset, 0))),
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_in_urls() {
        assert_eq!(pct("a b.txt"), "a%20b.txt");
        assert_eq!(pct("보고.md"), "%EB%B3%B4%EA%B3%A0.md");
        assert_eq!(pct("../x?y#z"), "..%2Fx%3Fy%23z");
    }

    #[test]
    fn ranges() {
        assert_eq!(parse_range(None, 10), Ok(None));
        assert_eq!(parse_range(Some("bytes=0-3"), 10), Ok(Some((0, 3))));
        assert_eq!(parse_range(Some("bytes=4-"), 10), Ok(Some((4, 9))));
        // 앞부분만 읽는 글 보기: 파일보다 긴 범위는 파일 끝까지
        assert_eq!(parse_range(Some("bytes=0-262143"), 10), Ok(Some((0, 9))));
        assert_eq!(parse_range(Some("bytes=-3"), 10), Ok(Some((7, 9))));
        assert_eq!(parse_range(Some("bytes=0-1,4-5"), 10), Ok(None));
        for bad in ["bytes=10-", "bytes=5-2", "bytes=x-", "bytes=-0"] {
            assert_eq!(parse_range(Some(bad), 10), Err(()), "{bad}");
        }
        assert_eq!(parse_range(Some("bytes=0-"), 0), Err(()));
    }
}

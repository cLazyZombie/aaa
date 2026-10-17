//! 붙여 넣은 그림(§8.20 M1-4 웹, §8.28 M3 TUI·CLI).
//!
//! - 브라우저는 pane WebSocket, TUI·CLI는 Front API(`PaneAttach`)로 512KiB 조각을 보낸다. 조각마다 조작 권한을 확인하고(그 front가 지금 쓰는 화면이 된다),
//!   host에 모으지 않고 바로 대상 머신 agent로 넘긴다(그 사이 화면 출력이 끼어든다).
//! - 첫 조각의 signature로 PNG·JPEG·GIF·WebP만 받고, 이름은 host가 정한다. agent는 세션 폴더에 0600으로 쓴다.
//! - 다 받으면 sha256을 대조하고 그 pane에 경로를 셸 따옴표로 감싸 뒤에 빈칸 하나를 붙여 넣는다(herdr-web-ui·herdr처럼
//!   셸에도, 결정 20 D7). Enter는 보내지 않는다(사람이 글을 덧붙여 보낸다).
//!   - harness(Claude·Codex) 입력창에는 bracketed paste로 넣는다. 확인창·선택 창이 떠 있으면 넣지 않는다: 붙여 넣은 글이
//!     그 답으로 읽힐 수 있다(참고에는 이 막기가 없다, plan §8.51).
//!   - 그 밖(셸 등)은 친 글처럼 넣는다. host는 그 프로그램이 bracketed paste를 켰는지 모르고, 줄바꿈·제어 문자가 없는
//!     따옴표 경로는 bracketed paste가 꺼진 붙여넣기와 같다.

use std::sync::Arc;

use aaa_proto::{AgentRequest, AgentResponse, B64, DeviceRole, ErrorBody, ErrorCode};

use crate::harness::modal_of;
use crate::{FrontRef, Host, err};

use aaa_proto::ATTACH_CHUNK as CHUNK;
/// 첨부 한 개 상한(agent도 같은 값으로 막는다)
const MAX_BYTES: u64 = 16 << 20;

/// 받는 중인 첨부 하나(WebSocket 연결마다 하나)
pub(crate) struct Upload {
    att: String,
    total: u64,
    sha256: String,
    next: u64,
}

pub(crate) struct Attached {
    pub path: String,
    pub inserted: bool,
    pub note: Option<String>,
}

/// 확장자. 브라우저가 알린 MIME이 아니라 내용의 signature로 정한다.
fn sniff(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if b.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("jpg")
    } else if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        Some("gif")
    } else if b.len() >= 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Some("webp")
    } else {
        None
    }
}

impl Host {
    /// 조각 하나를 처리한다. 마지막 조각이면 저장 결과를 돌려준다. 실패하면 `up`을 비운다.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn attach_chunk(
        self: &Arc<Self>,
        role: DeviceRole,
        actor: &str,
        front: &FrontRef,
        pane: &str,
        up: &mut Option<Upload>,
        offset: u64,
        total: u64,
        sha256: &str,
        data: Vec<u8>,
    ) -> Result<Option<Attached>, ErrorBody> {
        let r = self.attach_step(role, actor, front, pane, up, offset, total, sha256, data).await;
        if r.is_err() || matches!(r, Ok(Some(_))) {
            *up = None;
        }
        r
    }

    #[allow(clippy::too_many_arguments)]
    async fn attach_step(
        self: &Arc<Self>,
        role: DeviceRole,
        actor: &str,
        front: &FrontRef,
        pane: &str,
        up: &mut Option<Upload>,
        offset: u64,
        total: u64,
        sha256: &str,
        data: Vec<u8>,
    ) -> Result<Option<Attached>, ErrorBody> {
        if !role.can_control() {
            return Err(err(ErrorCode::Forbidden, "보기 권한으로는 붙여 넣을 수 없다"));
        }
        let (id, mid) = self.resolve_pane(pane)?;
        self.check_running(&id)?;
        if front.interactive() {
            self.lease_use(&id, front);
        }
        if data.is_empty() || data.len() > CHUNK {
            return Err(err(ErrorCode::BadRequest, format!("조각은 1..{CHUNK}바이트다")));
        }
        if offset == 0 {
            if total == 0 || total > MAX_BYTES {
                return Err(err(ErrorCode::BadRequest, format!("그림은 {}MiB까지 붙여 넣는다", MAX_BYTES >> 20)));
            }
            if sha256.len() != 64 || !sha256.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()) {
                return Err(err(ErrorCode::BadRequest, "sha256 형식이 아니다"));
            }
            let ext = sniff(&data).ok_or_else(|| err(ErrorCode::BadRequest, "PNG·JPEG·GIF·WebP 그림만 붙여 넣는다"))?;
            *up = Some(Upload {
                att: format!("{}.{ext}", hex::encode(rand::random::<[u8; 8]>())),
                total,
                sha256: sha256.to_string(),
                next: 0,
            });
        }
        let u = up.as_mut().ok_or_else(|| err(ErrorCode::BadRequest, "첨부 시작(offset 0) 없이 이어지는 조각이다"))?;
        if offset != u.next || total != u.total || sha256 != u.sha256 {
            return Err(err(ErrorCode::BadRequest, "조각 순서가 맞지 않다"));
        }
        let end = offset + data.len() as u64;
        if end > u.total {
            return Err(err(ErrorCode::BadRequest, "조각이 전체 길이를 넘는다"));
        }
        let eof = end == u.total;
        let conn = self.live_conn(&mid)?;
        let res = conn
            .request(AgentRequest::AttachWrite {
                session: id.clone(),
                att: u.att.clone(),
                offset,
                data: B64(data),
                eof,
            })
            .await?;
        u.next = end;
        if !eof {
            return Ok(None);
        }
        let AgentResponse::Attached { path, sha256: got } = res else {
            return Err(err(ErrorCode::Internal, "예상하지 못한 응답"));
        };
        if got != u.sha256 {
            return Err(err(ErrorCode::Internal, "저장한 그림의 sha256이 보낸 것과 다르다"));
        }
        let (inserted, note) = self.attach_insert(&id, &mid, &path).await;
        self.db.audit(actor, "pane_attach", &format!("{id} {} {} bytes inserted={inserted}", u.att, u.total));
        Ok(Some(Attached { path, inserted, note }))
    }

    /// pane에 경로를 넣을지 다시 확인하고 넣는다. 넣지 않으면 그 이유를 돌려준다.
    async fn attach_insert(self: &Arc<Self>, id: &str, mid: &str, path: &str) -> (bool, Option<String>) {
        let no = |s: &str| (false, Some(s.to_string()));
        if self.check_running(id).is_err() {
            return no("그 사이 세션이 끝났다");
        }
        // hook을 보내는 harness(Claude·Codex) pane은 입력창일 때만
        let harness = self.pane_agent(id).is_some_and(|a| a.harness != "report");
        if harness {
            if self.open_question_of(id).is_some() {
                return no("확인창이 떠 있어 경로를 넣지 않았다");
            }
            let Some(lines) = self.screen_lines(mid, id).await else {
                return no("화면을 읽지 못해 경로를 넣지 않았다");
            };
            if modal_of(&lines).is_some() {
                return no("선택 창이 떠 있어 경로를 넣지 않았다");
            }
        }
        let Ok(conn) = self.live_conn(mid) else { return no("agent와 연결이 끊겼다") };
        let quoted = shell_quoted(path);
        let paste = if harness { format!("\x1b[200~{quoted}\x1b[201~") } else { quoted };
        match conn.request(AgentRequest::Input { session: id.to_string(), data: B64(paste.into_bytes()) }).await {
            Ok(_) => (true, None),
            Err(e) => (false, Some(format!("경로를 넣지 못했다: {}", e.message))),
        }
    }
}

/// herdr-web-ui PaneTerminal 1443줄: 작은따옴표로 감싸고(안의 `'`는 `'\''`) 뒤에 빈칸 하나
fn shell_quoted(path: &str) -> String {
    format!("'{}' ", path.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoted_like_the_reference() {
        assert_eq!(shell_quoted("/a b/x.png"), "'/a b/x.png' ");
        assert_eq!(shell_quoted("/it's/x.png"), "'/it'\\''s/x.png' ");
    }

    #[test]
    fn signatures() {
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\n...."), Some("png"));
        assert_eq!(sniff(&[0xff, 0xd8, 0xff, 0xe0]), Some("jpg"));
        assert_eq!(sniff(b"GIF89a.."), Some("gif"));
        assert_eq!(sniff(b"RIFF\0\0\0\0WEBPVP8 "), Some("webp"));
        for bad in [&b"<svg xmlns"[..], b"%PDF-1.7", b"RIFF\0\0\0\0WAVE", b""] {
            assert_eq!(sniff(bad), None);
        }
    }
}

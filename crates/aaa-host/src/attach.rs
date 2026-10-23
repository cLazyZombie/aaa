//! 붙여 넣은 그림(§8.20 M1-4 웹, §8.28 M3 CLI).
//!
//! - 브라우저는 pane WebSocket, CLI는 Front API(`PaneAttach`)로 512KiB 조각을 보낸다. 조각마다 조작 권한을 확인하고(그 front가 지금 쓰는 화면이 된다),
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
pub(crate) fn sniff(b: &[u8]) -> Option<&'static str> {
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
    /// 대화 첨부(herdr-web-ui `savePaneImage`): 저장한 경로만 돌려준다. 입력은 Composer의 보내기 때 간다.
    /// coordinator는 host의 프로젝트 uploads/, 나머지 pane은 대상 agent의 세션 첨부 폴더다.
    pub(crate) async fn chat_image(
        self: &Arc<Self>,
        role: DeviceRole,
        actor: &str,
        pane: &str,
        bytes: &[u8],
    ) -> Result<String, ErrorBody> {
        use sha2::{Digest, Sha256};
        if !role.can_control() {
            return Err(err(ErrorCode::Forbidden, "보기 권한으로는 붙여 넣을 수 없다"));
        }
        let (id, mid) = self.resolve_pane(pane)?;
        self.check_running(&id)?;
        if bytes.is_empty() || bytes.len() as u64 > MAX_BYTES {
            return Err(err(ErrorCode::BadRequest, "그림은 1바이트부터 16MiB까지 붙여 넣는다"));
        }
        let ext = sniff(bytes).ok_or_else(|| err(ErrorCode::BadRequest, "PNG·JPEG·GIF·WebP 그림만 붙여 넣는다"))?;
        let project = self
            .projects_all()
            .into_iter()
            .find(|p| self.coordinator_panes(std::path::Path::new(&p.path)).iter().any(|p| p.id == id));
        let path = if let Some(project) = project {
            // task 메모로도 넘기는 프로젝트 uploads는 저장 바이트의 hash로 이름을 붙인다.
            // 같은 저장 경계를 써서 두 화면의 이름·중복·기존 원본 보호 규칙이 갈라지지 않게 한다.
            let relative = self.task_file_save(role, actor, &project.name, None, bytes)?;
            self.project_dir(&project.name)?.join(relative).to_string_lossy().into_owned()
        } else {
            let att = format!("{}.{ext}", hex::encode(rand::random::<[u8; 8]>()));
            let conn = self.live_conn(&mid)?;
            let mut saved = None;
            for (i, chunk) in bytes.chunks(CHUNK).enumerate() {
                let offset = i * CHUNK;
                let response = conn
                    .request(AgentRequest::AttachWrite {
                        session: id.clone(),
                        att: att.clone(),
                        offset: offset as u64,
                        data: B64(chunk.to_vec()),
                        eof: offset + chunk.len() == bytes.len(),
                    })
                    .await?;
                if let AgentResponse::Attached { path, sha256 } = response {
                    if sha256 != hex::encode(Sha256::digest(bytes)) {
                        return Err(err(ErrorCode::Internal, "저장한 그림의 sha256이 보낸 것과 다르다"));
                    }
                    saved = Some(path);
                }
            }
            saved.ok_or_else(|| err(ErrorCode::Internal, "그림 저장 응답이 없다"))?
        };
        if actor.strip_prefix("device:").is_some_and(|d| self.is_revoked(d)) {
            return Err(err(ErrorCode::Unauthorized, "기기가 폐기됐다"));
        }
        let name = path.rsplit(['/', '\\']).next().unwrap_or_default();
        self.db.audit(actor, "chat_image", &format!("{id} {name} {} bytes", bytes.len()));
        Ok(path)
    }

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
        if r.as_ref()
            .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
            .is_err()
            || matches!(r, Ok(Some(_)))
        {
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
        // 마지막 조각을 쓰는 동안 기기가 폐기됐으면 경로를 넣지 않는다(§6.4 「전송이 끝나기 직전에 권한을 다시」, t-0340 A9)
        if actor.strip_prefix("device:").is_some_and(|d| self.is_revoked(d)) {
            return Err(err(ErrorCode::Unauthorized, "기기가 폐기됐다"));
        }
        let (inserted, note) = self.attach_insert(&id, &mid, &path).await;
        self.db.audit(actor, "pane_attach", &format!("{id} {} {} bytes inserted={inserted}", u.att, u.total));
        Ok(Some(Attached { path, inserted, note }))
    }

    /// pane에 경로를 넣을지 다시 확인하고 넣는다. 넣지 않으면 그 이유를 돌려준다.
    async fn attach_insert(self: &Arc<Self>, id: &str, mid: &str, path: &str) -> (bool, Option<String>) {
        let no = |s: &str| (false, Some(s.to_string()));
        if self
            .check_running(id)
            .as_ref()
            .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
            .is_err()
        {
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
        let Ok(conn) = self
            .live_conn(mid)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다"))
        else {
            return no("agent와 연결이 끊겼다");
        };
        let quoted = shell_quoted(path);
        // herdr-web-ui는 xterm `paste`로 넣는다: 프로그램이 bracketed paste를 켰을 때만 감싼다(셸·harness 같다)
        let paste = self.key_modes(id).paste(&quoted);
        match conn.request(AgentRequest::Input { session: id.to_string(), data: B64(paste) }).await {
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

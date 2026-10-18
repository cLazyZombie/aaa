//! 길이 접두 frame 코덱.
//!
//! wire 형식: `u32 BE 길이 | u8 종류 | payload`. 길이는 종류 byte와 payload를 합한 크기다.
//! 제어 plane(JSON)과 터미널 plane(binary)을 같은 연결에서 종류 byte로 나눈다.
//! 상한을 넘는 길이는 payload를 읽기 전에 거부한다(메모리 할당 전에).

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// 기본 frame 상한(1 MiB). 연결마다 더 작게 정할 수 있다(인증 전 등).
pub const MAX_FRAME: usize = 1 << 20;
/// 인증 전에 받을 수 있는 frame 상한.
pub const MAX_FRAME_PREAUTH: usize = 16 << 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FrameKind {
    /// 버전이 있는 JSON 제어 메시지
    Control = 1,
    /// 터미널 출력(binary, `TerminalChunk`)
    Terminal = 2,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub kind: FrameKind,
    pub payload: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("frame 크기 {len}이 상한 {max}을 넘는다")]
    TooLarge { len: usize, max: usize },
    #[error("빈 frame")]
    Empty,
    #[error("모르는 frame 종류 {0}")]
    UnknownKind(u8),
    #[error("연결이 frame 중간에 끊겼다")]
    Truncated,
    #[error("터미널 chunk 형식 오류")]
    BadTerminalChunk,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// 다음 frame을 읽는다. 연결이 frame 경계에서 정상 종료하면 `Ok(None)`.
/// 취소에 안전하지 않다: 읽던 frame 앞부분을 이 함수 안에만 두어, `select!`에서 다른 갈래가 이기면 그 바이트를 잃고 다음 읽기의
/// 경계가 어긋난다. `select!` 안에서는 읽은 바이트를 연결 쪽에 두는 `take_frame`(aaa-common `FrameRx`)을 쓴다.
pub async fn read_frame<R: AsyncRead + Unpin>(r: &mut R, max: usize) -> Result<Option<Frame>, FrameError> {
    let mut len_buf = [0u8; 4];
    let mut got = 0;
    while got < 4 {
        let n = r.read(&mut len_buf[got..]).await?;
        if n == 0 {
            return if got == 0 { Ok(None) } else { Err(FrameError::Truncated) };
        }
        got += n;
    }
    let len = u32::from_be_bytes(len_buf) as usize;
    if len == 0 {
        return Err(FrameError::Empty);
    }
    if len > max {
        return Err(FrameError::TooLarge { len, max });
    }
    let mut kind = [0u8; 1];
    r.read_exact(&mut kind).await.map_err(eof_as_truncated)?;
    let kind = match kind[0] {
        1 => FrameKind::Control,
        2 => FrameKind::Terminal,
        k => return Err(FrameError::UnknownKind(k)),
    };
    let mut payload = vec![0u8; len - 1];
    r.read_exact(&mut payload).await.map_err(eof_as_truncated)?;
    Ok(Some(Frame { kind, payload }))
}

/// 모은 바이트 맨 앞에서 frame 하나를 꺼낸다(꺼낸 만큼 `buf`에서 지운다). 아직 덜 왔으면 `Ok(None)`이고 `buf`는 그대로다.
/// `read_frame`과 같이 길이를 읽자마자 상한을 보고, 종류 byte가 오면 바로 본다(payload가 다 오기 전에).
pub fn take_frame(buf: &mut Vec<u8>, max: usize) -> Result<Option<Frame>, FrameError> {
    let Some(head) = buf.first_chunk::<4>() else { return Ok(None) };
    let len = u32::from_be_bytes(*head) as usize;
    if len == 0 {
        return Err(FrameError::Empty);
    }
    if len > max {
        return Err(FrameError::TooLarge { len, max });
    }
    let Some(&kind) = buf.get(4) else { return Ok(None) };
    let kind = match kind {
        1 => FrameKind::Control,
        2 => FrameKind::Terminal,
        k => return Err(FrameError::UnknownKind(k)),
    };
    if buf.len() < 4 + len {
        return Ok(None);
    }
    let payload = buf[5..4 + len].to_vec();
    buf.drain(..4 + len);
    Ok(Some(Frame { kind, payload }))
}

fn eof_as_truncated(e: std::io::Error) -> FrameError {
    if e.kind() == std::io::ErrorKind::UnexpectedEof { FrameError::Truncated } else { FrameError::Io(e) }
}

/// frame 하나를 쓴다(flush 포함).
pub async fn write_frame<W: AsyncWrite + Unpin>(
    w: &mut W,
    kind: FrameKind,
    payload: &[u8],
    max: usize,
) -> Result<(), FrameError> {
    let len = payload.len() + 1;
    if len > max {
        return Err(FrameError::TooLarge { len, max });
    }
    let mut buf = Vec::with_capacity(4 + len);
    buf.extend_from_slice(&(len as u32).to_be_bytes());
    buf.push(kind as u8);
    buf.extend_from_slice(payload);
    w.write_all(&buf).await?;
    w.flush().await?;
    Ok(())
}

/// 터미널 출력 chunk: `session id 길이(u8) | session id(UTF-8) | offset(u64 BE) | data`.
/// offset은 세션 출력 전체에서 이 chunk 첫 byte의 위치다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalChunk {
    pub session: String,
    pub offset: u64,
    pub data: Vec<u8>,
}

impl TerminalChunk {
    pub fn encode(&self) -> Vec<u8> {
        let id = self.session.as_bytes();
        assert!(id.len() <= u8::MAX as usize, "session id가 너무 길다");
        let mut out = Vec::with_capacity(1 + id.len() + 8 + self.data.len());
        out.push(id.len() as u8);
        out.extend_from_slice(id);
        out.extend_from_slice(&self.offset.to_be_bytes());
        out.extend_from_slice(&self.data);
        out
    }

    pub fn decode(buf: &[u8]) -> Result<Self, FrameError> {
        let (&id_len, rest) = buf.split_first().ok_or(FrameError::BadTerminalChunk)?;
        let id_len = id_len as usize;
        if rest.len() < id_len + 8 {
            return Err(FrameError::BadTerminalChunk);
        }
        let session = std::str::from_utf8(&rest[..id_len]).map_err(|_| FrameError::BadTerminalChunk)?.to_string();
        let offset = u64::from_be_bytes(rest[id_len..id_len + 8].try_into().unwrap());
        Ok(TerminalChunk { session, offset, data: rest[id_len + 8..].to_vec() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn roundtrip_and_clean_eof() {
        let mut buf = Vec::new();
        write_frame(&mut buf, FrameKind::Control, b"{}", MAX_FRAME).await.unwrap();
        write_frame(&mut buf, FrameKind::Terminal, b"xyz", MAX_FRAME).await.unwrap();
        let mut r = buf.as_slice();
        let a = read_frame(&mut r, MAX_FRAME).await.unwrap().unwrap();
        let b = read_frame(&mut r, MAX_FRAME).await.unwrap().unwrap();
        assert_eq!((a.kind, a.payload.as_slice()), (FrameKind::Control, &b"{}"[..]));
        assert_eq!((b.kind, b.payload.as_slice()), (FrameKind::Terminal, &b"xyz"[..]));
        assert!(read_frame(&mut r, MAX_FRAME).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn take_frame_waits_for_whole_frames_and_keeps_the_rest() {
        let mut wire = Vec::new();
        write_frame(&mut wire, FrameKind::Control, b"{}", MAX_FRAME).await.unwrap();
        write_frame(&mut wire, FrameKind::Terminal, b"xyz", MAX_FRAME).await.unwrap();
        // 한 바이트씩 모으면 frame이 다 온 때에만 나오고, 그 전에는 모은 것을 그대로 둔다
        let mut buf = Vec::new();
        let mut got = Vec::new();
        for &b in &wire {
            buf.push(b);
            let before = buf.len();
            match take_frame(&mut buf, MAX_FRAME).unwrap() {
                Some(f) => got.push((f.kind, f.payload)),
                None => assert_eq!(buf.len(), before),
            }
        }
        assert_eq!(got, vec![(FrameKind::Control, b"{}".to_vec()), (FrameKind::Terminal, b"xyz".to_vec())]);
        assert!(buf.is_empty());
        // 두 frame이 한 번에 와도 차례로 하나씩
        let mut both = wire.clone();
        assert_eq!(take_frame(&mut both, MAX_FRAME).unwrap().unwrap().payload, b"{}");
        assert_eq!(take_frame(&mut both, MAX_FRAME).unwrap().unwrap().payload, b"xyz");
        assert!(take_frame(&mut both, MAX_FRAME).unwrap().is_none());
    }

    #[test]
    fn take_frame_rejects_like_read_frame_before_the_payload() {
        let mut big = u32::MAX.to_be_bytes().to_vec();
        assert!(matches!(take_frame(&mut big, MAX_FRAME_PREAUTH), Err(FrameError::TooLarge { .. })));
        let mut empty = 0u32.to_be_bytes().to_vec();
        assert!(matches!(take_frame(&mut empty, MAX_FRAME), Err(FrameError::Empty)));
        let mut bad_kind = vec![0, 0, 0, 3, 9];
        assert!(matches!(take_frame(&mut bad_kind, MAX_FRAME), Err(FrameError::UnknownKind(9))));
    }

    #[tokio::test]
    async fn oversized_length_is_rejected_before_reading_payload() {
        // 길이만 4GiB 가까이 적고 payload는 없다. 할당 없이 바로 거부해야 한다.
        let buf = (u32::MAX).to_be_bytes().to_vec();
        let err = read_frame(&mut buf.as_slice(), MAX_FRAME_PREAUTH).await.unwrap_err();
        assert!(matches!(err, FrameError::TooLarge { max: MAX_FRAME_PREAUTH, .. }));
    }

    #[tokio::test]
    async fn rejects_unknown_kind_empty_and_truncated() {
        let mut bad_kind = 2u32.to_be_bytes().to_vec();
        bad_kind.extend_from_slice(&[9, 0]);
        assert!(matches!(read_frame(&mut bad_kind.as_slice(), MAX_FRAME).await, Err(FrameError::UnknownKind(9))));
        let empty = 0u32.to_be_bytes().to_vec();
        assert!(matches!(read_frame(&mut empty.as_slice(), MAX_FRAME).await, Err(FrameError::Empty)));
        let mut cut = 10u32.to_be_bytes().to_vec();
        cut.extend_from_slice(&[1, b'{']);
        assert!(matches!(read_frame(&mut cut.as_slice(), MAX_FRAME).await, Err(FrameError::Truncated)));
        let half_len = vec![0u8, 0];
        assert!(matches!(read_frame(&mut half_len.as_slice(), MAX_FRAME).await, Err(FrameError::Truncated)));
    }

    #[tokio::test]
    async fn writer_refuses_frames_over_limit() {
        let mut buf = Vec::new();
        let big = vec![0u8; 100];
        assert!(matches!(write_frame(&mut buf, FrameKind::Control, &big, 50).await, Err(FrameError::TooLarge { .. })));
        assert!(buf.is_empty());
    }

    #[test]
    fn terminal_chunk_roundtrip_and_bounds() {
        let c = TerminalChunk { session: "01J0ABC".into(), offset: 1 << 40, data: "한글\x1b[0m".into() };
        assert_eq!(TerminalChunk::decode(&c.encode()).unwrap(), c);
        assert!(TerminalChunk::decode(&[]).is_err());
        assert!(TerminalChunk::decode(&[5, b'a', b'b']).is_err());
    }
}

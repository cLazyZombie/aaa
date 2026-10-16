//! 세션 출력 보존 버퍼. 전체 출력에서의 절대 offset으로 다룬다.

use std::collections::VecDeque;

pub struct Ring {
    buf: VecDeque<u8>,
    cap: usize,
    /// buf[0]의 절대 offset
    start: u64,
}

impl Ring {
    pub fn new(cap: usize) -> Self {
        Ring { buf: VecDeque::with_capacity(cap.min(1 << 20)), cap, start: 0 }
    }

    /// 지금까지 쓴 전체 길이(= 다음 byte의 offset)
    pub fn end(&self) -> u64 {
        self.start + self.buf.len() as u64
    }

    pub fn start(&self) -> u64 {
        self.start
    }

    pub fn push(&mut self, data: &[u8]) {
        self.buf.extend(data);
        let over = self.buf.len().saturating_sub(self.cap);
        if over > 0 {
            self.buf.drain(..over);
            self.start += over as u64;
        }
    }

    /// `from`부터 끝까지. 보존 범위 밖이면 None(snapshot으로 재동기화해야 한다).
    pub fn since(&self, from: u64) -> Option<Vec<u8>> {
        if from < self.start || from > self.end() {
            return None;
        }
        let skip = (from - self.start) as usize;
        Some(self.buf.iter().skip(skip).copied().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_survive_eviction() {
        let mut r = Ring::new(8);
        r.push(b"abcdef");
        assert_eq!(r.since(2).unwrap(), b"cdef");
        r.push(b"ghijk"); // 11 bytes 중 앞 3개가 밀려난다
        assert_eq!((r.start(), r.end()), (3, 11));
        assert_eq!(r.since(3).unwrap(), b"defghijk");
        assert_eq!(r.since(11).unwrap(), b"");
        assert!(r.since(2).is_none(), "밀려난 위치는 재동기화가 필요하다");
        assert!(r.since(12).is_none(), "미래 offset은 거부한다");
    }
}

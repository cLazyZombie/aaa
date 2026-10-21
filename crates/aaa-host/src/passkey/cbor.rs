//! passkey 등록에 필요한 만큼의 CBOR 읽기(RFC 8949): attestation object와 COSE 공개키. 길이가 정해진 값만 받고(CTAP2는
//! 정해진 길이만 쓴다) 깊이·크기를 묶는다. 새 의존성을 들이지 않는다.

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Cbor {
    Uint(u64),
    /// 음의 정수 `-1 - n`
    Nint(u64),
    Bytes(Vec<u8>),
    Text(String),
    Array(Vec<Cbor>),
    Map(Vec<(Cbor, Cbor)>),
    /// false·true·null·undefined와 실수(값은 쓰지 않는다)
    Simple,
}

const MAX_DEPTH: usize = 16;

impl Cbor {
    /// 정수 키·값(COSE 키는 작은 정수다)
    pub(crate) fn int(&self) -> Option<i64> {
        match *self {
            Cbor::Uint(n) => i64::try_from(n).ok(),
            Cbor::Nint(n) => i64::try_from(n).ok().map(|n| -1 - n),
            _ => None,
        }
    }

    /// 지도에서 key의 값
    pub(crate) fn get(&self, key: &Cbor) -> Option<&Cbor> {
        match self {
            Cbor::Map(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
}

/// 맨 앞 값 하나와 그 값이 쓴 바이트 수
pub(crate) fn decode(buf: &[u8]) -> Result<(Cbor, usize), &'static str> {
    let mut at = 0;
    let value = item(buf, &mut at, 0)?;
    Ok((value, at))
}

fn take<'a>(buf: &'a [u8], at: &mut usize, n: usize) -> Result<&'a [u8], &'static str> {
    let end = at.checked_add(n).filter(|&e| e <= buf.len()).ok_or("CBOR가 중간에 끝났다")?;
    let out = &buf[*at..end];
    *at = end;
    Ok(out)
}

fn argument(buf: &[u8], at: &mut usize, info: u8) -> Result<u64, &'static str> {
    Ok(match info {
        0..=23 => info as u64,
        24 => take(buf, at, 1)?[0] as u64,
        25 => u16::from_be_bytes(take(buf, at, 2)?.try_into().unwrap()) as u64,
        26 => u32::from_be_bytes(take(buf, at, 4)?.try_into().unwrap()) as u64,
        27 => u64::from_be_bytes(take(buf, at, 8)?.try_into().unwrap()),
        _ => return Err("길이를 정하지 않은 CBOR 값은 받지 않는다"),
    })
}

fn item(buf: &[u8], at: &mut usize, depth: usize) -> Result<Cbor, &'static str> {
    if depth > MAX_DEPTH {
        return Err("CBOR가 너무 깊다");
    }
    let head = take(buf, at, 1)?[0];
    let (major, info) = (head >> 5, head & 0x1f);
    if major == 7 {
        // 단순 값과 실수: 뒤 바이트만 건너뛴다
        let skip = match info {
            0..=23 => 0,
            24 => 1,
            25 => 2,
            26 => 4,
            27 => 8,
            _ => return Err("길이를 정하지 않은 CBOR 값은 받지 않는다"),
        };
        take(buf, at, skip)?;
        return Ok(Cbor::Simple);
    }
    let n = argument(buf, at, info)?;
    // 남은 바이트보다 많은 항목은 있을 수 없다. 겹친 머리마다 미리 잡지 않게 자리는 조금만 먼저 잡는다
    let count =
        |n: u64| usize::try_from(n).ok().filter(|&n| n <= buf.len() - *at).ok_or("CBOR 길이가 남은 크기보다 크다");
    Ok(match major {
        0 => Cbor::Uint(n),
        1 => Cbor::Nint(n),
        2 => Cbor::Bytes(take(buf, at, count(n)?)?.to_vec()),
        3 => Cbor::Text(String::from_utf8(take(buf, at, count(n)?)?.to_vec()).map_err(|_| "CBOR 글이 UTF-8이 아니다")?),
        4 => {
            let n = count(n)?;
            let mut items = Vec::with_capacity(n.min(16));
            for _ in 0..n {
                items.push(item(buf, at, depth + 1)?);
            }
            Cbor::Array(items)
        }
        5 => {
            let n = count(n)?;
            let mut entries = Vec::with_capacity(n.min(16));
            for _ in 0..n {
                let k = item(buf, at, depth + 1)?;
                let v = item(buf, at, depth + 1)?;
                entries.push((k, v));
            }
            Cbor::Map(entries)
        }
        // 6: tag(WebAuthn은 쓰지 않는다)
        _ => return Err("이 CBOR 종류는 받지 않는다"),
    })
}

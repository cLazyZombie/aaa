//! 크기 제한 private 입력. 마지막 EOF를 확인하기 전에는 발급/소비/파일 쓰기를 시작하지 않는다.
use super::*;
use std::io::{Read, Write};

#[cfg(windows)]
mod password;

const MAGIC: &[u8] = b"aaa.install.input.v1\0";
const LIMIT: usize = 1024 * 1024;
fn invalid() -> Fault {
    Fault { code: "installation_private_input_invalid", exit: 2 }
}

struct Bytes(Vec<u8>);
impl Drop for Bytes {
    fn drop(&mut self) {
        for byte in &mut self.0 {
            // 소유한 비밀 입력 buffer를 최적화로 생략하지 않고 지운다.
            unsafe { std::ptr::write_volatile(byte, 0) };
        }
        std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
    }
}

pub(super) struct Input {
    pub(super) authorization: InstallAuthorization,
    pub(super) password: Option<supervisor::Password>,
}
impl Input {
    /// authorization 단독 입력은 한 JSON 문서와 EOF다. 비밀 frame 입력과 동시에 소비하지 않는다.
    pub(super) fn authorization(mut reader: impl Read) -> Result<InstallAuthorization> {
        let mut bytes = Bytes(Vec::new());
        reader.by_ref().take(LIMIT as u64 + 1).read_to_end(&mut bytes.0).map_err(|_| invalid())?;
        if bytes.0.len() > LIMIT {
            return Err(invalid());
        }
        serde_json::from_slice(&bytes.0).map_err(|_| invalid())
    }
    /// secrets 입력은 magic 뒤 kind(u8)·length(u32 LE)·payload frame, 끝은 실제 EOF다.
    /// kind 1은 authorization JSON, 2는 암호 UTF-8이다. 중복/모르는 frame은 거부한다.
    pub(super) fn framed(mut reader: impl Read, provided: Option<InstallAuthorization>) -> Result<Self> {
        let mut bytes = Bytes(Vec::new());
        reader.by_ref().take((LIMIT + 4096 + 64) as u64 + 1).read_to_end(&mut bytes.0).map_err(|_| invalid())?;
        if bytes.0.len() > LIMIT + 4096 + 64 || !bytes.0.starts_with(MAGIC) {
            return Err(invalid());
        }
        let mut rest = &bytes.0[MAGIC.len()..];
        let mut authorization = provided;
        let mut password = None;
        while !rest.is_empty() {
            let header = rest.get(..5).ok_or_else(invalid)?;
            let kind = header[0];
            let length = u32::from_le_bytes(header[1..].try_into().map_err(|_| invalid())?) as usize;
            let payload = rest.get(5..5usize.checked_add(length).ok_or_else(invalid)?).ok_or_else(invalid)?;
            match kind {
                1 if authorization.is_none() && length <= LIMIT => {
                    authorization = Some(serde_json::from_slice(payload).map_err(|_| invalid())?);
                }
                2 if password.is_none() && length <= 4096 => {
                    let text = std::str::from_utf8(payload).map_err(|_| invalid())?;
                    password = Some(supervisor::Password::new(text.encode_utf16().collect())?);
                }
                _ => return Err(invalid()),
            }
            rest = &rest[5 + length..];
        }
        Ok(Self { authorization: authorization.ok_or_else(invalid)?, password })
    }
    pub(super) fn file(path: &Path) -> Result<InstallAuthorization> {
        let bytes = Bytes(private::read(path, LIMIT as u64).map_err(|_| invalid())?);
        serde_json::from_slice(&bytes.0).map_err(|_| invalid())
    }
    pub(super) fn send(&self, mut writer: impl Write) -> Result<()> {
        fn frame(writer: &mut impl Write, kind: u8, bytes: &[u8]) -> Result<()> {
            let length = u32::try_from(bytes.len()).map_err(|_| invalid())?;
            writer.write_all(&[kind]).map_err(|_| invalid())?;
            writer.write_all(&length.to_le_bytes()).map_err(|_| invalid())?;
            writer.write_all(bytes).map_err(|_| invalid())
        }
        let bytes = Bytes(serde_json::to_vec(&self.authorization).map_err(|_| invalid())?);
        if bytes.0.len() > LIMIT {
            return Err(invalid());
        }
        writer.write_all(MAGIC).map_err(|_| invalid())?;
        frame(&mut writer, 1, &bytes.0)?;
        if let Some(password) = &self.password {
            password.send(&mut writer, |writer, bytes| frame(writer, 2, bytes))?;
        }
        writer.flush().map_err(|_| invalid())
    }
}

#[cfg(windows)]
pub(super) use password::prompt;

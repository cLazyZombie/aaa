//! 전용 입력의 수명과 출력 경계. 디스크·일반 job/DTO에는 비밀을 두지 않는다.
use aaa_proto::{MachineSetupPassword, install::InstallAuthorization};

pub(super) struct Input(pub Vec<u8>);
impl Drop for Input {
    fn drop(&mut self) {
        for byte in &mut self.0 {
            unsafe { std::ptr::write_volatile(byte, 0) };
        }
        std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
    }
}

pub(super) fn framed(auth: &InstallAuthorization, password: Option<&MachineSetupPassword>) -> Result<Input, String> {
    let mut input = Input(b"aaa.install.input.v1\0".to_vec());
    let authorization = serde_json::to_vec(auth).map_err(|_| "보호 승인을 만들지 못했다")?;
    for (kind, bytes) in [(1u8, Some(authorization.as_slice())), (2, password.map(|p| p.0.as_bytes()))] {
        if let Some(bytes) = bytes {
            let n = u32::try_from(bytes.len()).map_err(|_| "보호 입력이 너무 크다")?;
            input.0.push(kind);
            input.0.extend_from_slice(&n.to_le_bytes());
            input.0.extend_from_slice(bytes);
        }
    }
    Ok(input)
}

pub(super) fn scrub(text: &str, password: Option<&MachineSetupPassword>) -> String {
    match password.filter(|p| !p.0.is_empty()) {
        Some(p) => text.replace(&p.0, "[비밀]"),
        None => text.to_string(),
    }
}

/// 원격의 잘못된 오류/사건에도 암호를 되돌려 주지 않는다. JSON escape를 푼 문자열에서 가린다.
pub(super) fn result(
    result: &aaa_proto::install::InstallResult,
    password: Option<&MachineSetupPassword>,
) -> Result<aaa_proto::install::InstallResult, String> {
    fn visit(value: &mut serde_json::Value, password: Option<&MachineSetupPassword>) {
        match value {
            serde_json::Value::String(s) => *s = scrub(s, password),
            serde_json::Value::Array(v) => v.iter_mut().for_each(|v| visit(v, password)),
            serde_json::Value::Object(v) => v.values_mut().for_each(|v| visit(v, password)),
            _ => {}
        }
    }
    let mut value = serde_json::to_value(result).map_err(|_| "설치 결과를 읽지 못했다")?;
    visit(&mut value, password);
    serde_json::from_value(value).map_err(|_| "설치 결과를 읽지 못했다".into())
}

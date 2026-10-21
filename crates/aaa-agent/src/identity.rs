//! agent의 머신 identity(처음 실행 때 만든 machine ID)와 등록한 host의 고정 fingerprint.

use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use aaa_common::util;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentIdentity {
    pub machine_id: String,
    pub created_at_ms: u64,
}

pub fn load_or_create(state: &Path) -> Result<AgentIdentity> {
    let p = state.join("identity.json");
    if let Ok(id) = util::read_json::<AgentIdentity>(&p) {
        return Ok(id);
    }
    util::ensure_private_dir(state)?;
    let id = AgentIdentity { machine_id: util::new_id(), created_at_ms: util::now_ms() };
    util::write_json(&p, &id)?;
    Ok(id)
}

/// 등록(enrollment) 때 확인한 host. 주소가 바뀌어도 fingerprint로 같은 host인지 본다.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostPin {
    pub address: String,
    pub fingerprint: String,
    pub enrolled_at_ms: u64,
}

pub fn load_pin(state: &Path) -> Option<HostPin> {
    util::read_json(&state.join("host.json")).ok()
}

pub fn save_pin(state: &Path, pin: &HostPin) -> Result<()> {
    util::write_json(&state.join("host.json"), pin)?;
    Ok(())
}

/// 승인된 installer가 사용한다. 기존 identity 손상을 새 ID 발급으로 덮지 않는다.
pub fn load_or_create_private(state: &Path) -> Result<AgentIdentity> {
    use aaa_common::private_file as private;
    private::verify_directory(state)?;
    let path = state.join("identity.json");
    match path.symlink_metadata() {
        Ok(_) => {
            let id: AgentIdentity = serde_json::from_slice(&private::read(&path, 65536)?)?;
            anyhow::ensure!(
                id.machine_id.len() == 26
                    && id.machine_id.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit()),
                "설치 machine identity가 올바르지 않습니다"
            );
            Ok(id)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let id = AgentIdentity { machine_id: util::new_id(), created_at_ms: util::now_ms() };
            use std::io::Write;
            let mut file = private::new_file(&path)?;
            file.write_all(&serde_json::to_vec(&id)?)?;
            file.sync_all()?;
            drop(file);
            private::sync_parent(&path)?;
            Ok(id)
        }
        Err(e) => Err(e.into()),
    }
}

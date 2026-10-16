//! agent의 머신 identity(처음 실행 때 만든 machine ID)와 등록한 host의 고정 fingerprint.

use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use aa_common::util;

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

//! 승인 전 bytes와 우리 내구 intent의 bytes만 허용한다. private payload는 공개 journal에 넣지 않는다.
use super::*;
use std::{collections::BTreeMap, io::Write};
pub(super) type Snapshot = BTreeMap<String, Option<String>>;
const IDENTITY: [&str; 3] = ["identity.json", "tls/key.der", "tls/cert.der"];
const PIN: &str = "host.json";
pub(super) fn snapshot(agent: &Path) -> Result<Snapshot> {
    for directory in [agent.to_path_buf(), agent.join("tls")] {
        match directory.symlink_metadata() {
            Ok(_) => private::verify_directory(&directory).map_err(|_| Fault::conflict())?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(Fault::conflict()),
        }
    }
    IDENTITY
        .into_iter()
        .chain([PIN])
        .map(|name| {
            let path = agent.join(name);
            let digest = match path.symlink_metadata() {
                Ok(_) => Some(hash(&private::read(&path, 1024 * 1024).map_err(|_| Fault::conflict())?)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(_) => return Err(Fault::conflict()),
            };
            Ok((name.into(), digest))
        })
        .collect()
}
pub(super) fn digest(snapshot: &Snapshot) -> Result<String> {
    Ok(hash(&aaa_common::install_crypto::canonical(snapshot).map_err(|_| Fault::conflict())?))
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    private::parents(path.parent().ok_or_else(Fault::conflict)?).map_err(Fault::io)?;
    let mut file = private::new_file(path).map_err(|_| Fault::conflict())?;
    file.write_all(bytes).and_then(|_| file.sync_all()).map_err(Fault::io)?;
    drop(file);
    private::sync_parent(path).map_err(Fault::io)
}
fn save(path: &Path, record: &Prepared) -> Result<()> {
    private::write(path, &serde_json::to_vec(record).map_err(|_| Fault::conflict())?).map_err(Fault::io)
}
/// 생성된 새 identity도 실제 위치 쓰기 전에 private staging과 원하는 해시를 내구 기록한다.
pub(super) fn prepare(
    plan: &InstallPlan,
    lease: &Lease,
    path: &Path,
    record: &mut Prepared,
) -> Result<(aaa_agent::identity::AgentIdentity, aaa_common::tls::Identity)> {
    let agent = Path::new(&plan.context.state_dir).join("agent");
    let stage = path.with_extension("payload");
    if record.prepared.is_none() {
        if snapshot(&agent)? != record.before {
            return Err(Fault::conflict());
        }
        if record.before["tls/key.der"].is_some() != record.before["tls/cert.der"].is_some() {
            return Err(required("installation_agent_identity_untrusted"));
        }
        if record.before[PIN].is_some() {
            serde_json::from_slice::<aaa_agent::identity::HostPin>(
                &private::read(&agent.join(PIN), 65536).map_err(Fault::io)?,
            )
            .map_err(|_| Fault::conflict())?;
        }
        private::directory(&stage).map_err(Fault::io)?;
        for name in IDENTITY {
            if let Some(expected) = &record.before[name] {
                let bytes = private::read(&agent.join(name), 1024 * 1024).map_err(Fault::io)?;
                if hash(&bytes) != *expected {
                    return Err(Fault::conflict());
                }
                let target = stage.join(name);
                match target.symlink_metadata() {
                    Ok(_) if private::read(&target, 1024 * 1024).is_ok_and(|old| old == bytes) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => write_new(&target, &bytes)?,
                    _ => return Err(Fault::conflict()),
                }
            }
        }
        aaa_agent::identity::load_or_create_private(&stage)
            .map_err(|_| required("installation_agent_identity_untrusted"))?;
        aaa_common::tls::load_or_create_private(&stage, "aaa-agent")
            .map_err(|_| required("installation_agent_identity_untrusted"))?;
        let mut desired = snapshot(&stage)?;
        desired.insert(PIN.into(), record.before[PIN].clone());
        record.prepared = Some(desired);
        save(path, record)?; // 실제 agent 파일 쓰기보다 먼저.
    }
    lease.valid_at(aaa_common::util::now_ms())?;
    let desired = record.prepared.as_ref().ok_or_else(Fault::conflict)?;
    let current = snapshot(&agent)?;
    if current[PIN] != record.before[PIN] && current[PIN] != record.pin_intent {
        return Err(Fault::conflict());
    }
    for name in IDENTITY {
        if current[name] == desired[name] {
            continue;
        }
        if current[name] != record.before[name] || current[name].is_some() {
            return Err(Fault::conflict());
        }
        let bytes = private::read(&stage.join(name), 1024 * 1024).map_err(Fault::io)?;
        if Some(hash(&bytes)) != desired[name] {
            return Err(Fault::conflict());
        }
        write_new(&agent.join(name), &bytes)?;
    }
    let final_snapshot = snapshot(&agent)?;
    if IDENTITY.iter().any(|name| final_snapshot[*name] != desired[*name]) {
        return Err(Fault::conflict());
    }
    let id = aaa_agent::identity::load_or_create_private(&agent).map_err(|_| Fault::conflict())?;
    let tls = aaa_common::tls::load_or_create_private(&agent, "aaa-agent").map_err(|_| Fault::conflict())?;
    if Some(tls.fingerprint.clone()) != desired["tls/cert.der"] {
        return Err(Fault::conflict());
    }
    record.complete = true;
    record.agent_identity = Some(tls.fingerprint.clone());
    save(path, record)?;
    Ok((id, tls))
}
/// TLS 응답은 쓰기 권한이 아니다. 동의 당시 pin 또는 앞서 기록한 우리 intent만 교체한다.
pub(super) fn commit(plan: &InstallPlan, lease: &Lease, pin: &aaa_agent::identity::HostPin) -> Result<()> {
    lease.valid_at(aaa_common::util::now_ms())?;
    let path = record_path(plan, lease.id());
    let mut record = read_record(plan, lease, &path)?;
    let agent = Path::new(&plan.context.state_dir).join("agent");
    let current = snapshot(&agent)?;
    let desired = record.prepared.as_ref().ok_or_else(Fault::conflict)?;
    if IDENTITY.iter().any(|name| current[*name] != desired[*name])
        || current[PIN] != record.before[PIN] && current[PIN] != record.pin_intent
        || Some(&pin.address) != plan.choices.host.as_ref()
        || record.host_identity.as_deref().is_some_and(|old| old != pin.fingerprint)
    {
        return Err(Fault::conflict());
    }
    let pin_path = agent.join(PIN);
    if current[PIN].is_some() {
        let old: aaa_agent::identity::HostPin =
            serde_json::from_slice(&private::read(&pin_path, 65536).map_err(Fault::io)?)
                .map_err(|_| Fault::conflict())?;
        if old.address == pin.address && old.fingerprint == pin.fingerprint {
            return record_enrolled(plan, lease, &pin.fingerprint);
        }
    }
    let bytes = serde_json::to_vec(pin).map_err(|_| Fault::conflict())?;
    record.pin_intent = Some(hash(&bytes));
    save(&path, &record)?;
    if snapshot(&agent)? != current {
        return Err(Fault::conflict());
    }
    if current[PIN].is_none() {
        write_new(&pin_path, &bytes)?;
    } else {
        private::write(&pin_path, &bytes).map_err(Fault::io)?;
    }
    record_enrolled(plan, lease, &pin.fingerprint)
}

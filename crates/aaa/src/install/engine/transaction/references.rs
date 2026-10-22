//! 같은 사용자의 instance가 공유하는 binary/hook 참조. PID는 소유 증명이 아니다.
use super::*;
use std::collections::BTreeSet;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Origin {
    pub job: PathBuf,
    pub index: usize,
    pub before: Option<String>,
    pub mode: Option<u32>,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Record {
    schema: u32,
    resource: String,
    pub after: String,
    pub mode: u32,
    pub inode: String,
    pub managed: bool,
    pub owners: BTreeSet<String>,
    pub origin: Option<Origin>,
}
pub(super) fn key(state: &Path, manifest: &Manifest) -> String {
    hash(
        &json_bytes(&(state, &manifest.plan.choices.instance, &manifest.plan.context.target_account))
            .expect("고정 타입 JSON"),
    )
}
pub(super) fn path(file: &FileSpec) -> Result<PathBuf> {
    Ok(metadata_parent(file)?.join(format!(".aaa-install-refs-{}.json", resource_key(&file.path))))
}
pub(super) fn read(file: &FileSpec) -> Result<Option<Record>> {
    let bytes = match private::read(&path(file)?, 1024 * 1024) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(Fault::io(e)),
    };
    let record: Record = serde_json::from_slice(&bytes).map_err(|_| Fault::conflict())?;
    if record.schema != 1
        || record.resource != resource_key(&file.path)
        || !digest_ok(&record.after)
        || record.mode & !0o777 != 0
        || record.owners.len() > 1024
        || record.owners.iter().any(|key| !digest_ok(key))
        || record.origin.as_ref().is_some_and(|o| {
            !o.job.is_absolute() || o.index >= 128 || o.before.as_ref().is_some_and(|sha| !digest_ok(sha))
        })
    {
        return Err(Fault::conflict());
    }
    Ok(Some(record))
}
pub(super) fn owned(file: &FileSpec, record: &Record) -> Result<()> {
    if target(&file.path)? != file.path
        || snapshot(&file.path)? != (Some(record.after.clone()), Some(record.mode))
        || identity(&file.path)? != record.inode
    {
        return Err(Fault::conflict());
    }
    Ok(())
}
/// 다른 instance의 권한을 옮기지 않는다. 명시적 repair와 같은 계정/상태의 단독 소유,
/// 승인에 묶인 현재 bytes/mode가 모두 확인된 변경만 다음 세대로 갱신한다.
fn may_update(manifest: &Manifest, file: &FileSpec, record: &Record) -> Result<bool> {
    if manifest.plan.choices.mode != Some(InstallMode::Repair) || record.owners.len() != 1 {
        return Ok(false);
    }
    let state = real_directory(Path::new(&manifest.plan.context.state_dir))?;
    Ok(record.owners.contains(&key(&state, manifest))
        && file.before.as_ref() == Some(&record.after)
        && file.before_mode == Some(record.mode))
}
pub(super) fn preflight(manifest: &Manifest) -> Result<()> {
    for f in &manifest.files {
        if !f.shared {
            continue;
        }
        if let Some(record) = read(f)? {
            if (record.managed || !record.owners.is_empty())
                && (record.after != f.after || record.mode != f.mode)
                && !may_update(manifest, f, &record)?
            {
                return Err(Fault::conflict());
            }
            // repair만 소유 파일의 소실을 복원할 수 있다. 같은 내용의 외부 교체도 소유 증명이 아니다.
            if f.before.is_some() && (record.managed || !record.owners.is_empty()) {
                owned(f, &record)?;
            }
        } else if f.kind == Kind::Binary && f.before.is_some() && f.before.as_ref() != Some(&f.after) {
            return Err(Fault::conflict());
        }
    }
    Ok(())
}
pub(super) fn register(tx: &Transaction) -> Result<()> {
    for (index, f) in tx.manifest.files.iter().enumerate() {
        if !f.shared {
            continue;
        }
        let inode = identity(&f.path)?;
        if snapshot(&f.path)? != (Some(f.after.clone()), Some(f.mode))
            || tx.journal.entries[index].inode.as_ref().is_some_and(|expected| expected != &inode)
        {
            return Err(Fault::conflict());
        }
        let proof = tx.journal.entries[index].inode.as_ref() == Some(&inode);
        let created = proof && (f.before.is_none() || f.kind == Kind::Hook && f.before.as_ref() != Some(&f.after));
        let mut record = read(f)?.filter(|r| r.managed || !r.owners.is_empty()).unwrap_or(Record {
            schema: 1,
            resource: resource_key(&f.path),
            after: f.after.clone(),
            mode: f.mode,
            inode: inode.clone(),
            managed: created,
            owners: BTreeSet::new(),
            origin: None,
        });
        if record.after != f.after || record.mode != f.mode {
            if !proof || !may_update(&tx.manifest, f, &record)? || f.reference_before.as_ref() != Some(&record) {
                return Err(Fault::conflict());
            }
            record.after = f.after.clone();
            record.mode = f.mode;
        }
        if created {
            record.managed = true;
        }
        if proof {
            record.inode = inode;
        }
        if created && record.origin.is_none() {
            record.origin = Some(Origin { job: tx.dir.clone(), index, before: f.before.clone(), mode: f.before_mode });
        }
        record.owners.insert(key(&tx.state, &tx.manifest));
        private::write(&path(f)?, &json_bytes(&record)?).map_err(Fault::io)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::engine::transaction::tests::{Lab, tree};

    fn request(lab: &Lab, state: &str, after: &[u8], repair: bool) -> Request {
        let path = lab.0.join("hooks/hooks.json");
        let before = std::fs::read(&path).ok();
        let hook = prepare::proposal(path, Kind::Hook, before, after.to_vec(), None, false, true).unwrap();
        let mut request = lab.request(state, vec![hook]);
        if repair {
            request.plan.choices.mode = Some(InstallMode::Repair);
            request.plan = aaa_config::install::plan(request.plan.choices, request.plan.context).unwrap();
        }
        request
    }
    #[test]
    fn explicit_repair_updates_only_its_sole_shared_hook_and_preserves_the_original() {
        let lab = Lab::new();
        let hook = lab.put("hooks/hooks.json", b"user hooks");
        let mut first = Transaction::create(request(&lab, "one", b"user hooks + old aaa", false)).unwrap();
        first.run().unwrap();
        drop(first);
        let mut updated = Transaction::create(request(&lab, "one", b"user hooks + new aaa", true)).unwrap();
        assert_eq!(updated.run().unwrap().exit_code, 0);
        assert_eq!(std::fs::read(&hook).unwrap(), b"user hooks + new aaa");
        let state = updated.state.clone();
        let id = updated.journal.id.clone();
        updated.uninstall(false).unwrap();
        assert_eq!(std::fs::read(&hook).unwrap(), b"user hooks", "새 installer 제거도 최초 사용자 hook을 복원한다");
        drop(updated);
        assert!(read_journal(&state, &id, Some("lab")).unwrap().phase == Phase::Uninstalled);
    }
    #[test]
    fn repair_never_changes_another_or_a_shared_instances_hook() {
        for shared in [false, true] {
            let lab = Lab::new();
            let hook = lab.put("hooks/hooks.json", b"user hooks");
            let mut other = Transaction::create(request(&lab, "other", b"user hooks + aaa", false)).unwrap();
            other.run().unwrap();
            drop(other);
            if shared {
                let mut ours = Transaction::create(request(&lab, "ours", b"user hooks + aaa", false)).unwrap();
                ours.run().unwrap();
                drop(ours);
            }
            let attempt = request(&lab, "ours", b"user hooks + changed aaa", true);
            let before = tree(&lab.0);
            assert!(Transaction::create(attempt).is_err());
            assert_eq!(std::fs::read(&hook).unwrap(), b"user hooks + aaa");
            assert_eq!(tree(&lab.0), before, "다른/공유 owner를 바꾸거나 참조를 지우지 않는다");
        }
    }
}

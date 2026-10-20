//! 같은 사용자의 instance가 공유하는 binary/hook 참조. PID는 소유 증명이 아니다.
use super::*;
use std::collections::BTreeSet;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Origin {
    pub job: PathBuf,
    pub index: usize,
    pub before: Option<String>,
    pub mode: Option<u32>,
}
#[derive(Clone, Serialize, Deserialize)]
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
pub(super) fn path(file: &FileSpec) -> PathBuf {
    file.path.parent().unwrap().join(format!(".aaa-install-refs-{}.json", resource_key(&file.path)))
}
pub(super) fn read(file: &FileSpec) -> Result<Option<Record>> {
    let bytes = match private::read(&path(file), 1024 * 1024) {
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
pub(super) fn preflight(manifest: &Manifest) -> Result<()> {
    for f in &manifest.files {
        if !f.shared {
            continue;
        }
        if let Some(record) = read(f)? {
            if (record.managed || !record.owners.is_empty()) && (record.after != f.after || record.mode != f.mode) {
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
            return Err(Fault::conflict());
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
        private::write(&path(f), &json_bytes(&record)?).map_err(Fault::io)?;
    }
    Ok(())
}

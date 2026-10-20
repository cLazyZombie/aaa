//! 복구는 저장된 payload와 현재 소유 상태를 대조한다. 버전 갱신은 기존 update의 책임이다.
use super::*;

impl Transaction {
    fn recover(state: &Path, id: &str, instance: Option<&str>, reinstall: bool) -> Result<Self> {
        let prior = Self::resume(state, id, instance)?;
        if prior.journal.phase != Phase::Configured {
            return Err(Fault::conflict());
        }
        let owner = references::key(&prior.state, &prior.manifest);
        let mut files = Vec::new();
        for (i, f) in prior.manifest.files.iter().enumerate() {
            let (current, permissions) = snapshot(&f.path)?;
            let same = current.as_ref() == Some(&f.after) && permissions == Some(f.mode);
            let force = reinstall && f.kind == Kind::Binary;
            if force {
                let record = references::read(f)?.ok_or_else(Fault::conflict)?;
                if !record.managed || !record.owners.contains(&owner) {
                    return Err(Fault::conflict());
                }
                if current.is_some() {
                    references::owned(f, &record)?;
                }
            }
            // reinstall은 사용자가 바꾼 TOML/프로젝트를 다시 쓰지 않는다. hook/binary 손상은 충돌로 남긴다.
            let preserve_current = reinstall && f.preserve && current.is_some();
            if !same && current.is_some() && !preserve_current {
                return Err(Fault::conflict());
            }
            let bytes = if preserve_current {
                std::fs::read(&f.path).map_err(Fault::io)?
            } else {
                let bytes =
                    private::read(&prior.dir.join(format!("payload-{i}")), 1024 * 1024 * 1024).map_err(Fault::io)?;
                if hash(&bytes) != f.after {
                    return Err(Fault::conflict());
                }
                bytes
            };
            files.push(Proposed {
                path: f.path.clone(),
                kind: f.kind,
                before: current.clone(),
                before_mode: permissions,
                before_inode: if current.is_some() { Some(identity(&f.path)?) } else { None },
                force,
                bytes,
                mode: Some(if preserve_current { permissions.unwrap() } else { f.mode }),
                preserve: f.preserve,
                shared: f.shared,
            });
        }
        let mut plan = prior.manifest.plan.clone();
        plan.choices.mode = Some(if reinstall { InstallMode::Reinstall } else { InstallMode::Repair });
        let request =
            Request { plan, files, root: prior.manifest.root.clone(), directories: prior.manifest.directories.clone() };
        // 새로운 작업도 전체 자원 잠금을 다시 얻고 위에서 읽은 baseline을 재검사한다.
        drop(prior);
        Self::create(request)
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{Lab, tree};
    use super::*;

    #[test]
    fn repair_missing_owned_files_and_reinstall_preserve_user_data() {
        let lab = Lab::new();
        let plan = lab.plan("state");
        let binary = PathBuf::from(&plan.context.bin_path);
        let cfg = PathBuf::from(&plan.context.config_dir).join("agent.toml");
        let mut tx = Transaction::create(prepare::install(plan).unwrap()).unwrap();
        tx.run().unwrap();
        let state = tx.state.clone();
        let first = tx.journal.id.clone();
        drop(tx);
        std::fs::remove_file(&binary).unwrap();
        let mut repair = Transaction::recover(&state, &first, Some("lab"), false).unwrap();
        assert_eq!(repair.run().unwrap().status, InstallStatus::Configured);
        assert_eq!(std::fs::read(&binary).unwrap(), b"verified executable bytes");
        let repaired = repair.journal.id.clone();
        drop(repair);
        let original = std::fs::read(&cfg).unwrap();
        let mut changed = original.clone();
        changed.extend_from_slice(b"\n# user edit preserved\n");
        private::write(&cfg, &changed).unwrap();
        let stable = lab.put("state/identity", b"private identity and pin");
        let db = lab.put("state/history.db", b"user records");
        let repo = lab.put("state-work/dirty.txt", b"uncommitted work");
        let before_inode = identity(&binary).unwrap();
        let mut reinstall = Transaction::recover(&state, &repaired, Some("lab"), true).unwrap();
        reinstall.run().unwrap();
        assert_ne!(identity(&binary).unwrap(), before_inode, "같은 버전 실행 파일도 다시 배치한다");
        assert_eq!(std::fs::read(&cfg).unwrap(), changed);
        assert_eq!(std::fs::read(stable).unwrap(), b"private identity and pin");
        assert_eq!(std::fs::read(db).unwrap(), b"user records");
        assert_eq!(std::fs::read(repo).unwrap(), b"uncommitted work");
        let id = reinstall.journal.id.clone();
        drop(reinstall);
        std::fs::write(&binary, b"external binary edit").unwrap();
        let before = tree(&lab.0);
        assert_eq!(Transaction::recover(&state, &id, Some("lab"), true).err(), Some(Fault::conflict()));
        assert_eq!(tree(&lab.0), before, "소유 증명이 다른 실행 파일을 덮지 않는다");
    }
}

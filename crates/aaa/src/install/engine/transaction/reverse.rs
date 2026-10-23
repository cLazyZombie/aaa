//! 제거/rollback도 작업 전 intent, stage inode, 파일 완료, 참조 완료를 차례로 내구화한다.
//! 완료된 파일 작업은 재개 때 반복하지 않는다. 원본과 다른 바이트·권한·inode는 보존한다.
use super::*;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Action {
    index: usize,
    before: Option<String>,
    before_mode: Option<u32>,
    before_inode: Option<String>,
    after: Option<String>,
    after_mode: Option<u32>,
    origin: Option<references::Origin>,
    record_before: Option<String>,
    record_after: Option<references::Record>,
    change: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reverse {
    schema: u32,
    rollback: bool,
    actions: Vec<Action>,
}
fn backup(origin: &references::Origin, expected: &str) -> Result<Vec<u8>> {
    let bytes =
        private::read(&origin.job.join(format!("backup-{}", origin.index)), 1024 * 1024 * 1024).map_err(Fault::io)?;
    if hash(&bytes) != expected {
        return Err(Fault::conflict());
    }
    Ok(bytes)
}
impl Transaction {
    fn begin_reverse(&mut self, rollback: bool, keep_binary: bool) -> Result<()> {
        if matches!(self.journal.phase, Phase::RollingBack | Phase::Uninstalling) {
            return Ok(());
        }
        if rollback && self.journal.phase != Phase::Applying || !rollback && self.journal.phase != Phase::Configured {
            return Err(Fault::conflict());
        }
        let owner = references::key(&self.state, &self.manifest);
        let mut actions = Vec::new();
        for (index, f) in self.manifest.files.iter().enumerate().rev() {
            let entry = &self.journal.entries[index];
            let record = if f.shared { references::read(f)? } else { None };
            let registered = record.as_ref().is_some_and(|r| r.owners.contains(&owner));
            if !rollback && !registered || rollback && entry.inode.is_none() && !registered {
                continue;
            }
            let (before, before_mode) = snapshot(&f.path)?;
            let before_inode = if before.is_some() { Some(identity(&f.path)?) } else { None };
            let mut after = before.clone();
            let mut after_mode = before_mode;
            let mut origin = None;
            let record_before = record.as_ref().map(json_bytes).transpose()?.map(|v| hash(&v));
            let mut record_after = record;
            let mut change = false;
            if rollback {
                let installed = before == Some(f.after.clone())
                    && before_mode == Some(f.mode)
                    && entry.inode.is_some()
                    && entry.inode == before_inode;
                let untouched = before == f.before && before_mode == f.before_mode && before_inode == f.before_inode;
                if !installed && !untouched {
                    return Err(Fault::conflict());
                }
                if installed {
                    origin = Some(references::Origin {
                        job: self.dir.clone(),
                        index,
                        before: f.before.clone(),
                        mode: f.before_mode,
                    });
                    after = f.before.clone();
                    after_mode = f.before_mode;
                    change = true;
                }
                record_after = f.reference_before.clone();
            } else if let Some(record) = record_after.as_mut() {
                record.owners.remove(&owner);
                if record.owners.is_empty() && record.managed && !f.preserve && !(keep_binary && f.kind == Kind::Binary)
                {
                    references::owned(f, record)?;
                    origin = record.origin.clone();
                    after = origin.as_ref().and_then(|o| o.before.clone());
                    after_mode = origin.as_ref().and_then(|o| o.mode);
                    change = true;
                }
                if record.owners.is_empty() {
                    record.managed = false;
                }
            }
            if let (Some(source), Some(sha)) = (&origin, &after) {
                backup(source, sha)?;
            }
            actions.push(Action {
                index,
                before,
                before_mode,
                before_inode,
                after,
                after_mode,
                origin,
                record_before,
                record_after,
                change,
            });
        }
        self.journal.rollback_remaining.clear();
        if rollback {
            if self.manifest.root.as_ref().is_some_and(|root| root.path.exists()) {
                self.journal.rollback_remaining.push("work_root_preserved".into());
            }
            if self.journal.entries.iter().enumerate().any(|(i, entry)| {
                entry.inode.is_none()
                    && self
                        .stage_path(i)
                        .symlink_metadata()
                        .as_ref()
                        .inspect_err(|error| {
                            aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")
                        })
                        .is_ok()
            }) {
                self.journal.rollback_remaining.push("staging_unverified".into());
            }
        }
        let reverse = Reverse { schema: 1, rollback, actions };
        let bytes = json_bytes(&reverse)?;
        private::write(&self.dir.join("reverse.json"), &bytes).map_err(Fault::io)?;
        self.journal.reverse_sha256 = Some(hash(&bytes));
        self.journal.reverse = reverse
            .actions
            .iter()
            .map(|a| ReverseEntry { index: a.index, state: Step::Pending, inode: None, changed: a.change })
            .collect();
        self.journal.phase = if rollback { Phase::RollingBack } else { Phase::Uninstalling };
        self.save()
    }
    fn reverse_plan(&self) -> Result<Reverse> {
        let bytes = private::read(&self.dir.join("reverse.json"), 2 * 1024 * 1024).map_err(Fault::io)?;
        if Some(hash(&bytes)) != self.journal.reverse_sha256 {
            return Err(Fault::conflict());
        }
        let reverse: Reverse = serde_json::from_slice(&bytes)
            .map_err(|source_error| aaa_common::diagnostic::context(source_error, Fault::conflict()))?;
        if reverse.schema != 1
            || reverse.actions.len() != self.journal.reverse.len()
            || reverse
                .actions
                .iter()
                .zip(&self.journal.reverse)
                .any(|(a, e)| a.index != e.index || a.index >= self.manifest.files.len() || a.change != e.changed)
            || reverse.rollback != (self.journal.phase == Phase::RollingBack)
        {
            return Err(Fault::conflict());
        }
        Ok(reverse)
    }
    fn reverse_stage_path(&self, index: usize) -> PathBuf {
        self.manifest.files[index].path.parent().unwrap().join(format!(".aaa-revert-{}-{index}.tmp", self.journal.id))
    }
    fn reverse_stage(&mut self, slot: usize, action: &Action) -> Result<()> {
        let f = &self.manifest.files[action.index];
        if snapshot(&f.path)? != (action.before.clone(), action.before_mode)
            || action.before_inode.as_ref().is_some_and(|ino| {
                identity(&f.path)
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
                    .as_ref()
                    != Some(ino)
            })
        {
            return Err(Fault::conflict());
        }
        if action.change
            && let Some(after) = &action.after
        {
            let bytes = backup(action.origin.as_ref().ok_or_else(Fault::conflict)?, after)?;
            let stage = self.reverse_stage_path(action.index);
            if !stage.exists() {
                private::write(&stage, &bytes).map_err(Fault::io)?;
            }
            if snapshot(&stage)?.0 != Some(after.clone()) {
                return Err(Fault::conflict());
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(
                    &stage,
                    std::fs::Permissions::from_mode(action.after_mode.ok_or_else(Fault::conflict)?),
                )
                .map_err(Fault::io)?;
            }
            private::sync_file(&stage).map_err(Fault::io)?;
            private::sync_parent(&stage).map_err(Fault::io)?;
            self.journal.reverse[slot].inode = Some(identity(&stage)?);
        }
        self.journal.reverse[slot].state = Step::Staged;
        self.save()
    }
    fn reverse_commit(&mut self, slot: usize, action: &Action) -> Result<()> {
        let f = &self.manifest.files[action.index];
        if target(&f.path)? != f.path {
            return Err(Fault::conflict());
        }
        if action.change {
            let current = snapshot(&f.path)?;
            let already = current == (action.after.clone(), action.after_mode)
                && (action.after.is_none()
                    || self.journal.reverse[slot].inode.as_ref()
                        == identity(&f.path)
                            .inspect_err(|error| {
                                aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                            })
                            .ok()
                            .as_ref());
            if !already {
                if current != (action.before.clone(), action.before_mode)
                    || action.before_inode.as_ref()
                        != identity(&f.path)
                            .inspect_err(|error| {
                                aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                            })
                            .ok()
                            .as_ref()
                {
                    return Err(Fault::conflict());
                }
                if let Some(after) = &action.after {
                    let stage = self.reverse_stage_path(action.index);
                    if snapshot(&stage)? != (Some(after.clone()), action.after_mode)
                        || self.journal.reverse[slot].inode.as_ref() != Some(&identity(&stage)?)
                    {
                        return Err(Fault::conflict());
                    }
                    replace(&stage, &f.path, true)?;
                } else {
                    std::fs::remove_file(&f.path).map_err(Fault::io)?;
                }
                private::sync_parent(&f.path).map_err(Fault::io)?;
            }
        }
        self.journal.reverse[slot].state = Step::Applied;
        self.save()
    }
    fn discard_stage(&self, index: usize) -> Result<()> {
        let Some(expected) = &self.journal.entries[index].inode else { return Ok(()) };
        let stage = self.stage_path(index);
        match stage.symlink_metadata() {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(Fault::io(e)),
            Ok(_) => {}
        }
        let file = &self.manifest.files[index];
        if identity(&stage)? != *expected || snapshot(&stage)? != (Some(file.after.clone()), Some(file.mode)) {
            return Err(Fault::conflict());
        }
        std::fs::remove_file(&stage).map_err(Fault::io)?;
        private::sync_parent(&stage).map_err(Fault::io)
    }
    pub(super) fn reverse_advance(&mut self) -> Result<bool> {
        if matches!(self.journal.phase, Phase::Uninstalled | Phase::RolledBack) {
            return Ok(false);
        }
        let reverse = self.reverse_plan()?;
        for (slot, action) in reverse.actions.iter().enumerate() {
            match self.journal.reverse[slot].state {
                Step::Pending => self.reverse_stage(slot, action)?,
                Step::Staged => self.reverse_commit(slot, action)?,
                Step::Applied => {
                    if reverse.rollback {
                        self.discard_stage(action.index)?;
                    }
                    let f = &self.manifest.files[action.index];
                    if f.shared {
                        let current = references::read(f)?.as_ref().map(json_bytes).transpose()?;
                        let mut record = action.record_after.clone();
                        if reverse.rollback
                            && action.change
                            && let Some(record) = record.as_mut()
                            && record.managed
                        {
                            record.inode = self.journal.reverse[slot].inode.clone().ok_or_else(Fault::conflict)?;
                        }
                        let desired = record.as_ref().map(json_bytes).transpose()?;
                        if current != desired {
                            if current.as_ref().map(|v| hash(v)) != action.record_before {
                                return Err(Fault::conflict());
                            }
                            if let Some(bytes) = &desired {
                                private::write(&references::path(f)?, bytes).map_err(Fault::io)?;
                            } else {
                                std::fs::remove_file(references::path(f)?).map_err(Fault::io)?;
                                private::sync_parent(&references::path(f)?).map_err(Fault::io)?;
                            }
                        }
                    }
                    self.journal.reverse[slot].state = Step::Reverted;
                    if reverse.rollback {
                        self.journal.entries[action.index].state = Step::Reverted;
                    }
                    self.save()?;
                }
                Step::Reverted => continue,
            }
            return Ok(true);
        }
        self.journal.phase = if reverse.rollback { Phase::RolledBack } else { Phase::Uninstalled };
        self.save()?;
        Ok(false)
    }
    pub(super) fn uninstall(&mut self, keep_binary: bool) -> Result<InstallResult> {
        if self.journal.phase == Phase::Uninstalled {
            return Ok(self.journal.result());
        }
        self.begin_reverse(false, keep_binary)?;
        self.run()
    }
    pub(super) fn rollback(&mut self) -> Result<InstallResult> {
        if self.journal.phase == Phase::RolledBack {
            return Ok(self.journal.result());
        }
        self.begin_reverse(true, true)?;
        self.run()
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{Lab, tree};
    use super::*;

    fn shared(lab: &Lab, name: &str, kind: Kind, bytes: &[u8]) -> Proposed {
        let mut f = lab.edit(name, bytes);
        f.kind = kind;
        f.preserve = false;
        f.shared = true;
        f
    }
    #[test]
    fn uninstall_shared_last_reference_resumes_deletion_and_preserves_data() {
        let lab = Lab::new();
        let hook = lab.put("hooks/config", b"user hook original");
        let mut first = Transaction::create(lab.request(
            "one",
            vec![
                shared(&lab, "bin/aaa", Kind::Binary, b"binary"),
                shared(&lab, "hooks/config", Kind::Hook, b"user hook original + aaa"),
                lab.edit("config/agent.toml", b"keep user configuration"),
            ],
        ))
        .unwrap();
        first.run().unwrap();
        let state1 = first.state.clone();
        let id1 = first.journal.id.clone();
        drop(first);
        let mut second = Transaction::create(lab.request(
            "two",
            vec![
                shared(&lab, "bin/aaa", Kind::Binary, b"binary"),
                shared(&lab, "hooks/config", Kind::Hook, b"user hook original + aaa"),
            ],
        ))
        .unwrap();
        second.run().unwrap();
        let binary = lab.0.join("bin/aaa");
        let inode = identity(&binary).unwrap();
        let state2 = second.state.clone();
        let id2 = second.journal.id.clone();
        drop(second);
        let mut first = Transaction::resume(&state1, &id1, Some("lab")).unwrap();
        first.uninstall(false).unwrap();
        assert_eq!(identity(&binary).unwrap(), inode, "다른 instance의 공유 binary 보존");
        assert_eq!(std::fs::read(&hook).unwrap(), b"user hook original + aaa");
        assert_eq!(std::fs::read(lab.0.join("config/agent.toml")).unwrap(), b"keep user configuration");
        drop(first);
        let mut second = Transaction::resume(&state2, &id2, Some("lab")).unwrap();
        second.begin_reverse(false, false).unwrap();
        let reverse = second.reverse_plan().unwrap();
        let slot = reverse.actions.iter().position(|a| a.after.is_none() && a.change).unwrap();
        // 앞의 hook 복원 완료 후 binary 삭제 intent를 기록한다.
        while second.journal.reverse[..slot].iter().any(|e| e.state != Step::Reverted) {
            second.reverse_advance().unwrap();
        }
        second.reverse_stage(slot, &reverse.actions[slot]).unwrap();
        std::fs::remove_file(&binary).unwrap();
        private::sync_parent(&binary).unwrap();
        let state2 = second.state.clone();
        let id2 = second.journal.id.clone();
        drop(second);
        let mut resumed = Transaction::resume(&state2, &id2, Some("lab")).unwrap();
        assert_eq!(resumed.run().unwrap().exit_code, 0);
        assert!(!binary.exists());
        assert_eq!(std::fs::read(&hook).unwrap(), b"user hook original");
        // 완료 후 같은 ID의 재호출은 외부에서 새로 만든 파일을 지우지 않는다.
        lab.put("bin/aaa", b"new unrelated file");
        let before = tree(&lab.0);
        resumed.uninstall(false).unwrap();
        assert_eq!(tree(&lab.0), before);
    }
    #[test]
    fn uninstall_conflict_and_keep_binary_never_erase_user_changes() {
        let lab = Lab::new();
        lab.put("hooks/config", b"original");
        let mut tx = Transaction::create(lab.request(
            "one",
            vec![
                shared(&lab, "bin/aaa", Kind::Binary, b"binary"),
                shared(&lab, "hooks/config", Kind::Hook, b"original + aaa"),
            ],
        ))
        .unwrap();
        tx.run().unwrap();
        lab.put("hooks/config", b"original + aaa + user addition");
        let before = tree(&lab.0);
        assert_eq!(tx.uninstall(false).err(), Some(Fault::conflict()));
        assert_eq!(tree(&lab.0), before, "부분 제거 전에 전체 공유 항목을 대조한다");
        // 별도 작업에서는 keep-binary도 자기 참조를 해제하고 binary를 보존한다.
        let lab2 = Lab::new();
        let mut tx =
            Transaction::create(lab2.request("one", vec![shared(&lab2, "bin/aaa", Kind::Binary, b"binary")])).unwrap();
        tx.run().unwrap();
        tx.uninstall(true).unwrap();
        let record = references::read(&tx.manifest.files[0]).unwrap().unwrap();
        assert!(record.owners.is_empty() && !record.managed);
        assert_eq!(std::fs::read(lab2.0.join("bin/aaa")).unwrap(), b"binary");
    }
    #[test]
    fn rollback_restores_only_owned_commits_and_resumes_after_restore_rename() {
        let lab = Lab::new();
        let file = lab.put("config/value", b"original");
        let mut tx = Transaction::create(
            lab.request("one", vec![lab.edit("config/value", b"installed"), lab.edit("config/new", b"pending")]),
        )
        .unwrap();
        tx.stage(0).unwrap();
        tx.commit(0).unwrap();
        tx.begin_reverse(true, true).unwrap();
        let reverse = tx.reverse_plan().unwrap();
        let slot = reverse.actions.iter().position(|a| a.index == 0).unwrap();
        tx.reverse_stage(slot, &reverse.actions[slot]).unwrap();
        replace(&tx.reverse_stage_path(0), &file, true).unwrap();
        let inode = identity(&file).unwrap();
        let state = tx.state.clone();
        let id = tx.journal.id.clone();
        drop(tx);
        let mut resumed = Transaction::resume(&state, &id, Some("lab")).unwrap();
        let result = resumed.rollback().unwrap();
        assert_eq!(result.rollback.state, InstallRollbackState::Completed);
        assert_eq!(std::fs::read(&file).unwrap(), b"original");
        assert_eq!(identity(&file).unwrap(), inode, "복원 rename 뒤 중단은 다시 쓰지 않는다");
        assert!(!lab.0.join("config/new").exists());
        assert!(resumed.dir.join("backup-0").is_file());
        let roots = Lab::new();
        let plan = roots.plan("state");
        let root = PathBuf::from(plan.choices.work_root.clone().unwrap());
        let mut tx = Transaction::create(prepare::install(plan).unwrap()).unwrap();
        while tx.journal.root != Some(Step::Applied) {
            assert!(tx.advance().unwrap());
        }
        let dirty = roots.put("state-work/dirty", b"uncopied work");
        let result = tx.rollback().unwrap();
        assert_eq!(result.rollback.state, InstallRollbackState::Partial);
        assert!(result.rollback.remaining.iter().any(|code| code == "work_root_preserved"));
        assert_eq!(std::fs::read(dirty).unwrap(), b"uncopied work");
        assert!(root.join(".aaa-root-owner.json").exists());
    }
}

#[cfg(test)]
mod staging_tests {
    use super::super::tests::Lab;
    use super::*;

    #[test]
    fn rollback_cleans_only_proven_staging_and_reports_unproven_leftovers() {
        let lab = Lab::new();
        let original = lab.put("config/value", b"original");
        let mut tx = Transaction::create(lab.request("state", vec![lab.edit("config/value", b"installer")])).unwrap();
        tx.stage(0).unwrap();
        let stage = tx.stage_path(0);
        assert!(stage.is_file());
        assert_eq!(tx.rollback().unwrap().rollback.state, InstallRollbackState::Completed);
        assert!(!stage.exists(), "소유 stage를 rollback이 정리한다");
        assert_eq!(std::fs::read(original).unwrap(), b"original");

        let unknown = Lab::new();
        let mut tx =
            Transaction::create(unknown.request("state", vec![unknown.edit("config/new", b"installer")])).unwrap();
        let stage = tx.stage_path(0);
        private::parents(stage.parent().unwrap()).unwrap();
        private::write(&stage, b"external temporary file").unwrap();
        let result = tx.rollback().unwrap();
        assert_eq!(result.rollback.state, InstallRollbackState::Partial);
        assert!(result.rollback.remaining.iter().any(|code| code == "staging_unverified"));
        assert_eq!(std::fs::read(&stage).unwrap(), b"external temporary file");
        assert!(!unknown.0.join("config/new").exists());
        let edited = Lab::new();
        let original = edited.put("config/value", b"original");
        let mut tx =
            Transaction::create(edited.request("state", vec![edited.edit("config/value", b"installer")])).unwrap();
        tx.stage(0).unwrap();
        let stage = tx.stage_path(0);
        std::fs::write(&stage, b"external stage edit").unwrap();
        assert_eq!(tx.rollback().err(), Some(Fault::conflict()));
        assert_eq!(std::fs::read(stage).unwrap(), b"external stage edit");
        assert_eq!(std::fs::read(original).unwrap(), b"original");
    }
}

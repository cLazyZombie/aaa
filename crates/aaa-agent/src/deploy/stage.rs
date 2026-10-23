//! 빌드 후보 읽기와 rollout별 조각 저장. 설치 경로는 입력으로 받지 않는다.
use super::*;
use aaa_proto::{AgentRequest, AgentResponse, B64, DEPLOY_CHUNK, DEPLOY_MAX_BYTES, DeployStageView};
use std::io::{Read, Seek, SeekFrom, Write};

fn valid_sha(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
impl Manager {
    fn stage_path(&self, id: &str, suffix: &str) -> Result<PathBuf, ErrorBody> {
        if !valid(id) {
            return Err(error("rollout ID 형식이 아니다"));
        }
        Ok(self.root.join(format!("{id}.{suffix}")))
    }
    fn stage_meta(&self, id: &str) -> Result<DeployStageView, ErrorBody> {
        let bytes = std::fs::read(self.stage_path(id, "stage.json")?).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ErrorBody::new(ErrorCode::NotFound, "전송 기록이 없다")
            } else {
                error(e)
            }
        })?;
        serde_json::from_slice(&bytes).map_err(error)
    }
    fn save_stage(&self, stage: &DeployStageView) -> Result<(), ErrorBody> {
        aaa_config::profile_edit::write_atomic(
            &self.stage_path(&stage.rollout, "stage.json")?,
            &serde_json::to_vec(stage).map_err(error)?,
        )
        .map_err(error)
    }
    fn stage_status(&self, id: &str) -> Result<DeployStageView, ErrorBody> {
        let mut stage = self.stage_meta(id)?;
        let bin = self.stage_path(id, "bin")?;
        let path = if bin.exists() { bin } else { self.stage_path(id, "part")? };
        stage.offset = std::fs::metadata(path).map_err(error)?.len();
        if stage.offset > stage.total || (stage.complete && stage.offset != stage.total) {
            return Err(error("전송 파일 크기가 기록과 다르다"));
        }
        Ok(stage)
    }
    fn remove_stage(&self, id: &str) -> Result<(), ErrorBody> {
        for suffix in ["part", "bin", "stage.json"] {
            match std::fs::remove_file(self.stage_path(id, suffix)?) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(error(e)),
            }
        }
        Ok(())
    }
    fn prune_stages(&self, keep: &str, partial_only: bool) -> Result<(), ErrorBody> {
        for entry in std::fs::read_dir(&self.root).map_err(error)? {
            let entry = entry.map_err(error)?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(id) = name.strip_suffix(".stage.json") else { continue };
            if id != keep && valid(id) && (!partial_only || !self.stage_meta(id)?.complete) {
                self.remove_stage(id)?;
            }
        }
        Ok(())
    }
    pub(super) fn staged_action(&self, project: &str, id: &str, dry_run: bool) -> Result<DeployAction, ErrorBody> {
        let _guard = self.stage_gate.lock().unwrap();
        let stage = self.stage_status(id)?;
        if stage.project != project || !stage.complete {
            return Err(error("완료된 자기 프로젝트 후보가 아니다"));
        }
        let path = self.stage_path(id, "bin")?;
        if sha(&path)? != stage.sha256 {
            return Err(error("전송 후보 SHA-256 불일치"));
        }
        Ok(DeployAction::CheckedUpdate { from: path.to_string_lossy().into_owned(), sha256: stage.sha256, dry_run })
    }
    pub(crate) fn transport(&self, exe: &Path, req: AgentRequest) -> Result<AgentResponse, ErrorBody> {
        let _guard = self.stage_gate.lock().unwrap();
        match req {
            AgentRequest::DeployFetch { path, offset, len, sha256 } => fetch(&path, offset, len, &sha256),
            AgentRequest::DeployCandidate { path } => {
                // 실행기의 staging 파일 이름은 agent PID 하나다: 도는 배포 작업이 있으면 거부하고, 점검하는 동안은 새 작업을 받지 않는다.
                // 작업 gate는 표지만 세우는 동안 쥔다(self-check가 길어도 async `deploy_start`를 붙잡지 않는다). 전송 끝의 dry-run과는
                // stage_gate로 이미 차례를 지킨다. 거절 글에는 작업 ID를 싣지 않는다(웹으로 간다)
                {
                    let _jobs = self.gate.lock().unwrap();
                    if self.unfinished()?.is_some() {
                        return Err(error("배포 작업이 진행 중이다. 끝난 뒤 다시 점검한다"));
                    }
                    self.inspecting.store(true, std::sync::atomic::Ordering::SeqCst);
                }
                struct Done<'a>(&'a std::sync::atomic::AtomicBool);
                impl Drop for Done<'_> {
                    fn drop(&mut self) {
                        self.0.store(false, std::sync::atomic::Ordering::SeqCst);
                    }
                }
                let _done = Done(&self.inspecting);
                let execute = (*self.executor.lock().unwrap()).ok_or_else(|| error("배포 실행기가 없다"))?;
                inspect(exe, execute, &path)
            }
            AgentRequest::DeployStageStatus { rollout } => {
                self.stage_status(&rollout).map(|stage| AgentResponse::DeployStage { stage })
            }
            AgentRequest::DeployStageClean { rollout, keep } => {
                if !valid(&rollout) {
                    return Err(error("rollout ID 형식이 아니다"));
                }
                if keep {
                    if !self.stage_status(&rollout)?.complete {
                        return Err(error("미완료 후보는 보존 대상으로 정리하지 않는다"));
                    }
                    self.prune_stages(&rollout, false)?;
                } else {
                    self.remove_stage(&rollout)?;
                }
                Ok(AgentResponse::Done)
            }
            AgentRequest::DeployStage { rollout, project, offset, data, eof, sha256, total } => {
                if !valid_sha(&sha256)
                    || total == 0
                    || total > DEPLOY_MAX_BYTES
                    || data.0.len() > DEPLOY_CHUNK
                    || offset.checked_add(data.0.len() as u64).is_none_or(|n| n > total)
                    || (data.0.is_empty() && !eof)
                {
                    return Err(error("조각 크기·SHA-256·전체 크기를 확인한다"));
                }
                let mut stage = match self.stage_meta(&rollout) {
                    Ok(old) => old,
                    Err(e) if e.code == ErrorCode::NotFound => {
                        if offset != 0 {
                            return Err(error("첫 조각 offset은0이다"));
                        }
                        self.remove_stage(&rollout)?;
                        self.prune_stages(&rollout, true)?;
                        let mut options = std::fs::OpenOptions::new();
                        options.write(true).create_new(true);
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::OpenOptionsExt;
                            options.mode(0o600);
                        }
                        options.open(self.stage_path(&rollout, "part")?).map_err(error)?;
                        let s = DeployStageView {
                            rollout: rollout.clone(),
                            project: project.clone(),
                            sha256: sha256.clone(),
                            total,
                            offset: 0,
                            complete: false,
                            version: None,
                        };
                        self.save_stage(&s)?;
                        s
                    }
                    Err(e) => return Err(e),
                };
                if stage.project != project || stage.sha256 != sha256 || stage.total != total {
                    return Err(error("같은 rollout을 다른 전송에 쓸 수 없다"));
                }
                stage = self.stage_status(&rollout)?;
                if stage.complete {
                    return Err(error("완료된 후보에는 다시 쓰지 않는다"));
                }
                if stage.offset != offset {
                    return Err(error("전송 offset 불일치: 상태를 다시 조회한다"));
                }
                let bin = self.stage_path(&rollout, "bin")?;
                let part = self.stage_path(&rollout, "part")?;
                let candidate = if bin.exists() { &bin } else { &part };
                if !data.0.is_empty() {
                    let mut file = std::fs::OpenOptions::new().append(true).open(candidate).map_err(error)?;
                    file.write_all(&data.0).map_err(error)?;
                    file.sync_all().map_err(error)?;
                }
                stage.offset += data.0.len() as u64;
                if eof {
                    if stage.offset != total || sha(candidate)? != sha256 {
                        return Err(error("전송 EOF SHA-256/크기 불일치"));
                    }
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        std::fs::set_permissions(candidate, std::fs::Permissions::from_mode(0o700)).map_err(error)?;
                    }
                    let execute = (*self.executor.lock().unwrap()).ok_or_else(|| error("배포 실행기가 없다"))?;
                    let receipt = execute(
                        exe,
                        DeployAction::CheckedUpdate {
                            from: candidate.to_string_lossy().into_owned(),
                            sha256: sha256.clone(),
                            dry_run: true,
                        },
                    )
                    .map_err(error)?;
                    let receipt: serde_json::Value = serde_json::from_str(&receipt).map_err(error)?;
                    if receipt["sha256"] != sha256 || sha(candidate)? != sha256 {
                        return Err(error("self-check 뒤 후보 SHA-256 불일치"));
                    }
                    stage.version =
                        Some(receipt["version"].as_str().ok_or_else(|| error("self-check 판이 없다"))?.into());
                    if candidate == &part {
                        std::fs::rename(&part, &bin).map_err(error)?;
                    }
                    stage.complete = true;
                    self.save_stage(&stage)?;
                    self.prune_stages(&rollout, false)?;
                }
                Ok(AgentResponse::DeployStage { stage })
            }
            _ => Err(error("전송 요청이 아니다")),
        }
    }
}
/// 빌드 폴더의 `aaa`와 옆 `provenance.json`(16KiB 이하)만 받는다. symlink·다른 이름·일반 파일이 아닌 입력은 거부한다
fn candidate_files(path: &str) -> Result<(PathBuf, serde_json::Value), ErrorBody> {
    let path = path.strip_prefix("~/").map(|p| aaa_config::paths::home().join(p)).unwrap_or_else(|| path.into());
    if !path.is_absolute() || path.file_name().is_none_or(|n| n != "aaa" && n != "aaa.exe") {
        return Err(error("빌드 후보 폴더의 aaa만 읽는다"));
    }
    let parent = path.parent().unwrap().canonicalize().map_err(error)?;
    let file_path = path.canonicalize().map_err(error)?;
    let provenance = parent.join("provenance.json");
    if file_path != parent.join(path.file_name().unwrap()) || provenance.canonicalize().map_err(error)? != provenance {
        return Err(error("후보 symlink는 허용하지 않는다"));
    }
    let provenance_meta = std::fs::metadata(&provenance).map_err(error)?;
    if !provenance_meta.is_file() || provenance_meta.len() > 16 * 1024 {
        return Err(error("provenance 크기 초과"));
    }
    let manifest = serde_json::from_slice(&std::fs::read(provenance).map_err(error)?).map_err(error)?;
    if !std::fs::metadata(&file_path).map_err(error)?.is_file() {
        return Err(error("후보는 일반 파일이어야 한다"));
    }
    Ok((file_path, manifest))
}
fn fetch(path: &str, offset: u64, len: u32, expected: &str) -> Result<AgentResponse, ErrorBody> {
    if !valid_sha(expected) || len == 0 || len as usize > DEPLOY_CHUNK {
        return Err(error("후보 읽기 한도/SHA-256 오류"));
    }
    let (file_path, manifest) = candidate_files(path)?;
    if manifest["sha256"] != expected {
        return Err(error("빌드 provenance SHA-256 불일치"));
    }
    let mut file = std::fs::File::open(&file_path).map_err(error)?;
    let total = file.metadata().map_err(error)?.len();
    if total == 0 || total > DEPLOY_MAX_BYTES || offset > total {
        return Err(error("후보 크기/offset 한도 초과"));
    }
    if offset == 0 && sha(&file_path)? != expected {
        return Err(error("빌드 후보 SHA-256 불일치"));
    }
    file.seek(SeekFrom::Start(offset)).map_err(error)?;
    let mut data = vec![0; (total - offset).min(len as u64) as usize];
    file.read_exact(&mut data).map_err(error)?;
    Ok(AgentResponse::DeployChunk { offset, eof: offset + data.len() as u64 == total, data: B64(data), total })
}
/// 웹 「배포」가 고를 후보(관리 GUI W6a): provenance의 SHA·커밋을 파일과 대조하고, rollout 전송 끝과 같은 dry-run
/// `CheckedUpdate`로 self-check·판 고정을 본다(설치 파일은 바꾸지 않는다). 실행기 오류에는 경로·후보 출력이 섞여 고정 글로 답한다
fn inspect(exe: &Path, execute: Executor, path: &str) -> Result<AgentResponse, ErrorBody> {
    let (file_path, manifest) = candidate_files(path)?;
    let sha256 =
        manifest["sha256"].as_str().filter(|s| valid_sha(s)).ok_or_else(|| error("provenance에 SHA-256이 없다"))?;
    let commit = match &manifest["commit"] {
        serde_json::Value::Null => None,
        serde_json::Value::String(c) if c.len() == 40 && c.bytes().all(|b| b.is_ascii_hexdigit()) => {
            Some(c.to_ascii_lowercase())
        }
        _ => return Err(error("provenance 커밋이 40자리 hex가 아니다")),
    };
    let total_bytes = std::fs::metadata(&file_path).map_err(error)?.len();
    if total_bytes == 0 || total_bytes > DEPLOY_MAX_BYTES {
        return Err(error("후보 크기 한도 초과"));
    }
    if sha(&file_path)? != sha256 {
        return Err(error("빌드 후보 SHA-256 불일치: provenance와 파일이 다르다"));
    }
    let checked = execute(
        exe,
        DeployAction::CheckedUpdate {
            from: file_path.to_string_lossy().into_owned(),
            sha256: sha256.into(),
            dry_run: true,
        },
    )
    .map_err(|cause| {
        aaa_common::diagnostic::context(
            cause,
            error("후보 dry-run(self-check·판 확인) 실패: 실행기 출력 원문은 비밀값 때문에 생략한다"),
        )
    })?;
    let receipt: serde_json::Value = serde_json::from_str(&checked).map_err(error)?;
    let version = receipt["version"].as_str().ok_or_else(|| error("self-check 판이 없다"))?;
    if receipt["sha256"] != sha256 || sha(&file_path)? != sha256 {
        return Err(error("self-check 뒤 후보 SHA-256 불일치"));
    }
    if commit.as_ref().is_some_and(|c| !version.ends_with(&format!("+{}", &c[..12]))) {
        return Err(error("후보 판의 커밋 표시가 provenance와 다르다"));
    }
    let built_at = manifest["built_at"].as_str().filter(|t| t.len() <= 64).map(str::to_string);
    Ok(AgentResponse::DeployCandidate { sha256: sha256.into(), commit, version: version.into(), total_bytes, built_at })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    fn checked(exe: &Path, action: DeployAction) -> Result<String, String> {
        let DeployAction::CheckedUpdate { from, sha256, .. } = action else {
            panic!("typed checked update여야 한다")
        };
        assert!(Path::new(&from).is_file());
        std::fs::write(exe.with_extension("checked"), b"checked").unwrap();
        Ok(serde_json::json!({"sha256":sha256,"version":"0.1.2+candidate"}).to_string())
    }
    #[test]
    fn transfer_resumes_durable_offsets_and_checks_sha_before_execution() {
        let root = std::env::temp_dir().join(format!("aaa-stage-{}", aaa_common::util::new_id()));
        std::fs::create_dir_all(&root).unwrap();
        let exe = root.join("installed");
        std::fs::write(&exe, b"old").unwrap();
        let bytes = b"new candidate";
        let expected = hex::encode(Sha256::digest(bytes));
        let id = aaa_common::util::new_id();
        let request = |id: &str, offset, data: &[u8], eof, sha: &str| AgentRequest::DeployStage {
            rollout: id.into(),
            project: "demo".into(),
            offset,
            data: B64(data.to_vec()),
            eof,
            sha256: sha.into(),
            total: bytes.len() as u64,
        };
        let manager = Manager::new(&root, &exe).unwrap();
        manager.configure(&exe, checked).unwrap();
        let bad = aaa_common::util::new_id();
        let mismatch = manager.transport(&exe, request(&bad, 0, bytes, true, &"a".repeat(64))).unwrap_err();
        assert!(mismatch.message.contains("SHA-256"));
        assert!(!exe.with_extension("checked").exists(), "SHA 불일치 후보를 실행했다");
        assert!(!manager.stage_path(&bad, "bin").unwrap().exists());
        manager.transport(&exe, AgentRequest::DeployStageClean { rollout: bad, keep: false }).unwrap();
        let mut too_big = request(&id, 0, &[], false, &expected);
        if let AgentRequest::DeployStage { total, .. } = &mut too_big {
            *total = DEPLOY_MAX_BYTES + 1;
        }
        assert!(manager.transport(&exe, too_big).is_err());
        // 첫 파일 쓰기와 metadata 사이에 끝난 프로세스의 찌꺼기도 새 offset0부터 복구한다.
        std::fs::write(manager.stage_path(&id, "part").unwrap(), b"orphan").unwrap();
        let first = manager.transport(&exe, request(&id, 0, &bytes[..3], false, &expected)).unwrap();
        assert!(matches!(first, AgentResponse::DeployStage { stage } if stage.offset == 3 && !stage.complete));
        assert!(
            manager
                .transport(&exe, request(&id, 0, &bytes[3..], true, &expected))
                .unwrap_err()
                .message
                .contains("offset")
        );
        drop(manager);
        let manager = Manager::new(&root, &exe).unwrap();
        manager.configure(&exe, checked).unwrap();
        assert_eq!(manager.stage_status(&id).unwrap().offset, 3);
        let done = manager.transport(&exe, request(&id, 3, &bytes[3..], true, &expected)).unwrap();
        assert!(
            matches!(done, AgentResponse::DeployStage { stage } if stage.complete && stage.version.as_deref() == Some("0.1.2+candidate"))
        );
        assert_eq!(std::fs::read(&exe).unwrap(), b"old", "전송만으로 설치 파일을 바꾸면 안 된다");
        assert!(manager.staged_action("other", &id, false).is_err());
        assert!(manager.staged_action("demo", "../outside", false).is_err());
        assert!(
            matches!(manager.staged_action("demo", &id, false).unwrap(), DeployAction::CheckedUpdate { from, sha256, dry_run:false } if Path::new(&from).starts_with(&manager.root) && sha256 == expected)
        );
        std::fs::write(manager.stage_path(&id, "bin").unwrap(), b"tampered").unwrap();
        assert!(manager.staged_action("demo", &id, false).is_err(), "완료 뒤 바뀐 후보를 설치하면 안 된다");
        manager.remove_stage(&id).unwrap();
        manager.transport(&exe, request(&id, 0, bytes, true, &expected)).unwrap();
        let next = aaa_common::util::new_id();
        manager.transport(&exe, request(&next, 0, bytes, true, &expected)).unwrap();
        assert!(!manager.stage_path(&id, "bin").unwrap().exists(), "완료 후보는 마지막 하나만 보관한다");
        assert!(manager.stage_path(&next, "bin").unwrap().exists());
        for wrong in ["invalid".to_string(), aaa_common::util::new_id()] {
            assert!(manager.transport(&exe, AgentRequest::DeployStageClean { rollout: wrong, keep: true }).is_err());
            assert!(manager.stage_path(&next, "bin").unwrap().exists(), "잘못된 보존 ID로 정상 후보를 지웠다");
        }
        drop(manager);
        std::fs::remove_dir_all(root).unwrap();
    }
    fn dry_run_ok(_: &Path, action: DeployAction) -> Result<String, String> {
        let DeployAction::CheckedUpdate { sha256, dry_run: true, .. } = action else {
            panic!("후보 읽기는 dry-run 교체만 부른다")
        };
        Ok(serde_json::json!({"sha256":sha256,"version":"0.1.2+abcdefabcdef"}).to_string())
    }
    fn dry_run_fails(_: &Path, _: DeployAction) -> Result<String, String> {
        Err("self-check 실패: /private/install-marker/.aaa-update-1.tmp 출력 secret-marker".into())
    }
    fn never(_: &Path, _: DeployAction) -> Result<String, String> {
        panic!("SHA가 다른 후보를 실행했다")
    }
    #[test]
    fn candidate_inspection_matches_provenance_and_dry_runs_before_answering() {
        let root = std::env::temp_dir().join(format!("aaa-candidate-{}", aaa_common::util::new_id()));
        std::fs::create_dir_all(&root).unwrap();
        let exe = root.join("installed");
        let candidate = root.join("aaa");
        std::fs::write(&candidate, b"candidate").unwrap();
        let hash = hex::encode(Sha256::digest(b"candidate"));
        let commit = format!("abcdefabcdef{}", "0".repeat(28));
        let provenance = |v: serde_json::Value| std::fs::write(root.join("provenance.json"), v.to_string()).unwrap();
        let path = candidate.to_str().unwrap();
        provenance(serde_json::json!({"sha256":hash,"commit":commit,"built_at":"2026-10-21T00:00:00Z"}));
        let AgentResponse::DeployCandidate { sha256, commit: got, version, total_bytes, built_at } =
            inspect(&exe, dry_run_ok, path).unwrap()
        else {
            panic!("후보 응답이 아니다")
        };
        assert_eq!(
            (sha256, got.as_deref(), version.as_str(), total_bytes),
            (hash.clone(), Some(commit.as_str()), "0.1.2+abcdefabcdef", 9)
        );
        assert_eq!(built_at.as_deref(), Some("2026-10-21T00:00:00Z"));
        // 판의 커밋 표시가 provenance와 다르면 rollout 전송 끝처럼 거부한다
        provenance(serde_json::json!({"sha256":hash,"commit":"1".repeat(40)}));
        assert!(inspect(&exe, dry_run_ok, path).unwrap_err().message.contains("커밋"));
        provenance(serde_json::json!({"sha256":hash,"commit":"short"}));
        assert!(inspect(&exe, dry_run_ok, path).is_err());
        // 실행기 원문(경로·후보 출력)은 싣지 않는다
        provenance(serde_json::json!({"sha256":hash}));
        let failed = inspect(&exe, dry_run_fails, path).unwrap_err().message;
        assert!(failed.contains("dry-run") && !failed.contains("marker"), "{failed}");
        // provenance와 다른 파일은 실행하지 않는다
        std::fs::write(&candidate, b"tampered").unwrap();
        assert!(inspect(&exe, never, path).unwrap_err().message.contains("SHA-256"));
        assert!(inspect(&exe, never, root.join("other").to_str().unwrap()).is_err(), "aaa가 아닌 이름을 읽었다");
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn candidate_inspection_waits_for_no_running_job() {
        // 실행기 staging 이름은 agent PID 하나다: 도는 배포 작업이 있으면 점검이 그 작업과 겹치지 않게 거부한다
        let root = std::env::temp_dir().join(format!("aaa-candidate-busy-{}", aaa_common::util::new_id()));
        std::fs::create_dir_all(&root).unwrap();
        let exe = root.join("installed");
        std::fs::write(&exe, b"old").unwrap();
        std::fs::write(root.join("aaa"), b"candidate").unwrap();
        let hash = hex::encode(Sha256::digest(b"candidate"));
        std::fs::write(root.join("provenance.json"), serde_json::json!({"sha256":hash}).to_string()).unwrap();
        let manager = Manager::new(&root, &exe).unwrap();
        manager.configure(&exe, never).unwrap();
        let restart = DeployAction::Restart { service: aaa_proto::DeployService::Agent };
        let job = aaa_common::util::new_id();
        manager.prepare(&exe, job.clone(), "demo".into(), restart.clone()).unwrap();
        let ask = AgentRequest::DeployCandidate { path: root.join("aaa").to_string_lossy().into() };
        let refused = manager.transport(&exe, ask).unwrap_err().message;
        assert!(refused.contains("진행 중") && !refused.contains(&job), "{refused}");
        // 점검하는 동안 들어온 작업은 바로 거부한다(실행기를 lock 밖에서 돌린다)
        manager.inspecting.store(true, std::sync::atomic::Ordering::SeqCst);
        let next = manager.prepare(&exe, aaa_common::util::new_id(), "demo".into(), restart.clone());
        assert!(next.unwrap_err().message.contains("점검"));
        drop(manager);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn fetch_accepts_only_the_provenance_candidate_and_bounded_chunks() {
        let root = std::env::temp_dir().join(format!("aaa-fetch-{}", aaa_common::util::new_id()));
        std::fs::create_dir_all(&root).unwrap();
        let bytes = vec![42; DEPLOY_CHUNK + 7];
        let hash = hex::encode(Sha256::digest(&bytes));
        let candidate = root.join("aaa");
        std::fs::write(&candidate, &bytes).unwrap();
        let path = candidate.to_str().unwrap();
        assert!(fetch(path, 0, 1, &hash).is_err(), "provenance 없는 파일을 읽었다");
        std::fs::write(root.join("provenance.json"), serde_json::json!({"sha256":hash}).to_string()).unwrap();
        assert!(fetch(path, 0, (DEPLOY_CHUNK + 1) as u32, &hash).is_err());
        let first = fetch(path, 0, DEPLOY_CHUNK as u32, &hash).unwrap();
        assert!(
            matches!(first, AgentResponse::DeployChunk { offset:0, data, eof:false, total } if data.0 == bytes[..DEPLOY_CHUNK] && total == bytes.len() as u64)
        );
        assert!(
            matches!(fetch(path, DEPLOY_CHUNK as u64, DEPLOY_CHUNK as u32, &hash).unwrap(), AgentResponse::DeployChunk { data, eof:true, .. } if data.0 == [42;7])
        );
        let other = root.join("other");
        std::fs::write(&other, &bytes).unwrap();
        assert!(fetch(other.to_str().unwrap(), 0, 1, &hash).is_err(), "후보 밖 자유 경로를 읽었다");
        #[cfg(unix)]
        {
            std::fs::remove_file(&candidate).unwrap();
            std::os::unix::fs::symlink(&other, &candidate).unwrap();
            assert!(fetch(path, 0, 1, &hash).is_err(), "후보 symlink로 경계를 넘었다");
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}

//! public NDJSON과 선택적 private 사본. 승인된 worker Started 전에는 파일을 만들지 않는다.
use super::*;
use std::{fs::File, path::PathBuf};
type Result<T> = std::result::Result<T, Box<InstallResult>>;
#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Progress {
    StageRunning,
    FilesConfigured,
    FilesUnchanged,
    EnrollRunning,
    EnrollCompleted,
    ServiceRunning,
    ServiceReady,
    WebRunning,
    WebCompleted,
    PairingRunning,
    PairingCompleted,
    VerifyCompleted,
}
pub(super) struct Reporter {
    stream: bool,
    seq: u64,
    id: Option<String>,
    path: Option<PathBuf>,
    file: Option<File>,
}
impl Reporter {
    pub(super) fn new(stream: bool) -> Self {
        Self { stream, seq: 0, id: None, path: None, file: None }
    }
    pub(super) fn configure(&mut self, invocation: &InstallInvocation, plan: &InstallPlan) -> Result<()> {
        if let Some(path) = &invocation.events_file {
            let path = Path::new(path);
            if !path.is_absolute() || path.components().any(|p| p == std::path::Component::ParentDir) {
                return Err(error("installation_events_path_invalid", 2));
            }
            let parent =
                engine::normalize_state(path.parent().ok_or_else(|| error("installation_events_path_invalid", 2))?)?;
            let path = parent.join(path.file_name().ok_or_else(|| error("installation_events_path_invalid", 2))?);
            if path.symlink_metadata().is_ok()
                || path.starts_with(&plan.context.state_dir)
                || path.starts_with(&plan.context.config_dir)
                || path == Path::new(&plan.context.bin_path)
                || plan.choices.from.as_deref().is_some_and(|source| path == Path::new(source))
            {
                return Err(error("installation_events_path_conflict", 4));
            }
            self.path = Some(path);
        }
        Ok(())
    }
    pub(super) fn started(&mut self, id: &str) -> Result<()> {
        if self.id.is_some() {
            return Err(error("installation_worker_unverified", 4));
        }
        self.id = Some(id.into());
        if let Some(path) = &self.path {
            let parent = path.parent().ok_or_else(|| error("installation_events_path_invalid", 2))?;
            aaa_common::private_file::parents(parent).map_err(|_| error("installation_events_unavailable", 3))?;
            self.file = Some(
                aaa_common::private_file::new_file(path).map_err(|_| error("installation_events_unavailable", 3))?,
            );
        }
        self.event(
            InstallPhase::Authorize,
            InstallEventState::Completed,
            "installation_authorized",
            "이 계획의 승인과 실행 기록을 확인했습니다",
        )
    }
    fn write(&mut self, item: &InstallStreamItem) -> Result<()> {
        let mut bytes = serde_json::to_vec(item).map_err(|_| error("installation_progress_unavailable", 3))?;
        bytes.push(b'\n');
        if let Some(file) = &mut self.file {
            file.write_all(&bytes)
                .and_then(|_| file.flush())
                .map_err(|_| error("installation_events_unavailable", 3))?;
        }
        if self.stream {
            let mut stdout = std::io::stdout().lock();
            stdout
                .write_all(&bytes)
                .and_then(|_| stdout.flush())
                .map_err(|_| error("installation_progress_unavailable", 3))?;
        }
        Ok(())
    }
    fn event(&mut self, phase: InstallPhase, state: InstallEventState, code: &str, message: &str) -> Result<()> {
        if self.id.is_none() {
            return Err(error("installation_worker_unverified", 4));
        }
        self.seq = self.seq.checked_add(1).ok_or_else(|| error("installation_progress_unavailable", 3))?;
        self.write(&InstallStreamItem::Event {
            schema: InstallEventSchema::V1,
            installation_id: self.id.clone(),
            seq: self.seq,
            phase,
            state,
            code: code.into(),
            message: message.into(),
            progress: None,
            needs_user: None,
        })
    }
    pub(super) fn progress(&mut self, progress: Progress) -> Result<()> {
        use Progress::*;
        let (phase, state, code, message) = match progress {
            StageRunning => {
                (InstallPhase::Stage, InstallEventState::Running, "installation_stage", "검증한 파일 작업을 진행합니다")
            }
            FilesConfigured => (
                InstallPhase::Configure,
                InstallEventState::Completed,
                "installation_files_configured",
                "승인한 파일 작업을 대조했습니다",
            ),
            FilesUnchanged => (
                InstallPhase::Configure,
                InstallEventState::Unchanged,
                "installation_files_unchanged",
                "기존 파일 receipt의 실제 상태를 대조했습니다",
            ),
            EnrollRunning => (
                InstallPhase::Enroll,
                InstallEventState::Running,
                "installation_enroll",
                "승인한 host 등록을 확인합니다",
            ),
            EnrollCompleted => (
                InstallPhase::Enroll,
                InstallEventState::Completed,
                "installation_enrolled",
                "요청한 host 등록 상태를 확인했습니다",
            ),
            ServiceRunning => (
                InstallPhase::Service,
                InstallEventState::Running,
                "installation_service",
                "승인한 서비스 작업을 진행합니다",
            ),
            ServiceReady => (
                InstallPhase::Service,
                InstallEventState::Completed,
                "installation_service_verified",
                "요청한 서비스 상태를 확인했습니다",
            ),
            WebRunning => (
                InstallPhase::Web,
                InstallEventState::Running,
                "installation_web",
                "승인한 정확 포트의 공개 상태를 확인합니다",
            ),
            WebCompleted => (
                InstallPhase::Web,
                InstallEventState::Completed,
                "installation_web_verified",
                "정확 mapping과 현재 host의 공개 HTTPS 응답을 확인했습니다",
            ),
            PairingRunning => (
                InstallPhase::Pairing,
                InstallEventState::Running,
                "installation_pairing",
                "첫 Controller 등록 상태를 확인합니다",
            ),
            PairingCompleted => (
                InstallPhase::Pairing,
                InstallEventState::Completed,
                "installation_paired",
                "실제 Controller 등록 상태를 확인했습니다",
            ),
            VerifyCompleted => (
                InstallPhase::Verify,
                InstallEventState::Completed,
                "installation_verified",
                "실제 상태 대조를 마쳤습니다",
            ),
        };
        self.event(phase, state, code, message)
    }
    pub(super) fn finish(mut self, result: InstallResult, json: bool) -> ExitCode {
        self.seq += 1;
        if let Some(file) = &mut self.file {
            let item = InstallStreamItem::Result {
                schema: InstallEventSchema::V1,
                installation_id: result.installation_id.clone(),
                seq: self.seq,
                result: Box::new(result.clone()),
            };
            let written = serde_json::to_writer(&mut *file, &item).is_ok()
                && writeln!(file).is_ok()
                && file.flush().is_ok()
                && file.sync_all().is_ok();
            if !written {
                return super::emit_at_seq(
                    {
                        let mut failed = result;
                        failed.status = InstallStatus::NeedsUser;
                        failed.exit_code = 3;
                        failed.error = None;
                        failed.needs_user.push(InstallNeedsUser {
                            code: "installation_events_unavailable".into(),
                            target: "output".into(),
                            message: "진행 기록 사본을 저장하지 못했습니다. 같은 ID의 status로 실제 결과를 확인하세요"
                                .into(),
                        });
                        failed
                    },
                    json,
                    self.stream,
                    self.seq,
                );
            }
        }
        super::emit_at_seq(result, json, self.stream, self.seq)
    }
}

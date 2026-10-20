//! I2 파일 엔진. CLI 쓰기는 닫혀 있고 I6의 검증된 executor가 이 경계에 연결된다.
//! status는 private journal을 읽기만 한다. 원문 payload/경로/계정은 공개 결과에 넣지 않는다.
use std::fs::File;
use std::path::{Path, PathBuf};

use aaa_proto::install::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

mod private;
#[cfg_attr(not(test), expect(dead_code, reason = "I5 감독자 adapter는 I6 승인 executor에서 연결한다"))]
mod supervisor;

pub(super) fn service_dispatch(context: &Path, sha: &str) -> std::process::ExitCode {
    supervisor::dispatch(context, sha)
}

type Result<T> = std::result::Result<T, Fault>;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Fault {
    code: &'static str,
    exit: u8,
}
impl Fault {
    fn io(_: std::io::Error) -> Self {
        Self { code: "installation_io_failed", exit: 1 }
    }
    fn conflict() -> Self {
        Self { code: "installation_content_conflict", exit: 4 }
    }
}
fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn account_hash() -> Result<String> {
    super::platform::account()
        .map(|account| hash(account.as_bytes()))
        .ok_or(Fault { code: "account_unavailable", exit: 5 })
}
fn id_ok(id: &str) -> bool {
    id.len() == 26 && id.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}
pub(super) fn validate_id(id: &str) -> std::result::Result<(), Box<InstallResult>> {
    if id_ok(id) { Ok(()) } else { Err(super::error("invalid_installation_id", 2)) }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Phase {
    Preparing,
    Applying,
    Configured,
    RollingBack,
    RolledBack,
    Conflict,
    Uninstalling,
    Uninstalled,
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Step {
    Pending,
    Staged,
    Applied,
    Reverted,
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Binary,
    Config,
    Project,
    Hook,
}
impl Kind {
    fn text(self) -> &'static str {
        match self {
            Self::Binary => "binary",
            Self::Config => "config",
            Self::Project => "project",
            Self::Hook => "hooks",
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    kind: Kind,
    before: Option<String>,
    after: String,
    state: Step,
    inode: Option<String>,
    changed: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReverseEntry {
    index: usize,
    state: Step,
    inode: Option<String>,
    changed: bool,
}
/// raw TOML/경로는 이 일반 journal에 넣지 않는다. payload/backup은 별도 private 파일이다.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    schema: u32,
    id: String,
    account: String,
    instance: Option<String>,
    role: InstallRole,
    platform: InstallPlatform,
    version: Option<String>,
    binary_sha256: String,
    plan_sha256: String,
    goal_sha256: String,
    manifest_sha256: String,
    phase: Phase,
    entries: Vec<Entry>,
    root: Option<Step>,
    reverse_sha256: Option<String>,
    reverse: Vec<ReverseEntry>,
    rollback_remaining: Vec<String>,
}

fn journal_dir(state: &Path, id: &str) -> PathBuf {
    state.join("install/jobs").join(id)
}
fn read_journal(state: &Path, id: &str, instance: Option<&str>) -> Result<Journal> {
    let path = journal_dir(state, id).join("journal.json");
    let bytes = private::read(&path, 2 * 1024 * 1024).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            Fault { code: "installation_not_found", exit: 2 }
        } else {
            Fault { code: "installation_record_untrusted", exit: 4 }
        }
    })?;
    let j: Journal =
        serde_json::from_slice(&bytes).map_err(|_| Fault { code: "installation_record_invalid", exit: 2 })?;
    if j.schema != 1
        || j.id != id
        || j.account != account_hash()?
        || j.instance.as_deref() != instance
        || j.entries.len() > 128
        || j.reverse.len() > j.entries.len()
        || j.rollback_remaining.len() > 2
        || j.rollback_remaining
            .iter()
            .any(|code| !matches!(code.as_str(), "work_root_preserved" | "staging_unverified"))
        || j.reverse_sha256.as_ref().is_some_and(|s| !digest_ok(s))
        || j.reverse.iter().any(|e| e.index >= j.entries.len())
        || !digest_ok(&j.plan_sha256)
        || !digest_ok(&j.binary_sha256)
        || !digest_ok(&j.manifest_sha256)
        || !digest_ok(&j.goal_sha256)
        || j.entries.iter().any(|e| !digest_ok(&e.after) || e.before.as_ref().is_some_and(|s| !digest_ok(s)))
        || j.version.as_deref().is_some_and(|v| aaa_config::parse_version(v).is_none())
    {
        return Err(Fault { code: "installation_record_invalid", exit: 2 });
    }
    Ok(j)
}
fn digest_ok(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

pub(super) fn status(
    state: &Path,
    instance: Option<&str>,
    id: &str,
) -> std::result::Result<InstallResult, Box<InstallResult>> {
    validate_id(id)?;
    let j = read_journal(state, id, instance).map_err(|e| super::error(e.code, e.exit))?;
    let result = transaction::observe(state, j).map_err(|e| super::error(e.code, e.exit))?;
    supervisor::observe(state, instance, id, result).map_err(|e| super::error(e.code, e.exit))
}
impl Journal {
    fn result(&self) -> InstallResult {
        let mut result = *super::error("installation_incomplete", 1);
        result.installation_id = Some(self.id.clone());
        result.plan_sha256 = Some(self.plan_sha256.clone());
        result.role = Some(self.role);
        result.instance = self.instance.clone();
        result.platform = Some(self.platform.clone());
        result.version = self.version.clone();
        result.binary_sha256 = Some(self.binary_sha256.clone());
        result.changes = self
            .entries
            .iter()
            .map(|entry| InstallChange {
                kind: entry.kind.text().into(),
                action: "configure".into(),
                state: match entry.state {
                    Step::Pending => InstallChangeState::Pending,
                    Step::Staged => InstallChangeState::Pending,
                    Step::Applied => {
                        if !entry.changed {
                            InstallChangeState::Unchanged
                        } else {
                            InstallChangeState::Applied
                        }
                    }
                    Step::Reverted => InstallChangeState::Reverted,
                },
            })
            .collect();
        if self.phase == Phase::Uninstalled {
            result.changes = self
                .reverse
                .iter()
                .map(|entry| InstallChange {
                    kind: self.entries[entry.index].kind.text().into(),
                    action: "uninstall".into(),
                    state: if entry.changed { InstallChangeState::Applied } else { InstallChangeState::Unchanged },
                })
                .collect();
        }
        match self.phase {
            Phase::Configured | Phase::Uninstalled => {
                result.status = InstallStatus::Configured;
                result.exit_code = 0;
                result.error = None;
            }
            Phase::Conflict => {
                result.error = Some(super::error("installation_content_conflict", 4).error.unwrap());
                result.exit_code = 4;
            }
            Phase::RolledBack => {
                result.rollback.state = if self.rollback_remaining.is_empty() {
                    InstallRollbackState::Completed
                } else {
                    InstallRollbackState::Partial
                };
                result.rollback.remaining = self.rollback_remaining.clone();
            }
            Phase::Preparing | Phase::Applying | Phase::RollingBack | Phase::Uninstalling => {
                result.status = InstallStatus::NeedsUser;
                result.exit_code = 3;
                result.error = None;
                result.needs_user.push(InstallNeedsUser {
                    code: "installation_authorization_required".into(),
                    target: "installation".into(),
                    message: "검증된 승인 경로에서 같은 ID의 실제 상태를 재대조하세요".into(),
                });
                result.resume = Some(InstallResume { installation_id: self.id.clone() });
            }
        }
        result
    }
}

// I2의 쓰기 엔진은 I6의 실제 승인 executor 연결 전까지 private owner가 직접 검증한다.
// 승인 우회용 CLI/환경 flag나 시험 전용 export를 만들지 않는다.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "I2 단계 경계: coordinator 승인대로 CLI 쓰기는 I6까지 닫고 실제 파일 owner가 엔진을 검증한다"
    )
)]
mod transaction;

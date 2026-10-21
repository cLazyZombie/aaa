//! SSH로 설치·등록(웹 머신 추가 M3, plan §8.420). 원격 설치 단계는 install(t-0485)의 검증 SSH 입구가 한다. 이 파일은 그 앞뒤만 맡는다:
//!
//! 1. **계획.** 같은 판 aaa를 원격 임시 폴더에 보내 확인하고(`binary`), 사람이 넣은 설정을 install choices(role agent·등록 grant)로 옮겨
//!    원격 `aaa install-probe`에 stdin으로 준다. 돌아온 계획(경로·계정이 든 private 원문)은 host 작업 폴더의 0600 파일에만 두고, 화면에는
//!    install 공개 결과(바꿀 것·검사·사람 일)와 plan hash만 보인다.
//! 2. **적용.** 사람이 그 hash로 「설치하고 등록」을 누르면 host가 자기 install 발급자로 계획에 서명한다(`install_approve`, 조작 기기·세션
//!    밖 사람만). package를 원격 `install-bootstrap`에 주면 원격이 issuer pin을 받고 agent 신원을 준비해 알린다. host는 그 신원에 묶은 등록
//!    grant를 더해 다시 서명해 한 번 더 전하고, 원격 `install --resume`이 적용한다. 원격 worker가 grant로 host에 등록한다(TLS 신원 대조).
//! 3. **끝.** install 결과와 host 머신 목록을 함께 본다. 원격 임시 폴더는 끝나면 지운다.
//!
//! 원격 명령은 고정 글이다: 원격 폴더는 `binary::safe_dir`를 통과한 것, SHA는 hex, 설치 ID는 ULID만 들어간다. 원격 상태 폴더는 원격 셸의
//! `$HOME`으로 쓴다. choices에는 감지한 HOME으로 같은 경로를 적는다(install이 둘이 같은지 본다).
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use aaa_proto::install::*;
use aaa_proto::{
    DeviceRole, ErrorBody, ErrorCode, FrontResponse, MachineDetected, MachineSetupInstall, MachineSetupPhase,
    MachineSetupReview, MachineSetupSettings,
};
use base64::Engine as _;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::binary::{self, Source};
use super::{Ran, Ssh, Target, bad, parse_target};
use crate::Host;

/// 원격 install의 읽기 계획·bootstrap 시한
const STEP_TIMEOUT: Duration = Duration::from_secs(120);
/// 원격 적용 전체 시한(install의 등록 대기 상한 기본 600초보다 길게)
const APPLY_TIMEOUT: Duration = Duration::from_secs(900);
/// 원격 install 출력 상한(계획·결과 JSON)
const OUTPUT_LIMIT: usize = 4 * 1024 * 1024;

fn hex_ok(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}
fn id_ok(s: &str) -> bool {
    s.len() == 26 && s.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}
fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// 원격 상태 폴더: HOME 아래 상대 경로(install의 기본 자리와 같다). Windows는 M4
pub(crate) fn state_suffix(os: &str) -> Option<&'static str> {
    match os {
        "macos" => Some("Library/Application Support/aaa"),
        "linux" => Some(".local/state/aaa"),
        _ => None,
    }
}

fn install_os(os: &str) -> Option<InstallOs> {
    match os {
        "macos" => Some(InstallOs::Macos),
        "linux" => Some(InstallOs::Linux),
        "windows" => Some(InstallOs::Windows),
        _ => None,
    }
}
fn install_arch(arch: &str) -> Option<InstallArch> {
    match arch {
        "aarch64" => Some(InstallArch::Aarch64),
        "x86_64" => Some(InstallArch::X86_64),
        _ => None,
    }
}

/// 원격 절대 경로(POSIX)로 쓸 수 있는가: 제어 문자·`..` 없음
fn remote_path_ok(p: &str) -> bool {
    p.starts_with('/') && p.len() <= 4096 && !p.chars().any(char::is_control) && !p.split('/').any(|c| c == "..")
}

/// 웹 설정 검사(host 쪽 규칙. 실물 경로·mount는 원격 install이 본다)
pub(crate) fn check_settings(s: &MachineSetupSettings) -> Result<(), String> {
    if !aaa_common::util::machine_name_ok(&s.name) {
        return Err("머신 이름은 영문·숫자·.-_ 64자까지다".into());
    }
    let host = s.host.trim();
    if host.is_empty()
        || host.len() > 255
        || !host.bytes().all(|b| b.is_ascii_alphanumeric() || b".-_:[]%".contains(&b))
    {
        return Err("host 주소는 이름·IP·포트 글자만 쓴다(직접 설치 안내의 후보를 고른다)".into());
    }
    if !remote_path_ok(&s.work_root) {
        return Err("작업 root는 그 머신의 절대 경로다(/로 시작)".into());
    }
    if let Some(m) = &s.work_mount
        && !remote_path_ok(m)
    {
        return Err("외장 디스크 mount는 그 머신의 절대 경로다".into());
    }
    if s.harness.contains(&InstallHarness::None) && s.harness.len() > 1 {
        return Err("harness 없음과 다른 harness를 함께 고를 수 없다".into());
    }
    Ok(())
}

/// 웹 설정 + 감지 → install choices(role agent, 등록 grant). 원격 상태·실행 파일 자리는 감지한 HOME 아래 install 기본 자리다
pub(crate) fn choices(s: &MachineSetupSettings, d: &MachineDetected) -> Result<InstallChoices, String> {
    let suffix = state_suffix(&d.os).ok_or_else(|| format!("{}는 아직 SSH로 설치하지 않는다(macOS부터)", d.os))?;
    let home = d.home.as_deref().map(|h| h.trim_end_matches('/')).filter(|h| remote_path_ok(h) && !h.is_empty());
    let home = home.ok_or("그 머신의 HOME을 읽지 못했다(감지를 다시 한다)")?;
    let mut harness = s.harness.clone();
    harness.sort();
    harness.dedup();
    if harness.is_empty() {
        harness.push(InstallHarness::None);
    }
    let (work_mount, work_storage) = match &s.work_mount {
        Some(m) if !m.trim().is_empty() => (Some(m.trim().to_string()), None),
        _ => (None, Some(InstallStorage::Internal)),
    };
    Ok(InstallChoices {
        role: Some(InstallRole::Agent),
        name: Some(s.name.clone()),
        state_dir: Some(format!("{home}/{suffix}")),
        bin_path: Some(format!("{home}/.local/bin/aaa")),
        expected_os: install_os(&d.os),
        expected_arch: install_arch(&d.arch),
        work_root: Some(s.work_root.clone()),
        work_mount,
        work_storage,
        harness,
        login: Some(s.login),
        hooks: Some(InstallHooks::Preserve),
        host: Some(s.host.trim().to_string()),
        network: Some(InstallNetwork::Direct),
        enroll: Some(InstallEnroll::Grant),
        // install 승인 executor는 아직 macOS launchd를 받지 않는다(t-0485 I3). 그 전에는 파일·등록까지만 한다
        service: Some(InstallService::None),
        start: Some(InstallStart::Deferred),
        ..Default::default()
    })
}

/// `sh -c '…'` 안의 원격 aaa 호출(전역 `--state-dir`은 원격 셸의 HOME으로)
fn remote_aaa(dir: &str, os: &str, rest: &str) -> Option<String> {
    let suffix = state_suffix(os)?;
    binary::safe_dir(dir).then(|| format!("sh -c '\"{dir}/aaa\" --state-dir \"$HOME/{suffix}\" {rest}'"))
}
pub(crate) fn probe_script(dir: &str, os: &str, sha: &str) -> Option<String> {
    hex_ok(sha).then(|| remote_aaa(dir, os, &format!("install-probe --executor-sha256 {sha}"))).flatten()
}
pub(crate) fn bootstrap_script(dir: &str, os: &str, sha: &str, issuer: &str) -> Option<String> {
    (hex_ok(sha) && hex_ok(issuer))
        .then(|| remote_aaa(dir, os, &format!("install-bootstrap --executor-sha256 {sha} --issuer-sha256 {issuer}")))
        .flatten()
}
pub(crate) fn apply_script(dir: &str, os: &str, id: &str) -> Option<String> {
    id_ok(id)
        .then(|| remote_aaa(dir, os, &format!("install --resume {id} --non-interactive --yes --json-stream")))
        .flatten()
}

/// install 검증 SSH 입구의 종료 코드를 사람 말로
pub(crate) fn remote_why(step: &str, r: &Ran, code: Option<i32>) -> String {
    match code {
        Some(3) => format!(
            "{step}: 그 머신의 install이 이 접속을 시스템 SSH 서버로 확인하지 못했다(macOS 「원격 로그인」·시스템 sshd만 믿는다)"
        ),
        Some(4) => {
            format!("{step}: 그 머신의 install이 계획·파일 충돌로 멈췄다(이미 설치됐거나 다른 계획이 진행 중이다)")
        }
        Some(255) | None => format!("{step}: {}", super::why(&r.stderr)),
        Some(c) => format!("{step}: 원격 install이 실패했다(exit {c})"),
    }
}

/// 원격 bootstrap이 private 채널로 돌려주는 준비 결과
#[derive(Debug, Deserialize)]
pub(crate) struct Prepared {
    pub installation_id: String,
    pub plan_sha256: String,
    pub agent_identity: Option<String>,
}

/// 원격 install 계획(제안)을 공개 요약으로
pub(crate) fn review(
    p: &InstallProposal,
    binary_sha256: &str,
    binary_version: &str,
) -> Result<MachineSetupReview, String> {
    let plan_sha256 = aaa_config::install::plan_sha256(&p.plan);
    if p.plan.context.binary_sha256 != binary_sha256 {
        return Err("원격 install 계획의 실행 파일 SHA가 보낸 파일과 다르다".into());
    }
    if p.result.plan_sha256.as_deref().is_some_and(|h| h != plan_sha256) {
        return Err("원격 install 계획 hash가 계획 내용과 다르다".into());
    }
    let mut operations: Vec<String> = p
        .permissions
        .iter()
        .filter_map(|x| serde_json::to_value(x.operation).ok()?.as_str().map(str::to_string))
        .collect();
    operations.sort();
    operations.dedup();
    Ok(MachineSetupReview {
        plan_sha256,
        binary_sha256: binary_sha256.into(),
        binary_version: binary_version.into(),
        changes: p.result.changes.clone(),
        checks: p.result.checks.clone(),
        needs_user: p.result.needs_user.clone(),
        operations,
    })
}

/// NDJSON 한 줄: 진행 글 또는 마지막 결과
pub(crate) enum Line {
    Event(String),
    Result(Box<InstallResult>),
    Other,
}
pub(crate) fn stream_line(line: &str) -> Line {
    match serde_json::from_str::<InstallStreamItem>(line.trim_end_matches('\r')) {
        Ok(InstallStreamItem::Event { message, .. }) => Line::Event(message),
        Ok(InstallStreamItem::Result { result, .. }) => Line::Result(result),
        Err(_) => Line::Other,
    }
}

/// host가 넣을 실행 파일과 판(시험은 바꿔 끼운다)
fn running_binary() -> std::io::Result<(PathBuf, String)> {
    #[cfg(test)]
    if let Some(v) = tests_binary::get() {
        return Ok(v);
    }
    Ok((std::env::current_exe()?, aaa_common::util::software()))
}

impl Ssh {
    /// stdin에 bytes를 주고 믿는 키로 고정 명령 하나
    async fn exec_input(&self, target: &Target, command: &str, input: &[u8], timeout: Duration) -> (Ran, Option<i32>) {
        use tokio::io::AsyncWriteExt;
        let mut c = self.trusted(target, command);
        c.stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
        let fail = |e: String| (Ran { ok: false, stdout: String::new(), stderr: e }, None);
        let mut child = match c.spawn() {
            Ok(c) => c,
            Err(e) => return fail(format!("ssh를 실행하지 못했다: {e}")),
        };
        let mut stdin = child.stdin.take();
        let feed = async {
            if let Some(s) = stdin.as_mut() {
                let _ = s.write_all(input).await;
                let _ = s.shutdown().await;
            }
            drop(stdin.take());
        };
        let run = async {
            feed.await;
            child.wait_with_output().await
        };
        match tokio::time::timeout(timeout, run).await {
            Ok(Ok(o)) if o.stdout.len() <= OUTPUT_LIMIT => (
                Ran {
                    ok: o.status.success(),
                    stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
                    stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
                },
                o.status.code(),
            ),
            Ok(Ok(_)) => fail("원격 출력이 너무 크다".into()),
            Ok(Err(e)) => fail(format!("ssh: {e}")),
            Err(_) => fail("ssh: timed out".into()),
        }
    }

    /// 출력 줄마다 `each`를 부르며 끝까지 읽는다(원격 install NDJSON)
    async fn stream(
        &self,
        target: &Target,
        command: &str,
        timeout: Duration,
        mut each: impl FnMut(&str),
    ) -> (Ran, Option<i32>) {
        use tokio::io::{AsyncBufReadExt, AsyncReadExt};
        let mut c = self.trusted(target, command);
        c.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
        let fail = |e: String| (Ran { ok: false, stdout: String::new(), stderr: e }, None);
        let mut child = match c.spawn() {
            Ok(c) => c,
            Err(e) => return fail(format!("ssh를 실행하지 못했다: {e}")),
        };
        let (Some(out), Some(mut err)) = (child.stdout.take(), child.stderr.take()) else {
            return fail("ssh 출력을 받지 못했다".into());
        };
        let run = async {
            let mut stderr = String::new();
            let errs = async {
                let mut buf = Vec::new();
                let _ = (&mut err).take(OUTPUT_LIMIT as u64).read_to_end(&mut buf).await;
                stderr = String::from_utf8_lossy(&buf).into_owned();
            };
            let lines = async {
                let mut lines = tokio::io::BufReader::new(out).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    each(&line);
                }
            };
            tokio::join!(errs, lines);
            let status = child.wait().await;
            (status, stderr)
        };
        match tokio::time::timeout(timeout, run).await {
            Ok((Ok(status), stderr)) => (Ran { ok: status.success(), stdout: String::new(), stderr }, status.code()),
            Ok((Err(e), _)) => fail(format!("ssh: {e}")),
            Err(_) => fail("ssh: timed out".into()),
        }
    }
}

fn conflict(msg: &str) -> ErrorBody {
    ErrorBody::new(ErrorCode::Conflict, msg)
}

impl Host {
    /// 작업의 private 계획 파일(host 작업 폴더, 0600)
    fn setup_plan_path(&self, job: &str) -> PathBuf {
        self.state.join("ssh").join("jobs").join(job).join("proposal.json")
    }

    pub(crate) fn setup_plan(
        self: &Arc<Self>,
        id: &str,
        settings: MachineSetupSettings,
        actor: &str,
    ) -> Result<aaa_proto::MachineSetupView, ErrorBody> {
        check_settings(&settings).map_err(bad)?;
        let taken = self.db.machines().map_err(|e| ErrorBody::new(ErrorCode::Internal, e.to_string()))?;
        if taken.iter().any(|m| m.name == settings.name) {
            return Err(bad(format!("{}은 이미 있는 머신 이름이다", settings.name)));
        }
        let (job, detected) = {
            let _g = self.setup_gate.lock().unwrap();
            let mut job = self.setup_load(id)?;
            if !matches!(job.view.phase, MachineSetupPhase::Ready | MachineSetupPhase::Review) {
                return Err(conflict("이 작업은 설치 설정을 넣는 단계가 아니다(화면을 다시 읽는다)"));
            }
            let detected = job.view.detected.clone().ok_or_else(|| conflict("감지 결과가 없다"))?;
            choices(&settings, &detected).map_err(bad)?;
            job.view.settings = Some(settings.clone());
            job.view.review = None;
            job.view.install = None;
            job.view.error = None;
            job.view.phase = MachineSetupPhase::Planning;
            self.setup_save(&mut job);
            self.db.audit(actor, "machine_setup_plan", &format!("{} {}", job.view.target, settings.name));
            (job, detected)
        };
        let t = parse_target(&job.view.target)?;
        let host = self.clone();
        let id = id.to_string();
        tokio::spawn(async move { host.setup_planning(&id, t, settings, detected, job.remote_dir).await });
        Ok(job.view)
    }

    async fn setup_planning(
        self: Arc<Self>,
        id: &str,
        t: Target,
        settings: MachineSetupSettings,
        d: MachineDetected,
        old: Option<String>,
    ) {
        let ssh = self.ssh();
        if let Some(dir) = old {
            ssh.cleanup(&t, &dir).await;
        }
        let fail = |e: String| self.setup_fail(id, MachineSetupPhase::Planning, e);
        let (exe, software) = match running_binary() {
            Ok(v) => v,
            Err(e) => return fail(format!("host 실행 파일을 찾지 못했다: {e}")),
        };
        let here = (std::env::consts::OS, std::env::consts::ARCH);
        let file = match binary::pick(here, &software, (d.os.as_str(), d.arch.as_str()), &exe) {
            Source::Running { path } => path,
            Source::Release { version, key } => {
                let cache = self.state.join("ssh").join("release");
                match super::release::fetch(&cache, &release_base(), &version, &key).await {
                    Ok(f) => f.path,
                    Err(e) => return fail(e),
                }
            }
            Source::Refuse(why) => return fail(why),
        };
        let sha = match std::fs::read(&file) {
            Ok(b) => sha256_hex(&b),
            Err(e) => return fail(format!("host 실행 파일을 읽지 못했다: {e}")),
        };
        let placed = match ssh.upload(&t, &file, &sha, &software).await {
            Ok(p) => p,
            Err(e) => return fail(e),
        };
        let keep = placed.dir.clone();
        if self.setup_update(id, MachineSetupPhase::Planning, |j| j.remote_dir = Some(keep.clone())).is_none() {
            ssh.cleanup(&t, &keep).await;
            return;
        }
        let planned = async {
            let (Ok(c), Some(script)) = (choices(&settings, &d), probe_script(&placed.dir, &d.os, &sha)) else {
                return Err("설치 설정을 install choices로 옮기지 못했다".to_string());
            };
            let input = serde_json::to_vec(&c).map_err(|e| e.to_string())?;
            let (r, code) = ssh.exec_input(&t, &script, &input, STEP_TIMEOUT).await;
            if code != Some(0) {
                return Err(remote_why("원격 install 계획", &r, code));
            }
            let proposal: InstallProposal = serde_json::from_str(&r.stdout)
                .map_err(|_| "원격 install 계획을 읽지 못했다(같은 판인지 본다)".to_string())?;
            let summary = review(&proposal, &sha, &placed.version)?;
            let path = self.setup_plan_path(id);
            serde_json::to_vec(&proposal)
                .map_err(std::io::Error::other)
                .and_then(|b| {
                    aaa_common::private_file::directory(path.parent().unwrap())?;
                    let _ = std::fs::remove_file(&path);
                    aaa_common::private_file::write(&path, &b)
                })
                .map_err(|e| format!("계획을 host 작업 폴더에 두지 못했다: {e}"))?;
            Ok((proposal.result, summary))
        }
        .await;
        let phase = match &planned {
            Ok((result, _)) if result.status == InstallStatus::Failed => MachineSetupPhase::Failed,
            Ok((_, summary)) if summary.needs_user.is_empty() => MachineSetupPhase::Review,
            Ok(_) => MachineSetupPhase::NeedsUser,
            Err(_) => MachineSetupPhase::Failed,
        };
        // 확인을 기다리는 계획만 원격 임시 폴더를 남긴다(적용이 그 파일을 쓴다)
        if phase != MachineSetupPhase::Review {
            ssh.cleanup(&t, &keep).await;
            let _ = std::fs::remove_file(self.setup_plan_path(id));
        }
        self.setup_update(id, MachineSetupPhase::Planning, |j| {
            j.view.phase = phase;
            if phase != MachineSetupPhase::Review {
                j.remote_dir = None;
            }
            match planned {
                Ok((result, summary)) => {
                    j.view.error = result.error.as_ref().map(|e| format!("원격 install: {}", e.message));
                    j.view.review = Some(Box::new(summary));
                }
                Err(e) => j.view.error = Some(e),
            }
        });
    }

    pub(crate) fn setup_apply(
        self: &Arc<Self>,
        role: DeviceRole,
        id: &str,
        plan_sha256: &str,
        actor: &str,
    ) -> Result<aaa_proto::MachineSetupView, ErrorBody> {
        let (job, proposal) = {
            let _g = self.setup_gate.lock().unwrap();
            let mut job = self.setup_load(id)?;
            if job.view.phase != MachineSetupPhase::Review {
                return Err(conflict("이 작업은 설치를 확인하는 단계가 아니다(화면을 다시 읽는다)"));
            }
            if job.view.review.as_ref().map(|r| r.plan_sha256.as_str()) != Some(plan_sha256) {
                return Err(conflict("화면의 계획이 지금 계획과 다르다: 계획을 다시 본다"));
            }
            let raw = aaa_common::private_file::read(&self.setup_plan_path(id), OUTPUT_LIMIT as u64)
                .map_err(|_| conflict("계획 파일이 없다: 계획을 다시 만든다"))?;
            let proposal: InstallProposal =
                serde_json::from_slice(&raw).map_err(|_| conflict("계획 파일을 읽지 못했다: 계획을 다시 만든다"))?;
            if aaa_config::install::plan_sha256(&proposal.plan) != plan_sha256 {
                return Err(conflict("계획 파일이 화면의 계획과 다르다"));
            }
            job.view.phase = MachineSetupPhase::Installing;
            job.view.error = None;
            job.view.install = Some(Box::new(MachineSetupInstall {
                installation_id: None,
                last_event: None,
                result: None,
                machine: None,
                checks: vec![],
                check_pane: None,
            }));
            self.setup_save(&mut job);
            (job, proposal)
        };
        // 서명 승인은 사람이 누른 이 요청의 기기·actor로 한다(세션 안·Reader는 install_approve가 거부한다)
        let first = match self.setup_approve(role, actor, &proposal, None) {
            Ok(v) => v,
            Err(e) => {
                self.setup_fail(id, MachineSetupPhase::Installing, format!("서명 승인: {}", e.message));
                return Err(e);
            }
        };
        self.db.audit(actor, "machine_setup_apply", &format!("{} {plan_sha256}", job.view.target));
        let t = parse_target(&job.view.target)?;
        let host = self.clone();
        let (id, actor) = (id.to_string(), actor.to_string());
        let os = job.view.detected.as_ref().map(|d| d.os.clone()).unwrap_or_default();
        let dir = job.remote_dir.clone().unwrap_or_default();
        tokio::spawn(async move { host.setup_installing(&id, role, &actor, t, &os, &dir, proposal, first).await });
        Ok(self.setup_load(&job.view.job)?.view)
    }

    /// host install 발급자로 계획(+agent 신원이면 등록 grant)에 서명한다. 반환은 설치 ID
    fn setup_approve(
        &self,
        role: DeviceRole,
        actor: &str,
        p: &InstallProposal,
        again: Option<(&str, &str)>,
    ) -> Result<String, ErrorBody> {
        let scope = aaa_common::install_crypto::canonical(&p.permissions)
            .map_err(|_| ErrorBody::new(ErrorCode::Internal, "승인 범위를 정리하지 못했다"))?;
        let request = InstallApprovalRequest {
            installation_id: again.map(|a| a.0.to_string()),
            plan: p.plan.clone(),
            expected_plan_sha256: aaa_config::install::plan_sha256(&p.plan),
            permissions: p.permissions.clone(),
            expected_scope_sha256: sha256_hex(&scope),
            agent_identity: again.map(|a| a.1.to_string()),
        };
        match self.install_approve(role, actor, request)? {
            FrontResponse::InstallApproval { approval } => Ok(approval.installation_id),
            _ => Err(ErrorBody::new(ErrorCode::Internal, "서명 승인 답이 다르다")),
        }
    }

    /// 서명된 package를 원격 bootstrap에 준다
    async fn setup_bootstrap(&self, ssh: &Ssh, t: &Target, os: &str, dir: &str, id: &str) -> Result<Prepared, String> {
        let package =
            crate::installation::export(&self.state, id).map_err(|e| format!("서명 package: {}", e.message))?;
        let public = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(&package.issuer_public_key)
            .map_err(|_| "서명 package의 발급자 키를 읽지 못했다".to_string())?;
        let bytes = serde_json::to_vec(&package).map_err(|e| e.to_string())?;
        let script = bootstrap_script(dir, os, &package.plan.context.binary_sha256, &sha256_hex(&public))
            .ok_or("원격 bootstrap 명령을 만들지 못했다")?;
        let (r, code) = ssh.exec_input(t, &script, &bytes, STEP_TIMEOUT).await;
        if code != Some(0) {
            return Err(remote_why("원격 install 승인 받기", &r, code));
        }
        let prepared: Prepared =
            serde_json::from_str(r.stdout.trim()).map_err(|_| "원격 bootstrap 답을 읽지 못했다".to_string())?;
        if prepared.installation_id != id
            || prepared.plan_sha256 != package.authorization.claims.plan_sha256
            || prepared.agent_identity.as_deref().is_some_and(|a| !hex_ok(a))
        {
            return Err("원격 bootstrap 답이 승인한 설치와 다르다".into());
        }
        Ok(prepared)
    }

    #[allow(clippy::too_many_arguments)]
    async fn setup_installing(
        self: Arc<Self>,
        id: &str,
        role: DeviceRole,
        actor: &str,
        t: Target,
        os: &str,
        dir: &str,
        proposal: InstallProposal,
        installation: String,
    ) {
        let ssh = self.ssh();
        let step = |event: &str| {
            let e = event.to_string();
            let installation = installation.clone();
            self.setup_update(id, MachineSetupPhase::Installing, move |j| {
                let i = j.view.install.get_or_insert_with(|| {
                    Box::new(MachineSetupInstall {
                        installation_id: None,
                        last_event: None,
                        result: None,
                        machine: None,
                        checks: vec![],
                        check_pane: None,
                    })
                });
                i.installation_id = Some(installation);
                i.last_event = Some(e);
            })
        };
        let outcome = async {
            step("원격에 승인을 전한다");
            let prepared = self.setup_bootstrap(&ssh, &t, os, dir, &installation).await?;
            if let Some(agent) = prepared.agent_identity.as_deref() {
                // 원격이 준비한 agent 신원에만 묶은 등록 grant(믿은 host key의 SSH로 받았다)
                self.setup_approve(role, actor, &proposal, Some((&installation, agent)))
                    .map_err(|e| format!("등록 grant 서명: {}", e.message))?;
                self.setup_bootstrap(&ssh, &t, os, dir, &installation).await?;
            }
            step("원격 install 적용 중");
            let script = apply_script(dir, os, &installation).ok_or("원격 적용 명령을 만들지 못했다")?;
            let mut result = None;
            let (r, code) = ssh
                .stream(&t, &script, APPLY_TIMEOUT, |line| match stream_line(line) {
                    Line::Event(m) => {
                        step(&m);
                    }
                    Line::Result(res) => result = Some(res),
                    Line::Other => {}
                })
                .await;
            let result = result.ok_or_else(|| remote_why("원격 install 적용(결과 없음)", &r, code))?;
            if code.is_some_and(|c| u8::try_from(c).ok() != Some(result.exit_code)) {
                return Err(format!("원격 install의 종료 코드({code:?})가 결과({})와 다르다", result.exit_code));
            }
            Ok::<_, String>(result)
        }
        .await;
        ssh.cleanup(&t, dir).await;
        let _ = std::fs::remove_file(self.setup_plan_path(id));
        let name = proposal.plan.choices.name.clone().unwrap_or_default();
        let registered = self.db.machines().ok().is_some_and(|m| m.iter().any(|m| m.name == name));
        self.setup_update(id, MachineSetupPhase::Installing, |j| {
            j.remote_dir = None;
            let i = j.view.install.get_or_insert_with(|| {
                Box::new(MachineSetupInstall {
                    installation_id: Some(installation.clone()),
                    last_event: None,
                    result: None,
                    machine: None,
                    checks: vec![],
                    check_pane: None,
                })
            });
            match outcome {
                Err(e) => {
                    j.view.phase = MachineSetupPhase::Failed;
                    j.view.error = Some(e);
                }
                Ok(result) => {
                    j.view.phase = match result.status {
                        InstallStatus::Completed | InstallStatus::Configured | InstallStatus::Unchanged
                            if registered =>
                        {
                            i.machine = Some(name.clone());
                            MachineSetupPhase::Installed
                        }
                        InstallStatus::Completed | InstallStatus::Configured | InstallStatus::Unchanged => {
                            j.view.error = Some(format!("install은 끝났지만 host에 {name}이 등록되지 않았다"));
                            MachineSetupPhase::Failed
                        }
                        InstallStatus::NeedsUser => MachineSetupPhase::NeedsUser,
                        _ => {
                            j.view.error =
                                Some(result.error.as_ref().map_or("원격 install이 실패했다".into(), |e| {
                                    format!("원격 install: {}", e.message)
                                }));
                            MachineSetupPhase::Failed
                        }
                    };
                    i.result = Some(*result);
                }
            }
        });
    }
}

/// 작업 폴더 안 private 계획이 남았는가(시험)
#[cfg(test)]
pub(crate) fn plan_file(state: &std::path::Path, job: &str) -> PathBuf {
    state.join("ssh").join("jobs").join(job).join("proposal.json")
}

/// release 주소(시험은 임시 HTTP 서버로 바꿔 끼운다)
fn release_base() -> String {
    #[cfg(test)]
    if let Some(b) = tests_binary::base() {
        return b;
    }
    super::release::RELEASES.to_string()
}

#[cfg(test)]
pub(crate) mod tests_binary {
    use std::cell::RefCell;
    use std::path::PathBuf;
    thread_local! {
        static BINARY: RefCell<Option<(PathBuf, String)>> = const { RefCell::new(None) };
    }
    /// 이 스레드(current_thread runtime)의 host가 보낼 파일과 판
    pub(crate) fn set(v: Option<(PathBuf, String)>) {
        BINARY.with(|b| *b.borrow_mut() = v);
    }
    pub(super) fn get() -> Option<(PathBuf, String)> {
        BINARY.with(|b| b.borrow().clone())
    }
    thread_local! {
        static BASE: RefCell<Option<String>> = const { RefCell::new(None) };
    }
    /// 이 스레드의 host가 release를 받을 주소
    pub(crate) fn set_base(v: Option<String>) {
        BASE.with(|b| *b.borrow_mut() = v);
    }
    pub(super) fn base() -> Option<String> {
        BASE.with(|b| b.borrow().clone())
    }
}

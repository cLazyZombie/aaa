//! 보호된 SSH executor 입구. self-reported env/flag는 최초 pin 권한이 아니다.
use super::*;
use std::io::Read;

pub(super) struct VerifiedSsh(());
fn denied() -> Fault {
    Fault { code: "installation_verified_ssh_required", exit: 3, cause: None }
}
impl VerifiedSsh {
    fn current() -> Result<Self> {
        let ancestry = aaa_common::process::ancestors(std::process::id());
        if ancestry.iter().copied().any(system_sshd) { Ok(Self(())) } else { Err(denied()) }
    }
}
#[cfg(unix)]
fn system_sshd(pid: u32) -> bool {
    use std::os::unix::fs::MetadataExt;
    #[cfg(target_os = "macos")]
    let image = {
        let mut path = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
        let size = unsafe { libc::proc_pidpath(pid as i32, path.as_mut_ptr().cast(), path.len() as u32) };
        if size <= 0 {
            return false;
        }
        path.truncate(path.iter().position(|b| *b == 0).unwrap_or(path.len()));
        PathBuf::from(std::ffi::OsString::from_vec(path))
    };
    #[cfg(target_os = "macos")]
    use std::os::unix::ffi::OsStringExt;
    #[cfg(not(target_os = "macos"))]
    let image = match std::fs::read_link(format!("/proc/{pid}/exe")) {
        Ok(p) => p,
        Err(source_error) => {
            aaa_common::diagnostic::observe(&source_error, "오류 뒤 기존 복구·거부 분기를 따른다");
            return false;
        }
    };
    if image != Path::new("/usr/sbin/sshd") && image != Path::new("/usr/libexec/sshd-keygen-wrapper") {
        return false;
    }
    let Ok(meta) = image
        .symlink_metadata()
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다"))
    else {
        return false;
    };
    if !meta.is_file() || meta.uid() != 0 || meta.mode() & 0o022 != 0 {
        return false;
    }
    // 일반 사용자 세션에서 root sshd를 읽는다: `PROC_PIDTBSDINFO`는 다른 사용자 프로세스에 EPERM이라 짧은 정보를 쓴다
    #[cfg(target_os = "macos")]
    {
        aaa_common::process::uid(pid) == Some(0)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let Ok(status) = std::fs::read_to_string(format!("/proc/{pid}/status"))
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다"))
        else {
            return false;
        };
        status.lines().find_map(|l| l.strip_prefix("Uid:")).and_then(|v| v.split_whitespace().nth(1)) == Some("0")
    }
}
#[cfg(windows)]
fn system_sshd(pid: u32) -> bool {
    supervisor::system_sshd(pid)
}

/// 최초 pin 이전의 읽기 제안도 검증 SSH 경계에서만 private 채널로 돌려준다.
pub(super) fn probe(state: &Path, executor_sha256: &str) -> std::process::ExitCode {
    let result = (|| -> Result<()> {
        let _ssh = VerifiedSsh::current()?;
        let exe = std::env::current_exe().map_err(Fault::io)?;
        if !digest_ok(executor_sha256)
            || crate::update::sha256(&exe)
                .map_err(|source_error| aaa_common::diagnostic::context(source_error, Fault::conflict()))?
                != executor_sha256
        {
            return Err(Fault::conflict());
        }
        let mut bytes = Vec::new();
        std::io::stdin().lock().take(2 * 1024 * 1024 + 1).read_to_end(&mut bytes).map_err(Fault::io)?;
        if bytes.len() > 2 * 1024 * 1024 {
            return Err(Fault::conflict());
        }
        let choices: InstallChoices = serde_json::from_slice(&bytes)
            .map_err(|source_error| aaa_common::diagnostic::context(source_error, Fault::conflict()))?;
        let invocation = InstallInvocation { choices, dry_run: true, non_interactive: true, ..Default::default() };
        let (result, plan) = crate::install::detect::inspect(&invocation)
            .map_err(|source_error| aaa_common::diagnostic::context(source_error, Fault::conflict()))?;
        if Path::new(&plan.context.state_dir) != transaction::real_directory(state)?
            || plan.context.binary_sha256 != executor_sha256
        {
            return Err(Fault::conflict());
        }
        let permissions = scope::all(&plan)?;
        serde_json::to_writer(std::io::stdout().lock(), &InstallProposal { plan, permissions, result })
            .map_err(|source_error| aaa_common::diagnostic::context(source_error, Fault::conflict()))?;
        Ok(())
    })();
    std::process::ExitCode::from(result.map_or_else(|fault| fault.exit, |_| 0))
}

/// 이 stdout은 일반 NDJSON이 아니라 검증 SSH executor의 private 결과 채널이다.
#[derive(Serialize)]
struct Prepared {
    installation_id: String,
    plan_sha256: String,
    agent_identity: Option<String>,
}
pub(super) fn dispatch(state: &Path, executor_sha256: &str, issuer_sha256: &str) -> std::process::ExitCode {
    let result = (|| -> Result<()> {
        // 입력 읽기/파일 생성보다 먼저 OS ancestry와 고정 helper image를 확인한다.
        let ssh = VerifiedSsh::current()?;
        let exe = std::env::current_exe().map_err(Fault::io)?;
        if !digest_ok(executor_sha256)
            || crate::update::sha256(&exe)
                .map_err(|source_error| aaa_common::diagnostic::context(source_error, Fault::conflict()))?
                != executor_sha256
        {
            return Err(Fault::conflict());
        }
        let mut input = Vec::new();
        std::io::stdin().lock().take(2 * 1024 * 1024 + 1).read_to_end(&mut input).map_err(Fault::io)?;
        if input.len() > 2 * 1024 * 1024 {
            return Err(Fault::conflict());
        }
        let package: InstallBootstrapPackage = serde_json::from_slice(&input)
            .map_err(|source_error| aaa_common::diagnostic::context(source_error, Fault::conflict()))?;
        use base64::Engine as _;
        let public = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(&package.issuer_public_key)
            .map_err(|source_error| aaa_common::diagnostic::context(source_error, Fault::conflict()))?;
        if !digest_ok(issuer_sha256)
            || hash(&public) != issuer_sha256
            || package.plan.context.binary_sha256 != executor_sha256
        {
            return Err(Fault::conflict());
        }
        let state = transaction::real_directory(state)?;
        let authority = authorization::Authority::import(&state, &package, &ssh, aaa_common::util::now_ms())?;
        let plan = &package.plan;
        let auth = &package.authorization;
        let lease = authority.begin(
            auth,
            plan,
            &auth.claims.permissions,
            aaa_common::util::now_ms(),
            Some(&auth.claims.installation_id),
        )?;
        // 파일 적용 없이 승인된 신원 준비만 한다. 이후 같은 ID에 결속된 grant를 다시 전달한다.
        let identity = super::enrollment::prepare(plan, &lease)?;
        let response = Prepared {
            installation_id: auth.claims.installation_id.clone(),
            plan_sha256: auth.claims.plan_sha256.clone(),
            agent_identity: identity.map(|(_, tls)| tls.fingerprint),
        };
        serde_json::to_writer(std::io::stdout().lock(), &response).map_err(|source_error| {
            aaa_common::diagnostic::context(source_error, Fault::io(std::io::Error::other("private result")))
        })?;
        Ok(())
    })();
    // raw SSH/계정/서명/입력은 stderr나 public 결과에 넣지 않는다.
    std::process::ExitCode::from(result.map_or_else(|f| f.exit, |_| 0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::engine::transaction::tests::{Lab, tree};
    use authorization::Authority;
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
    #[test]
    fn ssh_import_binds_the_real_target_keeps_issuer_pin_and_cannot_reset_a_consumed_nonce() {
        let lab = Lab::new();
        let now = aaa_common::util::now_ms();
        let mut plan = lab.plan("target");
        plan.choices.enroll = Some(InstallEnroll::Grant);
        let plan = aaa_config::install::plan(plan.choices, plan.context).unwrap();
        let source = lab.0.join("issuer");
        let target = Path::new(&plan.context.state_dir);
        let issuer = Authority::initialize(&source).unwrap();
        let allowed = scope::all(&plan).unwrap();
        let auth = issuer.issue(&plan, allowed.clone(), now, 300_000).unwrap();
        let public = private::read(&source.join("install/authorization/issuer.pin"), 32).unwrap();
        let package = InstallBootstrapPackage {
            issuer_public_key: URL_SAFE_NO_PAD.encode(&public),
            plan: plan.clone(),
            authorization: auth.clone(),
        };
        // OS SSH 경계만 대체한다. 서명·ACL·계정·scope·nonce/실제 신원 준비는 production 그대로다.
        let ssh = VerifiedSsh(());
        let mut forged = package.clone();
        forged.authorization.signature = URL_SAFE_NO_PAD.encode([0u8; 64]);
        let before = tree(&lab.0);
        assert!(Authority::import(target, &forged, &ssh, now).is_err());
        assert_eq!(tree(&lab.0), before);
        assert!(!target.exists());
        let authority = Authority::import(target, &package, &ssh, now).unwrap();
        assert!(!target.join("install/authorization/issuer.pk8").exists(), "target는 host 발급 비밀키를 받지 않는다");
        let lease = authority.begin(&auth, &plan, &allowed, now, None).unwrap();
        let (_, identity) = super::super::enrollment::prepare(&plan, &lease).unwrap().unwrap();
        drop(lease);
        let same = Authority::import(target, &package, &ssh, now).unwrap();
        assert!(same.begin(&auth, &plan, &allowed, now, None).is_err(), "package 재전달은 nonce를 되살리지 않는다");
        assert!(same.begin(&auth, &plan, &allowed, now, Some(&auth.claims.installation_id)).is_ok());
        let other_signer =
            aaa_common::install_crypto::Signer::open(&aaa_common::install_crypto::Signer::generate().unwrap()).unwrap();
        let mut changed = package.clone();
        changed.issuer_public_key = URL_SAFE_NO_PAD.encode(other_signer.public_key());
        changed.authorization.claims.issuer = format!("local-ed25519:{}", hash(other_signer.public_key()));
        changed.authorization.signature = other_signer.sign_authorization(&changed.authorization.claims).unwrap();
        let before = tree(&lab.0);
        assert!(Authority::import(target, &changed, &ssh, now).is_err());
        assert_eq!(tree(&lab.0), before);
        let mut refresh = issuer.refresh(&auth.claims.installation_id, now + 300_000, 300_000).unwrap();
        let signer = aaa_common::install_crypto::Signer::open(
            &private::read(&source.join("install/authorization/issuer.pk8"), 4096).unwrap(),
        )
        .unwrap();
        let mut grant = InstallEnrollmentGrant {
            schema: InstallGrantSchema::V1,
            installation_id: auth.claims.installation_id.clone(),
            plan_sha256: auth.claims.plan_sha256.clone(),
            agent_identity: identity.fingerprint,
            host_identity: hash(b"host TLS"),
            issuer: refresh.claims.issuer.clone(),
            expires_at_ms: now + 600_000,
            nonce: aaa_common::util::new_id(),
            signature: String::new(),
        };
        grant.signature = signer.sign_enrollment(&grant).unwrap();
        refresh.enrollment = Some(grant);
        let refreshed = InstallBootstrapPackage { authorization: refresh.clone(), ..package };
        let remote = Authority::import(target, &refreshed, &ssh, now + 300_000).unwrap();
        assert!(remote.begin(&auth, &plan, &allowed, now + 300_000, Some(&auth.claims.installation_id)).is_err());
        let lease = remote.begin(&refresh, &plan, &allowed, now + 300_000, Some(&auth.claims.installation_id)).unwrap();
        assert_eq!(lease.id(), auth.claims.installation_id);
        assert!(Authority::initialize(target).is_err(), "로컬 발급 명령이 remote pin을 새 키로 바꾸지 않는다");
    }
}

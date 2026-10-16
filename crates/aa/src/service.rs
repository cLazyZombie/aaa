//! macOS 감독자 등록(SMAppService.agent). 기획서 §10.4 2, P0 E2.
//!
//! `aa.app/Contents/Library/LaunchAgents/<label>.plist`를 SMAppService로 등록한다.
//! 등록은 시스템 설정(로그인 항목·백그라운드 항목)을 바꾸므로 사용자가 직접 `aa service register`를 실행한다.
//! `aa service plan`은 바꾸지 않고 무엇이 등록되는지만 보인다.

use anyhow::{Context, Result, bail};
use std::path::PathBuf;

pub const AGENT_LABEL: &str = "dev.aa.agent";
/// host 등록 label(지금은 `--label`로 넘긴다. 기본값으로 쓰는 명령은 아직 없다)
#[allow(dead_code)]
pub const HOST_LABEL: &str = "dev.aa.host";

/// 실행 파일이 들어 있는 .app 번들 경로
pub fn bundle_root() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let app = contents.parent()?;
    (macos.file_name()? == "MacOS" && contents.file_name()? == "Contents" && app.extension()? == "app")
        .then(|| app.to_path_buf())
}

pub fn plist_path(label: &str) -> Result<PathBuf> {
    let app =
        bundle_root().context("aa.app 번들 안의 실행 파일이 아니다(예: /Applications/aa.app/Contents/MacOS/aa)")?;
    Ok(app.join("Contents/Library/LaunchAgents").join(format!("{label}.plist")))
}

pub fn plan(label: &str) -> Result<String> {
    let plist = plist_path(label)?;
    let text = std::fs::read_to_string(&plist).with_context(|| format!("{} 읽기", plist.display()))?;
    let app = bundle_root().unwrap();
    let mut out = String::new();
    out.push_str(&format!("등록 대상: SMAppService.agent(plistName: \"{label}.plist\")\n"));
    out.push_str(&format!("번들: {}\nplist: {}\n", app.display(), plist.display()));
    out.push_str(&format!("현재 상태: {}\n", status(label).unwrap_or_else(|e| format!("조회 실패: {e:#}"))));
    out.push_str("register 하면 macOS가 「백그라운드 항목이 추가됨」 알림을 띄우고,\n");
    out.push_str("시스템 설정 > 일반 > 로그인 항목 및 확장 프로그램의 「백그라운드에서 허용」 목록에 aa가 나타난다.\n");
    out.push_str("승인이 필요하면 상태가 requires_approval이 되며, 그 화면에서 aa 스위치를 켠다.\n\n");
    out.push_str("--- plist ---\n");
    out.push_str(&text);
    Ok(out)
}

#[cfg(target_os = "macos")]
fn service(label: &str) -> objc2::rc::Retained<objc2_service_management::SMAppService> {
    use objc2_foundation::NSString;
    use objc2_service_management::SMAppService;
    unsafe { SMAppService::agentServiceWithPlistName(&NSString::from_str(&format!("{label}.plist"))) }
}

#[cfg(target_os = "macos")]
pub fn status(label: &str) -> Result<String> {
    use objc2_service_management::SMAppServiceStatus as S;
    plist_path(label)?;
    let s = unsafe { service(label).status() };
    Ok(match s {
        S::NotRegistered => "not_registered".into(),
        S::Enabled => "enabled".into(),
        S::RequiresApproval => "requires_approval".into(),
        S::NotFound => "not_found".into(),
        other => format!("unknown({})", other.0),
    })
}

#[cfg(target_os = "macos")]
pub fn register(label: &str) -> Result<String> {
    plist_path(label)?;
    let svc = service(label);
    if let Err(e) = unsafe { svc.registerAndReturnError() } {
        bail!("등록 실패: {}", e.localizedDescription());
    }
    status(label)
}

#[cfg(target_os = "macos")]
pub fn unregister(label: &str) -> Result<String> {
    plist_path(label)?;
    let svc = service(label);
    if let Err(e) = unsafe { svc.unregisterAndReturnError() } {
        bail!("해제 실패: {}", e.localizedDescription());
    }
    status(label)
}

#[cfg(target_os = "macos")]
pub fn open_settings() {
    unsafe { objc2_service_management::SMAppService::openSystemSettingsLoginItems() };
}

#[cfg(not(target_os = "macos"))]
pub fn status(_label: &str) -> Result<String> {
    bail!("SMAppService는 macOS 전용이다")
}
#[cfg(not(target_os = "macos"))]
pub fn register(_label: &str) -> Result<String> {
    bail!("SMAppService는 macOS 전용이다")
}
#[cfg(not(target_os = "macos"))]
pub fn unregister(_label: &str) -> Result<String> {
    bail!("SMAppService는 macOS 전용이다")
}
#[cfg(not(target_os = "macos"))]
pub fn open_settings() {}

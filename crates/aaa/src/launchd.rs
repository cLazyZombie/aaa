//! 운영 감독자(결정 14): 사용자 LaunchAgent → 불변 launcher(`aaa-launcher.app`) → `aaa host`·`aaa agent`.
//!
//! - 개발자 도구·전체 디스크 접근에는 launcher를 한 번 등록한다. `aaa`는 다시 빌드·교체해도 등록을 다시 하지 않는다.
//! - 설치·해제는 launchd 설정을 바꾸므로 사람이 터미널에서 직접 한다(aaa 세션 안이거나 터미널이 아니면 거부).
//! - 멈추기·띄우기·다시 띄우기(herdr `server stop`, 차이 목록 4장 A #497)도 같은 문을 지난다. launchd가 KeepAlive로
//!   감독하므로 프로세스를 끝내면 다시 뜬다: 멈추기는 `bootout`(plist는 남아 다음 로그인에 뜬다), 띄우기는 `bootstrap`,
//!   다시 띄우기는 `kickstart -k`다. 세션은 session host가 가져 이어진다.
//! - launchd가 주는 PATH는 짧다(`/usr/bin:/bin:…`). 설치하는 셸의 PATH를 plist에 적어 `claude`·`codex`를 찾게 한다.
//! - 인스턴스(plan §8.294)는 label `dev.aaa.<이름>.<role>`, 로그 `~/Library/Logs/aaa-<이름>/`이고 plist가 `AAA_INSTANCE`를 넘긴다.
//!   기본 인스턴스는 지금 label·로그 그대로다(바꾸면 지금 서비스를 다시 설치해야 한다).

use std::path::{Path, PathBuf};
#[cfg(test)]
use std::process::Command;

use anyhow::{Result, bail};

pub fn label(role: &str) -> String {
    label_in(aaa_config::paths::instance().as_deref(), role)
}

fn label_in(instance: Option<&str>, role: &str) -> String {
    match instance {
        Some(name) => format!("dev.aaa.{name}.{role}"),
        None => format!("dev.aaa.{role}"),
    }
}

/// 사람에게 알리는 감독자 명령(`aaa --instance work service start --role agent`)
fn service_cmd(sub: &str, role: &str) -> String {
    format!("aaa{} service {sub} --role {role}", aaa_config::paths::instance_flag())
}

pub fn plist_path(role: &str) -> PathBuf {
    aaa_config::paths::home().join("Library/LaunchAgents").join(format!("{}.plist", label(role)))
}

pub fn default_launcher() -> PathBuf {
    aaa_config::paths::home().join("Applications/aaa-launcher.app/Contents/MacOS/aaa-launcher")
}

pub fn log_path(role: &str) -> PathBuf {
    log_path_in(&aaa_config::paths::home(), aaa_config::paths::instance().as_deref(), role)
}

fn log_path_in(home: &Path, instance: Option<&str>, role: &str) -> PathBuf {
    home.join("Library/Logs").join(aaa_config::paths::named("aaa", instance)).join(format!("{role}.log"))
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

pub struct Spec {
    pub role: String,
    pub launcher: PathBuf,
    pub aaa: PathBuf,
    /// plist에 적을 환경(PATH 등)
    pub env: Vec<(String, String)>,
}

pub fn plist(s: &Spec) -> String {
    let log = log_path(&s.role);
    let args = [s.launcher.to_string_lossy().to_string(), s.aaa.to_string_lossy().to_string(), s.role.clone()];
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n<dict>\n",
    );
    out.push_str(&format!("\t<key>Label</key>\n\t<string>{}</string>\n", label(&s.role)));
    out.push_str("\t<key>ProgramArguments</key>\n\t<array>\n");
    for a in &args {
        out.push_str(&format!("\t\t<string>{}</string>\n", xml(a)));
    }
    out.push_str("\t</array>\n");
    out.push_str("\t<key>RunAtLoad</key>\n\t<true/>\n\t<key>KeepAlive</key>\n\t<true/>\n");
    out.push_str("\t<key>ThrottleInterval</key>\n\t<integer>10</integer>\n");
    // GUI 로그인 세션(키체인이 풀린 곳)에서만, 사용자 작업과 같은 우선순위로 돈다
    out.push_str("\t<key>LimitLoadToSessionType</key>\n\t<string>Aqua</string>\n");
    out.push_str("\t<key>ProcessType</key>\n\t<string>Interactive</string>\n");
    let log = xml(&log.to_string_lossy());
    out.push_str(&format!("\t<key>StandardOutPath</key>\n\t<string>{log}</string>\n"));
    out.push_str(&format!("\t<key>StandardErrorPath</key>\n\t<string>{log}</string>\n"));
    if !s.env.is_empty() {
        out.push_str("\t<key>EnvironmentVariables</key>\n\t<dict>\n");
        for (k, v) in &s.env {
            out.push_str(&format!("\t\t<key>{}</key>\n\t\t<string>{}</string>\n", xml(k), xml(v)));
        }
        out.push_str("\t</dict>\n");
    }
    out.push_str("</dict>\n</plist>\n");
    out
}

/// 설치하는 셸에서 가져갈 환경. 인스턴스면 그 이름도 넘겨 launchd가 띄운 aaa가 같은 폴더를 본다
pub fn captured_env() -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> =
        ["PATH", "LANG"].iter().filter_map(|k| std::env::var(k).ok().map(|v| (k.to_string(), v))).collect();
    if let Some(name) = aaa_config::paths::instance() {
        env.push(("AAA_INSTANCE".into(), name));
    }
    env
}

fn uid() -> String {
    // SAFETY: getuid는 실패하지 않는다
    #[cfg(unix)]
    {
        unsafe { libc::getuid() }.to_string()
    }
    #[cfg(not(unix))]
    {
        String::new()
    }
}

fn launchctl(args: &[&str]) -> Result<(bool, String)> {
    crate::update::run_limited(Path::new("launchctl"), args)
}

fn service_target(role: &str) -> String {
    format!("gui/{}/{}", uid(), label(role))
}

/// launchd에 올라가 있으면 그 상태 줄(state·pid·last exit code), 아니면 None
fn launchd_state(role: &str) -> Result<Option<String>> {
    let (ok, text) = launchctl(&["print", &service_target(role)])?;
    Ok(ok.then(|| {
        let keep: Vec<&str> = text
            .lines()
            .map(str::trim)
            .filter(|l| l.starts_with("state =") || l.starts_with("pid =") || l.starts_with("last exit code ="))
            .collect();
        keep.join(" · ")
    }))
}

/// 읽기만 한다: plist 내용·경로 확인·launchd 상태
pub fn show(s: &Spec) -> Result<String> {
    let mut out = String::new();
    let p = plist_path(&s.role);
    let check = |path: &Path| if path.is_file() { "있음" } else { "없음" };
    out.push_str(&format!("label: {}\nplist: {} ({})\n", label(&s.role), p.display(), check(&p)));
    out.push_str(&format!("launcher: {} ({})\n", s.launcher.display(), check(&s.launcher)));
    out.push_str(&format!("aaa: {} ({})\nlog: {}\n", s.aaa.display(), check(&s.aaa), log_path(&s.role).display()));
    if cfg!(target_os = "macos") {
        match launchd_state(&s.role)? {
            Some(state) => out.push_str(&format!("launchd: 올라가 있다 · {state}\n")),
            None => out.push_str("launchd: 올라가 있지 않다\n"),
        }
    }
    out.push_str("--- 설치할 plist ---\n");
    out.push_str(&plist(s));
    Ok(out)
}

/// 사람이 터미널에서 직접 실행했는지 확인한다(agent가 대신 감독자를 바꾸지 못하게)
fn human_only(what: &str) -> Result<()> {
    crate::person::require(what)?;
    if crate::person::ask(&format!("{what}. 계속하려면 yes를 입력: "))? != "yes" {
        bail!("그만뒀다");
    }
    Ok(())
}

pub fn install(s: &Spec, replace: bool) -> Result<String> {
    if !cfg!(target_os = "macos") {
        bail!("launchd 설치는 macOS 전용이다");
    }
    for (what, p) in [("launcher", &s.launcher), ("aaa", &s.aaa)] {
        if !p.is_absolute() || !p.is_file() {
            bail!("{what} 실행 파일이 없다(절대 경로): {}", p.display());
        }
    }
    // 이름 변경(결정 18) 전 감독자가 남아 있으면 같은 port를 두고 다툰다. 인스턴스는 그 뒤에 생겨 옛 감독자가 없다
    let legacy = aaa_config::paths::home().join("Library/LaunchAgents").join(format!("dev.aa.{}.plist", s.role));
    if aaa_config::paths::instance().is_none() && legacy.exists() {
        bail!(
            "옛 감독자가 있다: {}. 먼저 옛 실행 파일로 내린다: ~/.local/bin/aa service uninstall --role {}",
            legacy.display(),
            s.role
        );
    }
    let p = plist_path(&s.role);
    if p.exists() && !replace {
        bail!("이미 있다: {} (바꾸려면 --replace)", p.display());
    }
    println!("{}", plist(s));
    human_only(&format!("{} LaunchAgent 설치", label(&s.role)))?;
    let target = format!("gui/{}", uid());
    if p.exists() {
        let _ = launchctl(&["bootout", &format!("{target}/{}", label(&s.role))]);
    }
    std::fs::create_dir_all(p.parent().unwrap())?;
    std::fs::create_dir_all(log_path(&s.role).parent().unwrap())?;
    let tmp = p.with_extension("plist.tmp");
    std::fs::write(&tmp, plist(s))?;
    std::fs::rename(&tmp, &p)?;
    let (ok, text) = launchctl(&["bootstrap", &target, &p.to_string_lossy()])?;
    if !ok {
        // 반쯤 설치된 상태를 남기지 않는다
        let _ = std::fs::remove_file(&p);
        bail!("launchctl bootstrap 실패(plist를 지웠다): {}", text.trim());
    }
    show(s)
}

/// 설치된 감독자(plist)가 있어야 멈추고 띄운다
fn installed(role: &str, what: &str) -> Result<PathBuf> {
    if !cfg!(target_os = "macos") {
        bail!("launchd {what}는 macOS 전용이다");
    }
    let p = plist_path(role);
    if !p.is_file() {
        bail!(
            "{} 감독자가 설치되지 않았다: {} 없음. 먼저 `{}`",
            label(role),
            p.display(),
            service_cmd("install", role)
        );
    }
    Ok(p)
}

/// 감독자가 aaa를 끝내고 다시 띄운다(`launchctl kickstart -k`). 업그레이드(새 binary로 교체) 뒤에 쓴다
pub fn restart(role: &str) -> Result<String> {
    restart_with(role, false)
}
/// host가 승인한 배포 executor에서만 부른다. CLI에 우회 인자는 없다.
pub(crate) fn restart_authorized(role: &str) -> Result<String> {
    restart_with(role, true)
}
fn restart_with(role: &str, authorized: bool) -> Result<String> {
    installed(role, "다시 띄우기")?;
    if launchd_state(role)?.is_none() {
        bail!("{} 감독자가 멈춰 있다. 띄우려면 `{}`", label(role), service_cmd("start", role));
    }
    if !authorized {
        human_only(&format!("{} 다시 띄우기(세션은 이어진다)", label(role)))?;
    }
    let (ok, text) = launchctl(&["kickstart", "-k", &service_target(role)])?;
    if !ok {
        bail!("launchctl kickstart 실패: {}", text.trim());
    }
    let state = launchd_state(role)?.unwrap_or_else(|| "올라가 있지 않다".into());
    Ok(format!("다시 띄웠다: {} · {state}\n", label(role)))
}

/// 감독자에서 내린다(`launchctl bootout`): 다시 뜨지 않는다. plist는 남아 다음 로그인 때 다시 뜬다
pub fn stop(role: &str) -> Result<String> {
    installed(role, "멈추기")?;
    if launchd_state(role)?.is_none() {
        bail!("{} 감독자는 이미 멈춰 있다", label(role));
    }
    human_only(&format!("{} 멈추기(세션은 남는다)", label(role)))?;
    let (ok, text) = launchctl(&["bootout", &service_target(role)])?;
    if !ok {
        bail!("launchctl bootout 실패: {}", text.trim());
    }
    Ok(format!(
        "멈췄다: {}. 다시 띄우려면 `{}`(plist가 남아 다음 로그인 때도 뜬다. 아주 내리려면 `{}`)\n",
        label(role),
        service_cmd("start", role),
        service_cmd("uninstall", role)
    ))
}

/// 멈춘 감독자를 다시 올린다(`launchctl bootstrap`)
pub fn start(role: &str) -> Result<String> {
    let p = installed(role, "띄우기")?;
    if launchd_state(role)?.is_some() {
        bail!("{} 감독자는 이미 올라가 있다. 다시 띄우려면 `{}`", label(role), service_cmd("restart", role));
    }
    human_only(&format!("{} 띄우기", label(role)))?;
    let (ok, text) = launchctl(&["bootstrap", &format!("gui/{}", uid()), &p.to_string_lossy()])?;
    if !ok {
        bail!("launchctl bootstrap 실패: {}", text.trim());
    }
    let state = launchd_state(role)?.unwrap_or_else(|| "올라가 있지 않다".into());
    Ok(format!("띄웠다: {} · {state}\n", label(role)))
}

pub fn uninstall(role: &str) -> Result<String> {
    if !cfg!(target_os = "macos") {
        bail!("launchd 해제는 macOS 전용이다");
    }
    human_only(&format!("{} LaunchAgent 해제", label(role)))?;
    let (_, text) = launchctl(&["bootout", &format!("gui/{}/{}", uid(), label(role))])?;
    let p = plist_path(role);
    if p.exists() {
        std::fs::remove_file(&p)?;
    }
    Ok(format!("해제했다: {} {}\n", label(role), text.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 기본 인스턴스의 label·로그는 지금 그대로다(바꾸면 지금 서비스를 다시 설치해야 한다)
    #[test]
    fn instance_labels_and_logs() {
        let home = Path::new("/opt/example");
        assert_eq!(label_in(None, "host"), "dev.aaa.host");
        assert_eq!(log_path_in(home, None, "agent"), Path::new("/opt/example/Library/Logs/aaa/agent.log"));
        assert_eq!(label_in(Some("work"), "host"), "dev.aaa.work.host");
        assert_eq!(log_path_in(home, Some("work"), "agent"), Path::new("/opt/example/Library/Logs/aaa-work/agent.log"));
    }

    #[test]
    fn plist_shape() {
        let s = Spec {
            role: "agent".into(),
            launcher: "/opt/example/Applications/aaa-launcher.app/Contents/MacOS/aaa-launcher".into(),
            aaa: "/opt/example/.local/bin/a&a".into(),
            env: vec![("PATH".into(), "/opt/homebrew/bin:/usr/bin".into())],
        };
        let x = plist(&s);
        assert!(x.contains("<string>dev.aaa.agent</string>"));
        assert!(x.contains(
            "<string>/opt/example/Applications/aaa-launcher.app/Contents/MacOS/aaa-launcher</string>\n\t\t<string>/opt/example/.local/bin/a&amp;a</string>\n\t\t<string>agent</string>"
        ));
        assert!(x.contains("<key>PATH</key>\n\t\t<string>/opt/homebrew/bin:/usr/bin</string>"));
        assert!(x.contains("<key>LimitLoadToSessionType</key>\n\t<string>Aqua</string>"));
        // plutil이 있으면 형식도 확인한다(macOS)
        if Path::new("/usr/bin/plutil").exists() {
            let f = std::env::temp_dir().join(format!("aaa-plist-{}.plist", std::process::id()));
            std::fs::write(&f, &x).unwrap();
            let o = Command::new("/usr/bin/plutil").arg("-lint").arg(&f).output().unwrap();
            std::fs::remove_file(&f).unwrap();
            assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stdout));
        }
    }
}

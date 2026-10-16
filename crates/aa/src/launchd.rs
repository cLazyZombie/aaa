//! 운영 감독자(결정 14): 사용자 LaunchAgent → 불변 launcher(`aa-launcher.app`) → `aa host`·`aa agent`.
//!
//! - 개발자 도구·전체 디스크 접근에는 launcher를 한 번 등록한다. `aa`는 다시 빌드·교체해도 등록을 다시 하지 않는다.
//! - 설치·해제는 launchd 설정을 바꾸므로 사람이 터미널에서 직접 한다(aa 세션 안이거나 터미널이 아니면 거부).
//! - launchd가 주는 PATH는 짧다(`/usr/bin:/bin:…`). 설치하는 셸의 PATH를 plist에 적어 `claude`·`codex`를 찾게 한다.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

pub fn label(role: &str) -> String {
    format!("dev.aa.{role}")
}

pub fn plist_path(role: &str) -> PathBuf {
    aa_config::paths::home().join("Library/LaunchAgents").join(format!("{}.plist", label(role)))
}

pub fn default_launcher() -> PathBuf {
    aa_config::paths::home().join("Applications/aa-launcher.app/Contents/MacOS/aa-launcher")
}

pub fn log_path(role: &str) -> PathBuf {
    aa_config::paths::home().join("Library/Logs/aa").join(format!("{role}.log"))
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

pub struct Spec {
    pub role: String,
    pub launcher: PathBuf,
    pub aa: PathBuf,
    /// plist에 적을 환경(PATH 등)
    pub env: Vec<(String, String)>,
}

pub fn plist(s: &Spec) -> String {
    let log = log_path(&s.role);
    let args = [s.launcher.to_string_lossy().to_string(), s.aa.to_string_lossy().to_string(), s.role.clone()];
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

/// 설치하는 셸에서 가져갈 환경
pub fn captured_env() -> Vec<(String, String)> {
    ["PATH", "LANG"].iter().filter_map(|k| std::env::var(k).ok().map(|v| (k.to_string(), v))).collect()
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
    let o = Command::new("launchctl").args(args).output().context("launchctl 실행")?;
    let text = String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr);
    Ok((o.status.success(), text))
}

/// 읽기만 한다: plist 내용·경로 확인·launchd 상태
pub fn show(s: &Spec) -> Result<String> {
    let mut out = String::new();
    let p = plist_path(&s.role);
    let check = |path: &Path| if path.is_file() { "있음" } else { "없음" };
    out.push_str(&format!("label: {}\nplist: {} ({})\n", label(&s.role), p.display(), check(&p)));
    out.push_str(&format!("launcher: {} ({})\n", s.launcher.display(), check(&s.launcher)));
    out.push_str(&format!("aa: {} ({})\nlog: {}\n", s.aa.display(), check(&s.aa), log_path(&s.role).display()));
    if cfg!(target_os = "macos") {
        let (ok, text) = launchctl(&["print", &format!("gui/{}/{}", uid(), label(&s.role))])?;
        if ok {
            let keep: Vec<&str> = text
                .lines()
                .map(str::trim)
                .filter(|l| l.starts_with("state =") || l.starts_with("pid =") || l.starts_with("last exit code ="))
                .collect();
            out.push_str(&format!("launchd: 올라가 있다 · {}\n", keep.join(" · ")));
        } else {
            out.push_str("launchd: 올라가 있지 않다\n");
        }
    }
    out.push_str("--- 설치할 plist ---\n");
    out.push_str(&plist(s));
    Ok(out)
}

/// 사람이 터미널에서 직접 실행했는지 확인한다(agent가 대신 감독자를 바꾸지 못하게)
fn human_only(what: &str) -> Result<()> {
    use std::io::{BufRead as _, IsTerminal as _, Write as _};
    if std::env::var_os("AA_SESSION_ID").is_some() || std::env::var_os("AA_HOOK_SOCK").is_some() {
        bail!("{what}: aa 세션 안에서는 하지 않는다. 사람이 자기 터미널에서 실행한다");
    }
    if !std::io::stdin().is_terminal() {
        bail!("{what}: 터미널에서 사람이 직접 실행한다");
    }
    print!("{what}. 계속하려면 yes를 입력: ");
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    if line.trim() != "yes" {
        bail!("그만뒀다");
    }
    Ok(())
}

pub fn install(s: &Spec, replace: bool) -> Result<String> {
    if !cfg!(target_os = "macos") {
        bail!("launchd 설치는 macOS 전용이다");
    }
    for (what, p) in [("launcher", &s.launcher), ("aa", &s.aa)] {
        if !p.is_absolute() || !p.is_file() {
            bail!("{what} 실행 파일이 없다(절대 경로): {}", p.display());
        }
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
        bail!("launchctl bootstrap 실패: {}", text.trim());
    }
    show(s)
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

    #[test]
    fn plist_shape() {
        let s = Spec {
            role: "agent".into(),
            launcher: "/Users/u/Applications/aa-launcher.app/Contents/MacOS/aa-launcher".into(),
            aa: "/Users/u/.local/bin/a&a".into(),
            env: vec![("PATH".into(), "/opt/homebrew/bin:/usr/bin".into())],
        };
        let x = plist(&s);
        assert!(x.contains("<string>dev.aa.agent</string>"));
        assert!(x.contains(
            "<string>/Users/u/Applications/aa-launcher.app/Contents/MacOS/aa-launcher</string>\n\t\t<string>/Users/u/.local/bin/a&amp;a</string>\n\t\t<string>agent</string>"
        ));
        assert!(x.contains("<key>PATH</key>\n\t\t<string>/opt/homebrew/bin:/usr/bin</string>"));
        assert!(x.contains("<key>LimitLoadToSessionType</key>\n\t<string>Aqua</string>"));
        // plutil이 있으면 형식도 확인한다(macOS)
        if Path::new("/usr/bin/plutil").exists() {
            let f = std::env::temp_dir().join(format!("aa-plist-{}.plist", std::process::id()));
            std::fs::write(&f, &x).unwrap();
            let o = Command::new("/usr/bin/plutil").arg("-lint").arg(&f).output().unwrap();
            std::fs::remove_file(&f).unwrap();
            assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stdout));
        }
    }
}

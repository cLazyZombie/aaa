//! launchd/systemd 경계. 기록한 단위 파일·설정 경로가 모두 맞는 자기 서비스만 조작한다.
use super::super::{
    ServiceKind,
    flow::{Layout, ServiceInfo},
};
use aaa_common::child_process::StdCommandExt as _;
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Spec {
    pub kind: ServiceKind,
    pub name: String,
    pub exe: PathBuf,
    pub home: PathBuf,
    pub config: PathBuf,
    pub state: PathBuf,
    pub instance: Option<String>,
    pub uid: u32,
    pub user: String,
    pub file: PathBuf,
    pub launcher: Option<PathBuf>,
    pub env: BTreeMap<String, String>,
}
impl Spec {
    pub fn path(layout: &Layout) -> PathBuf {
        layout.state.join("setup-unix.json")
    }
    pub fn new(layout: &Layout, user: String, launcher: Option<PathBuf>) -> Result<Self> {
        let name = layout.service();
        let file = match layout.kind {
            ServiceKind::Launchd => layout.home.join("Library/LaunchAgents").join(format!("{name}.plist")),
            ServiceKind::SystemdUser => std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| layout.home.join(".config"))
                .join("systemd/user")
                .join(format!("{name}.service")),
            ServiceKind::SystemdSystem => Path::new("/etc/systemd/system").join(format!("{name}.service")),
            ServiceKind::WindowsScm => bail!("Unix에서 Windows 서비스를 설치하지 않습니다"),
        };
        let mut env: BTreeMap<String, String> = ["PATH", "LANG", "LC_ALL", "LC_CTYPE", "TERM"]
            .into_iter()
            .filter_map(|key| std::env::var(key).ok().map(|v| (key.into(), v)))
            .collect();
        for (key, value) in
            [("HOME", &layout.home), ("AAA_CONFIG_DIR", &layout.config), ("AAA_STATE_DIR", &layout.state)]
        {
            env.insert(key.into(), value.to_string_lossy().into_owned());
        }
        if let Some(instance) = &layout.instance {
            env.insert("AAA_INSTANCE".into(), instance.clone());
        }
        let this = Self {
            kind: layout.kind,
            name,
            exe: layout.exe.clone(),
            home: layout.home.clone(),
            config: layout.config.clone(),
            state: layout.state.clone(),
            instance: layout.instance.clone(),
            uid: unsafe { libc::getuid() },
            user,
            file,
            launcher,
            env,
        };
        this.validate()?;
        Ok(this)
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.uid != 0
                && !self.user.is_empty()
                && self.user.chars().all(|c| c.is_ascii_alphanumeric() || "_.-".contains(c)),
            "서비스는 일반 사용자 계정으로 실행해야 합니다"
        );
        for path in
            [&self.home, &self.exe, &self.config, &self.state, &self.file].into_iter().chain(self.launcher.iter())
        {
            ensure!(
                path.is_absolute() && !path.components().any(|c| matches!(c, std::path::Component::ParentDir)),
                "서비스 경로는 상위 이동 없는 절대 경로여야 합니다: {}",
                path.display()
            );
            checked(&path.to_string_lossy())?;
        }
        for (key, value) in &self.env {
            ensure!(
                [
                    "PATH",
                    "LANG",
                    "LC_ALL",
                    "LC_CTYPE",
                    "TERM",
                    "HOME",
                    "AAA_CONFIG_DIR",
                    "AAA_STATE_DIR",
                    "AAA_INSTANCE"
                ]
                .contains(&key.as_str()),
                "서비스 기록에 허용하지 않은 환경 항목이 있습니다: {key}"
            );
            checked(value)?;
        }
        ensure!(self.kind != ServiceKind::Launchd || self.launcher.is_some(), "macOS launcher가 없습니다");
        Ok(())
    }
    pub fn argv(&self) -> Vec<String> {
        let mut args = Vec::new();
        if let Some(launcher) = &self.launcher {
            args.push(launcher.to_string_lossy().into_owned());
        }
        args.push(self.exe.to_string_lossy().into_owned());
        if let Some(instance) = &self.instance {
            args.extend(["--instance".into(), instance.clone()]);
        }
        args.extend([
            "--config-dir".into(),
            self.config.to_string_lossy().into_owned(),
            "--state-dir".into(),
            self.state.to_string_lossy().into_owned(),
            "setup-service".into(),
        ]);
        args
    }
    pub fn render(&self) -> String {
        let log = self.state.join("agent-service.log");
        if self.kind == ServiceKind::Launchd {
            return crate::launchd::render(
                &self.name,
                &self.argv(),
                &self.env.iter().map(|(k, v)| (k.clone(), v.clone())).collect::<Vec<_>>(),
                &log,
                true,
            );
        }
        let mut out = String::from(
            "# aaa setup이 만든 서비스. setup-unix.json과 함께 소유권을 확인한다.\n[Unit]\nDescription=aaa agent\nAfter=network-online.target\n\n[Service]\nType=exec\n",
        );
        if self.kind == ServiceKind::SystemdSystem {
            out.push_str(&format!("User={}\n", self.user));
        }
        out.push_str(&format!(
            "WorkingDirectory={}\nExecStart={}\n",
            self.home.to_string_lossy().replace('%', "%%"),
            self.argv()
                .iter()
                .enumerate()
                // ':'는 argv 전체의 환경 변수 치환을 끈다. 실행 파일 이름의 '$'도 그대로 쓴다.
                .map(|(i, a)| quote(&if i == 0 { format!(":{a}") } else { a.clone() }))
                .collect::<Vec<_>>()
                .join(" ")
        ));
        for (key, value) in &self.env {
            out.push_str(&format!("Environment={}\n", quote(&format!("{key}={value}"))));
        }
        out.push_str(&format!(
            "StandardOutput=append:{}\nStandardError=append:{}\n",
            log.to_string_lossy().replace('%', "%%"),
            log.to_string_lossy().replace('%', "%%")
        ));
        out.push_str("Restart=on-failure\nRestartSec=5\nTimeoutStopSec=60\nKillMode=mixed\nUMask=0077\n\n[Install]\n");
        out.push_str(if self.kind == ServiceKind::SystemdSystem {
            "WantedBy=multi-user.target\n"
        } else {
            "WantedBy=default.target\n"
        });
        out
    }
    pub fn target(&self) -> String {
        format!("gui/{}/{}", self.uid, self.name)
    }
    fn unit(&self) -> String {
        format!("{}.service", self.name)
    }
    pub fn identity_matches(&self, other: &Self) -> bool {
        self.kind == other.kind
            && self.name == other.name
            && self.exe == other.exe
            && self.home == other.home
            && self.config == other.config
            && self.state == other.state
            && self.instance == other.instance
            && self.uid == other.uid
            && self.user == other.user
            && self.file == other.file
    }
}
fn checked(value: &str) -> Result<()> {
    ensure!(!value.chars().any(char::is_control), "서비스 경로·환경에 제어 문자를 넣을 수 없습니다");
    Ok(())
}
fn quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\"").replace('%', "%%"))
}

pub(super) struct Manager {
    pub spec: Spec,
}
impl Manager {
    fn command(&self, program: &str, args: &[&str], privileged: bool) -> Result<(bool, String)> {
        let (bin, args) = if privileged {
            let mut prefixed = vec!["-n", "--", program];
            prefixed.extend_from_slice(args);
            ("/usr/bin/sudo", prefixed)
        } else {
            (program, args.to_vec())
        };
        crate::update::run_limited(Path::new(bin), &args).with_context(|| format!("{} {} 실행", bin, args.join(" ")))
    }
    fn execute(&self, program: &str, args: &[&str], privileged: bool) -> Result<()> {
        let (ok, text) = self.command(program, args, privileged)?;
        ensure!(ok, "{} {} 실패: {}", program, args.join(" "), text.trim());
        Ok(())
    }
    fn systemctl(&self, args: &[&str], mutation: bool) -> Result<(bool, String)> {
        let mut scoped = if self.spec.kind == ServiceKind::SystemdUser { vec!["--user"] } else { vec!["--system"] };
        scoped.extend_from_slice(args);
        self.command("/usr/bin/systemctl", &scoped, mutation && self.spec.kind == ServiceKind::SystemdSystem)
    }
    fn systemctl_ok(&self, args: &[&str]) -> Result<()> {
        let (ok, text) = self.systemctl(args, true)?;
        ensure!(ok, "systemctl {} 실패: {}", args.join(" "), text.trim());
        Ok(())
    }
    pub fn available(&self) -> Result<()> {
        if self.spec.kind == ServiceKind::Launchd {
            self.execute("/bin/launchctl", &["print", &format!("gui/{}", self.spec.uid)], false)
                .context("GUI 로그인 세션이 없습니다. 로그인한 사용자 터미널에서 실행하세요")
        } else {
            let (ok, text) = self.systemctl(&["show", "--property=Version", "--value"], false)?;
            ensure!(ok && !text.trim().is_empty(), "systemd 관리자를 확인하지 못했습니다: {}", text.trim());
            Ok(())
        }
    }
    pub fn authorize(&self) -> Result<()> {
        if self.spec.kind == ServiceKind::SystemdSystem {
            println!("시스템 서비스 등록 권한을 확인합니다. 암호가 필요하면 sudo가 이 터미널에서 직접 받습니다.");
            let status = std::process::Command::new("/usr/bin/sudo")
                .arg("-v")
                .status_guarded()
                .context("sudo 권한 확인 실행")?;
            ensure!(status.success(), "sudo 권한 확인 실패: {status}");
        }
        Ok(())
    }
    /// 일반 파일만 읽는다. 링크·수동 변경·다른 설치기의 파일은 조작하지 않는다.
    pub fn owned(&self) -> Result<bool> {
        let meta = match self.spec.file.symlink_metadata() {
            Ok(meta) => meta,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e).with_context(|| format!("{} 조회", self.spec.file.display())),
        };
        ensure!(
            meta.is_file() && !meta.file_type().is_symlink(),
            "서비스 파일이 일반 파일이 아닙니다: {}",
            self.spec.file.display()
        );
        ensure!(
            std::fs::read(&self.spec.file)? == self.spec.render().as_bytes(),
            "서비스 파일이 이 설치기의 기록과 다릅니다. 바꾸지 않습니다: {}",
            self.spec.file.display()
        );
        Ok(true)
    }
    pub fn query(&self) -> Result<Option<ServiceInfo>> {
        let owned = self.owned()?;
        let (pid, loaded) = if self.spec.kind == ServiceKind::Launchd {
            let (ok, text) = self.command("/bin/launchctl", &["print", &self.spec.target()], false)?;
            if !ok {
                ensure!(text.contains("Could not find service"), "launchctl print 실패: {}", text.trim());
                (0, false)
            } else {
                ensure!(owned, "같은 이름의 다른 LaunchAgent가 실행 중입니다: {}", self.spec.name);
                let path = text
                    .lines()
                    .find_map(|s| s.trim().strip_prefix("path = "))
                    .context("LaunchAgent 경로가 없습니다")?;
                ensure!(
                    Path::new(path).canonicalize()? == self.spec.file.canonicalize()?,
                    "실행 중 LaunchAgent 경로가 다릅니다"
                );
                let pid = text
                    .lines()
                    .find_map(|s| s.trim().strip_prefix("pid = "))
                    .map(str::parse)
                    .transpose()?
                    .unwrap_or(0);
                (pid, true)
            }
        } else {
            let (ok, text) = self.systemctl(
                &[
                    "show",
                    &self.spec.unit(),
                    "--property=LoadState,MainPID,FragmentPath,DropInPaths,ActiveState,Result,ExecMainStatus",
                ],
                false,
            )?;
            ensure!(ok, "systemctl show 실패: {}", text.trim());
            let properties: BTreeMap<_, _> = text.lines().filter_map(|s| s.split_once('=')).collect();
            if properties.get("LoadState") == Some(&"not-found") {
                (0, false)
            } else {
                ensure!(owned, "같은 이름의 다른 systemd 서비스가 있습니다: {}", self.spec.name);
                ensure!(
                    properties.get("FragmentPath") == Some(&self.spec.file.to_str().context("unit 경로 UTF-8")?)
                        && properties.get("DropInPaths").is_none_or(|s| s.is_empty()),
                    "systemd 단위 경로·drop-in이 설치 기록과 다릅니다"
                );
                (properties.get("MainPID").context("systemd MainPID 누락")?.parse()?, true)
            }
        };
        Ok((owned || loaded).then(|| ServiceInfo {
            name: self.spec.name.clone(),
            running: pid != 0,
            pid,
            automatic: true,
            account: self.spec.user.clone(),
            command: self.spec.argv().join(" "),
        }))
    }
    pub fn create(&self) -> Result<()> {
        ensure!(!self.owned()?, "서비스가 이미 있습니다");
        let content = self.spec.render();
        if self.spec.kind == ServiceKind::SystemdSystem {
            let stage = self.spec.state.join("setup-unit.tmp");
            aaa_common::util::write_private(&stage, content.as_bytes())?;
            let temporary = self.spec.file.with_extension(format!("service.{}.tmp", aaa_common::util::new_id()));
            let result = (|| {
                self.execute(
                    "/usr/bin/install",
                    &["-m", "644", "--", &stage.to_string_lossy(), &temporary.to_string_lossy()],
                    true,
                )?;
                self.execute(
                    "/usr/bin/ln",
                    &["--", &temporary.to_string_lossy(), &self.spec.file.to_string_lossy()],
                    true,
                )
            })();
            let cleanup = self.execute("/usr/bin/rm", &["-f", "--", &temporary.to_string_lossy()], true);
            let stage_cleanup = std::fs::remove_file(&stage).context("임시 unit 파일 정리");
            // 링크 생성이 성공했으면 호출자가 서비스를 되돌릴 수 있도록 성공을 전달한다.
            // 정리 오류 때문에 등록 성공을 숨기면 rollback 목록에 서비스가 빠진다.
            for error in [cleanup.err(), stage_cleanup.err()].into_iter().flatten() {
                eprintln!("서비스 등록 임시 파일 정리 실패: {error:#}");
            }
            result?;
        } else {
            use std::os::unix::fs::OpenOptionsExt;
            std::fs::create_dir_all(self.spec.file.parent().context("서비스 파일 부모")?)?;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&self.spec.file)
                .with_context(|| format!("서비스 파일 {} 만들기", self.spec.file.display()))?;
            if let Err(error) = file.write_all(content.as_bytes()).and_then(|()| file.sync_all()) {
                let cleanup = std::fs::remove_file(&self.spec.file);
                bail!("서비스 파일 쓰기 실패: {error}; 정리: {cleanup:?}");
            }
        }
        Ok(())
    }
    pub fn start(&self) -> Result<u32> {
        ensure!(self.owned()?, "등록된 서비스가 없습니다");
        // systemd 시스템 관리자는 User=로 전환하기 전에 로그를 연다. 사용자가 먼저 만들어
        // root 소유 로그가 생기지 않게 한다. 기존 로그·링크를 덮어쓰거나 권한을 바꾸지 않는다.
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
        let log = self.spec.state.join("agent-service.log");
        let file = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&log)
            .with_context(|| format!("서비스 로그 {} 준비", log.display()))?;
        let meta = file.metadata()?;
        ensure!(meta.is_file() && meta.uid() == self.spec.uid, "서비스 로그가 현재 사용자 소유 일반 파일이 아닙니다");
        drop(file);
        if self.spec.kind == ServiceKind::Launchd {
            self.execute(
                "/bin/launchctl",
                &["bootstrap", &format!("gui/{}", self.spec.uid), &self.spec.file.to_string_lossy()],
                false,
            )?;
        } else {
            self.systemctl_ok(&["daemon-reload"])?;
            let (ok, diagnosis) =
                self.systemctl(&["show", &self.spec.unit(), "--property=LoadState,LoadError"], false)?;
            ensure!(
                ok && diagnosis.lines().any(|line| line == "LoadState=loaded"),
                "systemd 단위 파일을 읽지 못했습니다: {}",
                diagnosis.trim()
            );
            self.systemctl_ok(&["enable", &self.spec.unit()])?;
            self.systemctl_ok(&["start", &self.spec.unit()])?;
        }
        let started = Instant::now();
        loop {
            if let Some(info) = self.query()?
                && info.running
            {
                return Ok(info.pid);
            }
            ensure!(
                started.elapsed() < Duration::from_secs(30),
                "서비스 PID를 30초 안에 확인하지 못했습니다: {}",
                self.spec.name
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    pub fn stop(&self) -> Result<()> {
        if !self.owned()? {
            return Ok(());
        }
        if self.spec.kind == ServiceKind::Launchd {
            let (ok, text) = self.command("/bin/launchctl", &["bootout", &self.spec.target()], false)?;
            ensure!(
                ok || text.contains("No such process") || text.contains("Could not find service"),
                "launchctl bootout 실패: {}",
                text.trim()
            );
        } else {
            self.systemctl_ok(&["stop", &self.spec.unit()])?;
        }
        Ok(())
    }
    pub fn delete(&self) -> Result<()> {
        if !self.owned()? {
            return Ok(());
        }
        ensure!(self.query()?.is_none_or(|info| !info.running), "실행 중인 서비스는 먼저 멈춰야 제거할 수 있습니다");
        if self.spec.kind != ServiceKind::Launchd {
            self.systemctl_ok(&["disable", &self.spec.unit()])?;
        }
        if self.spec.kind == ServiceKind::SystemdSystem {
            self.execute("/usr/bin/rm", &["--", &self.spec.file.to_string_lossy()], true)?;
        } else {
            std::fs::remove_file(&self.spec.file)?;
        }
        if self.spec.kind != ServiceKind::Launchd {
            self.systemctl_ok(&["daemon-reload"])?;
        }
        Ok(())
    }
}

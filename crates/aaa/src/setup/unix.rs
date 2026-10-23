//! macOS·Linux 설치 입구. 시스템 권한은 일반 사용자가 자기 터미널에서 확인한다.
use super::{
    Args, ServiceKind,
    flow::{
        Account, Cancelled, InstallFile, Layout, Logon, Receipt, Right, Secret, ServiceDef, ServiceInfo, Setup, System,
        Ui, code_lines,
    },
};
use anyhow::{Context, Result, bail, ensure};
use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, Instant};

mod launcher;
mod runtime;
mod service;
mod terminal;
#[cfg(test)]
mod tests;
pub(crate) use runtime::dispatch;

fn layout(args: &Args) -> Result<Layout> {
    let instance = aaa_config::paths::instance();
    let config = aaa_config::paths::config_dir();
    let state = aaa_config::paths::state_dir();
    let home = aaa_config::paths::home();
    let receipt = Receipt::read(&state.join("setup.json"));
    let kind = args.service.or(receipt.map(|r| r.kind)).unwrap_or(if cfg!(target_os = "macos") {
        ServiceKind::Launchd
    } else {
        ServiceKind::SystemdUser
    });
    ensure!(
        matches!(
            (std::env::consts::OS, kind),
            ("macos", ServiceKind::Launchd) | ("linux", ServiceKind::SystemdUser | ServiceKind::SystemdSystem)
        ),
        "이 OS에서 {}를 쓸 수 없습니다",
        kind.title()
    );
    ensure!(kind == ServiceKind::Launchd || args.launcher.is_none(), "--launcher는 macOS 전용입니다");
    let explicit_dirs = std::env::var_os("AAA_CONFIG_DIR").is_some() || std::env::var_os("AAA_STATE_DIR").is_some();
    // 상태 폴더와 나란한 고유 위치: --purge도 다른 명령에 쓰는 실행 파일은 보존한다.
    let exe = if instance.is_none() && !explicit_dirs {
        home.join(".local/bin/aaa")
    } else {
        state
            .with_file_name(format!("{}-bin", state.file_name().context("상태 폴더 이름")?.to_string_lossy()))
            .join("aaa")
    };
    for path in [&home, &config, &state, &exe] {
        ensure!(path.is_absolute(), "설치 경로는 절대 경로여야 합니다: {}", path.display());
    }
    Ok(Layout {
        kind,
        instance,
        source: std::env::current_exe()?.canonicalize()?,
        exe,
        config,
        state,
        explicit_dirs: true,
        home,
    })
}

pub(super) fn run(args: &Args) -> ExitCode {
    let result = (|| -> Result<()> {
        if !args.status {
            crate::person::require("aaa setup 서비스 설치·제거")?;
        }
        ensure!(
            unsafe { libc::getuid() } != 0,
            "일반 사용자로 실행하세요. Linux 시스템 서비스는 --service systemd-system에서 sudo 권한만 요청합니다"
        );
        let layout = layout(args)?;
        let mut sys = UnixSystem::new(&layout, args)?;
        let mut ui = terminal::Terminal::new()?;
        let mut setup = Setup::new(&mut sys, &mut ui, layout, aaa_common::util::software());
        if args.status {
            setup.status()
        } else if args.uninstall {
            setup.uninstall(args)
        } else {
            setup.install(args)
        }
    })();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) if error.downcast_ref::<Cancelled>().is_some() => ExitCode::from(130),
        Err(error) => {
            eprintln!("aaa setup: {error:#}");
            ExitCode::FAILURE
        }
    }
}

struct UnixSystem {
    manager: service::Manager,
    files: Vec<InstallFile>,
}
impl UnixSystem {
    fn new(layout: &Layout, args: &Args) -> Result<Self> {
        if layout.kind == ServiceKind::Launchd && !args.status && !args.uninstall {
            let legacy = layout
                .home
                .join("Library/LaunchAgents")
                .join(format!("{}.plist", crate::launchd::label_in(layout.instance.as_deref(), "agent")));
            ensure!(
                !legacy.exists(),
                "같은 인스턴스의 기존 aaa service 감독자가 있습니다: {}. 기존 서비스를 이관하지 않습니다. 별도 --instance를 쓰세요",
                legacy.display()
            );
        }
        let user = current_user()?;
        let launcher = (layout.kind == ServiceKind::Launchd).then(|| {
            args.launcher
                .clone()
                .unwrap_or_else(|| layout.home.join("Applications/aaa-launcher.app/Contents/MacOS/aaa-launcher"))
        });
        let expected = service::Spec::new(layout, user, launcher)?;
        let record = service::Spec::path(layout);
        let (spec, mut files) = match record.symlink_metadata() {
            Ok(meta) => {
                ensure!(meta.is_file() && !meta.file_type().is_symlink(), "서비스 기록이 일반 파일이 아닙니다");
                let stored: service::Spec =
                    serde_json::from_slice(&std::fs::read(&record)?).context("Unix 서비스 기록 읽기")?;
                stored.validate()?;
                ensure!(
                    stored.identity_matches(&expected),
                    "Unix 서비스 기록의 계정·이름·설정·경로가 현재 설치와 다릅니다"
                );
                if let Some(launcher) = &args.launcher {
                    ensure!(stored.launcher.as_ref() == Some(launcher), "기존 launcher를 바꾸지 않습니다");
                }
                (stored, Vec::new())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let file = InstallFile { path: record, bytes: serde_json::to_vec_pretty(&expected)?, mode: 0o600 };
                (expected, vec![file])
            }
            Err(e) => return Err(e).context("Unix 서비스 기록 조회"),
        };
        let manager = service::Manager { spec };
        manager.available()?;
        // 기존 서비스도 파일 소유권부터 확인한다. status·제거에는 launcher package가 필요 없다.
        manager.query()?;
        if !args.status
            && !args.uninstall
            && let Some(launcher) = &manager.spec.launcher
        {
            files.extend(launcher::prepare(layout, launcher)?);
        }
        Ok(Self { manager, files })
    }
}

fn current_user() -> Result<String> {
    // getpwuid_r의 결과는 호출자 buffer에서 복사해 다른 thread의 libc 조회와 겹치지 않는다.
    let mut buffer = vec![0u8; 65536];
    let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
    let mut found = std::ptr::null_mut();
    let code =
        unsafe { libc::getpwuid_r(libc::getuid(), &mut entry, buffer.as_mut_ptr().cast(), buffer.len(), &mut found) };
    ensure!(code == 0 && !found.is_null(), "현재 사용자 조회 실패: {}", std::io::Error::from_raw_os_error(code));
    Ok(unsafe { std::ffi::CStr::from_ptr(entry.pw_name) }.to_str()?.into())
}

impl System for UnixSystem {
    fn authorize(&mut self) -> Result<()> {
        self.manager.authorize()
    }
    fn additional_files(&self) -> Vec<InstallFile> {
        self.files.clone()
    }
    fn account(&mut self) -> Result<Account> {
        Ok(Account {
            sam: self.manager.spec.user.clone(),
            sid: self.manager.spec.uid.to_string(),
            profile: self.manager.spec.home.clone(),
        })
    }
    fn computer(&mut self) -> String {
        aaa_common::util::hostname()
    }
    fn service(&mut self, name: &str) -> Result<Option<ServiceInfo>> {
        ensure!(name == self.manager.spec.name, "다른 서비스 조회를 거부합니다");
        self.manager.query()
    }
    fn aaa_services(&mut self) -> Result<Vec<ServiceInfo>> {
        Ok(Vec::new())
    }
    fn reach(&mut self, addr: &str) -> Result<()> {
        use std::net::ToSocketAddrs;
        let targets = addr.to_socket_addrs().context("host 이름 조회")?;
        let mut errors = Vec::new();
        for target in targets {
            match std::net::TcpStream::connect_timeout(&target, Duration::from_secs(5)) {
                Ok(_) => return Ok(()),
                Err(error) => errors.push(error.to_string()),
            }
        }
        bail!("host TCP 연결 실패: {}", if errors.is_empty() { "주소 없음".into() } else { errors.join("; ") })
    }
    fn enroll(&mut self, ui: &mut dyn Ui, state: &Path, addr: &str, name: &str) -> Result<()> {
        let ident = aaa_agent::identity::load_or_create(state)?;
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        rt.block_on(async {
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            let request = aaa_agent::link::enroll_with(state, addr, &ident.machine_id, name, move |code, _| {
                let _ = tx.send(code.to_string());
            });
            tokio::pin!(request);
            let started = Instant::now();
            let mut tick = tokio::time::interval(Duration::from_millis(120));
            loop {
                tokio::select! {
                    result = &mut request => { ui.spin_end(); return result.map(|_| ()); }
                    Some(code) = rx.recv() => { for (tone, line) in code_lines(name, &code) { ui.say(tone, &line); } }
                    _ = tick.tick() => {
                        if ui.cancelled() { return Err(Cancelled.into()); }
                        ui.spin("host에서 승인을 기다리는 중 (최대 15분)", started.elapsed());
                    }
                }
            }
        })
    }
    fn logon(&mut self, _: &Account, _: &Secret, _: bool) -> Result<Logon> {
        bail!("Unix에는 Windows 로그온 시험이 없습니다")
    }
    fn logon_right(&mut self, _: &Account) -> Result<Right> {
        bail!("Unix에는 Windows 로그온 권한이 없습니다")
    }
    fn account_services(&mut self, _: &Account) -> Result<Vec<String>> {
        bail!("Unix에는 Windows 계정 서비스 조회가 없습니다")
    }
    fn set_logon_right(&mut self, _: &Account, _: bool) -> Result<()> {
        bail!("Unix에는 Windows 로그온 권한이 없습니다")
    }
    fn create_service(&mut self, def: &ServiceDef, _: &Account, _: &Secret) -> Result<()> {
        ensure!(def.name == self.manager.spec.name, "다른 서비스 등록을 거부합니다");
        self.manager.create()
    }
    fn start_service(&mut self, name: &str) -> Result<u32> {
        ensure!(name == self.manager.spec.name, "다른 서비스 시작을 거부합니다");
        self.manager.start()
    }
    fn stop_service(&mut self, name: &str) -> Result<()> {
        ensure!(name == self.manager.spec.name, "다른 서비스 정지를 거부합니다");
        self.manager.stop()
    }
    fn delete_service(&mut self, name: &str) -> Result<()> {
        ensure!(name == self.manager.spec.name, "다른 서비스 제거를 거부합니다");
        self.manager.delete()
    }
    fn user_path(&mut self) -> Result<Option<String>> {
        bail!("Unix 설치는 셸 PATH 파일을 바꾸지 않습니다")
    }
    fn set_user_path(&mut self, _: Option<&str>) -> Result<()> {
        bail!("Unix 설치는 셸 PATH 파일을 바꾸지 않습니다")
    }
}

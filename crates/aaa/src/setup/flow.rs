//! 설치 흐름. OS 조작은 `System`, 화면은 `Ui` 뒤에 두어 macOS 시험에서도 같은 흐름을 돈다.
//!
//! 바꾸기 전에 질문과 점검(관리자·콘솔·host 연결·암호)을 모두 끝낸다. 바꾸는 단계는 `Change`로 기록하고,
//! 실패·취소하면 그 기록만 거꾸로 되돌린 뒤 되돌린 것과 남은 것을 보인다. host 등록은 되돌리지 않는다:
//! 다시 실행하면 승인 없이 이어서 쓰고, 지우는 길(host 웹의 폐기)을 알려 준다.
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use super::Args;

/// 원격 agent가 붙는 host 기본 포트(host.toml `agent_listen`)
pub(crate) const AGENT_PORT: u16 = 7430;
/// 서비스를 시작한 뒤 host 연결을 기다리는 시간
const CONNECT_WAIT: Duration = Duration::from_secs(90);
/// 암호를 다시 묻는 횟수
const PASSWORD_TRIES: usize = 3;

/// 사용자가 취소했다(Ctrl+C·EOF·「아니요」)
#[derive(Debug)]
pub(crate) struct Cancelled;
impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("취소했습니다")
    }
}
impl std::error::Error for Cancelled {}

/// Windows 암호(NUL 끝 UTF-16). 드롭할 때 지운다. Debug·로그에 내용을 쓰지 않는다
pub(crate) struct Secret(Vec<u16>);
impl Secret {
    pub(crate) fn new(mut units: Vec<u16>) -> Secret {
        units.push(0);
        Secret(units)
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.0.len() <= 1
    }
    /// LogonUserW·CreateServiceW에 넘길 NUL 끝 글
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) fn as_ptr(&self) -> *const u16 {
        self.0.as_ptr()
    }
    #[cfg(test)]
    pub(crate) fn text(&self) -> String {
        String::from_utf16_lossy(&self.0[..self.0.len() - 1])
    }
}
impl Drop for Secret {
    fn drop(&mut self) {
        for unit in &mut self.0 {
            unsafe { std::ptr::write_volatile(unit, 0) };
        }
        std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
    }
}
impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(..)")
    }
}

/// 화면 한 줄의 성격(터미널이 색·기호로 그린다)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tone {
    Plain,
    Title,
    Step,
    Ok,
    Warn,
    Fail,
    Hint,
    Code,
    /// 단계 안의 보통 글(들여 씀)
    Info,
}

pub(crate) trait Ui {
    fn say(&mut self, tone: Tone, line: &str);
    /// 한 줄 답. 빈 답이면 기본값. Ctrl+C·EOF는 `Cancelled`
    fn ask(&mut self, label: &str, default: Option<&str>) -> Result<String>;
    /// 화면에 보이지 않는 입력
    fn secret(&mut self, label: &str) -> Result<Secret>;
    /// 진행 줄을 같은 자리에 다시 그린다(`spin_end`로 지운다)
    fn spin(&mut self, label: &str, elapsed: Duration);
    fn spin_end(&mut self);
    fn cancelled(&self) -> bool;
    /// 기다림 한 번(시험은 시간을 쓰지 않는다)
    fn pause(&mut self, d: Duration);
}

/// 서비스가 돌 계정. 서비스는 이 계정으로 돌아야 그 사람의 codex·claude 로그인을 쓴다
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Account {
    /// `PC\user`(GetUserNameExW NameSamCompatible). Microsoft 계정도 짧은 로컬 이름이다
    pub sam: String,
    pub sid: String,
    pub profile: PathBuf,
}
impl Account {
    /// 화면용 `.\user`
    pub(crate) fn short(&self) -> String {
        match self.sam.rsplit_once('\\') {
            Some((_, user)) => format!(".\\{user}"),
            None => self.sam.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ServiceInfo {
    pub name: String,
    pub running: bool,
    pub pid: u32,
    pub automatic: bool,
    pub account: String,
    pub command: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ServiceDef {
    pub name: String,
    pub display: String,
    pub description: String,
    pub command: String,
}

/// 암호 확인 결과
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Logon {
    Ok,
    WrongPassword,
    /// 그 로그온 방식이 정책으로 막혀 있다(ERROR_LOGON_TYPE_NOT_GRANTED)
    NotGranted,
    Other(String),
}

/// 「서비스로 로그온」 권한(SeServiceLogonRight)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Right {
    Granted,
    Missing,
    /// SeDenyServiceLogonRight가 계정에 있다
    Denied,
}

pub(crate) trait System {
    fn account(&mut self) -> Result<Account>;
    /// 이 PC의 이름(DNS hostname)
    fn computer(&mut self) -> String;
    fn service(&mut self, name: &str) -> Result<Option<ServiceInfo>>;
    /// 이름이나 실행 파일이 aaa인 서비스(안내용)
    fn aaa_services(&mut self) -> Result<Vec<ServiceInfo>>;
    /// TCP로 붙어 본다(바꾸는 것 없음)
    fn reach(&mut self, addr: &str) -> Result<()>;
    /// host 등록. 확인 코드는 `code_lines`로 보이고 승인을 기다린다. 거부는 `aaa_agent::link::EnrollRejected`
    fn enroll(&mut self, ui: &mut dyn Ui, state: &Path, addr: &str, name: &str) -> Result<()>;
    /// 암호 확인. `service`면 서비스 로그온(LOGON32_LOGON_SERVICE), 아니면 네트워크 로그온으로 본다
    fn logon(&mut self, account: &Account, secret: &Secret, service: bool) -> Result<Logon>;
    fn logon_right(&mut self, account: &Account) -> Result<Right>;
    /// 이 계정으로 도는 서비스 이름들(권한을 뺄 때 다른 서비스가 쓰는지 본다)
    fn account_services(&mut self, account: &Account) -> Result<Vec<String>>;
    fn set_logon_right(&mut self, account: &Account, on: bool) -> Result<()>;
    fn create_service(&mut self, def: &ServiceDef, account: &Account, secret: &Secret) -> Result<()>;
    /// 시작하고 RUNNING이 된 PID를 돌려준다
    fn start_service(&mut self, name: &str) -> Result<u32>;
    fn stop_service(&mut self, name: &str) -> Result<()>;
    fn delete_service(&mut self, name: &str) -> Result<()>;
    /// 사용자 PATH(HKCU\Environment `Path`) 원문. 없으면 None
    fn user_path(&mut self) -> Result<Option<String>>;
    /// None이면 값을 지운다
    fn set_user_path(&mut self, value: Option<&str>) -> Result<()>;
}

/// 설치 자리. 인스턴스마다 설정·상태·서비스 이름이 따로다
#[derive(Debug, Clone)]
pub(crate) struct Layout {
    pub instance: Option<String>,
    /// 설치하는 실행 파일(이 설치기 자신)
    pub source: PathBuf,
    /// 서비스가 쓸 aaa.exe
    pub exe: PathBuf,
    pub config: PathBuf,
    pub state: PathBuf,
    /// `--config-dir`·`--state-dir`로 정했으면 서비스 명령에도 넘긴다
    pub explicit_dirs: bool,
    pub home: PathBuf,
}
impl Layout {
    /// `aaa-agent`, 인스턴스는 `aaa-<이름>-agent`(이름이 이미 `aaa-`로 시작하면 겹쳐 붙이지 않는다)
    pub(crate) fn service(&self) -> String {
        match &self.instance {
            Some(name) if name.starts_with("aaa-") => format!("{name}-agent"),
            Some(name) => format!("aaa-{name}-agent"),
            None => "aaa-agent".into(),
        }
    }
    fn agent_state(&self) -> PathBuf {
        self.state.join("agent")
    }
    pub(crate) fn receipt(&self) -> PathBuf {
        self.state.join("setup.json")
    }
    /// 서비스가 쓰는 연결 상태(`ServiceStatus`)
    pub(crate) fn status(&self) -> PathBuf {
        self.state.join("setup-service.json")
    }
    pub(crate) fn log(&self) -> PathBuf {
        self.state.join("agent-service.log")
    }
    fn agent_toml(&self) -> PathBuf {
        self.config.join("agent.toml")
    }
    fn bin_dir(&self) -> Option<&Path> {
        self.exe.parent()
    }
    /// 기본 인스턴스만 사용자 PATH에 실행 파일 폴더를 둔다(인스턴스는 `aaa --instance`로 쓴다)
    fn wants_path(&self) -> bool {
        self.instance.is_none()
    }
    pub(crate) fn command(&self) -> String {
        let mut parts = vec![quote(&self.exe.to_string_lossy())];
        if let Some(name) = &self.instance {
            parts.push("--instance".into());
            parts.push(quote(name));
        }
        if self.explicit_dirs {
            parts.push("--config-dir".into());
            parts.push(quote(&self.config.to_string_lossy()));
            parts.push("--state-dir".into());
            parts.push(quote(&self.state.to_string_lossy()));
        }
        parts.push("setup-service".into());
        parts.join(" ")
    }
    fn definition(&self, name: &str) -> ServiceDef {
        let display = match &self.instance {
            Some(instance) => format!("aaa agent ({instance})"),
            None => "aaa agent".into(),
        };
        ServiceDef {
            name: self.service(),
            display,
            description: format!("이 PC({name})를 aaa host에 연결해 터미널·작업을 맡는 agent"),
            command: self.command(),
        }
    }
}

/// Windows 명령줄 한 칸(CommandLineToArgvW 규칙)
fn quote(value: &str) -> String {
    if !value.is_empty() && !value.contains([' ', '\t', '"']) {
        return value.into();
    }
    let mut out = String::from('"');
    let mut slashes = 0;
    for c in value.chars() {
        match c {
            '\\' => slashes += 1,
            '"' => {
                out.extend(std::iter::repeat_n('\\', slashes * 2 + 1));
                out.push('"');
                slashes = 0;
            }
            _ => {
                out.extend(std::iter::repeat_n('\\', slashes));
                out.push(c);
                slashes = 0;
            }
        }
    }
    out.extend(std::iter::repeat_n('\\', slashes * 2));
    out.push('"');
    out
}

/// 서비스가 연결 상태를 쓰는 파일(`setup-service.json`)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ServiceStatus {
    pub pid: u32,
    pub connected: bool,
    pub changed_ms: u64,
}

/// 이 설치기가 바꾼 것(`setup.json`). 제거는 여기 적힌 것만 되돌린다
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Receipt {
    pub schema: u32,
    pub service: String,
    pub exe: PathBuf,
    pub host: String,
    pub name: String,
    pub version: String,
    /// 「서비스로 로그온」 권한을 이 설치기가 넣었다
    pub logon_right_added: bool,
    /// 사용자 PATH에 이 폴더를 이 설치기가 넣었다
    pub path_added: Option<PathBuf>,
    pub installed_ms: u64,
}
impl Receipt {
    pub(crate) fn read(path: &Path) -> Option<Receipt> {
        serde_json::from_slice(&std::fs::read(path).ok()?).ok()
    }
}

/// 이번 실행이 바꾼 것(되돌릴 순서의 반대로 쌓는다)
#[derive(Debug)]
enum Change {
    /// 새로 만든 장치 키·machine ID(등록이 끝나지 않았을 때만 지운다). `shells`는 그때 새로 생긴 위 폴더(비면 지운다)
    Identity { created: Vec<PathBuf>, shells: Vec<PathBuf>, enrolled: bool },
    Binary { path: PathBuf, backup: Option<PathBuf> },
    File { path: PathBuf, previous: Option<Vec<u8>> },
    Path { previous: Option<String> },
    /// 이번에 만든 폴더(깊은 것부터). 비어 있을 때만 지운다
    Dirs { created: Vec<PathBuf> },
    LogonRight,
    Service { name: String },
    Stopped { name: String },
}

/// 고른 값
#[derive(Debug, Clone, PartialEq, Eq)]
struct Choices {
    host: String,
    name: String,
    work_root: PathBuf,
}

pub(crate) struct Setup<'a> {
    pub sys: &'a mut dyn System,
    pub ui: &'a mut dyn Ui,
    pub layout: Layout,
    pub version: String,
    changes: Vec<Change>,
}

impl<'a> Setup<'a> {
    pub(crate) fn new(sys: &'a mut dyn System, ui: &'a mut dyn Ui, layout: Layout, version: String) -> Setup<'a> {
        Setup { sys, ui, layout, version, changes: Vec::new() }
    }

    fn say(&mut self, tone: Tone, line: impl AsRef<str>) {
        self.ui.say(tone, line.as_ref());
    }

    fn check_cancel(&self) -> Result<()> {
        if self.ui.cancelled() {
            return Err(Cancelled.into());
        }
        Ok(())
    }

    /// `aaa setup`
    pub(crate) fn install(&mut self, args: &Args) -> Result<()> {
        self.say(Tone::Title, format!("aaa 설치 · Windows agent  {}", self.version));
        self.say(Tone::Plain, "이 PC를 aaa host에 연결하는 agent를 설치하고 Windows 서비스로 띄웁니다.");
        self.say(Tone::Plain, "[ ] 안은 기본값입니다(Enter). Ctrl+C로 취소하면 바꾼 것을 되돌립니다.");
        self.say(Tone::Plain, "");
        let account = self.sys.account()?;
        let service = self.layout.service();
        let existing = self.sys.service(&service)?;
        let receipt = Receipt::read(&self.layout.receipt());
        if let Some(existing) = existing {
            return match receipt {
                Some(receipt) if receipt.service == service => self.update(args, &account, existing, receipt),
                _ => {
                    self.say(Tone::Fail, format!("서비스 {service}가 이미 있지만 이 설치기가 만든 것이 아닙니다."));
                    self.say(Tone::Hint, format!("실행 파일: {}", existing.command));
                    self.say(Tone::Hint, "예전 `aaa install`로 만든 서비스일 수 있습니다. 건드리지 않았습니다.");
                    self.say(Tone::Hint, "다른 인스턴스로 설치하려면 `--instance <이름>`을 붙이세요.");
                    bail!("서비스 이름 충돌: {service}")
                }
            };
        }
        let (choices, mut secret, right) = match self.prepare(args, &account) {
            Ok(prepared) => prepared,
            Err(error) => {
                self.ui.spin_end();
                if error.downcast_ref::<Cancelled>().is_some() {
                    self.say(Tone::Warn, "취소했습니다. 아무것도 바꾸지 않았습니다.");
                } else {
                    self.say(Tone::Fail, format!("설치하지 못했습니다: {error:#}"));
                    self.say(Tone::Plain, "아무것도 바꾸지 않았습니다.");
                }
                return Err(error);
            }
        };
        self.say(Tone::Plain, "");
        let result = self.apply(&account, choices, &mut secret, right);
        drop(secret);
        match result {
            Ok(()) => Ok(()),
            Err(error) => {
                self.ui.spin_end();
                self.fail_and_rollback(&error);
                Err(error)
            }
        }
    }

    /// 바꾸기 전: 질문·점검·계획·확인
    fn prepare(&mut self, args: &Args, account: &Account) -> Result<(Choices, Secret, Right)> {
        let choices = self.ask_choices(args)?;
        let secret = self.ask_password(account)?;
        let right = self.sys.logon_right(account)?;
        if right == Right::Denied {
            self.say(Tone::Hint, format!("{}에 「서비스로 로그온 거부」 정책이 걸려 있습니다.", account.short()));
            self.say(Tone::Hint, "secpol.msc › 로컬 정책 › 사용자 권한 할당 › 「서비스로 로그온 거부」에서");
            self.say(Tone::Hint, "이 계정을 빼거나 관리자에게 문의하세요.");
            bail!("이 계정은 서비스로 로그온할 수 없습니다(정책)");
        }
        self.show_plan(account, &choices, right)?;
        let answer = self.ui.ask("진행할까요? (Y/n)", Some("Y"))?;
        if !yes(&answer) {
            return Err(Cancelled.into());
        }
        Ok((choices, secret, right))
    }

    fn ask_choices(&mut self, args: &Args) -> Result<Choices> {
        let config = read_agent_toml(&self.layout.agent_toml());
        let mut host = args.host.clone().or(config.host.clone()).or_else(|| sibling_host(&self.layout));
        loop {
            self.check_cancel()?;
            let answer = if args.yes && host.is_some() {
                host.clone().unwrap_or_default()
            } else {
                self.ui.ask("host 주소 (이름:포트)", host.as_deref())?
            };
            let Some(address) = normalize_host(&answer) else {
                self.say(Tone::Warn, "host 주소를 넣어 주세요. 예: studio.example.ts.net(포트 7430은 생략 가능)");
                host = None;
                continue;
            };
            match self.sys.reach(&address) {
                Ok(()) => {
                    self.say(Tone::Ok, format!("host {address}에 연결됩니다"));
                    host = Some(address);
                    break;
                }
                Err(e) => {
                    self.say(Tone::Warn, format!("{address}에 연결하지 못했습니다: {e:#}"));
                    self.say(Tone::Hint, "주소·포트(기본 7430)를 확인하세요. host.toml의 agent_listen이");
                    self.say(Tone::Hint, "이 PC에서 닿는 주소여야 합니다(tailnet 이름 등).");
                    if args.yes {
                        bail!("host에 연결하지 못했습니다: {address}");
                    }
                    host = Some(address);
                }
            }
        }
        let host = host.unwrap_or_default();
        let default_name = args
            .name
            .clone()
            .or(config.name.clone())
            .or_else(|| sibling_name(&self.layout))
            .unwrap_or_else(|| aaa_common::util::machine_name_from(&self.sys.computer().to_ascii_lowercase()));
        let name = if args.yes || args.name.is_some() {
            default_name
        } else {
            self.ask_name("이 PC 이름 (host 목록에 보일 이름)", &default_name)?
        };
        if !aaa_common::util::machine_name_ok(&name) {
            bail!("이 PC 이름은 영문·숫자·`.`·`-`·`_` 1~64자입니다: {name}");
        }
        let default_root = args
            .work_root
            .clone()
            .or_else(|| config.work_root.clone().filter(|r| !r.as_os_str().is_empty()))
            .unwrap_or_else(|| default_work_root(&self.layout));
        let work_root = if args.yes || args.work_root.is_some() {
            default_root
        } else {
            let answer = self.ui.ask("작업 폴더 (thread·worktree가 생기는 곳)", Some(&default_root.to_string_lossy()))?;
            PathBuf::from(answer)
        };
        if !work_root.is_absolute() {
            bail!("작업 폴더는 전체 경로로 적습니다: {}", work_root.display());
        }
        Ok(Choices { host, name, work_root })
    }

    fn ask_name(&mut self, label: &str, default: &str) -> Result<String> {
        loop {
            let name = self.ui.ask(label, Some(default))?;
            if aaa_common::util::machine_name_ok(&name) {
                return Ok(name);
            }
            self.say(Tone::Warn, "영문·숫자·`.`·`-`·`_`만, 1~64자로 적어 주세요.");
        }
    }

    fn ask_password(&mut self, account: &Account) -> Result<Secret> {
        self.say(Tone::Plain, "");
        self.say(Tone::Plain, format!("서비스는 이 계정({})으로 돌아 codex·claude 로그인을 그대로 씁니다.", account.short()));
        self.say(Tone::Plain, "Windows가 서비스를 띄울 때 쓸 로그인 암호를 넣어 주세요.");
        self.say(Tone::Hint, "PIN이 아니라 로그인 암호입니다. Microsoft 계정이면 그 계정 암호입니다.");
        self.say(Tone::Hint, "암호는 Windows 서비스 설정에만 들어가고 파일·로그에 남지 않습니다.");
        for attempt in 1..=PASSWORD_TRIES {
            let secret = self.ui.secret("Windows 암호")?;
            if secret.is_empty() {
                self.say(Tone::Warn, "빈 암호로는 서비스를 띄울 수 없습니다(Windows 정책). 암호를 넣어 주세요.");
                continue;
            }
            match self.sys.logon(account, &secret, false)? {
                Logon::Ok => {
                    self.say(Tone::Ok, "암호가 맞습니다");
                    return Ok(secret);
                }
                Logon::NotGranted => {
                    // 네트워크 로그온이 정책으로 막힌 PC: 서비스 로그온 권한을 넣은 뒤 다시 확인한다
                    self.say(Tone::Hint, "이 PC는 암호를 미리 확인할 수 없어 서비스 등록 때 확인합니다.");
                    return Ok(secret);
                }
                Logon::WrongPassword => {
                    let left = PASSWORD_TRIES - attempt;
                    if left > 0 {
                        self.say(Tone::Warn, format!("암호가 맞지 않습니다. 다시 넣어 주세요({left}번 남음)."));
                    }
                }
                Logon::Other(why) => bail!("암호를 확인하지 못했습니다: {why}"),
            }
        }
        bail!("암호가 {PASSWORD_TRIES}번 맞지 않았습니다")
    }

    fn show_plan(&mut self, account: &Account, choices: &Choices, right: Right) -> Result<()> {
        self.say(Tone::Plain, "");
        self.say(Tone::Title, "이렇게 설치합니다");
        let exe = &self.layout.exe;
        let binary = if exe == &self.layout.source {
            "지금 실행한 파일 그대로".to_string()
        } else if exe.exists() {
            "기존 파일은 .aaa-old에 보관하고 바꿈".to_string()
        } else {
            "새로 둠".to_string()
        };
        let mut rows = vec![
            ("host", {
                let pinned = aaa_agent::identity::load_pin(&self.layout.agent_state())
                    .is_some_and(|pin| pin.address == choices.host);
                let how = if pinned { "이미 등록됨, 승인 필요 없음" } else { "host에서 승인" };
                format!("{} 에 「{}」로 등록({how})", choices.host, choices.name)
            }),
            ("실행 파일", format!("{} ({binary})", exe.display())),
            ("설정", self.layout.agent_toml().display().to_string()),
            ("작업 폴더", choices.work_root.display().to_string()),
            ("서비스", format!("{} · 자동 시작 · {} 계정 · 멈추면 다시 시작", self.layout.service(), account.short())),
        ];
        if right == Right::Missing {
            rows.push(("권한", format!("{}에 「서비스로 로그온」 권한 추가", account.short())));
        }
        if self.layout.wants_path()
            && let Some(dir) = self.layout.bin_dir()
        {
            let present = path_contains(&split_path(self.sys.user_path()?.as_deref()), dir, &self.layout.home);
            rows.push(("PATH", if present { "이미 있음".into() } else { format!("사용자 PATH에 {} 추가", dir.display()) }));
        }
        for (key, value) in rows {
            self.say(Tone::Plain, format!("  {} {value}", pad(key, 10)));
        }
        let others: Vec<ServiceInfo> =
            self.sys.aaa_services()?.into_iter().filter(|s| s.name != self.layout.service()).collect();
        if !others.is_empty() {
            self.say(Tone::Plain, "");
            for other in others {
                let state = if other.running { "실행 중" } else { "정지" };
                self.say(Tone::Hint, format!("참고: 이 PC에 다른 aaa 서비스 {}({state})가 있습니다.", other.name));
                self.say(Tone::Hint, "      이 설치는 그 서비스를 건드리지 않습니다.");
            }
        }
        self.say(Tone::Plain, "");
        Ok(())
    }

    fn apply(&mut self, account: &Account, mut choices: Choices, secret: &mut Secret, right: Right) -> Result<()> {
        const TOTAL: usize = 5;
        self.say(Tone::Step, format!("[1/{TOTAL}] host에 이 PC 등록"));
        self.enroll(&mut choices)?;
        self.check_cancel()?;

        self.say(Tone::Step, format!("[2/{TOTAL}] 실행 파일 두기"));
        self.place_binary()?;
        self.check_cancel()?;

        self.say(Tone::Step, format!("[3/{TOTAL}] 설정 쓰기"));
        self.write_config(&choices)?;
        let path_added = self.add_path()?;
        self.check_cancel()?;

        self.say(Tone::Step, format!("[4/{TOTAL}] Windows 서비스 등록"));
        let mut logon_right_added = false;
        if right == Right::Missing {
            self.sys.set_logon_right(account, true).context("「서비스로 로그온」 권한을 넣지 못했습니다")?;
            self.changes.push(Change::LogonRight);
            logon_right_added = true;
            self.say(Tone::Ok, format!("「서비스로 로그온」 권한 추가 ({})", account.short()));
        }
        self.confirm_service_logon(account, secret)?;
        let def = self.layout.definition(&choices.name);
        self.sys.create_service(&def, account, secret).context("서비스를 등록하지 못했습니다")?;
        self.changes.push(Change::Service { name: def.name.clone() });
        let receipt = Receipt {
            schema: 1,
            service: def.name.clone(),
            exe: self.layout.exe.clone(),
            host: choices.host.clone(),
            name: choices.name.clone(),
            version: self.version.clone(),
            logon_right_added,
            path_added,
            installed_ms: aaa_common::util::now_ms(),
        };
        self.write_receipt(&receipt)?;
        self.say(Tone::Ok, format!("서비스 {} 등록 (자동 시작)", def.name));
        self.check_cancel()?;

        self.say(Tone::Step, format!("[5/{TOTAL}] 서비스 시작과 host 연결 확인"));
        let _ = std::fs::remove_file(self.layout.status());
        let pid = self.sys.start_service(&def.name).context("서비스를 시작하지 못했습니다")?;
        self.say(Tone::Ok, format!("서비스 시작 (PID {pid})"));
        if !self.wait_connected(pid)? {
            self.say(Tone::Warn, "아직 host에 연결되지 않았습니다.");
            self.show_log_tail();
            let keep = self.ui.ask("서비스를 그대로 두고 끝낼까요? 아니면 모두 되돌립니다 (Y/n)", Some("Y"))?;
            if !yes(&keep) {
                bail!("host 연결을 확인하지 못했습니다");
            }
            self.changes.clear();
            self.say(Tone::Warn, "서비스를 그대로 두었습니다. 연결되면 host 목록에 이 PC가 보입니다.");
            self.finish_lines(&choices, false);
            return Ok(());
        }
        self.changes.clear();
        self.finish_lines(&choices, true);
        Ok(())
    }

    /// 등록이 끝난 pin이 같은 host면 다시 묻지 않는다. 등록 창 닫힘·이름 충돌은 그 자리에서 고치고 다시 한다
    fn enroll(&mut self, choices: &mut Choices) -> Result<()> {
        let state = self.layout.agent_state();
        if let Some(pin) = aaa_agent::identity::load_pin(&state)
            && pin.address == choices.host
        {
            self.say(Tone::Ok, format!("이미 이 host에 등록돼 있습니다 (host fingerprint {}…)", &pin.fingerprint[..16]));
            return Ok(());
        }
        self.say(Tone::Hint, "host 웹 › 설정 › 머신에서 「등록 창 열기」를 먼저 눌러 두면 바로 이어집니다.");
        let before = list_dir(&state);
        let existed = state.exists();
        let shells: Vec<PathBuf> = state.ancestors().skip(1).take_while(|p| !p.exists()).map(Path::to_path_buf).collect();
        let mut created = Vec::new();
        let result = loop {
            self.check_cancel()?;
            let attempt = self.sys.enroll(&mut *self.ui, &state, &choices.host, &choices.name);
            if created.is_empty() {
                created = if existed {
                    list_dir(&state).into_iter().filter(|p| !before.contains(p)).collect()
                } else if state.exists() {
                    vec![state.clone()]
                } else {
                    vec![]
                };
                if !created.is_empty() {
                    let shells = shells.clone();
                    self.changes.push(Change::Identity { created: created.clone(), shells, enrolled: false });
                }
            }
            let Err(error) = attempt else { break Ok(()) };
            let Some(rejected) = error.downcast_ref::<aaa_agent::link::EnrollRejected>() else { break Err(error) };
            use aaa_proto::ErrorCode as E;
            match rejected.0.code {
                E::EnrollClosed => {
                    self.say(Tone::Warn, "host의 등록 창이 닫혀 있습니다.");
                    self.say(Tone::Hint, "host 웹 › 설정 › 머신 › 「등록 창 열기」를 누르세요.");
                    self.say(Tone::Hint, "(host 터미널이면: aaa machine enroll --open 10m)");
                    self.ui.ask("열었으면 Enter", Some(""))?;
                }
                E::NameTaken => {
                    self.say(Tone::Warn, format!("이름 「{}」는 host에서 다른 등록이 쓰고 있습니다.", choices.name));
                    self.say(Tone::Hint, format!("이 PC의 예전 설치라면 host 웹 › 설정 › 머신에서 「{}」를 폐기하고", choices.name));
                    self.say(Tone::Hint, "Enter를 누르면 같은 이름으로 다시 등록합니다. 다른 이름을 넣어도 됩니다.");
                    let current = choices.name.clone();
                    choices.name = self.ask_name("이 PC 이름", &current)?;
                }
                E::EnrollCodeMismatch => {
                    self.say(Tone::Warn, "host에서 승인하지 않았거나 코드가 맞지 않았습니다.");
                    let again = self.ui.ask("다시 요청할까요? (Y/n)", Some("Y"))?;
                    if !yes(&again) {
                        break Err(error);
                    }
                }
                E::UnsupportedVersion => {
                    self.say(Tone::Fail, format!("host와 판이 맞지 않습니다: {}", rejected.0.message));
                    self.say(Tone::Hint, "host와 같은 판의 설치기를 쓰세요(host 웹 › 설정 › 머신 › 「직접 설치 안내」).");
                    break Err(error);
                }
                _ => break Err(error),
            }
        };
        result?;
        for change in &mut self.changes {
            if let Change::Identity { enrolled, .. } = change {
                *enrolled = true;
            }
        }
        self.say(Tone::Ok, format!("host가 「{}」를 승인했습니다", choices.name));
        Ok(())
    }

    fn place_binary(&mut self) -> Result<()> {
        let (source, exe) = (self.layout.source.clone(), self.layout.exe.clone());
        if same_file(&source, &exe) {
            self.say(Tone::Ok, format!("{} (지금 실행한 파일)", exe.display()));
            return Ok(());
        }
        let bytes = std::fs::read(&source).with_context(|| format!("{} 읽기", source.display()))?;
        if std::fs::read(&exe).ok().is_some_and(|old| old == bytes) {
            self.say(Tone::Ok, format!("{} (이미 같은 파일)", exe.display()));
            return Ok(());
        }
        let dir = exe.parent().context("실행 파일 폴더")?.to_path_buf();
        self.make_dirs(&dir)?;
        let dir = dir.as_path();
        let backup = if exe.exists() {
            let old = dir.join(".aaa-old");
            std::fs::create_dir_all(&old)?;
            let to = old.join(format!("aaa-{}-before-setup.exe", aaa_common::util::now_ms()));
            std::fs::rename(&exe, &to).with_context(|| format!("기존 {}를 보관하지 못했습니다", exe.display()))?;
            Some(to)
        } else {
            None
        };
        self.changes.push(Change::Binary { path: exe.clone(), backup: backup.clone() });
        let partial = dir.join(format!(".aaa-setup-{}.exe", std::process::id()));
        std::fs::write(&partial, &bytes).with_context(|| format!("{} 쓰기", partial.display()))?;
        std::fs::rename(&partial, &exe).with_context(|| format!("{} 두기", exe.display()))?;
        let size = bytes.len() as f64 / 1024.0 / 1024.0;
        self.say(Tone::Ok, format!("{} ({size:.0} MB)", exe.display()));
        if let Some(backup) = backup {
            self.say(Tone::Hint, format!("기존 파일 보관: {}", backup.display()));
        }
        Ok(())
    }

    fn write_config(&mut self, choices: &Choices) -> Result<()> {
        let path = self.layout.agent_toml();
        let previous = std::fs::read(&path).ok();
        let text = agent_toml(previous.as_deref(), choices)?;
        if previous.as_deref() == Some(text.as_bytes()) {
            self.say(Tone::Ok, format!("{} (바꿀 것 없음)", path.display()));
        } else {
            self.make_dirs(&self.layout.config.clone())?;
            self.changes.push(Change::File { path: path.clone(), previous });
            aaa_common::util::write_private(&path, text.as_bytes()).with_context(|| format!("{} 쓰기", path.display()))?;
            self.say(Tone::Ok, path.display().to_string());
        }
        self.make_dirs(&choices.work_root).with_context(|| format!("작업 폴더 {} 만들기", choices.work_root.display()))?;
        Ok(())
    }

    /// 없는 폴더를 만들고 새로 생긴 것만 기록한다
    fn make_dirs(&mut self, dir: &Path) -> Result<()> {
        let mut created = Vec::new();
        let mut at = Some(dir);
        while let Some(path) = at
            && !path.exists()
        {
            created.push(path.to_path_buf());
            at = path.parent();
        }
        std::fs::create_dir_all(dir).with_context(|| format!("{} 만들기", dir.display()))?;
        if !created.is_empty() {
            self.changes.push(Change::Dirs { created });
        }
        Ok(())
    }

    fn add_path(&mut self) -> Result<Option<PathBuf>> {
        if !self.layout.wants_path() {
            return Ok(None);
        }
        let Some(dir) = self.layout.bin_dir().map(Path::to_path_buf) else { return Ok(None) };
        let previous = self.sys.user_path()?;
        if path_contains(&split_path(previous.as_deref()), &dir, &self.layout.home) {
            return Ok(None);
        }
        let next = match previous.as_deref() {
            None | Some("") => dir.to_string_lossy().into_owned(),
            Some(raw) if raw.ends_with(';') => format!("{raw}{}", dir.display()),
            Some(raw) => format!("{raw};{}", dir.display()),
        };
        self.sys.set_user_path(Some(&next))?;
        self.changes.push(Change::Path { previous });
        self.say(Tone::Ok, format!("사용자 PATH에 {} 추가 (새로 연 터미널부터 `aaa`로 씁니다)", dir.display()));
        Ok(Some(dir))
    }

    /// 권한을 넣은 뒤 Windows가 서비스를 띄울 때와 같은 로그온을 해 본다. 틀린 암호면 그 자리에서 다시 묻는다
    fn confirm_service_logon(&mut self, account: &Account, secret: &mut Secret) -> Result<()> {
        for attempt in 1..=PASSWORD_TRIES {
            match self.sys.logon(account, secret, true)? {
                Logon::Ok => {
                    self.say(Tone::Ok, "서비스 로그온 확인");
                    return Ok(());
                }
                Logon::WrongPassword if attempt < PASSWORD_TRIES => {
                    self.say(Tone::Warn, "암호가 맞지 않습니다. 다시 넣어 주세요.");
                    *secret = self.ui.secret("Windows 암호")?;
                }
                Logon::WrongPassword => bail!("암호가 맞지 않습니다"),
                Logon::NotGranted => {
                    self.say(
                        Tone::Fail,
                        "그룹 정책이 이 계정의 서비스 로그온을 막고 있습니다(「서비스로 로그온 거부」 또는 조직 정책).",
                    );
                    bail!("서비스 로그온이 정책으로 막혀 있습니다");
                }
                Logon::Other(why) => bail!("서비스 로그온을 확인하지 못했습니다: {why}"),
            }
        }
        bail!("암호가 맞지 않습니다")
    }

    fn write_receipt(&mut self, receipt: &Receipt) -> Result<()> {
        let path = self.layout.receipt();
        let previous = std::fs::read(&path).ok();
        std::fs::create_dir_all(&self.layout.state)?;
        aaa_common::util::write_private(&path, &serde_json::to_vec_pretty(receipt)?)?;
        self.changes.push(Change::File { path, previous });
        Ok(())
    }

    /// 서비스가 쓴 연결 상태를 본다. 다른 PID의 옛 파일은 보지 않는다. 서비스가 멈추면 바로 돌아온다
    fn wait_connected(&mut self, pid: u32) -> Result<bool> {
        let started = Instant::now();
        let name = self.layout.service();
        let mut checks = 0u32;
        loop {
            self.check_cancel()?;
            if let Some(status) = std::fs::read(self.layout.status())
                .ok()
                .and_then(|b| serde_json::from_slice::<ServiceStatus>(&b).ok())
                && status.pid == pid
                && status.connected
            {
                self.ui.spin_end();
                self.say(Tone::Ok, format!("host에 연결됐습니다 ({:.1}초)", started.elapsed().as_secs_f32()));
                return Ok(true);
            }
            checks += 1;
            if checks.is_multiple_of(8) {
                match self.sys.service(&name)? {
                    Some(info) if info.running => {}
                    _ => {
                        self.ui.spin_end();
                        self.say(Tone::Fail, "서비스가 멈췄습니다.");
                        return Ok(false);
                    }
                }
            }
            if started.elapsed() >= CONNECT_WAIT {
                self.ui.spin_end();
                return Ok(false);
            }
            self.ui.spin("host 연결을 기다리는 중", started.elapsed());
            self.ui.pause(Duration::from_millis(250));
        }
    }

    fn show_log_tail(&mut self) {
        let log = self.layout.log();
        let Ok(text) = std::fs::read_to_string(&log) else {
            self.say(Tone::Hint, format!("서비스 로그가 아직 없습니다: {}", log.display()));
            return;
        };
        self.say(Tone::Hint, format!("서비스 로그 마지막 줄 ({}):", log.display()));
        let lines: Vec<&str> = text.lines().collect();
        for line in &lines[lines.len().saturating_sub(8)..] {
            self.say(Tone::Code, format!("  {line}"));
        }
    }

    fn finish_lines(&mut self, choices: &Choices, connected: bool) {
        let cli = cli_name(&self.layout);
        self.say(Tone::Plain, "");
        if connected {
            self.say(Tone::Title, format!("완료 — 「{}」가 host {}에 연결됐습니다.", choices.name, choices.host));
            self.say(Tone::Plain, "  host 웹의 머신 목록에서 이 PC를 볼 수 있습니다.");
        } else {
            self.say(Tone::Title, format!("설치는 끝났고 「{}」의 연결을 기다립니다.", choices.name));
        }
        self.say(Tone::Plain, format!("  상태 보기    {cli} setup --status"));
        self.say(Tone::Plain, format!("  서비스 로그  {}", self.layout.log().display()));
        self.say(Tone::Plain, format!("  되돌리기     {cli} setup --uninstall"));
    }

    /// 실패·취소: 기록한 것을 거꾸로 되돌리고, 되돌린 것과 남은 것을 보인다
    fn fail_and_rollback(&mut self, error: &anyhow::Error) {
        self.say(Tone::Plain, "");
        if error.downcast_ref::<Cancelled>().is_some() {
            self.say(Tone::Warn, "취소했습니다. 이번에 바꾼 것을 되돌립니다.");
        } else {
            self.say(Tone::Fail, format!("설치하지 못했습니다: {error:#}"));
            self.say(Tone::Plain, "이번에 바꾼 것을 되돌립니다.");
        }
        let mut undone = Vec::new();
        let mut left = Vec::new();
        while let Some(change) = self.changes.pop() {
            match self.undo(change) {
                Ok(Some(line)) => undone.push(line),
                Ok(None) => {}
                Err(line) => left.push(line),
            }
        }
        if undone.is_empty() {
            self.say(Tone::Plain, "  되돌릴 것이 없습니다(바꾼 것이 없음).");
        }
        for line in undone {
            self.say(Tone::Ok, format!("되돌림: {line}"));
        }
        for line in left {
            self.say(Tone::Warn, format!("남음: {line}"));
        }
    }

    /// 성공하면 화면 글, 남기는 것은 Err
    fn undo(&mut self, change: Change) -> Result<Option<String>, String> {
        match change {
            Change::Identity { created, shells, enrolled: false } => {
                for path in &created {
                    let _ = if path.is_dir() { std::fs::remove_dir_all(path) } else { std::fs::remove_file(path) };
                }
                for shell in &shells {
                    let _ = std::fs::remove_dir(shell);
                }
                Ok(Some("새로 만든 장치 키(등록 전)".into()))
            }
            Change::Identity { enrolled: true, .. } => Err(format!(
                "host 등록과 장치 키({}). 다시 실행하면 승인 없이 이어서 씁니다. 지우려면 host 웹 › 설정 › 머신에서 폐기",
                self.layout.agent_state().display()
            )),
            Change::Binary { path, backup } => {
                let restored = std::fs::remove_file(&path).and_then(|()| match &backup {
                    Some(backup) => std::fs::rename(backup, &path),
                    None => Ok(()),
                });
                match (restored, backup) {
                    (Ok(()), Some(_)) => Ok(Some(format!("실행 파일 {} (예전 파일로)", path.display()))),
                    (Ok(()), None) => Ok(Some(format!("실행 파일 {}", path.display()))),
                    (Err(e), _) => Err(format!("실행 파일 {}: {e}", path.display())),
                }
            }
            Change::File { path, previous } => {
                let result = match &previous {
                    Some(bytes) => aaa_common::util::write_private(&path, bytes),
                    None => std::fs::remove_file(&path),
                };
                result.map(|()| Some(format!("파일 {}", path.display()))).map_err(|e| format!("파일 {}: {e}", path.display()))
            }
            Change::Dirs { created } => {
                let removed = created.iter().filter(|dir| std::fs::remove_dir(dir).is_ok()).count();
                Ok((removed > 0).then(|| format!("새로 만든 폴더 {}", created.last().unwrap_or(&created[0]).display())))
            }
            Change::Path { previous } => self
                .sys
                .set_user_path(previous.as_deref())
                .map(|()| Some("사용자 PATH".into()))
                .map_err(|e| format!("사용자 PATH: {e:#}")),
            Change::LogonRight => {
                let account = self.sys.account().map_err(|e| format!("「서비스로 로그온」 권한: {e:#}"))?;
                self.sys
                    .set_logon_right(&account, false)
                    .map(|()| Some(format!("「서비스로 로그온」 권한 ({})", account.short())))
                    .map_err(|e| format!("「서비스로 로그온」 권한: {e:#}"))
            }
            Change::Service { name } => {
                let _ = self.sys.stop_service(&name);
                self.sys
                    .delete_service(&name)
                    .map(|()| Some(format!("서비스 {name}")))
                    .map_err(|e| format!("서비스 {name}: {e:#}"))
            }
            Change::Stopped { name } => self
                .sys
                .start_service(&name)
                .map(|_| Some(format!("서비스 {name} 다시 시작")))
                .map_err(|e| format!("서비스 {name}(멈춘 채): {e:#}")),
        }
    }

    /// 이미 설치돼 있으면 실행 파일만 이 판으로 바꾸고 다시 시작한다(암호·등록은 다시 묻지 않는다)
    fn update(&mut self, args: &Args, _account: &Account, existing: ServiceInfo, receipt: Receipt) -> Result<()> {
        self.say(
            Tone::Plain,
            format!(
                "이미 설치돼 있습니다: 서비스 {} · host {} · 이름 「{}」 · 판 {}",
                existing.name, receipt.host, receipt.name, receipt.version
            ),
        );
        if args.host.as_ref().is_some_and(|h| normalize_host(h).as_deref() != Some(receipt.host.as_str()))
            || args.name.as_ref().is_some_and(|n| n != &receipt.name)
        {
            self.say(Tone::Hint, "host·이름을 바꾸려면 먼저 `setup --uninstall`로 서비스를 지운 뒤 다시 설치하세요.");
        }
        let same = same_file(&self.layout.source, &self.layout.exe)
            || std::fs::read(&self.layout.source).ok() == std::fs::read(&self.layout.exe).ok();
        let question = if same {
            if existing.running {
                self.say(Tone::Ok, "같은 판이 이미 돌고 있습니다. 바꿀 것이 없습니다.");
                return Ok(());
            }
            "서비스가 멈춰 있습니다. 다시 시작할까요? (Y/n)"
        } else {
            "실행 파일을 이 판으로 바꾸고 서비스를 다시 시작할까요? (Y/n)"
        };
        let answer = if args.yes { "Y".to_string() } else { self.ui.ask(question, Some("Y"))? };
        if !yes(&answer) {
            self.say(Tone::Warn, "취소했습니다. 아무것도 바꾸지 않았습니다.");
            return Err(Cancelled.into());
        }
        let result = (|| -> Result<()> {
            if existing.running {
                self.sys.stop_service(&existing.name).context("서비스를 멈추지 못했습니다")?;
                self.changes.push(Change::Stopped { name: existing.name.clone() });
                self.say(Tone::Ok, format!("서비스 {} 멈춤", existing.name));
            }
            if !same {
                self.place_binary()?;
            }
            let _ = std::fs::remove_file(self.layout.status());
            let pid = self.sys.start_service(&existing.name).context("서비스를 시작하지 못했습니다")?;
            self.changes.retain(|c| !matches!(c, Change::Stopped { .. }));
            self.say(Tone::Ok, format!("서비스 시작 (PID {pid})"));
            if !self.wait_connected(pid)? {
                self.say(Tone::Warn, "아직 host에 연결되지 않았습니다.");
                self.show_log_tail();
            }
            let mut next = receipt.clone();
            next.version = self.version.clone();
            self.write_receipt(&next)?;
            self.changes.clear();
            self.say(Tone::Plain, "");
            self.say(Tone::Title, format!("완료 — 「{}」를 {}로 바꿨습니다.", receipt.name, self.version));
            Ok(())
        })();
        if let Err(error) = &result {
            self.ui.spin_end();
            self.fail_and_rollback(error);
        }
        result
    }

    /// `aaa setup --uninstall`: 서비스와 이 설치기가 넣은 권한·PATH만 되돌린다. 등록·설정은 `--purge`가 아니면 남긴다
    pub(crate) fn uninstall(&mut self, args: &Args) -> Result<()> {
        let service = self.layout.service();
        let receipt = Receipt::read(&self.layout.receipt());
        let existing = self.sys.service(&service)?;
        self.say(Tone::Title, format!("aaa 제거 · {service}"));
        let Some(receipt) = receipt else {
            if existing.is_some() {
                self.say(Tone::Fail, format!("서비스 {service}는 이 설치기 기록이 없어 지우지 않습니다."));
                bail!("설치 기록 없음");
            }
            self.say(Tone::Plain, "이 인스턴스에는 `aaa setup` 설치 기록이 없습니다. 바꾼 것이 없습니다.");
            return Ok(());
        };
        let mut plan = vec![format!("서비스 {service} 멈추고 지우기")];
        if receipt.logon_right_added {
            plan.push("이 설치기가 넣은 「서비스로 로그온」 권한 빼기".into());
        }
        if let Some(dir) = &receipt.path_added {
            plan.push(format!("이 설치기가 넣은 사용자 PATH 항목 {} 빼기", dir.display()));
        }
        if args.purge {
            plan.push(format!("설정·상태 폴더 지우기: {} · {}", self.layout.config.display(), self.layout.state.display()));
        }
        for line in &plan {
            self.say(Tone::Plain, format!("  {line}"));
        }
        if !args.yes {
            let answer = self.ui.ask("진행할까요? (Y/n)", Some("Y"))?;
            if !yes(&answer) {
                return Err(Cancelled.into());
            }
        }
        let mut problems = Vec::new();
        if existing.is_some() {
            if let Err(e) = self.sys.stop_service(&service) {
                problems.push(format!("서비스 멈춤: {e:#}"));
            }
            match self.sys.delete_service(&service) {
                Ok(()) => self.say(Tone::Ok, format!("서비스 {service} 지움")),
                Err(e) => problems.push(format!("서비스 지움: {e:#}")),
            }
        }
        if receipt.logon_right_added {
            let account = self.sys.account()?;
            let others: Vec<String> =
                self.sys.account_services(&account)?.into_iter().filter(|name| name != &service).collect();
            if others.is_empty() {
                match self.sys.set_logon_right(&account, false) {
                    Ok(()) => self.say(Tone::Ok, format!("「서비스로 로그온」 권한 뺌 ({})", account.short())),
                    Err(e) => problems.push(format!("권한: {e:#}")),
                }
            } else {
                // 빼면 그 서비스가 다음 시작에 1069로 실패한다
                self.say(
                    Tone::Hint,
                    format!("「서비스로 로그온」 권한은 남깁니다: {}가 같은 계정으로 돕니다", others.join(", ")),
                );
            }
        }
        if let Some(dir) = &receipt.path_added {
            let entries = split_path(self.sys.user_path()?.as_deref());
            let next: Vec<String> =
                entries.iter().filter(|e| !same_dir(Path::new(e.as_str()), dir, &self.layout.home)).cloned().collect();
            if next.len() != entries.len() {
                let joined = next.join(";");
                match self.sys.set_user_path(if next.is_empty() { None } else { Some(&joined) }) {
                    Ok(()) => self.say(Tone::Ok, format!("사용자 PATH에서 {} 뺌", dir.display())),
                    Err(e) => problems.push(format!("PATH: {e:#}")),
                }
            }
        }
        if !problems.is_empty() {
            for problem in &problems {
                self.say(Tone::Fail, problem);
            }
            bail!("제거를 다 끝내지 못했습니다(설치 기록은 남겨 다시 할 수 있습니다)");
        }
        let _ = std::fs::remove_file(self.layout.receipt());
        let _ = std::fs::remove_file(self.layout.status());
        if args.purge {
            for dir in [self.layout.config.clone(), self.layout.state.clone()] {
                match std::fs::remove_dir_all(&dir) {
                    Ok(()) => self.say(Tone::Ok, format!("지움: {}", dir.display())),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => self.say(Tone::Warn, format!("남음: {} ({e})", dir.display())),
                }
            }
        } else {
            self.say(
                Tone::Hint,
                format!(
                    "남긴 것: 설정 {} · 장치 키와 등록 {} (다시 설치하면 승인 없이 이어 씁니다)",
                    self.layout.config.display(),
                    self.layout.agent_state().display()
                ),
            );
        }
        self.say(Tone::Hint, format!("남긴 것: 실행 파일 {} (다른 aaa 명령도 씁니다)", receipt.exe.display()));
        self.say(Tone::Hint, format!("host에서도 지우려면 host 웹 › 설정 › 머신에서 「{}」를 폐기하세요.", receipt.name));
        Ok(())
    }

    /// `aaa setup --status`
    pub(crate) fn status(&mut self) -> Result<()> {
        let service = self.layout.service();
        let receipt = Receipt::read(&self.layout.receipt());
        let info = self.sys.service(&service)?;
        self.say(Tone::Title, format!("aaa agent 서비스 {service}"));
        match &info {
            Some(info) => self.say(
                if info.running { Tone::Ok } else { Tone::Warn },
                format!(
                    "{} · {} · 계정 {}",
                    if info.running { "실행 중" } else { "정지" },
                    if info.automatic { "자동 시작" } else { "수동 시작" },
                    info.account
                ),
            ),
            None => self.say(Tone::Warn, "등록돼 있지 않습니다"),
        }
        if let Some(receipt) = &receipt {
            self.say(Tone::Plain, format!("  host {} · 이름 「{}」 · 설치 판 {}", receipt.host, receipt.name, receipt.version));
        }
        let status = std::fs::read(self.layout.status()).ok().and_then(|b| serde_json::from_slice::<ServiceStatus>(&b).ok());
        match (&info, status) {
            (Some(info), Some(status)) if info.running && status.pid == info.pid => self.say(
                if status.connected { Tone::Ok } else { Tone::Warn },
                if status.connected { "host에 연결됨" } else { "host에 연결되지 않음(다시 붙는 중)" },
            ),
            (Some(info), _) if info.running => self.say(Tone::Warn, "연결 상태를 아직 모릅니다"),
            _ => {}
        }
        self.say(Tone::Plain, format!("  서비스 로그  {}", self.layout.log().display()));
        Ok(())
    }
}

fn yes(answer: &str) -> bool {
    matches!(answer.trim().to_ascii_lowercase().as_str(), "" | "y" | "yes" | "ㅛ" | "예" | "네")
}

/// 사람이 친 host 주소: 앞뒤 공백·`https://`·끝 `/`를 걷고 포트가 없으면 7430을 붙인다
pub(crate) fn normalize_host(raw: &str) -> Option<String> {
    let mut value = raw.trim();
    for scheme in ["tcp://", "https://", "http://"] {
        if let Some(rest) = value.strip_prefix(scheme) {
            value = rest;
        }
    }
    let value = value.trim_end_matches('/');
    if value.is_empty() || value.contains(char::is_whitespace) || value.contains('/') {
        return None;
    }
    let has_port = if let Some(rest) = value.strip_prefix('[') {
        rest.split_once(']').is_some_and(|(_, tail)| tail.starts_with(':'))
    } else {
        value.matches(':').count() == 1
    };
    if has_port {
        let (_, port) = value.rsplit_once(':')?;
        port.parse::<u16>().ok().filter(|p| *p != 0)?;
        Some(value.to_string())
    } else if value.contains(':') && !value.starts_with('[') {
        Some(format!("[{value}]:{AGENT_PORT}"))
    } else {
        Some(format!("{value}:{AGENT_PORT}"))
    }
}

#[derive(Debug, Default)]
struct AgentConfig {
    host: Option<String>,
    name: Option<String>,
    work_root: Option<PathBuf>,
}

fn read_agent_toml(path: &Path) -> AgentConfig {
    let Ok(text) = std::fs::read_to_string(path) else { return AgentConfig::default() };
    let Ok(doc) = text.parse::<toml_edit::DocumentMut>() else { return AgentConfig::default() };
    let get = |key: &str| doc.get("agent").and_then(|a| a.get(key)).and_then(|v| v.as_str()).map(str::to_owned);
    AgentConfig {
        host: get("host").filter(|h| !h.is_empty() && h != "local"),
        name: get("name").filter(|n| !n.is_empty()),
        work_root: get("work_root").filter(|r| !r.is_empty()).map(PathBuf::from),
    }
}

/// 이 PC의 다른 aaa 인스턴스 설정(`%LOCALAPPDATA%\aaa*\config\agent.toml`)에서 host 주소를 고른다
fn sibling_configs(layout: &Layout) -> Vec<AgentConfig> {
    let Some(parent) = layout.config.parent().and_then(Path::parent) else { return vec![] };
    let Ok(dir) = std::fs::read_dir(parent) else { return vec![] };
    let mut found: Vec<(PathBuf, AgentConfig)> = dir
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("aaa"))
        .map(|e| e.path().join("config").join("agent.toml"))
        .filter(|p| p != &layout.agent_toml())
        .map(|p| {
            let config = read_agent_toml(&p);
            (p, config)
        })
        .collect();
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found.into_iter().map(|(_, c)| c).collect()
}
fn sibling_host(layout: &Layout) -> Option<String> {
    sibling_configs(layout).into_iter().find_map(|c| c.host)
}
fn sibling_name(layout: &Layout) -> Option<String> {
    sibling_configs(layout).into_iter().find_map(|c| c.name)
}

fn default_work_root(layout: &Layout) -> PathBuf {
    layout.home.join(aaa_config::paths::named("aaa-work", layout.instance.as_deref()))
}

/// 기존 agent.toml의 다른 값·주석은 두고 `[agent]`의 host·name·work_root만 바꾼다
fn agent_toml(previous: Option<&[u8]>, choices: &Choices) -> Result<String> {
    let text = match previous {
        Some(bytes) => String::from_utf8(bytes.to_vec()).context("기존 agent.toml이 UTF-8이 아닙니다")?,
        None => "schema = 1\n".into(),
    };
    let mut doc: toml_edit::DocumentMut = text.parse().context("기존 agent.toml을 읽지 못했습니다")?;
    if doc.get("schema").is_none() {
        doc["schema"] = toml_edit::value(1);
    }
    let agent = doc["agent"].or_insert(toml_edit::table());
    let agent = agent.as_table_mut().context("agent.toml의 [agent]가 표가 아닙니다")?;
    agent["host"] = toml_edit::value(choices.host.as_str());
    agent["name"] = toml_edit::value(choices.name.as_str());
    agent["work_root"] = toml_edit::value(choices.work_root.to_string_lossy().as_ref());
    Ok(doc.to_string())
}

fn list_dir(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> =
        std::fs::read_dir(dir).map(|d| d.flatten().map(|e| e.path()).collect()).unwrap_or_default();
    out.sort();
    out
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// `%USERPROFILE%`를 펼치고 대소문자·끝 `\`를 무시해 같은 폴더인지 본다
fn same_dir(entry: &Path, dir: &Path, home: &Path) -> bool {
    let norm = |p: &str| {
        let p = p.replace("%USERPROFILE%", &home.to_string_lossy()).replace("%userprofile%", &home.to_string_lossy());
        p.trim_end_matches(['\\', '/']).replace('/', "\\").to_lowercase()
    };
    norm(&entry.to_string_lossy()) == norm(&dir.to_string_lossy())
}
fn split_path(raw: Option<&str>) -> Vec<String> {
    raw.unwrap_or_default().split(';').filter(|e| !e.trim().is_empty()).map(str::to_owned).collect()
}
fn path_contains(entries: &[String], dir: &Path, home: &Path) -> bool {
    entries.iter().any(|e| same_dir(Path::new(e), dir, home))
}

/// 화면에 보일 CLI 이름(기본 인스턴스는 PATH의 `aaa`)
fn cli_name(layout: &Layout) -> String {
    match &layout.instance {
        Some(name) => format!("{} --instance {name}", quote(&layout.exe.to_string_lossy())),
        None => "aaa".into(),
    }
}

/// 터미널 칸 너비(한글·전각은 2칸)
fn width(text: &str) -> usize {
    text.chars()
        .map(|c| match c as u32 {
            0x1100..=0x115F | 0x2E80..=0xA4CF | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6 => 2,
            _ => 1,
        })
        .sum()
}
fn pad(text: &str, columns: usize) -> String {
    format!("{text}{}", " ".repeat(columns.saturating_sub(width(text))))
}

/// 확인 코드 안내(System::enroll이 EnrollPending 때 보인다)
pub(crate) fn code_lines(name: &str, code: &str) -> Vec<(Tone, String)> {
    let spaced = if code.len() == 6 { format!("{} {}", &code[..3], &code[3..]) } else { code.to_string() };
    vec![
        (Tone::Code, format!("확인 코드  {spaced}")),
        (Tone::Info, "host 웹 › 설정 › 머신 › 「승인을 기다리는 머신」에서".to_string()),
        (Tone::Info, format!("「{name}」의 「승인…」을 누르고 위 코드를 넣으세요.")),
        (Tone::Hint, format!("(host 터미널이면: aaa machine approve {name} --code {code})")),
    ]
}

//! SSH로 머신 추가(웹 머신 추가 M2, plan §8.375): host가 시스템 OpenSSH로 새 머신에 접속해 host key를 받고, 사람이 지문을 믿으면
//! OS·CPU를 감지한다. 원격에 쓰지 않는다(설치는 M3).
//!
//! - **SSH는 host의 것만.** `ssh`(PATH)를 부르고 `~/.ssh/config`·ssh-agent·키를 그대로 쓴다(`[host] ssh_config`가 있으면 `-F`).
//!   비밀번호·개인 키는 받지 않는다(BatchMode). 원격에서는 고정 감지 스크립트만 실행한다.
//! - **host key.** 먼저 빈 임시 known_hosts에 `StrictHostKeyChecking=accept-new`·`PreferredAuthentications=none`으로 접속해 상대가
//!   내민 키만 받는다(인증하지 않으므로 원격에서 아무것도 돌지 않는다. ProxyJump도 같다). 그다음 aaa 전용 known_hosts와 사용자
//!   known_hosts로 `StrictHostKeyChecking=yes` 접속을 한다: 아는 키면 바로 감지, 모르는 키면 「믿음」 단계, 바뀐 키면 멈춘다.
//!   사람이 지문을 믿으면 받은 줄을 aaa 전용 known_hosts(`<state>/ssh/known_hosts`)에만 더한다. 사용자 known_hosts는 읽기만 한다.
//! - **감지.** `sh -c`로 `uname -s`·`uname -m`·HOME·SHELL·aaa 판을 읽고, 표지가 없으면(Windows 기본 셸 cmd) PowerShell
//!   `-EncodedCommand`로 다시 읽는다. 그래도 모르면 사람이 고른다. release 대상(macOS arm64·x86_64, Linux x86_64·arm64,
//!   Windows x86_64)이 아닌 조합은 「지원 안 함」.
//! - **작업.** host DB `machine_setups`에 JSON으로 남고 바뀔 때마다 웹 `enroll` 사건을 보낸다. host가 진행 중에 다시 시작하면
//!   그 작업은 「멈췄다」로 끝난다(다시 시작은 새 작업).
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use aaa_proto::{ErrorBody, ErrorCode, HostKeyView, MachineDetected, MachineSetupPhase, MachineSetupView, SshResolved};
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::Host;

const PROBE_TIMEOUT: Duration = Duration::from_secs(30);
const RUN_TIMEOUT: Duration = Duration::from_secs(45);
const CONNECT_TIMEOUT_S: u32 = 10;
/// 목록에 남기는 작업 수(오래된 것부터 지운다)
const KEEP: usize = 20;
const MARK: &str = "AAA-DETECT-1";
const END: &str = "AAA-DETECT-END";

/// release 대상(.github/workflows/release.yml)
pub(crate) const TARGETS: [(&str, &str); 5] =
    [("macos", "aarch64"), ("macos", "x86_64"), ("linux", "x86_64"), ("linux", "aarch64"), ("windows", "x86_64")];

fn bad(msg: impl Into<String>) -> ErrorBody {
    ErrorBody::new(ErrorCode::BadRequest, msg.into())
}

/// DB에 남기는 작업: 화면 칸과, 화면에 보이지 않는 받은 known_hosts 줄
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Job {
    pub view: MachineSetupView,
    #[serde(default)]
    pub key_lines: Vec<String>,
}

/// 사람이 넣은 대상을 ssh 인자로: 별칭·`host`·`user@host`·`host:port`·`user@[v6]:port`. 비밀번호(`user:pw@`)·옵션 꼴·공백은 거부한다
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Target {
    pub dest: String,
    pub port: Option<u16>,
}

pub(crate) fn parse_target(raw: &str) -> Result<Target, ErrorBody> {
    let t = raw.trim();
    if t.is_empty() || t.len() > 255 {
        return Err(bad("SSH 대상을 넣는다(별칭 또는 user@host:port)"));
    }
    if t.starts_with('-') {
        return Err(bad("SSH 대상은 -로 시작할 수 없다"));
    }
    if !t.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-@:[]%".contains(&b)) {
        return Err(bad("SSH 대상에 쓸 수 없는 글자가 있다(공백·따옴표·옵션 꼴은 받지 않는다)"));
    }
    let (user, rest) = match t.rsplit_once('@') {
        Some((u, r)) => (Some(u), r),
        None => (None, t),
    };
    if let Some(u) = user
        && (u.is_empty() || u.contains(':') || u.contains('@'))
    {
        return Err(bad("SSH 대상에 비밀번호를 넣지 않는다: host의 SSH 키·ssh-agent·설정만 쓴다"));
    }
    let (host, port) = if let Some(v6) = rest.strip_prefix('[') {
        let (h, after) = v6.split_once(']').ok_or_else(|| bad("[IPv6] 괄호가 닫히지 않았다"))?;
        let port = match after {
            "" => None,
            p => Some(p.strip_prefix(':').ok_or_else(|| bad("포트는 ]:<번호>로 쓴다"))?),
        };
        (h.to_string(), port)
    } else {
        match rest.split_once(':') {
            Some((h, p)) => (h.to_string(), Some(p)),
            None => (rest.to_string(), None),
        }
    };
    if host.is_empty() || host.starts_with('-') || host.contains('[') || host.contains(']') {
        return Err(bad("SSH 대상의 host가 비었거나 옳지 않다"));
    }
    let port = match port {
        None => None,
        Some(p) => Some(p.parse::<u16>().ok().filter(|p| *p > 0).ok_or_else(|| bad("포트는 1~65535"))?),
    };
    let dest = match user {
        Some(u) => format!("{u}@{host}"),
        None => host,
    };
    Ok(Target { dest, port })
}

/// `ssh -G` 출력에서 사용자·주소·포트·ProxyJump
pub(crate) fn parse_resolved(out: &str) -> Option<SshResolved> {
    let mut user = None;
    let mut host = None;
    let mut port = None;
    let mut proxy_jump = false;
    for line in out.lines() {
        let (k, v) = line.split_once(' ').unwrap_or((line, ""));
        match k.to_ascii_lowercase().as_str() {
            "user" => user = Some(v.to_string()),
            "hostname" => host = Some(v.to_string()),
            "port" => port = v.parse().ok(),
            "proxyjump" => proxy_jump = !v.is_empty() && v != "none",
            "proxycommand" => proxy_jump |= !v.is_empty() && v != "none",
            _ => {}
        }
    }
    Some(SshResolved { user: user?, host: host?, port: port?, proxy_jump })
}

/// known_hosts 줄(`<호스트> <종류> <base64>`)의 지문
pub(crate) fn key_of_line(line: &str) -> Option<HostKeyView> {
    let mut parts = line.split_whitespace();
    let first = parts.next()?;
    // `@cert-authority`·`@revoked` 표지 줄은 받지 않는다(accept-new가 쓰지 않는다)
    if first.starts_with('@') || first.starts_with('#') {
        return None;
    }
    let kind = parts.next()?;
    let blob = base64::engine::general_purpose::STANDARD.decode(parts.next()?).ok()?;
    let sum = Sha256::digest(&blob);
    Some(HostKeyView {
        kind: kind.to_string(),
        sha256: format!("SHA256:{}", base64::engine::general_purpose::STANDARD_NO_PAD.encode(sum)),
    })
}

/// 감지 결과 줄(표지 사이). POSIX: OS·CPU·HOME·SHELL·aaa, PowerShell: `Windows`·CPU·USERPROFILE·`-`·aaa
pub(crate) fn parse_detect(out: &str) -> Option<MachineDetected> {
    let body: Vec<&str> = out
        .lines()
        .map(|l| l.trim_end_matches('\r'))
        .skip_while(|l| *l != MARK)
        .skip(1)
        .take_while(|l| *l != END)
        .collect();
    if body.len() < 5 {
        return None;
    }
    let raw_os = body[0].trim();
    let raw_arch = body[1].trim();
    let os = match raw_os {
        "Darwin" => "macos",
        "Linux" => "linux",
        "Windows" => "windows",
        s if s.starts_with("MINGW") || s.starts_with("MSYS") || s.starts_with("CYGWIN") => "windows",
        _ => return None,
    };
    let arch = match raw_arch.to_ascii_lowercase().as_str() {
        "arm64" | "aarch64" => "aarch64",
        "x86_64" | "amd64" => "x86_64",
        other => other,
    }
    .to_string();
    let some = |s: &str| Some(s.trim().to_string()).filter(|s| !s.is_empty() && s != "-");
    Some(MachineDetected {
        os: os.into(),
        arch,
        raw: format!("{raw_os} {raw_arch}"),
        chosen: false,
        home: some(body[2]),
        shell: some(body[3]),
        aaa: some(body[4]),
    })
}

/// 원격 기본 셸이 무엇이든 `sh -c`로 돈다(csh·fish도). 작은따옴표 안에는 큰따옴표만 쓴다
pub(crate) fn posix_script() -> String {
    let body = [
        format!("echo {MARK}"),
        "uname -s".into(),
        "uname -m".into(),
        r#"printf "%s\n" "${HOME:--}""#.into(),
        r#"printf "%s\n" "${SHELL:--}""#.into(),
        r#"if command -v aaa >/dev/null 2>&1; then aaa --version 2>/dev/null | head -n 1; elif [ -x "$HOME/.local/bin/aaa" ]; then "$HOME/.local/bin/aaa" --version 2>/dev/null | head -n 1; else echo -; fi"#.into(),
        format!("echo {END}"),
    ]
    .join("; ");
    format!("sh -c '{body}'")
}

/// Windows(OpenSSH 기본 셸 cmd.exe): PowerShell에 UTF-16LE base64로 넘긴다(따옴표 해석을 피한다)
pub(crate) fn powershell_script() -> String {
    let ps = format!(
        "Write-Output '{MARK}'; Write-Output 'Windows'; Write-Output $env:PROCESSOR_ARCHITECTURE; Write-Output $env:USERPROFILE; \
         Write-Output '-'; $a = Join-Path $env:LOCALAPPDATA 'aaa\\bin\\aaa.exe'; \
         if (Test-Path $a) {{ (& $a --version | Select-Object -First 1) }} else {{ Write-Output '-' }}; Write-Output '{END}'"
    );
    let utf16: Vec<u8> = ps.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    format!(
        "powershell -NoProfile -NonInteractive -EncodedCommand {}",
        base64::engine::general_purpose::STANDARD.encode(utf16)
    )
}

/// stderr에서 사람이 읽을 까닭
pub(crate) fn why(stderr: &str) -> String {
    let s = stderr;
    if s.contains("Could not resolve hostname") {
        "host 이름을 찾지 못했다(이름·~/.ssh/config 별칭을 확인한다)".into()
    } else if s.contains("Connection refused") {
        "연결이 거부됐다(그 머신의 SSH 서버가 켜져 있는지 본다)".into()
    } else if s.contains("timed out") || s.contains("No route to host") || s.contains("Network is unreachable") {
        "시간 안에 닿지 않았다(주소·네트워크·방화벽)".into()
    } else if s.contains("Permission denied") || s.contains("Too many authentication failures") {
        "인증이 거부됐다: host의 SSH 키·ssh-agent로 그 머신에 로그인되는지 터미널에서 먼저 확인한다(웹은 비밀번호를 받지 않는다)".into()
    } else {
        let last = s.lines().rev().find(|l| !l.trim().is_empty() && !l.starts_with("Warning:")).unwrap_or("").trim();
        if last.is_empty() { "ssh가 실패했다".into() } else { format!("ssh: {last}") }
    }
}

/// host가 부르는 ssh 한 번. 설정 파일·known_hosts·공통 옵션을 붙인다
struct Ssh {
    config: Option<PathBuf>,
    known: PathBuf,
}

struct Ran {
    ok: bool,
    stdout: String,
    stderr: String,
}

impl Ssh {
    fn base(&self, target: &Target) -> tokio::process::Command {
        let mut c = tokio::process::Command::new("ssh");
        if let Some(cfg) = &self.config {
            c.arg("-F").arg(cfg);
        }
        if let Some(p) = target.port {
            c.arg("-p").arg(p.to_string());
        }
        c.args([
            "-o",
            "BatchMode=yes",
            "-o",
            &format!("ConnectTimeout={CONNECT_TIMEOUT_S}"),
            "-o",
            "ClearAllForwardings=yes",
            "-o",
            "ForwardAgent=no",
            "-o",
            "ForwardX11=no",
            "-o",
            "RequestTTY=no",
            "-o",
            "LogLevel=ERROR",
        ]);
        c.stdin(std::process::Stdio::null());
        c.kill_on_drop(true);
        c
    }

    async fn run(mut c: tokio::process::Command, timeout: Duration) -> Ran {
        match tokio::time::timeout(timeout, c.output()).await {
            Ok(Ok(o)) => Ran {
                ok: o.status.success(),
                stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
            },
            Ok(Err(e)) => {
                Ran { ok: false, stdout: String::new(), stderr: format!("ssh를 실행하지 못했다: {e}") }
            }
            Err(_) => Ran { ok: false, stdout: String::new(), stderr: "ssh: timed out".into() },
        }
    }

    async fn resolve(&self, target: &Target) -> Option<SshResolved> {
        let mut c = tokio::process::Command::new("ssh");
        if let Some(cfg) = &self.config {
            c.arg("-F").arg(cfg);
        }
        if let Some(p) = target.port {
            c.arg("-p").arg(p.to_string());
        }
        c.arg("-G").arg("--").arg(&target.dest).stdin(std::process::Stdio::null()).kill_on_drop(true);
        let r = Self::run(c, PROBE_TIMEOUT).await;
        r.ok.then(|| parse_resolved(&r.stdout)).flatten()
    }

    /// 인증 없이 상대가 내민 host key 줄만 받는다
    async fn probe(&self, target: &Target, scratch: &Path) -> Result<Vec<String>, String> {
        let file = scratch.join("probe_known_hosts");
        let empty = scratch.join("empty_known_hosts");
        std::fs::write(&file, b"").map_err(|e| e.to_string())?;
        std::fs::write(&empty, b"").map_err(|e| e.to_string())?;
        let mut c = self.base(target);
        c.arg("-o").arg(format!("UserKnownHostsFile={}", quote(&file)));
        c.arg("-o").arg(format!("GlobalKnownHostsFile={}", quote(&empty)));
        c.args([
            "-o",
            "StrictHostKeyChecking=accept-new",
            "-o",
            "PreferredAuthentications=none",
            "-o",
            "UpdateHostKeys=no",
        ]);
        c.arg("--").arg(&target.dest).arg("true");
        let r = Self::run(c, PROBE_TIMEOUT).await;
        let lines: Vec<String> = std::fs::read_to_string(&file)
            .unwrap_or_default()
            .lines()
            .filter(|l| key_of_line(l).is_some())
            .map(str::to_string)
            .collect();
        let _ = std::fs::remove_file(&file);
        let _ = std::fs::remove_file(&empty);
        if lines.is_empty() { Err(why(&r.stderr)) } else { Ok(lines) }
    }

    /// 믿는 키로만 접속해 명령 하나
    async fn exec(&self, target: &Target, command: &str) -> Ran {
        let mut c = self.base(target);
        c.arg("-o").arg(format!("UserKnownHostsFile={} ~/.ssh/known_hosts ~/.ssh/known_hosts2", quote(&self.known)));
        c.args(["-o", "StrictHostKeyChecking=yes", "-o", "UpdateHostKeys=no"]);
        c.arg("--").arg(&target.dest).arg(command);
        Self::run(c, RUN_TIMEOUT).await
    }
}

/// ssh 설정 값 안의 경로(공백이 든 macOS `Application Support`)
fn quote(p: &Path) -> String {
    format!("\"{}\"", p.display())
}

enum KeyCheck {
    Known(Ran),
    New,
    Changed,
    Failed(String),
}

fn classify(r: Ran) -> KeyCheck {
    if r.stderr.contains("IDENTIFICATION HAS CHANGED")
        || (r.stderr.contains("Host key for") && r.stderr.contains("has changed"))
    {
        KeyCheck::Changed
    } else if r.stderr.contains("Host key verification failed") || r.stderr.contains("host key is known") {
        KeyCheck::New
    } else if r.stdout.contains(MARK) || r.ok {
        KeyCheck::Known(r)
    } else {
        KeyCheck::Failed(why(&r.stderr))
    }
}

impl Host {
    fn ssh(&self) -> Ssh {
        let cfg = self.cfg.host.ssh_config.trim();
        Ssh { config: (!cfg.is_empty()).then(|| PathBuf::from(cfg)), known: self.state.join("ssh").join("known_hosts") }
    }

    fn setup_scratch(&self, job: &str) -> std::io::Result<PathBuf> {
        let dir = self.state.join("ssh").join("jobs").join(job);
        aaa_common::util::ensure_private_dir(&self.state.join("ssh"))?;
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    pub(crate) fn setup_load(&self, job: &str) -> Result<Job, ErrorBody> {
        let raw = self.db.machine_setup(job).map_err(|e| ErrorBody::new(ErrorCode::Internal, e.to_string()))?;
        let raw = raw.ok_or_else(|| ErrorBody::new(ErrorCode::NotFound, format!("SSH 추가 작업 {job}이 없다")))?;
        serde_json::from_str(&raw).map_err(|e| ErrorBody::new(ErrorCode::Internal, e.to_string()))
    }

    fn setup_save(&self, job: &mut Job) {
        job.view.updated_ms = aaa_common::util::now_ms();
        if let Ok(raw) = serde_json::to_string(job) {
            let _ = self.db.machine_setup_put(&job.view.job, &raw, job.view.updated_ms, KEEP);
        }
        self.enroll_bump();
    }

    /// 작업이 아직 그 단계일 때만 바꾼다(그사이 멈춤·다른 단계로 갔으면 버린다)
    fn setup_update(&self, id: &str, from: MachineSetupPhase, f: impl FnOnce(&mut Job)) -> Option<Job> {
        let _g = self.setup_gate.lock().unwrap();
        let mut job = self.setup_load(id).ok()?;
        if job.view.phase != from {
            return None;
        }
        f(&mut job);
        self.setup_save(&mut job);
        Some(job)
    }

    pub(crate) fn setup_list(&self) -> Result<Vec<MachineSetupView>, ErrorBody> {
        let rows = self.db.machine_setups().map_err(|e| ErrorBody::new(ErrorCode::Internal, e.to_string()))?;
        Ok(rows.iter().filter_map(|r| serde_json::from_str::<Job>(r).ok()).map(|j| j.view).collect())
    }

    /// host가 진행 중에 내려갔던 작업은 멈췄다고 적는다(host 시작 때 한 번)
    pub(crate) fn setup_recover(&self) {
        let Ok(rows) = self.db.machine_setups() else { return };
        for raw in rows {
            let Ok(mut job) = serde_json::from_str::<Job>(&raw) else { continue };
            if matches!(job.view.phase, MachineSetupPhase::Connecting | MachineSetupPhase::Detecting) {
                job.view.phase = MachineSetupPhase::Failed;
                job.view.error = Some("host가 다시 시작해 작업이 멈췄다: 새로 시작한다".into());
                self.setup_save(&mut job);
            }
        }
    }

    pub(crate) fn setup_start(self: &Arc<Self>, target: &str, actor: &str) -> Result<MachineSetupView, ErrorBody> {
        let t = parse_target(target)?;
        let now = aaa_common::util::now_ms();
        let mut job = Job {
            view: MachineSetupView {
                job: aaa_common::util::new_id(),
                target: target.trim().to_string(),
                phase: MachineSetupPhase::Connecting,
                ssh: None,
                host_keys: vec![],
                host_key_state: None,
                detected: None,
                error: None,
                created_ms: now,
                updated_ms: now,
            },
            key_lines: vec![],
        };
        self.setup_save(&mut job);
        self.db.audit(actor, "machine_setup_start", &job.view.target);
        let host = self.clone();
        let id = job.view.job.clone();
        tokio::spawn(async move { host.setup_connect(&id, t).await });
        Ok(job.view)
    }

    async fn setup_connect(self: Arc<Self>, id: &str, t: Target) {
        let ssh = self.ssh();
        let resolved = ssh.resolve(&t).await;
        let scratch = match self.setup_scratch(id) {
            Ok(d) => d,
            Err(e) => {
                self.setup_fail(id, MachineSetupPhase::Connecting, format!("작업 폴더를 만들지 못했다: {e}"));
                return;
            }
        };
        let probed = ssh.probe(&t, &scratch).await;
        let _ = std::fs::remove_dir(&scratch);
        let lines = match probed {
            Ok(l) => l,
            Err(e) => {
                self.setup_update(id, MachineSetupPhase::Connecting, |j| {
                    j.view.ssh = resolved;
                    j.view.phase = MachineSetupPhase::Failed;
                    j.view.error = Some(e);
                });
                return;
            }
        };
        let keys: Vec<HostKeyView> = lines.iter().filter_map(|l| key_of_line(l)).collect();
        let check = classify(ssh.exec(&t, &posix_script()).await);
        let next = match &check {
            KeyCheck::Known(_) => MachineSetupPhase::Detecting,
            KeyCheck::New => MachineSetupPhase::Trust,
            KeyCheck::Changed | KeyCheck::Failed(_) => MachineSetupPhase::Failed,
        };
        let updated = self.setup_update(id, MachineSetupPhase::Connecting, |j| {
            j.view.ssh = resolved;
            j.view.host_keys = keys;
            j.key_lines = lines;
            j.view.phase = next;
            j.view.host_key_state = Some(
                match &check {
                    KeyCheck::Changed => "changed",
                    KeyCheck::New => "new",
                    _ => "known",
                }
                .into(),
            );
            match &check {
                KeyCheck::Changed => {
                    j.view.error = Some(
                        "host key가 믿던 것과 다르다(중간자이거나 그 머신을 다시 설치했다): 확인하기 전에는 멈춘다. 정말 바뀌었으면 known_hosts의 옛 줄을 지운 뒤 다시 시작한다"
                            .into(),
                    )
                }
                KeyCheck::Failed(e) => j.view.error = Some(e.clone()),
                _ => {}
            }
        });
        if updated.is_some()
            && let KeyCheck::Known(r) = check
        {
            self.setup_detected(id, &ssh, &t, r).await;
        }
    }

    fn setup_fail(&self, id: &str, from: MachineSetupPhase, error: String) {
        self.setup_update(id, from, |j| {
            j.view.phase = MachineSetupPhase::Failed;
            j.view.error = Some(error);
        });
    }

    /// 감지 출력(POSIX)을 읽고, 표지가 없으면 PowerShell로 한 번 더
    async fn setup_detected(&self, id: &str, ssh: &Ssh, t: &Target, first: Ran) {
        let mut found = parse_detect(&first.stdout);
        if found.is_none() || found.as_ref().is_some_and(|d| d.os == "windows") {
            let r = ssh.exec(t, &powershell_script()).await;
            found = parse_detect(&r.stdout).or(found);
        }
        self.setup_update(id, MachineSetupPhase::Detecting, |j| match found {
            Some(d) => {
                j.view.phase = if TARGETS.contains(&(d.os.as_str(), d.arch.as_str())) {
                    MachineSetupPhase::Ready
                } else {
                    j.view.error = Some(format!(
                        "{}에 넣을 aaa release가 없다(지원: macOS·Linux arm64·x86_64, Windows x86_64)",
                        d.raw
                    ));
                    MachineSetupPhase::Unsupported
                };
                j.view.detected = Some(d);
            }
            None => {
                j.view.phase = MachineSetupPhase::Choose;
                j.view.error = Some("OS·CPU를 알아내지 못했다: 직접 고른다".into());
            }
        });
    }

    pub(crate) async fn setup_trust(
        self: &Arc<Self>,
        id: &str,
        sha256: &str,
        actor: &str,
    ) -> Result<MachineSetupView, ErrorBody> {
        {
            let _g = self.setup_gate.lock().unwrap();
            let mut job = self.setup_load(id)?;
            if job.view.phase != MachineSetupPhase::Trust {
                return Err(ErrorBody::new(
                    ErrorCode::Conflict,
                    "이 작업은 지문을 믿는 단계가 아니다(화면을 다시 읽는다)",
                ));
            }
            let line = job
                .key_lines
                .iter()
                .find(|l| key_of_line(l).is_some_and(|k| k.sha256 == sha256))
                .cloned()
                .ok_or_else(|| ErrorBody::new(ErrorCode::Conflict, "그 지문은 이 머신이 내민 키가 아니다"))?;
            let ssh = self.ssh();
            append_known(&ssh.known, &line)
                .map_err(|e| ErrorBody::new(ErrorCode::Internal, format!("known_hosts: {e}")))?;
            job.view.phase = MachineSetupPhase::Detecting;
            job.view.host_key_state = Some("known".into());
            job.view.error = None;
            self.setup_save(&mut job);
            self.db.audit(actor, "machine_setup_trust", &format!("{} {sha256}", job.view.target));
        }
        let job = self.setup_load(id)?;
        let t = parse_target(&job.view.target)?;
        let host = self.clone();
        let id2 = id.to_string();
        tokio::spawn(async move {
            let ssh = host.ssh();
            match classify(ssh.exec(&t, &posix_script()).await) {
                KeyCheck::Known(r) => host.setup_detected(&id2, &ssh, &t, r).await,
                KeyCheck::Changed => {
                    host.setup_fail(&id2, MachineSetupPhase::Detecting, "host key가 그사이 바뀌었다: 멈춘다".into())
                }
                KeyCheck::New => host.setup_fail(
                    &id2,
                    MachineSetupPhase::Detecting,
                    "믿은 키로 접속하지 못했다(다른 키를 내민다)".into(),
                ),
                KeyCheck::Failed(e) => host.setup_fail(&id2, MachineSetupPhase::Detecting, e),
            }
        });
        Ok(job.view)
    }

    pub(crate) fn setup_choose(
        &self,
        id: &str,
        os: &str,
        arch: &str,
        actor: &str,
    ) -> Result<MachineSetupView, ErrorBody> {
        if !TARGETS.contains(&(os, arch)) {
            return Err(bad("release 대상만 고른다: macos·linux의 aarch64·x86_64, windows의 x86_64"));
        }
        let _g = self.setup_gate.lock().unwrap();
        let mut job = self.setup_load(id)?;
        if job.view.phase != MachineSetupPhase::Choose {
            return Err(ErrorBody::new(ErrorCode::Conflict, "이 작업은 OS를 고르는 단계가 아니다"));
        }
        job.view.detected = Some(MachineDetected {
            os: os.into(),
            arch: arch.into(),
            raw: String::new(),
            chosen: true,
            home: None,
            shell: None,
            aaa: None,
        });
        job.view.phase = MachineSetupPhase::Ready;
        job.view.error = None;
        self.setup_save(&mut job);
        self.db.audit(actor, "machine_setup_choose", &format!("{} {os} {arch}", job.view.target));
        Ok(job.view)
    }

    pub(crate) fn setup_cancel(&self, id: &str, actor: &str) -> Result<(), ErrorBody> {
        let _g = self.setup_gate.lock().unwrap();
        let job = self.setup_load(id)?;
        self.db.machine_setup_delete(id).map_err(|e| ErrorBody::new(ErrorCode::Internal, e.to_string()))?;
        self.db.audit(actor, "machine_setup_cancel", &job.view.target);
        self.enroll_bump();
        Ok(())
    }
}

/// aaa 전용 known_hosts에 한 줄(폴더 0700, 파일 0600). 같은 줄이 있으면 그대로
fn append_known(path: &Path, line: &str) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        aaa_common::util::ensure_private_dir(dir)?;
    }
    let have = std::fs::read_to_string(path).unwrap_or_default();
    if have.lines().any(|l| l == line) {
        return Ok(());
    }
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    if !have.is_empty() && !have.ends_with('\n') {
        f.write_all(b"\n")?;
    }
    f.write_all(format!("{line}\n").as_bytes())
}

#[cfg(test)]
#[path = "machine_setup/tests.rs"]
mod tests;

//! 새 머신에 넣을 aaa 실행 파일(웹 머신 추가 M3, 계획 t-0471 §1.5): 출처를 고르고 SSH stdin으로 원격 임시 폴더에 쓴 뒤, 원격의
//! `shasum -a 256`/`sha256sum`과 `--version`으로 같은 파일인지 확인한다. 설치 자리로 옮기는 일은 install(t-0485)이 한다.
//!
//! - **출처.** 대상 OS·CPU가 host와 같으면 host가 지금 실행 중인 파일(SHA가 host와 같다). 다르면 같은 판 release 파일이어야 한다.
//!   host가 `+커밋` 개발 판이면 다른 종류 머신에 넣을 파일이 없으므로 거절하고 까닭을 보인다.
//! - **원격.** `mktemp -d`로 만든 그 사용자 전용 폴더(umask 077)에만 쓴다. 확인이 어긋나면 그 폴더만 지운다. 폴더 이름은 표지 사이에서
//!   읽고 `aaa-setup.` 꼴과 안전한 글자만 받는다(지우기 명령에 그대로 들어가므로).
//! - **launcher.** macOS `--service launchd`의 install은 실행 파일 옆 `aaa-launcher-macos-<CPU>.json`(같은 release·같은 SHA에 묶인
//!   package, install I3)을 읽는다. host는 그 파일을 같은 폴더에 같은 방식으로 놓고 SHA를 확인한다. package 내용은 원격 install이 본다.
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::{Ran, Ssh, Target};

/// 원격에 쓰고 확인하는 데 쓰는 시한(파일 전송 포함)
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(300);
const UP: &str = "AAA-UP-1";
const UP_END: &str = "AAA-UP-END";

/// 넣을 파일의 출처
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Source {
    /// host가 실행 중인 파일(같은 OS·CPU)
    Running { path: PathBuf },
    /// 같은 판 release 파일(`{os}-{arch}` asset)
    Release { version: String, key: String },
    /// 넣을 파일이 없다(사람이 읽을 까닭)
    Refuse(String),
}

/// 대상 OS·CPU에 넣을 파일을 고른다. `software`는 host 판(`aaa 0.1.2` 또는 `aaa 0.1.2+abc`)
pub(crate) fn pick(host: (&str, &str), software: &str, target: (&str, &str), exe: &Path) -> Source {
    if host == target {
        return Source::Running { path: exe.to_path_buf() };
    }
    let version = software.strip_prefix("aaa ").unwrap_or(software);
    if version.contains('+') || version.is_empty() {
        return Source::Refuse(format!(
            "host가 개발 판({version})이라 {}·{}용 같은 판 파일이 없다: release 판 host에서 하거나 같은 종류 머신만 넣는다",
            target.0, target.1
        ));
    }
    Source::Release { version: version.into(), key: format!("{}-{}", target.0, target.1) }
}

/// 원격에 놓인 파일
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Placed {
    /// `mktemp -d`가 만든 폴더(정리 대상)
    pub dir: String,
    pub sha256: String,
    /// `aaa --version` 첫 줄(실행되지 않으면 빈 값)
    pub version: String,
}

/// 원격(POSIX)에서: 전용 임시 폴더를 만들고 stdin을 `aaa`로 쓴 뒤 폴더·SHA·판을 표지 사이에 낸다. 작은따옴표 안에는 큰따옴표만 쓴다
pub(crate) fn upload_script() -> String {
    let body = [
        "umask 077".to_string(),
        r#"d=$(mktemp -d "${TMPDIR:-/tmp}/aaa-setup.XXXXXXXX") || exit 70"#.into(),
        r#"cat > "$d/aaa" || exit 71"#.into(),
        r#"chmod 700 "$d/aaa""#.into(),
        format!("echo {UP}"),
        r#"printf "%s\n" "$d""#.into(),
        r#"if command -v shasum >/dev/null 2>&1; then shasum -a 256 "$d/aaa"; else sha256sum "$d/aaa"; fi | cut -d" " -f1"#
            .into(),
        r#""$d/aaa" --version 2>/dev/null | head -n 1"#.into(),
        format!("echo {UP_END}"),
    ]
    .join("; ");
    format!("sh -c '{body}'")
}

/// 실행 파일 옆에 놓는 파일(stdin을 `<dir>/<name>`으로 쓰고 같은 표지로 폴더·SHA를 낸다). 폴더는 [`safe_dir`], 이름은 [`launcher_name`]만
pub(crate) fn beside_script(dir: &str, name: &str) -> Option<String> {
    if !safe_dir(dir)
        || !name.strip_prefix("aaa-launcher-macos-").is_some_and(|a| a == "aarch64.json" || a == "x86_64.json")
    {
        return None;
    }
    let body = [
        "umask 077".to_string(),
        format!(r#"cat > "{dir}/{name}" || exit 71"#),
        format!("echo {UP}"),
        format!(r#"printf "%s\n" "{dir}""#),
        format!(
            r#"if command -v shasum >/dev/null 2>&1; then shasum -a 256 "{dir}/{name}"; else sha256sum "{dir}/{name}"; fi | cut -d" " -f1"#
        ),
        format!("echo {UP_END}"),
    ]
    .join("; ");
    Some(format!("sh -c '{body}'"))
}

/// macOS 대상 CPU의 launcher package 이름(install이 실행 파일 옆에서 찾는 이름). 다른 OS는 없다
pub(crate) fn launcher_name(os: &str, arch: &str) -> Option<String> {
    (os == "macos" && matches!(arch, "aarch64" | "x86_64")).then(|| format!("aaa-launcher-macos-{arch}.json"))
}

/// 지우기 명령에 넣어도 되는 원격 폴더인가: 절대 경로, 마지막 이름이 `aaa-setup.<영숫자>`, 안전한 글자만
pub(crate) fn safe_dir(dir: &str) -> bool {
    let Some(name) = dir.rsplit('/').next() else { return false };
    dir.starts_with('/')
        && !dir.split('/').any(|p| p == "..")
        && dir.bytes().all(|b| b.is_ascii_alphanumeric() || b"/._-".contains(&b))
        && name
            .strip_prefix("aaa-setup.")
            .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric()))
}

/// 전송 출력(표지 사이: 폴더·SHA·판)
pub(crate) fn parse_upload(out: &str) -> Option<Placed> {
    let body: Vec<&str> = out
        .lines()
        .map(|l| l.trim_end_matches('\r'))
        .skip_while(|l| *l != UP)
        .skip(1)
        .take_while(|l| *l != UP_END)
        .collect();
    let dir = body.first()?.trim().to_string();
    let sha256 = body.get(1)?.trim().to_ascii_lowercase();
    if !safe_dir(&dir) || sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(Placed { dir, sha256, version: body.get(2).map(|v| v.trim().to_string()).unwrap_or_default() })
}

/// 원격 임시 폴더를 지우는 명령(폴더는 [`safe_dir`]를 통과한 것만)
pub(crate) fn cleanup_script(dir: &str) -> Option<String> {
    safe_dir(dir).then(|| format!("sh -c 'rm -rf -- \"{dir}\"'"))
}

/// 왜 놓은 파일을 쓰지 않는가(맞으면 None)
pub(crate) fn mismatch(placed: &Placed, sha256: &str, software: &str) -> Option<String> {
    if !placed.sha256.eq_ignore_ascii_case(sha256) {
        return Some(format!(
            "원격에 쓴 파일의 SHA-256이 다르다(보냄 {sha256}, 원격 {}): 전송이 깨졌다",
            placed.sha256
        ));
    }
    if placed.version != software {
        let got = if placed.version.is_empty() { "실행되지 않음" } else { placed.version.as_str() };
        return Some(format!("원격에서 보낸 파일을 실행한 판이 다르다(기대 {software}, 원격 {got})"));
    }
    None
}

impl Ssh {
    /// `file`을 원격 임시 폴더에 쓰고 SHA·판을 확인한다. 어긋나면 그 폴더를 지우고 까닭을 돌려준다
    pub(crate) async fn upload(
        &self,
        target: &Target,
        file: &Path,
        sha256: &str,
        software: &str,
    ) -> Result<Placed, String> {
        let input = std::fs::File::open(file).map_err(|e| format!("보낼 파일을 열지 못했다: {e}"))?;
        let mut c = self.trusted(target, &upload_script());
        c.stdin(std::process::Stdio::from(input));
        let r = Ssh::run(c, UPLOAD_TIMEOUT).await;
        let Some(placed) = parse_upload(&r.stdout) else {
            return Err(if r.ok {
                "원격이 파일 자리·SHA를 알리지 않았다(sh·mktemp·shasum이 있는지 본다)".into()
            } else {
                super::why(&r.stderr)
            });
        };
        if let Some(why) = mismatch(&placed, sha256, software) {
            self.cleanup(target, &placed.dir).await;
            return Err(why);
        }
        Ok(placed)
    }

    /// `file`을 이미 놓은 폴더 `dir`의 `name`으로 쓰고 SHA를 확인한다(실행 파일 옆 launcher package)
    pub(crate) async fn place_beside(
        &self,
        target: &Target,
        dir: &str,
        name: &str,
        file: &Path,
        sha256: &str,
    ) -> Result<(), String> {
        let script = beside_script(dir, name).ok_or("원격 폴더·파일 이름이 안전한 꼴이 아니다")?;
        let input = std::fs::File::open(file).map_err(|e| format!("보낼 launcher를 열지 못했다: {e}"))?;
        let mut c = self.trusted(target, &script);
        c.stdin(std::process::Stdio::from(input));
        let r = Ssh::run(c, UPLOAD_TIMEOUT).await;
        match parse_upload(&r.stdout) {
            Some(p) if p.dir == dir && p.sha256.eq_ignore_ascii_case(sha256) => Ok(()),
            Some(p) => Err(format!("원격에 쓴 launcher의 SHA-256이 다르다(보냄 {sha256}, 원격 {})", p.sha256)),
            None if r.ok => Err("원격이 launcher 자리·SHA를 알리지 않았다".into()),
            None => Err(super::why(&r.stderr)),
        }
    }

    /// 놓았던 임시 폴더를 지운다(실패해도 작업 결과는 바꾸지 않는다)
    pub(crate) async fn cleanup(&self, target: &Target, dir: &str) -> Option<Ran> {
        let script = cleanup_script(dir)?;
        Some(Ssh::run(self.trusted(target, &script), super::RUN_TIMEOUT).await)
    }
}

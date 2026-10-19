//! 여러 역할이 함께 쓰는 작은 도구: ID, 시각, boot ID, 사용자 전용 파일.

use std::io::Write as _;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// harness 세션 ID(Claude hook `session_id`·Codex notify `thread-id`)는 UUID 꼴(8-4-4-4-12 16진수)만 받는다. 값은 pane
/// 프로그램이 hook으로 알리므로 믿지 않는다: `--resume` 인자, 재부팅 뒤 셸에 치는 줄, 기록 파일 이름이 된다(보안 감사
/// t-0340: `--dangerously-skip-permissions` 같은 인자나 `; 명령`이 들어갈 수 있었다). Claude·Codex 모두 UUID를 쓴다.
pub fn harness_session_ok(s: &str) -> bool {
    s.len() == 36
        && s.bytes()
            .enumerate()
            .all(|(i, c)| if matches!(i, 8 | 13 | 18 | 23) { c == b'-' } else { c.is_ascii_hexdigit() })
}

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// UTC (년, 월, 일, 시, 분, 초). 시간대 라이브러리 없이 날짜를 계산한다(Howard Hinnant의 civil_from_days).
fn civil(ms: u64) -> (i64, i64, i64, u64, u64, u64) {
    let secs = ms / 1000;
    let (days, rem) = ((secs / 86_400) as i64, secs % 86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d, rem / 3600, rem % 3600 / 60, rem % 60)
}

/// 사람이 읽는 UTC 시각(`2026-10-16 12:05Z`)
pub fn utc_text(ms: u64) -> String {
    let (y, m, d, h, mi, _) = civil(ms);
    format!("{y:04}-{m:02}-{d:02} {h:02}:{mi:02}Z")
}

/// ISO 8601 UTC 시각(`2026-10-16T12:05:00.000Z`, JavaScript `toISOString`과 같은 꼴)
pub fn utc_iso(ms: u64) -> String {
    let (y, m, d, h, mi, s) = civil(ms);
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{s:02}.{:03}Z", ms % 1000)
}

/// ISO 8601 시각(`2026-10-16T12:05:00Z`, 소수 초·`+09:00` 같은 시간대, 날짜만도 된다) → epoch ms.
/// 1970년 전이나 형식이 아니면 None.
pub fn parse_iso(text: &str) -> Option<u64> {
    let t = text.trim();
    let num = |s: &str| -> Option<i64> {
        if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) { s.parse().ok() } else { None }
    };
    let (date, time) = match t.split_once(['T', 't', ' ']) {
        Some((d, rest)) => (d, Some(rest)),
        None => (t, None),
    };
    let mut parts = date.splitn(3, '-');
    let (y, m, d) = (num(parts.next()?)?, num(parts.next()?)?, num(parts.next()?)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // Howard Hinnant의 days_from_civil
    let yy = if m <= 2 { y - 1 } else { y };
    let era = yy.div_euclid(400);
    let yoe = yy - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let mut ms = days * 86_400_000;
    if let Some(time) = time {
        // 시간대: Z, +HH:MM, -HH:MM, +HHMM
        let (clock, offset) = if let Some(c) = time.strip_suffix(['Z', 'z']) {
            (c, 0)
        } else if let Some(i) = time.rfind(['+', '-']) {
            let (c, o) = time.split_at(i);
            let sign = if o.starts_with('-') { -1 } else { 1 };
            let digits: String = o[1..].chars().filter(|c| *c != ':').collect();
            if digits.len() != 4 {
                return None;
            }
            (c, sign * (num(&digits[..2])? * 60 + num(&digits[2..])?) * 60_000)
        } else {
            (time, 0)
        };
        let (hms, frac) = clock.split_once('.').unwrap_or((clock, ""));
        let mut hms = hms.split(':');
        let h = num(hms.next()?)?;
        let mi = num(hms.next()?)?;
        let s = hms.next().map(num).unwrap_or(Some(0))?;
        if h > 23 || mi > 59 || s > 60 {
            return None;
        }
        let frac_ms = if frac.is_empty() { 0 } else { num(&format!("{frac:0<3}")[..3])? };
        ms += ((h * 60 + mi) * 60 + s) * 1000 + frac_ms - offset;
    }
    u64::try_from(ms).ok()
}

/// 파일 이름에 쓰는 UTC 시각(`20261016T120500Z`)
pub fn utc_compact(ms: u64) -> String {
    let (y, m, d, h, mi, s) = civil(ms);
    format!("{y:04}{m:02}{d:02}T{h:02}{mi:02}{s:02}Z")
}

/// 정렬 가능한 새 ID(ULID 26자)
pub fn new_id() -> String {
    ulid::Ulid::new().to_string()
}

/// 커밋 빌드·P0 업그레이드 모의의 구분값. `AAA_BUILD_SALT`로 바꿔 「다시 빌드한 실행 파일」을 만든다.
pub const BUILD_SALT: &str = match option_env!("AAA_BUILD_SALT") {
    Some(s) => s,
    None => "",
};

pub fn software() -> String {
    if BUILD_SALT.is_empty() {
        format!("aaa {}", env!("CARGO_PKG_VERSION"))
    } else {
        format!("aaa {}+{}", env!("CARGO_PKG_VERSION"), BUILD_SALT)
    }
}

/// 이 부팅을 가리키는 ID. 재부팅하면 바뀐다(boot fencing).
pub fn boot_id() -> String {
    // 시험용: 재부팅을 흉내 낸다(E2E가 agent를 다른 boot ID로 다시 띄운다)
    if let Ok(v) = std::env::var("AAA_TEST_BOOT_ID")
        && !v.is_empty()
    {
        return v;
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(v) = sysctl_string("kern.bootsessionuuid") {
            return v;
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(v) = std::fs::read_to_string("/proc/sys/kernel/random/boot_id") {
            return v.trim().to_string();
        }
    }
    #[cfg(windows)]
    {
        // 부팅 시각(초)을 30초 단위로 반올림한다. 정밀한 boot ID는 M2에서 정한다.
        let up_ms = unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() };
        let boot_s = (now_ms().saturating_sub(up_ms)) / 1000;
        return format!("boot-{}", (boot_s + 15) / 30 * 30);
    }
    #[allow(unreachable_code)]
    "unknown-boot".to_string()
}

#[cfg(target_os = "macos")]
pub fn sysctl_string(name: &str) -> Option<String> {
    let cname = std::ffi::CString::new(name).ok()?;
    let mut len: libc::size_t = 0;
    unsafe {
        if libc::sysctlbyname(cname.as_ptr(), std::ptr::null_mut(), &mut len, std::ptr::null_mut(), 0) != 0 {
            return None;
        }
        let mut buf = vec![0u8; len];
        if libc::sysctlbyname(cname.as_ptr(), buf.as_mut_ptr().cast(), &mut len, std::ptr::null_mut(), 0) != 0 {
            return None;
        }
        buf.truncate(len);
        while buf.last() == Some(&0) {
            buf.pop();
        }
        String::from_utf8(buf).ok()
    }
}

pub fn hostname() -> String {
    #[cfg(unix)]
    {
        let mut buf = [0u8; 256];
        let ok = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) } == 0;
        if ok {
            let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
            let name = String::from_utf8_lossy(&buf[..end]).to_string();
            return name.trim_end_matches(".local").to_string();
        }
    }
    std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_else(|_| "unknown".into())
}

/// 머신 이름으로 쓸 수 있는가: 영문·숫자·`.`·`_`·`-`로 1~64자.
/// 이름은 CLI 표·push 본문·nudge에 그대로 나간다. 제어 문자로 화면을 위조하지 못하게 막는다(보안 감사 t-0340 B5).
pub fn machine_name_ok(name: &str) -> bool {
    (1..=64).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_alphanumeric() || b".-_".contains(&b))
}

/// hostname 같은 글에서 머신 이름 기본값을 만든다: 쓸 수 없는 글자는 `-`로 바꾸고 64자로 자른다.
pub fn machine_name_from(raw: &str) -> String {
    let s: String =
        raw.chars().map(|c| if c.is_ascii_alphanumeric() || ".-_".contains(c) { c } else { '-' }).take(64).collect();
    if s.is_empty() { "machine".into() } else { s }
}

/// 사용자만 읽고 쓰는 폴더(0700)를 만든다.
pub fn ensure_private_dir(p: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(p)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// 사용자 전용(0600) 파일을 원자적으로 쓴다(임시 파일 → rename).
pub fn write_private(p: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = p.parent().unwrap_or(Path::new("."));
    let tmp = dir.join(format!(".{}.tmp-{}", p.file_name().unwrap().to_string_lossy(), std::process::id()));
    {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, p)
}

pub fn write_json<T: serde::Serialize>(p: &Path, v: &T) -> std::io::Result<()> {
    write_private(p, &serde_json::to_vec_pretty(v).expect("JSON 직렬화"))
}

pub fn read_json<T: serde::de::DeserializeOwned>(p: &Path) -> anyhow::Result<T> {
    let bytes = std::fs::read(p)?;
    Ok(serde_json::from_slice(&bytes)?)
}

/// 같은 사용자가 띄운 프로세스가 살아 있는지
pub fn pid_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let r = unsafe { libc::kill(pid as libc::pid_t, 0) };
        r == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h.is_null() {
                return false;
            }
            let mut code = 0u32;
            let ok = GetExitCodeProcess(h, &mut code) != 0;
            CloseHandle(h);
            ok && code == 259 // STILL_ACTIVE
        }
    }
}

#[cfg(test)]
mod time_tests {
    use super::{parse_iso, utc_iso};

    #[test]
    fn iso_round_trip() {
        for ms in [0u64, 1_792_211_449_034, 951_782_400_000, 4_102_444_800_999] {
            assert_eq!(parse_iso(&utc_iso(ms)), Some(ms), "{}", utc_iso(ms));
        }
        assert_eq!(utc_iso(1_792_211_449_034), "2026-10-17T04:30:49.034Z");
        assert_eq!(parse_iso("2026-10-17T12:30:49+09:00"), parse_iso("2026-10-17T03:30:49Z"));
        assert_eq!(parse_iso("2026-10-17T03:30:49.123456+00:00"), Some(1_792_207_849_123));
        assert_eq!(parse_iso("2026-10-17"), Some(1_792_195_200_000));
        assert_eq!(parse_iso("not a date"), None);
        assert_eq!(parse_iso("2026-13-01T00:00:00Z"), None);
    }

    #[test]
    fn machine_names() {
        use super::{machine_name_from, machine_name_ok};
        for ok in ["mac-a", "build_01.lan", "A", &"x".repeat(64)] {
            assert!(machine_name_ok(ok), "{ok}");
        }
        for bad in ["", "mac a", "mac\x1b[2Ja", "탭", "a\nb", "a\u{202e}b", &"x".repeat(65)] {
            assert!(!machine_name_ok(bad), "{bad:?}");
        }
        assert_eq!(machine_name_from("Jo's Mac\x07"), "Jo-s-Mac-");
        assert_eq!(machine_name_from(""), "machine");
        assert!(machine_name_ok(&machine_name_from(&"한".repeat(100))));
    }

    #[test]
    fn utc_text_dates() {
        assert_eq!(super::utc_text(0), "1970-01-01 00:00Z");
        // 2024-02-29 23:59:59Z(윤일)
        assert_eq!(super::utc_text(1_709_251_199_000), "2024-02-29 23:59Z");
        assert_eq!(super::utc_text(1_792_152_000_000), "2026-10-16 12:00Z");
        assert_eq!(super::utc_compact(1_709_251_199_000), "20240229T235959Z");
    }
}

#[cfg(test)]
mod session_tests {
    use super::harness_session_ok;

    #[test]
    fn harness_session_is_a_uuid() {
        for ok in ["0f5b2c1e-7a3d-4c8e-9b21-5d6e7f809a1b", "019A4C5E-1F2B-7C3D-8E4F-A5B6C7D8E9F0"] {
            assert!(harness_session_ok(ok), "{ok}");
        }
        for bad in [
            "",
            "--dangerously-skip-permissions",
            "0f5b2c1e-7a3d-4c8e-9b21-5d6e7f809a1b; rm -rf ~",
            "x; touch pwned",
            "../../../../etc/passwd",
            "0f5b2c1e7a3d4c8e9b215d6e7f809a1b",
            "0f5b2c1e-7a3d-4c8e-9b21-5d6e7f809a1",
            "0f5b2c1e-7a3d-4c8e-9b21_5d6e7f809a1b",
            "0f5b2c1e-7a3d-4c8e-9b21-5d6e7f809a1g",
            "-f5b2c1e-7a3d-4c8e-9b21-5d6e7f809a1b",
            "０f5b2c1e-7a3d-4c8e-9b21-5d6e7f809a1b",
        ] {
            assert!(!harness_session_ok(bad), "{bad}");
        }
    }
}

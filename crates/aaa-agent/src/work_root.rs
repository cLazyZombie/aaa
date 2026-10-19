//! 작업 디스크가 없는데 내장 디스크에 대체 폴더를 만들거나 삭제로 판정하지 않는다.
//! 경로는 t3code worktreesDirectory의 실제 경로 검사처럼 존재하는 조상부터 해석한다.
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

mod registry;
pub(crate) use registry::{Lease, shared};
pub use registry::{RootInfo, RootState, RootTransaction, activate, inspect, probe};

/// 존재하지 않는 마지막 조각도 실제 조상 기준으로 해석한다. 끊긴 symlink는 오류다.
pub(crate) fn real_location(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute() || path.components().any(|p| p == std::path::Component::ParentDir) {
        bail!("작업 경로는 상위 탈출 없는 절대 경로여야 한다: {}", path.display());
    }
    match path.symlink_metadata() {
        Ok(_) => path.canonicalize().with_context(|| format!("작업 경로를 해석하지 못했다: {}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let parent = path.parent().context("작업 경로의 부모가 없다")?;
            Ok(real_location(parent)?.join(path.file_name().context("작업 경로 이름이 없다")?))
        }
        Err(e) => Err(e).with_context(|| format!("작업 경로를 읽지 못했다: {}", path.display())),
    }
}

fn existing(path: &Path) -> Result<&Path> {
    for p in path.ancestors() {
        match p.metadata() {
            Ok(m) if m.is_dir() => return Ok(p),
            Ok(_) => bail!("작업 경로의 조상이 폴더가 아니다: {}", p.display()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    bail!("작업 경로의 디스크를 확인하지 못했다: {}", path.display())
}

/// macOS 외장 기본 위치는 설정 누락으로 검사를 건너뛰지 않는다.
pub(crate) fn inferred_mount(path: &Path) -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let mut components = path.strip_prefix("/Volumes").ok()?.components();
        if let Some(std::path::Component::Normal(name)) = components.next() {
            return Some(Path::new("/Volumes").join(name));
        }
    }
    let _ = path;
    None
}

/// mount 경로가 존재하는지만 보면 빠진 디스크 자리에 남은 일반 폴더를 통과시킨다.
pub fn check(path: &Path, configured_mount: Option<&Path>) -> Result<PathBuf> {
    let inferred = inferred_mount(path);
    let real = real_location(path)?;
    let inferred_real = inferred_mount(&real);
    // 명시 설정으로 /Volumes 필수 검사를 덮어쓰지 않는다. symlink 뒤의 경로도 검사한다.
    for expected in [configured_mount, inferred.as_deref(), inferred_real.as_deref()].into_iter().flatten() {
        let mount = expected.canonicalize().with_context(|| {
            format!("작업 볼륨이 연결되지 않았다: {}. 연결한 뒤 agent를 다시 시작한다", expected.display())
        })?;
        let actual = mount_point(existing(&mount)?)?.canonicalize()?;
        if actual != mount {
            bail!("작업 볼륨이 마운트되지 않았다: {} (현재 디스크: {})", expected.display(), actual.display());
        }
        // macOS /Users 같은 firmlink는 Data mount의 문자열 접두사 아래가 아닐 수 있다.
        // 실제 mount 지점이 같은지로 지정 볼륨 소속을 확인한다.
        if mount_point(existing(&real)?)?.canonicalize()? != mount {
            bail!("작업 경로가 지정한 볼륨 밖을 가리킨다: {}", path.display());
        }
    }
    Ok(real)
}

/// 관리 경로의 중간 symlink도 root 밖에 새 작업을 만들게 할 수 없다.
pub(crate) fn check_managed_path(root: &Path, mount: Option<&Path>, path: &Path) -> Result<()> {
    if root.is_absolute() && path.starts_with(root) {
        let base = check(root, mount)?;
        if !check(path, mount)?.starts_with(base) {
            bail!("작업 경로가 root 밖을 가리킨다: {}", path.display());
        }
    } else {
        check(path, None)?;
    }
    Ok(())
}

/// agent 시작의 동기 preflight. 마운트를 확인하기 전에는 폴더도 probe도 만들지 않는다.
pub fn initialize(root: &Path, mount: Option<&Path>) -> Result<()> {
    let real = check(root, mount)?;
    if real == mount_point(existing(&real)?)?.canonicalize()? {
        bail!("작업 root는 볼륨 최상위일 수 없다: {}", root.display());
    }
    std::fs::create_dir_all(root).with_context(|| format!("작업 root를 만들지 못했다: {}", root.display()))?;
    let probe = root.join(format!(".aaa-probe-{}", aaa_common::util::new_id()));
    let value = aaa_common::util::new_id();
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .with_context(|| format!("작업 root에 쓸 수 없다: {}", root.display()))?;
    use std::io::Write;
    let result = (|| -> Result<()> {
        file.write_all(value.as_bytes())?;
        file.sync_all()?;
        if std::fs::read(&probe)? != value.as_bytes() {
            bail!("작업 root에서 읽은 내용이 다르다");
        }
        Ok(())
    })();
    drop(file);
    let remove = std::fs::remove_file(&probe);
    result?;
    remove?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn mount_point(path: &Path) -> Result<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes())?;
    let mut fs: libc::statfs = unsafe { std::mem::zeroed() };
    // SAFETY: c는 NUL로 끝나고 fs는 statfs가 쓸 수 있는 메모리다.
    if unsafe { libc::statfs(c.as_ptr(), &mut fs) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: 성공한 statfs의 f_mntonname은 NUL로 끝나는 mount 지점이다.
    let name = unsafe { std::ffi::CStr::from_ptr(fs.f_mntonname.as_ptr()) };
    Ok(PathBuf::from(std::ffi::OsStr::from_bytes(name.to_bytes())))
}

#[cfg(target_os = "linux")]
fn mount_point(path: &Path) -> Result<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    let text = std::fs::read_to_string("/proc/self/mountinfo")?;
    let mut best: Option<PathBuf> = None;
    for line in text.lines() {
        let Some(raw) = line.split_whitespace().nth(4) else { continue };
        let mut bytes = Vec::new();
        let mut input = raw.as_bytes().iter().copied();
        while let Some(b) = input.next() {
            if b == b'\\' {
                let digits: Vec<_> = input.by_ref().take(3).collect();
                if digits.len() != 3 || digits.iter().any(|d| !(b'0'..=b'7').contains(d)) {
                    bail!("잘못된 mountinfo escape");
                }
                let byte =
                    u16::from(digits[0] - b'0') * 64 + u16::from(digits[1] - b'0') * 8 + u16::from(digits[2] - b'0');
                bytes.push(u8::try_from(byte).context("잘못된 mountinfo byte")?);
            } else {
                bytes.push(b);
            }
        }
        let p = PathBuf::from(std::ffi::OsString::from_vec(bytes));
        if path.starts_with(&p) && best.as_ref().is_none_or(|b| p.components().count() > b.components().count()) {
            best = Some(p);
        }
    }
    best.context("작업 디스크의 mount 지점을 찾지 못했다")
}

#[cfg(windows)]
fn mount_point(path: &Path) -> Result<PathBuf> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    let input: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut output = vec![0u16; 32768];
    // SAFETY: 입력은 NUL로 끝나고 output은 전달한 길이만큼 쓰기 가능하다.
    if unsafe {
        windows_sys::Win32::Storage::FileSystem::GetVolumePathNameW(
            input.as_ptr(),
            output.as_mut_ptr(),
            output.len() as u32,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    let end = output.iter().position(|c| *c == 0).context("volume 경로의 끝이 없다")?;
    Ok(std::ffi::OsString::from_wide(&output[..end]).into())
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn mount_point(_: &Path) -> Result<PathBuf> {
    bail!("이 OS에서 작업 mount 검사를 지원하지 않는다")
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn missing_or_unmounted_volume_never_creates_a_replacement_or_reports_parent_space() {
        let lab = std::env::temp_dir().join(format!("aaa-workroot-{}", aaa_common::util::new_id()));
        std::fs::create_dir(&lab).unwrap();
        let mount = lab.join("not-a-mount");
        let root = mount.join("work");
        // 없음과 빠진 볼륨 자리에 남은 일반 폴더를 둘 다 거부해야 한다.
        for leftover in [false, true] {
            if leftover {
                std::fs::create_dir(&mount).unwrap();
            }
            let error = initialize(&root, Some(&mount)).unwrap_err().to_string();
            assert!(error.contains("작업 볼륨"), "{error}");
            assert!(!root.exists());
            let health = crate::health::collect(&aaa_config::HealthSection::default(), &root, Some(&mount)).await;
            assert!(health.disk_free_bytes.is_none());
            assert!(health.problems.iter().any(|p| p.code == "work_root"));
            // routine 기본 cwd도 같은 실패를 거쳐 명령 자체를 실행하지 않아야 한다.
            assert!(
                crate::command::run(
                    &root,
                    Some(&mount),
                    "demo",
                    None,
                    "echo unexpected",
                    std::time::Duration::from_secs(10)
                )
                .await
                .is_err()
            );
            assert!(!root.exists());
        }
        let good = lab.join("local/work");
        initialize(&good, None).unwrap();
        assert_eq!(std::fs::read_dir(&good).unwrap().count(), 0, "probe는 남기지 않는다");
        let escape = good.join("repo-link");
        std::os::unix::fs::symlink(&lab, &escape).unwrap();
        assert!(check_managed_path(&good, None, &escape.join("new-thread")).is_err());
        assert!(!lab.join("new-thread").exists());
        // 실제 경로가 디스크 root인 symlink도 root 설정으로 쓸 수 없다.
        let link = lab.join("root-link");
        std::os::unix::fs::symlink("/", &link).unwrap();
        assert!(initialize(&link, None).unwrap_err().to_string().contains("최상위"));
        std::fs::remove_dir_all(lab).unwrap();
    }
}

#[cfg(all(test, unix))]
#[path = "work_root/recovery_tests.rs"]
mod recovery_tests;

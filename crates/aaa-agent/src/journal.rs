//! agent가 소유한 작은 JSON 기록을 원자적으로 보관한다.
use anyhow::{Context, Result, bail};
use std::path::Path;

pub(crate) fn write<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    let dir = path.parent().context("journal 부모가 없다")?;
    std::fs::create_dir_all(dir)?;
    if dir.symlink_metadata()?.file_type().is_symlink() {
        bail!("journal 폴더는 symlink일 수 없다");
    }
    aaa_common::util::ensure_private_dir(dir)?;
    let tmp = dir.join(format!(".{}.tmp", aaa_common::util::new_id()));
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts.open(&tmp)?;
    use std::io::Write;
    let mut value = serde_json::to_value(value)?;
    aaa_common::redact::value(&mut value);
    file.write_all(&serde_json::to_vec(&value)?)?;
    file.sync_all()?;
    drop(file);
    // Windows에서도 기존 journal을 원자적으로 교체한다.
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub(crate) fn read<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    let m = match path.symlink_metadata() {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    if !m.is_file() || m.len() > 4 << 20 {
        bail!("journal은 4MiB 이하 일반 파일이어야 한다");
    }
    Ok(Some(serde_json::from_slice(&std::fs::read(path)?)?))
}

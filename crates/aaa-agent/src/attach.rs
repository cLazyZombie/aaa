//! 붙여 넣은 그림(§8.20, M1-4). host가 조각으로 보낸 파일을 세션 폴더 `attachments/`에 쓴다.
//!
//! - 이름은 host가 정한다(`<hex>.<png|jpg|gif|webp>`, task 첨부 문서는 `md|html`). 다른 이름·경로는 받지 않는다.
//! - `.part`에 이어 쓰고 마지막 조각에서 이름을 바꾼다. 폴더 0700, 파일 0600(unix).
//! - 세션 기록을 지우면(`forget`) 폴더와 함께 사라진다.
//! - task 첨부(plan §8.323)는 thread 작업 폴더 `attachments/`에 같은 꼴로 쓴다(`write_work`). 이름이 내용 hash라 다시 시작하면
//!   같은 이름을 덮어쓴다.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use sha2::{Digest, Sha256};

/// 첨부 한 개 상한
pub const MAX_BYTES: u64 = 16 << 20;
/// 세션 하나에 남기는 첨부 수 상한
const MAX_FILES: usize = 64;

fn valid_att(att: &str) -> bool {
    let Some((stem, ext)) = att.split_once('.') else { return false };
    stem.len() == 16
        && stem.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        && matches!(ext, "png" | "jpg" | "gif" | "webp" | "md" | "html")
}

fn private_dir(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// 한 조각을 쓴다. 마지막 조각이면 (절대 경로, sha256)을 돌려준다.
pub fn write(session_dir: &Path, att: &str, offset: u64, data: &[u8], eof: bool) -> Result<Option<(String, String)>> {
    write_in(&session_dir.join("attachments"), att, offset, data, eof, false)
}

/// task 첨부 한 조각을 thread 작업 폴더 `attachments/`에 쓴다(`AgentRequest::WorkAttachWrite`). 같은 이름은 덮어쓴다
pub fn write_work(work_dir: &Path, att: &str, offset: u64, data: &[u8], eof: bool) -> Result<Option<(String, String)>> {
    let dir = work_dir.join("attachments");
    if dir.symlink_metadata().is_ok_and(|m| !m.is_dir()) {
        bail!("attachments가 폴더가 아니다(symlink 등)");
    }
    write_in(&dir, att, offset, data, eof, true)
}

fn write_in(
    dir: &Path,
    att: &str,
    offset: u64,
    data: &[u8],
    eof: bool,
    replace: bool,
) -> Result<Option<(String, String)>> {
    if !valid_att(att) {
        bail!("첨부 이름 형식이 아니다");
    }
    if offset + data.len() as u64 > MAX_BYTES {
        bail!("첨부가 {}MiB를 넘는다", MAX_BYTES >> 20);
    }
    let part = dir.join(format!("{att}.part"));
    let mut f = if offset == 0 {
        private_dir(dir)?;
        if dir.join(att).exists() && !replace {
            bail!("같은 이름의 첨부가 있다");
        }
        if replace {
            // 끊긴 앞 전송의 조각은 버린다
            let _ = std::fs::remove_file(&part);
        }
        let n = std::fs::read_dir(dir)?
            .filter(|e| e.as_ref().is_ok_and(|e| !e.file_name().to_string_lossy().ends_with(".part")))
            .count();
        if n >= MAX_FILES {
            bail!("이 세션의 첨부가 {MAX_FILES}개를 넘었다(세션 기록을 지우면 함께 지워진다)");
        }
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            o.mode(0o600);
        }
        o.open(&part)?
    } else {
        let f = std::fs::OpenOptions::new().append(true).open(&part)?;
        let have = f.metadata()?.len();
        if have != offset {
            bail!("조각 위치가 맞지 않다(쓴 길이 {have}, 받은 위치 {offset})");
        }
        f
    };
    f.write_all(data)?;
    if !eof {
        return Ok(None);
    }
    f.sync_all()?;
    drop(f);
    let sha = hex::encode(Sha256::digest(std::fs::read(&part)?));
    let done = dir.join(att);
    std::fs::rename(&part, &done)?;
    let abs: PathBuf = std::path::absolute(&done)?;
    Ok(Some((abs.to_string_lossy().into_owned(), sha)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_and_names() {
        let root = std::env::temp_dir().join(format!("aaa-attach-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let s = root.join("sessions").join("s1");
        for bad in ["../x.png", "0123456789abcdef.exe", "0123456789ABCDEF.png", "x.png"] {
            assert!(write(&s, bad, 0, b"a", true).is_err(), "{bad}");
        }
        let att = "0123456789abcdef.png";
        assert!(write(&s, att, 0, b"ab", false).unwrap().is_none());
        assert!(write(&s, att, 5, b"cd", true).is_err(), "건너뛴 위치");
        let (path, sha) = write(&s, att, 2, b"cd", true).unwrap().unwrap();
        // 경로 구분자는 OS마다 다르다(Windows는 `\`)
        let p = Path::new(&path);
        assert!(p.is_absolute() && p.ends_with(Path::new("attachments").join(att)), "{path}");
        assert_eq!(std::fs::read(&path).unwrap(), b"abcd");
        assert_eq!(sha, hex::encode(Sha256::digest(b"abcd")));
        assert!(write(&s, att, 0, b"x", true).is_err(), "같은 이름을 다시 만들지 않는다");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!((mode(&s.join("attachments")), mode(Path::new(&path))), (0o700, 0o600));
        }
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn work_attachments_replace_same_name() {
        let root = std::env::temp_dir().join(format!("aaa-work-attach-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let w = root.join(".aaa").join("t-0001");
        std::fs::create_dir_all(&w).unwrap();
        let att = "0123456789abcdef.png";
        assert!(write_work(&w, att, 0, b"ab", false).unwrap().is_none());
        // 끊긴 전송 뒤 다시 시작해도 처음부터 쓴다
        let (path, _) = write_work(&w, att, 0, b"xy", true).unwrap().unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"xy");
        let (path, _) = write_work(&w, att, 0, b"zz", true).unwrap().unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"zz", "다시 시작하면 덮어쓴다");
        assert!(write_work(&w, "../x.png", 0, b"a", true).is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }
}

//! 공용 저장 helper(할 일 편집 T2, plan §8.310. W2 자동화 저장이 그대로 쓴다, t-0469 §6.1): 파일을 글과 sha256(판)으로 읽고, 고침
//! closure가 새 글을 정하면 같은 폴더 임시 파일 → rename으로 쓴다(`orch::write_atomic`). 기대 판 대조는 closure가 읽은 판으로 한다.
//!
//! - 쓰기는 파일마다 하나씩 줄 세운다(`Locks`). 그 파일을 보는 다른 host 일(ticker의 외부 변경 감지)도 같은 lock 안에서 본다.
//! - rename 직전에 다시 읽어 그사이 사람·agent의 직접 편집이 있었으면 새 판으로 closure를 다시 돌린다(동작은 블록 hash로 대상을
//!   찾으므로 새 판 위에서도 같은 뜻이다). 계속 바뀌면 멈추고 오류를 돌려준다.
//! - UTF-8이 아닌 파일은 고치지 않는다(글로 읽어 다시 쓰면 바이트가 바뀐다).

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use sha2::Digest as _;

/// 읽은 판
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Rev {
    pub text: String,
    /// 파일 바이트의 sha256(파일이 없으면 빈 글의 hash)
    pub sha256: String,
    pub exists: bool,
}

pub(crate) fn sha256(bytes: &[u8]) -> String {
    hex::encode(sha2::Sha256::digest(bytes))
}

pub(crate) fn read(path: &Path) -> io::Result<Rev> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Ok(Rev { text: String::new(), sha256: sha256(b""), exists: false });
        }
        Err(e) => return Err(e),
    };
    let sha256 = sha256(&bytes);
    let text = String::from_utf8(bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, format!("{}: UTF-8 글이 아니다", path.display())))?;
    Ok(Rev { text, sha256, exists: true })
}

/// 파일마다의 쓰기 lock
#[derive(Default)]
pub(crate) struct Locks(Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>);

impl Locks {
    pub(crate) fn of(&self, path: &Path) -> Arc<Mutex<()>> {
        self.0.lock().unwrap().entry(path.to_path_buf()).or_default().clone()
    }
}

/// 고침 closure의 답: 새 글을 쓰거나, 그대로 둔다
pub(crate) enum Edit<T> {
    Write(String, T),
    Keep(T),
}

pub(crate) struct Saved<T> {
    /// closure가 마지막으로 본 판
    pub before: Rev,
    /// 쓴 판(그대로 두었으면 없음)
    pub after: Option<Rev>,
    pub out: T,
}

/// 다시 읽어 판이 바뀌었으면 closure를 다시 돌리는 횟수
const ROUNDS: usize = 3;

/// 저장 실패: 파일 읽기·쓰기, 또는 고침 closure가 거부했다
#[derive(Debug)]
pub(crate) enum SaveError<E> {
    Io(io::Error),
    Edit(E),
}

/// `path`를 읽어 `edit`에 주고, 새 글이면 임시 파일 → rename으로 쓴다. 부르는 쪽이 `Locks::of(path)`를 쥐고 부른다.
pub(crate) fn save<T, E>(
    path: &Path,
    mut edit: impl FnMut(&Rev) -> Result<Edit<T>, E>,
) -> Result<Saved<T>, SaveError<E>> {
    for _ in 0..ROUNDS {
        let before = read(path).map_err(SaveError::Io)?;
        let (text, out) = match edit(&before).map_err(SaveError::Edit)? {
            Edit::Keep(out) => return Ok(Saved { before, after: None, out }),
            Edit::Write(text, out) => (text, out),
        };
        if read(path).map_err(SaveError::Io)?.sha256 != before.sha256 {
            continue;
        }
        crate::orch::write_atomic(path, text.as_bytes()).map_err(SaveError::Io)?;
        let after = Rev { sha256: sha256(text.as_bytes()), text, exists: true };
        return Ok(Saved { before, after: Some(after), out });
    }
    Err(SaveError::Io(io::Error::other(format!("{}: 저장하는 동안 파일이 계속 바뀌었다. 다시 연다", path.display()))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("aaa-file-rev-{name}-{}", aaa_common::util::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn save_writes_through_rename_and_reports_revisions() {
        let dir = scratch("save");
        let path = dir.join("TASKS.md");
        // 없는 파일은 빈 판이다
        let s = save::<_, ()>(&path, |r| {
            assert!(!r.exists && r.text.is_empty());
            Ok(Edit::Write("## A\n".into(), ()))
        })
        .unwrap();
        assert_eq!(s.after.unwrap().sha256, sha256(b"## A\n"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "## A\n");
        // 그대로 두면 쓰지 않는다
        let s = save::<_, ()>(&path, |_| Ok(Edit::Keep(1))).unwrap();
        assert!(s.after.is_none() && s.out == 1);
        // 기대 판 대조는 closure가 한다(W2 AutomationSave 꼴)
        let stale = sha256(b"old");
        let e =
            save::<(), &str>(&path, |r| if r.sha256 == stale { Ok(Edit::Keep(())) } else { Err("revision_conflict") });
        assert!(matches!(e, Err(SaveError::Edit("revision_conflict"))));
        // 임시 파일이 남지 않는다
        let names: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names, ["TASKS.md"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// closure가 고치는 동안 다른 쓰기가 끼면 새 판으로 다시 돌린다
    #[test]
    fn save_reruns_on_a_concurrent_write() {
        let dir = scratch("rerun");
        let path = dir.join("f.md");
        std::fs::write(&path, "a\n").unwrap();
        let mut seen = Vec::new();
        let s = save::<_, ()>(&path, |r| {
            seen.push(r.text.clone());
            if seen.len() == 1 {
                std::fs::write(&path, "b\n").unwrap();
            }
            Ok(Edit::Write(format!("{}c\n", r.text), ()))
        })
        .unwrap();
        assert_eq!(seen, ["a\n", "b\n"]);
        assert_eq!(s.before.text, "b\n");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "b\nc\n");
        // UTF-8이 아니면 읽지 않는다
        std::fs::write(&path, [0xff, 0xfe]).unwrap();
        assert_eq!(read(&path).unwrap_err().kind(), io::ErrorKind::InvalidData);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

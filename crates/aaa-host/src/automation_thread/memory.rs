//! 기억 회수의 파일별 3-way 판정. 공통 판의 hash만 저장하고 충돌 내용은 따로 보존한다.
use super::{bad, digest, local_files, relative};
use aaa_proto::ErrorBody;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub(super) type Files = BTreeMap<String, Vec<u8>>;
pub(crate) type Hashes = BTreeMap<String, String>;

pub(super) fn hashes(files: &Files) -> Hashes {
    files.iter().map(|(p, b)| (p.clone(), digest(b))).collect()
}

pub(super) struct Merge {
    pub files: Files,
    pub common: Hashes,
    pub conflicts: Vec<String>,
}

pub(super) fn merge(base: Option<&Hashes>, host: &Files, thread: &Files) -> Merge {
    let host_hashes = hashes(host);
    let thread_hashes = hashes(thread);
    let paths: BTreeSet<_> = host.keys().chain(thread.keys()).chain(base.into_iter().flat_map(|b| b.keys())).collect();
    let mut common = base.cloned().unwrap_or_default();
    let mut files = host.clone();
    let mut conflicts = BTreeSet::new();
    for path in paths {
        let h = host_hashes.get(path);
        let t = thread_hashes.get(path);
        let b = base.and_then(|b| b.get(path));
        if h == t || base.is_some() && h == b {
            if let Some(bytes) = thread.get(path) {
                files.insert(path.clone(), bytes.clone());
                common.insert(path.clone(), t.unwrap().clone());
            } else {
                files.remove(path);
                common.remove(path);
            }
        } else if base.is_none() || t != b {
            // 옛 실행처럼 기준이 없으면 같은 바이트만 합의된 판으로 인정한다.
            conflicts.insert(path.clone());
        }
        // host만 바뀌었으면 원문과 옛 공통 판을 유지한다. 뒤 회수도 그 변경을 덮지 않는다.
    }
    // 서로 다른 파일을 더했어도 부모 파일과 자식 경로는 함께 쓸 수 없다.
    let collisions: Vec<_> = files
        .keys()
        .filter(|path| Path::new(path).ancestors().skip(1).any(|p| p.to_str().is_some_and(|p| files.contains_key(p))))
        .cloned()
        .collect();
    for child in collisions {
        let Some(parent) =
            Path::new(&child).ancestors().skip(1).filter_map(|p| p.to_str()).find(|p| files.contains_key(*p))
        else {
            continue;
        };
        let parent = parent.to_owned();
        let affected: BTreeSet<_> = files
            .keys()
            .chain(host.keys())
            .chain(base.into_iter().flat_map(|b| b.keys()))
            .filter(|p| **p == parent || p.starts_with(&format!("{parent}/")))
            .cloned()
            .collect();
        for path in affected {
            conflicts.insert(path.clone());
            files.remove(&path);
            common.remove(&path);
            if let Some(bytes) = host.get(&path) {
                files.insert(path.clone(), bytes.clone());
            }
            if let Some(hash) = base.and_then(|b| b.get(&path)) {
                common.insert(path, hash.clone());
            }
        }
    }
    Merge { files, common, conflicts: conflicts.into_iter().collect() }
}

pub(super) fn read(root: &Path) -> Result<Files, ErrorBody> {
    Ok(local_files(root)?.into_iter().collect())
}

pub(super) fn write(root: &Path, files: &Files) -> Result<(), ErrorBody> {
    std::fs::create_dir_all(root).map_err(bad)?;
    for (path, bytes) in files {
        if !relative(path) {
            return Err(bad("기억 상대 경로가 아니다"));
        }
        let dest = root.join(path);
        std::fs::create_dir_all(dest.parent().unwrap()).map_err(bad)?;
        crate::orch::write_atomic(&dest, bytes).map_err(bad)?;
    }
    Ok(())
}

/// 회수 대상 밖에 두므로 4KiB 요약·8MiB 전송 상한에 보존 사본이 섞이지 않는다.
pub(super) fn preserve(
    dest: &Path,
    thread_id: &str,
    host: &Files,
    thread: &Files,
    conflicts: &[String],
) -> Result<String, ErrorBody> {
    let host_hashes = hashes(host);
    let thread_hashes = hashes(thread);
    let revisions: Vec<_> = conflicts.iter().map(|p| (p, host_hashes.get(p), thread_hashes.get(p))).collect();
    let revision = digest(&serde_json::to_vec(&revisions).map_err(bad)?);
    let name = dest.file_name().unwrap().to_string_lossy();
    let relative = format!("{name}-conflicts/{thread_id}/{revision}");
    let root = dest.parent().unwrap().join(&relative);
    // symlink로 운영 기억 밖을 따라가지 않는다.
    for path in root.ancestors().take(3) {
        if path
            .symlink_metadata()
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "성공 여부만 쓰는 분기의 하위 오류"))
            .is_ok_and(|m| !m.is_dir())
        {
            return Err(bad("충돌 보존 자리가 일반 폴더가 아니다"));
        }
    }
    if !root.exists() {
        let tmp = root.with_file_name(format!(".{}", aaa_common::util::new_id()));
        let result = (|| {
            write(&tmp.join("host"), host)?;
            write(&tmp.join("thread"), thread)?;
            // 양쪽에서 지운 경로도 목록과 각 tree의 부재로 판별할 수 있다.
            crate::orch::write_atomic(&tmp.join("conflicts.json"), &serde_json::to_vec_pretty(conflicts).map_err(bad)?)
                .map_err(bad)?;
            std::fs::rename(&tmp, &root).map_err(bad)
        })();
        if result
            .as_ref()
            .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
            .is_err()
        {
            aaa_common::diagnostic::ignored(
                std::fs::remove_dir_all(&tmp),
                "정리 실패가 본 작업의 결과를 덮지 않게 한다",
            );
        }
        result?;
    }
    Ok(format!("automations/{relative}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn files(values: &[(&str, &str)]) -> Files {
        values.iter().map(|(p, b)| (p.to_string(), b.as_bytes().to_vec())).collect()
    }
    #[test]
    fn independent_changes_deletions_and_repeated_compaction() {
        let seed = files(&[("memory.md", "old"), ("history/old.md", "old")]);
        let host = files(&[("memory.md", "old"), ("history/coordinator.md", "keep")]);
        let thread = files(&[("memory.md", "compact"), ("history/old.md", "old"), ("history/new.md", "archive")]);
        let first = merge(Some(&hashes(&seed)), &host, &thread);
        assert!(first.conflicts.is_empty());
        assert_eq!(
            first.files,
            files(&[("memory.md", "compact"), ("history/coordinator.md", "keep"), ("history/new.md", "archive")])
        );
        let next = files(&[("memory.md", "next"), ("history/old.md", "old"), ("history/new.md", "archive")]);
        let second = merge(Some(&first.common), &first.files, &next);
        assert!(second.conflicts.is_empty());
        assert_eq!(second.files["memory.md"], b"next");
        assert_eq!(second.files["history/coordinator.md"], b"keep");
        assert!(!second.files.contains_key("history/old.md"));
    }
    #[test]
    fn conflicting_edit_delete_add_and_unknown_base_keep_host() {
        for (base, host, thread) in [
            (files(&[("memory.md", "old")]), files(&[("memory.md", "host")]), files(&[("memory.md", "thread")])),
            (files(&[("memory.md", "old")]), Files::new(), files(&[("memory.md", "thread")])),
            (files(&[("memory.md", "old")]), files(&[("memory.md", "host")]), Files::new()),
            (Files::new(), files(&[("memory.md", "host")]), files(&[("memory.md", "thread")])),
        ] {
            let m = merge(Some(&hashes(&base)), &host, &thread);
            assert_eq!(m.files, host);
            assert_eq!(m.conflicts, ["memory.md"]);
            assert_eq!(merge(Some(&m.common), &m.files, &thread).conflicts, m.conflicts);
        }
        let host = files(&[("memory.md", "host")]);
        let remote = files(&[("memory.md", "thread")]);
        assert_eq!(merge(None, &host, &remote).conflicts, ["memory.md"]);
        assert!(merge(None, &host, &host).conflicts.is_empty());
    }
    #[test]
    fn same_change_agrees_and_path_collisions_preserve_host() {
        let host = files(&[("history/item", "host")]);
        let thread = files(&[("history/item/child", "thread"), ("history/item/second", "thread2")]);
        let m = merge(Some(&Hashes::new()), &host, &thread);
        assert_eq!(m.files, host);
        assert!(!m.conflicts.is_empty());
        let m = merge(Some(&Hashes::new()), &thread, &host);
        assert_eq!(m.files, thread);
        assert!(!m.conflicts.is_empty());
        assert!(merge(Some(&Hashes::new()), &host, &host).conflicts.is_empty());
        let base = files(&[("history/item/old", "old")]);
        let host = files(&[("history/item/old", "old"), ("history/item/new", "host")]);
        let thread = files(&[("history/item", "thread")]);
        assert_eq!(merge(Some(&hashes(&base)), &host, &thread).files, host);
    }
    #[test]
    fn host_only_edit_stays_protected_until_both_sides_agree() {
        let original = files(&[("memory.md", "old")]);
        let host = files(&[("memory.md", "old\ncoordinator")]);
        let first = merge(Some(&hashes(&original)), &host, &original);
        assert_eq!(first.files, host);
        assert!(first.conflicts.is_empty());
        let thread = files(&[("memory.md", "compact")]);
        let next = merge(Some(&first.common), &first.files, &thread);
        assert_eq!(next.files, host);
        assert_eq!(next.conflicts, ["memory.md"]);
        let resolved = merge(Some(&next.common), &thread, &thread);
        assert_eq!(resolved.common, hashes(&thread));
        assert!(resolved.conflicts.is_empty());
    }
}

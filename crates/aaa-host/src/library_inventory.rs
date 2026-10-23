//! manifest 밖에 남은 파일·폴더도 실제 host 사본 용량과 정리 후보에 포함한다.
use crate::{Host, library::manifest_path, library_publish::io_error, orch::internal};
use aaa_proto::ErrorBody;
use std::{collections::BTreeSet, path::Path};

pub(crate) struct LibraryCopy {
    pub project: String,
    pub name: String,
    /// 최신 별칭과 그 불변 판은 함께 정리해 별칭이 옛 r 번호로 돌아가지 않게 한다.
    pub family: String,
    pub size: u64,
    pub updated_at_ms: u64,
    pub protected: bool,
    pub archived: bool,
}

fn disk_size(root: &Path) -> std::io::Result<u64> {
    let meta = match std::fs::symlink_metadata(root) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e),
    };
    if meta.file_type().is_symlink() {
        return Ok(0);
    }
    if meta.is_file() {
        return Ok(meta.len());
    }
    if !meta.is_dir() {
        return Ok(0);
    }
    let mut size = 0u64;
    for e in std::fs::read_dir(root)? {
        size = size.saturating_add(disk_size(&e?.path())?);
    }
    Ok(size)
}

impl Host {
    pub(crate) fn library_inventory(&self) -> Result<Vec<LibraryCopy>, ErrorBody> {
        let mut copies = Vec::new();
        // catalog의 보호 판정은 source마다 한 번만 한다.
        let reports = self.library_reports(None, false)?;
        for project in self.projects_all() {
            let p = &project.name;
            let dir = self.project_dir(p)?;
            let catalog = self.db.library_catalog(p).map_err(internal)?;
            let protected_names =
                self.db.archive_list(p).ok().map(|rows| crate::library_catalog::referenced_names(&rows));
            let mut names = BTreeSet::new();
            for e in std::fs::read_dir(dir.join("threads")).map_err(io_error)? {
                let e = e.map_err(io_error)?;
                if let Some(name) = e.file_name().to_str().and_then(|s| s.strip_suffix(".files.json")) {
                    names.insert(name.to_string());
                }
            }
            let root = dir.join("library");
            if root.exists() {
                for e in std::fs::read_dir(&root).map_err(io_error)? {
                    names.insert(e.map_err(io_error)?.file_name().to_string_lossy().into_owned());
                }
            }
            let aliases = names.clone();
            for name in names {
                if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\']) {
                    continue;
                }
                let target = self.db.report_target(p, &name).map_err(internal)?;
                let source = target.as_ref().map_or(name.as_str(), |v| v.source.as_str());
                let parent = reports.iter().find(|r| &r.project == p && r.name == source && !r.archived);
                let row = catalog.rows.get(&name);
                // live 등록은 외부 원본이다. 불변 판은 실제 host 사본으로 계속 센다.
                if row.is_some_and(|r| r.live.is_some()) {
                    continue;
                }
                let mut size = disk_size(&root.join(&name)).map_err(io_error)?;
                if catalog.reserved.contains(&name) {
                    size = size
                        .saturating_add(disk_size(&dir.join("threads").join(format!("{name}.md"))).map_err(io_error)?);
                }
                let updated = row.map(|r| r.updated).or_else(|| parent.map(|r| r.updated_at_ms)).unwrap_or_else(|| {
                    let path =
                        if manifest_path(&dir, &name).exists() { manifest_path(&dir, &name) } else { root.join(&name) };
                    std::fs::metadata(path)
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map_or(0, |t| t.as_millis() as u64)
                });
                // 일하는 수집의 임시 폴더와 출처를 알 수 없는 thread는 보호한다. 숨은 사본도 용량에서 빼지 않는다.
                let protected = protected_names.as_ref().is_none_or(|names| {
                    names.contains(source)
                        || catalog
                            .versions
                            .get(source)
                            .is_some_and(|versions| versions.iter().any(|id| names.contains(id)))
                }) || name.starts_with('.')
                    || parent.is_some_and(|r| r.live || r.busy)
                    || (source.starts_with("t-")
                        && parent.is_none()
                        && self.load(p, source).map_or(true, |r| r.status != aaa_proto::ThreadStatus::Resolved));
                let family = if target.is_some()
                    && catalog.versions.get(source).and_then(|v| v.first()) == Some(&name)
                    && aliases.contains(source)
                    && !catalog.archived.contains(&name)
                {
                    source.to_string()
                } else {
                    name.clone()
                };
                copies.push(LibraryCopy {
                    family,
                    project: p.clone(),
                    archived: catalog.archived.contains(&name),
                    name,
                    size,
                    updated_at_ms: updated,
                    protected,
                });
            }
        }
        Ok(copies)
    }
}

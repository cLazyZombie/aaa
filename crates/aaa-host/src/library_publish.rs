//! host 전용 보고서 올리기·폴더 등록·일괄 이전·정리(§8.200).
//! herdr-projects의 library 사본과 임시 폴더 교체, 기존 회수 상한·숨은 이름·symlink 규칙을 따른다.
use std::path::Path;

use aaa_proto::{ErrorBody, ErrorCode, FileKind, FrontResponse, LibraryFile, LibraryReport, LibrarySkip};

use crate::db::LibraryReportRow;
use crate::library::{manifest_path, safe_rel};
use crate::{Host, err};

fn name_ok(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 100
        && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
        && !name.starts_with("t-")
        && !matches!(name, "con" | "prn" | "aux" | "nul")
        && !(name.len() == 4
            && (name.starts_with("com") || name.starts_with("lpt"))
            && matches!(name.as_bytes()[3], b'1'..=b'9'))
}

fn io_error(_: std::io::Error) -> ErrorBody {
    // 개인 경로를 API 오류·로그로 내보내지 않는다.
    err(ErrorCode::Unavailable, "보고서 파일을 읽거나 쓸 수 없다")
}

fn plain(path: &Path) -> Result<std::fs::Metadata, ErrorBody> {
    let m = std::fs::symlink_metadata(path).map_err(io_error)?;
    if m.file_type().is_symlink() {
        return Err(err(ErrorCode::BadRequest, "symlink는 가져오거나 등록하지 않는다"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // junction도 밖을 가리킬 수 있는 reparse point다.
        if m.file_attributes() & 0x400 != 0 {
            return Err(err(ErrorCode::BadRequest, "reparse point는 열지 않는다"));
        }
    }
    Ok(m)
}

/// 등록한 canonical root의 조상이 다른 곳을 가리키게 바뀌어도 닫힌 쪽이다.
fn root_ok(root: &Path) -> Result<(), ErrorBody> {
    if !plain(root)?.is_dir() || std::fs::canonicalize(root).map_err(io_error)? != root {
        return Err(err(ErrorCode::NotFound, "등록 폴더가 없거나 바뀌었다"));
    }
    Ok(())
}

/// 일반 파일만, 숨은 이름과 symlink는 빼고 상한을 넘으면 보고서 전체를 거부한다.
fn scan(root: &Path, limit: u64) -> Result<Vec<LibraryFile>, ErrorBody> {
    root_ok(root)?;
    let mut files = Vec::new();
    let mut dirs = vec![root.to_path_buf()];
    let mut total = 0u64;
    while let Some(dir) = dirs.pop() {
        // 큐에 넣은 뒤 바뀐 폴더도 확인한다. 파일 항목은 아래에서 안전하게 연 handle로 다시 확인한다.
        if std::fs::canonicalize(&dir).map_err(io_error)? != dir || !plain(&dir)?.is_dir() {
            return Err(err(ErrorCode::NotFound, "등록 폴더가 없거나 바뀌었다"));
        }
        for e in std::fs::read_dir(dir).map_err(io_error)? {
            let e = e.map_err(io_error)?;
            let p = e.path();
            let rel = p
                .strip_prefix(root)
                .unwrap()
                .components()
                .map(|c| c.as_os_str().to_str())
                .collect::<Option<Vec<_>>>()
                .map(|c| c.join("/"));
            let Some(rel) = rel.filter(|p| safe_rel(p)) else { continue };
            let m = match plain(&p) {
                Ok(m) => m,
                Err(e) if e.code == ErrorCode::BadRequest => continue,
                Err(e) => return Err(e),
            };
            if m.is_dir() {
                dirs.push(p);
            } else if m.is_file() {
                let entry = scanned_file(root, &rel)?;
                total = total.saturating_add(entry.size);
                if total > limit {
                    return Err(err(ErrorCode::TooLarge, "보고서가 library_copy_mb 상한을 넘는다"));
                }
                files.push(entry);
            }
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

/// 순회 중 발견한 파일은 여기서 보고서 목록 항목으로 만든다.
fn scanned_file(root: &Path, path: &str) -> Result<LibraryFile, ErrorBody> {
    let (_, size) = live_file(root, path)?;
    Ok(LibraryFile { path: path.into(), size, on_agent: false, mime: String::new(), kind: FileKind::Binary })
}

/// 매 요청마다 모든 성분·실제 경로를 확인한다. 파일 handle을 반환해 검사 뒤 이름 교체가 stream에 영향을 주지 않는다.
pub(crate) fn live_file(root: &Path, path: &str) -> Result<(std::fs::File, u64), ErrorBody> {
    root_ok(root)?;
    if !safe_rel(path) {
        return Err(err(ErrorCode::BadRequest, "library 상대 경로가 아니다"));
    }
    let mut p = root.to_path_buf();
    for part in path.split('/') {
        p.push(part);
        plain(&p)?;
    }
    if !std::fs::canonicalize(&p).map_err(io_error)?.starts_with(root) {
        return Err(err(ErrorCode::NotFound, "등록 폴더 밖은 열지 않는다"));
    }
    let f = open_regular(&p)?;
    #[cfg(windows)]
    constrain_opened_file(&f, root)?;
    let m = f.metadata().map_err(io_error)?;
    if !m.is_file() {
        return Err(err(ErrorCode::NotFound, "일반 파일이 아니다"));
    }
    Ok((f, m.len()))
}

/// Unix에서는 성분마다 openat(O_NOFOLLOW)로 연다. 검사와 열기 사이 symlink 교체도 따라가지 않는다.
#[cfg(unix)]
fn open_regular(path: &Path) -> Result<std::fs::File, ErrorBody> {
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    let parts = path
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(n) => Some(n),
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut file = std::fs::File::open("/").map_err(io_error)?;
    for (i, part) in parts.iter().enumerate() {
        let name = std::ffi::CString::new(part.as_bytes())
            .map_err(|_| err(ErrorCode::BadRequest, "파일 이름 형식이 아니다"))?;
        let flags = libc::O_RDONLY
            | libc::O_CLOEXEC
            | libc::O_NOFOLLOW
            | libc::O_NONBLOCK
            | if i + 1 < parts.len() { libc::O_DIRECTORY } else { 0 };
        // SAFETY: 유효한 parent fd와 NUL로 끝난 이름이다. 새 fd의 소유권을 즉시 File로 넘긴다.
        let fd = unsafe { libc::openat(file.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            return Err(io_error(std::io::Error::last_os_error()));
        }
        file = unsafe { std::fs::File::from_raw_fd(fd) };
    }
    if !file.metadata().map_err(io_error)?.is_file() {
        return Err(err(ErrorCode::NotFound, "일반 파일이 아니다"));
    }
    Ok(file)
}

#[cfg(not(unix))]
fn open_regular(path: &Path) -> Result<std::fs::File, ErrorBody> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x00200000); // FILE_FLAG_OPEN_REPARSE_POINT: 최종 symlink도 따라가지 않는다
    }
    let file = options.open(path).map_err(io_error)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if file.metadata().map_err(io_error)?.file_attributes() & 0x400 != 0 {
            return Err(err(ErrorCode::NotFound, "reparse point는 열지 않는다"));
        }
    }
    Ok(file)
}

/// Windows의 최종 reparse 검사만으로는 중간 junction 교체를 막지 못한다.
/// 문자열 경로를 다시 검사하지 않고 이미 연 handle의 최종 정규 경로를 등록 root와 대조한다.
#[cfg(windows)]
fn constrain_opened_file(file: &std::fs::File, root: &Path) -> Result<(), ErrorBody> {
    use std::os::windows::ffi::OsStringExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::GetFinalPathNameByHandleW;
    let mut buf = vec![0u16; 32768];
    // SAFETY: 살아 있는 File의 handle과 쓰기 가능한 UTF-16 버퍼를 넘긴다. flags=0은 정규화된 DOS 경로다.
    let n = unsafe { GetFinalPathNameByHandleW(file.as_raw_handle(), buf.as_mut_ptr(), buf.len() as u32, 0) } as usize;
    if n == 0 || n >= buf.len() {
        return Err(io_error(std::io::Error::last_os_error()));
    }
    let actual = std::path::PathBuf::from(std::ffi::OsString::from_wide(&buf[..n]));
    if !actual.starts_with(root) {
        return Err(err(ErrorCode::NotFound, "등록 폴더 밖은 열지 않는다"));
    }
    Ok(())
}

impl Host {
    fn library_limit(&self) -> u64 {
        self.cfg.projects.library_copy_mb.saturating_mul(1024 * 1024)
    }

    /// live는 그때의 파일 목록을 쓰므로 수정·추가·삭제가 즉시 반영된다.
    pub(crate) fn report_manifest(&self, dir: &Path, project: &str, name: &str) -> Result<Vec<LibraryFile>, ErrorBody> {
        if !safe_rel(name) || name.contains('/') {
            return Err(err(ErrorCode::BadRequest, "보고서 이름 형식이 아니다"));
        }
        if let Some(row) = self.db.library_report(project, name).map_err(super::orch::internal)?
            && let Some(root) = row.live
        {
            return scan(Path::new(&root), self.library_limit());
        }
        Ok(self.manifest(dir, name))
    }

    pub(crate) fn library_publish(
        &self,
        project: &str,
        name: &str,
        source: &str,
        live: bool,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        let _lock = self.library_ops.lock().unwrap();
        self.publish_report(project, name, Path::new(source), live, false, actor)?;
        Ok(FrontResponse::LibraryLink(self.library_link(project, name, None, actor)?))
    }

    fn publish_report(
        &self,
        project: &str,
        name: &str,
        source: &Path,
        live: bool,
        no_replace: bool,
        actor: &str,
    ) -> Result<(), ErrorBody> {
        let dir = self.project_dir(project)?;
        if !name_ok(name) {
            return Err(err(ErrorCode::BadRequest, "이름은 소문자·숫자·-·_ 1~100자이며 t-로 시작할 수 없다"));
        }
        let manifest = manifest_path(&dir, name);
        if no_replace
            && (manifest.exists()
                || dir.join("library").join(name).exists()
                || self.db.library_report(project, name).map_err(super::orch::internal)?.is_some())
        {
            return Err(err(ErrorCode::BadRequest, "같은 이름의 보고서가 있다"));
        }
        let m = plain(source)?;
        if live && !m.is_dir() {
            return Err(err(ErrorCode::BadRequest, "실시간 등록은 폴더만 된다"));
        }
        let source = std::fs::canonicalize(source).map_err(io_error)?;
        let files = if m.is_dir() {
            scan(&source, self.library_limit())?
        } else if m.is_file() {
            let name = source
                .file_name()
                .and_then(|n| n.to_str())
                .filter(|n| safe_rel(n))
                .ok_or_else(|| err(ErrorCode::BadRequest, "숨은 이름이나 안전하지 않은 파일 이름이다"))?;
            if m.len() > self.library_limit() {
                return Err(err(ErrorCode::TooLarge, "보고서가 library_copy_mb 상한을 넘는다"));
            }
            vec![LibraryFile {
                path: name.into(),
                size: m.len(),
                on_agent: false,
                mime: String::new(),
                kind: FileKind::Binary,
            }]
        } else {
            return Err(err(ErrorCode::BadRequest, "일반 파일이나 폴더만 가져온다"));
        };
        let library = dir.join("library");
        std::fs::create_dir_all(&library).map_err(io_error)?;
        let dest = library.join(name);
        // Windows에서도 열린 파일 위 rename이나 비어 있지 않은 폴더 위 rename을 하지 않는다.
        let staging = library.join(format!(".publish-{}", aaa_common::util::new_id()));
        std::fs::create_dir(&staging).map_err(io_error)?;
        let result = (|| {
            if !live {
                for entry in &files {
                    let (mut input, size) = if m.is_dir() {
                        live_file(&source, &entry.path)?
                    } else {
                        let f = open_regular(&source)?;
                        let n = f.metadata().map_err(io_error)?.len();
                        (f, n)
                    };
                    if size != entry.size {
                        return Err(err(ErrorCode::BadRequest, "복사 중 원본 크기가 바뀌었다"));
                    }
                    let out = staging.join(&entry.path);
                    std::fs::create_dir_all(out.parent().unwrap()).map_err(io_error)?;
                    let mut output = std::fs::File::create(out).map_err(io_error)?;
                    use std::io::Read;
                    let copied = std::io::copy(&mut (&mut input).take(entry.size.saturating_add(1)), &mut output)
                        .map_err(io_error)?;
                    if copied != entry.size {
                        return Err(err(ErrorCode::BadRequest, "복사 중 원본 크기가 바뀌었다"));
                    }
                }
            }
            let backup = library.join(format!(".previous-{}", aaa_common::util::new_id()));
            let existed = dest.exists();
            if existed {
                std::fs::rename(&dest, &backup).map_err(io_error)?;
            }
            let old_manifest = std::fs::read(&manifest).ok();
            let commit = (|| {
                std::fs::rename(&staging, &dest).map_err(io_error)?;
                crate::orch::write_atomic(&manifest, &serde_json::to_vec(&files).unwrap()).map_err(io_error)?;
                self.db
                    .library_report_put(&LibraryReportRow {
                        project: project.into(),
                        name: name.into(),
                        live: live.then(|| source.to_string_lossy().into_owned()),
                        updated: aaa_common::util::now_ms(),
                    })
                    .map_err(super::orch::internal)
            })();
            if let Err(e) = commit {
                let _ = std::fs::remove_dir_all(&dest);
                if existed {
                    let _ = std::fs::rename(&backup, &dest);
                }
                if let Some(b) = old_manifest {
                    let _ = crate::orch::write_atomic(&manifest, &b);
                } else {
                    let _ = std::fs::remove_file(&manifest);
                }
                return Err(e);
            }
            if existed {
                std::fs::remove_dir_all(&backup).map_err(io_error)?;
            }
            self.db.audit(
                actor,
                if live { "library_register" } else { "library_publish" },
                &format!("{project}/{name}"),
            );
            Ok(())
        })();
        if staging.exists() {
            let _ = std::fs::remove_dir_all(&staging);
        }
        result
    }

    pub(crate) fn library_import(
        &self,
        project: &str,
        source: &str,
        prefix: &str,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        let _lock = self.library_ops.lock().unwrap();
        self.project_dir(project)?;
        if prefix.len() > 90
            || !prefix.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
        {
            return Err(err(ErrorCode::BadRequest, "접두사는 소문자·숫자·-·_ 90자 이내다"));
        }
        let root = Path::new(source);
        if !plain(root)?.is_dir() {
            return Err(err(ErrorCode::BadRequest, "가져올 폴더가 아니다"));
        }
        let mut entries =
            std::fs::read_dir(root).map_err(io_error)?.collect::<Result<Vec<_>, _>>().map_err(io_error)?;
        entries.sort_by_key(|e| e.file_name());
        let mut reports = Vec::new();
        let mut skipped = Vec::new();
        for e in entries {
            let name = e.file_name().to_string_lossy().into_owned();
            let result = (|| {
                if !plain(&e.path())?.is_dir() {
                    return Err(err(ErrorCode::BadRequest, "하위 폴더가 아니다(낱 파일은 publish한다)"));
                }
                // 숨은 원본 이름은 접두사를 붙여도 가져오지 않는다.
                if name.starts_with('.') {
                    return Err(err(ErrorCode::BadRequest, "숨은 이름은 가져오지 않는다"));
                }
                let target = format!("{prefix}{name}");
                self.publish_report(project, &target, &e.path(), false, true, actor)?;
                self.library_link(project, &target, None, actor)?;
                self.report_view(project, &target, true)
            })();
            match result {
                Ok(r) => reports.push(r),
                Err(e) => skipped.push(LibrarySkip { name, reason: e.message }),
            }
        }
        Ok(FrontResponse::LibraryImported { reports, skipped })
    }

    fn report_view(&self, project: &str, name: &str, reveal: bool) -> Result<LibraryReport, ErrorBody> {
        let dir = self.project_dir(project)?;
        let row = self.db.library_report(project, name).map_err(super::orch::internal)?;
        let updated = row.as_ref().map(|r| r.updated).unwrap_or_else(|| {
            std::fs::metadata(manifest_path(&dir, name))
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|t| t.as_millis() as u64)
                .unwrap_or(0)
        });
        let (size, problem) = match self.report_manifest(&dir, project, name) {
            Ok(files) => (files.iter().filter(|f| !f.on_agent && safe_rel(&f.path)).map(|f| f.size).sum(), None),
            Err(e) => (0, Some(e.message)),
        };
        Ok(LibraryReport {
            project: project.into(),
            name: name.into(),
            size,
            updated_at_ms: updated,
            live: row.is_some_and(|r| r.live.is_some()),
            problem,
            link: reveal.then(|| self.latest_library_link(project, name)).flatten(),
        })
    }

    pub(crate) fn library_reports(&self, project: Option<&str>, reveal: bool) -> Result<Vec<LibraryReport>, ErrorBody> {
        let projects = if let Some(p) = project {
            self.project_dir(p)?;
            vec![p.to_string()]
        } else {
            self.projects_all().into_iter().map(|p| p.name).collect()
        };
        let mut reports = Vec::new();
        for p in projects {
            let dir = self.project_dir(&p)?;
            for e in std::fs::read_dir(dir.join("threads")).map_err(io_error)? {
                let e = e.map_err(io_error)?;
                if let Some(name) = e.file_name().to_str().and_then(|s| s.strip_suffix(".files.json"))
                    && safe_rel(name)
                {
                    reports.push(self.report_view(&p, name, reveal)?);
                }
            }
        }
        reports.sort_by(|a, b| {
            b.updated_at_ms.cmp(&a.updated_at_ms).then_with(|| (&a.project, &a.name).cmp(&(&b.project, &b.name)))
        });
        Ok(reports)
    }
}

#[cfg(all(test, unix))]
mod scan_tests {
    use super::*;

    /// 큐에 있던 폴더가 밖을 가리키게 바뀐 뒤, 최종 일반 파일만 검사하는 순회 단계를 재현한다.
    #[test]
    fn scanned_file_cannot_expose_a_replaced_directory() {
        let state = std::env::temp_dir().join(format!("aaa-scan-scope-{}", aaa_common::util::new_id()));
        std::fs::create_dir_all(state.join("root/sub")).unwrap();
        std::fs::create_dir(state.join("outside")).unwrap();
        std::fs::write(state.join("outside/private.txt"), b"outside metadata").unwrap();
        let root = std::fs::canonicalize(state.join("root")).unwrap();
        std::fs::remove_dir(root.join("sub")).unwrap();
        std::os::unix::fs::symlink(state.join("outside"), root.join("sub")).unwrap();
        let result = scanned_file(&root, "sub/private.txt");
        std::fs::remove_dir_all(&state).unwrap();
        assert!(result.is_err(), "순회에서 얻은 외부 이름과 크기를 목록에 싣지 않는다");
    }
}

#[cfg(all(test, windows))]
mod opened_file_tests {
    use super::*;

    /// junction 교체로 밖의 일반 파일 handle이 열렸더라도 최종 handle 판정은 그 파일을 거부한다.
    #[test]
    fn opened_handle_must_stay_within_the_registered_root() {
        let state = std::env::temp_dir().join(format!("aaa-open-scope-{}", aaa_common::util::new_id()));
        std::fs::create_dir_all(state.join("root")).unwrap();
        std::fs::write(state.join("root/index.html"), b"inside").unwrap();
        std::fs::write(state.join("outside.txt"), b"outside").unwrap();
        let root = std::fs::canonicalize(state.join("root")).unwrap();
        let inside = std::fs::File::open(root.join("index.html")).unwrap();
        let outside = std::fs::File::open(state.join("outside.txt")).unwrap();
        let allowed = constrain_opened_file(&inside, &root);
        let denied = constrain_opened_file(&outside, &root);
        drop((inside, outside));
        std::fs::remove_dir_all(state).unwrap();
        assert!(allowed.is_ok());
        assert!(denied.is_err(), "밖에서 열린 일반 파일 handle을 등록 폴더의 파일로 내보내지 않는다");
    }
}

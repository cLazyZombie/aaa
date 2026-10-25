//! 웹 「보고서 올리기」(관리 GUI W5b, plan §8.355). 브라우저가 파일을 하나씩 host 상태 폴더 `library-uploads/<올리기>/<경로>`에
//! 올리고(`POST /api/v1/projects/<p>/library-uploads/<올리기>?path=…`), `library_upload_publish`가 그 폴더를 보고서 하나로 등록한 뒤
//! 지운다. 등록은 CLI `aaa library publish`와 같은 `library_publish`다(이름 규칙·사용자 보고서의 index.html·library 상한·사용자 알림).
//! 파일당 16MiB(할 일 첨부와 같다), 한 올리기에 합계 64MiB·1000개까지. 끝내지 않은 올리기는 하루 뒤 다음 올리기 때 지운다.
use std::path::{Path, PathBuf};

use aaa_proto::{DeviceRole, ErrorBody, ErrorCode, FrontResponse};

use crate::{Host, err};

pub(crate) const FILE_MAX: u64 = 16 << 20;
pub(crate) const TOTAL_MAX: u64 = 64 << 20;
const FILES_MAX: usize = 1000;
const STALE_MS: u64 = 24 * 3600 * 1000;
/// 올리기 쓰기·등록은 하나씩(합계 상한을 셀 때 겹치지 않게, 등록이 읽는 동안 다른 쓰기의 치우기가 지우지 않게)
// 마지막 올리기 하나만 캐시한다. 다른 올리기·host·재시작에서는 디스크를 다시 읽는다.
// 이 폴더의 writer는 아래 잠금을 공유한다. 게시 때는 캐시 대신 실제 파일 수를 확인한다.
static PUTTING: std::sync::Mutex<Option<UploadUsage>> = std::sync::Mutex::new(None);
struct UploadUsage {
    dir: PathBuf,
    files: usize,
    total: u64,
}

/// 웹이 만든 올리기 이름: 소문자 hex 32자
pub(crate) fn valid_upload(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// 폴더 아래 일반 파일 수와 합계 크기(symlink는 따라가지 않는다: 이 폴더에는 host만 쓴다)
fn usage(dir: &std::path::Path) -> (usize, u64) {
    let mut stack = vec![dir.to_path_buf()];
    let (mut n, mut size) = (0, 0);
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다"))
        else {
            continue;
        };
        for e in rd.flatten() {
            let Ok(m) = e.path().symlink_metadata().inspect_err(|error| {
                aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다")
            }) else {
                continue;
            };
            if m.is_dir() {
                stack.push(e.path());
            } else if m.is_file() {
                n += 1;
                size += m.len();
            }
        }
    }
    (n, size)
}

/// 올리기마다 마지막으로 쓴 때의 표지(올리기 폴더 옆 `<올리기>.put`, 내용은 그 시각 ms). 폴더 시각은 믿지 않는다: 하위 폴더에
/// 쓰면 맨 위 폴더 시각은 그대로이고, Windows는 폴더를 파일로 열어 시각을 바꾸지 못한다(Access denied)
fn mark(root: &Path, upload: &str) -> PathBuf {
    root.join(format!("{upload}.put"))
}

/// 그 올리기를 마지막으로 쓴 때(ms): 표지의 내용, 표지가 없으면(표지 전에 만든 올리기) 폴더 시각
fn last_put_ms(root: &Path, upload: &str) -> Option<u64> {
    if let Some(ms) = std::fs::read_to_string(mark(root, upload))
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
        .ok()
        .and_then(|t| {
            t.trim()
                .parse()
                .inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                })
                .ok()
        })
    {
        return Some(ms);
    }
    std::fs::metadata(root.join(upload))
        .and_then(|m| m.modified())
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
        .ok()
        .and_then(|t| {
            t.duration_since(std::time::UNIX_EPOCH)
                .inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                })
                .ok()
        })
        .map(|t| t.as_millis() as u64)
}

impl Host {
    fn uploads_root(&self) -> PathBuf {
        self.state.join("library-uploads")
    }

    /// 하루 넘게 손대지 않은 올리기를 지운다(등록하지 않고 떠난 화면). 지금 쓰는 올리기는 두고, 쓸 때마다 그 표지를 새로 한다
    fn sweep_uploads(&self, keep: &str, now_ms: u64) {
        let root = self.uploads_root();
        let Ok(rd) = std::fs::read_dir(&root)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다"))
        else {
            return;
        };
        for e in rd.flatten() {
            let Some(name) = e.file_name().to_str().map(str::to_string) else { continue };
            let id = name.strip_suffix(".put").unwrap_or(&name);
            if !valid_upload(id) || id == keep {
                continue;
            }
            // 폴더 없이 남은 표지도 같은 시각으로 치운다
            if last_put_ms(&root, id).is_some_and(|ms| ms.saturating_add(STALE_MS) < now_ms) {
                aaa_common::diagnostic::ignored(
                    std::fs::remove_dir_all(root.join(id)),
                    "정리 실패가 본 작업의 결과를 덮지 않게 한다",
                );
                aaa_common::diagnostic::ignored(
                    std::fs::remove_file(mark(&root, id)),
                    "정리 실패가 본 작업의 결과를 덮지 않게 한다",
                );
            }
        }
    }

    /// 파일 하나를 올리기 폴더에 쓴다. 돌려주는 값은 그 올리기의 (파일 수, 합계)
    pub(crate) fn library_upload_put(
        &self,
        role: DeviceRole,
        project: &str,
        upload: &str,
        path: &str,
        bytes: &[u8],
    ) -> Result<(usize, u64), ErrorBody> {
        let started = std::time::Instant::now();
        if !role.can_control() {
            return Err(err(ErrorCode::Forbidden, "보기 권한으로는 보고서를 올릴 수 없다"));
        }
        if bytes.len() as u64 > FILE_MAX {
            return Err(err(ErrorCode::TooLarge, "파일 하나는 16MiB까지 올린다"));
        }
        self.project_dir(project)?;
        if !valid_upload(upload) {
            return Err(err(ErrorCode::BadRequest, "올리기 이름이 아니다"));
        }
        if !crate::library::safe_rel(path) {
            return Err(err(ErrorCode::BadRequest, format!("숨은 이름이나 안전하지 않은 경로다: {path}")));
        }
        let validated_us = started.elapsed().as_micros() as u64;
        let waiting = std::time::Instant::now();
        let mut one = PUTTING.lock().unwrap();
        let lock_us = waiting.elapsed().as_micros() as u64;
        let scanning = std::time::Instant::now();
        self.sweep_uploads(upload, aaa_common::util::now_ms());
        let root = self.uploads_root();
        let dir = root.join(upload);
        let target = dir.join(path);
        let cached = one.as_ref().filter(|u| u.dir == dir && dir.is_dir());
        let cache_hit = cached.is_some();
        let (files, total) = cached.map_or_else(|| usage(&dir), |u| (u.files, u.total));
        let replaced = target
            .symlink_metadata()
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()
            .filter(|m| m.is_file())
            .map(|m| m.len());
        if files >= FILES_MAX && replaced.is_none() {
            return Err(err(ErrorCode::TooLarge, format!("한 보고서에 파일은 {FILES_MAX}개까지 올린다")));
        }
        let total = total - replaced.unwrap_or(0) + bytes.len() as u64;
        if total > TOTAL_MAX {
            return Err(err(ErrorCode::TooLarge, "한 보고서는 합계 64MiB까지 올린다"));
        }
        let scan_us = scanning.elapsed().as_micros() as u64;
        let writing = std::time::Instant::now();
        // 디렉터리 생성·rename·표지 쓰기 중 어느 단계가 실패해도 다음 요청은 다시 스캔한다.
        *one = None;
        let parent = target.parent().ok_or_else(|| err(ErrorCode::BadRequest, "경로가 없다"))?;
        for d in [&root, &dir] {
            aaa_common::util::ensure_private_dir(d).map_err(crate::orch::internal)?;
        }
        std::fs::create_dir_all(parent).map_err(crate::orch::internal)?;
        let temp = parent.join(format!(".{}.part", aaa_common::util::new_id()));
        std::fs::write(&temp, bytes).and_then(|_| std::fs::rename(&temp, &target)).map_err(|e| {
            aaa_common::diagnostic::ignored(std::fs::remove_file(&temp), "정리 실패가 본 작업의 결과를 덮지 않게 한다");
            crate::orch::internal(e)
        })?;
        // 치우기가 보는 시각을 지금으로. 못 쓰면 이 올리기가 하루 뒤 치워질 수 있어 실패로 알린다
        std::fs::write(mark(&root, upload), aaa_common::util::now_ms().to_string()).map_err(crate::orch::internal)?;
        let files = files + usize::from(replaced.is_none());
        *one = Some(UploadUsage { dir, files, total });
        tracing::debug!(target: "aaa_upload", validated_us, lock_us, scan_us, cache_hit,
            write_us = writing.elapsed().as_micros() as u64,
            elapsed_us = started.elapsed().as_micros() as u64,
            bytes = bytes.len(), files,
            "보고서 파일 올리기");
        Ok((files, total))
    }

    /// 올린 파일 묶음을 보고서 하나로 등록하고 올리기 폴더를 지운다. 실패하면 폴더를 두어 고쳐 다시 등록할 수 있다
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn library_upload_publish(
        &self,
        project: &str,
        upload: &str,
        name: &str,
        files: u32,
        to_user: bool,
        title: Option<&str>,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        if !valid_upload(upload) {
            return Err(err(ErrorCode::BadRequest, "올리기 이름이 아니다"));
        }
        if files == 0 {
            return Err(err(ErrorCode::BadRequest, "올린 파일이 없다"));
        }
        let mut one = PUTTING.lock().unwrap();
        *one = None;
        let dir = self.uploads_root().join(upload);
        let have = usage(&dir).0;
        // 하나도 없어도(통째로 치워짐) 같은 답: 웹은 새 자리에 처음부터 올린다
        if have != files as usize {
            return Err(err(
                ErrorCode::Conflict,
                format!(
                    "올린 파일이 {files}개 중 {have}개다(오래돼 치워졌거나 대소문자만 다른 이름이 겹쳤다): 처음부터 다시 올린다"
                ),
            ));
        }
        let source = dir.to_str().ok_or_else(|| err(ErrorCode::BadRequest, "경로가 글이 아니다"))?;
        let out = self.library_publish(project, name, source, false, to_user.then_some(title), None, actor)?;
        aaa_common::diagnostic::ignored(std::fs::remove_dir_all(&dir), "정리 실패가 본 작업의 결과를 덮지 않게 한다");
        aaa_common::diagnostic::ignored(
            std::fs::remove_file(mark(&self.uploads_root(), upload)),
            "정리 실패가 본 작업의 결과를 덮지 않게 한다",
        );
        self.db.audit(actor, "library_upload", &format!("{project}/{name}"));
        Ok(out)
    }
}

#[cfg(test)]
mod tests;

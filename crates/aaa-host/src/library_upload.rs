//! 웹 「보고서 올리기」(관리 GUI W5b, plan §8.352). 브라우저가 파일을 하나씩 host 상태 폴더 `library-uploads/<올리기>/<경로>`에
//! 올리고(`POST /api/v1/projects/<p>/library-uploads/<올리기>?path=…`), `library_upload_publish`가 그 폴더를 보고서 하나로 등록한 뒤
//! 지운다. 등록은 CLI `aaa library publish`와 같은 `library_publish`다(이름 규칙·사용자 보고서의 index.html·library 상한·사용자 알림).
//! 파일당 16MiB(할 일 첨부와 같다), 한 올리기에 합계 64MiB·1000개까지. 끝내지 않은 올리기는 하루 뒤 다음 올리기 때 지운다.
use std::path::PathBuf;

use aaa_proto::{DeviceRole, ErrorBody, ErrorCode, FrontResponse};

use crate::{Host, err};

pub(crate) const FILE_MAX: u64 = 16 << 20;
pub(crate) const TOTAL_MAX: u64 = 64 << 20;
const FILES_MAX: usize = 1000;
const STALE_MS: u64 = 24 * 3600 * 1000;
/// 올리기 쓰기·등록은 하나씩(합계 상한을 셀 때 겹치지 않게, 등록이 읽는 동안 다른 쓰기의 치우기가 지우지 않게)
static PUTTING: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 웹이 만든 올리기 이름: 소문자 hex 32자
pub(crate) fn valid_upload(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// 폴더 아래 일반 파일 수와 합계 크기(symlink는 따라가지 않는다: 이 폴더에는 host만 쓴다)
fn usage(dir: &std::path::Path) -> (usize, u64) {
    let mut stack = vec![dir.to_path_buf()];
    let (mut n, mut size) = (0, 0);
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let Ok(m) = e.path().symlink_metadata() else { continue };
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

impl Host {
    fn uploads_root(&self) -> PathBuf {
        self.state.join("library-uploads")
    }

    /// 하루 넘게 손대지 않은 올리기를 지운다(등록하지 않고 떠난 화면). 지금 쓰는 올리기는 두고, 쓸 때마다 그 폴더 시각을 새로 한다
    fn sweep_uploads(&self, keep: &str, now_ms: u64) {
        let Ok(rd) = std::fs::read_dir(self.uploads_root()) else { return };
        for e in rd.flatten() {
            let old = e
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .is_some_and(|t| (t.as_millis() as u64).saturating_add(STALE_MS) < now_ms);
            if old && e.file_name().to_str().is_some_and(|n| valid_upload(n) && n != keep) {
                let _ = std::fs::remove_dir_all(e.path());
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
        if !role.can_control() {
            return Err(err(ErrorCode::Forbidden, "보기 권한으로는 보고서를 올릴 수 없다"));
        }
        self.project_dir(project)?;
        if !valid_upload(upload) {
            return Err(err(ErrorCode::BadRequest, "올리기 이름이 아니다"));
        }
        if !crate::library::safe_rel(path) {
            return Err(err(ErrorCode::BadRequest, format!("숨은 이름이나 안전하지 않은 경로다: {path}")));
        }
        let _one = PUTTING.lock().unwrap();
        self.sweep_uploads(upload, aaa_common::util::now_ms());
        let root = self.uploads_root();
        let dir = root.join(upload);
        let target = dir.join(path);
        let (files, total) = usage(&dir);
        let replaced = target.symlink_metadata().ok().filter(|m| m.is_file()).map_or(0, |m| m.len());
        if files >= FILES_MAX && replaced == 0 {
            return Err(err(ErrorCode::TooLarge, format!("한 보고서에 파일은 {FILES_MAX}개까지 올린다")));
        }
        let total = total - replaced + bytes.len() as u64;
        if total > TOTAL_MAX {
            return Err(err(ErrorCode::TooLarge, "한 보고서는 합계 64MiB까지 올린다"));
        }
        let parent = target.parent().ok_or_else(|| err(ErrorCode::BadRequest, "경로가 없다"))?;
        for d in [&root, &dir] {
            aaa_common::util::ensure_private_dir(d).map_err(crate::orch::internal)?;
        }
        std::fs::create_dir_all(parent).map_err(crate::orch::internal)?;
        let temp = parent.join(format!(".{}.part", aaa_common::util::new_id()));
        std::fs::write(&temp, bytes).and_then(|_| std::fs::rename(&temp, &target)).map_err(|e| {
            let _ = std::fs::remove_file(&temp);
            crate::orch::internal(e)
        })?;
        // 하위 폴더에 쓰면 맨 위 폴더 시각은 그대로다: 치우기가 보는 시각을 지금으로
        let _ = std::fs::File::open(&dir).and_then(|f| f.set_modified(std::time::SystemTime::now()));
        Ok((files + usize::from(replaced == 0), total))
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
        let _one = PUTTING.lock().unwrap();
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
        let _ = std::fs::remove_dir_all(&dir);
        self.db.audit(actor, "library_upload", &format!("{project}/{name}"));
        Ok(out)
    }
}

#[cfg(test)]
mod tests;

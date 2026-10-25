use super::Timing;
use std::{
    collections::BTreeSet,
    fs::{File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::Instant,
};

// 파일 핸들과 동기화 작업 모두 여덟 개로 제한한다. 디스크 flush를 파일마다 직렬로 기다리지 않는다.
pub(super) const WIDTH: usize = 8;

struct Staged {
    name: String,
    original: Option<String>,
    dest: PathBuf,
    temp: PathBuf,
    file: Option<File>,
}

impl Drop for Staged {
    fn drop(&mut self) {
        drop(self.file.take());
        if let Err(error) = std::fs::remove_file(&self.temp)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(path = %self.temp.display(), %error, "자동화 변환 임시 파일 정리 실패");
        }
    }
}

pub(super) struct Batch<'a> {
    root: &'a Path,
    backup: bool,
    staged: Vec<Staged>,
    directories: BTreeSet<PathBuf>,
}

impl<'a> Batch<'a> {
    pub(super) fn new(root: &'a Path, backup: bool) -> Self {
        Self { root, backup, staged: Vec::new(), directories: BTreeSet::new() }
    }

    pub(super) fn touch(&mut self, path: &Path) {
        let mut at = Some(path);
        while let Some(dir) = at.filter(|dir| dir.starts_with(self.root)) {
            self.directories.insert(dir.to_owned());
            if dir == self.root {
                break;
            }
            at = dir.parent();
        }
    }

    pub(super) fn stage(
        &mut self,
        name: &str,
        text: &str,
        original: Option<&str>,
        timing: &mut Timing,
    ) -> Result<bool, String> {
        let dest = self.root.join(name);
        let parent = dest.parent().unwrap();
        if !self.directories.contains(parent) {
            timing
                .measure("create_directory", name, || std::fs::create_dir_all(parent))
                .map_err(|e| format!("{name}: 폴더 준비: {e}"))?;
        }
        self.touch(parent);
        let temp = parent.join(format!(".aaa-convert-{}.tmp", aaa_common::util::new_id()));
        let file = timing
            .measure("create_file", name, || OpenOptions::new().create_new(true).write(true).open(&temp))
            .map_err(|e| format!("{name}: 임시 파일 준비: {e}"))?;
        let mut staged =
            Staged { name: name.into(), original: original.map(str::to_owned), dest, temp, file: Some(file) };
        let step = if self.backup { "backup_write" } else { "output_write" };
        timing
            .measure(step, name, || staged.file.as_mut().unwrap().write_all(text.as_bytes()))
            .map_err(|e| format!("{name}: 파일 쓰기: {e}"))?;
        self.staged.push(staged);
        Ok(self.staged.len() == WIDTH)
    }

    pub(super) fn flush(
        &mut self,
        timing: &mut Timing,
        published: &mut impl FnMut(&str) -> Result<(), String>,
    ) -> Result<(), String> {
        // 모든 data flush가 성공한 묶음만 공개한다. 실패해도 원본과 DB journal은 남는다.
        let results = std::thread::scope(|scope| {
            let tasks: Vec<_> = self
                .staged
                .iter()
                .map(|s| {
                    std::thread::Builder::new().spawn_scoped(scope, move || {
                        let started = Instant::now();
                        let result = s.file.as_ref().unwrap().sync_all();
                        (started.elapsed(), result)
                    })
                })
                .collect();
            tasks
                .into_iter()
                .map(|task| {
                    task.map_err(|e| format!("동기화 worker 시작 실패: {e}"))
                        .and_then(|task| task.join().map_err(|_| "동기화 worker 비정상 종료".to_string()))
                })
                .collect::<Vec<_>>()
        });
        let sync_step = if self.backup { "backup_file_sync" } else { "output_file_sync" };
        let mut failure = None;
        for (staged, result) in self.staged.iter().zip(results) {
            match result {
                Ok((elapsed, result)) => {
                    timing.duration(sync_step, &staged.name, elapsed);
                    if let Err(e) = result {
                        failure.get_or_insert_with(|| format!("{}: 파일 동기화: {e}", staged.name));
                    }
                }
                Err(error) => {
                    failure.get_or_insert_with(|| format!("{}: {error}", staged.name));
                }
            }
        }
        if let Some(error) = failure {
            return Err(error);
        }
        for mut staged in self.staged.drain(..) {
            // Windows에서도 열린 handle 없이 최종 이름으로 공개하고 임시 파일을 정리한다.
            drop(staged.file.take());
            if !self.backup {
                match staged.dest.symlink_metadata() {
                    Ok(_) => {
                        let current = super::project_text(self.root, &staged.name)?;
                        if staged.original.as_deref() != Some(current.as_str()) {
                            return Err(format!("{}: 공개 전 다른 편집 발견", staged.name));
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(format!("{}: 공개 대상 확인: {e}", staged.name)),
                }
            }
            let step = if self.backup { "backup_publish" } else { "output_publish" };
            timing
                .measure(step, &staged.name, || {
                    if self.backup {
                        std::fs::hard_link(&staged.temp, &staged.dest)
                    } else {
                        std::fs::rename(&staged.temp, &staged.dest)
                    }
                })
                .map_err(|e| format!("{}: 원자적 공개: {e}", staged.name))?;
            published(&staged.name)?;
        }
        Ok(())
    }

    pub(super) fn sync_directories(&self, timing: &mut Timing, step: &'static str) -> Result<(), String> {
        for dir in self.directories.iter().rev() {
            #[cfg(unix)]
            timing
                .measure(step, &dir.strip_prefix(self.root).unwrap().to_string_lossy(), || File::open(dir)?.sync_all())
                .map_err(|e| format!("{}: 폴더 동기화: {e}", dir.display()))?;
            #[cfg(not(unix))]
            let _ = (dir, &timing, step);
        }
        Ok(())
    }
}

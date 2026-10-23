//! CLI 파일 엔진과 host 발급자가 같은 실제 파일/ACL 경계를 쓴다.
pub(super) use aaa_common::private_file::{
    directory, new_file, parents, read, sync_file, sync_parent, verify_directory, write,
};

#[cfg(any(windows, test))]
pub(super) use aaa_common::private_file::verify;

/// install의 짧은 발급 scope와 내구 owner가 같은 잠금 수명을 사용한다.
/// Unix는 fork/dup 사본이 남아도 owner 종료 때 unlock한다.
/// Windows는 기존 share reservation을 File handle close로 해제한다.
pub(super) struct Lock(std::fs::File);
impl std::ops::Deref for Lock {
    type Target = std::fs::File;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl Drop for Lock {
    fn drop(&mut self) {
        #[cfg(unix)]
        aaa_common::diagnostic::ignored(self.0.unlock(), "보조 작업 실패 뒤 기존 처리를 계속한다");
    }
}
pub(super) fn lock(path: &std::path::Path) -> std::io::Result<Lock> {
    aaa_common::private_file::lock(path).map(Lock)
}

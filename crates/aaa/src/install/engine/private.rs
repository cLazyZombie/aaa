//! CLI 파일 엔진과 host 발급자가 같은 실제 파일/ACL 경계를 쓴다.
pub(super) use aaa_common::private_file::{
    directory, lock, new_file, parents, read, sync_file, sync_parent, verify_directory, write,
};

#[cfg(any(windows, test))]
pub(super) use aaa_common::private_file::verify;

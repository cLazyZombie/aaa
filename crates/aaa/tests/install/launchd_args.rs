//! 실제 등록 owner와 로컬 CLI 파서 owner가 공유하는 승인 인자.
pub(crate) fn authorize(mut args: Vec<String>, approval: &std::path::Path) -> Vec<String> {
    args.extend([
        "--prompt".into(),
        "plain".into(),
        "authorize".into(),
        "--out".into(),
        approval.to_string_lossy().into_owned(),
    ]);
    args
}

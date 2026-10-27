use aaa_common::{child_process::TokioCommandExt as _, github::GH_TIMEOUT};
use std::{path::Path, process::Stdio};

pub(crate) async fn origin(dir: &Path) -> String {
    let out = tokio::time::timeout(
        GH_TIMEOUT,
        tokio::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["remote", "get-url", "origin"])
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output_guarded(),
    )
    .await;
    match out {
        Ok(Ok(o)) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => String::new(),
    }
}

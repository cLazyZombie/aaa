//! host가 부르는 이 머신 설정 고정 작업(관리 GUI W5c, plan §8.357): 작업 root 보기·점검·전환(work_root.manage.v1)과 Codex
//! 전역 hook 상태·설치·제거(hook.manage.v1). `aaa work-root`·`aaa integration install|uninstall codex`와 같은 코드다.
//! 이 머신의 설정 폴더(`AAA_CONFIG_DIR`)와 agent 상태 폴더를 쓴다. hook 명령은 이 agent 실행 파일이다(요청이 경로를 주지 않는다).
use std::path::Path;

use aaa_proto::{AgentResponse, ErrorBody, ErrorCode, HookView, WorkRootCheckView, WorkRootOp, WorkRootView};

use crate::work_root::admin;

fn text(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn view(s: &admin::Shown) -> WorkRootView {
    WorkRootView {
        path: text(&s.path),
        mount: s.mount.as_deref().map(text),
        last_start: s.last_start.as_ref().map(|(p, _)| text(p)),
        last_start_mount: s.last_start.as_ref().and_then(|(_, m)| m.as_deref().map(text)),
        previous_roots: s.previous.iter().map(|r| text(&r.path)).collect(),
        restart_required: s.restart_required,
        changed: None,
        imported: None,
    }
}

fn bad(e: anyhow::Error) -> ErrorBody {
    ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}"))
}

/// 파일 I/O(끊긴 볼륨의 canonicalize·검사 파일)는 blocking thread에서 한다: link를 돌리는 runtime을 붙잡지 않는다
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, ErrorBody> + Send + 'static,
) -> Result<T, ErrorBody> {
    tokio::task::spawn_blocking(work)
        .await
        .unwrap_or_else(|e| Err(ErrorBody::new(ErrorCode::Internal, format!("설정 작업이 끝나지 못했다: {e}"))))
}

pub(crate) async fn work_root(state: &Path, action: WorkRootOp) -> Result<AgentResponse, ErrorBody> {
    let dir = aaa_config::paths::config_dir();
    let state_owned = state.to_path_buf();
    match action {
        WorkRootOp::Show => {
            blocking(move || admin::show(&dir, &state_owned).map(|s| AgentResponse::WorkRoot(view(&s))).map_err(bad))
                .await
        }
        WorkRootOp::Check { path, mount } => {
            blocking(move || {
                admin::check(&dir, &state_owned, path.as_deref(), mount.as_deref())
                    .map(|c| {
                        AgentResponse::WorkRootChecked(WorkRootCheckView {
                            path: text(&c.path),
                            real_path: text(&c.real_path),
                            mount: c.mount.as_deref().map(text),
                        })
                    })
                    .map_err(bad)
            })
            .await
        }
        WorkRootOp::Set { path, mount } => {
            // 점검·등록·저장은 동기 파일 I/O라 통째로 blocking thread에서 돈다(worktree 보호의 await는 그 안에서 기다린다)
            let rt = tokio::runtime::Handle::current();
            let done =
                blocking(move || rt.block_on(admin::set(&dir, &state_owned, &path, mount.as_deref())).map_err(bad))
                    .await?;
            let mut v = view(&done.shown);
            v.changed = Some(done.changed);
            v.imported = Some(done.imported as u32);
            Ok(AgentResponse::WorkRoot(v))
        }
    }
}

/// Codex 설정 오류는 맨 위 까닭만 싣는다: 아래 TOML 오류 글은 그 줄 원문(비밀 값일 수 있다)을 함께 찍는다
fn hook_problem(e: anyhow::Error) -> String {
    e.to_string()
}

pub(crate) async fn codex_hook(install: Option<bool>) -> Result<AgentResponse, ErrorBody> {
    let exe = std::env::current_exe()
        .map_err(|e| ErrorBody::new(ErrorCode::Internal, format!("agent 실행 파일을 모른다: {e}")))?;
    blocking(move || {
        let home = crate::integration::codex_home;
        let backups = match install {
            None => None,
            Some(install) => Some(
                crate::integration::configure(home(), &exe, !install)
                    .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, hook_problem(e)))?
                    .len() as u32,
            ),
        };
        let (codex_installed, codex_problem) = match crate::integration::healthy(home(), &exe) {
            Ok(ok) => (ok, None),
            Err(e) => (false, Some(hook_problem(e))),
        };
        Ok(AgentResponse::Hook(HookView { codex_installed, codex_problem, backups }))
    })
    .await
}

//! host가 부르는 이 머신 설정 고정 작업(관리 GUI W5c, plan §8.356): 작업 root 보기·점검·전환(work_root.manage.v1)과 Codex
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

pub(crate) async fn work_root(state: &Path, action: WorkRootOp) -> Result<AgentResponse, ErrorBody> {
    let dir = aaa_config::paths::config_dir();
    match action {
        WorkRootOp::Show => admin::show(&dir, state).map(|s| AgentResponse::WorkRoot(view(&s))).map_err(bad),
        WorkRootOp::Check { path, mount } => admin::check(&dir, state, path.as_deref(), mount.as_deref())
            .map(|c| {
                AgentResponse::WorkRootChecked(WorkRootCheckView {
                    path: text(&c.path),
                    real_path: text(&c.real_path),
                    mount: c.mount.as_deref().map(text),
                })
            })
            .map_err(bad),
        WorkRootOp::Set { path, mount } => {
            let done = admin::set(&dir, state, &path, mount.as_deref()).await.map_err(bad)?;
            let mut v = view(&done.shown);
            v.changed = Some(done.changed);
            v.imported = Some(done.imported as u32);
            Ok(AgentResponse::WorkRoot(v))
        }
    }
}

pub(crate) fn codex_hook(install: Option<bool>) -> Result<AgentResponse, ErrorBody> {
    let exe = std::env::current_exe()
        .map_err(|e| ErrorBody::new(ErrorCode::Internal, format!("agent 실행 파일을 모른다: {e}")))?;
    let home = crate::integration::codex_home;
    let backups = match install {
        None => None,
        Some(install) => Some(crate::integration::configure(home(), &exe, !install).map_err(bad)?.len() as u32),
    };
    let (codex_installed, codex_problem) = match crate::integration::healthy(home(), &exe) {
        Ok(ok) => (ok, None),
        Err(e) => (false, Some(format!("{e:#}"))),
    };
    Ok(AgentResponse::Hook(HookView { codex_installed, codex_problem, backups }))
}

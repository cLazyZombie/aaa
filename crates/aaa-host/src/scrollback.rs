//! pane 기록을 편집기로(herdr 0.9.5 `prefix e` `edit_scrollback`·`OverlayPaneState`, `src/app/custom_commands.rs`·`src/app/api.rs`,
//! plan §8.162).
//!
//! - 화면 사본의 기록 전체(`Vt::unwrapped_all`: 접힌 줄은 잇는다)를 그 pane 머신 agent에 조각으로 보내 임시 파일로 쓰게 한다.
//!   기록은 host 화면 사본이 가진 만큼이다(상한 10,000줄, host를 다시 띄운 뒤에는 구독 snapshot의 최근 2000줄부터). herdr는 서버
//!   pane 기록 전체다.
//! - 그 pane을 좌우로 나눠 agent가 준 편집기 명령으로 새 pane을 띄우고 탭을 zoom한다(herdr `spawn_overlay_argv_command`). 한 번에
//!   저장해 layout 사건 하나로 보인다. 포커스는 front가 새 pane으로 옮긴다(host는 front마다의 포커스를 모른다).
//! - 편집기 pane이 실행 중이 아니게 되면(끝남·끝내기·session host 유실·재부팅) 그 pane을 닫고 파일을 지운다(herdr
//!   `restore_overlay_after_exit`). 탭 zoom은 `restore_zoom`.
//! - overlay 표시는 host 메모리에만 있다. host를 다시 띄우면 잊고, 편집기 pane은 끝난 화면이 남는 프로그램 pane이 된다(unix 편집기
//!   명령은 스스로 파일을 지운다).
//! - 기록에는 비밀 글이 있을 수 있다: 내용과 경로를 로그에 남기지 않는다.

use std::sync::Arc;

use aaa_proto::ATTACH_CHUNK as CHUNK;
use aaa_proto::{AgentRequest, AgentResponse, B64, ErrorBody, ErrorCode, SpawnSpec, SplitDir, Workspace};

use crate::{AgentConn, Host, err};

/// 편집기 overlay pane 하나(herdr `OverlayPaneState`)
pub(crate) struct EditOverlay {
    machine: String,
    workspace: String,
    tab: String,
    /// 나누기 전 그 탭의 zoom
    previous_zoomed: bool,
    /// agent 임시 파일 이름(hex)
    name: String,
}

impl Host {
    /// `PaneEditScrollback`: 기록을 임시 파일로 쓰고 그 pane을 나눠 편집기 pane을 띄운 뒤 탭을 zoom한다
    pub(crate) async fn pane_edit_scrollback(
        self: &Arc<Self>,
        workspace: &str,
        pane: &str,
        actor: &str,
    ) -> Result<Workspace, ErrorBody> {
        let (target, mid) = self.resolve_pane(pane)?;
        if !aaa_proto::layout::ws_contains(&self.workspace(workspace), &target) {
            return Err(err(ErrorCode::NotFound, format!("workspace {workspace}에 pane {pane}이 없다")));
        }
        if !self.machine_feature(&mid, "scrollback.v1") {
            return Err(err(
                ErrorCode::BadRequest,
                "그 머신 agent가 기록 편집을 모른다. agent를 새 aaa로 바꾼 뒤 다시 한다",
            ));
        }
        let text = {
            let mut i = self.inner.lock().unwrap();
            let m = i.panes.get_mut(&target).and_then(|p| p.mirror.as_mut());
            m.map(|m| m.vt.unwrapped_all()).ok_or_else(|| err(ErrorCode::NotFound, "화면 사본이 없다"))?
        };
        let conn = self.live_conn(&mid)?;
        let name = hex::encode(rand::random::<[u8; 8]>());
        let argv = match write_file(&conn, &name, text.as_bytes()).await {
            Ok(argv) => argv,
            Err(e) => {
                remove_file(&conn, &name).await;
                return Err(e);
            }
        };
        // herdr: 편집기는 그 pane이 있는 폴더에서
        let cwd = self.new_pane_cwd(None, Some(&target), &mid).await;
        let spec = SpawnSpec {
            name: String::new(),
            argv,
            cwd,
            env: vec![],
            cols: 80,
            rows: 24,
            role: None,
            harness: None,
            profile: None,
        };
        let id = match self.spawn_on(&mid, spec, actor).await {
            Ok(id) => id,
            Err(e) => {
                remove_file(&conn, &name).await;
                return Err(e);
            }
        };
        // 띄우는 사이 다른 변경이 있었을 수 있다: 최신을 다시 읽어 붙인다(`layout_apply` Split과 같다)
        let mut w = self.workspace(workspace);
        let placed = w.tabs.iter_mut().find(|t| aaa_proto::layout::leaves(&t.root).contains(&target)).map(|t| {
            let previous_zoomed = t.zoomed;
            aaa_proto::layout::split(&mut t.root, &target, &id, SplitDir::Horizontal);
            t.zoomed = true;
            (t.id.clone(), previous_zoomed)
        });
        let (tab, previous_zoomed) = placed.clone().unwrap_or_default();
        let overlay =
            EditOverlay { machine: mid.clone(), workspace: workspace.to_string(), tab, previous_zoomed, name };
        self.overlays.lock().unwrap().insert(id.clone(), overlay);
        if placed.is_none() {
            // 나누는 사이에 pane이 layout에서 빠졌다: 띄운 편집기를 끝낸다(끝나면 overlay 닫기가 파일을 지운다)
            let _ = self.kill_session(&conn, &id).await;
            return Err(err(ErrorCode::NotFound, "나누는 사이에 pane이 layout에서 빠졌다"));
        }
        self.save_workspace(&w);
        self.db.audit(actor, "pane_edit_scrollback", &format!("{target} {id}"));
        // 넣기 전에 끝났으면 그 끝남 사건은 overlay를 보지 못했다
        if !self.inner.lock().unwrap().panes.get(&id).is_some_and(|p| p.info.state.is_running()) {
            self.overlay_ended(&id);
        }
        Ok(w)
    }

    /// 실행 중이 아니게 된 pane(`on_session`·agent 다시 붙기)이 편집기 overlay면 닫는다
    pub(crate) fn overlay_ended(self: &Arc<Self>, id: &str) {
        let Some(o) = self.overlays.lock().unwrap().remove(id) else { return };
        let me = self.clone();
        let id = id.to_string();
        tokio::spawn(async move {
            // 빼기와 zoom 되돌리기를 한 번에 저장한다(layout 사건 하나). pane을 빼면 그 탭 zoom이 풀리니(`ws_remove`) 빼기 전에
            // 본다. 빼면 workspace가 비는 때(연 pane이 먼저 닫혔다)는 `close_record`가 빼고 workspace를 지운다
            let mut w = me.workspace(&o.workspace);
            let zoomed =
                w.tabs.iter().any(|t| t.id == o.tab && t.zoomed && aaa_proto::layout::leaves(&t.root).contains(&id));
            let mut rest = w.clone();
            if aaa_proto::layout::ws_remove(&mut rest, &id) && !rest.tabs.is_empty() {
                w = rest;
                restore_zoom(&mut w, &o.tab, zoomed, o.previous_zoomed);
                me.save_workspace(&w);
            }
            me.close_record(&id, &o.machine, "overlay").await;
            if let Ok(conn) = me.live_conn(&o.machine) {
                remove_file(&conn, &o.name).await;
            }
        });
    }
}

/// 기록을 조각으로 보낸다(빈 기록도 빈 조각 하나). 마지막 조각의 답으로 편집기 명령을 받는다.
async fn write_file(conn: &AgentConn, name: &str, bytes: &[u8]) -> Result<Vec<String>, ErrorBody> {
    let mut chunks = bytes.chunks(CHUNK).peekable();
    let mut offset = 0;
    loop {
        let data = chunks.next().unwrap_or_default();
        let eof = chunks.peek().is_none();
        let res = conn
            .request(AgentRequest::ScrollbackWrite { name: name.to_string(), offset, data: B64(data.to_vec()), eof })
            .await?;
        offset += data.len() as u64;
        if eof {
            let AgentResponse::Scrollback { argv, .. } = res else {
                return Err(err(ErrorCode::Internal, "예상하지 못한 응답"));
            };
            return Ok(argv);
        }
    }
}

async fn remove_file(conn: &AgentConn, name: &str) {
    if let Err(e) = conn.request(AgentRequest::ScrollbackRemove { name: name.to_string() }).await {
        tracing::warn!("기록 파일을 지우지 못했다: {}", e.message);
    }
}

/// 편집기 pane을 뺀 뒤 그 탭 zoom(herdr `restore_overlay_after_exit`의 `previous_zoomed`). 뺄 때까지 zoom이었으면 나누기 전
/// 값으로 돌린다(빼면 풀리니 나누기 전이 zoom일 때만 다시 켠다). 사람이 그 사이 풀었으면 그대로 둔다(herdr는 overlay에 포커스가
/// 있었으면 나누기 전 값으로 돌리지만 host는 포커스를 모른다). pane 하나 남은 탭은 켜지 않는다(`LayoutChange::Zoom`과 같다).
/// 바꿨으면 true.
fn restore_zoom(w: &mut Workspace, tab: &str, zoomed_at_exit: bool, previous_zoomed: bool) -> bool {
    let Some(t) = w.tabs.iter_mut().find(|t| t.id == tab) else { return false };
    if !zoomed_at_exit || !previous_zoomed || t.zoomed || aaa_proto::layout::leaves(&t.root).len() < 2 {
        return false;
    }
    t.zoomed = true;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use aaa_proto::{LayoutNode, Tab};

    fn ws(panes: &[&str], zoomed: bool) -> Workspace {
        let mut root = LayoutNode::Pane { pane: panes[0].into() };
        for p in &panes[1..] {
            aaa_proto::layout::split(&mut root, panes[0], p, SplitDir::Horizontal);
        }
        Workspace { name: "w".into(), tabs: vec![Tab { id: "t".into(), name: String::new(), root, zoomed }] }
    }

    #[test]
    fn zoom_comes_back_only_if_it_was_zoomed_before_and_still_at_exit() {
        // 나누기 전 zoom이었고 끝날 때도 zoom: 다시 켠다(편집기를 빼면 `ws_remove`가 풀었다)
        let mut w = ws(&["a", "b"], false);
        assert!(restore_zoom(&mut w, "t", true, true));
        assert!(w.tabs[0].zoomed);
        // 사람이 그 사이 풀었다: 그대로
        let mut w = ws(&["a", "b"], false);
        assert!(!restore_zoom(&mut w, "t", false, true));
        assert!(!w.tabs[0].zoomed);
        // 나누기 전이 zoom이 아니었다: 풀린 채로
        let mut w = ws(&["a", "b"], false);
        assert!(!restore_zoom(&mut w, "t", true, false));
        assert!(!w.tabs[0].zoomed);
        // pane 하나 남은 탭, 없어진 탭
        let mut w = ws(&["a"], false);
        assert!(!restore_zoom(&mut w, "t", true, true));
        assert!(!w.tabs[0].zoomed);
        assert!(!restore_zoom(&mut w, "gone", true, true));
    }
}

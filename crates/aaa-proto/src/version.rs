//! 계약 버전과 hello 협상.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Version {
    pub major: u16,
    pub minor: u16,
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// host ↔ agent. 1.4: `ThreadRebrief.profile`(thread restart --profile).
/// 버전을 올리지 않은 것(plan §8.113): `ThreadCleanup.merged_head`(옛 agent는 버리고 branch를 남긴다),
/// `PrQuery::OpenOwn`(agent 기능 `pr.v2`가 있을 때만 보낸다), `MergeLand`·`Landed`를 없앰(새 host는 보내지 않는다, §8.135),
/// `ThreadCleanup.uncopied`(옛 agent는 버리지만 host가 `keep_worktree`도 켜서 worktree는 남는다, §8.135),
/// `ThreadPrepare.safety`·`ThreadRebrief.safety`(agent 기능 `launch_safety.v1`이 있을 때만 보낸다, §8.163),
/// `ScrollbackWrite`·`ScrollbackRemove`(agent 기능 `scrollback.v1`이 있을 때만 보낸다, §8.176),
/// `AgentRequest::ThreadAdopt`(agent 기능 `adopt.v1`이 있을 때만 보낸다, §8.180).
/// `Post`·`PostRejected`·`PostAck`는 post.v1을 양쪽이 알렸을 때만 쓴다(§8.199).
/// 1.5 host의 N-1은 1.4다. 1.3 agent는 먼저 1.4 이상으로 갱신해야 한다.
// 1.5: 특수 임무. mission.v1을 알린 상대에만 임무 세션을 만든다.
pub const AGENT_LINK: Version = Version { major: 1, minor: 5 };
/// front ↔ host. 1.4: CLI pane·tab·workspace(plan §8.75), 1.5: 새 탭 이름(`NewTab.label`)·cwd 따라가기(`NewTab.from`)(plan §8.76),
/// 1.6: `routine_toggle`·`ThreadView.pr`(plan §8.80), 1.7: `devices`·`device_rename`·`device_revoke`, 웹 기기의 `web_pair`(plan §8.81),
/// 1.8: `thread read`·`thread keys`(plan §8.86), 1.9: `ticker_tick`(docs/testing.md),
/// 1.10: zoom·자리 바꾸기(`LayoutChange::Zoom`·`Swap`, `Tab.zoomed`)(plan §8.91),
/// 1.11: `inbox` 응답의 `omitted_open`·`omitted_done`(plan §8.98),
/// 1.12: ThreadView `warnings`(plan §8.100), 결정 기록 요청(`decision_add`·`decisions`)과 ThreadView `decisions_rev`·`decisions_now`를 없앰(plan §8.101),
/// 1.13: thread start `kind` 생략·restart `profile`·resolve `reopen`(plan §8.104),
/// 1.14: TUI 마우스(`GridFrame.modes`, `LayoutChange::SplitRatio`·`MoveTab`)(plan §8.106),
/// 1.15: `pane_rename`과 PaneView `label`(plan §8.109),
/// 1.16: `PaneView.restore_error`(plan §8.111),
/// 1.17: 묶음 `landing`(PR 승인, plan §8.113),
/// 1.18: 파일 보기 `pane_file`(plan §8.125),
/// 1.19: `pane_run`의 `profile`(plan §8.126),
/// 1.20: `pane_chat`의 `thinking`과 ChatRole `thinking`(plan §8.129),
/// 1.21: 병합 대기열 요청(`merge_enqueue`·`merge_drop`·`merge_list`·`merge_land`)을 없앰(옛 front가 보내면 모르는 요청
/// 오류다), resolve `skip_copy`·`discard_uncopied`(plan §8.135),
/// 1.22: `ReadSource::Detection`(`pane read --source detection`)(plan §8.110),
/// 1.23: TUI 붙여넣기 `pane_paste`(plan §8.147),
/// 1.24: 폴더 보기 `pane_dir`(plan §8.148),
/// 1.25: `AgentStatus.plan`·`background`(계획 n/m·배경 작업 수, plan §8.151),
/// 1.26: TUI 휠 기록 넘기기(`PaneScroll`, `GridFrame.scroll`)(plan §8.153),
/// 1.27: TUI 고른 글을 기록까지 읽기(`PaneSelectionRead`, `TextPoint`)·copy 모드 이동·찾기(`PaneCopyMotion`·`PaneCopySearch`)(plan §8.156·§8.157),
/// 1.28: ProfileView `args`·`builtin`(기본 profile claude·codex, plan §8.163),
/// 1.29: PaneView `replaced`(새 pane이 대신한 옛 ID, 보던 화면이 따라간다, plan §8.111 QA 고침),
/// 1.30: 사건 `web`(host가 주는 웹 빌드, plan §8.166),
/// 1.31: 결과물 목록의 `LibraryFile.mime`·`kind`(앱 안 파일 보기, plan §8.168)
/// 같은 1.31 배포에 host 정본 `ProfileResolve`를 더한다(§8.170). host_profiles.v1은 AGENT_LINK 1.4의 선택 기능이다.
/// 1.32: 보고서 안 파일 링크 `thread_file`(thread worktree 기준, plan §8.171)과 질문의 화면 선택 창 `QuestionView.menu`·그 답
/// `question_answer`의 `menu`·`pick`(plan §8.172). 한 번에 착지해 minor 하나로 묶는다(1.31 front가 그대로 붙는다)
/// 같은 1.32 배포에 TUI 기록을 편집기로(`PaneEditScrollback`, plan §8.176)를 더한다(1.31 front가 그대로 붙게 minor를
/// 올리지 않는다, 그 요청을 모르는 1.32 host는 오류로 답한다).
/// 1.33: `project_create` 옵션(goal·repos·profile)·`project_set`, ProjectView `status`·`projects` `all`·`project_lifecycle`·
/// `project_delete`·`project_rename`·`project_sweep`, `thread_adopt`·ThreadKind `adopted`, `project_open`·`ProjectOpened`·ProjectView
/// `coordinator_profile`·`coordinator_profiles`(4장 G·A, plan §8.180). 한 번에 착지해 minor 하나로 묶는다(1.32 front가 그대로 붙는다)
/// 1.34: `project_files`(`doctor files [--fix]`, plan §8.184). 1.33(bdbd3f0)이 10-18 14:19에 배포돼 올린다(1.33 front가 그대로 붙는다)
/// 같은 1.34(배포 전)에 앱 안 알림 사건 `notice`(턴 끝남·새 보고, plan §8.185)를 더한다. 1.33 front는 모르는 사건을 버린다
/// 같은 1.34(배포 전)에 웹 화면 색(`TermClientMsg::Colors`, pane WebSocket 주소의 `fg`·`bg`, plan §8.187)을 더한다(옛 host는 모르는
/// 메시지·주소 칸을 버리고 옛 웹은 보내지 않는다).
/// 같은 1.34(배포 전)에 `AgentStatus.background_wait`(배경 작업을 기다리는 쉼, 차이 목록 S1, plan §8.189)와
///   상태 사건 연결의 `chat_watch`·`chat_changed`·`chat_watching`(대화 기록 변화 push, 차이 목록 C3, plan §8.191)을 더한다
/// 1.35: TUI 링크 열기(`PaneLink`→`Link`, plan §8.194). 1.34가 배포돼 새 요청은 올린다(1.34 front가 그대로 붙고, TUI는 1.35
///   이상 host에만 묻는다. 옛 host에서는 ctrl+누름이 보통 누름이다).
/// 1.36: 보고서별 링크(`library_link`·`library_links`·`library_link_revoke`, `LinkView`·`LibraryView.link`, plan §8.197).
///   1.35는 TUI 링크 열기로 이미 배포됐다. 새 host는 N-1인 1.35 front까지 받는다.
/// 같은 1.36(배포 전)에 grid frame의 보이는 OSC 8 링크 칸(`GridFrame.links`, plan §8.198)을 더한다(옛 front는 모르는 칸을 버리고,
///   옛 host는 보내지 않아 TUI가 링크 없이 그린다).
/// 1.37: Post·PostStatus·PostList·PostUnmute(§8.199). 1.36이 배포되어 다음 판으로 분리한다.
/// 1.38: library publish·import·prune·rotate·전체 목록·앱 안 열기·용량(§8.200).
/// 1.39: 임무 목록·승인·수명주기와 ThreadView.mission.
/// 1.40: 보고서 사본 보관 한도·보호 용량과 정리 dry-run.
pub const FRONT_API: Version = Version { major: 1, minor: 40 };
/// agent ↔ session host(내부). 세션이 몇 주 이어질 수 있으므로 이전 session host와 호환해야 한다.
/// 1.1: `SessionRequest::Detect`(harness 상태 감지 입력). agent는 1.0 session host에 보내지 않고 `Screen`을 쓴다.
/// 1.2: `SessionRequest::Colors`(색 질의에 답할 화면 색, plan §8.187). agent는 1.1 이하 session host에 보내지 않는다.
pub const SESSION_IPC: Version = Version { major: 1, minor: 2 };
/// `SessionRequest::Detect`를 받는 첫 SESSION_IPC minor
pub const SESSION_IPC_DETECT: u16 = 1;
/// `SessionRequest::Colors`를 받는 첫 SESSION_IPC minor
pub const SESSION_IPC_COLORS: u16 = 2;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NegotiateError {
    #[error("major 버전이 다르다(host {host}, 상대 {peer})")]
    MajorMismatch { host: Version, peer: Version },
    #[error("상대 버전 {peer}이 너무 오래됐다(host {host}는 minor {min} 이상을 지원)")]
    PeerTooOld { host: Version, peer: Version, min: u16 },
    #[error("필수 기능이 없다: {0:?}")]
    MissingFeatures(Vec<String>),
}

/// host가 상대(agent)의 버전을 받아 쓸 버전을 정한다.
///
/// 규칙(기획서 §4.3, §11): 같은 major 안에서 host는 상대 minor N-1까지 지원한다.
/// 상대가 더 새 minor면 host의 minor로 내려 쓴다.
pub fn negotiate(host: Version, peer: Version) -> Result<Version, NegotiateError> {
    if host.major != peer.major {
        return Err(NegotiateError::MajorMismatch { host, peer });
    }
    let min = host.minor.saturating_sub(1);
    if peer.minor < min {
        return Err(NegotiateError::PeerTooOld { host, peer, min });
    }
    Ok(Version { major: host.major, minor: host.minor.min(peer.minor) })
}

/// 필수 기능이 상대 목록에 모두 있는지 본다. 모르는 기능은 무시한다.
pub fn require_features(required: &[&str], offered: &[String]) -> Result<(), NegotiateError> {
    let missing: Vec<String> =
        required.iter().filter(|r| !offered.iter().any(|o| o == *r)).map(|s| s.to_string()).collect();
    if missing.is_empty() { Ok(()) } else { Err(NegotiateError::MissingFeatures(missing)) }
}

#[cfg(test)]
mod tests {
    use super::*;
    const fn v(major: u16, minor: u16) -> Version {
        Version { major, minor }
    }

    #[test]
    fn same_major_window() {
        assert_eq!(negotiate(v(1, 3), v(1, 3)), Ok(v(1, 3)));
        // 이전 agent(N-1)는 받아들이고 그 minor로 내려 쓴다.
        assert_eq!(negotiate(v(1, 3), v(1, 2)), Ok(v(1, 2)));
        // 더 새 agent는 host minor로 내려 쓴다(갱신 직후 롤링 중 정상 상태).
        assert_eq!(negotiate(v(1, 3), v(1, 7)), Ok(v(1, 3)));
        assert!(matches!(negotiate(v(1, 3), v(1, 1)), Err(NegotiateError::PeerTooOld { min: 2, .. })));
        assert_eq!(negotiate(v(1, 0), v(1, 0)), Ok(v(1, 0)));
    }

    #[test]
    fn major_mismatch_is_explicit() {
        assert!(matches!(negotiate(v(2, 0), v(1, 9)), Err(NegotiateError::MajorMismatch { .. })));
        assert!(matches!(negotiate(v(1, 0), v(2, 0)), Err(NegotiateError::MajorMismatch { .. })));
    }

    #[test]
    fn features() {
        let offered = vec!["terminal.v1".to_string(), "future.thing".to_string()];
        assert!(require_features(&["terminal.v1"], &offered).is_ok());
        assert_eq!(
            require_features(&["terminal.v1", "attach.v1"], &offered),
            Err(NegotiateError::MissingFeatures(vec!["attach.v1".into()]))
        );
    }
}

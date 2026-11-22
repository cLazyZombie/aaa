//! Agent link와 Session IPC 메시지(host ↔ agent ↔ session-host).

use super::*;

// ───────────────────────── Agent link ─────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentHello {
    pub proto: Version,
    pub machine_id: String,
    pub machine_name: String,
    pub boot_id: String,
    pub os: String,
    pub arch: String,
    pub software: String,
    /// 실행 파일 SHA-256(소문자 hex, 웹 머신 상세, plan §8.372). AGENT_LINK를 올리지 않는 선택 칸이다: 옛 agent는 보내지 않고 옛 host는 버린다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default)]
    pub dev_mode: bool,
    /// 시작 때 자가 점검 결과(doctor probe 요약). 해석은 host가 표시만 한다.
    #[serde(default)]
    pub selfcheck: Option<serde_json::Value>,
    #[serde(default)]
    pub sessions: Vec<SessionInfo>,
    /// 세션별 마지막 harness 사건(agent가 `harness.json`에 둔 것). host가 연결 즉시 harness 세션 ID를 알아 재부팅 뒤
    /// 대화를 이어 띄운다. host가 `harness_hello.v1`을 알리면 agent는 연결 뒤 이 스냅샷 이후 변경분만 보낸다.
    #[serde(default)]
    pub harness: Vec<HarnessEvent>,
    /// agent 실행 신원(관리 GUI W6c). 재시작만 하는 rollout이 재시작 전 신원으로 감시를 세운다. 옛 agent는 없다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<crate::RuntimeIdentity>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostWelcome {
    pub proto: Version,
    pub host_id: String,
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default)]
    pub identity: Option<crate::RuntimeIdentity>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnrollRequest {
    pub proto: Version,
    pub machine_id: String,
    pub machine_name: String,
    /// install_enroll.v1 협상 뒤만 사용한다. host의 사람이 SAS를 승인할 때 결과를 이 ID에 결속한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installation: Option<crate::install::InstallEnrollmentBinding>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum LinkMsg {
    Hello(AgentHello),
    Welcome(HostWelcome),
    Enroll(EnrollRequest),
    /// 등록 전 기능 협상. 구판에는 grant를 보내지 않는다.
    InstallEnrollCapabilities {
        proto: Version,
        features: Vec<String>,
    },
    InstallEnrollProbe {
        proto: Version,
    },
    InstallEnroll(Box<crate::install::InstallEnrollRequest>),
    InstallEnrollLookup {
        proto: Version,
        installation_id: String,
        plan_sha256: String,
    },
    /// 기존 agent 자기 신원만 읽는다. 설치 grant/등록창 권한을 쓰지 않는다.
    InstallAgentQuery {
        proto: Version,
        machine_id: String,
        machine_name: String,
    },
    InstallAgentKnown {
        machine_id: String,
    },
    InstallEnrollUnknown,
    InstallEnrolled {
        installation_id: String,
        machine_id: String,
    },
    /// host가 등록 요청을 받아 사용자의 확인을 기다린다.
    EnrollPending,
    EnrollAccepted {
        machine_id: String,
    },
    Reject(ErrorBody),
    Req {
        id: String,
        #[serde(default)]
        deadline_ms: Option<u64>,
        body: AgentRequest,
    },
    Res {
        id: String,
        result: Outcome<AgentResponse>,
    },
    /// response_parts.v1: 큰 응답 JSON을 순서대로 나눈다. 조립 후에도 Res와 같은 ID여야 한다.
    ResPart {
        id: String,
        offset: u32,
        total: u32,
        data: B64,
    },
    Event(AgentEvent),
    Ping {
        seq: u64,
    },
    Pong {
        seq: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum AgentRequest {
    /// machine_label.v1: host의 이름표를 agent 상태에 보관한다(세션은 유지).
    MachineLabel {
        name: String,
    },
    /// account_quota.v1: 해당 pane이 관측한 비밀 없는 창만 읽는다.
    AccountQuota {
        session: String,
        generation: String,
    },
    /// account_auth.v1. 비밀을 spec·journal에 넣지 않는 전용 spawn.
    SpawnAccount {
        spec: SpawnSpec,
        credential: crate::AccountCredential,
    },
    AccountRefreshed {
        session: String,
        result: crate::AccountRefreshed,
    },
    /// deploy_stage.v1: provenance가 있는 빌드 폴더의 aaa만 읽는다.
    DeployFetch {
        path: String,
        offset: u64,
        len: u32,
        sha256: String,
    },
    DeployStage {
        rollout: String,
        project: String,
        offset: u64,
        data: B64,
        eof: bool,
        sha256: String,
        total: u64,
    },
    DeployStageStatus {
        rollout: String,
    },
    /// deploy_candidate.v1: 빌드 폴더의 `aaa`와 옆 `provenance.json`을 대조하고 dry-run으로 self-check한다(관리 GUI W6a).
    /// 작업 기록·교체는 없다
    DeployCandidate {
        path: String,
    },
    DeployStageClean {
        rollout: String,
        keep: bool,
    },
    DeployGuardArm {
        spec: crate::DeployGuardSpec,
    },
    DeployGuardControl {
        id: String,
        control: crate::DeployGuardControl,
    },
    /// deployment.v1: host가 coordinator 출처와 safety를 확인한 요청만 보낸다.
    DeployStart {
        id: String,
        project: String,
        action: crate::DeployAction,
    },
    DeployStatus {
        id: String,
    },
    /// post.v1을 양쪽이 알렸을 때만 보낸다.
    PostAck {
        post_id: String,
        result: crate::PostResult,
    },
    /// thread_read.v1: 세션 읽기 요청의 응답.
    ThreadReadReply {
        request_id: String,
        result: Outcome<FrontResponse>,
    },
    /// imagegen.v1: 이미지 요청의 비밀 없는 응답.
    ImageReply {
        request_id: String,
        result: Outcome<crate::ImageResponse>,
    },
    Spawn(SpawnSpec),
    List,
    /// 출력 구독. `from`이 보존 범위 안이면 그 위치부터 bytes만, 아니면 snapshot을 먼저 보낸다.
    Subscribe {
        session: String,
        from: Option<u64>,
    },
    Unsubscribe {
        session: String,
    },
    Input {
        session: String,
        data: B64,
    },
    Resize {
        session: String,
        cols: u16,
        rows: u16,
    },
    Kill {
        session: String,
    },
    Screen {
        session: String,
    },
    /// 그 세션이 지금 있는 폴더(앞 프로그램, 없으면 셸). agent가 `cwd.v1` 기능을 알릴 때만 보낸다.
    Cwd {
        session: String,
    },
    /// session host 기록에서 `anchor` 바로 앞 덩어리(host mirror에 없는 앞 기록, plan §8.118).
    /// agent가 `history.v1` 기능을 알리고 snapshot이 `history_more`였을 때만 보낸다.
    History {
        session: String,
        anchor: Vec<String>,
        depth: u32,
    },
    /// 파일 보기: 그 세션이 지금 있는 폴더 기준으로 경로를 찾는다(plan §8.125). 허용 폴더 밖이면 Forbidden,
    /// 없으면 NotFound다. agent가 `file.v1` 기능을 알릴 때만 보낸다.
    FileLocate {
        session: String,
        path: String,
    },
    /// 파일 보기: 세션 없이 정한 폴더(thread worktree) 기준으로 찾는다(보고서 안 파일 링크, plan §8.171). 찾는 규칙과 허용
    /// 폴더는 `FileLocate`와 같다. agent가 `file_in.v1` 기능을 알릴 때만 보낸다.
    FileLocateIn {
        dir: String,
        path: String,
    },
    /// 폴더 보기: 그 세션이 지금 있는 폴더 기준으로 폴더 한 단계를 읽는다(빈 경로·`~`는 집). 허용 폴더 밖이면 Forbidden,
    /// 폴더가 아니면 NotFound다. agent가 `dir.v1` 기능을 알릴 때만 보낸다.
    /// 입력칸 「/」 목록(plan §8.323): 그 harness의 내장 명령 표와 세션 폴더·사용자 폴더의 명령·skill 파일. agent가 `commands.v1`
    /// 기능을 알릴 때만 보낸다
    Commands {
        session: String,
        harness: String,
    },
    DirList {
        session: String,
        path: String,
        #[serde(default)]
        hidden: bool,
    },
    /// 파일 보기: `FileLocate`가 돌려준 실제 경로의 한 조각(`FileChunk`). 그 사이 바뀌었거나 허용 폴더 밖이면 거부한다.
    /// `page`면 HTML 파일을 페이지로 그린 bytes(제 폴더의 그림·style·script를 넣은 것)의 조각이다.
    FileView {
        path: String,
        offset: u64,
        len: u32,
        #[serde(default)]
        page: bool,
    },
    /// 끝난(종료·중단·유실) 세션의 기록을 지운다. 실행 중이면 거부한다.
    Forget {
        session: String,
    },
    /// 세션에 사람이 붙인 이름(`None`이면 지움). 답은 바뀐 `Session`이다. agent가 `rename.v1` 기능을 알릴 때만 보낸다.
    Rename {
        session: String,
        label: Option<String>,
    },
    /// harness 대화 기록의 끝부분(§8.27). agent는 자기 Claude·Codex 기록 폴더 안의 파일만 읽는다.
    Chat {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session: Option<String>,
        harness: String,
        /// Claude hook이 알린 기록 파일
        transcript: Option<String>,
        /// Codex thread-id(기록 파일 이름에 들어 있다)
        harness_session: Option<String>,
        /// 생각도 싣는다(plan §8.129). 옛 agent는 이 칸을 모르고 빼고 보낸다
        #[serde(default)]
        thinking: bool,
        /// 정리된 자리(`ChatRole::Compact`)도 싣는다(plan §8.309). 옛 host는 보내지 않아 agent가 빼고, 옛 agent는 이 칸을 모른다
        #[serde(default)]
        compact: bool,
        /// harness 명령과 결과(`ChatRole::Command`·`CommandOutput`, plan §8.323)도 싣는다. 옛 host는 보내지 않는다
        #[serde(default)]
        commands: bool,
        /// 이전 응답의 before. 생략하면 최신 쪽, 옛 상대는 무시한다.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        before: Option<String>,
    },
    /// 옛 host 호환: agent.toml에 정의된 profile 목록
    Profiles,
    /// host_profiles.v1: 정의를 제외한 설치·로그인한 기본 harness만 묻는다.
    Harnesses,
    /// thread 자리(worktree 또는 작업 폴더)를 만들고 brief.md를 쓰고 profile을 해석한다
    ThreadPrepare(ThreadPrepare),
    /// thread_journal.v1: 준비와 같은 lock에서 기록된 자리를 조회한다(새 자리는 만들지 않는다).
    ThreadPlacement {
        project: String,
        thread: String,
        title: String,
        kind: ThreadKind,
        repo: Option<String>,
    },
    /// 사람이 띄운 agent 세션을 thread로 데려온다: 그 세션의 지금 폴더에 작업 폴더·brief.md를 만든다(`thread adopt`).
    /// agent 기능 `adopt.v1`이 있을 때만 보낸다(옛 agent는 모르는 요청에 연결을 끊는다)
    ThreadAdopt(ThreadAdopt),
    /// report.md 변화를 `ReportChanged`로 알린다. 등록 즉시 지금 hash를 한 번 보낸다. `thread`는 `<project>/<thread>`다.
    ThreadWatch {
        thread: String,
        work_dir: String,
    },
    ThreadUnwatch {
        thread: String,
    },
    /// 대화 기록 파일 감시(차이 목록 C3, herdr-web-ui `conversation-watch`): 켜면 `Chat`과 같은 파일을 250ms마다 보고 바뀌면
    /// `ChatChanged`로 알린다. 켤 때 지금 값을 한 번 보낸다. `key`는 host가 정한 이름(pane ID)이고 같은 key를 다시 켜면 바꾼다.
    /// agent 기능 `chat_watch.v1`이 있을 때만 보낸다
    ChatWatch {
        key: String,
        harness: String,
        transcript: Option<String>,
        harness_session: Option<String>,
        enabled: bool,
    },
    /// report_hash.v1: library를 걷지 않고 report.md hash만 읽는다. 빈 본문은 Conflict(다음 쓰기까지 보류).
    ReportHash {
        work_dir: String,
    },
    /// report.md 본문과 library 목록. symlink는 따라가지 않고, 상한을 넘는 파일은 건너뛴다.
    /// 빈 본문은 Conflict. host는 기존 사본을 보존하고 다음 쓰기 사건에서 다시 수집한다.
    ReportCollect {
        work_dir: String,
        max_library_bytes: u64,
    },
    /// report_copy.v2: 인증한 link에서 원본 무결성을 확인한 뒤 host 저장 경계에서 텍스트만 가린다.
    /// ReportCollect처럼 빈 본문은 Conflict로 보류한다.
    ReportTransfer {
        work_dir: String,
        max_library_bytes: u64,
    },
    ReportFileRead {
        work_dir: String,
        path: String,
        offset: u64,
        len: u32,
    },
    /// work_dir 아래 파일 한 조각(library 사본용)
    FileRead {
        work_dir: String,
        path: String,
        offset: u64,
        len: u32,
    },
    /// 새 대화 준비. host_profiles.v1이면 host가 푼 resolved_profile을 받고 머신 경로·env·hook을 적용한다.
    /// 옛 host는 profile 이름과 safety를 보내며, 둘 다 없으면 저장된 launch를 쓴다.
    ThreadRebrief {
        work_dir: String,
        brief: String,
        launch: Launch,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        profile: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resolved_profile: Option<crate::ResolvedProfile>,
        /// profile을 다시 풀 때 더할 safety(plan §8.163, `launch_safety.v1`을 알린 agent에만)
        #[serde(default, skip_serializing_if = "Option::is_none")]
        safety: Option<crate::LaunchSafety>,
    },
    /// routine 명령을 `sh -c`로 한 번 실행한다(자기 process group, 시간 초과면 그 group을 끝낸다).
    /// `cwd`가 없으면 `<work_root>/<project>/routines`에서 실행한다.
    RunCommand {
        project: String,
        cwd: Option<String>,
        command: String,
        timeout_ms: u64,
    },
    /// 붙여 넣은 그림 한 조각을 세션 폴더(`sessions/<id>/attachments/`)에 쓴다. `att`는 host가 정한 이름(hex.확장자)이다.
    /// `offset`은 지금까지 쓴 길이와 같아야 하고, `eof`면 sha256을 확인해 이름을 바꾸고 절대 경로를 돌려준다.
    AttachWrite {
        session: String,
        att: String,
        offset: u64,
        data: B64,
        eof: bool,
    },
    /// task 첨부(plan §8.327): thread 작업 폴더 `attachments/`에 `AttachWrite`와 같은 꼴로 쓴다(같은 이름은 덮어쓴다). 답도 같다.
    /// agent 기능 `work_attach.v1`이 있을 때만 보낸다(옛 agent는 모르는 요청에 연결을 끊는다)
    WorkAttachWrite {
        work_dir: String,
        att: String,
        offset: u64,
        data: B64,
        eof: bool,
    },
    /// 화면의 터미널 기본 색이 바뀌었다(plan §8.187). agent는 모든 session host에 알리고 새 세션에 실은 뒤 답한다.
    /// agent 기능 `term_colors.v1`이 있을 때만 보낸다.
    TermColors(TermColors),
    /// 사후 평가자 사건 묶음을 작업 폴더의 `inbox/` 아래에 쓴다(§8.13). `path`는 `inbox/<이름>.json`만 받는다.
    /// mission.v1을 양쪽이 알린 연결에서만 임무 입력의 처리 여부를 읽는다.
    WorkFileInboxStatus {
        work_dir: String,
    },
    /// mission.v1: repo·machine 변경 때 상태 파일만 옮긴다. 본문은 모두 자료다.
    WorkFileList {
        work_dir: String,
    },
    WorkFileRead {
        work_dir: String,
        path: String,
        offset: u64,
    },
    WorkFileWriteChunk {
        work_dir: String,
        path: String,
        offset: u64,
        data: B64,
        eof: bool,
        sha256: String,
    },
    MissionInboxStatus {
        work_dir: String,
    },
    /// mission.v1: repo·machine 변경 때 상태 파일만 옮긴다. 본문은 모두 자료다.
    MissionStateList {
        work_dir: String,
    },
    MissionStateRead {
        work_dir: String,
        path: String,
        offset: u64,
    },
    MissionStateWrite {
        work_dir: String,
        path: String,
        offset: u64,
        data: B64,
        eof: bool,
        sha256: String,
    },
    WorkFileWrite {
        work_dir: String,
        path: String,
        data: B64,
    },
    /// cleanup_probe.v1 기능을 알린 agent에만 보낸다. 삭제하지 않고 입력 순서대로 부재를 확인한다(최대 500개).
    ThreadCleanupProbe {
        repo: String,
        targets: Vec<crate::CleanupTarget>,
    },
    /// thread 정리: worktree를 force 없이 지우고 현재 branch 끝이 main에 포함됐음을 확인했을 때 branch를 지운다.
    /// `merged_head`: 원격 main의 조상인 branch 끝. None이면 branch를 남긴다. 확인 뒤 끝이 바뀌면 지우지 않는다.
    ThreadCleanup {
        repo: Option<String>,
        cwd: String,
        branch: Option<String>,
        #[serde(default)]
        merged_head: Option<String>,
        keep_worktree: bool,
        /// 마지막 사본에 집에 오지 않은 것이 있다(herdr-projects `clean` `copy_complete`, plan §8.135): 폴더가 남아 있으면
        /// worktree를 남긴다. 이 칸을 모르는 옛 agent도 지우지 않게 host는 `keep_worktree`를 함께 켠다(둘 다 켜져 있으면
        /// 사람의 「남기기」가 아니라 이것이다).
        #[serde(default)]
        uncopied: bool,
    },
    /// git.v1을 알린 repo 머신 agent만 origin head·변화 객체를 읽는다.
    /// workspace_metrics.v2: 등록한 main checkout의 push 확인 결과와 원격 신원을 회수한다.
    WorkspaceMetrics {
        cwd: String,
        workspace_key: String,
        known_code_commits: Vec<String>,
    },
    GitQuery {
        cwd: String,
        query: crate::GitQuery,
    },
    /// ci.v1을 알린 agent만 조건부 GitHub CI를 읽는다.
    CiQuery {
        cwd: String,
        query: crate::CiQuery,
    },
    /// diff.v1을 알린 agent만 thread worktree(`cwd`)의 바뀐 파일·patch를 읽는다(plan §8.305). `base`는 thread를 시작할 때
    /// 잡은 기준 ref(`origin/main` 등)이고, 없으면 agent가 `origin/HEAD`·`main`·`master`에서 고른다
    ThreadDiff {
        cwd: String,
        #[serde(default)]
        base: Option<String>,
        query: crate::DiffQuery,
    },
    /// design.v1을 알린 repo 머신 agent만: 저장소(`repo`, main checkout)의 기획 폴더 `folder` 안 Markdown 목록. origin을 받은 뒤
    /// (1분에 한 번) origin 기본 branch commit의 git 객체에서 읽는다. 작업 파일·index·branch는 바꾸지 않는다
    DesignTree {
        repo: String,
        folder: String,
        /// design_write.v1: 기획 branch `design/<slug>`의 끝에서 읽고 main과 달리 고친 파일을 함께 싣는다
        #[serde(default, skip_serializing_if = "Option::is_none")]
        branch: Option<String>,
    },
    /// design.v1: 기획 폴더 안 문서 글 하나. `commit`이 없으면 가지고 있는 origin 기본 branch(fetch하지 않는다)
    DesignRead {
        repo: String,
        folder: String,
        #[serde(default)]
        commit: Option<String>,
        path: String,
        /// design_write.v1: 기획 branch에서 읽는다(`commit`은 그 branch의 판이어도 된다)
        #[serde(default, skip_serializing_if = "Option::is_none")]
        branch: Option<String>,
    },
    /// design.v1: 기획 폴더 안 파일(그림·영상)의 한 조각. 답은 `DesignChunk`다
    DesignFile {
        repo: String,
        folder: String,
        commit: String,
        path: String,
        offset: u64,
        len: u32,
        /// design_write.v1: `commit`이 이 기획 branch의 판이어도 된다
        #[serde(default, skip_serializing_if = "Option::is_none")]
        branch: Option<String>,
    },
    /// design_write.v1(t-1894 2단계): 기획 branch `design/<slug>`의 전용 worktree를 연다. 없으면 origin 기본 branch에서 만들고,
    /// branch만 남은 것(쉼·다른 머신)은 그 branch로 다시 꺼낸다. 답은 `DesignBranch`다
    DesignOpen {
        project: String,
        repo: String,
        slug: String,
    },
    /// design_write.v1: 기획 branch worktree에 문서 하나를 쓰고 그 파일만 commit한다. `base`는 읽은 blob이고 새 문서는 None이다.
    /// 그 사이 다른 판이 생겼으면 쓰지 않고 conflict다. 답은 `DesignSaved`다
    DesignSave {
        project: String,
        repo: String,
        folder: String,
        slug: String,
        path: String,
        #[serde(default)]
        base: Option<String>,
        text: String,
        message: String,
    },
    /// design_attach.v1(t-1894 2-3): 기획 branch에 그림·영상 파일 하나를 조각(`ATTACH_CHUNK`)으로 넘겨 새 파일로 commit한다.
    /// 조각은 agent 상태 폴더에 모으고(`offset`은 이어 붙일 자리), 마지막 조각(`eof`)에서 크기·`sha256`을 확인한 뒤 그 파일만
    /// commit한다. 답은 중간 조각이 `Done`, 마지막이 `DesignSaved`다
    DesignAttach {
        project: String,
        repo: String,
        folder: String,
        slug: String,
        path: String,
        offset: u64,
        data: crate::B64,
        eof: bool,
        sha256: String,
        message: String,
    },
    /// design_detach.v1: 기획 branch에서 그림 하나를 지우고 참조 문서의 경고를 돌려준다
    DesignDetach {
        project: String,
        repo: String,
        folder: String,
        slug: String,
        path: String,
        message: String,
    },
    /// design_ids.v1(t-1957): 기획 branch worktree의 문서 여럿에 ID 표지를 넣고 바뀐 파일만 한 commit으로 남긴다.
    /// 이미 표지가 있는 문서는 그대로 둔다(다시 돌려도 안전하다). 답은 `DesignIdsMarked`다
    DesignMarkIds {
        project: String,
        repo: String,
        folder: String,
        slug: String,
        ids: Vec<crate::DesignIdMark>,
        message: String,
    },
    /// design_write.v1: 기획 branch를 origin의 같은 이름으로 올린다(force 없음). `check`가 있으면 먼저 그 worktree에서 셸 없이 돌려
    /// 통과해야 올린다(저장소의 `design_check`, 공개 저장소의 공개 검사). 답은 `DesignPushed`다
    DesignPush {
        project: String,
        repo: String,
        slug: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        check: Vec<String>,
    },
    /// design_write.v1: 기획 branch worktree를 force 없이 내린다(쉼). `discard`면 로컬 branch도(`keep_local`이면 남긴다),
    /// `remote`가 있으면 origin branch가 그 판일 때만 지운다(lease, 남이 그 위에 올린 것은 지우지 않는다). 답은 `Done`이다
    DesignClose {
        project: String,
        repo: String,
        slug: String,
        discard: bool,
        #[serde(default)]
        keep_local: bool,
        #[serde(default)]
        remote: Option<String>,
    },
    /// design_main.v1(t-2002): 저장소의 「main 기획 자리」(작업 root마다 `<저장소>/<프로젝트>-design-main`, detached)를 만들거나
    /// origin 기본 branch 끝으로 맞춘다. 기획 대화 thread가 읽는 자리다. 답은 `DesignBranch`(`opened` = `main`)다
    DesignMainOpen {
        project: String,
        repo: String,
    },
    /// design_main.v1: 그림·영상 조각을 agent 상태 폴더에 모은다(뒤의 `DesignMainWrite`가 `Staged { sha256 }`으로 쓴다).
    /// 마지막 조각(`eof`)에서 sha256을 확인한다. 답은 `Done`이다
    DesignMainStage {
        project: String,
        repo: String,
        /// 전송 ID(영문 대문자·숫자, host가 붙일 때마다 새로)
        #[serde(default)]
        upload: String,
        sha256: String,
        offset: u64,
        data: crate::B64,
        eof: bool,
    },
    /// design_main.v1: 기획 폴더 파일 여럿을 한 commit으로 origin 기본 branch에 올린다. 받기 → 자리를 그 끝으로 맞춤 → 파일마다
    /// 기준 blob 비교(다르면 Stale) → commit → 검사(`check`, 인자의 `{base}`·`{head}`를 바꾼다) → force 없이 push. push가
    /// non-fast-forward로 거절되면 새 끝에서 처음부터 다시 한다(`DESIGN_MAIN_TRIES`). 답은 `DesignMainSaved`다
    DesignMainWrite {
        project: String,
        repo: String,
        folder: String,
        files: Vec<crate::DesignMainFile>,
        message: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        check: Vec<String>,
    },
    /// design_main.v1: 기획서 ID 표지를 문서 여럿에 넣어 한 commit으로 올린다(`DesignMainWrite`와 같은 절차).
    /// 답은 `DesignIdsMarked`다
    DesignMainMarkIds {
        project: String,
        repo: String,
        folder: String,
        ids: Vec<crate::DesignIdMark>,
        message: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        check: Vec<String>,
    },
    /// design_main.v1: 기획 폴더 안 파일 하나의 main 이력(`--first-parent`, 새것부터 `limit`개). 답은 `DesignLog`다
    DesignLog {
        repo: String,
        folder: String,
        path: String,
        limit: u32,
    },
    /// work_root.manage.v1을 알린 agent만: 그 머신 작업 root 보기·점검·전환(관리 GUI W5c, `aaa work-root`와 같다)
    WorkRoot {
        action: crate::WorkRootOp,
    },
    /// hook.manage.v1을 알린 agent만: Codex 전역 hook 상태(`install` 없음)·설치(true)·제거(false). 고정 작업이라 다른 인자는 없다
    CodexHook {
        #[serde(default)]
        install: Option<bool>,
    },
    /// model_catalog.v1을 알린 agent만: 그 머신 harness가 아는 모델 목록(Codex `models_cache.json`의 보이는 모델)
    ModelCatalog,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentResponse {
    WorkspaceMetrics(crate::WorkspaceCollected),
    AccountQuota {
        snapshot: Option<crate::QuotaSnapshot>,
    },
    DeployGuard {
        guard: Box<crate::DeployGuardView>,
    },
    DeployChunk {
        offset: u64,
        data: B64,
        eof: bool,
        total: u64,
    },
    DeployStage {
        stage: crate::DeployStageView,
    },
    DeployCandidate {
        sha256: String,
        commit: Option<String>,
        version: String,
        total_bytes: u64,
        built_at: Option<String>,
    },
    Deployment {
        job: crate::DeployJob,
        running_software: String,
        running_sha256: String,
        #[serde(default)]
        running_instance: String,
        #[serde(default)]
        running_started_ms: u64,
    },
    WorkFileInbox {
        pending: u32,
        names: Vec<String>,
    },
    WorkFiles {
        files: Vec<crate::LibFile>,
    },
    MissionInbox {
        pending: u32,
        names: Vec<String>,
    },
    MissionFiles {
        files: Vec<crate::LibFile>,
    },
    Session(SessionInfo),
    Sessions {
        sessions: Vec<SessionInfo>,
    },
    /// 구독 시작. snapshot이 없으면 `from`부터 이어진다.
    Subscribed {
        session: String,
        snapshot: Option<Snapshot>,
        offset: u64,
    },
    Screen(ScreenText),
    /// 상태 감지 입력(session host → agent, SESSION_IPC 1.1)
    Detect(DetectScreen),
    Done,
    Chat {
        messages: Vec<ChatMessage>,
        truncated: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        before: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        history: Option<String>,
    },
    Profiles {
        profiles: Vec<ProfileView>,
    },
    ThreadReady(ThreadReady),
    ThreadPlacement {
        placement: Option<ThreadPlacement>,
    },
    ThreadAdopted(ThreadAdopted),
    Report(ReportSnapshot),
    ReportHash {
        hash: String,
    },
    /// report_copy.v2 전용 전송 응답. 표시용 FileChunk와 구별한다.
    ReportTransfer(ReportSnapshot),
    ReportFileChunk {
        data: B64,
        eof: bool,
    },
    FileChunk {
        data: B64,
        eof: bool,
    },
    /// design.v1 `DesignTree`의 답
    DesignTree(crate::DesignTree),
    /// design.v1 `DesignRead`의 답
    DesignText {
        text: String,
        blob: String,
        commit: String,
        committed_ms: u64,
    },
    /// design.v1 `DesignFile`의 답: 조각과 파일 전체 크기
    DesignChunk {
        data: B64,
        total: u64,
    },
    /// design_write.v1 `DesignOpen`의 답: worktree 자리, branch 끝, 갈라진 기준 ref, 어떻게 열었나(`open` 이미 열림·`created` 새로
    /// 만듦·`local` 있던 로컬 branch·`remote` origin branch에서 꺼냄)
    DesignBranch {
        cwd: String,
        head: String,
        base: String,
        #[serde(default)]
        opened: String,
    },
    /// design_write.v1 `DesignSave`의 답: 새 commit과 쓴 파일의 blob
    DesignSaved {
        commit: String,
        blob: String,
    },
    /// design_detach.v1 `DesignDetach`의 답: 삭제 commit과 참조 검사 경고
    DesignDetached {
        commit: String,
        warnings: Vec<String>,
    },
    /// design_ids.v1 `DesignMarkIds`의 답: 표지를 넣은 문서(경로)와 그 commit(넣은 것이 없으면 없음)
    DesignIdsMarked {
        commit: Option<String>,
        marked: Vec<String>,
    },
    /// design_write.v1 `DesignPush`의 답: 올린 commit
    DesignPushed {
        head: String,
    },
    /// design_main.v1 `DesignMainWrite`의 답: 올린 commit과 그 바로 앞 main 판, 파일별 새 blob, 지운 그림의 남은 참조 경고.
    /// 바뀐 것이 없으면(같은 그림을 다시 붙임) `commit`은 지금 main 끝이고 `base`와 같다
    DesignMainSaved {
        commit: String,
        base: String,
        blobs: Vec<crate::DesignMainBlob>,
        #[serde(default)]
        warnings: Vec<String>,
    },
    /// design_main.v1 `DesignLog`의 답
    DesignLog {
        entries: Vec<crate::DesignLogEntry>,
    },
    Cleanup(Cleanup),
    CleanupAbsent {
        absent: Vec<bool>,
    },
    Relaunch(Launch),
    CommandRan(CommandRan),
    /// 첨부를 다 썼다(절대 경로와 sha256)
    Attached {
        path: String,
        sha256: String,
    },
    /// 세션이 지금 있는 폴더(읽지 못하면 None)
    Cwd {
        cwd: Option<String>,
    },
    /// 읽은 폴더 한 단계
    DirListed(DirListing),
    /// `AgentRequest::Commands`의 답
    Commands(CommandList),
    /// 찾은 파일. 없으면 끝이 같은 파일 후보(실제 경로, 둘 이상)
    FileLocated {
        info: Option<FileInfo>,
        candidates: Vec<String>,
    },
    Ci(crate::CiAnswer),
    Git(crate::GitAnswer),
    History(HistoryChunk),
    Diff(crate::DiffView),
    WorkRoot(crate::WorkRootView),
    WorkRootChecked(crate::WorkRootCheckView),
    Hook(crate::HookView),
    /// model_catalog.v1: 모델과 읽지 못한 까닭
    ModelCatalog {
        models: Vec<crate::CatalogModel>,
        #[serde(default)]
        problems: Vec<String>,
    },
}

/// routine 명령 실행 결과
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandRan {
    /// 종료 코드(신호로 끝났거나 시간 초과면 None)
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    /// stdout 뒤에 stderr. 상한에서 자른다.
    pub output: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "ev", rename_all = "snake_case")]
pub enum AgentEvent {
    /// thread_read.v1: agent가 세션 token·peer PID를 확인한 읽기 요청.
    ThreadRead {
        request_id: String,
        session: String,
        request: Box<FrontRequest>,
    },
    /// imagegen.v1: agent가 peer PID·hook token을 확인한 자기 thread 요청.
    Image {
        request_id: String,
        session: String,
        request: crate::ImageRequest,
    },
    AccountRefresh(crate::AccountRefresh),
    AgyQuota(crate::AgyQuotaSample),
    DeploymentChanged {
        job: crate::DeployJob,
    },
    DeployGuardChanged {
        id: String,
    },
    /// post.v1: hook 판정에서 거부한 글의 메타데이터(본문·token 없음).
    PostRejected {
        session: String,
        post_id: String,
        bytes: u64,
        sha256: String,
        reason: String,
    },
    /// peer+token을 agent가 판정했다. 역할·프로젝트는 host가 정한다.
    Post {
        session: String,
        #[serde(flatten)]
        input: crate::PostInput,
    },
    /// harness hook·notify·`aaa report`에서 온 상태 사건(adapter가 정규화)
    Harness(HarnessEvent),
    /// 감시 중인 thread의 report.md hash(없으면 None)가 바뀌었다
    ReportChanged {
        thread: String,
        hash: Option<String>,
    },
    SessionStarted(SessionInfo),
    SessionUpdated(SessionInfo),
    /// 이 세션의 출력 구독이 끊겼다(느린 연결). host가 마지막 offset부터 다시 구독한다.
    Resync {
        session: String,
    },
    /// 머신 건강(주기 보고). host가 `health.v1`을 알렸을 때만 보낸다.
    Health(MachineHealth),
    /// `ChatWatch`로 감시하는 기록 파일이 바뀌었다. `signature`는 그 파일 상태의 뜻 없는 이름이다(경로를 싣지 않는다)
    ChatChanged {
        key: String,
        signature: String,
    },
}

// ───────────────────────── Session IPC ─────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum SessionMsg {
    AccountRefresh(crate::AccountRefresh),
    Hello {
        proto: Version,
        token: String,
    },
    Welcome {
        proto: Version,
        info: SessionInfo,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        account_rules: Option<crate::SecretText>,
    },
    Reject(ErrorBody),
    Req {
        id: String,
        body: SessionRequest,
    },
    Res {
        id: String,
        result: Outcome<AgentResponse>,
    },
    /// 세션 상태 변화(종료·크기)
    Updated(SessionInfo),
    /// 구독자가 너무 느려 구독을 끊었다. 마지막으로 받은 offset부터 다시 구독한다.
    Resync,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum SessionRequest {
    AccountRefreshed(crate::AccountRefreshed),
    Subscribe {
        from: Option<u64>,
    },
    Unsubscribe,
    Input {
        data: B64,
    },
    Resize {
        cols: u16,
        rows: u16,
    },
    Kill,
    Screen,
    /// 상태 감지 입력(SESSION_IPC 1.1부터). 순번이 `unchanged_since`와 같으면 글을 싣지 않는다.
    Detect {
        unchanged_since: Option<u64>,
    },
    /// 기록에서 `anchor` 바로 앞 덩어리(plan §8.118). snapshot에 `history_more`를 실은 session host에만 보낸다.
    History {
        anchor: Vec<String>,
        depth: u32,
    },
    /// 프로그램의 색 질의(OSC 10/11)에 답할 화면 색(SESSION_IPC 1.2부터, plan §8.187)
    Colors(TermColors),
}

/// 화면(front)의 터미널 기본 색. 프로그램의 색 질의(OSC 10/11)에 session host가 이 색으로 답한다(herdr 0.9.5 host terminal theme,
/// plan §8.187)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TermColors {
    /// 기본 글자 색 RGB
    pub fg: [u8; 3],
    /// 기본 바탕 색 RGB
    pub bg: [u8; 3],
}

impl TermColors {
    /// `rrggbb` 둘(웹 화면의 CSS token, 앞의 `#`은 있어도 된다)
    pub fn from_hex(fg: &str, bg: &str) -> Option<TermColors> {
        let rgb = |s: &str| -> Option<[u8; 3]> {
            let s = s.trim().trim_start_matches('#');
            if s.len() != 6 || !s.is_ascii() {
                return None;
            }
            let at = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok();
            Some([at(0)?, at(2)?, at(4)?])
        };
        Some(TermColors { fg: rgb(fg)?, bg: rgb(bg)? })
    }
}

impl AgentRequest {
    /// 1.9 agent와 파일 전송을 계속한다. 새 기능이 없는 상대에게 새 태그를 보내지 않는다.
    pub fn legacy_work_file(self) -> Self {
        match self {
            Self::WorkFileInboxStatus { work_dir } => Self::MissionInboxStatus { work_dir },
            Self::WorkFileList { work_dir } => Self::MissionStateList { work_dir },
            Self::WorkFileRead { work_dir, path, offset } => Self::MissionStateRead { work_dir, path, offset },
            Self::WorkFileWriteChunk { work_dir, path, offset, data, eof, sha256 } => {
                Self::MissionStateWrite { work_dir, path, offset, data, eof, sha256 }
            }
            other => other,
        }
    }
}

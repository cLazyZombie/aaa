//! Front API 요청·응답·사건(CLI·웹 ↔ host).

use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum FrontRequest {
    /// 저장한 지표만 읽는다. agent 호출·로그 읽기 없음.
    WorkspaceDashboard {
        query: crate::WorkspaceQuery,
    },
    /// 설치 전용 서명 승인. Controller 또는 세션 밖 Local만, raw 승인서는 응답하지 않는다.
    InstallApprove {
        request: Box<crate::install::InstallApprovalRequest>,
    },
    /// 등록은 host의 세션 밖 Local만. 비밀값은 반환하거나 감사 기록에 넣지 않는다.
    AccountImport {
        name: String,
        harness: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        email: Option<crate::SecretText>,
        secret: crate::SecretText,
    },
    AccountList,
    AccountPlans,
    AccountPlan {
        name: String,
        plan: Option<String>,
    },
    AccountDefaults,
    AccountPool,
    AccountPoolMember {
        revision: String,
        account: String,
        member: Option<crate::AccountPoolMember>,
    },
    AccountPoolProject {
        revision: String,
        project: String,
        enabled: bool,
    },
    AccountStarts {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
    },
    AccountStartCancel {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        id: String,
    },
    /// 별칭 `auto`와 충돌하지 않는 명시적 풀 시작. 같은 ID의 재요청은 같은 시작이다.
    ThreadStartPool {
        id: String,
        size: crate::AccountTaskSize,
        start: Box<FrontRequest>,
    },
    AccountDefault {
        harness: String,
        account: Option<String>,
    },
    /// Claude 조회용 인증. 실행 인증은 바꾸지 않는다.
    AccountUsageLogin {
        name: String,
        remote: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        email: Option<crate::SecretText>,
    },
    AccountLogin {
        name: String,
        harness: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        email: Option<crate::SecretText>,
    },
    /// FRONT_API 1.151: 접속 기기에서 공식 CLI 인증을 마친다. 조작 기기 전용.
    AccountLoginRemote {
        name: String,
        harness: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        email: Option<crate::SecretText>,
    },
    /// id가 없으면 요청 기기에서 진행 중인 인증을 복원한다. 다른 기기에는 제공하지 않는다.
    AccountLoginInteraction {
        id: Option<String>,
    },
    AccountLoginCode {
        id: String,
        code: crate::SecretText,
    },
    AccountLoginStatus {
        id: String,
    },
    AccountLoginCancel {
        id: String,
    },
    /// FRONT_API 1.79. 다시 로그인에 다른 메일이 들어왔을 때(`mismatch`) 고른다: `switch`면 이 별칭을 새 메일 인증으로 바꾸고,
    /// 아니면 새 인증을 버리고 옛 인증을 남긴다(`kept`). 조작 기기와 host의 세션 밖 CLI만
    AccountLoginConfirm {
        id: String,
        switch: bool,
    },
    /// 재로그인 없이 같은 harness의 별칭과 참조를 옮긴다. 사람 전용(FRONT_API 1.128).
    AccountRename {
        name: String,
        to: String,
    },
    /// FRONT_API 1.169. 사람만 계정 이름 변경을 막는 이미지 작업을 확인한다.
    AccountImageJobs {
        name: String,
    },
    /// 결과 불명 기록을 사람이 확인하여 정리한다. 재생성·사용량 예약 해제는 하지 않는다.
    AccountImageDismiss {
        name: String,
        id: String,
    },
    /// 등록 메일 저장·수정. 조작 기기와 세션 밖 CLI만 허용한다.
    AccountEmail {
        name: String,
        email: crate::SecretText,
    },
    AccountRemove {
        name: String,
    },
    /// 옛 host가 account 필드를 버리고 기본 인증으로 실행하지 않게 별도 요청을 쓴다.
    ThreadStartAccount {
        account: String,
        start: Box<FrontRequest>,
    },
    /// 로컬 입력 진단. 본문·화면을 담지 않는다.
    DebugInput {
        pane: Option<String>,
        thread: Option<String>,
        since_ms: u64,
    },
    RolloutStart {
        id: String,
        spec: crate::RolloutSpec,
    },
    RolloutStatus {
        id: String,
    },
    DeployContext,
    DeployStart {
        machine: Option<String>,
        id: String,
        action: crate::DeployAction,
    },
    DeployStatus {
        machine: Option<String>,
        id: String,
    },
    /// FRONT_API 1.82. 웹 「배포」의 시작 전 점검: 빌드 머신 후보의 provenance·SHA·dry-run self-check와 대상(host 먼저)
    RolloutPlan {
        source_machine: String,
        path: String,
        /// 비었으면 등록된 모든 머신
        #[serde(default)]
        machines: Vec<String>,
    },
    /// FRONT_API 1.82. 이 프로젝트의 최근 rollout 요약(새것부터)
    Rollouts,
    /// FRONT_API 1.82. 이 프로젝트의 최근 개별 배포 작업(재시작) 요약(새것부터)
    DeployJobs,
    Post {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        input: crate::PostInput,
    },
    PostStatus {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        post_id: String,
    },
    /// 사용자 답을 받은 확인 필요 post를 처리됨으로 표시한다. 질문 선택·승인·실행은 하지 않는다.
    PostHandled {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        post_id: String,
    },
    PostList {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
    },
    /// 「답할 것」(FRONT_API 1.78, plan §8.345, 보기 권한): 열린 coordinator 확인 필요 post와 답한 것 기록
    Asks {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        /// FRONT_API 1.161: 원문 첫 쪽과 가벼운 전체 색인을 읽는다. 생략하면 구판 전체 응답.
        #[serde(default)]
        #[ts(optional, as = "Option<bool>")]
        paged: bool,
    },
    /// FRONT_API 1.161: 답한 원문의 다음 쪽 또는 직접 링크 하나(보기 권한).
    AskHistory {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        before: Option<String>,
        id: Option<String>,
    },
    /// coordinator 확인 필요 post를 「답함으로 표시」한다: 그 inbox 항목을 끝내고 기록에 남긴다
    AskHandle {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        item: String,
    },
    /// coordinator post의 구조화 질문에 답하고 요청을 닫는다(FRONT_API 1.87).
    AskAnswer {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        item: String,
        body_hash: String,
        picks: Vec<Option<u32>>,
        text: Option<String>,
        /// 질문 배열 순서의 직접 답·의견. text는 옛 공통 답으로 유지한다.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        #[ts(optional, as = "Option<Vec<Option<String>>>")]
        comments: Vec<Option<String>>,
    },
    /// coordinator 모드 상태. 읽기는 모든 front, 변경은 사람만.
    AutoDecisions {
        #[serde(default)]
        project: String,
        #[serde(default)]
        before: Option<String>,
        #[serde(default)]
        id: Option<String>,
    },
    AutoModeGet {
        #[serde(default)]
        project: String,
    },
    AutoModeSet {
        #[serde(default)]
        project: String,
        on: bool,
    },
    /// 검증한 coordinator만 자동 답을 보낸다. 일반 답과 별도 op라 구판 host에서 사람 답으로 강등되지 않는다.
    QuestionAutoAnswer {
        #[serde(default)]
        project: String,
        id: String,
        choice: Option<String>,
        text: Option<String>,
        reason: String,
    },
    /// 확인 처리 또는 새 답 전달. null은 확인, 글은 coordinator에 재판단 요청(되돌림 실행은 하지 않는다).
    AutoDecisionReview {
        #[serde(default)]
        project: String,
        id: String,
        answer: Option<String>,
    },
    /// 자동 결정 기록을 host 원장에서 지운다(모든 기기). `all`이면 모두. 이미 실행한 일은 되돌리지 않는다.
    AutoDecisionForget {
        #[serde(default)]
        project: String,
        #[serde(default)]
        ids: Vec<String>,
        #[serde(default)]
        all: bool,
    },
    /// 사람이 읽는 질문 ID로 선택·직접 답을 기록한다. 일부만 답하면 나머지는 열린 채로 남는다.
    QuestionReply {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        id: String,
        choice: Option<String>,
        text: Option<String>,
    },
    /// 답한 것 기록의 답을 고친다(FRONT_API 1.191, t-2117). `base_at_ms`는 고치기 시작할 때 본 지금 답의 시각이다:
    /// 그사이 다른 기기가 고쳤으면 거절한다. 같은 thread·coordinator에 정정을 알리고 처음 답은 이력에 남긴다
    AskCorrect {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        id: String,
        #[ts(type = "number")]
        base_at_ms: u64,
        picks: Vec<Option<u32>>,
        text: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        #[ts(optional, as = "Option<Vec<Option<String>>>")]
        comments: Vec<Option<String>>,
    },
    /// 답한 것 기록을 지운다(모든 기기). `all`이면 모두
    AskForget {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        #[serde(default)]
        ids: Vec<String>,
        #[serde(default)]
        all: bool,
    },
    PostUnmute {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        thread: String,
    },
    Status,
    Machines,
    /// pane 목록. 기본은 평가자 세션을 빼고, `role`을 주면 그 역할만 보인다(§8.13).
    Panes {
        #[serde(default)]
        role: Option<Role>,
    },
    PaneRun {
        machine: Option<String>,
        name: Option<String>,
        argv: Vec<String>,
        cwd: Option<String>,
        cols: Option<u16>,
        rows: Option<u16>,
        /// host 정본 profile로 harness를 띄운다(`argv`는 비운다, herdr-web-ui 새 세션의 agent 고르기)
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        profile: Option<String>,
    },
    PaneRead {
        pane: String,
    },
    /// harness(Claude·Codex) 대화 기록 보기(§8.27, 읽기 전용)
    PaneChat {
        pane: String,
        /// 생각도 싣는다(웹 설정 「생각 표시」, plan §8.129)
        #[serde(default)]
        thinking: bool,
        /// 정리된 자리(`ChatRole::Compact`, plan §8.309, 1.61)도 싣는다. 옛 front는 생략해 그 줄을 받지 않는다(모르는 role로 답 전체를
        /// 읽지 못하므로)
        #[serde(default)]
        compact: bool,
        /// harness 명령과 결과(`ChatRole::Command`·`CommandOutput`, plan §8.323, 1.68)도 싣는다. 옛 front는 생략한다
        #[serde(default)]
        commands: bool,
        /// 이전 응답의 before. 생략하면 최신 쪽, 옛 상대는 무시한다.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        before: Option<String>,
    },
    /// 입력칸 「/」 목록(plan §8.323): 그 pane harness의 명령. agent가 `commands.v1`을 알릴 때만, 조작 기기만
    PaneCommands {
        pane: String,
    },
    /// 파일 보기(plan §8.125, 결정 20 D7): pane이 지금 있는 폴더 기준으로 경로를 찾고, 찾았으면 짧은 수명의
    /// 읽기 경로(`url`)를 준다. 조작 기기만(참고도 보기 전용 기기는 403).
    PaneFile {
        pane: String,
        path: String,
    },
    /// 보고서 안 파일 링크(plan §8.171): 그 thread의 머신에서 thread 작업 폴더(worktree) 기준으로 찾는다. 답은 `PaneFile`과
    /// 같다. 조작 기기만(`PaneFile`과 같다)
    ThreadFile {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        thread: String,
        path: String,
    },
    /// FRONT_API 1.59. thread worktree의 바뀐 파일과 patch(plan §8.305): 그 thread의 머신 agent가 읽는다(diff.v1). 소스를
    /// 보이므로 조작 기기만(`ThreadFile`과 같다)
    ThreadDiff {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        thread: String,
        #[serde(default)]
        #[ts(optional, as = "Option<crate::DiffQuery>")]
        query: crate::DiffQuery,
    },
    /// 폴더 보기(파일 보기 안, 참고 `DirectoryBrowser`): pane이 지금 있는 폴더 기준으로 폴더 한 단계를 읽는다. 허용 폴더 안만,
    /// 조작 기기만(`PaneFile`과 같다)
    PaneDir {
        pane: String,
        path: String,
        #[serde(default)]
        hidden: bool,
    },
    PaneSend {
        pane: String,
        data: B64,
    },
    /// 글자로 읽는다(herdr `pane read`). host가 가진 화면 사본에서 읽는다.
    PaneReadText {
        pane: String,
        #[serde(default)]
        source: ReadSource,
        /// 줄 수(기본: Recent 80줄, Visible은 화면 전부, 최대 1000)
        #[serde(default)]
        lines: Option<u32>,
        #[serde(default)]
        format: ReadFormat,
    },
    /// 글을 그대로 보낸다(herdr `pane send-text`: 감싸지 않고 Enter도 없다)
    PaneSendText {
        pane: String,
        text: String,
    },
    /// 키 이름(herdr 문법: `enter`·`esc`·`ctrl+c`·`shift+tab`·`up`·`f5`·글자 하나)을 그 프로그램이 켠 입력 모드(DECCKM 등)대로 보낸다.
    /// 모두 해석한 뒤에 보낸다(하나라도 모르면 아무것도 보내지 않는다).
    PaneSendKeys {
        pane: String,
        keys: Vec<String>,
    },
    /// 글 + Enter(herdr `pane run`): 프로그램이 bracketed paste를 켰으면 글을 감싸고 Enter와 한 번에 쓴다
    PaneSubmit {
        pane: String,
        text: String,
    },
    /// 출력에 글이 나올 때까지 기다린다(herdr `pane wait-output`). 화면 사본에 출력이 들어올 때마다 다시 본다.
    PaneWaitOutput {
        pane: String,
        #[serde(rename = "match")]
        matcher: OutputMatch,
        #[serde(default)]
        source: ReadSource,
        #[serde(default)]
        lines: Option<u32>,
        /// 없으면 끝없이 기다린다(herdr와 같다)
        #[serde(default)]
        #[ts(type = "number | null")]
        timeout_ms: Option<u64>,
    },
    /// pane 하나(ID·ID 앞부분·이름)
    PaneGet {
        pane: String,
    },
    /// pane을 닫는다(herdr `pane close`): 세션을 끝내고, 끝나기를 기다려 layout과 기록에서 지운다
    PaneClose {
        pane: String,
    },
    /// workspace 목록
    Workspaces,
    /// workspace를 닫는다: 그 안 pane을 끝내고 layout을 지운다. thread pane은 끝내지 않고 layout에서만 뺀다.
    WorkspaceClose {
        name: String,
    },
    WorkspaceRename {
        name: String,
        to: String,
    },
    PaneResize {
        pane: String,
        cols: u16,
        rows: u16,
    },
    PaneKill {
        pane: String,
    },
    /// 대화형 입력(웹 터미널). 조작 lease를 확인하고, 비어 있으면 이 연결이 갖는다.
    /// 한 번 보내는 `PaneSend`(CLI·API)는 lease 대상이 아니다(§4.5).
    PaneInput {
        pane: String,
        data: B64,
    },
    /// 웹의 Ctrl+Enter. host가 감지 입력줄로 steering 키를 정한다(줄바꿈 없음, 작업 중일 때만, 모르면 보내지 않음). 권한·입력 순서는 PaneInput과 같다.
    PaneCtrlEnter {
        pane: String,
        #[serde(default)]
        trace: Option<String>,
    },
    /// 대화형 붙여넣기(herdr client가 서버로 넘기는 붙여넣기와 같은 형식).
    /// host가 그 프로그램이 켠 bracketed paste대로 감싸고 ESC·C1 CSI를 뺀다(`KeyModes::paste`). lease는 `PaneInput`과 같다.
    PanePaste {
        pane: String,
        text: String,
    },
    /// 조작 lease를 이 연결로 가져온다(이전 보유자에게는 State로 알린다)
    PaneTake {
        pane: String,
    },
    PaneRelease {
        pane: String,
    },
    /// 붙여 넣은 그림 한 조각(`aaa pane attach`, 최대 512KiB). 웹 `TermClientMsg::Attach`와 같은 규칙이다:
    /// `offset` 0이 새 첨부의 시작이고, 마지막 조각에서 `sha256`(파일 전체)을 확인한다. 받는 중인 첨부는 연결마다 하나다.
    PaneAttach {
        pane: String,
        #[ts(type = "number")]
        offset: u64,
        #[ts(type = "number")]
        total: u64,
        sha256: String,
        data: B64,
    },
    /// 끝난 pane(중단·유실·종료)을 같은 spec으로 다시 띄우고 layout 자리를 바꾼다
    PaneResume {
        pane: String,
    },
    /// 끝난 pane 기록을 지운다
    PaneForget {
        pane: String,
    },
    /// pane에 이름을 붙인다(herdr `pane rename`). 앞뒤 공백을 떼고, `None`이나 빈 이름은 붙인 이름을 지워 띄울 때 이름으로 돌린다.
    PaneRename {
        pane: String,
        label: Option<String>,
    },
    Workspace {
        name: Option<String>,
    },
    LayoutApply {
        workspace: Option<String>,
        change: LayoutChange,
    },
    EnrollOpen {
        minutes: u32,
    },
    /// FRONT_API 1.80. 등록 창을 시한 전에 닫는다(대기 중인 요청은 그대로, 새 요청만 막는다)
    EnrollClose,
    /// FRONT_API 1.81(관리 GUI W5c). safety 실효값과 출처. `scope`가 global이면 모든 프로젝트 기본 표
    SafetyShow {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        #[serde(default)]
        scope: crate::SettingsScope,
    },
    /// FRONT_API 1.81. safety 값 하나를 바꾼다(웹은 yolo·start_threads·trust_screens·coordinator_deploy만). `value`가 없으면
    /// 그 표의 값을 지워 아래 층을 따른다. `expected`는 읽은 host.toml 판
    SafetySet {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        #[serde(default)]
        scope: crate::SettingsScope,
        key: String,
        value: Option<String>,
        expected: String,
    },
    /// FRONT_API 1.81. host.toml profile 목록(env는 이름만)과 판
    ProfileAdmin,
    /// FRONT_API 1.81. profile·허용 목록·기본값 바꾸기 미리보기(쓰지 않는다)
    ProfilePlan {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        change: crate::ProfileChangeReq,
    },
    /// FRONT_API 1.81. 미리 본 바꾸기를 쓴다. `expected`와 host.toml 판이 다르면 conflict
    ProfileApply {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        change: crate::ProfileChangeReq,
        expected: String,
    },
    /// FRONT_API 1.81. 머신 작업 root 보기(그 머신 agent의 work_root.manage.v1)
    WorkRootShow {
        machine: String,
    },
    /// FRONT_API 1.81. 작업 root 점검(검사 파일을 만들고 쓰기·읽기·삭제, 없는 root는 만든다). `path`가 없으면 지금 설정
    WorkRootCheck {
        machine: String,
        path: Option<String>,
        mount: Option<String>,
    },
    /// FRONT_API 1.81. 다음 agent 시작의 작업 root를 바꾼다(절대 경로, agent가 다시 점검한 뒤 저장)
    WorkRootSet {
        machine: String,
        path: String,
        mount: Option<String>,
    },
    /// FRONT_API 1.81. 그 머신 harness hook 연동 상태
    HookStatus {
        machine: String,
    },
    /// FRONT_API 1.81. 그 머신 agent의 고정 작업으로 Codex 전역 hook을 설치(true)·제거(false)한다
    HookSet {
        machine: String,
        install: bool,
    },
    /// FRONT_API 1.158. provider별 모델 목록(머신이 알린 것·끈 것·profile이 쓰는 것)과 켜짐
    ModelAdmin,
    /// FRONT_API 1.158. `harness`의 `models`를 켜거나(`enabled`) 끈다. `expected`와 host.toml 판이 다르면 conflict
    ModelSet {
        harness: String,
        models: Vec<String>,
        enabled: bool,
        expected: String,
    },
    EnrollPending,
    /// FRONT_API 1.93. SSH로 머신 추가 작업을 시작한다(웹 머신 추가 M2, plan §8.384): host의 OpenSSH로 접속해 host key만 받고, 아는 키면
    /// OS·CPU를 감지한다. 원격에 쓰지 않는다. 사람만(조작 기기·세션 밖)
    MachineSetupStart {
        target: String,
    },
    /// FRONT_API 1.93. 화면에서 본 host key 지문을 믿는다: host가 받은 키와 같으면 aaa 전용 known_hosts에 넣고 감지로 간다. 사람만
    MachineSetupTrust {
        job: String,
        host_key_sha256: String,
    },
    /// FRONT_API 1.93. 감지가 안 된 머신의 OS·CPU를 사람이 고른다(release 대상 다섯만). 사람만
    MachineSetupChoose {
        job: String,
        os: String,
        arch: String,
    },
    /// FRONT_API 1.93. 작업을 멈추고 목록에서 뺀다(원격 임시 폴더가 있으면 지운다). 사람만
    MachineSetupCancel {
        job: String,
    },
    /// FRONT_API 1.103. 준비된 작업에 설치 설정을 넣는다(M3, plan §8.420): host가 같은 판 aaa를 원격 임시 폴더에 보내 SHA·판을 확인하고
    /// 원격 `install-probe`로 계획을 받는다. 원격 설치 자리에는 아직 쓰지 않는다. 사람만
    MachineSetupPlan {
        job: String,
        settings: crate::MachineSetupSettings,
    },
    /// FRONT_API 1.103. 화면에서 본 계획 hash로 설치하고 등록한다: host가 그 계획에 서명하고, 원격 install이 적용하며 grant로 등록한다. 사람만
    MachineSetupApply {
        job: String,
        plan_sha256: String,
        /// FRONT_API 1.112. 사람의 웹 확인 창에서만 일회 전달한다. CLI·세션·저장·응답 금지.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        service_password: Option<MachineSetupPassword>,
    },
    /// FRONT_API 1.103. 설치·등록된 머신에 끝 확인 shell pane을 연다(작업 폴더·git·harness·PATH를 적는다, 모델 비용 없음). 사람만
    MachineSetupCheck {
        job: String,
    },
    /// FRONT_API 1.103. 설치·등록된 머신에 harness 로그인 pane(`claude`·`codex login`)을 연다. 사람만
    MachineSetupLogin {
        job: String,
        harness: crate::install::InstallHarness,
    },
    /// FRONT_API 1.93. 최근 SSH 추가 작업(Reader도 본다)
    MachineSetupList,
    /// FRONT_API 1.92. 새 머신에서 칠 「직접 설치」 명령에 넣을 host 주소 후보·host 지문·판(웹 머신 추가 M1, plan §8.383)
    EnrollGuide,
    EnrollApprove {
        name: String,
        code: String,
    },
    /// 등록 신원·연결을 유지하고 머신 이름과 참조를 함께 옮긴다.
    MachineRename {
        name: String,
        to: String,
    },
    MachineRevoke {
        name: String,
    },
    /// FRONT_API 1.148. 머신 독점 예약을 걸거나(같은 주인이면 고친다) 푼다. 사용자, 또는 `user_request`를 준 그 프로젝트
    /// coordinator만. thread는 자기 머신을 `aaa machine reserve`(post 길)로 잡는다
    MachineReserve {
        machine: String,
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        /// `t-NNNN` thread 또는 할 일(`d-NNNN`·ULID·제목)
        holder: String,
        #[serde(default)]
        #[ts(optional)]
        note: Option<String>,
        #[serde(default)]
        #[ts(optional, type = "number")]
        until_ms: Option<u64>,
        #[serde(default)]
        #[ts(optional)]
        user_request: Option<String>,
    },
    MachineRelease {
        machine: String,
        #[serde(default)]
        #[ts(optional)]
        user_request: Option<String>,
    },
    /// FRONT_API 1.148. 자동 배정 비중(사람만). None은 CPU 수로 되돌린다
    MachineWeightSet {
        machine: String,
        weight: Option<u32>,
    },
    /// pairing 코드를 만든다. 아직 쓰지 않은 앞 코드는 버린다(herdr-web-ui `startPairing`). 웹 기기는 controller 코드만(D6)
    WebPair {
        role: DeviceRole,
    },
    /// pairing한 웹 기기 목록
    Devices,
    /// 기기 이름을 바꾼다
    DeviceRename {
        id: String,
        label: String,
    },
    /// 기기를 취소한다. 그 기기의 열린 연결은 바로 닫히고 push 구독도 지워진다
    DeviceRevoke {
        id: String,
    },
    /// W0와 별개로, 실제 승인된 같은 머신 worker의 한 포트 범위만 검증한다.
    InstallPublish {
        installation_id: String,
    },
    InstallPublishStatus {
        installation_id: String,
    },
    WebPublish,
    /// 공개 포트를 바꾸고 저장한 링크와 푸시 구독을 이전한다(사람의 host CLI).
    WebPublishPort {
        port: u16,
    },
    WebPublishOff,
    WebStatus,
    /// 프로젝트 폴더를 만든다(herdr-projects `new`). 준 값만 project.toml에 적는다(FRONT_API 1.33)
    ProjectCreate {
        name: String,
        #[serde(default)]
        goal: String,
        /// `PATH` 또는 `PATH@MACHINE`(머신이 없으면 host 머신, 상대 경로는 CLI가 절대 경로로 바꿔 보낸다)
        #[serde(default)]
        repos: Vec<String>,
        #[serde(default)]
        thread_profile: Option<String>,
        #[serde(default)]
        coordinator_profile: Option<String>,
    },
    /// project.toml 값 하나를 바꾼다(herdr-projects `set`, FRONT_API 1.33). key는 `aaa project set --help`
    ProjectSet {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        key: String,
        value: String,
    },
    /// 프로젝트 목록. archived는 `all`일 때만(herdr-projects `list --all`, FRONT_API 1.33)
    Projects {
        #[serde(default)]
        #[ts(optional, as = "Option<bool>")]
        all: bool,
    },
    /// 이 host가 맡은 프로젝트 하나(보관하지 않은 것, FRONT_API 1.50, plan §8.288). 아직 없으면 `project`가 없다
    Instance,
    /// 멈추기·되살리기·보관·보관 풀기(herdr-projects `pause`·`resume`·`archive`·`unarchive`, FRONT_API 1.33)
    ProjectLifecycle {
        project: String,
        action: ProjectAction,
    },
    /// 프로젝트 폴더를 휴지통(`<projects root>/.trash`)으로 옮기고 host 기록을 그 안 `aaa-records.json`으로 뺀다.
    /// worktree·branch는 건드리지 않는다(herdr-projects `delete`)
    ProjectDelete {
        project: String,
        /// 살아 있는 pane이 있어도
        #[serde(default)]
        force: bool,
    },
    /// 프로젝트 이름(폴더 이름)을 바꾼다(herdr-projects `rename`). 끝나지 않은 thread나 도는 coordinator가 있으면 거부한다
    ProjectRename {
        project: String,
        to: String,
        #[serde(default)]
        dry_run: bool,
    },
    /// 끝난 thread가 남긴 worktree·branch를 그 머신 agent가 다시 정리한다(herdr-projects `sweep`)
    ProjectSweep {
        project: String,
        #[serde(default)]
        dry_run: bool,
    },
    /// 모든 프로젝트 폴더의 aaa 파일(coordinator 안내 AGENTS.md·uploads/)을 본다.
    /// `fix`면 고친다: 표지 있는 AGENTS.md는 지금 안내로 다시 쓰고, 표지 없는 것은 `.before-aaa`로 옮겨 둔다
    /// (herdr-projects `doctor [--fix]`, FRONT_API 1.34)
    ProjectFiles {
        #[serde(default)]
        fix: bool,
    },
    /// coordinator를 연다(herdr-projects `open`): 도는 coordinator가 있으면 그것을 알려 주고, 없으면(또는 `new`) host 머신에서
    /// 프로젝트 폴더를 작업 폴더로 profile을 띄워 프로젝트 이름 workspace의 새 탭에 둔다(FRONT_API 1.33)
    ProjectOpen {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        /// 안 주면 project.toml `coordinator_profile` → host.toml `[defaults] coordinator_profile` → claude
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        profile: Option<String>,
        /// coordinator가 돌아도 하나 더 띄운다
        #[serde(default)]
        #[ts(optional, as = "Option<bool>")]
        new: bool,
    },
    /// host 정본 profile과 그 머신에 설치·로그인한 기본 harness 목록
    Profiles {
        machine: Option<String>,
    },
    /// host 정본의 profile을 확인한다(설치 여부와 무관, 기본값도 host에서 고른다).
    ProfileResolve {
        name: Option<String>,
    },
    ThreadStart {
        /// FRONT_API 1.98. 사용자가 명시한 시작만 프로젝트 실행 한도를 넘길 수 있다.
        #[serde(default)]
        #[ts(optional, as = "Option<bool>")]
        over_limit: bool,
        /// coordinator는 해당 task를 강제로 시작하라는 사용자 지시 근거를 한 줄로 남긴다.
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        user_request: Option<String>,
        /// FRONT_API 1.148. 다른 일이 독점 예약한 머신에도 시작한다. 사용자, 또는 `user_request`를 준 coordinator만
        #[serde(default)]
        #[ts(optional, as = "Option<bool>")]
        ignore_reservation: bool,
        #[serde(default)]
        holds: Vec<String>,
        /// 준비 완료를 기다리지 않고 Starting 기록을 돌려준다.
        #[serde(default)]
        background: bool,
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        title: String,
        machine: Option<String>,
        /// 비면 `from_task`의 owner, 그다음 project.toml `thread_profile`(herdr-projects `thread_profile`)
        profile: String,
        /// 없으면 repo가 있으면 worktree, 없으면 dir(herdr-projects `placement`)
        #[serde(default)]
        kind: Option<ThreadKind>,
        repo: Option<String>,
        base: Option<String>,
        /// 할 일(threads/<id>.task.md로 저장하고 brief에 넣는다)
        task: String,
        /// TASKS.md의 task ID(d-0001·ULID) 또는 제목(대소문자 무시). owner가 profile·machine의 기본값이 되고, 메모가 할 일에 붙는다.
        #[serde(default)]
        from_task: Option<String>,
        /// 호환 옵션(FRONT_API 1.63). 일반 기동은 생략해도 할 일을 먼저 기록하고 연결한다(plan §8.430).
        /// from_task는 읽은 블록 hash로 연결하며, 달라졌으면 pane을 띄우지 않는다. 자동화 do.start는 제외한다.
        #[serde(default)]
        #[ts(optional, as = "Option<bool>")]
        link_task: bool,
        /// 화면이 본 `from_task`의 블록 hash(FRONT_API 1.63). 지금 파일의 블록과 다르면 시작하지 않는다(본 것과 다른 일을 맡기지 않게)
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        task_block: Option<String>,
    },
    /// 할 일 「바로 시작」 확인창이 보일 것(FRONT_API 1.63, 조작 권한): `from_task`로 시작하면 쓸 제목·profile·머신·할 일 글과
    /// 저장소 고르기, 열린 thread 수와 한도. 아무것도 시작하지 않는다
    ThreadStartPreview {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        from_task: String,
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        task_block: Option<String>,
    },
    /// 사람이 이미 띄운 agent pane(Claude·Codex)을 이 프로젝트의 thread로 기록하고 brief를 안내한다(herdr-projects
    /// `thread adopt`). pane은 그대로 두고 그 폴더에 작업 폴더를 만든다
    ThreadAdopt {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        pane: String,
        title: String,
        /// 할 일. 비면 「하던 일을 이어 하되 이제 이 프로젝트 thread다」
        #[serde(default)]
        task: String,
    },
    /// thread의 Next 목록을 보이거나(`line`·`add` 없음), N번째 줄을 thread에 메시지로 보내거나, 줄을 더한다
    ThreadNext {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        thread: String,
        line: Option<u32>,
        add: Option<String>,
    },
    /// 완료 보고의 후속을 같은 프로젝트의 할 일에 잇거나 사유를 남겨 버린다.
    ThreadFollowup {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        thread: String,
        id: String,
        task: Option<String>,
        reason: Option<String>,
    },
    /// coordinator용 프로젝트 요약. 보인 inbox 항목은 「봤다」가 된다(`peek`이면 그대로).
    Context {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        #[serde(default)]
        peek: bool,
    },
    /// thread를 맡길 수 있는 owner 목록. `check`를 주면 그 owner를 TASKS.md에 써도 되는지 본다.
    Assignable {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        check: Option<String>,
    },
    /// 할 일 화면이 읽는 TASKS.md 전체(FRONT_API 1.60, Reader 가능). 형식 어긋남·줄 번호·블록 hash를 함께 준다.
    Tasks {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
    },
    /// 예약 읽기·등록/내용 갱신·취소·순서 변경. 등록은 해당 내용의 실행 승인이다.
    TaskQueue {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        action: crate::QueueOp,
    },
    TaskArchive {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        /// 비면 보기, 있으면 같은 ID로 활성 목록에 복원(예약은 복원하지 않는다).
        restore: Option<String>,
        /// FRONT_API 1.106: 검색·저장소·상태를 host에서 거른 뒤 바이트 상한 안의 페이지로 준다.
        #[serde(default)]
        #[ts(optional)]
        view: Option<crate::TaskArchiveQuery>,
    },
    /// 할 일 편집(FRONT_API 1.62, 조작 권한). 동작을 차례로 적용해 한 번에 쓴다. 세션 안에서는 그 프로젝트 coordinator만 된다.
    /// `base_sha256`을 주면 파일 전체가 그 판일 때만 쓴다. 답은 `task_edited`(충돌이면 `conflict`, 쓰지 않음)
    TaskEdit {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        ops: Vec<TaskOp>,
        #[serde(default)]
        base_sha256: Option<String>,
        /// 사용자 지시로 모델·머신만 변경하는 근거 한 줄(FRONT_API 1.97). 예약 순서와 시작 승인을 유지한다.
        #[serde(default)]
        #[ts(optional)]
        user_request: Option<String>,
    },
    /// FRONT_API 1.160: 확인 카드에서 직접 완료 보관·재배정 예약·연결 해제. 본 task 블록과 보고 판이 같아야 처리한다.
    TaskAttentionAnswer {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        at: crate::TaskAnchor,
        report_hash: Option<String>,
        action: crate::TaskAttentionAction,
    },
    /// 할 일 기록(FRONT_API 1.62, 보기 권한). 최신부터, `before`(id)보다 앞의 것
    TaskLog {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        #[serde(default)]
        #[ts(type = "number | null")]
        before: Option<i64>,
        #[serde(default)]
        limit: Option<u32>,
        #[serde(default)]
        filter: Option<TaskLogFilter>,
        /// 표시 ID·ULID 또는 제목으로 기록을 거른다.
        #[serde(default)]
        #[ts(optional)]
        task: Option<String>,
        #[serde(default)]
        #[ts(optional)]
        search: Option<crate::TaskHistorySearch>,
    },
    /// 기획 트리(FRONT_API 1.170, 보기 권한): 프로젝트 저장소마다 기획 폴더의 문서 목록. 기획 폴더를 정하지 않은 저장소도 싣는다
    DesignTree {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        /// 저장소 하나만 읽는다(FRONT_API 1.182). 없으면 종전처럼 전부 읽는다
        #[serde(default)]
        #[ts(optional)]
        repo: Option<String>,
        /// agent를 기다리지 않고 설정·host가 가진 마지막 트리만 돌려준다(FRONT_API 1.182)
        #[serde(default)]
        #[ts(optional, as = "Option<bool>")]
        cached: bool,
    },
    /// 지운 옛 아이디어의 짧은 ID(`n-NNNN`) → 옮긴 기획 문서(FRONT_API 1.178, t-1959, 보기 권한). 프로젝트 `idea-links.toml`에서
    /// 읽고, 기획 폴더가 있는 프로젝트 저장소의 그 폴더 안 문서만 싣는다. 웹은 글 속 `n-NNNN`을 이 문서로 잇고, 표에 없으면 글자로 둔다
    IdeaLinks {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
    },
    /// 기획 문서 하나(FRONT_API 1.170, 보기 권한). `path`는 저장소 기준 경로이고 기획 폴더 안이어야 한다
    DesignRead {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        repo: String,
        path: String,
        /// 옛 기획 branch 이름(FRONT_API 1.171). t-2002부터 읽지 않는다(늘 main)
        #[serde(default)]
        #[ts(optional)]
        branch: Option<String>,
        /// 이 main 판(commit)에서 읽는다(FRONT_API 1.186, 문서 이력). 없으면 host가 기억한 main 트리의 판
        #[serde(default)]
        #[ts(optional)]
        at: Option<String>,
    },
    /// 기획 대화 열기(FRONT_API 1.171, 조작): 대화 기록을 만들고(있으면 그대로) 저장소 머신의 main 기획 자리를 맞춘다(t-2002, branch 없음)
    DesignBranchOpen {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        repo: String,
        slug: String,
        /// 주제(없으면 slug). 이미 있는 branch는 바꾸지 않는다
        #[serde(default)]
        #[ts(optional)]
        title: Option<String>,
        /// 대화 thread profile(FRONT_API 1.173, 없으면 프로젝트 기본·있던 고르기)
        #[serde(default)]
        #[ts(optional)]
        profile: Option<String>,
    },
    /// 기획 문서 저장(FRONT_API 1.171, 조작): 문서(와 `extra` 파일)를 한 commit으로 origin main에 바로 올린다(t-2002, FRONT_API
    /// 1.186). `base`는 읽은 blob(새 문서는 없음)이고 그 사이 다른 판이 생겼으면 conflict다. 저장소 문서 검사를 통과하지 못하면
    /// 올리지 않고 그 까닭을 돌려준다. `branch`는 사건을 남길 기획 대화(주제, 비우면 없음: 대화 thread는 자기 대화)다
    DesignSave {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        repo: String,
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        branch: String,
        path: String,
        #[serde(default)]
        #[ts(optional)]
        base: Option<String>,
        text: String,
        /// commit 메시지의 요약(없으면 「<경로> 고침」)
        #[serde(default)]
        #[ts(optional)]
        summary: Option<String>,
        /// 같은 commit에 함께 저장할 다른 글 파일(FRONT_API 1.186): 서로 링크하는 문서·문서 옮기기
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        #[ts(optional, as = "Option<Vec<crate::DesignSaveFile>>")]
        extra: Vec<crate::DesignSaveFile>,
    },
    /// 기획 대화 thread가 PNG·JPEG·GIF·WebP를 main에 붙인다(FRONT_API 1.184, t-2002부터 main).
    /// 조각마다 `ATTACH_CHUNK` 이하, 중간 응답은 `Done`, 마지막은 `DesignSaved`다.
    DesignAttach {
        project: String,
        repo: String,
        #[serde(default)]
        branch: String,
        /// 파일 이름(FRONT_API 1.186, 확장자는 내용과 같아야 한다). 없으면 내용 hash 이름
        #[serde(default)]
        #[ts(optional)]
        name: Option<String>,
        /// 저장소 기준 폴더. 생략하면 저장소의 기획 폴더
        #[serde(default)]
        #[ts(optional)]
        dir: Option<String>,
        upload: String,
        offset: u64,
        total: u64,
        data: B64,
    },
    /// 기획 대화 thread가 main에서 그림 하나를 지운다(FRONT_API 1.185, t-2002부터 main).
    DesignDetach {
        project: String,
        repo: String,
        #[serde(default)]
        branch: String,
        path: String,
    },
    /// 기획 대화에 말하기(FRONT_API 1.172, 조작): 그 기획 branch의 대화 thread에 보낸다. thread가 없거나 끝났으면 그 저장소 머신에
    /// 새로 띄우고, 쉬느라 내린 것은 잇고, 쉬는 branch는 다시 연다. 말은 thread가 brief를 받은 뒤 차례로 간다
    DesignSay {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        repo: String,
        branch: String,
        text: String,
    },
    /// 기획 대화 끝내기(FRONT_API 1.171, 조작, t-2002부터): 대화 thread를 끝내고 대화 기록을 지운다. main에 저장한 글은 그대로다
    DesignDiscard {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        repo: String,
        branch: String,
    },
    /// 「main에 반영」(FRONT_API 1.173): t-2002(FRONT_API 1.186)부터 없어졌다. 기획은 저장하면 바로 main에 들어가고, host는
    /// 이 요청을 까닭과 함께 거절한다(옛 front)
    DesignApply {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        repo: String,
        branch: String,
    },
    /// 「main에 반영」 예약 취소(FRONT_API 1.178): t-2002부터 없어져 거절한다
    DesignApplyCancel {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        repo: String,
        branch: String,
    },
    /// 문서 이력(FRONT_API 1.186, 보기 권한, t-2002): main이 실제로 가졌던 판(`--first-parent`)을 새것부터
    DesignLog {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        repo: String,
        path: String,
    },
    /// 이 판으로 되돌리기(FRONT_API 1.186, 조작·사람만, t-2002): 문서의 옛 main 판(`at`) 글을 지금 판(`base`) 위에 새로 저장한다.
    /// 지금 판의 넘긴 블록 표지(`[!계획] d-NNNN`)는 옛 판의 같은 블록에 옮겨 넣고, 그 블록이 옛 판에 없으면 거절한다
    DesignRevert {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        repo: String,
        path: String,
        at: String,
        base: String,
        /// 사건을 남길 기획 대화(비우면 없음)
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        branch: String,
    },
    /// 기획서 ID 목록(FRONT_API 1.181, 보기 권한, t-1957): 지금 main 기획 트리에 있는 문서의 `k-NNNN`. host 원장에서 읽는다(agent를
    /// 거치지 않는다)
    DesignIds {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
    },
    /// 기획서 ID 한 번에 달기(FRONT_API 1.181, 조작·사람만, t-1957): main 기획 트리에서 표지가 없는 문서마다 원장 번호의 표지를
    /// main에 한 commit으로 넣는다(t-2002, 저장소 문서 검사를 지난다)
    DesignMarkIds {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        repo: String,
    },
    /// 구현 맡기기(FRONT_API 1.175, 조작, t-1894 3-2): 기획 branch 판 문서의 계획 블록 여럿을 할 일 하나로 넘긴다.
    /// 할 일을 만들고 고른 블록 표지에 그 d-ID를 적어 그 branch에 commit해 바로 올린다. 그 할 일의 구현 thread는
    /// `origin/design/<주제>`에서 갈라진 worktree에서 일한다. `reserve`면 예약한다(자리가 나면 host가 띄운다). 답은 `design_handoff`
    DesignHandoff {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        repo: String,
        /// 기획 branch 주제. 비면 main 문서에서 바로 맡긴다(FRONT_API 1.176): 표지는 구현 branch 첫 commit에 적는다
        branch: String,
        /// 저장소 기준 문서 경로
        path: String,
        /// 고를 때 읽은 판(blob). 그 사이 바뀌었으면 쓰지 않는다
        blob: String,
        /// 고른 표지 블록(차례와 첫 줄. 차례는 `design_marks`와 웹 `calloutOf`가 같다)
        blocks: Vec<crate::DesignPick>,
        title: String,
        /// 모델(profile). 비우면 프로젝트 기본
        #[serde(default)]
        profile: Option<String>,
        /// 할 일 메모(여러 줄)
        memo: String,
        reserve: bool,
    },
    /// 이 시각(ms)보다 오래된 할 일 기록을 지운다(FRONT_API 1.62, 조작 권한)
    TaskLogPrune {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        #[ts(type = "number")]
        before_ms: u64,
    },
    /// thread 목록. role 생략은 모든 역할. FRONT_API 1.130부터 커서·개수·바이트 상한을 둔다.
    Threads {
        project: Option<String>,
        #[serde(default)]
        role: Option<Role>,
        #[serde(flatten)]
        query: crate::ThreadListQuery,
    },
    ThreadShow {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        thread: String,
    },
    /// FRONT_API 1.44. thread 작업 요약(plan §8.259)
    ThreadSummary {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        thread: String,
    },
    /// FRONT_API 1.44. 작업 요약을 통째로 다시 쓴다(조작 권한, coordinator가 `thread summary --file`로)
    ThreadSummarySet {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        thread: String,
        text: String,
    },
    /// 보고서와 결과물 목록, 그리고 결과물을 볼 짧은 수명의 읽기 전용 경로
    ThreadLibrary {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        thread: String,
    },
    /// FRONT_API 1.36. 보고서별 링크 발급(유효한 링크는 재사용). 기본 30일, 조작 권한만.
    LibraryLink {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        thread: String,
        #[serde(default)]
        days: Option<u32>,
    },
    /// FRONT_API 1.36. 서명 경로를 담으므로 목록도 조작 권한만.
    LibraryLinks {
        project: Option<String>,
    },
    /// FRONT_API 1.36. 보고서 링크 폐기.
    LibraryLinkRevoke {
        id: String,
    },
    /// FRONT_API 1.38. host 머신의 CLI만 로컬 파일을 읽는다. live는 복사 없이 등록한다.
    LibraryPublish {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        name: String,
        source: String,
        #[serde(default)]
        live: bool,
        /// FRONT_API 1.43. 사용자에게 보내는 웹 보고서(index.html 필수, 「내 보고서」·안 봄·사용자 알림 post)
        #[serde(default)]
        to_user: bool,
        /// FRONT_API 1.43. 「내 보고서」에 보일 제목(없으면 이름)
        #[serde(default)]
        title: Option<String>,
        /// FRONT_API 1.56. 보고서를 낸 thread(`t-NNNN`). 없으면 host가 원본 경로·이름에서 그 프로젝트의 thread를 찾는다
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        from_thread: Option<String>,
    },
    /// FRONT_API 1.114. 현재 판 또는 지정한 r 판을 보관함에 넣거나 뺀다. 사본을 추가하지 않는다.
    LibraryArchive {
        reports: Vec<LibraryRef>,
        archived: bool,
    },
    /// FRONT_API 1.119. 확인한 목록의 안 본 보고서를 한 번에 읽음 처리한다. Reader도 기존 열람과 같은 권한이다.
    LibraryMarkRead {
        reports: Vec<LibraryRef>,
    },
    /// FRONT_API 1.43. 모든 판의 host 사본·목록·공유 링크·thread 보고를 지운다. 보관함도 명시 삭제한다.
    /// 진행 중 thread·완료 할 일 참조 보호는 유지한다.
    LibraryDelete {
        reports: Vec<LibraryRef>,
    },
    LibraryImport {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        source: String,
        /// 0.2의 t- 이름을 새 thread id와 구분해 가져올 때 쓴다.
        #[serde(default)]
        prefix: String,
    },
    LibraryPrune {
        project: Option<String>,
        older_than_days: u32,
    },
    /// FRONT_API 1.40. 별도 요청이라 옛 host가 dry-run을 삭제 명령으로 해석할 수 없다.
    LibraryPrunePreview {
        project: Option<String>,
        older_than_days: Option<u32>,
    },
    LibraryRotate,
    /// FRONT_API 1.80. 웹으로 올린 파일 묶음(`POST /api/v1/projects/<p>/library-uploads/<upload>?path=…`)을 보고서 하나로 등록한다
    /// (관리 GUI W5b). 등록하면 올린 자리는 지운다. 사용자 보고서(`to_user`)는 맨 위 index.html이 있어야 한다.
    /// `files`는 웹이 올린 파일 수다: 올린 자리의 파일 수가 다르면(오래돼 치워진 뒤 나머지만 올라온 경우) `conflict`로 거부한다
    LibraryUploadPublish {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        upload: String,
        name: String,
        files: u32,
        #[serde(default)]
        to_user: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        title: Option<String>,
    },
    /// host에 보관된 완료 보고·답한 질문·할 일의 과거 판까지 검색한다.
    RecordSearch {
        query: crate::RecordSearchQuery,
    },
    /// 검색에서 반환한 프로젝트와 내용 해시로 당시 본문을 연다.
    RecordOpen {
        project: String,
        key: String,
        #[serde(default)]
        #[ts(optional, type = "number")]
        offset: Option<u64>,
    },
    /// 링크는 조작 권한에만 싣는다. 앱 안 열기는 읽기 권한도 된다.
    LibraryReports {
        project: Option<String>,
    },
    /// FRONT_API 1.146: manifest 안 파일을 64KiB씩 읽는다.
    LibraryRead {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        name: String,
        path: String,
        #[ts(type = "number")]
        offset: u64,
    },
    LibraryOpen {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        name: String,
    },
    /// FRONT_API 1.71. 보고서 보기 주소(`/v/<cap>/…`)의 cap이 이 프로젝트의 어느 보고서인지(task 첨부가 붙인 주소를 앱 안
    /// 보고서 문서로 바꾼다). 조작 권한만
    LibraryResolve {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        cap: String,
    },
    LibraryUsage,
    /// FRONT_API 1.168. host 전체 보고서 보존 설정(조작 권한).
    LibraryRetention,
    /// FRONT_API 1.168. 사람이 본 host.toml 판에만 저장한다. 저장 자체는 삭제하지 않는다.
    LibraryRetentionSet {
        #[ts(type = "number")]
        max_days: u64,
        #[ts(type = "number")]
        max_mb: u64,
        expected: String,
    },
    /// 지금 보고를 확인했다
    ThreadAck {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        thread: String,
    },
    /// 끝낸다: 마지막 사본(실패면 거부) → 세션 종료 → worktree 정리(force 없음). 사본에 빠진 것이 있으면 worktree를 남긴다
    ThreadResolve {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        thread: String,
        #[serde(default)]
        keep_worktree: bool,
        /// 끝낸 thread를 다시 연다(herdr-projects `thread resolve --reopen`). 아무것도 띄우지 않는다
        #[serde(default)]
        reopen: bool,
        /// 마지막 사본 없이 끝낸다(사본을 만들 수 없을 때, worktree는 남긴다. herdr-projects `--skip-copy`)
        #[serde(default)]
        skip_copy: bool,
        /// 집에 다 오지 않았어도 worktree를 지운다(herdr-projects `--discard-uncopied`)
        #[serde(default)]
        discard_uncopied: bool,
    },
    /// FRONT_API 1.105(plan §8.426). 보류: 기본은 thread에게 정리(커밋·push·`## Handoff` 보고 `[보류 준비됨]`)를 맡기고 그 보고가
    /// 오면 pane을 닫는다. `now`면 바로 닫는다. 자리를 돌려주고 worktree·대화 ID·보고는 남긴다. `cancel`은 맡긴 정리를 거둔다.
    /// 사용자(조작 기기·세션 밖 CLI)와, `user_request`를 인용한 그 프로젝트 coordinator만
    ThreadPark {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        thread: String,
        #[serde(default)]
        #[ts(optional, as = "Option<bool>")]
        now: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        note: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        user_request: Option<String>,
        #[serde(default)]
        #[ts(optional, as = "Option<bool>")]
        cancel: bool,
    },
    /// FRONT_API 1.143. 할 일(`d-0001`·ULID·제목) 또는 thread(`t-0001`, 이어진 할 일)의 자동 병합을 켜고 끈다. 값이 같으면 바꾸지 않는다.
    /// 사용자(조작 기기·세션 밖 CLI)와, `user_request`를 인용한 그 프로젝트 coordinator만. 답은 `auto_merge`
    AutoMergeSet {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        target: String,
        on: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        user_request: Option<String>,
    },
    /// FRONT_API 1.105. 보류를 풀고 이어 한다. 같은 머신(기본)은 같은 대화를 잇고, 다른 머신은 push된 branch로 새 worktree를 만들고
    /// 할 일·마지막 보고(Handoff)를 담은 brief로 새 대화를 띄운다. `dry_run`은 머신별 길만 답한다(`UnparkPreview`)
    ThreadUnpark {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        thread: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        machine: Option<String>,
        /// 다른 머신의 저장소 경로(같은 이름의 저장소가 하나뿐이면 생략)
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        repo: Option<String>,
        #[serde(default)]
        #[ts(optional, as = "Option<bool>")]
        dry_run: bool,
    },
    /// thread agent를 멈춘다(Esc 한 번). 세션·작업 폴더는 그대로다
    ThreadStop {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        thread: String,
    },
    /// 끝난 thread 세션을 다시 띄운다. 기본은 task를 다시 읽어 새 brief로 새 대화, `resume`이면 저장된 대화를 잇는다
    ThreadRestart {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        thread: String,
        #[serde(default)]
        resume: bool,
        /// 이 profile로 띄운다. 다른 harness는 새 thread를 만들며 대화를 이을 수 없다.
        #[serde(default)]
        profile: Option<String>,
        /// FRONT_API 1.195. 같은 프로젝트 coordinator가 채팅의 직접 지시를 인용한다.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        user_request: Option<String>,
        /// FRONT_API 1.195. 모델을 바꾸기 전에 실행 중인 pane 종료를 허용한다.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        #[ts(optional, as = "Option<bool>")]
        stop_running: bool,
    },
    /// 사람이 명시적으로 brief 안내를 다시 보낸다(자동 재전송은 하지 않는다)
    ThreadBrief {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        thread: String,
    },
    /// thread pane이 지금 보이는 것(신뢰 창·질문 메뉴·권한 확인창, herdr-projects `thread read`). `lines`면 보이는 화면 대신
    /// 기록 끝 N줄
    ThreadRead {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        thread: String,
        #[serde(default)]
        lines: Option<u32>,
    },
    /// thread pane이 보이는 것에 답한다(herdr-projects `thread keys`): `text`를 치고(Enter 없음) `keys`를 누른다
    ThreadKeys {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        thread: String,
        #[serde(default)]
        keys: Vec<String>,
        #[serde(default)]
        text: Option<String>,
    },
    /// 열린 확인창 질문(기본은 열린 것만)
    Questions {
        #[serde(default)]
        all: bool,
    },
    /// 질문에 답한다. host가 질문이 아직 열려 있는지 다시 확인한다. `choice`(예·아니오) 대신 `pick`이면 화면 선택 창
    /// `menu`(그 id)에 답한다(plan §8.172)
    QuestionAnswer {
        id: String,
        #[serde(default)]
        choice: String,
        #[serde(default)]
        menu: Option<String>,
        #[serde(default)]
        pick: Option<MenuPick>,
    },
    /// thread agent에 메시지를 보낸다(전달을 모르면 다시 보내지 않는다)
    ThreadMessage {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        thread: String,
        text: String,
        #[serde(default)]
        mode: MessageMode,
    },
    /// thread의 `## Questions`에 답한다(FRONT_API 1.72, plan §8.329). host가 지금 보고의 질문인지·아직 답하지 않았는지 다시 보고,
    /// 「1A 2B」 꼴 글을 `ThreadMessage`와 같은 길로 thread에 보낸 뒤 답을 기록한다(모든 기기가 「답함」을 본다)
    ThreadAnswer {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        thread: String,
        /// 답하는 질문의 보고 hash(`ThreadQuestions.report_hash`)
        report_hash: String,
        /// 질문마다 고른 선택지(0부터). 직접 쓴 답으로 대신하는 질문은 null
        picks: Vec<Option<u32>>,
        #[serde(default)]
        text: Option<String>,
        /// 질문 배열 순서의 직접 답·의견. 고르지 않은 질문은 자기 의견이 필요하다.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        #[ts(optional, as = "Option<Vec<Option<String>>>")]
        comments: Vec<Option<String>>,
    },
    /// 이 host의 웹 화면들이 함께 쓰는 설정과 고정(herdr-web-ui `GET /api/preferences`)
    Preferences,
    /// 공유 설정을 바꾼다: 바꾼 설정만, 고정은 바뀌었을 때만(herdr-web-ui `PUT /api/preferences`). 보기 권한은 못 한다
    PreferencesSet {
        #[ts(type = "Record<string, unknown>")]
        settings: BTreeMap<String, serde_json::Value>,
        #[serde(default)]
        pins: Option<Vec<String>>,
    },
    /// inbox. `peek`가 아니면 보인 항목을 「봤다」로 표시한다(처음 보는 항목만 coordinator에게 알린다)
    Inbox {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        #[serde(default)]
        all: bool,
        #[serde(default)]
        peek: bool,
    },
    /// coordinator hook(PostToolUse)이 묻는다: 이 pane에 아직 알리지 않은 처음 보는 inbox 항목 한 줄(t-1627, FRONT_API 1.165).
    /// 알린 항목은 턴 끝 알림(nudge)이 다시 보내지 않는다. 그 프로젝트 coordinator pane이 아니면 빈 답이다.
    InboxHookNotice {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        pane: String,
    },
    /// inbox 항목을 끝낸다(`all`이면 끝내지 않은 것 모두)
    InboxDone {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        #[serde(default)]
        ids: Vec<String>,
        #[serde(default)]
        all: bool,
    },
    /// ticker를 지금 한 번 돌리고, 이 요청 뒤에 시작한 tick이 끝나면 답한다(herdr-projects 시험의 `ticker::tick_project`,
    /// t3code worker `drain`과 같은 자리). host가 「봤는데 할 일이 없었다」를 시험이 기다릴 수 있다(docs/testing.md).
    TickerTick,
    /// 특수 임무 목록과 적용 판. Reader도 볼 수 있다.
    MissionList {
        project: Option<String>,
    },
    MissionShow {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        name: String,
    },
    /// 사람만 승인한다. Controller 웹 기기에서도 가능하며 세션 안 연결은 거부한다.
    MissionApprove {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        name: String,
        approval_sha256: String,
    },
    MissionReject {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        name: String,
    },
    MissionStart {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        name: String,
    },
    MissionStop {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        name: String,
    },
    MissionPause {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        name: String,
    },
    MissionResume {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        name: String,
    },
    MissionReload {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        name: String,
    },
    MissionRestart {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        name: String,
    },
    MissionOpen {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        name: String,
    },
    /// routine 목록(`projects/<p>/routines/*.md`)과 상태
    Routines {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
    },
    /// 옛 routine·임무 파일을 값 검증 뒤 순수 TOML로 변환한다(Controller).
    AutomationApprove {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        name: String,
        approval_sha256: String,
    },
    /// `dry_run`이면 쓰지 않고 계획만 답한다(FRONT_API 1.58, Reader 가능). 옛 host는 그 칸을 버리고 변환하므로
    /// 웹은 `/api/v1/me`의 `front_api`가 1.58 이상일 때만 보낸다. `plan_sha256`을 주면 계획이 그 사이 바뀌었을 때 거부한다.
    AutomationConvert {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        #[serde(default)]
        #[ts(optional, as = "Option<bool>")]
        dry_run: bool,
        #[serde(default)]
        #[ts(optional)]
        plan_sha256: Option<String>,
    },
    /// 보냈는지 모르는 `do.message` 묶음을 사람이 확인하고 푼다(FRONT_API 1.58). `batch`는 화면에서 본 묶음이다.
    /// 그 묶음은 다시 보내지 않는다. `discard`면 남은 입력도 버리고, 아니면 남은 입력을 이어 보낸다. 세션 안 연결은 거부한다.
    AutomationUnblock {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        name: String,
        batch: String,
        #[serde(default)]
        discard: bool,
    },
    /// 자동화 파일 원문·판·본문과 검사 결과(관리 GUI W2, FRONT_API 1.76, Reader 가능)
    AutomationRead {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        name: String,
    },
    /// 초안을 쓰지 않고 검사한다(Reader 가능). `form`(TOML과 같은 모양의 값)을 주면 host가 그 값을 원문에 합친다:
    /// 바뀐 칸만 고치고 주석·나머지 칸은 그대로 둔다. `body`는 `text_file`·`brief_file`이 가리킬 본문 초안이다.
    AutomationValidate {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        name: String,
        text: String,
        #[serde(default)]
        #[ts(optional, type = "Record<string, unknown>")]
        form: Option<serde_json::Value>,
        #[serde(default)]
        #[ts(optional)]
        body: Option<String>,
    },
    /// 검사한 초안을 쓴다. `expected_sha256`이 없으면 새로 만들기(그 이름 파일이 있으면 거부), 있으면 그 판일 때만 고친다
    /// (지운 규칙은 되살리지 않는다). 다르면 `conflict`로 거부하고 아무것도 쓰지 않는다. 본문 파일은 먼저, 규칙은 마지막에 쓴다.
    AutomationSave {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        name: String,
        text: String,
        #[serde(default)]
        #[ts(optional)]
        expected_sha256: Option<String>,
        #[serde(default)]
        #[ts(optional)]
        body: Option<String>,
        /// 본문 파일의 본 판. 없으면 그 파일이 아직 없어야 한다
        #[serde(default)]
        #[ts(optional)]
        body_expected_sha256: Option<String>,
    },
    /// 끈 뒤 `automations/.trash/`로 옮긴다. 대기 입력은 취소하고 처리 중 묶음·기록은 둔다. 본문 파일은 그대로 둔다.
    AutomationDelete {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        name: String,
        expected_sha256: String,
    },
    /// 지운 자동화를 꺼진 채로 되살린다. 같은 이름이 있으면 거부한다.
    AutomationRestore {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        id: String,
    },
    /// 조건을 기다리지 않고 동작을 한 번 돌린다(수동 사건 하나). 같은 `key`는 한 번만 받는다. 꺼짐·파일 오류·승인되지 않은 범위는 거부한다.
    AutomationRun {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        name: String,
        key: String,
    },
    /// FRONT_API 1.77. 「이 aaa」·「저장소」 설정 화면이 읽는 프로젝트 설정(관리 GUI W4, plan §8.343). Reader 가능
    ProjectSettings {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
    },
    /// FRONT_API 1.77. 저장소 하나를 제자리에서 바꾼다(`PATH[@MACHINE]`, `repos.remove`+`repos.add`를 한 번에, 차례는 그대로)
    /// 머신별 workspace 자동 배정 허용(Reader도 읽는다). 경로는 project 설정이 정본이다.
    WorkspaceAccess {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        machine: String,
    },
    /// Owner(웹 조작 기기·세션 밖 CLI)만 허용을 바꾼다.
    WorkspaceAccessSet {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        machine: String,
        workspace: String,
        enabled: bool,
        revision: String,
    },
    ProjectRepoEdit {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        from: String,
        to: String,
    },
    /// FRONT_API 1.77. 끝내기 전에 잃을 수 있는 것(미커밋·미병합·사본 상태)을 읽는다. 기록을 고치지 않는다. 바뀐 파일 수를
    /// 그 머신 agent가 읽으므로 조작 기기만(`ThreadDiff`와 같다)
    ThreadResolvePreview {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        thread: String,
    },
    /// FRONT_API 1.77. 미리보기(`library_prune_preview`)와 같은 판정으로 지우되, 후보가 `expected`(`project/name`)와 같을 때만
    /// 지운다(그 사이 바뀌면 `conflict`). 웹 「보고서 정리」의 실행이다. 지우는 것은 `library_delete`보다 좁다(report.md
    /// 사본·thread 기록은 남는다)
    LibraryPruneApply {
        project: Option<String>,
        older_than_days: Option<u32>,
        expected: Vec<String>,
    },
    /// 최근 실행·건너뜀 기록, 최대 50건(Reader 가능).
    AutomationExecutions {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        name: String,
        #[serde(default)]
        #[ts(optional)]
        before: Option<String>,
    },
    AutomationExecution {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        id: String,
    },
    RoutineRuns {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        name: String,
    },
    /// routine 명령 승인. `command_sha256`은 사람이 본 명령(머신 포함)의 hash다. 파일이 그 사이 바뀌면 거부한다.
    RoutineApprove {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        name: String,
        command_sha256: String,
    },
    /// routine 켜기·끄기(front matter `enabled`, herdr-projects `routine toggle`). `on`이 없으면 뒤집는다.
    /// 명령 승인은 그대로 필요하다.
    RoutineToggle {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        name: String,
        #[serde(default)]
        on: Option<bool>,
    },
    /// 흐름 사건을 `after`(cursor) 다음부터 읽는다
    Events {
        project: Option<String>,
        #[serde(default)]
        #[ts(type = "number")]
        after: u64,
        #[serde(default)]
        limit: Option<u32>,
        /// 가장 최근 `limit`개(차례는 그대로 오래된 것부터, FRONT_API 1.77). 웹 알림함 「흐름 사건」이 쓴다. 옛 host는 버린다
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        #[ts(optional, as = "Option<bool>")]
        tail: bool,
    },
    /// 사후 평가자 지정(§8.13): 역할이 evaluator인 thread를 시작한다. 프로젝트당 하나다.
    EvaluatorAssign {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        profile: String,
        machine: Option<String>,
        /// 개선을 직접 고칠 repo(없으면 repo 없는 작업 폴더)
        #[serde(default)]
        repo: Option<String>,
        /// 평가자에게 더할 말(brief 끝에 붙는다)
        #[serde(default)]
        note: Option<String>,
    },
    /// 사후 평가자 상태(프로젝트를 주지 않으면 전체)
    EvaluatorStatus {
        project: Option<String>,
    },
    /// 사후 평가자를 끝낸다(평가자 thread resolve)
    EvaluatorUnassign {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
    },
    /// 평가 기록 목록. 기본은 정하지 않은 개선안이 있는 것만
    Evaluations {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        #[serde(default)]
        all: bool,
    },
    EvaluationShow {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        id: String,
    },
    /// 개선안 채택·보류(평가 기록에 누가 언제 정했는지 남는다)
    EvaluationDecide {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        id: String,
        proposals: Vec<String>,
        adopt: bool,
        #[serde(default)]
        note: Option<String>,
    },
}

impl FrontRequest {
    /// 프로젝트를 받는 요청의 프로젝트 칸(FRONT_API 1.50, plan §8.288). 비어 있으면(생략) host가 맡은 프로젝트로 채운다.
    /// 프로젝트 자체를 바꾸는 요청(lifecycle·delete·rename·sweep)은 이름을 늘 받으므로 여기 없다.
    pub fn project_mut(&mut self) -> Option<&mut String> {
        match self {
            FrontRequest::ThreadStartAccount { start, .. } | FrontRequest::ThreadStartPool { start, .. } => {
                start.project_mut()
            }
            FrontRequest::AccountStarts { project } | FrontRequest::AccountStartCancel { project, .. } => Some(project),
            FrontRequest::Post { project, .. }
            | FrontRequest::PostStatus { project, .. }
            | FrontRequest::PostHandled { project, .. }
            | FrontRequest::PostList { project, .. }
            | FrontRequest::Asks { project, .. }
            | FrontRequest::AskHistory { project, .. }
            | FrontRequest::AskHandle { project, .. }
            | FrontRequest::AskAnswer { project, .. }
            | FrontRequest::QuestionReply { project, .. }
            | FrontRequest::AutoModeGet { project, .. }
            | FrontRequest::AutoDecisions { project, .. }
            | FrontRequest::AutoModeSet { project, .. }
            | FrontRequest::QuestionAutoAnswer { project, .. }
            | FrontRequest::AutoDecisionReview { project, .. }
            | FrontRequest::AutoDecisionForget { project, .. }
            | FrontRequest::AskForget { project, .. }
            | FrontRequest::AskCorrect { project, .. }
            | FrontRequest::PostUnmute { project, .. }
            | FrontRequest::ThreadFile { project, .. }
            | FrontRequest::ThreadDiff { project, .. }
            | FrontRequest::ProjectSet { project, .. }
            | FrontRequest::ProjectOpen { project, .. }
            | FrontRequest::ThreadStart { project, .. }
            | FrontRequest::ThreadAdopt { project, .. }
            | FrontRequest::ThreadNext { project, .. }
            | FrontRequest::ThreadFollowup { project, .. }
            | FrontRequest::Context { project, .. }
            | FrontRequest::Assignable { project, .. }
            | FrontRequest::RecordOpen { project, .. }
            | FrontRequest::Tasks { project }
            | FrontRequest::TaskAttentionAnswer { project, .. }
            | FrontRequest::TaskQueue { project, .. }
            | FrontRequest::TaskArchive { project, .. }
            | FrontRequest::TaskEdit { project, .. }
            | FrontRequest::TaskLog { project, .. }
            | FrontRequest::TaskLogPrune { project, .. }
            | FrontRequest::DesignTree { project, .. }
            | FrontRequest::IdeaLinks { project }
            | FrontRequest::DesignRead { project, .. }
            | FrontRequest::DesignBranchOpen { project, .. }
            | FrontRequest::DesignSave { project, .. }
            | FrontRequest::DesignAttach { project, .. }
            | FrontRequest::DesignDetach { project, .. }
            | FrontRequest::DesignSay { project, .. }
            | FrontRequest::DesignDiscard { project, .. }
            | FrontRequest::DesignApply { project, .. }
            | FrontRequest::DesignApplyCancel { project, .. }
            | FrontRequest::DesignHandoff { project, .. }
            | FrontRequest::DesignIds { project }
            | FrontRequest::DesignMarkIds { project, .. }
            | FrontRequest::DesignLog { project, .. }
            | FrontRequest::DesignRevert { project, .. }
            | FrontRequest::ThreadStartPreview { project, .. }
            | FrontRequest::ThreadShow { project, .. }
            | FrontRequest::ThreadSummary { project, .. }
            | FrontRequest::ThreadSummarySet { project, .. }
            | FrontRequest::ThreadLibrary { project, .. }
            | FrontRequest::LibraryLink { project, .. }
            | FrontRequest::LibraryPublish { project, .. }
            | FrontRequest::LibraryUploadPublish { project, .. }
            | FrontRequest::SafetyShow { project, .. }
            | FrontRequest::SafetySet { project, .. }
            | FrontRequest::ProfilePlan { project, .. }
            | FrontRequest::ProfileApply { project, .. }
            | FrontRequest::LibraryImport { project, .. }
            | FrontRequest::LibraryRead { project, .. }
            | FrontRequest::LibraryOpen { project, .. }
            | FrontRequest::LibraryResolve { project, .. }
            | FrontRequest::ThreadAck { project, .. }
            | FrontRequest::ThreadResolve { project, .. }
            | FrontRequest::ThreadStop { project, .. }
            | FrontRequest::ThreadPark { project, .. }
            | FrontRequest::AutoMergeSet { project, .. }
            | FrontRequest::ThreadUnpark { project, .. }
            | FrontRequest::ThreadRestart { project, .. }
            | FrontRequest::ThreadBrief { project, .. }
            | FrontRequest::ThreadRead { project, .. }
            | FrontRequest::ThreadKeys { project, .. }
            | FrontRequest::ThreadMessage { project, .. }
            | FrontRequest::ThreadAnswer { project, .. }
            | FrontRequest::Inbox { project, .. }
            | FrontRequest::InboxHookNotice { project, .. }
            | FrontRequest::InboxDone { project, .. }
            | FrontRequest::MissionShow { project, .. }
            | FrontRequest::MissionApprove { project, .. }
            | FrontRequest::MissionReject { project, .. }
            | FrontRequest::MissionStart { project, .. }
            | FrontRequest::MissionStop { project, .. }
            | FrontRequest::MissionPause { project, .. }
            | FrontRequest::MissionResume { project, .. }
            | FrontRequest::MissionReload { project, .. }
            | FrontRequest::MissionRestart { project, .. }
            | FrontRequest::MissionOpen { project, .. }
            | FrontRequest::Routines { project, .. }
            | FrontRequest::RoutineRuns { project, .. }
            | FrontRequest::AutomationExecution { project, .. }
            | FrontRequest::AutomationExecutions { project, .. }
            | FrontRequest::RoutineApprove { project, .. }
            | FrontRequest::AutomationApprove { project, .. }
            | FrontRequest::AutomationConvert { project, .. }
            | FrontRequest::AutomationUnblock { project, .. }
            | FrontRequest::AutomationRead { project, .. }
            | FrontRequest::AutomationValidate { project, .. }
            | FrontRequest::AutomationSave { project, .. }
            | FrontRequest::AutomationDelete { project, .. }
            | FrontRequest::AutomationRestore { project, .. }
            | FrontRequest::AutomationRun { project, .. }
            | FrontRequest::ProjectSettings { project }
            | FrontRequest::WorkspaceAccess { project, .. }
            | FrontRequest::WorkspaceAccessSet { project, .. }
            | FrontRequest::MachineReserve { project, .. }
            | FrontRequest::ProjectRepoEdit { project, .. }
            | FrontRequest::ThreadResolvePreview { project, .. }
            | FrontRequest::RoutineToggle { project, .. }
            | FrontRequest::EvaluatorAssign { project, .. }
            | FrontRequest::EvaluatorUnassign { project, .. }
            | FrontRequest::Evaluations { project, .. }
            | FrontRequest::EvaluationShow { project, .. }
            | FrontRequest::EvaluationDecide { project, .. } => Some(project),
            _ => None,
        }
    }

    /// thread_read.v1의 명시적 허용 목록. Reader 요청 중에도 부수 효과·관리 조회는 전달하지 않는다.
    /// host 밖 머신 thread 세션의 CLI가 agent를 거쳐 host에 보낼 수 있는 요청(`thread_read.v1`): 위 읽기와 기획 읽기, 그리고 자기
    /// 기획 branch 저장과 그림 첨부·삭제(host가 그 세션 thread의 기획 branch인지 다시 본다)
    pub fn thread_relay_allowed(&self) -> bool {
        self.thread_read_allowed()
            || matches!(
                self,
                Self::DesignTree { .. } | Self::DesignRead { .. } | Self::DesignIds { .. } | Self::DesignLog { .. }
            )
            || self.design_relay_write()
    }

    /// 원격 중계에서 Git 쓰기를 기다리는 요청
    pub fn design_relay_write(&self) -> bool {
        matches!(self, Self::DesignSave { .. } | Self::DesignAttach { .. } | Self::DesignDetach { .. })
    }

    pub fn thread_read_allowed(&self) -> bool {
        matches!(
            self,
            Self::Projects { .. }
                | Self::Threads { .. }
                | Self::ThreadShow { .. }
                | Self::ThreadSummary { .. }
                | Self::ThreadLibrary { .. }
                | Self::LibraryReports { .. }
                | Self::LibraryOpen { .. }
                | Self::LibraryRead { .. }
                | Self::RecordSearch { .. }
                | Self::RecordOpen { .. }
        )
    }

    /// 조작(controller) 권한이 필요한 요청인가
    pub fn needs_control(&self) -> bool {
        !matches!(
            self,
            FrontRequest::AccountList
                | FrontRequest::AccountPlans
                | FrontRequest::AccountDefaults
                | FrontRequest::AccountPool
                | FrontRequest::AccountStarts { .. }
                | FrontRequest::Status
                | FrontRequest::DeployContext
                | FrontRequest::DeployStatus { .. }
                | FrontRequest::Rollouts
                | FrontRequest::DeployJobs
                | FrontRequest::PostStatus { .. }
                | FrontRequest::Asks { .. }
                | FrontRequest::AskHistory { .. }
                | FrontRequest::AutoModeGet { .. }
                | FrontRequest::AutoDecisions { .. }
                | FrontRequest::WorkspaceDashboard { .. }
                | FrontRequest::Machines
                | FrontRequest::MachineSetupList
                | FrontRequest::Panes { .. }
                | FrontRequest::Projects { .. }
                | FrontRequest::Instance
                | FrontRequest::ProjectRename { dry_run: true, .. }
                | FrontRequest::ProjectSweep { dry_run: true, .. }
                | FrontRequest::ProjectFiles { fix: false }
                | FrontRequest::Profiles { .. }
                | FrontRequest::ProfileResolve { .. }
                | FrontRequest::Threads { .. }
                | FrontRequest::ThreadShow { .. }
                | FrontRequest::ThreadLibrary { .. }
                | FrontRequest::ThreadSummary { .. }
                | FrontRequest::ThreadUnpark { dry_run: true, .. }
                | FrontRequest::RecordSearch { .. }
                | FrontRequest::RecordOpen { .. }
                | FrontRequest::LibraryReports { .. }
                | FrontRequest::LibraryRead { .. }
                | FrontRequest::LibraryOpen { .. }
                | FrontRequest::LibraryMarkRead { .. }
                | FrontRequest::LibraryUsage
                | FrontRequest::LibraryPrunePreview { .. }
                | FrontRequest::ThreadNext { line: None, add: None, .. }
                | FrontRequest::Context { peek: true, .. }
                | FrontRequest::Assignable { .. }
                | FrontRequest::Tasks { .. }
                | FrontRequest::TaskQueue { action: crate::QueueOp::List, .. }
                | FrontRequest::TaskArchive { restore: None, .. }
                | FrontRequest::TaskLog { .. }
                | FrontRequest::DesignTree { .. }
                | FrontRequest::IdeaLinks { .. }
                | FrontRequest::DesignRead { .. }
                | FrontRequest::DesignIds { .. }
                | FrontRequest::DesignLog { .. }
                // 기획 대화 thread(보기 권한)도 자기 기획 branch에 저장한다: 권한은 host가 요청자로 다시 본다
                | FrontRequest::DesignSave { .. }
                | FrontRequest::DesignAttach { .. }
                | FrontRequest::DesignDetach { .. }
                | FrontRequest::Events { .. }
                | FrontRequest::Questions { .. }
                | FrontRequest::Inbox { peek: true, .. }
                | FrontRequest::MissionList { .. }
                | FrontRequest::MissionShow { .. }
                | FrontRequest::Routines { .. }
                | FrontRequest::RoutineRuns { .. }
                | FrontRequest::AutomationExecution { .. }
                | FrontRequest::AutomationExecutions { .. }
                | FrontRequest::AutomationConvert { dry_run: true, .. }
                | FrontRequest::AutomationRead { .. }
                | FrontRequest::AutomationValidate { .. }
                | FrontRequest::WorkspaceAccess { .. }
                | FrontRequest::ProjectSettings { .. }
                | FrontRequest::ThreadRead { .. }
                | FrontRequest::PaneRead { .. }
                | FrontRequest::PaneReadText { .. }
                | FrontRequest::PaneWaitOutput { .. }
                | FrontRequest::Workspaces
                | FrontRequest::PaneGet { .. }
                | FrontRequest::PaneChat { .. }
                | FrontRequest::Workspace { .. }
                | FrontRequest::InstallPublish { .. }
                | FrontRequest::InstallPublishStatus { .. }
                | FrontRequest::WebStatus
                | FrontRequest::Devices
                | FrontRequest::EvaluatorStatus { .. }
                | FrontRequest::Evaluations { .. }
                | FrontRequest::EvaluationShow { .. }
                | FrontRequest::Preferences
        )
    }

    /// 같은 머신(Local)에서만 허용하는 관리 요청인가
    pub fn local_only(&self) -> bool {
        matches!(
            self,
            FrontRequest::AccountImport { .. }
                | FrontRequest::WebPublishPort { .. }
                | FrontRequest::AccountRemove { .. }
                | FrontRequest::DebugInput { .. }
                | FrontRequest::TickerTick
                | FrontRequest::LibraryPublish { .. }
                | FrontRequest::LibraryImport { .. }
                | FrontRequest::LibraryPrune { .. }
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FrontResponse {
    WorkspaceDashboard(crate::WorkspaceDashboard),
    InstallPublished {
        publication: crate::install::InstallPublication,
    },
    InstallApproval {
        approval: crate::install::InstallApprovalView,
    },
    Accounts {
        accounts: Vec<crate::AccountView>,
    },
    AccountImageJobs {
        jobs: Vec<crate::AccountImageJob>,
    },
    AccountDefaults {
        defaults: Vec<crate::AccountDefault>,
    },
    AccountPlans {
        plans: Vec<crate::AccountPlanOption>,
    },
    AccountPool {
        pool: crate::AccountPoolView,
    },
    AccountStarts {
        starts: Vec<crate::AccountStartView>,
    },
    AccountStart {
        start: crate::AccountStartView,
    },
    /// 인증 URL·일회용 코드는 이 요청 기기의 직접 응답에만 담는다.
    AccountLoginInteraction {
        login: Option<crate::AccountLoginView>,
        prompt: Option<crate::AccountLoginPrompt>,
        #[ts(type = "number")]
        expires_ms: u64,
    },
    AccountLogin {
        login: crate::AccountLoginView,
    },
    InputEvents {
        events: Vec<crate::InputEvent>,
    },
    DeployContext {
        project: Option<String>,
        session: bool,
        allowed: bool,
    },
    Rollout {
        rollout: crate::RolloutView,
    },
    /// 웹 조작 기기의 rollout 시작·조회와 `rollouts`. 표시용 요약만 싣는다
    RolloutCards {
        cards: Vec<crate::RolloutCard>,
    },
    RolloutPlan {
        plan: crate::RolloutPlanView,
    },
    /// 웹 조작 기기의 재시작 시작·조회와 `deploy_jobs`
    DeployJobs {
        jobs: Vec<crate::DeployJobCard>,
    },
    Deployment {
        machine: String,
        job: crate::DeployJob,
        running_software: String,
        running_sha256: String,
    },
    Missions {
        missions: Vec<crate::MissionView>,
    },
    Mission {
        mission: Box<crate::MissionView>,
    },
    Post {
        result: crate::PostResult,
        /// status 조회에서 확인한 inbox 처리 상태(FRONT_API 1.84). 옛 host는 생략한다.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        handled: Option<bool>,
    },
    Posts {
        posts: Vec<crate::PostEnvelope>,
        /// 음소거된 보낸 쪽(`post unmute`에 줄 이름, FRONT_API 1.77). 옛 host는 생략한다
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        #[ts(optional, as = "Option<Vec<String>>")]
        muted: Vec<String>,
    },
    /// 「답할 것」(FRONT_API 1.78): 열린 확인 필요 post, 답한 것(최신부터), 자동 정리 기간
    AutoMode(crate::AutoMode),
    /// `auto_merge_set`의 답(FRONT_API 1.143): 할 일 표시 ID·ULID, 이어진 thread, 지금 값, 이번에 바꿨는가
    AutoMerge {
        task_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        short_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        thread: Option<String>,
        on: bool,
        changed: bool,
    },
    AutoDecisions {
        decisions: Vec<crate::AutoDecision>,
        next: Option<String>,
    },
    /// 자동 답 처리 결과(FRONT_API 1.166). ok는 이 질문의 답 저장 성공, complete는 묶음 답 처리 완료다.
    QuestionAnswered {
        id: String,
        complete: bool,
    },
    Asks {
        #[serde(default)]
        auto_mode: crate::AutoMode,
        #[serde(default)]
        auto_decisions: Vec<crate::AutoDecision>,
        #[serde(default)]
        auto_next: Option<String>,
        #[serde(default)]
        auto_exceptions: u32,
        requests: Vec<crate::UserRequestView>,
        done: Vec<crate::AskRecord>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        done_index: Option<Vec<crate::AskRecordSummary>>,
        #[serde(default)]
        done_next: Option<String>,
        keep_days: u32,
    },
    AskHistory {
        done: Vec<crate::AskRecord>,
        next: Option<String>,
    },
    Status(HostStatus),
    Machines {
        machines: Vec<MachineView>,
    },
    Panes {
        panes: Vec<PaneView>,
    },
    Pane(PaneView),
    Next {
        lines: Vec<String>,
        /// `line`으로 보낸 메시지
        message: Option<MessageView>,
    },
    Context(ProjectContext),
    Assignable {
        owners: Vec<OwnerChoice>,
        /// `check`가 통과했을 때 설명
        valid: Option<String>,
    },
    /// `tasks`의 답(FRONT_API 1.60)
    TaskBoard(TaskBoard),
    /// `design_tree`의 답(FRONT_API 1.170)
    DesignTree {
        repos: Vec<crate::DesignRepoView>,
    },
    /// `idea_links`의 답(FRONT_API 1.178): n-ID → 기획 문서(`<저장소>/<경로>`)
    IdeaLinks {
        links: std::collections::BTreeMap<String, String>,
    },
    /// `design_read`의 답(FRONT_API 1.170)
    DesignDoc {
        doc: crate::DesignDoc,
    },
    /// `design_branch_open`의 답(FRONT_API 1.171)
    DesignBranch {
        branch: crate::DesignBranchView,
    },
    /// `design_save`의 답(FRONT_API 1.171): 새 commit과 쓴 파일의 blob(다음 저장의 기준)
    DesignSaved {
        path: String,
        commit: String,
        blob: String,
    },
    /// `design_detach`의 답(FRONT_API 1.185)
    DesignDetached {
        path: String,
        commit: String,
        warnings: Vec<String>,
    },
    /// `design_log`의 답(FRONT_API 1.186)
    DesignLog {
        entries: Vec<crate::DesignLogEntry>,
    },
    /// `design_ids`의 답(FRONT_API 1.181)
    DesignIds {
        ids: Vec<crate::DesignIdView>,
    },
    /// `design_mark_ids`의 답(FRONT_API 1.181): 표지를 넣은 곳(t-2002부터 `main`, 옛 host는 기획 branch `aaa-ids`), 넣은 문서 수,
    /// commit(넣은 것이 없으면 없음)
    DesignIdsMarked {
        branch: String,
        marked: u32,
        /// 고른 문서 중 넣지 않은 수: 그 사이 옮겼거나 지웠거나 이미 표지가 있다
        skipped: u32,
        commit: Option<String>,
    },
    /// `design_handoff`의 답(FRONT_API 1.175)
    DesignHandoff {
        handoff: crate::DesignHandoffView,
    },
    TaskQueue {
        entries: Vec<crate::TaskReservation>,
    },
    TaskArchive {
        entries: Vec<crate::ArchivedTask>,
        #[serde(default)]
        #[ts(optional)]
        next: Option<String>,
        #[serde(default)]
        #[ts(optional, as = "Option<Vec<crate::TaskHistoryRepo>>")]
        repos: Vec<crate::TaskHistoryRepo>,
        #[serde(default)]
        #[ts(optional, as = "Option<Vec<String>>")]
        profiles: Vec<String>,
        #[serde(default)]
        #[ts(optional, as = "Option<Vec<String>>")]
        machines: Vec<String>,
    },
    /// `task_edit`의 답(FRONT_API 1.62): 쓴 뒤(또는 충돌이면 지금) board. `log_ids`는 이 편집이 남긴 기록
    TaskEdited {
        board: TaskBoard,
        conflict: Option<TaskConflict>,
        #[ts(type = "number[]")]
        log_ids: Vec<i64>,
        /// 메모 교체로 기존 줄이 빠졌을 때 줄 수·원문 기록 번호(FRONT_API 1.134)
        #[serde(default)]
        #[ts(optional, as = "Option<Vec<String>>")]
        warnings: Vec<String>,
    },
    /// `thread_start_preview`의 답(FRONT_API 1.63)
    ThreadStartPreview(ThreadStartPreview),
    /// `task_log`의 답(FRONT_API 1.62). `next`를 `before`로 주면 그 앞을 읽는다
    TaskLog {
        entries: Vec<TaskLogEntry>,
        #[ts(type = "number | null")]
        next: Option<i64>,
        #[serde(default)]
        #[ts(optional, as = "Option<Vec<crate::TaskHistoryRepo>>")]
        repos: Vec<crate::TaskHistoryRepo>,
    },
    Screen(ScreenText),
    Read(ReadView),
    /// `thread read`·`thread keys`
    ThreadPane(ThreadPaneView),
    /// `pane wait-output`이 찾았다: 맞은 줄과 그때 읽은 글
    OutputMatched {
        matched_line: String,
        read: ReadView,
    },
    Workspaces {
        workspaces: Vec<WorkspaceSummary>,
    },
    Chat(ChatView),
    /// 찾은 파일과 읽기 경로(`/f/<cap>/<이름>`, 30분), 아니면 고를 후보
    PaneFile {
        info: Option<FileInfo>,
        candidates: Vec<String>,
        url: Option<String>,
    },
    PaneDir {
        listing: DirListing,
    },
    PaneCommands {
        list: CommandList,
    },
    EnrollGuide {
        guide: crate::EnrollGuide,
    },
    MachineSetup {
        setup: crate::MachineSetupView,
    },
    MachineSetups {
        setups: Vec<crate::MachineSetupView>,
    },
    Pending {
        pending: Vec<PendingEnroll>,
        /// 등록 창이 열려 있으면 닫히는 시각(ms, FRONT_API 1.80). 닫혔으면 생략
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional, type = "number")]
        open_until_ms: Option<u64>,
    },
    PairCode {
        code: String,
        role: DeviceRole,
        #[ts(type = "number")]
        expires_in_s: u64,
    },
    Publish(PublishState),
    Devices {
        devices: Vec<DeviceView>,
    },
    Workspace(Workspace),
    Done,
    /// 붙여 넣은 그림을 대상 머신에 저장했다(`PaneAttach` 마지막 조각). `inserted`면 harness 입력창에 경로를 넣었다.
    Attached {
        path: String,
        inserted: bool,
        note: Option<String>,
    },
    Projects {
        projects: Vec<ProjectView>,
    },
    Instance {
        project: Option<ProjectView>,
    },
    Project(ProjectView),
    ProjectChange(ProjectChange),
    ProjectOpened(Box<ProjectOpened>),
    ProjectFiles {
        checks: Vec<ProjectFilesCheck>,
    },
    Profiles {
        machine: String,
        profiles: Vec<ProfileView>,
    },
    ProfileResolved {
        profile: ProfileView,
    },
    Threads {
        threads: Vec<ThreadView>,
        /// 다음 페이지가 있으면 이 커서를 after로 보낸다.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        next_after: Option<String>,
    },
    Thread(Box<ThreadView>),
    /// FRONT_API 1.44. thread 작업 요약
    ThreadSummary(ThreadSummaryView),
    Events {
        events: Vec<FlowEvent>,
    },
    Questions {
        questions: Vec<QuestionView>,
    },
    Question(QuestionView),
    Message(MessageView),
    Inbox {
        items: Vec<InboxItem>,
        /// `inbox_done`이 끝낸 항목 수(그때 `items`는 남은 항목). 목록 요청에는 없다.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        done: Option<u32>,
        /// 응답 크기 한도로 싣지 못한 끝내지 않은 항목 수(실은 것은 오래된 것부터다)
        #[serde(default)]
        omitted_open: u32,
        /// 응답 크기 한도로 싣지 못한 끝낸 항목 수(`all`, 실은 것은 최근 것부터다)
        #[serde(default)]
        omitted_done: u32,
    },
    /// `inbox_hook_notice`: hook에 붙일 한 줄. 새로 알릴 항목이 없으면 없다
    InboxHookNotice {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        text: Option<String>,
    },
    /// `ticker_tick`: 이 host 실행에서 끝난 tick 수(방금 돈 것까지)
    Ticker {
        runs: u64,
    },
    Routines {
        routines: Vec<RoutineView>,
        /// 지운 자동화(FRONT_API 1.76). 옛 host는 생략한다
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        #[ts(optional, as = "Option<Vec<crate::AutomationTrashEntry>>")]
        trash: Vec<crate::AutomationTrashEntry>,
    },
    /// `automation_read`·`automation_save`의 답
    AutomationDoc {
        doc: crate::AutomationDoc,
    },
    /// `automation_validate`의 답: 합친 원문과 검사 결과
    AutomationChecked {
        text: String,
        check: crate::AutomationCheck,
    },
    /// `automation_run`의 답. `accepted`가 거짓이면 같은 key를 이미 받았다
    AutomationRan {
        accepted: bool,
        detail: String,
    },
    /// `project_settings`·`project_repo_edit`의 답
    ProjectSettings(crate::ProjectSettingsView),
    WorkspaceAccess(crate::WorkspaceAccessView),
    Safety(crate::SafetyView),
    ProfileAdmin(crate::ProfileAdminView),
    ProfilePlan(crate::ProfilePlanView),
    /// profile을 바꾼 뒤: 알림 글과 경고, 새 목록
    ProfileApplied {
        message: String,
        warnings: Vec<String>,
        view: crate::ProfileAdminView,
    },
    WorkRoot(crate::WorkRootView),
    WorkRootChecked(crate::WorkRootCheckView),
    Hook(crate::HookView),
    ModelAdmin(crate::ModelAdminView),
    /// `thread_resolve_preview`의 답
    ResolvePreview(crate::ResolvePreview),
    /// FRONT_API 1.105. `thread_unpark { dry_run: true }`
    UnparkPreview(crate::UnparkPreview),
    AutomationConverted {
        files: Vec<String>,
        /// 쓰지 않은 계획이다(FRONT_API 1.58). `files`는 실제 변환과 같은 모양(새 파일과 `.converted/` 보존 사본)이다.
        #[serde(default)]
        dry_run: bool,
        /// 계획 hash. 실행 요청에 실어 보내면 그 사이 바뀐 계획을 거부한다.
        #[serde(default)]
        plan_sha256: String,
    },
    AutomationExecutions {
        runs: Vec<crate::AutomationExecution>,
        next: Option<String>,
    },
    AutomationExecution {
        run: crate::AutomationExecution,
    },
    RoutineRuns {
        runs: Vec<crate::RoutineRun>,
        #[serde(default)]
        executions: Vec<crate::AutomationExecution>,
        /// 파일 원문(FRONT_API 1.58). 임무 조건·깨진 파일·옛 host는 없다.
        #[serde(default)]
        source: Option<crate::RoutineSource>,
    },
    RecordSearch(crate::RecordSearchPage),
    RecordDocument(crate::RecordDocument),
    LibraryChunk {
        data: B64,
        #[ts(type = "number | null")]
        next: Option<u64>,
    },
    Library(LibraryView),
    /// `thread_diff`의 답
    ThreadDiff {
        project: String,
        thread: String,
        diff: crate::DiffView,
    },
    LibraryLink(LinkView),
    /// `library_resolve`의 답: 그 cap이 가리키는 보고서
    LibraryResolved {
        project: String,
        name: String,
    },
    LibraryReports {
        reports: Vec<LibraryReport>,
    },
    LibraryImported {
        reports: Vec<LibraryReport>,
        skipped: Vec<LibrarySkip>,
    },
    LibraryPruned {
        removed: Vec<String>,
    },
    LibraryPrunePreview(LibraryPrunePlan),
    LibraryRotated,
    LibraryRetention(LibraryRetentionView),
    LibraryUsage {
        /// FRONT_API 1.114. size·max_size에서 제외한 보관함 용량.
        #[serde(default)]
        #[ts(type = "number")]
        archived_size: u64,
        #[ts(type = "number")]
        size: u64,
        reports: u32,
        live: u32,
        problems: u32,
        #[serde(default)]
        #[ts(type = "number")]
        max_size: u64,
        #[serde(default)]
        #[ts(type = "number")]
        max_days: u64,
        #[serde(default)]
        #[ts(type = "number")]
        protected_size: u64,
    },
    LibraryLinks {
        links: Vec<LinkView>,
    },
    Evaluators {
        evaluators: Vec<EvaluatorView>,
    },
    Evaluations {
        evaluations: Vec<EvaluationView>,
    },
    Evaluation(Box<EvaluationView>),
    Preferences(SharedPreferences),
}

/// host 전체 사본 보존 설정. host.local.toml이 덮으면 웹 고치기는 막는다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LibraryRetentionView {
    #[ts(type = "number")]
    pub max_days: u64,
    #[ts(type = "number")]
    pub max_mb: u64,
    pub rev: String,
    pub editable: bool,
}

/// 이 host의 모든 웹 화면이 함께 쓰는 것: 기기에 기대지 않는 설정과 고정(herdr-web-ui `shared/preferences.ts` `SharedPreferences`).
/// 값의 뜻은 웹이 본다. host는 공유 이름(`SHARED_SETTING_KEYS`)과 모양·한도만 지킨다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SharedPreferences {
    /// 처음 쓰기 전에는 0, 쓸 때마다 하나 오른다
    #[ts(type = "number")]
    pub revision: u64,
    /// 어느 화면이 나눈 설정만 있다. 아무도 안 바꾼 설정은 없고, 기기마다 제 것으로 남는다
    #[ts(type = "Record<string, unknown>")]
    pub settings: BTreeMap<String, serde_json::Value>,
    /// 목록 고정 열쇠(agent는 `machine:pane`), 고정한 순서대로. 화면이 처음 고정을 나누기 전에는 없다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub pins: Option<Vec<String>>,
}

/// front가 알리는 복사 결과. 본문·clipboard 내용·OS 오류 문자열은 host로 보내지 않는다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "ev", rename_all = "snake_case")]
pub enum FrontEvent {
    /// 머신·pane 목록이 바뀌었다(전체 목록)
    State { machines: Vec<MachineView>, panes: Vec<PaneView> },
    /// workspace layout이 바뀌었다
    Workspace(Workspace),
    /// 흐름 사건(새로 기록될 때). 놓쳤으면 `Events`로 cursor부터 다시 읽는다.
    Flow(FlowEvent),
    /// thread 기록이 바뀌었다. JSON은 그대로 두고 큰 본체를 별도로 보관한다.
    Thread(Box<ThreadView>),
    /// 확인창 질문이 생기거나 답해졌다(알림 대상)
    Question(QuestionView),
    /// 공유 설정 문서 전체. 웹 events 연결이 열릴 때와 바뀔 때마다 온다(herdr-web-ui `preferences` 사건)
    Preferences(SharedPreferences),
    /// 상태 사건 연결의 `ping`에 대한 답(herdr-web-ui ws.ts 「Liveness」: 그 ping의 답만 연결이 살아 있음을 말한다)
    Pong { id: u32 },
    /// host가 주는 웹 빌드(주 묶음 파일 이름, 예 `index-BsC_6E06`). 웹 events 연결이 열릴 때마다 온다. 제 빌드와 다르면 웹은
    /// 「다시 열기」 줄을 띄운다(배포 뒤 홈 화면 앱이 옛 웹을 계속 돌리는 문제, herdr-web-ui `reloadReason` "updated", plan §8.166)
    Web { build: String },
    /// FRONT_API 1.43. 보고서 목록이 바뀌었다(보내기·열기·지우기·thread 사본). 웹 events 연결에만, 열 때와 바뀔 때 보낸다.
    /// `unseen`은 열지 않은 사용자 보고서 수(활동 막대 보고서 배지)
    Reports { unseen: u32 },
    /// FRONT_API 1.78. 「답할 것」의 확인 필요 post나 답한 것 기록이 바뀌었다(웹은 `asks`를 다시 읽는다). 웹 events 연결에만, 열 때와
    /// 바뀔 때 보낸다. `requests`는 열린 확인 필요 post 수
    Asks { requests: u32 },
    /// FRONT_API 1.77. 알림함(inbox)이 바뀌었다(항목이 더해지거나 끝났다). 웹 events 연결에만, 열 때와 바뀔 때 보낸다.
    /// 웹은 `inbox`(`peek`)를 다시 읽는다(배지·알림함 패널)
    Inbox,
    /// FRONT_API 1.79. host 계정이 바뀌었다(로그인 진행·끝, 기본, 메일·상태). 웹 events 연결에만, 열 때와 바뀔 때 보낸다.
    /// 웹은 `account_list`·`account_defaults`와 보던 로그인의 `account_login_status`를 다시 읽는다
    Accounts,
    /// FRONT_API 1.111. 원장 저장 뒤 웹 지표를 다시 읽는다. 웹 events 연결에만 보낸다.
    WorkspaceMetrics,
    /// FRONT_API 1.80. 머신 등록 창·대기 목록이 바뀌었다(열기·닫기·새 요청·승인·거부). 웹 events 연결에만, 열 때와 바뀔 때 보낸다.
    /// 웹은 `enroll_pending`을 다시 읽는다
    Enroll,
    /// FRONT_API 1.60. 프로젝트 TASKS.md가 바뀌었다(ticker가 크기·mtime을 보고 내용 hash로 확인한다). 웹은 `tasks`를 다시 읽는다.
    /// 옛 CLI는 모르는 사건을 풀지 못하므로 1.60 이상과 합의한 연결에만 보낸다.
    Tasks { project: String, sha256: String },
    /// 설치 앱 아이콘의 host 공통 수. 웹 events 연결에서 처음과 값이 바뀔 때만 보낸다.
    AppBadge { count: u32 },
    /// 웹 연결 처음과 배포 내구 상태가 바뀔 때 주는 최신 요약. 조회만 하며 진행기를 재개하지 않는다.
    Rollout { result: Outcome<Option<crate::RolloutCard>> },
    /// 앱 안 알림(웹 Droplet, plan §8.185). 기록하지 않는다: 놓친 알림은 다시 오지 않는다(FRONT_API 1.34,
    /// 옛 front는 모르는 사건을 버린다)
    Notice(NoticeView),
    /// `chat_watch`로 청한 pane의 대화 기록이 바뀌었다(herdr-web-ui `conversation-changed`). 웹은 대화를 다시 읽는다.
    /// `signature`는 뜻 없는 이름이다(REST ETag가 아니다)
    ChatChanged { pane: String, signature: String },
    /// `chat_watch`로 청한 pane의 기록 변화를 지금 알릴 수 있는가(그 머신 agent가 `chat_watch.v1`이고 붙어 있다). 청하면 한 번,
    /// 바뀌면 다시 온다. 아니면 웹은 2초마다 읽는다(참고 conversationRefresh.ts push 없는 판)
    ChatWatching { pane: String, live: bool },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum FrontMsg {
    Hello {
        proto: Version,
        client: String,
        /// aaa 세션 안에서 실행한 CLI면 그 세션 ID(`AAA_SESSION_ID`). 사후 평가자 세션이면 host가 보기 권한만 준다(§8.13).
        #[serde(default)]
        session: Option<String>,
    },
    /// `front`는 이 연결의 ID다(lease 보유자 비교에 쓴다)
    Welcome {
        proto: Version,
        host_id: String,
        role: DeviceRole,
        front: String,
        /// coordinator의 다음 CLI 연결에 보일 활성 post 음소거. 옛 host는 비어 있다.
        #[serde(default)]
        warnings: Vec<String>,
    },
    Reject(ErrorBody),
    Req {
        id: String,
        body: FrontRequest,
    },
    Res {
        id: String,
        result: Outcome<FrontResponse>,
    },
    Event(FrontEvent),
}

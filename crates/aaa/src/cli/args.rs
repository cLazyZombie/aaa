//! 명령 문법: 최상위 명령과, 처리 코드가 따로 없는 작은 하위 명령.

use super::*;

#[derive(Parser)]
#[command(name = "aaa", version, about = "여러 머신의 agent 오케스트레이션")]
pub(super) struct Cli {
    /// 설정 폴더(기본: AAA_CONFIG_DIR 또는 OS 기본 위치)
    #[arg(long, global = true)]
    pub(super) config_dir: Option<PathBuf>,
    /// 상태 폴더(기본: AAA_STATE_DIR 또는 OS 기본 위치)
    #[arg(long, global = true)]
    pub(super) state_dir: Option<PathBuf>,
    /// 기계가 읽는 JSON으로 출력
    #[arg(long, global = true)]
    pub(super) json: bool,
    /// 인스턴스(기본: AAA_INSTANCE, 없으면 기본 인스턴스). 설정·상태·홈·작업 root·launchd label·웹 쿠키가 따로다(plan §8.294).
    /// --config-dir·--state-dir와 그 env가 있으면 그 폴더가 이긴다
    #[arg(long, global = true, value_name = "이름")]
    pub(super) instance: Option<String>,
    #[command(subcommand)]
    pub(super) cmd: Cmd,
}

#[derive(Subcommand)]
pub(super) enum AccountCmd {
    /// harness별 계정 자동 배정. add/remove로 계정마다 켜고 끈다.
    Pool {
        #[command(subcommand)]
        cmd: AccountPoolCmd,
    },
    /// 내구 계정 배정 대기와 시작 영수증을 본다.
    Starts {
        project: Option<String>,
    },
    CancelStart {
        id: String,
        project: Option<String>,
    },
    /// host의 격리된 공식 CLI로 로그인한다. 인증값은 출력하지 않는다.
    Login {
        name: String,
        #[arg(long, value_parser=["claude", "codex"])]
        harness: String,
        #[arg(long)]
        email: Option<String>,
    },
    /// 실행 인증과 별도로 Claude 사용량 조회에 로그인한다.
    UsageLogin {
        name: String,
        /// 등록 메일을 모를 때 기대 메일. 조회 로그인에 성공하면 보관한다.
        #[arg(long)]
        email: Option<String>,
    },
    LoginStatus {
        id: String,
    },
    LoginCancel {
        id: String,
    },
    /// 다시 로그인에 다른 메일이 들어왔을 때(`mismatch`): --switch면 이 별칭을 새 메일 인증으로 바꾸고, --keep이면 옛 인증을 남긴다
    LoginConfirm {
        id: String,
        #[arg(long, conflicts_with = "keep", required_unless_present = "keep")]
        switch: bool,
        #[arg(long)]
        keep: bool,
    },
    /// harness별 기본 계정. 이름을 생략하면 조회하고 --off면 해제한다.
    Default {
        #[arg(value_parser=["claude", "codex"])]
        harness: String,
        #[arg(conflicts_with = "off")]
        account: Option<String>,
        #[arg(long)]
        off: bool,
    },
    /// 결과 파일에서 읽어 암호화 보관한다. 토큰을 argv로 받지 않는다.
    Import {
        name: String,
        #[arg(long,value_parser=["claude","codex"])]
        harness: String,
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        email: Option<String>,
    },
    /// 등록 메일을 저장하거나 수정한다.
    Email {
        name: String,
        email: String,
    },
    List,
    /// 다시 로그인 없이 계정 별칭을 바꾼다. aaa 세션 밖에서만 쓴다.
    Rename {
        name: String,
        to: String,
    },
    Remove {
        name: String,
    },
}

#[derive(Subcommand)]
pub(super) enum AccountPoolCmd {
    List,
    Add {
        account: String,
        /// 구판 호환 인자. 그룹은 사용하지 않는다.
        #[arg(long, hide = true)]
        group: Option<String>,
        #[arg(long = "model", hide = true)]
        models: Vec<String>,
    },
    Remove {
        account: String,
    },
    #[command(hide = true)]
    Project {
        project: String,
        #[arg(long, conflicts_with = "off", required_unless_present = "off")]
        on: bool,
        #[arg(long)]
        off: bool,
    },
}

#[derive(Subcommand)]
pub(super) enum Cmd {
    /// host에 보관된 완료 보고·답한 질문·할 일 본문의 과거 판까지 찾는다
    Search {
        query: String,
        #[arg(long)]
        project: Option<String>,
        #[arg(long, value_parser=["report","question","task"])]
        kind: Option<String>,
        #[arg(long)]
        after: Option<String>,
        #[arg(long)]
        limit: Option<u32>,
    },
    /// 검색 결과의 프로젝트와 해시로 당시 원문 판을 연다
    Record { project: String, key: String },
    /// 저장소의 검증 스크립트를 찾아 실행하고 workspace 원장을 자동으로 남긴다
    Verify {
        /// 생략하면 scripts/verify.sh·verify_engine.sh·ci-local.sh를 찾는다. 직접 지정할 때는 -- 뒤에 명령을 쓴다
        #[arg(last = true, value_name = "명령")]
        command: Vec<std::ffi::OsString>,
    },
    /// 어느 harness든 host Codex 계정으로 이미지를 생성·편집한다.
    Image {
        #[command(subcommand)]
        cmd: crate::image::Cmd,
    },
    /// host에서 로그인하고 새 작업의 기본 인증을 정한다(aaa 세션 밖 CLI)
    Account {
        #[command(subcommand)]
        cmd: AccountCmd,
    },
    /// 역할·작업 경로·서비스를 계획하고 명시적 승인 뒤 설치·재개한다
    Install(Box<crate::install::Args>),
    /// 이 PC를 agent로 설치하고 OS 서비스로 띄워 host에 연결한다(`aaa-setup`으로 두면 인자 없이 뜬다)
    Setup(crate::setup::Args),
    /// (내부) `aaa setup`이 등록한 서비스 입구. OS 감독자만 실행한다.
    #[command(hide = true)]
    SetupService,
    /// harness 전역 연동을 설치하거나 제거한다(기존 사용자 hook은 보존)
    Integration {
        #[command(subcommand)]
        cmd: IntegrationCmd,
    },
    /// 자료·알림·요청을 inbox에 남긴다. 기밀성 없음: 비밀값을 넣지 않는다
    Post(PostArgs),
    /// 이 머신의 작업 root를 조회·점검·설정한다(서비스는 재시작하지 않는다)
    WorkRoot {
        #[command(subcommand)]
        cmd: crate::work_root::Command,
    },
    /// host backend 실행(보통 OS 감독자가 띄운다)
    Host,
    /// host DB의 암호화 백업·새 파일 복원(host 머신에서 실행).
    DbBackup {
        #[command(subcommand)]
        cmd: crate::db_backup::Command,
    },
    /// 첨부(uploads)의 암호화 백업·새 폴더 복원(host 머신에서 실행).
    FileBackup {
        #[command(subcommand)]
        cmd: crate::file_backup::Command,
    },
    /// (내부) SCM에서만 실행하는 고정 Windows 서비스 입구. console fallback은 없다.
    #[command(hide = true)]
    ServiceRun {
        #[arg(long)]
        context: PathBuf,
        #[arg(long)]
        context_sha256: String,
    },
    /// (내부) 신뢰를 만들지 않는 검증 SSH의 private 설치 제안.
    #[command(hide = true)]
    InstallProbe {
        #[arg(long)]
        executor_sha256: String,
    },
    /// (내부) 검증된 SSH 계보와 고정 helper SHA에서만 최초 설치 pin을 받는다.
    #[command(hide = true)]
    InstallBootstrap {
        #[arg(long)]
        executor_sha256: String,
        #[arg(long)]
        issuer_sha256: String,
    },
    /// (내부) private 승인 입력과 EOF 뒤에만 진행하는 고정 installer worker.
    #[command(hide = true)]
    InstallWorker {
        #[arg(long)]
        id: String,
        #[arg(long)]
        executor_sha256: String,
        #[arg(long)]
        resume: bool,
    },
    /// agent backend 실행, 또는 host에 등록(enroll)
    Agent(AgentArgs),
    /// (내부) agent가 승인한 독립 복구 감시. 직접 실행 경로/명령은 받지 않는다.
    #[command(hide = true)]
    DeploymentGuard {
        #[arg(long)]
        id: String,
    },
    /// (내부) 세션 하나의 PTY를 소유하는 프로세스
    #[command(hide = true)]
    SessionHost {
        #[arg(long)]
        dir: PathBuf,
        #[arg(long, hide = true)]
        startup_stdin: bool,
    },
    /// host 입력 사건 진단(로컬에서만, 원문 없음)
    Debug {
        #[command(subcommand)]
        cmd: DebugCmd,
    },
    /// host 상태
    Status,
    /// 여러 머신의 배포를 host부터 차례로 진행한다(coordinator safety 필요)
    Deploy {
        #[command(subcommand)]
        cmd: crate::rollout::DeployCmd,
    },
    /// pane: 읽기·입력·나누기·닫기(herdr `pane` 명령과 같은 이름. 출력은 늘 JSON 한 줄)
    Pane {
        #[command(subcommand)]
        cmd: PaneCmd,
    },
    /// 탭: 만들기·목록·이름·닫기(herdr `tab`. 출력은 늘 JSON 한 줄)
    Tab {
        #[command(subcommand)]
        cmd: TabCmd,
    },
    /// workspace(이름 있는 layout): 목록·만들기·이름·닫기(출력은 늘 JSON 한 줄)
    Workspace {
        #[command(subcommand)]
        cmd: WorkspaceCmd,
    },
    /// 셸 완성 스크립트를 출력한다(bash·elvish·fish·powershell·zsh)
    Completion {
        #[arg(value_parser = ["bash", "elvish", "fish", "powershell", "zsh"])]
        shell: String,
    },
    /// 삭제된 TUI: 웹 UI 주소는 host의 `aaa web status`로 확인한다
    #[command(alias = "tui")]
    Attach {
        /// workspace 이름(기본 main)
        #[arg(long, short)]
        workspace: Option<String>,
        /// 다른 기기에서 host 웹 주소로 붙는다(`aaa web publish`의 https 주소). 처음에는 host에서 사람이 `aaa web pair`로
        /// 낸 코드를 묻고, 받은 기기 token을 이 기기에 둔다
        #[arg(long, value_name = "웹 주소")]
        remote: Option<String>,
    },
    /// 머신 등록·목록·폐기(host에서)
    Machine {
        #[command(subcommand)]
        cmd: MachineCmd,
    },
    /// host 웹: pairing 코드, tailscale serve HTTPS 공개
    Web {
        #[command(subcommand)]
        cmd: WebCmd,
    },
    /// 보고서별 공유 링크(host 사본 읽기만)
    Library {
        #[command(subcommand)]
        cmd: LibraryCmd,
    },
    /// 자가 점검(계보·키체인·첫 실행 지연)
    Doctor(DoctorArgs),
    /// macOS 감독자: 운영은 show·install·uninstall·restart·stop·start(LaunchAgent → aaa-launcher → aaa, 결정 14). plan·register 등은 보류한 SMAppService 경로
    Service {
        #[command(subcommand)]
        cmd: ServiceCmd,
    },
    /// 설정 확인·예시
    Config {
        #[command(subcommand)]
        cmd: ConfigCmd,
    },
    /// 공개 GitHub release(기본 최신판) 또는 --from 파일로 바꾼다. SHA-256·self-check·version 고정 검사 뒤
    /// rename하고 옛것은 rollback으로 되돌린다. 실제 교체는 사람 또는 safety로 허용한 coordinator만
    #[command(args_conflicts_with_subcommands = true)]
    Update {
        #[command(subcommand)]
        cmd: Option<UpdateCmd>,
        /// coordinator 배포 대상 머신. --from은 그 머신의 파일이다.
        #[arg(long)]
        machine: Option<String>,
        /// 새 실행 파일(같은 OS·CPU로 빌드한 aaa)
        #[arg(long, value_name = "파일")]
        from: Option<PathBuf>,
        /// 받을 release 판(x.y.z 또는 vx.y.z). --from과 함께 쓰지 않는다
        #[arg(long = "version", value_name = "판", conflicts_with = "from")]
        release_version: Option<String>,
        /// 검사만 하고 바꾸지 않는다(누구나)
        #[arg(long)]
        dry_run: bool,
    },
    /// 같은 Mac에 둘째 aaa를 만든다: `aaa --instance <이름> init`(폴더·겹치지 않는 포트·홈, plan §8.294)
    Init,
    /// 프로젝트 폴더(host)
    Project {
        #[command(subcommand)]
        cmd: ProjectCmd,
    },
    /// 설정 값 하나를 바꾼다(`aaa project set`의 새 꼴, plan §8.285): goal, thread_profile, coordinator_profile,
    /// max_parallel_threads, auto_resolve_days, repos.add, repos.remove(workspaces.add·remove도 같다), nudge, mute.
    /// 프로젝트 홈의 aaa.toml이 있으면 거기, 없으면 project.toml에 쓴다(plan §8.288)
    Set { key: String, value: String },
    /// coordinator(`aaa project open`의 새 꼴, plan §8.285)
    Coordinator {
        #[command(subcommand)]
        cmd: CoordinatorCmd,
    },
    /// 없어졌다: 결정은 프로젝트 memory에 적는다(안내만 하고 끝난다)
    #[command(hide = true)]
    Decision {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        _rest: Vec<String>,
    },
    /// coordinator가 매 턴 처음 보는 프로젝트 요약: TASKS.md, 열린 thread와 Next, start_threads, 맡길 수 있는 owner
    Context {
        /// (옛 꼴의 프로젝트, 생략한다)
        #[arg(hide = true)]
        project: Option<String>,
        /// 보인 inbox 항목을 「봤다」로 표시하지 않는다
        #[arg(long)]
        peek: bool,
    },
    /// 필요할 때 읽는 세부 안내(AGENTS.md·brief에는 매 턴 규칙만 둔다, plan §8.491). 주제 없이 부르면 목록을 보인다
    Guide {
        /// 주제 이름(`aaa guide`가 목록을 보인다)
        topic: Option<String>,
        /// 모든 주제를 차례로 보인다
        #[arg(long, conflicts_with = "topic")]
        all: bool,
    },
    /// TASKS.md owner로 쓸 수 있는 profile·머신. --check로 owner 하나를 확인한다.
    Assignable {
        /// (옛 꼴의 프로젝트, 생략한다)
        #[arg(hide = true)]
        project: Option<String>,
        #[arg(long)]
        check: Option<String>,
    },
    /// TASKS.md 할 일: 보기(list)와 고치기(add·edit·move·rm·restore·list-add 등, 바뀐 블록만 쓰고 충돌을 감지하며 기록을 남긴다)
    Task {
        #[command(subcommand)]
        cmd: TaskCmd,
    },
    /// 기획서: 프로젝트 저장소마다 기획 폴더(aaa.toml의 design)의 문서 목록·보기. origin 기본 branch에서 읽는다(웹 「기획」 화면과 같다).
    /// 수정본은 작업 branch에서 웹 보고서로 발행한다.
    Plan {
        #[command(subcommand)]
        cmd: PlanCmd,
    },
    /// inbox: 보고·질문·세션 끝남·머신 단절 같은 잃지 않는 항목(끝내기로 정리)
    Inbox {
        #[command(subcommand)]
        cmd: InboxCmd,
    },
    /// host 안의 ticker(inbox·outage·routine·건강·nudge를 주기마다 본다, herdr-projects `ticker`)
    Ticker {
        #[command(subcommand)]
        cmd: TickerCmd,
    },
    /// 없어졌다: 착지는 thread의 Next로 진행한다(안내만 하고 끝난다)
    #[command(hide = true)]
    Merge {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        _rest: Vec<String>,
    },
    /// routine(projects/<p>/routines/*.md) 목록과 명령 승인
    Routine {
        #[command(subcommand)]
        cmd: RoutineCmd,
    },
    /// 자동화 정의·실행 기록·승인·이관
    Automation {
        #[command(subcommand)]
        cmd: AutomationCmd,
    },
    /// 특수 임무 정의·승인·깨움 상태
    #[command(hide = true)]
    Mission {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        _rest: Vec<String>,
    },
    /// thread profile: host.toml과 기본 profile(claude·codex). 바꾸기·허용 목록은 사람만
    Profile {
        #[command(subcommand)]
        cmd: crate::settings::ProfileCmd,
    },
    /// provider별 모델 켜기·끄기(host.toml [models.*]). 끈 모델은 profile model 칸에 쓸 수 없다. 바꾸기는 사람만
    Model {
        #[command(subcommand)]
        cmd: crate::settings::ModelCmd,
    },
    /// yolo와 사람만 바꾸는 safety 설정(host.toml [safety.*], herdr-projects `safety`)
    Safety {
        #[command(subcommand)]
        cmd: crate::settings::SafetyCmd,
    },
    /// thread 시작·목록·확인(ack)·끝내기(resolve)
    Thread {
        #[command(subcommand)]
        cmd: ThreadCmd,
    },
    /// 사후 평가자: 배경에서 일의 흐름(시간·재작업·막힌 곳)을 평가하는 agent 지정·상태·열기·끝내기
    #[command(hide = true)]
    Evaluator {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        _rest: Vec<String>,
    },
    /// 평가 기록 목록·보기, 개선안 채택(adopt)·보류(hold)
    #[command(hide = true)]
    Evaluation {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        _rest: Vec<String>,
    },
    /// (harness hook) 세션 안에서 harness 사건을 이 머신 agent에 보낸다. 사건 JSON은 인자 또는 표준 입력
    #[command(hide = true)]
    Hook {
        #[arg(long, value_parser = [
            "claude", "codex", "quota", "agy-status", "agy-session-start", "agy-pre-invocation",
            "agy-post-tool-use", "agy-stop"
        ])]
        agent: String,
        payload: Option<String>,
    },
    /// 세션 안에서 진행을 보고한다(예: `aaa report --percent 40 --activity "시험 중"`, herdr-projects `report`와 같은 인자)
    Report {
        /// 어림한 진행(0~100). 100이면 활동은 `Done`이다
        #[arg(long, value_parser = clap::value_parser!(u8).range(0..=100), required_unless_present = "unknown", conflicts_with = "unknown")]
        percent: Option<u8>,
        /// 범위를 아직 모른다
        #[arg(long)]
        unknown: bool,
        /// 두~네 단어의 활동. 사용자를 기다리면 `Waiting for you`
        #[arg(long)]
        activity: String,
    },
    /// 확인창 질문(폴더 신뢰·권한 확인)을 보고 답한다
    Question {
        #[command(subcommand)]
        cmd: QuestionCmd,
    },
    /// 흐름 사건(프로젝트별 순번). cursor(`--after`) 다음부터 읽는다
    Events {
        /// 프로젝트(생략하면 모든 프로젝트). 다른 프로젝트 명령처럼 위치 인자다
        project: Option<String>,
        /// 이 순번 다음부터(프로젝트를 줄 때)
        #[arg(long, default_value_t = 0)]
        after: u64,
        /// 다 읽은 뒤 새 사건을 계속 보인다
        #[arg(long)]
        follow: bool,
    },
    /// 지금 운영(herdr·herdr-projects 0.2)에서 옮기기: inventory·import·shadow 비교(plan §15, docs/migration.md)
    Migrate {
        #[command(subcommand)]
        cmd: crate::migrate::MigrateCmd,
    },
}

#[derive(clap::Subcommand)]
pub(super) enum DebugCmd {
    /// 최근 입력 사건을 시간순으로 본다. --since는 10m·2h·1d 또는 Unix milliseconds
    Input {
        #[arg(long)]
        pane: Option<String>,
        #[arg(long)]
        thread: Option<String>,
        #[arg(long, default_value = "1h")]
        since: String,
    },
}

#[derive(Subcommand)]
pub(super) enum QuestionCmd {
    List {
        /// 답한 것도 보인다
        #[arg(long)]
        all: bool,
    },
    /// 질문 ID에 답한다(A·B 또는 --text). 기존 확인창의 yes·no도 받는다
    Answer {
        id: String,
        choice: Option<String>,
        #[arg(long)]
        text: Option<String>,
        /// coordinator auto mode로 답한다(검증한 coordinator만)
        #[arg(long, requires = "reason_file")]
        auto: bool,
        #[arg(long, requires = "auto")]
        reason_file: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
pub(super) enum TickerCmd {
    /// 지금 한 번 돌리고, 이 명령 뒤에 시작한 tick이 끝나면 끝난다(돌던 tick이 있으면 그 다음 것까지). host 머신 CLI에서만.
    /// 시험이 「host가 봤는데 할 일이 없었다」를 고정 sleep 없이 기다린다(docs/testing.md)
    Tick,
}

#[derive(Subcommand)]
pub(super) enum CoordinatorCmd {
    /// coordinator를 연다: 돌고 있으면 그 pane을 알리고, 없으면 host 머신에서 프로젝트 폴더를 작업 폴더로 띄운다
    Open {
        /// coordinator profile(안 주면 project.toml coordinator_profile → host.toml [defaults] → claude). `aaa profile list`
        #[arg(long, value_name = "NAME")]
        profile: Option<String>,
        /// coordinator가 돌아도 하나 더 띄운다
        #[arg(long)]
        new: bool,
    },
}

#[derive(Subcommand)]
pub(super) enum InboxCmd {
    /// 끝내지 않은 항목. 처음 보는 항목은 「봤다」로 표시한다(`--peek`이면 표시하지 않는다)
    List {
        /// (옛 꼴의 프로젝트, 생략한다)
        #[arg(hide = true)]
        project: Option<String>,
        /// 끝낸 항목도 보인다
        #[arg(long)]
        all: bool,
        #[arg(long)]
        peek: bool,
    },
    /// 항목을 끝낸다
    Done {
        ids: Vec<String>,
        /// 끝내지 않은 항목 모두
        #[arg(long)]
        all: bool,
    },
}

#[derive(Subcommand)]
pub(super) enum PlanCmd {
    /// 저장소마다 기획 폴더와 문서(경로·제목)와 기획 대화
    List,
    /// 지운 옛 아이디어의 n-ID가 가리키는 기획 문서(줄마다 n-ID와 `<저장소>/<경로>`, 프로젝트 `idea-links.toml`)
    Links,
    /// 문서 글. 첫 줄에 저장소·경로·ID·commit·blob(저장의 기준 판), 빈 줄 뒤에 글이다. `aaa plan show k-0012`처럼 기획서 ID로도 연다
    Show {
        /// 저장소 이름(경로 끝 폴더 이름), 또는 기획서 ID(`k-NNNN`)
        repo: String,
        /// 저장소 기준 문서 경로(기획 폴더 안). 기획서 ID로 열면 주지 않는다
        path: Option<String>,
        /// 옛 main 판(commit, `aaa plan log`의 첫 칸)에서 읽는다
        #[arg(long)]
        at: Option<String>,
        /// 옛 기획 branch 주제(t-2002부터 늘 main을 읽는다: 주면 무시하고 알린다)
        #[arg(long, hide = true)]
        branch: Option<String>,
    },
    /// 문서 이력: main이 실제로 가졌던 판을 새것부터(commit·때·누가·요약)
    Log {
        repo: String,
        /// 저장소 기준 문서·그림 경로(기획 폴더 안)
        path: String,
    },
    /// 착지할 최종 통합 worktree에서 저장소 문서 검사와 새 그림 경로를 확인한다
    Check {
        path: String,
        #[arg(long)]
        base: String,
        #[arg(long)]
        head: String,
        /// 문서 폴더 아래의 design_media
        #[arg(long)]
        media: Option<String>,
        /// host.toml design_check의 인자 배열(JSON)
        #[arg(long, default_value = "[]")]
        command_json: String,
    },
    /// 작업 branch HEAD와 기준 판에서 문서·그림을 읽어 library용 검토 페이지를 만든다
    Preview {
        /// 저장소 기준 문서 경로
        path: String,
        #[arg(long)]
        base: String,
        /// 새로 만들 결과 폴더
        #[arg(long)]
        output: std::path::PathBuf,
    },
}

#[derive(Subcommand)]
pub(super) enum MachineCmd {
    List,
    /// 머신별 workspace 자동 배정 허용을 읽거나 바꾼다(변경은 세션 밖 CLI만).
    Workspaces {
        name: String,
        #[arg(long)]
        project: Option<String>,
        #[arg(long, conflicts_with = "deny")]
        allow: Option<String>,
        #[arg(long)]
        deny: Option<String>,
    },
    /// 등록 창을 연다(예: --open 10m)
    Enroll {
        #[arg(long, default_value = "10m")]
        open: String,
        /// 등록 창을 시한 전에 닫는다(대기 중인 요청은 그대로)
        #[arg(long, conflicts_with = "open")]
        close: bool,
    },
    /// 승인을 기다리는 등록
    Pending,
    /// agent 화면의 확인 코드로 승인한다
    Approve {
        name: String,
        #[arg(long)]
        code: String,
    },
    /// 등록·열린 pane을 유지하고 머신 이름을 바꾼다(host 자신은 제외).
    Rename {
        name: String,
        to: String,
    },
    /// 머신 독점 예약을 건다. 주인이 아닌 일은 자동 배정에서 빠지고 이 머신을 지정한 시작은 풀릴 때까지 기다린다.
    /// 사용자, 사용자 지시를 인용한 coordinator(--user-request), thread 세션 안이면 자기 머신을 자기 이름으로 건다
    Reserve {
        name: String,
        /// 주인: t-NNNN thread나 할 일(d-NNNN). thread 세션 안에서는 생략한다(그 thread)
        #[arg(long)]
        holder: Option<String>,
        /// 까닭 한 줄
        #[arg(long)]
        note: Option<String>,
        /// 저절로 풀릴 때까지(예: 90m·2h·1d). 생략하면 주인이 끝나거나 풀 때까지
        #[arg(long = "for")]
        duration: Option<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        user_request: Option<String>,
    },
    /// 머신 독점 예약을 푼다(건 쪽과 같은 권한, thread는 자기 예약만)
    Release {
        name: String,
        #[arg(long)]
        user_request: Option<String>,
    },
    /// 자동 배정 비중: 숫자(0은 자동 배정 제외) 또는 auto(CPU 수). 사람만 바꾼다
    Weight {
        name: String,
        value: String,
    },
    Revoke {
        name: String,
    },
}

#[derive(Subcommand)]
pub(super) enum LibraryCmd {
    /// 현재 판 또는 지정한 r 판을 자동 정리에서 제외한다. --remove는 보관함에서 뺀다.
    Archive {
        #[arg(required = true)]
        names: Vec<String>,
        #[arg(long)]
        remove: bool,
    },
    /// 이름·thread ID·r-NNNN으로 특정 보고 판과 파일 목록을 읽는다.
    #[command(alias = "open")]
    Show { name: String },
    /// 보고 판의 library 파일을 읽는다. 기본은 원문 bytes, --json은 조각 응답이다.
    Cat { name: String, path: String },
    /// host 머신의 폴더·파일을 복사한다. 같은 이름은 최신으로 바꾼다.
    Publish {
        source: std::path::PathBuf,
        /// (옛 꼴, 생략한다)
        #[arg(long, hide = true)]
        project: Option<String>,
        #[arg(long)]
        name: String,
        /// 복사 없이 등록한 폴더에서 변경을 즉시 보여 준다(폴더만).
        #[arg(long)]
        live: bool,
        /// 사용자에게 보내는 웹 보고서(맨 위 index.html 필수). 보고서 화면 「내 보고서」에 안 봄으로 뜨고 사용자 알림 post를 남긴다.
        /// 다시 보내면 다시 안 봄이다.
        #[arg(long)]
        to_user: bool,
        /// 「내 보고서」에 보일 제목(--to-user와 함께, 없으면 이름)
        #[arg(long, requires = "to_user")]
        title: Option<String>,
        /// 보고서를 낸 thread(t-0001). 없으면 host가 이름·원본 경로(예: library/t-0001)에 든 이 프로젝트의 thread를 쓴다.
        /// 보고서 화면이 이 번호를 보인다.
        #[arg(long = "thread", value_name = "THREAD")]
        from_thread: Option<String>,
    },
    /// 이름·r 번호의 보고서를 모든 보존 판과 함께 지운다(host 사본·목록·공유 링크·thread 보고 사본). 되돌릴 수 없다.
    /// 끝나지 않은(resolve 전) thread의 보고가 있으면 아무것도 지우지 않는다.
    Delete {
        /// (옛 꼴, 생략한다)
        #[arg(long, hide = true)]
        project: Option<String>,
        #[arg(required = true)]
        names: Vec<String>,
    },
    /// 하위 폴더를 각각 가져온다. 충돌·상한 초과·symlink는 건너뛴 목록에 적는다.
    Import {
        source: std::path::PathBuf,
        /// (옛 꼴, 생략한다)
        #[arg(long, hide = true)]
        project: Option<String>,
        /// 저장 이름 앞에 붙인다(예: archive- → archive-t-0001).
        #[arg(long, default_value = "")]
        prefix: String,
    },
    /// 오래된 사본을 지운다. 진행 중 thread와 실시간 폴더는 남긴다.
    Prune {
        #[arg(long, required_unless_present = "dry_run")]
        older_than: Option<u32>,
        #[arg(long)]
        project: Option<String>,
        /// 자동 정리의 기한·합계 상한으로 지울 목록만 본다. 쓰기·삭제·알림을 하지 않는다.
        #[arg(long)]
        dry_run: bool,
    },
    /// 보고서 서명 키를 교체한다. 옛 공유 링크가 모두 끝난다(pairing은 그대로).
    Rotate,
    /// 유효한 링크를 받는다. 없거나 만료·폐기됐으면 새로 만든다.
    Link {
        thread: String,
        /// (옛 꼴 `<프로젝트> <대상>`의 대상)
        #[arg(hide = true)]
        legacy: Option<String>,
        /// 새 링크 수명(기본 30일, 1~3650일). 기존 링크 수명은 바꾸지 않는다.
        #[arg(long)]
        days: Option<u32>,
    },
    /// 링크·만료·마지막 열람·사본 크기
    #[command(alias = "ls")]
    List {
        #[arg(long)]
        project: Option<String>,
    },
    /// 서명 링크를 즉시 폐기한다(앱 안 보기는 계속 된다)
    Revoke { id: String },
}

#[derive(Subcommand)]
pub(super) enum WebCmd {
    /// 웹 pairing 코드(10분, 1회용). 아직 쓰지 않은 앞 코드는 버린다(웹 설정 → 기기의 코드도)
    Pair {
        /// 보기 권한만 준다
        #[arg(long)]
        reader: bool,
    },
    /// pairing한 웹 기기 목록
    Devices,
    /// 웹 기기를 취소한다. 그 기기의 열린 화면은 바로 닫힌다
    Revoke {
        id: String,
    },
    /// 웹 기기 이름을 바꾼다
    Rename {
        id: String,
        label: String,
    },
    /// tailscale serve로 HTTPS 공개(off: aaa가 만든 공개만 끈다)
    Publish {
        #[arg(value_parser = ["off"], conflicts_with = "port")]
        off: Option<String>,
        /// 공개 포트를 변경하고 저장한 링크를 이전한다. 옛 외부 링크는 끊기며 알림은 새 주소에서 다시 켠다
        #[arg(long, value_parser = clap::value_parser!(u16).range(1..))]
        port: Option<u16>,
    },
    Status,
}

#[derive(Subcommand)]
pub(super) enum IntegrationCmd {
    /// 전역 hook을 더하고 hooks 기능을 켠다(agy는 먼저 `aaa config agy`로 머신 설정)
    Install {
        #[arg(value_parser = ["codex", "agy"])]
        target: String,
    },
    /// aaa hook만 제거한다(다른 도구도 쓰는 hooks 기능은 켜 둔다)
    Uninstall {
        #[arg(value_parser = ["codex", "agy"])]
        target: String,
    },
}

#[derive(Subcommand)]
pub(super) enum UpdateCmd {
    /// coordinator 배포 작업 기록. 재시작으로 응답을 잃어도 같은 ID를 확인한다.
    Status {
        id: String,
        #[arg(long)]
        wait: bool,
        #[arg(long)]
        machine: Option<String>,
    },
    /// 보관한 옛 실행 파일로 되돌린다(이름이 없으면 지금과 다른 가장 새 것)
    Rollback {
        #[arg(long)]
        machine: Option<String>,
        /// `aaa update list`의 이름
        name: Option<String>,
        /// 검사만 하고 바꾸지 않는다(누구나)
        #[arg(long)]
        dry_run: bool,
    },
    /// 지금 실행 파일과 보관한 옛 실행 파일(version·sha256)
    List,
}

//! 명령줄. 역할 실행(`host`·`agent`·`session-host`)과 front 명령을 한 binary에 둔다.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};

use crate::service;
use aa_doctor as doctor;
use aa_proto::{B64, DeviceRole, FrontRequest, FrontResponse, LayoutChange, Role, SplitDir, ThreadKind};

#[derive(Parser)]
#[command(name = "aa", version, about = "여러 머신의 agent 오케스트레이션")]
struct Cli {
    /// 설정 폴더(기본: AA_CONFIG_DIR 또는 OS 기본 위치)
    #[arg(long, global = true)]
    config_dir: Option<PathBuf>,
    /// 상태 폴더(기본: AA_STATE_DIR 또는 OS 기본 위치)
    #[arg(long, global = true)]
    state_dir: Option<PathBuf>,
    /// 기계가 읽는 JSON으로 출력
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// host backend 실행(보통 OS 감독자가 띄운다)
    Host,
    /// agent backend 실행, 또는 host에 등록(enroll)
    Agent(AgentArgs),
    /// (내부) 세션 하나의 PTY를 소유하는 프로세스
    #[command(hide = true)]
    SessionHost {
        #[arg(long)]
        dir: PathBuf,
    },
    /// host 상태
    Status,
    /// pane(세션) 목록·실행·읽기·입력
    Pane {
        #[command(subcommand)]
        cmd: PaneCmd,
    },
    /// workspace layout(탭·나누기). 정본은 host에 있다
    Ws {
        #[command(subcommand)]
        cmd: WsCmd,
    },
    /// TUI front: host의 workspace를 터미널에서 보고 조작한다(같은 머신)
    Attach {
        /// workspace 이름(기본 main)
        #[arg(long, short)]
        workspace: Option<String>,
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
    /// 자가 점검(계보·키체인·첫 실행 지연)
    Doctor(DoctorArgs),
    /// macOS 감독자(SMAppService LaunchAgent) 등록·해제·상태
    Service {
        #[command(subcommand)]
        cmd: ServiceCmd,
    },
    /// 설정 확인·예시
    Config {
        #[command(subcommand)]
        cmd: ConfigCmd,
    },
    /// 프로젝트 폴더(host)
    Project {
        #[command(subcommand)]
        cmd: ProjectCmd,
    },
    /// 결정 기록(projects/<p>/decisions.md). thread brief에 들어간다
    Decision {
        #[command(subcommand)]
        cmd: DecisionCmd,
    },
    /// coordinator가 매 턴 처음 보는 프로젝트 요약: TASKS.md, 열린 thread와 Next, start_threads, 맡길 수 있는 owner
    Context { project: String },
    /// TASKS.md owner로 쓸 수 있는 profile·머신. --check로 owner 하나를 확인한다.
    Assignable {
        project: String,
        #[arg(long)]
        check: Option<String>,
    },
    /// inbox: 보고·질문·세션 끝남·머신 단절 같은 잃지 않는 항목(끝내기로 정리)
    Inbox {
        #[command(subcommand)]
        cmd: InboxCmd,
    },
    /// 병합 대기열(순번과 사건만, 병합 단계는 coordinator가 한다)
    Merge {
        #[command(subcommand)]
        cmd: MergeCmd,
    },
    /// routine(projects/<p>/routines/*.md) 목록과 명령 승인
    Routine {
        #[command(subcommand)]
        cmd: RoutineCmd,
    },
    /// 머신의 agent.toml profile 목록
    Profile {
        #[command(subcommand)]
        cmd: ProfileCmd,
    },
    /// thread 시작·목록·확인(ack)·끝내기(resolve)
    Thread {
        #[command(subcommand)]
        cmd: ThreadCmd,
    },
    /// (harness hook) 세션 안에서 harness 사건을 이 머신 agent에 보낸다. 사건 JSON은 인자 또는 표준 입력
    #[command(hide = true)]
    Hook {
        #[arg(long, value_parser = ["claude", "codex"])]
        agent: String,
        payload: Option<String>,
    },
    /// 세션 안에서 진행을 보고한다(예: `aa report --percent 40 --activity "시험 중"`)
    Report {
        #[arg(long)]
        percent: Option<u8>,
        #[arg(long)]
        activity: Option<String>,
    },
    /// 확인창 질문(폴더 신뢰·권한 확인)을 보고 답한다
    Question {
        #[command(subcommand)]
        cmd: QuestionCmd,
    },
    /// 흐름 사건(프로젝트별 순번). cursor(`--after`) 다음부터 읽는다
    Events {
        #[arg(long, short)]
        project: Option<String>,
        /// 이 순번 다음부터(프로젝트를 줄 때)
        #[arg(long, default_value_t = 0)]
        after: u64,
        /// 다 읽은 뒤 새 사건을 계속 보인다
        #[arg(long)]
        follow: bool,
    },
}

#[derive(Subcommand)]
enum QuestionCmd {
    List {
        /// 답한 것도 보인다
        #[arg(long)]
        all: bool,
    },
    /// 답한다(예: yes·no). host가 질문이 아직 열려 있는지 다시 확인한다
    Answer { id: String, choice: String },
}

#[derive(Subcommand)]
enum ProjectCmd {
    List,
    /// 프로젝트 폴더를 만든다(PROJECT.md·MEMORY.md·TASKS.md·decisions.md …)
    Create {
        name: String,
    },
}

#[derive(Subcommand)]
enum DecisionCmd {
    /// 결정을 덧붙인다(번호는 자동)
    Add {
        project: String,
        #[arg(long, conflicts_with = "text_file")]
        text: Option<String>,
        /// 내용 파일(`-`면 표준 입력)
        #[arg(long)]
        text_file: Option<String>,
        /// 이 결정이 대체하는 번호
        #[arg(long)]
        replaces: Option<u32>,
    },
    List {
        project: String,
    },
}

#[derive(Subcommand)]
enum InboxCmd {
    /// 끝내지 않은 항목. 처음 보는 항목은 「봤다」로 표시한다(`--peek`이면 표시하지 않는다)
    List {
        project: String,
        /// 끝낸 항목도 보인다
        #[arg(long)]
        all: bool,
        #[arg(long)]
        peek: bool,
    },
    /// 항목을 끝낸다
    Done {
        project: String,
        #[arg(required_unless_present = "all")]
        ids: Vec<String>,
        /// 끝내지 않은 항목 모두
        #[arg(long, conflicts_with = "ids")]
        all: bool,
    },
}

#[derive(Subcommand)]
enum MergeCmd {
    /// 맨 뒤에 넣는다(이미 있으면 그 자리)
    Enqueue {
        project: String,
        thread: String,
    },
    List {
        project: String,
    },
    /// 병합을 마쳤다: 대기열에서 빼고 다음 차례로 넘긴다
    Done {
        project: String,
        thread: String,
    },
    /// 병합하지 않고 뺀다
    Drop {
        project: String,
        thread: String,
    },
}

#[derive(Subcommand)]
enum RoutineCmd {
    List {
        project: String,
    },
    /// 명령을 승인한다. 사람이 host 머신 터미널(aa 세션 밖)에서 명령을 보고 routine 이름을 쳐야 한다
    Approve {
        project: String,
        name: String,
    },
}

#[derive(Subcommand)]
enum ProfileCmd {
    List {
        #[arg(long)]
        machine: Option<String>,
    },
}

#[derive(Subcommand)]
enum ThreadCmd {
    /// thread를 시작한다: 그 머신 agent가 worktree(또는 작업 폴더)를 만들고 profile로 agent를 띄운 뒤 brief를 안내한다
    Start {
        project: String,
        /// 제목(--from-task면 비워도 된다)
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        machine: Option<String>,
        /// profile(--from-task의 owner가 정하면 비워도 된다)
        #[arg(long)]
        profile: Option<String>,
        /// worktree(기본, --repo 필요) 또는 dir(repo 없는 작업 폴더)
        #[arg(long, value_parser = ["worktree", "dir"], default_value = "worktree")]
        kind: String,
        /// 그 머신의 repo 경로
        #[arg(long)]
        repo: Option<String>,
        /// 기준 ref(기본: origin/HEAD, 없으면 현재 branch)
        #[arg(long)]
        base: Option<String>,
        /// 할 일 파일(`-`면 표준 입력). --from-task면 비워도 된다.
        #[arg(long)]
        task_file: Option<String>,
        /// TASKS.md의 task 제목. owner가 profile·machine 기본값이 되고 메모가 할 일에 붙는다.
        #[arg(long)]
        from_task: Option<String>,
    },
    /// thread의 Next 목록(보고서 `## Next` + 더한 줄). --line N은 그 줄을 thread에 메시지로 보낸다.
    Next {
        project: String,
        thread: String,
        #[arg(long, conflicts_with = "add")]
        line: Option<u32>,
        /// Next 목록에 한 줄 더한다
        #[arg(long)]
        add: Option<String>,
    },
    List {
        #[arg(long, short)]
        project: Option<String>,
    },
    /// 보고서와 결과물 목록, 웹에서 볼 읽기 전용 경로(2시간)
    Library {
        project: String,
        thread: String,
    },
    Show {
        project: String,
        thread: String,
    },
    /// 지금 보고를 확인했다
    Ack {
        project: String,
        thread: String,
    },
    /// 끝낸다: 마지막 사본 → 세션 종료 → worktree 정리(force 없음, 병합된 branch만 삭제)
    Resolve {
        project: String,
        thread: String,
        #[arg(long)]
        keep_worktree: bool,
    },
    /// brief 안내를 사람이 다시 보낸다(자동 재전송은 하지 않는다)
    Brief {
        project: String,
        thread: String,
    },
    /// thread agent를 멈춘다(Esc 한 번). 세션·작업 폴더는 그대로다
    Stop {
        project: String,
        thread: String,
    },
    /// 끝난 thread 세션을 다시 띄운다. 기본은 task를 다시 읽어 새 brief로 새 대화
    Restart {
        project: String,
        thread: String,
        /// 저장된 대화를 잇는다(brief 안내 없음)
        #[arg(long)]
        resume: bool,
    },
    /// thread agent에 메시지를 보낸다. 기본은 대기열(턴이 끝나면), --interrupt면 지금
    Message {
        project: String,
        thread: String,
        #[arg(long)]
        interrupt: bool,
        /// 보낼 글 파일(`-`면 표준 입력)
        #[arg(long, conflicts_with = "text")]
        text_file: Option<String>,
        #[arg(long)]
        text: Option<String>,
    },
}

#[derive(Args)]
struct AgentArgs {
    #[command(subcommand)]
    cmd: Option<AgentCmd>,
    /// host 주소("local" 또는 <이름>:<포트>). 기본: agent.toml
    #[arg(long)]
    host: Option<String>,
    /// 머신 이름. 기본: agent.toml 또는 hostname
    #[arg(long)]
    name: Option<String>,
    /// 개발 모드 표시(host가 자식으로 띄운 로컬 agent)
    #[arg(long)]
    dev_mode: bool,
}

#[derive(Subcommand)]
enum AgentCmd {
    /// host의 등록 창에 이 머신을 등록한다(확인 코드를 host에서 입력해 승인)
    Enroll {
        /// host 주소 <이름 또는 주소>:<포트>
        host: String,
        #[arg(long)]
        name: Option<String>,
    },
}

#[derive(Subcommand)]
enum PaneCmd {
    /// pane 목록(기본은 평가자 세션을 뺀다)
    List {
        /// 이 역할(coordinator·worker·evaluator)의 세션만
        #[arg(long)]
        role: Option<String>,
    },
    /// 새 pane(세션)을 띄운다. 명령이 없으면 그 머신 사용자의 기본 셸
    Run {
        #[arg(long)]
        machine: Option<String>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        cwd: Option<String>,
        #[arg(long)]
        cols: Option<u16>,
        #[arg(long)]
        rows: Option<u16>,
        #[arg(last = true)]
        argv: Vec<String>,
    },
    /// 보이는 화면을 글자로 읽는다
    Read {
        pane: String,
    },
    /// 입력을 보낸다(문자열 그대로, --enter면 끝에 Enter)
    Send {
        pane: String,
        #[arg(long)]
        text: String,
        #[arg(long)]
        enter: bool,
    },
    Resize {
        pane: String,
        cols: u16,
        rows: u16,
    },
    Kill {
        pane: String,
    },
    /// 끝난(중단·유실·종료) pane을 같은 spec으로 다시 띄운다(layout 자리도 바꾼다)
    Resume {
        pane: String,
    },
    /// 끝난 pane 기록을 지운다
    Forget {
        pane: String,
    },
}

#[derive(Subcommand)]
enum WsCmd {
    /// workspace의 탭·나누기
    Show {
        #[arg(long, short)]
        workspace: Option<String>,
    },
    /// 새 탭에 새 pane(명령이 없으면 기본 셸)
    Tab {
        #[arg(long, short)]
        workspace: Option<String>,
        #[arg(long)]
        machine: Option<String>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        cwd: Option<String>,
        #[arg(last = true)]
        argv: Vec<String>,
    },
    /// pane 자리를 나눠 새 pane을 둔다(h: 좌우, v: 위아래)
    Split {
        pane: String,
        #[arg(long, value_parser = ["h", "v"], default_value = "h")]
        dir: String,
        #[arg(long, short)]
        workspace: Option<String>,
        #[arg(long)]
        machine: Option<String>,
        #[arg(long)]
        cwd: Option<String>,
        #[arg(last = true)]
        argv: Vec<String>,
    },
    /// 이미 있는 pane을 새 탭으로 넣는다
    Add {
        pane: String,
        #[arg(long, short)]
        workspace: Option<String>,
    },
    /// layout에서 뺀다(--kill이면 세션도 끝낸다)
    Close {
        pane: String,
        #[arg(long, short)]
        workspace: Option<String>,
        #[arg(long)]
        kill: bool,
    },
}

#[derive(Subcommand)]
enum MachineCmd {
    List,
    /// 등록 창을 연다(예: --open 10m)
    Enroll {
        #[arg(long, default_value = "10m")]
        open: String,
    },
    /// 승인을 기다리는 등록
    Pending,
    /// agent 화면의 확인 코드로 승인한다
    Approve {
        name: String,
        #[arg(long)]
        code: String,
    },
    Revoke {
        name: String,
    },
}

#[derive(Subcommand)]
enum WebCmd {
    /// 웹 pairing 코드(5분, 1회용)
    Pair {
        /// 보기 권한만 준다
        #[arg(long)]
        reader: bool,
    },
    /// tailscale serve로 HTTPS 공개(off: aa가 만든 공개만 끈다)
    Publish {
        #[arg(value_parser = ["off"])]
        off: Option<String>,
    },
    Status,
}

#[derive(Args)]
struct DoctorArgs {
    #[command(subcommand)]
    cmd: Option<DoctorCmd>,
}

#[derive(Subcommand)]
enum DoctorCmd {
    /// 지금 프로세스 계보에서 측정한다(P0 E1·E2)
    Probe {
        /// 첫 실행 표본 수(새 실행 파일 수)
        #[arg(long, default_value_t = 30)]
        samples: u32,
        /// 실제로 읽어 볼 키체인 generic password의 service 이름
        #[arg(long)]
        keychain_service: Option<String>,
        /// 읽기·쓰기를 확인할 폴더(외장 볼륨)
        #[arg(long)]
        volume_dir: Option<String>,
        /// 결과에 붙일 조건 이름
        #[arg(long)]
        label: Option<String>,
        /// 결과 JSON을 이 파일에 쓴다
        #[arg(long)]
        out: Option<PathBuf>,
        /// warm·처리량을 비교할 기준선 결과 JSON
        #[arg(long)]
        baseline: Option<PathBuf>,
        /// 처리량 측정을 건너뛴다
        #[arg(long)]
        no_throughput: bool,
        /// 전체 디스크 접근(FDA)이 필요한 파일을 읽어 본다
        #[arg(long)]
        fda_check: bool,
        /// 결과를 상태 폴더 probe/results/<label>-<시각>.json에도 쓴다(LaunchAgent 실행용)
        #[arg(long)]
        out_state: bool,
        /// 음성 대조: 책임을 버린(disclaim) 자식으로 다시 실행해 잰다. 느리게 나와야 정상이다.
        #[arg(long)]
        disclaim: bool,
    },
    /// 결과 JSON 여러 개를 한 표로
    Summary { files: Vec<PathBuf> },
    /// 다른 프로세스의 책임 프로세스(E6: codex app-server·빌드 캐시 daemon 등)
    Lineage {
        /// 명령줄에 이 문자열이 들어 있는 프로세스를 찾는다(여러 번)
        #[arg(long = "match", default_values_t = ["app-server".to_string(), "kache".to_string(), "sccache".to_string(), "gradle".to_string(), "bazel".to_string()])]
        patterns: Vec<String>,
    },
}

#[derive(Subcommand)]
enum ServiceCmd {
    /// 바꾸지 않고 무엇을 등록하는지 보인다
    Plan {
        #[arg(long, default_value = service::AGENT_LABEL)]
        label: String,
    },
    /// 등록한다(시스템 설정 변경: 사용자가 직접 실행)
    Register {
        #[arg(long, default_value = service::AGENT_LABEL)]
        label: String,
    },
    Unregister {
        #[arg(long, default_value = service::AGENT_LABEL)]
        label: String,
    },
    Status {
        #[arg(long, default_value = service::AGENT_LABEL)]
        label: String,
    },
    /// 시스템 설정의 로그인 항목 화면을 연다
    OpenSettings,
}

#[derive(Subcommand)]
enum ConfigCmd {
    /// host.toml·agent.toml(+ .local.toml)을 검증한다
    Check,
    /// 예시 설정을 출력한다
    Example {
        #[arg(value_parser = ["host", "agent"])]
        role: String,
    },
    /// schema 갱신 계획(지금은 schema 1뿐)
    Migrate,
}

pub fn main() -> ExitCode {
    let cli = Cli::parse();
    if let Some(d) = &cli.config_dir {
        // 하위 프로세스(session host 등)도 같은 위치를 쓰게 한다.
        unsafe { std::env::set_var("AA_CONFIG_DIR", d) };
    }
    if let Some(d) = &cli.state_dir {
        unsafe { std::env::set_var("AA_STATE_DIR", d) };
    }
    match run(cli) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("aa: {e:#}");
            ExitCode::from(1)
        }
    }
}

fn init_log(default: &str) {
    let filter = tracing_subscriber::EnvFilter::try_from_env("AA_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(default));
    use std::io::IsTerminal;
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal())
        .with_target(false)
        .try_init();
}

fn runtime() -> Result<tokio::runtime::Runtime> {
    Ok(tokio::runtime::Builder::new_multi_thread().enable_all().build()?)
}

fn run(cli: Cli) -> Result<ExitCode> {
    let json = cli.json;
    match cli.cmd {
        Cmd::Host => {
            init_log("info");
            let cfg = load_or_exit(aa_config::load_host(&aa_config::paths::config_dir()))?;
            runtime()?.block_on(aa_host::run(cfg, aa_config::paths::state_dir()))?;
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Agent(a) => {
            init_log("info");
            agent_cmd(a)
        }
        Cmd::SessionHost { dir } => {
            init_log("info");
            aa_session::host::main(dir)?;
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Status => front(json, FrontRequest::Status),
        Cmd::Pane { cmd } => front(
            json,
            match cmd {
                PaneCmd::List { role } => FrontRequest::Panes {
                    role: match role {
                        Some(r) => Some(Role::parse(&r).with_context(|| format!("모르는 역할: {r}"))?),
                        None => None,
                    },
                },
                PaneCmd::Run { machine, name, cwd, cols, rows, argv } => {
                    FrontRequest::PaneRun { machine, name, argv, cwd, cols, rows }
                }
                PaneCmd::Read { pane } => FrontRequest::PaneRead { pane },
                PaneCmd::Send { pane, text, enter } => {
                    // 글과 Enter를 한 덩어리로 보내면 Codex는 붙여넣기로 보고 Enter를 줄바꿈으로 넣는다(실측).
                    // 글을 먼저 보내고 조금 뒤 Enter를 따로 보낸다(host의 brief·메시지 전달과 같은 간격).
                    if enter && !text.is_empty() {
                        let first = FrontRequest::PaneSend { pane: pane.clone(), data: B64(text.into_bytes()) };
                        if let Err(e) = front_call(first)? {
                            return front_print(json, Err(e));
                        }
                        std::thread::sleep(std::time::Duration::from_millis(300));
                        FrontRequest::PaneSend { pane, data: B64(b"\r".to_vec()) }
                    } else {
                        let mut data = text.into_bytes();
                        if enter {
                            data.push(b'\r');
                        }
                        FrontRequest::PaneSend { pane, data: B64(data) }
                    }
                }
                PaneCmd::Resize { pane, cols, rows } => FrontRequest::PaneResize { pane, cols, rows },
                PaneCmd::Kill { pane } => FrontRequest::PaneKill { pane },
                PaneCmd::Resume { pane } => FrontRequest::PaneResume { pane },
                PaneCmd::Forget { pane } => FrontRequest::PaneForget { pane },
            },
        ),
        Cmd::Ws { cmd } => front(
            json,
            match cmd {
                WsCmd::Show { workspace } => FrontRequest::Workspace { name: workspace },
                WsCmd::Tab { workspace, machine, name, cwd, argv } => {
                    FrontRequest::LayoutApply { workspace, change: LayoutChange::NewTab { machine, name, argv, cwd } }
                }
                WsCmd::Split { pane, dir, workspace, machine, cwd, argv } => FrontRequest::LayoutApply {
                    workspace,
                    change: LayoutChange::Split {
                        pane,
                        dir: if dir == "v" { SplitDir::Vertical } else { SplitDir::Horizontal },
                        machine,
                        argv,
                        cwd,
                    },
                },
                WsCmd::Add { pane, workspace } => {
                    FrontRequest::LayoutApply { workspace, change: LayoutChange::AddTab { pane } }
                }
                WsCmd::Close { pane, workspace, kill } => {
                    FrontRequest::LayoutApply { workspace, change: LayoutChange::Close { pane, kill } }
                }
            },
        ),
        Cmd::Project { cmd } => front(
            json,
            match cmd {
                ProjectCmd::List => FrontRequest::Projects,
                ProjectCmd::Create { name } => FrontRequest::ProjectCreate { name },
            },
        ),
        Cmd::Decision { cmd } => front(
            json,
            match cmd {
                DecisionCmd::Add { project, text, text_file, replaces } => {
                    FrontRequest::DecisionAdd { project, text: text_arg(text, text_file)?, replaces }
                }
                DecisionCmd::List { project } => FrontRequest::Decisions { project },
            },
        ),
        Cmd::Context { project } => front(json, FrontRequest::Context { project }),
        Cmd::Assignable { project, check } => front(json, FrontRequest::Assignable { project, check }),
        Cmd::Inbox { cmd } => front(
            json,
            match cmd {
                InboxCmd::List { project, all, peek } => FrontRequest::Inbox { project, all, peek },
                InboxCmd::Done { project, ids, all } => FrontRequest::InboxDone { project, ids, all },
            },
        ),
        Cmd::Merge { cmd } => front(
            json,
            match cmd {
                MergeCmd::Enqueue { project, thread } => FrontRequest::MergeEnqueue { project, thread },
                MergeCmd::List { project } => FrontRequest::MergeList { project },
                MergeCmd::Done { project, thread } => FrontRequest::MergeDrop { project, thread, landed: true },
                MergeCmd::Drop { project, thread } => FrontRequest::MergeDrop { project, thread, landed: false },
            },
        ),
        Cmd::Routine { cmd: RoutineCmd::List { project } } => front(json, FrontRequest::Routines { project }),
        Cmd::Routine { cmd: RoutineCmd::Approve { project, name } } => routine_approve(json, project, name),
        Cmd::Profile { cmd: ProfileCmd::List { machine } } => front(json, FrontRequest::Profiles { machine }),
        Cmd::Thread { cmd } => front(
            json,
            match cmd {
                ThreadCmd::Start { project, title, machine, profile, kind, repo, base, task_file, from_task } => {
                    if from_task.is_none() && (title.is_none() || profile.is_none() || task_file.is_none()) {
                        anyhow::bail!("--title·--profile·--task-file이 필요하다(TASKS.md task로 시작하면 --from-task)");
                    }
                    let task = match task_file.as_deref() {
                        None => String::new(),
                        Some("-") => {
                            let mut t = String::new();
                            std::io::Read::read_to_string(&mut std::io::stdin(), &mut t)?;
                            t
                        }
                        Some(f) => std::fs::read_to_string(f).with_context(|| format!("할 일 파일: {f}"))?,
                    };
                    let kind = if kind == "dir" { ThreadKind::Dir } else { ThreadKind::Worktree };
                    FrontRequest::ThreadStart {
                        project,
                        title: title.unwrap_or_default(),
                        machine,
                        profile: profile.unwrap_or_default(),
                        kind,
                        repo,
                        base,
                        task,
                        from_task,
                    }
                }
                ThreadCmd::Next { project, thread, line, add } => {
                    FrontRequest::ThreadNext { project, thread, line, add }
                }
                ThreadCmd::List { project } => FrontRequest::Threads { project },
                ThreadCmd::Show { project, thread } => FrontRequest::ThreadShow { project, thread },
                ThreadCmd::Library { project, thread } => FrontRequest::ThreadLibrary { project, thread },
                ThreadCmd::Ack { project, thread } => FrontRequest::ThreadAck { project, thread },
                ThreadCmd::Resolve { project, thread, keep_worktree } => {
                    FrontRequest::ThreadResolve { project, thread, keep_worktree }
                }
                ThreadCmd::Brief { project, thread } => FrontRequest::ThreadBrief { project, thread },
                ThreadCmd::Stop { project, thread } => FrontRequest::ThreadStop { project, thread },
                ThreadCmd::Restart { project, thread, resume } => {
                    FrontRequest::ThreadRestart { project, thread, resume }
                }
                ThreadCmd::Message { project, thread, interrupt, text_file, text } => {
                    let text = text_arg(text, text_file)?;
                    let mode = if interrupt { aa_proto::MessageMode::Interrupt } else { aa_proto::MessageMode::Queue };
                    FrontRequest::ThreadMessage { project, thread, text, mode }
                }
            },
        ),
        Cmd::Events { project, after, follow } => events_cmd(json, project, after, follow),
        Cmd::Hook { agent, payload } => {
            // harness를 막지 않는다: 무엇이 실패해도 종료 코드 0, 출력 없음
            let body = match payload {
                Some(p) => p,
                None => {
                    let mut t = String::new();
                    let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut t);
                    t
                }
            };
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&body)
                && let Ok(rt) = runtime()
                && let Err(e) = rt.block_on(aa_agent::hooks::send(&agent, v))
            {
                eprintln!("aa hook: {e:#}");
            }
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Report { percent, activity } => {
            let v = serde_json::json!({ "percent": percent, "activity": activity });
            runtime()?.block_on(aa_agent::hooks::send("report", v))?;
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Question { cmd } => front(
            json,
            match cmd {
                QuestionCmd::List { all } => FrontRequest::Questions { all },
                QuestionCmd::Answer { id, choice } => FrontRequest::QuestionAnswer { id, choice },
            },
        ),
        Cmd::Attach { workspace } => {
            init_log("warn");
            aa_tui::run(workspace)
        }
        Cmd::Machine { cmd } => front(
            json,
            match cmd {
                MachineCmd::List => FrontRequest::Machines,
                MachineCmd::Enroll { open } => FrontRequest::EnrollOpen { minutes: parse_minutes(&open)? },
                MachineCmd::Pending => FrontRequest::EnrollPending,
                MachineCmd::Approve { name, code } => FrontRequest::EnrollApprove { name, code },
                MachineCmd::Revoke { name } => FrontRequest::MachineRevoke { name },
            },
        ),
        Cmd::Web { cmd } => front(
            json,
            match cmd {
                WebCmd::Pair { reader } => {
                    FrontRequest::WebPair { role: if reader { DeviceRole::Reader } else { DeviceRole::Controller } }
                }
                WebCmd::Publish { off: Some(_) } => FrontRequest::WebPublishOff,
                WebCmd::Publish { off: None } => FrontRequest::WebPublish,
                WebCmd::Status => FrontRequest::WebStatus,
            },
        ),
        Cmd::Doctor(a) => doctor_cmd(a, json),
        Cmd::Service { cmd } => service_cmd(cmd),
        Cmd::Config { cmd } => config_cmd(cmd, json),
    }
}

fn doctor_cmd(a: DoctorArgs, json: bool) -> Result<ExitCode> {
    match a.cmd {
        None => {
            let r = doctor::probe(&doctor::ProbeOptions { samples: 3, throughput: false, ..Default::default() });
            print_report(&r, json)?;
            Ok(ExitCode::SUCCESS)
        }
        Some(DoctorCmd::Probe { disclaim: true, .. }) => {
            #[cfg(target_os = "macos")]
            {
                let args: Vec<String> = std::env::args().filter(|a| a != "--disclaim").collect();
                let code = doctor::macos::rerun_disclaimed(&args)?;
                Ok(ExitCode::from(code as u8))
            }
            #[cfg(not(target_os = "macos"))]
            anyhow::bail!("--disclaim은 macOS 전용이다")
        }
        Some(DoctorCmd::Probe {
            samples,
            keychain_service,
            volume_dir,
            label,
            out,
            baseline,
            no_throughput,
            fda_check,
            out_state,
            disclaim: false,
        }) => {
            let mut r = doctor::probe(&doctor::ProbeOptions {
                samples,
                keychain_service,
                volume_dir,
                label,
                throughput: !no_throughput,
                activity_held: false,
                fda_check,
            });
            if let Some(b) = baseline {
                let base: doctor::ProbeReport =
                    aa_common::util::read_json(&b).with_context(|| format!("기준선 {}", b.display()))?;
                let worse = doctor::compare_throughput(&base, &r);
                if !worse.is_empty() {
                    r.reasons.extend(worse.into_iter().map(|w| format!("처리량 gate 실패: {w}")));
                    r.pass = false;
                }
            }
            let mut outs: Vec<PathBuf> = out.into_iter().collect();
            if out_state {
                let name = format!("{}-{}.json", r.label.clone().unwrap_or("probe".into()), r.created_at_ms);
                outs.push(aa_config::paths::state_dir().join("probe/results").join(name));
            }
            for out in &outs {
                if let Some(dir) = out.parent() {
                    std::fs::create_dir_all(dir)?;
                }
                std::fs::write(out, serde_json::to_vec_pretty(&r)?)?;
            }
            print_report(&r, json)?;
            Ok(if r.pass { ExitCode::SUCCESS } else { ExitCode::from(2) })
        }
        Some(DoctorCmd::Summary { files }) => {
            let mut rows = Vec::new();
            for f in files {
                let r: doctor::ProbeReport =
                    aa_common::util::read_json(&f).with_context(|| format!("{}", f.display()))?;
                rows.push((f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), r));
            }
            print!("{}", doctor::summary_table(&rows));
            Ok(ExitCode::SUCCESS)
        }
        Some(DoctorCmd::Lineage { patterns }) => {
            #[cfg(target_os = "macos")]
            {
                let rows = doctor::macos::daemons(&patterns);
                if json {
                    let v: Vec<_> = rows
                        .iter()
                        .map(|(pid, cmd, rp, path)| serde_json::json!({"pid":pid,"command":cmd,"responsible_pid":rp,"responsible_path":path}))
                        .collect();
                    println!("{}", serde_json::to_string_pretty(&v)?);
                } else if rows.is_empty() {
                    println!("해당 프로세스 없음: {patterns:?}");
                } else {
                    for (pid, cmd, rp, path) in rows {
                        println!("{pid}\t책임 {}\t{}\n\t{cmd}", rp.unwrap_or(-1), path.unwrap_or_default());
                    }
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = (patterns, json);
                println!("macOS에서만 의미가 있다");
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn load_or_exit<T>(r: Result<aa_config::Loaded<T>, aa_config::ConfigErrors>) -> Result<T> {
    match r {
        Ok(l) => {
            for w in &l.warnings {
                eprintln!("{w}");
            }
            Ok(l.config)
        }
        Err(e) => anyhow::bail!("설정을 읽지 못해 시작하지 않는다:\n{}", e.render()),
    }
}

fn parse_minutes(s: &str) -> Result<u32> {
    let t = s.trim_end_matches('m');
    t.parse::<u32>().with_context(|| format!("분 단위로 적는다(예: 10m): {s}"))
}

fn agent_cmd(a: AgentArgs) -> Result<ExitCode> {
    let cfg = load_or_exit(aa_config::load_agent(&aa_config::paths::config_dir()))?;
    let root = aa_config::paths::state_dir();
    let state = root.join("agent");
    let ident = aa_agent::identity::load_or_create(&state)?;
    let name = a
        .name
        .clone()
        .filter(|n| !n.is_empty())
        .or_else(|| Some(cfg.agent.name.clone()).filter(|n| !n.is_empty()))
        .unwrap_or_else(aa_common::util::hostname);
    let rt = runtime()?;
    if let Some(AgentCmd::Enroll { host, name: n }) = a.cmd {
        let name = n.unwrap_or(name);
        let pin = rt.block_on(aa_agent::link::enroll(&state, &host, &ident.machine_id, &name))?;
        println!("등록됐다: {name} → {} (host fingerprint {}…)", pin.address, &pin.fingerprint[..16]);
        println!("agent.toml의 [agent] host = \"{}\"로 두고 `aa agent`를 실행한다", pin.address);
        return Ok(ExitCode::SUCCESS);
    }
    let host = a
        .host
        .clone()
        .unwrap_or_else(|| if cfg.agent.host.is_empty() { "local".into() } else { cfg.agent.host.clone() });
    let target = if host == "local" {
        aa_agent::link::Target::Local(root.join("host").join("agent.sock"))
    } else {
        let pin = aa_agent::identity::load_pin(&state)
            .context("이 머신은 아직 등록되지 않았다: `aa agent enroll <host:port>`")?;
        let id = aa_common::tls::load_or_create(&state, "aa-agent")?;
        aa_agent::link::Target::Remote { addr: host.clone(), pin: pin.fingerprint, id }
    };
    let default_cwd = Some(cfg.agent.default_cwd.clone())
        .filter(|c| !c.is_empty())
        .or_else(|| std::env::var("HOME").ok().or_else(|| std::env::var("USERPROFILE").ok()));
    rt.block_on(async move {
        let agent = aa_agent::Agent::new(state, ident.machine_id, name, a.dev_mode, default_cwd)?;
        let work_root = if cfg.agent.work_root.is_empty() {
            aa_config::paths::home().join("aa-work")
        } else {
            PathBuf::from(&cfg.agent.work_root)
        };
        agent.configure_threads(aa_agent::threads::ThreadsConfig {
            work_root,
            exe: std::env::current_exe()?,
            profiles: cfg.profiles.clone(),
        });
        tracing::info!(machine = %agent.name, id = %agent.machine_id, %target, "agent 시작");
        agent.recover().await?;
        agent.start_hooks()?;
        aa_agent::start_selfcheck(&agent, &cfg.selfcheck);
        agent.start_health(cfg.health.clone());
        let max = std::time::Duration::from_secs(cfg.reconnect.max_backoff_s);
        tokio::select! {
            _ = aa_agent::link::run(agent.clone(), target, max) => {}
            _ = aa_host::shutdown_signal() => tracing::info!("agent 종료(세션은 session host가 유지한다)"),
        }
        anyhow::Ok(())
    })?;
    Ok(ExitCode::SUCCESS)
}

/// routine 명령 승인: agent가 대신 승인하지 못하게, aa 세션 안이거나 터미널이 아니면 거부하고 사람에게 이름을 치게 한다.
/// 남는 한계: 사람의 터미널을 흉내 내는 다른 수단(aa 밖의 PTY)까지 막지는 못한다(plan §8.19).
fn routine_approve(json: bool, project: String, name: String) -> Result<ExitCode> {
    use std::io::{BufRead as _, IsTerminal as _, Write as _};
    if std::env::var_os("AA_SESSION_ID").is_some() || std::env::var_os("AA_HOOK_SOCK").is_some() {
        anyhow::bail!("aa 세션 안에서는 routine 명령을 승인할 수 없다. 사람이 host 머신의 일반 터미널에서 한다");
    }
    if !std::io::stdin().is_terminal() {
        anyhow::bail!("`aa routine approve`는 사람이 터미널에서 실행해야 한다(표준 입력이 터미널이 아니다)");
    }
    let rt = runtime()?;
    let list = rt.block_on(async {
        let mut c = aa_common::front::FrontClient::connect(&aa_config::paths::state_dir()).await?;
        c.call(FrontRequest::Routines { project: project.clone() }).await
    })?;
    let routines = match list {
        Ok(FrontResponse::Routines { routines }) => routines,
        Ok(_) => anyhow::bail!("예상하지 못한 응답"),
        Err(e) => anyhow::bail!("{e}"),
    };
    let r = routines
        .into_iter()
        .find(|r| r.name == name)
        .ok_or_else(|| anyhow::anyhow!("routine이 없다: {project}/{name}"))?;
    if let Some(e) = &r.error {
        anyhow::bail!("routine 파일을 읽지 못한다: {e}");
    }
    if r.command.is_empty() {
        anyhow::bail!("명령이 없는 routine이다. 승인할 것이 없다");
    }
    let machine = r.machine.as_deref().unwrap_or("host 머신");
    println!("routine `{name}`({project})은 {machine}에서 이 명령을 `sh -c`로 실행한다. 일정: {}\n", r.schedule);
    println!("    {}\n", r.command);
    if !r.commands_allowed {
        println!(
            "지금은 host.toml [projects] routine_commands가 이 프로젝트를 허용하지 않아, 승인해도 그것을 켜기 전에는 돌지 않는다.\n"
        );
    }
    print!("승인하려면 routine 이름을 친다: ");
    std::io::stdout().flush()?;
    let mut typed = String::new();
    std::io::stdin().lock().read_line(&mut typed)?;
    if typed.trim() != name {
        anyhow::bail!("이름이 다르다. 승인하지 않았다");
    }
    front(json, FrontRequest::RoutineApprove { project, name, command_sha256: r.command_sha256 })
}

fn front(json: bool, req: FrontRequest) -> Result<ExitCode> {
    front_print(json, front_call(req)?)
}

fn front_call(req: FrontRequest) -> Result<Result<FrontResponse, aa_proto::ErrorBody>> {
    runtime()?.block_on(async {
        let mut c = aa_common::front::FrontClient::connect(&aa_config::paths::state_dir()).await?;
        c.call(req).await
    })
}

fn front_print(json: bool, res: Result<FrontResponse, aa_proto::ErrorBody>) -> Result<ExitCode> {
    match res {
        Ok(r) => {
            if json {
                println!("{}", serde_json::to_string_pretty(&r)?);
            } else {
                print!("{}", render_front(&r));
            }
            Ok(ExitCode::SUCCESS)
        }
        Err(e) => {
            if json {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "error": e }))?);
            } else {
                eprintln!("aa: {e}");
            }
            Ok(ExitCode::from(3))
        }
    }
}

/// 흐름 사건: cursor 다음을 읽고, `follow`면 새 사건을 계속 보인다
fn events_cmd(json: bool, project: Option<String>, after: u64, follow: bool) -> Result<ExitCode> {
    let rt = runtime()?;
    rt.block_on(async {
        let mut c = aa_common::front::FrontClient::connect(&aa_config::paths::state_dir()).await?;
        let first = c.call(FrontRequest::Events { project: project.clone(), after, limit: None }).await?;
        let events = match first {
            Ok(FrontResponse::Events { events }) => events,
            Ok(_) => anyhow::bail!("예상하지 못한 응답"),
            Err(e) => {
                eprintln!("aa: {e}");
                return Ok(ExitCode::from(3));
            }
        };
        let show = |e: &aa_proto::FlowEvent| -> Result<()> {
            if json {
                println!("{}", serde_json::to_string(e)?);
            } else {
                print!("{}", render_event(e));
            }
            Ok(())
        };
        if !follow {
            // 한 번 읽기: 다른 명령처럼 JSON 객체 하나(따라 읽기는 사건마다 한 줄)
            if json {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "events": events }))?);
            } else {
                events.iter().for_each(|e| print!("{}", render_event(e)));
            }
            return Ok(ExitCode::SUCCESS);
        }
        let mut seen: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
        for e in &events {
            show(e)?;
            seen.insert(e.project.clone(), e.seq);
        }
        while let Some(m) = c.next().await? {
            if let aa_proto::FrontMsg::Event(aa_proto::FrontEvent::Flow(e)) = m
                && project.as_ref().is_none_or(|p| *p == e.project)
                && seen.get(&e.project).is_none_or(|s| e.seq > *s)
            {
                show(&e)?;
                seen.insert(e.project.clone(), e.seq);
            }
        }
        Ok(ExitCode::SUCCESS)
    })
}

fn render_event(e: &aa_proto::FlowEvent) -> String {
    format!(
        "{}#{}\t{}\t{}\t{}\t{}{}\n",
        e.project,
        e.seq,
        e.kind,
        e.thread.clone().unwrap_or_default(),
        e.machine.clone().unwrap_or_default(),
        e.detail,
        e.evidence.as_ref().map(|p| format!("\t({p})")).unwrap_or_default()
    )
}

fn render_question(s: &mut String, q: &aa_proto::QuestionView) {
    use std::fmt::Write as _;
    let at = match (&q.project, &q.thread) {
        (Some(p), Some(t)) => format!("{p}/{t}"),
        _ => q.pane.clone(),
    };
    let state = match q.state {
        aa_proto::QuestionState::Open => "열림",
        aa_proto::QuestionState::Answered => "답함",
        aa_proto::QuestionState::Gone => "사라짐",
    };
    let _ = writeln!(
        s,
        "{}\t{}\t{}@{}\t{state}{}\t{}\t답: {}",
        q.id,
        q.question_kind,
        at,
        q.machine,
        q.answer.as_ref().map(|a| format!("({a})")).unwrap_or_default(),
        q.text,
        q.options.join("·")
    );
}

/// `--text` 또는 `--text-file`(`-`면 표준 입력)
fn text_arg(text: Option<String>, file: Option<String>) -> Result<String> {
    Ok(match (text, file) {
        (Some(t), _) => t,
        (None, Some(f)) if f == "-" => {
            let mut t = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut t)?;
            t
        }
        (None, Some(f)) => std::fs::read_to_string(&f).with_context(|| format!("글 파일: {f}"))?,
        (None, None) => anyhow::bail!("--text 또는 --text-file이 필요하다"),
    })
}

fn render_decision(s: &mut String, d: &aa_proto::DecisionView) {
    use std::fmt::Write as _;
    let mut tags = String::new();
    if let Some(r) = d.replaces {
        tags.push_str(&format!(" · {r}번 대체"));
    }
    if let Some(b) = d.replaced_by {
        tags.push_str(&format!(" · {b}번으로 대체됨"));
    }
    let _ = writeln!(s, "{}\t{}{tags}", d.num, d.at);
    for l in d.text.lines() {
        let _ = writeln!(s, "  {l}");
    }
}

fn render_thread(s: &mut String, t: &aa_proto::ThreadView) {
    use std::fmt::Write as _;
    let group = match t.group {
        aa_proto::ThreadGroup::WaitingOnYou => "사람 차례",
        aa_proto::ThreadGroup::ReadyForReview => "검토 대기",
        aa_proto::ThreadGroup::Working => "작업 중",
        aa_proto::ThreadGroup::Resolved => "끝남",
    };
    let _ = writeln!(
        s,
        "{}/{}\t{}\t{}\t{}@{}\t[{}]\tbrief {}",
        t.project,
        t.id,
        group,
        t.title,
        t.profile,
        t.machine,
        t.report_state.clone().unwrap_or("-".into()),
        t.brief.as_str()
    );
    if let Some(e) = &t.error {
        let _ = writeln!(s, "  오류: {e}");
    }
    if let (Some(now), Some(had)) = (t.decisions_now, t.decisions_rev.or(Some(0)))
        && had < now
        && t.status != aa_proto::ThreadStatus::Resolved
    {
        let _ = writeln!(s, "  결정 {had}번까지 보고 시작했다(지금 {now}번). 새 결정을 알린다: aa thread message");
    }
    for n in &t.next {
        let _ = writeln!(s, "  - {n}");
    }
}

fn state_label(s: &aa_proto::SessionState) -> String {
    match s {
        aa_proto::SessionState::Running => "실행".into(),
        aa_proto::SessionState::Exited { code } => {
            format!("종료({})", code.map(|c| c.to_string()).unwrap_or("?".into()))
        }
        aa_proto::SessionState::Interrupted => "중단(재부팅)".into(),
        aa_proto::SessionState::Lost => "유실".into(),
    }
}

/// 머신 건강 한 칸: 이상이면 그 이유, 아니면 디스크 여유·부하·주의
fn health_text(m: &aa_proto::MachineView) -> String {
    if !m.problems.is_empty() {
        return format!("\t이상: {}", m.problems.iter().map(|p| p.text.as_str()).collect::<Vec<_>>().join("; "));
    }
    let Some(h) = &m.health else { return String::new() };
    let mut parts = Vec::new();
    if let Some(free) = h.disk_free_bytes {
        parts.push(format!("디스크 여유 {:.0}GB", free as f64 / (1u64 << 30) as f64));
    }
    if let Some(l) = h.load_x100 {
        parts.push(format!("부하 {:.2}/{}", l as f64 / 100.0, h.cpus));
    }
    parts.extend(h.warnings.iter().cloned());
    format!("\t{}", parts.join(" · "))
}

/// `claude, codex @mac-b: claude|codex, @mac-c(끊김, 모름)`
fn assignable_line(owners: &[aa_proto::OwnerChoice]) -> String {
    let parts: Vec<String> = owners
        .iter()
        .map(|o| {
            if !o.connected {
                format!("@{}(끊김, 모름)", o.machine)
            } else {
                format!("@{}: {}", o.machine, if o.profiles.is_empty() { "-".into() } else { o.profiles.join("|") })
            }
        })
        .collect();
    format!("맡길 수 있는 owner(profile@machine): {}", parts.join(", "))
}

fn render_context(s: &mut String, c: &aa_proto::ProjectContext) {
    use std::fmt::Write as _;
    let _ = writeln!(s, "프로젝트 {} ({})", c.project, c.path);
    let _ = writeln!(s, "thread 시작: start_threads={} (propose면 제안하고 승인을 기다린다)", c.start_threads);
    let _ = writeln!(s, "\nTasks(TASKS.md, coordinator만 고친다):");
    if c.tasks.is_empty() {
        s.push_str("  (없음)\n");
    }
    for t in &c.tasks {
        let owner = t.owner.as_ref().map(|o| format!(" ({o})")).unwrap_or_default();
        let thread = t.thread.as_ref().map(|x| format!(" · {x}")).unwrap_or_default();
        let done = if t.done { " [x]" } else { "" };
        let _ = writeln!(s, "  [{}] {}{owner}{thread}{done}", t.list, t.title);
        if !t.notes.is_empty() {
            let n: String = t.notes.join(" ").chars().take(60).collect();
            let _ = writeln!(s, "    notes: {n}");
        }
    }
    let _ = writeln!(s, "\n열린 thread:");
    if c.threads.is_empty() {
        s.push_str("  (없음)\n");
    }
    for t in &c.threads {
        let state = t.agent.as_ref().map(|a| format!(" · {:?}", a.activity)).unwrap_or_default();
        let _ = writeln!(s, "  {} {:?} {} {}@{}{state}", t.id, t.group, t.title, t.profile, t.machine);
        for l in &t.next {
            let _ = writeln!(s, "    next: {l}");
        }
    }
    let _ = writeln!(s, "\n{}", assignable_line(&c.assignable));
    let _ = writeln!(s, "inbox: 끝내지 않은 항목 {}개 (aa inbox list {})", c.inbox_open, c.project);
}

fn render_front(r: &FrontResponse) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    let machines = |s: &mut String, ms: &[aa_proto::MachineView]| {
        for m in ms {
            let _ = writeln!(
                s,
                "{}\t{}\t{}{}{}{}",
                m.name,
                if m.connection == aa_proto::Connection::Connected { "연결" } else { "끊김" },
                m.os.clone().unwrap_or_default(),
                if m.local { "\t로컬" } else { "" },
                if m.dev_mode { "\t개발 모드" } else { "" },
                health_text(m)
            );
        }
    };
    match r {
        FrontResponse::Status(h) => {
            let _ = writeln!(
                s,
                "host {} · {} · {}",
                h.host_id,
                h.software,
                if h.dev_mode { "개발 모드(로컬 agent를 host가 띄움)" } else { "운영 모드" }
            );
            let _ = writeln!(
                s,
                "Agent link: {} · 웹: {}",
                h.agent_listen.clone().unwrap_or("-".into()),
                h.web_listen.clone().unwrap_or("-".into())
            );
            machines(&mut s, &h.machines);
        }
        FrontResponse::Machines { machines: ms } => machines(&mut s, ms),
        FrontResponse::Panes { panes } => {
            for p in panes {
                let _ = writeln!(
                    s,
                    "{}\t{}\t{}\t{}{}\t{}",
                    p.id,
                    p.machine,
                    p.name,
                    state_label(&p.state),
                    if p.connection == aa_proto::Connection::Disconnected { "·끊김(읽기 전용)" } else { "" },
                    p.argv.join(" ")
                );
            }
        }
        FrontResponse::Pane(p) => {
            let _ = writeln!(s, "{}\t{}\t{}\t{}", p.id, p.machine, p.name, state_label(&p.state));
        }
        FrontResponse::Screen(t) => {
            let mut lines = t.lines.clone();
            while lines.last().is_some_and(|l| l.is_empty()) {
                lines.pop();
            }
            for l in lines {
                let _ = writeln!(s, "{l}");
            }
        }
        FrontResponse::Pending { pending } => {
            if pending.is_empty() {
                let _ = writeln!(s, "대기 중인 등록이 없다");
            }
            for p in pending {
                let _ = writeln!(
                    s,
                    "{}\tfingerprint {}…\t`aa machine approve {} --code <agent 화면의 코드>`",
                    p.name, p.fingerprint_prefix, p.name
                );
            }
        }
        FrontResponse::PairCode { code, role, expires_in_s } => {
            let _ = writeln!(s, "pairing 코드: {code} ({role:?}, {}분 유효, 1회용)", expires_in_s / 60);
        }
        FrontResponse::Publish(p) => {
            let _ = writeln!(
                s,
                "tailscale: {}{}",
                p.tailscale,
                p.dns_name.as_ref().map(|d| format!(" ({d})")).unwrap_or_default()
            );
            let _ = writeln!(s, "주소: {}", p.url.clone().unwrap_or("-".into()));
            if let Some(port) = p.owned_port {
                let _ = writeln!(s, "aa가 만든 serve 포트: {port} → 127.0.0.1:{}", p.web_port);
            }
            if let Some(m) = &p.message {
                let _ = writeln!(s, "{m}");
            }
        }
        FrontResponse::Workspace(w) => {
            if w.tabs.is_empty() {
                let _ = writeln!(s, "workspace {}: 탭 없음(`aa ws tab`으로 만든다)", w.name);
            }
            for (i, t) in w.tabs.iter().enumerate() {
                let _ = writeln!(s, "탭 {}\t{}", i + 1, render_node(&t.root));
            }
        }
        FrontResponse::Done => s.push_str("완료\n"),
        FrontResponse::Projects { projects } => {
            if projects.is_empty() {
                let _ = writeln!(s, "프로젝트가 없다(`aa project create <이름>`)");
            }
            for p in projects {
                let _ = writeln!(s, "{}\tthread {}\t{}", p.name, p.threads, p.path);
            }
        }
        FrontResponse::Project(p) => {
            let _ = writeln!(s, "{}\t{}", p.name, p.path);
        }
        FrontResponse::Profiles { machine, profiles } => {
            if profiles.is_empty() {
                let _ = writeln!(s, "{machine}: agent.toml에 [profiles.<이름>]이 없다");
            }
            for p in profiles {
                let _ = writeln!(
                    s,
                    "{machine}\t{}\t{}\t{}\t{}\t{}",
                    p.name,
                    p.harness,
                    p.model.clone().unwrap_or("-".into()),
                    p.effort.clone().unwrap_or("-".into()),
                    p.description
                );
            }
        }
        FrontResponse::Threads { threads } => {
            if threads.is_empty() {
                let _ = writeln!(s, "thread가 없다");
            }
            for t in threads {
                render_thread(&mut s, t);
            }
        }
        FrontResponse::Thread(t) => render_thread(&mut s, t),
        FrontResponse::Events { events } => {
            for e in events {
                s.push_str(&render_event(e));
            }
        }
        FrontResponse::Questions { questions } => {
            if questions.is_empty() {
                let _ = writeln!(s, "열린 질문이 없다");
            }
            for q in questions {
                render_question(&mut s, q);
            }
        }
        FrontResponse::Question(q) => render_question(&mut s, q),
        FrontResponse::Message(m) => {
            let _ = writeln!(s, "{}/{} 메시지 {}: {:?} ({:?})", m.project, m.thread, m.id, m.state, m.mode);
        }
        FrontResponse::Decisions { decisions } => {
            if decisions.is_empty() {
                s.push_str("결정 기록이 없다\n");
            }
            for d in decisions {
                render_decision(&mut s, d);
            }
        }
        FrontResponse::Decision(d) => render_decision(&mut s, d),
        FrontResponse::Merge { queue } => {
            if queue.is_empty() {
                s.push_str("병합 대기열이 비었다\n");
            }
            for e in queue {
                let branch = e.branch.as_deref().unwrap_or("-");
                let _ =
                    writeln!(s, "{}\t{}/{}\t{}\t{}@{}", e.position, e.project, e.thread, e.title, branch, e.machine);
            }
        }
        FrontResponse::Routines { routines } => {
            if routines.is_empty() {
                s.push_str("routine이 없다\n");
            }
            for r in routines {
                if let Some(e) = &r.error {
                    let _ = writeln!(s, "{}\t쓸 수 없음: {e}", r.name);
                    continue;
                }
                let when = |t: Option<u64>| t.map(aa_common::util::utc_text).unwrap_or_else(|| "-".into());
                let cmd = if r.command.is_empty() {
                    "안내만".to_string()
                } else {
                    let ok = match (r.commands_allowed, r.approved) {
                        (true, true) => "승인됨",
                        (_, false) => "승인 필요",
                        (false, true) => "host.toml에서 꺼짐",
                    };
                    format!("명령 {ok}@{}", r.machine.as_deref().unwrap_or("host"))
                };
                let off = if r.enabled { "" } else { " · 꺼짐" };
                let _ = writeln!(
                    s,
                    "{}\t{}\t{cmd}{off}\t지난 {} · 다음 {} · 건너뜀 {} · coordinator 없음 {}",
                    r.name,
                    r.schedule,
                    when(r.last_run_ms),
                    when(r.next_run_ms),
                    r.skipped,
                    r.no_coordinator
                );
            }
        }
        FrontResponse::Library(l) => {
            let _ = writeln!(s, "{}/{} 결과물 {}개(보기 경로 2시간: {})", l.project, l.thread, l.files.len(), l.base);
            for f in &l.files {
                let at = if f.on_agent { " · agent에 있음" } else { "" };
                let _ = writeln!(s, "  {}\t{}B{at}", f.path, f.size);
            }
            if l.report.is_none() {
                s.push_str("회수한 보고가 없다\n");
            }
        }
        FrontResponse::Next { lines, message } => {
            if let Some(m) = message {
                let _ = writeln!(s, "보냈다({:?}): {}", m.state, m.text);
            } else if lines.is_empty() {
                s.push_str("Next 목록이 비었다\n");
            } else {
                for (n, l) in lines.iter().enumerate() {
                    let _ = writeln!(s, "{}. {l}", n + 1);
                }
            }
        }
        FrontResponse::Assignable { owners, valid } => match valid {
            Some(v) => {
                let _ = writeln!(s, "{v}");
            }
            None => {
                let _ = writeln!(s, "{}", assignable_line(owners));
            }
        },
        FrontResponse::Context(c) => render_context(&mut s, c),
        FrontResponse::Inbox { items } => {
            if items.is_empty() {
                s.push_str("inbox가 비었다\n");
            }
            for i in items {
                let mark = if i.done {
                    "끝남"
                } else if i.seen {
                    "봄"
                } else {
                    "새것"
                };
                let _ = writeln!(s, "{}\t{}\t{} {}\t{mark}", i.id, i.item_kind, i.subject, i.event);
                let _ = writeln!(s, "  {}", i.summary);
            }
        }
    }
    s
}

fn render_node(n: &aa_proto::LayoutNode) -> String {
    match n {
        aa_proto::LayoutNode::Pane { pane } => pane.clone(),
        aa_proto::LayoutNode::Split { dir, a, b, .. } => {
            let sep = if *dir == SplitDir::Horizontal { " | " } else { " / " };
            format!("({}{sep}{})", render_node(a), render_node(b))
        }
    }
}

fn print_report(r: &doctor::ProbeReport, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(r)?);
    } else {
        print!("{}", doctor::render(r));
    }
    Ok(())
}

fn service_cmd(cmd: ServiceCmd) -> Result<ExitCode> {
    match cmd {
        ServiceCmd::Plan { label } => print!("{}", service::plan(&label)?),
        ServiceCmd::Register { label } => println!("{label}: {}", service::register(&label)?),
        ServiceCmd::Unregister { label } => println!("{label}: {}", service::unregister(&label)?),
        ServiceCmd::Status { label } => println!("{label}: {}", service::status(&label)?),
        ServiceCmd::OpenSettings => service::open_settings(),
    }
    Ok(ExitCode::SUCCESS)
}

fn config_cmd(cmd: ConfigCmd, json: bool) -> Result<ExitCode> {
    let dir = aa_config::paths::config_dir();
    match cmd {
        ConfigCmd::Example { role } => {
            print!("{}", aa_config::example(&role));
            Ok(ExitCode::SUCCESS)
        }
        ConfigCmd::Migrate => {
            for stem in ["host", "agent"] {
                match aa_config::migrate_plan(&dir, stem) {
                    Ok(m) => println!("{m}"),
                    Err(e) => println!("{}", e.render()),
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        ConfigCmd::Check => {
            let mut ok = true;
            let mut report = Vec::new();
            let host = aa_config::load_host(&dir).map(|l| (l.files, l.warnings));
            let agent = aa_config::load_agent(&dir).map(|l| (l.files, l.warnings));
            for (name, r) in [("host", host), ("agent", agent)] {
                match r {
                    Ok((files, warnings)) => {
                        report.push(serde_json::json!({"role":name,"ok":true,
                            "files":files.iter().map(|f| f.display().to_string()).collect::<Vec<_>>(),
                            "warnings":warnings.iter().map(|w| w.to_string()).collect::<Vec<_>>()}));
                        if !json {
                            let used = if files.is_empty() {
                                "파일 없음(기본값)".to_string()
                            } else {
                                files.iter().map(|f| f.display().to_string()).collect::<Vec<_>>().join(", ")
                            };
                            println!("{name}: 정상 · {used}");
                            for w in warnings {
                                println!("  {w}");
                            }
                        }
                    }
                    Err(e) => {
                        ok = false;
                        report.push(serde_json::json!({"role":name,"ok":false,"errors":e.0.iter().map(|d| d.to_string()).collect::<Vec<_>>()}));
                        if !json {
                            println!("{name}: 오류\n{}", e.render());
                        }
                    }
                }
            }
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            }
            Ok(if ok { ExitCode::SUCCESS } else { ExitCode::from(2) })
        }
    }
}

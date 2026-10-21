//! CLI 값을 공유 DTO로 옮긴다. 선택 검증은 aaa-config의 공통 planner가 소유한다.
use aaa_proto::install::InstallInvocation;
use serde::Serialize;

#[derive(clap::Args, Serialize)]
pub struct Args {
    #[command(subcommand)]
    #[serde(skip)]
    action: Option<Action>,
    /// 외부 TTY에서 명시적으로 저장한 비밀 없는 선택 초안.
    #[arg(long)]
    #[serde(skip)]
    draft: Option<std::path::PathBuf>,
    #[command(flatten)]
    choices: Choices,
    #[arg(long)]
    non_interactive: bool,
    #[arg(long)]
    yes: bool,
    #[arg(long)]
    dry_run: bool,
    #[arg(long)]
    json_stream: bool,
    #[arg(long)]
    prompt: Option<String>,
    #[arg(long)]
    events_file: Option<String>,
    #[arg(long)]
    authorization_stdin: bool,
    #[arg(long)]
    authorization_file: Option<String>,
    #[arg(long)]
    secrets_stdin: bool,
    #[arg(long)]
    service_password_ref: Option<String>,
    #[arg(long)]
    resume: Option<String>,
    #[arg(long)]
    status: Option<String>,
}

#[derive(clap::Subcommand)]
pub(super) enum Action {
    /// aaa 밖 TTY에서 실제 계획을 검토하고 짧은 private 설치 승인을 발급한다.
    Authorize {
        #[arg(long, conflicts_with = "resume")]
        plan: Option<std::path::PathBuf>,
        #[arg(long)]
        out: std::path::PathBuf,
        /// 만료/중단된 같은 설치 ID를 원래 범위로 재검토한다.
        #[arg(long)]
        resume: Option<String>,
    },
}

#[derive(clap::Args, Serialize)]
struct Choices {
    #[arg(long)]
    role: Option<String>,
    #[arg(long)]
    mode: Option<String>,
    #[arg(long)]
    name: Option<String>,
    #[arg(long)]
    local_agent: Option<String>,
    #[arg(long)]
    bin_path: Option<String>,
    #[arg(long)]
    version: Option<String>,
    #[arg(long)]
    from: Option<String>,
    #[arg(long)]
    sha256: Option<String>,
    #[arg(long)]
    expected_os: Option<String>,
    #[arg(long)]
    expected_arch: Option<String>,
    #[arg(long)]
    project_mode: Option<String>,
    #[arg(long)]
    project: Option<String>,
    #[arg(long)]
    projects_root: Option<String>,
    #[arg(long)]
    goal: Option<String>,
    #[arg(long)]
    work_root: Option<String>,
    #[arg(long)]
    work_mount: Option<String>,
    #[arg(long)]
    work_storage: Option<String>,
    #[arg(long)]
    min_free_gb: Option<u32>,
    #[arg(long)]
    default_cwd: Option<String>,
    #[arg(long)]
    shell: Option<String>,
    #[arg(long)]
    claude_path: Option<String>,
    #[arg(long)]
    codex_path: Option<String>,
    #[arg(long)]
    claude_home: Option<String>,
    #[arg(long)]
    codex_home: Option<String>,
    #[arg(long)]
    login: Option<String>,
    #[arg(long)]
    hooks: Option<String>,
    #[arg(long)]
    thread_profile: Option<String>,
    #[arg(long)]
    coordinator_profile: Option<String>,
    #[arg(long)]
    service: Option<String>,
    #[arg(long)]
    service_account: Option<String>,
    #[arg(long)]
    service_password_source: Option<String>,
    #[arg(long)]
    startup: Option<String>,
    #[arg(long)]
    start: Option<String>,
    #[arg(long)]
    linger: Option<String>,
    #[arg(long)]
    path: Option<String>,
    #[arg(long)]
    launcher: Option<String>,
    #[arg(long)]
    launcher_policy: Option<String>,
    #[arg(long)]
    host: Option<String>,
    #[arg(long)]
    agent_listen: Option<String>,
    #[arg(long)]
    web_listen: Option<String>,
    #[arg(long)]
    network: Option<String>,
    #[arg(long)]
    web: Option<String>,
    #[arg(long)]
    tailscale_serve: Option<String>,
    #[arg(long)]
    serve_port: Option<u16>,
    #[arg(long)]
    pairing: Option<String>,
    #[arg(long)]
    enroll: Option<String>,
    #[arg(long)]
    wait_timeout: Option<u32>,
    #[arg(long)]
    open_coordinator: Option<String>,
    #[arg(long)]
    repo: Vec<String>,
    #[arg(long)]
    harness: Vec<String>,
    #[arg(long)]
    harness_env: Vec<String>,
    #[arg(long)]
    profile_file: Vec<String>,
    #[arg(long)]
    allow_thread_profile: Vec<String>,
    #[arg(long)]
    allow_coordinator_profile: Vec<String>,
    #[arg(long)]
    no_repo: bool,
}

impl Args {
    pub(super) fn draft(&self) -> Option<&std::path::Path> {
        self.draft.as_deref()
    }
    pub(super) fn action(&self) -> Option<&Action> {
        self.action.as_ref()
    }
    pub(super) fn automated(&self, json: bool) -> bool {
        json || self.json_stream || self.non_interactive
    }
    pub(super) fn plain(&self) -> bool {
        self.prompt.as_deref() == Some("plain")
    }
    pub fn invocation(
        &self,
        json: bool,
        instance: Option<&str>,
        config: Option<&std::path::Path>,
        state: Option<&std::path::Path>,
    ) -> Result<InstallInvocation, ()> {
        let mut value = serde_json::to_value(self).map_err(|_| ())?;
        value["json"] = json.into();
        let choices = &mut value["choices"];
        for (key, text) in [
            ("instance", instance.map(str::to_owned)),
            ("config_dir", config.map(|p| p.to_string_lossy().into_owned())),
            ("state_dir", state.map(|p| p.to_string_lossy().into_owned())),
        ] {
            // status의 instance/config/state는 조회 위치이며 변경 선택이 아니다.
            // 명시한 role 등 나머지 선택은 남겨 공통 read-only 검증기가 거부하게 한다.
            if self.status.is_none() {
                choices[key] = text.into();
            }
        }
        let mut envs = vec![];
        for text in &self.choices.harness_env {
            let (harness, rest) = text.split_once(':').ok_or(())?;
            let (key, value) = rest.split_once('=').ok_or(())?;
            envs.push(serde_json::json!({"harness":harness,"key":key,"value":value}));
        }
        choices["harness_env"] = envs.into();
        serde_json::from_value(value).map_err(|_| ())
    }
    pub fn stream(&self) -> bool {
        self.json_stream
    }
}

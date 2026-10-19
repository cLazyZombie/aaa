//! L1 CLI: 시작은 coordinator safety, 대기는 host 단절 뒤에도 같은 rollout ID 조회만 한다.
use aaa_proto::{FrontRequest, FrontResponse, RolloutSpec, RolloutState};
use anyhow::{Result, bail};
use clap::Subcommand;
use std::{process::ExitCode, time::Duration};

#[derive(Subcommand)]
pub(crate) enum DeployCmd {
    /// 빌드 머신의 후보를 전송하거나, 미리 복사한 경로로 시작한다
    Start {
        #[arg(long, required_unless_present = "file", conflicts_with = "file")]
        from: Option<String>,
        /// 빌드 폴더의 aaa. 예: mac-b:/tmp/aaa-candidate/aaa (또는 mac-b:~/aaa-candidate/aaa)
        #[arg(long, conflicts_with = "from")]
        file: Option<String>,
        #[arg(long)]
        sha256: String,
        #[arg(long)]
        commit: Option<String>,
        /// 생략/all이면 등록된 전부. host 머신은 항상 맨 앞에 포함한다
        #[arg(long, value_delimiter = ',', default_value = "all")]
        machines: Vec<String>,
    },
    /// 같은 rollout ID를 조회한다. --wait는 host 재연결 뒤에도 조회만 한다
    Status {
        id: String,
        #[arg(long)]
        wait: bool,
    },
}
pub(crate) fn run(json: bool, cmd: DeployCmd) -> Result<ExitCode> {
    tokio::runtime::Runtime::new()?.block_on(async move {
        use aaa_common::front::FrontClient;
        let state = aaa_config::paths::state_dir();
        match cmd {
            DeployCmd::Start { from, file, sha256, commit, mut machines } => {
                let (from, source_machine) = match file {
                    Some(file) => {
                        let (machine, path) = file
                            .split_once(':')
                            .filter(|(m, p)| !m.is_empty() && !p.is_empty())
                            .ok_or_else(|| anyhow::anyhow!("--file은 머신:후보폴더/aaa 형식이다"))?;
                        (path.to_string(), Some(machine.to_string()))
                    }
                    None => (from.ok_or_else(|| anyhow::anyhow!("--file 또는 --from이 필요하다"))?, None),
                };
                if machines == ["all"] {
                    machines.clear();
                }
                let mut c = FrontClient::connect(&state).await?;
                if c.proto.minor < if source_machine.is_some() { 49 } else { 46 } {
                    bail!("먼저 rollout API를 아는 host·agent를 반영한다");
                }
                let id = aaa_common::util::new_id();
                eprintln!(
                    "rollout ID: {id}. 응답이 끊겨도 다시 시작하지 말고 `aaa deploy status {id} --wait`로 확인한다"
                );
                crate::cli::front_print(
                    json,
                    c.call(FrontRequest::RolloutStart {
                        id,
                        spec: RolloutSpec { from, source_machine, sha256, commit, machines },
                    })
                    .await?,
                )
            }
            DeployCmd::Status { id, wait } => {
                let mut disconnected = None;
                loop {
                    let began = tokio::time::Instant::now();
                    let mut connected = false;
                    let result = async {
                        let mut c = connect_ready(&state, wait).await?;
                        connected = true;
                        if c.proto.minor < 46 {
                            return crate::cli::front_print(
                                json,
                                Err(aaa_proto::ErrorBody::new(
                                    aaa_proto::ErrorCode::UnsupportedVersion,
                                    "이 host는 rollout 상태 API가 없다",
                                )),
                            );
                        }
                        loop {
                            let response = c.call(FrontRequest::RolloutStatus { id: id.clone() }).await?;
                            match &response {
                                Ok(FrontResponse::Rollout { rollout })
                                    if wait && rollout.state == RolloutState::Running =>
                                {
                                    eprintln!(
                                        "rollout {}: {}/{} · 진행 중",
                                        rollout.id,
                                        rollout.cursor,
                                        rollout.steps.len()
                                    );
                                }
                                Ok(FrontResponse::Rollout { rollout }) => {
                                    let stopped = rollout.state == RolloutState::Stopped;
                                    crate::cli::front_print(json, response)?;
                                    return Ok(if stopped { ExitCode::from(1) } else { ExitCode::SUCCESS });
                                }
                                _ => return crate::cli::front_print(json, response),
                            }
                            loop {
                                match c.next().await? {
                                    Some(aaa_proto::FrontMsg::Event(aaa_proto::FrontEvent::Flow(event)))
                                        if event.kind == "rollout_changed" && event.detail == id =>
                                    {
                                        break;
                                    }
                                    None => bail!("host 연결이 끊겼다"),
                                    _ => {}
                                }
                            }
                        }
                    }
                    .await;
                    match result {
                        Ok(code) => return Ok(code),
                        Err(e) if !wait => return Err(e),
                        Err(e) => {
                            if connected {
                                disconnected = Some(tokio::time::Instant::now());
                            }
                            let since = disconnected.get_or_insert(began);
                            if since.elapsed() >= Duration::from_secs(120) {
                                bail!("host 무응답: {e:#}. 같은 ID {id}의 상태를 나중에 확인한다");
                            }
                            // host가 내려간 동안에는 Front 사건원이 없다. 재연결만 시도하며 시작은 보내지 않는다.
                            tokio::time::sleep(Duration::from_millis(250)).await;
                        }
                    }
                }
            }
        }
    })
}

/// host의 wait 전용 handshake가 local agent의 pane 자료를 기다린 뒤 peer PID를 판정한다.
async fn connect_ready(state: &std::path::Path, wait: bool) -> Result<aaa_common::front::FrontClient> {
    tokio::time::timeout(Duration::from_secs(120), async {
        if wait {
            aaa_common::front::FrontClient::connect_as(state, "deploy-wait").await
        } else {
            aaa_common::front::FrontClient::connect(state).await
        }
    })
    .await
    .map_err(|_| anyhow::anyhow!("로컬 agent가 120초 안에 재연결하지 않았다"))?
}

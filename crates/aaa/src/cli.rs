//! 명령줄. 역할 실행(`host`·`agent`·`session-host`)과 front 명령을 한 binary에 둔다.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Args, CommandFactory, FromArgMatches, Parser, Subcommand};

use crate::service;
use aaa_doctor as doctor;
mod args;
use args::*;
mod task;
use task::*;
mod thread;
use thread::*;
mod pane;
use pane::*;
mod agent;
pub(crate) use agent::*;
mod project;
pub(crate) use project::*;
mod doctor_cmd;
use doctor_cmd::*;
mod service_cmd;
use service_cmd::*;
mod events;
use events::*;
mod render;
use aaa_proto::{
    DeviceRole, FrontRequest, FrontResponse, LayoutChange, LibraryRef, ProjectAction, ProjectStatus, Role, SplitDir,
    ThreadKind,
};
use render::*;

fn with_cli_stack<T: Send + 'static>(build: impl FnOnce() -> T + Send + 'static) -> std::io::Result<T> {
    // clap derive의 명령 트리 빌더는 debug에서 Cmd·ThreadCmd 프레임만 약 1MiB다.
    // Windows main의 1MiB 스택을 넘지 않게 명령 트리 생성·파싱만 별도 스택에서 끝낸다.
    // 서비스·PTY·async 실행은 join 뒤 원래 main thread에서 계속한다.
    std::thread::Builder::new()
        .name("aaa-cli".into())
        .stack_size(4 * 1024 * 1024)
        .spawn(build)
        .map(|thread| thread.join().unwrap_or_else(|panic| std::panic::resume_unwind(panic)))
}

pub fn main() -> ExitCode {
    let cli = match with_cli_stack(|| {
        let version: &'static str =
            Box::leak(aaa_common::util::software().trim_start_matches("aaa ").to_string().into_boxed_str());
        let command = Cli::command().version(version);
        let install_output = crate::install::requested_output(&command);
        let mut args: Vec<std::ffi::OsString> = std::env::args_os().collect();
        // `aaa-setup.exe`(더블클릭·SSH)는 설치 마법사다. 전역 인자는 setup 뒤에 와도 된다
        if crate::setup::implied(&args, |name| command.find_subcommand(name).is_some()) {
            args.insert(1, "setup".into());
        }
        (install_output, command.try_get_matches_from(args).and_then(|m| Cli::from_arg_matches(&m)))
    }) {
        Ok((_, Ok(cli))) => cli,
        Ok((install_output, Err(e))) => {
            if !matches!(e.kind(), clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion)
                && let Some((json, stream)) = install_output
            {
                return crate::install::emit(aaa_proto::install::InstallResult::invalid(), json, stream);
            }
            e.exit()
        }
        Err(e) => {
            eprintln!("aaa: 명령줄 해석 thread를 시작하지 못했다: {e}");
            return ExitCode::FAILURE;
        }
    };
    use aaa_common::panic_log::{ProcessKind, set_process};
    set_process(match &cli.cmd {
        Cmd::Host => ProcessKind::Host,
        Cmd::Agent(AgentArgs { cmd: None, .. }) | Cmd::DeploymentGuard { .. } | Cmd::SetupService => ProcessKind::Agent,
        Cmd::SessionHost { .. } => ProcessKind::SessionHost,
        _ => ProcessKind::Cli,
    });
    if let Cmd::Install(args) = &cli.cmd {
        init_log("off");
        return crate::install::run(
            args,
            cli.json,
            cli.instance.as_deref(),
            cli.config_dir.as_deref(),
            cli.state_dir.as_deref(),
        );
    }
    if let Cmd::ServiceRun { context, context_sha256 } = &cli.cmd {
        return crate::restart::finish(crate::install::service_dispatch(context, context_sha256));
    }
    if let Cmd::InstallProbe { executor_sha256 } = &cli.cmd {
        let Some(state) = cli.state_dir.as_deref() else {
            eprintln!("aaa: install-probe에는 --state-dir이 필요하다");
            return ExitCode::from(2);
        };
        return crate::install::probe_dispatch(state, executor_sha256);
    }
    if let Cmd::InstallBootstrap { executor_sha256, issuer_sha256 } = &cli.cmd {
        let Some(state) = cli.state_dir.as_deref() else {
            eprintln!("aaa: install-bootstrap에는 --state-dir이 필요하다");
            return ExitCode::from(2);
        };
        return crate::install::bootstrap_dispatch(state, executor_sha256, issuer_sha256);
    }
    if let Cmd::InstallWorker { id, executor_sha256, resume } = &cli.cmd {
        let Some(state) = cli.state_dir.as_deref() else {
            eprintln!("aaa: install-worker에는 --state-dir이 필요하다");
            return ExitCode::from(2);
        };
        return crate::install::worker_dispatch(state, id, executor_sha256, *resume);
    }
    if let Some(name) = &cli.instance {
        // 하위 프로세스(session host·pane 안 CLI·launchd가 띄운 aaa)도 같은 인스턴스를 본다
        unsafe { std::env::set_var("AAA_INSTANCE", name) };
    }
    if let Some(name) = aaa_config::paths::instance()
        && let Err(e) = aaa_config::paths::check_instance(&name)
    {
        eprintln!("aaa: {e}");
        return ExitCode::from(2);
    }
    if let Some(d) = &cli.config_dir {
        // 하위 프로세스(session host 등)도 같은 위치를 쓰게 한다.
        unsafe { std::env::set_var("AAA_CONFIG_DIR", d) };
    }
    if let Some(d) = &cli.state_dir {
        unsafe { std::env::set_var("AAA_STATE_DIR", d) };
    }
    match &cli.cmd {
        Cmd::Setup(args) => return crate::setup::run(args),
        #[cfg(windows)]
        Cmd::SetupService => return crate::setup::service::dispatch(),
        #[cfg(unix)]
        Cmd::SetupService => return crate::setup::unix::dispatch(),
        #[cfg(not(any(windows, unix)))]
        Cmd::SetupService => {
            eprintln!("aaa: setup-service는 Windows 서비스 입구다");
            return ExitCode::from(2);
        }
        _ => {}
    }
    #[cfg(unix)]
    let logs = match &cli.cmd {
        Cmd::Host => crate::service_logs::direct(aaa_proto::install::InstallRole::Host),
        Cmd::Agent(AgentArgs { cmd: None, .. }) => crate::service_logs::direct(aaa_proto::install::InstallRole::Agent),
        _ => None,
    };
    let code = match run(cli) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("aaa: {}", aaa_common::redact::text(&format!("{e:#}")));
            ExitCode::from(1)
        }
    };
    #[cfg(unix)]
    drop(logs);
    crate::restart::finish(code)
}

fn init_log(default: &str) {
    let filter = tracing_subscriber::EnvFilter::try_from_env("AAA_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(default));
    use std::io::IsTerminal;
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(aaa_common::redact::LogWriter::default)
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
        Cmd::Verify { command } => {
            let status = aaa_agent::verify::run(&std::env::current_dir()?, &command, json)?;
            let code = status.code().and_then(|c| u8::try_from(c).ok()).unwrap_or(1);
            #[cfg(unix)]
            let code = {
                use std::os::unix::process::ExitStatusExt;
                status.signal().map_or(code, |signal| 128u8.saturating_add(signal as u8))
            };
            if json {
                println!("{}", serde_json::json!({"success":status.success(),"code":code}));
            }
            Ok(ExitCode::from(code))
        }
        Cmd::DbBackup { cmd } => {
            init_log("info");
            aaa_common::resource::disable_core_dump("db-backup");
            crate::db_backup::run(cmd, json)?;
            Ok(ExitCode::SUCCESS)
        }
        Cmd::FileBackup { cmd } => {
            init_log("info");
            aaa_common::resource::disable_core_dump("file-backup");
            crate::file_backup::run(cmd, json)?;
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Install(_) => unreachable!("install은 환경 변경 전에 처리한다"),
        Cmd::ServiceRun { .. } => unreachable!("SCM은 환경 변경 전에 연결한다"),
        Cmd::Setup(_) | Cmd::SetupService => unreachable!("setup은 run 전에 처리한다"),
        Cmd::InstallProbe { .. } | Cmd::InstallBootstrap { .. } | Cmd::InstallWorker { .. } => {
            unreachable!("installer worker는 환경 변경 전에 연결한다")
        }
        Cmd::Host => {
            init_log("info");
            aaa_common::resource::raise_nofile_limit("host");
            aaa_common::resource::disable_core_dump("host");
            legacy_guard()?;
            runtime()?.block_on(aaa_host::run(
                aaa_config::paths::config_dir(),
                aaa_config::paths::state_dir(),
                None,
                crate::restart::shutdown("host"),
            ))?;
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Agent(a) => {
            init_log("info");
            aaa_common::resource::raise_nofile_limit("agent");
            aaa_common::resource::disable_core_dump("agent");
            legacy_guard()?;
            agent_cmd(a)
        }
        Cmd::DeploymentGuard { id } => {
            init_log("info");
            runtime()?.block_on(aaa_agent::deployment_guard(&id, crate::deployment::execute_recovery))?;
            Ok(ExitCode::SUCCESS)
        }
        Cmd::SessionHost { dir, startup_stdin } => {
            init_log("info");
            aaa_common::resource::disable_core_dump("session host");
            aaa_session::host::main_with_startup(dir, startup_stdin)?;
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Debug { cmd: DebugCmd::Input { pane, thread, since } } => {
            let since_ms = debug_since(&since)?;
            front(json, FrontRequest::DebugInput { pane, thread, since_ms })
        }
        Cmd::Account { cmd } => match cmd {
            AccountCmd::Pool { cmd } => account_pool_command(json, cmd),
            AccountCmd::Starts { project } => {
                front(json, FrontRequest::AccountStarts { project: project_or_here(project)? })
            }
            AccountCmd::CancelStart { id, project } => {
                front(json, FrontRequest::AccountStartCancel { project: project_or_here(project)?, id })
            }
            AccountCmd::Login { name, harness, email } => {
                front(json, FrontRequest::AccountLogin { name, harness, email: email.map(aaa_proto::SecretText) })
            }
            AccountCmd::Email { name, email } => {
                front(json, FrontRequest::AccountEmail { name, email: aaa_proto::SecretText(email) })
            }
            AccountCmd::UsageLogin { name, email } => front(
                json,
                FrontRequest::AccountUsageLogin { name, remote: false, email: email.map(aaa_proto::SecretText) },
            ),
            AccountCmd::LoginStatus { id } => front(json, FrontRequest::AccountLoginStatus { id }),
            AccountCmd::LoginCancel { id } => front(json, FrontRequest::AccountLoginCancel { id }),
            AccountCmd::LoginConfirm { id, switch, keep: _ } => {
                front(json, FrontRequest::AccountLoginConfirm { id, switch })
            }
            AccountCmd::Default { harness, account, off } => {
                if off || account.is_some() {
                    front(json, FrontRequest::AccountDefault { harness, account })
                } else {
                    front(json, FrontRequest::AccountDefaults)
                }
            }
            AccountCmd::List => front(json, FrontRequest::AccountList),
            AccountCmd::Rename { name, to } => front(json, FrontRequest::AccountRename { name, to }),
            AccountCmd::Remove { name } => front(json, FrontRequest::AccountRemove { name }),
            AccountCmd::Import { name, harness, file, email } => {
                aaa_common::resource::disable_core_dump("account import");
                if std::env::var_os("AAA_SESSION_ID").is_some() || std::env::var_os("AAA_HOOK_SOCK").is_some() {
                    anyhow::bail!("계정 등록은 host의 aaa 밖 터미널에서 한다");
                }
                use std::io::Read;
                let m = file.symlink_metadata().context("인증 결과 파일을 열지 못했다")?;
                anyhow::ensure!(m.is_file() && m.len() <= 128 * 1024, "인증 결과는 128KiB 이하 일반 파일이어야 한다");
                let mut secret = String::new();
                std::fs::File::open(file)?.take(128 * 1024 + 1).read_to_string(&mut secret).map_err(
                    |source_error| {
                        aaa_common::diagnostic::context(source_error, anyhow::anyhow!("인증 결과 파일을 읽지 못했다"))
                    },
                )?;
                aaa_common::redact::remember_auth(&harness, &secret);
                front(
                    json,
                    FrontRequest::AccountImport {
                        name,
                        harness,
                        secret: aaa_proto::SecretText(secret),
                        email: email.map(aaa_proto::SecretText),
                    },
                )
            }
        },
        Cmd::Status => front(json, FrontRequest::Status),
        Cmd::Pane { cmd } => pane_cmd(json, cmd),
        Cmd::Tab { cmd } => tab_cmd(cmd),
        Cmd::Workspace { cmd } => workspace_cmd(cmd),
        Cmd::Completion { shell } => completion(&shell),
        Cmd::Init => crate::init::run(json),
        Cmd::Project { cmd } => project_cmd(json, cmd),
        Cmd::Set { key, value } => project_cmd(json, ProjectCmd::Set { project: project_or_here(None)?, key, value }),
        Cmd::Coordinator { cmd: CoordinatorCmd::Open { profile, new } } => {
            front(json, FrontRequest::ProjectOpen { project: project_or_here(None)?, profile, new })
        }
        Cmd::Decision { .. } => anyhow::bail!(
            "`aaa decision`은 없어졌다. 뒤 thread가 알아야 할 결정은 coordinator가 프로젝트 memory(MEMORY.md 색인과 \
             memory/ 파일)에 적는다. memory는 모든 thread brief에 들어간다"
        ),
        Cmd::Guide { topic, all } => guide(json, topic.as_deref(), all),
        Cmd::Context { project, peek } => {
            front(json, FrontRequest::Context { project: project_or_here(project)?, peek })
        }
        Cmd::Plan { cmd: PlanCmd::Check { path, base, head, media, command_json } } => {
            let command: Vec<String> = serde_json::from_str(&command_json).context("design_check 인자 배열")?;
            crate::plan_preview::check(&path, &base, &head, media.as_deref(), &command)?;
            if json {
                println!("{}", serde_json::json!({"checked":head}));
            } else {
                println!("기획 검사 통과 {head}");
            }
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Plan { cmd: PlanCmd::Preview { path, base, output } } => {
            crate::plan_preview::write(&path, &base, &output)?;
            let page = output.join("index.html");
            if json {
                println!("{}", serde_json::json!({"path": page}));
            } else {
                println!("{}", page.display());
            }
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Plan { cmd } => {
            let project = project_or_here(None)?;
            match cmd {
                PlanCmd::Preview { .. } | PlanCmd::Check { .. } => unreachable!(),
                PlanCmd::List => front(json, FrontRequest::DesignTree { project, repo: None, cached: false }),
                PlanCmd::Links => front(json, FrontRequest::IdeaLinks { project }),
                PlanCmd::Show { repo, path, at, branch } => {
                    if branch.is_some() {
                        eprintln!("알림: 기획 branch는 없어졌다(t-2002). main을 읽는다");
                    }
                    let (repo, path) = match path {
                        Some(path) => (repo, path),
                        None => design_id_target(&project, &repo)?,
                    };
                    front(json, FrontRequest::DesignRead { project, repo, path, branch: None, at })
                }
                PlanCmd::Log { repo, path } => front(json, FrontRequest::DesignLog { project, repo, path }),
            }
        }
        Cmd::Task { cmd: TaskCmd::Queue } => {
            front(json, FrontRequest::TaskQueue { project: project_or_here(None)?, action: aaa_proto::QueueOp::List })
        }
        Cmd::Task { cmd: TaskCmd::Reserve { task, block, profile, account, after, auto_merge, user_request } } => {
            let project = project_or_here(None)?;
            auto_merge_first(&project, &task, auto_merge.as_deref(), user_request)?;
            let response = front_call(FrontRequest::ThreadStartPreview {
                project: project.clone(),
                from_task: task.clone(),
                task_block: block,
            })?
            .map_err(|e| anyhow::anyhow!(e.message))?;
            let FrontResponse::ThreadStartPreview(p) = response else {
                anyhow::bail!("예약 미리보기 응답이 아니다")
            };
            front(
                json,
                FrontRequest::TaskQueue {
                    project,
                    action: aaa_proto::QueueOp::Reserve {
                        task,
                        block: p.task_block,
                        profile: profile.unwrap_or(p.profile),
                        profile_revision: p.profile_revision,
                        account,
                        after: after.first().cloned(),
                        after_all: Some(after),
                    },
                },
            )
        }
        Cmd::Task { cmd: TaskCmd::Unreserve { task_id } } => front(
            json,
            FrontRequest::TaskQueue { project: project_or_here(None)?, action: aaa_proto::QueueOp::Cancel { task_id } },
        ),
        Cmd::Task { cmd: TaskCmd::QueueMove { task_id, position } } => front(
            json,
            FrontRequest::TaskQueue {
                project: project_or_here(None)?,
                action: aaa_proto::QueueOp::Move { task_id, position },
            },
        ),
        Cmd::Task { cmd: TaskCmd::Archive { restore } } => task_archive_cli(json, project_or_here(None)?, restore),
        Cmd::Task { cmd: TaskCmd::List } => front(json, FrontRequest::Tasks { project: project_or_here(None)? }),
        Cmd::Task { cmd: TaskCmd::Log { task, limit, before, deleted } } => front(
            json,
            FrontRequest::TaskLog {
                project: project_or_here(None)?,
                before,
                limit: Some(limit),
                filter: deleted.then_some(aaa_proto::TaskLogFilter::Deleted),
                task,
                search: None,
            },
        ),
        Cmd::Task { cmd } => {
            use aaa_proto::TaskOp;
            let mut user_request = None;
            // 할 일을 쓴 뒤 자동 병합을 정한다(새 할 일은 제목으로 찾는다)
            let mut after_merge = None;
            let mut ops = Vec::new();
            let op = match cmd {
                TaskCmd::List
                | TaskCmd::Log { .. }
                | TaskCmd::Queue
                | TaskCmd::Reserve { .. }
                | TaskCmd::Unreserve { .. }
                | TaskCmd::QueueMove { .. }
                | TaskCmd::Archive { .. } => unreachable!(),
                TaskCmd::Add {
                    title,
                    list,
                    profile,
                    machine,
                    review_profile,
                    review_account,
                    owner,
                    person,
                    notes,
                    parent,
                    before,
                    thread,
                    repo,
                    auto_merge,
                    user_request: reason,
                } => {
                    if let Some(mode) = auto_merge {
                        after_merge = Some((title.clone(), mode, reason));
                    }
                    TaskOp::Add {
                        list,
                        title,
                        owner,
                        person,
                        profile,
                        machine,
                        review: (review_profile.is_some() || review_account.is_some())
                            .then_some(aaa_proto::TaskReview { profile: review_profile, account: review_account }),
                        notes,
                        parent: parent.as_deref().map(task_at),
                        before: before.as_deref().map(task_at),
                        thread,
                        repo,
                    }
                }
                TaskCmd::Edit {
                    user_request: reason,
                    auto_merge,
                    task,
                    block,
                    title,
                    profile,
                    machine,
                    review_profile,
                    review_account,
                    owner,
                    person,
                    notes,
                    add_notes,
                    clear_notes,
                    thread,
                    repo,
                    done,
                    undone,
                } => {
                    let only_merge = title.is_none()
                        && profile.is_none()
                        && machine.is_none()
                        && review_profile.is_none()
                        && review_account.is_none()
                        && owner.is_none()
                        && !person
                        && notes.is_empty()
                        && add_notes.is_empty()
                        && !clear_notes
                        && thread.is_none()
                        && repo.is_none()
                        && !done
                        && !undone;
                    if let Some(mode) = auto_merge {
                        if only_merge {
                            let project = project_or_here(None)?;
                            return front(json, auto_merge_request(project, task, &mode, reason));
                        }
                        after_merge = Some((task.clone(), mode, reason.clone()));
                    }
                    user_request = reason;
                    let at = task_at_block(&task, block);
                    if !add_notes.is_empty() {
                        ops.push(TaskOp::AppendNotes { at: at.clone(), notes: add_notes });
                    }
                    TaskOp::Update {
                        at,
                        title,
                        owner,
                        person,
                        profile,
                        machine,
                        review: (review_profile.is_some() || review_account.is_some())
                            .then_some(aaa_proto::TaskReview { profile: review_profile, account: review_account }),
                        notes: if clear_notes { Some(Vec::new()) } else { (!notes.is_empty()).then_some(notes) },
                        thread,
                        done: if done {
                            Some(true)
                        } else if undone {
                            Some(false)
                        } else {
                            None
                        },
                        repo,
                    }
                }
                TaskCmd::Move { task, block, list, before, parent } => TaskOp::Move {
                    at: task_at_block(&task, block),
                    to_list: list,
                    before: before.as_deref().map(task_at),
                    parent: parent.as_deref().map(task_at),
                },
                TaskCmd::Rm { task, block } => TaskOp::Delete { at: task_at_block(&task, block) },
                TaskCmd::Restore { task, list } => {
                    let log_id = if let Ok(id) = task.parse::<i64>() {
                        id
                    } else {
                        let project = project_or_here(None)?;
                        let response = front_call(FrontRequest::TaskLog {
                            project: project.clone(),
                            before: None,
                            limit: Some(500),
                            filter: Some(aaa_proto::TaskLogFilter::Deleted),
                            task: Some(task.clone()),
                            search: None,
                        })?
                        .map_err(|e| anyhow::anyhow!(e.message))?;
                        let FrontResponse::TaskLog { entries, .. } = response else {
                            anyhow::bail!("기록 응답이 아니다")
                        };
                        if let Some(e) = entries.iter().find(|e| e.restored_by.is_none()) {
                            e.id
                        } else {
                            // 완료 보관은 별도 복원 경로다. ID·제목을 모두 host가 해석한다.
                            if list.is_some() {
                                anyhow::bail!(
                                    "완료 보관은 원래 목록에 복원한다. --list 없이 복원한 뒤 task move로 옮긴다"
                                );
                            }
                            return task_archive_cli(json, project, Some(task));
                        }
                    };
                    TaskOp::Restore { log_id, list }
                }
                TaskCmd::ListAdd { name, after } => TaskOp::ListAdd { name, after },
                TaskCmd::ListRename { name, to } => TaskOp::ListRename { name, to },
                TaskCmd::ListMerge { name, into } => TaskOp::ListMerge { name, into },
                TaskCmd::ListRm { name } => TaskOp::ListDelete { name },
                TaskCmd::ListMove { name, before } => TaskOp::ListMove { name, before },
            };
            ops.push(op);
            let code = task_edit_cmd(json, ops, user_request)?;
            if code == ExitCode::SUCCESS
                && let Some((task, mode, reason)) = after_merge
            {
                auto_merge_first(&project_or_here(None)?, &task, Some(&mode), reason)?;
            }
            Ok(code)
        }
        Cmd::Assignable { project, check } => {
            front(json, FrontRequest::Assignable { project: project_or_here(project)?, check })
        }
        Cmd::Inbox { cmd } => front(
            json,
            match cmd {
                InboxCmd::List { project, all, peek } => {
                    FrontRequest::Inbox { project: project_or_here(project)?, all, peek }
                }
                InboxCmd::Done { mut ids, all } => {
                    // 옛 꼴 `inbox done <프로젝트> <ID…>`: 첫 값이 프로젝트 이름이면 뺀다
                    let given = match ids.first() {
                        Some(first) if known_projects()?.iter().any(|(n, _)| n == first) => Some(ids.remove(0)),
                        _ => None,
                    };
                    let project = project_or_here(given)?;
                    anyhow::ensure!(all != !ids.is_empty(), "끝낼 항목 ID나 --all 중 하나만 준다");
                    FrontRequest::InboxDone { project, ids, all }
                }
            },
        ),
        Cmd::Ticker { cmd: TickerCmd::Tick } => front(json, FrontRequest::TickerTick),
        Cmd::Merge { .. } => anyhow::bail!(
            "`aaa merge`는 없어졌다. aaa는 병합하지 않는다: 사용자가 병합하라고 하면 그 thread의 Next 줄을 \
             `aaa thread next <id> --line N`으로 보내 착지를 진행한다. 결과는 main SHA와 push 여부로 확인한다"
        ),
        Cmd::Automation {
            cmd: AutomationCmd::Input { cmd: AutomationInputCmd::Complete { runs, outcome, report } },
        } => {
            let mut seen = std::collections::HashSet::new();
            let runs: Vec<_> = runs.into_iter().filter(|id| seen.insert(id.clone())).collect();
            let completion = aaa_proto::AutomationCompletionBatch {
                runs: runs.clone(),
                outcome: match outcome.as_str() {
                    "completed" => aaa_proto::AutomationOutcome::Completed,
                    "failed" => aaa_proto::AutomationOutcome::Failed,
                    _ => aaa_proto::AutomationOutcome::NeedsDecision,
                },
                report,
            };
            use sha2::Digest;
            let body = if runs.len() == 1 {
                serde_json::to_string(&aaa_proto::AutomationCompletion {
                    run: runs[0].clone(),
                    outcome: completion.outcome,
                    report: completion.report,
                })?
            } else {
                serde_json::to_string(&completion)?
            };
            let input = aaa_proto::PostInput {
                post_id: hex::encode(
                    &sha2::Sha256::digest(
                        format!("{}\n{body}", std::env::var("AAA_SESSION_ID").unwrap_or_default()).as_bytes(),
                    )[..16],
                ),
                to: "coordinator".into(),
                post_type: aaa_proto::PostType::Data,
                topic: "automation-input-complete".into(),
                title: "자동화 입력 처리 결과".into(),
                body,
                needs_user: false,
                refs: if runs.len() == 1 { runs.clone() } else { vec![] },
            };
            let rt = tokio::runtime::Runtime::new()?;
            let result = rt.block_on(aaa_agent::post::send(serde_json::to_value(input)?))?;
            let success = match &result {
                aaa_proto::PostResult::AutomationCompleted { run, runs: recorded, .. } => {
                    if runs.len() == 1 {
                        run == &runs[0]
                    } else {
                        recorded == &runs
                    }
                }
                _ => false,
            };
            println!("{}", serde_json::to_string(&result)?);
            if !success && matches!(result, aaa_proto::PostResult::AutomationCompleted { .. }) {
                eprintln!("완료 등록 영수증의 실행 ID가 요청과 다르다. CLI·agent·host 판을 확인하라");
            }
            Ok(if success { ExitCode::SUCCESS } else { ExitCode::from(1) })
        }
        Cmd::Automation { cmd: AutomationCmd::Approve { name, approval_sha256, capability } } => {
            crate::person::require("aaa automation approve")?;
            let project = project_or_here(None)?;
            front(
                json,
                if capability == "command" {
                    FrontRequest::RoutineApprove { project, name, command_sha256: approval_sha256 }
                } else {
                    FrontRequest::AutomationApprove { project, name, approval_sha256 }
                },
            )
        }
        Cmd::Automation { cmd: AutomationCmd::List } => {
            front(json, FrontRequest::Routines { project: project_or_here(None)? })
        }
        Cmd::Automation { cmd: AutomationCmd::Show { name } } => {
            front(json, FrontRequest::AutomationRead { project: project_or_here(None)?, name })
        }
        Cmd::Automation { cmd: AutomationCmd::Runs { name, before } } => {
            front(json, FrontRequest::AutomationExecutions { project: project_or_here(None)?, name, before })
        }
        Cmd::Automation { cmd: AutomationCmd::Enable { name } } => {
            front(json, FrontRequest::RoutineToggle { project: project_or_here(None)?, name, on: Some(true) })
        }
        Cmd::Automation { cmd: AutomationCmd::Disable { name } } => {
            front(json, FrontRequest::RoutineToggle { project: project_or_here(None)?, name, on: Some(false) })
        }
        Cmd::Automation { cmd: AutomationCmd::Migrate { dry_run, plan_sha256 } } => {
            anyhow::ensure!(dry_run || plan_sha256.is_some(), "적용에는 --plan-sha256 <미리보기 hash>가 필요하다");
            front(json, FrontRequest::AutomationConvert { project: project_or_here(None)?, dry_run, plan_sha256 })
        }
        Cmd::Automation { cmd: AutomationCmd::Convert { project } } => front(
            json,
            FrontRequest::AutomationConvert { project: project_or_here(project)?, dry_run: false, plan_sha256: None },
        ),
        Cmd::Automation { cmd: AutomationCmd::Unblock { name, batch, discard } } => {
            crate::person::require("aaa automation unblock")?;
            front(json, FrontRequest::AutomationUnblock { project: project_or_here(None)?, name, batch, discard })
        }
        Cmd::Automation { cmd: AutomationCmd::Run { name, id } } => {
            let project = project_or_here(None)?;
            if name == "show"
                && let Some(id) = id
            {
                front(json, FrontRequest::AutomationExecution { project, id })
            } else {
                anyhow::ensure!(id.is_none(), "run <이름> 또는 run show <실행 ID>를 쓴다");
                front(json, FrontRequest::AutomationRun { project, name, key: aaa_common::util::new_id() })
            }
        }
        Cmd::Automation { cmd: AutomationCmd::Delete { name } } => {
            let project = project_or_here(None)?;
            // 지금 판을 읽고 그 판을 지운다(그 사이 바뀌면 host가 거부한다)
            let doc = match front_call(FrontRequest::AutomationRead { project: project.clone(), name: name.clone() })? {
                Ok(FrontResponse::AutomationDoc { doc }) => doc,
                other => return front_print(json, other),
            };
            front(json, FrontRequest::AutomationDelete { project, name, expected_sha256: doc.sha256 })
        }
        Cmd::Automation { cmd: AutomationCmd::Restore { id } } => {
            front(json, FrontRequest::AutomationRestore { project: project_or_here(None)?, id })
        }
        Cmd::Automation { cmd: AutomationCmd::Schema { mission } } => {
            if mission {
                anyhow::bail!("없어짐: 자동화로 바꿨다(aaa automation convert)");
            }
            println!("{}", aaa_host::automation_schema());
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Routine { cmd: RoutineCmd::Runs { name, legacy } } => {
            let (project, name) = on_target(name, legacy)?;
            front(json, FrontRequest::RoutineRuns { project, name })
        }
        Cmd::Routine { cmd: RoutineCmd::List { project } } => {
            front(json, FrontRequest::Routines { project: project_or_here(project)? })
        }
        Cmd::Routine { cmd: RoutineCmd::Toggle { name, legacy, on, off } } => {
            let (project, name) = on_target(name, legacy)?;
            let on = if on {
                Some(true)
            } else if off {
                Some(false)
            } else {
                None
            };
            front(json, FrontRequest::RoutineToggle { project, name, on })
        }
        Cmd::Routine { cmd: RoutineCmd::Approve { name, legacy } } => {
            let (project, name) = on_target(name, legacy)?;
            routine_approve(json, project, name)
        }
        Cmd::Mission { .. } | Cmd::Evaluator { .. } | Cmd::Evaluation { .. } => {
            anyhow::bail!("없어짐: 자동화로 바꿨다(aaa automation convert)")
        }
        Cmd::Profile { cmd } => crate::settings::profile_cmd(json, cmd),
        Cmd::Model { cmd } => crate::settings::model_cmd(json, cmd),
        Cmd::Safety { cmd } => crate::settings::safety_cmd(json, cmd),
        Cmd::Thread { cmd } => front(
            json,
            match cmd {
                ThreadCmd::Start(args) => {
                    let ThreadStartArgs {
                        account_pool,
                        account_size,
                        start_id,
                        over_limit,
                        user_request,
                        ignore_reservation,
                        auto_merge,
                        holds,
                        project,
                        title,
                        machine,
                        profile,
                        account,
                        kind,
                        repo,
                        base,
                        task_file,
                        from_task,
                        link_task,
                        summary_file,
                        background,
                    } = *args;
                    if link_task && from_task.is_none() {
                        anyhow::bail!("--link-task는 --from-task와 함께 쓴다");
                    }
                    if from_task.is_none() && (title.is_none() || task_file.is_none()) {
                        anyhow::bail!("--title·--task-file이 필요하다(TASKS.md task로 시작하면 --from-task)");
                    }
                    if task_file.as_deref() == Some("-") && summary_file.as_deref() == Some("-") {
                        anyhow::bail!("--task-file과 --summary-file을 둘 다 표준 입력(-)으로 줄 수 없다");
                    }
                    if user_request.is_some() && !over_limit && !ignore_reservation && auto_merge.is_none() {
                        anyhow::bail!("--user-request는 --over-limit·--ignore-reservation·--auto-merge와 함께 쓴다");
                    }
                    let task = task_text(task_file.as_deref())?;
                    let summary = summary_file.as_deref().map(|f| file_text(f, "요약 파일")).transpose()?;
                    let kind = kind.map(|k| if k == "dir" { ThreadKind::Dir } else { ThreadKind::Worktree });
                    let project = project_or_here(project)?;
                    // 할 일에서 띄우면 먼저 정한다(시작 뒤 brief가 지금 값을 싣는다). 제목으로 띄우면 host가 만든 할 일에 시작 뒤 정한다
                    let merge_after = match (&from_task, auto_merge) {
                        (Some(t), Some(mode)) => {
                            auto_merge_first(&project, t, Some(&mode), user_request.clone())?;
                            None
                        }
                        (None, Some(mode)) => Some((mode, user_request.clone())),
                        (_, None) => None,
                    };
                    let req = FrontRequest::ThreadStart {
                        over_limit,
                        user_request: user_request.filter(|_| over_limit || ignore_reservation),
                        ignore_reservation,
                        holds,
                        background,
                        project: project.clone(),
                        title: title.unwrap_or_default(),
                        machine,
                        profile: profile.unwrap_or_default(),
                        kind,
                        repo,
                        base,
                        task,
                        from_task,
                        link_task,
                        task_block: None,
                    };
                    let req = if account_pool {
                        let size = match account_size.as_deref() {
                            Some("small") => aaa_proto::AccountTaskSize::Small,
                            Some("large") => aaa_proto::AccountTaskSize::Large,
                            _ => aaa_proto::AccountTaskSize::Medium,
                        };
                        FrontRequest::ThreadStartPool {
                            id: start_id.unwrap_or_else(aaa_common::util::new_id),
                            size,
                            start: Box::new(req),
                        }
                    } else {
                        match account {
                            Some(account) => FrontRequest::ThreadStartAccount { account, start: Box::new(req) },
                            None => req,
                        }
                    };
                    return with_summary(json, req, project, None, summary, merge_after);
                }
                ThreadCmd::AutoMerge { thread, mode, user_request } => {
                    let project = project_or_here(None)?;
                    match mode {
                        Some(mode) => auto_merge_request(project, thread, &mode, user_request),
                        None => FrontRequest::ThreadShow { project, thread },
                    }
                }
                ThreadCmd::Adopt { project, pane, title, task_file } => FrontRequest::ThreadAdopt {
                    project: project_or_here(project)?,
                    pane,
                    title,
                    task: task_text(task_file.as_deref())?,
                },
                ThreadCmd::Next { thread, legacy, line, add } => {
                    let (project, thread) = on_target(thread, legacy)?;
                    FrontRequest::ThreadNext { project, thread, line, add }
                }
                ThreadCmd::Followup { thread, legacy, id, task, reason_file } => {
                    let (project, thread) = on_target(thread, legacy)?;
                    FrontRequest::ThreadFollowup {
                        project,
                        thread,
                        id,
                        task,
                        reason: reason_file
                            .as_deref()
                            .map(|p| task_text(Some(p)))
                            .transpose()?
                            .map(|s| s.trim().to_string()),
                    }
                }
                ThreadCmd::List { project, role, open, status, limit, after } => {
                    return threads_cli(
                        json,
                        project,
                        role.as_deref().and_then(aaa_proto::Role::parse),
                        aaa_proto::ThreadListQuery {
                            open,
                            status: status.as_deref().and_then(aaa_proto::ThreadStatus::parse),
                            limit,
                            after,
                            details: false,
                        },
                    );
                }
                ThreadCmd::Show { thread, legacy } => {
                    let (project, thread) = on_target(thread, legacy)?;
                    FrontRequest::ThreadShow { project, thread }
                }
                ThreadCmd::Summary { thread, legacy, file, check_file } => {
                    if let Some(f) = check_file {
                        return check_summary_file(json, &f);
                    }
                    let (project, thread) = on_target(thread.context("thread ID가 필요하다")?, legacy)?;
                    match file {
                        None => FrontRequest::ThreadSummary { project, thread },
                        Some(f) => {
                            FrontRequest::ThreadSummarySet { project, thread, text: file_text(&f, "요약 파일")? }
                        }
                    }
                }
                ThreadCmd::Library { thread, legacy } => {
                    let (project, thread) = on_target(thread, legacy)?;
                    FrontRequest::ThreadLibrary { project, thread }
                }
                ThreadCmd::Ack { thread, legacy } => {
                    let (project, thread) = on_target(thread, legacy)?;
                    FrontRequest::ThreadAck { project, thread }
                }
                ThreadCmd::Resolve { thread, legacy, dry_run: true, .. } => {
                    let (project, thread) = on_target(thread, legacy)?;
                    FrontRequest::ThreadResolvePreview { project, thread }
                }
                ThreadCmd::Resolve { thread, legacy, keep_worktree, reopen, skip_copy, discard_uncopied, .. } => {
                    let (project, thread) = on_target(thread, legacy)?;
                    FrontRequest::ThreadResolve { project, thread, keep_worktree, reopen, skip_copy, discard_uncopied }
                }
                ThreadCmd::Brief { thread, legacy } => {
                    let (project, thread) = on_target(thread, legacy)?;
                    FrontRequest::ThreadBrief { project, thread }
                }
                ThreadCmd::Read { thread, legacy, lines } => {
                    let (project, thread) = on_target(thread, legacy)?;
                    FrontRequest::ThreadRead { project, thread, lines }
                }
                ThreadCmd::Keys { thread, mut keys, text } => {
                    let (project, thread) = shift_project(thread, &mut keys)?;
                    FrontRequest::ThreadKeys { project, thread, keys, text }
                }
                ThreadCmd::Stop { thread, legacy } => {
                    let (project, thread) = on_target(thread, legacy)?;
                    FrontRequest::ThreadStop { project, thread }
                }
                ThreadCmd::Restart { thread, legacy, resume, profile } => {
                    let (project, thread) = on_target(thread, legacy)?;
                    FrontRequest::ThreadRestart { project, thread, resume, profile }
                }
                ThreadCmd::Park { thread, legacy, now, note, user_request, cancel } => {
                    let (project, thread) = on_target(thread, legacy)?;
                    FrontRequest::ThreadPark { project, thread, now, note, user_request, cancel }
                }
                ThreadCmd::Unpark { thread, legacy, machine, repo, dry_run } => {
                    let (project, thread) = on_target(thread, legacy)?;
                    FrontRequest::ThreadUnpark { project, thread, machine, repo, dry_run }
                }
                ThreadCmd::Answer { thread, mut answers, text, comments } => {
                    let (project, thread) = shift_project(thread, &mut answers)?;
                    if aaa_proto::canonical_short_id(&thread, 'q').is_some() {
                        if !comments.is_empty() {
                            anyhow::bail!("q- 질문에는 --text를 쓴다. --comment는 t- 카드의 질문 번호별 의견이다");
                        }
                        return front(
                            json,
                            FrontRequest::QuestionReply {
                                project,
                                id: thread,
                                choice: (!answers.is_empty()).then(|| answers.join(" ")),
                                text,
                            },
                        );
                    }
                    let view = match front_call(FrontRequest::ThreadShow {
                        project: project.clone(),
                        thread: thread.clone(),
                    })? {
                        Ok(FrontResponse::Thread(t)) => t,
                        Ok(_) => anyhow::bail!("host가 thread 대신 다른 답을 했다"),
                        Err(e) => anyhow::bail!("{}", e.message),
                    };
                    let Some(q) = view.questions else { anyhow::bail!("{thread}에는 답할 질문이 없다") };
                    let count = q
                        .items
                        .iter()
                        .enumerate()
                        .map(|(i, q)| q.source_number.unwrap_or(i as u32 + 1) as usize)
                        .max()
                        .unwrap_or(0);
                    let letters = answer_picks(&answers.join(" "), count)?;
                    let mut picks = Vec::new();
                    for (i, item) in q.items.iter().enumerate() {
                        let number = item.source_number.unwrap_or(i as u32 + 1) as usize;
                        picks.push(
                            letters[number - 1]
                                .map(|n| {
                                    let letter = char::from(b'A' + n as u8).to_string();
                                    item.choice_index(&letter).ok_or_else(|| anyhow::anyhow!("없는 선택지다: {letter}"))
                                })
                                .transpose()?,
                        );
                    }
                    for (i, pick) in letters.iter().enumerate() {
                        if pick.is_some()
                            && !q
                                .items
                                .iter()
                                .enumerate()
                                .any(|(n, q)| q.source_number.unwrap_or(n as u32 + 1) as usize == i + 1)
                        {
                            anyhow::bail!("{}번 질문은 이미 답했거나 없다", i + 1);
                        }
                    }
                    let comments = answer_comments(&comments, &q.items)?;
                    FrontRequest::ThreadAnswer { project, thread, report_hash: q.report_hash, picks, text, comments }
                }
                ThreadCmd::Message { thread, legacy, interrupt, text_file, text, summary_file } => {
                    let (project, thread) = on_target(thread, legacy)?;
                    if text_file.as_deref() == Some("-") && summary_file.as_deref() == Some("-") {
                        anyhow::bail!("--text-file과 --summary-file을 둘 다 표준 입력(-)으로 줄 수 없다");
                    }
                    let text = text_arg(text, text_file)?;
                    let summary = summary_file.as_deref().map(|f| file_text(f, "요약 파일")).transpose()?;
                    let mode =
                        if interrupt { aaa_proto::MessageMode::Interrupt } else { aaa_proto::MessageMode::Queue };
                    let req =
                        FrontRequest::ThreadMessage { project: project.clone(), thread: thread.clone(), text, mode };
                    return with_summary(json, req, project, Some(thread), summary, None);
                }
            },
        ),
        Cmd::Image { cmd } => crate::image::run(json, cmd),
        Cmd::Review { cmd } => crate::review::run(json, cmd),
        Cmd::Post(args) => post_cmd(json, args),
        Cmd::Events { project, after, follow } => events_cmd(json, project, after, follow),
        Cmd::Migrate { cmd } => crate::migrate::run(json, cmd),
        Cmd::Hook { agent, payload } => {
            // 전역 hook: aaa 환경이 없으면 stdin도 읽지 않고 끝낸다(herdr-projects progress::hook).
            if !aaa_agent::hooks::in_session() {
                return Ok(ExitCode::SUCCESS);
            }
            // 기본은 무음. AAA_LOG=debug로만 전송 실패 원인을 stderr에 남긴다.
            init_log("off");
            // harness를 막지 않는다: 무엇이 실패해도 종료 코드 0, 기본 출력 없음
            // 표준 출력은 agent가 돌려준 진행 안내 JSON 한 줄만이다(Claude Code는 JSON이 아니면 버린다)
            let body = match payload {
                Some(p) => p,
                None => {
                    let mut t = String::new();
                    aaa_common::diagnostic::ignored(
                        std::io::Read::read_to_string(&mut std::io::Read::take(std::io::stdin(), 1 << 20), &mut t),
                        "보조 작업 실패 뒤 기존 처리를 계속한다",
                    );
                    t
                }
            };
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&body)
                && let Ok(rt) = runtime()
            {
                match rt.block_on(aaa_agent::hooks::send(&agent, v)) {
                    Ok(Some(out)) => println!("{out}"),
                    Ok(None) => {}
                    Err(error) => tracing::debug!("hook 전송 실패: {error:#}"),
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Report { percent, unknown: _, activity } => {
            // herdr-projects `progress::report`: 100은 「끝」이다
            let activity = if percent == Some(100) { "Done".to_string() } else { activity };
            if activity.trim().is_empty() {
                anyhow::bail!("--activity가 비었다");
            }
            runtime()?.block_on(aaa_agent::hooks::report(percent, &activity))?;
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Search { query, project, kind, after, limit } => front(
            json,
            FrontRequest::RecordSearch {
                query: aaa_proto::RecordSearchQuery {
                    query,
                    project,
                    kind: kind.map(|k| serde_json::from_value(serde_json::Value::String(k))).transpose()?,
                    after,
                    limit,
                },
            },
        ),
        Cmd::Record { project, key } => {
            let mut offset = None;
            let mut full: Option<aaa_proto::RecordDocument> = None;
            loop {
                let r = front_call(FrontRequest::RecordOpen { project: project.clone(), key: key.clone(), offset })?;
                match r {
                    Ok(FrontResponse::RecordDocument(doc)) => {
                        offset = doc.next;
                        if let Some(f) = full.as_mut() {
                            f.body.push_str(&doc.body);
                            f.next = doc.next;
                        } else {
                            full = Some(doc);
                        }
                        if offset.is_none() {
                            break;
                        }
                    }
                    Err(e) => return front_print(json, Err(e)),
                    Ok(_) => anyhow::bail!("원문 응답 형식이 다르다"),
                }
            }
            front_print(json, Ok(FrontResponse::RecordDocument(full.unwrap())))
        }
        Cmd::Question { cmd } => front(
            json,
            match cmd {
                QuestionCmd::List { all } => FrontRequest::Questions { all },
                QuestionCmd::Answer { id, choice, text, auto, reason_file } => {
                    if auto {
                        FrontRequest::QuestionAutoAnswer {
                            project: project_or_here(None)?,
                            id,
                            choice,
                            text,
                            reason: std::fs::read_to_string(reason_file.expect("clap requires reason_file"))?,
                        }
                    } else if aaa_proto::canonical_short_id(&id, 'q').is_some() {
                        FrontRequest::QuestionReply { project: project_or_here(None)?, id, choice, text }
                    } else {
                        FrontRequest::QuestionAnswer {
                            id,
                            choice: choice.or(text).ok_or_else(|| anyhow::anyhow!("답이 필요하다"))?,
                            menu: None,
                            pick: None,
                        }
                    }
                }
            },
        ),
        Cmd::Attach { .. } => {
            anyhow::bail!("{}", aaa_common::front::TUI_REMOVED)
        }
        Cmd::Machine { cmd } => {
            if let Some(code) = machine_post_cmd(json, &cmd)? {
                return Ok(code);
            }
            front(
                json,
                match cmd {
                    MachineCmd::List => FrontRequest::Machines,
                    MachineCmd::Workspaces { name, project, allow, deny } => {
                        let project = project_or_here(project)?;
                        let request = FrontRequest::WorkspaceAccess { project: project.clone(), machine: name.clone() };
                        if let Some(workspace) = allow.as_ref().or(deny.as_ref()) {
                            let view = match front_call(request)? {
                                Ok(FrontResponse::WorkspaceAccess(view)) => view,
                                Err(e) => return front_print(json, Err(e)),
                                _ => anyhow::bail!("workspace 허용 응답이 아니다"),
                            };
                            FrontRequest::WorkspaceAccessSet {
                                project,
                                machine: name,
                                workspace: workspace.clone(),
                                enabled: allow.is_some(),
                                revision: view.revision,
                            }
                        } else {
                            request
                        }
                    }
                    MachineCmd::Enroll { close: true, .. } => FrontRequest::EnrollClose,
                    MachineCmd::Enroll { open, .. } => FrontRequest::EnrollOpen { minutes: parse_minutes(&open)? },
                    MachineCmd::Pending => FrontRequest::EnrollPending,
                    MachineCmd::Approve { name, code } => FrontRequest::EnrollApprove { name, code },
                    MachineCmd::Rename { name, to } => FrontRequest::MachineRename { name, to },
                    MachineCmd::Revoke { name } => FrontRequest::MachineRevoke { name },
                    MachineCmd::Reserve { name, holder, note, duration, project, user_request } => {
                        FrontRequest::MachineReserve {
                            machine: name,
                            project: project_or_here(project)?,
                            holder: holder.context("--holder가 필요하다(t-NNNN thread나 d-NNNN 할 일)")?,
                            note,
                            until_ms: duration.as_deref().map(until_ms).transpose()?,
                            user_request,
                        }
                    }
                    MachineCmd::Release { name, user_request } => {
                        FrontRequest::MachineRelease { machine: name, user_request }
                    }
                    MachineCmd::Weight { name, value } => FrontRequest::MachineWeightSet {
                        machine: name,
                        weight: if value == "auto" {
                            None
                        } else {
                            Some(value.parse().with_context(|| format!("비중은 0 이상 숫자나 auto다: {value}"))?)
                        },
                    },
                },
            )
        }
        Cmd::Library { cmd: LibraryCmd::Cat { name, path } } => library_cat(json, name, path),
        Cmd::Library { cmd } => front(
            json,
            match cmd {
                LibraryCmd::Link { thread, legacy, days } => {
                    let (project, thread) = on_target(thread, legacy)?;
                    FrontRequest::LibraryLink { project, thread, days }
                }
                LibraryCmd::Archive { names, remove } => {
                    let project = project_or_here(None)?;
                    FrontRequest::LibraryArchive {
                        reports: names
                            .into_iter()
                            .map(|name| aaa_proto::LibraryRef { project: project.clone(), name })
                            .collect(),
                        archived: !remove,
                    }
                }
                LibraryCmd::Cat { .. } => unreachable!(),
                LibraryCmd::Show { name } => FrontRequest::LibraryOpen { project: project_or_here(None)?, name },
                LibraryCmd::List { project } => FrontRequest::LibraryReports { project },
                LibraryCmd::Publish { source, project, name, live, to_user, title, from_thread } => {
                    FrontRequest::LibraryPublish {
                        source: absolute_source(&source)?,
                        project: project_or_here(project)?,
                        name,
                        live,
                        to_user,
                        title,
                        from_thread,
                    }
                }
                LibraryCmd::Delete { project, names } => {
                    let project = project_or_here(project)?;
                    FrontRequest::LibraryDelete {
                        reports: names.into_iter().map(|name| LibraryRef { project: project.clone(), name }).collect(),
                    }
                }
                LibraryCmd::Import { source, project, prefix } => FrontRequest::LibraryImport {
                    source: absolute_source(&source)?,
                    project: project_or_here(project)?,
                    prefix,
                },
                LibraryCmd::Prune { older_than, project, dry_run } => {
                    if dry_run {
                        FrontRequest::LibraryPrunePreview { project, older_than_days: older_than }
                    } else {
                        FrontRequest::LibraryPrune {
                            project,
                            older_than_days: older_than.context("--older-than이 필요하다")?,
                        }
                    }
                }
                LibraryCmd::Rotate => FrontRequest::LibraryRotate,
                LibraryCmd::Revoke { id } => FrontRequest::LibraryLinkRevoke { id },
            },
        ),
        Cmd::Web { cmd } => front(
            json,
            match cmd {
                WebCmd::Pair { reader } => {
                    FrontRequest::WebPair { role: if reader { DeviceRole::Reader } else { DeviceRole::Controller } }
                }
                WebCmd::Publish { off: Some(_), .. } => FrontRequest::WebPublishOff,
                WebCmd::Publish { port: Some(port), .. } => FrontRequest::WebPublishPort { port },
                WebCmd::Publish { .. } => FrontRequest::WebPublish,
                WebCmd::Status => FrontRequest::WebStatus,
                WebCmd::Devices => FrontRequest::Devices,
                WebCmd::Revoke { id } => FrontRequest::DeviceRevoke { id },
                WebCmd::Rename { id, label } => FrontRequest::DeviceRename { id, label },
            },
        ),
        Cmd::Doctor(a) => doctor_cmd(a, json),
        Cmd::Integration { cmd } => {
            let remove = matches!(cmd, IntegrationCmd::Uninstall { .. });
            let target = match cmd {
                IntegrationCmd::Install { target } | IntegrationCmd::Uninstall { target } => target,
            };
            let exe = std::env::current_exe()?;
            let backups = if target == "agy" {
                crate::integration::configure_agy(&aaa_config::paths::home(), &exe, remove)?
            } else {
                crate::integration::configure(crate::integration::codex_home(), &exe, remove)?
            };
            if json {
                println!("{}", serde_json::json!({"removed":remove,"backups":backups}));
            } else {
                let action = if remove { "제거" } else { "설치" };
                if target == "agy" {
                    println!("agy 전역 연동을 {action}했다.");
                } else {
                    println!("Codex 전역 연동을 {action}했다. 새 hook은 Codex에서 신뢰해야 한다.");
                }
                for path in backups {
                    println!("백업: {}", path.display());
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Service { cmd } => service_cmd(cmd, json),
        Cmd::Config { cmd } => config_cmd(cmd, json),
        Cmd::WorkRoot { cmd } => crate::work_root::run(json, cmd),
        Cmd::Deploy { cmd } => crate::rollout::run(json, cmd),
        Cmd::Update { cmd: None, from, release_version, dry_run, machine } => {
            let action = aaa_proto::DeployAction::Update {
                from: from.as_ref().map(|p| p.to_string_lossy().into()),
                version: release_version.clone(),
                dry_run,
            };
            if (!dry_run || machine.is_some())
                && let Some(code) = crate::deployment::route(json, machine, action)?
            {
                return Ok(code);
            }
            crate::update::update(json, from, release_version, dry_run)
        }
        Cmd::Update { cmd: Some(UpdateCmd::Rollback { name, dry_run, machine }), .. } => {
            if (!dry_run || machine.is_some())
                && let Some(code) = crate::deployment::route(
                    json,
                    machine,
                    aaa_proto::DeployAction::Rollback { name: name.clone(), dry_run },
                )?
            {
                return Ok(code);
            }
            crate::update::rollback(json, name, dry_run)
        }
        Cmd::Update { cmd: Some(UpdateCmd::Status { id, wait, machine }), .. } => {
            crate::deployment::status(json, machine, id, wait)
        }
        Cmd::Update { cmd: Some(UpdateCmd::List), .. } => crate::update::list(json),
    }
}

/// 셸 완성(herdr `completion`, src/cli/completion.rs): clap 명령 정의에서 만든다. zsh는 `--opt=[…]`를 `--opt[…]`로 바꿔
/// `--opt 값`(빈칸) 꼴을 완성한다(herdr `space_separated_zsh_long_options`).
fn completion(shell: &str) -> Result<ExitCode> {
    use clap::CommandFactory as _;
    use clap_complete::{Shell, generate};
    let sh = match shell {
        "bash" => Shell::Bash,
        "elvish" => Shell::Elvish,
        "fish" => Shell::Fish,
        "powershell" => Shell::PowerShell,
        _ => Shell::Zsh,
    };
    let mut cmd = with_cli_stack(Cli::command).context("완성 명령 트리 thread를 시작하지 못했다")?;
    let mut out = Vec::new();
    generate(sh, &mut cmd, "aaa", &mut out);
    let script = String::from_utf8(out).context("완성 스크립트가 UTF-8이 아니다")?;
    let script = if sh == Shell::Zsh { space_separated_zsh_long_options(&script) } else { script };
    use std::io::Write as _;
    std::io::stdout().write_all(script.as_bytes())?;
    Ok(ExitCode::SUCCESS)
}

fn space_separated_zsh_long_options(script: &str) -> String {
    let mut output = String::with_capacity(script.len());
    let mut rest = script;
    while let Some(start) = rest.find("--") {
        output.push_str(&rest[..start]);
        let candidate = &rest[start + 2..];
        let name_len: usize =
            candidate.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-').map(char::len_utf8).sum();
        if name_len > 0 && candidate[name_len..].starts_with("=[") {
            output.push_str("--");
            output.push_str(&candidate[..name_len]);
            output.push('[');
            rest = &candidate[name_len + 2..];
        } else {
            output.push_str("--");
            rest = candidate;
        }
    }
    output.push_str(rest);
    output
}

/// host와 CLI의 현재 폴더가 다를 수 있다. symlink는 canonicalize하지 않고 host에서 거부한다.
fn absolute_source(path: &std::path::Path) -> Result<String> {
    let path = if path.is_absolute() { path.to_path_buf() } else { std::env::current_dir()?.join(path) };
    Ok(path.to_str().context("파일 경로는 UTF-8이어야 한다")?.to_string())
}

fn load_or_exit<T>(r: Result<aaa_config::Loaded<T>, aaa_config::ConfigErrors>) -> Result<T> {
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

/// `90m`·`2h`·`1d` 뒤의 시각(ms)
fn until_ms(s: &str) -> Result<u64> {
    let (n, unit) = s.trim().split_at(s.trim().len().saturating_sub(1));
    let n: u64 = n.parse().with_context(|| format!("기간은 90m·2h·1d처럼 쓴다: {s}"))?;
    let ms = match unit {
        "m" => 60_000,
        "h" => 3_600_000,
        "d" => 86_400_000,
        _ => anyhow::bail!("기간은 90m·2h·1d처럼 쓴다: {s}"),
    };
    anyhow::ensure!(n > 0, "기간은 0보다 길다: {s}");
    Ok(aaa_common::util::now_ms() + n * ms)
}

/// thread 세션 안(보기 권한)의 `machine reserve|release`: 자기 머신을 post 길로 잡고 푼다(host가 보낸 pane의 thread·머신을
/// 확인한다). 세션 밖·조작 권한이면 None(Front 요청)
fn machine_post_cmd(json: bool, cmd: &MachineCmd) -> Result<Option<ExitCode>> {
    use aaa_proto::{PostInput, PostResult, PostType};
    use sha2::Digest;
    let (topic, name, note, duration) = match cmd {
        MachineCmd::Reserve { name, note, duration, holder, .. } => {
            ("machine-hold", name, note.clone(), duration.clone().filter(|_| holder.is_none()))
        }
        MachineCmd::Release { name, .. } => ("machine-release", name, None, None),
        _ => return Ok(None),
    };
    if std::env::var_os(aaa_agent::hooks::ENV_SESSION).is_none() {
        return Ok(None);
    }
    let rt = runtime()?;
    if local_front_in_session(&rt)?.is_some_and(|c| c.role.can_control()) {
        return Ok(None);
    }
    if let MachineCmd::Reserve { holder: Some(_), .. } = cmd {
        anyhow::bail!("thread는 자기 머신을 자기 이름으로만 예약한다(--holder 없이)");
    }
    let body = serde_json::json!({
        "machine": name,
        "note": note,
        "until_ms": duration.as_deref().map(until_ms).transpose()?,
    });
    let input = PostInput {
        post_id: hex::encode(&sha2::Sha256::digest(aaa_common::util::new_id().as_bytes())[..16]),
        to: "coordinator".into(),
        post_type: PostType::Notice,
        topic: topic.into(),
        title: format!("{name} {}", if topic == "machine-hold" { "독점 예약" } else { "독점 예약 풂" }),
        body: body.to_string(),
        needs_user: false,
        refs: vec![],
    };
    let result = rt.block_on(aaa_agent::post::send(serde_json::to_value(input)?))?;
    if json {
        println!("{}", serde_json::to_value(&result)?);
    }
    Ok(Some(match result {
        PostResult::Accepted { .. } | PostResult::AutomationCompleted { .. } => {
            if !json {
                println!("ok {name}");
            }
            ExitCode::SUCCESS
        }
        PostResult::Refused { reason, .. } => {
            if !json {
                eprintln!("err {reason}");
            }
            ExitCode::from(1)
        }
        PostResult::Queued { post_id } => {
            if !json {
                println!("queued {post_id} · host 연결 뒤 처리한다");
            }
            ExitCode::from(2)
        }
    }))
}

fn parse_minutes(s: &str) -> Result<u32> {
    let t = s.trim_end_matches('m');
    t.parse::<u32>().with_context(|| format!("분 단위로 적는다(예: 10m): {s}"))
}

fn front(json: bool, req: FrontRequest) -> Result<ExitCode> {
    front_print(json, front_call(req)?)
}

fn local_front_in_session(rt: &tokio::runtime::Runtime) -> Result<Option<aaa_common::front::FrontClient>> {
    match rt.block_on(aaa_common::front::FrontClient::connect(&aaa_config::paths::state_dir())) {
        Ok(client) => Ok(Some(client)),
        Err(error) if aaa_common::front::host_absent(&error) => Ok(None),
        Err(error) => Err(error),
    }
}

/// `<대상>`(새 꼴) 또는 `<프로젝트> <대상>`(옛 꼴): (프로젝트, 대상)
/// `1A 2B`(쉼표·붙여 쓰기도)를 질문마다 고른 선택지 번호로. 고르지 않은 질문은 None(--text가 대신한다)
fn answer_picks(text: &str, questions: usize) -> Result<Vec<Option<u32>>> {
    let mut picks = vec![None; questions];
    let compact: String = text.chars().filter(|c| !c.is_whitespace() && *c != ',').collect();
    let mut rest = compact.as_str();
    while !rest.is_empty() {
        let digits = rest.chars().take_while(char::is_ascii_digit).count();
        let letter = rest[digits..].chars().next().filter(char::is_ascii_alphabetic);
        let (Some(letter), true) = (letter, digits > 0) else {
            anyhow::bail!("답은 질문 번호와 선택지 글자다(예: 1A 2B): {text}");
        };
        let n: usize = rest[..digits].parse()?;
        if n == 0 || n > questions {
            anyhow::bail!("{n}번 질문이 없다(질문 {questions}개)");
        }
        picks[n - 1] = Some(u32::from(letter.to_ascii_uppercase() as u8 - b'A'));
        rest = &rest[digits + 1..];
    }
    Ok(picks)
}

fn answer_comments(values: &[String], items: &[aaa_proto::ThreadQuestion]) -> Result<Vec<Option<String>>> {
    if values.is_empty() {
        return Ok(Vec::new());
    }
    let mut comments = vec![None; items.len()];
    for value in values {
        let (number, text) = value.split_once('=').ok_or_else(|| anyhow::anyhow!("의견은 번호=글 꼴이다: {value}"))?;
        let number: u32 = number.trim().parse().context("의견의 질문 번호를 읽지 못했다")?;
        let index = items
            .iter()
            .enumerate()
            .position(|(i, q)| q.source_number.unwrap_or(i as u32 + 1) == number)
            .ok_or_else(|| anyhow::anyhow!("{number}번 질문은 이미 답했거나 없다"))?;
        let text = text.trim();
        if text.is_empty() || comments[index].is_some() {
            anyhow::bail!("{number}번 의견이 비었거나 중복됐다");
        }
        comments[index] = Some(text.to_string());
    }
    Ok(comments)
}

fn library_cat(json: bool, name: String, path: String) -> Result<ExitCode> {
    use std::io::Write;
    let project = project_or_here(None)?;
    let mut offset = 0;
    loop {
        match front_call(FrontRequest::LibraryRead {
            project: project.clone(),
            name: name.clone(),
            path: path.clone(),
            offset,
        })? {
            Ok(FrontResponse::LibraryChunk { data, next }) => {
                if json {
                    println!("{}", serde_json::to_string(&FrontResponse::LibraryChunk { data, next })?);
                } else {
                    std::io::stdout().lock().write_all(&data.0)?;
                }
                match next {
                    Some(n) if n > offset => offset = n,
                    None => return Ok(ExitCode::SUCCESS),
                    _ => anyhow::bail!("library 읽기 위치가 나아가지 않았다"),
                }
            }
            other => return front_print(json, other),
        }
    }
}

pub(crate) fn front_call(req: FrontRequest) -> Result<Result<FrontResponse, aaa_proto::ErrorBody>> {
    // 한 CLI 요청의 writer·응답 수신은 이 block_on 안에서 끝난다. 요청마다
    // CPU 수만큼 worker thread를 만들지 않아도 같은 I/O·timer를 구동한다.
    tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(async {
        let mut c = match aaa_common::front::FrontClient::connect(&aaa_config::paths::state_dir()).await {
            Ok(client) => client,
            Err(error)
                if aaa_agent::hooks::in_session()
                    && req.thread_relay_allowed()
                    && aaa_common::front::host_absent(&error) =>
            {
                return aaa_agent::thread_read::send(req).await;
            }
            Err(error) => return Err(error),
        };
        if matches!(&req, FrontRequest::LibraryRead { .. }) && c.proto.minor < 146 {
            anyhow::bail!("library 파일 읽기는 Front API 1.146 이상 host가 필요하다");
        }
        // 기획 main 직접 저장(t-2002): 옛 host는 같은 요청을 기획 branch에 쓴다
        if matches!(
            &req,
            FrontRequest::DesignSave { .. }
                | FrontRequest::DesignAttach { .. }
                | FrontRequest::DesignDetach { .. }
                | FrontRequest::DesignLog { .. }
                | FrontRequest::DesignRevert { .. }
        ) && c.proto.minor < 186
        {
            anyhow::bail!("기획 main 직접 저장·이력은 Front API 1.186 이상 host가 필요하다. host를 갱신한다");
        }
        if matches!(&req, FrontRequest::RecordSearch { .. } | FrontRequest::RecordOpen { .. }) && c.proto.minor < 140 {
            anyhow::bail!("작업 기록 검색은 host 1.140 이상이 필요하다");
        }
        if matches!(&req, FrontRequest::ThreadAnswer { comments, .. } | FrontRequest::AskAnswer { comments, .. } if !comments.is_empty()) && c.proto.minor < 133 {
            anyhow::bail!("질문별 의견은 Front API 1.133 이상 host가 필요하다. host를 갱신한다");
        }
        if let FrontRequest::Threads { query, .. } = &req
            && c.proto.minor < 130
            && (query.open || query.status.is_some() || query.limit.is_some() || query.after.is_some())
        {
            anyhow::bail!("thread 목록의 거르기·페이지는 Front API 1.130 이상 host가 필요하다");
        }
        if matches!(&req, FrontRequest::TaskLog { task: Some(_), .. }) && c.proto.minor < 89 {
            anyhow::bail!("ID로 기록을 찾으려면 Front API 1.89 이상 host가 필요하다");
        }
        if let FrontRequest::TaskEdit { ops, .. } = &req
            && c.proto.minor < 194
            && ops.iter().any(|op| matches!(op, aaa_proto::TaskOp::Add { review: Some(_), .. } | aaa_proto::TaskOp::Update { review: Some(_), .. }))
        {
            anyhow::bail!("할 일 검수 설정은 Front API 1.194 이상 host가 필요하다");
        }
        if let FrontRequest::TaskQueue { action: aaa_proto::QueueOp::Reserve { after_all: Some(ids), .. }, .. } = &req
            && c.proto.minor < 150 && ids.len() > 1
        {
            anyhow::bail!("여러 앞 할 일을 붙인 예약은 Front API 1.150 이상 host가 필요하다");
        }
        if matches!(&req, FrontRequest::TaskQueue { action: aaa_proto::QueueOp::Reserve { account: Some(_), .. }, .. })
            && c.proto.minor < 164
        {
            anyhow::bail!("계정을 지정한 예약은 Front API 1.164 이상 host가 필요하다");
        }
        // 옛 host는 모르는 앞 할 일 칸을 버리고 바로 자리 대기에 넣는다.
        if matches!(&req, FrontRequest::TaskQueue { action: aaa_proto::QueueOp::Reserve { after: Some(_), .. }, .. })
            && c.proto.minor < 113
        {
            anyhow::bail!("앞 할 일을 붙인 예약은 Front API 1.113 이상 host가 필요하다");
        }
        c.call(req).await
    })
}

/// 응답 출력(herdr `print_response`): JSON은 한 줄. 오류는 표준 오류로 가고 종료 코드 1이다(JSON이면 `{"error":…}` 한 줄).
pub(crate) fn front_print(json: bool, res: Result<FrontResponse, aaa_proto::ErrorBody>) -> Result<ExitCode> {
    match res {
        Ok(r) => {
            if json {
                println!("{}", serde_json::to_string(&r)?);
            } else {
                print!("{}", render_front(&r));
            }
            Ok(ExitCode::SUCCESS)
        }
        Err(e) => {
            if json {
                eprintln!("{}", serde_json::to_string(&serde_json::json!({ "error": e }))?);
            } else {
                eprintln!("aaa: {e}");
            }
            Ok(ExitCode::from(1))
        }
    }
}

/// `thread start`·`thread message`의 `--summary-file`: 먼저 형식을 검사하고 앞 요청이 된 뒤 작업 요약을 쓴다(plan §8.259). 앞 요청이 실패하면
/// 요약을 쓰지 않는다. 요약을 따로 보내므로 요약을 모르는 옛 host(FRONT_API 1.43)는 그 요청만 거절한다
fn account_pool_command(json: bool, cmd: AccountPoolCmd) -> Result<ExitCode> {
    if matches!(cmd, AccountPoolCmd::List) {
        return front(json, FrontRequest::AccountPool);
    }
    let pool = match front_call(FrontRequest::AccountPool)? {
        Ok(FrontResponse::AccountPool { pool }) => pool,
        response => return front_print(json, response),
    };
    let request = match cmd {
        AccountPoolCmd::List => unreachable!(),
        AccountPoolCmd::Add { account, group, models } => FrontRequest::AccountPoolMember {
            revision: pool.revision,
            account: account.clone(),
            member: Some(aaa_proto::AccountPoolMember {
                group: group.unwrap_or_else(|| account.clone()),
                account,
                models,
            }),
        },
        AccountPoolCmd::Remove { account } => {
            FrontRequest::AccountPoolMember { revision: pool.revision, account, member: None }
        }
        AccountPoolCmd::Project { project, on, off: _ } => {
            FrontRequest::AccountPoolProject { revision: pool.revision, project, enabled: on }
        }
    };
    front(json, request)
}

/// `aaa guide [주제]`: host에 묻지 않고 이 실행 파일에 든 안내를 보인다(`aaa automation schema`처럼)
fn guide(json: bool, topic: Option<&str>, all: bool) -> Result<ExitCode> {
    use aaa_host::guide::{Audience, TOPICS};
    let who = |a: Audience| match a {
        Audience::Coordinator => "coordinator",
        Audience::Thread => "thread",
    };
    let picked: Vec<_> = match (topic, all) {
        (Some(name), _) => match aaa_host::guide::topic(name) {
            Some(t) => vec![t],
            None => {
                let names: Vec<&str> = TOPICS.iter().map(|t| t.name).collect();
                anyhow::bail!("없는 주제다: {name}. 주제: {}", names.join(", "));
            }
        },
        (None, true) => TOPICS.iter().collect(),
        (None, false) => {
            if json {
                let list: Vec<_> = TOPICS
                    .iter()
                    .map(|t| serde_json::json!({ "name": t.name, "audience": who(t.audience), "when": t.when }))
                    .collect();
                println!("{}", serde_json::json!({ "topics": list }));
            } else {
                println!("aaa guide <주제>: 그 일을 할 때 읽는 세부 안내(AGENTS.md·brief에는 매 턴 규칙만 있다)\n");
                for a in [Audience::Coordinator, Audience::Thread] {
                    println!("{}", who(a));
                    for t in TOPICS.iter().filter(|t| t.audience == a) {
                        println!("  {:<12} {}", t.name, t.when);
                    }
                }
            }
            return Ok(ExitCode::SUCCESS);
        }
    };
    if json {
        let list: Vec<_> = picked
            .iter()
            .map(|t| serde_json::json!({ "name": t.name, "audience": who(t.audience), "when": t.when, "text": t.text }))
            .collect();
        println!("{}", serde_json::json!({ "topics": list }));
    } else {
        let texts: Vec<&str> = picked.iter().map(|t| t.text).collect();
        print!("{}", texts.join("\n"));
    }
    Ok(ExitCode::SUCCESS)
}

fn debug_since(value: &str) -> Result<u64> {
    if let Some((digits, unit)) = value.char_indices().last().map(|(i, c)| (&value[..i], c)) {
        let factor = match unit {
            'm' => Some(60_000u64),
            'h' => Some(3_600_000),
            'd' => Some(86_400_000),
            _ => None,
        };
        if let Some(factor) = factor {
            let span =
                digits.parse::<u64>()?.checked_mul(factor).ok_or_else(|| anyhow::anyhow!("--since 범위를 넘었다"))?;
            return Ok(aaa_common::util::now_ms().saturating_sub(span));
        }
    }
    Ok(value.parse()?)
}

#[cfg(test)]
mod tests;

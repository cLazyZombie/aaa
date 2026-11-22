//! 프로젝트 명령과 대상 프로젝트 고르기.

use super::*;

#[derive(Subcommand)]
pub(super) enum ProjectCmd {
    /// coordinator auto mode를 켜거나 끈다(사람 전용, 생략하면 조회)
    AutoMode {
        #[arg(value_parser = ["on", "off"])]
        mode: Option<String>,
    },
    /// 프로젝트 목록(보관한 것은 `--all`일 때만)
    List {
        #[arg(long)]
        all: bool,
    },
    /// 프로젝트 폴더를 만든다(PROJECT.md·MEMORY.md·TASKS.md …, herdr-projects `new`)
    Create {
        /// 이름(인스턴스에서는 생략하면 인스턴스 이름: worktree·branch의 내부 이름이 다른 인스턴스와 겹치지 않는다)
        name: Option<String>,
        /// 프로젝트가 무엇을 위한 것인가(brief 머리에 들어간다)
        #[arg(long, default_value = "")]
        goal: String,
        /// 저장소. `PATH` 또는 `PATH@MACHINE`(머신이 없으면 host 머신), 여러 번 줄 수 있다
        #[arg(long = "repo", value_name = "PATH[@MACHINE]")]
        repos: Vec<String>,
        /// 이 프로젝트 thread의 기본 profile(안 주면 host.toml `[defaults] thread_profile`, 그다음 claude)
        #[arg(long, value_name = "NAME")]
        thread_profile: Option<String>,
        /// `aaa project open`이 coordinator를 띄우는 profile(안 주면 host.toml `[defaults] coordinator_profile`, 그다음 claude)
        #[arg(long, value_name = "NAME")]
        coordinator_profile: Option<String>,
    },
    /// coordinator를 연다(herdr-projects `open`): 돌고 있으면 그 pane을 알리고, 없으면 host 머신에서 프로젝트 폴더를 작업
    /// 폴더로 띄워 프로젝트 이름 workspace의 새 탭 「coordinator」에 둔다
    Open {
        project: String,
        /// coordinator profile(안 주면 project.toml coordinator_profile → host.toml [defaults] → claude). `aaa profile list`
        #[arg(long, value_name = "NAME")]
        profile: Option<String>,
        /// coordinator가 돌아도 하나 더 띄운다
        #[arg(long)]
        new: bool,
    },
    /// project.toml 값 하나를 바꾼다(herdr-projects `set`): goal, thread_profile, coordinator_profile,
    /// max_parallel_threads, auto_resolve_days, repos.add, repos.remove, nudge, mute
    Set { project: String, key: String, value: String },
    /// 지금 설정 값(`set`이 고치는 key 모두, 잘못된 값은 host가 쓰는 기본값으로)
    Show { project: String },
    /// 멈춘다: ticker가 건너뛰고 `thread start`를 거부한다. 도는 agent는 멈추지 않는다
    Pause {
        /// (옛 꼴의 프로젝트, 생략한다)
        #[arg(hide = true)]
        project: Option<String>,
    },
    /// 멈춘 프로젝트를 다시 돌린다
    Resume {
        /// (옛 꼴의 프로젝트, 생략한다)
        #[arg(hide = true)]
        project: Option<String>,
    },
    /// 보관한다: 멈추고, 목록에서 숨기고, 살아 있는 pane(coordinator·thread)을 닫는다. `project open`을 거부한다
    Archive {
        /// (옛 꼴의 프로젝트, 생략한다)
        #[arg(hide = true)]
        project: Option<String>,
    },
    /// 보관을 푼다
    Unarchive { project: String },
    /// 폴더를 휴지통(projects root의 .trash)으로 옮기고 host 기록을 그 안에 남긴다. worktree·branch는 건드리지 않는다
    Delete {
        /// (옛 꼴의 프로젝트, 생략한다)
        #[arg(hide = true)]
        project: Option<String>,
        /// coordinator나 thread pane이 살아 있어도
        #[arg(long)]
        force: bool,
    },
    /// 이름(폴더 이름)을 바꾼다. 끝나지 않은 thread나 도는 coordinator가 있으면 거부한다
    Rename {
        new_name: String,
        /// (옛 꼴 `<프로젝트> <새 이름>`의 새 이름)
        #[arg(hide = true)]
        legacy: Option<String>,
        /// 할 일만 보이고 바꾸지 않는다
        #[arg(long)]
        dry_run: bool,
    },
    /// 끝난 thread가 남긴 worktree·병합된 branch를 그 머신에서 치운다(force 없음)
    Sweep {
        /// (옛 꼴의 프로젝트, 생략한다)
        #[arg(hide = true)]
        project: Option<String>,
        /// 목록만
        #[arg(long)]
        dry_run: bool,
        /// 묻지 않고 치운다
        #[arg(long)]
        yes: bool,
    },
}

pub(super) fn project_cmd(json: bool, cmd: ProjectCmd) -> Result<ExitCode> {
    match cmd {
        ProjectCmd::AutoMode { mode } => {
            let project = project_or_here(None)?;
            front(
                json,
                match mode {
                    Some(mode) => FrontRequest::AutoModeSet { project, on: mode == "on" },
                    None => FrontRequest::AutoModeGet { project },
                },
            )
        }
        ProjectCmd::List { all } => front(json, FrontRequest::Projects { all }),
        ProjectCmd::Pause { project } => lifecycle(json, project_or_here(project)?, ProjectAction::Pause),
        ProjectCmd::Resume { project } => lifecycle(json, project_or_here(project)?, ProjectAction::Resume),
        ProjectCmd::Archive { project } => lifecycle(json, project_or_here(project)?, ProjectAction::Archive),
        ProjectCmd::Unarchive { project } => lifecycle(json, project, ProjectAction::Unarchive),
        ProjectCmd::Delete { project, force } => {
            front(json, FrontRequest::ProjectDelete { project: project_or_here(project)?, force })
        }
        ProjectCmd::Rename { new_name, legacy, dry_run } => {
            let (project, new_name) = on_target(new_name, legacy)?;
            front(json, FrontRequest::ProjectRename { project, to: new_name, dry_run })
        }
        ProjectCmd::Open { project, profile, new } => front(json, FrontRequest::ProjectOpen { project, profile, new }),
        ProjectCmd::Sweep { project, dry_run, yes } => {
            let project = project_or_here(project)?;
            if dry_run || yes {
                return front(json, FrontRequest::ProjectSweep { project, dry_run });
            }
            // 참고 `sweep::run`: 목록을 보이고 터미널에서 묻는다(터미널이 아니면 --yes)
            let list = front_call(FrontRequest::ProjectSweep { project: project.clone(), dry_run: true })?;
            let Ok(FrontResponse::ProjectChange(c)) = &list else { return front_print(json, list) };
            if c.notes.iter().any(|n| n.ends_with("치울 것이 없다")) {
                return front_print(json, list);
            }
            print!("{}", render_front(list.as_ref().unwrap()));
            if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
                anyhow::bail!("치우려면 --yes를 준다(목록만은 --dry-run)");
            }
            if !crate::person::confirm("모두 치울까?")? {
                println!("아무것도 치우지 않았다");
                return Ok(ExitCode::SUCCESS);
            }
            front(json, FrontRequest::ProjectSweep { project, dry_run: false })
        }
        ProjectCmd::Create { name, goal, repos, thread_profile, coordinator_profile } => {
            let Some(name) = name.or_else(aaa_config::paths::instance) else {
                anyhow::bail!("프로젝트 이름이 필요하다: `aaa project create <이름>`");
            };
            let repos = repos.iter().map(|r| local_repo_path(r)).collect();
            front(json, FrontRequest::ProjectCreate { name, goal, repos, thread_profile, coordinator_profile })
        }
        ProjectCmd::Show { project } => front(json, FrontRequest::ProjectSettings { project }),
        ProjectCmd::Set { project, key, value } => {
            let value = if key.starts_with("repos.") { local_repo_path(&value) } else { value };
            let res = front_call(FrontRequest::ProjectSet {
                project: project.clone(),
                key: key.clone(),
                value: value.clone(),
            })?;
            if json
                || res
                    .as_ref()
                    .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
                    .is_err()
            {
                return front_print(json, res);
            }
            println!("{project}: {key} = {}", value.trim());
            Ok(ExitCode::SUCCESS)
        }
    }
}

pub(super) fn lifecycle(json: bool, project: String, action: ProjectAction) -> Result<ExitCode> {
    front(json, FrontRequest::ProjectLifecycle { project, action })
}

/// 머신 없는 상대 경로는 이 셸의 cwd 기준 절대 경로로 보낸다(host는 다른 cwd에서 돈다)
pub(super) fn local_repo_path(arg: &str) -> String {
    let arg = arg.trim();
    let machine = arg.rsplit_once('@').filter(|(p, m)| {
        !p.is_empty() && !m.is_empty() && m.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
    });
    if machine.is_some() || std::path::Path::new(arg).is_absolute() {
        return arg.to_string();
    }
    std::path::absolute(arg).map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| arg.to_string())
}

/// 기획서 ID(`k-NNNN`)의 저장소·경로: host 원장의 지금 main 트리 목록에서 찾는다(t-1957)
pub(super) fn design_id_target(project: &str, id: &str) -> Result<(String, String)> {
    let Some(want) = aaa_proto::canonical_short_id(id, aaa_proto::DESIGN_ID_PREFIX) else {
        anyhow::bail!("문서 경로를 준다: `aaa plan show <저장소> <경로>` 또는 `aaa plan show k-NNNN`");
    };
    match front_call(FrontRequest::DesignIds { project: project.to_string() })? {
        Ok(FrontResponse::DesignIds { ids }) => {
            ids.into_iter().find(|i| i.id == want).map(|i| (i.repo, i.path)).ok_or_else(|| {
                anyhow::anyhow!("지금 기획 트리에 없는 ID다: {want}(`aaa plan list`로 트리를 새로 읽는다)")
            })
        }
        Ok(_) => anyhow::bail!("예상하지 못한 응답"),
        Err(e) => anyhow::bail!("{}", e.message),
    }
}

/// host가 아는 프로젝트(이름, 보관 여부). 프로젝트 인자를 생략한 명령이 한 번 묻는다(plan §8.285)
pub(super) fn known_projects() -> Result<&'static [(String, bool)]> {
    static KNOWN: std::sync::OnceLock<Vec<(String, bool)>> = std::sync::OnceLock::new();
    if let Some(k) = KNOWN.get() {
        return Ok(k);
    }
    let projects = match front_call(FrontRequest::Projects { all: true })? {
        Ok(FrontResponse::Projects { projects }) => projects,
        Ok(_) => anyhow::bail!("프로젝트 목록 응답이 아니다"),
        Err(e) => anyhow::bail!("{}", e.message),
    };
    let k = projects.into_iter().map(|p| (p.name, p.status == aaa_proto::ProjectStatus::Archived)).collect();
    Ok(KNOWN.get_or_init(|| k))
}

/// 프로젝트 인자(plan §8.285): host는 프로젝트 하나만 맡으므로 생략하면 그 이름(내부 이름)이다. 옛 꼴로 그 이름을 주면
/// 표준 오류에 한 줄 알리고 그대로 하고, 다른 이름이면 거부한다. 보관한 프로젝트·여럿이 남은 옛 상태의 이름은 그대로 받는다.
pub(crate) fn project_or_here(given: Option<String>) -> Result<String> {
    let known = known_projects()?;
    let open: Vec<&str> = known.iter().filter(|(_, archived)| !archived).map(|(n, _)| n.as_str()).collect();
    match given {
        None => match open.as_slice() {
            [one] => Ok(one.to_string()),
            [] => anyhow::bail!("프로젝트가 없다: 먼저 `aaa project create <이름>`으로 만든다"),
            many => anyhow::bail!(
                "보관하지 않은 프로젝트가 {}개다({}): host는 하나만 맡는다(plan §8.283)",
                many.len(),
                many.join(", ")
            ),
        },
        Some(p) => {
            if open.as_slice() == [p.as_str()] {
                static ONCE: std::sync::Once = std::sync::Once::new();
                ONCE.call_once(|| eprintln!("경고: 프로젝트 인자는 이제 생략한다(`{p}`를 빼고 쓴다, plan §8.285)"));
            } else if let [one] = open.as_slice()
                && !known.iter().any(|(n, _)| *n == p)
            {
                anyhow::bail!("이 host의 프로젝트는 `{one}`다(`{p}` 아님). 프로젝트 인자는 이제 생략한다");
            }
            Ok(p)
        }
    }
}

pub(super) fn on_target(first: String, second: Option<String>) -> Result<(String, String)> {
    match second {
        Some(target) => Ok((project_or_here(Some(first))?, target)),
        None => Ok((project_or_here(None)?, first)),
    }
}

/// 뒤에 여러 값이 오는 명령(`thread keys <t> <키…>`): 첫 값이 host가 아는 프로젝트 이름이면 옛 꼴이다
pub(super) fn shift_project(first: String, rest: &mut Vec<String>) -> Result<(String, String)> {
    if !rest.is_empty() && known_projects()?.iter().any(|(n, _)| *n == first) {
        let target = rest.remove(0);
        return Ok((project_or_here(Some(first))?, target));
    }
    Ok((project_or_here(None)?, first))
}

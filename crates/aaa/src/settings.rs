//! `aaa profile`·`aaa safety`(herdr-projects 0.2.35 `profile`·`safety` 명령, plan §8.163).
//!
//! - 읽기(`profile list`·`profile resolve`·`safety show`)는 누구나 한다.
//! - 바꾸기는 사람이 aaa 세션 밖 터미널에서만 한다([`crate::person::require`]). profile은 host 머신의 host.toml, 허용 목록·
//!   기본값·safety는 host.toml을 toml_edit로 그 key만 고친다(주석·차례는 그대로). host·agent는 쓸 때마다 다시 읽는다.
//! - coordinator 쪽(`profile default|allow coordinator`, `coordinator_agent_args`)은 `aaa project open`이 쓴다(§8.180).

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use aaa_config::safety::{self, Layers, SafetyLayer};
use aaa_config::{Profile, profile_edit};
use aaa_proto::{FrontRequest, FrontResponse};
use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};

#[derive(Args)]
pub struct ProfileFields {
    /// model 이름(`--model`·codex `-m`로 간다. 비우면 harness 기본)
    #[arg(long, value_name = "NAME")]
    model: Option<String>,
    /// effort: claude low·medium·high·xhigh·max, codex none·minimal·low·medium·high·xhigh·max·ultra
    #[arg(long, value_name = "LEVEL")]
    effort: Option<String>,
    /// 언제 쓰는지 한 줄(coordinator가 고를 때 읽는다)
    #[arg(long, value_name = "TEXT")]
    description: Option<String>,
    /// harness에 더할 인자, 여러 번(--arg --verbose --arg --add-dir --arg /work)
    #[arg(long = "arg", value_name = "ARG", allow_hyphen_values = true)]
    args: Vec<String>,
}

#[derive(Subcommand)]
pub enum ProfileCmd {
    /// host의 profile(host.toml과 설치·로그인한 기본 profile claude·codex). --project면 그 프로젝트 기본값·허용 목록도
    List {
        #[arg(long)]
        machine: Option<String>,
        #[arg(long, value_name = "PROJECT")]
        project: Option<String>,
        /// 이름만 한 줄에 하나
        #[arg(long, conflicts_with = "project")]
        names: bool,
    },
    /// host에서 profile이 띄우는 설정을 JSON으로(이름이 없으면 host.toml [defaults] thread_profile, 없으면 claude)
    Resolve { name: Option<String> },
    /// host 머신의 host.toml에 profile을 더한다(사람이 aaa 세션 밖 터미널에서만)
    Add {
        name: String,
        /// claude·codex·shell·custom
        #[arg(long = "agent", alias = "harness", value_name = "KIND")]
        harness: String,
        #[command(flatten)]
        fields: ProfileFields,
    },
    /// host 머신의 host.toml profile을 고친다. --arg는 인자를 모두 바꾼다(사람이 터미널에서만)
    Edit {
        name: String,
        #[arg(long = "agent", alias = "harness", value_name = "KIND")]
        harness: Option<String>,
        #[command(flatten)]
        fields: ProfileFields,
        /// 더한 인자를 모두 지운다
        #[arg(long, conflicts_with = "args")]
        clear_args: bool,
    },
    /// host 머신의 host.toml profile을 지운다(사람이 터미널에서만)
    Remove { name: String },
    /// thread·coordinator가 쓸 수 있는 profile: 한 프로젝트 또는 모든 프로젝트 기본(host.toml [safety.*]
    /// thread_profiles·coordinator_profiles, 사람만)
    Allow {
        #[arg(value_parser = ["threads", "coordinator"])]
        role: String,
        #[arg(required_unless_present = "all")]
        names: Vec<String>,
        /// 이 프로젝트만(없으면 제 목록이 없는 모든 프로젝트)
        #[arg(long, value_name = "PROJECT")]
        project: Option<String>,
        /// 모든 profile을 허용한다(목록을 지운다)
        #[arg(long, conflicts_with = "names")]
        all: bool,
    },
    /// thread start(threads)·`project open`(coordinator)에 profile이 없고 project.toml 값도 없을 때 쓸 profile
    /// (host.toml [defaults], 사람만)
    Default {
        #[arg(value_parser = ["threads", "coordinator"])]
        role: String,
        name: String,
    },
}

#[derive(Subcommand)]
pub enum SafetyCmd {
    /// 실제 safety 값과 출처, 바꾸는 방법
    Show {
        /// 프로젝트, 또는 모든 프로젝트 기본값이면 --global
        #[arg(allow_hyphen_values = true)]
        target: String,
    },
    /// yolo를 켜거나 끈다: thread를 묻지 않고 시작하고 thread·coordinator를 harness 권한 건너뛰기 인자로 띄운다(사람이 터미널에서
    /// y로만)
    Yolo {
        #[arg(allow_hyphen_values = true)]
        target: String,
        /// on, off, default(모든 프로젝트 값을 따른다)
        #[arg(value_parser = ["on", "off", "default"])]
        state: String,
    },
    /// safety 값 하나를 바꾼다(사람이 터미널에서 y로만)
    Set {
        #[arg(allow_hyphen_values = true)]
        target: String,
        /// yolo, start_threads, trust_screens, coordinator_agent_args, thread_agent_args, routine_commands
        key: String,
        /// 값(*_agent_args는 인자들, 없으면 빈 목록), 또는 default
        #[arg(allow_hyphen_values = true, trailing_var_arg = true)]
        value: Vec<String>,
    },
}

fn config_dir() -> PathBuf {
    aaa_config::paths::config_dir()
}

/// host.toml을 읽는다(설정이 깨졌으면 그 까닭이 오류다)
fn host_cfg() -> Result<aaa_config::HostConfig> {
    aaa_config::load_host(&config_dir())
        .map(|l| l.config)
        .map_err(|e| anyhow::anyhow!("host.toml을 읽지 못했다:\n{}", e.render()))
}

/// safety 대상: 모든 프로젝트(`--global`, `[safety.default]`) 또는 한 프로젝트(`[safety.<이름>]`)
enum Target {
    Global,
    Project { name: String, dir: PathBuf },
}

impl Target {
    fn parse(word: &str) -> Result<Target> {
        if word == "--global" {
            return Ok(Target::Global);
        }
        Ok(Target::Project { name: word.to_string(), dir: project_dir(word)? })
    }

    fn table(&self) -> &str {
        match self {
            Target::Global => safety::DEFAULT_TABLE,
            Target::Project { name, .. } => name,
        }
    }

    fn label(&self) -> String {
        match self {
            Target::Global => "모든 프로젝트".into(),
            Target::Project { name, .. } => name.clone(),
        }
    }

    fn word(&self) -> &str {
        match self {
            Target::Global => "--global",
            Target::Project { name, .. } => name,
        }
    }

    fn project(&self) -> Option<&str> {
        match self {
            Target::Global => None,
            Target::Project { name, .. } => Some(name),
        }
    }
}

fn project_dir(name: &str) -> Result<PathBuf> {
    if !aaa_proto::valid_project(name) {
        bail!("프로젝트 이름 형식이 아니다(영문 소문자·숫자·-·_): {name}");
    }
    if name == safety::DEFAULT_TABLE {
        bail!("`default`는 모든 프로젝트 표의 이름이라 프로젝트로 쓰지 않는다: 모든 프로젝트는 --global");
    }
    let cfg = host_cfg()?;
    let root = if cfg.projects.root.is_empty() {
        aaa_config::paths::home().join(".aaa").join("projects")
    } else {
        PathBuf::from(&cfg.projects.root)
    };
    let dir = root.join(name);
    if !dir.join("PROJECT.md").is_file() {
        bail!("프로젝트가 없다: {name}");
    }
    Ok(dir)
}

fn project_toml(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("project.toml")).unwrap_or_default()
}

/// 값을 정할 층(host와 같은 차례: 프로젝트 표 → default 표 → 옛 자리 → 기본값)과 버린 값 경고
fn layers(target: &Target) -> (Layers, Vec<String>) {
    let (default, own, mut legacy, warnings) = safety::read(&config_dir(), target.project());
    if let Target::Project { dir, .. } = target {
        let p = safety::legacy_project_layer(&project_toml(dir));
        legacy.start_threads = p.start_threads;
        legacy.trust_screens = p.trust_screens;
    }
    let layers = match target {
        // 모든 프로젝트 값: default 표가 제 표다
        Target::Global => Layers { own: default, default: SafetyLayer::default(), legacy },
        Target::Project { .. } => Layers { own, default, legacy },
    };
    (layers, warnings)
}

/// 프로젝트의 thread 기본 profile(host `default_thread_profile`과 같다): project.toml → host.toml [defaults] → claude
fn default_thread_profile(target: &Target) -> String {
    default_profile(target, "thread_profile")
}

/// 프로젝트의 `thread_profile`·`coordinator_profile` 기본값(host `default_thread_profile`·`default_coordinator_profile`과 같다)
fn default_profile(target: &Target, key: &str) -> String {
    let own = match target {
        Target::Project { dir, .. } => safety::project_profile(&project_toml(dir), key),
        Target::Global => String::new(),
    };
    if !own.is_empty() {
        return own;
    }
    let global = safety::default_profile(&config_dir(), key);
    if global.is_empty() { "claude".into() } else { global }
}

fn rows_json(rows: &[safety::Row]) -> serde_json::Value {
    rows.iter()
        .map(|r| serde_json::json!({ "key": r.key, "value": r.value, "source": r.source, "note": r.note }))
        .collect()
}

pub fn safety_cmd(json: bool, cmd: SafetyCmd) -> Result<ExitCode> {
    match cmd {
        SafetyCmd::Show { target } => {
            let target = Target::parse(&target)?;
            let (layers, warnings) = layers(&target);
            let rows = layers.rows(target.project().is_some());
            let eff = layers.resolve();
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "target": target.word(),
                        "rows": rows_json(&rows),
                        "thread_profile": default_thread_profile(&target),
                        "warnings": warnings,
                    })
                );
                return Ok(ExitCode::SUCCESS);
            }
            print!("{}", show_text(&target, &rows, &eff, &warnings));
            Ok(ExitCode::SUCCESS)
        }
        SafetyCmd::Yolo { target, state } => {
            let command = format!("aaa safety yolo {target} {state}");
            set(json, &command, &target, "yolo", &[state])
        }
        SafetyCmd::Set { target, key, value } => {
            let command = format!("aaa safety set {target} {key} {}", value.join(" "));
            set(json, command.trim_end(), &target, &key, &value)
        }
    }
}

fn show_text(target: &Target, rows: &[safety::Row], eff: &safety::Safety, warnings: &[String]) -> String {
    let mut out = format!("safety 설정 — {} (사람만 바꾼다. agent는 바꾸지 못한다):\n", target.label());
    for r in rows {
        out.push_str(&format!("  {:<20} {}   [{}]\n", r.key, r.text(), r.source));
    }
    for w in warnings {
        out.push_str(&format!("  경고: {w}\n"));
    }
    let flags: Vec<String> = profile_edit::BUILTINS
        .iter()
        .map(|h| format!("{h}: {}", safety::yolo_flags(h).unwrap_or_default().join(" ")))
        .collect();
    out.push_str(&format!(
        "\nyolo는 thread를 묻지 않고 시작하고(start_threads auto) thread와 coordinator(`aaa coordinator open`)를 harness의 권한 건너뛰기 인자로 띄운다({}). shell·custom profile은 인자가 없어 그대로 묻는다.\n",
        flags.join("; ")
    ));
    out.push_str(
        "신뢰 창(thread pane의 폴더·hook 신뢰 확인)은 coordinator가 답하거나(coordinator) 사람에게 남긴다(user). 정하지 않으면 yolo일 때만 coordinator가 답한다. brief나 메시지가 신뢰 창에 입력되는 일은 없다.\n",
    );
    out.push_str(
        "thread_agent_args·coordinator_agent_args는 프로젝트 기본 profile(thread·coordinator)과 같은 harness의 기본 profile(claude·codex)에만 붙는다. 사람이 정의한 profile은 제 인자를 쓴다.\n",
    );
    out.push_str("routine 명령은 yolo와 따로다: 켜고 나서도 명령마다 사람이 승인해야 돈다(`aaa routine approve`).\n");
    if eff.trust_screens == "coordinator" {
        out.push_str("지금 coordinator가 신뢰 창에 답할 수 있다: thread 자기 폴더의 것만 받아들이고 다른 경로는 사람에게 넘긴다.\n");
    }
    let w = target.word();
    out.push_str(&format!(
        "\n바꾸기(사람이 aaa 세션 밖 터미널에서):\n  aaa safety yolo {w} on|off|default\n  aaa safety set {w} <key> <값>|default\n  aaa profile allow threads|coordinator <이름…>{}\n",
        target.project().map(|p| format!(" --project {p}")).unwrap_or_default()
    ));
    out.push_str(&format!("{}.\n", safety::RESTART_NOTE));
    out
}

/// host.toml 글(없으면 `schema = 1`부터)
fn host_text() -> (PathBuf, String) {
    let path = config_dir().join("host.toml");
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let text = if text.trim().is_empty() { format!("schema = {}\n", aaa_config::SCHEMA) } else { text };
    (path, text)
}

/// host.local.toml이 같은 key를 덮으면 알린다(고친 값이 닿지 않는다)
fn shadowed(table: &str, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(config_dir().join("host.local.toml")).ok()?;
    safety::has_key(&text, table, key).then_some(())?;
    Some(format!("host.local.toml의 [safety.{table}] {key}가 이 값을 덮는다: 그 파일에서도 고친다"))
}

fn set(json: bool, command: &str, target: &str, key: &str, words: &[String]) -> Result<ExitCode> {
    let target = Target::parse(target)?;
    let item = safety::value_item(key, words).map_err(|e| anyhow::anyhow!(e))?;
    crate::person::require(&format!("`{command}`(safety 바꾸기)"))?;
    let row = |key: &str| layers(&target).0.rows(target.project().is_some()).into_iter().find(|r| r.key == key);
    let before = row(key).map(|r| r.text()).unwrap_or_default();
    if key == "yolo" && words.first().is_some_and(|w| matches!(w.as_str(), "on" | "true" | "yes")) {
        println!(
            "{}의 yolo: thread를 묻지 않고 시작하고, thread·coordinator agent를 harness의 권한 건너뛰기 인자로 띄운다.",
            target.label()
        );
        println!("agent가 묻지 않고 명령을 실행하고 파일을 고친다.");
    }
    let after = if words.is_empty() { "(빈 목록)".to_string() } else { words.join(" ") };
    if !crate::person::confirm(&format!("{}의 {key}: 지금 {before} → `{after}`. 바꿀까?", target.label()))? {
        bail!("바꾸지 않았다");
    }
    let (path, text) = host_text();
    let edited = safety::set_in(&text, target.table(), key, item).map_err(|e| anyhow::anyhow!(e))?;
    profile_edit::write_atomic(&path, edited.as_bytes())
        .with_context(|| format!("{}에 쓰지 못했다", path.display()))?;
    let shown = row(key).map(|r| r.text()).unwrap_or_default();
    let warn = shadowed(target.table(), key);
    if json {
        println!("{}", serde_json::json!({ "target": target.word(), "key": key, "value": shown, "warning": warn }));
    } else {
        let note = if key == "coordinator_deploy" {
            "다음 배포 요청부터 즉시 적용한다. thread는 계속 막는다. 사용자가 채팅에서 배포를 지시했을 때만 쓴다"
        } else {
            safety::RESTART_NOTE
        };
        println!("{}: {key} = {shown}. {note}", target.label());
        if let Some(w) = warn {
            println!("경고: {w}");
        }
    }
    Ok(ExitCode::SUCCESS)
}

// ───────────────────────── profile ─────────────────────────

pub fn profile_cmd(json: bool, cmd: ProfileCmd) -> Result<ExitCode> {
    match cmd {
        ProfileCmd::List { machine, project, names } => list(json, machine, project, names),
        ProfileCmd::Resolve { name } => resolve(name),
        ProfileCmd::Add { name, harness, fields } => edit_profiles(
            json,
            profile_edit::ProfileChange::Add {
                name,
                profile: Profile {
                    harness,
                    model: fields.model.unwrap_or_default(),
                    effort: fields.effort.unwrap_or_default(),
                    description: fields.description.unwrap_or_default(),
                    args: fields.args,
                    ..Profile::default()
                },
            },
        ),
        ProfileCmd::Edit { name, harness, fields, clear_args } => edit_profiles(
            json,
            profile_edit::ProfileChange::Edit {
                name,
                harness,
                model: fields.model,
                effort: fields.effort,
                description: fields.description,
                args: if clear_args { Some(Vec::new()) } else { (!fields.args.is_empty()).then_some(fields.args) },
            },
        ),
        ProfileCmd::Remove { name } => edit_profiles(json, profile_edit::ProfileChange::Remove { name }),
        ProfileCmd::Allow { role, names, project, all } => allow(json, &role, names, project, all),
        ProfileCmd::Default { role, name } => default(json, &role, &name),
    }
}

fn list(json: bool, machine: Option<String>, project: Option<String>, names: bool) -> Result<ExitCode> {
    let res = crate::cli::front_call(FrontRequest::Profiles { machine })?;
    let Ok(FrontResponse::Profiles { machine, profiles }) = &res else {
        return crate::cli::front_print(json, res);
    };
    if names {
        for p in profiles {
            println!("{}", p.name);
        }
        return Ok(ExitCode::SUCCESS);
    }
    let Some(project) = project else { return crate::cli::front_print(json, res) };
    let target = Target::parse(&project)?;
    let eff = layers(&target).0.resolve();
    let default = default_thread_profile(&target);
    let coordinator = default_profile(&target, "coordinator_profile");
    if json {
        println!(
            "{}",
            serde_json::json!({
                "machine": machine,
                "profiles": profiles,
                "project": {
                    "name": project,
                    "thread_profile": default,
                    "thread_profiles": eff.thread_profiles,
                    "coordinator_profile": coordinator,
                },
            })
        );
        return Ok(ExitCode::SUCCESS);
    }
    let code = crate::cli::front_print(false, res.clone())?;
    let allowed = match &eff.thread_profiles {
        None => "모든 profile".to_string(),
        Some(l) if l.is_empty() => "없음".to_string(),
        Some(l) => l.join(", "),
    };
    println!(
        "\n`{project}`:\n  thread_profile        {default}\n  thread_profiles       {allowed}\n  coordinator_profile   {coordinator}"
    );
    Ok(code)
}

/// `profile resolve`: host 머신의 host.toml(없으면 기본 profile)이 띄우는 harness와 인자(참고 `resolve_json`, env는 뺀다)
fn resolve(name: Option<String>) -> Result<ExitCode> {
    let res = crate::cli::front_call(FrontRequest::ProfileResolve { name })?;
    match res {
        Ok(FrontResponse::ProfileResolved { profile }) => {
            println!("{}", serde_json::to_string(&profile)?);
            Ok(ExitCode::SUCCESS)
        }
        other => crate::cli::front_print(true, other),
    }
}

fn edit_profiles(json: bool, change: profile_edit::ProfileChange) -> Result<ExitCode> {
    let what = match &change {
        profile_edit::ProfileChange::Add { name, .. } => format!("`aaa profile add {name}`"),
        profile_edit::ProfileChange::Edit { name, .. } => format!("`aaa profile edit {name}`"),
        profile_edit::ProfileChange::Remove { name } => format!("`aaa profile remove {name}`"),
    };
    // 값이 틀렸으면 사람인지 묻기 전에 알린다
    let file = config_dir().join("host.toml");
    if !file.is_file() && !aaa_config::paths::state_dir().join("host/host.db").is_file() {
        bail!(
            "profile 바꾸기는 host 머신 터미널에서 한다(host.toml 없음). 확인은 `aaa profile list`·`aaa profile resolve`로 한다"
        );
    }
    profile_edit::apply_in(&std::fs::read_to_string(&file).unwrap_or_default(), &change)
        .map_err(|e| anyhow::anyhow!(e))?;
    crate::person::require(&format!("{what}(profile 바꾸기)"))?;
    let message = profile_edit::apply(&config_dir(), &change).map_err(|e| anyhow::anyhow!(e))?;
    let note = "host가 다음에 profile을 쓸 때 읽는다(다시 띄우지 않아도 된다). 이미 도는 thread는 다시 띄울 때까지 옛 설정이다";
    if json {
        println!("{}", serde_json::json!({ "message": message, "file": file.display().to_string() }));
    } else {
        println!("{message}\n{note}");
    }
    Ok(ExitCode::SUCCESS)
}

/// 허용 목록과 기본값은 host 정본에 있는 이름만 참조한다(기본 profile은 설치와 무관하게 존재한다).
fn check_names(names: &[String]) -> Result<Option<String>> {
    for name in names {
        profile_edit::validate_name(name).map_err(|e| anyhow::anyhow!(e))?;
        match crate::cli::front_call(FrontRequest::ProfileResolve { name: Some(name.clone()) })? {
            Ok(FrontResponse::ProfileResolved { .. }) => {}
            Ok(other) => bail!("예상하지 못한 응답: {other:?}"),
            Err(e) => bail!("{}", e.message),
        }
    }
    Ok(None)
}

fn allow(json: bool, role: &str, names: Vec<String>, project: Option<String>, all: bool) -> Result<ExitCode> {
    let (key, who) =
        if role == "coordinator" { ("coordinator_profiles", "coordinator") } else { ("thread_profiles", "thread") };
    let target = match &project {
        Some(p) => Target::parse(p)?,
        None => Target::Global,
    };
    let list = (!all).then_some(names);
    let warning = match &list {
        Some(l) => check_names(l)?,
        None => None,
    };
    crate::person::require(&format!("`aaa profile allow`({who} profile 허용 목록 바꾸기)"))?;
    let (path, text) = host_text();
    let edited =
        safety::set_profiles_in(&text, target.table(), key, list.as_deref()).map_err(|e| anyhow::anyhow!(e))?;
    profile_edit::write_atomic(&path, edited.as_bytes())
        .with_context(|| format!("{}에 쓰지 못했다", path.display()))?;
    let scope = match &target {
        Target::Global => "제 목록이 없는 모든 프로젝트".to_string(),
        Target::Project { name, .. } => format!("`{name}`"),
    };
    let message = match &list {
        None => format!("{who}는 {scope}에서 모든 profile을 쓸 수 있다"),
        Some(l) if l.is_empty() => format!("{who}는 {scope}에서 어떤 profile도 쓸 수 없다"),
        Some(l) => format!("{who}는 {scope}에서 {}를 쓸 수 있다", l.join(", ")),
    };
    let shadow = shadowed(target.table(), key);
    if json {
        println!("{}", serde_json::json!({ "message": message, "warning": warning.or(shadow) }));
    } else {
        println!("{message}");
        for w in warning.iter().chain(shadow.iter()) {
            println!("경고: {w}");
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn default(json: bool, role: &str, name: &str) -> Result<ExitCode> {
    let warning = check_names(&[name.to_string()])?;
    let (key, what, message) = if role == "coordinator" {
        (
            "coordinator_profile",
            "coordinator",
            format!(
                "`aaa coordinator open`에 --profile이 없고 project.toml coordinator_profile도 없으면 `{name}`을 쓴다"
            ),
        )
    } else {
        (
            "thread_profile",
            "thread",
            format!("thread start에 profile이 없고 project.toml thread_profile도 없으면 `{name}`을 쓴다"),
        )
    };
    crate::person::require(&format!("`aaa profile default`({what} 기본 profile 바꾸기)"))?;
    let (path, text) = host_text();
    let edited = safety::set_default_profile_in(&text, key, name).map_err(|e| anyhow::anyhow!(e))?;
    profile_edit::write_atomic(&path, edited.as_bytes())
        .with_context(|| format!("{}에 쓰지 못했다", path.display()))?;
    if json {
        println!("{}", serde_json::json!({ "message": message, "warning": warning }));
    } else {
        println!("{message}");
        if let Some(w) = warning {
            println!("경고: {w}");
        }
    }
    Ok(ExitCode::SUCCESS)
}

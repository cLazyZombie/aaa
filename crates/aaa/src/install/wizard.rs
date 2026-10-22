//! 두 prompt backend는 같은 선택 state/history와 공유 planner를 사용한다.
use super::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeSet, path::PathBuf};
type Result<T> = std::result::Result<T, Box<InstallResult>>;
#[derive(Clone)]
struct Snapshot {
    choices: InstallChoices,
    answered: BTreeSet<&'static str>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Draft {
    schema: String,
    choices: InstallChoices,
}
struct Question {
    key: &'static str,
    label: &'static str,
    kind: Kind,
    default: Value,
    source: &'static str,
}
enum Kind {
    Text,
    Optional,
    List,
    Harness,
    Number,
    Boolean,
    Storage,
    Choice(&'static [&'static str]),
}
enum Answer {
    Value(String),
    Back,
    Cancel,
    Save(PathBuf),
}
fn cancelled() -> InstallResult {
    *error("installation_cancelled", 130)
}
fn provided(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Array(v) => !v.is_empty(),
        Value::Bool(v) => *v,
        _ => true,
    }
}
fn ask(args: &Args, q: &Question) -> Result<Answer> {
    let default = match &q.default {
        Value::Null => String::new(),
        Value::String(v) => v.clone(),
        Value::Array(_) if matches!(q.kind, Kind::List) => q.default.to_string(),
        Value::Array(v) => v.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(","),
        v => v.to_string(),
    };
    let options =
        if let Kind::Choice(options) = q.kind { format!(" [{}]", options.join(" / ")) } else { String::new() };
    let label = format!(
        "{}{} (출처: {}, 기본값: {})",
        q.label,
        options,
        q.source,
        if default.is_empty() { "없음" } else { &default }
    );
    let answer = if args.plain() {
        super::prompt::line(&format!("{label}: "))?
    } else {
        let prompt = inquire::Text::new(&label)
            .with_default(&default)
            .with_help_message("Esc: 이전 · Ctrl+C: 중단 · :save <절대경로>: 초안 저장 후 종료");
        match prompt.prompt() {
            Ok(answer) => answer,
            Err(inquire::InquireError::OperationCanceled) => return Ok(Answer::Back),
            Err(inquire::InquireError::OperationInterrupted | inquire::InquireError::NotTTY) => {
                return Ok(Answer::Cancel);
            }
            Err(_) => return Err(error("installation_prompt_closed", 3)),
        }
    };
    Ok(match answer.as_str() {
        ":back" => Answer::Back,
        ":cancel" => Answer::Cancel,
        text if text.starts_with(":save ") => Answer::Save(PathBuf::from(&text[6..])),
        "" => Answer::Value(default),
        _ => Answer::Value(answer),
    })
}
fn save(path: &Path, choices: &InstallChoices, dry_run: bool) -> Result<()> {
    if dry_run {
        return Err(error("dry_run_draft_forbidden", 2));
    }
    use aaa_common::private_file as private;
    if !path.is_absolute()
        || path.components().any(|p| p == std::path::Component::ParentDir)
        || choices.harness_env.iter().any(|v| !matches!(v.key.as_str(), "LANG" | "LC_ALL" | "LC_CTYPE" | "TZ" | "TERM"))
    {
        return Err(error("installation_draft_invalid", 2));
    }
    let parent = engine::normalize_state(path.parent().ok_or_else(|| error("installation_draft_invalid", 2))?)?;
    let path = parent.join(path.file_name().ok_or_else(|| error("installation_draft_invalid", 2))?);
    private::parents(&parent).map_err(|_| error("installation_draft_unavailable", 3))?;
    let mut file = private::new_file(&path).map_err(|_| error("installation_draft_conflict", 4))?;
    serde_json::to_writer(&mut file, &Draft { schema: "aaa.install.draft.v1".into(), choices: choices.clone() })
        .map_err(|_| error("installation_draft_invalid", 2))?;
    file.flush().map_err(|_| error("installation_draft_unavailable", 3))?;
    file.sync_all().map_err(|_| error("installation_draft_unavailable", 3))?;
    drop(file);
    private::sync_parent(&path).map_err(|_| error("installation_draft_unavailable", 3))?;
    println!("초안을 사용자 전용 파일로 저장했습니다: {}", path.display());
    Ok(())
}
fn defaults(choices: &InstallChoices) -> Result<(InstallChoices, bool)> {
    let config = choices
        .config_dir
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| aaa_config::paths::default_config_dir_for(choices.instance.as_deref()));
    let host = aaa_config::load_host(&config).map_err(|_| error("invalid_existing_config", 2))?;
    let agent = aaa_config::load_agent(&config).map_err(|_| error("invalid_existing_config", 2))?;
    let existing = !host.files.is_empty() || !agent.files.is_empty();
    let mut values = InstallChoices::default();
    if !host.files.is_empty() {
        values.role = Some(InstallRole::Host);
        values.local_agent = Some(if host.config.local_agent.mode == aaa_config::LocalAgentMode::None {
            InstallToggle::Disabled
        } else {
            InstallToggle::Enabled
        });
        values.name = Some(host.config.local_agent.name);
        values.agent_listen = Some(host.config.host.agent_listen);
        values.web_listen = Some(host.config.web.listen.clone());
        values.web = Some(if host.config.web.listen.is_empty() { InstallWeb::Off } else { InstallWeb::Local });
        values.serve_port = host.config.web.publish_ports.first().copied();
    } else if !agent.files.is_empty() {
        values.role = Some(InstallRole::Agent);
        values.name = Some(agent.config.agent.name.clone());
    }
    if !agent.files.is_empty() {
        values.host = Some(agent.config.agent.host);
        values.work_root = Some(agent.config.agent.work_root);
        values.default_cwd = Some(agent.config.agent.default_cwd);
        if !agent.config.agent.work_mount.is_empty() {
            values.work_mount = Some(agent.config.agent.work_mount);
        }
        values.min_free_gb = u32::try_from(agent.config.health.min_free_gb).ok();
    }
    Ok((values, existing))
}
fn questions(choices: &InstallChoices, prior: &InstallChoices, existing: bool) -> Vec<Question> {
    let mut result = vec![];
    let current = serde_json::to_value(choices).expect("choices");
    let prior = serde_json::to_value(prior).expect("choices");
    let mut add = |key, label, kind, default: Value| {
        let (default, source) = if provided(&current[key]) {
            (current[key].clone(), "초안/앞선 선택")
        } else if provided(&prior[key]) {
            (prior[key].clone(), "기존 실효 설정")
        } else {
            (default, "새 설치 기본값/감지 후보")
        };
        result.push(Question { key, label, kind, default, source });
    };
    // instance가 먼저 정해져야 기존 config의 repair 기본값을 고를 수 있다.
    add("instance", "aaa 이름(기본 aaa는 빈 값)", Kind::Optional, Value::Null);
    add(
        "mode",
        "설치 방식",
        Kind::Choice(&["fresh", "repair", "reinstall"]),
        json!(if existing { "repair" } else { "fresh" }),
    );
    add("role", "이 머신의 역할", Kind::Choice(&["host", "agent"]), Value::Null);
    add(
        "name",
        "머신 표시 이름",
        Kind::Text,
        json!(aaa_common::util::machine_name_from(&aaa_common::util::hostname())),
    );
    if choices.role == Some(InstallRole::Host) {
        add("local_agent", "host에 로컬 agent도 설치", Kind::Choice(&["enabled", "disabled"]), json!("enabled"));
        add("project_mode", "프로젝트", Kind::Choice(&["create", "use", "skip"]), json!("create"));
        if choices.project_mode.is_some_and(|p| p != InstallProjectMode::Skip) {
            add("project", "프로젝트 이름", Kind::Text, Value::Null);
            if choices.repo.is_empty() {
                add("no_repo", "저장소 없이 시작(true/false)", Kind::Boolean, json!(false));
            }
            if !choices.no_repo {
                add("repo", "저장소 경로(JSON 배열, 경로의 공백·기호 보존)", Kind::List, json!([]));
            }
        }
    }
    let local = choices.role == Some(InstallRole::Agent)
        || choices.role == Some(InstallRole::Host) && choices.local_agent != Some(InstallToggle::Disabled);
    if local {
        let work = aaa_config::paths::home().join("aaa-work");
        let work = choices.instance.as_deref().map_or(work.clone(), |name| work.join(name));
        add("work_root", "작업 root 절대 경로", Kind::Text, json!(work.to_string_lossy()));
        add(
            "storage",
            "저장소(internal 또는 외장 mount 절대 경로)",
            Kind::Storage,
            choices
                .work_mount
                .as_ref()
                .map_or_else(|| prior["work_mount"].as_str().map_or(json!("internal"), |v| json!(v)), |v| json!(v)),
        );

        add("min_free_gb", "남겨 둘 여유 GB", Kind::Number, json!(20));
        add("harness", "사용할 harness(claude,codex 또는 none)", Kind::Harness, json!(["codex"]));
        add("login", "harness 로그인 검사", Kind::Choice(&["require", "defer"]), json!("require"));
        if choices.harness.contains(&InstallHarness::Codex) {
            add("hooks", "Codex aaa hook", Kind::Choice(&["install", "preserve", "skip"]), json!("install"));
        }
        if choices.role == Some(InstallRole::Host) && choices.harness.len() > 1 {
            add("thread_profile", "thread 기본 profile", Kind::Text, json!("codex"));
            add("coordinator_profile", "coordinator 기본 profile", Kind::Text, json!("claude"));
        }
    }
    add(
        "service",
        "서비스 관리자",
        Kind::Choice(&["auto", "windows-scm", "launchd", "systemd-system", "systemd-user", "none"]),
        json!("auto"),
    );
    if choices.service.is_some_and(|s| s != InstallService::None) {
        add("startup", "서비스 시작 시점", Kind::Choice(&["auto", "manual"]), json!("auto"));
        if cfg!(windows) {
            add(
                "service_password_source",
                "계정 암호 입력 경로(Hello PIN 불가)",
                Kind::Choice(&["prompt", "stdin", "broker"]),
                json!("prompt"),
            );
        }
    }
    add(
        "start",
        "지금 시작할까요",
        Kind::Choice(&["now", "deferred"]),
        json!(if choices.service == Some(InstallService::None) { "deferred" } else { "now" }),
    );
    add("path", "사용자 PATH", Kind::Choice(&["preserve", "user"]), json!("preserve"));
    if choices.role == Some(InstallRole::Agent) {
        add("host", "연결할 host:port", Kind::Text, Value::Null);
        add("network", "네트워크", Kind::Choice(&["tailscale", "direct"]), json!("tailscale"));
        add("enroll", "host 등록 방식", Kind::Choice(&["wait", "grant", "skip"]), json!("wait"));
    } else if choices.role == Some(InstallRole::Host) {
        add("agent_listen", "Agent link listen 주소", Kind::Text, json!("0.0.0.0:7430"));
        add("web", "웹", Kind::Choice(&["local", "off"]), json!("local"));
        if choices.web != Some(InstallWeb::Off) {
            add("web_listen", "로컬 웹 주소(loopback)", Kind::Text, json!("127.0.0.1:7431"));
            add("tailscale_serve", "Tailscale Serve HTTPS 공개(확인 필수)", Kind::Choice(&["on", "off"]), json!("on"));
            if choices.tailscale_serve == Some(InstallServe::On) {
                add("serve_port", "외부 HTTPS 포트(다른 포트로 자동 변경하지 않음)", Kind::Number, json!(443));
            }
            add("pairing", "첫 기기 Controller pairing", Kind::Choice(&["controller", "skip"]), json!("controller"));
        }
        if choices.web == Some(InstallWeb::Off) {
            add("tailscale_serve", "웹 off의 Serve", Kind::Choice(&["off"]), json!("off"));
            add("pairing", "웹 off의 pairing", Kind::Choice(&["skip"]), json!("skip"));
        }
    }
    result
}
fn set(choices: &mut InstallChoices, q: &Question, text: String, explicit: &Value) -> Result<()> {
    let before = choices.clone();
    let mut value = serde_json::to_value(&*choices).map_err(|_| error("invalid_choices", 2))?;
    let next = match q.kind {
        Kind::Choice(options) => {
            if !options.contains(&text.as_str()) {
                return Err(error("invalid_choice", 2));
            }
            json!(text)
        }
        Kind::Text => {
            if text.is_empty() {
                return Err(error("required_choice", 2));
            }
            json!(text)
        }
        Kind::Optional => {
            if text.is_empty() {
                Value::Null
            } else {
                json!(text)
            }
        }
        Kind::Number => json!(text.parse::<u32>().map_err(|_| error("invalid_number", 2))?),
        Kind::Boolean => json!(text.parse::<bool>().map_err(|_| error("invalid_choice", 2))?),
        Kind::List => {
            serde_json::from_str::<Vec<String>>(&text).map(|v| json!(v)).map_err(|_| error("invalid_path_list", 2))?
        }
        Kind::Harness => json!(text.split(',').map(str::trim).collect::<Vec<_>>()),
        Kind::Storage => {
            if text == "internal" {
                value["work_storage"] = json!("internal");
                value["work_mount"] = Value::Null;
            } else {
                value["work_mount"] = json!(text);
                value["work_storage"] = Value::Null;
            }
            *choices = serde_json::from_value(value).map_err(|_| error("invalid_choices", 2))?;
            return Ok(());
        }
    };
    value[q.key] = next;
    *choices = serde_json::from_value(value).map_err(|_| error("invalid_choices", 2))?;
    // 앞 질문을 바꾸면 초안의 종속 답도 무효화한다. 명시 argv는 숨겨서 덮지 않는다.
    let mut next = serde_json::to_value(&*choices).expect("choices");
    let empty = serde_json::to_value(InstallChoices::default()).expect("choices");
    let mut clear = |keys: &[&str]| {
        for key in keys {
            if !provided(&explicit[*key]) {
                next[*key] = empty[*key].clone();
            }
        }
    };
    match q.key {
        "role" if before.role != choices.role => {
            let keys: Vec<_> = empty
                .as_object()
                .expect("choices")
                .keys()
                .map(String::as_str)
                .filter(|k| {
                    ![
                        "role",
                        "mode",
                        "instance",
                        "config_dir",
                        "state_dir",
                        "bin_path",
                        "version",
                        "from",
                        "sha256",
                        "expected_os",
                        "expected_arch",
                    ]
                    .contains(k)
                })
                .collect();
            clear(&keys);
        }
        "project_mode" if choices.project_mode == Some(InstallProjectMode::Skip) => {
            clear(&["project", "repo", "no_repo", "goal", "projects_root"])
        }
        "local_agent" if choices.local_agent == Some(InstallToggle::Disabled) => clear(&[
            "work_root",
            "work_mount",
            "work_storage",
            "min_free_gb",
            "default_cwd",
            "shell",
            "harness",
            "harness_env",
            "claude_path",
            "codex_path",
            "claude_home",
            "codex_home",
            "login",
            "hooks",
        ]),
        "service" if choices.service == Some(InstallService::None) => {
            clear(&["service_account", "service_password_source"])
        }
        "harness" if !choices.harness.contains(&InstallHarness::Codex) => clear(&["codex_path", "codex_home", "hooks"]),
        "web" if choices.web == Some(InstallWeb::Off) => {
            clear(&["web_listen", "serve_port", "tailscale_serve", "pairing"])
        }
        _ => {}
    }
    *choices = serde_json::from_value(next).map_err(|_| error("invalid_choices", 2))?;
    Ok(())
}
pub(super) fn run(args: &Args, original: &InstallInvocation, reporter: &mut output::Reporter) -> Result<InstallResult> {
    let explicit = serde_json::to_value(&original.choices).map_err(|_| error("invalid_choices", 2))?;
    let mut choices = if let Some(path) = args.draft() {
        let draft: Draft = serde_json::from_slice(
            &aaa_common::private_file::read(path, 1024 * 1024).map_err(|_| error("installation_draft_untrusted", 2))?,
        )
        .map_err(|_| error("installation_draft_invalid", 2))?;
        if draft.schema != "aaa.install.draft.v1" {
            return Err(error("installation_draft_invalid", 2));
        }
        draft.choices
    } else {
        InstallChoices::default()
    };
    let mut merged = serde_json::to_value(&choices).map_err(|_| error("invalid_choices", 2))?;
    for (key, value) in explicit.as_object().expect("choices") {
        if provided(value) {
            merged[key] = value.clone();
        }
    }
    choices = serde_json::from_value(merged).map_err(|_| error("invalid_choices", 2))?;
    let mut state = Snapshot { choices, answered: BTreeSet::new() };
    let mut history: Vec<Snapshot> = vec![];
    let location = state
        .choices
        .state_dir
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| aaa_config::paths::default_state_dir_for(state.choices.instance.as_deref()));
    if !original.dry_run {
        review::require_outside(&location)?;
    }
    println!("설치 선택: :back 이전 · :cancel 중단 · :save <절대경로> 비밀 없는 초안 저장");
    loop {
        let (prior, existing) = defaults(&state.choices)?;
        let instance_chosen = provided(&explicit["instance"]) || state.answered.contains("instance");
        if !state.answered.contains("recovery")
            && instance_chosen
            && existing
            && !original.dry_run
            && !provided(&explicit["mode"])
        {
            state.answered.insert("recovery");
            let location = state
                .choices
                .state_dir
                .as_deref()
                .map(PathBuf::from)
                .unwrap_or_else(|| aaa_config::paths::default_state_dir_for(state.choices.instance.as_deref()));
            if let Some((id, plan, result)) =
                engine::previous_installation(&location, state.choices.instance.as_deref())?
                && !matches!(
                    result.status,
                    InstallStatus::Completed | InstallStatus::Configured | InstallStatus::Unchanged
                )
            {
                let same_executor = result.exit_code != 4 && engine::executor_matches(&plan)?;
                println!("이전 설치 ID: {id}. 설정·작업·hook·journal은 보존합니다.");
                if !same_executor {
                    println!("다른 판 또는 기록 검증 실패이므로 새 repair 계획으로 실제 파일을 다시 검토합니다.");
                }
                let q = Question {
                    key: "recovery",
                    label: "남은 설치 처리",
                    kind: if same_executor {
                        Kind::Choice(&["resume", "repair", "cancel"])
                    } else {
                        Kind::Choice(&["repair", "cancel"])
                    },
                    default: json!(if same_executor { "resume" } else { "repair" }),
                    source: "기존 서명 기록/실제 상태",
                };
                match ask(args, &q)? {
                    Answer::Value(value) if value == "resume" => {
                        let mut resume = original.clone();
                        resume.resume = Some(id);
                        resume.choices = InstallChoices {
                            instance: state.choices.instance.clone(),
                            config_dir: state.choices.config_dir.clone(),
                            state_dir: state.choices.state_dir.clone(),
                            ..Default::default()
                        };
                        return review::resume(args, &resume, reporter);
                    }
                    Answer::Value(value) if value == "repair" => {
                        state.choices.mode = Some(InstallMode::Repair);
                        state.answered.insert("mode");
                        println!("원래 ID {id}는 보존하고 새 계획·승인으로 변경분만 적용합니다.");
                    }
                    _ => return Ok(cancelled()),
                }
            }
        }
        let question = questions(&state.choices, &prior, existing).into_iter().find(|q| {
            !state.answered.contains(q.key)
                && !provided(&explicit[q.key])
                && !(q.key == "storage" && (provided(&explicit["work_mount"]) || provided(&explicit["work_storage"])))
        });
        if let Some(q) = question {
            match ask(args, &q)? {
                Answer::Cancel => return Ok(cancelled()),
                Answer::Save(path) => {
                    review::require_outside(&location)?;
                    save(&path, &state.choices, original.dry_run)?;
                    return Ok(cancelled());
                }
                Answer::Back => {
                    if let Some(previous) = history.pop() {
                        state = previous;
                    } else {
                        let confirm = Question {
                            key: "cancel",
                            label: "설치를 중단할까요",
                            kind: Kind::Choice(&["yes", "no"]),
                            default: json!("no"),
                            source: "중단 확인",
                        };
                        if matches!(ask(args,&confirm)?,Answer::Value(ref v) if v=="yes") {
                            return Ok(cancelled());
                        }
                    }
                }
                Answer::Value(text) => {
                    let previous = state.clone();
                    match set(&mut state.choices, &q, text, &explicit) {
                        Ok(()) => {
                            history.push(previous);
                            state.answered.insert(q.key);
                            if q.key == "instance" {
                                state.answered.remove("recovery");
                            }
                        }
                        Err(e) => {
                            println!("선택을 확인하세요: {}", e.error.as_ref().map_or("invalid_choice", |v| &v.code));
                        }
                    }
                }
            }
            continue;
        }
        let mut invocation = original.clone();
        invocation.choices = state.choices.clone();
        let planned = match detect::inspect(&invocation) {
            Ok(planned) => planned,
            Err(error) => {
                println!(
                    "현지 검사를 통과하지 못했습니다: {}",
                    error.error.as_ref().map_or("invalid_plan", |e| &e.code)
                );
                let q = Question {
                    key: "retry",
                    label: "선택을 바꿀까요(:back / :cancel)",
                    kind: Kind::Text,
                    default: json!(":back"),
                    source: "검사 결과",
                };
                match ask(args, &q)? {
                    Answer::Cancel => return Ok(cancelled()),
                    Answer::Save(path) => {
                        review::require_outside(&location)?;
                        save(&path, &state.choices, original.dry_run)?;
                        return Ok(cancelled());
                    }
                    _ => {
                        if let Some(previous) = history.pop() {
                            state = previous;
                        } else {
                            return Err(error);
                        }
                    }
                }
                continue;
            }
        };
        if original.dry_run {
            return Ok(planned.0);
        }
        println!("요약: 역할 {:?} · 계획 {}", planned.1.choices.role, aaa_config::install::plan_sha256(&planned.1));
        let q = Question {
            key: "review",
            label: "검토 후 승인할까요(:back으로 선택 수정)",
            kind: Kind::Choice(&["review", "cancel"]),
            default: json!("cancel"),
            source: "명시 동의 전",
        };
        match ask(args, &q)? {
            Answer::Value(v) if v == "review" => {
                if !planned.0.needs_user.is_empty() {
                    return Ok(planned.0);
                }
                let consent = review::confirm(args, &planned.1, None)?;
                return engine::apply_inline(&invocation, planned, &consent, reporter);
            }
            Answer::Back => {
                if let Some(previous) = history.pop() {
                    state = previous;
                }
            }
            Answer::Save(path) => {
                review::require_outside(&location)?;
                save(&path, &state.choices, original.dry_run)?;
                return Ok(cancelled());
            }
            _ => return Ok(cancelled()),
        }
    }
}

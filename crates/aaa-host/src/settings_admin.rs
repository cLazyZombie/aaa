//! 웹 설정 관리(관리 GUI W5c, plan §8.353): host.toml의 safety·profile·허용 목록·기본 profile 읽기·고치기와, 머신마다 작업
//! root·Codex hook을 그 머신 agent의 고정 작업으로 부르기.
//!
//! - 고치기는 CLI `aaa safety`·`aaa profile`과 같은 `aaa_config` 함수로 한다. 웹이 본 판(host.toml sha256)과 지금 판이 다르면
//!   쓰지 않고 conflict다. 쓰기는 `file_rev`로 파일마다 하나씩 한다.
//! - 웹은 임의 명령을 심지 못한다(사용자 답 1A·2A): custom profile은 만들거나 고치거나 지우지 않고, safety는 yolo·start_threads·
//!   trust_screens·coordinator_deploy만 바꾼다. host.toml 글과 env 값은 웹에 보내지 않는다(env는 이름만).
//! - 작업 root 전환은 절대 경로만 받고, agent가 다시 점검한 뒤 다음 시작 설정에 저장한다(답 3A). hook은 인자 없는 고정 작업이다(4A).
use std::path::{Path, PathBuf};
use std::time::Duration;

use aaa_config::{Profile, profile_edit, safety};
use aaa_proto::{
    AgentRequest, AgentResponse, ErrorBody, ErrorCode, FrontResponse, HookView, ProfileAdminRow, ProfileAdminView,
    ProfileChangeReq, ProfilePlanView, SafetyRowView, SafetyView, SettingsScope, WorkRootOp,
};

use crate::file_rev::{self, Edit};
use crate::orch::{bad, internal};
use crate::{Host, err};

/// 웹에서 바꾸는 safety key(답 2A). 나머지(`*_agent_args`·`routine_commands`)는 읽기만 한다
pub(crate) const WEB_SAFETY_KEYS: [&str; 4] = ["yolo", "start_threads", "trust_screens", "coordinator_deploy"];
/// 웹에서 만들고 고치는 profile harness(답 1A). custom은 실행 명령이 있어 host CLI로만 한다
const WEB_HARNESSES: [&str; 3] = ["claude", "codex", "shell"];
/// 작업 root 전환은 이전 root의 worktree 보호(git)까지 한다
const WORK_ROOT_SET_WAIT: Duration = Duration::from_secs(120);

fn forbidden(msg: impl Into<String>) -> ErrorBody {
    err(ErrorCode::Forbidden, msg)
}

fn conflict() -> ErrorBody {
    err(ErrorCode::Conflict, "host.toml이 그사이 바뀌었다: 다시 읽고 고친다")
}

/// 빈 host.toml은 `schema = 1`부터 쓴다(CLI `host_text`와 같다)
fn base(text: &str) -> String {
    if text.trim().is_empty() { format!("schema = {}\n", aaa_config::SCHEMA) } else { text.to_string() }
}

/// host.toml 글의 profile(값이 틀린 파일은 읽지 않는다)
fn profiles_in(text: &str) -> Result<std::collections::BTreeMap<String, Profile>, ErrorBody> {
    if text.trim().is_empty() {
        return Ok(Default::default());
    }
    toml_edit::de::from_str::<aaa_config::HostConfig>(text)
        .map(|c| c.profiles)
        .map_err(|e| bad(format!("host.toml을 읽지 못했다: {e}")))
}

fn row(name: &str, p: &Profile, builtin: bool) -> ProfileAdminRow {
    ProfileAdminRow {
        name: name.to_string(),
        harness: p.harness.clone(),
        model: p.model.clone(),
        effort: p.effort.clone(),
        description: p.description.clone(),
        args: p.args.clone(),
        env_names: p.env.keys().cloned().collect(),
        builtin,
        custom: p.harness == "custom",
        launch: profile_edit::launch_flags(p),
    }
}

/// 이 profile 정의(host.toml에 없으면 기본 profile)
fn profile_row(profiles: &std::collections::BTreeMap<String, Profile>, name: &str) -> Option<ProfileAdminRow> {
    match profiles.get(name) {
        Some(p) => Some(row(name, p, false)),
        None => profile_edit::builtin(name).map(|p| row(name, &p, true)),
    }
}

fn read_in(dir: &Path, file: &str) -> String {
    std::fs::read_to_string(dir.join(file)).unwrap_or_default()
}

/// host.local.toml이 같은 key를 덮거나, 프로젝트 표를 고칠 때 `[safety]` 표 하나에 같은 key가 있으면 알린다(CLI `shadowed`와 같다)
fn shadowed(dir: &Path, table: &str, key: &str) -> Option<String> {
    let local = read_in(dir, "host.local.toml");
    if safety::has_key(&local, table, key) {
        return Some(format!("host.local.toml의 [safety.{table}] {key}가 이 값을 덮는다: 그 파일에서도 고친다"));
    }
    if table == safety::DEFAULT_TABLE {
        return None;
    }
    ["host.toml", "host.local.toml"].into_iter().find(|f| safety::has_top_key(&read_in(dir, f), key)).map(|f| {
        format!(
            "{f}의 [safety] {key}가 이 값을 덮는다([safety] 표 하나가 프로젝트 표보다 앞선다): 그 줄을 지우거나 고친다"
        )
    })
}

/// host.local.toml이 `[<table>.<name>]`이나 `[<table>] <name>`을 정의하는가(profile·기본값이 local에 덮이는가)
fn local_defines(dir: &Path, table: &str, name: &str) -> bool {
    read_in(dir, "host.local.toml")
        .parse::<toml_edit::DocumentMut>()
        .is_ok_and(|d| d.get(table).and_then(|t| t.get(name)).is_some())
}

/// 바꾸기를 적용한 글과 알릴 것
struct Planned {
    text: String,
    message: String,
    warnings: Vec<String>,
    before: Option<ProfileAdminRow>,
    after: Option<ProfileAdminRow>,
    audit: String,
}

/// 작업 root 경로: 절대 경로만(`~`·상대 경로는 받지 않는다). Unix `/…`나 Windows `C:\…`·`C:/…`
fn absolute(path: &str) -> bool {
    let b = path.as_bytes();
    path.starts_with('/')
        || (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/'))
}

fn optional_absolute(what: &str, value: Option<String>) -> Result<Option<String>, ErrorBody> {
    match value.map(|v| v.trim().to_string()).filter(|v| !v.is_empty()) {
        Some(v) if !absolute(&v) => Err(bad(format!("{what}는 절대 경로여야 한다(`~`·상대 경로는 받지 않는다): {v}"))),
        other => Ok(other),
    }
}

impl Host {
    fn settings_dir(&self) -> Result<&PathBuf, ErrorBody> {
        self.config_dir.get().ok_or_else(|| err(ErrorCode::Unavailable, "host 설정 폴더를 모른다"))
    }

    fn host_toml_rev(&self, dir: &Path) -> Result<file_rev::Rev, ErrorBody> {
        file_rev::read(&dir.join("host.toml")).map_err(|e| internal(format!("host.toml: {e}")))
    }

    /// safety 표 이름(이 host 프로젝트 또는 `default`)
    fn safety_table(&self, project: &str, scope: SettingsScope) -> Result<String, ErrorBody> {
        match scope {
            SettingsScope::Global => Ok(safety::DEFAULT_TABLE.to_string()),
            SettingsScope::Project => {
                if project == safety::DEFAULT_TABLE {
                    return Err(bad(
                        "`default`는 모든 프로젝트 표의 이름이다: 모든 프로젝트 기본(scope global)을 쓴다",
                    ));
                }
                self.project_dir(project)?;
                Ok(project.to_string())
            }
        }
    }

    pub(crate) fn safety_show(&self, project: &str, scope: SettingsScope) -> Result<SafetyView, ErrorBody> {
        let dir = self.settings_dir()?;
        self.safety_table(project, scope)?;
        let (layers, warnings) = match scope {
            SettingsScope::Project => self.safety_layers(Some(project)),
            SettingsScope::Global => {
                let (default, _, legacy, warnings) = safety::read(dir, None);
                (safety::Layers { own: default, default: Default::default(), legacy }, warnings)
            }
        };
        let eff = layers.resolve();
        let rows = layers
            .rows(scope == SettingsScope::Project)
            .into_iter()
            .map(|r| SafetyRowView {
                key: r.key.to_string(),
                value: r.value,
                source: r.source.to_string(),
                note: r.note,
                editable: WEB_SAFETY_KEYS.contains(&r.key),
            })
            .collect();
        let (thread_profile, coordinator_profile) = match scope {
            SettingsScope::Project => (self.default_thread_profile(project), self.default_coordinator_profile(project)),
            SettingsScope::Global => {
                let get = |key| Some(safety::default_profile(dir, key)).filter(|v| !v.is_empty());
                (
                    get("thread_profile").unwrap_or_else(|| "claude".into()),
                    get("coordinator_profile").unwrap_or_else(|| "claude".into()),
                )
            }
        };
        Ok(SafetyView {
            scope,
            project: project.to_string(),
            rows,
            thread_profile,
            coordinator_profile,
            thread_profiles: eff.thread_profiles,
            coordinator_profiles: eff.coordinator_profiles,
            warnings,
            rev: self.host_toml_rev(dir)?.sha256,
            notes: Vec::new(),
        })
    }

    pub(crate) fn safety_set(
        &self,
        project: &str,
        scope: SettingsScope,
        key: &str,
        value: Option<String>,
        expected: &str,
        actor: &str,
    ) -> Result<SafetyView, ErrorBody> {
        if !WEB_SAFETY_KEYS.contains(&key) {
            return Err(forbidden(format!(
                "`{key}`는 웹에서 바꾸지 않는다(명령·인자 목록): host의 aaa 세션 밖 터미널에서 `aaa safety set`으로 바꾼다"
            )));
        }
        let dir = self.settings_dir()?;
        let table = self.safety_table(project, scope)?;
        let words = vec![value.clone().unwrap_or_else(|| "default".into())];
        let item = safety::value_item(key, &words).map_err(bad)?;
        let path = dir.join("host.toml");
        let lock = self.file_locks.of(&path);
        let _guard = lock.lock().unwrap();
        file_rev::save(&path, |rev| {
            if rev.sha256 != expected {
                return Err(conflict());
            }
            let text = safety::set_in(&base(&rev.text), &table, key, item.clone()).map_err(bad)?;
            Ok(if text == rev.text { Edit::Keep(()) } else { Edit::Write(text, ()) })
        })
        .map_err(save_error)?;
        self.db.audit(
            actor,
            "safety_set",
            &format!("[safety.{table}] {key} = {}", value.as_deref().unwrap_or("default")),
        );
        let mut view = self.safety_show(project, scope)?;
        view.notes.push(if key == "coordinator_deploy" {
            "다음 배포 요청부터 즉시 적용한다. thread는 계속 막는다. 사용자가 채팅에서 배포를 지시했을 때만 쓴다".into()
        } else {
            safety::RESTART_NOTE.into()
        });
        view.warnings.extend(shadowed(dir, &table, key));
        Ok(view)
    }

    pub(crate) fn profile_admin(&self) -> Result<ProfileAdminView, ErrorBody> {
        let dir = self.settings_dir()?;
        let cfg = self.profile_config()?;
        let mut profiles: Vec<ProfileAdminRow> = cfg.profiles.iter().map(|(n, p)| row(n, p, false)).collect();
        for name in profile_edit::BUILTINS.iter().filter(|n| !cfg.profiles.contains_key(**n)) {
            profiles.extend(profile_row(&cfg.profiles, name));
        }
        profiles.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(ProfileAdminView { profiles, rev: self.host_toml_rev(dir)?.sha256 })
    }

    /// 허용 목록·기본값에 넣는 이름: host.toml(local 포함) profile이나 기본 profile만
    fn known_profile(&self, name: &str) -> Result<(), ErrorBody> {
        profile_edit::validate_name(name).map_err(bad)?;
        let cfg = self.profile_config()?;
        if cfg.profiles.contains_key(name) || profile_edit::builtin(name).is_some() {
            Ok(())
        } else {
            Err(bad(format!("profile `{name}`가 없다: 먼저 더한다")))
        }
    }

    /// `text`(host.toml 지금 글)에 바꾸기를 적용한다. 미리보기와 적용이 같이 쓴다
    fn plan_profile(&self, project: &str, text: &str, change: &ProfileChangeReq) -> Result<Planned, ErrorBody> {
        let dir = self.settings_dir()?;
        let current = profiles_in(text)?;
        let custom = |name: &str| current.get(name).is_some_and(|p| p.harness == "custom");
        let web_harness = |h: &str| {
            if WEB_HARNESSES.contains(&h) {
                Ok(())
            } else {
                Err(forbidden(format!(
                    "웹에서는 claude·codex·shell profile만 만든다(`{h}`): custom profile은 host의 aaa 세션 밖 터미널에서 `aaa profile add`로 만든다"
                )))
            }
        };
        let no_custom = |name: &str| {
            if custom(name) {
                Err(forbidden(format!(
                    "`{name}`는 custom profile(실행 명령이 있다)이라 웹에서 고치거나 지우지 않는다: host의 aaa 세션 밖 터미널에서 `aaa profile`로 한다"
                )))
            } else {
                Ok(())
            }
        };
        let mut warnings = Vec::new();
        let (edited, message, name, audit) = match change {
            ProfileChangeReq::Add { name, harness, model, effort, description, args } => {
                web_harness(harness)?;
                let profile = Profile {
                    harness: harness.clone(),
                    model: model.clone(),
                    effort: effort.clone(),
                    description: description.clone(),
                    args: args.clone(),
                    ..Profile::default()
                };
                let (t, m) =
                    profile_edit::apply_in(text, &profile_edit::ProfileChange::Add { name: name.clone(), profile })
                        .map_err(bad)?;
                (t, m, Some(name.as_str()), format!("add {name}"))
            }
            ProfileChangeReq::Edit { name, harness, model, effort, description, args } => {
                no_custom(name)?;
                if let Some(h) = harness {
                    web_harness(h)?;
                }
                let (t, m) = profile_edit::apply_in(
                    text,
                    &profile_edit::ProfileChange::Edit {
                        name: name.clone(),
                        harness: harness.clone(),
                        model: model.clone(),
                        effort: effort.clone(),
                        description: description.clone(),
                        args: args.clone(),
                    },
                )
                .map_err(bad)?;
                (t, m, Some(name.as_str()), format!("edit {name}"))
            }
            ProfileChangeReq::Remove { name } => {
                no_custom(name)?;
                let (t, m) = profile_edit::apply_in(text, &profile_edit::ProfileChange::Remove { name: name.clone() })
                    .map_err(bad)?;
                (t, m, Some(name.as_str()), format!("remove {name}"))
            }
            ProfileChangeReq::Allow { scope, role, names } => {
                let (key, who) = match role.as_str() {
                    "thread" => ("thread_profiles", "thread"),
                    "coordinator" => ("coordinator_profiles", "coordinator"),
                    other => return Err(bad(format!("role은 thread·coordinator다: {other}"))),
                };
                let table = self.safety_table(project, *scope)?;
                for n in names.iter().flatten() {
                    self.known_profile(n)?;
                }
                let t = safety::set_profiles_in(&base(text), &table, key, names.as_deref()).map_err(bad)?;
                let where_ = match scope {
                    SettingsScope::Global => "제 목록이 없는 모든 프로젝트".to_string(),
                    SettingsScope::Project => format!("`{project}`"),
                };
                let m = match names {
                    None => format!("{who}는 {where_}에서 모든 profile을 쓸 수 있다"),
                    Some(l) if l.is_empty() => format!("{who}는 {where_}에서 어떤 profile도 쓸 수 없다"),
                    Some(l) => format!("{who}는 {where_}에서 {}를 쓸 수 있다", l.join(", ")),
                };
                warnings.extend(shadowed(dir, &table, key));
                let list = names.as_ref().map(|l| l.join(",")).unwrap_or_else(|| "*".into());
                (t, m, None, format!("allow [safety.{table}] {key} = {list}"))
            }
            ProfileChangeReq::Default { role, name } => {
                let key = match role.as_str() {
                    "thread" => "thread_profile",
                    "coordinator" => "coordinator_profile",
                    other => return Err(bad(format!("role은 thread·coordinator다: {other}"))),
                };
                self.known_profile(name)?;
                let t = safety::set_default_profile_in(&base(text), key, name).map_err(bad)?;
                if local_defines(dir, "defaults", key) {
                    warnings.push(format!("host.local.toml의 [defaults] {key}가 이 값을 덮는다: 그 파일에서도 고친다"));
                }
                let own = match key {
                    "thread_profile" => self.settings(project).thread_profile,
                    _ => self.settings(project).coordinator_profile,
                };
                if !own.is_empty() && own != *name {
                    warnings.push(format!("`{project}` 프로젝트 설정의 {key} `{own}`가 이 기본값보다 앞선다"));
                }
                (
                    t,
                    format!("{role} 기본 profile을 `{name}`로 한다(프로젝트 설정에 없을 때)"),
                    None,
                    format!("default {key} = {name}"),
                )
            }
        };
        let (before, after) = match name {
            Some(n) => {
                if local_defines(dir, "profiles", n) {
                    warnings
                        .push(format!("host.local.toml의 [profiles.{n}]가 이 profile을 덮는다: 그 파일에서도 고친다"));
                }
                (profile_row(&current, n), profile_row(&profiles_in(&edited)?, n))
            }
            None => (None, None),
        };
        Ok(Planned { text: edited, message, warnings, before, after, audit })
    }

    pub(crate) fn profile_plan(&self, project: &str, change: &ProfileChangeReq) -> Result<ProfilePlanView, ErrorBody> {
        let rev = self.host_toml_rev(self.settings_dir()?)?;
        let p = self.plan_profile(project, &rev.text, change)?;
        Ok(ProfilePlanView {
            message: p.message,
            before: p.before,
            after: p.after,
            warnings: p.warnings,
            rev: rev.sha256,
        })
    }

    pub(crate) fn profile_apply(
        &self,
        project: &str,
        change: &ProfileChangeReq,
        expected: &str,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        let path = self.settings_dir()?.join("host.toml");
        let lock = self.file_locks.of(&path);
        let saved = {
            let _guard = lock.lock().unwrap();
            file_rev::save(&path, |rev| {
                if rev.sha256 != expected {
                    return Err(conflict());
                }
                let p = self.plan_profile(project, &rev.text, change)?;
                let out = (p.message, p.warnings, p.audit);
                Ok(if p.text == rev.text { Edit::Keep(out) } else { Edit::Write(p.text, out) })
            })
            .map_err(save_error)?
        };
        let (message, warnings, audit) = saved.out;
        if saved.after.is_some() {
            self.db.audit(actor, "profile_apply", &audit);
        }
        Ok(FrontResponse::ProfileApplied { message, warnings, view: self.profile_admin()? })
    }

    /// 머신 이름 → 그 기능을 알린 agent 연결
    fn settings_agent(&self, machine: &str, feature: &str) -> Result<(String, crate::AgentConn), ErrorBody> {
        let mid = self.resolve_machine(Some(machine))?;
        if !self.machine_feature(&mid, feature) {
            return Err(err(
                ErrorCode::Unavailable,
                format!(
                    "{} agent는 이 일을 아직 모른다({feature} 없음): 그 머신 aaa를 갱신하고 agent를 다시 띄운다",
                    self.machine_name(&mid)
                ),
            ));
        }
        Ok((mid.clone(), self.live_conn(&mid)?))
    }

    pub(crate) async fn work_root(
        &self,
        machine: &str,
        action: WorkRootOp,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        let action = match action {
            WorkRootOp::Show => WorkRootOp::Show,
            WorkRootOp::Check { path, mount } => WorkRootOp::Check {
                path: optional_absolute("작업 root", path)?,
                mount: optional_absolute("mount", mount)?,
            },
            WorkRootOp::Set { path, mount } => WorkRootOp::Set {
                path: optional_absolute("작업 root", Some(path))?.ok_or_else(|| bad("작업 root 경로가 비었다"))?,
                mount: optional_absolute("mount", mount)?,
            },
        };
        let (mid, conn) = self.settings_agent(machine, "work_root.manage.v1")?;
        let audit = match &action {
            WorkRootOp::Show => None,
            WorkRootOp::Check { .. } => Some("work_root_check"),
            WorkRootOp::Set { .. } => Some("work_root_set"),
        };
        let detail = match &action {
            WorkRootOp::Set { path, .. } | WorkRootOp::Check { path: Some(path), .. } => path.clone(),
            _ => String::new(),
        };
        let wait = if matches!(action, WorkRootOp::Set { .. }) { WORK_ROOT_SET_WAIT } else { crate::REQUEST_TIMEOUT };
        let res = conn.request_timeout(AgentRequest::WorkRoot { action }, wait).await?;
        if let Some(a) = audit {
            self.db.audit(actor, a, &format!("{} {detail}", self.machine_name(&mid)));
        }
        match res {
            AgentResponse::WorkRoot(v) => Ok(FrontResponse::WorkRoot(v)),
            AgentResponse::WorkRootChecked(v) => Ok(FrontResponse::WorkRootChecked(v)),
            other => Err(internal(format!("예상하지 못한 응답: {other:?}"))),
        }
    }

    pub(crate) async fn codex_hook(
        &self,
        machine: &str,
        install: Option<bool>,
        actor: &str,
    ) -> Result<HookView, ErrorBody> {
        let (mid, conn) = self.settings_agent(machine, "hook.manage.v1")?;
        let res = conn.request(AgentRequest::CodexHook { install }).await?;
        if let Some(install) = install {
            self.db.audit(
                actor,
                if install { "hook_install" } else { "hook_uninstall" },
                &format!("{} codex", self.machine_name(&mid)),
            );
        }
        match res {
            AgentResponse::Hook(v) => Ok(v),
            other => Err(internal(format!("예상하지 못한 응답: {other:?}"))),
        }
    }
}

fn save_error(e: file_rev::SaveError<ErrorBody>) -> ErrorBody {
    match e {
        file_rev::SaveError::Edit(e) => e,
        file_rev::SaveError::Io(e) => internal(format!("host.toml: {e}")),
    }
}

#[cfg(test)]
mod tests;

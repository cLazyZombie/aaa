//! 프로젝트 폴더·설정·목록과 새 프로젝트 뼈대.

use super::*;

/// 프로젝트 폴더 뼈대. 이미 있는 파일은 두고, PROJECT.md를 마지막에 써서 중간에 멈춘 폴더는 프로젝트로 치지 않는다
/// (`project create`와 옮기기 `migrate::convert`가 함께 쓴다).
pub(crate) fn write_skeleton(dir: &Path, name: &str, project_md: &str) -> std::io::Result<()> {
    let skeleton = project_scaffold(name, project_md, false);
    for sub in skeleton.directories {
        std::fs::create_dir_all(dir.join(sub))?;
    }
    let put = |f: &str, body: &str| -> std::io::Result<()> {
        if !dir.join(f).exists() {
            write_atomic(&dir.join(f), body.as_bytes())?;
        }
        Ok(())
    };
    for (path, body) in skeleton.files {
        put(path, &body)?;
    }
    Ok(())
}

/// host 생성·옮기기와 install 파일 journal이 공유하는 실제 프로젝트 뼈대.
pub struct ProjectScaffold {
    pub directories: &'static [&'static str],
    pub files: Vec<(&'static str, String)>,
}

pub fn project_scaffold(name: &str, project_md: &str, include_schema: bool) -> ProjectScaffold {
    let mut files = vec![
        ("MEMORY.md", "# Memory\n".into()),
        ("TASKS.md", "# Tasks\n\n## Backlog\n".into()),
        ("project.toml", "schema = 1\n".into()),
        ("AGENTS.md", coordinator_guide(name)),
    ];
    if include_schema {
        files.push(("automations/automation.schema.json", crate::automation_schema()));
    }
    files.push(("PROJECT.md", project_md.into()));
    ProjectScaffold {
        directories: &["memory", "routines", "automations", "threads", "library", "inbox", "uploads"],
        files,
    }
}

/// 표지 없는 AGENTS.md를 옮겨 두는 이름 끝(herdr-projects `.before-herdr-projects`)
pub(super) const KEPT: &str = ".before-aaa";

pub(super) fn retired_pr_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    for (folder, suffix) in [("routines", ".md"), ("automations", ".toml")] {
        for entry in std::fs::read_dir(dir.join(folder)).into_iter().flatten().filter_map(Result::ok) {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.ends_with(suffix) {
                continue;
            }
            let relative = format!("{folder}/{name}");
            if crate::automation::project_text(dir, &relative).is_ok_and(|text| crate::routine::removed_pr_rule(&text))
            {
                files.push(dir.join(relative));
            }
        }
    }
    files.sort();
    files
}

/// 프로젝트 폴더의 aaa 파일에서 고칠 것(herdr-projects `priming_problems`). 빈 목록이면 맞다.
/// AGENTS.md는 표지가 있으면 지금 안내와 같아야 하고, 표지가 없으면(옛 aaa·사람이 쓴 것) 옆에 옮겨 둘 것이다.
pub(crate) fn files_problems(dir: &Path, name: &str) -> Vec<String> {
    let mut problems = Vec::new();
    match std::fs::read(dir.join("AGENTS.md")) {
        Err(source_error) => {
            aaa_common::diagnostic::observe_type(&source_error, "오류 뒤 기존 복구·거부 분기를 따른다");
            problems.push("AGENTS.md가 없다".to_string())
        }
        Ok(b) => match String::from_utf8(b) {
            Ok(t) if t == coordinator_guide(name) => {}
            Ok(t) if t.contains(GUIDE_MARK) => problems.push("AGENTS.md가 지금 aaa의 coordinator 안내가 아니다".into()),
            _ => problems
                .push(format!("AGENTS.md에 aaa 표지가 없다(옛 aaa나 사람이 쓴 것): AGENTS.md{KEPT}로 옮겨 둔다")),
        },
    }
    if !dir.join("uploads").is_dir() {
        problems.push("uploads/가 없다".into());
    }
    for path in retired_pr_files(dir) {
        problems.push(format!(
            "{}: 폐지된 자동화 원본을 .before-aaa로 보존한다",
            path.strip_prefix(dir).unwrap_or(&path).display()
        ));
    }
    problems
}

/// `files_problems`가 본 것을 고친다(herdr-projects `write_priming`, `doctor --fix`). 몇 번 불러도 같다.
/// 사람이 쓴 AGENTS.md는 덮지 않고 `<이름>.before-aaa`로 옮겨 둔다. 그 이름이 이미 있으면 옮기지 못하니 고치지
/// 않고 오류로 알린다(참고는 그때 덮는다: 사람 글을 잃지 않게 aaa는 멈춘다).
pub(crate) fn write_files(dir: &Path, name: &str) -> std::io::Result<()> {
    let agents = dir.join("AGENTS.md");
    let guide = coordinator_guide(name);
    match std::fs::read(&agents) {
        Ok(b) if b == guide.as_bytes() => {}
        Ok(b)
            if std::str::from_utf8(&b)
                .inspect_err(|error| aaa_common::diagnostic::observe(error, "성공 여부만 쓰는 분기의 하위 오류"))
                .is_ok_and(|t| t.contains(GUIDE_MARK)) =>
        {
            write_atomic(&agents, guide.as_bytes())?
        }
        Ok(_) => {
            keep_aside(&agents)?;
            write_atomic(&agents, guide.as_bytes())?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => write_atomic(&agents, guide.as_bytes())?,
        Err(e) => return Err(e),
    }
    std::fs::create_dir_all(dir.join("uploads"))?;
    for path in retired_pr_files(dir) {
        keep_aside(&path)?;
    }
    Ok(())
}

/// 사람이 쓴 파일을 `<이름>.before-aaa`로 옮긴다. 이미 있으면 그것도 사람 글이라 옮기지 않고 실패한다
pub(super) fn keep_aside(path: &Path) -> std::io::Result<()> {
    let kept = path.with_file_name(format!("{}{KEPT}", path.file_name().unwrap().to_string_lossy()));
    if std::fs::symlink_metadata(&kept)
        .as_ref()
        .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
        .is_ok()
    {
        return Err(std::io::Error::other(format!(
            "{}가 이미 있어 {}를 옮겨 두지 못한다: 둘을 직접 합치거나 하나를 치운 뒤 다시 한다",
            kept.display(),
            path.file_name().unwrap().to_string_lossy()
        )));
    }
    std::fs::rename(path, kept)
}

/// 임시 파일에 쓴 뒤 rename으로 바꾼다
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap();
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".{}.tmp-{}", path.file_name().unwrap().to_string_lossy(), aaa_common::util::new_id()));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

/// 전체 누락 목록은 ThreadRec.copy_notes에 보존한다. 흐름 사건에는 앞부분만 싣는다.
pub(super) fn copy_notes_summary(notes: &[String]) -> String {
    let text = notes.join("; ");
    if text.len() <= 8 * 1024 {
        return text;
    }
    let mut end = 8 * 1024;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{} … (총 {}건, 전체 목록은 thread copy_notes에 보존)", &text[..end], notes.len())
}

impl Host {
    pub(crate) fn projects_root(&self) -> PathBuf {
        if self.cfg.projects.root.is_empty() {
            aaa_config::paths::projects_root_default()
        } else {
            PathBuf::from(&self.cfg.projects.root)
        }
    }

    pub(crate) fn project_dir(&self, name: &str) -> Result<PathBuf, ErrorBody> {
        if !valid_project(name) {
            return Err(bad(format!("프로젝트 이름 형식이 아니다(영문 소문자·숫자·-·_): {name}")));
        }
        let dir = self.projects_root().join(name);
        if !dir.join("PROJECT.md").is_file() {
            return Err(err(ErrorCode::NotFound, format!("프로젝트가 없다: {name}(`aaa project create {name}`)")));
        }
        Ok(dir)
    }

    pub(crate) fn project_view(&self, name: &str, dir: &Path) -> ProjectView {
        let threads = self
            .db
            .threads(Some(name))
            .map(|v| v.len() as u32)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "thread 개수 관측 실패 뒤 기존 0 표시"))
            .unwrap_or(0);
        ProjectView {
            auto_mode: self.auto_mode(name).unwrap_or_else(|e| {
                tracing::error!("auto mode 상태 읽기 실패: {}", e.message);
                Default::default()
            }),
            name: name.to_string(),
            path: dir.to_string_lossy().to_string(),
            threads,
            status: self.project_status(name),
            coordinator_profile: self.default_coordinator_profile(name),
            coordinator_profiles: self.safety(name).coordinator_profiles,
        }
    }

    /// 프로젝트 폴더를 만든다(herdr-projects `new`). PROJECT.md를 마지막에 써서, 중간에 멈춘 폴더는 프로젝트로 치지 않는다.
    /// goal·저장소·profile은 준 것만 project.toml에 적는다(안 준 profile은 쓸 때 기본 사슬로 푼다, plan §8.180).
    pub(crate) fn project_create(
        &self,
        name: &str,
        opts: ProjectOptions,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        if !valid_project(name) {
            return Err(bad(format!("프로젝트 이름 형식이 아니다(영문 소문자·숫자·-·_): {name}")));
        }
        if name == aaa_config::safety::DEFAULT_TABLE {
            // host.toml `[safety.default]`는 모든 프로젝트 표다(plan §8.163)
            return Err(bad("`default`는 safety의 모든 프로젝트 표 이름이라 프로젝트 이름으로 쓰지 않는다"));
        }
        let dir = self.projects_root().join(name);
        if dir.join("PROJECT.md").exists() {
            return Err(err(ErrorCode::NameTaken, format!("이미 있다: {}", dir.display())));
        }
        // 한 host는 프로젝트 하나(plan §8.283·§8.285): 보관하지 않은 프로젝트가 있으면 둘째를 만들지 않는다
        match self.sole_project() {
            Ok(None) => {}
            Ok(Some(have)) => {
                return Err(bad(format!(
                    "host는 프로젝트 하나만 맡는다: 이미 `{have}`가 있다. 명령은 프로젝트 인자 없이 쓴다"
                )));
            }
            Err(names) => return Err(bad(crate::lifecycle::sole_project_refusal(&names))),
        }
        // 값은 모두 먼저 맞춰 본다: 틀린 옵션으로 반쯤 만든 폴더를 남기지 않는다
        let mut toml = "schema = 1\n".to_string();
        let mut set = |key: &str, value: &str| -> Result<(), ErrorBody> {
            toml = crate::project::set_in(&toml, key, value).map_err(bad)?;
            Ok(())
        };
        if !opts.goal.trim().is_empty() {
            set("goal", &opts.goal)?;
        }
        if let Some(p) = &opts.thread_profile {
            set("thread_profile", p)?;
        }
        if let Some(p) = &opts.coordinator_profile {
            set("coordinator_profile", p)?;
        }
        for r in &opts.repos {
            set("repos.add", &self.repo_arg(r))?;
        }
        if let Some(p) = &opts.thread_profile {
            // 새 프로젝트의 thread 허용 목록(host.toml `[safety.default]`·옛 자리)에 있어야 한다(참고 `check_allowed`)
            self.check_profile_allowed(name, p)?;
        }
        write_atomic(&dir.join("project.toml"), toml.as_bytes()).map_err(internal)?;
        write_skeleton(&dir, name, &format!("# {name}\n\n지침을 여기에 쓴다.\n")).map_err(internal)?;
        self.db.audit(actor, "project_create", name);
        Ok(FrontResponse::Project(self.project_view(name, &dir)))
    }

    /// project.toml 값 하나를 바꾼다(herdr-projects `set`). thread_profile은 그 프로젝트 thread 허용 목록에 있어야 한다.
    pub(crate) fn project_set(
        &self,
        project: &str,
        key: &str,
        value: &str,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        let dir = self.project_dir(project)?;
        let value = if key.starts_with("repos.") { self.repo_arg(value) } else { value.trim().to_string() };
        if key == "thread_profile" {
            self.check_profile_allowed(project, &value)?;
        }
        // 있는 쪽 파일에 쓴다(aaa.toml, 없으면 project.toml. plan §8.288)
        if key == "max_parallel_threads"
            && value
                .parse::<u32>()
                .inspect_err(|error| aaa_common::diagnostic::observe(error, "성공 여부만 쓰는 분기의 하위 오류"))
                .is_ok_and(|n| n > self.settings(project).max_parallel_threads)
            && self.coordinator_project_of_actor(actor).as_deref() == Some(project)
        {
            return Err(err(
                ErrorCode::Forbidden,
                "coordinator는 실행 한도를 올려 우회하지 않는다. 사용자가 직접 설정한다",
            ));
        }
        let path = aaa_config::project_settings_file(&dir);
        let _guard = PROJECT_TOML.lock().unwrap();
        let text = std::fs::read_to_string(&path).unwrap_or_else(|_| "schema = 1\n".into());
        let edited =
            crate::project::set_in_table(&text, key, &value, crate::project::repos_table(&path)).map_err(bad)?;
        write_atomic(&path, edited.as_bytes()).map_err(internal)?;
        self.db.audit(actor, "project_set", &format!("{project} {key}={value}"));
        Ok(FrontResponse::Project(self.project_view(project, &dir)))
    }

    /// 「이 aaa」·「저장소」 설정 화면이 읽는 값(관리 GUI W4). 잘못된 값은 host가 쓰는 기본값으로 보인다
    pub(crate) fn project_settings_view(&self, project: &str) -> Result<aaa_proto::ProjectSettingsView, ErrorBody> {
        let dir = self.project_dir(project)?;
        let path = aaa_config::project_settings_file(&dir);
        let s = crate::project::ProjectSettings::load(&dir);
        Ok(aaa_proto::ProjectSettingsView {
            project: project.to_string(),
            file: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            status: self.project_status(project),
            goal: s.goal,
            thread_profile: s.thread_profile,
            coordinator_profile: s.coordinator_profile,
            max_parallel_threads: s.max_parallel_threads,
            auto_resolve_days: s.auto_resolve_days,
            nudge: s.nudge,
            mute: s.mute,
            repos: s.repos.into_iter().map(|r| aaa_proto::RepoChoice { path: r.path, machine: r.machine }).collect(),
            local_machine: self.machine_order().1,
        })
    }

    /// 저장소 하나를 제자리에서 바꾼다(웹 「저장소」 페이지의 고치기). 머신 없는 경로는 `repos.add`처럼 host 정본 경로로 바꾼다
    pub(crate) fn project_repo_edit(
        &self,
        project: &str,
        from: &str,
        to: &str,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        let dir = self.project_dir(project)?;
        let to = self.repo_arg(to);
        let path = aaa_config::project_settings_file(&dir);
        {
            let _guard = PROJECT_TOML.lock().unwrap();
            let text = std::fs::read_to_string(&path).unwrap_or_else(|_| "schema = 1\n".into());
            let edited =
                crate::project::replace_repo_in(&text, from, &to, crate::project::repos_table(&path)).map_err(bad)?;
            write_atomic(&path, edited.as_bytes()).map_err(internal)?;
        }
        self.db.audit(actor, "project_set", &format!("{project} repos.edit={} -> {to}", from.trim()));
        self.project_settings_view(project).map(FrontResponse::ProjectSettings)
    }

    /// 머신이 없는 저장소 경로는 host 머신 정본 경로로(참고 `create`·`set_in`: 없는 경로는 그대로)
    pub(super) fn repo_arg(&self, arg: &str) -> String {
        let r = crate::project::parse_repo_arg(arg.trim());
        match r.machine {
            Some(_) => arg.trim().to_string(),
            None => std::fs::canonicalize(&r.path).map(|p| p.to_string_lossy().into_owned()).unwrap_or(r.path),
        }
    }

    /// 보관하지 않은 프로젝트(herdr-projects `list`)
    pub(crate) fn projects(&self) -> Vec<ProjectView> {
        self.projects_all().into_iter().filter(|p| p.status != aaa_proto::ProjectStatus::Archived).collect()
    }

    /// 지금 도는 프로젝트(ticker가 보는 것): 멈추거나 보관한 것은 뺀다
    pub(crate) fn active_projects(&self) -> Vec<ProjectView> {
        self.projects_all().into_iter().filter(|p| p.status == aaa_proto::ProjectStatus::Active).collect()
    }

    pub(crate) fn projects_all(&self) -> Vec<ProjectView> {
        self.project_directories().into_iter().map(|(name, dir)| self.project_view(&name, &dir)).collect()
    }

    pub(crate) fn project_directories(&self) -> Vec<(String, PathBuf)> {
        let root = self.projects_root();
        let mut out: Vec<_> = std::fs::read_dir(&root)
            .into_iter()
            .flatten()
            .filter_map(|e| {
                e.inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                })
                .ok()
            })
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                (valid_project(&name) && e.path().join("PROJECT.md").is_file()).then(|| (name, e.path()))
            })
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }
}

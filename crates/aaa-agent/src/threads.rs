//! thread 자리(worktree·작업 폴더)와 보고 회수(§8.7, §8.12). git은 이 머신의 agent만 실행한다.
//!
//! - 작업 폴더는 `<cwd>/.aaa/<project>-<thread>/`이고 brief.md·report.md·library/를 둔다. repo면 `.aaa/`를 git exclude에 넣는다.
//! - report.md는 regular file만 본다(symlink는 무시). library는 symlink를 따라가지 않고 상한을 넘는 파일은 건너뛴다.
//! - 정리는 force 없이 한다: 고치지 않은 worktree만 지워지고, 기준에 병합된 branch만 지운다.

use aaa_common::child_process::TokioCommandExt as _;
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use aaa_config::Profile;
use aaa_proto::{
    Cleanup, Launch, LaunchSafety, LibFile, ProfileView, ReportSnapshot, Skipped, ThreadAdopt, ThreadAdopted,
    ThreadKind, ThreadPrepare, ThreadReady, fill_brief, slug, valid_project, valid_thread,
};
use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

/// report.md 본문 상한(이보다 크면 회수하지 않는다)
pub const MAX_REPORT_BYTES: u64 = 4 << 20;

#[path = "threads/preparation.rs"]
mod preparation;

#[derive(Debug, Clone, Default)]
pub struct ThreadsConfig {
    pub work_root: PathBuf,
    pub work_mount: Option<PathBuf>,
    /// 실제 agent가 configure_threads에서 붙이는 내구 기록 위치.
    pub state_dir: Option<PathBuf>,
    /// hook 명령에 쓰는 aaa 실행 파일
    pub exe: PathBuf,
    pub profiles: BTreeMap<String, Profile>,
    pub harnesses: BTreeMap<String, aaa_config::HarnessOverride>,
    /// 머신별 harness 덮어쓰기·옛 host 호환 profile을 쓸 때마다 다시 읽는다.
    pub config_dir: Option<PathBuf>,
}

impl ThreadsConfig {
    pub(crate) fn known_roots(&self) -> Result<Vec<crate::work_root::RootInfo>> {
        let mut roots = self
            .state_dir
            .as_deref()
            .map(crate::work_root::inspect)
            .transpose()?
            .flatten()
            .map(|s| s.roots)
            .unwrap_or_default();
        if self.work_root.is_absolute() && !roots.iter().any(|r| r.path == self.work_root) {
            roots.push(crate::work_root::RootInfo {
                path: self.work_root.clone(),
                real_path: crate::work_root::real_location(&self.work_root)?,
                mount: self.work_mount.clone(),
                imported: false,
            });
        }
        Ok(roots)
    }
    pub(crate) fn root_for_path(&self, path: &Path) -> Result<Option<crate::work_root::RootInfo>> {
        let roots = self.known_roots()?;
        if let Some(root) =
            roots.iter().filter(|r| path.starts_with(&r.path)).max_by_key(|r| r.path.components().count())
        {
            return Ok(Some(root.clone()));
        }
        let real = crate::work_root::real_location(path)?;
        Ok(roots
            .into_iter()
            .filter(|r| real.starts_with(&r.real_path))
            .max_by_key(|r| r.real_path.components().count()))
    }
    pub(crate) fn check_path(&self, path: &Path) -> Result<()> {
        match self.root_for_path(path)? {
            Some(root) => root.check(path),
            None => crate::work_root::check(path, None).map(|_| ()),
        }
    }
    pub(crate) fn lease(&self) -> Result<Option<crate::work_root::Lease>> {
        self.state_dir.as_deref().map(crate::work_root::shared).transpose()
    }
    pub(crate) fn lock_reason(&self, cwd: &Path) -> Result<Option<String>> {
        if !self.root_for_path(cwd)?.is_some_and(|r| r.portable()) {
            return Ok(None);
        }
        let real = crate::work_root::real_location(cwd)?;
        Ok(self
            .state_dir
            .as_deref()
            .map(crate::work_root::inspect)
            .transpose()?
            .flatten()
            .map(|s| format!("aaa:{}:{}", s.owner, hex::encode(Sha256::digest(real.as_os_str().as_encoded_bytes())))))
    }
    /// 지금 agent.toml의 머신별 덮어쓰기·호환 profile로 바꾼 설정. 읽지 못하면 그 까닭이 오류다(옛 값으로 띄우지 않는다).
    pub fn fresh(&self) -> Result<ThreadsConfig> {
        let Some(dir) = &self.config_dir else { return Ok(self.clone()) };
        let loaded = aaa_config::load_agent(dir)
            .map_err(|e| anyhow::anyhow!("이 머신 agent.toml을 읽지 못했다:\n{}", e.render()))?;
        Ok(ThreadsConfig { profiles: loaded.config.profiles, harnesses: loaded.config.harnesses, ..self.clone() })
    }
}

pub(crate) fn import_legacy(state: &Path, root: &crate::work_root::RootInfo) -> Result<usize> {
    preparation::import_legacy(state, root)
}

fn view(name: &str, p: &Profile, builtin: bool) -> ProfileView {
    ProfileView {
        name: name.to_string(),
        harness: p.harness.clone(),
        model: Some(p.model.clone()).filter(|m| !m.is_empty()),
        effort: Some(p.effort.clone()).filter(|m| !m.is_empty()),
        description: p.description.clone(),
        args: p.args.clone(),
        builtin,
    }
}

/// agent.toml profile과, 같은 이름이 없는 기본 profile 중 이 머신에 설치·로그인한 것(참고 `Config::listed`)
pub fn profile_views(cfg: &ThreadsConfig) -> Vec<ProfileView> {
    let mut out: Vec<ProfileView> = cfg.profiles.iter().map(|(name, p)| view(name, p, false)).collect();
    out.extend(builtin_views(cfg).into_iter().filter(|p| !cfg.profiles.contains_key(&p.name)));
    out
}

pub fn builtin_views(cfg: &ThreadsConfig) -> Vec<ProfileView> {
    // 시험은 이 머신의 설치·로그인과 상관없이 정한다(`AAA_TEST_BUILTINS`: 빈 값은 없음, 쉼표 목록은 그것만)
    let detected: Vec<String> = match std::env::var("AAA_TEST_BUILTINS") {
        Ok(v) => v.split(',').map(str::trim).filter(|n| !n.is_empty()).map(str::to_string).collect(),
        Err(source_error) => {
            aaa_common::diagnostic::observe(&source_error, "오류 뒤 기존 복구·거부 분기를 따른다");
            let home = aaa_config::paths::home();
            aaa_config::profile_edit::detect(
                std::env::var("PATH")
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
                    .as_deref(),
                &home,
                |n| {
                    std::env::var(n)
                        .inspect_err(|error| {
                            aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                        })
                        .ok()
                },
                &cfg.harnesses,
            )
        }
    };
    detected.iter().filter_map(|name| aaa_config::profile_edit::builtin(name).map(|p| view(name, &p, true))).collect()
}

/// 있는 작업 폴더에 brief.md를 다시 쓰고, 새 대화로 띄울 launch를 만든다(claude는 새 session ID).
/// brief의 작업 폴더·aaa 실행 파일 자리는 이 머신 값으로 채운다(`fill_brief`).
pub fn rebrief(
    cfg: &ThreadsConfig,
    work_dir: &str,
    brief: &str,
    mut launch: Launch,
    profile: Option<&str>,
    safety: Option<&LaunchSafety>,
    resolved: Option<&aaa_proto::ResolvedProfile>,
) -> Result<Launch> {
    cfg.check_path(Path::new(work_dir))?;
    let dir = check_work_dir(work_dir)?;
    std::fs::write(dir.join("brief.md"), fill_brief(brief, &dir.to_string_lossy(), &cfg.exe.to_string_lossy()))?;
    if let Some(name) = profile {
        // profile이 모든 설정을 가진다: 옛 model·args는 쓰지 않는다(herdr-projects threads.rs `restart`)
        return match resolved {
            Some(p) => {
                let p = local_profile(cfg, p)?;
                with_hooks(cfg, resolve(name, &p)?, &dir, p.command.len().max(1))
            }
            None => launch_in(cfg, name, &dir, safety),
        };
    }
    if let Some(i) = launch.argv.iter().position(|a| a == "--session-id")
        && i + 1 < launch.argv.len()
    {
        let key = uuid_v4();
        launch.argv[i + 1] = key.clone();
        launch.session_key = Some(key);
    }
    Ok(launch)
}

fn uuid_v4() -> String {
    let mut b: [u8; 16] = rand::random();
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h = hex::encode(b);
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

/// profile → 실행 argv. resume argv는 같은 model·effort·args·env를 쓰고 `{session}` 자리에 harness 세션 ID가 들어간다.
pub fn resolve(name: &str, p: &Profile) -> Result<Launch> {
    let base = |default: &str| if p.command.is_empty() { vec![default.to_string()] } else { p.command.clone() };
    let env: Vec<(String, String)> = p.env.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    for (key, value) in &env {
        aaa_common::redact::remember_env(key, value);
    }
    let (argv, resume_argv, session_key) = match p.harness.as_str() {
        "claude" => {
            let flags = aaa_config::profile_edit::launch_flags(p);
            let key = uuid_v4();
            let mut argv = base("claude");
            argv.extend(flags.iter().cloned());
            argv.extend(["--session-id".to_string(), key.clone()]);
            let mut resume = base("claude");
            resume.extend(flags);
            resume.extend(["--resume".to_string(), "{session}".to_string()]);
            (argv, resume, Some(key))
        }
        "codex" => {
            let flags = aaa_config::profile_edit::launch_flags(p);
            let mut argv = base("codex");
            argv.extend(flags.iter().cloned());
            let mut resume = base("codex");
            resume.extend(["resume".to_string(), "{session}".to_string()]);
            resume.extend(flags);
            (argv, resume, None)
        }
        "shell" | "custom" => {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
            let mut argv = base(&shell);
            argv.extend(p.args.iter().cloned());
            (argv.clone(), argv, None)
        }
        other => bail!("profile {name}: 모르는 harness {other:?}"),
    };
    Ok(Launch { profile: name.to_string(), harness: p.harness.clone(), argv, env, resume_argv, session_key })
}

pub(crate) async fn git(dir: &Path, args: &[&str], timeout: Duration) -> Result<String> {
    let out = tokio::time::timeout(
        timeout,
        tokio::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true)
            .output_guarded(),
    )
    .await
    .with_context(|| format!("git {} 시간 초과", args.join(" ")))?
    .context("git 실행")?;
    if !out.status.success() {
        bail!("git {} 실패: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

const GIT_T: Duration = Duration::from_secs(30);

/// repo의 `info/exclude`에 `.aaa/`를 한 번만 넣는다(작업 폴더가 git 상태에 보이지 않게)
async fn exclude_work_dirs(repo: &Path) -> Result<()> {
    let common = git(repo, &["rev-parse", "--git-common-dir"], GIT_T).await?;
    let common = if Path::new(&common).is_absolute() { PathBuf::from(common) } else { repo.join(common) };
    let path = common.join("info").join("exclude");
    let text = std::fs::read_to_string(&path)
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 파일을 읽지 못해 기본값을 쓴다"))
        .unwrap_or_default();
    if !text.lines().any(|l| l.trim() == ".aaa/") {
        std::fs::create_dir_all(path.parent().unwrap())?;
        let sep = if text.is_empty() || text.ends_with('\n') { "" } else { "\n" };
        std::fs::write(&path, format!("{text}{sep}.aaa/\n"))?;
    }
    Ok(())
}

pub async fn prepare(cfg: &ThreadsConfig, p: ThreadPrepare, wait: Duration) -> Result<ThreadReady> {
    let thread = p.thread.clone();
    preparation::prepare(cfg, p, wait).await.with_context(|| format!("thread {thread} 준비"))
}

async fn prepare_inner(cfg: &ThreadsConfig, p: ThreadPrepare, wait: Duration) -> Result<ThreadReady> {
    if !valid_project(&p.project) || !valid_thread(&p.thread) {
        bail!("프로젝트·thread 이름 형식이 아니다: {} {}", p.project, p.thread);
    }
    let profile = match &p.resolved_profile {
        Some(profile) => local_profile(cfg, profile)?,
        None => with_safety(cfg, find_profile(cfg, &p.profile)?, p.safety.as_ref()),
    };
    let launch = resolve(&p.profile, &profile)?;
    let (cwd, branch, base) = preparation::place(cfg, &p, wait).await.context("thread 자리 준비")?;
    let work_dir = cwd.join(".aaa").join(format!("{}-{}", p.project, p.thread));
    cfg.check_path(&work_dir)?;
    std::fs::create_dir_all(work_dir.join("library"))
        .with_context(|| format!("thread 자료 폴더 생성: {}", work_dir.join("library").display()))?;
    std::fs::write(
        work_dir.join("brief.md"),
        fill_brief(&p.brief, &work_dir.to_string_lossy(), &cfg.exe.to_string_lossy()),
    )
    .with_context(|| format!("thread brief 쓰기: {}", work_dir.join("brief.md").display()))?;
    let launch = with_hooks(cfg, launch, &work_dir, profile.command.len().max(1)).context("thread hook 준비")?;
    Ok(ThreadReady {
        cwd: cwd.to_string_lossy().to_string(),
        work_dir: work_dir.to_string_lossy().to_string(),
        branch,
        base,
        launch,
    })
}

/// 사람이 띄운 agent 세션을 thread로 데려온다(herdr-projects `adopt`): 세션이 있는 폴더 `cwd`에 작업 폴더를 만들고
/// brief.md를 쓴다. git repo 안이면 `.aaa/`를 exclude에 넣고 repo top과 지금 branch를 알린다. 같은 폴더의 두 thread도
/// 작업 폴더는 따로다.
pub async fn adopt(exe: &Path, p: ThreadAdopt, cwd: &Path) -> Result<ThreadAdopted> {
    if !valid_project(&p.project) || !valid_thread(&p.thread) {
        bail!("프로젝트·thread 이름 형식이 아니다: {} {}", p.project, p.thread);
    }
    if !cwd.is_absolute() || !cwd.is_dir() {
        bail!("세션 폴더가 없다: {}", cwd.display());
    }
    let work_dir = cwd.join(".aaa").join(format!("{}-{}", p.project, p.thread));
    if work_dir.exists() {
        bail!("작업 폴더가 이미 있다: {}", work_dir.display());
    }
    let repo = git(cwd, &["rev-parse", "--show-toplevel"], GIT_T)
        .await
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
        .ok()
        .filter(|s| !s.is_empty());
    let branch = match &repo {
        Some(_) => git(cwd, &["rev-parse", "--abbrev-ref", "HEAD"], GIT_T)
            .await
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()
            .filter(|b| !b.is_empty() && b != "HEAD"),
        None => None,
    };
    if let Some(top) = &repo {
        exclude_work_dirs(Path::new(top)).await?;
    }
    std::fs::create_dir_all(work_dir.join("library"))?;
    std::fs::write(
        work_dir.join("brief.md"),
        fill_brief(&p.brief, &work_dir.to_string_lossy(), &exe.to_string_lossy()),
    )?;
    Ok(ThreadAdopted {
        cwd: cwd.to_string_lossy().to_string(),
        work_dir: work_dir.to_string_lossy().to_string(),
        repo,
        branch,
    })
}

/// 재개 argv를 바꾸지 않고 현재 인증 env만 복원할 때도 같은 덮어쓰기 순서를 쓴다.
pub(crate) fn local_env(cfg: &ThreadsConfig, spec: &aaa_proto::ResolvedProfile) -> BTreeMap<String, String> {
    let mut env = spec.env.clone();
    if let Some(local) = cfg.harnesses.get(&spec.harness) {
        env.extend(local.env.clone());
    }
    env
}

/// host 정의에 머신 실행 경로·환경만 덮어쓴다. 없는 harness로 조용히 바꾸지 않는다.
pub(crate) fn local_profile(cfg: &ThreadsConfig, spec: &aaa_proto::ResolvedProfile) -> Result<Profile> {
    let mut p = Profile {
        harness: spec.harness.clone(),
        model: spec.model.clone(),
        effort: spec.effort.clone(),
        args: spec.args.clone(),
        command: spec.command.clone(),
        env: local_env(cfg, spec),
        description: String::new(),
    };
    if let Some(local) = cfg.harnesses.get(&p.harness)
        && !local.command.is_empty()
    {
        p.command = local.command.clone();
    }
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
    let exe =
        p.command.first().map(String::as_str).unwrap_or_else(|| if p.harness == "shell" { &shell } else { &p.harness });
    if !aaa_config::profile_edit::executable_available(exe, p.env.get("PATH").map(String::as_str)) {
        bail!(
            "harness `{}`를 실행할 수 없다: 실행 파일 `{exe}`가 없거나 실행 권한이 없다. 이 머신 agent.toml [harnesses.{}] command 또는 PATH를 확인한다",
            p.harness,
            p.harness
        );
    }
    Ok(p)
}

/// 이름의 profile과 기본 profile인지: agent.toml `[profiles.*]`, 없으면 같은 이름의 기본 profile(claude·codex, 참고 `get`)
pub(crate) fn find_profile(cfg: &ThreadsConfig, name: &str) -> Result<(Profile, bool)> {
    if let Some(p) = cfg.profiles.get(name) {
        return Ok((p.clone(), false));
    }
    aaa_config::profile_edit::builtin(name).map(|p| (p, true)).with_context(|| {
        format!(
            "이 머신 agent.toml에 profile `{name}`가 없다(있는 것: {:?}, 기본 profile: claude, codex). 사람이 `aaa profile add`로 더한다",
            cfg.profiles.keys().collect::<Vec<_>>()
        )
    })
}

/// safety 인자를 profile 인자에 더한다(plan §8.163, 참고 `profiles::launch_args` 뒤 `Safety::launch_args`): 프로젝트 기본
/// profile과 같은 harness의 기본 profile이면 `thread_agent_args`를 앞에, yolo면 harness의 권한 건너뛰기 인자를 뒤에.
/// argv와 resume argv가 같은 인자를 쓴다.
fn with_safety(cfg: &ThreadsConfig, (mut p, builtin): (Profile, bool), safety: Option<&LaunchSafety>) -> Profile {
    let Some(s) = safety else { return p };
    let legacy_harness =
        find_profile(cfg, &s.default_profile).map(|(d, _)| d.harness).unwrap_or_else(|_| "claude".into());
    let mut args = aaa_config::safety::legacy_args(builtin, &p.harness, &s.thread_agent_args, &legacy_harness);
    args.extend(p.args.iter().cloned());
    p.args = aaa_config::safety::apply_yolo(s.yolo, &p.harness, &args);
    p
}

/// 있는 작업 폴더에서 profile을 다시 해석한다(brief.md는 host가 준 것을 이미 썼다)
fn launch_in(cfg: &ThreadsConfig, name: &str, work_dir: &Path, safety: Option<&LaunchSafety>) -> Result<Launch> {
    let p = with_safety(cfg, find_profile(cfg, name)?, safety);
    with_hooks(cfg, resolve(name, &p)?, work_dir, p.command.len().max(1))
}

/// harness별 상태 hook을 launch에 붙인다
pub(crate) fn with_hooks(
    cfg: &ThreadsConfig,
    mut launch: Launch,
    work_dir: &Path,
    command_len: usize,
) -> Result<Launch> {
    if launch.harness == "claude" {
        // 실행마다 hook 설정을 준다(사용자 ~/.claude 설정은 고치지 않는다). resume도 같은 설정을 쓴다.
        let settings = work_dir.join("claude-settings.json");
        let value = crate::hooks::claude_settings(&cfg.exe);
        std::fs::write(&settings, serde_json::to_vec_pretty(&value)?)?;
        let s = settings.to_string_lossy().to_string();
        for argv in [&mut launch.argv, &mut launch.resume_argv] {
            argv.insert(command_len, s.clone());
            argv.insert(command_len, "--settings".into());
        }
    }
    if launch.harness == "codex" {
        // 전역 hook도 pane의 env·peer 계보에서 돌아야 한다. 사용자 notify는 그대로 둔다.
        for argv in [&mut launch.argv, &mut launch.resume_argv] {
            ensure_codex_local(argv, command_len);
        }
    }
    Ok(launch)
}

/// 공유 daemon은 pane의 AAA 환경·peer 계보를 잃는다(§8.2). resume 앞의 전역 옵션으로 준다.
/// Codex는 commentary 출력 중 Working 줄을 숨긴다. 애니메이션과 별개인 run-state를 OSC 제목에 넣어 감지한다.
pub(crate) fn ensure_codex_local(argv: &mut Vec<String>, command_len: usize) {
    if !argv.iter().skip(command_len).take_while(|a| a.as_str() != "--").any(|a| a == "--no-daemon") {
        argv.insert(command_len, "--no-daemon".into());
    }
    // 사용자 파일은 고치지 않는다. 앞의 사용자 -c보다 나중에 두고, -- 뒤의 prompt에는 넣지 않는다.
    // launch/resume 준비와 최종 spawn에서 모두 지나므로 이미 끝에 붙인 옵션은 반복하지 않는다.
    let title = r#"tui.terminal_title=["run-state","activity","project-name"]"#;
    let end = argv.iter().skip(command_len).position(|a| a == "--").map_or(argv.len(), |n| command_len + n);
    if end < 2 || argv[end - 2] != "-c" || argv[end - 1] != title {
        argv.splice(end..end, ["-c".into(), title.into()]);
    }
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    std::io::copy(&mut f, &mut h)?;
    Ok(hex::encode(h.finalize()))
}

/// report.md hash. 없거나 regular file이 아니면 None
pub fn report_hash(work_dir: &Path) -> Option<String> {
    let path = work_dir.join("report.md");
    let meta = std::fs::symlink_metadata(&path)
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
        .ok()?;
    if !meta.file_type().is_file() || meta.len() > MAX_REPORT_BYTES {
        return None;
    }
    sha256_file(&path)
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
        .ok()
}

/// work_dir가 thread 작업 폴더 형식인가(host가 준 경로를 그대로 믿지 않는다)
pub(crate) fn check_work_dir(work_dir: &str) -> Result<PathBuf> {
    let p = PathBuf::from(work_dir);
    let ok = p.is_absolute()
        && p.parent().and_then(|d| d.file_name()).is_some_and(|n| n == ".aaa")
        && p.components().all(|c| !matches!(c, Component::ParentDir));
    if !ok {
        bail!("thread 작업 폴더 형식이 아니다: {work_dir}");
    }
    let meta = std::fs::symlink_metadata(&p).with_context(|| format!("작업 폴더가 없다: {work_dir}"))?;
    if !meta.is_dir() {
        bail!("작업 폴더가 폴더가 아니다(symlink 등): {work_dir}");
    }
    Ok(p)
}

pub fn collect(work_dir: &str, max_library_bytes: u64) -> Result<ReportSnapshot> {
    let dir = check_work_dir(work_dir)?;
    let path = dir.join("report.md");
    let meta = std::fs::symlink_metadata(&path).context("report.md가 없다")?;
    if !meta.file_type().is_file() {
        bail!("report.md가 regular file이 아니다");
    }
    if meta.len() > MAX_REPORT_BYTES {
        bail!("report.md가 너무 크다({} bytes)", meta.len());
    }
    let report = std::fs::read(&path)?;
    let hash = hex::encode(Sha256::digest(&report));
    let mut files = Vec::new();
    let mut skipped = Vec::new();
    let mut total = 0u64;
    let lib = dir.join("library");
    if std::fs::symlink_metadata(&lib)
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "성공 여부만 쓰는 분기의 하위 오류"))
        .is_ok_and(|m| m.is_dir())
    {
        walk(&lib, &lib, &mut |rel, meta, abs| {
            if meta.file_type().is_symlink() {
                skipped
                    .push(Skipped { path: rel, reason: "symlink는 따라가지 않는다".into(), lazy_size: None });
            } else if !meta.is_file() {
                skipped.push(Skipped { path: rel, reason: "일반 파일이 아니다".into(), lazy_size: None });
            } else if total + meta.len() > max_library_bytes {
                skipped.push(Skipped {
                    path: rel,
                    reason: format!("복사 상한({} MB)을 넘는다. agent에 남는다", max_library_bytes >> 20),
                    lazy_size: Some(meta.len()),
                });
            } else {
                // 계속 쓰는 로그도 manifest의 size와 같은 prefix를 해시한다.
                let hash = (|| -> Result<String> {
                    use std::io::Read;
                    let mut file = std::fs::File::open(abs)?.take(meta.len());
                    let mut digest = Sha256::new();
                    if std::io::copy(&mut file, &mut digest)? != meta.len() {
                        bail!("해시하는 동안 파일이 줄었다");
                    }
                    Ok(hex::encode(digest.finalize()))
                })();
                match hash {
                    Ok(sha256) => {
                        total += meta.len();
                        files.push(LibFile { path: rel, size: meta.len(), sha256 });
                    }
                    Err(e) => {
                        skipped.push(Skipped { path: rel, reason: format!("읽기 실패: {e}"), lazy_size: None })
                    }
                }
            }
        })?;
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(ReportSnapshot { hash, report: aaa_proto::B64(report), files, skipped })
}

fn walk(root: &Path, dir: &Path, f: &mut impl FnMut(String, std::fs::Metadata, &Path)) -> Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(|e| {
            e.inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
                .ok()
        })
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let path = e.path();
        let meta = std::fs::symlink_metadata(&path)?;
        let rel = path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
        if meta.is_dir() {
            walk(root, &path, f)?;
        } else {
            f(rel, meta, &path);
        }
    }
    Ok(())
}

/// library 파일 한 조각. 경로는 library 아래 상대 경로만, symlink는 거부한다.
pub fn read_chunk(work_dir: &str, rel: &str, offset: u64, len: u32) -> Result<(Vec<u8>, bool)> {
    use std::io::{Read, Seek, SeekFrom};
    let dir = check_work_dir(work_dir)?;
    let relp = Path::new(rel);
    if rel.is_empty() || !relp.components().all(|c| matches!(c, Component::Normal(_))) {
        bail!("library 상대 경로가 아니다: {rel}");
    }
    let mut cur = dir.join("library");
    for c in relp.components() {
        cur = cur.join(c);
        let meta = std::fs::symlink_metadata(&cur)?;
        if meta.file_type().is_symlink() {
            bail!("symlink는 읽지 않는다: {rel}");
        }
    }
    let mut f = std::fs::File::open(&cur)?;
    let size = f.metadata()?.len();
    f.seek(SeekFrom::Start(offset))?;
    let mut buf = vec![0u8; len.min(512 * 1024) as usize];
    let n = f.read(&mut buf)?;
    buf.truncate(n);
    Ok((buf, offset + n as u64 >= size))
}

/// 사후 평가자 사건 묶음을 `<work_dir>/inbox/<이름>.json`에 쓴다(§8.13). 다른 곳에는 쓰지 않는다.
pub fn write_inbox(work_dir: &str, rel: &str, data: &[u8]) -> Result<()> {
    const MAX: usize = 8 << 20;
    let dir = check_work_dir(work_dir)?;
    let name = rel
        .strip_prefix("inbox/")
        .and_then(|n| n.strip_suffix(".json"))
        .filter(|n| !n.is_empty() && n.len() <= 64 && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        .with_context(|| format!("inbox/<이름>.json만 쓴다: {rel}"))?;
    if data.len() > MAX {
        bail!("사건 묶음이 너무 크다({}바이트)", data.len());
    }
    let inbox = dir.join("inbox");
    match std::fs::symlink_metadata(&inbox) {
        Ok(m) if !m.is_dir() => bail!("inbox가 폴더가 아니다(symlink 등)"),
        Ok(_) => {}
        Err(source_error) => {
            aaa_common::diagnostic::observe(&source_error, "오류 뒤 기존 복구·거부 분기를 따른다");
            std::fs::create_dir(&inbox)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                std::fs::set_permissions(&inbox, std::fs::Permissions::from_mode(0o700))?;
            }
        }
    }
    let tmp = inbox.join(format!(".{name}.tmp"));
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, inbox.join(format!("{name}.json")))?;
    Ok(())
}

pub(crate) use preparation::protect_recorded_worktrees_locked;
/// thread 정리(herdr-projects threads.rs `clean`). worktree는 force 없이 지운다. branch는 PR이 병합됐고(`merged_head`가
/// 있다) worktree가 지워졌을 때만 `delete_branch`로 지운다. `uncopied`(마지막 사본에 빠진 것이 있다)면 폴더가 남아 있는 한
/// worktree를 남긴다: 작업 폴더(`.aaa/`)는 git이 무시하는 파일이라 force 없는 `worktree remove`도 지운다. host는 그때
/// `keep_worktree`도 켜서 보낸다(옛 agent도 지우지 않게).
pub use preparation::{cleanup_absent, cleanup_recorded, placement, protect_recorded_worktrees, recover_cleanup_locks};

#[allow(clippy::too_many_arguments)]
async fn cleanup(
    repo: Option<&str>,
    cwd: &str,
    branch: Option<&str>,
    merged_head: Option<&str>,
    keep_worktree: bool,
    uncopied: bool,
    owned_lock: Option<&str>,
    wait: Duration,
) -> Result<Cleanup> {
    let mut c = Cleanup { worktree_removed: false, branch_deleted: false, notes: Vec::new() };
    let Some(repo) = repo else {
        c.notes.push("작업 폴더는 남긴다(repo 없는 thread)".into());
        return Ok(c);
    };
    let repo = PathBuf::from(repo);
    if keep_worktree && !uncopied {
        c.notes.push("worktree를 남겼다(요청)".into());
    } else if uncopied && Path::new(cwd).exists() {
        c.notes.push("worktree를 남겼다: 마지막 사본에 집에 오지 않은 것이 있다(`--discard-uncopied`면 지운다)".into());
    } else {
        let registered = preparation::worktree_lock(&repo, Path::new(cwd)).await?;
        match &registered {
            Some(Some(reason)) if Some(reason.as_str()) != owned_lock => {
                c.notes.push("worktree를 남겼다: 사람이 걸었거나 다른 인스턴스의 Git 잠금이 있다".into());
                return Ok(c);
            }
            Some(Some(_)) => {
                git(&repo, &["worktree", "unlock", cwd], GIT_T).await?;
            }
            _ => {}
        }
        let gone = !Path::new(cwd).exists();
        if registered.is_none() && gone {
            c.worktree_removed = true;
        } else {
            // 대상 하나만 지운다. 넓은 prune은 빠진 다른 볼륨의 기록까지 지울 수 있다.
            match git(&repo, &["worktree", "remove", cwd], wait).await {
                Ok(_) => c.worktree_removed = true,
                Err(e) => c.notes.push(format!("worktree를 남겼다: {e:#}")),
            }
        }
        if c.worktree_removed && gone {
            c.notes.push("worktree는 이미 없다".into());
        }
    }
    if let Some(branch) = branch.filter(|b| !b.is_empty()) {
        match (merged_head, c.worktree_removed) {
            (Some(head), true) => match delete_branch(&repo, branch, head).await {
                Ok(true) => {
                    c.branch_deleted = true;
                    c.notes.push(format!("branch {branch}를 지웠다(PR이 병합됐다)"));
                }
                Ok(false) => {
                    c.branch_deleted = true;
                    c.notes.push(format!("branch {branch}는 이미 지워졌다"));
                }
                Err(e) => c.notes.push(format!("branch {branch}를 남겼다: {e:#}")),
            },
            (None, _) => c.notes.push(format!("branch {branch}를 남겼다: PR이 병합되지 않았다")),
            (Some(_), false) => c.notes.push(format!("branch {branch}를 남겼다: worktree가 남아 있다")),
        }
    }
    Ok(c)
}

/// branch 끝이 병합된 PR의 head commit일 때만 지운다(herdr-projects `delete_branch`): 병합 뒤 push하지 않은 commit이
/// 있으면 남는다. `-D`인 까닭: GitHub가 병합됐다고 하고, squash 병합은 git이 보기에 병합되지 않은 branch를 남긴다.
/// `false`: 지울 branch가 없었다.
async fn delete_branch(repo: &Path, branch: &str, head: &str) -> Result<bool> {
    if head.is_empty() {
        bail!("병합된 PR의 head commit을 모른다");
    }
    let tip = git(repo, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")], GIT_T)
        .await
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    if tip.is_empty() {
        return Ok(false);
    }
    if tip != head {
        bail!("병합된 PR에 없는 commit이 있다");
    }
    git(repo, &["branch", "-D", branch], GIT_T).await.map(|_| true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(harness: &str) -> Profile {
        Profile {
            harness: harness.into(),
            model: "opus".into(),
            effort: "high".into(),
            args: vec!["--no-daemon".into()],
            env: [("CODEX_SSH_SKIP_APP_SERVER_BOOT".to_string(), "true".to_string())].into(),
            command: vec![],
            description: String::new(),
        }
    }

    /// 기본·wrapper 실행과 resume을 pane 안에 두며 사용자 notify와 명시 인자를 보존한다.
    #[test]
    fn codex_launch_and_resume_preserve_user_notify() {
        let cfg = ThreadsConfig { exe: "/opt/aaa".into(), ..Default::default() };
        for (command, args, notify) in [
            (vec![], vec![], None),
            (vec!["wrapper".into(), "codex".into()], vec![], None),
            (vec!["env".into(), "--".into(), "codex".into()], vec![], None),
            (vec![], vec!["--no-daemon".into()], Some(r#"notify=["user-notify","fixed arg"]"#)),
            (vec![], vec!["-c".into(), "tui.terminal_title=[]".into()], None),
        ] {
            let mut p = profile("codex");
            p.command = command;
            p.args = args;
            p.model.clear();
            p.effort.clear();
            if let Some(n) = notify {
                p.args.extend(["-c".into(), n.into()]);
            }
            let prefix = p.command.len().max(1);
            let launch = with_hooks(&cfg, resolve("p", &p).unwrap(), Path::new("unused"), prefix).unwrap();
            for argv in [&launch.argv, &launch.resume_argv] {
                let values: Vec<_> = argv.iter().filter(|s| s.starts_with("notify=")).map(String::as_str).collect();
                assert_eq!(values, notify.into_iter().collect::<Vec<_>>(), "{argv:?}");
                if !p.args.iter().any(|a| a == "--no-daemon") {
                    assert_eq!(
                        argv.get(prefix).map(String::as_str),
                        Some("--no-daemon"),
                        "pane 밖 공유 daemon으로 실행될 수 있다: {argv:?}"
                    );
                }
                assert_eq!(argv.iter().filter(|a| *a == "--no-daemon").count(), 1, "{argv:?}");
                assert_eq!(
                    &argv[argv.len() - 2..],
                    ["-c", r#"tui.terminal_title=["run-state","activity","project-name"]"#],
                    "중간 설명에서 상태 줄이 사라져도 OSC 제목에 실행 상태가 있어야 한다: {argv:?}"
                );
            }
        }
    }

    /// Codex의 문서화된 notify 계약(설정 배열 + 마지막 JSON 인자)을 실행하는 최소 harness.
    /// 제품 argv로 사용자 알림이 실제 불리는지 본다. 실제 Codex 계정·모델 호출은 쓰지 않는다.
    #[cfg(unix)]
    #[test]
    fn codex_user_notify_is_invoked_for_launch_and_resume() {
        let dir = std::env::temp_dir().join(format!("aaa-notify-{}", aaa_common::util::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("codex.py");
        std::fs::write(
            &script,
            r#"
import json, os, pathlib, subprocess, sys, tomllib
assert sys.argv[1:2] == ['--no-daemon'], sys.argv
config = tomllib.loads((pathlib.Path(os.environ['CODEX_HOME']) / 'config.toml').read_text())
for i, arg in enumerate(sys.argv[1:], 1):
    if arg == '-c' and i + 1 < len(sys.argv):
        config.update(tomllib.loads(sys.argv[i + 1]))
payload = json.dumps({'type': 'agent-turn-complete', 'thread-id': 'test-thread'})
subprocess.run(config['notify'] + [payload], check=True)
"#,
        )
        .unwrap();
        let marker = dir.join("notified.json");
        let notify = serde_json::json!([
            "python3",
            "-c",
            "import pathlib,sys; pathlib.Path(sys.argv[1]).write_text(sys.argv[2])",
            marker.to_str().unwrap()
        ]);
        std::fs::write(dir.join("config.toml"), format!("notify = {notify}\n")).unwrap();
        let mut p = profile("codex");
        p.command = vec!["python3".into(), script.to_str().unwrap().into()];
        p.args.clear();
        p.model.clear();
        p.effort.clear();
        let cfg = ThreadsConfig { exe: std::env::current_exe().unwrap(), ..Default::default() };
        let l = with_hooks(&cfg, resolve("p", &p).unwrap(), &dir, 2).unwrap();
        for argv in [&l.argv, &l.resume_argv] {
            let o = std::process::Command::new(&argv[0]).args(&argv[1..]).env("CODEX_HOME", &dir).output().unwrap();
            assert!(o.status.success(), "{o:?}");
            let payload: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&marker).expect("사용자 notify가 불리지 않았다")).unwrap();
            assert_eq!(payload, serde_json::json!({"type":"agent-turn-complete","thread-id":"test-thread"}));
            std::fs::remove_file(&marker).unwrap();
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// resume은 launch와 같은 model·effort·args(daemon 금지 인자 포함)·env를 쓴다(§8.2).
    #[test]
    fn resume_keeps_launch_spec() {
        for h in ["claude", "codex"] {
            let l = resolve("p", &profile(h)).unwrap();
            for flag in ["opus", "--no-daemon"] {
                assert!(l.argv.iter().any(|a| a == flag), "{h} argv {:?}", l.argv);
                assert!(l.resume_argv.iter().any(|a| a == flag), "{h} resume {:?}", l.resume_argv);
            }
            assert!(l.argv.iter().chain(&l.resume_argv).any(|a| a.contains("high")));
            assert!(l.resume_argv.iter().any(|a| a == "{session}"));
            assert_eq!(l.env, [("CODEX_SSH_SKIP_APP_SERVER_BOOT".to_string(), "true".to_string())]);
        }
        let c = resolve("p", &profile("claude")).unwrap();
        let key = c.session_key.clone().unwrap();
        assert_eq!(key.len(), 36);
        assert!(c.argv.ends_with(&["--session-id".into(), key]));
        assert_eq!(c.argv[0], "claude");
        let x = resolve("p", &profile("codex")).unwrap();
        assert_eq!(&x.resume_argv[..3], ["codex", "resume", "{session}"]);
        assert!(x.argv.contains(&"model_reasoning_effort=high".to_string()));
    }

    #[test]
    fn rebrief_writes_brief_and_new_claude_session() {
        let root = std::env::temp_dir().join(format!("aaa-rebrief-{}", std::process::id()));
        let wd = root.join(".aaa").join("p-t-0001");
        std::fs::create_dir_all(&wd).unwrap();
        let old = resolve("c", &profile("claude")).unwrap();
        let brief = format!("새 brief `{}/report.md` `{} report`", aaa_proto::BRIEF_WORK_DIR, aaa_proto::BRIEF_EXE);
        let mut cfg = ThreadsConfig { exe: "/opt/aaa".into(), ..Default::default() };
        let new = rebrief(&cfg, wd.to_str().unwrap(), &brief, old.clone(), None, None, None).unwrap();
        // 작업 폴더·실행 파일 자리는 이 머신 값으로 채운다
        assert_eq!(
            std::fs::read_to_string(wd.join("brief.md")).unwrap(),
            format!("새 brief `{}/report.md` `/opt/aaa report`", wd.display())
        );
        assert_ne!(new.session_key, old.session_key);
        let i = new.argv.iter().position(|a| a == "--session-id").unwrap();
        assert_eq!(Some(&new.argv[i + 1]), new.session_key.as_ref());
        assert_eq!(new.resume_argv, old.resume_argv);
        // --profile: 이 머신 agent.toml에서 다시 해석하고 그 harness의 hook을 붙인다(옛 launch는 쓰지 않는다)
        cfg.profiles.insert("x".into(), profile("codex"));
        let other = rebrief(&cfg, wd.to_str().unwrap(), &brief, old.clone(), Some("x"), None, None).unwrap();
        assert_eq!((other.profile.as_str(), other.harness.as_str()), ("x", "codex"));
        assert_eq!(other.argv[0], "codex", "새 profile의 실행 파일을 쓴다: {:?}", other.argv);
        let e = rebrief(&cfg, wd.to_str().unwrap(), &brief, old, Some("nope"), None, None).unwrap_err();
        assert!(format!("{e:#}").contains("profile `nope`가 없다"), "{e:#}");
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// safety 인자(plan §8.163, 참고 `profiles::launch_args`·`Safety::launch_args`): thread_agent_args는 프로젝트 기본
    /// profile과 같은 harness의 기본 profile에만, yolo 인자는 harness마다(argv·resume argv 둘 다), 기본 profile은 이름으로 푼다.
    #[test]
    fn safety_args_reach_builtins_and_yolo_every_harness() {
        let mut cfg = ThreadsConfig::default();
        cfg.profiles.insert("deep".into(), profile("codex"));
        let s =
            LaunchSafety { yolo: true, thread_agent_args: vec!["--verbose".into()], default_profile: "claude".into() };
        let launch = |name: &str, s: Option<&LaunchSafety>| {
            resolve(name, &with_safety(&cfg, find_profile(&cfg, name).unwrap(), s)).unwrap()
        };
        let c = launch("claude", Some(&s));
        for argv in [&c.argv, &c.resume_argv] {
            assert!(argv.contains(&"--verbose".to_string()), "{argv:?}");
            assert!(argv.contains(&"--dangerously-skip-permissions".to_string()), "{argv:?}");
        }
        // 다른 harness의 기본 profile: 옛 인자는 없고 제 yolo 인자
        let x = launch("codex", Some(&s));
        assert!(!x.argv.contains(&"--verbose".to_string()), "{:?}", x.argv);
        assert!(x.argv.contains(&"--dangerously-bypass-approvals-and-sandbox".to_string()), "{:?}", x.argv);
        assert!(
            x.resume_argv.contains(&"--dangerously-bypass-approvals-and-sandbox".to_string()),
            "{:?}",
            x.resume_argv
        );
        // 사람이 정의한 profile: 제 인자 + yolo 인자만
        let d = launch("deep", Some(&s));
        assert!(
            !d.argv.contains(&"--verbose".to_string()) && d.argv.contains(&"--no-daemon".to_string()),
            "{:?}",
            d.argv
        );
        assert!(d.argv.contains(&"--dangerously-bypass-approvals-and-sandbox".to_string()), "{:?}", d.argv);
        // 기본 profile이 codex인 프로젝트: codex 기본 profile이 옛 인자를 받는다
        let s2 = LaunchSafety { yolo: false, default_profile: "deep".into(), ..s.clone() };
        assert!(launch("codex", Some(&s2)).argv.contains(&"--verbose".to_string()));
        assert!(!launch("codex", Some(&s2)).argv.iter().any(|a| a.starts_with("--dangerously")));
        // safety가 없으면(옛 host) 그대로
        assert_eq!(launch("claude", None).argv.iter().filter(|a| a.starts_with("--")).count(), 1, "--session-id만");
        assert!(format!("{:#}", find_profile(&cfg, "gpt").unwrap_err()).contains("기본 profile: claude, codex"));
    }

    #[test]
    fn work_dir_must_be_thread_folder() {
        assert!(check_work_dir("relative/.aaa/p-t-0001").is_err());
        assert!(check_work_dir("/tmp/x/../.aaa/p-t-0001").is_err());
        assert!(check_work_dir("/etc").is_err());
    }
}

/// 임무 입력의 처리 여부는 agent가 작업 폴더에서 읽는다. symlink는 따라가지 않는다.
pub fn work_file_inbox_pending(work_dir: &str) -> Result<Vec<String>> {
    let dir = check_work_dir(work_dir)?;
    let processed = dir.join("processed.txt");
    if let Ok(m) = processed.symlink_metadata()
        && (!m.is_file() || m.len() > 1024 * 1024)
    {
        bail!("processed.txt는 1MiB 이하 일반 파일이어야 한다");
    }
    let done = std::fs::read_to_string(processed).unwrap_or_default();
    let done: std::collections::HashSet<_> = done.lines().map(str::trim).collect();
    let inbox = dir.join("inbox");
    if let Ok(m) = inbox.symlink_metadata()
        && !m.is_dir()
    {
        bail!("inbox는 일반 폴더여야 한다");
    }
    let mut names = vec![];
    for e in std::fs::read_dir(inbox).into_iter().flatten() {
        let e = e?;
        let name = e.file_name().to_string_lossy().into_owned();
        if e.file_type()?.is_file() && name.ends_with(".json") && !done.contains(name.as_str()) {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

fn work_file_state_path(work_dir: &str, rel: &str) -> Result<PathBuf> {
    use std::path::Component;
    let dir = check_work_dir(work_dir)?;
    let path = Path::new(rel);
    if !path.components().all(|c| matches!(c, Component::Normal(_))) {
        bail!("임무 상태의 상대 경로가 아니다");
    }
    let allowed = matches!(rel, "state.md" | "processed.txt")
        || rel.starts_with("inbox/") && rel.ends_with(".json")
        || rel.starts_with("history/")
        || rel.starts_with("memory/");
    if !allowed || rel.len() > 1024 {
        bail!("임무 상태 파일이 아니다");
    }
    let mut at = dir;
    for c in path.components() {
        at.push(c);
        if let Ok(m) = at.symlink_metadata()
            && m.file_type().is_symlink()
        {
            bail!("임무 상태에 symlink가 있다");
        }
    }
    Ok(at)
}
/// 새 작업 폴더에 넘길 상태. source는 세션이 끝난 뒤 읽으며 원본을 지우지 않는다.
pub fn work_file_state_list(work_dir: &str) -> Result<Vec<aaa_proto::LibFile>> {
    use sha2::Digest;
    let dir = check_work_dir(work_dir)?;
    let mut paths = vec!["state.md".to_string(), "processed.txt".into()];
    for sub in ["inbox", "history", "memory"] {
        let mut dirs = vec![dir.join(sub)];
        while let Some(at) = dirs.pop() {
            if let Ok(m) = at.symlink_metadata()
                && !m.is_dir()
            {
                bail!("임무 상태 폴더가 일반 폴더가 아니다");
            }
            for e in std::fs::read_dir(&at).into_iter().flatten() {
                let e = e?;
                let t = e.file_type()?;
                if t.is_symlink() {
                    bail!("임무 상태에 symlink가 있다");
                }
                if t.is_dir() {
                    dirs.push(e.path());
                } else if t.is_file() {
                    paths.push(
                        e.path()
                            .strip_prefix(&dir)?
                            .components()
                            .map(|c| c.as_os_str().to_string_lossy())
                            .collect::<Vec<_>>()
                            .join("/"),
                    );
                }
            }
        }
    }
    paths.sort();
    let mut files = vec![];
    let mut size = 0;
    for rel in paths {
        let path = work_file_state_path(work_dir, &rel)?;
        if !path.exists() {
            continue;
        }
        let b = std::fs::read(path)?;
        size += b.len();
        if size > 8 * 1024 * 1024 || files.len() >= 1000 {
            bail!("옮길 임무 상태가 8MiB 또는 1000개를 넘는다. 원본을 보존했다");
        }
        files.push(aaa_proto::LibFile {
            path: rel,
            size: b.len() as u64,
            sha256: hex::encode(sha2::Sha256::digest(&b)),
        });
    }
    Ok(files)
}
pub fn work_file_state_read(work_dir: &str, rel: &str, offset: u64) -> Result<(Vec<u8>, bool)> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(work_file_state_path(work_dir, rel)?)?;
    let size = f.metadata()?.len();
    if offset > size {
        bail!("임무 상태 읽기 offset이 범위를 넘었다");
    }
    f.seek(SeekFrom::Start(offset))?;
    let mut b = vec![];
    f.take(384 * 1024).read_to_end(&mut b)?;
    let eof = offset + b.len() as u64 == size;
    Ok((b, eof))
}
pub fn work_file_state_write(work_dir: &str, rel: &str, offset: u64, data: &[u8], eof: bool, sha: &str) -> Result<()> {
    use sha2::Digest;
    use std::io::Write;
    if data.len() > 384 * 1024 || offset.saturating_add(data.len() as u64) > 8 * 1024 * 1024 {
        bail!("임무 상태 조각 상한이다");
    }
    let path = work_file_state_path(work_dir, rel)?;
    let parent = path.parent().context("임무 상태 부모가 없다")?;
    std::fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(".work-file-{}.tmp", path.file_name().unwrap().to_string_lossy()));
    if tmp.symlink_metadata().is_ok_and(|m| !m.is_file()) {
        bail!("임무 상태 임시 파일이 일반 파일이 아니다");
    }
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true);
    let mut f = if offset == 0 {
        opts.truncate(true).open(&tmp)?
    } else {
        let f = opts.append(true).open(&tmp)?;
        if f.metadata()?.len() != offset {
            bail!("임무 상태 조각 offset이 다르다");
        }
        f
    };
    f.write_all(data)?;
    drop(f);
    if eof {
        let actual = hex::encode(sha2::Sha256::digest(std::fs::read(&tmp)?));
        if actual != sha {
            bail!("임무 상태 hash가 다르다. 원본을 보존했다");
        }
        std::fs::rename(tmp, path)?;
    }
    Ok(())
}

#[cfg(test)]
mod work_file_tests {
    use super::*;

    /// Git 자체에서 옵션 종료 경계와 fsmonitor·textconv·index 갱신 차단을 검증한다.
    #[cfg(unix)]
    use sha2::Digest;
    #[test]
    fn work_file_inbox_counts_processed_and_state_transfer_is_exact() {
        let root = std::env::temp_dir().join(format!("aaa-work-files-{}", aaa_common::util::new_id()));
        let from = root.join("from/.aaa/demo-t-0001");
        let to = root.join("to/.aaa/demo-t-0001");
        std::fs::create_dir_all(&from).unwrap();
        std::fs::create_dir_all(&to).unwrap();
        let from = from.to_str().unwrap();
        let to = to.to_str().unwrap();
        write_inbox(from, "inbox/p-00000001.json", "첫 입력".as_bytes()).unwrap();
        write_inbox(from, "inbox/r-00000002.json", "둘째 입력".as_bytes()).unwrap();
        std::fs::write(Path::new(from).join("processed.txt"), "p-00000001.json\n").unwrap();
        std::fs::write(Path::new(from).join("state.md"), "이어 갈 판단").unwrap();
        assert_eq!(work_file_inbox_pending(from).unwrap().len(), 1);
        std::fs::create_dir_all(Path::new(from).join("memory/history")).unwrap();
        std::fs::write(Path::new(from).join("memory/memory.md"), "자동화 기억").unwrap();
        std::fs::write(Path::new(from).join("memory/history/day.md"), "원문").unwrap();
        let files = work_file_state_list(from).unwrap();
        assert_eq!(files.len(), 6);
        for f in &files {
            let (data, eof) = work_file_state_read(from, &f.path, 0).unwrap();
            assert!(eof);
            work_file_state_write(to, &f.path, 0, &data, eof, &f.sha256).unwrap();
            assert_eq!(
                std::fs::read(Path::new(from).join(&f.path)).unwrap(),
                std::fs::read(Path::new(to).join(&f.path)).unwrap()
            );
        }
        assert_eq!(work_file_inbox_pending(to).unwrap().len(), 1);
        assert!(work_file_state_write(to, "../escape", 0, b"x", true, "bad").is_err());
        assert!(
            work_file_state_write(
                to,
                "state.md",
                0,
                "오염".as_bytes(),
                true,
                &hex::encode(sha2::Sha256::digest("다른 내용".as_bytes()))
            )
            .is_err()
        );
        assert_eq!(std::fs::read_to_string(Path::new(to).join("state.md")).unwrap(), "이어 갈 판단");
        std::fs::remove_dir_all(root).unwrap();
    }
}

//! 계획에서 파일 변경을 만든다. 기존 TOML의 주석·알 수 없는 key와 local 우선순위를 보존한다.
use super::*;
use toml_edit::{DocumentMut, Item};

fn conflict() -> Fault {
    Fault::conflict()
}
fn text(path: &Path) -> Result<Option<String>> {
    let resolved = target(path)?;
    let meta = match resolved.metadata() {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(Fault::io(e)),
    };
    if !meta.is_file() || meta.len() > 1024 * 1024 {
        return Err(conflict());
    }
    std::fs::read_to_string(resolved).map(Some).map_err(Fault::io)
}
pub(super) fn proposal(
    path: PathBuf,
    kind: Kind,
    before: Option<Vec<u8>>,
    bytes: Vec<u8>,
    mode: Option<u32>,
    preserve: bool,
    shared: bool,
) -> Result<Proposed> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if path.symlink_metadata().is_ok_and(|m| m.nlink() != 1) {
            return Err(conflict());
        }
    }
    let expected = before.as_deref().map(hash);
    let (observed, before_mode) = snapshot(&path)?;
    if observed != expected {
        return Err(conflict());
    }
    let before_inode = if expected.is_some() { Some(identity(&path)?) } else { None };
    Ok(Proposed {
        path,
        kind,
        before: expected,
        before_mode,
        before_inode,
        force: false,
        bytes,
        mode,
        preserve,
        shared,
    })
}
struct Doc {
    path: PathBuf,
    before: Option<String>,
    value: DocumentMut,
}
impl Doc {
    fn load(path: PathBuf) -> Result<Self> {
        let before = text(&path)?;
        let value: DocumentMut = before.as_deref().unwrap_or("schema = 1\n").parse().map_err(|_| conflict())?;
        if value.get("schema").and_then(Item::as_integer) != Some(1) {
            return Err(conflict());
        }
        Ok(Self { path, before, value })
    }
    fn has(&self, keys: &[&str]) -> bool {
        let mut at = self.value.as_item();
        for key in keys {
            let Some(next) = at.as_table_like().and_then(|table| table.get(key)) else { return false };
            at = next;
        }
        true
    }
    fn put(&mut self, keys: &[&str], value: Item) -> Result<()> {
        let mut at = self.value.as_item_mut();
        for key in &keys[..keys.len() - 1] {
            if at.is_none() {
                *at = Item::Table(toml_edit::Table::new());
            }
            let table = at.as_table_like_mut().ok_or_else(conflict)?;
            if !table.contains_key(key) {
                table.insert(key, Item::Table(toml_edit::Table::new()));
            }
            at = table.get_mut(key).ok_or_else(conflict)?;
        }
        at.as_table_like_mut().ok_or_else(conflict)?.insert(keys[keys.len() - 1], value);
        Ok(())
    }
    fn finish(self, kind: Kind, files: &mut Vec<Proposed>) -> Result<()> {
        let bytes = self.value.to_string().into_bytes();
        if kind == Kind::Config && self.before.is_none() && bytes == b"schema = 1\n" {
            return Ok(());
        }
        files.push(proposal(self.path, kind, self.before.map(String::into_bytes), bytes, None, true, false)?);
        Ok(())
    }
}
struct Docs {
    base: Doc,
    local: Doc,
}
impl Docs {
    fn load(dir: &Path, stem: &str) -> Result<Self> {
        Ok(Self {
            base: Doc::load(dir.join(format!("{stem}.toml")))?,
            local: Doc::load(dir.join(format!("{stem}.local.toml")))?,
        })
    }
    fn put(&mut self, keys: &[&str], value: Item) -> Result<()> {
        if self.local.has(keys) { self.local.put(keys, value) } else { self.base.put(keys, value) }
    }
    fn finish(self, files: &mut Vec<Proposed>) -> Result<()> {
        self.base.finish(Kind::Config, files)?;
        self.local.finish(Kind::Config, files)
    }
}
fn string(s: &str) -> Item {
    toml_edit::value(s)
}

pub(super) fn install(plan: InstallPlan) -> Result<Request> {
    let checked = aaa_config::install::plan(plan.choices.clone(), plan.context.clone()).map_err(|_| conflict())?;
    if checked != plan {
        return Err(conflict());
    }
    let c = &plan.choices;
    let cfg = Path::new(&plan.context.config_dir);
    let mut files = Vec::new();
    let mut directories = Vec::new();
    // safety 확대·PATH·서비스·등록은 이 파일 단계의 쓰기 권한으로 수행하지 않는다.
    if !c.allow_thread_profile.is_empty() || !c.allow_coordinator_profile.is_empty() {
        return Err(Fault { code: "installation_policy_executor_required", exit: 5 });
    }
    let selected = c.from.as_ref().map(PathBuf::from).unwrap_or(std::env::current_exe().map_err(Fault::io)?);
    if snapshot(&selected)?.0.is_none() {
        return Err(conflict());
    }
    let binary = std::fs::read(&selected).map_err(Fault::io)?;
    if hash(&binary) != plan.context.binary_sha256 {
        return Err(conflict());
    }
    let bin = PathBuf::from(&plan.context.bin_path);
    let before = if bin.exists() { Some(std::fs::read(&bin).map_err(Fault::io)?) } else { None };
    files.push(proposal(bin, Kind::Binary, before, binary, Some(0o755), false, true)?);
    let local = c.role == Some(InstallRole::Agent) || c.local_agent != Some(InstallToggle::Disabled);
    if c.role == Some(InstallRole::Host) {
        let mut host = Docs::load(cfg, "host")?;
        host.put(&["host", "agent_listen"], string(c.agent_listen.as_deref().ok_or_else(conflict)?))?;
        host.put(
            &["web", "listen"],
            string(if c.web == Some(InstallWeb::Off) { "" } else { c.web_listen.as_deref().ok_or_else(conflict)? }),
        )?;
        host.put(
            &["local_agent", "mode"],
            string(if local {
                if c.service == Some(InstallService::None) { "child" } else { "launchd" }
            } else {
                "none"
            }),
        )?;
        host.put(&["local_agent", "name"], string(c.name.as_deref().ok_or_else(conflict)?))?;
        if c.project_mode != Some(InstallProjectMode::Skip) {
            host.put(&["projects", "root"], string(&plan.context.projects_root))?;
        }
        if let Some(port) = c.serve_port {
            let mut ports = toml_edit::Array::new();
            ports.push(i64::from(port));
            host.put(&["web", "publish_ports"], Item::Value(ports.into()))?;
        }
        for (key, profile) in [("thread_profile", &c.thread_profile), ("coordinator_profile", &c.coordinator_profile)] {
            if let Some(profile) = profile {
                host.put(&["defaults", key], string(profile))?;
            }
        }
        for path in &c.profile_file {
            let profile = text(Path::new(path))?.ok_or_else(conflict)?;
            super::super::super::profiles::names(&profile).map_err(|_| conflict())?;
            let imported: DocumentMut = profile.parse().map_err(|_| conflict())?;
            for (name, value) in imported.get("profiles").and_then(Item::as_table_like).ok_or_else(conflict)?.iter() {
                for doc in [&host.base, &host.local] {
                    if let Some(old) = doc.value.get("profiles").and_then(|p| p.get(name)) {
                        let mut isolated = DocumentMut::new();
                        isolated["schema"] = toml_edit::value(1);
                        isolated["profiles"][name] = old.clone();
                        super::super::super::profiles::names(&isolated.to_string()).map_err(|_| conflict())?;
                    }
                }
                for (key, value) in value.as_table_like().ok_or_else(conflict)?.iter() {
                    host.put(&["profiles", name, key], value.clone())?;
                }
            }
        }
        host.finish(&mut files)?;
    }
    let root = if local {
        let mut agent = Docs::load(cfg, "agent")?;
        agent.put(&["agent", "name"], string(c.name.as_deref().ok_or_else(conflict)?))?;
        agent.put(
            &["agent", "host"],
            string(if c.role == Some(InstallRole::Host) { "local" } else { c.host.as_deref().ok_or_else(conflict)? }),
        )?;
        let root = c.work_root.as_deref().ok_or_else(conflict)?;
        for (key, value) in [
            ("work_root", root),
            ("work_mount", c.work_mount.as_deref().unwrap_or("")),
            ("default_cwd", c.default_cwd.as_deref().unwrap_or(root)),
        ] {
            agent.put(&["agent", key], string(value))?;
        }
        if let Some(shell) = &c.shell {
            let mut command = toml_edit::Array::new();
            command.push(shell.as_str());
            agent.put(&["harnesses", "shell", "command"], Item::Value(command.into()))?;
        }
        agent.put(&["health", "min_free_gb"], toml_edit::value(i64::from(c.min_free_gb.unwrap_or(20))))?;
        for (name, path, home) in [("claude", &c.claude_path, &c.claude_home), ("codex", &c.codex_path, &c.codex_home)]
        {
            if let Some(path) = path {
                let mut args = toml_edit::Array::new();
                args.push(path.as_str());
                agent.put(&["harnesses", name, "command"], Item::Value(args.into()))?;
            }
            if let Some(home) = home {
                agent.put(
                    &["harnesses", name, "env", if name == "claude" { "CLAUDE_CONFIG_DIR" } else { "CODEX_HOME" }],
                    string(home),
                )?;
            }
        }
        for env in &c.harness_env {
            if aaa_config::secret_like(&env.key) {
                return Err(conflict());
            }
            let name = match env.harness {
                InstallHarness::Claude => "claude",
                InstallHarness::Codex => "codex",
                InstallHarness::None => return Err(conflict()),
            };
            agent.put(&["harnesses", name, "env", &env.key], string(&env.value))?;
        }
        agent.finish(&mut files)?;
        Some(RootSpec {
            path: root.into(),
            mount: c.work_mount.as_ref().map(PathBuf::from),
            volume: super::super::super::platform::storage(Path::new(root)).ok_or_else(conflict)?.0,
        })
    } else {
        None
    };
    if c.hooks == Some(InstallHooks::Install) {
        let home = c.codex_home.as_ref().map(PathBuf::from).unwrap_or_else(crate::integration::codex_home);
        for edit in crate::integration::plan(home, Path::new(&plan.context.bin_path), false).map_err(|_| conflict())? {
            if let Some(after) = edit.after {
                files.push(proposal(
                    edit.path,
                    Kind::Hook,
                    edit.before.map(String::into_bytes),
                    after.into_bytes(),
                    None,
                    false,
                    true,
                )?);
            }
        }
    }
    if c.role == Some(InstallRole::Host) && c.project_mode != Some(InstallProjectMode::Skip) {
        let name = c.project.as_deref().ok_or_else(conflict)?;
        if !aaa_proto::valid_project(name) || name == "default" {
            return Err(conflict());
        }
        let projects = Path::new(&plan.context.projects_root);
        let entries = match std::fs::read_dir(projects) {
            Ok(entries) => Some(entries),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(Fault::io(e)),
        };
        if let Some(entries) = entries {
            for entry in entries {
                let entry = entry.map_err(Fault::io)?;
                if entry.file_name() != name && entry.path().join("PROJECT.md").exists() {
                    return Err(conflict());
                }
            }
        }
        let dir = real_directory(&projects.join(name))?;
        match c.project_mode {
            Some(InstallProjectMode::Use) if !dir.join("PROJECT.md").is_file() => return Err(conflict()),
            Some(InstallProjectMode::Create) if dir.join("PROJECT.md").exists() => return Err(conflict()),
            _ => {}
        }
        if c.project_mode == Some(InstallProjectMode::Create) {
            let skeleton = aaa_host::project_scaffold(name, &format!("# {name}\n\n지침을 여기에 쓴다.\n"), true);
            directories.extend(skeleton.directories.iter().map(|path| dir.join(path)));
            for (path, bytes) in skeleton.files {
                if path == "project.toml" {
                    continue;
                }
                let path = dir.join(path);
                if !path.exists() {
                    files.push(proposal(path, Kind::Project, None, bytes.into_bytes(), None, true, false)?);
                }
            }
        }
        let mut project = Doc::load(aaa_config::project_settings_file(&dir))?;
        if let Some(goal) = &c.goal {
            project.put(&["goal"], string(goal))?;
        }
        for (key, value) in [("thread_profile", &c.thread_profile), ("coordinator_profile", &c.coordinator_profile)] {
            if let Some(value) = value {
                project.put(&[key], string(value))?;
            }
        }
        let table = if project.path.file_name().is_some_and(|p| p == "aaa.toml") { "workspaces" } else { "repos" };
        let mut repos = match project.value.get(table) {
            None => toml_edit::ArrayOfTables::new(),
            Some(Item::ArrayOfTables(repos)) => repos.clone(),
            Some(_) => return Err(conflict()),
        };
        for repo in &c.repo {
            let mut item = toml_edit::Table::new();
            if let Some((path, machine)) = repo.rsplit_once('@') {
                item["path"] = string(path);
                item["machine"] = string(machine);
            } else {
                item["path"] = string(repo);
            }
            if !repos.iter().any(|old| {
                old.get("path").and_then(Item::as_str) == item.get("path").and_then(Item::as_str)
                    && old.get("machine").and_then(Item::as_str) == item.get("machine").and_then(Item::as_str)
            }) {
                repos.push(item);
            }
        }
        project.put(&[table], Item::ArrayOfTables(repos))?;
        project.finish(Kind::Project, &mut files)?;
        // host가 PROJECT.md를 발견할 때 파일 설정이 모두 준비되어 있어야 한다.
        if let Some(i) = files.iter().position(|f| f.path == dir.join("PROJECT.md")) {
            let marker = files.remove(i);
            files.push(marker);
        }
    }
    Ok(Request { plan, files, root, directories })
}

#[cfg(test)]
mod tests {
    use super::super::tests::Lab;
    use super::*;

    fn host(lab: &Lab, mode: InstallProjectMode) -> InstallPlan {
        let mut plan = lab.plan("state");
        let c = &mut plan.choices;
        c.role = Some(InstallRole::Host);
        c.host = None;
        c.enroll = None;
        c.wait_timeout = None;
        c.local_agent = Some(InstallToggle::Enabled);
        c.web = Some(InstallWeb::Off);
        c.tailscale_serve = Some(InstallServe::Off);
        c.pairing = Some(InstallPairing::Skip);
        c.agent_listen = Some("127.0.0.1:17430".into());
        c.project_mode = Some(mode);
        c.project = Some("demo".into());
        c.no_repo = true;
        aaa_config::install::plan(plan.choices, plan.context).unwrap()
    }
    #[test]
    fn project_creation_and_existing_repos_use_the_host_scaffold_without_data_loss() {
        let lab = Lab::new();
        let plan = host(&lab, InstallProjectMode::Create);
        let project = PathBuf::from(&plan.context.projects_root).join("demo");
        let mut tx = Transaction::create(install(plan).unwrap()).unwrap();
        tx.run().unwrap();
        assert!(project.join("PROJECT.md").is_file());
        assert!(project.join("project.toml").is_file());
        assert!(project.join("uploads").is_dir());
        assert!(std::fs::read_to_string(project.join("AGENTS.md")).unwrap().contains("aaa"));
        let cfg = PathBuf::from(&tx.manifest.plan.context.config_dir);
        assert_eq!(aaa_config::load_host(&cfg).unwrap().config.local_agent.name, "mac-a");
        drop(tx);
        // 기존 project의 repo를 no-repo로 지우지 않고 새 repo를 덧붙인다.
        let other = Lab::new();
        let mut plan = host(&other, InstallProjectMode::Use);
        let project = PathBuf::from(&plan.context.projects_root).join("demo");
        private::parents(&project).unwrap();
        private::write(&project.join("PROJECT.md"), b"# existing project\n").unwrap();
        private::write(
            &project.join("project.toml"),
            b"schema = 1\n# keep\n[[repos]]\npath = '/existing-repo'\nmachine = 'mac-a'\n",
        )
        .unwrap();
        let request = install(plan.clone()).unwrap();
        let config = request.files.iter().find(|f| f.path == project.join("project.toml")).unwrap();
        let text = String::from_utf8(config.bytes.clone()).unwrap();
        assert!(text.contains("/existing-repo") && text.contains("# keep"));
        plan.choices.no_repo = false;
        plan.choices.repo = vec![other.0.join("new repo").to_string_lossy().into_owned()];
        plan = aaa_config::install::plan(plan.choices, plan.context).unwrap();
        let request = install(plan).unwrap();
        let config = request.files.iter().find(|f| f.path == project.join("project.toml")).unwrap();
        let text = String::from_utf8(config.bytes.clone()).unwrap();
        assert!(text.contains("/existing-repo") && text.contains("new repo"));
        let unsafe_lab = Lab::new();
        let mut plan = host(&unsafe_lab, InstallProjectMode::Create);
        let cfg = PathBuf::from(&plan.context.config_dir);
        private::parents(&cfg).unwrap();
        private::write(
            &cfg.join("host.toml"),
            b"schema = 1\n[profiles.review]\nagent = 'codex'\ncommand = ['unreviewed-command']\n",
        )
        .unwrap();
        let imported =
            unsafe_lab.put("profile.toml", b"schema = 1\n[profiles.review]\nagent = 'codex'\nmodel = 'reviewed'\n");
        plan.choices.profile_file = vec![imported.to_string_lossy().into_owned()];
        plan = aaa_config::install::plan(plan.choices, plan.context).unwrap();
        let before = super::super::tests::tree(&unsafe_lab.0);
        assert!(install(plan).is_err(), "검토된 import가 기존 command를 물려받으면 거부한다");
        assert_eq!(super::super::tests::tree(&unsafe_lab.0), before);
    }
}

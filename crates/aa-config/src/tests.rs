use super::*;

struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        let p = std::env::temp_dir().join(format!("aa-config-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Dir(p)
    }
    fn write(&self, name: &str, text: &str) -> &Self {
        std::fs::write(self.0.join(name), text).unwrap();
        self
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn missing_files_mean_defaults() {
    let d = Dir::new("defaults");
    let l = load_host(&d.0).unwrap();
    assert_eq!(l.config, HostConfig::default());
    assert!(l.files.is_empty() && l.warnings.is_empty());
}

#[test]
fn examples_are_valid_and_warning_free() {
    let d = Dir::new("examples");
    d.write("host.toml", example("host")).write("agent.toml", example("agent"));
    assert!(load_host(&d.0).unwrap().warnings.is_empty());
    assert!(load_agent(&d.0).unwrap().warnings.is_empty());
}

#[test]
fn bad_value_reports_file_line_col() {
    let d = Dir::new("badvalue");
    d.write("host.toml", "schema = 1\n\n[web]\nlisten = \"0.0.0.0:7431\"\n");
    let e = load_host(&d.0).unwrap_err();
    let err = e.0.iter().find(|d| d.severity == Severity::Error).unwrap();
    assert_eq!((err.line, err.col), (4, 10), "{}", e.render());
    assert!(err.file.ends_with("host.toml"));
    assert!(err.message.contains("web.listen"), "{}", err.message);
}

#[test]
fn type_error_points_at_value() {
    let d = Dir::new("typeerr");
    d.write("agent.toml", "schema = 1\n[reconnect]\nmax_backoff_s = \"soon\"\n");
    let e = load_agent(&d.0).unwrap_err();
    assert_eq!((e.0[0].line, e.0[0].col), (3, 17), "{}", e.render());
}

#[test]
fn unknown_keys_warn_with_location_and_do_not_block() {
    let d = Dir::new("unknown");
    d.write("agent.toml", "schema = 1\n[agent]\nname = \"m\"\nnmae = \"typo\"\n[future]\nx = 1\n");
    let l = load_agent(&d.0).unwrap();
    assert_eq!(l.config.agent.name, "m");
    let keys: Vec<_> = l.warnings.iter().map(|w| (w.line, w.message.clone())).collect();
    assert!(keys.iter().any(|(line, m)| *line == 4 && m.contains("agent.nmae")), "{keys:?}");
    assert!(keys.iter().any(|(_, m)| m.contains("future")), "{keys:?}");
}

#[test]
fn schema_rules() {
    let d = Dir::new("schema");
    d.write("host.toml", "[web]\nlisten = \"127.0.0.1:1\"\n");
    assert!(load_host(&d.0).unwrap_err().render().contains("schema = 1"));
    d.write("host.toml", "schema = 2\n");
    let r = load_host(&d.0).unwrap_err().render();
    assert!(r.contains("새롭다") && r.contains(":1:10:"), "{r}");
    d.write("host.toml", "schema = 0\n");
    assert!(load_host(&d.0).unwrap_err().render().contains("지원하지 않는다"));
}

#[test]
fn local_file_overrides_and_its_errors_point_at_local() {
    let d = Dir::new("local");
    d.write("agent.toml", "schema = 1\n[agent]\nname = \"shared\"\nhost = \"local\"\n")
        .write("agent.local.toml", "schema = 1\n[agent]\nname = \"mac-a\"\n");
    let l = load_agent(&d.0).unwrap();
    assert_eq!((l.config.agent.name.as_str(), l.config.agent.host.as_str()), ("mac-a", "local"));
    assert_eq!(l.files.len(), 2);

    d.write("agent.local.toml", "schema = 1\n[agent]\nhost = \"no-port\"\n");
    let e = load_agent(&d.0).unwrap_err();
    assert!(e.0[0].file.ends_with("agent.local.toml") && e.0[0].line == 3, "{}", e.render());
}

#[test]
fn agent_host_forms() {
    let d = Dir::new("hostforms");
    for (v, ok) in [
        ("local", true),
        ("mac-a.lan:7430", true),
        ("[::1]:7430", true),
        ("h:0", false),
        (":7430", false),
        ("h", false),
    ] {
        d.write("agent.toml", &format!("schema = 1\n[agent]\nhost = \"{v}\"\n"));
        assert_eq!(load_agent(&d.0).is_ok(), ok, "{v}");
    }
}

#[test]
fn profiles_are_checked() {
    let d = Dir::new("profiles");
    d.write(
        "agent.toml",
        "schema = 1\n[profiles.fake]\nharness = \"custom\"\ncommand = [\"/bin/sh\", \"x.sh\"]\n[profiles.c]\nharness = \"claude\"\nmodel = \"opus\"\n",
    );
    let l = load_agent(&d.0).unwrap();
    assert_eq!(l.config.profiles["c"].model, "opus");
    assert_eq!(l.config.profiles["fake"].command, ["/bin/sh", "x.sh"]);
    d.write("agent.toml", "schema = 1\n[profiles.bad]\nharness = \"custom\"\n");
    let e = load_agent(&d.0).unwrap_err();
    assert!(e.render().contains("profiles.bad.command") && e.0[0].line == 2, "{}", e.render());
    d.write("agent.toml", "schema = 1\n[profiles.x]\nharness = \"vim\"\n");
    assert!(load_agent(&d.0).unwrap_err().render().contains("profiles.x.harness"));
}

use super::*;

struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        let p = std::env::temp_dir().join(format!("aaa-config-test-{tag}-{}", std::process::id()));
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

/// `[tui]`: herdr 형식 prefix(하나·배열), 입력기 전환·자모 해석은 사용자 요청대로 기본 켬(결정 20 D12, 끌 수 있다),
/// 예전 이름 `prefix_ascii`도 읽는다
#[test]
fn tui_keys_like_herdr() {
    let d = Dir::new("tui");
    let t = load_host(&d.0).unwrap().config.tui;
    assert_eq!(t.prefix.values(), ["ctrl+b"]);
    assert!(t.switch_ascii() && t.prefix_jamo_keys);

    d.write(
        "host.toml",
        "schema = 1\n[tui]\nprefix = [\"ctrl+a\", \"ctrl+g\"]\nswitch_ascii_input_source_in_prefix = false\n",
    );
    let l = load_host(&d.0).unwrap();
    assert!(l.warnings.is_empty(), "{:?}", l.warnings);
    assert_eq!(l.config.tui.prefix.values(), ["ctrl+a", "ctrl+g"]);
    assert!(l.config.tui.prefix_jamo_keys && !l.config.tui.switch_ascii());

    // 예전 설정(`C-b`, prefix_ascii = false)을 그대로 둔 사용자는 계속 꺼진다
    d.write("host.toml", "schema = 1\n[tui]\nprefix = \"C-b\"\nprefix_ascii = false\n");
    let l = load_host(&d.0).unwrap();
    assert!(l.warnings.is_empty(), "{:?}", l.warnings);
    assert!(!l.config.tui.switch_ascii());
    d.write("host.toml", "schema = 1\n[tui]\nprefix_jamo_keys = false\n");
    let t = load_host(&d.0).unwrap().config.tui;
    assert!(!t.prefix_jamo_keys && t.switch_ascii());
}

/// herdr `[ui] prompt_new_tab_name`·`confirm_close`(기본 켬)과 `[terminal] new_cwd`(기본 follow)
#[test]
fn new_tab_prompt_and_new_cwd_like_herdr() {
    let d = Dir::new("newcwd");
    let c = load_host(&d.0).unwrap().config;
    assert!(c.tui.prompt_new_tab_name && c.tui.confirm_close);
    assert_eq!(c.terminal.new_cwd, "follow");
    // TOML 리터럴 문자열('…')이라 역슬래시가 그대로 값이 된다
    for ok in ["home", "/srv/work", r"C:\work", "D:/w", r"\\nas\share"] {
        d.write(
            "host.toml",
            &format!("schema = 1\n[terminal]\nnew_cwd = '{ok}'\n[tui]\nprompt_new_tab_name = false\n"),
        );
        let l = load_host(&d.0).unwrap();
        assert_eq!(l.config.terminal.new_cwd, ok);
        assert!(!l.config.tui.prompt_new_tab_name);
    }
    // herdr의 "current"(서버 프로세스 위치)와 상대 경로는 받지 않는다
    for bad in ["current", "work", "~/w"] {
        d.write("host.toml", &format!("schema = 1\n[terminal]\nnew_cwd = \"{bad}\"\n"));
        let e = load_host(&d.0).unwrap_err().render();
        assert!(e.contains("terminal.new_cwd") && e.contains("절대 경로"), "{e}");
    }
}

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
fn post_defaults_and_explicit_overrides() {
    let d = Dir::new("post");
    let expected =
        PostSection { burst: 30, burst_secs: 60, hourly: 200, daily: 2000, pair_hourly: 100, mute_after: 20 };
    assert_eq!(load_host(&d.0).unwrap().config.post, expected);
    d.write("host.toml", "schema = 1\n[post]\n");
    assert_eq!(load_host(&d.0).unwrap().config.post, expected);
    d.write("host.toml", "schema = 1\n[post]\nburst = 3\n");
    assert_eq!(load_host(&d.0).unwrap().config.post, PostSection { burst: 3, ..expected });
    d.write(
        "host.toml",
        "schema = 1\n[post]\nburst = 3\nburst_secs = 120\nhourly = 6\ndaily = 30\npair_hourly = 10\nmute_after = 3\n",
    );
    let explicit = PostSection { burst: 3, burst_secs: 120, hourly: 6, daily: 30, pair_hourly: 10, mute_after: 3 };
    assert_eq!(load_host(&d.0).unwrap().config.post, explicit);
    d.write("host.local.toml", "schema = 1\n[post]\nhourly = 50\n");
    assert_eq!(load_host(&d.0).unwrap().config.post, PostSection { hourly: 50, ..explicit });
}

#[test]
fn examples_are_valid_and_warning_free() {
    let d = Dir::new("examples");
    d.write("host.toml", example("host")).write("agent.toml", example("agent"));
    assert!(load_host(&d.0).unwrap().warnings.is_empty());
    assert!(load_agent(&d.0).unwrap().warnings.is_empty());
}

/// host.toml profile env 중 이름에 KEY·TOKEN·SECRET·PASSWORD가 든 평문 값은 그 자리(local 우선)를 경고한다. 경고와 Debug에는
/// 값이 없다(plan §8.307)
#[test]
fn plaintext_secret_env_warns_by_name_only() {
    let value = "aaa-canary-config-0123456789abcdef";
    let d = Dir::new("secretenv");
    d.write(
        "host.toml",
        &format!(
            "schema = 1\n[profiles.api]\nagent = \"claude\"\nenv = {{ ANTHROPIC_API_KEY = {value:?}, LANG = \"C\", EMPTY_TOKEN = \"\" }}\n"
        ),
    );
    d.write(
        "host.local.toml",
        &format!("schema = 1\n[profiles.api.env]\ngh_token = {value:?}\nDb_Password = {value:?}\n"),
    );
    let l = load_host(&d.0).unwrap();
    let w: Vec<String> = l.warnings.iter().map(|w| w.to_string()).collect();
    assert_eq!(w.len(), 3, "{w:?}");
    for (name, file, line) in [
        ("ANTHROPIC_API_KEY", "host.toml", 4),
        ("gh_token", "host.local.toml", 3),
        ("Db_Password", "host.local.toml", 4),
    ] {
        let d = l.warnings.iter().find(|d| d.message.contains(&format!("env.{name}`"))).expect(name);
        assert_eq!((d.file.file_name().unwrap().to_str().unwrap(), d.line), (file, line), "{d}");
    }
    for text in w.iter().cloned().chain([format!("{:?}", l.config)]) {
        assert!(!text.contains(value) && !text.contains("0123456789"), "{text}");
    }
    assert!(
        format!("{:?}", l.config.profiles["api"])
            .contains("«env 5개: ANTHROPIC_API_KEY, Db_Password, EMPTY_TOKEN, LANG, gh_token»")
    );
}

#[test]
fn work_root_settings_preserve_local_override_and_reject_unsafe_paths() {
    let d = Dir::new("workroot");
    d.write("agent.toml", "schema = 1\n[agent]\nwork_root = '/work/base'\n");
    d.write("agent.local.toml", "schema = 1\n[agent]\nwork_root = '~/external/work'\nwork_mount = '/external'\n");
    let loaded = load_agent(&d.0).unwrap();
    assert!(loaded.warnings.is_empty());
    let home = std::env::temp_dir();
    assert_eq!(work_root::resolve(&loaded.config.agent.work_root, &home).unwrap(), home.join("external/work"));
    assert_eq!(work_root::resolve("", &home).unwrap(), home.join("aaa-work"));
    for value in ["relative/work", "~/../work", "/work/../other", "/", "C:/"] {
        d.write("agent.local.toml", &format!("schema = 1\n[agent]\nwork_root = {value:?}\n"));
        assert!(load_agent(&d.0).unwrap_err().render().contains("agent.work_root"), "{value}");
    }
    // 타 OS 설정 문법은 읽을 수 있어도 이 머신에서 실행할 때는 native 절대 경로여야 한다.
    assert!(work_root::validate(r"D:\worktrees", false).is_ok());
    assert!(work_root::validate("/", true).is_ok());
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
    assert!(l.warnings.iter().any(|w| w.message.contains("host.toml [profiles]로 옮기라")));
    assert_eq!(l.config.profiles["fake"].command, ["/bin/sh", "x.sh"]);
    d.write("agent.toml", "schema = 1\n[profiles.bad]\nharness = \"custom\"\n");
    let e = load_agent(&d.0).unwrap_err();
    assert!(e.render().contains("profiles.bad.command") && e.0[0].line == 2, "{}", e.render());
    d.write("agent.toml", "schema = 1\n[profiles.x]\nharness = \"vim\"\n");
    assert!(load_agent(&d.0).unwrap_err().render().contains("profiles.x.harness"));
}

/// herdr `[ui] prompt_new_tab_name`·`confirm_close`(기본 켬)과 `[terminal] new_cwd`(기본 follow)
#[test]
fn new_cwd_like_herdr() {
    let d = Dir::new("newcwd");
    let c = load_host(&d.0).unwrap().config;
    assert_eq!(c.terminal.new_cwd, "follow");
    // TOML 리터럴 문자열('…')이라 역슬래시가 그대로 값이 된다
    for ok in ["home", "/srv/work", r"C:\work", "D:/w", r"\\nas\share"] {
        d.write("host.toml", &format!("schema = 1\n[terminal]\nnew_cwd = '{ok}'\n"));
        let l = load_host(&d.0).unwrap();
        assert_eq!(l.config.terminal.new_cwd, ok);
    }
    // herdr의 "current"(서버 프로세스 위치)와 상대 경로는 받지 않는다
    for bad in ["current", "work", "~/w"] {
        d.write("host.toml", &format!("schema = 1\n[terminal]\nnew_cwd = \"{bad}\"\n"));
        let e = load_host(&d.0).unwrap_err().render();
        assert!(e.contains("terminal.new_cwd") && e.contains("절대 경로"), "{e}");
    }
}

/// host의 profile schema는 참고 이름·model·effort 검증을 쓰고 local 파일도 같은 정본이다.
#[test]
fn host_profiles_and_machine_overrides() {
    let d = Dir::new("host-profiles");
    d.write("host.toml", "schema = 1\n[profiles.\"review.fast\"]\nagent = \"codex\"\nmodel = \"example-model\"\neffort = \"high\"\nargs = [\"--search\"]\ndescription = \"검토\"\n");
    d.write("host.local.toml", "schema = 1\n[profiles.\"review.fast\"]\neffort = \"low\"\n");
    let h = load_host(&d.0).unwrap();
    assert!(h.warnings.is_empty());
    let p = &h.config.profiles["review.fast"];
    assert_eq!((&*p.harness, &*p.model, &*p.effort), ("codex", "example-model", "low"));
    d.write(
        "agent.toml",
        "schema = 1\n[harnesses.codex]\ncommand = [\"/opt/tools/codex\"]\nenv = { EXAMPLE = \"local\" }\n",
    );
    let a = load_agent(&d.0).unwrap();
    assert!(a.warnings.is_empty() && a.config.profiles.is_empty());
    assert_eq!(a.config.harnesses["codex"].command, ["/opt/tools/codex"]);
    for bad in [
        "[profiles.\"-bad\"]\nagent = \"codex\"",
        "[profiles.bad]\nagent = \"codex\"\nmodel = \"--flag\"",
        "[profiles.bad]\nagent = \"claude\"\neffort = \"ultra\"",
    ] {
        d.write("host.toml", &format!("schema = 1\n{bad}\n"));
        assert!(load_host(&d.0).is_err(), "{bad}");
    }
}

#[test]
fn library_retention_config_defaults_disable_and_reject_bad_values() {
    let d = Dir::new("library-retention");
    let defaults = load_host(&d.0).unwrap().config.projects;
    assert_eq!((defaults.library_max_days, defaults.library_max_mb), (30, 20480));
    for value in ["0", "\"\""] {
        d.write(
            "host.toml",
            &format!("schema = 1\n[projects]\nlibrary_max_days = {value}\nlibrary_max_mb = {value}\n"),
        );
        let c = load_host(&d.0).unwrap().config.projects;
        assert_eq!((c.library_max_days, c.library_max_mb), (0, 0));
    }
    for key in ["library_max_days", "library_max_mb"] {
        for value in ["-1", "1.5", "true", "\"soon\"", "9223372036854775807"] {
            d.write("host.toml", &format!("schema = 1\n[projects]\n{key} = {value}\n"));
            let error = load_host(&d.0).unwrap_err();
            assert!(error.0.iter().any(|d| d.line == 3 && d.severity == Severity::Error), "{}", error.render());
        }
    }
}

/// 옛 설정은 10분 기본값을 쓰고, host.local.toml·검증·진단 위치도 기존 설정과 같다.
#[test]
fn agent_operation_timeouts_are_host_settings() {
    let d = Dir::new("operation-timeouts");
    let c = load_host(&d.0).unwrap().config;
    assert_eq!((c.host.thread_prepare_timeout_secs, c.host.thread_cleanup_timeout_secs), (600, 600));
    assert_eq!(c.host.file_request_timeout_secs, 120);
    d.write("host.toml", "schema = 1\n[host]\nthread_prepare_timeout_secs = 900\nthread_cleanup_timeout_secs = 1200\nfile_request_timeout_secs = 180\n");
    d.write("host.local.toml", "schema = 1\n[host]\nthread_prepare_timeout_secs = 1800\n");
    let l = load_host(&d.0).unwrap();
    assert!(l.warnings.is_empty(), "{:?}", l.warnings);
    assert_eq!(
        (
            l.config.host.thread_prepare_timeout_secs,
            l.config.host.thread_cleanup_timeout_secs,
            l.config.host.file_request_timeout_secs
        ),
        (1800, 1200, 180)
    );
    for key in ["thread_prepare_timeout_secs", "thread_cleanup_timeout_secs", "file_request_timeout_secs"] {
        for bad in ["0", "86401", "-1", "\"600\""] {
            d.write("host.local.toml", &format!("schema = 1\n[host]\n{key} = {bad}\n"));
            let e = load_host(&d.0).unwrap_err();
            if matches!(bad, "0" | "86401") {
                assert!(e.render().contains(key), "{}", e.render());
            }
            assert!(e.0.iter().any(|d| d.file.ends_with("host.local.toml") && d.line == 3), "{}", e.render());
        }
    }
}

/// `[safety]` 표 하나의 값(plan §8.288)과 아래 표(`default`·프로젝트)가 같은 표에 섞여도 읽고, local이 덮으며, 틀린 값은 그 줄을 가리킨다
#[test]
fn one_safety_table_mixes_values_and_tables() {
    let d = Dir::new("safety-one");
    d.write(
        "host.toml",
        "schema = 1\n\n[safety]\nyolo = true\n\n[safety.default]\nroutine_commands = true\n\n[safety.demo]\nthread_profiles = [\"opus\"]\n",
    )
    .write("host.local.toml", "schema = 1\n\n[safety]\ntrust_screens = \"user\"\n");
    let c = load_host(&d.0).unwrap().config;
    let one = &c.safety[safety::ONE_TABLE];
    assert_eq!((one.yolo, one.trust_screens.as_deref()), (Some(true), Some("user")));
    assert_eq!(c.safety["default"].routine_commands, Some(true));
    assert_eq!(c.safety["demo"].thread_profiles, Some(vec!["opus".to_string()]));
    // 다시 쓰면 같은 모양이다(값은 [safety]에, 표는 그 아래)
    let back: HostConfig = toml_edit::de::from_str(&toml_edit::ser::to_string(&c).unwrap()).unwrap();
    assert_eq!(back.safety, c.safety);
    d.write("host.local.toml", "schema = 1\n\n[safety]\nstart_threads = \"later\"\n");
    let e = load_host(&d.0).unwrap_err();
    let err = e.0.iter().find(|d| d.severity == Severity::Error).unwrap();
    assert!(err.file.ends_with("host.local.toml") && err.line == 4, "{}", e.render());
    assert!(err.message.contains("safety.start_threads"), "{}", err.message);
}

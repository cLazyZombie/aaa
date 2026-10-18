//! profile 고치기와 검증(herdr-projects 0.2.35 `src/profiles.rs` `validate_name`·`effort_values`·`typed_args`·`Change`·
//! `apply_in`·`detect`, plan §8.153).
//!
//! - profile은 머신마다 agent.toml `[profiles.<이름>]`에 둔다(§8.2). `aaa profile add|edit|remove`는 그 머신의 agent.toml을
//!   사람이 터미널에서 고칠 때만 쓴다. agent는 profile을 쓸 때마다 agent.toml을 다시 읽는다.
//! - claude·codex는 같은 이름의 기본 profile이다(인자 없음). 같은 이름의 `[profiles.*]`가 있으면 그것이 대신한다. 목록에는
//!   그 CLI가 설치돼 있고 로그인한 것으로 보일 때만 나오지만 이름으로는 언제나 고를 수 있다.

use std::path::{Path, PathBuf};

use toml_edit::{Array, DocumentMut, Item, Table};

use crate::{AgentConfig, Profile};

/// 기본 profile(harness 이름과 같다)
pub const BUILTINS: [&str; 2] = ["claude", "codex"];

/// 기본 profile `name`(없으면 `None`)
pub fn builtin(name: &str) -> Option<Profile> {
    BUILTINS.contains(&name).then(|| Profile { harness: name.to_string(), ..Profile::default() })
}

/// profile 이름: 영문·숫자·`.`·`_`·`-`, 40자까지, `-`·`.`로 시작하지 않는다(참고 `validate_name`)
pub fn validate_name(name: &str) -> Result<(), String> {
    let ok = !name.is_empty()
        && name.len() <= 40
        && !name.starts_with(['-', '.'])
        && name.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c));
    if ok {
        Ok(())
    } else {
        Err(format!("`{name}`는 profile 이름이 아니다: 영문·숫자·`.`·`_`·`-`로 40자까지 쓴다"))
    }
}

/// harness의 effort 플래그가 받는 값(참고 `effort_values`, 2026-09-25 확인). shell·custom은 effort 플래그가 없다.
pub fn effort_values(harness: &str) -> Option<&'static [&'static str]> {
    match harness {
        "claude" => Some(&["low", "medium", "high", "xhigh", "max"]),
        "codex" => Some(&["none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra"]),
        _ => None,
    }
}

/// model 이름처럼 보이는가: `opus`·`gpt-5.5`·`claude-opus-5-5[1m]`. 플래그는 아니다(참고 `is_model_name`).
pub fn is_model_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !value.starts_with('-')
        && value.chars().all(|c| c.is_ascii_alphanumeric() || "._-/:@+[]".contains(c))
}

/// profile 하나가 맞는가: 이름, harness, model, effort(그 harness가 받는 값)
pub fn validate_profile(name: &str, p: &Profile) -> Result<(), String> {
    validate_name(name)?;
    match p.harness.as_str() {
        "claude" | "codex" | "shell" => {}
        "custom" if !p.command.is_empty() => {}
        "custom" => return Err("custom harness는 command(실행할 argv)가 있어야 한다".into()),
        other => return Err(format!("harness는 claude·codex·shell·custom 중 하나다: {other:?}")),
    }
    if !p.model.is_empty() {
        if !matches!(p.harness.as_str(), "claude" | "codex") {
            return Err(format!("{}는 model 플래그가 없다: 비우고 args에 적는다", p.harness));
        }
        if !is_model_name(&p.model) {
            return Err(format!("`{}`는 model 이름으로 보이지 않는다", p.model));
        }
    }
    if !p.effort.is_empty() {
        let Some(values) = effort_values(&p.harness) else {
            return Err(format!("{}는 effort 플래그가 없다: 비우고 args에 적는다", p.harness));
        };
        if !values.contains(&p.effort.as_str()) {
            return Err(format!("effort `{}`는 {}의 값({})이 아니다", p.effort, p.harness, values.join(", ")));
        }
    }
    Ok(())
}

/// profile의 model·effort 플래그와 제 인자(참고 `Profile::args`·`typed_args`): claude `--model`·`--effort`, codex `-m`·
/// `-c model_reasoning_effort=`. shell·custom은 제 인자만. agent가 thread를 띄울 때와 `aaa profile resolve`가 같이 쓴다.
pub fn launch_flags(p: &Profile) -> Vec<String> {
    let mut flags = Vec::new();
    match p.harness.as_str() {
        "claude" => {
            if !p.model.is_empty() {
                flags.extend(["--model".to_string(), p.model.clone()]);
            }
            if !p.effort.is_empty() {
                flags.extend(["--effort".to_string(), p.effort.clone()]);
            }
        }
        "codex" => {
            if !p.model.is_empty() {
                flags.extend(["-m".to_string(), p.model.clone()]);
            }
            if !p.effort.is_empty() {
                flags.extend(["-c".to_string(), format!("model_reasoning_effort={}", p.effort)]);
            }
        }
        _ => {}
    }
    flags.extend(p.args.iter().cloned());
    flags
}

/// 한 줄 요약(`profile list`): `codex · model gpt-5.5 · effort high · args: --x — 설명`(참고 `summary`)
pub fn summary(p: &Profile) -> String {
    let mut parts =
        vec![p.harness.clone(), if p.model.is_empty() { "기본 model".into() } else { format!("model {}", p.model) }];
    if !p.effort.is_empty() {
        parts.push(format!("effort {}", p.effort));
    }
    if !p.args.is_empty() {
        parts.push(format!("args: {}", p.args.join(" ")));
    }
    let mut line = parts.join(" · ");
    if !p.description.is_empty() {
        line.push_str(" — ");
        line.push_str(&p.description);
    }
    line
}

/// agent.toml profile 고치기. 사람만 한다(참고 `Change`의 profile 쪽, 허용 목록·기본값은 host.toml이라 [`crate::safety`])
#[derive(Debug, Clone, PartialEq)]
pub enum ProfileChange {
    Add {
        name: String,
        profile: Profile,
    },
    Edit {
        name: String,
        harness: Option<String>,
        model: Option<String>,
        effort: Option<String>,
        description: Option<String>,
        args: Option<Vec<String>>,
    },
    Remove {
        name: String,
    },
}

fn string_array(items: &[String]) -> Item {
    let mut a = Array::new();
    for s in items {
        a.push(s.as_str());
    }
    toml_edit::value(a)
}

fn write_fields(t: &mut Table, p: &Profile) {
    t["harness"] = toml_edit::value(&p.harness);
    for (key, value) in [("model", &p.model), ("effort", &p.effort), ("description", &p.description)] {
        if value.is_empty() {
            t.remove(key);
        } else {
            t[key] = toml_edit::value(value);
        }
    }
    if p.args.is_empty() {
        t.remove("args");
    } else {
        t["args"] = string_array(&p.args);
    }
}

fn profiles_of(text: &str) -> Result<std::collections::BTreeMap<String, Profile>, String> {
    if text.trim().is_empty() {
        return Ok(Default::default());
    }
    toml_edit::de::from_str::<AgentConfig>(text)
        .map(|c| c.profiles)
        .map_err(|e| format!("agent.toml을 읽지 못했다: {e}"))
}

/// agent.toml 글에 `change`를 적용한다. 나머지(주석·차례)는 그대로 두고, 고친 글이 다시 읽히는지 본다(참고 `apply_in`).
/// 새 글과 알릴 말을 돌려준다. 빈 파일이면 `schema = 1`부터 쓴다.
pub fn apply_in(text: &str, change: &ProfileChange) -> Result<(String, String), String> {
    let base = if text.trim().is_empty() { format!("schema = {}\n", crate::SCHEMA) } else { text.to_string() };
    let mut doc = base.parse::<DocumentMut>().map_err(|e| format!("agent.toml을 읽지 못했다: {e}"))?;
    let current = profiles_of(text)?;
    let message = match change {
        ProfileChange::Add { name, profile } => {
            validate_profile(name, profile)?;
            if current.contains_key(name) {
                return Err(format!("profile `{name}`가 이미 있다: `aaa profile edit {name}`로 고친다"));
            }
            if !doc.contains_key("profiles") {
                let mut t = Table::new();
                t.set_implicit(true);
                doc["profiles"] = Item::Table(t);
            }
            let mut t = Table::new();
            write_fields(&mut t, profile);
            doc["profiles"].as_table_mut().ok_or("agent.toml의 `profiles`가 표가 아니다")?[name.as_str()] =
                Item::Table(t);
            let replaces =
                if builtin(name).is_some() { format!("(기본 `{name}`를 대신한다)") } else { String::new() };
            format!("profile `{name}`를 더했다{replaces}: {}", summary(profile))
        }
        ProfileChange::Edit { name, harness, model, effort, description, args } => {
            let mut p = current.get(name).cloned().ok_or_else(|| {
                format!(
                    "agent.toml에 profile `{name}`가 없다{}: `aaa profile add`로 만든다",
                    if builtin(name).is_some() {
                        "(기본 profile은 같은 이름으로 add하면 대신한다)"
                    } else {
                        ""
                    }
                )
            })?;
            if let Some(h) = harness {
                if *h != p.harness
                    && effort.is_none()
                    && !effort_values(h).is_some_and(|v| v.contains(&p.effort.as_str()))
                {
                    // 다른 harness: 받지 못하는 effort는 버린다
                    p.effort.clear();
                }
                if *h != p.harness && model.is_none() && !matches!(h.as_str(), "claude" | "codex") {
                    p.model.clear();
                }
                p.harness = h.clone();
            }
            for (field, value) in [(&mut p.model, model), (&mut p.effort, effort), (&mut p.description, description)] {
                if let Some(v) = value {
                    *field = v.clone();
                }
            }
            if let Some(a) = args {
                p.args = a.clone();
            }
            validate_profile(name, &p)?;
            let t = doc["profiles"][name.as_str()]
                .as_table_mut()
                .ok_or_else(|| format!("[profiles.{name}]가 표가 아니다"))?;
            write_fields(t, &p);
            format!("profile `{name}`를 고쳤다: {}", summary(&p))
        }
        ProfileChange::Remove { name } => {
            if !current.contains_key(name) {
                return Err(format!(
                    "agent.toml에 profile `{name}`가 없다{}",
                    if builtin(name).is_some() { "(기본 profile은 지울 수 없다)" } else { "" }
                ));
            }
            if let Some(t) = doc.get_mut("profiles").and_then(Item::as_table_mut) {
                t.remove(name);
            }
            let back =
                if builtin(name).is_some() { format!(": 기본 `{name}`가 돌아온다") } else { String::new() };
            format!("profile `{name}`를 지웠다{back}")
        }
    };
    let out = doc.to_string();
    for (name, p) in profiles_of(&out)? {
        validate_profile(&name, &p).map_err(|e| format!("[profiles.{name}]: {e}"))?;
    }
    Ok((out, message))
}

/// 설정 폴더의 agent.toml에 적용하고 바꿔 쓴다(임시 파일 뒤 rename). 사람이 시켰는지는 부르는 쪽이 본다.
pub fn apply(config_dir: &Path, change: &ProfileChange) -> Result<String, String> {
    let file = config_dir.join("agent.toml");
    let text = std::fs::read_to_string(&file).unwrap_or_default();
    let (edited, message) = apply_in(&text, change)?;
    write_atomic(&file, edited.as_bytes()).map_err(|e| format!("{}에 쓰지 못했다: {e}", file.display()))?;
    Ok(message)
}

/// 임시 파일에 쓰고 rename한다(반쯤 쓴 설정을 읽지 않게)
pub fn write_atomic(file: &Path, data: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = file.with_extension(format!("toml.tmp-{}", std::process::id()));
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, file)
}

/// harness CLI가 PATH에 있고 로그인한 것으로 보이는 기본 profile(참고 `detect`·`signed_in`: 파일과 환경 변수만 본다,
/// 네트워크·키체인·CLI 실행 없이)
pub fn detect(path: Option<&str>, home: &Path, var: impl Fn(&str) -> Option<String>) -> Vec<String> {
    let on_path = |name: &str| {
        path.unwrap_or("").split(':').filter(|d| !d.is_empty()).any(|d| is_executable(&Path::new(d).join(name)))
    };
    let any_var = |names: &[&str]| names.iter().any(|n| var(n).is_some_and(|v| !v.is_empty()));
    let dir_of = |v: &str, fallback: &str| var(v).map(PathBuf::from).unwrap_or_else(|| home.join(fallback));
    let file_has = |p: PathBuf, needle: &str| std::fs::read_to_string(p).is_ok_and(|t| t.contains(needle));
    let signed_in = |kind: &str| match kind {
        "claude" => {
            let config = dir_of("CLAUDE_CONFIG_DIR", ".claude");
            any_var(&[
                "ANTHROPIC_API_KEY",
                "CLAUDE_CODE_OAUTH_TOKEN",
                "CLAUDE_CODE_USE_BEDROCK",
                "CLAUDE_CODE_USE_VERTEX",
            ]) || config.join(".credentials.json").is_file()
                || file_has(home.join(".claude.json"), "\"oauthAccount\"")
                || file_has(config.join(".claude.json"), "\"oauthAccount\"")
        }
        "codex" => {
            any_var(&["OPENAI_API_KEY", "CODEX_ACCESS_TOKEN"])
                || dir_of("CODEX_HOME", ".codex").join("auth.json").is_file()
        }
        _ => false,
    };
    BUILTINS.iter().filter(|k| on_path(k) && signed_in(k)).map(|k| k.to_string()).collect()
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(p: &Path) -> bool {
    p.with_extension("exe").is_file() || p.with_extension("cmd").is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    const AGENT: &str = "schema = 1 # 내 머신\n\n[agent]\nname = \"mac-a\"\n\n[profiles.deep]\nharness = \"codex\"\nmodel = \"gpt-5.5\"\neffort = \"minimal\"\n";

    #[test]
    fn names_models_and_efforts_follow_the_reference() {
        for ok in ["opus", "a.b_c-1", &"x".repeat(40)] {
            assert!(validate_name(ok).is_ok(), "{ok}");
        }
        for bad in ["", "-x", ".x", "a b", "한글", &"x".repeat(41)] {
            assert!(validate_name(bad).is_err(), "{bad}");
        }
        assert!(is_model_name("claude-opus-5-5[1m]") && is_model_name("anthropic/claude-sonnet-5"));
        assert!(!is_model_name("--dangerously-skip-permissions") && !is_model_name("a b"));
        let p = |h: &str, m: &str, e: &str| Profile {
            harness: h.into(),
            model: m.into(),
            effort: e.into(),
            ..Profile::default()
        };
        assert!(validate_profile("x", &p("claude", "opus", "xhigh")).is_ok());
        assert!(validate_profile("x", &p("codex", "", "ultra")).is_ok());
        assert!(validate_profile("x", &p("claude", "", "ultra")).unwrap_err().contains("low, medium"));
        assert!(validate_profile("x", &p("shell", "", "high")).unwrap_err().contains("effort 플래그가 없다"));
        assert!(validate_profile("x", &p("claude", "--yolo", "")).unwrap_err().contains("model 이름"));
        assert!(validate_profile("x", &p("gemini", "", "")).is_err());
    }

    #[test]
    fn add_edit_remove_keep_the_rest_of_the_file() {
        let add = ProfileChange::Add {
            name: "claude".into(),
            profile: Profile {
                harness: "claude".into(),
                model: "opus".into(),
                args: s(&["--verbose"]),
                description: "깊게".into(),
                ..Profile::default()
            },
        };
        let (t, msg) = apply_in(AGENT, &add).unwrap();
        assert!(msg.contains("기본 `claude`를 대신한다"), "{msg}");
        assert!(t.starts_with("schema = 1 # 내 머신\n"), "{t}");
        assert!(t.contains("[profiles.claude]\nharness = \"claude\"\nmodel = \"opus\"\ndescription = \"깊게\"\nargs = [\"--verbose\"]\n"), "{t}");
        assert!(apply_in(&t, &add).unwrap_err().contains("이미 있다"));
        // harness를 바꾸면 받지 못하는 effort는 버린다
        let edit = |harness: Option<&str>, effort: Option<&str>| ProfileChange::Edit {
            name: "deep".into(),
            harness: harness.map(Into::into),
            model: None,
            effort: effort.map(Into::into),
            description: None,
            args: None,
        };
        let (t2, _) = apply_in(&t, &edit(Some("claude"), None)).unwrap();
        assert!(
            t2.contains("[profiles.deep]\nharness = \"claude\"\nmodel = \"gpt-5.5\"\n") && !t2.contains("effort"),
            "{t2}"
        );
        assert!(apply_in(&t, &edit(None, Some("turbo"))).unwrap_err().contains("effort `turbo`"));
        let (t3, msg) = apply_in(&t2, &ProfileChange::Remove { name: "claude".into() }).unwrap();
        assert!(msg.contains("기본 `claude`가 돌아온다") && !t3.contains("[profiles.claude]"), "{t3}");
        assert!(
            apply_in(&t3, &ProfileChange::Remove { name: "codex".into() })
                .unwrap_err()
                .contains("기본 profile은 지울 수 없다")
        );
        // 빈 파일이면 schema부터
        let (t4, _) = apply_in(
            "",
            &ProfileChange::Add {
                name: "sh".into(),
                profile: Profile { harness: "shell".into(), ..Profile::default() },
            },
        )
        .unwrap();
        assert!(t4.starts_with("schema = 1\n") && t4.contains("[profiles.sh]\nharness = \"shell\"\n"), "{t4}");
    }

    #[test]
    fn builtins_are_listed_when_installed_and_signed_in() {
        let tmp = std::env::temp_dir().join(format!("aaa-detect-{}", std::process::id()));
        let (bin, home) = (tmp.join("bin"), tmp.join("home"));
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(home.join(".codex")).unwrap();
        for exe in ["claude", "codex"] {
            std::fs::write(bin.join(exe), "#!/bin/sh\n").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                std::fs::set_permissions(bin.join(exe), std::fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        let path = bin.to_string_lossy().to_string();
        let none = |_: &str| None;
        assert!(detect(Some(&path), &home, none).is_empty(), "설치만 하고 로그인하지 않았다");
        std::fs::write(home.join(".codex/auth.json"), "{}").unwrap();
        assert_eq!(detect(Some(&path), &home, none), s(&["codex"]));
        let key = |n: &str| (n == "ANTHROPIC_API_KEY").then(|| "x".to_string());
        assert_eq!(detect(Some(&path), &home, key), s(&["claude", "codex"]));
        assert!(detect(Some("/nonexistent"), &home, key).is_empty(), "PATH에 없다");
        std::fs::remove_dir_all(&tmp).unwrap();
    }
}

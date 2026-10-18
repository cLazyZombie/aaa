//! 프로젝트 설정(`projects/<p>/project.toml`). herdr-projects 0.2.35 `project.rs` `Settings`(PROJECT.md front matter)와
//! `Safety`의 프로젝트 값 중 aaa가 쓰는 것을 한곳에서 읽는다(plan §8.77).
//!
//! - 값이 잘못된 key는 그 key만 기본값을 쓴다. 파일을 TOML로 읽지 못하면 모두 기본값이고, inbox `config-error`가 알린다.
//! - `aaa project create`의 옵션과 `aaa project set`(herdr-projects `new`·`set`, 4장 G)이 host를 거쳐 이 파일을 쓴다
//!   ([`set_in`], toml_edit라 다른 줄·주석은 그대로다). coordinator·사람이 직접 고쳐도 된다.
//! - `start_threads`·`trust_screens`는 옛 자리다: host.toml `[safety.*]`(사람만 고친다, plan §8.153)에 없을 때만 쓰고,
//!   `aaa safety show`가 출처를 「project.toml(프로젝트 폴더 안)」으로 보인다.

use std::path::Path;

/// `[[repos]]`: 프로젝트가 다루는 저장소(herdr-projects `Repo`). `machine`이 없으면 host 머신이다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Repo {
    pub path: String,
    pub machine: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProjectSettings {
    /// 프로젝트가 무엇을 위한 것인가(brief 머리에 들어간다)
    pub goal: String,
    pub repos: Vec<Repo>,
    /// `thread start`에 `--profile`이 없을 때 쓰는 profile(herdr-projects `thread_profile`).
    /// herdr-projects는 기본이 `claude`(내장 profile)지만 aaa profile은 머신 agent.toml에만 있어 기본은 비어 있다.
    pub thread_profile: String,
    /// `aaa project open`이 coordinator를 띄우는 profile(herdr-projects `coordinator_profile`). 비면 `claude`
    pub coordinator_profile: String,
    /// 열린 thread가 이만큼이면 `thread start`가 경고한다. 거부하지 않는다(coordinator가 지키는 규칙).
    pub max_parallel_threads: u32,
    /// `Idle`로 이만큼(일) 지난 thread를 마지막 사본 뒤 resolve한다. 0이면 하지 않는다.
    pub auto_resolve_days: u32,
    /// 옛 자리 `start_threads`(propose·auto, 잘못된 값은 없는 것)
    pub start_threads: Option<String>,
    /// 옛 자리 `trust_screens`(user·coordinator, 잘못된 값은 없는 것)
    pub trust_screens: Option<String>,
}

impl Default for ProjectSettings {
    fn default() -> Self {
        ProjectSettings {
            goal: String::new(),
            repos: Vec::new(),
            thread_profile: String::new(),
            coordinator_profile: String::new(),
            max_parallel_threads: 10,
            auto_resolve_days: 7,
            start_threads: None,
            trust_screens: None,
        }
    }
}

impl ProjectSettings {
    pub(crate) fn load(dir: &Path) -> ProjectSettings {
        let table = std::fs::read_to_string(dir.join("project.toml"))
            .ok()
            .and_then(|t| t.parse::<toml::Table>().ok())
            .unwrap_or_default();
        Self::from_table(&table)
    }

    fn from_table(t: &toml::Table) -> ProjectSettings {
        let d = ProjectSettings::default();
        let s = |k: &str| t.get(k).and_then(|v| v.as_str()).map(str::to_string);
        let n = |k: &str, def: u32| {
            t.get(k).and_then(|v| v.as_integer()).and_then(|i| u32::try_from(i).ok()).unwrap_or(def)
        };
        let start_threads = s("start_threads").filter(|v| matches!(v.as_str(), "propose" | "auto"));
        let trust_screens = s("trust_screens")
            .filter(|v| matches!(v.as_str(), crate::trust_screen::USER | crate::trust_screen::COORDINATOR));
        let repos = t
            .get("repos")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|r| r.as_table())
                    .filter_map(|r| {
                        let path = r.get("path")?.as_str()?.trim().to_string();
                        let machine = r.get("machine").and_then(|m| m.as_str()).map(str::to_string);
                        (!path.is_empty()).then_some(Repo { path, machine })
                    })
                    .collect()
            })
            .unwrap_or_default();
        ProjectSettings {
            goal: s("goal").unwrap_or_default(),
            repos,
            thread_profile: s("thread_profile").unwrap_or_default(),
            coordinator_profile: s("coordinator_profile").unwrap_or_default(),
            max_parallel_threads: n("max_parallel_threads", d.max_parallel_threads),
            auto_resolve_days: n("auto_resolve_days", d.auto_resolve_days),
            start_threads,
            trust_screens,
        }
    }
}

/// `aaa project set`이 받는 key와 값(herdr-projects `settings::KEYS`에서 aaa가 읽는 것만: 이름은 폴더 이름이라
/// `project rename`, nudge는 host.toml `[projects]`, mute는 아직 없다)
pub(crate) const KEYS: [(&str, &str); 7] = [
    ("goal", "글"),
    ("thread_profile", "profile 이름"),
    ("coordinator_profile", "profile 이름"),
    ("max_parallel_threads", "1~1000"),
    ("auto_resolve_days", "0~1000(0 = 하지 않음)"),
    ("repos.add", "PATH 또는 PATH@MACHINE"),
    ("repos.remove", "PATH 또는 PATH@MACHINE"),
];

/// `PATH[@MACHINE]`(herdr-projects `parse_repo_arg`): `@` 뒤가 머신 이름 꼴일 때만 머신으로 본다
pub(crate) fn parse_repo_arg(arg: &str) -> Repo {
    if let Some((path, machine)) = arg.rsplit_once('@') {
        let label_like =
            !machine.is_empty() && machine.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
        if label_like && !path.is_empty() {
            return Repo { path: path.to_string(), machine: Some(machine.to_string()) };
        }
    }
    Repo { path: arg.to_string(), machine: None }
}

/// `repos = [...]`(인라인 표)를 `[[repos]]`로 바꿔 더하고 뺄 수 있게 한다(herdr-projects `normalize_repos`)
fn normalize_repos(doc: &mut toml_edit::DocumentMut) -> Result<(), String> {
    use toml_edit::{ArrayOfTables, Item};
    let tables = match doc.get("repos") {
        None => Some(ArrayOfTables::new()),
        Some(Item::ArrayOfTables(_)) => None,
        Some(Item::Value(toml_edit::Value::Array(array))) => {
            let mut tables = ArrayOfTables::new();
            for v in array.iter() {
                tables.push(v.as_inline_table().ok_or("`repos`의 항목은 표여야 한다")?.clone().into_table());
            }
            Some(tables)
        }
        Some(_) => return Err("`repos`는 표 목록이어야 한다".into()),
    };
    if let Some(tables) = tables {
        // 표 목록은 일반 key 뒤에 와야 한다: 끝에 다시 넣는다
        doc.remove("repos");
        doc["repos"] = Item::ArrayOfTables(tables);
    }
    Ok(())
}

/// project.toml 글에 값 하나를 바꾼다(herdr-projects `set_in`). 로컬 저장소 경로는 부르는 쪽이 정본 경로로 바꿔 준다.
pub(crate) fn set_in(text: &str, key: &str, value: &str) -> Result<String, String> {
    use toml_edit::{Item, Table};
    let mut doc =
        text.parse::<toml_edit::DocumentMut>().map_err(|e| format!("project.toml을 읽지 못한다: {}", e.message()))?;
    let value = value.trim();
    let number = |lo: i64| -> Result<i64, String> {
        value
            .parse()
            .ok()
            .filter(|n| (lo..=1000).contains(n))
            .ok_or_else(|| format!("`{value}`는 {lo}~1000의 수가 아니다"))
    };
    match key {
        "goal" => doc[key] = toml_edit::value(value),
        "thread_profile" | "coordinator_profile" => {
            aaa_config::profile_edit::validate_name(value)?;
            doc[key] = toml_edit::value(value);
        }
        "max_parallel_threads" => doc[key] = toml_edit::value(number(1)?),
        "auto_resolve_days" => doc[key] = toml_edit::value(number(0)?),
        "repos.add" | "repos.remove" => {
            let want = parse_repo_arg(value);
            if want.path.is_empty() {
                return Err("저장소 경로를 준다".into());
            }
            normalize_repos(&mut doc)?;
            let repos = doc["repos"].as_array_of_tables_mut().ok_or("`repos`는 [[repos]] 표여야 한다")?;
            let same = |t: &Table| {
                t.get("path").and_then(Item::as_str) == Some(want.path.as_str())
                    && t.get("machine").and_then(Item::as_str) == want.machine.as_deref()
            };
            if key == "repos.add" {
                if repos.iter().any(same) {
                    return Err(format!("{value}는 이미 있다"));
                }
                let mut t = Table::new();
                t["path"] = toml_edit::value(want.path.as_str());
                if let Some(m) = &want.machine {
                    t["machine"] = toml_edit::value(m.as_str());
                }
                repos.push(t);
            } else {
                let before = repos.len();
                repos.retain(|t| !same(t));
                if repos.len() == before {
                    return Err(format!("{value}는 project.toml `[[repos]]`에 없다"));
                }
            }
        }
        "name" => return Err("aaa 프로젝트 이름은 폴더 이름이다: `aaa project rename <프로젝트> <새 이름>`".into()),
        "nudge" => {
            return Err(
                "coordinator nudge는 프로젝트마다가 아니라 host.toml `[projects] nudge_idle_secs`(0이면 끔)다".into()
            );
        }
        other => {
            return Err(format!(
                "없는 설정 `{other}`: {}",
                KEYS.iter().map(|(k, v)| format!("{k}({v})")).collect::<Vec<_>>().join(", ")
            ));
        }
    }
    Ok(doc.to_string())
}

impl crate::Host {
    pub(crate) fn settings(&self, project: &str) -> ProjectSettings {
        self.project_dir(project).map(|d| ProjectSettings::load(&d)).unwrap_or_default()
    }

    /// safety 층(plan §8.153): host.toml `[safety.*]`와 옛 자리(host.toml `[projects]`, project.toml). host.toml은 쓸 때마다
    /// 다시 읽는다(사람이 `aaa safety`로 고치면 host를 다시 띄우지 않아도 닿는다). 설정 폴더를 모르면(유닛 시험) 시작 때 값.
    pub(crate) fn safety_layers(&self, project: Option<&str>) -> (aaa_config::safety::Layers, Vec<String>) {
        use aaa_config::safety;
        let (default, own, mut legacy, warnings) = match self.config_dir.get() {
            Some(dir) => safety::read(dir, project),
            None => {
                let c = &self.cfg;
                let get = |k: &str| c.safety.get(k).cloned().unwrap_or_default();
                let legacy =
                    safety::legacy_host_layer(&c.projects.allow_profiles, &c.projects.routine_commands, project);
                (get(safety::DEFAULT_TABLE), project.map(get).unwrap_or_default(), legacy, Vec::new())
            }
        };
        if let Some(p) = project {
            let set = self.settings(p);
            legacy.start_threads = set.start_threads;
            legacy.trust_screens = set.trust_screens;
        }
        (safety::Layers { own, default, legacy }, warnings)
    }

    /// 그 프로젝트가 실제로 쓰는 safety 값
    pub(crate) fn safety(&self, project: &str) -> aaa_config::safety::Safety {
        self.safety_layers(Some(project)).0.resolve()
    }

    /// thread pane 신뢰 창에 답하는 쪽(safety `trust_screens`)
    pub(crate) fn trust_screens(&self, project: &str) -> String {
        self.safety(project).trust_screens
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_fall_back_one_by_one() {
        let t: toml::Table = r#"
            schema = 1
            goal = "웹을 빠르게"
            thread_profile = "opus"
            max_parallel_threads = "many"
            auto_resolve_days = 0
            start_threads = "auto"
            trust_screens = "nobody"
            [[repos]]
            path = "/work/app"
            [[repos]]
            path = "/work/lib"
            machine = "mac-b"
            [[repos]]
            machine = "mac-c"
            [evaluator]
            batch_s = 10
        "#
        .parse()
        .unwrap();
        let s = ProjectSettings::from_table(&t);
        assert_eq!(s.goal, "웹을 빠르게");
        assert_eq!(s.thread_profile, "opus");
        // 잘못된 값은 그 key만 기본값
        assert_eq!(s.max_parallel_threads, 10);
        assert_eq!(s.trust_screens, None);
        assert_eq!((s.auto_resolve_days, s.start_threads.as_deref()), (0, Some("auto")));
        assert_eq!(
            s.repos,
            vec![
                Repo { path: "/work/app".into(), machine: None },
                Repo { path: "/work/lib".into(), machine: Some("mac-b".into()) }
            ]
        );
        assert_eq!(ProjectSettings::from_table(&toml::Table::new()), ProjectSettings::default());
    }

    #[test]
    fn set_keeps_the_rest_and_checks_values() {
        let text = "schema = 1\n# 사람이 단 주석\ngoal = \"옛 목표\"\nrepos = [{ path = \"/work/app\" }]\n\n[evaluator]\nbatch_s = 10\n";
        let t = set_in(text, "goal", " 웹을 빠르게 ").unwrap();
        assert!(
            t.contains("# 사람이 단 주석") && t.contains("goal = \"웹을 빠르게\"") && t.contains("batch_s = 10"),
            "{t}"
        );
        let t = set_in(&t, "repos.add", "/work/lib@mac-b").unwrap();
        let t = set_in(&t, "thread_profile", "opus").unwrap();
        let t = set_in(&t, "max_parallel_threads", "3").unwrap();
        let s = ProjectSettings::from_table(&t.parse().unwrap());
        assert_eq!((s.goal.as_str(), s.thread_profile.as_str(), s.max_parallel_threads), ("웹을 빠르게", "opus", 3));
        assert_eq!(
            s.repos,
            vec![
                Repo { path: "/work/app".into(), machine: None },
                Repo { path: "/work/lib".into(), machine: Some("mac-b".into()) }
            ]
        );
        assert!(set_in(&t, "repos.add", "/work/app").unwrap_err().contains("이미"));
        let t = set_in(&t, "repos.remove", "/work/app").unwrap();
        assert_eq!(ProjectSettings::from_table(&t.parse().unwrap()).repos.len(), 1);
        assert!(set_in(&t, "repos.remove", "/work/app").unwrap_err().contains("없다"));
        // 잘못된 값은 거부하고 글을 바꾸지 않는다
        for (k, v) in
            [("max_parallel_threads", "0"), ("auto_resolve_days", "-1"), ("thread_profile", "-x"), ("name", "새")]
        {
            assert!(set_in(&t, k, v).is_err(), "{k}={v}");
        }
        assert!(set_in(&t, "colour", "red").unwrap_err().contains("auto_resolve_days"));
        assert_eq!(
            set_in(&t, "auto_resolve_days", "0")
                .map(|t| ProjectSettings::from_table(&t.parse().unwrap()).auto_resolve_days),
            Ok(0)
        );
        assert_eq!(parse_repo_arg("git@github.com:o/r.git").machine, None);
    }
}

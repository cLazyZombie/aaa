//! 프로젝트 설정(`projects/<p>/aaa.toml`, 없으면 옛 `project.toml`. plan §8.288). herdr-projects 0.2.35 `project.rs` `Settings`(PROJECT.md front matter)와
//! `Safety`의 프로젝트 값 중 aaa가 쓰는 것을 한곳에서 읽는다(plan §8.77).
//!
//! - 값이 잘못된 key는 그 key만 기본값을 쓴다. 파일을 TOML로 읽지 못하면 모두 기본값이고, inbox `config-error`가 알린다.
//! - `aaa project create`의 옵션과 `aaa project set`(herdr-projects `new`·`set`, 4장 G)이 host를 거쳐 이 파일을 쓴다
//!   ([`set_in`], toml_edit라 다른 줄·주석은 그대로다). coordinator·사람이 직접 고쳐도 된다.
//! - `aaa.toml`은 `[[repos]]` 대신 `[[workspaces]]`(같은 path·machine)라 쓴다. 두 파일 모두 두 이름을 읽는다. aaa는 aaa.toml을
//!   스스로 만들지 않는다(되돌린 옛 binary는 project.toml만 읽는다): 사람이 옮기면 그 뒤로 aaa.toml만 읽고 쓴다.
//! - `start_threads`·`trust_screens`는 옛 자리다: host.toml `[safety.*]`(사람만 고친다, plan §8.163)에 없을 때만 쓰고,
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
    /// `aaa project open`이 coordinator를 띄우는 profile(herdr-projects `coordinator_profile`). 비면 host.toml
    /// `[defaults] coordinator_profile`, 그다음 `claude`
    pub coordinator_profile: String,
    /// 열린 thread가 이만큼이면 `thread start`가 경고한다. 거부하지 않는다(coordinator가 지키는 규칙).
    pub max_parallel_threads: u32,
    /// `Idle`로 이만큼(일) 지난 thread를 마지막 사본 뒤 resolve한다. 0이면 하지 않는다.
    pub auto_resolve_days: u32,
    /// 옛 자리 `start_threads`(propose·auto, 잘못된 값은 없는 것)
    pub start_threads: Option<String>,
    /// 옛 자리 `trust_screens`(user·coordinator, 잘못된 값은 없는 것)
    pub trust_screens: Option<String>,
    /// 이 프로젝트의 밖으로 가는 알림(webhook·Web Push)을 오류·머신 이상만 남기고 끈다(herdr-projects `mute`, 차이 목록 N10).
    /// inbox 항목은 그대로 쌓이고 앱 안 알림도 그대로다.
    pub mute: bool,
    /// 처음 보는 inbox 항목 묶음을 coordinator에 한 줄로 알린다(herdr-projects `nudge`, 기본 켬). 끄면 묶음을 본 것으로만
    /// 적고 항목은 coordinator의 다음 턴 `context`가 보인다(사건마다 가는 알림은 그대로다).
    pub nudge: bool,
    /// 할 일 화면 「나중」 탭이 읽기만 하는 Markdown 파일(프로젝트 폴더 기준 상대 경로, 예 `memory/later.md`, plan §8.305)
    pub task_notes: Vec<String>,
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
            mute: false,
            nudge: true,
            task_notes: Vec::new(),
        }
    }
}

impl ProjectSettings {
    pub(crate) fn load(dir: &Path) -> ProjectSettings {
        let table = std::fs::read_to_string(aaa_config::project_settings_file(dir))
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
            .get("workspaces")
            .or_else(|| t.get("repos"))
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
            mute: t.get("mute").and_then(|v| v.as_bool()).unwrap_or(d.mute),
            nudge: t.get("nudge").and_then(|v| v.as_bool()).unwrap_or(d.nudge),
            task_notes: t
                .get("task_notes")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|v| v.as_str()).map(str::to_string).collect())
                .unwrap_or_default(),
        }
    }
}

/// `aaa project set`이 받는 key와 값(herdr-projects `settings::KEYS`에서 aaa가 읽는 것만: 이름은 폴더 이름이라
/// `project rename`)
pub(crate) const KEYS: [(&str, &str); 11] = [
    ("goal", "글"),
    ("thread_profile", "profile 이름"),
    ("coordinator_profile", "profile 이름"),
    ("max_parallel_threads", "1~1000"),
    ("auto_resolve_days", "0~1000(0 = 하지 않음)"),
    ("repos.add", "PATH 또는 PATH@MACHINE"),
    ("repos.remove", "PATH 또는 PATH@MACHINE"),
    ("nudge", "true/false"),
    ("mute", "true/false"),
    ("task_notes.add", "프로젝트 폴더 안 상대 경로(할 일 화면 「나중」 탭)"),
    ("task_notes.remove", "프로젝트 폴더 안 상대 경로"),
];

/// 켜고 끄는 값(herdr-projects `parse_bool`)
fn parse_bool(value: &str) -> Result<bool, String> {
    match value {
        "true" | "on" | "yes" => Ok(true),
        "false" | "off" | "no" => Ok(false),
        other => Err(format!("`{other}`는 true·false가 아니다")),
    }
}

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

/// 저장소 목록 표 이름: aaa.toml은 `workspaces`, project.toml은 `repos`(plan §8.288)
pub(crate) fn repos_table(path: &Path) -> &'static str {
    if path.file_name().is_some_and(|n| n == "aaa.toml") { "workspaces" } else { "repos" }
}

/// `repos = [...]`(인라인 표)를 `[[repos]]`로 바꿔 더하고 뺄 수 있게 한다(herdr-projects `normalize_repos`). `table`은
/// `repos`·`workspaces`
fn normalize_repos(doc: &mut toml_edit::DocumentMut, table: &str) -> Result<(), String> {
    use toml_edit::{ArrayOfTables, Item};
    let tables = match doc.get(table) {
        None => Some(ArrayOfTables::new()),
        Some(Item::ArrayOfTables(_)) => None,
        Some(Item::Value(toml_edit::Value::Array(array))) => {
            let mut tables = ArrayOfTables::new();
            for v in array.iter() {
                tables.push(v.as_inline_table().ok_or(format!("`{table}`의 항목은 표여야 한다"))?.clone().into_table());
            }
            Some(tables)
        }
        Some(_) => return Err(format!("`{table}`는 표 목록이어야 한다")),
    };
    if let Some(tables) = tables {
        // 표 목록은 일반 key 뒤에 와야 한다: 끝에 다시 넣는다
        doc.remove(table);
        doc[table] = Item::ArrayOfTables(tables);
    }
    Ok(())
}

/// project.toml 글에 값 하나를 바꾼다(herdr-projects `set_in`). 로컬 저장소 경로는 부르는 쪽이 정본 경로로 바꿔 준다.
pub(crate) fn set_in(text: &str, key: &str, value: &str) -> Result<String, String> {
    set_in_table(text, key, value, "repos")
}

/// 설정 글에 값 하나를 바꾼다. `repos`는 저장소 목록 표의 새 이름([`repos_table`])이고, 글에 이미 다른 이름의 목록이 있으면
/// 그것을 고친다. `workspaces.add|remove`는 `repos.add|remove`와 같다.
pub(crate) fn set_in_table(text: &str, key: &str, value: &str, repos: &str) -> Result<String, String> {
    use toml_edit::{Item, Table};
    let mut doc =
        text.parse::<toml_edit::DocumentMut>().map_err(|e| format!("설정 파일을 읽지 못한다: {}", e.message()))?;
    let key = match key {
        "workspaces.add" => "repos.add",
        "workspaces.remove" => "repos.remove",
        k => k,
    };
    let repos = ["workspaces", "repos"].into_iter().find(|t| doc.contains_key(t)).unwrap_or(repos);
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
            normalize_repos(&mut doc, repos)?;
            let list = repos;
            let repos =
                doc[list].as_array_of_tables_mut().ok_or_else(|| format!("`{list}`는 [[{list}]] 표여야 한다"))?;
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
                    return Err(format!("{value}는 `[[{list}]]`에 없다"));
                }
            }
        }
        "nudge" | "mute" => doc[key] = toml_edit::value(parse_bool(value)?),
        // `repos.add|remove`와 같은 꼴: 문자열 목록에 하나씩 더하고 뺀다
        "task_notes.add" | "task_notes.remove" => {
            crate::tasks::check_notes_path(value)?;
            if !doc.contains_key("task_notes") {
                doc["task_notes"] = toml_edit::value(toml_edit::Array::new());
            }
            let list = doc["task_notes"].as_array_mut().ok_or("`task_notes`는 문자열 목록이어야 한다")?;
            let at = list.iter().position(|v| v.as_str() == Some(value));
            match (key, at) {
                ("task_notes.add", Some(_)) => return Err(format!("{value}는 이미 있다")),
                ("task_notes.add", None) => list.push(value),
                (_, Some(i)) => {
                    list.remove(i);
                }
                (_, None) => return Err(format!("{value}는 `task_notes`에 없다")),
            }
        }
        "name" => return Err("aaa 프로젝트 이름은 폴더 이름이다: `aaa project rename <프로젝트> <새 이름>`".into()),
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

    /// safety 층(plan §8.163): host.toml `[safety.*]`와 옛 자리(host.toml `[projects]`, project.toml). host.toml은 쓸 때마다
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
                let own = project.map(|p| get(safety::ONE_TABLE).over(get(p))).unwrap_or_default();
                (get(safety::DEFAULT_TABLE), own, legacy, Vec::new())
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
            mute = "yes"
            nudge = 0
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
        assert!(!s.mute);
        assert!(ProjectSettings::from_table(&"mute = true".parse().unwrap()).mute);
        assert!(s.nudge && !ProjectSettings::from_table(&"nudge = false".parse().unwrap()).nudge);
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
        // 「나중」 파일: 상대 경로만, 더하고 빼기
        let t = set_in(&t, "task_notes.add", "memory/later.md").unwrap();
        assert_eq!(ProjectSettings::from_table(&t.parse().unwrap()).task_notes, ["memory/later.md"]);
        assert!(set_in(&t, "task_notes.add", "memory/later.md").unwrap_err().contains("이미"));
        for bad in ["../x.md", "/etc/hosts"] {
            assert!(set_in(&t, "task_notes.add", bad).is_err(), "{bad}");
        }
        let t = set_in(&t, "task_notes.remove", "memory/later.md").unwrap();
        assert!(ProjectSettings::from_table(&t.parse().unwrap()).task_notes.is_empty());
        assert!(set_in(&t, "task_notes.remove", "memory/later.md").unwrap_err().contains("없다"));
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
        // nudge·mute(main §8.136이 읽는 project.toml 값): 참고처럼 on·off·yes·no도
        let t = set_in(&set_in(&t, "nudge", "off").unwrap(), "mute", "on").unwrap();
        let s = ProjectSettings::from_table(&t.parse().unwrap());
        assert!(!s.nudge && s.mute, "{t}");
        assert!(set_in(&t, "mute", "maybe").unwrap_err().contains("true·false"));
    }

    /// `aaa.toml`(plan §8.288): 있으면 project.toml 대신 읽고 쓰며 저장소 목록은 `[[workspaces]]`다. aaa가 만들지는 않는다
    #[test]
    fn aaa_toml_comes_first_and_keeps_workspaces() {
        let state = std::env::temp_dir().join(format!("aaa-settings-{}", aaa_common::util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = state.join("projects").display().to_string();
        cfg.notify.web_push = false;
        let h = crate::Host::open(cfg, state.clone()).unwrap();
        h.project_create("demo", Default::default(), "test").unwrap();
        let dir = h.project_dir("demo").unwrap();
        h.project_set("demo", "goal", "옛 목표", "test").unwrap();
        assert!(!dir.join("aaa.toml").exists(), "aaa는 aaa.toml을 만들지 않는다");
        assert_eq!(h.settings("demo").goal, "옛 목표");
        std::fs::write(
            dir.join("aaa.toml"),
            "schema = 1\ngoal = \"새 목표\"\n\n[[workspaces]]\npath = \"/work/app\"\n",
        )
        .unwrap();
        let s = h.settings("demo");
        assert_eq!((s.goal.as_str(), s.repos), ("새 목표", vec![Repo { path: "/work/app".into(), machine: None }]));
        h.project_set("demo", "repos.add", "/work/lib@mac-b", "test").unwrap();
        h.project_set("demo", "workspaces.remove", "/work/app", "test").unwrap();
        h.project_set("demo", "max_parallel_threads", "3", "test").unwrap();
        let new = std::fs::read_to_string(dir.join("aaa.toml")).unwrap();
        assert!(
            new.contains("[[workspaces]]\npath = \"/work/lib\"\nmachine = \"mac-b\"") && !new.contains("repos"),
            "{new}"
        );
        let s = h.settings("demo");
        assert_eq!((s.repos.len(), s.max_parallel_threads), (1, 3));
        let old = std::fs::read_to_string(dir.join("project.toml")).unwrap();
        assert!(old.contains("옛 목표") && !old.contains("max_parallel_threads"), "project.toml은 그대로: {old}");
        // aaa.toml에 옛 이름 [[repos]]가 있으면 그것을 고친다
        let t = set_in_table("[[repos]]\npath = \"/a\"\n", "repos.add", "/b", "workspaces").unwrap();
        assert!(t.contains("[[repos]]\npath = \"/b\"") && !t.contains("workspaces"), "{t}");
        drop(h);
        std::fs::remove_dir_all(&state).unwrap();
    }
}

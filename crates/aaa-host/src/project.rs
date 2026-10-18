//! 프로젝트 설정(`projects/<p>/project.toml`). herdr-projects 0.2.35 `project.rs` `Settings`(PROJECT.md front matter)와
//! `Safety`의 프로젝트 값 중 aaa가 쓰는 것을 한곳에서 읽는다(plan §8.77).
//!
//! - 값이 잘못된 key는 그 key만 기본값을 쓴다. 파일을 TOML로 읽지 못하면 모두 기본값이고, inbox `config-error`가 알린다.
//! - 이 파일을 쓰는 CLI(`aaa project set` 등)는 아직 없다. coordinator·사람이 직접 고친다.
//! - `start_threads`·`trust_screens`는 옛 자리다: host.toml `[safety.*]`(사람만 고친다, plan §8.161)에 없을 때만 쓰고,
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
}

impl Default for ProjectSettings {
    fn default() -> Self {
        ProjectSettings {
            goal: String::new(),
            repos: Vec::new(),
            thread_profile: String::new(),
            max_parallel_threads: 10,
            auto_resolve_days: 7,
            start_threads: None,
            trust_screens: None,
            mute: false,
            nudge: true,
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
            max_parallel_threads: n("max_parallel_threads", d.max_parallel_threads),
            auto_resolve_days: n("auto_resolve_days", d.auto_resolve_days),
            start_threads,
            trust_screens,
            mute: t.get("mute").and_then(|v| v.as_bool()).unwrap_or(d.mute),
            nudge: t.get("nudge").and_then(|v| v.as_bool()).unwrap_or(d.nudge),
        }
    }
}

impl crate::Host {
    pub(crate) fn settings(&self, project: &str) -> ProjectSettings {
        self.project_dir(project).map(|d| ProjectSettings::load(&d)).unwrap_or_default()
    }

    /// safety 층(plan §8.161): host.toml `[safety.*]`와 옛 자리(host.toml `[projects]`, project.toml). host.toml은 쓸 때마다
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
}

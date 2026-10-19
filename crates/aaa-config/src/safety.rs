//! safety 설정(herdr-projects 0.2.35 `src/safety.rs`·`src/project.rs` `Safety`·`SafetyLayer`·`load_safety`, plan §8.163).
//!
//! - host.toml(·host.local.toml)의 `[safety.default]`(모든 프로젝트)와 `[safety."<프로젝트>"]`(그 프로젝트). 프로젝트 폴더
//!   밖에 두어 coordinator가 고치지 못한다(§8.19). 바꾸기는 사람이 터미널에서만 한다(`aaa safety`, `aaa profile allow`).
//! - 값마다 「`[safety]` 표 → 프로젝트 표 → default 표 → 옛 자리 → 기본값」 차례로 정한다. `[safety]` 표 하나는 host가 프로젝트
//!   하나만 맡은 뒤의 자리다(plan §8.288). 되돌리기(옛 binary)를 위해 호환 기간에는 `aaa safety`가 프로젝트 표에 쓴다.
//!   옛 자리는 host.toml `[projects] allow_profiles`·`routine_commands`와 project.toml `start_threads`·`trust_screens`
//!   (프로젝트 폴더 안이라 출처에 드러낸다).
//! - host는 쓸 때마다 이 표만 느슨하게 다시 읽는다: 다른 절이 깨져도, 값 하나가 잘못돼도 그 값만 기본값으로 간다.
//! - coordinator 쪽 값(`coordinator_profiles`·`coordinator_agent_args`, yolo의 coordinator 몫)은 `aaa project open`이 쓴다
//!   (plan §8.180). aaa에 옛 자리가 없어 프로젝트 표 → default 표 → 기본값이다.

use std::path::Path;

use serde::{Deserialize, Serialize};
use toml_edit::{Array, DocumentMut, Item, Table};

/// 모든 프로젝트가 따르는 `[safety.<표>]`. 프로젝트 이름은 이 이름을 쓸 수 없다.
pub const DEFAULT_TABLE: &str = "default";

/// `aaa safety set`이 받는 key와 값(참고 `KEYS`)
pub const KEYS: [(&str, &str); 7] = [
    ("yolo", "on/off"),
    ("start_threads", "propose/auto"),
    ("trust_screens", "coordinator/user"),
    ("coordinator_agent_args", "인자"),
    ("thread_agent_args", "인자"),
    ("routine_commands", "on/off"),
    ("coordinator_deploy", "on/off"),
];

/// 바꾼 뒤 알리는 말(참고 `RESTART_NOTE`)
pub const RESTART_NOTE: &str = "이제부터 띄우는 thread·coordinator가 쓴다. 이미 도는 것은 다시 띄울 때까지 옛 인자다(thread는 \
                                `aaa thread restart`, coordinator는 닫고 `aaa coordinator open`)";

/// harness가 권한을 묻지 않게 하는 인자(참고 `yolo_flags`). 모르는 harness는 `None`: yolo여도 묻는다.
pub fn yolo_flags(harness: &str) -> Option<&'static [&'static str]> {
    match harness {
        "claude" => Some(&["--dangerously-skip-permissions"]),
        "codex" => Some(&["--dangerously-bypass-approvals-and-sandbox"]),
        _ => None,
    }
}

/// `[safety.*]` 표 하나, 적힌 그대로. 빠진 key는 아래 층을 따른다.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SafetyLayer {
    /// yolo: thread를 묻지 않고 시작하고(start_threads auto), thread·coordinator를 harness의 권한 건너뛰기 인자로 띄운다
    #[serde(skip_serializing_if = "Option::is_none")]
    pub yolo: Option<bool>,
    /// coordinator가 따르는 안내 값: propose(제안하고 승인을 기다림)·auto
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_threads: Option<String>,
    /// thread pane 신뢰 창에 누가 답하나: user·coordinator. 없으면 yolo를 따른다(켬: coordinator)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trust_screens: Option<String>,
    /// coordinator(`project open`)를 띄울 때 프로젝트 기본 coordinator profile과 같은 harness의 기본 profile 인자 앞에 붙이는 인자
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coordinator_agent_args: Option<Vec<String>>,
    /// thread를 띄울 때 profile 인자 뒤에 붙이는 인자
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_agent_args: Option<Vec<String>>,
    /// routine 명령을 실행해도 되는가(그래도 명령마다 사람 승인이 필요하다, §8.19). yolo와 따로다
    #[serde(skip_serializing_if = "Option::is_none")]
    pub routine_commands: Option<bool>,
    /// 사용자가 채팅으로 지시한 coordinator 배포만 허용한다. yolo와 별개이며 기본은 false다.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coordinator_deploy: Option<bool>,
    /// thread가 쓸 수 있는 profile. 없으면 모두
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_profiles: Option<Vec<String>>,
    /// coordinator가 쓸 수 있는 profile. 없으면 모두
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coordinator_profiles: Option<Vec<String>>,
}

/// host.toml `[safety]` 표 하나의 값(plan §8.288)이 [`crate::HostConfig::safety`]에서 쓰는 열쇠. 프로젝트 이름은 비지 않는다.
pub const ONE_TABLE: &str = "";

/// `[safety]`: 아래 표(`default`·`"<프로젝트>"`)와 표 하나의 값(`yolo = …`)이 섞인다. 값들은 [`ONE_TABLE`] 열쇠로 읽고 쓴다.
pub mod tables {
    use std::collections::BTreeMap;

    use serde::de::Error as _;
    use serde::ser::Error as _;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use super::{ONE_TABLE, SafetyLayer};

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<BTreeMap<String, SafetyLayer>, D::Error> {
        let raw = BTreeMap::<String, serde_json::Value>::deserialize(d)?;
        let (mut out, mut one) = (BTreeMap::new(), serde_json::Map::new());
        for (k, v) in raw {
            if v.is_object() {
                out.insert(k, serde_json::from_value(v).map_err(D::Error::custom)?);
            } else {
                one.insert(k, v);
            }
        }
        if !one.is_empty() {
            out.insert(ONE_TABLE.into(), serde_json::from_value(one.into()).map_err(D::Error::custom)?);
        }
        Ok(out)
    }

    pub fn serialize<S: Serializer>(m: &BTreeMap<String, SafetyLayer>, s: S) -> Result<S::Ok, S::Error> {
        let mut out = serde_json::Map::new();
        for (k, l) in m {
            match serde_json::to_value(l).map_err(S::Error::custom)? {
                serde_json::Value::Object(values) if k.is_empty() => out.extend(values),
                v => {
                    out.insert(k.clone(), v);
                }
            }
        }
        out.serialize(s)
    }
}

impl SafetyLayer {
    /// 이 층의 값, 빠진 key는 `below`의 값(`[safety]` 표 하나 → 프로젝트 표처럼 위가 앞선다)
    pub fn over(self, below: SafetyLayer) -> SafetyLayer {
        SafetyLayer {
            yolo: self.yolo.or(below.yolo),
            start_threads: self.start_threads.or(below.start_threads),
            trust_screens: self.trust_screens.or(below.trust_screens),
            coordinator_agent_args: self.coordinator_agent_args.or(below.coordinator_agent_args),
            thread_agent_args: self.thread_agent_args.or(below.thread_agent_args),
            routine_commands: self.routine_commands.or(below.routine_commands),
            coordinator_deploy: self.coordinator_deploy.or(below.coordinator_deploy),
            thread_profiles: self.thread_profiles.or(below.thread_profiles),
            coordinator_profiles: self.coordinator_profiles.or(below.coordinator_profiles),
        }
    }
}

/// 실제로 쓰는 값(참고 `Safety`)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Safety {
    pub yolo: bool,
    pub start_threads: String,
    pub trust_screens: String,
    pub coordinator_agent_args: Vec<String>,
    pub thread_agent_args: Vec<String>,
    pub routine_commands: bool,
    pub coordinator_deploy: bool,
    /// `None`이면 모든 profile
    pub thread_profiles: Option<Vec<String>>,
    /// `None`이면 모든 profile
    pub coordinator_profiles: Option<Vec<String>>,
}

impl Safety {
    /// thread가 이 profile을 쓸 수 있는가. 목록의 `"*"`는 모두다(옛 `allow_profiles`와 같다).
    pub fn profile_allowed(&self, name: &str) -> bool {
        self.thread_profiles.as_ref().is_none_or(|l| l.iter().any(|a| a == name || a == "*"))
    }

    /// coordinator가 이 profile을 쓸 수 있는가(참고 `check_allowed`의 Role::Coordinator)
    pub fn coordinator_profile_allowed(&self, name: &str) -> bool {
        self.coordinator_profiles.as_ref().is_none_or(|l| l.iter().any(|a| a == name || a == "*"))
    }

    /// 실제로 띄울 인자: `args` 뒤에 yolo면 그 harness의 인자(참고 `Safety::launch_args`)
    pub fn launch_args(&self, harness: &str, args: &[String]) -> Vec<String> {
        apply_yolo(self.yolo, harness, args)
    }
}

/// `args` 뒤에 yolo면 그 harness의 권한 건너뛰기 인자(이미 있으면 한 번만). agent가 profile을 푼 뒤 쓴다.
pub fn apply_yolo(yolo: bool, harness: &str, args: &[String]) -> Vec<String> {
    let mut out = args.to_vec();
    if yolo {
        for flag in yolo_flags(harness).unwrap_or_default() {
            if !out.iter().any(|a| a == flag) {
                out.push(flag.to_string());
            }
        }
    }
    out
}

/// `thread_agent_args`가 닿는 profile(참고 `profiles::launch_args`, herdr-projects issue #45): 프로젝트 기본 profile의
/// harness(`legacy_harness`)를 위해 적은 인자라 그 harness의 기본 profile에만 붙인다. 다른 harness(claude 인자로 codex가
/// 뜨지 않는다)나 사람이 정의한 profile(제 인자를 가진다)에는 붙이지 않는다.
pub fn legacy_args(builtin: bool, harness: &str, legacy: &[String], legacy_harness: &str) -> Vec<String> {
    if builtin && harness == legacy_harness { legacy.to_vec() } else { Vec::new() }
}

/// 값을 정할 층들. `legacy`는 옛 자리(host.toml `[projects]`·project.toml)에서 host가 채운다.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Layers {
    pub own: SafetyLayer,
    pub default: SafetyLayer,
    pub legacy: SafetyLayer,
}

/// 신뢰 창 기본값: yolo면 coordinator, 아니면 user(참고 `default_trust_screens`)
pub fn default_trust_screens(yolo: bool) -> &'static str {
    if yolo { "coordinator" } else { "user" }
}

impl Layers {
    /// 참고 `load_safety`: 프로젝트 표 → default 표 → 옛 자리 → 기본값. yolo는 start_threads를 auto로 만든다.
    pub fn resolve(&self) -> Safety {
        let (o, d, l) = (&self.own, &self.default, &self.legacy);
        let yolo = o.yolo.or(d.yolo).or(l.yolo).unwrap_or(false);
        let pick =
            |f: fn(&SafetyLayer) -> Option<String>, def: String| f(o).or_else(|| f(d)).or_else(|| f(l)).unwrap_or(def);
        Safety {
            start_threads: if yolo { "auto".into() } else { pick(|x| x.start_threads.clone(), "propose".into()) },
            trust_screens: pick(|x| x.trust_screens.clone(), default_trust_screens(yolo).into()),
            coordinator_agent_args: o
                .coordinator_agent_args
                .clone()
                .or_else(|| d.coordinator_agent_args.clone())
                .unwrap_or_default(),
            thread_agent_args: o
                .thread_agent_args
                .clone()
                .or_else(|| d.thread_agent_args.clone())
                .or_else(|| l.thread_agent_args.clone())
                .unwrap_or_default(),
            routine_commands: o.routine_commands.or(d.routine_commands).or(l.routine_commands).unwrap_or(false),
            coordinator_deploy: o.coordinator_deploy.or(d.coordinator_deploy).unwrap_or(false),
            thread_profiles: o
                .thread_profiles
                .clone()
                .or_else(|| d.thread_profiles.clone())
                .or_else(|| l.thread_profiles.clone()),
            coordinator_profiles: o.coordinator_profiles.clone().or_else(|| d.coordinator_profiles.clone()),
            yolo,
        }
    }

    /// `aaa safety show`의 줄들: 실제 값과 출처(참고 `rows`). `project`가 없으면 모든 프로젝트 기본값을 본다.
    pub fn rows(&self, project: bool) -> Vec<Row> {
        let s = self.resolve();
        let own_source = if project { "프로젝트" } else { "모든 프로젝트" };
        let (o, d, l) = (&self.own, &self.default, &self.legacy);
        let src = |own: bool, def: bool, legacy: bool, legacy_source: &'static str| {
            if own {
                own_source
            } else if def {
                "모든 프로젝트"
            } else if legacy {
                legacy_source
            } else {
                "기본값"
            }
        };
        let args = |v: &[String]| if v.is_empty() { "(없음)".to_string() } else { v.join(" ") };
        let on_off = |b: bool| if b { "on" } else { "off" }.to_string();
        let trust_source =
            src(o.trust_screens.is_some(), d.trust_screens.is_some(), l.trust_screens.is_some(), LEGACY_PROJECT);
        vec![
            Row {
                key: "yolo",
                value: on_off(s.yolo),
                source: src(o.yolo.is_some(), d.yolo.is_some(), false, ""),
                note: String::new(),
            },
            Row {
                key: "start_threads",
                value: s.start_threads.clone(),
                source: src(
                    o.start_threads.is_some(),
                    d.start_threads.is_some(),
                    l.start_threads.is_some(),
                    LEGACY_PROJECT,
                ),
                note: if s.yolo { "yolo".into() } else { String::new() },
            },
            Row {
                key: "trust_screens",
                value: s.trust_screens.clone(),
                source: trust_source,
                note: if trust_source == "기본값" { "yolo를 따른다".into() } else { String::new() },
            },
            Row {
                key: "coordinator_agent_args",
                value: args(&s.coordinator_agent_args),
                source: src(o.coordinator_agent_args.is_some(), d.coordinator_agent_args.is_some(), false, ""),
                note: String::new(),
            },
            Row {
                key: "thread_agent_args",
                value: args(&s.thread_agent_args),
                source: src(o.thread_agent_args.is_some(), d.thread_agent_args.is_some(), false, ""),
                note: String::new(),
            },
            Row {
                key: "routine_commands",
                value: on_off(s.routine_commands),
                source: src(
                    o.routine_commands.is_some(),
                    d.routine_commands.is_some(),
                    l.routine_commands.is_some(),
                    LEGACY_HOST,
                ),
                note: String::new(),
            },
            Row {
                key: "coordinator_deploy",
                value: on_off(s.coordinator_deploy),
                source: src(o.coordinator_deploy.is_some(), d.coordinator_deploy.is_some(), false, ""),
                note: "사용자가 채팅으로 지시한 배포만".into(),
            },
            Row {
                key: "thread_profiles",
                value: match &s.thread_profiles {
                    None => "모든 profile".into(),
                    Some(l) if l.is_empty() => "없음".into(),
                    Some(l) => l.join(", "),
                },
                source: src(
                    o.thread_profiles.is_some(),
                    d.thread_profiles.is_some(),
                    l.thread_profiles.is_some(),
                    LEGACY_HOST,
                ),
                note: String::new(),
            },
            Row {
                key: "coordinator_profiles",
                value: match &s.coordinator_profiles {
                    None => "모든 profile".into(),
                    Some(l) if l.is_empty() => "없음".into(),
                    Some(l) => l.join(", "),
                },
                source: src(o.coordinator_profiles.is_some(), d.coordinator_profiles.is_some(), false, ""),
                note: String::new(),
            },
        ]
    }
}

/// 옛 자리 출처: project.toml(프로젝트 폴더 안이라 coordinator도 고칠 수 있다)
pub const LEGACY_PROJECT: &str = "project.toml(프로젝트 폴더 안)";
/// 옛 자리 출처: host.toml `[projects]`
pub const LEGACY_HOST: &str = "host.toml [projects]";

/// `aaa safety show` 한 줄
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub key: &'static str,
    pub value: String,
    pub source: &'static str,
    /// 적힌 값과 다른 까닭(yolo가 start_threads를 auto로 만든다 등)
    pub note: String,
}

impl Row {
    pub fn text(&self) -> String {
        if self.note.is_empty() { self.value.clone() } else { format!("{} ({})", self.value, self.note) }
    }
}

/// host.toml `[projects] allow_profiles`·`routine_commands`(옛 자리)를 그 프로젝트의 층으로 바꾼다
pub fn legacy_host_layer(allow_profiles: &[String], routine_commands: &[String], project: Option<&str>) -> SafetyLayer {
    SafetyLayer {
        thread_profiles: (!allow_profiles.iter().any(|a| a == "*")).then(|| allow_profiles.to_vec()),
        routine_commands: routine_commands.iter().any(|r| r == "*" || Some(r.as_str()) == project).then_some(true),
        ..SafetyLayer::default()
    }
}

/// project.toml(옛 자리, 프로젝트 폴더 안)의 `start_threads`·`trust_screens`. 잘못된 값은 없는 것이다.
pub fn legacy_project_layer(project_toml: &str) -> SafetyLayer {
    let doc = project_toml.parse::<DocumentMut>().unwrap_or_default();
    let get = |k: &str, ok: &[&str]| doc.get(k).and_then(Item::as_str).filter(|v| ok.contains(v)).map(str::to_string);
    SafetyLayer {
        start_threads: get("start_threads", &["propose", "auto"]),
        trust_screens: get("trust_screens", &["coordinator", "user"]),
        ..SafetyLayer::default()
    }
}

/// project.toml의 `thread_profile`(없으면 빈 값)
pub fn project_thread_profile(project_toml: &str) -> String {
    project_profile(project_toml, "thread_profile")
}

/// project.toml의 `thread_profile`·`coordinator_profile`(없으면 빈 값)
pub fn project_profile(project_toml: &str, key: &str) -> String {
    let doc = project_toml.parse::<DocumentMut>().unwrap_or_default();
    doc.get(key).and_then(Item::as_str).unwrap_or_default().to_string()
}

/// 글(host.local.toml)에 `[safety.<table>] key`가 있는가: 있으면 host.toml을 고쳐도 닿지 않는다
pub fn has_key(text: &str, table: &str, key: &str) -> bool {
    text.parse::<DocumentMut>()
        .ok()
        .is_some_and(|d| d.get("safety").and_then(|s| s.get(table)).and_then(|t| t.get(key)).is_some())
}

/// 글에 `[safety] key`(표 하나, plan §8.288)가 있는가: 있으면 프로젝트 표(`[safety.<p>]`)를 고쳐도 닿지 않는다
pub fn has_top_key(text: &str, key: &str) -> bool {
    text.parse::<DocumentMut>()
        .ok()
        .is_some_and(|d| d.get("safety").and_then(|s| s.get(key)).is_some_and(|v| !v.is_table_like()))
}

/// host.toml·host.local.toml 글(뒤가 덮는다)에서 default 표와 그 프로젝트 표, 옛 `[projects]` 층, 버린 값의 경고.
/// 값마다 따로 읽어 잘못된 값은 그 값만 버린다(기본값 쪽으로 간다).
pub fn read_texts(texts: &[String], project: Option<&str>) -> (SafetyLayer, SafetyLayer, SafetyLayer, Vec<String>) {
    let mut warnings = Vec::new();
    let (mut default, mut own) = (SafetyLayer::default(), SafetyLayer::default());
    let (mut allow, mut commands): (Option<Vec<String>>, Option<Vec<String>>) = (None, None);
    for text in texts {
        let Ok(doc) = text.parse::<DocumentMut>() else {
            warnings.push("host.toml을 TOML로 읽지 못해 safety는 기본값이다".into());
            continue;
        };
        if let Some(t) = doc.get("safety").and_then(Item::as_table_like) {
            if let Some(d) = t.get(DEFAULT_TABLE).and_then(Item::as_table_like) {
                overlay(&mut default, d, DEFAULT_TABLE, &mut warnings);
            }
            if let Some(p) = project {
                if let Some(x) = t.get(p).and_then(Item::as_table_like) {
                    overlay(&mut own, x, p, &mut warnings);
                }
                // `[safety]` 표 하나(host는 프로젝트 하나, plan §8.288): 그 프로젝트 표보다 앞선다. 아래 표(default·프로젝트)는 값이 아니다
                let mut top = Table::new();
                for (k, v) in t.iter().filter(|(_, v)| !v.is_table_like()) {
                    top.insert(k, v.clone());
                }
                overlay(&mut own, &top, "", &mut warnings);
            }
        }
        if let Some(p) = doc.get("projects").and_then(Item::as_table_like) {
            if let Some(v) = p.get("allow_profiles").and_then(strings) {
                allow = Some(v);
            }
            if let Some(v) = p.get("routine_commands").and_then(strings) {
                commands = Some(v);
            }
        }
    }
    let legacy = legacy_host_layer(&allow.unwrap_or_else(|| vec!["*".into()]), &commands.unwrap_or_default(), project);
    (default, own, legacy, warnings)
}

/// 설정 폴더의 host.toml·host.local.toml을 읽는다([`read_texts`])
pub fn read(config_dir: &Path, project: Option<&str>) -> (SafetyLayer, SafetyLayer, SafetyLayer, Vec<String>) {
    let texts: Vec<String> = ["host.toml", "host.local.toml"]
        .iter()
        .filter_map(|f| std::fs::read_to_string(config_dir.join(f)).ok())
        .collect();
    read_texts(&texts, project)
}

fn strings(item: &Item) -> Option<Vec<String>> {
    item.as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
}

fn overlay(layer: &mut SafetyLayer, t: &dyn toml_edit::TableLike, table: &str, warnings: &mut Vec<String>) {
    let name = if table.is_empty() { "[safety]".to_string() } else { format!("[safety.{table}]") };
    let mut bad = |key: &str, why: &str| warnings.push(format!("{name} {key}: {why}, 버리고 아래 층을 쓴다"));
    if let Some(v) = t.get("yolo") {
        match v.as_bool() {
            Some(b) => layer.yolo = Some(b),
            None => bad("yolo", "true·false여야 한다"),
        }
    }
    if let Some(v) = t.get("coordinator_deploy") {
        match v.as_bool() {
            Some(b) => layer.coordinator_deploy = Some(b),
            None => bad("coordinator_deploy", "true·false여야 한다"),
        }
    }
    if let Some(v) = t.get("routine_commands") {
        match v.as_bool() {
            Some(b) => layer.routine_commands = Some(b),
            None => bad("routine_commands", "true·false여야 한다"),
        }
    }
    if let Some(v) = t.get("start_threads") {
        match v.as_str() {
            Some(s @ ("propose" | "auto")) => layer.start_threads = Some(s.into()),
            _ => bad("start_threads", "\"propose\"·\"auto\"여야 한다"),
        }
    }
    if let Some(v) = t.get("trust_screens") {
        match v.as_str() {
            Some(s @ ("coordinator" | "user")) => layer.trust_screens = Some(s.into()),
            _ => bad("trust_screens", "\"coordinator\"·\"user\"여야 한다"),
        }
    }
    for (key, slot) in [
        ("coordinator_agent_args", &mut layer.coordinator_agent_args),
        ("thread_agent_args", &mut layer.thread_agent_args),
        ("thread_profiles", &mut layer.thread_profiles),
        ("coordinator_profiles", &mut layer.coordinator_profiles),
    ] {
        if let Some(v) = t.get(key) {
            match v.as_array().filter(|a| a.iter().all(|x| x.is_str())) {
                Some(_) => *slot = strings(v),
                None => bad(key, "글자 배열이어야 한다"),
            }
        }
    }
}

// ───────────────────────── 쓰기(사람이 터미널에서) ─────────────────────────

fn on_off(value: &str) -> Result<bool, String> {
    match value {
        "on" | "true" | "yes" => Ok(true),
        "off" | "false" | "no" => Ok(false),
        other => Err(format!("`{other}`는 on·off가 아니다")),
    }
}

/// `words`가 `key`에 줄 TOML 값. `default`이면 `None`(key를 지워 아래 층을 쓴다). 인자 목록은 빈칸으로 나누고, 낱말이
/// 없으면 빈 목록이다(참고 `value_item`).
pub fn value_item(key: &str, words: &[String]) -> Result<Option<Item>, String> {
    if words.len() == 1 && words[0] == "default" {
        return Ok(None);
    }
    let one = || match words {
        [w] => Ok(w.as_str()),
        _ => Err(format!("`{key}`는 값 하나를 받는다")),
    };
    Ok(Some(match key {
        "yolo" | "routine_commands" | "coordinator_deploy" => toml_edit::value(on_off(one()?)?),
        "start_threads" => match one()? {
            v @ ("propose" | "auto") => toml_edit::value(v),
            other => return Err(format!("start_threads는 propose·auto다, `{other}`가 아니다")),
        },
        "trust_screens" => match one()? {
            v @ ("coordinator" | "user") => toml_edit::value(v),
            other => return Err(format!("trust_screens는 coordinator·user다, `{other}`가 아니다")),
        },
        "thread_agent_args" | "coordinator_agent_args" => {
            let mut a = Array::new();
            for arg in words.iter().flat_map(|w| w.split_whitespace()) {
                a.push(arg);
            }
            toml_edit::value(a)
        }
        other => {
            return Err(format!(
                "모르는 safety 설정 `{other}`. 이 중 하나다: {}",
                KEYS.iter().map(|(k, _)| *k).collect::<Vec<_>>().join(", ")
            ));
        }
    }))
}

/// host.toml 글에서 `[safety.<table>] key`를 바꾸거나(`None`이면 지운다) 나머지는 그대로 둔다. 빈 표는 지운다(참고 `set_in`).
pub fn set_in(text: &str, table: &str, key: &str, item: Option<Item>) -> Result<String, String> {
    let mut doc = text.parse::<DocumentMut>().map_err(|e| format!("host.toml을 읽지 못했다: {e}"))?;
    if !doc.contains_key("safety") {
        let mut t = Table::new();
        t.set_implicit(true);
        doc["safety"] = Item::Table(t);
    }
    let safety = doc["safety"].as_table_mut().ok_or("host.toml의 `safety`가 표가 아니다")?;
    match item {
        Some(item) => {
            if !safety.contains_key(table) {
                safety[table] = Item::Table(Table::new());
            }
            safety[table].as_table_mut().ok_or_else(|| format!("`safety.{table}`가 표가 아니다"))?[key] = item;
        }
        None => {
            if let Some(t) = safety.get_mut(table).and_then(Item::as_table_mut) {
                t.remove(key);
                if t.is_empty() {
                    safety.remove(table);
                }
            }
        }
    }
    Ok(doc.to_string())
}

/// `[safety.<table>] thread_profiles`·`coordinator_profiles`(`key`): `Some`이면 그 목록, `None`이면 지워 모든 profile
/// (참고 `Change::Allow`)
pub fn set_profiles_in(text: &str, table: &str, key: &str, names: Option<&[String]>) -> Result<String, String> {
    let item = names.map(|n| {
        let mut a = Array::new();
        for name in n {
            a.push(name.as_str());
        }
        toml_edit::value(a)
    });
    set_in(text, table, key, item)
}

/// `[defaults] thread_profile`(참고 `Change::Default`): `thread start`에 profile이 정해지지 않으면 쓰고, `project create`가
/// project.toml에 적는다
pub fn set_default_profile_in(text: &str, key: &str, name: &str) -> Result<String, String> {
    let mut doc = text.parse::<DocumentMut>().map_err(|e| format!("host.toml을 읽지 못했다: {e}"))?;
    if !doc.contains_key("defaults") {
        doc["defaults"] = Item::Table(Table::new());
    }
    doc["defaults"].as_table_mut().ok_or("host.toml의 `defaults`가 표가 아니다")?[key] = toml_edit::value(name);
    Ok(doc.to_string())
}

/// host.toml(local이 덮는다)의 `[defaults] thread_profile`(없으면 빈 값). host는 쓸 때마다 읽는다.
pub fn default_thread_profile(config_dir: &Path) -> String {
    default_profile(config_dir, "thread_profile")
}

/// host.toml(local이 덮는다)의 `[defaults]` `thread_profile`·`coordinator_profile`(없으면 빈 값)
pub fn default_profile(config_dir: &Path, key: &str) -> String {
    let mut out = String::new();
    for f in ["host.toml", "host.local.toml"] {
        if let Some(v) = std::fs::read_to_string(config_dir.join(f))
            .ok()
            .and_then(|t| t.parse::<DocumentMut>().ok())
            .and_then(|d| d.get("defaults").and_then(|x| x.get(key)).and_then(|v| v.as_str().map(str::to_string)))
        {
            out = v;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    const HOST: &str = "schema = 1 # 내 것\n\n[projects]\nallow_profiles = [\"opus\", \"codex\"]\nroutine_commands = [\"shop\"]\n\n[safety.default]\nthread_agent_args = [\"--verbose\"]\n\n[safety.demo]\nyolo = true\nthread_profiles = [\"opus\"]\n";

    #[test]
    fn layers_fall_through_like_the_reference() {
        let (d, o, l, w) = read_texts(&[HOST.into()], Some("demo"));
        assert!(w.is_empty(), "{w:?}");
        let demo = Layers { own: o, default: d.clone(), legacy: l }.resolve();
        // 프로젝트 yolo: start_threads는 auto, 신뢰 창은 coordinator(yolo를 따른다), 인자는 default 표에서
        assert_eq!(
            (demo.yolo, demo.start_threads.as_str(), demo.trust_screens.as_str()),
            (true, "auto", "coordinator")
        );
        let legacy = legacy_args(true, "claude", &demo.thread_agent_args, "claude");
        assert_eq!(demo.launch_args("claude", &legacy), s(&["--verbose", "--dangerously-skip-permissions"]));
        assert_eq!(demo.launch_args("shell", &[]), Vec::<String>::new(), "모르는 harness는 yolo 인자가 없다");
        let once = demo.launch_args("codex", &s(&["--dangerously-bypass-approvals-and-sandbox"]));
        assert_eq!(once, s(&["--dangerously-bypass-approvals-and-sandbox"]), "이미 있으면 한 번만");
        // 옛 인자는 기본 harness의 기본 profile에만(issue #45)
        assert!(legacy_args(true, "codex", &demo.thread_agent_args, "claude").is_empty());
        assert!(legacy_args(false, "claude", &demo.thread_agent_args, "claude").is_empty());
        assert!(demo.profile_allowed("opus") && !demo.profile_allowed("codex"));
        assert!(!demo.routine_commands, "routine_commands는 yolo와 따로다");
        // 다른 프로젝트: 옛 [projects] 값이 아래 층으로 온다
        let (d, o, l, _) = read_texts(&[HOST.into()], Some("shop"));
        let shop = Layers { own: o, default: d, legacy: l }.resolve();
        assert_eq!((shop.yolo, shop.start_threads.as_str(), shop.trust_screens.as_str()), (false, "propose", "user"));
        assert!(shop.routine_commands && shop.profile_allowed("codex") && !shop.profile_allowed("other"));
        // 옛 allow_profiles가 "*"면 모두
        let (d, o, l, _) = read_texts(&["[projects]\nallow_profiles = [\"*\"]\n".into()], Some("x"));
        assert!(Layers { own: o, default: d, legacy: l }.resolve().profile_allowed("anything"));
        // coordinator 쪽: 옛 자리가 없고, 목록이 없으면 모두, thread 목록과 따로다
        assert!(demo.coordinator_profile_allowed("codex") && demo.coordinator_agent_args.is_empty());
        let local = "[safety.demo]\ncoordinator_profiles = [\"luna\"]\n\n[safety.default]\ncoordinator_agent_args = [\"--c\"]\n";
        let (d, o, l, w) = read_texts(&[HOST.into(), local.into()], Some("demo"));
        assert!(w.is_empty(), "{w:?}");
        let demo = Layers { own: o, default: d, legacy: l }.resolve();
        assert!(demo.coordinator_profile_allowed("luna") && !demo.coordinator_profile_allowed("opus"));
        assert_eq!((demo.coordinator_agent_args, demo.thread_profiles), (s(&["--c"]), Some(s(&["opus"]))));
    }

    /// `[safety]` 표 하나(plan §8.288): 프로젝트 표보다 앞서고, 아래 표들은 값으로 읽지 않는다. 잘못된 값은 그 값만 버린다.
    #[test]
    fn one_safety_table_comes_before_the_project_table() {
        let text = "[safety]\nyolo = false\ntrust_screens = \"coordinator\"\nstart_threads = \"later\"\n\n[safety.default]\nroutine_commands = true\n\n[safety.demo]\nyolo = true\nstart_threads = \"auto\"\nthread_profiles = [\"opus\"]\n";
        let (d, o, l, w) = read_texts(&[text.into()], Some("demo"));
        assert_eq!(w, vec!["[safety] start_threads: \"propose\"·\"auto\"여야 한다, 버리고 아래 층을 쓴다".to_string()]);
        let demo = Layers { own: o, default: d.clone(), legacy: l }.resolve();
        assert_eq!(
            (demo.yolo, demo.start_threads.as_str(), demo.trust_screens.as_str()),
            (false, "auto", "coordinator"),
            "[safety]의 yolo가 프로젝트 표를 덮고, 버린 start_threads는 프로젝트 표 값이다"
        );
        assert!(demo.routine_commands && demo.thread_profiles == Some(s(&["opus"])));
        // 모든 프로젝트 기본값(--global)에는 닿지 않는다
        assert_eq!(read_texts(&[text.into()], None).1, SafetyLayer::default());
        assert!(has_top_key(text, "yolo") && !has_top_key(text, "demo") && !has_top_key(HOST, "yolo"));
    }

    #[test]
    fn local_file_overrides_and_bad_values_fall_back_one_by_one() {
        let local = "[safety.demo]\nyolo = false\nstart_threads = \"later\"\ntrust_screens = 3\n".to_string();
        let (d, o, l, w) = read_texts(&[HOST.into(), local], Some("demo"));
        assert_eq!(w.len(), 2, "{w:?}");
        let demo = Layers { own: o.clone(), default: d, legacy: l }.resolve();
        assert_eq!((demo.yolo, demo.start_threads.as_str(), demo.trust_screens.as_str()), (false, "propose", "user"));
        assert_eq!(o.thread_profiles, Some(s(&["opus"])), "local이 덮지 않은 값은 남는다");
    }

    #[test]
    fn rows_name_where_each_value_comes_from() {
        let (d, o, _, _) = read_texts(&[HOST.into()], Some("shop"));
        let legacy = SafetyLayer {
            trust_screens: Some("coordinator".into()),
            ..legacy_host_layer(&s(&["*"]), &[], Some("shop"))
        };
        let rows = Layers { own: o, default: d, legacy }.rows(true);
        let by = |k: &str| rows.iter().find(|r| r.key == k).unwrap().clone();
        assert_eq!((by("yolo").value.as_str(), by("yolo").source), ("off", "기본값"));
        assert_eq!((by("trust_screens").value.as_str(), by("trust_screens").source), ("coordinator", LEGACY_PROJECT));
        assert_eq!(
            (by("thread_agent_args").value.as_str(), by("thread_agent_args").source),
            ("--verbose", "모든 프로젝트")
        );
        assert_eq!(
            (by("thread_profiles").text(), by("thread_profiles").source),
            ("모든 profile".to_string(), "기본값")
        );
        assert_eq!(
            (by("coordinator_agent_args").text(), by("coordinator_agent_args").source),
            ("(없음)".to_string(), "기본값")
        );
        assert_eq!(
            (by("coordinator_profiles").text(), by("coordinator_profiles").source),
            ("모든 profile".to_string(), "기본값")
        );
    }

    /// 배포 허용은 yolo와 독립이고 host 바깥 safety 층에서만 온다.
    #[test]
    fn coordinator_deployment_is_opt_in_and_can_be_revoked() {
        let resolve_rows = |text: &str| {
            let (d, o, l, w) = read_texts(&[text.into()], Some("demo"));
            assert!(w.is_empty(), "{w:?}");
            Layers { own: o, default: d, legacy: l }
                .rows(true)
                .into_iter()
                .find(|r| r.key == "coordinator_deploy")
                .unwrap()
                .value
        };
        let on = set_in(HOST, "default", "coordinator_deploy", value_item("coordinator_deploy", &s(&["on"])).unwrap())
            .unwrap();
        assert_eq!(resolve_rows(HOST), "off", "yolo는 배포를 허용하지 않는다");
        assert_eq!(resolve_rows(&on), "on");
        let off =
            set_in(&on, "demo", "coordinator_deploy", value_item("coordinator_deploy", &s(&["off"])).unwrap()).unwrap();
        assert_eq!(resolve_rows(&off), "off", "프로젝트에서 즉시 철회한다");
        assert!(legacy_project_layer("coordinator_deploy = true").coordinator_deploy.is_none());
    }

    #[test]
    fn edits_keep_the_rest_of_the_file() {
        let t = set_in(HOST, "demo", "yolo", value_item("yolo", &s(&["off"])).unwrap()).unwrap();
        assert!(t.starts_with("schema = 1 # 내 것\n"), "{t}");
        assert!(t.contains("[safety.demo]\nyolo = false\n"), "{t}");
        // default면 지우고, 빈 표는 없앤다
        let t = set_in(&t, "demo", "yolo", None).unwrap();
        let t = set_profiles_in(&t, "demo", "thread_profiles", None).unwrap();
        assert!(!t.contains("[safety.demo]"), "{t}");
        let c = set_profiles_in(&t, "demo", "coordinator_profiles", Some(&s(&["luna"]))).unwrap();
        assert!(c.contains("[safety.demo]\ncoordinator_profiles = [\"luna\"]\n"), "{c}");
        let t = set_in(&t, "new", "thread_agent_args", value_item("thread_agent_args", &s(&["--a b", "--c"])).unwrap())
            .unwrap();
        let (_, o, _, w) = read_texts(std::slice::from_ref(&t), Some("new"));
        assert!(w.is_empty());
        assert_eq!(o.thread_agent_args, Some(s(&["--a", "b", "--c"])));
        let c =
            set_in(&t, "new", "coordinator_agent_args", value_item("coordinator_agent_args", &s(&["--x y"])).unwrap())
                .unwrap();
        assert_eq!(read_texts(&[c], Some("new")).1.coordinator_agent_args, Some(s(&["--x", "y"])));
        let t = set_default_profile_in(&t, "thread_profile", "opus").unwrap();
        assert!(t.contains("[defaults]\nthread_profile = \"opus\"\n"), "{t}");
        let t = set_default_profile_in(&t, "coordinator_profile", "luna").unwrap();
        assert!(t.contains("[defaults]\nthread_profile = \"opus\"\ncoordinator_profile = \"luna\"\n"), "{t}");
        for (key, words, why) in [
            ("yolo", s(&["maybe"]), "on·off"),
            ("start_threads", s(&["now"]), "propose·auto"),
            ("colour", s(&["x"]), "모르는 safety 설정"),
        ] {
            assert!(value_item(key, &words).unwrap_err().contains(why), "{key}");
        }
    }
}

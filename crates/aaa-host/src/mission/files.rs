//! 임무 TOML은 파일 원본과 읽은 brief를 함께 적용 사본에 보존한다.
use super::*;
use schemars::JsonSchema;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct File {
    #[serde(default)]
    pub title: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    pub profile: String,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub machine: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    pub brief_file: String,
    #[serde(default)]
    pub limits: Limits,
    #[serde(default)]
    pub reset: Reset,
    #[serde(default = "criteria")]
    pub withdrawal: String,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub retire_no_posts_days: u32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub retire_cap_days: u32,
}
fn yes() -> bool {
    true
}
fn criteria() -> String {
    "criteria".into()
}
fn is_zero(value: &u32) -> bool {
    *value == 0
}
fn no_gap(value: &str) -> bool {
    matches!(value, "0m" | "0s")
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Limits {
    pub batch: String,
    #[serde(skip_serializing_if = "no_gap", deserialize_with = "crate::automation::duration_value")]
    pub min_gap: String,
    #[serde(skip_serializing_if = "is_zero")]
    pub max_wakes: u32,
    #[serde(skip_serializing_if = "is_zero")]
    pub max_minutes: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_session_below: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_week_below: Option<f64>,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            batch: "5m".into(),
            min_gap: "0m".into(),
            max_wakes: 0,
            max_minutes: 0,
            usage_session_below: None,
            usage_week_below: None,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Reset {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub daily: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub every: Option<String>,
    pub after_wakes: u32,
}
impl Default for Reset {
    fn default() -> Self {
        Self { daily: None, every: None, after_wakes: 6 }
    }
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    mission_toml: String,
    brief: String,
}
fn parse_snapshot(n: &str, snapshot: Snapshot) -> Result<Mission, ErrorBody> {
    let path = format!("missions/{n}.toml");
    let f: File = crate::automation::decode(&path, &snapshot.mission_toml).map_err(bad)?;
    let fail = |field: &str, e: &str| bad(crate::automation::field_error(&path, &snapshot.mission_toml, field, e));
    let batch = crate::automation::duration_ms(&f.limits.batch).map_err(|e| fail("limits.batch", &e))? / 1000;
    if batch > 86400 {
        return Err(fail("limits.batch", "0~86400초 범위여야 한다"));
    }
    let daily = if f.reset.every.is_none() && f.reset.daily.is_none() { Some("05:00".into()) } else { f.reset.daily };
    let reset = crate::automation::Scheduled { every: f.reset.every, daily, catch_up: Default::default() }
        .expression()
        .map_err(|e| fail("reset", e))?;
    if f.profile.trim().is_empty() || snapshot.brief.trim().is_empty() {
        return Err(fail("brief_file", "profile과 임무 본문은 필수다"));
    }
    if f.reset.after_wakes == 0 {
        return Err(fail("reset.after_wakes", "양수여야 한다"));
    }
    let seconds = std::env::var(crate::routine::TEST_SECONDS).as_deref() == Ok("1");
    crate::automation::duration_ms(&f.limits.min_gap).map_err(|e| fail("limits.min_gap", &e))?;
    parse_schedule(&reset, seconds).map_err(|e| fail("reset", &e))?;
    for (name, value) in
        [("usage_session_below", f.limits.usage_session_below), ("usage_week_below", f.limits.usage_week_below)]
    {
        if value.is_some_and(|n| !n.is_finite() || !(0.0..=100.0).contains(&n)) {
            return Err(fail(&format!("limits.{name}"), "사용량 문턱은 0~100이다"));
        }
    }
    let d = Definition {
        title: f.title,
        enabled: f.enabled,
        profile: f.profile,
        machine: f.machine,
        repo: f.repo,
        batch_s: batch,
        min_gap: f.limits.min_gap,
        max_wakes: f.limits.max_wakes,
        max_minutes: f.limits.max_minutes,
        usage_session_below: f.limits.usage_session_below,
        usage_week_below: f.limits.usage_week_below,
        reset,
        reset_after_wakes: f.reset.after_wakes,
        withdrawal: f.withdrawal,
        retire_no_posts_days: f.retire_no_posts_days,
        retire_cap_days: f.retire_cap_days,
        ..Default::default()
    };
    let legacy = format!("+++\n{}+++\n{}\n", toml::to_string(&d).map_err(internal)?, snapshot.brief);
    let mut m = parse(n, &legacy).map_err(|e| fail("임무", &e.message))?;
    m.file_text = snapshot.mission_toml.clone();
    m.text = serde_json::to_string(&snapshot).map_err(internal)?;
    m.sha = sha(&m.text)[..12].into();
    Ok(m)
}
pub(super) fn snapshot(n: &str, text: &str) -> Result<Mission, ErrorBody> {
    parse_snapshot(n, serde_json::from_str(text).map_err(|e| bad(format!("임무 적용 사본: {e}")))?)
}
pub(super) fn prepare(root: &Path, n: &str, text: &str) -> Result<Mission, ErrorBody> {
    prepare_with(n, text, |path| crate::automation::project_text(root, path))
}
pub(super) fn prepare_with(
    n: &str,
    text: &str,
    read: impl Fn(&str) -> Result<String, String>,
) -> Result<Mission, ErrorBody> {
    let file = format!("missions/{n}.toml");
    let f: File = crate::automation::decode(&file, text).map_err(bad)?;
    let brief = read(&f.brief_file).map_err(|e| bad(crate::automation::field_error(&file, text, "brief_file", &e)))?;
    parse_snapshot(n, Snapshot { mission_toml: text.into(), brief })
}
pub(super) fn path(root: &Path, n: &str) -> String {
    let toml = format!("missions/{n}.toml");
    if root.join(&toml).symlink_metadata().is_ok() { toml } else { format!("missions/{n}.md") }
}
pub(super) fn names(root: &Path) -> Vec<String> {
    let mut out = std::collections::BTreeSet::new();
    let mut briefs = std::collections::BTreeSet::new();
    let files: Vec<_> = std::fs::read_dir(root.join("missions"))
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    for file in files.iter().filter(|f| f.ends_with(".toml")) {
        out.insert(file.trim_end_matches(".toml").to_string());
        if let Ok(text) = crate::automation::project_text(root, &format!("missions/{file}"))
            && let Ok(f) = crate::automation::decode::<File>(file, &text)
        {
            briefs.insert(f.brief_file);
        }
    }
    for file in files.iter().filter(|f| f.ends_with(".md")) {
        let rel = format!("missions/{file}");
        if !briefs.contains(&rel) || crate::automation::project_text(root, &rel).is_ok_and(|s| s.starts_with("+++\n")) {
            out.insert(file.trim_end_matches(".md").to_string());
        }
    }
    out.into_iter().collect()
}
pub(crate) fn schema() -> serde_json::Value {
    let mut value = serde_json::to_value(schemars::schema_for!(File)).expect("mission schema");
    value["title"] = "AAA 임무".into();
    value["$defs"]["Limits"]["properties"]["min_gap"] =
        serde_json::json!({"anyOf":[{"type":"string"},{"const":0}],"description":"생략 또는0이면 제한 없음"});
    for field in ["max_wakes", "max_minutes"] {
        value["$defs"]["Limits"]["properties"][field]["minimum"] = 0.into();
    }
    for field in ["usage_session_below", "usage_week_below"] {
        value["$defs"]["Limits"]["properties"][field]["minimum"] = 0.into();
        value["$defs"]["Limits"]["properties"][field]["maximum"] = 100.into();
    }
    value["$defs"]["Reset"]["not"] = serde_json::json!({"required":["every","daily"]});
    value["$defs"]["Reset"]["properties"]["after_wakes"]["minimum"] = 1.into();
    value
}

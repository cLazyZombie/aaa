//! 화면 감지 규칙 엔진. herdr 0.9.5 `src/detect/manifest.rs`(Apache-2.0, `LICENSE-herdr.txt`)를 옮겼다.
//!
//! 바꾼 것: 규칙은 저장소에 넣은 `manifests/*.toml`(herdr 0.9.5 그대로, 2026.10.11.1)만 쓴다. herdr의 원격 갱신·local
//! override·explain 출력은 옮기지 않았다(plan §8.3 「감지 규칙은 버전을 pin하고 원격에서 자동 갱신하지 않는다」).
//! 맞은 규칙 ID를 결과에 함께 둔다(질문 종류를 정하고 진단에 쓴다).

use std::sync::OnceLock;

use regex::Regex;
use serde::Deserialize;

use super::{Detection, DetectionInput, Harness, State};

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub(crate) struct AgentManifest {
    id: String,
    version: Option<String>,
    min_engine_version: Option<u32>,
    #[serde(rename = "updated_at")]
    _updated_at: Option<String>,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    rules: Vec<ManifestRule>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct ManifestRule {
    id: String,
    state: Option<ManifestState>,
    #[serde(default)]
    priority: i32,
    #[serde(default = "default_region")]
    region: String,
    #[serde(default)]
    visible_idle: bool,
    #[serde(default)]
    visible_blocker: bool,
    #[serde(default)]
    visible_working: bool,
    #[serde(default)]
    skip_state_update: bool,
    #[serde(default)]
    all: Vec<ManifestGate>,
    #[serde(default)]
    any: Vec<ManifestGate>,
    #[serde(default, rename = "not")]
    not_gate: Vec<ManifestGate>,
    #[serde(default)]
    contains: Vec<String>,
    #[serde(default)]
    regex: Vec<String>,
    #[serde(default)]
    line_regex: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct ManifestGate {
    #[serde(default)]
    all: Vec<ManifestGate>,
    #[serde(default)]
    any: Vec<ManifestGate>,
    #[serde(default, rename = "not")]
    not_gate: Vec<ManifestGate>,
    #[serde(default)]
    contains: Vec<String>,
    #[serde(default)]
    regex: Vec<String>,
    #[serde(default)]
    line_regex: Vec<String>,
}

#[derive(Debug, Clone)]
struct CompiledGate {
    all: Vec<CompiledGate>,
    any: Vec<CompiledGate>,
    not_gate: Vec<CompiledGate>,
    contains: Vec<String>,
    regex: Vec<Regex>,
    line_regex: Vec<Regex>,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ManifestState {
    Idle,
    Working,
    Blocked,
    Unknown,
}

impl From<ManifestState> for State {
    fn from(value: ManifestState) -> Self {
        match value {
            ManifestState::Idle => State::Idle,
            ManifestState::Working => State::Working,
            ManifestState::Blocked => State::Blocked,
            ManifestState::Unknown => State::Unknown,
        }
    }
}

fn default_region() -> String {
    "whole_recent".to_string()
}

/// 컴파일한 규칙 묶음(정규식 검색 캐시를 pane polling 사이에 살려 둔다)
pub(crate) struct Loaded {
    manifest: AgentManifest,
    compiled: Vec<CompiledGate>,
}

impl Loaded {
    pub(crate) fn version(&self) -> Option<&str> {
        self.manifest.version.as_deref()
    }
}

const CLAUDE: &str = include_str!("manifests/claude.toml");
const CODEX: &str = include_str!("manifests/codex.toml");

/// herdr `MANIFEST_ENGINE_VERSION`(manifest_update.rs). 규칙 파일이 요구하는 엔진 판을 이 판까지 받는다.
pub(crate) const ENGINE_VERSION: u32 = 3;

const MAX_RULES_PER_MANIFEST: usize = 128;
const MAX_GATE_DEPTH: usize = 8;
const MAX_TOTAL_GATES: usize = 512;
const MAX_MATCHERS_PER_GATE: usize = 32;
const MAX_TOTAL_MATCHERS: usize = 1024;
const MAX_MATCHER_CHARS: usize = 512;
const TOP_NON_EMPTY_LINES_ENGINE_VERSION: u32 = 3;
const MAX_TOP_REGION_LINE_COUNT: usize = u16::MAX as usize;

pub(crate) fn bundled(harness: Harness) -> &'static Loaded {
    static CLAUDE_RULES: OnceLock<Loaded> = OnceLock::new();
    static CODEX_RULES: OnceLock<Loaded> = OnceLock::new();
    let (cell, text) = match harness {
        Harness::Claude => (&CLAUDE_RULES, CLAUDE),
        Harness::Codex => (&CODEX_RULES, CODEX),
    };
    // 저장소에 넣은 규칙이라 읽히지 않으면 build가 잘못된 것이다(시험 `bundled_manifests_parse_and_validate`)
    cell.get_or_init(|| load(text, harness).unwrap_or_else(|e| panic!("{} 감지 규칙: {e}", harness.label())))
}

pub(crate) fn load(text: &str, harness: Harness) -> Result<Loaded, String> {
    let manifest = parse_manifest(text)?;
    if !manifest_matches(&manifest, harness) {
        return Err(format!("manifest id {}가 {}와 다르다", manifest.id, harness.label()));
    }
    if let Some(min) = manifest.min_engine_version
        && min > ENGINE_VERSION
    {
        return Err(format!("manifest가 엔진 {min}을 요구한다(지금 {ENGINE_VERSION})"));
    }
    let compiled = manifest
        .rules
        .iter()
        .map(|rule| {
            compile_gate(&gate_of(rule)).map_err(|err| format!("rule {} could not be compiled: {err}", rule.id))
        })
        .collect::<Result<_, _>>()?;
    Ok(Loaded { manifest, compiled })
}

fn manifest_matches(manifest: &AgentManifest, harness: Harness) -> bool {
    let id = harness.label();
    manifest.id == id
        || manifest.aliases.iter().any(|alias| alias == id)
        || Harness::from_label(&manifest.id) == Some(harness)
        || manifest.aliases.iter().any(|alias| Harness::from_label(alias) == Some(harness))
}

/// 규칙을 차례로 평가해 우선순위가 가장 높은 것을 고른다(같으면 앞 규칙). 아무것도 맞지 않으면 알려진 agent라 Idle이다
/// (herdr `fallback_explain`의 `default_known_agent_idle_fallback`).
pub(crate) fn evaluate(loaded: &Loaded, input: DetectionInput<'_>) -> Detection {
    let mut matched: Option<&ManifestRule> = None;
    for (rule, gate) in loaded.manifest.rules.iter().zip(loaded.compiled.iter()) {
        let text = region(input, &rule.region);
        if !compiled_rule_matches(gate, text) {
            continue;
        }
        match matched {
            Some(previous) if previous.priority >= rule.priority => {}
            _ => matched = Some(rule),
        }
    }
    let Some(rule) = matched else {
        return Detection {
            state: State::Idle,
            skip_state_update: false,
            visible_idle: false,
            visible_blocker: false,
            visible_working: false,
            rule: None,
        };
    };
    let state = rule.state.map(State::from).unwrap_or(State::Unknown);
    Detection {
        state,
        skip_state_update: rule.skip_state_update,
        visible_idle: rule.visible_idle && state == State::Idle,
        visible_blocker: rule.visible_blocker && state == State::Blocked,
        visible_working: rule.visible_working && state == State::Working,
        rule: Some(rule.id.clone()),
    }
}

pub(crate) fn parse_manifest(content: &str) -> Result<AgentManifest, String> {
    let manifest = toml::from_str::<AgentManifest>(content).map_err(|err| err.to_string())?;
    validate_manifest(&manifest)?;
    Ok(manifest)
}

fn validate_manifest(manifest: &AgentManifest) -> Result<(), String> {
    if manifest.rules.is_empty() {
        return Err("manifest must contain at least one rule".to_string());
    }
    if manifest.rules.len() > MAX_RULES_PER_MANIFEST {
        return Err(format!("manifest contains {} rules, max is {MAX_RULES_PER_MANIFEST}", manifest.rules.len()));
    }

    let mut complexity = ManifestComplexity::default();
    for rule in &manifest.rules {
        if rule.id.trim().is_empty() {
            return Err("manifest rule id must not be empty".to_string());
        }
        if rule.skip_state_update {
            if rule.state != Some(ManifestState::Unknown) {
                return Err(format!("rule {} uses skip_state_update without state = \"unknown\"", rule.id));
            }
            if rule.visible_idle || rule.visible_blocker || rule.visible_working {
                return Err(format!("rule {} uses skip_state_update with visible state evidence", rule.id));
            }
        }
        validate_region_name(&rule.region).map_err(|err| format!("rule {} uses invalid region: {err}", rule.id))?;
        if rule.region.trim().starts_with("top_non_empty_lines(")
            && manifest.min_engine_version.is_some_and(|version| version < TOP_NON_EMPTY_LINES_ENGINE_VERSION)
        {
            return Err(format!(
                "rule {} uses top_non_empty_lines but min_engine_version is below {}",
                rule.id, TOP_NON_EMPTY_LINES_ENGINE_VERSION
            ));
        }
        validate_gate(&gate_of(rule), "rule", 0, &mut complexity)
            .map_err(|err| format!("rule {} has invalid matcher gates: {err}", rule.id))?;
    }

    Ok(())
}

#[derive(Default)]
struct ManifestComplexity {
    total_gates: usize,
    total_matchers: usize,
}

fn validate_gate(
    gate: &ManifestGate,
    context: &str,
    depth: usize,
    complexity: &mut ManifestComplexity,
) -> Result<(), String> {
    if depth > MAX_GATE_DEPTH {
        return Err(format!("{context} exceeds max gate depth {MAX_GATE_DEPTH}"));
    }
    complexity.total_gates += 1;
    if complexity.total_gates > MAX_TOTAL_GATES {
        return Err(format!("manifest exceeds max gate count {MAX_TOTAL_GATES}"));
    }
    validate_matcher_limits(gate, context, complexity)?;
    if !gate_has_positive_matcher(gate) {
        return Err(format!("{context} must contain a positive matcher"));
    }
    validate_regex_patterns(&gate.regex, context, "regex")?;
    validate_regex_patterns(&gate.line_regex, context, "line_regex")?;
    for nested in &gate.all {
        validate_gate(nested, "all gate", depth + 1, complexity)?;
    }
    for nested in &gate.any {
        validate_gate(nested, "any gate", depth + 1, complexity)?;
    }
    for nested in &gate.not_gate {
        if !gate_has_any_matcher(nested) {
            return Err(format!("{context} contains an empty not gate"));
        }
        validate_not_gate(nested, depth + 1, complexity)?;
    }
    Ok(())
}

fn validate_not_gate(gate: &ManifestGate, depth: usize, complexity: &mut ManifestComplexity) -> Result<(), String> {
    if depth > MAX_GATE_DEPTH {
        return Err(format!("not gate exceeds max gate depth {MAX_GATE_DEPTH}"));
    }
    complexity.total_gates += 1;
    if complexity.total_gates > MAX_TOTAL_GATES {
        return Err(format!("manifest exceeds max gate count {MAX_TOTAL_GATES}"));
    }
    validate_matcher_limits(gate, "not gate", complexity)?;
    if !gate_has_any_matcher(gate) {
        return Err("not gate must contain a matcher".to_string());
    }
    validate_regex_patterns(&gate.regex, "not gate", "regex")?;
    validate_regex_patterns(&gate.line_regex, "not gate", "line_regex")?;
    for nested in &gate.all {
        validate_gate(nested, "not all gate", depth + 1, complexity)?;
    }
    for nested in &gate.any {
        validate_gate(nested, "not any gate", depth + 1, complexity)?;
    }
    for nested in &gate.not_gate {
        validate_not_gate(nested, depth + 1, complexity)?;
    }
    Ok(())
}

fn validate_matcher_limits(
    gate: &ManifestGate,
    context: &str,
    complexity: &mut ManifestComplexity,
) -> Result<(), String> {
    let matcher_count = gate.contains.len() + gate.regex.len() + gate.line_regex.len();
    if matcher_count > MAX_MATCHERS_PER_GATE {
        return Err(format!("{context} has {matcher_count} direct matchers, max is {MAX_MATCHERS_PER_GATE}"));
    }
    complexity.total_matchers += matcher_count;
    if complexity.total_matchers > MAX_TOTAL_MATCHERS {
        return Err(format!("manifest exceeds max matcher count {MAX_TOTAL_MATCHERS}"));
    }
    for value in gate.contains.iter().chain(gate.regex.iter()).chain(gate.line_regex.iter()) {
        if value.chars().count() > MAX_MATCHER_CHARS {
            return Err(format!("{context} matcher exceeds max length {MAX_MATCHER_CHARS}"));
        }
    }
    Ok(())
}

fn validate_regex_patterns(patterns: &[String], context: &str, field: &str) -> Result<(), String> {
    for pattern in patterns {
        Regex::new(pattern).map_err(|err| format!("{context} contains invalid {field} pattern {pattern:?}: {err}"))?;
    }
    Ok(())
}

fn gate_has_positive_matcher(gate: &ManifestGate) -> bool {
    !gate.contains.is_empty()
        || !gate.regex.is_empty()
        || !gate.line_regex.is_empty()
        || !gate.all.is_empty()
        || !gate.any.is_empty()
}

fn gate_has_any_matcher(gate: &ManifestGate) -> bool {
    gate_has_positive_matcher(gate) || !gate.not_gate.is_empty()
}

fn validate_region_name(spec: &str) -> Result<(), String> {
    let trimmed = spec.trim();
    match trimmed {
        "whole_recent"
        | "after_last_prompt_marker"
        | "before_current_prompt_marker"
        | "whole_recent_without_current_prompt_marker"
        | "current_prompt_block_marker"
        | "after_current_prompt_block_marker"
        | "prompt_box_body"
        | "above_prompt_box"
        | "last_non_empty_above_prompt_box"
        | "after_last_horizontal_rule"
        | "osc_title"
        | "osc_progress" => Ok(()),
        _ if region_count(trimmed, "bottom_lines").is_some()
            || region_count(trimmed, "bottom_non_empty_lines").is_some()
            || top_region_count(trimmed).is_some() =>
        {
            Ok(())
        }
        _ => Err(trimmed.to_string()),
    }
}

fn gate_of(rule: &ManifestRule) -> ManifestGate {
    ManifestGate {
        all: rule.all.clone(),
        any: rule.any.clone(),
        not_gate: rule.not_gate.clone(),
        contains: rule.contains.clone(),
        regex: rule.regex.clone(),
        line_regex: rule.line_regex.clone(),
    }
}

fn compile_gate(gate: &ManifestGate) -> Result<CompiledGate, String> {
    Ok(CompiledGate {
        all: gate.all.iter().map(compile_gate).collect::<Result<_, _>>()?,
        any: gate.any.iter().map(compile_gate).collect::<Result<_, _>>()?,
        not_gate: gate.not_gate.iter().map(compile_gate).collect::<Result<_, _>>()?,
        contains: gate.contains.iter().map(|needle| needle.to_lowercase()).collect(),
        regex: gate
            .regex
            .iter()
            .map(|pattern| Regex::new(pattern).map_err(|err| err.to_string()))
            .collect::<Result<_, _>>()?,
        line_regex: gate
            .line_regex
            .iter()
            .map(|pattern| Regex::new(pattern).map_err(|err| err.to_string()))
            .collect::<Result<_, _>>()?,
    })
}

fn compiled_rule_matches(gate: &CompiledGate, text: &str) -> bool {
    let lower_text = text.to_lowercase();
    compiled_gate_matches(gate, text, &lower_text)
}

/// contains는 대소문자를 가리지 않고(소문자로 비교), regex는 영역 전체, line_regex는 어느 한 줄에 맞으면 된다.
/// all은 모두, any는 하나 이상(비어 있으면 통과), not은 하나라도 맞으면 실패다.
fn compiled_gate_matches(gate: &CompiledGate, text: &str, lower_text: &str) -> bool {
    if !gate.contains.iter().all(|needle| lower_text.contains(needle)) {
        return false;
    }
    if !gate.regex.iter().all(|regex| regex.is_match(text)) {
        return false;
    }
    if !gate.line_regex.iter().all(|regex| text.lines().any(|line| regex.is_match(line))) {
        return false;
    }
    if !gate.all.iter().all(|nested| compiled_gate_matches(nested, text, lower_text)) {
        return false;
    }
    if !gate.any.is_empty() && !gate.any.iter().any(|nested| compiled_gate_matches(nested, text, lower_text)) {
        return false;
    }
    if gate.not_gate.iter().any(|nested| compiled_gate_matches(nested, text, lower_text)) {
        return false;
    }
    true
}

fn region<'a>(input: DetectionInput<'a>, spec: &str) -> &'a str {
    let trimmed = spec.trim();
    // OSC 영역은 화면이 아니라 따로 받은 값이다
    match trimmed {
        "osc_title" => return input.osc_title,
        "osc_progress" => return input.osc_progress,
        _ => {}
    }
    let content = input.screen;
    match trimmed {
        "whole_recent" => content,
        "after_last_prompt_marker" => after_last_prompt_marker(content),
        "before_current_prompt_marker" => before_current_prompt_marker(content),
        "whole_recent_without_current_prompt_marker" => whole_recent_without_current_prompt_marker(content),
        "current_prompt_block_marker" => current_prompt_block_marker(content).unwrap_or(""),
        "after_current_prompt_block_marker" => after_current_prompt_block_marker(content).unwrap_or(""),
        "prompt_box_body" => prompt_box_body(content).unwrap_or(""),
        "above_prompt_box" => above_prompt_box(content),
        "last_non_empty_above_prompt_box" => last_non_empty_line(above_prompt_box(content)),
        "after_last_horizontal_rule" => after_last_horizontal_rule(content),
        _ => {
            if let Some(count) = region_count(trimmed, "bottom_lines") {
                return bottom_lines(content, count);
            }
            if let Some(count) = region_count(trimmed, "bottom_non_empty_lines") {
                return bottom_non_empty_lines(content, count);
            }
            if let Some(count) = top_region_count(trimmed) {
                return top_non_empty_lines(content, count);
            }
            ""
        }
    }
}

fn region_count(spec: &str, name: &str) -> Option<usize> {
    spec.strip_prefix(name)
        .and_then(|rest| rest.strip_prefix('('))
        .and_then(|rest| rest.strip_suffix(')'))
        .and_then(|count| count.parse::<usize>().ok())
}

fn top_region_count(spec: &str) -> Option<usize> {
    let count = spec.strip_prefix("top_non_empty_lines")?.strip_prefix('(')?.strip_suffix(')')?;
    if count.starts_with('0') || !count.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    count.parse::<usize>().ok().filter(|count| *count <= MAX_TOP_REGION_LINE_COUNT)
}

fn bottom_lines(content: &str, count: usize) -> &str {
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(count);
    slice_from_line_index(content, &lines, start)
}

fn bottom_non_empty_lines(content: &str, count: usize) -> &str {
    let lines: Vec<&str> = content.lines().collect();
    let Some(start_index) = lines
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, line)| !line.trim().is_empty())
        .take(count)
        .last()
        .map(|(index, _)| index)
    else {
        return "";
    };
    slice_from_line_index(content, &lines, start_index)
}

fn top_non_empty_lines(content: &str, count: usize) -> &str {
    let lines: Vec<&str> = content.lines().collect();
    let Some(end_index) =
        lines.iter().enumerate().filter(|(_, line)| !line.trim().is_empty()).take(count).last().map(|(index, _)| index)
    else {
        return "";
    };
    let byte_offset = line_start_offset(content, &lines, end_index + 1);
    &content[..byte_offset]
}

fn after_last_prompt_marker(content: &str) -> &str {
    let lines: Vec<&str> = content.lines().collect();
    let Some(index) = lines.iter().rposition(|line| codex_prompt_line(line)) else {
        return content;
    };
    slice_from_line_index(content, &lines, index + 1)
}

fn before_current_prompt_marker(content: &str) -> &str {
    let lines: Vec<&str> = content.lines().collect();
    let Some(index) = current_codex_prompt_index(&lines) else {
        return content;
    };
    let byte_offset = lines[..index].iter().map(|line| line.len() + 1).sum::<usize>();
    &content[..byte_offset.min(content.len())]
}

fn whole_recent_without_current_prompt_marker(content: &str) -> &str {
    let lines: Vec<&str> = content.lines().collect();
    if current_codex_prompt_index(&lines).is_some() { "" } else { content }
}

fn current_prompt_block_marker(content: &str) -> Option<&str> {
    let lines: Vec<&str> = content.lines().collect();
    let prompt_index = current_codex_prompt_index(&lines)?;
    lines[..prompt_index].iter().rev().find(|line| codex_block_marker_line(line)).copied()
}

fn after_current_prompt_block_marker(content: &str) -> Option<&str> {
    let lines: Vec<&str> = content.lines().collect();
    let prompt_index = current_codex_prompt_index(&lines)?;
    let block_index = lines[..prompt_index].iter().rposition(|line| codex_block_marker_line(line))?;
    Some(slice_from_line_index(content, &lines, block_index))
}

/// Codex 입력 줄(`›`) 가운데 지금 것: 그 아래에 응답 표지(•■✗✓)가 있으면 지난 입력이다
fn current_codex_prompt_index(lines: &[&str]) -> Option<usize> {
    let prompt_index = lines.iter().rposition(|line| codex_prompt_line(line))?;
    if lines[prompt_index + 1..].iter().any(|line| codex_block_marker_line(line)) {
        return None;
    }
    Some(prompt_index)
}

fn codex_prompt_line(line: &str) -> bool {
    line == "›" || line.starts_with("› ")
}

fn codex_block_marker_line(line: &str) -> bool {
    line.starts_with('•') || line.starts_with('■') || line.starts_with('✗') || line.starts_with('✓')
}

/// 맨 아래에서 두 번째 가로줄과 그 다음 가로줄 사이(Claude 입력 상자의 안쪽)
fn prompt_box_body(content: &str) -> Option<&str> {
    let lines: Vec<&str> = content.lines().collect();
    let top = prompt_box_top_border_index(&lines)?;
    let start = line_start_offset(content, &lines, top + 1);
    let end_index = lines[top + 1..]
        .iter()
        .position(|line| is_horizontal_rule(line))
        .map(|relative| top + 1 + relative)
        .unwrap_or(lines.len());
    let end = line_start_offset(content, &lines, end_index);
    Some(&content[start.min(content.len())..end.min(content.len())])
}

fn above_prompt_box(content: &str) -> &str {
    let lines: Vec<&str> = content.lines().collect();
    let Some(top) = prompt_box_top_border_index(&lines) else {
        return content;
    };
    let end = line_start_offset(content, &lines, top);
    &content[..end.min(content.len())]
}

fn after_last_horizontal_rule(content: &str) -> &str {
    let mut last_rule_end = 0usize;
    let mut offset = 0usize;
    for line in content.lines() {
        let next_offset = offset + line.len() + 1;
        if is_horizontal_rule(line) {
            last_rule_end = next_offset.min(content.len());
        }
        offset = next_offset;
    }
    &content[last_rule_end..]
}

fn last_non_empty_line(content: &str) -> &str {
    content.lines().rev().find(|line| !line.trim().is_empty()).unwrap_or("")
}

fn prompt_box_top_border_index(lines: &[&str]) -> Option<usize> {
    let mut border_count = 0;
    for index in (0..lines.len()).rev() {
        if is_horizontal_rule(lines[index]) {
            border_count += 1;
            if border_count == 2 {
                return Some(index);
            }
        }
    }
    None
}

/// `─`로 시작하는 줄: 줄 전체가 `─`이거나, `─`가 3개 이상 이어진 뒤 다른 글(세션 이름 등)이 붙은 줄
fn is_horizontal_rule(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return false;
    }
    let rule_chars = trimmed.chars().take_while(|&ch| ch == '─').count();
    if rule_chars == 0 {
        return false;
    }
    let rule_bytes = trimmed.char_indices().nth(rule_chars).map(|(index, _)| index).unwrap_or(trimmed.len());
    let suffix = trimmed[rule_bytes..].trim_start();
    suffix.is_empty() || rule_chars >= 3
}

fn slice_from_line_index<'a>(content: &'a str, lines: &[&str], index: usize) -> &'a str {
    let byte_offset = line_start_offset(content, lines, index);
    &content[byte_offset.min(content.len())..]
}

fn line_start_offset(content: &str, lines: &[&str], index: usize) -> usize {
    lines[..index.min(lines.len())].iter().map(|line| line.len() + 1).sum::<usize>().min(content.len())
}

#[cfg(test)]
mod tests;

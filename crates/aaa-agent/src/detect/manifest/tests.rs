//! herdr 0.9.5 `src/detect/manifest/tests.rs`에서 옮긴 엔진 의미 시험. 원격 갱신·local override·cache 시험은 그 기능을
//! 옮기지 않아 뺐다. 규칙은 합성(시험 전용 id "codex")이다.

use super::*;

fn rules_manifest(rules: &str) -> String {
    format!("\nid = \"codex\"\n\n{rules}\n")
}

fn eval(rules: &str, input: DetectionInput<'_>) -> Detection {
    let loaded = load(&rules_manifest(rules), Harness::Codex).unwrap();
    evaluate(&loaded, input)
}

fn screen(screen: &str) -> DetectionInput<'_> {
    DetectionInput { screen, osc_title: "", osc_progress: "" }
}

#[test]
fn known_agent_no_match_defaults_to_idle_fallback() {
    let d =
        eval("[[rules]]\nid = \"test\"\nstate = \"working\"\ncontains = [\"active-marker\"]\n", screen("unmatched"));
    assert_eq!((d.state, d.visible_idle, d.rule), (State::Idle, false, None));
}

#[test]
fn rule_semantics_apply_gates_priority_and_line_regex() {
    let rules = r#"
[[rules]]
id = "low_contains"
state = "idle"
priority = 1
contains = ["match"]

[[rules]]
id = "high_nested_gates"
state = "working"
priority = 10
contains = ["match"]
all = [
  { any = [{ regex = ["w[io]n"] }, { contains = ["fallback"] }] },
]
not = [
  { contains = ["blocked"] },
]

[[rules]]
id = "line_regex"
state = "blocked"
priority = 20
line_regex = ["^exact line$"]
"#;
    let high = eval(rules, screen("match win"));
    assert_eq!((high.state, high.rule.as_deref()), (State::Working, Some("high_nested_gates")));
    // contains는 대소문자를 가리지 않는다
    assert_eq!(eval(rules, screen("MATCH FALLBACK")).rule.as_deref(), Some("high_nested_gates"));
    let not_gate = eval(rules, screen("match win blocked"));
    assert_eq!((not_gate.state, not_gate.rule.as_deref()), (State::Idle, Some("low_contains")));
    let line = eval(rules, screen("before\nexact line\nafter"));
    assert_eq!((line.state, line.rule.as_deref()), (State::Blocked, Some("line_regex")));
    assert_eq!(eval(rules, screen("before exact line")).rule.as_deref(), None);
}

#[test]
fn equal_priority_keeps_the_earlier_rule() {
    let rules = r#"
[[rules]]
id = "first"
state = "idle"
priority = 5
contains = ["x"]

[[rules]]
id = "second"
state = "working"
priority = 5
contains = ["x"]
"#;
    assert_eq!(eval(rules, screen("x")).rule.as_deref(), Some("first"));
}

// 끝(\z)에 묶은 표지: 그 아래에 새 출력이 오면 지난 화면이다
#[test]
fn end_anchored_footer_gates_allow_only_auxiliary_tail() {
    let rules = r#"
[[rules]]
id = "live_controls"
state = "blocked"
region = "whole_recent"
regex = ['(?i)(?:^|\n)controls:\s+(?:confirm-marker|select-marker\s+navigate-marker)\s+cancel-marker[ \t]*\n---[^\n]*(?:\npanel: \d+(?:\nrow: [^\n]+)+(?:\n… \+\d+ pending)?)?(?:\n[ \t]*)*\z']
"#;
    let footer = "controls: select-marker navigate-marker cancel-marker";
    for tail in [
        "--- name",
        "--- name\npanel: 5\nrow: active\nrow: pending\nrow: pending\nrow: pending\nrow: done\n",
        "--- name\npanel: 5\nrow: active\nrow: done\n… +3 pending",
    ] {
        let live = format!("old output\n{footer}\n{tail}");
        assert_eq!(eval(rules, screen(&live)).state, State::Blocked);
        for later in ["new output", "prompt:", "--- next\nprompt:", "…ordinary output", "… +2 arbitrary words"] {
            assert_eq!(eval(rules, screen(&format!("{live}\n{later}"))).state, State::Idle, "{later}");
        }
    }
    for tail in ["--- name\n… +2 pending", "--- name\nrow: active", "--- name\npanel: 5"] {
        assert_eq!(eval(rules, screen(&format!("{footer}\n{tail}"))).state, State::Idle);
    }
}

#[test]
fn paired_alternatives_require_complete_gates_at_bottom_buffer() {
    let rules = r#"
[[rules]]
id = "paired"
state = "blocked"
region = "bottom_non_empty_lines(3)"
visible_blocker = true
any = [
  { all = [
    { contains = ["alpha heading"] },
    { regex = ['alpha\s+control\s*\z'] },
  ] },
  { all = [
    { contains = ["beta heading"] },
    { regex = ['beta\s+control\s*\z'] },
  ] },
]
"#;
    for text in ["alpha heading\nalpha control", "beta heading\nbeta\ncontrol"] {
        let d = eval(rules, screen(text));
        assert_eq!((d.state, d.visible_blocker), (State::Blocked, true));
    }
    for text in [
        "alpha heading\nbeta control",
        "beta heading\nalpha control",
        "beta control",
        "beta heading\nbeta control\nnewer content",
        "beta heading\nfiller one\nfiller two\nbeta control",
    ] {
        let d = eval(rules, screen(text));
        assert!(d.rule.is_none() && !d.visible_blocker, "{text}");
    }
}

#[test]
fn osc_regions_use_separate_inputs_and_share_rule_priority() {
    let rules = r#"
[[rules]]
id = "screen"
state = "idle"
priority = 10
region = "whole_recent"
visible_idle = true
contains = ["screen-marker"]

[[rules]]
id = "title"
state = "working"
priority = 20
region = "osc_title"
visible_working = true
regex = ['^title-marker$']

[[rules]]
id = "progress"
state = "blocked"
priority = 30
region = "osc_progress"
visible_blocker = true
regex = ['^progress-marker$']
"#;
    for (screen, osc_title, osc_progress, state, rule) in [
        ("screen-marker", "", "", State::Idle, "screen"),
        ("screen-marker", "title-marker", "", State::Working, "title"),
        ("screen-marker", "title-marker", "progress-marker", State::Blocked, "progress"),
        ("screen-marker title-marker progress-marker", "", "", State::Idle, "screen"),
    ] {
        let d = eval(rules, DetectionInput { screen, osc_title, osc_progress });
        assert_eq!((d.state, d.rule.as_deref()), (state, Some(rule)));
        assert_eq!(
            (d.visible_idle, d.visible_working, d.visible_blocker),
            (state == State::Idle, state == State::Working, state == State::Blocked)
        );
    }
    let swapped =
        eval(rules, DetectionInput { screen: "", osc_title: "progress-marker", osc_progress: "title-marker" });
    assert!(swapped.rule.is_none());
}

#[test]
fn skip_rule_suppresses_state_update_without_visible_state_evidence() {
    let rules = r#"
[[rules]]
id = "activity"
state = "working"
priority = 10
visible_working = true
contains = ["activity-marker"]

[[rules]]
id = "overlay"
state = "unknown"
priority = 20
skip_state_update = true
contains = ["overlay-marker"]
"#;
    let d = eval(rules, screen("activity-marker overlay-marker"));
    assert_eq!((d.state, d.skip_state_update, d.rule.as_deref()), (State::Unknown, true, Some("overlay")));
    assert!(!d.visible_idle && !d.visible_working && !d.visible_blocker);
}

#[test]
fn screen_regions_extract_structure_without_classifying_agent_state() {
    for (screen, spec, expected) in [
        ("old\n\nnew\n", "bottom_lines(2)", "\nnew\n"),
        ("before\n› input\nafter\n", "after_last_prompt_marker", "after\n"),
        ("before\n› input\nafter\n", "before_current_prompt_marker", "before\n"),
        ("before\n› input\nafter\n", "whole_recent_without_current_prompt_marker", ""),
        ("no marker\n", "whole_recent_without_current_prompt_marker", "no marker\n"),
        ("• old\n■ latest\n› input\n", "current_prompt_block_marker", "■ latest"),
        ("• old\n■ latest\n› input\n", "after_current_prompt_block_marker", "■ latest\n› input\n"),
        ("› old\n• new\n", "current_prompt_block_marker", ""),
        ("above\n\n───\nbody\n───\nfooter\n", "above_prompt_box", "above\n\n"),
        ("above\n\n───\nbody\n───\nfooter\n", "last_non_empty_above_prompt_box", "above"),
        ("above\n───\nbody\n───\nfooter\n", "prompt_box_body", "body\n"),
        ("above\n───\nbody\n───\nfooter\n", "after_last_horizontal_rule", "footer\n"),
        // 세션 이름이 붙은 가로줄도 가로줄이다(`─` 3개 이상), `─ x`는 아니다
        ("a\n──── name ──\nb\n", "after_last_horizontal_rule", "b\n"),
        ("a\n─ x\nb\n", "after_last_horizontal_rule", "a\n─ x\nb\n"),
        ("marker\nold\n\nmiddle\nmarker\nnew\n", "bottom_non_empty_lines(2)", "marker\nnew\n"),
        ("\nmarker\nold\n\nmiddle\nmarker\nnew\n", "top_non_empty_lines(2)", "\nmarker\nold\n"),
    ] {
        assert_eq!(region(DetectionInput { screen, osc_title: "", osc_progress: "" }, spec), expected, "region={spec}");
    }
}

#[test]
fn bundled_manifests_parse_and_validate() {
    for h in [Harness::Claude, Harness::Codex] {
        let loaded = bundled(h);
        // herdr 0.9.5가 쓰는 판(설치본 `herdr agent explain`의 manifest_version)과 같아야 한다
        assert_eq!(loaded.version(), Some("2026.10.11.1"), "{}", h.label());
    }
    // 이름이 다른 규칙 파일은 받지 않는다(claude.toml에는 alias "claude-code"가 있다)
    assert!(load(CLAUDE, Harness::Codex).is_err());
    assert!(load(CODEX, Harness::Claude).is_err());
}

#[test]
fn manifest_validation_rejects_unknown_fields_empty_rules_invalid_regions_and_regexes() {
    for bad in [
        "[[rules]]\nid = \"typo\"\nstate = \"working\"\ncontain = [\"Working\"]\n",
        "[[rules]]\nid = \"empty\"\nstate = \"working\"\n",
        "[[rules]]\nid = \"bad_region\"\nstate = \"working\"\nregion = \"after_last_promt_marker\"\ncontains = [\"W\"]\n",
        "[[rules]]\nid = \"bad_regex\"\nstate = \"working\"\nregex = [\"[\"]\n",
        "[[rules]]\nid = \"bad_nested_regex\"\nstate = \"working\"\nany = [{ line_regex = [\"[\"] }]\n",
        // 상태를 바꾸지 않는 규칙은 상태·표지를 갖지 않는다
        "[[rules]]\nid = \"bad_skip\"\nstate = \"idle\"\nskip_state_update = true\ncontains = [\"menu\"]\n",
        "[[rules]]\nid = \"bad_skip_visible\"\nstate = \"unknown\"\nskip_state_update = true\nvisible_blocker = true\ncontains = [\"m\"]\n",
        "",
    ] {
        assert!(parse_manifest(&rules_manifest(bad)).is_err(), "{bad}");
    }
}

#[test]
fn manifest_validation_rejects_excessive_rules_depth_and_matchers() {
    let mut many = String::new();
    for index in 0..129 {
        many.push_str(&format!("[[rules]]\nid = \"rule_{index}\"\nstate = \"idle\"\ncontains = [\"ready\"]\n"));
    }
    assert!(parse_manifest(&rules_manifest(&many)).is_err());

    let mut gate = "{ contains = [\"9\"] }".to_string();
    for depth in (1..=8).rev() {
        gate = format!("{{ contains = [\"{depth}\"], all = [{gate}] }}");
    }
    let deep = format!("[[rules]]\nid = \"deep\"\nstate = \"idle\"\ncontains = [\"ready\"]\nall = [{gate}]\n");
    assert!(parse_manifest(&rules_manifest(&deep)).is_err());

    let matchers = (0..33).map(|index| format!("\"m{index}\"")).collect::<Vec<_>>().join(", ");
    let wide = format!("[[rules]]\nid = \"many\"\nstate = \"idle\"\ncontains = [{matchers}]\n");
    assert!(parse_manifest(&rules_manifest(&wide)).is_err());
}

#[test]
fn top_non_empty_lines_requires_a_canonical_positive_bounded_count_and_engine_three() {
    let name = "top_non_empty_lines";
    assert!(validate_region_name(&format!("{name}(1)")).is_ok());
    assert!(validate_region_name(&format!("{name}({})", u16::MAX)).is_ok());
    for count in ["0", "01", "+1", "65536", "999999999999999999999999"] {
        assert!(validate_region_name(&format!("{name}({count})")).is_err(), "{count}");
    }
    let old_engine = "\nid = \"codex\"\nversion = \"1\"\nmin_engine_version = 2\n\n[[rules]]\nid = \"b\"\nstate = \"working\"\nregion = \" top_non_empty_lines(1) \"\ncontains = [\"active\"]\n";
    assert!(parse_manifest(old_engine).is_err());
    // 이 엔진보다 새 엔진을 요구하는 규칙 파일은 읽지 않는다
    let future =
        "\nid = \"codex\"\nmin_engine_version = 4\n\n[[rules]]\nid = \"x\"\nstate = \"idle\"\ncontains = [\"a\"]\n";
    assert!(load(future, Harness::Codex).is_err());
}

//! harness 상태의 화면 감지(결정 20 D8, plan §8.110). herdr 0.9.5처럼 화면 끝과 OSC 제목·진행으로 Claude·Codex 상태를 정한다.
//!
//! - 규칙 엔진은 herdr `src/detect/manifest.rs`, 규칙은 herdr `src/detect/manifests/{claude,codex}.toml`(Apache-2.0,
//!   `LICENSE-herdr.txt`)을 옮겼다(Codex run-state 제목 보완은 plan §8.230). 상태 전이(언제 알리나)는 herdr
//!   `src/pane/agent_detection.rs`(`state.rs`)다.
//! - hook은 herdr처럼 harness 세션 ID(SessionStart)를 잇는 데만 쓴다. 상태는 이 모듈만 낸다.

pub mod foreground;
mod manifest;
mod state;
mod watch;

pub use state::{Detector, POLL, POLL_PENDING_IDLE, Published, STARTUP_GRACE, Tick};
pub use watch::{Report, Watch};

/// 화면으로 상태를 아는 harness(aaa가 띄우는 것). herdr는 24개 agent 규칙을 갖지만 aaa profile은 Claude·Codex뿐이다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Harness {
    Claude,
    Codex,
}

impl Harness {
    pub fn label(self) -> &'static str {
        match self {
            Harness::Claude => "claude",
            Harness::Codex => "codex",
        }
    }

    /// herdr `lookup_agent`의 Claude·Codex 이름(경로는 끝 이름만, `.exe` 등 확장자는 뗀다: `normalized_agent_lookup_name`)
    pub fn from_label(name: &str) -> Option<Harness> {
        let mut name = name.trim().to_lowercase();
        for suffix in [".exe", ".cmd", ".bat", ".ps1", ".js"] {
            if name.ends_with(suffix) {
                name.truncate(name.len() - suffix.len());
                break;
            }
        }
        let base = name.rsplit(['/', '\\']).find(|c| !c.is_empty()).unwrap_or(&name);
        match base {
            "claude" | "claude-code" => Some(Harness::Claude),
            "codex" => Some(Harness::Codex),
            _ => None,
        }
    }

    /// 세션의 harness: 띄운 쪽이 알려 준 것(profile harness)이 먼저, 그것으로 모르면(shell·custom) 실행 파일 이름.
    /// 이것으로 모르는 pane(셸)은 herdr처럼 앞 프로세스 이름으로 찾는다(`foreground`, plan §8.110).
    pub fn of_session(declared: Option<&str>, argv: &[String]) -> Option<Harness> {
        declared.and_then(Harness::from_label).or_else(|| argv.first().and_then(|a| Harness::from_label(a)))
    }

    /// 감지 규칙 판(herdr manifest `version`)
    pub fn rules_version(self) -> &'static str {
        manifest::bundled(self).version().unwrap_or("")
    }
}

/// herdr `AgentState`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// 끝나서 입력을 기다린다
    Idle,
    Working,
    /// 사람의 답(확인창·질문)을 기다린다
    Blocked,
    Unknown,
}

/// 감지 입력: 화면 끝 글(herdr `detection_text`)과 마지막 OSC 0/2 제목, OSC 9 진행 payload. 모르면 빈 글.
#[derive(Debug, Clone, Copy)]
pub struct DetectionInput<'a> {
    pub screen: &'a str,
    pub osc_title: &'a str,
    pub osc_progress: &'a str,
}

/// herdr `AgentDetection`과 맞은 규칙 ID
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detection {
    pub state: State,
    /// 화면이 지금 상태가 아니라 기록 보기(transcript viewer)라 상태를 바꾸지 않는다
    pub skip_state_update: bool,
    pub visible_idle: bool,
    pub visible_blocker: bool,
    pub visible_working: bool,
    pub rule: Option<String>,
}

/// 한 번 평가한다(herdr `detect_with_osc`)
pub fn detect(harness: Harness, input: DetectionInput<'_>) -> Detection {
    manifest::evaluate(manifest::bundled(harness), input)
}

/// 화면 줄(보이는 화면, `ScreenText.lines`)로 감지 글을 만든다. herdr `ghostty_recent_read_range`처럼 마지막 내용 줄과
/// 커서 줄 가운데 아래쪽까지, 끝 빈 줄은 빼고 줄마다 `\n`으로 끝낸다. 화면 위 기록은 없다: 감지 글을 내지 못하는
/// 옛 session host(SESSION_IPC 1.0)에 쓴다.
pub fn text_from_screen(lines: &[String], cursor_row: u16) -> String {
    let last = lines.iter().rposition(|l| !l.trim().is_empty());
    let end = match last {
        Some(i) => i.max(usize::from(cursor_row)).min(lines.len().saturating_sub(1)),
        None => return String::new(),
    };
    let mut rows: Vec<&str> = lines[..=end].iter().map(|l| l.trim_end()).collect();
    while rows.last().is_some_and(|r| r.trim().is_empty()) {
        rows.pop();
    }
    if rows.is_empty() {
        return String::new();
    }
    format!("{}\n", rows.join("\n"))
}

/// 정리 표지를 찾는 범위(화면 끝에서 내용 줄 수). Claude는 표지 아래에 입력 상자·대기 글·상태 줄을 둔다(실측 9줄).
const COMPACTING_ROWS: usize = 16;

/// 대화 정리(compaction) 중인가(plan §8.304). 참고 herdr의 규칙에는 정리 상태가 없어(정리 중 화면은 작업 중이다) aaa가 더한다.
/// 표지는 실제 harness 화면이다(`fixtures/*compact*`, 10-20 실측): Claude 2.1.304 spinner 줄
/// `✽ Compacting conversation… (5s · ↓ 168 tokens)`(manual·auto 같은 글), Codex 0.163 `• Compacting context (4s • esc to interrupt)`.
/// 줄 맨 앞(Claude spinner 글자, Codex `•`)부터 맞춰 사람 글·답 글(`❯ `·`⏺ `·`› `로 시작)이 흉내 내지 못하게 한다.
/// 화면 표지는 300ms마다 다시 읽는 상태라 끝 신호를 놓치지 않는다: 표지가 사라지면 정리가 끝났다(hook은 쓰지 않는다).
pub fn compacting(harness: Harness, screen: &str) -> bool {
    use std::sync::LazyLock;
    static CLAUDE: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"^[\x{002A}\x{00B7}\x{2722}\x{2733}\x{2736}\x{273B}\x{273D}]\s+Compacting conversation…").unwrap()
    });
    static CODEX: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"^•\s+Compacting context \(\d+[smh]").unwrap());
    let marker = match harness {
        Harness::Claude => &*CLAUDE,
        Harness::Codex => &*CODEX,
    };
    let rows: Vec<&str> = screen.lines().filter(|l| !l.trim().is_empty()).collect();
    rows[rows.len().saturating_sub(COMPACTING_ROWS)..].iter().any(|l| marker.is_match(l))
}

/// 질문 본문을 찾는 범위(화면 끝에서 내용 줄 수). host `modal_of`의 선택 창 범위(`DIALOG_ROWS`)와 같다.
const QUESTION_ROWS: usize = 24;
const QUESTION_MAX_CHARS: usize = 300;

/// 확인창 감지(blocked)의 질문 종류와 본문. herdr에는 없는 aaa 질문 API용이다(plan §8.110).
/// - 종류: 폴더 신뢰(`trust`), 도구·명령 허락(`permission`), 그 밖(`confirm`). host `modal_of`처럼 창에 trust가 보이면 trust다.
/// - 본문: host `modal_of`와 같이 화면 끝 쪽에서 물음표가 있는 마지막 줄, 없으면 마지막 내용 줄(300자까지).
pub fn question_of(rule: Option<&str>, screen: &str) -> (&'static str, String) {
    let rows: Vec<&str> = screen.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let dialog = &rows[rows.len().saturating_sub(QUESTION_ROWS)..];
    let all = dialog.join(" ");
    let kind = if rule == Some("trust_directory") || all.contains("trust") || all.contains("Trust") {
        "trust"
    } else if matches!(
        rule,
        Some("bash_permission_prompt" | "generic_permission_prompt" | "live_strong_blocker" | "weak_blocker")
    ) {
        "permission"
    } else {
        "confirm"
    };
    let text = dialog.iter().rev().find(|l| l.contains('?')).or(dialog.last()).copied().unwrap_or_default();
    (kind, text.chars().take(QUESTION_MAX_CHARS).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 실제 화면 fixture가 herdr 원본 엔진과 같은 상태·규칙·보이는 표지로 판정된다(`fixtures/expected.txt`)
    #[test]
    fn real_screens_match_the_reference_engine() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/detect/fixtures");
        let expected = std::fs::read_to_string(dir.join("expected.txt")).unwrap();
        let mut n = 0;
        for line in expected.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
            let [file, state, rule, flags] = line.split_whitespace().collect::<Vec<_>>()[..] else {
                panic!("expected.txt 줄 형식: {line}")
            };
            let harness = Harness::from_label(file.split('-').next().unwrap()).unwrap();
            let screen = std::fs::read_to_string(dir.join(file)).unwrap();
            let d = detect(harness, DetectionInput { screen: &screen, osc_title: "", osc_progress: "" });
            let got_state = match d.state {
                State::Idle => "idle",
                State::Working => "working",
                State::Blocked => "blocked",
                State::Unknown => "unknown",
            };
            let got_flags: String = [(d.visible_idle, 'i'), (d.visible_blocker, 'b'), (d.visible_working, 'w')]
                .iter()
                .filter(|(on, _)| *on)
                .map(|(_, c)| *c)
                .collect();
            let got =
                (got_state, d.rule.as_deref().unwrap_or("-"), if got_flags.is_empty() { "-" } else { &got_flags });
            assert_eq!(got, (state, rule, flags), "{file}");
            assert!(!d.skip_state_update, "{file}");
            n += 1;
        }
        assert_eq!(n, 9, "fixture 수");
    }

    /// 실제 정리 화면(10-20 실측, Claude 2.1.304 `/compact`·Codex 0.163 `/compact`, 정리 중 친 글이 대기열에 든 것 포함)은
    /// 참고 엔진으로 작업 중이고 정리 표지가 있다. 정리가 끝난 화면(「Compacted」·「Context compacted · 6s」)과 다른 작업 중·쉼·
    /// 확인창 화면에는 표지가 없다
    #[test]
    fn compaction_screens_are_working_with_the_marker() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/detect/fixtures");
        for (file, compacting_now) in [
            ("claude-compacting.txt", true),
            ("claude-compacting-queued.txt", true),
            ("codex-compacting.txt", true),
            ("codex-compacting-queued.txt", true),
            ("claude-compacted.txt", false),
            ("codex-compacted.txt", false),
            ("claude-working.txt", false),
            ("claude-idle.txt", false),
            ("claude-bash-permission.txt", false),
            ("codex-working.txt", false),
            ("codex-idle.txt", false),
        ] {
            let harness = Harness::from_label(file.split('-').next().unwrap()).unwrap();
            let screen = std::fs::read_to_string(dir.join(file)).unwrap();
            assert_eq!(compacting(harness, &screen), compacting_now, "{file}");
            if compacting_now {
                // 참고 규칙만으로는 쉼일 수 있다(Claude 「(0s)」는 입력 상자): 감지기가 작업 중·정리 중으로 알린다
                let t0 = std::time::Instant::now();
                let mut d = Detector::new(harness, t0 - STARTUP_GRACE);
                assert_eq!(d.tick(t0), Tick::Wait);
                let input = DetectionInput { screen: &screen, osc_title: "", osc_progress: "" };
                let p = d.observe(t0, None, input).unwrap();
                assert_eq!((p.state, p.compacting), (State::Working, true), "{file}: {:?}", p.rule);
            }
        }
        // 참고 규칙의 판정(그래서 표지를 덧댄다): 요약 글이 오기 전 Claude는 쉼, 글이 오는 중이면 작업 중
        let read = |f: &str| std::fs::read_to_string(dir.join(f)).unwrap();
        let reference = |h, f: &str| detect(h, DetectionInput { screen: &read(f), osc_title: "", osc_progress: "" }).state;
        assert_eq!(reference(Harness::Claude, "claude-compacting.txt"), State::Idle);
        assert_eq!(reference(Harness::Claude, "claude-compacting-queued.txt"), State::Working);
        // 사람 글·답 글 안의 같은 말은 표지가 아니다
        let quoted = "❯ ✻ Compacting conversation… (3s)\n⏺ ✻ Compacting conversation… 라고 보였다\n";
        assert!(!compacting(Harness::Claude, quoted));
        assert!(!compacting(Harness::Codex, "› • Compacting context (3s • esc to interrupt)\n"));
    }

    #[test]
    fn question_kind_and_line_come_from_the_dialog() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/detect/fixtures");
        let read = |f: &str| std::fs::read_to_string(dir.join(f)).unwrap();
        let (kind, text) = question_of(Some("live_blocked_form"), &read("claude-trust.txt"));
        assert_eq!(kind, "trust");
        assert!(text.starts_with("Quick safety check: Is this a project you created"), "{text}");
        assert_eq!(
            question_of(Some("bash_permission_prompt"), &read("claude-bash-permission.txt")),
            ("permission", "Do you want to proceed?".to_string())
        );
        assert_eq!(
            question_of(Some("legacy_no_prompt_blocker"), &read("claude-create-file.txt")),
            ("confirm", "Do you want to create report.md?".to_string())
        );
        let (kind, text) = question_of(Some("live_strong_blocker"), &read("codex-approval.txt"));
        assert_eq!(kind, "permission");
        assert!(text.starts_with("Reason: "), "물음표가 있는 마지막 줄: {text}");
    }
}

//! Ctrl+Enter 공통 판정(웹). host mirror의 detection 입력줄과 같은 화면 ANSI를 읽는다.
//! herdr-web-ui aa624cb `server/prompt.ts` claudeInputEmpty와 `server/codex-steering.ts`를 옮겼다.
//! 스타일 판독은 기존 herdr-projects 입력 상자 판정기(prompt_box.rs)를 재사용한다.
//! 바꾼 점(사용자 10-19 「control+enter 줄바꿈은 필수인거야? 스티어링만 남기고 싶어」, plan §8.239 후속): Ctrl+Enter는
//! steering만 하고 줄바꿈은 넣지 않는다(줄바꿈은 Shift+Enter). 입력 상태를 모르면 아무것도 보내지 않는다.
//! 정정(사용자 10-19 15:1x): agent가 작업 중일 때만 동작한다. 쉬는 Claude에 글이 있어도 보내지 않는다(쉬는 Codex와 같다).
//!
//! MIT License
//!
//! Copyright (c) 2026 devswha
//!
//! Permission is hereby granted, free of charge, to any person obtaining a copy
//! of this software and associated documentation files (the "Software"), to deal
//! in the Software without restriction, including without limitation the rights
//! to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
//! copies of the Software, and to permit persons to whom the Software is
//! furnished to do so, subject to the following conditions:
//!
//! The above copyright notice and this permission notice shall be included in all
//! copies or substantial portions of the Software.
//!
//! THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
//! IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
//! FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
//! AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
//! LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
//! OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
//! SOFTWARE.

use crate::prompt_box::{self, Draft};
use aaa_proto::SteerRefusal;
use regex::Regex;
use std::sync::OnceLock;

/// 판정과 키 선택은 host에서 한 번만 한다. 일반 프로그램은 기존 Ctrl+Enter다. 빈 목록이면 보내지 않는다.
/// `cursor_home`: Codex 커서가 입력 줄 시작 칸에 있다(빈 입력 판정에만 쓴다).
/// `working`: host가 아는 그 pane의 agent 상태가 작업 중이다(사람이 목록에서 보는 「작업 중」과 같다). 아니거나 모르면 보내지 않는다.
///
/// - Claude Code: 작업 중이고(상태와 현재 footer·spinner 작업 표시가 함께) 살아 있는 입력 상자가 확인되면 비었든 글이
///   있든 `chat:sendNow`(Ctrl+X Ctrl+S). 2.1.303 기본 바인딩은 `ctrl+x ctrl+s`·`ctrl+enter`가 모두 sendNow이고, 처리기는 글(붙여넣기
///   포함)이 있으면 그 글을 지금 보내고(진행 중 턴을 끊는다) 비었으면 대기 메시지를 지금 보낸다. 쉬는 중의 sendNow는 그냥 제출이라
///   steering이 아니므로 보내지 않는다. 상자가 없으면(확인창·`/model` 등 다른 화면의 Ctrl+S는 다른 뜻) 보내지 않는다.
/// - Codex 0.163: 빈 입력 + 현재 대기 steering 패널이면 Esc(「interrupt and send immediately」). 한 줄 글 + 작업 중이면 Enter:
///   작업 중 제출은 `pending_steers`로 가는 정식 steering이다(`chatwidget/input_submission.rs` `render_in_history =
///   !agent_turn_running`). 쉬는 중 Enter는 새 턴 제출이라 steering이 아니므로 보내지 않는다. 목록(slash·`!` 등)이 떠
///   footer가 다르면 Enter가 고르기라 보내지 않는다.
pub(crate) fn ctrl_enter_keys(
    harness: Option<&str>,
    live: &str,
    colors: Option<&str>,
    cursor_home: bool,
    working: bool,
) -> &'static [&'static str] {
    match harness {
        None => &["ctrl+enter"],
        Some(_) if !working => &[],
        Some("claude") if claude_working_line(live) && claude_input_live(live, colors) => &["ctrl+x", "ctrl+s"],
        Some("codex") => {
            let draft = colors.map(|ansi| prompt_box::check("codex", ansi));
            match codex_state(live) {
                Some(Codex { pending: true, typed: false }) if cursor_home && draft == Some(Draft::Empty) => &["esc"],
                Some(Codex { typed: true, .. }) if draft == Some(Draft::Typed) => &["enter"],
                _ => &[],
            }
        }
        Some(_) => &[],
    }
}

/// 웹 입력창(대화 입력창·터미널 입력줄)이 Ctrl+Enter로 보낸 글의 마지막 키(사용자 10-19 15:3x, plan §8.239 후속). host는 글을 친 뒤
/// Enter 대신 이것을 보낸다. 위 판정과 같은 조건으로 Claude가 작업 중이면 sendNow(Ctrl+X Ctrl+S)다. 상태와 현재 화면의 작업 표시가
/// 함께 있고 입력 상자가 살아 있어야 하며, 그러면 진행 중 턴에 그 글을 지금 넣는다. 그 밖은 Enter, 곧 보통 보내기다.
/// Codex는 작업 중 Enter가 정식 steering이다(`pending_steers`). 위 판정의 Esc는 빈 입력에서만 뜻이 있고 글이 있으면 쓰지 않는다.
/// 쉬는 agent는 사용자 결정대로 보통 보내기다. 작업 중인지 모르면(web은 작업 중으로 보았지만 화면이 확인하지 않음)
/// 다음 턴으로 줄 서는 보통 보내기다. 글은 버리지 않는다.
pub(crate) fn steer_submit_keys(
    harness: Option<&str>,
    live: &str,
    colors: Option<&str>,
    working: bool,
) -> &'static [&'static str] {
    match harness {
        Some("claude") if working && claude_working_line(live) && claude_input_live(live, colors) => {
            &["ctrl+x", "ctrl+s"]
        }
        _ => &["enter"],
    }
}

/// 대기 메시지 「지금 보내기」(plan §8.270). harness 대기열에 이미 있는 글을 지금 턴에 넣는 키만 고른다(글은 다시 보내지 않는다).
/// 빈 입력 Ctrl+Enter와 같은 키지만 더 좁다. 입력창에 보내지 않은 글이 있으면 그 키가 그 글까지 보내므로 거절하고, 대기 글이
/// 화면에 보여야 한다(없으면 「보냄」이 거짓이다). herdr-web-ui `actOnPending`이 터미널 draft를 `held_input`으로 거절하는 것과 같다.
///
/// - Claude Code 2.1.303: 대기 글 아래에 늘 `<sendNow 키> to send now`(dim) 줄을 그린다(`$_e`, 실측 fixture `claude-queued.ansi`).
///   빈 입력 sendNow는 대기열 전부를 지금 보낸다: 돌던 도구는 끊지 않고 배경으로 옮기거나(실측) 턴을 끊는다.
///   대기열이 비면 아무 일도 하지 않는다(`nothing_to_send`). 입력창의 「Press up to edit queued messages」는 처음 몇 번만 나와 쓰지 않는다.
/// - Codex 0.163: 대기 steering 패널이 있고 입력이 빌 때만 Esc. Esc는 pending_steers가 있으면 턴을 끊고 전부 보내지만, 비면 그냥
///   중단이다(`chatwidget/interaction.rs` 247-259). 그래서 패널을 이 판정(쓰기 직전, 같은 mirror)에서 확인한다.
pub(crate) fn queued_steer_keys(
    harness: Option<&str>,
    live: &str,
    colors: Option<&str>,
    cursor_home: bool,
    working: bool,
) -> Result<&'static [&'static str], SteerRefusal> {
    match harness {
        Some("claude" | "codex") if !working => Err(SteerRefusal::NotWorking),
        Some("claude") => {
            if !(claude_working_line(live) && claude_input_live(live, colors)) {
                return Err(SteerRefusal::UnknownScreen);
            }
            match colors.map(|ansi| prompt_box::check("claude", ansi)) {
                Some(Draft::Empty) => {}
                Some(Draft::Typed) => return Err(SteerRefusal::Draft),
                _ => return Err(SteerRefusal::UnknownScreen),
            }
            if claude_queue_hint(live) { Ok(&["ctrl+x", "ctrl+s"]) } else { Err(SteerRefusal::NoQueue) }
        }
        Some("codex") => {
            let draft = colors.map(|ansi| prompt_box::check("codex", ansi));
            match codex_state(live) {
                None => Err(SteerRefusal::UnknownScreen),
                Some(Codex { typed: true, .. }) => Err(SteerRefusal::Draft),
                _ if draft == Some(Draft::Typed) => Err(SteerRefusal::Draft),
                Some(Codex { pending: false, .. }) => Err(SteerRefusal::NoQueue),
                Some(_) if cursor_home && draft == Some(Draft::Empty) => Ok(&["esc"]),
                Some(_) => Err(SteerRefusal::UnknownScreen),
            }
        }
        _ => Err(SteerRefusal::Unsupported),
    }
}

/// Claude 입력 상자 위(대기 글 자리)에 `… to send now` 줄이 있다. 보낸 뒤에는 그 자리를 다시 그려 사라진다(실측 `claude-queue-sent.ansi`)
fn claude_queue_hint(live: &str) -> bool {
    let lines: Vec<&str> = live.lines().map(str::trim_end).collect();
    let rule = |line: &&str| {
        let t = line.trim();
        t.starts_with("───") || (!t.is_empty() && t.chars().all(|c| c == '─'))
    };
    let Some(bottom) = lines.iter().rposition(rule) else { return false };
    let Some(top) = lines[..bottom].iter().rposition(rule) else { return false };
    lines[top.saturating_sub(20)..top].iter().any(|line| {
        let t = line.trim();
        t.ends_with(" to send now") && t.len() > " to send now".len()
    })
}

pub(crate) fn diagnostic(harness: Option<&str>, live: &str, colors: Option<&str>) -> String {
    match harness {
        Some("claude") => {
            format!(
                "working_line={} working_source={} input_live={} queue_hint={}",
                claude_working_line(live),
                claude_working_source(live),
                claude_input_live(live, colors),
                claude_queue_hint(live)
            )
        }
        Some("codex") => {
            format!("state={:?} draft={:?}", codex_state(live), colors.map(|c| prompt_box::check("codex", c)))
        }
        _ => "unsupported_or_plain=true".to_string(),
    }
}

// manifest의 prompt_box_body와 같은 마지막 두 가로줄 사이. 닫히지 않은 상자는 모름이다.
fn claude_body(screen: &str) -> Option<String> {
    let lines: Vec<&str> = screen.lines().map(str::trim_end).collect();
    let rule = |line: &&str| {
        let t = line.trim();
        t.starts_with("───") || (!t.is_empty() && t.chars().all(|c| c == '─'))
    };
    let bottom = lines.iter().rposition(rule)?;
    let top = lines[..bottom].iter().rposition(rule)?;
    let body = lines[top + 1..bottom].join("\n");
    body.starts_with('❯').then_some(body)
}

/// 지금 화면에 Claude 입력 상자가 있다(비었든 글이 있든). detection은 scrollback까지 읽으므로 같은 화면 색 사본에도 같은 본문이 있어야 한다
fn claude_input_live(live: &str, colors: Option<&str>) -> bool {
    let (Some(body), Some(colors)) = (claude_body(live), colors) else {
        return false;
    };
    claude_body(&prompt_box::plain(colors)).as_ref() == Some(&body)
}

/// 입력 상자 바로 위(대기 메시지 몇 줄을 넘어) Claude 작업 상태 줄 `✻ Working… (1s · esc to interrupt)`이 있다.
/// herdr claude.toml의 working 표지(`esc to interrupt`)와 같다. 상자를 모르면 false
fn claude_working_line(live: &str) -> bool {
    claude_working_source(live) != "none"
}

fn claude_working_source(live: &str) -> &'static str {
    let lines: Vec<&str> = live.lines().map(str::trim_end).collect();
    let rule = |line: &&str| {
        let t = line.trim();
        t.starts_with("───") || (!t.is_empty() && t.chars().all(|c| c == '─'))
    };
    let Some(bottom) = lines.iter().rposition(rule) else { return "none" };
    let Some(top) = lines[..bottom].iter().rposition(rule) else { return "none" };
    // 2.1.303은 spinner에는 시간·interrupt 안내를 두지 않고 입력 상자 아래 footer에 둔다.
    // 입력 상자 뒤의 현재 footer만 읽는다(위 scrollback의 지난 작업 안내는 증거가 아니다).
    if lines[bottom + 1..]
        .iter()
        .take(3)
        .any(|line| line.trim().split('·').any(|part| part.trim() == "esc to interrupt"))
    {
        return "footer";
    }
    if lines[top.saturating_sub(6)..top].iter().any(|line| line.trim_end().ends_with("esc to interrupt)")) {
        return "legacy";
    }
    // paste toast가 footer를 가린 때도 현재 spinner는 남는다. agent manifest의 live_turn_working과 같다.
    static SPINNER: OnceLock<Regex> = OnceLock::new();
    let spinner = SPINNER.get_or_init(|| {
        Regex::new(
            r"^\s*[\x{002A}\x{00B7}\x{2722}\x{2733}\x{2736}\x{273B}\x{273D}]\s+\S.*…(?:\s+\(\d+[smh](?:\s|·)|\s*$)",
        )
        .unwrap()
    });
    if lines[top.saturating_sub(6)..top].iter().any(|line| spinner.is_match(line)) { "spinner" } else { "none" }
}

const STEERING_HINT: &str =
    "Messages to be submitted after next tool call (press esc to interrupt and send immediately)";
fn compact(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

/// 작업 중인 Codex 입력 줄의 상태
#[derive(Debug, PartialEq, Eq)]
struct Codex {
    /// 입력 위에 현재 대기 steering 패널(안내 + ↳ 항목)이 있다
    pending: bool,
    /// 입력 줄에 글이 있다(자리표시 글이 아닌)
    typed: bool,
}

// 참고 codexSteeringPending. 아래에서부터 입력 줄(›)·알려진 footer·Working 줄을 찾고, 그 사이는 비었거나 대기 패널이어야 한다.
// 작업 중이 아니거나 화면을 모르면 None이다.
fn codex_state(live: &str) -> Option<Codex> {
    let rows: Vec<&str> = live.lines().map(|l| l.trim_end()).collect();
    let composer = rows.iter().rposition(|l| *l == "›" || l.starts_with("› "))?;
    // 편집 중인 여러 줄의 첫 줄이 비었어도 아래 글을 footer로 오인하지 않는다.
    if rows.get(composer + 1).is_some_and(|line| !line.trim().is_empty()) {
        return None;
    }
    let footer: Vec<&str> = rows[composer + 1..].iter().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
    static FOOTER: OnceLock<Regex> = OnceLock::new();
    let footer_re = FOOTER.get_or_init(|| Regex::new(r"^(?:\S+\s+(?:default|minimal|none|low|medium|high|xhigh|max|ultra)\s*·|\d+% context left\b|⚠\s*\d+\s*warnings?\b|(?:←[^·]*·\s*)?\?\s*for shortcuts\b)").unwrap());
    if footer.len() > 4 || footer.iter().any(|l| !footer_re.is_match(l)) {
        return None;
    }
    static STATUS: OnceLock<Regex> = OnceLock::new();
    let status_re = STATUS.get_or_init(|| Regex::new(r"^[•◦]\s*(?:Working|Reconnecting(?:\.{3}|…)?(?: waiting for network)?)\s*\((?:\d+[hms]\s*)+•\s*esc to interrupt\)$").unwrap());
    let status = rows[..composer].iter().rposition(|l| status_re.is_match(l.trim()))?;
    let panel: Vec<&str> = rows[status + 1..composer].iter().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
    let pending = match panel.iter().position(|l| l.starts_with("↳ ") && !l["↳ ".len()..].trim().is_empty()) {
        Some(first) => compact(panel[..first].join("").trim_start_matches(['•', '◦'])) == compact(STEERING_HINT),
        None => false,
    };
    // Working 줄과 입력 사이에 대기 패널 말고 다른 글이 있으면 지난 화면이다
    if !panel.is_empty() && !pending {
        return None;
    }
    let text = rows[composer].strip_prefix('›').unwrap_or_default().trim();
    Some(Codex { pending, typed: !text.is_empty() && text != "Ask Codex to do anything" })
}
#[cfg(test)]
mod tests {
    use super::*;

    // 작업 중인 Claude: 상태 줄 + 입력 상자
    const EMPTY: &str =
        "done\n✻ Working… (1s · esc to interrupt)\n\n────────────────────\n❯\n────────────────────\nmode\n";
    const TEXT: &str = "done\n✻ Working… (1s · esc to interrupt)\n\n────────────────────\n❯ 한글\n  다음 줄\n────────────────────\nmode\n";
    const NONE: &[&str] = &[];

    #[test]
    fn ctrl_enter_steers_and_never_inserts_a_newline() {
        // 실제 2.1.303 pane: spinner에는 안내가 없고 입력 아래 footer에 있다.
        let modern = "❯ queued message\n  ctrl+x ctrl+s to send now\n✻ Shimmying…\n\n────────────────────\n❯ Press up to edit queued messages\n────────────────────\n  ⏸ manual mode on · esc to interrupt · ← for agents\n";
        for (harness, screen, expected) in [
            (None, EMPTY, &["ctrl+enter"][..]),
            (Some("claude"), EMPTY, &["ctrl+x", "ctrl+s"][..]),
            (Some("claude"), modern, &["ctrl+x", "ctrl+s"][..]),
            (
                Some("claude"),
                &modern.replace("⏸ manual mode on · esc to interrupt · ← for agents", "paste again to expand"),
                &["ctrl+x", "ctrl+s"][..],
            ),
            // 글이 있어도 sendNow: Claude가 그 글을 지금 보낸다
            (Some("claude"), TEXT, &["ctrl+x", "ctrl+s"][..]),
            (Some("claude"), "❯\n────────────────────\n", NONE),
            (Some("claude"), "tool output\n", NONE),
            (Some("other"), EMPTY, NONE),
            (Some("unknown"), EMPTY, NONE),
            (Some("codex"), "›\n", NONE),
        ] {
            assert_eq!(ctrl_enter_keys(harness, screen, Some(screen), true, true), expected, "{harness:?} {screen:?}");
        }
        // 일반 프로그램은 상태와 상관없이 그 프로그램의 Ctrl+Enter다
        assert_eq!(ctrl_enter_keys(None, "", None, false, false), ["ctrl+enter"]);
    }

    #[test]
    fn steered_text_ends_with_send_now_only_for_a_working_claude() {
        let idle_text = TEXT.replace("✻ Working… (1s · esc to interrupt)\n", "");
        // 작업 중인 Claude의 상자(글이 화면에 아직 없어도 살아 있는 상자): 지금 턴에 넣는다
        for screen in [TEXT, EMPTY] {
            assert_eq!(
                steer_submit_keys(Some("claude"), screen, Some(screen), true),
                ["ctrl+x", "ctrl+s"],
                "{screen:?}"
            );
        }
        // 쉬는 Claude·상태 모름·화면에 작업 줄 없음·상자 모름은 보통 보내기(Enter): 글은 다음 턴으로 간다
        for (screen, working, what) in [
            (idle_text.as_str(), false, "쉬는 중"),
            (TEXT, false, "상태가 작업 중이 아니다"),
            (idle_text.as_str(), true, "화면에 작업 줄이 없다"),
            ("tool output\n", true, "상자 모름"),
        ] {
            assert_eq!(steer_submit_keys(Some("claude"), screen, Some(screen), working), ["enter"], "{what}");
        }
        // Codex는 작업 중이든 아니든 Enter: 작업 중 Enter가 steering이고 대기 패널의 Esc는 쓰지 않는다
        let pending = codex("›", true, true);
        for working in [true, false] {
            assert_eq!(steer_submit_keys(Some("codex"), &pending, Some(&pending), working), ["enter"]);
        }
        for harness in [None, Some("unknown")] {
            assert_eq!(steer_submit_keys(harness, TEXT, Some(TEXT), true), ["enter"]);
        }
    }

    #[test]
    fn ctrl_enter_acts_only_while_the_agent_works() {
        let idle_text = TEXT.replace("✻ Working… (1s · esc to interrupt)\n", "");
        let idle_empty = EMPTY.replace("✻ Working… (1s · esc to interrupt)\n", "");
        for (screen, working, what) in [
            // 쉬는 Claude의 글은 sendNow가 그냥 제출한다: 보내지 않는다(쉬는 Codex와 같다)
            (idle_text.as_str(), false, "쉬는 중 글"),
            (idle_empty.as_str(), false, "쉬는 중 빈 입력"),
            // host 상태와 화면이 함께 작업 중이어야 한다(어느 하나라도 모르면 보내지 않는다)
            (TEXT, false, "상태가 작업 중이 아니다"),
            (idle_text.as_str(), true, "화면에 작업 줄이 없다"),
            // 상자 위 몇 줄보다 먼 지난 작업 줄은 지금 것이 아니다
            (
                &TEXT.replace(
                    "✻ Working… (1s · esc to interrupt)\n\n",
                    "✻ Working… (1s · esc to interrupt)\n1\n2\n3\n4\n5\n6\n",
                ),
                true,
                "먼 작업 줄",
            ),
        ] {
            assert_eq!(ctrl_enter_keys(Some("claude"), screen, Some(screen), true, working), NONE, "{what}");
        }
        let pending = codex("›", true, true);
        assert_eq!(
            ctrl_enter_keys(Some("codex"), &pending, Some(&pending), true, false),
            NONE,
            "Codex도 상태가 작업 중이어야 한다"
        );
    }

    #[test]
    fn claude_needs_the_live_box_in_both_reads() {
        let hint = EMPTY.replace("❯", "❯ Try something");
        let grey = hint.replace("❯ Try something", "❯ \x1b[7mT\x1b[27;2mry something\x1b[0m");
        assert_eq!(ctrl_enter_keys(Some("claude"), &hint, Some(&grey), true, true), ["ctrl+x", "ctrl+s"]);
        // 색 사본이 없거나 다른 본문이면(지난 상자) 보내지 않는다
        for colors in [None, Some(grey.replace("something", "different"))] {
            assert_eq!(ctrl_enter_keys(Some("claude"), &hint, colors.as_deref(), true, true), NONE);
        }
        // 친 글·붙여넣기·그림 자리표시도 sendNow가 지금 보낸다
        for content in [
            "\x1b[38;2;80;90;100mtyped\x1b[0m",
            "\x1b[7mx\x1b[0m",
            "\x1b[2m[Image #1]\x1b[0m",
            "[Pasted text #1 +20 lines]",
        ] {
            let ansi = EMPTY.replace("❯", &format!("❯ {content}"));
            let live = prompt_box::plain(&ansi);
            assert_eq!(ctrl_enter_keys(Some("claude"), &live, Some(&ansi), true, true), ["ctrl+x", "ctrl+s"]);
        }
    }

    // 실제 Claude Code 2.1.303 화면(tmux capture -e, 10-19 t-0417): 작업 중 대기 글 하나 / 빈 입력 sendNow로 보낸 직후
    const CLAUDE_QUEUED: &str = include_str!("../tests/fixtures/prompt_box/claude-queued.ansi");
    const CLAUDE_QUEUE_SENT: &str = include_str!("../tests/fixtures/prompt_box/claude-queue-sent.ansi");

    fn queued(
        harness: &str,
        ansi: &str,
        cursor_home: bool,
        working: bool,
    ) -> Result<&'static [&'static str], SteerRefusal> {
        queued_steer_keys(Some(harness), &prompt_box::plain(ansi), Some(ansi), cursor_home, working)
    }

    #[test]
    fn queued_steer_sends_claude_send_now_only_for_an_empty_box_over_a_visible_queue() {
        assert_eq!(queued("claude", CLAUDE_QUEUED, true, true), Ok(&["ctrl+x", "ctrl+s"][..]));
        assert_eq!(queued("claude", CLAUDE_QUEUED, true, false), Err(SteerRefusal::NotWorking));
        // 보낸 뒤: 작업 중이고 상자는 비었지만 대기 글이 없다(sendNow는 아무 일도 하지 않으니 「보냄」이라 하지 않는다)
        assert_eq!(queued("claude", CLAUDE_QUEUE_SENT, true, true), Err(SteerRefusal::NoQueue));
        // 터미널에서 쓰던 글이 있으면 sendNow가 그 글까지 보낸다: 거절
        let placeholder = "❯\u{a0}\x1b[2m\x1b[39mPress up to edit queued messages\x1b[0m";
        assert!(CLAUDE_QUEUED.contains(placeholder));
        let typed = CLAUDE_QUEUED.replace(placeholder, "❯ 쓰던 글");
        assert_eq!(queued("claude", &typed, true, true), Err(SteerRefusal::Draft));
        // 색이 없으면(NO_COLOR) 자리표시인지 알 수 없다: 글로 보고 거절한다
        let plain = prompt_box::plain(CLAUDE_QUEUED);
        assert_eq!(queued("claude", &plain, true, true), Err(SteerRefusal::Draft));
        // 상자를 모름(확인창 등)·작업 표시 없음
        assert_eq!(queued("claude", "tool output\n", true, true), Err(SteerRefusal::UnknownScreen));
        let idle = CLAUDE_QUEUED.replace("Beboppin'…", "Beboppin'").replace("(38s · ↓", "38s ↓");
        assert_eq!(queued("claude", &idle, true, true), Err(SteerRefusal::UnknownScreen), "작업 줄이 없다");
        // 지난 작업 안내가 아니라 상자 위 대기 자리의 줄만 증거다
        let far = CLAUDE_QUEUED.replace("ctrl+x ctrl+s to send now", "");
        assert_eq!(queued("claude", &far, true, true), Err(SteerRefusal::NoQueue));
        // 같은 화면의 Ctrl+Enter 판정은 그대로다(빈 입력 sendNow)
        let live = prompt_box::plain(CLAUDE_QUEUED);
        assert_eq!(ctrl_enter_keys(Some("claude"), &live, Some(CLAUDE_QUEUED), true, true), ["ctrl+x", "ctrl+s"]);
    }

    #[test]
    fn queued_steer_sends_codex_esc_only_over_the_pending_panel() {
        let pending = codex("›", true, true);
        assert_eq!(queued("codex", &pending, true, true), Ok(&["esc"][..]));
        // 패널이 없으면 Esc는 그냥 중단이다: 보내지 않는다(실측 0.163은 작업 중 글을 거의 바로 넘겨 패널이 짧게만 보인다)
        assert_eq!(queued("codex", &codex("›", false, true), true, true), Err(SteerRefusal::NoQueue));
        assert_eq!(queued("codex", &codex("› 방향 바꿔", true, true), false, true), Err(SteerRefusal::Draft));
        assert_eq!(queued("codex", &pending, true, false), Err(SteerRefusal::NotWorking));
        assert_eq!(
            queued("codex", &pending, false, true),
            Err(SteerRefusal::UnknownScreen),
            "커서가 입력 시작이 아니다"
        );
        assert_eq!(
            queued("codex", &format!("{pending}Do you want to proceed?\n"), true, true),
            Err(SteerRefusal::UnknownScreen)
        );
        for harness in [None, Some("other")] {
            assert_eq!(
                queued_steer_keys(harness, &pending, Some(&pending), true, true),
                Err(SteerRefusal::Unsupported)
            );
        }
    }

    fn codex(composer: &str, panel: bool, working: bool) -> String {
        let status = if working { "• Working (2s • esc to interrupt)\n" } else { "" };
        let panel = if panel { format!("• {STEERING_HINT}\n  ↳ change course\n") } else { String::new() };
        format!("› task\n{status}{panel}{composer}\n\n  GPT-6.1 low · /workspace\n  ? for shortcuts\n")
    }

    #[test]
    fn codex_esc_sends_a_pending_steer_now_and_enter_steers_a_typed_draft_while_working() {
        let pending = codex("›", true, true);
        assert_eq!(ctrl_enter_keys(Some("codex"), &pending, Some(&pending), true, true), ["esc"]);
        assert_eq!(
            ctrl_enter_keys(Some("codex"), &pending, Some(&pending), false, true),
            NONE,
            "커서가 입력 시작이 아니다"
        );
        let live = pending.replace("›\n\n", "› Ask Codex to do anything\n\n");
        let grey = pending.replace("›\n\n", "› \x1b[2mAsk Codex to do anything\x1b[0m\n\n");
        assert_eq!(ctrl_enter_keys(Some("codex"), &live, Some(&grey), true, true), ["esc"]);
        assert_eq!(ctrl_enter_keys(Some("codex"), &live, Some(&live), true, true), NONE, "자리표시인지 모른다");
        // 작업 중 한 줄 글: Enter가 pending_steers로 보낸다(대기 패널이 있어도 같다)
        for screen in [codex("› 방향 바꿔", false, true), codex("› 방향 바꿔", true, true)] {
            assert_eq!(ctrl_enter_keys(Some("codex"), &screen, Some(&screen), false, true), ["enter"], "{screen}");
        }
        for screen in [
            // 쉬는 중 Enter는 새 턴 제출이지 steering이 아니다
            codex("› 방향 바꿔", false, false),
            codex("›", false, false),
            // 작업 중 빈 입력이라도 대기 메시지가 없으면 Esc는 작업 취소다
            codex("›", false, true),
            codex("›", true, true).replace("• Working", "• Finished"),
            format!("{}• new response\n", codex("›", true, true)),
            format!("{}Do you want to proceed?\n", codex("›", true, true)),
            codex("›", true, true).replace(STEERING_HINT, "quoted hint"),
            codex("› 방향 바꿔", true, true).replace("  ↳ change course\n", "  지난 글\n"),
            codex("›", true, true).replace("›\n\n", "›\n  /typed/path\n\n"),
            // slash 목록이 footer를 덮으면 Enter는 고르기다
            codex("› /mo", false, true).replace("  ? for shortcuts\n", "  /model  choose what model to use\n"),
        ] {
            assert_eq!(ctrl_enter_keys(Some("codex"), &screen, Some(&screen), true, true), NONE, "{screen}");
        }
    }
}

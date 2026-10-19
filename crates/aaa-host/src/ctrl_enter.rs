//! Ctrl+Enter 공통 판정(웹·TUI). host mirror의 detection 입력줄과 같은 화면 ANSI를 읽는다.
//! herdr-web-ui aa624cb `server/prompt.ts` claudeInputEmpty와 `server/codex-steering.ts`를 옮겼다.
//! 스타일 판독은 기존 herdr-projects 입력 상자 판정기(prompt_box.rs)를 재사용한다.
//! 바꾼 점(사용자 10-19 「control+enter 줄바꿈은 필수인거야? 스티어링만 남기고 싶어」, plan §8.239 후속): Ctrl+Enter는
//! steering만 하고 줄바꿈은 넣지 않는다(줄바꿈은 Shift+Enter). 입력 상태를 모르면 아무것도 보내지 않는다.
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
use regex::Regex;
use std::sync::OnceLock;

/// 판정과 키 선택은 host에서 한 번만 한다. 일반 프로그램은 기존 Ctrl+Enter다. 빈 목록이면 보내지 않는다.
/// `cursor_home`: Codex 커서가 입력 줄 시작 칸에 있다(빈 입력 판정에만 쓴다).
///
/// - Claude Code: 살아 있는 입력 상자가 확인되면 비었든 글이 있든 `chat:sendNow`(Ctrl+X Ctrl+S). 2.1.303 기본 바인딩은
///   `ctrl+x ctrl+s`·`ctrl+enter`가 모두 sendNow이고, 처리기는 글(붙여넣기 포함)이 있으면 그 글을 지금 보내고(진행 중 턴을 끊는다)
///   비었으면 대기 메시지를 지금 보낸다. 상자가 없으면(확인창·`/model` 등 다른 화면의 Ctrl+S는 다른 뜻) 보내지 않는다.
/// - Codex 0.163: 빈 입력 + 현재 대기 steering 패널이면 Esc(「interrupt and send immediately」). 한 줄 글 + 작업 중이면 Enter:
///   작업 중 제출은 `pending_steers`로 가는 정식 steering이다(`chatwidget/input_submission.rs` `render_in_history =
///   !agent_turn_running`). 쉬는 중 Enter는 새 턴 제출이라 steering이 아니므로 보내지 않는다. 목록(slash·`!` 등)이 떠
///   footer가 다르면 Enter가 고르기라 보내지 않는다.
pub(crate) fn ctrl_enter_keys(
    harness: Option<&str>,
    live: &str,
    colors: Option<&str>,
    cursor_home: bool,
) -> &'static [&'static str] {
    match harness {
        None => &["ctrl+enter"],
        Some("claude") if claude_input_live(live, colors) => &["ctrl+x", "ctrl+s"],
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

    const EMPTY: &str = "done\n────────────────────\n❯\n────────────────────\nmode\n";
    const TEXT: &str = "done\n────────────────────\n❯ 한글\n  다음 줄\n────────────────────\nmode\n";
    const NONE: &[&str] = &[];

    #[test]
    fn ctrl_enter_steers_and_never_inserts_a_newline() {
        for (harness, screen, expected) in [
            (None, EMPTY, &["ctrl+enter"][..]),
            (Some("claude"), EMPTY, &["ctrl+x", "ctrl+s"][..]),
            // 글이 있어도 sendNow: Claude가 그 글을 지금 보낸다
            (Some("claude"), TEXT, &["ctrl+x", "ctrl+s"][..]),
            (Some("claude"), "❯\n────────────────────\n", NONE),
            (Some("claude"), "tool output\n", NONE),
            (Some("other"), EMPTY, NONE),
            (Some("unknown"), EMPTY, NONE),
            (Some("codex"), "›\n", NONE),
        ] {
            assert_eq!(ctrl_enter_keys(harness, screen, Some(screen), true), expected, "{harness:?} {screen:?}");
        }
    }

    #[test]
    fn claude_needs_the_live_box_in_both_reads() {
        let hint = EMPTY.replace("❯", "❯ Try something");
        let grey = hint.replace("❯ Try something", "❯ \x1b[7mT\x1b[27;2mry something\x1b[0m");
        assert_eq!(ctrl_enter_keys(Some("claude"), &hint, Some(&grey), true), ["ctrl+x", "ctrl+s"]);
        // 색 사본이 없거나 다른 본문이면(지난 상자) 보내지 않는다
        for colors in [None, Some(grey.replace("something", "different"))] {
            assert_eq!(ctrl_enter_keys(Some("claude"), &hint, colors.as_deref(), true), NONE);
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
            assert_eq!(ctrl_enter_keys(Some("claude"), &live, Some(&ansi), true), ["ctrl+x", "ctrl+s"]);
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
        assert_eq!(ctrl_enter_keys(Some("codex"), &pending, Some(&pending), true), ["esc"]);
        assert_eq!(ctrl_enter_keys(Some("codex"), &pending, Some(&pending), false), NONE, "커서가 입력 시작이 아니다");
        let live = pending.replace("›\n\n", "› Ask Codex to do anything\n\n");
        let grey = pending.replace("›\n\n", "› \x1b[2mAsk Codex to do anything\x1b[0m\n\n");
        assert_eq!(ctrl_enter_keys(Some("codex"), &live, Some(&grey), true), ["esc"]);
        assert_eq!(ctrl_enter_keys(Some("codex"), &live, Some(&live), true), NONE, "자리표시인지 모른다");
        // 작업 중 한 줄 글: Enter가 pending_steers로 보낸다(대기 패널이 있어도 같다)
        for screen in [codex("› 방향 바꿔", false, true), codex("› 방향 바꿔", true, true)] {
            assert_eq!(ctrl_enter_keys(Some("codex"), &screen, Some(&screen), false), ["enter"], "{screen}");
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
            assert_eq!(ctrl_enter_keys(Some("codex"), &screen, Some(&screen), true), NONE, "{screen}");
        }
    }
}

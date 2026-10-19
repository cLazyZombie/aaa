//! Ctrl+Enter 공통 판정(웹·TUI). host mirror의 detection 입력줄과 같은 화면 ANSI를 읽는다.
//! herdr-web-ui aa624cb `server/prompt.ts` claudeInputEmpty와 `server/codex-steering.ts`를 옮겼다.
//! 스타일 판독은 기존 herdr-projects 입력 상자 판정기(prompt_box.rs)를 재사용한다.
//! 바꾼 점: NO_COLOR 추천문을 빈 입력으로 추정하지 않는다. 모르면 줄바꿈이다.
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

/// 판정과 키 선택은 host에서 한 번만 한다. 일반 프로그램은 기존 Ctrl+Enter다.
pub(crate) fn ctrl_enter_keys(harness: Option<&str>, live: &str, colors: Option<&str>) -> &'static [&'static str] {
    match harness {
        None => &["ctrl+enter"],
        Some("claude") if claude_input_empty(live, colors) => &["ctrl+x", "ctrl+s"],
        Some("codex")
            if colors.is_some_and(|ansi| prompt_box::check("codex", ansi) == Draft::Empty)
                && codex_steering_pending(live) =>
        {
            &["esc"]
        }
        Some(_) => &["alt+enter"],
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

fn claude_input_empty(live: &str, colors: Option<&str>) -> bool {
    let Some(body) = claude_body(live) else {
        return false;
    };
    if body.contains("[Pasted text #") || body.contains("[Image #") {
        return false;
    }
    let Some(colors) = colors else {
        return false;
    };
    // detection은 scrollback까지 읽는다. 화면에 없는 지난 빈 상자를 현재 입력으로 삼지 않는다.
    if claude_body(&prompt_box::plain(colors)).as_ref() != Some(&body) {
        return false;
    }
    prompt_box::check("claude", colors) == Draft::Empty
}

const STEERING_HINT: &str =
    "Messages to be submitted after next tool call (press esc to interrupt and send immediately)";
fn compact(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

// 참고 codexSteeringPending. 새 글 제출은 아니므로 특정 메시지 대신 대기 ↳ 항목의 존재를 확인한다.
fn codex_steering_pending(live: &str) -> bool {
    let rows: Vec<&str> = live.lines().map(|l| l.trim_end()).collect();
    let Some(composer) = rows.iter().rposition(|l| *l == "›" || l.starts_with("› ")) else {
        return false;
    };
    if !matches!(rows[composer], "›" | "› Ask Codex to do anything") {
        return false;
    }
    // 편집 중인 여러 줄의 첫 줄이 비었어도 아래 글을 footer로 오인하지 않는다.
    if rows.get(composer + 1).is_some_and(|line| !line.trim().is_empty()) {
        return false;
    }
    let footer: Vec<&str> = rows[composer + 1..].iter().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
    static FOOTER: OnceLock<Regex> = OnceLock::new();
    let footer_re = FOOTER.get_or_init(|| Regex::new(r"^(?:\S+\s+(?:default|minimal|none|low|medium|high|xhigh|max|ultra)\s*·|\d+% context left\b|⚠\s*\d+\s*warnings?\b|(?:←[^·]*·\s*)?\?\s*for shortcuts\b)").unwrap());
    if footer.len() > 4 || footer.iter().any(|l| !footer_re.is_match(l)) {
        return false;
    }
    static STATUS: OnceLock<Regex> = OnceLock::new();
    let status_re = STATUS.get_or_init(|| Regex::new(r"^[•◦]\s*(?:Working|Reconnecting(?:\.{3}|…)?(?: waiting for network)?)\s*\((?:\d+[hms]\s*)+•\s*esc to interrupt\)$").unwrap());
    let Some(status) = rows[..composer].iter().rposition(|l| status_re.is_match(l.trim())) else {
        return false;
    };
    let panel: Vec<&str> = rows[status + 1..composer].iter().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
    let Some(first) = panel.iter().position(|l| l.starts_with("↳ ") && !l["↳ ".len()..].trim().is_empty()) else {
        return false;
    };
    let hint = panel[..first].join("");
    compact(hint.trim_start_matches(['•', '◦'])) == compact(STEERING_HINT)
}
#[cfg(test)]
mod tests {
    use super::*;

    const EMPTY: &str = "done\n────────────────────\n❯\n────────────────────\nmode\n";
    const TEXT: &str = "done\n────────────────────\n❯ 한글\n  다음 줄\n────────────────────\nmode\n";

    #[test]
    fn ctrl_enter_is_newline_unless_the_live_agent_input_proves_empty() {
        for (harness, screen, expected) in [
            (None, EMPTY, &["ctrl+enter"][..]),
            (Some("claude"), EMPTY, &["ctrl+x", "ctrl+s"][..]),
            (Some("claude"), TEXT, &["alt+enter"][..]),
            (Some("claude"), "❯\n────────────────────\n", &["alt+enter"][..]),
            (Some("claude"), "tool output\n", &["alt+enter"][..]),
            (Some("other"), EMPTY, &["alt+enter"][..]),
            (Some("codex"), "›\n", &["alt+enter"][..]),
        ] {
            assert_eq!(ctrl_enter_keys(harness, screen, Some(screen)), expected, "{harness:?} {screen:?}");
        }
    }

    #[test]
    fn claude_grey_suggestions_need_matching_colors_and_a_drawn_cursor_is_not_a_draft() {
        let hint = EMPTY.replace("❯", "❯ Try something");
        let grey = hint.replace("❯ Try something", "❯ \x1b[7mT\x1b[27;2mry something\x1b[0m");
        assert_eq!(ctrl_enter_keys(Some("claude"), &hint, Some(&grey)), ["ctrl+x", "ctrl+s"]);
        for colors in [None, Some(hint.as_str()), Some(grey.replace("something", "different").as_str())] {
            assert_eq!(ctrl_enter_keys(Some("claude"), &hint, colors), ["alt+enter"]);
        }
        for content in ["\x1b[38;2;80;90;100mtyped\x1b[0m", "\x1b[7mx\x1b[0m", "\x1b[2m[Image #1]\x1b[0m"] {
            let ansi = EMPTY.replace("❯", &format!("❯ {content}"));
            let live = prompt_box::plain(&ansi);
            assert_eq!(ctrl_enter_keys(Some("claude"), &live, Some(&ansi)), ["alt+enter"]);
        }
    }

    #[test]
    fn codex_only_interrupts_a_current_native_pending_steering_panel_beside_empty_input() {
        let screen = format!(
            "› task\n• Working (2s • esc to interrupt)\n• {STEERING_HINT}\n  ↳ change course\n›\n\n  GPT-6.1 low · /workspace\n  ? for shortcuts\n"
        );
        assert_eq!(ctrl_enter_keys(Some("codex"), &screen, Some(&screen)), ["esc"]);
        let grey = screen.replace("›\n", "› \x1b[2mAsk Codex to do anything\x1b[0m\n");
        let live = screen.replace("›\n", "› Ask Codex to do anything\n");
        assert_eq!(ctrl_enter_keys(Some("codex"), &live, Some(&grey)), ["esc"]);
        assert_eq!(ctrl_enter_keys(Some("codex"), &live, Some(&live)), ["alt+enter"]);
        for screen in [
            screen.replace("›\n", "› typed\n"),
            screen.replace("• Working", "• Finished"),
            format!("{screen}• new response\n"),
            format!("{screen}Do you want to proceed?\n"),
            screen.replace("  ↳ change course\n", ""),
            screen.replace(STEERING_HINT, "quoted hint"),
            screen.replace("›\n\n", "›\n  /typed/path\n\n"),
        ] {
            assert_eq!(ctrl_enter_keys(Some("codex"), &screen, Some(&screen)), ["alt+enter"], "{screen}");
        }
    }
}

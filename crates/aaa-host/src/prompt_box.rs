//! herdr-projects 0.2.35(`1f05c00`) `src/prompt_box.rs`를 그대로 옮겼다(MIT, Copyright (c) 2026 Elias Stravik).
//! 바꾼 것: 이 머리말, 저장소 rustfmt가 나눈 긴 줄 둘, fixture를 aaa vt로 다시 그려도 같게 읽는지 보는 시험 하나(fixture도 같은 상대 경로
//! `tests/fixtures/prompt_box/`에 그대로 옮겼다). `input_box`가 상자 첫 줄의 번호도 돌려주게 하고, 그 번호로 흐린 글만 있는 입력 줄에
//! 표지를 다는 `mark_dim_box`(t-1940: Claude 다음 입력 예측을 색 없는 글에서 친 글로 오인)를 더했다. 화면은 agent session host가 `ScreenText.ansi`(herdr `agent read --format ansi`와 같은 모양)로 보낸다
//! (plan §8.77). kind는 thread launch의 harness(`claude`·`codex` …)다.
//!
//! Is an agent's input box empty? Read from the pane's screen (`agent read
//! --format ansi`), so it works for every harness without hooks: each
//! supported kind has its own anchor for the box, and text drawn dim, a known
//! placeholder, or a harness-drawn cursor over a placeholder does not count as
//! typed. Anything the check cannot place is `Unknown`, and the ticker treats
//! that like a draft: it never types into a box it cannot see is empty.

/// What the input box holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Draft {
    Empty,
    /// Someone typed text that was not submitted.
    Typed,
    /// No input box was found on the screen (a menu or dialog, a scrolled
    /// view, a layout this binary does not know, an unsupported kind).
    Unknown,
}

/// Placeholders drawn in a normal (not dim) colour, per kind. Only strings
/// nobody types by accident.
fn placeholders(kind: &str) -> &'static [&'static str] {
    match kind {
        "gemini" => &["Type your message or @path/to/file"],
        "opencode" => &["Ask anything…"],
        _ => &[],
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Cell {
    ch: char,
    dim: bool,
    reverse: bool,
    bg: String,
}

type Line = Vec<Cell>;

fn text(line: &[Cell]) -> String {
    line.iter().map(|c| c.ch).collect()
}

/// Screen text with SGR styling into cells. Other escape sequences are
/// skipped; only dim, reverse and the background colour are kept.
fn parse(screen: &str) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut line = Line::new();
    let (mut dim, mut reverse, mut bg) = (false, false, String::new());
    let mut chars = screen.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\u{1b}' => {
                if chars.peek() != Some(&'[') {
                    chars.next();
                    continue;
                }
                chars.next();
                let mut params = String::new();
                let mut last = ' ';
                for c in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&c) {
                        last = c;
                        break;
                    }
                    params.push(c);
                }
                if last == 'm' {
                    sgr(&params, &mut dim, &mut reverse, &mut bg);
                }
            }
            '\n' => lines.push(std::mem::take(&mut line)),
            '\r' => {}
            c if c.is_control() => {}
            c => line.push(Cell { ch: c, dim, reverse, bg: bg.clone() }),
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

fn sgr(params: &str, dim: &mut bool, reverse: &mut bool, bg: &mut String) {
    let codes: Vec<&str> = if params.is_empty() { vec!["0"] } else { params.split(';').collect() };
    let mut i = 0;
    while i < codes.len() {
        match codes[i] {
            "0" | "" => {
                *dim = false;
                *reverse = false;
                bg.clear();
            }
            "2" => *dim = true,
            "22" => *dim = false,
            "7" => *reverse = true,
            "27" => *reverse = false,
            "49" => bg.clear(),
            code @ ("38" | "48") => {
                let take = match codes.get(i + 1) {
                    Some(&"5") => 2,
                    Some(&"2") => 4,
                    _ => 0,
                };
                if code == "48" {
                    *bg = codes[i + 1..(i + 1 + take).min(codes.len())].join(";");
                }
                i += take;
            }
            code if code.len() == 2 && code.starts_with('4') => *bg = code.to_string(),
            code if code.len() == 3 && code.starts_with("10") => *bg = code.to_string(),
            _ => {}
        }
        i += 1;
    }
}

/// The cells after the first `glyph` on the line (the anchor itself dropped).
fn after(line: &[Cell], glyph: char) -> Option<Line> {
    let at = line.iter().position(|c| c.ch == glyph)?;
    Some(line[at + 1..].to_vec())
}

fn trimmed_starts(line: &[Cell], prefix: &str) -> bool {
    text(line).trim_start().starts_with(prefix)
}

/// A horizontal rule: a line of box-drawing `─` only.
fn is_rule(line: &[Cell]) -> bool {
    let t = text(line);
    let t = t.trim();
    t.chars().count() >= 10 && t.chars().all(|c| c == '─')
}

/// The top border of Claude Code's input box: a rule that may carry one label.
///
///   - The session name, an agent name or a tmux hint: `──── label ─`.
///   - Effort and fast-mode tags, right-aligned: `──── ultracode  fast mode ─`.
///   - The history position, left-aligned: `─── History 3/12 ────`.
///   - The bottom border never carries a label, so only the top is read this way.
fn is_labelled_rule(line: &[Cell]) -> bool {
    let t = text(line);
    let t = t.trim();
    let dashes = t.chars().filter(|c| *c == '─').count();
    dashes >= 10 && t.starts_with("──") && t.split('─').filter(|s| !s.trim().is_empty()).count() <= 1
}

/// The typed text in a box's cells: non-blank, not dim, and not a
/// harness-drawn cursor sitting on a dim placeholder.
fn typed(cells: &[Cell]) -> String {
    let mut out = String::new();
    for (i, c) in cells.iter().enumerate() {
        if c.dim || c.ch.is_whitespace() {
            out.push(' ');
            continue;
        }
        if c.reverse && cells.get(i + 1).is_some_and(|next| next.dim) {
            out.push(' ');
            continue;
        }
        out.push(c.ch);
    }
    out
}

/// The input box's first line index and cells, one entry per line, or `None`
/// when it is not on the screen.
fn input_box(kind: &str, lines: &[Line]) -> Option<(usize, Vec<Line>)> {
    let last =
        |pred: &dyn Fn(usize, &Line) -> bool| lines.iter().enumerate().rev().find(|(i, l)| pred(*i, l)).map(|(i, _)| i);
    match kind {
        // `❯` right under the top border, continued until the next rule.
        "claude" => {
            let at = last(&|i, l| trimmed_starts(l, "❯") && i > 0 && is_labelled_rule(&lines[i - 1]))?;
            let end = (at + 1..lines.len()).find(|&i| is_rule(&lines[i]))?;
            let mut rows = vec![after(&lines[at], '❯')?];
            rows.extend(lines[at + 1..end].iter().cloned());
            Some((at, rows))
        }
        "codex" => last(&|_, l| trimmed_starts(l, "›")).and_then(|at| Some((at, vec![after(&lines[at], '›')?]))),
        // agy 1.5.13. 슬래시 목록·사용량 창은 상자 바로 아래에 별도 줄을 그린다.
        "agy" => {
            let footer = last(&|_, l| !text(l).trim().is_empty())?;
            let shortcuts = trimmed_starts(&lines[footer], "? for shortcuts");
            let hint = text(&lines[footer]);
            // 1.5.13은 글이 입력되면 단축키 표지를 숨기고 우측 모델·effort만 그린다.
            let model_hint = hint.starts_with("  ") && hint.contains(" · ") && !hint.contains("esc");
            if !shortcuts && !model_hint {
                return None;
            }
            let bottom = (0..footer).rev().find(|&i| !text(&lines[i]).trim().is_empty())?;
            if !is_rule(&lines[bottom]) {
                return None;
            }
            let at = (0..bottom).rev().find(|&i| is_rule(&lines[i]))? + 1;
            if at >= bottom
                || !matches!(text(&lines[at]).trim_start().strip_prefix('>'), Some(rest) if rest.is_empty() || rest.starts_with(' '))
            {
                return None;
            }
            let mut rows = vec![after(&lines[at], '>')?];
            rows.extend(lines[at + 1..bottom].iter().cloned());
            if !shortcuts && rows.iter().all(|row| typed(row).trim().is_empty()) {
                return None;
            }
            Some((at, rows))
        }
        "cursor" => last(&|_, l| trimmed_starts(l, "→")).and_then(|at| Some((at, vec![after(&lines[at], '→')?]))),
        // `│ > ` (or `!` shell mode, `*` yolo mode) in a bordered box.
        "gemini" => {
            let at = last(&|_, l| {
                let t = text(l);
                let t = t.trim_start();
                t.starts_with('│') && matches!(t['│'.len_utf8()..].trim_start().chars().next(), Some('>' | '!' | '*'))
            })?;
            let end = (at + 1..lines.len()).find(|&i| trimmed_starts(&lines[i], "╰"))?;
            let mut rows = Vec::new();
            for (n, line) in lines[at..end].iter().enumerate() {
                let mut row = after(line, '│')?;
                if let Some(close) = row.iter().rposition(|c| c.ch == '│') {
                    row.truncate(close);
                }
                if n == 0 {
                    let prompt = row.iter().position(|c| matches!(c.ch, '>' | '!' | '*'))?;
                    row.drain(..=prompt);
                }
                rows.push(row);
            }
            Some((at, rows))
        }
        // The `┃` lines above the box's `╹▀▀▀` bottom, less the last one (the
        // mode and model line); only cells on the box's own background count,
        // so the session sidebar to its right is left out.
        "opencode" => {
            let bottom = last(&|_, l| trimmed_starts(l, "╹"))?;
            let top = (0..bottom).rev().take_while(|&i| trimmed_starts(&lines[i], "┃")).last()?;
            if bottom - top < 2 {
                return None;
            }
            let rows = lines[top..bottom - 1]
                .iter()
                .map(|line| {
                    let row = after(line, '┃').unwrap_or_default();
                    let bg = row.first().map(|c| c.bg.clone()).unwrap_or_default();
                    row.into_iter().take_while(|c| c.bg == bg).collect()
                })
                .collect();
            Some((top, rows))
        }
        // The editor between the last two rules.
        "pi" => {
            let bottom = last(&|_, l| is_rule(l))?;
            let top = (0..bottom).rev().find(|&i| is_rule(&lines[i]))?;
            Some((top + 1, lines[top + 1..bottom].to_vec()))
        }
        _ => None,
    }
}

/// Reads the input box of an agent of `kind` from its screen, as `agent read
/// --source visible --format ansi` prints it.
pub fn check(kind: &str, screen: &str) -> Draft {
    match box_text(kind, screen) {
        None => Draft::Unknown,
        Some(text) if text.is_empty() => Draft::Empty,
        Some(_) => Draft::Typed,
    }
}

/// The text typed in the input box, its lines joined by spaces (empty for an
/// empty box), or `None` when the box is not on the screen.
pub fn box_text(kind: &str, screen: &str) -> Option<String> {
    let lines = parse(screen);
    let (_, rows) = input_box(kind, &lines)?;
    let typed: Vec<String> = rows.iter().map(|row| typed(row).trim().to_string()).filter(|t| !t.is_empty()).collect();
    match typed.as_slice() {
        [only] if placeholders(kind).iter().any(|p| only.starts_with(p)) => Some(String::new()),
        _ => Some(typed.join(" ")),
    }
}

/// 입력 줄의 흐린 글 표지(t-1940). 색을 뺀 글에서는 흐린 글이 친 글처럼 보인다.
pub const DIM_MARK: &str = "[흐린 글·입력 아님]";

/// 색을 뺀 화면 글 `text`에서, 입력창에 친 글 없이 흐린 글(Claude 다음 입력 예측·placeholder)만 있는 입력 줄에 `DIM_MARK`를
/// 단다. `ansi`는 같은 화면을 같은 자리에서 읽은 색 있는 글(`ReadFormat::Ansi`, 줄 수가 같다)이다. 입력 줄 표지 글자가 있는
/// harness(Claude `❯`·Codex `›`·Cursor `→`)만 보고, 두 글의 그 줄이 맞지 않거나 친 글이 섞였으면 그대로 돌려준다.
pub fn mark_dim_box(kind: &str, text: &str, ansi: &str) -> String {
    let glyph = match kind {
        "claude" => '❯',
        "codex" => '›',
        "cursor" => '→',
        _ => return text.to_string(),
    };
    let lines = parse(ansi);
    let Some((at, rows)) = input_box(kind, &lines) else { return text.to_string() };
    if rows.iter().any(|row| !typed(row).trim().is_empty()) {
        return text.to_string();
    }
    // 흐린 칸과 흐린 글 위의 커서 칸(`typed`가 빈칸으로 치는 것)
    let first = &rows[0];
    let dim: String = first
        .iter()
        .enumerate()
        .filter(|(i, c)| c.dim || (c.reverse && first.get(i + 1).is_some_and(|next| next.dim)))
        .map(|(_, c)| c.ch)
        .filter(|ch| !ch.is_whitespace())
        .collect();
    if dim.is_empty() {
        return text.to_string();
    }
    let mut out: Vec<String> = text.split('\n').map(str::to_string).collect();
    let Some(row) = out.get_mut(at) else { return text.to_string() };
    let Some(pos) = row.find(glyph) else { return text.to_string() };
    let rest = &row[pos + glyph.len_utf8()..];
    if rest.chars().filter(|ch| !ch.is_whitespace()).collect::<String>() != dim {
        return text.to_string();
    }
    *row = format!("{}{glyph} {DIM_MARK} {}", &row[..pos], rest.trim());
    out.join("\n")
}

/// True for the kinds whose input box this module can find.
pub fn knows(kind: &str) -> bool {
    matches!(kind, "claude" | "codex" | "agy" | "cursor" | "gemini" | "opencode" | "pi")
}

/// The screen's plain text, styling dropped.
pub fn plain(screen: &str) -> String {
    parse(screen).iter().map(|line| text(line)).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agy_uses_the_current_shortcuts_footer_and_bordered_prompt() {
        let rule = "─".repeat(20);
        let screen = |input| format!("오래된 대화\n{rule}\n> {input}\n{rule}\n? for shortcuts Gemini 3.8 Flash\n");
        assert_eq!(check("agy", &screen("")), Draft::Empty);
        assert_eq!(box_text("agy", &screen("first\n  second")).as_deref(), Some("first second"));
        let typing = |input| screen(input).replace("? for shortcuts Gemini 3.8 Flash", "     Gemini 3.8 Flash · low");
        assert_eq!(box_text("agy", &typing("first")).as_deref(), Some("first"));
        assert_eq!(check("agy", &typing("")), Draft::Unknown);
        assert_eq!(
            check(
                "agy",
                &screen("/").replace("? for shortcuts", "↑/↓ Navigate · enter Select · tab Complete\n? for shortcuts")
            ),
            Draft::Unknown
        );
        assert_eq!(check("agy", &screen("").replace("? for shortcuts", "esc to cancel")), Draft::Unknown);
    }

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(format!("{}/tests/fixtures/prompt_box/{name}.ansi", env!("CARGO_MANIFEST_DIR")))
            .unwrap()
    }

    /// aaa 화면은 session host의 vt가 다시 그린 `ScreenText.ansi`다. 참고 fixture(herdr `agent read --format ansi`)를
    /// vt에 먹여 다시 그려도 같은 판정이 나와야 한다(aaa에서 더한 시험, plan §8.77).
    #[test]
    fn the_session_hosts_redrawn_screen_reads_the_same() {
        for kind in ["claude", "codex", "cursor", "gemini", "opencode", "pi"] {
            for (name, want) in [("empty", Draft::Empty), ("draft", Draft::Typed)] {
                let raw = fixture(&format!("{kind}-{name}"));
                let rows = raw.lines().count().max(1) as u16;
                let cols = raw.lines().map(|l| plain(l).chars().count()).max().unwrap_or(80).max(80) as u16;
                let mut vt = aaa_vt::Vt::new(rows, cols);
                vt.feed(raw.replace('\n', "\r\n").trim_end_matches(['\r', '\n']).as_bytes());
                let ansi = vt.text("s").ansi;
                assert_eq!(check(kind, &ansi), want, "{kind}-{name}\n{ansi}");
            }
        }
    }

    #[test]
    fn every_supported_kind_tells_an_empty_box_from_a_draft() {
        for kind in ["claude", "codex", "cursor", "gemini", "opencode", "pi"] {
            assert_eq!(check(kind, &fixture(&format!("{kind}-empty"))), Draft::Empty, "{kind} empty");
            assert_eq!(check(kind, &fixture(&format!("{kind}-draft"))), Draft::Typed, "{kind} draft");
        }
        assert_eq!(check("claude", &fixture("claude-empty-after-turn")), Draft::Empty);
        assert_eq!(check("opencode", &fixture("opencode-empty-session")), Draft::Empty);
    }

    #[test]
    fn the_top_border_may_carry_one_label() {
        // Captured from Claude Code 2.1.300 started with `--name hp-demo-t-0001`.
        assert_eq!(check("claude", &fixture("claude-named-session-draft")), Draft::Typed);
        assert_eq!(check("claude", &fixture("claude-named-session-empty")), Draft::Empty);
        let d = |n: usize| "─".repeat(n);
        let cases = [
            // Shapes Claude Code draws.
            ("no label", d(70), true),
            ("session name", format!("{} hp-demo-t-0001 ─", d(52)), true),
            ("session name reaching the pane's edge", format!("{} polish-release-notes", d(54)), true),
            ("agent name", format!("{} @reviewer ─", d(58)), true),
            ("label with spaces", format!("{} View teammates: `tmux -L claude-swarm a` ─", d(27)), true),
            ("fast-mode tag and session name", format!("{} fast mode  hp-demo-t-0001 ─", d(41)), true),
            ("effort and fast-mode tags", format!("{} ultracode  fast mode /fast ─", d(42)), true),
            ("history position, left-aligned", format!("─── History 3/12 {}", d(53)), true),
            ("dim label", format!("─── \u{1b}[2mHistory\u{1b}[22m {}", d(58)), true),
            ("coloured border", format!("\u{1b}[38;5;135m{} hp-demo-t-0001 ─\u{1b}[39m", d(52)), true),
            ("indented", format!("  {} hp-demo-t-0001 ─", d(50)), true),
            ("non-Latin label", format!("{} Архитектура трёхслойной системы ──", d(40)), true),
            // Lines that only resemble a border.
            ("two labels", format!("{} a {} b {}", d(10), d(10), d(10)), false),
            ("text before the dashes", format!("note {}", d(60)), false),
            ("prose between two dashes", "─ a note about the box ─".to_string(), false),
            ("fewer than ten dashes", "───── short ─".to_string(), false),
            ("other box-drawing characters", format!("╭{}╮", d(60)), false),
            // Claude Code truncates a label to the pane's width less three, which can leave no dashes before it.
            ("label filling a narrow pane", format!(" {} ─", "a-very-long-session-name-that-fills-it"), false),
        ];
        for (name, top, want) in cases {
            assert_eq!(is_labelled_rule(&parse(&top).remove(0)), want, "{name}: {top}");
            let (empty, typed) = if want { (Draft::Empty, Draft::Typed) } else { (Draft::Unknown, Draft::Unknown) };
            assert_eq!(check("claude", &format!("{top}\n❯ \n{}\n", d(70))), empty, "{name}, empty box");
            assert_eq!(check("claude", &format!("{top}\n❯ hello\n{}\n", d(70))), typed, "{name}, typed box");
        }
    }

    #[test]
    fn a_labelled_box_is_read_like_a_plain_one() {
        let plain = "─".repeat(40);
        for top in [
            format!("{} Архитектура трёхслойной системы ──", "─".repeat(60)),
            format!("── Session name {}", "─".repeat(30)),
        ] {
            // Claude Code draws a no-break space after `❯`.
            assert_eq!(check("claude", &format!("{top}\n❯\u{a0}\n{plain}\n")), Draft::Empty, "{top}");
            assert_eq!(
                check("claude", &format!("{top}\n❯\u{a0}\u{1b}[7m \u{1b}[0m\n{plain}\n")),
                Draft::Empty,
                "{top}"
            );
            assert_eq!(
                check("claude", &format!("{top}\n❯ \u{1b}[2mTry \"fix lint\"\u{1b}[0m\n{plain}\n")),
                Draft::Empty,
                "{top}"
            );
            assert_eq!(
                check("claude", &format!("{top}\n❯\u{a0}привет, проверь тесты\n{plain}\n")),
                Draft::Typed,
                "{top}"
            );
            assert_eq!(check("claude", &format!("{top}\n❯\u{a0}\n  вторая строка\n{plain}\n")), Draft::Typed, "{top}");
            // A transcript `❯` above the box is still not the box.
            assert_eq!(
                check("claude", &format!("❯ an earlier prompt\n\n{top}\n❯\u{a0}\n{plain}\n")),
                Draft::Empty,
                "{top}"
            );
            assert_eq!(check("claude", &format!("❯ an earlier prompt\n{top}\n")), Draft::Unknown, "{top}");
        }
    }

    #[test]
    fn only_the_top_border_may_carry_a_label() {
        let d = |n: usize| "─".repeat(n);
        let labelled = format!("{} hp-demo-t-0001 ─", d(52));
        // A labelled line does not close the box: it runs on to the plain rule.
        let open = format!("{labelled}\n❯ \n{labelled}\n");
        assert_eq!(check("claude", &open), Draft::Unknown);
        let runs_on = format!("{labelled}\n❯ \n{labelled}\n{}\n", d(70));
        assert_eq!(check("claude", &runs_on), Draft::Typed);
        // Pi's editor sits between two plain rules; a labelled line is not one of them.
        let pi = format!("{}\n{labelled}\n{}\n", d(70), d(70));
        assert_eq!(check("pi", &pi), Draft::Typed);
    }

    #[test]
    fn a_screen_without_the_box_or_an_unknown_kind_is_unknown() {
        assert_eq!(check("copilot", &fixture("claude-empty")), Draft::Unknown);
        assert_eq!(check("claude", "some output\nno box here\n"), Draft::Unknown);
        assert_eq!(check("codex", &fixture("claude-empty")), Draft::Unknown);
    }

    #[test]
    fn menus_and_multi_line_drafts_count_as_typed() {
        // Codex's trust menu uses the same `›` for its selection.
        let menu =
            "  Do you trust the contents of this directory?\n\u{1b}[1m›\u{1b}[0m 1. Yes, continue\n  2. No, quit\n";
        assert_eq!(check("codex", menu), Draft::Typed);
        let rule = "─".repeat(40);
        let multi = format!("{rule}\n❯ \n  second line typed\n{rule}\n");
        assert_eq!(check("claude", &multi), Draft::Typed);
        let placeholder = format!("{rule}\n❯ \u{1b}[2mTry \"fix lint\"\u{1b}[0m\n{rule}\n");
        assert_eq!(check("claude", &placeholder), Draft::Empty);
        // A transcript line with `❯` that is not under a rule is not the box.
        let transcript = format!("❯ an earlier prompt\n\n{rule}\n❯ \n{rule}\n");
        assert_eq!(check("claude", &transcript), Draft::Empty);
    }

    /// Claude Code 2.1.326이 턴 끝에 그리는 다음 입력 예측(t-1940 운영 화면의 꼴: 회색 줄 사이 `❯`, NBSP, SGR 2 글). 글은 지어냈다.
    fn claude_suggestion(box_line: &str) -> String {
        let rule = format!("\u{1b}[38;2;136;136;136m{}\u{1b}[m", "─".repeat(60));
        format!(
            "\u{1b}[38;2;153;153;153m✻\u{1b}[m Baked for 1m 2s\u{1b}[m\n\u{1b}[m\n{rule}\n{box_line}\n{rule}\n  \u{1b}[38;2;153;153;153mstatus\u{1b}[m\n"
        )
    }

    #[test]
    fn a_predicted_next_prompt_is_an_empty_box_and_is_marked_in_plain_text() {
        let screen = claude_suggestion("❯\u{a0}\u{1b}[2m바뀐 것을 올려\u{1b}[m");
        assert_eq!(check("claude", &screen), Draft::Empty);
        assert_eq!(box_text("claude", &screen).as_deref(), Some(""));
        let text = "✻ Baked for 1m 2s\n\n────\n❯\u{a0}바뀐 것을 올려\n────\n  status\n";
        let marked = mark_dim_box("claude", text, &screen);
        assert_eq!(marked.lines().nth(3), Some("❯ [흐린 글·입력 아님] 바뀐 것을 올려"), "{marked}");
        assert_eq!(marked.lines().count(), text.lines().count());
        // 친 글은 표지가 없다
        let typed = claude_suggestion("❯\u{a0}바뀐 것을 올려");
        assert_eq!(check("claude", &typed), Draft::Typed);
        assert_eq!(mark_dim_box("claude", text, &typed), text);
        // 두 글의 그 줄이 맞지 않으면(다른 때 읽음) 고치지 않는다
        let other = text.replace("올려", "내려");
        assert_eq!(mark_dim_box("claude", &other, &screen), other);
        // 빈 입력창, 입력창이 없는 화면, 모르는 harness도 그대로다
        let empty = claude_suggestion("❯\u{a0}");
        assert_eq!(mark_dim_box("claude", text, &empty), text);
        assert_eq!(mark_dim_box("claude", "no box\n", "no box\n"), "no box\n");
        assert_eq!(mark_dim_box("gemini", text, &screen), text);
        // Codex의 흐린 placeholder도 같다
        let codex = "\u{1b}[1m›\u{1b}[0m \u{1b}[2mAsk Codex to do anything\u{1b}[0m\n";
        assert_eq!(
            mark_dim_box("codex", "› Ask Codex to do anything\n", codex),
            "› [흐린 글·입력 아님] Ask Codex to do anything\n"
        );
    }

    /// host가 실제로 읽는 길: session host vt가 다시 그린 화면의 Text와 Ansi(빈칸이 커서 이동 `CSI C`로 나온다)
    #[test]
    fn the_mark_lines_up_with_the_session_hosts_text_and_ansi() {
        let raw = claude_suggestion("❯\u{a0}\u{1b}[2m바뀐 것을 main에 올려\u{1b}[m");
        let mut vt = aaa_vt::Vt::new(12, 70);
        vt.feed(raw.replace('\n', "\r\n").as_bytes());
        let text = vt.read(aaa_proto::ReadSource::Visible, None, aaa_proto::ReadFormat::Text).text;
        let ansi = vt.read(aaa_proto::ReadSource::Visible, None, aaa_proto::ReadFormat::Ansi).text;
        assert_eq!(check("claude", &ansi), Draft::Empty, "{ansi:?}");
        let marked = mark_dim_box("claude", &text, &ansi);
        assert!(marked.contains("❯ [흐린 글·입력 아님] 바뀐 것을 main에 올려"), "{marked}");
        let recent = vt.read(aaa_proto::ReadSource::Recent, None, aaa_proto::ReadFormat::Text).text;
        let recent_ansi = vt.read(aaa_proto::ReadSource::Recent, None, aaa_proto::ReadFormat::Ansi).text;
        assert!(mark_dim_box("claude", &recent, &recent_ansi).contains(DIM_MARK));
    }

    #[test]
    fn a_real_character_under_a_cursor_is_typed() {
        // One typed character with the harness cursor on it is text, while a
        // cursor on a dim placeholder is not.
        let rule = "─".repeat(40);
        assert_eq!(check("pi", &format!("{rule}\n\u{1b}[7mx\u{1b}[0m   \n{rule}\n")), Draft::Typed);
        assert_eq!(
            check("cursor", "  \u{1b}[2m→ \u{1b}[0m\u{1b}[7mP\u{1b}[0m\u{1b}[2mlan, search\u{1b}[0m\n"),
            Draft::Empty
        );
    }
}

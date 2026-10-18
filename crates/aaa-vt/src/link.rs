//! pane 화면의 링크(herdr 0.9.5 ctrl+누름 `pane.link.activate`). 누른 칸에 OSC 8 링크가 있으면 그 주소, 없으면 그 칸을 포함한
//! 공백 사이 낱말(접힌 줄 포함) 안의 `http://`·`https://` URL이다(herdr `crates/ghostty-vt` `viewport_link_selection`,
//! `src/app/actions.rs` `url_from_link_target`·`url_span_at_column`·`trim_url_edges`·`safe_web_url`). 여는 것은 http·https만이다.
//!
//! vt100은 칸마다 OSC 8을 담지 않는다. 그래서 OSC 8을 열고 닫을 때 커서 자리와 그 사이 글을 [`Links`]에 남기고, 찾을 때 그
//! 자리의 지금 글이 남긴 글과 같을 때만 링크로 친다(그 뒤에 덮어쓴 칸은 링크가 아니다).
//!
//! herdr(https://github.com/herdrdev/herdr, Apache License 2.0)의 코드를 옮겨 aaa에 맞게 고쳤다.

use std::collections::VecDeque;

/// 남겨 두는 OSC 8 구간 수(오래된 것부터 버린다)
const MAX_SPANS: usize = 512;
/// 한 구간이 걸칠 수 있는 줄 수(닫지 않은 채 한참 뒤에 닫으면 남기지 않는다: 닫을 때 그 사이 글을 읽는다)
const MAX_SPAN_ROWS: usize = 64;
/// 남기는 uri 길이
pub(crate) const MAX_URI_CHARS: usize = 4096;
/// herdr `ghostty_terminal_select_word_bounded`의 8192칸: 누른 칸 앞뒤로 읽는 칸 상한
pub(crate) const MAX_TOKEN_CELLS: usize = 8192;

/// 연 주소만 받는다(herdr `safe_web_url`)
pub fn safe_web_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

/// OSC 8 구간 하나: 절대 줄(기록 맨 앞이 0, alt 화면이면 화면 줄)과 칸, 끝은 닫을 때 커서 자리(그 칸은 넣지 않는다)
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Span {
    pub uri: String,
    pub alt: bool,
    pub start: (usize, u16),
    pub end: (usize, u16),
    pub text: String,
}

/// 열린 OSC 8과 남긴 구간들
#[derive(Debug, Clone, Default)]
pub(crate) struct Links {
    open: Option<(String, bool, (usize, u16))>,
    pub spans: VecDeque<Span>,
}

impl Links {
    /// `OSC 8 ; params ; uri`. 빈 uri는 닫기, 아니면 (앞 링크를 닫고) 연다. `at`은 지금 커서의 절대 자리, `text`는 열린
    /// 자리부터 `at`까지의 화면 글을 읽는다.
    pub fn osc(&mut self, uri: &str, alt: bool, at: (usize, u16), text: impl FnOnce((usize, u16)) -> Option<String>) {
        if let Some((open_uri, open_alt, start)) = self.open.take()
            && open_alt == alt
            && start < at
            && at.0 - start.0 < MAX_SPAN_ROWS
            && let Some(t) = text(start)
            && !t.is_empty()
        {
            if self.spans.len() == MAX_SPANS {
                self.spans.pop_front();
            }
            self.spans.push_back(Span { uri: open_uri, alt, start, end: at, text: t });
        }
        if !uri.is_empty() {
            self.open = Some((uri.to_string(), alt, at));
        }
    }

    /// 크기가 바뀌면 칸 자리가 달라진다(다시 접기): 남긴 구간을 버린다
    pub fn clear(&mut self) {
        self.open = None;
        self.spans.clear();
    }

    /// normal 화면 구간만 버린다(버린 줄 수를 모를 때)
    pub fn clear_normal(&mut self) {
        self.open = self.open.take().filter(|o| o.1);
        self.spans.retain(|s| s.alt);
    }
}

/// 링크를 찾을 한 칸: 글자와, 붙여 만들 글(넓은 글자·결합 글자는 그 칸의 글 전부)
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TextCell {
    pub ch: char,
    pub text: String,
}

/// 칸들 안에서 `clicked` 칸을 덮는 http(s) URL(herdr `url_span_at_column`·`trim_url_edges`·`url_byte_range`)
pub(crate) fn url_at(cells: &[TextCell], clicked: usize) -> Option<String> {
    let starts =
        |i: usize, prefix: &str| prefix.chars().enumerate().all(|(k, c)| cells.get(i + k).is_some_and(|x| x.ch == c));
    let mut start = 0;
    while start < cells.len() {
        if starts(start, "http://") || starts(start, "https://") {
            let mut end = start;
            while end + 1 < cells.len() && !cells[end + 1].ch.is_whitespace() {
                end += 1;
            }
            if (start..=end).contains(&clicked) {
                let end = trim_url_end(cells, start, end)?;
                if !(start..=end).contains(&clicked) {
                    return None;
                }
                let url: String = cells[start..=end].iter().map(|c| c.text.as_str()).collect();
                return safe_web_url(&url).then_some(url);
            }
            start = end + 1;
        } else {
            start += 1;
        }
    }
    None
}

/// 끝의 문장 부호와 짝이 없는 닫는 괄호를 뺀다(herdr `should_trim_trailing_url_cell`)
fn trim_url_end(cells: &[TextCell], start: usize, mut end: usize) -> Option<usize> {
    let balanced = |end: usize, open: char, close: char| {
        let n: i32 = cells[start..end].iter().map(|c| i32::from(c.ch == open) - i32::from(c.ch == close)).sum();
        n > 0
    };
    loop {
        let trim = match cells[end].ch {
            '"' | '\'' | '`' | '.' | ',' | ';' | ':' | '!' | '?' => true,
            ')' => !balanced(end, '(', ')'),
            ']' => !balanced(end, '[', ']'),
            '}' => !balanced(end, '{', '}'),
            _ => false,
        };
        if !trim {
            return Some(end);
        }
        if end == start {
            return None;
        }
        end -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cells(s: &str) -> Vec<TextCell> {
        s.chars().map(|ch| TextCell { ch, text: ch.to_string() }).collect()
    }

    fn at(s: &str, needle: &str) -> Option<String> {
        let line = cells(s);
        let byte = s.find(needle).unwrap();
        url_at(&line, s[..byte].chars().count())
    }

    /// herdr `url_at_byte_returns_safe_visible_url_only`
    #[test]
    fn finds_only_safe_visible_urls_like_herdr() {
        assert_eq!(at("see https://example.com/a(b)c.", "example").as_deref(), Some("https://example.com/a(b)c"));
        assert_eq!(at("[docs](https://example.com/docs),", "example").as_deref(), Some("https://example.com/docs"));
        assert_eq!(at("[docs](https://example.com/docs)", "docs"), None);
        assert_eq!(at("open file:///tmp/report", "file"), None);
        assert_eq!(at("x http://a.b/c?d=e;f x", "http").as_deref(), Some("http://a.b/c?d=e;f"));
        // 가운데 점은 URL 안, 끝에서 뺀 점을 누르면 링크가 아니다
        assert_eq!(at("go https://a.b/c.", ".b").as_deref(), Some("https://a.b/c"));
        assert_eq!(url_at(&cells("go https://a.b/c."), 16), None);
        assert_eq!(at("(https://a.b/(c))", "a.b").as_deref(), Some("https://a.b/(c)"));
        assert_eq!(at("x https://a.b/c x", " x").as_deref(), None);
    }

    #[test]
    fn keeps_spans_and_drops_the_oldest() {
        let mut l = Links::default();
        l.osc("https://a", false, (3, 2), |_| unreachable!("연 링크가 없다"));
        l.osc("", false, (3, 9), |s| Some(format!("{s:?}")));
        assert_eq!(
            l.spans[0],
            Span { uri: "https://a".into(), alt: false, start: (3, 2), end: (3, 9), text: "(3, 2)".into() }
        );
        // 화면이 바뀐(alt) 뒤 닫으면 남기지 않는다
        l.osc("https://b", false, (4, 0), |_| None);
        l.osc("", true, (4, 5), |_| Some("x".into()));
        assert_eq!(l.spans.len(), 1);
        for i in 0..MAX_SPANS {
            l.osc("https://c", false, (10 + i, 0), |_| None);
            l.osc("", false, (10 + i, 3), |_| Some("abc".into()));
        }
        assert_eq!(l.spans.len(), MAX_SPANS);
        assert_eq!(l.spans[0].start, (10, 0), "맨 처음 구간 (3, 2)를 버렸다");
    }
}

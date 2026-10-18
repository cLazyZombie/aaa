//! pane 화면의 OSC 8 링크를 바깥 터미널에 다시 내보낸다(herdr 0.9.5 `src/protocol/render_ansi.rs` 616–699 `write_cell`·
//! `write_hyperlink_if_changed`·`close_hyperlink`, 서버 `visible_hyperlinks`). 그러면 바깥 터미널이 그 칸을 링크로 알아 자기
//! 방식(밑줄·누르기)으로 연다.
//!
//! host가 grid frame에 보이는 링크 칸(`GridFrame.links`)을 싣는다. TUI는 pane을 그리며 그 칸과 그린 글자를 적고([`note`]),
//! 다 그린 뒤 위에 다른 것(창·메뉴·알림)이 덮지 않은 칸만([`collect`]) OSC 8로 감싸 다시 쓴다([`emit`]). ratatui는 바뀐 칸만
//! 쓰고 OSC 8을 모르므로 그린 뒤에 덧쓴다: 링크 칸은 그릴 때마다 다시 쓰고, 앞에서 링크였는데 이제 아닌 칸은 링크 없이 다시
//! 써 바깥 터미널에 남은 링크를 지운다. 커서와 글자 모양은 DECSC·DECRC로 되돌린다.
//!
//! herdr(https://github.com/herdrdev/herdr, Apache License 2.0)의 코드를 옮겨 aaa에 맞게 고쳤다.

use std::io::Write;

use aaa_proto::GridLink;
use aaa_proto::layout::Rect;
use ratatui::backend::Backend;
use ratatui::buffer::{Buffer, Cell};

/// 그린 링크 칸 하나: 자리, 그때 그린 글자, uri
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Painted {
    pub x: u16,
    pub y: u16,
    pub symbol: String,
    pub uri: String,
}

/// pane 안쪽 `r`에 grid를 그린 직후 그 링크 칸을 적는다(칸 밖은 자른다)
pub fn note(buf: &Buffer, r: Rect, links: &[GridLink], out: &mut Vec<Painted>) {
    for l in links.iter().filter(|l| l.row < r.h) {
        let y = r.y + l.row;
        for col in l.col..l.end.min(r.w) {
            let x = r.x + col;
            if let Some(cell) = buf.cell((x, y)) {
                out.push(Painted { x, y, symbol: cell.symbol().to_string(), uri: l.uri.clone() });
            }
        }
    }
}

/// 다시 쓸 칸 하나(자리와 그 칸)
pub type Placed = (u16, u16, Cell);

/// 내보낼 것: uri마다 이어 쓸 칸들과, 앞에서 링크였는데 이제 아닌 칸
#[derive(Debug, Default, PartialEq)]
pub struct Emit {
    pub links: Vec<(String, Vec<Placed>)>,
    pub plain: Vec<Placed>,
}

/// 다 그린 화면에서 적은 칸 중 글자가 그대로인 것(덮이지 않은 것)을 고른다. 넓은 글자 뒤 칸은 쓰지 않는다(쓰면 그 반쪽을
/// 덮는다). `before`는 앞에서 링크로 내보낸 칸이다: 이번에 링크가 아니면 링크 없이 다시 쓴다.
pub fn collect(buf: &Buffer, painted: &[Painted], before: &[(u16, u16)]) -> (Emit, Vec<(u16, u16)>) {
    let mut emit = Emit::default();
    let mut now = Vec::new();
    let mut hidden: Option<(u16, u16)> = None;
    for p in painted {
        if hidden == Some((p.x, p.y)) {
            continue;
        }
        let Some(cell) = buf.cell((p.x, p.y)).filter(|c| c.symbol() == p.symbol) else { continue };
        if unicode_width::UnicodeWidthStr::width(cell.symbol()) > 1 {
            hidden = Some((p.x + 1, p.y));
        }
        match emit.links.last_mut() {
            Some((uri, cells)) if *uri == p.uri => cells.push((p.x, p.y, cell.clone())),
            _ => emit.links.push((p.uri.clone(), vec![(p.x, p.y, cell.clone())])),
        }
        now.push((p.x, p.y));
    }
    for &(x, y) in before {
        if !now.contains(&(x, y))
            && let Some(cell) = buf.cell((x, y))
        {
            emit.plain.push((x, y, cell.clone()));
        }
    }
    (emit, now)
}

/// 바깥 터미널에 쓴다. 쓸 것이 없으면 아무것도 쓰지 않는다. 커서가 보이면 쓰는 동안 숨긴다.
pub fn emit<B>(backend: &mut B, e: &Emit, cursor_shown: bool) -> std::io::Result<()>
where
    B: Backend + Write,
    B::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    if e.links.is_empty() && e.plain.is_empty() {
        return Ok(());
    }
    // 동기 출력 시작, 커서 숨김, 커서·모양 저장, 혹시 열린 링크 닫기(herdr: 매 frame 알려진 OSC 8 상태에서 시작)
    backend.write_all(b"\x1b[?2026h")?;
    if cursor_shown {
        backend.write_all(b"\x1b[?25l")?;
    }
    backend.write_all(b"\x1b7\x1b]8;;\x1b\\")?;
    if !e.plain.is_empty() {
        backend.draw(e.plain.iter().map(|(x, y, c)| (*x, *y, c))).map_err(std::io::Error::other)?;
    }
    for (uri, cells) in &e.links {
        let uri: String = uri.chars().filter(|c| !c.is_control()).collect();
        if uri.is_empty() {
            continue;
        }
        write!(backend, "\x1b]8;;{uri}\x1b\\")?;
        backend.draw(cells.iter().map(|(x, y, c)| (*x, *y, c))).map_err(std::io::Error::other)?;
        backend.write_all(b"\x1b]8;;\x1b\\")?;
    }
    backend.write_all(b"\x1b8")?;
    if cursor_shown {
        backend.write_all(b"\x1b[?25h")?;
    }
    backend.write_all(b"\x1b[?2026l")?;
    Write::flush(backend)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;

    /// 덮이지 않은 칸만, 넓은 글자 뒤 칸은 빼고, 같은 uri는 이어서, 앞에서 링크였던 칸은 링크 없이
    #[test]
    fn collects_uncovered_link_cells() {
        let mut buf = Buffer::empty(ratatui::layout::Rect::new(0, 0, 10, 2));
        buf.set_string(0, 0, "ab한c", ratatui::style::Style::default());
        let r = Rect { x: 0, y: 0, w: 10, h: 2 };
        let links = [GridLink { row: 0, col: 0, end: 5, uri: "https://a".into() }];
        let mut painted = Vec::new();
        note(&buf, r, &links, &mut painted);
        assert_eq!(painted.len(), 5);
        // 창이 1칸(`b`)을 덮었다
        buf.set_string(1, 0, "X", ratatui::style::Style::default());
        let (e, now) = collect(&buf, &painted, &[(0, 1)]);
        assert_eq!(now, [(0, 0), (2, 0), (4, 0)], "넓은 글자 뒤 3칸·덮인 1칸은 뺀다");
        assert_eq!(e.links.len(), 1);
        assert_eq!(e.links[0].1.iter().map(|(_, _, c)| c.symbol()).collect::<String>(), "a한c");
        assert_eq!(e.plain.iter().map(|(x, y, _)| (*x, *y)).collect::<Vec<_>>(), [(0, 1)]);
        let (e, _) = collect(&buf, &[], &[]);
        assert_eq!(e, Emit::default());
    }

    /// OSC 8로 감싸 쓰고, 커서·모양을 되돌리고, 쓸 것이 없으면 아무것도 쓰지 않는다
    #[test]
    fn emits_osc8_around_link_cells() {
        let mut backend = TestBackend::new(10, 2);
        let mut cell = Cell::default();
        cell.set_symbol("a");
        let e = Emit { links: vec![("https://a\x07x".into(), vec![(1, 0, cell)])], plain: vec![] };
        let mut out = Vec::new();
        emit(&mut Recorder { inner: &mut backend, out: &mut out }, &e, true).unwrap();
        let s = String::from_utf8_lossy(&out);
        assert_eq!(
            s,
            "\x1b[?2026h\x1b[?25l\x1b7\x1b]8;;\x1b\\\x1b]8;;https://ax\x1b\\\x1b]8;;\x1b\\\x1b8\x1b[?25h\x1b[?2026l"
        );
        assert_eq!(backend.buffer()[(1, 0)].symbol(), "a");
        let mut out = Vec::new();
        emit(&mut Recorder { inner: &mut backend, out: &mut out }, &Emit::default(), true).unwrap();
        assert!(out.is_empty());
    }

    /// 쓴 bytes를 모으는 backend(그리기는 TestBackend에 맡긴다)
    struct Recorder<'a> {
        inner: &'a mut TestBackend,
        out: &'a mut Vec<u8>,
    }

    impl Write for Recorder<'_> {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.out.extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Backend for Recorder<'_> {
        type Error = <TestBackend as Backend>::Error;
        fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
        where
            I: Iterator<Item = (u16, u16, &'a Cell)>,
        {
            self.inner.draw(content)
        }
        fn hide_cursor(&mut self) -> Result<(), Self::Error> {
            self.inner.hide_cursor()
        }
        fn show_cursor(&mut self) -> Result<(), Self::Error> {
            self.inner.show_cursor()
        }
        fn get_cursor_position(&mut self) -> Result<ratatui::layout::Position, Self::Error> {
            self.inner.get_cursor_position()
        }
        fn set_cursor_position<P: Into<ratatui::layout::Position>>(&mut self, p: P) -> Result<(), Self::Error> {
            self.inner.set_cursor_position(p)
        }
        fn clear(&mut self) -> Result<(), Self::Error> {
            self.inner.clear()
        }
        fn clear_region(&mut self, t: ratatui::backend::ClearType) -> Result<(), Self::Error> {
            self.inner.clear_region(t)
        }
        fn size(&self) -> Result<ratatui::layout::Size, Self::Error> {
            self.inner.size()
        }
        fn window_size(&mut self) -> Result<ratatui::backend::WindowSize, Self::Error> {
            self.inner.window_size()
        }
        fn flush(&mut self) -> Result<(), Self::Error> {
            Backend::flush(self.inner)
        }
    }
}

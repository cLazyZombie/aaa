//! 직접 만든 시험 터미널도 ConPTY/CLI의 질의에 실제 화면 위치로 답한다.
//! session host와 같은 parser를 사용하며 제품 승인/입력 경로에는 관여하지 않는다.
use aaa_vt::{DefaultColor, DefaultColors, Found, Query, QueryScanner, Vt};
use std::io::{self, Write};

pub struct TerminalQueries {
    queries: QueryScanner,
    vt: Vt,
    colors: DefaultColors,
}
impl TerminalQueries {
    pub fn new(rows: u16, cols: u16) -> Self {
        Self { queries: QueryScanner::default(), vt: Vt::new(rows, cols), colors: DefaultColors::new(None) }
    }
    pub fn feed(&mut self, bytes: &[u8], writer: &mut impl Write) -> io::Result<Vec<u8>> {
        self.feed_observed(bytes, writer, |_, _, _| {})
    }
    pub fn feed_observed(
        &mut self,
        bytes: &[u8],
        writer: &mut impl Write,
        mut observe: impl FnMut(&'static str, &'static str, Option<i32>),
    ) -> io::Result<Vec<u8>> {
        let scanned = self.queries.scan(bytes);
        let mut start = 0;
        for (at, found) in scanned.found {
            self.vt.feed(&scanned.out[start..at]);
            start = at;
            let kind = match found {
                Found::Query(Query::CursorPosition) => "cursor_position",
                Found::Query(Query::PrimaryAttributes) => "primary_attributes",
                Found::Query(Query::SecondaryAttributes) => "secondary_attributes",
                Found::Query(Query::DefaultColor(DefaultColor::Foreground, _)) => "foreground_color",
                Found::Query(Query::DefaultColor(DefaultColor::Background, _)) => "background_color",
                other => {
                    self.colors.note(other);
                    continue;
                }
            };
            observe(kind, "received", None);
            let reply = match found {
                Found::Query(Query::CursorPosition) => Some(self.vt.cursor_report()),
                Found::Query(Query::DefaultColor(color, terminator)) => self.colors.reply(color, terminator),
                Found::Query(query) => query.fixed_reply().map(<[u8]>::to_vec),
                _ => unreachable!(),
            };
            if let Some(reply) = reply {
                if let Err(error) = writer.write_all(&reply) {
                    observe(kind, "write_failed", error.raw_os_error());
                    return Err(error);
                }
                if let Err(error) = writer.flush() {
                    observe(kind, "flush_failed", error.raw_os_error());
                    return Err(error);
                }
                observe(kind, "flushed", None);
            } else {
                observe(kind, "unavailable", None);
            }
        }
        self.vt.feed(&scanned.out[start..]);
        Ok(scanned.out)
    }
}

#[test]
fn query_observation_tracks_replies_and_failures_without_recording_bytes() {
    let mut terminal = TerminalQueries::new(36, 120);
    let mut output = Vec::new();
    let mut events = Vec::new();
    let mut note = |kind, stage, code| events.push((kind, stage, code));
    assert_eq!(terminal.feed(b"private\x1b[6", &mut output).unwrap(), b"private");
    assert!(output.is_empty());
    terminal.feed_observed(b"n\x1b[c\x1b[>c", &mut output, &mut note).unwrap();
    assert_eq!(output, b"\x1b[1;8R\x1b[?62;22c\x1b[>1;0;0c");
    assert_eq!(
        events,
        [
            ("cursor_position", "received", None),
            ("cursor_position", "flushed", None),
            ("primary_attributes", "received", None),
            ("primary_attributes", "flushed", None),
            ("secondary_attributes", "received", None),
            ("secondary_attributes", "flushed", None),
        ]
    );
    assert!(!format!("{events:?}").contains("private"));
    struct Broken(bool);
    impl Write for Broken {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.0 { Ok(bytes.len()) } else { Err(io::Error::from_raw_os_error(5)) }
        }
        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::from_raw_os_error(6))
        }
    }
    for (flush, expected, code) in [(false, "write_failed", 5), (true, "flush_failed", 6)] {
        let mut events = Vec::new();
        let error = terminal
            .feed_observed(b"\x1b[6n", &mut Broken(flush), |kind, stage, code| {
                events.push((kind, stage, code));
            })
            .unwrap_err();
        assert_eq!(error.raw_os_error(), Some(code));
        assert_eq!(events, [("cursor_position", "received", None), ("cursor_position", expected, Some(code))]);
    }
}

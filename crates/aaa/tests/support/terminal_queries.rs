//! 직접 만든 시험 터미널도 ConPTY/CLI의 질의에 실제 화면 위치로 답한다.
//! session host와 같은 parser를 사용하며 제품 승인/입력 경로에는 관여하지 않는다.
use aaa_vt::{DefaultColors, Found, Query, QueryScanner, Vt};
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
        let scanned = self.queries.scan(bytes);
        let mut start = 0;
        for (at, found) in scanned.found {
            self.vt.feed(&scanned.out[start..at]);
            start = at;
            let reply = match found {
                Found::Query(Query::CursorPosition) => Some(self.vt.cursor_report()),
                Found::Query(Query::DefaultColor(color, terminator)) => self.colors.reply(color, terminator),
                Found::Query(query) => query.fixed_reply().map(<[u8]>::to_vec),
                other => {
                    self.colors.note(other);
                    None
                }
            };
            if let Some(reply) = reply {
                writer.write_all(&reply)?;
                writer.flush()?;
            }
        }
        self.vt.feed(&scanned.out[start..]);
        Ok(scanned.out)
    }
}

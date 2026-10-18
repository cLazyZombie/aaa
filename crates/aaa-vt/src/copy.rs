//! copy 모드 낱말·문단 이동과 찾기(herdr 0.9.5 `src/pane/terminal.rs` `RetainedTextBuffer`·`next_word_start`·
//! `previous_word_start`·`next_word_end`·`paragraph_motion_target`·`search_window`). 줄 번호는 기록 맨 앞이 0인 절대 줄,
//! 칸은 화면 칸이다. 줄 칸 읽기와 창 넓히기는 [`crate::Vt`]가 한다.
//!
//! herdr(https://github.com/herdrdev/herdr, Apache License 2.0)의 코드를 옮겨 aaa에 맞게 고쳤다.

/// (절대 줄, 칸)
pub type Point = (usize, u16);

/// 한 칸(넓은 글자 뒤 칸은 없다): 시작 칸, 끝 칸(넓은 글자면 시작 + 1), 글(빈 칸은 "")
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    pub col: u16,
    pub end_col: u16,
    pub text: String,
}

/// 한 줄의 칸과 다음 줄로 접혔는지
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Row {
    pub cells: Vec<Cell>,
    pub wrapped: bool,
}

impl Row {
    /// vt100에는 herdr의 `SpacerHead`(넓은 글자가 다음 줄로 넘어가며 남긴 줄 끝 칸)가 없다. 접힌 줄은 끝까지 찬 줄이라
    /// 끝 칸이 비었으면 그 자리다: 낱말은 앞 글자 종류를 잇고, 찾기 글에는 넣지 않는다.
    fn spacer(&self, k: usize) -> bool {
        self.wrapped && k + 1 == self.cells.len() && self.cells[k].text.is_empty()
    }
}

/// 빈 줄인가(모든 칸이 비었거나 공백, herdr `paragraph_motion_target`)
pub fn is_blank(r: &Row) -> bool {
    r.cells.iter().all(|c| c.text.chars().all(char::is_whitespace))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    NextWordStart,
    PreviousWordStart,
    NextWordEnd,
    NextBigWordStart,
    PreviousBigWordStart,
    NextBigWordEnd,
    PreviousParagraph,
    NextParagraph,
}

impl Motion {
    pub fn backward(self) -> bool {
        matches!(self, Motion::PreviousWordStart | Motion::PreviousBigWordStart)
    }

    pub fn word_end(self) -> bool {
        matches!(self, Motion::NextWordEnd | Motion::NextBigWordEnd)
    }

    fn big(self) -> bool {
        matches!(self, Motion::NextBigWordStart | Motion::PreviousBigWordStart | Motion::NextBigWordEnd)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Whitespace,
    Separator,
    Word,
}

/// herdr `COPY_MODE_WORD_SEPARATORS`: 이 ASCII 문장 부호만 낱말을 가른다(`_`·한글·CJK 문장 부호·— 는 낱말 글자)
const SEPARATORS: &str = "!\"#$%&'()*+,-./:;<=>?@[\\]^`{|}~";

/// 칸 글의 첫 글자로 정한다(빈 칸은 공백)
fn class_of(text: &str) -> Class {
    let ch = text.chars().next().unwrap_or(' ');
    if ch.is_whitespace() {
        Class::Whitespace
    } else if ch.is_ascii() && SEPARATORS.contains(ch) {
        Class::Separator
    } else {
        Class::Word
    }
}

/// 칸 하나 또는 줄바꿈(`point` 없음, 공백). 줄바꿈은 접히지 않은 줄 뒤에만 둔다: 접힌 줄은 그대로 잇고 빈 줄에서도 멈추지 않는다.
struct Atom {
    point: Option<Point>,
    end_col: u16,
    class: Class,
}

fn atoms(rows: &[Row], first: usize) -> Vec<Atom> {
    let mut out: Vec<Atom> = Vec::new();
    for (i, r) in rows.iter().enumerate() {
        for (k, c) in r.cells.iter().enumerate() {
            let class = if r.spacer(k) { out.last().map_or(Class::Whitespace, |a| a.class) } else { class_of(&c.text) };
            out.push(Atom { point: Some((first + i, c.col)), end_col: c.end_col, class });
        }
        if !r.wrapped {
            out.push(Atom { point: None, end_col: 0, class: Class::Whitespace });
        }
    }
    out
}

/// 창 `rows`(맨 위 줄 `first`) 안의 낱말 이동 자리. 갈 곳이 없으면 None(herdr는 그대로 둔다). 문단 이동은 다루지 않는다.
/// 넓은 글자는 어느 칸에 있어도 그 글자이고, 자리는 늘 글자의 시작 칸이다(`e`도 머리 칸).
pub fn word_target(rows: &[Row], first: usize, cursor: Point, m: Motion) -> Option<Point> {
    let atoms = atoms(rows, first);
    let i = atoms
        .iter()
        .position(|a| a.point.is_some_and(|(r, c)| r == cursor.0 && c <= cursor.1 && cursor.1 <= a.end_col))?;
    let big = m.big();
    // 큰 낱말은 공백이 아닌 글자 덩어리
    let k = |j: usize| match atoms[j].class {
        Class::Separator if big => Class::Word,
        c => c,
    };
    let n = atoms.len();
    let j = match m {
        Motion::NextWordStart | Motion::NextBigWordStart => {
            let mut j = i;
            let c = k(i);
            if c != Class::Whitespace {
                while j < n && k(j) == c {
                    j += 1;
                }
            }
            while j < n && k(j) == Class::Whitespace {
                j += 1;
            }
            j
        }
        Motion::PreviousWordStart | Motion::PreviousBigWordStart => {
            let mut j = i.checked_sub(1)?;
            while k(j) == Class::Whitespace {
                j = j.checked_sub(1)?;
            }
            let c = k(j);
            while j > 0 && k(j - 1) == c {
                j -= 1;
            }
            j
        }
        Motion::NextWordEnd | Motion::NextBigWordEnd => {
            let mut j = i + 1;
            while j < n && k(j) == Class::Whitespace {
                j += 1;
            }
            if j >= n {
                return None;
            }
            let c = k(j);
            while j + 1 < n && k(j + 1) == c {
                j += 1;
            }
            j
        }
        Motion::PreviousParagraph | Motion::NextParagraph => return None,
    };
    atoms.get(j)?.point
}

/// 찾을 글: 글자 그대로(정규식 아님), 큰 글자가 하나라도 있으면 대소문자를 가린다(herdr smart case)
pub struct Matcher {
    query: Vec<char>,
    fold: bool,
}

/// 대소문자 접기: 글자마다 소문자 첫 글자(herdr는 정규식 Unicode 접기라 ß 같은 여러 글자 접기가 다를 수 있다)
fn fold(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

impl Matcher {
    /// 빈 글이면 None
    pub fn new(query: &str) -> Option<Matcher> {
        if query.is_empty() {
            return None;
        }
        let fold_case = !query.chars().any(char::is_uppercase);
        let query = query.chars().map(|c| if fold_case { fold(c) } else { c }).collect();
        Some(Matcher { query, fold: fold_case })
    }

    fn eq(&self, c: char, q: char) -> bool {
        (if self.fold { fold(c) } else { c }) == q
    }
}

/// 찾은 범위(시작 칸, 끝 칸 포함. 넓은 글자 끝은 머리 + 1)
pub type Range = (Point, Point);

/// 논리 줄(접힌 줄을 이은 줄)을 모아 찾는다(herdr `RetainedTextBuffer::new_search`): 줄 끝 공백은 빼고, 칸 가운데서 시작하거나
/// 끝나는 일치는 버린다(겹치지 않는 왼쪽부터 찾기라 그 범위는 건너뛴다).
#[derive(Default)]
pub struct Lines {
    chars: Vec<(char, usize, bool, bool)>,
    cells: Vec<(Point, u16)>,
}

impl Lines {
    pub fn push_row(&mut self, row: usize, r: &Row, m: &Matcher, out: &mut Vec<Range>) {
        for (k, c) in r.cells.iter().enumerate() {
            if r.spacer(k) {
                continue;
            }
            let idx = self.cells.len();
            self.cells.push(((row, c.col), c.end_col));
            let text = if c.text.is_empty() { " " } else { c.text.as_str() };
            let n = text.chars().count();
            for (x, ch) in text.chars().enumerate() {
                self.chars.push((ch, idx, x == 0, x + 1 == n));
            }
        }
        if !r.wrapped {
            self.finish(m, out);
        }
    }

    pub fn finish(&mut self, m: &Matcher, out: &mut Vec<Range>) {
        let len = self.chars.iter().rposition(|(c, ..)| !c.is_whitespace()).map_or(0, |p| p + 1);
        let q = m.query.len();
        let mut i = 0;
        while i + q <= len {
            if (0..q).all(|x| m.eq(self.chars[i + x].0, m.query[x])) {
                let j = i + q - 1;
                let (s, e) = (self.chars[i], self.chars[j]);
                if s.2 && e.3 {
                    let end = self.cells[e.1];
                    out.push((self.cells[s.1].0, (end.0.0, end.1)));
                }
                i = j + 1;
            } else {
                i += 1;
            }
        }
        self.chars.clear();
        self.cells.clear();
    }
}

/// 찾은 결과(herdr `PaneCopySearch` 응답): 현재 일치 언저리 `keep`개, 전체 수, 그 안의 현재·전체에서의 현재
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Found {
    pub matches: Vec<Range>,
    pub total: usize,
    pub current: Option<usize>,
    pub current_global: Option<usize>,
}

/// 다음 일치(herdr `search_window`): 앞으로는 기준(`previous` 끝, 없으면 커서)보다 뒤에서 시작하는 첫 일치, 뒤로는 기준(`previous`
/// 시작, 없으면 커서)보다 앞에서 끝나는 마지막 일치. 없으면 반대 끝으로 돈다. 커서 자리에서 시작하는 일치는 새 찾기에서 건너뛴다.
pub fn pick(all: Vec<Range>, forward: bool, cursor: Point, previous: Option<Range>, keep: usize) -> Found {
    let total = all.len();
    if total == 0 {
        return Found::default();
    }
    let target = if forward {
        let origin = previous.map_or(cursor, |p| p.1);
        all.iter().position(|m| m.0 > origin).unwrap_or(0)
    } else {
        let origin = previous.map_or(cursor, |p| p.0);
        all.iter().rposition(|m| m.1 < origin).unwrap_or(total - 1)
    };
    let retained = keep.min(total).max(1);
    let start = target.saturating_sub(retained / 2).min(total - retained);
    Found {
        matches: all[start..start + retained].to_vec(),
        total,
        current: Some(target - start),
        current_global: Some(target),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 줄 글로 칸을 만든다(한 칸 한 글자, 넓은 글자는 두 칸). `\` 뒤 줄은 앞 줄에서 접힌 것
    fn rows(lines: &[&str], width: u16) -> Vec<Row> {
        let mut out: Vec<Row> = Vec::new();
        for l in lines {
            let (wrapped_from_prev, l) = match l.strip_prefix('\\') {
                Some(rest) => (true, rest),
                None => (false, *l),
            };
            if wrapped_from_prev && let Some(prev) = out.last_mut() {
                prev.wrapped = true;
            }
            let mut cells = Vec::new();
            let mut col = 0u16;
            for ch in l.chars() {
                let w = if ch == '한' || ch == '글' || ch == '字' { 2 } else { 1 };
                cells.push(Cell { col, end_col: col + w - 1, text: ch.to_string() });
                col += w;
            }
            while col < width {
                cells.push(Cell { col, end_col: col, text: String::new() });
                col += 1;
            }
            out.push(Row { cells, wrapped: false });
        }
        out
    }

    fn go(lines: &[&str], cursor: Point, m: Motion) -> Option<Point> {
        word_target(&rows(lines, 20), 0, cursor, m)
    }

    /// herdr 시험이 못 박은 낱말 규칙: ASCII 문장 부호만 가르고(`—d`는 한 낱말), 빈 줄에서 멈추지 않으며, 큰 낱말은 공백까지
    #[test]
    fn word_motions_like_herdr() {
        use Motion::*;
        let l = ["one two-three", "", "four c—d"];
        assert_eq!(go(&l, (0, 0), NextWordStart), Some((0, 4)));
        assert_eq!(go(&l, (0, 4), NextWordStart), Some((0, 7)));
        assert_eq!(go(&l, (0, 7), NextWordStart), Some((0, 8)));
        // 빈 줄은 건너뛴다(vim과 다름)
        assert_eq!(go(&l, (0, 8), NextWordStart), Some((2, 0)));
        assert_eq!(go(&l, (2, 5), NextWordEnd), Some((2, 7)));
        assert_eq!(go(&l, (0, 0), NextWordEnd), Some((0, 2)));
        assert_eq!(go(&l, (0, 2), NextWordEnd), Some((0, 6)));
        assert_eq!(go(&l, (2, 0), PreviousWordStart), Some((0, 8)));
        assert_eq!(go(&l, (0, 10), PreviousWordStart), Some((0, 8)));
        assert_eq!(go(&l, (0, 0), PreviousWordStart), None);
        assert_eq!(go(&l, (0, 4), NextBigWordStart), Some((2, 0)));
        assert_eq!(go(&l, (0, 0), NextBigWordEnd), Some((0, 2)));
        assert_eq!(go(&l, (0, 4), NextBigWordEnd), Some((0, 12)));
        assert_eq!(go(&l, (2, 7), PreviousBigWordStart), Some((2, 5)));
        // 마지막 낱말에서 w는 움직이지 않는다
        assert_eq!(go(&l, (2, 5), NextWordStart), None);
        // 접힌 줄은 잇는다
        let wrapped = ["ab", "\\cd ef"];
        assert_eq!(word_target(&rows(&wrapped, 2), 0, (0, 0), NextWordEnd), Some((1, 1)));
        // 넓은 글자는 어느 칸이든 그 글자, e는 머리 칸
        let wide = ["x 한글 y"];
        assert_eq!(go(&wide, (0, 0), NextWordStart), Some((0, 2)));
        assert_eq!(go(&wide, (0, 3), NextWordEnd), Some((0, 4)));
        assert_eq!(go(&wide, (0, 5), PreviousWordStart), Some((0, 2)));
        // 넓은 글자가 다음 줄로 넘어가 남긴 빈 끝 칸은 낱말을 가르지 않는다(herdr SpacerHead)
        let mut spill = rows(&["ab", "字c"], 3);
        spill[0].wrapped = true;
        assert_eq!(word_target(&spill, 0, (0, 0), NextWordEnd), Some((1, 2)));
    }

    /// smart case 글자 그대로 찾기: 접힌 줄을 넘는 일치, 줄 끝 공백 빼기, 넓은 글자 끝 칸
    #[test]
    fn search_like_herdr() {
        let find = |lines: &[&str], width: u16, q: &str| {
            let m = Matcher::new(q).unwrap();
            let (mut lines_, mut out) = (Lines::default(), Vec::new());
            for (i, r) in rows(lines, width).iter().enumerate() {
                lines_.push_row(i, r, &m, &mut out);
            }
            lines_.finish(&m, &mut out);
            out
        };
        assert_eq!(find(&["café", "CAFÉ"], 8, "café"), vec![((0, 0), (0, 3)), ((1, 0), (1, 3))]);
        assert_eq!(find(&["café", "CAFÉ"], 8, "CAFÉ"), vec![((1, 0), (1, 3))]);
        assert_eq!(find(&["a.b a?b"], 8, "a?b"), vec![((0, 4), (0, 6))]);
        assert_eq!(find(&["needle", "\\dle"], 6, "edled"), vec![((0, 2), (1, 0))]);
        assert_eq!(find(&["x 한글"], 8, "한글"), vec![((0, 2), (0, 5))]);
        assert_eq!(find(&["aaa"], 8, "aa"), vec![((0, 0), (0, 1))]);
        assert!(find(&["a  "], 8, " ").is_empty());
        assert!(Matcher::new("").is_none());

        let all = vec![((0, 0), (0, 2)), ((0, 11), (0, 13)), ((3, 0), (3, 2))];
        // 앞으로 새 찾기는 커서 자리 일치를 건너뛰고, 끝에서는 처음으로 돈다
        assert_eq!(pick(all.clone(), true, (0, 0), None, 1024).current_global, Some(1));
        assert_eq!(pick(all.clone(), true, (3, 0), None, 1024).current_global, Some(0));
        assert_eq!(pick(all.clone(), true, (0, 0), Some(all[1]), 1024).current_global, Some(2));
        // 뒤로는 커서에 닿는 일치를 건너뛴다
        assert_eq!(pick(all.clone(), false, (0, 12), None, 1024).current_global, Some(0));
        assert_eq!(pick(all.clone(), false, (0, 1), None, 1024).current_global, Some(2));
        let f = pick(all, true, (0, 5), None, 2);
        assert_eq!((f.matches.len(), f.total, f.current, f.current_global), (2, 3, Some(1), Some(1)));
        assert_eq!(pick(vec![], true, (0, 0), None, 1024), Found::default());
    }
}

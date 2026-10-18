//! aaa가 더한 것(plan §8.150): 글자 폭과 묶음을 웹 터미널(xterm + grapheme addon + herdr 보정)과 같게 센다.
//! 표(`widths_table.rs`)는 웹 provider에서 만들고, 앞 글자와 묶을지는 addon `shouldJoin`·`charProperties`와 같은 규칙이다.
//! 만든 쪽(web/scripts/vt-widths.ts)이 모든 code point에서 「표 + 이 규칙」이 웹과 같은지 대조한다.

const EXTEND: u8 = 2;
const REGIONAL: u8 = 3;
const SPACING_MARK: u8 = 4;
const L: u8 = 5;
const V: u8 = 6;
const T: u8 = 7;
const LV: u8 = 8;
const LVT: u8 = 9;
const ZWJ: u8 = 10;
const EXT_PIC: u8 = 11;
const PREPEND: u8 = 1;
const OTHER: u8 = 0;

const NEVER: u8 = 0x20;
const INVISIBLE: u8 = 0x40;

/// 앞 글자가 남긴 상태: grapheme 부류(지역 표시 짝 뒤는 Other)와 그 묶음의 폭
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Join {
    code: u8,
    pub(crate) width: u16,
}

/// 한 글자를 놓는 법
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Step {
    /// 앞 칸의 묶음에 붙는다
    pub(crate) join: bool,
    /// 붙으면 그 묶음의 새 폭, 아니면 이 글자의 폭(1이나 2)
    pub(crate) width: u16,
    pub(crate) next: Join,
}

fn lookup(c: char) -> u8 {
    let cp = u32::from(c);
    let table = &crate::widths_table::RANGES;
    let i = table.partition_point(|&(start, _)| start <= cp);
    // 표는 0부터 시작하므로 i는 1 이상이다
    table[i - 1].1
}

/// addon `_shouldJoin`(GB6~GB13을 줄인 판)
fn joins(before: u8, after: u8) -> bool {
    if before == L && matches!(after, L | V | LV | LVT) {
        return true;
    }
    if matches!(before, LV | V) && matches!(after, V | T) {
        return true;
    }
    if matches!(before, LVT | T) && after == T {
        return true;
    }
    if matches!(after, EXTEND | ZWJ | SPACING_MARK) || before == PREPEND {
        return true;
    }
    if before == ZWJ && after == EXT_PIC {
        return true;
    }
    before == REGIONAL && after == REGIONAL
}

/// 앞 상태(`None`은 줄 처음이나 제어·escape 뒤)에서 `c`를 놓는 법
pub(crate) fn step(prev: Option<Join>, c: char) -> Step {
    let v = lookup(c);
    let code = v & 0x0f;
    let width = if v & 0x10 == 0 { 1 } else { 2 };
    let alone = Step {
        join: false,
        width,
        next: Join { code, width },
    };
    let Some(prev) = prev else { return alone };
    if v & 0x60 == NEVER {
        return alone;
    }
    // 보이지 않는 글자는 앞 칸에 붙는 0칸(ZWNJ처럼)
    let (code, width) = if v & 0x60 == INVISIBLE {
        (EXTEND, 1)
    } else {
        (code, width)
    };
    if !joins(prev.code, code) {
        return Step {
            join: false,
            width,
            next: Join { code, width },
        };
    }
    // 묶이면 넓은 쪽이다. 지역 표시 둘은 두 칸(국기)이고 셋째는 새로 시작한다
    if code == REGIONAL {
        let width = prev.width.max(2);
        return Step {
            join: true,
            width,
            next: Join { code: OTHER, width },
        };
    }
    let width = prev.width.max(width);
    Step {
        join: true,
        width,
        next: Join { code, width },
    }
}

/// 한 칸의 글을 줄 처음부터 놓은 뒤의 상태(그 칸의 글은 모두 한 묶음이다)
pub(crate) fn fold(cell: &str) -> Option<Join> {
    cell.chars().fold(None, |s, c| Some(step(s, c).next))
}

/// 앞 상태 뒤에 `cell`을 이어 쓰면 그 첫 글자가 앞 묶음에 붙는가
pub(crate) fn would_join(prev: Option<Join>, cell: &str) -> bool {
    match (prev, cell.chars().next()) {
        (Some(prev), Some(c)) => step(Some(prev), c).join,
        _ => false,
    }
}

/// 따로 놓인 두 칸의 글(`prev` 바로 뒤 `next`)을 escape 없이 이어 쓰면 받는 터미널이 `next`를 `prev` 칸에 붙이는가.
/// 둘 사이에 escape가 있어서 따로 놓였던 묶음(ESC 뒤 결합 문자 등)이다. 화면을 다시 쓰는 쪽은 이때 사이에 escape를 넣는다
pub fn joins_after(prev: &str, next: &str) -> bool {
    // 흔한 경우: ASCII 뒤 ASCII는 붙지 않는다(ASCII는 Prepend 뒤에만 붙는다)
    if next.as_bytes().first().is_some_and(u8::is_ascii)
        && prev.as_bytes().last().is_some_and(u8::is_ascii)
    {
        return false;
    }
    would_join(fold(prev), next)
}

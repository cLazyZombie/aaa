//! 키 → 터미널 입력 bytes, 키 이름 해석(순수 함수).
//!
//! 키 이름과 맞추기는 herdr 0.9.5 `src/config/keybinds.rs`(`parse_key_combo`·`format_key_combo`·
//! `normalize_key_combo`·`key_parts_match_combo`)를 따른다.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// 키 하나(키 + modifier). Shift+Tab은 `BackTab`, Shift+글자는 소문자 + SHIFT로 맞춰 둔다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Combo {
    pub code: KeyCode,
    pub mods: KeyModifiers,
}

impl Combo {
    pub const fn new(code: KeyCode, mods: KeyModifiers) -> Combo {
        Combo { code, mods }
    }

    pub const fn plain(c: char) -> Combo {
        Combo { code: KeyCode::Char(c), mods: KeyModifiers::NONE }
    }

    pub const fn shift(c: char) -> Combo {
        Combo { code: KeyCode::Char(c), mods: KeyModifiers::SHIFT }
    }
}

/// herdr 기본 prefix
pub const DEFAULT_PREFIX: Combo = Combo::new(KeyCode::Char('b'), KeyModifiers::CONTROL);

/// 비교할 모양으로 맞춘다: Shift+Tab → BackTab, Shift 글자는 소문자 + SHIFT, 기호는 Shift를 뗀다
/// (터미널은 `?`·`%`를 Shift와 함께 보내기도 하고 아니기도 하다).
pub fn normalize(code: KeyCode, mods: KeyModifiers) -> Combo {
    let mut mods = mods & (KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT | KeyModifiers::SUPER);
    let code = match code {
        KeyCode::Tab if mods.contains(KeyModifiers::SHIFT) => {
            mods.remove(KeyModifiers::SHIFT);
            KeyCode::BackTab
        }
        KeyCode::BackTab => {
            mods.remove(KeyModifiers::SHIFT);
            KeyCode::BackTab
        }
        KeyCode::Char(c) if c.is_ascii_uppercase() => {
            mods.insert(KeyModifiers::SHIFT);
            KeyCode::Char(c.to_ascii_lowercase())
        }
        KeyCode::Char(c) if c.is_ascii_lowercase() => KeyCode::Char(c),
        KeyCode::Char(c) => {
            // Ctrl 조합이 아닌 기호·숫자·글자(한글 등)는 Shift를 보지 않는다
            if !mods.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) {
                mods.remove(KeyModifiers::SHIFT);
            }
            KeyCode::Char(c)
        }
        other => other,
    };
    Combo { code, mods }
}

pub fn of(k: &KeyEvent) -> Combo {
    normalize(k.code, k.modifiers)
}

/// `ctrl+b`·`prefix`에 쓰는 이름(herdr 형식). 예전 `C-b` 형식도 읽는다.
pub fn parse_combo(s: &str) -> Option<Combo> {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("C-") {
        let mut it = rest.chars();
        let c = it.next()?;
        return it.next().is_none().then(|| normalize(KeyCode::Char(c.to_ascii_lowercase()), KeyModifiers::CONTROL));
    }
    if s == "+" {
        return Some(Combo::plain('+'));
    }
    let mut mods = KeyModifiers::NONE;
    let mut key: Option<&str> = None;
    for part in s.split('+') {
        let t = part.trim();
        if t.is_empty() {
            return None;
        }
        match t.to_lowercase().as_str() {
            "ctrl" | "control" => mods |= KeyModifiers::CONTROL,
            "shift" => mods |= KeyModifiers::SHIFT,
            "alt" | "option" | "meta" => mods |= KeyModifiers::ALT,
            "cmd" | "command" | "super" => mods |= KeyModifiers::SUPER,
            _ if key.is_some() => return None,
            _ => key = Some(t),
        }
    }
    let key = key?;
    let mut chars = key.chars();
    let single = chars.next().filter(|_| chars.next().is_none());
    let code = match key.to_lowercase().as_str() {
        "space" => KeyCode::Char(' '),
        "enter" | "return" => KeyCode::Enter,
        "esc" | "escape" => KeyCode::Esc,
        "tab" => KeyCode::Tab,
        "backspace" | "bs" => KeyCode::Backspace,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "minus" => KeyCode::Char('-'),
        "comma" => KeyCode::Char(','),
        "period" => KeyCode::Char('.'),
        "slash" => KeyCode::Char('/'),
        "backslash" => KeyCode::Char('\\'),
        "semicolon" => KeyCode::Char(';'),
        "plus" => KeyCode::Char('+'),
        _ if single.is_some() => KeyCode::Char(single?),
        k if k.starts_with('f') => KeyCode::F(k[1..].parse().ok()?),
        _ => return None,
    };
    Some(normalize(code, mods))
}

/// prefix 설정 값들. 읽지 못한 값은 빼고, 남는 것이 없으면 herdr처럼 기본(`ctrl+b`)으로 둔다.
/// 둘째 값은 읽지 못한 값(알림용).
pub fn parse_prefixes(values: &[&str]) -> (Vec<Combo>, Vec<String>) {
    let mut out: Vec<Combo> = Vec::new();
    let mut bad = Vec::new();
    for v in values {
        match parse_combo(v) {
            Some(c) if !out.contains(&c) => out.push(c),
            Some(_) => {}
            None => bad.push(v.to_string()),
        }
    }
    if out.is_empty() {
        out.push(DEFAULT_PREFIX);
    }
    (out, bad)
}

/// herdr `format_key_combo`와 같은 이름(`ctrl+b`, `shift+x`, `tab`)
pub fn label(c: Combo) -> String {
    let mut parts: Vec<String> = Vec::new();
    if c.mods.contains(KeyModifiers::CONTROL) {
        parts.push("ctrl".into());
    }
    if c.mods.contains(KeyModifiers::ALT) {
        parts.push("alt".into());
    }
    if c.mods.contains(KeyModifiers::SHIFT) {
        parts.push("shift".into());
    }
    if c.mods.contains(KeyModifiers::SUPER) {
        parts.push("cmd".into());
    }
    let key = match c.code {
        KeyCode::Char(' ') => "space".to_string(),
        KeyCode::Char(ch) => ch.to_string(),
        KeyCode::Enter => "enter".into(),
        KeyCode::Esc => "esc".into(),
        KeyCode::Tab => "tab".into(),
        KeyCode::BackTab => "shift+tab".into(),
        KeyCode::Backspace => "backspace".into(),
        KeyCode::Left => "left".into(),
        KeyCode::Right => "right".into(),
        KeyCode::Up => "up".into(),
        KeyCode::Down => "down".into(),
        KeyCode::F(n) => format!("f{n}"),
        other => format!("{other:?}").to_lowercase(),
    };
    parts.push(key);
    parts.join("+")
}

/// prefix 키를 그 pane에 보낼 때의 bytes(prefix를 두 번 누름)
pub fn combo_bytes(c: Combo) -> Vec<u8> {
    let mut code = c.code;
    let mut mods = c.mods;
    if let KeyCode::Char(ch) = code
        && mods.contains(KeyModifiers::SHIFT)
        && ch.is_ascii_lowercase()
    {
        code = KeyCode::Char(ch.to_ascii_uppercase());
        mods.remove(KeyModifiers::SHIFT);
    }
    encode(&KeyEvent::new(code, mods))
}

/// 두벌식 자모 → 같은 자리 영문 키(prefix 뒤 명령 키, `[tui] prefix_jamo_keys`). 한글 입력기가 켜진 채
/// prefix 뒤 키를 누르면 터미널이 자모를 보낸다(입력기를 바꾸지 못하는 ssh·다른 OS).
/// 쌍자음·ㅒ·ㅖ는 Shift 자리(대문자)다.
pub fn jamo_key(c: char) -> Option<char> {
    const MAP: &[(char, char)] = &[
        ('ㅂ', 'q'),
        ('ㅈ', 'w'),
        ('ㄷ', 'e'),
        ('ㄱ', 'r'),
        ('ㅅ', 't'),
        ('ㅛ', 'y'),
        ('ㅕ', 'u'),
        ('ㅑ', 'i'),
        ('ㅐ', 'o'),
        ('ㅔ', 'p'),
        ('ㅁ', 'a'),
        ('ㄴ', 's'),
        ('ㅇ', 'd'),
        ('ㄹ', 'f'),
        ('ㅎ', 'g'),
        ('ㅗ', 'h'),
        ('ㅓ', 'j'),
        ('ㅏ', 'k'),
        ('ㅣ', 'l'),
        ('ㅋ', 'z'),
        ('ㅌ', 'x'),
        ('ㅊ', 'c'),
        ('ㅍ', 'v'),
        ('ㅠ', 'b'),
        ('ㅜ', 'n'),
        ('ㅡ', 'm'),
        ('ㅃ', 'Q'),
        ('ㅉ', 'W'),
        ('ㄸ', 'E'),
        ('ㄲ', 'R'),
        ('ㅆ', 'T'),
        ('ㅒ', 'O'),
        ('ㅖ', 'P'),
    ];
    MAP.iter().find(|(j, _)| *j == c).map(|(_, k)| *k)
}

fn ctrl_byte(c: char) -> Option<u8> {
    match c {
        'a'..='z' => Some(c as u8 - b'a' + 1),
        '@' | ' ' | '2' => Some(0),
        '[' => Some(0x1b),
        '\\' => Some(0x1c),
        ']' => Some(0x1d),
        '^' => Some(0x1e),
        '_' => Some(0x1f),
        _ => None,
    }
}

fn modifier_param(m: KeyModifiers) -> u8 {
    1 + u8::from(m.contains(KeyModifiers::SHIFT))
        + 2 * u8::from(m.contains(KeyModifiers::ALT))
        + 4 * u8::from(m.contains(KeyModifiers::CONTROL))
}

/// xterm 방식 입력 bytes. 보낼 것이 없으면 빈 Vec.
pub fn encode(k: &KeyEvent) -> Vec<u8> {
    encode_in(k, false)
}

/// `app_cursor`(pane 프로그램이 DECCKM을 켬)면 수정키 없는 화살표를 `ESC O x`로(xterm·herdr, aaa-vt `KeyModes::key`와 같다)
pub fn encode_in(k: &KeyEvent, app_cursor: bool) -> Vec<u8> {
    let m = k.modifiers;
    let csi = |end: &str| -> Vec<u8> {
        let p = modifier_param(m);
        if p == 1 { format!("\x1b[{end}").into_bytes() } else { format!("\x1b[1;{p}{end}").into_bytes() }
    };
    let arrow = |end: &str| -> Vec<u8> {
        if app_cursor && modifier_param(m) == 1 { format!("\x1bO{end}").into_bytes() } else { csi(end) }
    };
    let tilde = |n: u8| -> Vec<u8> {
        let p = modifier_param(m);
        if p == 1 { format!("\x1b[{n}~").into_bytes() } else { format!("\x1b[{n};{p}~").into_bytes() }
    };
    let mut out = match k.code {
        KeyCode::Char(c) if m.contains(KeyModifiers::CONTROL) => match ctrl_byte(c.to_ascii_lowercase()) {
            Some(b) => vec![b],
            None => c.to_string().into_bytes(),
        },
        KeyCode::Char(c) => c.to_string().into_bytes(),
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Esc => vec![0x1b],
        KeyCode::Up => arrow("A"),
        KeyCode::Down => arrow("B"),
        KeyCode::Right => arrow("C"),
        KeyCode::Left => arrow("D"),
        KeyCode::Home => csi("H"),
        KeyCode::End => csi("F"),
        KeyCode::Insert => tilde(2),
        KeyCode::Delete => tilde(3),
        KeyCode::PageUp => tilde(5),
        KeyCode::PageDown => tilde(6),
        KeyCode::F(n @ 1..=4) => format!("\x1bO{}", (b'P' + n - 1) as char).into_bytes(),
        KeyCode::F(n @ 5..=12) => tilde([15, 17, 18, 19, 20, 21, 23, 24][(n - 5) as usize]),
        _ => Vec::new(),
    };
    // Alt+글자는 ESC를 앞에 붙인다(화살표 등은 이미 modifier 인자에 들어갔다)
    if m.contains(KeyModifiers::ALT) && matches!(k.code, KeyCode::Char(_) | KeyCode::Enter | KeyCode::Backspace) {
        out.insert(0, 0x1b);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(code: KeyCode, m: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, m)
    }

    #[test]
    fn jamo_is_read_as_the_key_at_the_same_place() {
        for (j, k) in
            [('ㅊ', 'c'), ('ㅌ', 'x'), ('ㅂ', 'q'), ('ㅍ', 'v'), ('ㅠ', 'b'), ('ㅈ', 'w'), ('ㅜ', 'n'), ('ㅔ', 'p')]
        {
            assert_eq!(jamo_key(j), Some(k), "{j}");
        }
        assert_eq!(jamo_key('ㅆ'), Some('T'));
        // 자모가 아니면 그대로 둔다(완성 음절·영문·기호)
        for c in ['가', 'c', '%', '"', '1'] {
            assert_eq!(jamo_key(c), None, "{c}");
        }
    }

    #[test]
    fn combo_names_like_herdr() {
        let ctrl = KeyModifiers::CONTROL;
        assert_eq!(parse_combo("ctrl+b"), Some(Combo::new(KeyCode::Char('b'), ctrl)));
        assert_eq!(parse_combo("Ctrl+A"), Some(Combo::new(KeyCode::Char('a'), ctrl | KeyModifiers::SHIFT)));
        assert_eq!(parse_combo("ctrl+space"), Some(Combo::new(KeyCode::Char(' '), ctrl)));
        assert_eq!(parse_combo("shift+tab"), Some(Combo::new(KeyCode::BackTab, KeyModifiers::NONE)));
        assert_eq!(parse_combo("minus"), Some(Combo::plain('-')));
        // 예전 형식
        assert_eq!(parse_combo("C-g"), Some(Combo::new(KeyCode::Char('g'), ctrl)));
        assert_eq!(parse_combo(" C-A "), Some(Combo::new(KeyCode::Char('a'), ctrl)));
        for bad in ["", "ctrl+", "ctrl+bb", "C-bb", "b+c"] {
            assert_eq!(parse_combo(bad), None, "{bad}");
        }
        for s in ["ctrl+b", "shift+x", "tab", "shift+tab", "ctrl+space", "?", "-"] {
            assert_eq!(label(parse_combo(s).unwrap()), s);
        }
        // 읽지 못한 값은 빼고, 남는 것이 없으면 기본
        assert_eq!(parse_prefixes(&["nope"]), (vec![DEFAULT_PREFIX], vec!["nope".to_string()]));
        assert_eq!(parse_prefixes(&["ctrl+a", "ctrl+a", "C-b"]).0.len(), 2);
    }

    #[test]
    fn keys_from_the_terminal_match_combos() {
        // 터미널은 Shift 글자를 대문자로(+SHIFT 또는 없이), 기호는 Shift와 함께 또는 없이 보낸다
        assert_eq!(of(&k(KeyCode::Char('X'), KeyModifiers::SHIFT)), Combo::shift('x'));
        assert_eq!(of(&k(KeyCode::Char('X'), KeyModifiers::NONE)), Combo::shift('x'));
        assert_eq!(of(&k(KeyCode::Char('?'), KeyModifiers::SHIFT)), Combo::plain('?'));
        assert_eq!(of(&k(KeyCode::Tab, KeyModifiers::SHIFT)), Combo::new(KeyCode::BackTab, KeyModifiers::NONE));
        assert_eq!(of(&k(KeyCode::BackTab, KeyModifiers::SHIFT)), Combo::new(KeyCode::BackTab, KeyModifiers::NONE));
        assert_eq!(of(&k(KeyCode::Char('b'), KeyModifiers::CONTROL)), DEFAULT_PREFIX);
        // prefix 두 번은 그 키 bytes
        assert_eq!(combo_bytes(DEFAULT_PREFIX), [2]);
        assert_eq!(combo_bytes(parse_combo("alt+x").unwrap()), b"\x1bx");
    }

    #[test]
    fn encodes_like_xterm() {
        let none = KeyModifiers::NONE;
        assert_eq!(encode(&k(KeyCode::Char('한'), none)), "한".as_bytes());
        assert_eq!(encode(&k(KeyCode::Char('c'), KeyModifiers::CONTROL)), [3]);
        assert_eq!(encode(&k(KeyCode::Char('x'), KeyModifiers::ALT)), b"\x1bx");
        assert_eq!(encode(&k(KeyCode::Enter, none)), b"\r");
        assert_eq!(encode(&k(KeyCode::Backspace, none)), [0x7f]);
        assert_eq!(encode(&k(KeyCode::Up, none)), b"\x1b[A");
        assert_eq!(encode(&k(KeyCode::Left, KeyModifiers::CONTROL)), b"\x1b[1;5D");
        assert_eq!(encode(&k(KeyCode::PageDown, none)), b"\x1b[6~");
        assert_eq!(encode(&k(KeyCode::F(1), none)), b"\x1bOP");
        assert_eq!(encode(&k(KeyCode::F(5), none)), b"\x1b[15~");
        // DECCKM: 수정키 없는 화살표만 SS3
        assert_eq!(encode_in(&k(KeyCode::Up, none), true), b"\x1bOA");
        assert_eq!(encode_in(&k(KeyCode::Left, KeyModifiers::CONTROL), true), b"\x1b[1;5D");
        assert_eq!(encode_in(&k(KeyCode::Home, none), true), b"\x1b[H");
    }
}

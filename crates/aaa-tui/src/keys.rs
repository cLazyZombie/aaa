//! 키 → 터미널 입력 bytes, prefix 키 해석(순수 함수).

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// `C-b` 같은 prefix 설정을 해석한다.
pub fn parse_prefix(s: &str) -> Option<(char, u8)> {
    let c = s.trim().strip_prefix("C-")?;
    let mut it = c.chars();
    let ch = it.next()?.to_ascii_lowercase();
    if it.next().is_some() {
        return None;
    }
    let byte = ctrl_byte(ch)?;
    Some((ch, byte))
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

/// prefix 키인가(Ctrl+글자)
pub fn is_prefix(k: &KeyEvent, prefix: char) -> bool {
    k.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(k.code, KeyCode::Char(c) if c.to_ascii_lowercase() == prefix)
}

fn modifier_param(m: KeyModifiers) -> u8 {
    1 + u8::from(m.contains(KeyModifiers::SHIFT))
        + 2 * u8::from(m.contains(KeyModifiers::ALT))
        + 4 * u8::from(m.contains(KeyModifiers::CONTROL))
}

/// xterm 방식 입력 bytes. 보낼 것이 없으면 빈 Vec.
pub fn encode(k: &KeyEvent) -> Vec<u8> {
    let m = k.modifiers;
    let csi = |end: &str| -> Vec<u8> {
        let p = modifier_param(m);
        if p == 1 { format!("\x1b[{end}").into_bytes() } else { format!("\x1b[1;{p}{end}").into_bytes() }
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
        KeyCode::Up => csi("A"),
        KeyCode::Down => csi("B"),
        KeyCode::Right => csi("C"),
        KeyCode::Left => csi("D"),
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
    fn prefix_parse() {
        assert_eq!(parse_prefix("C-b"), Some(('b', 2)));
        assert_eq!(parse_prefix("C-g"), Some(('g', 7)));
        assert_eq!(parse_prefix(" C-A "), Some(('a', 1)));
        assert_eq!(parse_prefix("b"), None);
        assert_eq!(parse_prefix("C-bb"), None);
        assert!(is_prefix(&k(KeyCode::Char('b'), KeyModifiers::CONTROL), 'b'));
        assert!(!is_prefix(&k(KeyCode::Char('b'), KeyModifiers::NONE), 'b'));
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
    }
}

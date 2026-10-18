//! pane 기록을 편집기로(herdr 0.9.5 `prefix e` `edit_scrollback`, `src/app/custom_commands.rs`·`src/platform/*.rs`, plan §8.175).
//!
//! - host가 화면 사본의 기록을 조각으로 보내면 이 머신 임시 폴더에 `aaa-scrollback-<name>.txt`로 쓴다(herdr
//!   `write_scrollback_temp_file`: 새 파일만, 0600). 이름은 host가 정한 hex 16자이고 다른 이름·경로는 받지 않는다.
//! - 임시 폴더가 `[agent] file_roots` 밖이면 쓰지 않는다(lib.rs `scrollback_write`).
//! - 다 쓰면 그 파일을 여는 편집기 명령을 돌려준다(herdr `scrollback_editor_argv`). host가 그 명령으로 편집기 pane을 띄운다.
//! - pane 기록에는 비밀 글이 있을 수 있다: 내용과 경로를 로그에 남기지 않는다.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

/// 기록 파일 한 개 상한(host 화면 사본 기록 10,000줄의 넉넉한 위)
pub const MAX_BYTES: u64 = 64 << 20;

fn valid_name(name: &str) -> bool {
    name.len() == 16 && name.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

fn file_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("aaa-scrollback-{name}.txt"))
}

/// 한 조각을 쓴다. 마지막 조각이면 절대 경로를 돌려준다.
pub fn write(dir: &Path, name: &str, offset: u64, data: &[u8], eof: bool) -> Result<Option<String>> {
    if !valid_name(name) {
        bail!("기록 파일 이름 형식이 아니다");
    }
    if offset + data.len() as u64 > MAX_BYTES {
        bail!("기록이 {}MiB를 넘는다", MAX_BYTES >> 20);
    }
    let path = file_path(dir, name);
    let mut f = if offset == 0 {
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            o.mode(0o600);
        }
        o.open(&path)?
    } else {
        let f = std::fs::OpenOptions::new().append(true).open(&path)?;
        let have = f.metadata()?.len();
        if have != offset {
            bail!("조각 위치가 맞지 않다(쓴 길이 {have}, 받은 위치 {offset})");
        }
        f
    };
    f.write_all(data)?;
    if !eof {
        return Ok(None);
    }
    f.flush()?;
    Ok(Some(std::path::absolute(&path)?.to_string_lossy().into_owned()))
}

/// 쓴 파일을 지운다. 이미 없으면 그대로.
pub fn remove(dir: &Path, name: &str) -> Result<()> {
    if !valid_name(name) {
        bail!("기록 파일 이름 형식이 아니다");
    }
    match std::fs::remove_file(file_path(dir, name)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

/// herdr unix `shell_quote`
fn shell_quote(value: &str) -> String {
    let plain =
        |c: char| c.is_ascii_alphanumeric() || matches!(c, '@' | '%' | '_' | '+' | '=' | ':' | ',' | '.' | '/' | '-');
    if !value.is_empty() && value.chars().all(plain) {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// 편집기 명령(herdr `scrollback_editor_argv`).
/// - unix: `/bin/sh -c`가 pane 안에서 `${EDITOR:-vi}`를 풀고, 편집기가 끝나면 파일을 지우고 그 종료 코드로 끝난다.
/// - Windows: `editor`(`VISUAL` → `EDITOR`, 빈 값은 없는 것) → `notepad.exe` 뒤에 경로를 붙인다. 파일은 pane이 끝나면 host가
///   `ScrollbackRemove`로 지운다(herdr도 pane이 끝나면 지운다).
pub fn editor_argv(windows: bool, editor: Option<&str>, path: &str) -> Result<Vec<String>> {
    if !windows {
        let command = format!(
            r#"scrollback_file={}; eval "${{EDITOR:-vi}} \"\$scrollback_file\""; status=$?; rm -f "$scrollback_file"; exit $status"#,
            shell_quote(path)
        );
        return Ok(vec!["/bin/sh".into(), "-c".into(), command]);
    }
    let mut argv = match editor.filter(|e| !e.trim().is_empty()) {
        Some(e) => windows_words(e),
        None => vec!["notepad.exe".into()],
    };
    if argv.is_empty() {
        bail!("편집기 명령이 비었다");
    }
    argv.push(path.to_string());
    Ok(argv)
}

/// Windows 명령 줄 낱말(herdr는 `CommandLineToArgvW`). OS API 없이 그 규칙만 옮겼다: 첫 낱말(프로그램)은 `"`로 시작하면 다음 `"`까지,
/// 아니면 빈칸까지(역슬래시는 글자 그대로). 나머지는 `"`가 묶음을 켜고 끄며, `"` 앞 역슬래시 2n개는 n개, 2n+1개는 n개와 `"` 글자다.
fn windows_words(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = line.trim_start().chars().peekable();
    let mut first = String::new();
    if chars.peek() == Some(&'"') {
        chars.next();
        first.extend(chars.by_ref().take_while(|&c| c != '"'));
    } else {
        while let Some(&c) = chars.peek() {
            if c == ' ' || c == '\t' {
                break;
            }
            first.push(c);
            chars.next();
        }
    }
    out.push(first);
    loop {
        while chars.peek().is_some_and(|&c| c == ' ' || c == '\t') {
            chars.next();
        }
        if chars.peek().is_none() {
            return out;
        }
        let (mut word, mut quoted, mut slashes) = (String::new(), false, 0usize);
        while let Some(&c) = chars.peek() {
            match c {
                '\\' => slashes += 1,
                '"' => {
                    word.extend(std::iter::repeat_n('\\', slashes / 2));
                    if slashes % 2 == 1 {
                        word.push('"');
                    } else {
                        quoted = !quoted;
                    }
                    slashes = 0;
                }
                ' ' | '\t' if !quoted => break,
                _ => {
                    word.extend(std::iter::repeat_n('\\', slashes));
                    slashes = 0;
                    word.push(c);
                }
            }
            chars.next();
        }
        word.extend(std::iter::repeat_n('\\', slashes));
        out.push(word);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_names_and_remove() {
        let dir = std::env::temp_dir().join(format!("aaa-scrollback-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for bad in ["../0123456789abcd", "0123456789ABCDEF", "0123456789abcde", "0123456789abcdeg"] {
            assert!(write(&dir, bad, 0, b"a", true).is_err(), "{bad}");
            assert!(remove(&dir, bad).is_err(), "{bad}");
        }
        let name = "0123456789abcdef";
        assert!(write(&dir, name, 0, b"ab", false).unwrap().is_none());
        assert!(write(&dir, name, 5, b"cd", true).is_err(), "건너뛴 위치");
        let path = write(&dir, name, 2, b"cd", true).unwrap().unwrap();
        let p = Path::new(&path);
        assert!(p.is_absolute() && p.ends_with(format!("aaa-scrollback-{name}.txt")), "{path}");
        assert_eq!(std::fs::read(p).unwrap(), b"abcd");
        assert!(write(&dir, name, 0, b"x", true).is_err(), "있는 파일을 덮어쓰지 않는다");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(p).unwrap().permissions().mode() & 0o777, 0o600);
        }
        remove(&dir, name).unwrap();
        assert!(!p.exists());
        remove(&dir, name).unwrap();
        // 빈 기록도 파일이 된다(herdr도 빈 글을 쓴다)
        let empty = "fedcba9876543210";
        let path = write(&dir, empty, 0, b"", true).unwrap().unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// herdr `scrollback_editor_argv_preserves_unix_editor_shell_semantics`
    #[test]
    fn unix_editor_runs_in_the_pane_shell() {
        let argv = editor_argv(false, Some("ignored"), "/tmp/aaa scrollback.txt").unwrap();
        assert_eq!(&argv[..2], ["/bin/sh", "-c"]);
        assert!(argv[2].contains("EDITOR:-vi"));
        assert!(argv[2].starts_with("scrollback_file='/tmp/aaa scrollback.txt'; "), "{}", argv[2]);
        assert!(argv[2].contains(r#"rm -f "$scrollback_file""#));
        assert_eq!(shell_quote("/tmp/aaa-scrollback-0123.txt"), "/tmp/aaa-scrollback-0123.txt");
        assert_eq!(shell_quote("/it's"), r"'/it'\''s'");
    }

    /// herdr `scrollback_editor_argv_uses_editor_env_and_appends_path`·`…_falls_back_to_notepad`
    #[test]
    fn windows_editor_from_env_or_notepad() {
        let path = r"C:\Users\User\AppData\Local\Temp\aaa scrollback.txt";
        let argv = editor_argv(true, Some(r#""C:\Program Files\Microsoft VS Code\Code.exe" --wait"#), path).unwrap();
        assert_eq!(argv, [r"C:\Program Files\Microsoft VS Code\Code.exe", "--wait", path]);
        assert_eq!(editor_argv(true, None, path).unwrap(), ["notepad.exe", path]);
        assert_eq!(editor_argv(true, Some("  "), path).unwrap(), ["notepad.exe", path]);
        assert_eq!(windows_words(r#"C:\bin\ed.exe "a b" c\\\"d e\\"#), [r"C:\bin\ed.exe", "a b", r#"c\"d"#, r"e\\"]);
    }
}

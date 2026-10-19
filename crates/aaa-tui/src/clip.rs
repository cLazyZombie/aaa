//! front 기기 clipboard의 그림(§8.28, M3, `aaa attach`에서 Ctrl+V를 누르면 읽는다)과 고른 글 복사(묶음 3d-1).
//!
//! - macOS NSPasteboard, Windows clipboard(PNG·CF_DIBV5·CF_DIB), Linux X11을 arboard로 읽는다. 순수 Rust라 빌드에
//!   X11·Wayland 개발 파일이 필요 없다. Linux는 `DISPLAY`가 있는 desktop에서만 된다(Wayland는 XWayland로).
//! - arboard는 RGBA를 돌려주므로 PNG로 다시 만든다. 원래 파일 형식·색 profile은 남지 않는다.
//! - 그림이 없거나 clipboard를 열지 못하면(ssh 셸·desktop 없음) `None`이다. 호출한 쪽이 원래 키(0x16)를 보낸다.
//! - 시험용 `AAA_TEST_CLIPBOARD_PNG=<파일>`: 시스템 clipboard 대신 그 파일을 읽는다(없으면 그림 없음).
//!   개발 Mac에서 사람의 clipboard를 덮어쓰지 않고 E2E를 돌리려고 둔다. 실제 clipboard 경로는 CI가 `AAA_CLIP_NATIVE=1`로 본다.
//! - 터미널에 끌어 놓은 그림 파일(차이 목록 4E): 터미널은 놓은 파일의 경로를 붙여 넣는다. 그 글이 그림 파일 경로 하나면
//!   파일을 그림으로 올린다(`dropped_image`, herdr `read_image_file_from_terminal_drop`).

use std::path::{Path, PathBuf};

/// 고른 글을 front 기기 clipboard에(herdr `copy_to_clipboard`, src/selection.rs). ssh·WSL·VS Code 터미널이면 OSC 52
/// (`ESC ] 52 ; c ; <base64> BEL`, 수신 확인 없는 요청), 아니면 시스템 clipboard.
/// Windows 로컬 쓰기는 유효한 자기 창 소유자를 지정하며, 실패는 OSC 52로 감추지 않는다.
/// 시험용 `AAA_TEST_CLIPBOARD_TEXT=<파일>`: 사람의 clipboard 대신 그 파일에 쓴다.
#[derive(Debug, PartialEq, Eq)]
pub enum CopyResult {
    Copied,
    Requested,
}

pub fn copy_text(text: &str) -> Result<CopyResult, String> {
    if let Some(p) = std::env::var_os("AAA_TEST_CLIPBOARD_TEXT") {
        return std::fs::write(p, text).map(|()| CopyResult::Copied).map_err(|e| e.to_string());
    }
    let env = |k: &str| std::env::var_os(k).is_some();
    let remote = env("SSH_CONNECTION")
        || env("SSH_TTY")
        || env("WSL_DISTRO_NAME")
        || (!cfg!(windows) && std::env::var("TERM_PROGRAM").is_ok_and(|t| t == "vscode"));
    if !remote {
        match native_text(text) {
            Ok(()) => return Ok(CopyResult::Copied),
            // Windows Terminal·conhost의 OSC 52 지원에 기대어 실패를 숨기지 않는다.
            Err(e) if cfg!(windows) => return Err(e),
            Err(_) => {}
        }
    }
    osc52(text).map(|()| CopyResult::Requested)
}

/// 시스템 clipboard. Linux X11은 쓴 연결이 살아 있어야 다른 앱이 읽으므로 연결을 들고 있는다.
#[cfg(not(windows))]
fn native_text(text: &str) -> Result<(), String> {
    static CLIP: std::sync::Mutex<Option<arboard::Clipboard>> = std::sync::Mutex::new(None);
    let mut c = CLIP.lock().map_err(|e| e.to_string())?;
    if c.is_none() {
        *c = Some(arboard::Clipboard::new().map_err(|e| e.to_string())?);
    }
    c.as_mut().map_or(Err("clipboard 없음".into()), |c| c.set_text(text).map_err(|e| e.to_string()))
}

#[cfg(windows)]
#[path = "clip_windows.rs"]
mod windows;
#[cfg(windows)]
use windows::native_text;

fn osc52(text: &str) -> Result<(), String> {
    use base64::Engine;
    use std::io::Write;
    let seq = format!("\x1b]52;c;{}\x07", base64::engine::general_purpose::STANDARD.encode(text));
    let mut out = std::io::stdout();
    out.write_all(seq.as_bytes()).and_then(|()| out.flush()).map_err(|e| e.to_string())
}

/// clipboard 그림을 PNG bytes로. 오래 걸릴 수 있으니 blocking thread에서 부른다.
pub fn image_png() -> Option<Vec<u8>> {
    if let Some(p) = std::env::var_os("AAA_TEST_CLIPBOARD_PNG") {
        return std::fs::read(p).ok().filter(|b| !b.is_empty());
    }
    let img = arboard::Clipboard::new().ok()?.get_image().ok()?;
    encode_png(img.width, img.height, &img.bytes).ok()
}

/// RGBA 8bit → PNG
pub fn encode_png(width: usize, height: usize, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let (w, h) = (u32::try_from(width).map_err(|_| "너비가 크다")?, u32::try_from(height).map_err(|_| "높이가 크다")?);
    if w == 0 || h == 0 || width.checked_mul(height).and_then(|n| n.checked_mul(4)) != Some(rgba.len()) {
        return Err(format!("그림 크기({width}x{height})와 bytes({})가 맞지 않다", rgba.len()));
    }
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::Fast);
    let mut wr = enc.write_header().map_err(|e| e.to_string())?;
    wr.write_image_data(rgba).map_err(|e| e.to_string())?;
    wr.finish().map_err(|e| e.to_string())?;
    Ok(out)
}

/// 끌어 놓은 그림으로 올리는 파일 크기 상한(host 첨부 상한과 같다, herdr `MAX_CLIPBOARD_IMAGE_PAYLOAD`)
pub const MAX_DROPPED_IMAGE: u64 = 16 << 20;

/// 붙여 넣은 글이 터미널에 끌어 놓은 그림 파일이면 그 경로(herdr `image_path_from_terminal_drop`,
/// src/client/clipboard_images.rs). 한 줄(끝 줄바꿈은 뺀다), 같은 따옴표로 감쌌으면 벗기고, unix는 `\ ` 같은 escape를 푼다.
/// 절대 경로이고 확장자가 그림이어야 한다. herdr는 원격 client(`--remote`)일 때만 보지만 aaa TUI는 늘 agent 머신과 떨어진
/// front라 늘 본다(차이 목록 #568과 같은 판단).
pub fn dropped_image(text: &str) -> Option<PathBuf> {
    let text = normalized_drop_text(text)?;
    let text = strip_matching_quotes(text);
    #[cfg(unix)]
    let text = unescape_drop_path(text);
    let path = PathBuf::from(text);
    if !path.is_absolute() {
        return None;
    }
    image_extension(path.extension()?.to_str()?)?;
    Some(path)
}

/// 끌어 놓은 그림 파일의 bytes. 보통 파일이 아니거나, 비었거나, 상한을 넘거나, 읽지 못하면 None이다(그 글은 그대로 붙여 넣는다).
pub fn read_dropped_image(path: &Path) -> Option<Vec<u8>> {
    use std::io::Read;
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() == 0 || meta.len() > MAX_DROPPED_IMAGE {
        return None;
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path).ok()?.take(MAX_DROPPED_IMAGE + 1).read_to_end(&mut bytes).ok()?;
    (!bytes.is_empty() && bytes.len() as u64 <= MAX_DROPPED_IMAGE).then_some(bytes)
}

fn normalized_drop_text(text: &str) -> Option<&str> {
    let text = text.trim_end_matches(['\r', '\n']);
    (!text.is_empty() && !text.contains(['\r', '\n'])).then_some(text)
}

fn strip_matching_quotes(text: &str) -> &str {
    match text.as_bytes() {
        [b'\'', .., b'\''] | [b'"', .., b'"'] if text.len() >= 2 => &text[1..text.len() - 1],
        _ => text,
    }
}

#[cfg(unix)]
fn unescape_drop_path(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            out.push(chars.next().unwrap_or(c));
        } else {
            out.push(c);
        }
    }
    out
}

/// 그림 확장자. herdr는 bmp도 보지만 aaa host는 PNG·JPEG·GIF·WebP만 받으므로(signature로 판정) bmp는 글로 붙인다.
fn image_extension(ext: &str) -> Option<&'static str> {
    ["png", "jpg", "jpeg", "gif", "webp"].into_iter().find(|e| ext.eq_ignore_ascii_case(e))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 시험이 끝나면 지우는 임시 파일(이름 조각을 넣을 수 있다). 쓰는 시험이 모두 unix 것이다
    #[cfg(unix)]
    struct TempImage(PathBuf);

    #[cfg(unix)]
    impl TempImage {
        fn new(fragment: &str, ext: &str, bytes: &[u8]) -> TempImage {
            static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let p = std::env::temp_dir().join(format!("aaa-drop-{}-{n}{fragment}.{ext}", std::process::id()));
            std::fs::write(&p, bytes).unwrap();
            TempImage(p)
        }
        fn read(&self, text: &str) -> Option<Vec<u8>> {
            read_dropped_image(&dropped_image(text)?)
        }
    }

    #[cfg(unix)]
    impl Drop for TempImage {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    // herdr src/client/tests/mod.rs `remote_image_file_drop_bridge_*` 넷. crossterm이 bracketed paste 표지를 벗겨 준다
    #[cfg(unix)]
    #[test]
    fn reads_an_absolute_image_path() {
        let f = TempImage::new("", "PNG", b"image-bytes");
        assert_eq!(f.read(&f.0.display().to_string()).as_deref(), Some(&b"image-bytes"[..]));
    }

    #[cfg(unix)]
    #[test]
    fn reads_a_quoted_path_with_a_newline() {
        let f = TempImage::new("", "jpeg", b"jpeg-bytes");
        assert_eq!(f.read(&format!("'{}'\n", f.0.display())).as_deref(), Some(&b"jpeg-bytes"[..]));
        assert_eq!(f.read(&format!("\"{}\"\r\n", f.0.display())).as_deref(), Some(&b"jpeg-bytes"[..]));
    }

    #[cfg(unix)]
    #[test]
    fn unescapes_spaces_in_paths() {
        let f = TempImage::new("space test", "png", b"image-bytes");
        assert_eq!(f.read(&f.0.display().to_string().replace(' ', "\\ ")).as_deref(), Some(&b"image-bytes"[..]));
    }

    #[cfg(unix)]
    #[test]
    fn ignores_what_is_not_one_image_path() {
        let f = TempImage::new("", "png", b"image-bytes");
        assert!(dropped_image("relative.png\n").is_none());
        assert!(dropped_image("/tmp/file.txt\n").is_none());
        assert!(dropped_image(&format!("{}\nextra", f.0.display())).is_none());
        assert!(dropped_image("").is_none());
        // aaa host가 받지 않는 형식은 그림으로 보지 않는다(herdr와 다름)
        assert!(dropped_image("/tmp/a.bmp").is_none());
        // 그림 이름이어도 보통 파일이 아니거나 비었으면 읽지 않는다
        assert!(read_dropped_image(&std::env::temp_dir()).is_none());
        let empty = TempImage::new("", "png", b"");
        assert!(empty.read(&empty.0.display().to_string()).is_none());
        // 상한을 넘는 파일(host 첨부 상한, herdr `LimitedRead::Oversized`)은 글로 붙인다. 빈 곳은 sparse라 쓰지 않는다
        let big = TempImage::new("", "png", b"");
        std::fs::File::options().write(true).open(&big.0).unwrap().set_len(MAX_DROPPED_IMAGE + 1).unwrap();
        assert!(big.read(&big.0.display().to_string()).is_none());
        std::fs::File::options().write(true).open(&big.0).unwrap().set_len(MAX_DROPPED_IMAGE).unwrap();
        assert_eq!(big.read(&big.0.display().to_string()).map(|b| b.len() as u64), Some(MAX_DROPPED_IMAGE));
    }

    /// 불투명한 기본색(색 관리가 바꾸지 않는 값)으로 만든 3x2 그림
    pub(crate) const PIXELS: [[u8; 4]; 6] = [
        [255, 0, 0, 255],
        [0, 255, 0, 255],
        [0, 0, 255, 255],
        [255, 255, 255, 255],
        [0, 0, 0, 255],
        [255, 255, 0, 255],
    ];

    fn decode(png_bytes: &[u8]) -> (u32, u32, Vec<u8>) {
        let mut r = png::Decoder::new(std::io::Cursor::new(png_bytes)).read_info().unwrap();
        let mut buf = vec![0; r.output_buffer_size().unwrap()];
        let info = r.next_frame(&mut buf).unwrap();
        assert_eq!((info.color_type, info.bit_depth), (png::ColorType::Rgba, png::BitDepth::Eight));
        buf.truncate(info.buffer_size());
        (info.width, info.height, buf)
    }

    #[test]
    fn png_roundtrip() {
        let rgba: Vec<u8> = PIXELS.concat();
        let p = encode_png(3, 2, &rgba).unwrap();
        assert!(p.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert_eq!(decode(&p), (3, 2, rgba.clone()));
        assert!(encode_png(3, 3, &rgba).is_err());
        assert!(encode_png(0, 0, &[]).is_err());
    }

    /// 실제 시스템 clipboard에 그림을 쓰고 이 모듈로 읽는다(쓰는 쪽은 arboard). 사람의 clipboard를 덮어쓰므로
    /// `AAA_CLIP_NATIVE=1`일 때만 돈다(CI 세 OS, Linux는 Xvfb). 켰는데 clipboard를 열지 못하면 실패다.
    #[test]
    fn native_clipboard_roundtrip() {
        if std::env::var_os("AAA_CLIP_NATIVE").is_none() {
            eprintln!("건너뜀: AAA_CLIP_NATIVE=1일 때만 시스템 clipboard를 쓴다");
            return;
        }
        // 시험 override가 섞이면 실제 경로를 보지 않는다
        assert!(std::env::var_os("AAA_TEST_CLIPBOARD_PNG").is_none());
        let rgba: Vec<u8> = PIXELS.concat();
        let mut c = arboard::Clipboard::new().expect("clipboard를 열지 못했다");
        c.set_image(arboard::ImageData { width: 3, height: 2, bytes: rgba.as_slice().into() }).expect("그림 쓰기");
        // Linux X11은 쓴 쪽(c)이 살아 있어야 다른 연결이 읽는다
        let got = image_png().expect("clipboard 그림을 읽지 못했다");
        assert_eq!(decode(&got), (3, 2, rgba));
        drop(c);
        assert!(std::env::var_os("AAA_TEST_CLIPBOARD_TEXT").is_none());
        // 읽기와 달리 쓰기는 유효한 창 소유자가 필요하다. Windows CI에서도 선택 복사와 같은 경로를 탄다.
        let text = "선택 복사 한글 😀\nsecond line";
        let written = native_text(text);
        if let Err(e) = &written {
            // --nocapture인 CI도 실패 요약 annotation에서 실제 API 오류를 보게 한다.
            eprintln!("error: 선택한 글 쓰기: {e}");
        }
        written.expect("선택한 글 쓰기");
        let got = arboard::Clipboard::new().unwrap().get_text().expect("선택한 글 읽기");
        assert_eq!(got.replace("\r\n", "\n"), text);
    }
}

//! front 기기 clipboard의 그림(§8.28, M3). `aaa attach`에서 Ctrl+V를 누르면 읽는다.
//!
//! - macOS NSPasteboard, Windows clipboard(PNG·CF_DIBV5·CF_DIB), Linux X11을 arboard로 읽는다. 순수 Rust라 빌드에
//!   X11·Wayland 개발 파일이 필요 없다. Linux는 `DISPLAY`가 있는 desktop에서만 된다(Wayland는 XWayland로).
//! - arboard는 RGBA를 돌려주므로 PNG로 다시 만든다. 원래 파일 형식·색 profile은 남지 않는다.
//! - 그림이 없거나 clipboard를 열지 못하면(ssh 셸·desktop 없음) `None`이다. 호출한 쪽이 원래 키(0x16)를 보낸다.
//! - 시험용 `AAA_TEST_CLIPBOARD_PNG=<파일>`: 시스템 clipboard 대신 그 파일을 읽는다(없으면 그림 없음).
//!   개발 Mac에서 사람의 clipboard를 덮어쓰지 않고 E2E를 돌리려고 둔다. 실제 clipboard 경로는 CI가 `AAA_CLIP_NATIVE=1`로 본다.

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

#[cfg(test)]
mod tests {
    use super::*;

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
    }
}

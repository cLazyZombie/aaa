use super::*;

#[test]
fn query_state_keeps_no_raw_scrollback_after_resize() {
    let mut query = QueryState::new(3, 4);
    query.feed(b"abcdef");
    query.resize(3, 8);
    assert_eq!(query.cursor_report(), b"\x1b[1;7R");
    for _ in 0..100 {
        query.feed(b"raw line\r\n");
    }
    query.0.parser.screen_mut().set_scrollback(usize::MAX);
    assert_eq!(query.0.parser.screen().scrollback(), 0);
}

/// `ScreenText.ansi`는 `read`의 Visible·Ansi와 같은 글이다(prompt_box가 흐린 placeholder를 가른다)
#[test]
fn screen_text_ansi_is_the_visible_ansi_read() {
    let mut v = Vt::new(3, 30);
    v.feed("❯ \x1b[2mTry \"fix\"\x1b[0m\r\n\x1b[7mx\x1b[27m \x1b[38;5;135m─\x1b[0m".as_bytes());
    let a = v.text("s").ansi;
    assert_eq!(a, v.read(ReadSource::Visible, None, ReadFormat::Ansi).text);
    assert!(a.contains("\x1b[2m") || a.contains(";2m"), "{a:?}");
    assert_eq!(strip_sgr(&a), "❯ Try \"fix\"\nx ─\n");
}

/// 질의는 출력에서 빠지고 자리와 함께 나온다. chunk 경계에 걸친 질의는 다음 chunk까지 붙들고, 질의가 아닌 escape는 그대로 둔다
#[test]
fn queries_are_taken_out_of_the_output_across_chunks() {
    use Query::*;
    let q = |at: usize, q: Query| (at, Found::Query(q));
    let mut s = QueryScanner::default();
    let got = s.scan(b"ab\x1b[6ncd\x1b[c\x1b[0c\x1b[>c\x1b[>0cef");
    assert_eq!(got.out, b"abcdef");
    let at4 = [PrimaryAttributes, PrimaryAttributes, SecondaryAttributes, SecondaryAttributes].map(|k| q(4, k));
    assert_eq!(got.found, [vec![q(2, CursorPosition)], at4.to_vec()].concat());
    // 끝의 앞부분은 붙들었다가 이어 본다
    assert_eq!(s.scan(b"x\x1b["), Scanned { out: b"x".to_vec(), found: vec![] });
    assert_eq!(s.scan(b">"), Scanned::default());
    assert_eq!(s.scan(b"0cy\x1b"), Scanned { out: b"y".to_vec(), found: vec![q(0, SecondaryAttributes)] });
    assert_eq!(s.take_held(), b"\x1b");
    // 질의가 아닌 것: 다른 인자의 DA·DECXCPR·DSR 5·DA3·kitty 키보드 질의(aaa는 그 키 방식을 쓰지 않는다)·커서 색·팔레트·글자 모양
    let other: &[u8] =
        b"\x1b[1c\x1b[>1c\x1b[?6n\x1b[5n\x1b[=c\x1b[?u\x1b]12;?\x07\x1b]4;1;?\x07\x1b[61n\x1b[31m\x1b\x1b[6";
    let got = s.scan(other);
    assert_eq!(got.out, &other[..other.len() - 3]);
    assert!(got.found.is_empty());
    assert_eq!(s.scan(b"n"), Scanned { out: vec![], found: vec![q(0, CursorPosition)] });
    let mut v = Vt::new(5, 20);
    v.feed(b"hi\r\nthere");
    assert_eq!(v.cursor_report(), b"\x1b[2;6R");
    assert_eq!(PrimaryAttributes.fixed_reply(), Some(&b"\x1b[?62;22c"[..]));
}

/// 기본 색 질의(OSC 10/11 `?`)는 빼고 그 끝을 적는다. 색 지정·되돌리기(110/111·`ESC c`)는 출력에 남기고 그 뒤 자리에 적는다
#[test]
fn color_osc_is_found_across_chunks() {
    use DefaultColor::*;
    use Terminator::*;
    let color = |c, t| Found::Query(Query::DefaultColor(c, t));
    let mut s = QueryScanner::default();
    let kept: &[u8] = b"\x1b]11;#102030\x07d\x1b]10;rgb:ffff/8000/0\x1b\\e\x1b]111\x07\x1bc";
    let got = s.scan(&[b"a\x1b]10;?\x1b\\b\x1b]11;?\x07c".as_slice(), kept].concat());
    assert_eq!(got.out, [b"abc".as_slice(), kept].concat());
    let after = |needle: &[u8]| got.out.windows(needle.len()).position(|w| w == needle).unwrap() + needle.len();
    assert_eq!(
        got.found,
        vec![
            (1, color(Foreground, St)),
            (2, color(Background, Bel)),
            (after(b"#102030\x07"), Found::Set(Background, [0x10, 0x20, 0x30])),
            // 0x8000 * 255 / 0xffff = 127(ghostty도 나머지를 버린다)
            (after(b"/0\x1b\\"), Found::Set(Foreground, [0xff, 0x7f, 0])),
            (after(b"111\x07"), Found::Reset(Background)),
            (got.out.len(), Found::Reset(Foreground)),
            (got.out.len(), Found::Reset(Background)),
        ]
    );
    // 끝나지 않은 OSC는 끝(ST의 ESC 하나까지)이 올 때까지 붙든다
    assert_eq!(s.scan(b"x\x1b]1"), Scanned { out: b"x".to_vec(), found: vec![] });
    assert_eq!(s.scan(b"0;"), Scanned::default());
    assert_eq!(s.scan(b"?\x1b"), Scanned::default());
    assert_eq!(s.scan(b"\\y"), Scanned { out: b"y".to_vec(), found: vec![(0, color(Foreground, St))] });
    // 이어진 질의는 차례로 답한다(xterm `10;?;?`는 10과 11)
    assert_eq!(s.scan(b"\x1b]10;?;?\x07").found, vec![(0, color(Foreground, Bel)), (0, color(Background, Bel))]);
    // 화면이 받는 것: 질의와 지정이 섞임, 12에 닿는 질의, 끝 전에 다른 escape, 읽지 못한 색(사건 없이), 너무 긴 OSC
    let long = [b"\x1b]10;".as_slice(), &[b'x'; COLOR_OSC_MAX], b"\x07"].concat();
    for pass in [b"\x1b]10;?;#000\x07".as_slice(), b"\x1b]11;?;?\x07", b"\x1b]10;?\x1b[31m", b"\x1b]10;red\x07", &long]
    {
        let got = s.scan(pass);
        assert_eq!((got.out.as_slice(), got.found.as_slice()), (pass, &[][..]), "{pass:?}");
    }
    assert!(s.take_held().is_empty());
}

/// 색 질의의 답은 프로그램이 정한 색(질의의 끝으로), 아니면 화면 색(늘 ST)이다. 둘 다 모르면 답하지 않는다
#[test]
fn default_color_replies_prefer_the_program_then_the_screen() {
    use DefaultColor::*;
    use Terminator::*;
    let mut c = DefaultColors::default();
    assert_eq!(c.reply(Foreground, Bel), None);
    c.host = Some(TermColors { fg: [0xd8, 0xd0, 0xc3], bg: [0x18, 0x16, 0x13] });
    assert_eq!(c.reply(Background, Bel).unwrap(), b"\x1b]11;rgb:1818/1616/1313\x1b\\");
    c.note(Found::Set(Background, [0xff, 0, 0x80]));
    assert_eq!(c.reply(Background, Bel).unwrap(), b"\x1b]11;rgb:ffff/0000/8080\x07");
    assert_eq!(c.reply(Background, St).unwrap(), b"\x1b]11;rgb:ffff/0000/8080\x1b\\");
    assert_eq!(c.reply(Foreground, Bel).unwrap(), b"\x1b]10;rgb:d8d8/d0d0/c3c3\x1b\\");
    c.note(Found::Reset(Background));
    assert_eq!(c.reply(Background, Bel).unwrap(), b"\x1b]11;rgb:1818/1616/1313\x1b\\");
    // 색 지정 꼴(ghostty `RGB.parse`의 hex 규칙: 자리 수의 최댓값을 255로)
    for (spec, rgb) in [
        ("#fff", Some([255, 255, 255])),
        ("#123456789abc", Some([0x12, 0x56, 0x9a])),
        ("abc", Some([0xaa, 0xbb, 0xcc])),
        (" #102030\t", Some([0x10, 0x20, 0x30])),
        ("rgb:f/80/000", Some([255, 128, 0])),
        ("rgb:1/2", None),
        ("rgb:1/2/3/4", None),
        ("red", None),
        ("#12345", None),
    ] {
        assert_eq!(parse_color(spec), rgb, "{spec}");
    }
}

#[test]
fn replay_carries_scrollback_into_a_fresh_vt() {
    let mut a = Vt::new(5, 20);
    for i in 1..=30 {
        a.feed(format!("\x1b[1mline-{i}\x1b[m\r\n").as_bytes());
    }
    a.feed(b"tail");
    let mut b = Vt::new(5, 20);
    b.feed(&a.replay());
    assert_eq!(b.text("s").lines, a.text("s").lines);
    // 위로 26줄: 1~26은 기록, 27~30과 tail이 화면
    let mut s = b.parser.screen().clone();
    s.set_scrollback(usize::MAX);
    assert_eq!(s.scrollback(), 26);
    assert_eq!(s.rows(0, 20).next().unwrap(), "line-1");
    assert!(s.cell(0, 0).unwrap().bold());
    s.set_scrollback(1);
    assert_eq!(s.rows(0, 20).next().unwrap(), "line-26");
    // 상한을 넘는 기록은 최근 것만 싣는다
    let mut long = Vt::new(5, 20);
    for i in 1..=REPLAY_HISTORY + 100 {
        long.feed(format!("n{i}\r\n").as_bytes());
    }
    let mut d = Vt::new(5, 20);
    d.feed(&long.replay());
    let mut s = d.parser.screen().clone();
    s.set_scrollback(usize::MAX);
    assert_eq!(s.scrollback(), REPLAY_HISTORY);
    // 화면은 마지막 4줄과 빈 커서 줄이라 기록은 n1~n(N-4): 그 가운데 끝 REPLAY_HISTORY줄
    assert_eq!(s.rows(0, 20).next().unwrap(), format!("n{}", 100 - 4 + 1));
    // 칸마다 색이 바뀌는 넓은 줄이 가득해도 snapshot이 한 frame 안에 든다(base64는 4/3배)
    let mut dense = Vt::new(50, 250);
    for i in 0..REPLAY_HISTORY + 50 {
        let mut line = String::new();
        for c in 0..250 {
            line.push_str(&format!(
                "\x1b[38;2;{};{};{}m{}",
                (i + c) % 256,
                c % 256,
                i % 256,
                char::from(b'a' + (c % 26) as u8)
            ));
        }
        dense.feed(format!("{line}\x1b[m\r\n").as_bytes());
    }
    let replay = dense.replay();
    assert!(replay.len() * 4 / 3 + 4096 < aaa_proto::MAX_FRAME, "replay {} bytes", replay.len());
    let mut e = Vt::new(50, 250);
    e.feed(&replay);
    assert_eq!(e.text("s").lines, dense.text("s").lines);
    let mut s = e.parser.screen().clone();
    s.set_scrollback(usize::MAX);
    assert!(s.scrollback() > 50, "기록 {}줄", s.scrollback());
    // 기록이 없으면 앞에 아무것도 붙이지 않는다
    let mut c = Vt::new(5, 20);
    c.feed(b"x");
    assert!(c.replay().starts_with(b"\x1b[H\x1b[2J"));
}

#[test]
fn replay_keeps_history_scrolled_above_a_fixed_composer() {
    let mut a = Vt::new(6, 30);
    a.feed(b"\x1b[6;1Hfixed-input>\x1b[1;4r\x1b[1;1H");
    for i in 0..30 {
        a.feed(format!("history-{i}\r\n").as_bytes());
    }
    a.feed(b"\x1b[r\x1b[6;13H");
    let text = a.read(ReadSource::Recent, Some(100), ReadFormat::Text);
    assert!(text.text.contains("history-0"), "입력줄 위에서 밀린 기록이 사라졌다: {}", text.text);
    let mut b = Vt::new(6, 30);
    b.feed(&a.replay());
    assert_eq!(b.read(ReadSource::Recent, Some(100), ReadFormat::Text).text, text.text);
    assert!(b.text("s").lines.last().unwrap().contains("fixed-input>"));
}

#[test]
fn interior_regions_and_explicit_scroll_up_do_not_create_history() {
    for scroll in [b"\x1b[2;4r\x1b[4;1H\n".as_slice(), b"\x1b[1;4r\x1b[S", b"\x1b[S"] {
        let mut v = Vt::new(6, 30);
        v.feed(b"top\r\nsecond\r\nthird\r\nfourth\r\nfifth\r\nfixed-input>");
        v.feed(scroll);
        assert_eq!(v.history_len(), 0, "{scroll:?}");
        assert!(v.text("s").lines.last().unwrap().contains("fixed-input>") || scroll == b"\x1b[S");
    }
}

#[test]
fn replay_keeps_alternate_screen_over_normal_screen() {
    let mut a = Vt::new(5, 20);
    for i in 1..=10 {
        a.feed(format!("n{i}\r\n").as_bytes());
    }
    a.feed(b"\x1b[?1049h\x1b[Halt-screen");
    let mut b = Vt::new(5, 20);
    b.feed(&a.replay());
    assert!(b.parser.screen().alternate_screen());
    assert_eq!(b.text("s").lines, a.text("s").lines);
    // 프로그램이 나가면 그 밑 normal screen과 기록이 돌아온다
    a.feed(b"\x1b[?1049l");
    b.feed(b"\x1b[?1049l");
    assert_eq!(b.text("s").lines, a.text("s").lines);
    let mut s = b.parser.screen().clone();
    s.set_scrollback(usize::MAX);
    assert_eq!(s.rows(0, 20).next().unwrap(), "n1");
}

#[test]
fn shrinking_lifts_lines_into_history_and_keeps_the_cursor_line() {
    let mut v = Vt::new(10, 20);
    for i in 1..=9 {
        v.feed(format!("l{i}\r\n").as_bytes());
    }
    v.feed(b"\x1b[1m$ cmd");
    // 커서는 9번째 줄(0부터) 6칸. 5줄로 줄이면 위 5줄이 기록으로 가고 프롬프트 줄이 맨 아래에 남는다
    v.resize(5, 20);
    let lines = v.text("s").lines;
    assert_eq!(lines, vec!["l6", "l7", "l8", "l9", "$ cmd"]);
    assert_eq!(v.cursor(), (4, 5, true));
    let mut s = v.parser.screen().clone();
    s.set_scrollback(usize::MAX);
    assert_eq!(s.scrollback(), 5);
    assert_eq!(s.rows(0, 20).next().unwrap(), "l1");
    // 글자 모양(굵게)도 그대로: 뒤에 쓴 글이 굵다
    v.feed(b"x");
    assert!(v.parser.screen().cell(4, 5).unwrap().bold());
    // 커서가 새 높이 안이면 아래 빈 줄만 줄인다(기록으로 올리지 않는다)
    let mut w = Vt::new(10, 20);
    w.feed(b"top\r\nnext");
    w.resize(5, 20);
    assert_eq!(w.text("s").lines[..2], ["top".to_string(), "next".to_string()]);
    let mut s = w.parser.screen().clone();
    s.set_scrollback(usize::MAX);
    assert_eq!(s.scrollback(), 0);
}

#[test]
fn replay_reproduces_screen_in_a_fresh_vt() {
    let mut a = Vt::new(5, 20);
    a.feed("한글 ok\r\n\x1b[31mred\x1b[0m\r\nthird".as_bytes());
    let mut b = Vt::new(5, 20);
    b.feed(b"garbage that must be cleared");
    b.feed(&a.replay());
    assert_eq!(a.text("s").lines, b.text("s").lines);
    assert_eq!(b.text("s").lines[0], "한글 ok");
    assert_eq!((b.text("s").cursor_row, b.text("s").cursor_col), (2, 5));
}

/// 기록 줄 글(오래된 것부터)과 화면 글
fn all_text(vt: &Vt) -> Vec<String> {
    let mut s = vt.parser.screen().clone();
    let (rows, cols) = s.size();
    s.set_scrollback(usize::MAX);
    let len = s.scrollback();
    let mut out = Vec::new();
    let mut back = len;
    while back > 0 {
        s.set_scrollback(back);
        let take = back.min(usize::from(rows));
        out.extend(s.rows(0, cols).take(take).map(|r| r.trim_end().to_string()));
        back -= take;
    }
    s.set_scrollback(0);
    out.extend(s.rows(0, cols).map(|r| r.trim_end().to_string()));
    out
}

/// 화면 replay도 이어진 줄(넓은 글자를 내린 줄 포함)을 줄바꿈 없이 써서, 받은 터미널이 제 폭으로 넓히면 줄이 이어진다.
/// 늦게 붙은 웹이 다른 기기 크기로 받은 뒤 제 크기를 거는 경우다
#[test]
fn replayed_screen_keeps_joins_for_the_receiver() {
    let mut a = Vt::new(6, 5);
    a.feed("가나다라마 끝\r\nabcdefgh\r\n$ ".as_bytes());
    // xterm은 이음 표시로만 잇는다: 논리 줄 안에 줄바꿈이 없어야 한다(vt100은 글로 알아보므로 아래 비교만으로는 모른다)
    let bytes = String::from_utf8_lossy(&a.replay()).replace("\x1b[m", "");
    assert!(bytes.contains("가나다라마 끝\r\nabcdefgh\r\n$"), "{bytes:?}");
    let mut b = Vt::new(6, 5);
    b.feed(&a.replay());
    assert_eq!(screen_lines(&b), screen_lines(&a));
    assert_eq!(b.cursor(), a.cursor());
    b.resize(6, 20);
    assert_eq!(screen_lines(&b)[..3], ["가나다라마 끝", "abcdefgh", "$"]);
}

#[test]
fn replay_says_whether_older_history_is_left_out() {
    let mut vt = Vt::new(10, 40);
    for i in 1..=50 {
        vt.feed(format!("L{i:04}\r\n").as_bytes());
    }
    assert!(!vt.replay_more().1, "다 실렸다");
    for i in 51..=REPLAY_HISTORY + 100 {
        vt.feed(format!("L{i:04}\r\n").as_bytes());
    }
    assert!(vt.replay_more().1, "앞 기록이 남았다");
}

#[test]
fn older_gives_the_chunk_just_above_the_anchor_until_history_ends() {
    let mut vt = Vt::new(10, 40);
    for i in 1..=5000 {
        vt.feed(format!("\x1b[3{}mL{i:04}\x1b[m\r\n", i % 7).as_bytes());
    }
    let text = all_text(&vt);
    let total = text.len();
    // 웹이 L3001부터 갖고 있다: 그 앞 2000줄은 L1001~L3000
    let at = text.iter().position(|l| l == "L3001").unwrap();
    let anchor = vec!["L3001".to_string(), "L3002".to_string()];
    let o = vt.older(&anchor, total - at, 2000).unwrap();
    assert_eq!((o.lines, o.more), (2000, true));
    // 받은 덩어리를 빈 vt에 쓰면 그 줄들이 같은 글·색으로 기록에 쌓인다
    let mut fresh = Vt::new(10, 40);
    fresh.feed(&o.data);
    let got = all_text(&fresh);
    let got: Vec<_> = got.iter().filter(|l| !l.is_empty()).cloned().collect();
    assert_eq!(got.first().map(String::as_str), Some("L1001"));
    assert_eq!(got.last().map(String::as_str), Some("L3000"));
    assert_eq!(got.len(), 2000);
    assert!(String::from_utf8_lossy(&o.data).contains("\x1b[3"), "색을 싣는다");
    // 그 다음 덩어리는 기록 끝까지: L0001~L1000, 더 없음
    let at = text.iter().position(|l| l == "L1001").unwrap();
    let o = vt.older(&["L1001".to_string()], total - at, 2000).unwrap();
    assert_eq!((o.lines, o.more), (1000, false));
    assert!(String::from_utf8_lossy(&o.data).starts_with("\x1b[31mL0001"), "{:?}", &o.data[..12]);
    // 맨 앞이면 빈 덩어리, 더 없음
    let o = vt.older(&["L0001".to_string()], total, 2000).unwrap();
    assert_eq!((o.lines, o.more), (0, false));
    // 없는 글이면 맞출 수 없다
    assert!(vt.older(&["nope".to_string()], 10, 2000).is_none());
}

#[test]
fn older_picks_the_repeated_anchor_nearest_the_given_depth() {
    let mut vt = Vt::new(5, 20);
    // 같은 글 「same」이 세 번: 각 앞 줄로 어느 것을 골랐는지 본다
    for i in 1..=300 {
        let l = if i % 100 == 0 { "same".to_string() } else { format!("x{i}") };
        vt.feed(format!("{l}\r\n").as_bytes());
    }
    let text = all_text(&vt);
    let total = text.len();
    for want in [100, 200, 300] {
        let at = text.iter().position(|l| *l == format!("x{}", want - 1)).unwrap() + 1;
        let o = vt.older(&["same".to_string()], total - at, 1).unwrap();
        assert_eq!(String::from_utf8_lossy(&o.data).trim_end_matches("\x1b[m\r\n"), format!("x{}", want - 1));
    }
}

#[test]
fn older_is_none_on_the_alternate_screen() {
    let mut vt = Vt::new(5, 20);
    for i in 1..=50 {
        vt.feed(format!("L{i}\r\n").as_bytes());
    }
    vt.feed(b"\x1b[?1049h");
    assert!(vt.older(&["L40".to_string()], 15, 10).is_none());
}

#[test]
fn replay_restores_input_modes_in_a_fresh_vt() {
    // 다시 붙은 화면도 화살표 모드(DECCKM)·bracketed paste·mouse 보고를 안다(차이 목록 「DECCKM」 재연결 뒤)
    for alt in [false, true] {
        let mut a = Vt::new(5, 20);
        if alt {
            a.feed(b"[?1049h");
        }
        a.feed(b"[?1h[?2004h[?1000h[?1006h");
        let mut b = Vt::new(5, 20);
        b.feed(&a.replay());
        let s = b.parser.screen();
        assert!(s.application_cursor() && s.bracketed_paste(), "alt={alt}");
        assert_eq!(s.mouse_protocol_mode(), vt100::MouseProtocolMode::PressRelease, "alt={alt}");
        assert_eq!(s.mouse_protocol_encoding(), vt100::MouseProtocolEncoding::Sgr, "alt={alt}");
        assert_eq!(s.alternate_screen(), alt);
        // 끄면 끈 채로
        a.feed(b"[?1l");
        let mut c = Vt::new(5, 20);
        c.feed(&a.replay());
        assert!(!c.parser.screen().application_cursor(), "alt={alt}");
    }
}

#[test]
fn replay_says_modify_other_keys_again() {
    let mut v = Vt::new(5, 20);
    assert!(!v.replay().windows(4).any(|w| w == b"\x1b[>4"));
    v.feed(b"\x1b[>4;2m");
    assert!(v.replay().ends_with(b"\x1b[>4;2m"));
    // 다른 자원(1번)은 단계를 바꾸지 않는다
    v.feed(b"\x1b[>1;1m");
    assert!(v.replay().ends_with(b"\x1b[>4;2m"));
    v.feed(b"\x1b[>4n");
    assert!(!v.replay().windows(4).any(|w| w == b"\x1b[>4"));
    v.feed(b"\x1b[>4;1m\x1b[>m");
    assert!(!v.replay().windows(4).any(|w| w == b"\x1b[>4"));
}

fn read(v: &mut Vt, source: ReadSource, lines: Option<u32>) -> ReadText {
    v.read(source, lines, ReadFormat::Text)
}

#[test]
fn read_follows_herdr_sources() {
    let mut v = Vt::new(5, 10);
    for i in 1..=20 {
        v.feed(format!("l{i}\r\n").as_bytes());
    }
    v.feed(b"$ ");
    // Visible: 보이는 줄(끝 빈 줄 없음)
    assert_eq!(read(&mut v, ReadSource::Visible, None).text, "l17\nl18\nl19\nl20\n$\n");
    assert_eq!(read(&mut v, ReadSource::Visible, Some(2)), ReadText { text: "l20\n$\n".into(), truncated: true });
    // Recent: 기록까지 이어 마지막 줄들, 앞에 더 있으면 truncated
    let r = read(&mut v, ReadSource::Recent, Some(3));
    assert_eq!(r, ReadText { text: "l19\nl20\n$\n".into(), truncated: true });
    let all = read(&mut v, ReadSource::Recent, None);
    assert!(all.text.starts_with("l1\nl2\n") && all.text.ends_with("l20\n$\n"), "{}", all.text);
    assert!(!all.truncated);
    // 읽은 뒤 scrollback 위치는 그대로(화면이 기록으로 밀리지 않는다)
    assert_eq!(read(&mut v, ReadSource::Visible, None).text, "l17\nl18\nl19\nl20\n$\n");
    // 커서가 마지막 글 줄보다 아래면 범위는 커서 줄까지다(herdr와 같다). 그 끝 빈 줄은 글에서 뺀다.
    let mut c = Vt::new(6, 10);
    c.feed(b"a\r\nb\r\n\r\n");
    assert_eq!(read(&mut c, ReadSource::Recent, Some(2)).text, "");
    assert_eq!(read(&mut c, ReadSource::Recent, Some(3)).text, "b\n");
    assert_eq!(read(&mut c, ReadSource::Recent, Some(10)).text, "a\nb\n");
}

#[test]
fn recent_unwrapped_joins_soft_wrapped_rows() {
    let mut v = Vt::new(4, 5);
    v.feed(b"abcdefgh\r\nxy");
    assert_eq!(read(&mut v, ReadSource::Recent, None).text, "abcde\nfgh\nxy\n");
    assert_eq!(read(&mut v, ReadSource::RecentUnwrapped, None).text, "abcdefgh\nxy\n");
    // 실제 출력 공백이 접히는 경계에 있어도 지우지 않는다(경로의 공백 포함).
    for rows in [2, 4] {
        let mut spaced = Vt::new(rows, 5);
        spaced.feed(b"abcd efgh\r\nx");
        assert_eq!(read(&mut spaced, ReadSource::RecentUnwrapped, None).text, "abcd efgh\nx\n");
        assert_eq!(read(&mut spaced, ReadSource::Recent, None).text, "abcd\nefgh\nx\n");
    }
    // 접힌 줄이 기록과 화면에 걸쳐도 잇는다
    let mut w = Vt::new(2, 5);
    w.feed(b"0123456789AB\r\nz");
    assert_eq!(read(&mut w, ReadSource::RecentUnwrapped, None).text, "0123456789AB\nz\n");
}

/// 기록 편집기: 읽기 상한(`READ_MAX_LINES`)과 상관없이 기록 처음부터, 접힌 줄은 잇는다
#[test]
fn unwrapped_all_reads_the_whole_history() {
    let mut v = Vt::new(5, 10);
    for i in 1..=READ_MAX_LINES + 500 {
        v.feed(format!("l{i}\r\n").as_bytes());
    }
    v.feed(b"0123456789AB\r\n$ ");
    let text = v.unwrapped_all();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), READ_MAX_LINES + 502);
    assert_eq!((lines[0], lines[lines.len() - 2], lines[lines.len() - 1]), ("l1", "0123456789AB", "$"));
}

/// 넓은 글자가 끝 칸 하나에 들지 않아 내려간 줄도 이어 읽는다(ghostty는 그 칸을 spacer로 두고 잇는다)
#[test]
fn recent_unwrapped_joins_rows_split_before_a_wide_char() {
    let mut v = Vt::new(4, 5);
    v.feed("가나다라마 끝\r\n".as_bytes());
    assert_eq!(read(&mut v, ReadSource::Recent, None).text, "가나\n다라\n마 끝\n");
    assert_eq!(read(&mut v, ReadSource::RecentUnwrapped, None).text, "가나다라마 끝\n");
    // 기록과 화면에 걸쳐도
    let mut h = Vt::new(2, 5);
    h.feed("가나다라마바사\r\n$ ".as_bytes());
    assert_eq!(read(&mut h, ReadSource::RecentUnwrapped, None).text, "가나다라마바사\n$\n");
}

#[test]
fn read_alternate_screen_and_ansi() {
    let mut v = Vt::new(3, 10);
    v.feed(b"under\r\n\x1b[?1049h\x1b[Htop\r\n\x1b[31mred\x1b[m");
    assert_eq!(read(&mut v, ReadSource::Recent, None).text, "top\nred\n");
    let a = v.read(ReadSource::Visible, None, ReadFormat::Ansi).text;
    assert!(a.contains("\x1b[31mred") && a.contains("top"), "{a:?}");
    assert!(a.lines().all(|l| l.ends_with("\x1b[m")), "{a:?}");
}

/// herdr `read_terminal_snapshot` Detection: 감지 글 그대로(보이는 화면도 기록 전부도 아니다), `lines`면 마지막 줄들,
/// ansi여도 모양 없는 글
#[test]
fn read_detection_is_the_detection_text() {
    let mut v = Vt::new(4, 20);
    for i in 1..=6 {
        v.feed(format!("l{i}\r\n").as_bytes());
    }
    v.feed(b"\x1b[2J\x1b[H\x1b[31ma\x1b[m\r\nb");
    let all = ReadText { text: "l2\nl3\na\nb\n".into(), truncated: false };
    assert_eq!(read(&mut v, ReadSource::Detection, None), all);
    assert_eq!(read(&mut v, ReadSource::Detection, Some(1)), ReadText { text: "b\n".into(), truncated: true });
    assert_eq!(v.read(ReadSource::Detection, None, ReadFormat::Ansi), all);
}

#[test]
fn key_names_follow_herdr_grammar_and_modes() {
    let plain = KeyModes::default();
    let k = |m: &KeyModes, n: &str| m.key(n);
    assert_eq!(k(&plain, "enter").unwrap(), b"\r");
    assert_eq!(k(&plain, "Return").unwrap(), b"\r");
    assert_eq!(k(&plain, "esc").unwrap(), [0x1b]);
    assert_eq!(k(&plain, "ctrl+c").unwrap(), [3]);
    assert_eq!(k(&plain, "C-c").unwrap(), [3]);
    assert_eq!(k(&plain, "Ctrl+C").unwrap(), [3]);
    assert_eq!(k(&plain, "shift+tab").unwrap(), b"\x1b[Z");
    assert_eq!(k(&plain, "tab").unwrap(), b"\t");
    assert_eq!(k(&plain, "up").unwrap(), b"\x1b[A");
    assert_eq!(k(&plain, "ctrl+left").unwrap(), b"\x1b[1;5D");
    assert_eq!(k(&plain, "alt+x").unwrap(), b"\x1bx");
    assert_eq!(k(&plain, "A").unwrap(), b"A");
    assert_eq!(k(&plain, "y").unwrap(), b"y");
    assert_eq!(k(&plain, "1").unwrap(), b"1");
    assert_eq!(k(&plain, "space").unwrap(), b" ");
    assert_eq!(k(&plain, "+").unwrap(), b"+");
    assert_eq!(k(&plain, "ctrl++").unwrap(), b"+");
    assert_eq!(k(&plain, "backspace").unwrap(), [0x7f]);
    assert_eq!(k(&plain, "f1").unwrap(), b"\x1bOP");
    assert_eq!(k(&plain, "f5").unwrap(), b"\x1b[15~");
    assert_eq!(k(&plain, "shift+f5").unwrap(), b"\x1b[15;2~");
    assert_eq!(k(&plain, "shift+enter").unwrap(), b"\r");
    for bad in ["", "ctrl+", "nope", "ctrl+a+b", "f0", "f21"] {
        assert!(k(&plain, bad).is_none(), "{bad}");
    }
    // 프로그램이 켠 모드를 따른다
    let app = KeyModes { application_cursor: true, bracketed_paste: true, modify_other_keys: 2 };
    assert_eq!(k(&app, "up").unwrap(), b"\x1bOA");
    assert_eq!(k(&app, "ctrl+up").unwrap(), b"\x1b[1;5A");
    assert_eq!(k(&app, "ctrl+enter").unwrap(), b"\x1b[27;5;13~");
    assert_eq!(k(&app, "shift+enter").unwrap(), b"\x1b[27;2;13~");
    assert_eq!(app.paste("hi"), b"\x1b[200~hi\x1b[201~");
    assert_eq!(plain.paste("hi"), b"hi");
    // 글 안의 ESC·C1 CSI는 빠진다: 붙여넣기를 일찍 끝내지 못한다
    assert_eq!(app.paste("a\x1b[201~rm x\r\u{9b}201~b"), "\x1b[200~a[201~rm x\r201~b\x1b[201~".as_bytes());
    assert_eq!(plain.paste("a\x1bb"), b"ab");
}

#[test]
fn modes_come_from_the_program() {
    let mut v = Vt::new(3, 10);
    assert_eq!(v.modes(), KeyModes::default());
    v.feed(b"\x1b[?1h\x1b[?2004h\x1b[>4;2m");
    assert_eq!(v.modes(), KeyModes { application_cursor: true, bracketed_paste: true, modify_other_keys: 2 });
}

/// herdr pane/osc.rs `AgentOscStateTracker`: 마지막 OSC 0/2 제목·OSC 9 진행. `;`가 든 제목, 빈 제목(지움), 제어 문자,
/// 길이 상한, ST·BEL 끝, 나뉘어 온 bytes
#[test]
fn osc_title_and_progress_are_kept_for_detection() {
    let mut v = Vt::new(5, 40);
    assert_eq!((v.osc_title(), v.osc_progress()), ("", ""));
    v.feed("\x1b]0;⠋ codex · demo\x07".as_bytes());
    assert_eq!(v.osc_title(), "⠋ codex · demo");
    v.feed(b"\x1b]2;a;b;c\x1b\\");
    assert_eq!(v.osc_title(), "a;b;c");
    v.feed("\x1b]0;✳ x\x1b".as_bytes());
    v.feed(b"\\");
    assert_eq!(v.osc_title(), "✳ x");
    v.feed(b"\x1b]9;4;3;\x07");
    assert_eq!(v.osc_progress(), "4;3;");
    v.feed(b"\x1b]9;4;0;\x07\x1b]0;\x07");
    assert_eq!((v.osc_title(), v.osc_progress()), ("", "4;0;"));
    // 아이콘 이름(OSC 1)·다른 OSC는 제목이 아니다
    v.feed(b"\x1b]1;icon\x07\x1b]7;file:///tmp\x07");
    assert_eq!(v.osc_title(), "");
    let long = format!("\x1b]0;{}\x07", "가".repeat(300));
    v.feed(long.as_bytes());
    assert_eq!(v.osc_title().chars().count(), AGENT_OSC_MAX_CHARS);
    v.feed(b"\x1b]0;a\tb\x07");
    assert_eq!(v.osc_title(), "ab");
    // 화면 글에는 남지 않는다
    assert!(v.text("s").lines.iter().all(|l| l.is_empty()));
}

/// herdr pane/terminal.rs `ghostty_recent_read_range`: 끝은 마지막 내용 줄과 커서 줄 가운데 아래쪽, 덜 찬 화면은 위를
/// 기록 줄로 채우고, 끝의 빈 줄은 뺀다. alternate screen은 보이는 화면만.
#[test]
fn detection_text_ends_at_content_or_cursor_and_reaches_into_scrollback() {
    let mut v = Vt::new(4, 20);
    assert_eq!(v.detection_text(), "");
    for i in 1..=6 {
        v.feed(format!("l{i}\r\n").as_bytes());
    }
    v.feed(b"tail   ");
    // 화면: l4 l5 l6 tail, 기록: l1 l2 l3
    assert_eq!(v.detection_text(), "l4\nl5\nl6\ntail\n");
    // 화면을 지우고(지운 줄은 기록으로 가지 않는다) 위쪽 두 줄만 그리면 그 위는 기록 끝 두 줄이다(herdr는 rows줄을 읽는다)
    v.feed(b"\x1b[2J\x1b[Ha\r\nb");
    assert_eq!(v.detection_text(), "l2\nl3\na\nb\n");
    // 커서가 내용보다 아래면 커서 줄까지(빈 줄은 끝에서 뺀다)
    v.feed(b"\x1b[4;1H");
    assert_eq!(v.detection_text(), "a\nb\n");
    // 기록이 모자라면 있는 만큼만
    let mut short = Vt::new(5, 20);
    short.feed(b"x\r\ny");
    assert_eq!(short.detection_text(), "x\ny\n");
    // 읽은 뒤 화면은 그대로다(기록 위치를 되돌린다)
    assert_eq!(v.text("s").lines, vec!["a", "b", "", ""]);
    v.feed(b"\x1b[?1049h\x1b[Hfull\x1b[3;1Hlast");
    assert_eq!(v.detection_text(), "full\n\nlast\n");
}

/// 보이는 줄(끝 공백 뺌)
fn screen_lines(v: &Vt) -> Vec<String> {
    v.text("s").lines
}

/// 기록과 화면의 줄, 끝 빈 줄 뺌
fn all_lines(v: &Vt) -> Vec<String> {
    let mut out = all_text(v);
    while out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    out
}

// 폭 reflow(coordinator 10-18, t-0330 발견): vt100 `set_size`는 폭이 줄면 줄 끝을 버렸다. herdr(ghostty)처럼 다시 접는다
#[test]
fn shrink_then_widen_keeps_line_ends() {
    let mut v = Vt::new(5, 20);
    v.feed(b"0123456789abcdefghij\r\n$ ");
    v.resize(5, 8);
    assert_eq!(screen_lines(&v), ["01234567", "89abcdef", "ghij", "$", ""]);
    assert_eq!(v.cursor(), (3, 2, true));
    v.resize(5, 20);
    assert_eq!(screen_lines(&v), ["0123456789abcdefghij", "$", "", "", ""]);
    assert_eq!(v.cursor(), (1, 2, true));
    // 다시 접은 화면을 replay해도 같다(새로 붙는 front)
    let mut fresh = Vt::new(5, 20);
    fresh.feed(&v.replay());
    assert_eq!(screen_lines(&fresh), screen_lines(&v));
}

/// 휴대폰은 폭과 줄 수를 함께 줄인다: 폭을 먼저 다시 접고 줄 수를 바꾼다. 아래 빈 줄을 먼저 버리고, 그래도 커서 줄이 밖이면
/// 위 줄을 기록으로 올린다(xterm `Buffer.resize`). 빈 줄을 높이 12로 세어 글이 화면 밖(아래)으로 밀리지 않는다
#[test]
fn rows_and_cols_shrink_together() {
    let mut v = Vt::new(12, 20);
    v.feed(b"$ echo 0123456789abcdefgh\r\n0123456789abcdefgh\r\n$ ");
    v.resize(10, 8);
    assert_eq!(screen_lines(&v)[..8], ["$ echo 0", "12345678", "9abcdefg", "h", "01234567", "89abcdef", "gh", "$"]);
    assert_eq!(v.cursor(), (7, 2, true));
    v.resize(5, 8);
    assert_eq!(screen_lines(&v), ["h", "01234567", "89abcdef", "gh", "$"]);
    assert_eq!(all_lines(&v)[..3], ["$ echo 0", "12345678", "9abcdefg"]);
    assert_eq!(v.cursor(), (4, 2, true));
    v.resize(12, 20);
    assert_eq!(screen_lines(&v)[..4], ["$ echo 0123456789abc", "defgh", "0123456789abcdefgh", "$"]);
    assert_eq!(v.cursor(), (3, 2, true));
}

/// 줄 수만 바뀌어도(휴대폰 키보드) 접힌 줄은 이어진 채라 나중에 넓히면 펴진다. vt100 `set_size`는 보이는 줄의 이음 표시를 지웠다
#[test]
fn rows_only_resize_keeps_soft_wraps() {
    let mut v = Vt::new(6, 10);
    v.feed(b"abcdefghijklmnopqrstuvwxy\r\n$ ");
    v.resize(4, 10);
    v.resize(6, 10);
    v.resize(6, 30);
    assert_eq!(screen_lines(&v)[..2], ["abcdefghijklmnopqrstuvwxy", "$"]);
    assert_eq!(v.cursor(), (1, 2, true));
}

/// alternate screen(vim·less)이 떠 있는 동안 줄 수만 바뀌어도 그 밑 normal screen의 접힌 줄은 이어진 채다(vt100 `set_size`는 두
/// 화면의 보이는 줄 이음 표시를 다 지운다)
#[test]
fn rows_only_resize_under_alternate_screen_keeps_soft_wraps() {
    let mut v = Vt::new(6, 10);
    v.feed(b"abcdefghijklmnopqrstuvwxy\r\n$ \x1b[?1049h\x1b[HALT");
    v.resize(4, 10);
    v.resize(6, 10);
    assert_eq!(screen_lines(&v)[0], "ALT");
    v.feed(b"\x1b[?1049l");
    v.resize(6, 30);
    assert_eq!(screen_lines(&v)[..2], ["abcdefghijklmnopqrstuvwxy", "$"]);
}

/// 줄이 줄면 커서 아래 행은 버린다(xterm·vt100과 같다). 커서 줄 안에서도 커서 행 아래는 버린다
#[test]
fn rows_below_cursor_are_dropped_when_rows_shrink() {
    let mut v = Vt::new(6, 20);
    v.feed(b"top\r\ninput> abcdefghijk\r\nstatus bar here\x1b[2;3H");
    v.resize(2, 10);
    assert_eq!(all_lines(&v), ["top", "input> abc"]);
    assert_eq!(v.cursor(), (1, 2, true));
}

#[test]
fn soft_wrapped_line_unwraps_when_widened() {
    let mut v = Vt::new(4, 10);
    v.feed(b"abcdefghijklmnopqrstuvwxy\r\n");
    v.resize(4, 30);
    assert_eq!(screen_lines(&v), ["abcdefghijklmnopqrstuvwxy", "", "", ""]);
}

#[test]
fn history_rewraps_too() {
    let mut v = Vt::new(3, 12);
    for i in 0..10 {
        v.feed(format!("line-{i}-abcdefg\r\n").as_bytes());
    }
    let before = all_lines(&v);
    v.resize(3, 6);
    v.resize(3, 12);
    assert_eq!(all_lines(&v), before);
    v.resize(3, 40);
    let wide: Vec<String> = (0..10).map(|i| format!("line-{i}-abcdefg")).collect();
    assert_eq!(all_lines(&v), wide);
}

#[test]
fn colors_survive_reflow() {
    let mut v = Vt::new(3, 10);
    v.feed(b"\x1b[31mred\x1b[0m plain \x1b[1;44mbold-on-blue\x1b[0m\r\n");
    v.resize(3, 5);
    v.resize(3, 40);
    assert_eq!(screen_lines(&v)[0], "red plain bold-on-blue");
    let s = v.parser.screen();
    assert_eq!(s.cell(0, 0).unwrap().fgcolor(), vt100::Color::Idx(1));
    assert_eq!(s.cell(0, 4).unwrap().fgcolor(), vt100::Color::Default);
    let b = s.cell(0, 10).unwrap();
    assert!(b.bold());
    assert_eq!(b.bgcolor(), vt100::Color::Idx(4));
}

#[test]
fn wide_chars_rejoin() {
    let mut v = Vt::new(4, 20);
    v.feed("가나다라마바사 끝\r\n".as_bytes());
    // 5칸이면 넓은 글자 둘 뒤 끝 칸 하나가 남아 셋째 글자는 이음 표시 없이 다음 줄로 간다(vt100 `col_wrap`)
    v.resize(4, 5);
    assert_eq!(all_lines(&v), ["가나", "다라", "마바", "사 끝"]);
    v.resize(4, 20);
    assert_eq!(screen_lines(&v)[0], "가나다라마바사 끝");
}

#[test]
fn prompt_after_clear_stays_on_top() {
    let mut v = Vt::new(5, 20);
    for i in 0..30 {
        v.feed(format!("old-{i:02}-xxxxxxxxxxxx\r\n").as_bytes());
    }
    v.feed(b"\x1b[H\x1b[2J$ ");
    v.resize(5, 10);
    assert_eq!(screen_lines(&v), ["$", "", "", "", ""]);
    assert_eq!(v.cursor(), (0, 2, true));
}

#[test]
fn cursor_in_the_middle_of_a_line() {
    let mut v = Vt::new(4, 20);
    v.feed(b"abcdefghij");
    v.resize(4, 4);
    assert_eq!(screen_lines(&v), ["abcd", "efgh", "ij", ""]);
    assert_eq!(v.cursor(), (2, 2, true));
    v.feed(b"K");
    v.resize(4, 20);
    assert_eq!(screen_lines(&v)[0], "abcdefghijK");
}

#[test]
fn content_below_cursor_stays_below() {
    let mut v = Vt::new(6, 20);
    v.feed(b"top\r\ninput> abcdefghijk\r\nstatus bar here\x1b[2A\x1b[8G\x1b[B");
    assert_eq!(v.cursor(), (1, 7, true));
    v.resize(6, 10);
    v.feed(b"Z");
    v.resize(6, 20);
    assert_eq!(screen_lines(&v)[..3], ["top", "input> Zbcdefghijk", "status bar here"]);
}

#[test]
fn alternate_screen_keeps_normal_underneath() {
    let mut v = Vt::new(4, 20);
    v.feed(b"0123456789abcdefghij\r\n$ ");
    v.feed(b"\x1b[?1049h\x1b[HALT");
    v.resize(4, 8);
    assert!(v.parser.screen().alternate_screen());
    assert_eq!(screen_lines(&v)[0], "ALT");
    assert_eq!(v.cursor(), (0, 3, true));
    v.resize(4, 20);
    assert_eq!(v.cursor(), (0, 3, true));
    v.feed(b"\x1b[?1049l");
    assert_eq!(screen_lines(&v)[..2], ["0123456789abcdefghij", "$"]);
    assert_eq!(v.cursor(), (1, 2, true));
}

#[test]
fn input_modes_survive_reflow() {
    let mut v = Vt::new(4, 20);
    v.feed(b"0123456789abcdefghij\r\n\x1b[?2004h\x1b[?1h\x1b[?1000h\x1b[?1006h\x1b[>4;2m\x1b[?25l");
    v.resize(4, 8);
    let s = v.parser.screen();
    assert!(s.bracketed_paste());
    assert!(s.application_cursor());
    assert!(s.hide_cursor());
    assert_eq!(s.mouse_protocol_mode(), vt100::MouseProtocolMode::PressRelease);
    assert_eq!(s.mouse_protocol_encoding(), vt100::MouseProtocolEncoding::Sgr);
    assert_eq!(v.modes().modify_other_keys, 2);
}

#[test]
fn blank_lines_between_stay() {
    let mut v = Vt::new(6, 20);
    v.feed(b"one\r\n\r\n\r\nfour-abcdefghijklmnop\r\n$ ");
    v.resize(6, 10);
    v.resize(6, 20);
    // 넓히면 기록에 갔던 "one"이 다시 화면에 내려온다(21자 줄은 20칸에서 두 줄)
    assert_eq!(screen_lines(&v), ["one", "", "", "four-abcdefghijklmno", "p", "$"]);
    assert_eq!(v.cursor(), (5, 2, true));
}

#[test]
fn no_change_when_nothing_wraps() {
    let mut v = Vt::new(4, 20);
    v.feed(b"short\r\n$ ");
    v.resize(4, 10);
    assert_eq!(screen_lines(&v)[..2], ["short", "$"]);
    assert_eq!(v.cursor(), (1, 2, true));
}

/// 기록 replay는 접힌 줄을 줄바꿈 없이 이어 써서, 받은 터미널도 이음 표시를 갖고 폭이 바뀌면 다시 접는다(xterm도 같다)
#[test]
fn replayed_history_keeps_soft_wraps() {
    let mut a = Vt::new(3, 10);
    let lines: Vec<String> =
        (0..6).map(|i| format!("w{i}-abcdefghijklmnopq")).chain(["가나다라마바사아자".to_string()]).collect();
    for l in &lines {
        a.feed(format!("{l}\r\n").as_bytes());
    }
    // 화면 경계에 걸친 줄이 없게 빈 줄을 화면만큼
    a.feed(b"\r\n\r\n");
    let mut b = Vt::new(3, 10);
    b.feed(&a.replay());
    assert_eq!(all_lines(&b), all_lines(&a));
    b.resize(3, 40);
    assert_eq!(all_lines(&b), lines);
    // 위로 굴려 받는 앞 기록도 같다: 셋째 행(w1 첫 행) 앞은 20자 줄 w0의 두 행
    let anchor = vec![all_text(&a)[2].clone()];
    let o = a.older(&anchor, all_text(&a).len() - 2, 10).unwrap();
    assert_eq!(o.lines, 2);
    let mut c = Vt::new(3, 10);
    c.feed(&o.data);
    c.resize(3, 40);
    assert_eq!(all_lines(&c), [lines[0].clone()]);
}

/// 보이는 줄 r의 묶음 칸: (시작 칸, 폭, 글). 빈칸 글자 칸은 빈 칸으로 본다(replay는 줄 가운데 빈 칸을 빈칸으로 쓴다)
fn clusters(s: &vt100::Screen, r: u16) -> Vec<(u16, u16, String)> {
    let (_, cols) = s.size();
    (0..cols)
        .filter_map(|c| {
            let cell = s.cell(r, c).filter(|x| x.has_contents() && x.contents() != " ")?;
            Some((c, if cell.is_wide() { 2 } else { 1 }, cell.contents().to_string()))
        })
        .collect()
}

fn cl(want: &[(u16, u16, &str)]) -> Vec<(u16, u16, String)> {
    want.iter().map(|&(c, w, s)| (c, w, s.to_string())).collect()
}

/// 줄 하나의 묶음 칸과 이음 표시
type Clustered = (Vec<(u16, u16, String)>, bool);

/// 기록과 화면의 줄마다(오래된 줄부터) 묶음 칸과 이음 표시
fn all_clusters(v: &Vt) -> Vec<Clustered> {
    let mut s = v.parser.screen().clone();
    let mut out = Vec::new();
    each_row(&mut s, |s, r| {
        out.push((clusters(s, r), s.row_wrapped(r)));
        true
    });
    out
}

/// 글자 폭과 묶음은 웹 터미널(xterm + grapheme addon + herdr 보정)과 같다(plan §8.167). 사례의 칸은 생성기가 웹
/// provider로 한 줄에 써서 만든다(web/scripts/vt-widths.ts). 한 byte씩 받아도 같다(xterm도 받은 덩이 경계에서 끊지 않는다)
#[test]
fn widths_follow_the_web_terminal() {
    for (text, want) in width_cases::CASES {
        let want = cl(want);
        let end = want.last().map_or(0, |&(c, w, _)| c + w);
        let mut v = Vt::new(3, 40);
        v.feed(text.as_bytes());
        assert_eq!(clusters(v.parser.screen(), 0), want, "{text:?}");
        assert_eq!(v.cursor(), (0, end, true), "{text:?}");
        let mut bytes = Vt::new(3, 40);
        for b in text.as_bytes() {
            bytes.feed(&[*b]);
        }
        assert_eq!(clusters(bytes.parser.screen(), 0), want, "한 byte씩 {text:?}");
    }
}

/// 끝 칸의 한 칸 묶음이 붙은 글자로 두 칸이 되면(⚠ 뒤 VS16) xterm처럼 그 묶음을 이은 줄로 다음 줄 처음에 옮기고 끝 칸은 비운다
#[test]
fn widened_cluster_at_the_last_column_moves_to_the_next_row() {
    let mut v = Vt::new(3, 5);
    v.feed("abcd\u{26a0}\u{fe0f}x".as_bytes());
    let s = v.parser.screen();
    assert_eq!(clusters(s, 0), cl(&[(0, 1, "a"), (1, 1, "b"), (2, 1, "c"), (3, 1, "d")]));
    assert!(s.row_wrapped(0));
    assert_eq!(clusters(s, 1), cl(&[(0, 2, "\u{26a0}\u{fe0f}"), (2, 1, "x")]));
    assert_eq!(v.cursor(), (1, 3, true));
}

/// replay를 받은 터미널의 칸은 살아 있는 칸과 같다. escape로 따로 놓인 칸은 모양이 같아도 이어 쓰면 받는 쪽에서 한 묶음이
/// 되므로 끊어 쓴다: 기록·화면·alternate screen, 접힌 줄을 넘어서도
#[test]
fn replay_keeps_clusters_as_they_were_placed() {
    let lines = [
        // 모양을 바꾸지 않는 SGR로 끊긴 결합 문자, 같은 색을 다시 건 SGR
        "e\x1b[m\u{301}x",
        "\x1b[31me\x1b[31m\u{301}x\x1b[m",
        // 국기 반쪽 둘, VS16 앞 escape
        "\u{1f1f0}\x1b[m\u{1f1f7}",
        "\u{26a0}\x1b[m\u{fe0f}",
        // 커서를 옮긴 빈 칸 뒤 결합 문자(빈 칸은 빈칸 글자로 쓴다)
        "a\x1b[2C\u{301}b",
        // 끊기지 않은 묶음은 그대로 한 칸이다
        "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}\u{26a0}\u{fe0f}e\u{301}",
    ];
    let text: String = lines.iter().map(|l| format!("{l}\r\n")).collect();
    let check = |rows: u16, cols: u16, alt: bool, input: &str| {
        let mut a = Vt::new(rows, cols);
        if alt {
            a.feed(b"\x1b[?1049h");
        }
        a.feed(input.as_bytes());
        let mut b = Vt::new(rows, cols);
        b.feed(&a.replay());
        assert_eq!(all_clusters(&b), all_clusters(&a), "alt {alt} {input:?}");
        assert_eq!(b.cursor(), a.cursor(), "alt {alt} {input:?}");
    };
    // 셋째 줄 뒤로는 기록으로 밀린다
    check(3, 12, false, &text);
    check(8, 12, true, &text);
    // 끝 칸 뒤 escape로 끊겨 다음 줄로 접힌 결합 문자
    for alt in [false, true] {
        check(4, 5, alt, "abcde\x1b[m\u{301}");
    }
}

/// 폭이 바뀌어 다시 접어도 묶음과 따로 놓인 칸이 그대로다(다시 쓰는 줄도 끊어 쓴다)
#[test]
fn reflow_keeps_clusters_as_they_were_placed() {
    let mut v = Vt::new(3, 12);
    v.feed("a\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}e\x1b[m\u{301}\u{1f1f0}\x1b[m\u{1f1f7}x\r\n".as_bytes());
    let before = all_clusters(&v);
    v.resize(3, 4);
    assert_eq!(
        all_clusters(&v)[..2],
        [
            (cl(&[(0, 1, "a"), (1, 2, "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}"), (3, 1, "e")]), true),
            (cl(&[(0, 1, "\u{301}"), (1, 1, "\u{1f1f0}"), (2, 1, "\u{1f1f7}"), (3, 1, "x")]), false),
        ]
    );
    v.resize(3, 12);
    assert_eq!(all_clusters(&v), before);
}

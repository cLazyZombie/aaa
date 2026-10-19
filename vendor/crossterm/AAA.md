# aaa 입력 수리

crates.io crossterm 0.29.0(MIT)의 `src`, 정규화된 Cargo.toml, README, LICENSE다.
사용하지 않는 example target 목록을 빼고 줄 끝을 LF로 맞췄다. 원래 라이선스와 저작권은 LICENSE에 보존한다.

변경점:

- Unix `event/sys/unix/parse.rs`: xterm modifyOtherKeys `CSI 27;modifier;Unicode~`를 키로 읽는다.
- Windows `event/source/windows_paste.rs`·`source/windows.rs`: 콘솔 key record의 bracketed paste를
  하나의 Paste로 복원한다. 빈 paste도 전달하고 UTF-16·key-up·Escape fallback·16Mi자 상한을 처리한다.
  참고: herdr 0.9.5 `src/client/input/windows_vti.rs`의 Windows 콘솔 붙여넣기 복원 경계.
- Windows `event/sys/windows/parse.rs`: virtual-key=0인 합성 ESC·Enter·Tab도 해석한다.
- `terminal/sys/unix.rs`: 현행 Rust가 경고하는 불필요한 괄호 하나를 제거했다.

시험: `cargo test --manifest-path vendor/crossterm/Cargo.toml --locked --lib aaa_`.
공식 `scripts/ci-local.sh`의 test 단계에서 세 OS 모두 실행한다. 타이머를 자지 않고 주어진 시계로 검사한다.

Windows Terminal이 그림-only clipboard를 빈 bracketed paste로 보내는 근거:
Microsoft Terminal `TerminalPage::_PasteFromClipboardHandler`(src/cascadia/TerminalApp/TerminalPage.cpp).
aaa TUI는 Windows에서도 bracketed paste를 요청하며 사용자의 Terminal 설정은 바꾸지 않는다.

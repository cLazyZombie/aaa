/**
 * 붙여 넣는 글에서 ESC와 C1 CSI(U+009B)를 뺀다. 붙여 넣는 것은 글이다(키는 키 막대·send-keys로 보낸다).
 * 프로그램이 bracketed paste를 켰어도 글 안의 `ESC[201~`가 붙여넣기를 일찍 끝내 뒤 글을 친 키로 읽게 한다. pane 프로그램이
 * OSC 52(설정 「패널에서 클립보드로 복사」, 기본 켬)로 클립보드에 심은 글을 붙이면 셸이 그 명령을 실행했다(t-0340 보안 감사).
 * 참고 herdr-web-ui도 xterm.js 5.5 `bracketTextForPaste`로 그대로 감싸 같은 구멍이 있다(plan §8.106).
 */
export const pasteText = (text: string): string => text.replace(/[\x1b\x9b]/g, "");

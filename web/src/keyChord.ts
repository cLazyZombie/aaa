// 키 막대 유지 키 chord(`ctrl+left`, `alt+b` …)를 pane에 보낼 bytes로 바꾼다.
//
// herdr-web-ui는 chord를 herdr에 보내고 herdr가 pane 프로그램의 키보드 프로토콜로 인코딩한다(PaneTerminal
// 1360~1415줄, keys.ts `terminalChord`). aaa host에는 그 인코더가 없어 front에서 인코딩한다(참고와 다른 곳).
// 기준: 같은 수식을 하드웨어 키로 눌렀을 때 이 웹 터미널(xterm.js 기본 인코더 + Term.tsx 재매핑)이 보내는
// bytes와 같다. 웹 E2E 「키 막대」 단계가 키마다 그 둘을 견준다.
import { controlCode, ctrlEnterSequence } from "./herdr/keys.ts";

export interface ChordContext {
  /** 프로그램이 켠 화살표 모드(DECCKM): 수식 없는 화살표·Home·End가 SS3 */
  applicationCursorKeys: boolean;
  /** 프로그램이 요청한 modifyOtherKeys 단계(Ctrl+Enter) */
  modifyOtherKeys: number;
  /** xterm.js `isMac`(navigator.platform이 Mac 계열). Alt+화살표가 이것에 따라 다르다 */
  isMac: boolean;
}

/** xterm.js common/Platform.ts `isMac`와 같은 판정 */
export const IS_MAC =
  typeof navigator !== "undefined" && ["Macintosh", "MacIntel", "MacPPC", "Mac68K"].includes(navigator.platform);

const NAMES: Record<string, string> = { space: " ", plus: "+" };
const CURSOR: Record<string, string> = { up: "A", down: "B", right: "C", left: "D", home: "H", end: "F" };

/** `terminalChord`가 만든 chord를 bytes로. 모르는 이름이면 null. */
export function chordSequence(chord: string, ctx: ChordContext): string | null {
  const parts = chord.split("+");
  // 「+」 자체는 `plus`로 온다. 마지막 조각이 이름이다.
  const name = parts.pop()!;
  const ctrl = parts.includes("ctrl");
  const alt = parts.includes("alt");
  const shift = parts.includes("shift");
  const param = 1 + (shift ? 1 : 0) + (alt ? 2 : 0) + (ctrl ? 4 : 0);
  const esc = (s: string) => (alt ? "\x1b" + s : s);

  if (CURSOR[name]) {
    if (param === 1) return (ctx.applicationCursorKeys ? "\x1bO" : "\x1b[") + CURSOR[name];
    // xterm.js Keyboard.ts의 HACK과 같다: Alt만 누른 ←→는 단어 이동(macOS ESC b·f, 그 밖 Ctrl+←→),
    // Alt만 누른 ↑↓는 macOS가 아니면 Ctrl+↑↓
    if (param === 3 && (name === "left" || name === "right"))
      return ctx.isMac ? (name === "left" ? "\x1bb" : "\x1bf") : `\x1b[1;5${CURSOR[name]}`;
    if (param === 3 && (name === "up" || name === "down") && !ctx.isMac) return `\x1b[1;5${CURSOR[name]}`;
    return `\x1b[1;${param}${CURSOR[name]}`;
  }
  const f = /^f([1-9]|1[0-2])$/.exec(name);
  if (f) {
    const n = Number(f[1]);
    if (n <= 4) {
      const final = "PQRS"[n - 1];
      return param === 1 ? `\x1bO${final}` : `\x1b[1;${param}${final}`;
    }
    const code = [15, 17, 18, 19, 20, 21, 23, 24][n - 5];
    return param === 1 ? `\x1b[${code}~` : `\x1b[${code};${param}~`;
  }
  switch (name) {
    case "enter":
      // Term.tsx 재매핑과 같다: Shift+Enter는 ESC CR, Ctrl+Enter는 modifyOtherKeys가 켜졌으면 CSI 27;5;13~
      if (shift && !ctrl) return "\x1b\r";
      if (ctrl && !shift) return esc(ctrlEnterSequence(ctx.modifyOtherKeys) ?? "\r");
      return esc("\r");
    case "tab":
      return shift ? "\x1b[Z" : esc("\t");
    case "esc":
      return esc("\x1b");
    case "backspace":
      return esc(ctrl ? "\x08" : "\x7f");
  }
  const ch = NAMES[name] ?? name;
  if ([...ch].length !== 1) return null;
  let out = ch;
  // 글자는 그 자판이 낸 그대로 둔다(US Shift를 가정하지 않는다). 라틴 글자만 Shift를 대문자로.
  if (shift && /^[a-z]$/.test(out)) out = out.toUpperCase();
  if (ctrl) out = out === " " ? "\x00" : (controlCode(out) ?? out);
  return esc(out);
}

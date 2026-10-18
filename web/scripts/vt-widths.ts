// session host 화면(들여온 vt100)이 웹 터미널과 같은 칸을 쓰도록, 웹의 폭 규칙(herdr-web-ui terminalWidths.ts
// `herdrWidthProvider`: xterm grapheme addon 0.4.0 + herdr 보정 셋)을 code point 표로 옮긴다(plan §8.150).
// 쓰기: node web/scripts/vt-widths.ts → vendor/vt100/src/widths_table.rs, crates/aaa-vt/src/width_cases.rs
// 표는 code point마다 grapheme 부류·폭·묶기 방식이고, 앞 글자와 묶을지는 vt100 `widths.rs`가 addon과 같은 규칙으로 정한다.
// 만들 때 모든 code point × 여러 앞 상태에서 「표 + 그 규칙」이 provider와 같은지 대조하고, 다르면 멈춘다.
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import type { IUnicodeVersionProvider } from "@xterm/xterm";

/** grapheme 부류(addon UnicodeProperties.ts GRAPHEME_BREAK_*) */
const GB = { Other: 0, Prepend: 1, Extend: 2, Regional: 3, SpacingMark: 4, L: 5, V: 6, T: 7, LV: 8, LVT: 9, ZWJ: 10, ExtPic: 11 } as const;
/** 묶기 방식: 보통(addon 규칙), 묶지 않음(herdr: spacing mark는 제 칸), 보이지 않음(herdr: 앞 칸에 붙는 0칸, 줄 처음은 addon대로) */
const MODE = { plain: 0, never: 1, invisible: 2 } as const;

export interface Entry {
  code: number;
  width: number;
  mode: number;
}

interface State {
  code: number;
  width: number;
}

interface Step extends State {
  join: boolean;
}

/** addon `_shouldJoin`(GB6~GB13을 줄인 판) */
export function joins(before: number, after: number): boolean {
  if (before === GB.L && (after === GB.L || after === GB.V || after === GB.LV || after === GB.LVT)) return true;
  if ((before === GB.LV || before === GB.V) && (after === GB.V || after === GB.T)) return true;
  if ((before === GB.LVT || before === GB.T) && after === GB.T) return true;
  if (after === GB.Extend || after === GB.ZWJ || before === GB.Prepend || after === GB.SpacingMark) return true;
  if (before === GB.ZWJ && after === GB.ExtPic) return true;
  return before === GB.Regional && after === GB.Regional;
}

/** vt100 `widths::step`와 같은 규칙: 앞 상태(없으면 줄 처음·제어 뒤)와 이 글자의 표 값으로 묶음·폭·다음 상태 */
export function step(prev: State | null, e: Entry): Step {
  if (prev === null || e.mode === MODE.never) return { join: false, width: e.width, code: e.code };
  const [code, width] = e.mode === MODE.invisible ? [GB.Extend, 1] : [e.code, e.width];
  if (!joins(prev.code, code)) return { join: false, width, code };
  // 묶이면 넓은 쪽, 지역 표시 둘은 두 칸(국기)이고 셋째는 새로 시작한다
  if (code === GB.Regional) return { join: true, width: Math.max(prev.width, 2), code: GB.Other };
  return { join: true, width: Math.max(prev.width, width), code };
}

const props = (v: number): Step => ({ join: (v & 1) === 1, width: (v >> 1) & 3, code: (v >> 3) & 0xf });

/**
 * 웹 provider. Node에서 addon은 trie를 `Buffer.from`으로 풀어 BMP 밖을 잘못 읽는다(pool된 Buffer의 byteOffset):
 * 브라우저처럼 `atob`로 풀게 Buffer를 잠시 치운다
 */
export async function webProvider(): Promise<IUnicodeVersionProvider> {
  const saved = globalThis.Buffer;
  // @ts-expect-error 불러오는 동안만 없앤다
  delete globalThis.Buffer;
  try {
    const { herdrWidthProvider } = await import("../src/herdr/terminalWidths.ts");
    return herdrWidthProvider();
  } finally {
    globalThis.Buffer = saved;
  }
}

/**
 * code point의 표 값: 줄 처음의 답과 앞 글자가 있을 때의 답으로 묶기 방식을 가린다.
 * spacing mark(일반 범주 Mc)는 addon 부류가 Extend·Other 등 여럿이라 부류로 가리지 못한다. Prepend 뒤에는 보통 규칙이면
 * 무엇이든 묶이므로(GB9b) 거기서도 묶이지 않으면 「묶지 않음」이다
 */
function entryOf(p: IUnicodeVersionProvider, after: { letter: number; prepend: number }, cp: number): Entry {
  const alone = props(p.charProperties(cp, 0));
  const joined = props(p.charProperties(cp, after.letter));
  let mode: number = MODE.plain;
  if (!props(p.charProperties(cp, after.prepend)).join) mode = MODE.never;
  else if (joined.join && joined.code === GB.Extend && alone.code !== GB.Extend) mode = MODE.invisible;
  return { code: alone.code, width: alone.width, mode };
}

const pack = (e: Entry) => e.code | (e.width === 2 ? 0x10 : 0) | (e.mode << 5);

/** 대조에 쓰는 앞 글자들: 부류마다, 묶인 뒤, 지역 표시 짝 뒤, 보이지 않는 글자 뒤 */
const PREFIXES = ["a", "؀", "é", "\u{1F600}", "\u{1F600}‍", "\u{1F1F0}", "\u{1F1F0}\u{1F1F7}", "ᄀ", "ᅡ", "ᆨ", "가", "각", "का", "​", "a​", "⚠️", "1️", "\u{1F44B}\u{1F3FD}", "️", "ำ"];

/** 시험 사례: 한 줄에 쓰면 웹이 놓는 칸(묶음마다 시작 칸·폭·글). CSI·OSC는 묶음을 끊는다(xterm parser가 그 뒤 앞 글자와 묶지 않는다) */
const CASES = [
  "a\u{1F468}‍\u{1F469}‍\u{1F467}b",
  "⚠️x",
  "⚠︎x",
  "\u{1F1F0}\u{1F1F7}\u{1F1EF}x",
  "1️⃣x",
  "\u{1F44B}\u{1F3FD}x",
  "\u{1FAE0}x",
  "\u{1F3F3}️‍\u{1F308}x",
  "각x",
  "น้ำx",
  "किx",
  "a​b",
  "​x",
  "éx",
  "一\u{1F600}x",
  "⚠\x1b[m️x",
  "e\x1b[m\u{301}x",
  "\u{1F1F0}\x1b[m\u{1F1F7}x",
  "⚠\x1b]0;t\x07️x",
];

function layout(p: IUnicodeVersionProvider, text: string): [number, number, string][] {
  const cells: [number, number, string][] = [];
  let prev = 0;
  let col = 0;
  for (const ch of text.replace(/\x1b(\[[0-9;]*[@-~]|\][^\x07]*\x07)/g, "\x1b")) {
    if (ch === "\x1b") {
      prev = 0;
      continue;
    }
    const v = p.charProperties(ch.codePointAt(0)!, prev);
    prev = v;
    const s = props(v);
    const last = cells[cells.length - 1];
    if (s.join && last) {
      col += s.width - last[1];
      last[1] = s.width;
      last[2] += ch;
    } else {
      cells.push([col, s.width, ch]);
      col += s.width;
    }
  }
  return cells;
}

const rustStr = (s: string) => `"${[...s].map((c) => (c.codePointAt(0)! >= 0x20 && c.codePointAt(0)! < 0x80 ? c : `\\u{${c.codePointAt(0)!.toString(16)}}`)).join("")}"`;

/** 표와 시험 사례의 Rust 소스. 「표 + step」이 provider와 다르면 던진다(`verify: false`면 대조를 건너뛴다: 신선도 시험) */
export async function generate({ verify = true } = {}): Promise<{ table: string; cases: string }> {
  const p = await webProvider();
  const after = { letter: p.charProperties(0x61, 0), prepend: p.charProperties(0x600, 0) };
  const entries: Entry[] = [];
  for (let cp = 0; cp <= 0x10ffff; cp++) entries.push(cp >= 0xd800 && cp <= 0xdfff ? entries[0xd7ff] : entryOf(p, after, cp));
  // 표는 폭을 한 칸·두 칸으로만 담는다
  const odd = entries.findIndex((e) => e.width !== 1 && e.width !== 2);
  if (odd >= 0) throw new Error(`U+${odd.toString(16)} 홀로 폭 ${entries[odd].width}`);

  // 대조: 앞 상태마다 provider와 step이 같은 답(묶음·폭·부류)을 내는가
  for (const prefix of verify ? PREFIXES : []) {
    let pv = 0;
    let st: State | null = null;
    for (const ch of prefix) {
      const cp = ch.codePointAt(0)!;
      pv = p.charProperties(cp, pv);
      st = step(st, entries[cp]);
    }
    const want = props(pv);
    if (st === null || want.width !== st.width || want.code !== st.code) throw new Error(`앞 글자 ${JSON.stringify(prefix)}의 상태가 다르다`);
    for (let cp = 0x20; cp <= 0x10ffff; cp++) {
      if (cp >= 0xd800 && cp <= 0xdfff) continue;
      const a = props(p.charProperties(cp, pv));
      const b = step(st, entries[cp]);
      if (a.join !== b.join || a.width !== b.width || a.code !== b.code)
        throw new Error(`U+${cp.toString(16)} 뒤 ${JSON.stringify(prefix)}: provider ${JSON.stringify(a)}, 표 ${JSON.stringify(b)}`);
    }
  }

  const ranges: [number, number][] = [];
  entries.forEach((e, cp) => {
    const v = pack(e);
    if (ranges.length === 0 || ranges[ranges.length - 1][1] !== v) ranges.push([cp, v]);
  });
  const rows: string[] = [];
  for (let i = 0; i < ranges.length; i += 6) rows.push("    " + ranges.slice(i, i + 6).map(([cp, v]) => `(0x${cp.toString(16)}, 0x${v.toString(16)})`).join(", ") + ",");
  const table = `// 생성: node web/scripts/vt-widths.ts(웹 폭 규칙 herdrWidthProvider에서, plan §8.150). 손으로 고치지 않는다.
//! code point 범위의 글자 폭 표. 값의 아래 4비트는 grapheme 부류, 0x10은 두 칸, 0x60은 묶기 방식(0 보통, 0x20 묶지 않음, 0x40 보이지 않음)

/// (범위 시작 code point, 값). 다음 시작 앞까지 같은 값이다
#[rustfmt::skip]
pub(crate) static RANGES: [(u32, u8); ${ranges.length}] = [
${rows.join("\n")}
];
`;
  const cases = `// 생성: node web/scripts/vt-widths.ts(웹 폭 규칙 herdrWidthProvider로 한 줄에 쓴 칸, plan §8.150). 손으로 고치지 않는다.
//! 웹이 놓는 칸: (쓴 글, [(시작 칸, 폭, 그 칸의 글)])

#[rustfmt::skip]
pub(crate) const CASES: &[(&str, &[(u16, u16, &str)])] = &[
${CASES.map((text) => `    (${rustStr(text)}, &[${layout(p, text).map(([c, w, s]) => `(${c}, ${w}, ${rustStr(s)})`).join(", ")}]),`).join("\n")}
];
`;
  return { table, cases };
}

export const TABLE_PATH = join(fileURLToPath(new URL(".", import.meta.url)), "..", "..", "vendor", "vt100", "src", "widths_table.rs");
export const CASES_PATH = join(fileURLToPath(new URL(".", import.meta.url)), "..", "..", "crates", "aaa-vt", "src", "width_cases.rs");

if (import.meta.url === `file://${process.argv[1]}`) {
  const { table, cases } = await generate();
  writeFileSync(TABLE_PATH, table);
  writeFileSync(CASES_PATH, cases);
  console.log(`${TABLE_PATH}\n${CASES_PATH}`);
}

// 고정 sleep: 기다리는 것이 오기를 보지 않고 정한 시간만 흘려보내는 기다림. 느린 CI에서는 짧고 다른 곳에서는 길다
// (사용자 10-17 「기다리기만하는 테스트는 다르게 풀어」, docs/testing.md). herdr-web-ui scripts/fixed-sleeps.ts를 옮겼다.
// 참고가 세지 않던 Playwright `waitForTimeout`과 Rust 시험의 `thread::sleep`·`tokio::time::sleep`도 센다.
//
// 세지 않는 것(참고와 같다):
// - 시한(deadline)이나 횟수로 끝나는 반복 안의 읽기 사이 쉼(`while (Date.now() < deadline) …`, `for (let i = 0; i < 40 …)`,
//   본문이 시한에 throw·assert·return·break하는 반복. Rust는 `Instant::now()`·`deadline`·`elapsed()`를 보고 panic·assert·return·break하는 반복)
// - 실제 사건과 겨루는 시한(`Promise.race([event, sleep(ms)])`)
// - 0ms(차례 넘기기)
// - 일부러 고정한 기다림이 그 줄이나 바로 윗줄에 `// fixed wait: <이유>`로 말하는 것(「아무것도 오지 않음」 증명 등)
//
// 세는 곳: web/e2e·web/test·web/src(JS·TS), crates/*/tests(Rust), crates/*/src의 `#[cfg(test)]` 뒤.
// fixed-sleeps.test.mjs가 파일마다 fixed-sleeps.json의 수를 넘지 않게 하고, 그 수는 줄기만 한다.
// `node web/test/fixed-sleeps.mjs`가 지금 수를 보인다(`--list`면 줄마다).

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const SELF = ["web/test/fixed-sleeps.mjs", "web/test/fixed-sleeps.test.mjs"];

export const read = (file) => fs.readFileSync(path.join(repo, file), "utf8");

/** 저장소 기준 경로(정렬). node_modules·target·dist는 보지 않는다 */
function walk(dir, ok) {
  const abs = path.join(repo, dir);
  if (!fs.existsSync(abs)) return [];
  return fs
    .readdirSync(abs, { recursive: true })
    .map((f) => path.join(dir, String(f)).split(path.sep).join("/"))
    .filter((f) => !f.split("/").some((p) => p === "node_modules" || p === "target" || p === "dist" || p === "gen"))
    .filter((f) => ok(f) && fs.statSync(path.join(repo, f)).isFile())
    .sort();
}

/** 이 규칙이 보는 파일 */
export function sleepFiles() {
  const js = (f) => /\.(mjs|js|ts|tsx)$/.test(f) && !SELF.includes(f);
  const crates = fs.existsSync(path.join(repo, "crates")) ? fs.readdirSync(path.join(repo, "crates")).sort() : [];
  return [
    ...walk("web/e2e", js),
    ...walk("web/test", js),
    ...walk("web/src", js),
    ...crates.flatMap((c) => [...walk(`crates/${c}/tests`, (f) => f.endsWith(".rs")), ...walk(`crates/${c}/src`, (f) => f.endsWith(".rs"))]),
  ];
}

/**
 * 주석을 글자마다 빈칸으로 바꾼 글(줄바꿈은 두어 줄 번호가 맞는다). `strings`면 문자열·template 안도 비운다
 * (herdr-web-ui source-scan.ts stripComments). 나눗셈과 정규식 `/`를 가리지 않는다: 따옴표나 `//`를 든 정규식만 문제다.
 */
export function stripJs(text, { strings = false } = {}) {
  let out = "";
  for (let i = 0; i < text.length; ) {
    const ch = text[i];
    const next = text[i + 1];
    if (ch === "/" && (next === "/" || next === "*")) {
      const close = next === "/" ? text.indexOf("\n", i) : text.indexOf("*/", i + 2);
      const end = close < 0 ? text.length : next === "/" ? close : close + 2;
      out += text.slice(i, end).replace(/[^\n]/g, " ");
      i = end;
      continue;
    }
    if (ch === '"' || ch === "'" || ch === "`") {
      let j = i + 1;
      while (j < text.length && text[j] !== ch && (ch === "`" || text[j] !== "\n")) j += text[j] === "\\" ? 2 : 1;
      out += strings ? ch + text.slice(i + 1, j).replace(/[^\n]/g, " ") + text.slice(j, j + 1) : text.slice(i, j + 1);
      i = j + 1;
      continue;
    }
    out += ch;
    i += 1;
  }
  return out;
}

/** Rust: 주석·문자열(raw 문자열 `r#"…"#` 포함)·문자 글자를 비운다. 수명(`'a`)은 둔다 */
export function stripRust(text) {
  let out = "";
  const blank = (s) => s.replace(/[^\n]/g, " ");
  for (let i = 0; i < text.length; ) {
    const ch = text[i];
    const next = text[i + 1];
    if (ch === "/" && next === "/") {
      const close = text.indexOf("\n", i);
      const end = close < 0 ? text.length : close;
      out += blank(text.slice(i, end));
      i = end;
      continue;
    }
    if (ch === "/" && next === "*") {
      let depth = 0;
      let j = i;
      while (j < text.length) {
        if (text.startsWith("/*", j)) (depth += 1), (j += 2);
        else if (text.startsWith("*/", j)) {
          depth -= 1;
          j += 2;
          if (depth === 0) break;
        } else j += 1;
      }
      out += blank(text.slice(i, j));
      i = j;
      continue;
    }
    const raw = /^b?r(#*)"/.exec(text.slice(i, i + 40));
    if (raw && !/[\w]/.test(text[i - 1] ?? "")) {
      const close = `"${raw[1]}`;
      const end = text.indexOf(close, i + raw[0].length);
      const stop = end < 0 ? text.length : end + close.length;
      out += raw[0] + blank(text.slice(i + raw[0].length, stop - close.length)) + text.slice(stop - close.length, stop);
      i = stop;
      continue;
    }
    if (ch === '"') {
      let j = i + 1;
      while (j < text.length && text[j] !== '"') j += text[j] === "\\" ? 2 : 1;
      out += '"' + blank(text.slice(i + 1, j)) + '"';
      i = j + 1;
      continue;
    }
    if (ch === "'" && (next === "\\" || text[i + 2] === "'")) {
      let j = i + 1;
      while (j < text.length && text[j] !== "'") j += text[j] === "\\" ? 2 : 1;
      out += "'" + blank(text.slice(i + 1, j)) + "'";
      i = j + 1;
      continue;
    }
    out += ch;
    i += 1;
  }
  return out;
}

const lineOf = (text, index) => text.slice(0, index).split("\n").length;

/** `// fixed wait:` 다음에 이유 */
const MARKER = /\/\/\s*fixed wait:\s*\S/;

// ── JS ──
/** Playwright waitForTimeout, 또는 setTimeout으로 푸는 Promise(0ms 넘기기 아님) */
const JS_SLEEP =
  /\.waitForTimeout\s*\((?!\s*0\s*\))|\bnew Promise(?:<[^>]*>)?\(\s*(?:async\s*)?(?:function\s*)?\(?\s*(\w+)\s*\)?\s*(?:=>)?\s*\{?\s*(?:window\.|globalThis\.)?setTimeout\(\s*(?:\1|\(\s*\)\s*=>\s*\{?\s*\1\s*\(\s*\)\s*;?\s*\}?)\s*,(?!\s*0\s*\))/g;
/** 시계·시한·횟수로 끝나는 반복 머리(`i < 40`, `i < MAX_TRIES`). 그냥 `i < list.length`는 아니다 */
const JS_BOUND = /Date\.now\(\)|performance\.now\(\)|[Dd]eadline|\b\w+\s*<=?\s*(?:\d|[A-Z][A-Z0-9_]*\b|\w*(?:[Tt]ries|[Aa]ttempts|[Rr]etries|[Mm]ax|[Ll]imit)\w*)/;

/** `at`를 감싼 가장 안쪽 반복(그 함수 안)이 끝이 있는가. `code`는 주석·문자열을 비운 글 */
function jsBoundedLoop(code, at) {
  for (let from = at; ; ) {
    let open = -1;
    for (let i = from - 1, depth = 0; i >= 0; i--) {
      if (code[i] === "}") depth += 1;
      else if (code[i] === "{") {
        if (depth === 0) {
          open = i;
          break;
        }
        depth -= 1;
      }
    }
    if (open < 0) return false;
    let close = open + 1;
    for (let depth = 1; depth > 0 && close < code.length; close++) depth += code[close] === "{" ? 1 : code[close] === "}" ? -1 : 0;
    const head = code.slice(code.lastIndexOf("\n", open - 1) + 1, open).trim();
    const body = code.slice(open, close);
    const doWhile = /(?:^|[\s;}])do$/.test(head) ? (/^\s*while\s*\(([^\n]*)/.exec(code.slice(close))?.[1] ?? "") : null;
    if (doWhile !== null || /\b(?:while|for)\s*\(/.test(head)) {
      return JS_BOUND.test(doWhile ?? head) || (/Date\.now\(\)|performance\.now\(\)|deadline/i.test(body) && /\b(?:throw|return|break|assert|expect)\b/.test(body));
    }
    // 함수 본문: 반복이 있다면 그 안이다
    if (/=>$|\)\s*(?::[^{]*)?$/.test(head) && !/\b(?:if|switch|catch|with)\s*\(/.test(head)) return false;
    from = open;
  }
}

/** sleep에 묶인 이름(`const sleep = (ms) => new Promise(…setTimeout…)`)과 그 정의 자리, 기본이 0ms라 그냥 부르면 넘기기인 것 */
function jsSleepHelpers(code, source) {
  const names = new Set();
  const definitions = new Set();
  const yields = new Set();
  const define =
    /(?:\b(?:const|let|var)\s+(\w+)\s*=\s*(?:async\s*)?(\([^)]*\)|\w+)\s*(?::[^=;\n]*)?=>\s*(?:\{\s*return\s+)?|\bfunction\s+(\w+)\s*(\([^)]*\))[^{;]*\{\s*return\s+)(?=\bnew Promise)/g;
  for (const m of code.matchAll(define)) {
    const at = m.index + m[0].length;
    if (new RegExp(JS_SLEEP.source, "y").exec(code.slice(at)) === null) continue;
    const name = m[1] ?? m[3];
    names.add(name);
    definitions.add(at);
    if (/=\s*0\s*\)$/.test(m[2] ?? m[4])) yields.add(name);
  }
  for (const m of source.matchAll(/\bimport\s*\{([^}]*)\}\s*from\s*["'](?:node:)?timers\/promises["']/g)) {
    const named = /\bsetTimeout(?:\s+as\s+(\w+))?\b/.exec(m[1]);
    if (named) names.add(named[1] ?? "setTimeout");
  }
  return { names, definitions, yields };
}

function jsFixedSleeps(file, text) {
  const code = stripJs(text, { strings: true });
  const lines = text.split("\n");
  const codeLines = code.split("\n");
  const helpers = jsSleepHelpers(code, stripJs(text));
  const calls = helpers.names.size ? [...code.matchAll(new RegExp(`(?<![\\w.])(?:${[...helpers.names].join("|")})\\s*\\((?!\\s*0\\s*\\))`, "g"))] : [];
  const sleeps = [...code.matchAll(JS_SLEEP)].filter((m) => !helpers.definitions.has(m.index));
  const waits = [
    ...sleeps,
    ...calls.filter(
      (m) =>
        !/\bfunction\s+$/.test(code.slice(Math.max(0, m.index - 20), m.index)) &&
        !(helpers.yields.has(m[0].replace(/\s*\($/, "")) && /^\(\s*\)/.test(code.slice(m.index + m[0].length - 1))),
    ),
  ].sort((a, b) => a.index - b.index);
  const found = [];
  for (const m of waits) {
    const line = lineOf(code, m.index);
    const codeLine = codeLines[line - 1];
    if (/\bPromise\.race\s*\(/.test(codeLine)) continue;
    if (MARKER.test(lines[line - 1]) || MARKER.test(lines[line - 2] ?? "")) continue;
    const before = codeLine.slice(0, m.index - (code.lastIndexOf("\n", m.index) + 1));
    if (/^\s*(?:while|for)\s*\(/.test(before) && JS_BOUND.test(before)) continue;
    if (jsBoundedLoop(code, m.index)) continue;
    found.push(`${file}:${line}: ${lines[line - 1].trim().slice(0, 160)}`);
  }
  return found;
}

// ── Rust ──
/** thread::sleep·tokio::time::sleep(0이 아닌 시간) */
const RS_SLEEP = /\b(?:std::)?thread::sleep\s*\((?!\s*Duration::ZERO|\s*Duration::from_\w+\(\s*0\s*\))|\btokio::time::sleep\s*\((?!\s*Duration::ZERO|\s*Duration::from_\w+\(\s*0\s*\))/g;
const RS_CLOCK = /Instant::now\(\)|[Dd]eadline|\.elapsed\(\)/;

/** `at`를 감싼 가장 안쪽 `loop`·`while`·`for`(그 함수·closure 안)가 끝이 있는가 */
function rsBoundedLoop(code, at) {
  for (let from = at; ; ) {
    let open = -1;
    for (let i = from - 1, depth = 0; i >= 0; i--) {
      if (code[i] === "}") depth += 1;
      else if (code[i] === "{") {
        if (depth === 0) {
          open = i;
          break;
        }
        depth -= 1;
      }
    }
    if (open < 0) return false;
    let close = open + 1;
    for (let depth = 1; depth > 0 && close < code.length; close++) depth += code[close] === "{" ? 1 : code[close] === "}" ? -1 : 0;
    const head = code.slice(code.lastIndexOf("\n", open - 1) + 1, open).trim();
    const body = code.slice(open, close);
    const kw = /(?:^|[\s;:}=(])(loop|while\b[^{]*|for\b[^{]*\bin\b[^{]*)$/.exec(head);
    if (kw) {
      const h = kw[1];
      if (RS_CLOCK.test(h) || /\bin\s+0\.\.=?\s*(?:\d|[A-Z][A-Z0-9_]*\b)/.test(h)) return true;
      return RS_CLOCK.test(body) && /\b(?:panic!|assert!|assert_eq!|assert_ne!|return|break|unreachable!)/.test(body);
    }
    // 함수·closure 본문: 반복이 있다면 그 안이다
    if (/\)\s*(?:->[^{]*)?$|\|\s*(?:->[^{]*)?$|\bmove$/.test(head) && !/\b(?:if|match|while|for)\b/.test(head)) return false;
    from = open;
  }
}

function rsFixedSleeps(file, text) {
  let code = stripRust(text);
  // 제품 코드(src)는 `#[cfg(test)]` 뒤만 본다: 그 앞의 sleep은 시험의 기다림이 아니라 제품의 주기다
  if (/^crates\/[^/]+\/src\//.test(file) && !/\/tests?\.rs$|\/tests\//.test(file)) {
    const at = code.search(/#\[cfg\(test\)\]/);
    if (at < 0) return [];
    code = code.slice(0, at).replace(/[^\n]/g, " ") + code.slice(at);
  }
  const lines = text.split("\n");
  const found = [];
  for (const m of code.matchAll(RS_SLEEP)) {
    const line = lineOf(code, m.index);
    if (MARKER.test(lines[line - 1]) || MARKER.test(lines[line - 2] ?? "")) continue;
    if (rsBoundedLoop(code, m.index)) continue;
    found.push(`${file}:${line}: ${lines[line - 1].trim().slice(0, 160)}`);
  }
  return found;
}

/** 한 파일의 고정 sleep(`file:line: 글`) */
export function fixedSleeps(file, text = read(file)) {
  return file.endsWith(".rs") ? rsFixedSleeps(file, text) : jsFixedSleeps(file, text);
}

/** 파일마다 고정 sleep 수(0인 파일은 뺀다) */
export function fixedSleepCounts() {
  const counts = {};
  for (const file of sleepFiles()) {
    const n = fixedSleeps(file).length;
    if (n) counts[file] = n;
  }
  return counts;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv.includes("--list")) for (const f of sleepFiles()) for (const l of fixedSleeps(f)) console.log(l);
  else console.log(JSON.stringify(fixedSleepCounts(), null, 2));
}

// 웹 E2E 공통 준비: 실험실(host·agent·가짜 push·구독 서버·시험 프로그램)과 Chromium, 단계·기다림·화면·IME·push 도우미.
// spec(web/e2e/specs/*.mjs) 하나를 한 프로세스에서 돈다(lib/child.mjs). 이 파일을 불러오면 실험실을 만든다: 입구(web.e2e.mjs)는
// 불러오지 않는다. 실행·고르기는 web.e2e.mjs 머리, 쓰는 법은 docs/testing.md 「웹 E2E」.

import { spawn, execFile, execFileSync } from "node:child_process";
import { createRequire } from "node:module";
import crypto from "node:crypto";
import fs from "node:fs";
import http from "node:http";
import net from "node:net";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { terminateDeployGuards, terminateSessionHosts } from "./session-cleanup.mjs";

const require = createRequire(path.join(process.env.NODE_PATH ?? process.cwd(), "noop.js"));
const { chromium, webkit } = require("playwright");

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../..");
const AA = process.env.AAA_BIN ?? path.join(repo, "target/debug/aaa");
// 스크린샷 폴더: 입구(web.e2e.mjs [폴더])가 넘긴다
const shots = process.env.AAA_E2E_SHOTS || null;
const root = `/tmp/aaw-${process.pid}`;
// CLAUDE_CONFIG_DIR: agent가 대화 기록을 읽는 Claude 폴더(실험실 안, 실제 ~/.claude는 건드리지 않는다)
const env = {
  ...process.env,
  AAA_CONFIG_DIR: `${root}/c`,
  AAA_STATE_DIR: `${root}/s`,
  CLAUDE_CONFIG_DIR: `${root}/claude`,
  RUST_LOG: "aaa=info",
  // 상태 사건 연결의 WebSocket ping 1초, 3초 동안 아무것도 받지 못하면 host가 끊는다(기본 15초·45초, 「두 기기·죽은 연결」 단계)
  AAA_TEST_EVENTS_PING_MS: "1000",
  // thread brief: 가짜 agent 화면이 이만큼 그대로면 친다(기본 3초. 12-threads-push는 thread를 단계마다 연다)
  AAA_TEST_BRIEF_SETTLE_MS: "300",
  // 기본 profile(claude·codex) 목록이 이 머신의 설치·로그인에 따라 바뀌지 않게
  AAA_TEST_BUILTINS: "",
};
// 운영 pane에서 시험을 실행해도 실험실 CLI가 그 세션·hook 신원을 물려받지 않는다(Rust Lab과 같다).
for (const key of ["AAA_SESSION_ID", "AAA_PROJECT", "AAA_THREAD", "AAA_HOOK_SOCK", "AAA_HOOK_TOKEN"]) delete env[key];
const procs = new Map();

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
// t-0578 동시 검증(load 28.96)의 후보 점검 결과 대기만 30초→60초로 둔다(docs/testing.md).
// 실패 상한이며 표지가 오면 바로 끝난다. 기본 locator·단계 전체 시한은 그대로다.
const LOAD_TIMEOUT_MS = 60_000;

function aaa(...args) {
  // --json은 맨 앞에: `pane run <pane> <글…>`처럼 뒤 인자를 모두 글로 받는 명령에 섞이지 않게
  const a = ["--json", ...args];
  try {
    return JSON.parse(execFileSync(AA, a, { env, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }));
  } catch (e) {
    // 실패하면 CLI가 답한 것(--json은 오류도 stdout에 쓴다)과 stderr를 남긴다
    if (e.status !== undefined) e.message = `aaa ${args.join(" ")}: 끝 코드 ${e.status} ${String(e.stdout ?? "").trim()} ${String(e.stderr ?? "").trim()}`.slice(0, 600);
    throw e;
  }
}

// pane 출력에 글이 나올 때까지(`pane wait-output`: host가 출력 사건마다 다시 본다). 기다리는 동안 node를 막지 않는다.
function waitOutput(pane, text, source = "visible", ms = 20_000) {
  const args = ["--json", "pane", "wait-output", pane, `--match=${text}`, "--source", source, "--timeout", String(ms)];
  return new Promise((resolve, reject) =>
    execFile(AA, args, { env, encoding: "utf8" }, (e, out, err) =>
      e ? reject(new Error(`${pane}에 ${text}이 나오지 않았다: ${err || e.message}`)) : resolve(JSON.parse(out)),
    ),
  );
}

// 첫 인자가 객체면 이 프로세스에만 더할 환경 변수다(예: 재부팅 흉내의 AAA_TEST_BOOT_ID)
function start(tag, ...args) {
  const extra = typeof args[0] === "object" ? args.shift() : {};
  const log = fs.openSync(`${root}/${tag}.log`, "w");
  const p = spawn(AA, args, { env: { ...env, ...extra }, stdio: ["ignore", log, log] });
  procs.set(tag, p);
  return p;
}

async function stop(tag) {
  const p = procs.get(tag);
  procs.delete(tag);
  if (!p || p.exitCode !== null) return;
  const done = new Promise((r) => p.once("exit", r));
  p.kill("SIGKILL");
  await done;
}

async function until(what, fn, ms = 20_000) {
  const deadline = Date.now() + ms;
  for (;;) {
    try {
      const v = await fn();
      if (v) return v;
    } catch {
      // 아직 준비되지 않았다
    }
    if (Date.now() > deadline) throw new Error(`${ms}ms 안에 ${what}이 되지 않았다`);
    await sleep(100);
  }
}

// 사건으로 끝내는 기다림(새 시험): DOM은 MutationObserver, 주소는 hashchange. ms는 실패 한도다(통과용 기다림이 아니다).
// `fn(arg)`은 page에서 돈다(Playwright가 함수를 글로 옮기듯 글로 넘긴다).
/**
 * 페이지 안 대기에 Node 쪽 시한을 건다. 페이지 시계(setTimeout·requestAnimationFrame)는 멈추거나(Playwright `clock`) 얼 수 있고
 * `evaluate`에는 시한이 없다: 페이지 안 시한만 믿으면 끝나지 않는다(10-18 spec 11이 시계를 멈춘 뒤 9분 넘게 멈춤). 페이지 쪽
 * 오류 글이 먼저 오도록 1초 뒤에 끊는다
 */
function inTime(promise, ms, what) {
  let timer;
  const late = new Promise((_, fail) => {
    timer = setTimeout(() => fail(new Error(`${ms}ms 안에 ${what}이 되지 않았다(페이지가 답하지 않음)`)), ms + 1000);
  });
  return Promise.race([promise, late]).finally(() => clearTimeout(timer));
}

const domIs = (p, what, fn, arg, ms = 20_000) =>
  inTime(p.evaluate(`new Promise((done, fail) => {
    const ok = () => { try { return Boolean((${fn})(${JSON.stringify(arg)})); } catch { return false; } };
    if (ok()) return done();
    const mo = new MutationObserver(() => { if (ok()) { clearTimeout(stuck); mo.disconnect(); done(); } });
    const stuck = setTimeout(() => { mo.disconnect(); fail(new Error(${JSON.stringify(`${ms}ms 안에 ${what}이 되지 않았다`)})); }, ${ms});
    mo.observe(document, { subtree: true, childList: true, attributes: true, characterData: true });
  })`), ms, what);
const hashIs = (p, hash, ms = 20_000) =>
  inTime(p.evaluate(
    ([hash, ms]) =>
      new Promise((done, fail) => {
        if (location.hash === hash) return done();
        const on = () => {
          if (location.hash !== hash) return;
          clearTimeout(stuck);
          removeEventListener("hashchange", on);
          done();
        };
        const stuck = setTimeout(() => {
          removeEventListener("hashchange", on);
          fail(new Error(`${ms}ms 안에 주소가 ${hash}이 되지 않았다(${location.hash})`));
        }, ms);
        addEventListener("hashchange", on);
      }),
    [hash, ms],
  ), ms, `주소 ${hash}`);

// focus 기다림: focusin 사건으로 끝낸다(focus는 DOM을 바꾸지 않아 MutationObserver로는 못 본다)
const focusIs = (p, what, sel, ms = 20_000) =>
  inTime(p.evaluate(`new Promise((done, fail) => {
    const ok = () => document.activeElement?.matches(${JSON.stringify(sel)}) === true;
    if (ok()) return done();
    const on = () => { if (ok()) { clearTimeout(stuck); removeEventListener("focusin", on, true); done(); } };
    const stuck = setTimeout(() => { removeEventListener("focusin", on, true); fail(new Error(${JSON.stringify(`${ms}ms 안에 ${what}에 focus가 오지 않았다`)} + ": " + (document.activeElement?.outerHTML ?? "").slice(0, 160))); }, ${ms});
    addEventListener("focusin", on, true);
  })`), ms, `${what} focus`);

function freePort() {
  return new Promise((resolve) => {
    const s = net.createServer();
    s.listen(0, "127.0.0.1", () => {
      const { port } = s.address();
      s.close(() => resolve(port));
    });
  });
}

/** session host와 파일을 쓰는 자손이 모두 끝난 뒤 돌아온다. */
const killSessionHosts = () => terminateSessionHosts(`${root}/s/agent/sessions`);

const screenText = (page) => page.locator(".xterm-rows").innerText();
/**
 * CI에서만 터미널 글이 비어 보인 실패(10-17 dbacbfe·372e85d·69224fd·e784eac: snapshot은 왔는데 줄이 비었다)를 가리는 기록:
 * 그때 줄·칸 자리·가림을 남기고, 다시 그리기를 한 번 재촉한 뒤 글이 생기는지도 본다(생기면 그리기가 멈춰 있던 것이다).
 * eeee514에서는 글이 맨 아래보다 다섯 줄 위에서 멈춰 보였다(기록 높이는 맞다): viewport 자리와 pane WebSocket 메시지 순서(log)를 더 남긴다.
 * 실패를 넘기지 않는다: 이 값은 오류 글에만 들어간다.
 */
/** 이 화면이 pane WebSocket으로 보낸 메시지({t: "input", data}·submit·resize·take…, 바이너리 마우스 보고는 {t: "bin", data})를
 *  from번째부터. 사건 처리 안에서 보낸 것은 그 동작(click·press·evaluate)이 끝났을 때 이미 있다 */
const sentFrames = (page, from = 0) => page.evaluate((n) => window.__aaaSent.slice(n), from);
/** 지금까지 보낸 메시지 수(sentFrames의 from으로 쓴다) */
const sentCount = (page) => page.evaluate(() => window.__aaaSent.length);
/** 그중 터미널에 들어가는 글(input·bin)을 이어 붙인 것 */
const sentText = (frames) => frames.filter((f) => f.t === "input" || f.t === "bin").map((f) => f.data).join("");

async function termDiag(page) {
  const now = () =>
    page.evaluate(() => {
      const rows = document.querySelector(".xterm-rows");
      const screen = document.querySelector(".xterm-screen")?.getBoundingClientRect();
      const scroll = document.querySelector(".term-scroll");
      const vp = document.querySelector(".xterm-viewport");
      return {
        text: rows?.innerText.replace(/\s+/g, " ").trim().slice(-200),
        rows: rows?.childElementCount,
        font: rows ? getComputedStyle(rows).fontSize : null,
        screen: screen ? [screen.x, screen.y, screen.width, screen.height].map(Math.round) : null,
        win: [innerWidth, innerHeight],
        scroll: scroll ? [scroll.clientWidth, scroll.clientHeight, scroll.scrollTop, scroll.scrollHeight] : null,
        viewport: vp ? [vp.scrollTop, vp.scrollHeight, vp.clientHeight] : null,
        fontFamily: rows ? getComputedStyle(rows).fontFamily : null,
        log: (window.__aaaTermLog ?? []).slice(-40).map((x) => (x.kind ? `${x.at} bin×${x.bins} ${x.bytes}B ack×${x.acks} ${x.acked}` : `${x.at} ${x.e}`)),
        stackHidden: document.querySelector(".term-stack")?.hidden,
        vis: document.visibilityState,
        focus: document.hasFocus(),
        banners: [...document.querySelectorAll(".banner")].map((b) => b.textContent),
      };
    });
  const before = await now();
  await page.evaluate(() => {
    document.querySelector(".xterm-screen")?.scrollIntoView();
    window.dispatchEvent(new Event("resize"));
  });
  await sleep(600);
  const after = (await now()).text;
  return { ...before, afterNudge: after };
}

/** 터미널에 그림 파일 붙여 넣기(브라우저 붙여 넣기 사건과 같은 모양) */
async function pasteFile(page, bytes, type, name = "shot.png") {
  await page.evaluate(
    ({ b64, type, name }) => {
      const bin = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
      const dt = new DataTransfer();
      dt.items.add(new File([bin], name, { type }));
      const ev = new ClipboardEvent("paste", { clipboardData: dt, bubbles: true, cancelable: true });
      document.querySelector('[data-testid="terminal"] textarea').dispatchEvent(ev);
    },
    { b64: Buffer.from(bytes).toString("base64"), type, name },
  );
}

/** 배너의 「…: <절대 경로>」에서 경로 */
const bannerPath = (text) => /(\/\S+\.(png|jpg|gif|webp))/.exec(text)?.[1];
// 움직이는 중(창 크기를 바꾼 뒤 drawer·sheet가 미끄러지는 중)을 찍지 않는다: 끝나는 움직임은 끝까지 감고 끝없는 것은 처음 모습으로
const shot = async (page, name) => shots && (await page.screenshot({ path: path.join(shots, `${name}.png`), animations: "disabled" }));

/** 터미널 글꼴 측정: xterm cell 너비, 실제 글꼴 목록으로 잰 영문·한글 너비, 넣은 WOFF2만으로 잰 너비 */
async function termFont(page, cols) {
  return page.evaluate(async (cols) => {
    const box = document.querySelector('[data-testid="terminal"]');
    const rows = box.querySelector(".xterm-rows");
    const cs = getComputedStyle(rows);
    const fs = parseFloat(cs.fontSize);
    const cell = box.querySelector(".xterm-screen").getBoundingClientRect().width / cols;
    const ctx = document.createElement("canvas").getContext("2d");
    const w = (font, t) => ((ctx.font = font), ctx.measureText(t).width);
    const web = new FontFace("aaa-woff2-test", "url(/fonts/meslotendard-0.3.1/Meslotendard-Regular.woff2)");
    await web.load();
    document.fonts.add(web);
    return {
      family: cs.fontFamily,
      fs,
      cell,
      latin: w(`${fs}px ${cs.fontFamily}`, "M"),
      hangul: w(`${fs}px ${cs.fontFamily}`, "한"),
      webLatin: w(`${fs}px aaa-woff2-test`, "M"),
      webHangul: w(`${fs}px aaa-woff2-test`, "한"),
    };
  }, cols);
}

/** 터미널이 Meslotendard로 그려지고(넣은 WOFF2를 받았으면 그것으로 바뀐 뒤), cell 너비 = 영문, 한글 = 정확히 두 칸 */
async function fontOk(page, cols) {
  let m = null;
  await until("터미널 글꼴 Meslotendard", async () => {
    m = await termFont(page, cols);
    return m.family.includes('"Meslotendard Web"');
  });
  const near = (a, b, tol) => Math.abs(a - b) <= tol;
  if (!near(m.cell, m.latin, 0.02) || !near(m.hangul, 2 * m.latin, 0.01) || !near(m.webHangul, 2 * m.webLatin, 0.01) || !near(m.webLatin, m.latin, 0.01))
    throw new Error(`글꼴 너비가 맞지 않다: ${JSON.stringify(m)}`);
  return m;
}

/**
 * 터미널(xterm 숨은 textarea)에 macOS 두벌식으로 「상태」를 조합해 넣는다. 키마다 keydown(229) → 조합 사건 → input.
 * late: 첫 자모가 textarea에 들어간 다음 task에 compositionstart가 온다(keydown과 조합 사건이 따로 오는 브라우저).
 * xterm 6.0.0은 이때 첫 자모만 보내고 완성 음절을 잘랐다(사용자 보고 「상태」 → 「ㅅㅌ」).
 */
/** 같은 task에 keydown(229)·조합 사건이 오는 순서(Chromium·WebKit 데스크톱)로 「상태」를 친다 */
async function imeType(page) {
  await page.locator("textarea.xterm-helper-textarea").evaluate(async (el) => {
    const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
    const key = (type) => el.dispatchEvent(new KeyboardEvent(type, { key: "Process", keyCode: 229, which: 229, bubbles: true, cancelable: true }));
    const input = (d) => el.dispatchEvent(new InputEvent("input", { data: d, inputType: "insertCompositionText", isComposing: true, bubbles: true }));
    for (const syl of [["ㅅ", "사", "상"], ["ㅌ", "태"]]) {
      const base = el.value;
      for (const [i, d] of syl.entries()) {
        key("keydown");
        if (i === 0) el.dispatchEvent(new CompositionEvent("compositionstart", { data: "", bubbles: true }));
        el.dispatchEvent(new CompositionEvent("compositionupdate", { data: d, bubbles: true }));
        el.value = base + d;
        input(d);
        key("keyup");
        await sleep(40);
      }
      el.dispatchEvent(new CompositionEvent("compositionend", { data: syl.at(-1), bubbles: true }));
      await sleep(40);
    }
  });
}

// 아래 두 재생은 herdr-web-ui(MIT, aa624cb)의 scripts/terminal-safari-ime-regression.ts와
// scripts/terminal-ipad-ime-regression.ts가 기록한 실제 사건 순서를 그대로 옮긴 것이다(OS 입력기를 자동화하지 않는다).

/** macOS Safari 26.6.2(#432): 조합 사건 없이 DOM을 먼저 고치고 keydown(229)은 그 뒤에 온다. beforeinput 선택이 바꿀 범위다. */
async function safariEdit(page, type, data, start, end = start, keyCode = 229, order = "after") {
  await page.locator("textarea.xterm-helper-textarea").evaluate(
    async (box, { type, data, start, end, keyCode, order }) => {
      const key = keyCode === 8 ? "Backspace" : keyCode === 46 ? "Delete" : data;
      const keydown = () => box.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, cancelable: true, key, keyCode, isComposing: false }));
      if (order === "after" || keydown()) {
        box.setSelectionRange(start, end);
        box.dispatchEvent(new InputEvent("beforeinput", { bubbles: true, composed: true, inputType: type, data, isComposing: false }));
        box.setRangeText(data, start, end, "end");
        box.dispatchEvent(new InputEvent("input", { bubbles: true, composed: true, inputType: type, data, isComposing: false }));
        if (order === "after") keydown();
      }
      box.dispatchEvent(new KeyboardEvent("keyup", { bubbles: true, key, keyCode: keyCode === 229 ? 71 : keyCode, isComposing: false }));
      await new Promise((r) => requestAnimationFrame(() => r()));
    },
    { type, data, start, end, keyCode, order },
  );
}
async function safariSyllable(page, parts, at = 0) {
  await safariEdit(page, "insertText", parts[0], at);
  for (const part of parts.slice(1)) await safariEdit(page, "insertReplacementText", part, at, at + 1);
}

/** iPadOS Safari + 하드웨어 키보드: 자모마다 제 keyCode의 keydown·keypress, 뒤이어 편집기가 지우고 넣는다(조합 사건 없음) */
/** 사용자 기록(iOS 18.7 iPhone 홈 화면 앱, Safari 27.0.1, 화면 키보드 두벌식, 10-17 13:30): 자모마다 keydown(key는 자모,
 * code "Unidentified", keyCode 0, isComposing false) → keypress(keyCode·charCode는 자모 코드) → 편집기가 새 음절은 insertText,
 * 바뀐 음절은 deleteContentBackward 뒤 insertText로 고친다 → keyup. 조합 사건은 없다(iPad 하드웨어 키보드와 같은 꼴). */
async function iphoneKey(page, jamo, edits) {
  return page.locator("textarea.xterm-helper-textarea").evaluate(
    async (box, { jamo, edits }) => {
      const down = box.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, cancelable: true, key: jamo, code: "Unidentified", keyCode: 0, isComposing: false }));
      const press = down && box.dispatchEvent(new KeyboardEvent("keypress", { bubbles: true, cancelable: true, key: jamo, code: "Unidentified", keyCode: jamo.charCodeAt(0), charCode: jamo.charCodeAt(0) }));
      if (down && press) {
        for (const edit of edits) {
          const end = box.value.length;
          if (edit[0] === "delete") {
            box.dispatchEvent(new InputEvent("beforeinput", { bubbles: true, composed: true, inputType: "deleteContentBackward", isComposing: false }));
            box.setRangeText("", end - 1, end, "end");
            box.dispatchEvent(new InputEvent("input", { bubbles: true, composed: true, inputType: "deleteContentBackward", isComposing: false }));
          } else {
            box.dispatchEvent(new InputEvent("beforeinput", { bubbles: true, composed: true, inputType: "insertText", data: edit[1], isComposing: false }));
            box.setRangeText(edit[1], end, end, "end");
            box.dispatchEvent(new InputEvent("input", { bubbles: true, composed: true, inputType: "insertText", data: edit[1], isComposing: false }));
          }
        }
      }
      box.dispatchEvent(new KeyboardEvent("keyup", { bubbles: true, key: jamo, code: "Unidentified", keyCode: 0, isComposing: false }));
      await new Promise((r) => requestAnimationFrame(() => r()));
      return down;
    },
    { jamo, edits },
  );
}

const IPAD_KEYCODES = { "ㅎ": 71, "ㅏ": 75, "ㄴ": 83, "ㄱ": 82, "ㅡ": 77, "ㄹ": 70, "ㅇ": 68, "ㅁ": 65 };
async function ipadKey(page, jamo, edits) {
  return page.locator("textarea.xterm-helper-textarea").evaluate(
    async (box, { jamo, keyCode, edits }) => {
      const down = box.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, cancelable: true, key: jamo, code: "", keyCode, isComposing: false }));
      const press = down && box.dispatchEvent(new KeyboardEvent("keypress", { bubbles: true, cancelable: true, key: jamo, keyCode: jamo.charCodeAt(0), charCode: jamo.charCodeAt(0) }));
      if (down && press) {
        for (const edit of edits) {
          const end = box.value.length;
          if (edit[0] === "delete") {
            box.setSelectionRange(end - 1, end);
            box.dispatchEvent(new InputEvent("beforeinput", { bubbles: true, composed: true, inputType: "deleteContentBackward", isComposing: false }));
            box.setRangeText("", end - 1, end, "end");
            box.dispatchEvent(new InputEvent("input", { bubbles: true, composed: true, inputType: "deleteContentBackward", isComposing: false }));
          } else {
            box.setSelectionRange(end, end);
            box.dispatchEvent(new InputEvent("beforeinput", { bubbles: true, composed: true, inputType: "insertText", data: edit[1], isComposing: false }));
            box.setRangeText(edit[1], end, end, "end");
            box.dispatchEvent(new InputEvent("input", { bubbles: true, composed: true, inputType: "insertText", data: edit[1], isComposing: false }));
          }
        }
      }
      box.dispatchEvent(new KeyboardEvent("keyup", { bubbles: true, key: jamo, keyCode, isComposing: false }));
      await new Promise((r) => requestAnimationFrame(() => r()));
      return down;
    },
    { jamo, keyCode: IPAD_KEYCODES[jamo] ?? 65, edits },
  );
}
const ipadFresh = (page, jamo) => ipadKey(page, jamo, [["insert", jamo]]);

/**
 * 화면 키보드 흉내(herdr-web-ui scripts/keyboard-viewport-regression.ts처럼 플랫폼 입력만 가짜로): visualViewport·innerHeight·
 * 100lvh 자. pairPage의 init으로 넣고 softKeyboard(page, 보이는 높이)로 올리고 내린다.
 */
function fakeViewport() {
  const h = window.screen.height;
  const fake = { height: h, layout: h, large: h, scale: 1, viewport: new EventTarget() };
  Object.assign(window, { viewportFixture: fake });
  for (const key of ["height", "scale"]) Object.defineProperty(fake.viewport, key, { get: () => fake[key] });
  Object.defineProperty(fake.viewport, "width", { get: () => window.screen.width });
  Object.defineProperty(window, "visualViewport", { configurable: true, value: fake.viewport });
  Object.defineProperty(window, "innerHeight", { configurable: true, get: () => fake.layout });
  const nativeRect = HTMLElement.prototype.getBoundingClientRect;
  HTMLElement.prototype.getBoundingClientRect = function () {
    if (this.style.height === "100lvh") return new DOMRect(0, 0, 0, fake.large);
    if (this.style.height === "env(safe-area-inset-top, 0px)") return new DOMRect(0, 0, 0, 0);
    return nativeRect.call(this);
  };
}
/** 화면 키보드가 올라오거나(보이는 높이만 준다, iOS Safari) 내려간다 */
const softKeyboard = (page, visible) =>
  page.evaluate((h) => {
    const f = window.viewportFixture;
    f.height = h;
    f.viewport.dispatchEvent(new Event("resize"));
    window.dispatchEvent(new Event("resize"));
  }, visible);
const ipadReplace = (page, jamo, result) => ipadKey(page, jamo, [["delete"], ["insert", result]]);

// line: 이 기기의 입력 방식을 입력줄로 둔다(기본은 바로 입력, herdr-web-ui terminalInputMode)
// serviceWorkers: "block"이면 service worker 없이 연다(그 worker가 맡은 요청은 ctx.route를 거치지 않는다, plan §8.115)
// wsRoute·route: [주소, 처리]를 열기 전에 건다(Playwright routeWebSocket은 그 뒤에 만든 WebSocket만 가로채고, 글꼴은 연결 화면에서 이미 받는다)
/**
 * 목록 안의 것(pane·thread 줄, 바닥 줄의 설정·사용량)을 누른다. 768px 이하에서 목록은 pane 위의 drawer라(plan §8.146,
 * web/src/drawer.tsx) 닫혀 있으면 화면 머리의 「목록 열기」로 연 뒤 누른다. 넓은 창은 그대로 누른다.
 */
async function inList(p, target) {
  const item = typeof target === "string" ? p.getByTestId(target) : target;
  await item.waitFor({ state: "attached" });
  // 닫히는 중인 drawer의 항목은 움직임이 끝날 때까지 보인다: 끝난 뒤에 본다(transition의 finished)
  await drawerSettled(p);
  // 보고서·답할 것처럼 숨긴 채 그려 둔 화면의 단추도 DOM에는 남는다. 지금 보이는 화면의 단추만 누른다.
  const toggle = p.locator(".view [data-testid='drawer-toggle']:visible");
  if (!(await item.isVisible()) && (await toggle.isVisible()) && (await toggle.getAttribute("aria-expanded")) !== "true") {
    await toggle.click();
    await p.locator("#workspace-drawer.is-open").waitFor();
  }
  // 활동 막대(plan §8.240): 항목이 고르지 않은 패널이나 접은 사이드바에 있으면 그 패널 단추로 보인다(보이는 패널의 단추를 다시
  // 누르면 접히므로 숨었을 때만 누른다)
  const panel = await item.evaluate((el) => {
    const owner = el.closest("[data-panel]");
    const collapsed = innerWidth >= 769 && document.querySelector(".app.sidebar-collapsed") !== null;
    return owner && (owner.hidden || collapsed) ? owner.dataset.panel : null;
  });
  if (panel !== null) {
    await p.getByTestId(`activity-${panel}`).click();
    await p.locator(`#sidebar-column [data-panel="${panel}"]:not([hidden])`).waitFor();
  }
  if (!(await item.isVisible())) {
    // 기다려도 보이지 않을 자리다: 그때 화면을 말하고 끝낸다
    const at = await p.evaluate(() => ({
      view: document.querySelector(".view")?.className ?? null,
      heads: [...document.querySelectorAll(".view header")].map((h) => h.className),
      toggles: [...document.querySelectorAll("[data-testid='drawer-toggle']")].map((t) => `${t.closest("header")?.className}:${getComputedStyle(t).display}`),
      drawer: document.getElementById("workspace-drawer")?.className ?? null,
      hash: location.hash,
    }));
    throw new Error(`목록 항목이 보이지 않는다(${p.viewportSize()?.width}px): ${JSON.stringify(at)}`);
  }
  await item.click();
  // 줄을 고르면 drawer가 닫히며 미끄러진다. 그동안은 아직 pane 위에 있어 다음 누름을 가져가므로 끝날 때까지 기다린다
  await drawerSettled(p, true);
}

/** 활동 막대(plan §8.240): 넓은 창 사이드바에 그 패널을 보인다. 이미 보이면 누르지 않는다(보이는 패널의 단추를 다시 누르면 접힌다) */
async function showPanel(p, panel) {
  const tab = p.getByTestId(`activity-${panel}`);
  if ((await tab.getAttribute("aria-selected")) !== "true") await tab.click();
  await p.locator(`#sidebar-column [data-panel="${panel}"]:not([hidden])`).waitFor();
}

/** 목록 drawer의 움직임(transform·visibility transition)이 끝났다. nextFrame이면 방금 누른 것이 시작한 움직임까지 본다 */
function drawerSettled(p, nextFrame = false) {
  const settled = p.evaluate(async (nextFrame) => {
    if (nextFrame) await new Promise((r) => requestAnimationFrame(() => r()));
    await Promise.all((document.getElementById("workspace-drawer")?.getAnimations() ?? []).map((a) => a.finished.catch(() => null)));
  }, nextFrame);
  return inTime(settled, 20_000, "목록 서랍 움직임 끝(다음 frame)");
}

/** 그 이름의 pane을 가리키는 주소(pairPage `hash`) */
function paneHash(name) {
  return `#pane=${encodeURIComponent(aaa("pane", "list").panes.find((x) => x.name === name).id)}`;
}

/** 설정 창을 열고 그 쪽으로 간다(herdr-web-ui SettingsDialog: 넓은 창은 왼쪽 목록 옆에 쪽, 휴대폰은 목록에서 쪽으로) */
async function openSettings(p, pageName) {
  await inList(p, "settings-open");
  const dialog = p.getByRole("dialog", { name: "설정" });
  await dialog.waitFor();
  await dialog.getByRole("tab", { name: pageName }).click();
  await dialog.getByRole("tabpanel", { name: pageName }).waitFor();
  return dialog;
}

// setup(ctx): 화면을 열기 전에 context에 건다(lib/devices.mjs가 기기 기록 binding과 그 기기의 규칙을 건다)
// hash: 처음 여는 주소(`#pane=<id>`: 첫 화면이 그 pane이다, `paneHash`). 없으면 첫 화면은 기억한 것·첫 pane(App.tsx firstPick).
// onPage(page): 주소를 열기 전에(첫 화면이 여는 WebSocket까지 듣게)
// at: 실험실 주소 대신 열 주소(끝 `/`까지, 78-passkey의 `http://localhost:<port>/`). 없으면 base
async function pairPage(browser, code, viewport, { userAgent, platform, colorScheme, deviceScaleFactor, touch, init, line, wsRoute, route, serviceWorkers, setup, hash, onPage, at } = {}) {
  const ctx = await browser.newContext({
    viewport,
    isMobile: viewport.width < 700,
    hasTouch: touch ?? viewport.width < 700,
    ...(userAgent ? { userAgent } : {}),
    ...(colorScheme ? { colorScheme } : {}),
    ...(deviceScaleFactor ? { deviceScaleFactor } : {}),
    ...(serviceWorkers ? { serviceWorkers } : {}),
  });
  if (setup) await setup(ctx);
  if (platform) await ctx.addInitScript((p) => Object.defineProperty(navigator, "platform", { get: () => p }), platform);
  if (init) await ctx.addInitScript(init);
  // 모든 frame에서 돈다: sandbox frame(결과물 보기)은 localStorage를 읽기만 해도 던진다
  if (line)
    await ctx.addInitScript(() => {
      try {
        localStorage.setItem("aaa:terminal-input-mode", "line");
      } catch {}
    });
  // 터미널 진단 기록(termDiag가 읽는다): pane WebSocket의 글 메시지 종류·크기·순서, 받은 출력 묶음, 닫힘,
  // xterm viewport의 scroll 사건. CI에서만 난 실패의 순서를 보려고 둔다. 화면에는 손대지 않는다.
  // 보낸 것(sentFrames가 읽는다): pane WebSocket으로 보낸 ack·history 밖의 메시지 전부. 키·마우스·붙여넣기 처리는 그 사건 안에서
  // 바로 보내므로, 동작이 끝난 뒤 여기에 없으면 이 화면은 보내지 않았다(「보내지 않았다」를 sleep 없이 본다).
  await ctx.addInitScript(() => {
    const log = [];
    Object.defineProperty(window, "__aaaTermLog", { value: log });
    const sent = [];
    Object.defineProperty(window, "__aaaSent", { value: sent });
    const t0 = performance.now();
    // 받은 출력(bin)과 보낸 ACK(ack)는 다른 기록 사이마다 한 줄(io)로 묶는다(수가 많아 다른 기록을 밀어내지 않게)
    const push = (e) => {
      const at = Math.round(performance.now() - t0);
      const [kind, n] = e.split(" ");
      if (kind === "bin" || kind === "ack") {
        let io = log.at(-1);
        if (io?.kind !== "io") log.push((io = { kind: "io", at, bins: 0, bytes: 0, acks: 0, acked: 0 }));
        if (kind === "bin") {
          io.bins += 1;
          io.bytes += Number(n);
        } else {
          io.acks += 1;
          io.acked = Number(n);
        }
        return;
      }
      log.push({ at, e });
      if (log.length > 300) log.splice(0, log.length - 300);
    };
    const WS = window.WebSocket;
    window.WebSocket = class extends WS {
      constructor(...a) {
        super(...a);
        if (!String(a[0]).includes("/panes/")) return;
        this.pane = true;
        push("ws open");
        this.addEventListener("message", (m) => {
          if (typeof m.data !== "string") return push(`bin ${m.data.byteLength ?? m.data.size}`);
          const j = JSON.parse(m.data);
          push(j.t === "snapshot" ? `snapshot ${j.cols}x${j.rows} ${j.replay.length}` : j.t === "state" ? `state ${j.pane.cols}x${j.pane.rows}` : `msg ${j.t}`);
        });
        this.addEventListener("close", (e) => push(`ws close ${e.code}`));
      }
      send(d) {
        if (typeof d === "string") push(d.startsWith('{"t":"ack"') ? `ack ${JSON.parse(d).offset}` : `send ${d.slice(0, 50)}`);
        // 기록이 보내기를 막지 않는다(못 읽으면 그렇다고 남긴다)
        if (this.pane)
          try {
            if (typeof d !== "string") {
              const b = d instanceof ArrayBuffer ? new Uint8Array(d) : new Uint8Array(d.buffer, d.byteOffset, d.byteLength);
              sent.push({ t: "bin", data: String.fromCharCode(...b) });
            } else if (!/^\{"t":"(ack|history)"/.test(d)) sent.push(JSON.parse(d));
          } catch (e) {
            sent.push({ t: "unreadable", data: String(e) });
          }
        super.send(d);
      }
    };
    document.addEventListener(
      "scroll",
      (e) => {
        const t = e.target;
        if (t instanceof Element && t.classList.contains("xterm-viewport")) push(`vp ${t.scrollTop}/${t.scrollHeight - t.clientHeight}`);
      },
      true,
    );
  });
  // AAA_E2E_NO_LOCAL_FONT=1: 이 머신에 깔린 Meslotendard를 빼고 넣은 글꼴(WOFF2)만 쓴다(CI 러너·처음 여는 기기처럼).
  // 목록의 Meslotendard와 @font-face의 local()을 모두 뺀다(같은 이름의 @font-face를 뒤에 둔다). 기본은 끔
  if (process.env.AAA_E2E_NO_LOCAL_FONT)
    await ctx.addInitScript(() =>
      document.addEventListener("DOMContentLoaded", () => {
        const st = document.createElement("style");
        st.textContent = [400, 700]
          .map((w) => `@font-face{font-family:"Meslotendard Web";src:url("/fonts/meslotendard-0.3.1/Meslotendard-${w === 400 ? "Regular" : "Bold"}.woff2") format("woff2");font-weight:${w}}`)
          .concat(':root{--font-term:"Meslotendard Web", "JetBrains Mono", "SF Mono", ui-monospace, Menlo, monospace !important}')
          .join("\n");
        document.head.append(st);
      }),
    );
  // AAA_E2E_FONT_DELAY=ms: 넣은 글꼴(WOFF2)을 늦게 준다(CI 러너처럼 글꼴이 터미널을 연 뒤에 받아질 때). 기본은 끔
  if (process.env.AAA_E2E_FONT_DELAY)
    await ctx.route("**/fonts/**", async (route) => {
      await sleep(Number(process.env.AAA_E2E_FONT_DELAY));
      await route.continue();
    });
  if (wsRoute) await ctx.routeWebSocket(...wsRoute);
  if (route) await ctx.route(...route);
  const page = await ctx.newPage();
  openPages.add(page);
  page.on("close", () => openPages.delete(page));
  // AAA_E2E_THROTTLE=N: Chromium 화면을 N배 느리게(CI 러너처럼 느린 곳에서만 난 경합을 로컬에서 재현할 때). 기본은 끔
  if (process.env.AAA_E2E_THROTTLE && browser.browserType().name() === "chromium")
    await (await ctx.newCDPSession(page)).send("Emulation.setCPUThrottlingRate", { rate: Number(process.env.AAA_E2E_THROTTLE) });
  const errors = [];
  // 글과 stack 맨 위 두 자리(번들 안 file:줄:칸): 한 번만 난 화면 오류도 어디서 났는지 남긴다
  page.on("pageerror", (e) => errors.push([String(e), ...String(e.stack ?? "").split("\n").slice(1, 3).map((l) => l.trim())].join(" ")));
  if (onPage) onPage(page);
  await page.goto(`${at ?? base}${hash ?? ""}`);
  // 넣은 글꼴을 터미널을 열기 전에 받아 둔다(늘 쓰는 기기처럼 cache에 있는 상태). 늦게 받아지면 칸 높이·줄 수가 바뀌어
  // 그 단계가 재는 것(끌기 줄 수·크기)과 겹친다: CI 러너에는 깔린 글꼴이 없어 매번 늦게 받았다(41e81eb 휴대폰 끌기).
  // 늦게 받는 경우는 「글꼴이 늦게 와도 맨 아래」 단계가 따로 본다(route로 글꼴을 붙잡으므로 그때는 기다리지 않는다).
  if (!route) await page.evaluate(() => Promise.all([400, 700].map((w) => document.fonts.load(`${w} 13px "Meslotendard Web"`))));
  await page.getByLabel("페어링 코드").fill(code);
  await page.getByRole("button", { name: "이 기기 페어링" }).click();
  // 붙어서 목록을 받았다(좁은 창은 목록이 닫힌 drawer 안이라 보이지 않는다)
  await page.getByTestId("machine-labA").waitFor({ state: "attached" });
  return { page, errors };
}

// 가짜 push 서비스(Web Push, §8.26): host가 보낸 요청을 모은다. /gone/ 경로는 410(브라우저가 구독을 버림)을 돌려준다.
const pushes = [];
const pushServer = http.createServer((req, res) => {
  const chunks = [];
  req.on("data", (c) => chunks.push(c));
  req.on("end", () => {
    pushes.push({ path: req.url, headers: req.headers, body: Buffer.concat(chunks) });
    res.writeHead(req.url.startsWith("/gone/") ? 410 : 201).end();
  });
});
const pushPort = await new Promise((r) => pushServer.listen(0, "127.0.0.1", () => r(pushServer.address().port)));
const pushOrigin = `http://127.0.0.1:${pushPort}`;

const b64u = (b) => Buffer.from(b).toString("base64url");

/** 기기 하나(브라우저 대신): P-256 키와 auth 비밀 */
function pushDevice(name) {
  const ecdh = crypto.createECDH("prime256v1");
  ecdh.generateKeys();
  const auth = crypto.randomBytes(16);
  return { ecdh, auth, json: { endpoint: `${pushOrigin}/${name}`, keys: { p256dh: b64u(ecdh.getPublicKey()), auth: b64u(auth) } } };
}

/** RFC 8291 받는 쪽을 Node crypto로 따로 구현해 푼다(host 구현과 독립) */
function decryptPush(body, dev) {
  const salt = body.subarray(0, 16);
  const rs = body.readUInt32BE(16);
  const idlen = body[20];
  const asPub = body.subarray(21, 21 + idlen);
  const ct = body.subarray(21 + idlen);
  if (rs !== 4096 || idlen !== 65 || body.length > 4096) throw new Error(`머리 rs=${rs} idlen=${idlen} 길이=${body.length}`);
  const uaPub = dev.ecdh.getPublicKey();
  const secret = dev.ecdh.computeSecret(asPub);
  const info = Buffer.concat([Buffer.from("WebPush: info\0"), uaPub, asPub]);
  const ikm = Buffer.from(crypto.hkdfSync("sha256", secret, dev.auth, info, 32));
  const cek = Buffer.from(crypto.hkdfSync("sha256", ikm, salt, Buffer.from("Content-Encoding: aes128gcm\0"), 16));
  const nonce = Buffer.from(crypto.hkdfSync("sha256", ikm, salt, Buffer.from("Content-Encoding: nonce\0"), 12));
  const d = crypto.createDecipheriv("aes-128-gcm", cek, nonce);
  d.setAuthTag(ct.subarray(ct.length - 16));
  const pt = Buffer.concat([d.update(ct.subarray(0, ct.length - 16)), d.final()]);
  let i = pt.length - 1;
  while (i > 0 && pt[i] === 0) i--;
  if (pt[i] !== 2) throw new Error("마지막 레코드 구분(0x02)이 없다");
  return JSON.parse(pt.subarray(0, i).toString("utf8"));
}

/** VAPID(RFC 8292): JWT 서명을 공개키로 검증하고 claims를 돌려준다 */
function checkVapid(headers, key) {
  const m = /^vapid t=([^,]+), k=(\S+)$/.exec(headers.authorization ?? "");
  if (!m) throw new Error(`Authorization: ${headers.authorization}`);
  const [jwt, k] = [m[1], m[2]];
  if (k !== key) throw new Error("VAPID 공개키가 /api/v1/push의 키와 다르다");
  const [h, c, sig] = jwt.split(".");
  const raw = Buffer.from(k, "base64url");
  const pub = crypto.createPublicKey({
    key: { kty: "EC", crv: "P-256", x: b64u(raw.subarray(1, 33)), y: b64u(raw.subarray(33, 65)) },
    format: "jwk",
  });
  const ok = crypto.verify("sha256", Buffer.from(`${h}.${c}`), { key: pub, dsaEncoding: "ieee-p1363" }, Buffer.from(sig, "base64url"));
  if (!ok) throw new Error("JWT 서명이 맞지 않다");
  const header = JSON.parse(Buffer.from(h, "base64url"));
  if (header.alg !== "ES256") throw new Error(JSON.stringify(header));
  if (headers["content-encoding"] !== "aes128gcm" || !headers.ttl) throw new Error(JSON.stringify(headers));
  return JSON.parse(Buffer.from(c, "base64url"));
}

/** 그 기기 앞으로 온 push 하나를 풀어 내용을 본다 */
async function nextPush(dev, from, what) {
  const p = await until(what, () => pushes.slice(from).find((x) => x.path === new URL(dev.json.endpoint).pathname));
  return { msg: decryptPush(p.body, dev), claims: checkVapid(p.headers, pushKey), index: pushes.indexOf(p) };
}
let pushKey = "";

let base = "";
let failed = false;
const steps = [];
/** 열려 있는 화면(pairPage). 단계가 실패하면 화면마다 termDiag를 남긴다(CI에서만 난 실패가 여러 단계에 걸쳐 났다) */
const openPages = new Set();
// AAA_E2E_ONLY=<정규식>: 이름이 맞는 단계만 돈다(한 단계를 여러 번·부하 아래 되풀이해 볼 때). 준비 단계(`always`: host·agent 시작,
// push 판정 준비)와 단계 밖의 준비(첫 화면 연결)는 그대로 한다
const only = process.env.AAA_E2E_ONLY ? new RegExp(process.env.AAA_E2E_ONLY) : null;
/** 단계 하나의 시한. spec 하나를 다 돌아도 41초(durations.json 가장 긴 것)다. 단계 안 어느 대기가 시한 없이 남아도 이것으로
 * 실패하고, runSpec이 화면·host·agent를 내린다 */
const STEP_LIMIT_MS = 120_000;

async function step(name, fn, { always = false, diagnose } = {}) {
  if (only && !always && !only.test(name)) return;
  const t0 = Date.now();
  let limit;
  try {
    await Promise.race([
      fn(),
      new Promise((_, fail) => {
        limit = setTimeout(() => fail(new Error(`${STEP_LIMIT_MS}ms 안에 단계가 끝나지 않았다`)), STEP_LIMIT_MS);
      }),
    ]).finally(() => clearTimeout(limit));
    steps.push(`ok   ${name} (${Date.now() - t0}ms)`);
  } catch (e) {
    // viewport를 움직이는 기존 termDiag보다 먼저 실패 지점의 산출물을 저장한다.
    try { await diagnose?.(e); }
    catch (error) { console.log(`실패 진단 저장 오류: ${error}`); }
    const diags = [];
    for (const p of openPages) {
      const d = await Promise.race([termDiag(p).catch((x) => String(x)), sleep(5000).then(() => "시간 넘김")]);
      diags.push(`${p.viewportSize()?.width}px ${JSON.stringify(d)}`);
    }
    // 한 줄로 남긴다: CI annotation은 FAIL 줄만 옮긴다(ci-local.sh)
    steps.push(`FAIL ${name}: ${e.message}${diags.length ? ` ‖ 열린 화면: ${diags.join(" ‖ ")}` : ""}`);
    failed = true;
    throw e;
  }
}

fs.rmSync(root, { recursive: true, force: true });
fs.mkdirSync(`${root}/c`, { recursive: true });
if (shots) fs.mkdirSync(shots, { recursive: true });
// 실행 파일 이름이 claude인 셸: agent가 그 pane 화면을 Claude 규칙(herdr claude.toml)으로 보고 상태를 정한다(plan §8.110)
const claudeSh = `${root}/bin/claude`;
fs.mkdirSync(`${root}/bin`, { recursive: true });
fs.writeFileSync(claudeSh, '#!/bin/sh\nexec /bin/sh "$@"\n', { mode: 0o755 });
// Claude Code 화면 조각(sh 명령, 화면을 지우고 그린다). 판정은 herdr 원본 엔진으로 확인했다: 권한 확인창
// `bash_permission_prompt`(질문), spinner `live_turn_working`(작업 중), 입력 상자 `live_prompt_box`(idle)
const clearScreen = "printf '\\033[2J\\033[H'";
const claudeAsk = (cmd) =>
  `${clearScreen}; printf '%s\\n' ' Bash command' '' '   ${cmd}' '' ' Do you want to proceed?' ' ❯ 1. Yes' '   2. No' '' ' Esc to cancel · Tab to amend'`;
const claudeWorking = `${clearScreen}; printf '%s\\n' '✻ Working… (1s · esc to interrupt)'`;
const claudeIdle = `${clearScreen}; printf '%s\\n' '────────────────' '❯ ' '────────────────' '  ? for shortcuts'`;
// 가짜 Claude 질문 창(lib/fake-claude-ask.mjs): 이름이 claude라 화면 감지가 Claude 규칙으로 본다. `askClaude one|tabs`
const askClaude = `${root}/askbin/claude`;
fs.mkdirSync(`${root}/askbin`, { recursive: true });
fs.writeFileSync(askClaude, `#!${process.execPath}\n${fs.readFileSync(new URL("./fake-claude-ask.mjs", import.meta.url), "utf8")}`, { mode: 0o755 });
// 웹 port는 host가 고른다(listen 0, 「host·agent 시작」이 status로 받는다): 같이 도는 실험실끼리 겹치지 않는다
let webPort = 0;

// 구독 사용량(host usage.rs): 실험실 집과 가짜 구독 서버. host는 이 집만 읽고 keychain·gh·실제 구독 주소에 닿지 않는다
// (AAA_USAGE_HOME·AAA_USAGE_ORIGIN 시험 고리). 계정·token은 지어낸 값이다.
const usageHome = `${root}/usagehome`;
const b64url = (o) => Buffer.from(JSON.stringify(o)).toString("base64url");
const fakeJwt = (claims) => `${b64url({ alg: "none" })}.${b64url(claims)}.sig`;
const inHours = (h) => new Date(Date.now() + h * 3_600_000).toISOString();
fs.mkdirSync(`${usageHome}/.claude`, { recursive: true });
fs.mkdirSync(`${usageHome}/.codex`, { recursive: true });
fs.mkdirSync(`${usageHome}/.local/share/opencode`, { recursive: true });
fs.writeFileSync(
  `${usageHome}/.claude/.credentials.json`,
  JSON.stringify({ claudeAiOauth: { accessToken: "tok-claude", expiresAt: Date.now() + 86_400_000, subscriptionType: "max" } }),
);
fs.writeFileSync(`${usageHome}/.claude.json`, JSON.stringify({ oauthAccount: { accountUuid: "acct-1", emailAddress: "me@example.com" } }));
fs.writeFileSync(
  `${usageHome}/.codex/auth.json`,
  JSON.stringify({
    tokens: {
      access_token: fakeJwt({
        exp: Math.floor(Date.now() / 1000) + 86_400,
        "https://api.openai.com/auth": { chatgpt_plan_type: "pro", chatgpt_user_id: "user-1" },
        "https://api.openai.com/profile": { email: "me@example.com" },
      }),
      account_id: "ws-1",
    },
  }),
);
fs.writeFileSync(`${usageHome}/.local/share/opencode/auth.json`, JSON.stringify({ "opencode-go": { type: "api", key: "tok-oc" } }));
const usageHits = new Map();
const usageRoutes = {
  "/api/oauth/usage": () => [200, {}, { five_hour: { utilization: 20, resets_at: inHours(3) }, seven_day: { utilization: 95, resets_at: inHours(50) }, seven_day_sonnet: { utilization: 40, resets_at: inHours(50) } }],
  "/backend-api/wham/usage": () => [200, {}, { plan_type: "pro", rate_limit: { primary_window: { used_percent: 10, limit_window_seconds: 18000, reset_after_seconds: 7200 }, secondary_window: { used_percent: 73, limit_window_seconds: 604800, reset_after_seconds: 300000 } } }],
  // 늦추라고 한 구독: 처음부터 숫자가 없다. 10분 동안 다시 묻지 않아야 한다
  "/zen/go/v1/usage": () => [429, { "retry-after": "600" }, {}],
};
const usageServer = http.createServer((req, res) => {
  usageHits.set(req.url, (usageHits.get(req.url) ?? 0) + 1);
  const [status, headers, body] = usageRoutes[req.url]?.() ?? [404, {}, {}];
  res.writeHead(status, { "content-type": "application/json", ...headers });
  res.end(JSON.stringify(body));
});
await new Promise((r) => usageServer.listen(0, "127.0.0.1", r));
env.AAA_USAGE_HOME = usageHome;
env.AAA_USAGE_ORIGIN = `http://127.0.0.1:${usageServer.address().port}`;
fs.writeFileSync(
  `${root}/c/host.toml`,
  `schema = 1\n[host]\nagent_listen = ""\n[web]\nlisten = "127.0.0.1:0"\n[local_agent]\nmode = "none"\n` +
    `[projects]\nroot = "${root}/projects"\nallow_profiles = ["fake"]\nlibrary_copy_mb = 1\n` +
    `[post]\nburst = 100\nhourly = 100\ndaily = 100\n` +
    `[notify]\ndebounce_secs = 1\nkinds = ["report", "post"]\npush_hosts = ["127.0.0.1"]\n`,
);
// 가짜 thread agent: brief 안내를 받으면 Markdown 보고와 결과물(HTML·그림·복사 상한을 넘는 파일)을 쓴다.
// 결과물 HTML은 자기가 격리됐는지(부모 문서·cookie·API에 닿는지) body data에 적는다.
fs.writeFileSync(
  `${root}/pic.png`,
  Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFBQIAX8jx0gAAAABJRU5ErkJggg==", "base64"),
);
fs.writeFileSync(
  `${root}/index.html`,
  `<!doctype html><meta charset=utf-8><body><h1 id=t>결과 보고</h1><img id=pic src="pic.png"><script>
var r = document.body.dataset;
try { parent.document.title; r.parent = "open"; } catch (e) { r.parent = "blocked"; }
try { r.cookie = document.cookie === "" ? "none" : "visible"; } catch (e) { r.cookie = "blocked"; }
fetch("/api/v1/me").then(function (x) { r.api = String(x.status); }).catch(function () { r.api = "blocked"; });
</script>`,
);
// 결과물 안 하위 폴더의 Markdown: 위 폴더의 그림·페이지를 상대 경로로 가리킨다(앱 안 파일 보기, 차이 목록 R1)
fs.writeFileSync(`${root}/notes.md`, "# 결과 메모\n\n![점 그림](../pic.png)\n\n[결과 페이지](../index.html)\n");
// fake agent 보고: 실제 보고에 나오는 Markdown(표 안 code, 굵게 안 code, 링크, 번호 목록 안 목록, code block, 차이 목록 R1)
fs.writeFileSync(
  `${root}/report-fixture.md`,
  [
    "[완료] 맡긴 일과 확인 끝",
    "",
    "## Report",
    "- 웹 보기 **확인**: `index.html`",
    "- 착지 **`a1b2c3d`**(병합), 근거는 [참고 문서](https://example.com/doc)",
    // 보고서 안 파일 링크(plan §8.171): thread worktree 기준 문서 링크와 글 속 경로(줄 참조)
    "- 정리는 [작업 메모](notes/plan.md), 고친 곳은 src/app.ts:3",
    "",
    "| ID | 판정 | 근거 |",
    "| --- | :---: | ---: |",
    "| `T2` | 같음 | `prefs.rs` |",
    "| R1 | **일부** | 표와 `code` |",
    "| Q9 | 휴대폰 표 칸의 한글 낱말은 음절 사이에서 끊기지 않는다 | 경우마다 메모를 남긴다 |",
    "",
    "1. 첫째",
    "   - 안쪽 `하나`",
    "2. 둘째",
    "",
    "```sh",
    "cargo test -p aaa",
    "```",
    "",
    "## Next",
    "- 결과 확인",
    "",
  ].join("\n"),
);
fs.writeFileSync(
  `${root}/fake.sh`,
  `#!/bin/sh
printf 'fake> '
while IFS= read -r line; do
  case "$line" in
    post-probe\\ *) sh "\${line#post-probe }" ;;
    *brief.md*)
      dir=$(dirname "$(printf '%s' "$line" | sed 's/를 읽고.*//')")
      # 할 일에 「사람을 기다림」이 있으면 보고 없이 사람을 기다린다고만 알린다(host 묶음 확인 필요, 12 「프로젝트 머리 셈」)
      if grep -q '사람을 기다림' "$dir/brief.md"; then
        "${AA}" report --percent 50 --activity 'Waiting for you'
        printf 'fake> '
        continue
      fi
      # 할 일에 「모아 묻기 시험」이 있으면 구현 전에 모아 묻는 보고만 쓴다(brief 「시작할 때 묻기」, 17 「시작 질문」)
      if grep -q '모아 묻기 시험' "$dir/brief.md"; then
        printf '[결정 필요] 질문 1개: 배치\n\n## Report\n- 배치: A안(추천, 기존과 같음) · B안\n\n## Next\n- 배치를 정한다\n' > "$dir/report.md"
        echo asked
        printf 'fake> '
        continue
      fi
      # 「질문 카드 시험」: 보고 \`## Questions\`에 \`### N.\` 질문 둘(53 「질문 카드」, plan §8.329·§8.345). 1번은 본문(글·그림·표)과
      # 선택지 그림이 있다(그림은 보고서 옆 library). 답 메시지는 입력 줄로 화면에 남는다
      if grep -q '질문 카드 시험' "$dir/brief.md"; then
        mkdir -p "$dir/library" && cp "${root}/pic.png" "$dir/library/"
        printf '[결정 필요] 질문 2개: 로그인 화면\n\n## Report\n- 구현 전에 묻는다\n\n## Questions\n### 1. 로그인 단추 자리\n지금 화면은 단추가 **오른쪽 아래**다.\n\n![지금 화면](library/pic.png)\n\n| 안 | 차이 |\n| --- | --- |\n| A | 지금과 같음 |\n| B | 누르기 쉬움 |\n\n- A 오른쪽 아래 (추천) — 지금 화면과 같음\n  ![A안](library/pic.png)\n- B 가운데 큰 단추 — 휴대폰에서 누르기 쉬움\n  ![B안](library/pic.png)\n\n### 2. 비밀번호 눈 아이콘\n- A 넣는다 (추천)\n- B 넣지 않는다\n\n## Next\n- 답을 기다린다\n' > "$dir/report.md"
        echo asked-cards
        printf 'fake> '
        continue
      fi
      cp "${root}/index.html" "${root}/pic.png" "$dir/library/"
      mkdir -p "$dir/library/docs" && cp "${root}/notes.md" "$dir/library/docs/"
      head -c 2097152 /dev/urandom > "$dir/library/big.bin"
      cp "${root}/report-fixture.md" "$dir/report.md"
      wt=$(dirname "$(dirname "$dir")")
      mkdir -p "$wt/notes" "$wt/src"
      printf '# 작업 메모\n\n고친 까닭\n' > "$wt/notes/plan.md"
      printf 'const a = 1;\nconst b = 2;\nexport const fixed = a + b;\n' > "$wt/src/app.ts"
      echo done ;;
  esac
  printf 'fake> '
done
`,
);
// 파일 보기(16-file-viewer)가 여는 폴더: 이 밖은 열지 않는다(agent가 시작할 때 있는 폴더만 허용한다).
// thread 작업 폴더(work_root)는 보고서 안 파일 링크(12, plan §8.171)가 연다(기본 허용 폴더에도 work_root가 든다). 시작할 때 없는
// 폴더는 허용 폴더가 되지 않으므로 미리 만든다
fs.mkdirSync(`${root}/files`, { recursive: true });
fs.mkdirSync(`${root}/work`, { recursive: true });
fs.mkdirSync(`${root}/projects`, { recursive: true });
fs.writeFileSync(
  `${root}/c/agent.toml`,
  `schema = 1\n[agent]\nwork_root = "${root}/work"\nfile_roots = ["${root}/files", "${root}/work", "${root}/projects"]\n`,
);
fs.appendFileSync(`${root}/c/host.toml`, `\n[profiles.fake]\nagent = "custom"\ncommand = ["/bin/sh", "${root}/fake.sh"]\n`);
// bracketed paste를 켜는 입력창: 받은 조각을 JSON으로 그대로 보인다(composer 여러 줄·Enter 시험)
fs.writeFileSync(
  `${root}/paste.js`,
  `process.stdin.setRawMode(true);
process.stdout.write("\\x1b[?2004hpaste> ");
process.stdin.on("data", (d) => process.stdout.write("\\r\\nGOT:" + JSON.stringify(d.toString("utf8")) + "\\r\\npaste> "));
`,
);
// 계속 그리는 입력창(Claude Code의 상태 줄처럼): 100ms마다 마지막 줄을 다시 그리고 커서를 입력 자리로 되돌린다.
// 받은 조각은 paste.js처럼 JSON으로 보인다(조합 중에도 출력이 와서 화면 키보드가 터미널을 커서 줄로 옮긴다).
fs.writeFileSync(
  `${root}/spin.js`,
  `process.stdin.setRawMode(true);
// 프롬프트 아래 한 줄을 비워 두고(필요하면 화면을 올린다) 상태 줄은 그 줄에 그린다
const prompt = () => process.stdout.write("\\r\\n\\x1b[Apaste> ");
for (let i = 0; i < 80; i++) process.stdout.write("spin-line-" + i + "\\r\\n");
prompt();
// 크기가 바뀌면 입력 자리를 화면 맨 아래로 다시 둔다(Claude Code의 입력창처럼)
process.stdout.on("resize", () => {
  process.stdout.write("\\r\\x1b[2K" + "\\r\\n".repeat(process.stdout.rows));
  prompt();
});
let n = 0;
setInterval(() => process.stdout.write("\\x1b7\\x1b[B\\r\\x1b[2K* working " + (n++ % 10) + "\\x1b8"), 100);
process.stdin.on("data", (d) => {
  process.stdout.write("\\r\\x1b[B\\x1b[2K\\x1b[A\\r\\nGOT:" + JSON.stringify(d.toString("utf8")) + "\\r\\n");
  prompt();
});
`,
);
// 키 막대 시험 프로그램: 받은 조각을 일련번호와 JSON으로 보인다(화면이 밀려도 새 조각을 고른다).
// "K"를 받으면 화살표 모드(DECCKM)를 켜고 "J"면 끈다.
fs.writeFileSync(
  `${root}/bar.js`,
  `process.stdin.setRawMode(true);
process.stdout.write("bar> ");
let n = 0;
process.stdin.on("data", (d) => {
  const s = d.toString("utf8");
  if (s === "K") process.stdout.write("\\x1b[?1h");
  if (s === "J") process.stdout.write("\\x1b[?1l");
  process.stdout.write("\\r\\nGOT:" + ++n + ":" + JSON.stringify(s) + "\\r\\nbar> ");
});
`,
);
// 마우스 시험 프로그램: 마우스 보고(SGR)를 켜고 첫 줄에 고를 낱말을 쓴다. 받은 조각과 PTY 크기 바뀜을 일련번호로 보인다.
fs.writeFileSync(
  `${root}/mouse.js`,
  `process.stdin.setRawMode(true);
let n = 0;
process.stdout.write("\\x1b[?1000h\\x1b[?1006h\\x1b[2J\\x1b[Hpick-this-word\\r\\n");
process.stdout.on("resize", () => process.stdout.write("SIZE:" + ++n + ":" + process.stdout.columns + "\\r\\n"));
process.stdin.on("data", (d) => process.stdout.write("GOT:" + ++n + ":" + JSON.stringify(d.toString("utf8")) + "\\r\\n"));
`,
);
// 글자 폭 시험: 이모지·새 이모지(🫠)·ZWJ 가족·⚠️(VS16) 뒤에 글자 하나씩
// 스크롤 시험 프로그램: 번호 붙은 줄 200개를 쓰고 기다린다. alt면 alternate screen에서, mouse면 거기에 mouse 보고(1000·SGR 1006)를 켠다.
// "p"를 받으면 줄을 하나 더 쓰고, 다른 조각은 JSON으로 보인다(휠이 화살표·mouse 보고로 왔는지 본다).
fs.writeFileSync(
  `${root}/scroll.js`,
  `const mode = process.argv[2] || "plain";
process.stdin.setRawMode(true);
if (mode !== "plain") process.stdout.write("\\x1b[?1049h\\x1b[H");
if (mode === "mouse") process.stdout.write("\\x1b[?1000h\\x1b[?1006h");
for (let i = 1; i <= 200; i++) process.stdout.write("s-" + String(i).padStart(3, "0") + "\\r\\n");
let n = 0;
process.stdin.on("data", (d) => {
  const s = d.toString("latin1");
  if (s === "p") process.stdout.write("more-" + ++n + "\\r\\n");
  else process.stdout.write("GOT:" + JSON.stringify(s) + "\\r\\n");
});
`,
);
// 출력 흐름 시험 프로그램: "f"를 받으면 100바이트 줄 4000개(400KB)를 한 번에 쓴다. 다른 조각은 JSON으로 보인다.
fs.writeFileSync(
  `${root}/flood.js`,
  `process.stdin.setRawMode(true);
process.stdout.write("flood> ");
process.stdin.on("data", (d) => {
  const s = d.toString("latin1");
  if (s !== "f") return void process.stdout.write("\\r\\nGOT:" + JSON.stringify(s) + "\\r\\nflood> ");
  let out = "\\r\\n";
  for (let i = 1; i <= 4000; i++) out += "fl-" + String(i).padStart(5, "0") + " " + "x".repeat(88) + "\\r\\n";
  process.stdout.write(out + "flood-done\\r\\nflood> ");
});
`,
);
// 앞 기록 시험 프로그램: "h"를 받으면 번호 붙은 줄 6000개(열 줄마다 색)를 한 번에 쓴다(plan §8.67). "p"면 tick 줄 하나
fs.writeFileSync(
  `${root}/history.js`,
  `process.stdin.setRawMode(true);
process.stdout.write("\\x1b[?2004hhist> ");
let ticks = 0;
process.stdin.on("data", (d) => {
  if (d.toString("latin1") === "p") return void process.stdout.write("\\r\\ntick-" + ++ticks + "\\r\\nhist> ");
  // 짧은 줄 2600개(QA t-0337 probe-stuck처럼 앞 기록과 화면이 한 번에 읽히는 크기)
  if (d.toString("latin1") === "q") {
    let out = "\\r\\n";
    for (let i = 1; i <= 2600; i++) out += "q-" + String(i).padStart(4, "0") + "\\r\\n";
    return void process.stdout.write(out + "short-done\\r\\nhist> ");
  }
  if (d.toString("latin1") !== "h") return void process.stdout.write("\\r\\nGOT:" + JSON.stringify(d.toString("utf8")) + "\\r\\nhist> ");
  let out = "\\r\\n";
  for (let i = 1; i <= 6000; i++) {
    const n = "h-" + String(i).padStart(5, "0");
    out += (i % 10 === 0 ? "\\x1b[3" + (i % 7) + "m" + n + "\\x1b[m" : n) + " " + "y".repeat(i % 40) + "\\r\\n";
  }
  process.stdout.write(out + "history-done\\r\\nhist> ");
});
`,
);
fs.writeFileSync(`${root}/widths.js`, 'process.stdout.write("\\x1b[2J\\x1b[H😀X\\r\\n🫠Y\\r\\n👨‍👩‍👧Z\\r\\n⚠️W\\r\\n"); setInterval(() => {}, 1e9);\n');
// 키 처리 시험 프로그램: 시작할 때 modifyOtherKeys 2를 요청하고(Claude Code처럼), 받은 조각을 JSON으로 보인다.
// "n"을 받으면 modifyOtherKeys를 끄고, "m"이면 다시 켜고, "o"면 OSC 52로 클립보드에 쓴다.
fs.writeFileSync(
  `${root}/keys.js`,
  `process.stdin.setRawMode(true);
process.stdout.write("\\x1b[>4;2mkeys> ");
process.stdin.on("data", (d) => {
  const s = d.toString("utf8");
  if (s === "n") process.stdout.write("\\x1b[>4n");
  if (s === "m") process.stdout.write("\\x1b[>4;2m");
  if (s === "o") process.stdout.write("\\x1b]52;c;" + Buffer.from("클립 52").toString("base64") + "\\x07");
  process.stdout.write("\\r\\nGOT:" + JSON.stringify(s) + "\\r\\nkeys> ");
});
`,
);
const browser = await chromium.launch();

/** push 서명 키(「Web Push: 구독 검사」 단계·pushLab이 /api/v1/push에서 받는다). spec은 import한 이름에 쓸 수 없어 이걸로 둔다 */
const setPushKey = (key) => {
  pushKey = key;
};

// 쓰는 화면이 있으면 host는 Web Push를 보내지 않는다(herdr-web-ui presence, plan §8.69). headless 화면은 늘 focus가 있으므로,
// 보고 알림이 기기에 가는 것을 볼 동안 그 화면은 쓰지 않는 것으로 둔다(focus만 없앤다)
const setInUse = (p, on) =>
  p.evaluate((on) => {
    if (on) delete document.hasFocus;
    else document.hasFocus = () => false;
    window.dispatchEvent(new Event(on ? "focus" : "blur"));
  }, on);
/** host가 세는 쓰는 화면 수(그 화면으로 묻는다) */
const pushInUse = (p) => p.evaluate(async () => (await (await fetch("/api/v1/push")).json()).ok.in_use);
/** 그 화면 기기가 본 쓰는 기기(plan §8.105): `user`(앞 7자)·`you` */
const pushWho = (p) => p.evaluate(async () => (await (await fetch("/api/v1/push")).json()).ok);

/**
 * push 판정 단계의 준비(17-push-in-use·18-push-stalled): 서명 키, 첫 화면 기기의 구독 하나(push/dev1), demo 프로젝트.
 * 쓰는 화면이 없을 때 보낸 push가 이 구독으로 가므로 「push 0건」·「push 보냄」 단언이 빈말이 아니다. 12에서는 앞 단계(구독 검사·
 * 보고서)가 남기던 것이라, 단계를 하나만 골라(--grep) 돌아도 늘 한다
 */
async function pushLab(page) {
  await step(
    "push 판정 준비",
    async () => {
      const info = await pushWho(page);
      if (!info.enabled) throw new Error(`push가 꺼져 있다: ${JSON.stringify(info)}`);
      setPushKey(info.key);
      const dev = pushDevice("push/dev1");
      const status = await page.evaluate(
        async (body) => (await fetch("/api/v1/push/subscribe", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) })).status,
        dev.json,
      );
      if (status !== 200) throw new Error(`구독 ${status}`);
      aaa("project", "create", "demo");
    },
    { always: true },
  );
}

/** 실제 coordinator pane 자식 CLI로 사용자 post를 보낸다. 웹·밖 CLI가 coordinator를 자칭하지 않는다. 기본은 전체 완료,
 * `request`면 확인 필요(제목은 질문 요약·추천) */
async function coordinatorPost({ request = false, title = "전체 완료", body } = {}) {
  const opened = aaa("project", "open", "demo", "--profile", "fake");
  const coord = opened.pane.id;
  await waitOutput(coord, "fake>");
  const id = crypto.randomBytes(16).toString("hex");
  const file = `${root}/post-${id}`;
  const quote = (s) => `'${s.replaceAll("'", "'\\''")}'`;
  fs.writeFileSync(`${file}.body`, body ?? id);
  const kind = request ? "--request --needs-user --topic user-action" : "--notice --topic task-complete";
  fs.writeFileSync(`${file}.sh`, `${quote(AA)} --json post user ${kind} --title ${quote(title)} --body-file ${quote(`${file}.body`)} --post-id ${id} > ${quote(`${file}.out`)} 2>&1\nprintf 'post-${id}-done\\n'\n`);
  aaa("pane", "run", coord, `post-probe ${file}.sh`);
  await waitOutput(coord, `post-${id}-done`);
  const result = JSON.parse(fs.readFileSync(`${file}.out`, "utf8"));
  if (result.state !== "accepted") throw new Error(`coordinator post: ${JSON.stringify(result)}`);
  // 이 도우미가 연 pane만 정리한다. 뒤의 coordinator 열기 시나리오에 상태를 남기지 않는다.
  if (opened.started) {
    aaa("pane", "kill", coord);
    await until("coordinator 세션 종료", () => aaa("pane", "list").panes.find((p) => p.id === coord)?.state !== "running");
    aaa("pane", "forget", coord);
  }
  return result;
}

/**
 * spec 하나를 돈다: host·agent와 셸 탭 t1을 띄우고, 첫 화면(1280×800, 입력줄)을 t1에 연 뒤 spec의 단계를 차례로,
 * 끝나면 띄운 것을 모두 내리고 단계 결과를 찍는다. t1에는 표지 `t1-42`를 찍는다: 단계는 t1로 돌아온 것을 이 글로 본다.
 * `labEnv`는 그 spec 파일만 실험실(host·agent·CLI)에 더하는 환경이다(spec의 `export const labEnv`, 예: 18의 짧은 lease).
 */
async function runSpec(spec, labEnv = {}) {
  try {
    await step("host·agent 시작", async () => {
      Object.assign(env, labEnv);
      start("host", "host");
      webPort = Number((await until("host 준비", () => aaa("status").web_listen)).split(":").pop());
      base = `http://127.0.0.1:${webPort}/`;
      // 다시 띄우는 host(07 「host를 다시 띄운 뒤」·13 「host 재시작」)는 같은 port로 뜬다: 열린 화면이 그 주소로 다시 붙는다
      const toml = `${root}/c/host.toml`;
      fs.writeFileSync(toml, fs.readFileSync(toml, "utf8").replace('listen = "127.0.0.1:0"', `listen = "127.0.0.1:${webPort}"`));
      start("agent", "agent", "--host", "local", "--name", "labA");
      await until("labA 연결", () => aaa("machine", "list").machines.some((m) => m.name === "labA" && m.connection === "connected"));
      aaa("tab", "create", "--machine", "labA", "--label", "t1", "--", "/bin/sh");
    }, { always: true });
    // 입력줄을 쓰는 단계가 많아 이 화면은 입력줄로 둔다. 기본(바로 입력)은 「입력 방식」 단계가 본다.
    const ctl = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 }, { line: true });
    const page = ctl.page;
    aaa("pane", "run", "t1", "echo t1-$((40+2))");
    await waitOutput("t1", "t1-42");
    await page.getByTestId("pane-t1").click();
    await until("t1 화면", async () => (await screenText(page)).includes("t1-42"));
    await spec({ ctl, page });
    if (ctl.errors.length) throw new Error(`페이지 오류: ${ctl.errors.join("; ")}`);
  } catch (e) {
    failed = true;
    if (!steps.some((s) => s.startsWith("FAIL"))) steps.push(`FAIL ${e.message}`);
  } finally {
    await browser.close();
    pushServer.close();
    usageServer.close();
    for (const tag of [...procs.keys()]) await stop(tag);
    await killSessionHosts();
    await terminateDeployGuards(`${root}/s`);
    console.log(steps.join("\n"));
    if (failed) {
      console.log(`실험실 기록을 남겼다: ${root}`);
      const diagnostics = path.join(root, "diagnostics");
      if (env.AAA_E2E_ARTIFACTS && fs.existsSync(diagnostics)) {
        const target = path.join(env.AAA_E2E_ARTIFACTS, path.basename(root));
        fs.cpSync(diagnostics, target, { recursive: true });
        console.log(`E2E 실패 산출물: ${target}`);
      }
      process.exitCode = 1;
    } else {
      fs.rmSync(root, { recursive: true, force: true });
    }
  }
}

export {
  LOAD_TIMEOUT_MS,
  inList,
  showPanel,
  drawerSettled,
  inTime,
  STEP_LIMIT_MS,
  paneHash,
  require, chromium, webkit, repo, AA, shots, root, env, procs, sleep, aaa, waitOutput, start, stop, until, domIs,
  hashIs, freePort, killSessionHosts, screenText, termDiag, pasteFile, bannerPath, shot, termFont, fontOk, imeType,
  safariEdit, safariSyllable, iphoneKey, IPAD_KEYCODES, ipadKey, ipadFresh, fakeViewport, softKeyboard,
  ipadReplace, pairPage, pushes, pushServer, pushPort, pushOrigin, b64u, pushDevice, decryptPush, checkVapid,
  nextPush, pushKey, base, failed, steps, openPages, only, step, webPort, usageHome, b64url, fakeJwt, inHours,
  usageHits, usageRoutes, usageServer, browser, setPushKey, setInUse, pushInUse, pushWho, pushLab, coordinatorPost, runSpec, openSettings, focusIs, sentFrames, sentCount, sentText,
  claudeSh, clearScreen, claudeAsk, claudeWorking, claudeIdle, askClaude,
};

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
};
const procs = new Map();

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

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

function start(tag, ...args) {
  const log = fs.openSync(`${root}/${tag}.log`, "w");
  const p = spawn(AA, args, { env, stdio: ["ignore", log, log] });
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
const domIs = (p, what, fn, arg, ms = 20_000) =>
  p.evaluate(`new Promise((done, fail) => {
    const ok = () => { try { return Boolean((${fn})(${JSON.stringify(arg)})); } catch { return false; } };
    if (ok()) return done();
    const mo = new MutationObserver(() => { if (ok()) { clearTimeout(stuck); mo.disconnect(); done(); } });
    const stuck = setTimeout(() => { mo.disconnect(); fail(new Error(${JSON.stringify(`${ms}ms 안에 ${what}이 되지 않았다`)})); }, ${ms});
    mo.observe(document, { subtree: true, childList: true, attributes: true, characterData: true });
  })`);
const hashIs = (p, hash, ms = 20_000) =>
  p.evaluate(
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
  );

// focus 기다림: focusin 사건으로 끝낸다(focus는 DOM을 바꾸지 않아 MutationObserver로는 못 본다)
const focusIs = (p, what, sel, ms = 20_000) =>
  p.evaluate(`new Promise((done, fail) => {
    const ok = () => document.activeElement?.matches(${JSON.stringify(sel)}) === true;
    if (ok()) return done();
    const on = () => { if (ok()) { clearTimeout(stuck); removeEventListener("focusin", on, true); done(); } };
    const stuck = setTimeout(() => { removeEventListener("focusin", on, true); fail(new Error(${JSON.stringify(`${ms}ms 안에 ${what}에 focus가 오지 않았다`)} + ": " + (document.activeElement?.outerHTML ?? "").slice(0, 160))); }, ${ms});
    addEventListener("focusin", on, true);
  })`);

function freePort() {
  return new Promise((resolve) => {
    const s = net.createServer();
    s.listen(0, "127.0.0.1", () => {
      const { port } = s.address();
      s.close(() => resolve(port));
    });
  });
}

/** session host는 setsid로 분리돼 있다. meta.json의 PID 중 명령줄이 이 실험실을 가리키는 것만 끝낸다. */
function killSessionHosts() {
  const dir = `${root}/s/agent/sessions`;
  if (!fs.existsSync(dir)) return;
  for (const id of fs.readdirSync(dir)) {
    try {
      const meta = JSON.parse(fs.readFileSync(`${dir}/${id}/meta.json`, "utf8"));
      const cmd = execFileSync("ps", ["-o", "command=", "-p", String(meta.host_pid)], { encoding: "utf8" });
      if (cmd.includes(`${dir}/${id}`)) process.kill(meta.host_pid, "SIGKILL");
    } catch {
      // 이미 끝났다
    }
  }
}

const screenText = (page) => page.locator(".xterm-rows").innerText();
/**
 * CI에서만 터미널 글이 비어 보인 실패(10-17 dbacbfe·372e85d·69224fd·e784eac: snapshot은 왔는데 줄이 비었다)를 가리는 기록:
 * 그때 줄·칸 자리·가림을 남기고, 다시 그리기를 한 번 재촉한 뒤 글이 생기는지도 본다(생기면 그리기가 멈춰 있던 것이다).
 * eeee514에서는 글이 맨 아래보다 다섯 줄 위에서 멈춰 보였다(기록 높이는 맞다): viewport 자리와 pane WebSocket 메시지 순서(log)를 더 남긴다.
 * 실패를 넘기지 않는다: 이 값은 오류 글에만 들어간다.
 */
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
const shot = async (page, name) => shots && (await page.screenshot({ path: path.join(shots, `${name}.png`) }));

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
// wsRoute·route: [주소, 처리]를 열기 전에 건다(Playwright routeWebSocket은 그 뒤에 만든 WebSocket만 가로채고, 글꼴은 연결 화면에서 이미 받는다)
/** 설정 창을 열고 그 쪽으로 간다(herdr-web-ui SettingsDialog: 넓은 창은 왼쪽 목록 옆에 쪽, 휴대폰은 목록에서 쪽으로) */
async function openSettings(p, pageName) {
  await p.getByTestId("settings-open").click();
  const dialog = p.getByRole("dialog", { name: "설정" });
  await dialog.waitFor();
  await dialog.getByRole("tab", { name: pageName }).click();
  await dialog.getByRole("tabpanel", { name: pageName }).waitFor();
  return dialog;
}

async function pairPage(browser, code, viewport, { userAgent, platform, colorScheme, deviceScaleFactor, touch, init, line, wsRoute, route } = {}) {
  const ctx = await browser.newContext({
    viewport,
    isMobile: viewport.width < 700,
    hasTouch: touch ?? viewport.width < 700,
    ...(userAgent ? { userAgent } : {}),
    ...(colorScheme ? { colorScheme } : {}),
    ...(deviceScaleFactor ? { deviceScaleFactor } : {}),
  });
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
  await ctx.addInitScript(() => {
    const log = [];
    Object.defineProperty(window, "__aaaTermLog", { value: log });
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
  page.on("pageerror", (e) => errors.push(String(e)));
  await page.goto(base);
  // 넣은 글꼴을 터미널을 열기 전에 받아 둔다(늘 쓰는 기기처럼 cache에 있는 상태). 늦게 받아지면 칸 높이·줄 수가 바뀌어
  // 그 단계가 재는 것(끌기 줄 수·크기)과 겹친다: CI 러너에는 깔린 글꼴이 없어 매번 늦게 받았다(41e81eb 휴대폰 끌기).
  // 늦게 받는 경우는 「글꼴이 늦게 와도 맨 아래」 단계가 따로 본다(route로 글꼴을 붙잡으므로 그때는 기다리지 않는다).
  if (!route) await page.evaluate(() => Promise.all([400, 700].map((w) => document.fonts.load(`${w} 13px "Meslotendard Web"`))));
  await page.getByLabel("페어링 코드").fill(code);
  await page.getByRole("button", { name: "이 기기 페어링" }).click();
  await page.getByTestId("machine-labA").waitFor();
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
// AAA_E2E_ONLY=<정규식>: 이름이 맞는 단계만 돈다(한 단계를 여러 번·부하 아래 되풀이해 볼 때). 준비 단계(host·agent 시작)와
// 단계 밖의 준비(첫 화면 연결)는 그대로 한다
const only = process.env.AAA_E2E_ONLY ? new RegExp(process.env.AAA_E2E_ONLY) : null;
async function step(name, fn) {
  if (only && !only.test(name) && name !== "host·agent 시작") return;
  const t0 = Date.now();
  try {
    await fn();
    steps.push(`ok   ${name} (${Date.now() - t0}ms)`);
  } catch (e) {
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
// 실행 파일 이름이 claude인 셸: agent가 그 pane 화면을 Claude 규칙(herdr claude.toml)으로 보고 상태를 정한다(plan §8.105)
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
const webPort = await freePort();

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
  `schema = 1\n[host]\nagent_listen = ""\n[web]\nlisten = "127.0.0.1:${webPort}"\n[local_agent]\nmode = "none"\n` +
    `[projects]\nroot = "${root}/projects"\nallow_profiles = ["fake"]\nlibrary_copy_mb = 1\n` +
    `[notify]\ndebounce_secs = 1\nkinds = ["report"]\npush_hosts = ["127.0.0.1"]\n`,
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
// fake agent 보고: 실제 보고에 나오는 Markdown(표 안 code, 굵게 안 code, 링크, 번호 목록 안 목록, code block, 차이 목록 R1)
fs.writeFileSync(
  `${root}/report-fixture.md`,
  [
    "## Report",
    "- 웹 보기 **확인**: `index.html`",
    "- 착지 **`a1b2c3d`**(병합), 근거는 [참고 문서](https://example.com/doc)",
    "",
    "| ID | 판정 | 근거 |",
    "| --- | :---: | ---: |",
    "| `T2` | 같음 | `prefs.rs` |",
    "| R1 | **일부** | 표와 `code` |",
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
    *brief.md*)
      dir=$(dirname "$(printf '%s' "$line" | sed 's/를 읽고.*//')")
      cp "${root}/index.html" "${root}/pic.png" "$dir/library/"
      head -c 2097152 /dev/urandom > "$dir/library/big.bin"
      cp "${root}/report-fixture.md" "$dir/report.md"
      echo done ;;
  esac
  printf 'fake> '
done
`,
);
fs.writeFileSync(
  `${root}/c/agent.toml`,
  `schema = 1\n[agent]\nwork_root = "${root}/work"\n[profiles.fake]\nharness = "custom"\ncommand = ["/bin/sh", "${root}/fake.sh"]\n`,
);
// Codex처럼 구는 입력창: 글과 Enter가 한 덩어리로 오면 붙여넣기로 보고 Enter를 줄바꿈으로 넣는다(실측한 Codex 동작).
// Enter만 따로 오면 그때까지의 글을 제출한다.
fs.writeFileSync(
  `${root}/burst.js`,
  `process.stdin.setRawMode(true);
let buf = "";
process.stdout.write("burst> ");
process.stdin.on("data", (d) => {
  const s = d.toString("utf8");
  if (s === "\\r") { process.stdout.write("\\r\\nSUBMIT:" + buf + "\\r\\nburst> "); buf = ""; return; }
  if (s.length > 1 && s.endsWith("\\r")) { buf += s.slice(0, -1) + "\\n"; process.stdout.write("[newline]"); return; }
  buf += s;
  process.stdout.write(s);
});
`,
);
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
base = `http://127.0.0.1:${webPort}/`;
const browser = await chromium.launch();

/** push 서명 키(「Web Push: 구독 검사」 단계가 /api/v1/push에서 받는다). spec은 import한 이름에 쓸 수 없어 이걸로 둔다 */
const setPushKey = (key) => {
  pushKey = key;
};

/**
 * spec 하나를 돈다: host·agent와 셸 탭 t1을 띄우고, 첫 화면(1280×800, 입력줄)을 t1에 연 뒤 spec의 단계를 차례로,
 * 끝나면 띄운 것을 모두 내리고 단계 결과를 찍는다. t1에는 표지 `t1-42`를 찍는다: 단계는 t1로 돌아온 것을 이 글로 본다.
 */
async function runSpec(spec) {
  try {
    await step("host·agent 시작", async () => {
      start("host", "host");
      await until("host 준비", () => aaa("status").web_listen);
      start("agent", "agent", "--host", "local", "--name", "labA");
      await until("labA 연결", () => aaa("machine", "list").machines.some((m) => m.name === "labA" && m.connection === "connected"));
      aaa("tab", "create", "--machine", "labA", "--label", "t1", "--", "/bin/sh");
    });
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
    killSessionHosts();
    console.log(steps.join("\n"));
    if (failed) {
      console.log(`실험실 기록을 남겼다: ${root}`);
      process.exitCode = 1;
    } else {
      fs.rmSync(root, { recursive: true, force: true });
    }
  }
}

export {
  require, chromium, webkit, repo, AA, shots, root, env, procs, sleep, aaa, waitOutput, start, stop, until, domIs,
  hashIs, freePort, killSessionHosts, screenText, termDiag, pasteFile, bannerPath, shot, termFont, fontOk, imeType,
  safariEdit, safariSyllable, iphoneKey, IPAD_KEYCODES, ipadKey, ipadFresh, fakeViewport, softKeyboard,
  ipadReplace, pairPage, pushes, pushServer, pushPort, pushOrigin, b64u, pushDevice, decryptPush, checkVapid,
  nextPush, pushKey, base, failed, steps, openPages, only, step, webPort, usageHome, b64url, fakeJwt, inHours,
  usageHits, usageRoutes, usageServer, browser, setPushKey, runSpec, openSettings, focusIs, claudeSh, clearScreen,
  claudeAsk, claudeWorking, claudeIdle,
};

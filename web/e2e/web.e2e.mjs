// 웹 front E2E: 실제 aaa host·agent를 띄우고 headless Chromium으로 pairing·보기·입력·권한·끊김을 확인한다.
//
// 실행(저장소 루트): cargo build -p aaa && (cd web && npm run build) && cargo build -p aaa
//                    NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs [스크린샷 폴더]
// AAA_BIN으로 실행 파일을 바꿀 수 있다. 실패하면 실험실 폴더를 남기고 종료 코드 1로 끝난다.

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

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const AA = process.env.AAA_BIN ?? path.join(repo, "target/debug/aaa");
const shots = process.argv[2] ?? null;
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

try {
  await step("host·agent 시작", async () => {
    start("host", "host");
    await until("host 준비", () => aaa("status").web_listen);
    start("agent", "agent", "--host", "local", "--name", "labA");
    await until("labA 연결", () => aaa("machine", "list").machines.some((m) => m.name === "labA" && m.connection === "connected"));
    aaa("tab", "create", "--machine", "labA", "--label", "t1", "--", "/bin/sh");
  });

  await step("인증 없는 요청·다른 Origin 거부", async () => {
    const r1 = await fetch(`${base}api/v1/call`, {
      method: "POST",
      headers: { "content-type": "application/json", origin: base.slice(0, -1) },
      body: JSON.stringify({ op: "panes" }),
    });
    if (r1.status !== 401) throw new Error(`인증 없는 call이 ${r1.status}`);
    const r2 = await fetch(`${base}api/v1/pair`, {
      method: "POST",
      headers: { "content-type": "application/json", origin: "https://evil.example" },
      body: JSON.stringify({ code: "000000" }),
    });
    if (r2.status !== 403) throw new Error(`다른 Origin pair가 ${r2.status}`);
    // fetch는 Host 헤더를 바꾸지 못한다(DNS rebinding 흉내)
    const r3 = await new Promise((resolve, reject) =>
      http.get({ host: "127.0.0.1", port: webPort, path: "/", headers: { host: "evil.example" } }, resolve).on("error", reject),
    );
    r3.resume();
    if (r3.statusCode !== 421) throw new Error(`다른 Host가 ${r3.statusCode}`);
  });

  await step("PWA: manifest·아이콘을 인증 없이 받고 브라우저가 읽는다", async () => {
    const m = await fetch(`${base}manifest.webmanifest`);
    if (m.status !== 200 || !m.headers.get("content-type").startsWith("application/manifest+json")) {
      throw new Error(`manifest ${m.status} ${m.headers.get("content-type")}`);
    }
    const man = await m.json();
    if (man.id !== "/" || man.name !== "aaa" || man.display !== "standalone" || man.start_url !== "/") throw new Error(JSON.stringify(man));
    // 참고(herdr-web-ui manifest)처럼 일반·maskable을 192·512 모두(차이 목록 M4)
    const kinds = man.icons.map((i) => `${i.purpose} ${i.sizes}`).sort();
    if (JSON.stringify(kinds) !== JSON.stringify(["any 192x192", "any 512x512", "maskable 192x192", "maskable 512x512"])) throw new Error(`manifest 아이콘 ${kinds}`);
    for (const icon of [...man.icons, { src: "/apple-touch-icon.png" }, { src: "/favicon-32.png" }]) {
      const r = await fetch(new URL(icon.src, base));
      const buf = Buffer.from(await r.arrayBuffer());
      // PNG 서명(89 50 4E 47), 그리고 IHDR의 폭·높이가 적은 크기와 같다
      if (r.status !== 200 || r.headers.get("content-type") !== "image/png" || buf.readUInt32BE(0) !== 0x89504e47) {
        throw new Error(`아이콘 ${icon.src}: ${r.status} ${r.headers.get("content-type")}`);
      }
      if (icon.sizes && icon.sizes !== `${buf.readUInt32BE(16)}x${buf.readUInt32BE(20)}`) throw new Error(`아이콘 ${icon.src} 크기 ${buf.readUInt32BE(16)}x${buf.readUInt32BE(20)} ≠ ${icon.sizes}`);
    }
    // 관례 터치 아이콘 주소는 같은 PNG, 없는 파일은 화면(HTML)이 아니라 404, 확장자 없는 주소만 화면(plan §8.61)
    const touch = Buffer.from(await (await fetch(`${base}apple-touch-icon.png`)).arrayBuffer());
    for (const alias of ["apple-touch-icon-precomposed.png", "apple-touch-icon-180x180.png", "apple-touch-icon-120x120-precomposed.png"]) {
      const r = await fetch(`${base}${alias}`);
      if (r.status !== 200 || r.headers.get("content-type") !== "image/png" || !Buffer.from(await r.arrayBuffer()).equals(touch)) throw new Error(`${alias}: ${r.status} ${r.headers.get("content-type")}`);
    }
    for (const missing of ["favicon.ico", "nope.png", "assets/nope.js"]) {
      const r = await fetch(`${base}${missing}`);
      if (r.status !== 404) throw new Error(`${missing}: ${r.status} ${r.headers.get("content-type")}`);
    }
    const route = await fetch(`${base}thread/t-0001`);
    if (route.status !== 200 || !route.headers.get("content-type").startsWith("text/html")) throw new Error(`화면 경로 ${route.status}`);
    const ctx = await browser.newContext();
    const page = await ctx.newPage();
    await page.goto(base);
    const cdp = await ctx.newCDPSession(page);
    const got = await cdp.send("Page.getAppManifest");
    // 상태 막대 style은 두지 않는다(참고 index.html: 홈 화면 앱이 상태 막대 아래에 그리지 않는다)
    if ((await page.locator('meta[name="apple-mobile-web-app-status-bar-style"]').count()) !== 0) throw new Error("apple-mobile-web-app-status-bar-style meta가 있다");
    if (got.errors.length || !got.url.endsWith("/manifest.webmanifest?v=2")) throw new Error(`${got.url} ${JSON.stringify(got.errors)}`);
    // service worker(§8.26)가 이 화면을 맡고, 브라우저가 설치할 수 있다고 본다
    const sw = await fetch(`${base}sw.js`);
    if (sw.status !== 200 || !sw.headers.get("content-type").startsWith("text/javascript")) throw new Error(`sw.js ${sw.status}`);
    if ((await fetch(`${base}offline.html`)).status !== 200) throw new Error("offline.html");
    await until("service worker가 화면을 맡음", () => page.evaluate(() => !!navigator.serviceWorker.controller));
    const inst = await cdp.send("Page.getInstallabilityErrors");
    if (inst.installabilityErrors.length) throw new Error(`설치할 수 없다: ${JSON.stringify(inst.installabilityErrors)}`);
    // 아이콘을 바꾼 판(v=2, F3 a 블록 커서): 모든 아이콘 주소에 v가 붙고 service worker cache 이름도 올라가,
    // 이미 설치된 앱·열린 탭도 옛 파일 대신 새 파일을 받는다
    const links = await page.evaluate(() => [...document.querySelectorAll('link[rel="icon"], link[rel="apple-touch-icon"], link[rel="manifest"]')].map((l) => l.getAttribute("href")));
    const sws = await sw.text();
    const old = [...man.icons.map((i) => i.src), ...links].filter((u) => !u.endsWith("?v=2"));
    if (links.length !== 3 || old.length) throw new Error(`v 없는 아이콘 주소 ${JSON.stringify(old)} (link ${links.length}개)`);
    if (!sws.includes('"aaa-offline-v2"') || !sws.includes('"/icon-192.png?v=2"') || !sws.includes('"/favicon-32.png?v=2"')) throw new Error("sw.js cache 이름·알림 아이콘이 v2가 아니다");
    await ctx.close();
  });

  await step("앱 아이콘(사용자 요청 10-17 「모두 바꿔」): 머리·연결 화면·빈 화면의 표시는 icon.svg(F3 a 블록 커서), 옛 「aaa」 글자 배지 없음", async () => {
    const logos = (p, sel = ".logo") =>
      p.locator(sel).evaluateAll((els) => els.map((e) => `${e.tagName} ${e.getAttribute("src")} ${e.naturalWidth > 0} ${Math.round(e.getBoundingClientRect().width)} ${e.textContent}`));
    const bad = [];
    // 연결(pairing) 화면: 휴대폰
    const ctx = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });
    const pp = await ctx.newPage();
    await pp.goto(base);
    await pp.getByLabel("페어링 코드").waitFor();
    // 연결 화면은 herdr-web-ui AccessGate(.access-gate-mark, 44px)
    const gate = (p) => logos(p, ".logo, .access-gate-mark");
    await until("연결 화면 아이콘", async () => (await gate(pp)).join() === "IMG /icon.svg true 44 ").catch(async (e) => bad.push(`${e.message}: ${await gate(pp)}`));
    await shot(pp, "phone-pair-icon");
    await ctx.close();
    // iPhone WebKit 390×844(QA t-0337 Q2): 옛 .pair는 입력칸 고유 폭(20px mono·자간)이 grid 항목 카드를 47px 밀어 오른쪽이 잘렸다.
    // AccessGate는 카드 폭이 min(100%, …)이고 입력칸이 100%라 카드·입력칸·단추·설명·틀린 코드 말이 모두 화면 안이다
    const wk = await webkit.launch();
    try {
      const wctx = await wk.newContext({
        viewport: { width: 390, height: 844 },
        isMobile: true,
        hasTouch: true,
        userAgent: "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1",
      });
      const wp = await wctx.newPage();
      await wp.goto(base);
      const input = wp.getByLabel("페어링 코드");
      await input.waitFor();
      await input.fill("000000");
      await wp.getByRole("button", { name: "이 기기 페어링" }).click();
      await wp.getByRole("alert").waitFor();
      const fit = await wp.evaluate(() => {
        const right = (sel) => [...document.querySelectorAll(sel)].map((e) => Math.round(e.getBoundingClientRect().right));
        const card = document.querySelector(".access-gate").getBoundingClientRect();
        return {
          page: document.documentElement.scrollWidth,
          view: window.innerWidth,
          card: [Math.round(card.left), Math.round(card.right)],
          parts: right(".access-gate-input, .access-gate-submit, .access-gate-copy, .access-gate-error"),
        };
      });
      if (fit.page > fit.view || fit.card[0] < 0 || fit.card[1] > fit.view || fit.parts.some((r) => r > fit.card[1]))
        bad.push(`iPhone WebKit 연결 화면이 넘친다: ${JSON.stringify(fit)}`);
      await shot(wp, "phone-webkit-pair-gate");
      await wctx.close();
    } finally {
      await wk.close();
    }
    // 머리(목록 왼쪽 위)와 아무것도 고르지 않은 빈 화면
    const dp = (await pairPage(browser, aaa("web", "pair").code, { width: 1100, height: 760 })).page;
    await until("머리·빈 화면 아이콘", async () => (await logos(dp)).join("|") === "IMG /icon.svg true 22 |IMG /icon.svg true 44 ").catch(async (e) => bad.push(`${e.message}: ${await logos(dp)}`));
    if ((await dp.locator(".list-head .logo").getAttribute("alt")) !== "aaa") bad.push("머리 아이콘에 이름(alt)이 없다");
    await shot(dp, "desktop-head-icon");
    await dp.context().close();
    if (bad.length) throw new Error(bad.join("; "));
  });

  // 입력줄을 쓰는 단계가 많아 이 화면은 입력줄로 둔다. 기본(바로 입력)은 「입력 방식」 단계가 본다.
  const ctl = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 }, { line: true });
  const page = ctl.page;

  await step("controller: 키보드 입력", async () => {
    await page.getByTestId("pane-t1").click();
    await page.getByTestId("terminal").click();
    await until("터미널 준비", async () => (await screenText(page)).includes("$"));
    await page.keyboard.type("echo key-$((5*5))\n");
    await until("키보드 입력 결과", async () => (await screenText(page)).includes("key-25"));
  });

  await step("넓은 창: PTY가 터미널 칸을 채우고 창 크기를 따라간다(조작 중일 때)", async () => {
    // 터미널 글자 영역이 칸(안쪽 여백 뺀 크기)을 한 cell 안쪽까지 채우는지, header 크기·host PTY 크기가 같은지
    const filled = async () => {
      const m = await page.evaluate(() => {
        const box = document.querySelector('[data-testid="terminal"]');
        const scr = box.querySelector(".xterm-screen").getBoundingClientRect();
        const cs = getComputedStyle(box);
        const w = box.clientWidth - parseFloat(cs.paddingLeft) - parseFloat(cs.paddingRight);
        const h = box.clientHeight - parseFloat(cs.paddingTop) - parseFloat(cs.paddingBottom);
        const crumb = document.querySelector(".term-title .muted").textContent;
        return { w, h, sw: scr.width, sh: scr.height, crumb };
      });
      const p = aaa("pane", "list").panes.find((x) => x.name === "t1");
      const cw = m.sw / p.cols;
      const ch = m.sh / p.rows;
      // 오른쪽에는 xterm 세로 scrollbar 자리(기본 14px)가 남는다
      const ok = m.sw <= m.w + 1 && m.w - m.sw < cw + 15 && m.sh <= m.h + 1 && m.h - m.sh < ch + 1;
      return { ok: ok && m.crumb.includes(`${p.cols}×${p.rows}`), m, cols: p.cols, rows: p.rows };
    };
    let last = null;
    const fills = async () => (last = await filled()).ok;
    await until("1280×800에서 칸을 채움", fills).catch((e) => {
      throw new Error(`${e.message} 마지막 측정: ${JSON.stringify(last)}`);
    });
    const small = await filled();
    await page.setViewportSize({ width: 1900, height: 1150 });
    await until("넓힌 창을 따라감", async () => {
      const f = await filled();
      return f.ok && f.cols > small.cols && f.rows > small.rows;
    });
    await shot(page, "desktop-wide-fit");
    await page.setViewportSize({ width: 1280, height: 800 });
    await until("되돌린 창을 따라감", async () => {
      const f = await filled();
      return f.ok && f.cols === small.cols && f.rows === small.rows;
    });
  });

  await step("터미널 글꼴: Meslotendard로 cell 너비는 영문, 한글은 정확히 두 칸(1280×800)", async () => {
    // 넣은 WOFF2와 고지 파일을 host가 보낸다(버전 폴더라 오래 캐시한다)
    const dir = "fonts/meslotendard-0.3.1/";
    const want = fs.readFileSync(path.join(repo, "web", "public", dir, "Meslotendard-Regular.woff2"));
    const r = await fetch(`${base}${dir}Meslotendard-Regular.woff2`);
    const got = Buffer.from(await r.arrayBuffer());
    if (r.status !== 200 || r.headers.get("content-type") !== "font/woff2" || !got.equals(want))
      throw new Error(`woff2 ${r.status} ${r.headers.get("content-type")} ${got.length}`);
    if (!r.headers.get("cache-control").includes("immutable")) throw new Error(`cache ${r.headers.get("cache-control")}`);
    for (const f of ["LICENSE", "licenses/THIRD_PARTY_NOTICES.md", "licenses/MESLO_APACHE-2.0.txt", "licenses/NERD_FONTS_LICENSE.txt"]) {
      const n = await fetch(`${base}${dir}${f}`);
      if (n.status !== 200 || n.headers.get("content-type") !== "text/plain; charset=utf-8")
        throw new Error(`고지 파일 ${f}: ${n.status} ${n.headers.get("content-type")}`);
    }
    await page.getByLabel("보낼 글").fill("echo 한글 간격 확인 ABCD abcd 0123");
    await page.getByRole("button", { name: "보내기" }).click();
    await until("한글 줄", async () => (await screenText(page)).includes("한글 간격 확인 ABCD"));
    const cols = aaa("pane", "list").panes.find((x) => x.name === "t1").cols;
    console.log("     글꼴 측정(1280×800)", JSON.stringify(await fontOk(page, cols)));
    await shot(page, "desktop-hangul");
  });

  await step("글자 폭(herdr-web-ui terminalWidths): 이모지·새 이모지·ZWJ 묶음·⚠️는 두 칸", async () => {
    aaa("tab", "create", "--machine", "labA", "--label", "widths", "--", process.execPath, `${root}/widths.js`);
    const wp = (await pairPage(browser, aaa("web", "pair").code, { width: 900, height: 600 })).page;
    await wp.getByTestId("pane-widths").click();
    await until("폭 화면", async () => (await screenText(wp)).includes("W"));
    // 글자마다 화면 위치로 몇째 칸인지 잰다(DOM renderer의 span 묶음에 기대지 않는다)
    const cols = await wp.evaluate((cols) => {
      const screen = document.querySelector('[data-testid="terminal"] .xterm-screen').getBoundingClientRect();
      const cell = screen.width / cols;
      const at = (ch) => {
        const walk = document.createTreeWalker(document.querySelector(".xterm-rows"), NodeFilter.SHOW_TEXT);
        for (let n = walk.nextNode(); n; n = walk.nextNode()) {
          const i = n.textContent.indexOf(ch);
          if (i < 0) continue;
          const r = document.createRange();
          r.setStart(n, i);
          r.setEnd(n, i + 1);
          return Math.round((r.getBoundingClientRect().left - screen.left) / cell);
        }
        return null;
      };
      return ["X", "Y", "Z", "W"].map(at);
    }, aaa("pane", "list").panes.find((x) => x.name === "widths").cols);
    if (JSON.stringify(cols) !== "[2,2,2,2]") throw new Error(`글자 칸(기대 [2,2,2,2]) ${JSON.stringify(cols)}`);
    await wp.context().close();
    aaa("pane", "kill", "widths");
  });

  await step("controller: 입력 막대로 한글", async () => {
    await page.getByLabel("보낼 글").fill("echo 한글-$((2*4))");
    await page.getByRole("button", { name: "보내기" }).click();
    await until("한글 입력 결과", async () => (await screenText(page)).includes("한글-8"));
    await shot(page, "desktop-controller");
  });

  await step("키 막대(herdr-web-ui KeyBar): 터치 기기에서만, 유지 키는 다시 누를 때까지(래치)·하드웨어 키와 같은 bytes, DECCKM, 꾹 누르면 반복, 조합 중 회색, focus를 안 가져감", async () => {
    aaa("tab", "create", "--machine", "labA", "--label", "barapp", "--", process.execPath, `${root}/bar.js`);
    // [일련번호, 받은 조각]
    const lines = () =>
      aaa("pane", "read", "barapp", "--source", "visible")
        .text.split("\n").map((l) => /^GOT:(\d+):(.*)$/.exec(l))
        .filter(Boolean)
        .map((m) => [Number(m[1]), JSON.parse(m[2])]);
    const last = () => Math.max(0, ...lines().map(([i]) => i));
    // 데스크톱(마우스)에는 키 막대가 없다
    await page.getByTestId("pane-barapp").click();
    await until("bar 화면(데스크톱)", async () => (await screenText(page)).includes("bar>"));
    if (await page.locator(".key-bar").isVisible()) throw new Error("데스크톱에 키 막대가 보인다");
    // 터치 기기. Alt를 하드웨어 키와 견주려고 macOS가 아닌 platform으로 둔다(macOS xterm은 Option을 글자로 쓴다).
    const tp = (await pairPage(browser, aaa("web", "pair").code, { width: 820, height: 1180 }, { touch: true, platform: "Linux x86_64" })).page;
    await tp.getByTestId("pane-barapp").click();
    await until("bar 화면(터치)", async () => (await screenText(tp)).includes("bar>"));
    const bar = tp.locator(".key-bar");
    if (!(await bar.isVisible())) throw new Error("터치 기기에 키 막대가 없다");
    const order = await bar.locator(".key").evaluateAll((ks) => ks.map((k) => k.dataset.key));
    // 터치 기기는 맨 앞에 ⌨(입력줄 전환, KeyBar.tsx 200~205줄)
    const want = ["direct", "Escape", "Tab", "ctrl-c", "Control", "Alt", "Shift", "Enter", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"];
    if (JSON.stringify(order) !== JSON.stringify(want)) throw new Error(`키 순서 ${order}`);
    const key = (k) => bar.locator(`.key[data-key="${k}"]`);
    await tp.locator(".xterm-screen").click();
    // 한 동작이 pane에 보낸 조각들
    const sent = async (act, what = "") => {
      const n = last();
      await act();
      await sleep(250);
      await until(`pane 받음 ${what}`, () => last() > n);
      await sleep(150);
      return lines()
        .filter(([i]) => i > n)
        .map(([, d]) => d)
        .join("");
    };
    const bad = [];
    const expect = async (what, act, wantData) => {
      const got = await sent(act, what);
      if (got !== wantData) bad.push(`${what}: ${JSON.stringify(got)}(기대 ${JSON.stringify(wantData)})`);
    };
    // 키를 눌러도 focus는 터미널에 남는다(화면 키보드가 내려가지 않는다)
    await key("Escape").click();
    if (!(await tp.evaluate(() => document.activeElement?.classList.contains("xterm-helper-textarea")))) bad.push("키가 focus를 가져갔다");
    await sleep(300);
    // 래치: 켠 Ctrl은 두 번 써도 남고, 다시 눌러야 풀린다
    await key("Control").click();
    await expect("Ctrl 래치 a", () => tp.keyboard.press("a"), "\x01");
    if ((await key("Control").getAttribute("aria-pressed")) !== "true") bad.push("Ctrl이 한 번 쓰고 풀렸다");
    await expect("Ctrl 래치 b", () => tp.keyboard.press("b"), "\x02");
    await expect("Ctrl + 키 막대 ^C", () => key("ctrl-c").click(), "\x03");
    await key("Control").click();
    if ((await key("Control").getAttribute("aria-pressed")) !== "false") bad.push("Ctrl이 다시 눌러도 안 풀렸다");
    await expect("풀린 뒤 c", () => tp.keyboard.press("c"), "c");
    // 유지 키 + (키 막대 키 또는 하드웨어 키) = 같은 수식의 하드웨어 키
    const combos = [
      ["Control", "ArrowLeft", "bar"],
      ["Alt", "ArrowLeft", "bar"],
      ["Alt", "ArrowUp", "bar"],
      ["Shift", "ArrowUp", "bar"],
      ["Shift", "Tab", "bar"],
      ["Alt", "Enter", "bar"],
      ["Shift", "Enter", "bar"],
      ["Control", "Home", "hw"],
      ["Control", "PageUp", "hw"],
      ["Alt", "b", "hw"],
      ["Control", "Backspace", "hw"],
      ["Shift", "a", "hw"],
    ];
    for (const [mod, k, how] of combos) {
      // 글자는 물리 키 이름으로 누른다: Playwright는 "Shift+a"를 Shift를 쥔 "a"로 내지만 실제 자판은 "A"를 낸다
      const physical = /^[a-z]$/.test(k) ? `Key${k.toUpperCase()}` : k;
      const hardware = await sent(() => tp.keyboard.press(`${mod}+${physical}`), `하드웨어 ${mod}+${k}`);
      await key(mod).click();
      const viaBar = await sent(() => (how === "bar" ? key(k).click() : tp.keyboard.press(k)), `키 막대 ${mod}+${k}`);
      await key(mod).click();
      if (viaBar !== hardware) bad.push(`${mod}+${k}: 키 막대 ${JSON.stringify(viaBar)} ≠ 하드웨어 ${JSON.stringify(hardware)}`);
    }
    // 화살표 모드(DECCKM)를 켜면 키 막대 화살표도 SS3
    await sent(() => tp.keyboard.press("Shift+K"));
    await expect("DECCKM ↑", () => key("ArrowUp").click(), "\x1bOA");
    await sent(() => tp.keyboard.press("Shift+J"));
    await expect("DECCKM 끔 ↑", () => key("ArrowUp").click(), "\x1b[A");
    // 꾹 누르면 400ms 뒤 50ms마다 되풀이, 손을 떼면 한 번 더 가지 않는다
    const left = await key("ArrowLeft").boundingBox();
    const held = await sent(async () => {
      await tp.mouse.move(left.x + left.width / 2, left.y + left.height / 2);
      await tp.mouse.down();
      await sleep(700);
      await tp.mouse.up();
    });
    const repeats = held.split("\x1b[D").length - 1;
    if (repeats < 3 || held.replaceAll("\x1b[D", "") !== "") bad.push(`꾹 누름: ${JSON.stringify(held)}`);
    // 누른 채 밀면 취소(아무것도 안 감)
    const n0 = last();
    await tp.mouse.move(left.x + left.width / 2, left.y + left.height / 2);
    await tp.mouse.down();
    await tp.mouse.move(left.x + left.width / 2 + 15, left.y + left.height / 2, { steps: 3 });
    await sleep(600);
    await tp.mouse.up();
    await sleep(300);
    if (last() !== n0) bad.push(`밀어서 취소한 누름이 갔다: ${JSON.stringify(lines().filter(([i]) => i > n0))}`);
    // 조합 중에는 키 막대가 회색이고, 조합으로 확정된 글에는 유지 키가 붙지 않는다(다음 키를 기다린다)
    await key("Control").click();
    const textarea = tp.locator("textarea.xterm-helper-textarea");
    await textarea.evaluate((el) => el.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true, data: "" })));
    await sleep(100);
    if (!(await key("Escape").isDisabled())) bad.push("조합 중에 키 막대가 켜져 있다");
    await textarea.evaluate((el) => el.dispatchEvent(new CompositionEvent("compositionend", { bubbles: true, data: "" })));
    await sleep(100);
    if (await key("Escape").isDisabled()) bad.push("조합이 끝나도 키 막대가 꺼져 있다");
    const cdp = await tp.context().newCDPSession(tp);
    const ime = await sent(async () => {
      await cdp.send("Input.imeSetComposition", { text: "한", selectionStart: 1, selectionEnd: 1 });
      await cdp.send("Input.insertText", { text: "한" });
    });
    await cdp.detach();
    if (ime !== "한") bad.push(`조합 확정 글: ${JSON.stringify(ime)}`);
    if ((await key("Control").getAttribute("aria-pressed")) !== "true") bad.push("조합 확정이 Ctrl을 풀었다");
    await expect("조합 뒤 Ctrl b", () => tp.keyboard.press("b"), "\x02");
    await key("Control").click();
    if (bad.length) throw new Error(bad.join("; "));
    await tp.context().close();
    aaa("pane", "kill", "barapp");
    await page.getByTestId("pane-t1").click();
  });

  await step("입력 방식(herdr-web-ui terminalInputMode): 기본은 바로 입력, 터치 기기 ⌨로 입력줄, 입력줄은 키보드를 안 올리고 PTY 크기를 안 바꾼다, Enter 보내기·빈 Enter, 조합 중엔 ⌨만 회색", async () => {
    aaa("tab", "create", "--machine", "labA", "--label", "lineapp", "--", process.execPath, `${root}/bar.js`);
    // [일련번호, 받은 조각]
    const lines = () =>
      aaa("pane", "read", "lineapp", "--source", "visible")
        .text.split("\n").map((l) => /^GOT:(\d+):(.*)$/.exec(l))
        .filter(Boolean)
        .map((m) => [Number(m[1]), JSON.parse(m[2])]);
    const last = () => Math.max(0, ...lines().map(([i]) => i));
    const since = (n) =>
      lines()
        .filter(([i]) => i > n)
        .map(([, d]) => d);
    const size = () => {
      const x = aaa("pane", "list").panes.find((q) => q.name === "lineapp");
      return `${x.cols}x${x.rows}`;
    };
    const bad = [];
    // 데스크톱 기본: 입력줄이 없다(바로 입력)
    const desk = await pairPage(browser, aaa("web", "pair").code, { width: 1100, height: 700 });
    await desk.page.getByTestId("pane-lineapp").click();
    await until("bar 화면(데스크톱)", async () => (await screenText(desk.page)).includes("bar>"));
    if ((await desk.page.locator(".terminal-input").count()) !== 0) bad.push("데스크톱 기본에 입력줄이 있다");
    await desk.page.context().close();
    // 터치 기기 기본: 바로 입력. 키 막대 맨 앞 ⌨는 꺼져 있고 xterm 칸이 화면 키보드를 올린다
    const tp = (await pairPage(browser, aaa("web", "pair").code, { width: 820, height: 1180 }, { touch: true, platform: "Linux x86_64" })).page;
    await tp.getByTestId("pane-lineapp").click();
    await until("bar 화면(터치)", async () => (await screenText(tp)).includes("bar>"));
    const toggle = tp.locator('.key-bar .key[data-key="direct"]');
    const first = await tp.locator(".key-bar .key").first().getAttribute("data-key");
    if (first !== "direct") bad.push(`키 막대 맨 앞이 ⌨가 아니다: ${first}`);
    if ((await toggle.getAttribute("aria-pressed")) !== "false") bad.push("기본인데 ⌨가 켜져 있다");
    if ((await tp.locator(".terminal-input").count()) !== 0) bad.push("터치 기본에 입력줄이 있다");
    if ((await tp.locator(".term-scroll[data-direct-typing]").count()) !== 1) bad.push("바로 입력인데 data-direct-typing이 없다");
    const helper = tp.locator("textarea.xterm-helper-textarea");
    // ⌨ → 입력줄. xterm 칸은 화면 키보드를 올리지 않는다(inputmode=none)
    await toggle.click();
    await tp.locator(".terminal-input").waitFor();
    if ((await toggle.getAttribute("aria-pressed")) !== "true") bad.push("입력줄인데 ⌨가 꺼져 있다");
    if ((await helper.getAttribute("inputmode")) !== "none") bad.push(`입력줄인데 xterm inputmode ${await helper.getAttribute("inputmode")}`);
    if ((await tp.locator(".term-scroll[data-direct-typing]").count()) !== 0) bad.push("입력줄인데 data-direct-typing이 있다");
    if ((await tp.evaluate(() => localStorage.getItem("aaa:terminal-input-mode"))) !== "line") bad.push("입력 방식이 이 기기에 저장되지 않았다");
    const line = tp.getByLabel("보낼 글");
    const sendBtn = tp.locator(".terminal-input-send");
    // 글 + Enter: 글 다음 Enter가 따로 간다
    let n = last();
    await line.fill("echo hi");
    await line.press("Enter");
    await until("입력줄 Enter", () => since(n).join("") === "echo hi\r");
    if ((await line.inputValue()) !== "") bad.push("보낸 뒤 입력줄에 글이 남았다");
    // 빈 입력줄의 보내기는 Enter만. 단추 이름도 Enter다. 유지 Alt는 붙는다(PaneTerminal pressEnter)
    if ((await sendBtn.getAttribute("aria-label")) !== "터미널에 Enter 보내기") bad.push(`빈 줄 단추 ${await sendBtn.getAttribute("aria-label")}`);
    n = last();
    await sendBtn.click();
    await until("빈 Enter", () => since(n).join("") === "\r");
    await tp.locator('.key-bar .key[data-key="Alt"]').click();
    n = last();
    await sendBtn.click();
    await until("Alt + 빈 Enter", () => since(n).join("") === "\x1b\r");
    await tp.locator('.key-bar .key[data-key="Alt"]').click();
    // 키 막대 키는 입력줄의 키보드를 터미널로 옮기지 않는다
    await line.focus();
    n = last();
    await tp.locator('.key-bar .key[data-key="Escape"]').click();
    await until("Esc", () => since(n).join("") === "\x1b");
    if (!(await line.evaluate((el) => el === document.activeElement))) bad.push("키 막대 키가 입력줄의 focus를 가져갔다");
    // 입력줄이 4줄까지 자라도 PTY 크기는 그대로다(그리드 위에 뜬다)
    const before = size();
    const box0 = await tp.locator(".term-scroll").boundingBox();
    await line.fill("1\n2\n3\n4\n5\n6");
    await sleep(800);
    if ((await line.getAttribute("rows")) !== "4") bad.push(`여섯 줄 입력줄 rows ${await line.getAttribute("rows")}`);
    const box1 = await tp.locator(".term-scroll").boundingBox();
    if (size() !== before || Math.abs(box1.height - box0.height) > 0.5) bad.push(`입력줄이 자라며 터미널이 줄었다: ${before} → ${size()}, 높이 ${box0.height} → ${box1.height}`);
    await line.fill("");
    // 입력줄 조합 중에는 ⌨만 회색이고 키 막대는 그대로다
    await line.dispatchEvent("compositionstart", { data: "" });
    await sleep(100);
    if (!(await toggle.isDisabled())) bad.push("입력줄 조합 중에 ⌨가 켜져 있다");
    if (await tp.locator('.key-bar .key[data-key="Escape"]').isDisabled()) bad.push("입력줄 조합 중에 키 막대가 꺼졌다");
    await line.dispatchEvent("compositionend", { data: "" });
    await sleep(100);
    // ⌨를 다시 누르면 바로 입력: 입력줄이 사라지고 터미널이 키보드를 받는다
    await toggle.click();
    await until("입력줄 사라짐", async () => (await tp.locator(".terminal-input").count()) === 0);
    if ((await helper.getAttribute("inputmode")) !== null) bad.push("바로 입력인데 xterm inputmode가 남았다");
    if (!(await helper.evaluate((el) => el === document.activeElement))) bad.push("바로 입력을 켰는데 터미널이 focus를 받지 않았다");
    if (bad.length) throw new Error(bad.join("; "));
    await tp.context().close();
    aaa("pane", "kill", "lineapp");
    await page.getByTestId("pane-t1").click();
  });

  await step("키 바 편집(herdr-web-ui KeyBarSettings, 차이 목록 「키 막대 항목 설정」): 설정에서 키를 빼고·옮기고·더하고 조합을 만들면 터미널 키 막대가 바로 그 순서, 이 기기에 저장, 기본값으로 되돌리기", async () => {
    aaa("tab", "create", "--machine", "labA", "--label", "kbset", "--", process.execPath, `${root}/scroll.js`, "plain");
    await waitOutput("kbset", "s-200");
    const tp = (await pairPage(browser, aaa("web", "pair").code, { width: 820, height: 1180 }, { touch: true, platform: "Linux x86_64" })).page;
    await tp.getByTestId("pane-kbset").click();
    await until("kbset 화면", async () => (await screenText(tp)).includes("s-200"));
    const order = () => tp.locator(".key-bar .key").evaluateAll((ks) => ks.map((k) => k.dataset.key));
    const defaults = ["direct", "Escape", "Tab", "ctrl-c", "Control", "Alt", "Shift", "Enter", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"];
    if (JSON.stringify(await order()) !== JSON.stringify(defaults)) throw new Error(`처음 키 막대 ${await order()}`);
    const openKeyBar = async () => {
      (await openSettings(tp, "터미널")).getByRole("button", { name: "키 바 편집" }).click();
      return tp.getByRole("dialog", { name: "키 바" });
    };
    const kb = await openKeyBar();
    await kb.getByRole("button", { name: "Esc 제거" }).click();
    await kb.getByRole("button", { name: "Tab 아래로 이동" }).click();
    await kb.locator(".key-bar-settings-add select").selectOption("PageUp");
    await kb.locator(".key-bar-settings-add").getByRole("button", { name: "키 추가" }).click();
    // 사용자 조합: 기본 키 ←(처음 값)에 Ctrl
    await kb.getByRole("group", { name: "조합에 사용할 보조 키" }).getByRole("button", { name: "Ctrl" }).click();
    if ((await kb.locator(".key-bar-settings-preview").innerText()) !== "Ctrl+←") throw new Error(`조합 미리 보기 ${await kb.locator(".key-bar-settings-preview").innerText()}`);
    await kb.getByRole("button", { name: "조합 추가" }).click();
    await shot(tp, "tablet-key-bar-settings");
    const combo = 'key:"ArrowLeft":100';
    const want = ["direct", "ctrl-c", "Tab", "Control", "Alt", "Shift", "Enter", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "PageUp", combo];
    // ← 는 설정으로, Esc는 닫기
    await kb.getByRole("button", { name: "설정으로 돌아가기" }).click();
    await tp.getByRole("dialog", { name: "설정" }).waitFor();
    await tp.keyboard.press("Escape");
    await until("설정 닫힘", async () => (await tp.locator(".settings-dialog").count()) === 0);
    // 키 막대는 다시 열지 않아도 바로 바뀐다
    await until("키 막대가 편집한 순서", async () => JSON.stringify(await order()) === JSON.stringify(want));
    const stored = await tp.evaluate(() => JSON.parse(localStorage.getItem("aaa:keybar-items") ?? "null"));
    if (!Array.isArray(stored) || stored.length !== want.length - 1) throw new Error(`저장 ${JSON.stringify(stored)}`);
    // 저장한 조합은 그 보조 키로 간다(Ctrl+← = CSI 1;5D)
    await tp.locator(".xterm-screen").click();
    await tp.locator(`.key-bar .key[data-key='${combo}']`).click();
    await waitOutput("kbset", 'GOT:"\\u001b[1;5D"');
    // 다시 열어도 이 기기의 순서
    await tp.reload();
    await tp.getByTestId("pane-kbset").click();
    await until("다시 연 키 막대", async () => JSON.stringify(await order()) === JSON.stringify(want));
    // 기본값으로 되돌리기: 저장값을 지우고 참고 기본 배열
    await (await openKeyBar()).getByRole("button", { name: "기본값으로 되돌리기" }).click();
    await tp.keyboard.press("Escape");
    await tp.keyboard.press("Escape");
    await until("기본 키 막대", async () => JSON.stringify(await order()) === JSON.stringify(defaults));
    if ((await tp.evaluate(() => localStorage.getItem("aaa:keybar-items"))) !== null) throw new Error("기본값인데 저장값이 남았다");
    await shot(tp, "tablet-key-bar-defaults");
    await tp.context().close();
    aaa("pane", "kill", "kbset");
  });

  await step("안전 영역(herdr-web-ui styles.css safe-area-inset, 차이 목록 M3): 가로로 둔 휴대폰의 노치를 목록은 왼쪽, 터미널 칸은 오른쪽(좁으면 양쪽)으로 비킨다", async () => {
    // 가로 휴대폰(844×390, 두 칸)과 좁은 가로(667×375, 한 칸). Chromium이 env(safe-area-inset-*)를 이 값으로 준다
    const measure = (p) =>
      p.evaluate(() => {
        const pad = (sel) => {
          const el = document.querySelector(sel);
          if (!el || !el.getClientRects().length) return null;
          const cs = getComputedStyle(el);
          return [parseFloat(cs.paddingLeft), parseFloat(cs.paddingRight)];
        };
        return { list: pad(".list"), head: pad(".term-head"), scroll: pad(".term-scroll"), keys: pad(".key-bar"), input: pad(".terminal-input") };
      });
    const open = async (viewport, line) => {
      const { page: p } = await pairPage(browser, aaa("web", "pair").code, viewport, { touch: true, line });
      const cdp = await p.context().newCDPSession(p);
      await cdp.send("Emulation.setSafeAreaInsetsOverride", { insets: { top: 0, left: 47, right: 47, bottom: 21 } });
      await p.getByTestId("pane-t1").click();
      await until("t1 화면", async () => (await screenText(p)).includes("$"));
      return p;
    };
    const wide = await open({ width: 844, height: 390 });
    const w = await measure(wide);
    const want = { list: [47, 0], head: [16, 12 + 47], scroll: [16, 16 + 47], keys: [8, 8 + 47] };
    for (const [k, v] of Object.entries(want)) if (JSON.stringify(w[k]) !== JSON.stringify(v)) throw new Error(`가로 두 칸 ${k} ${JSON.stringify(w[k])} ≠ ${JSON.stringify(v)} (${JSON.stringify(w)})`);
    await shot(wide, "phone-landscape-safe-area");
    await wide.context().close();
    // 한 칸씩 보이는 좁은 가로: 터미널 칸이 양쪽을 비킨다(입력줄 화면)
    const narrow = await open({ width: 667, height: 375 }, true);
    const n = await measure(narrow);
    const wantN = { head: [8 + 47, 8 + 47], scroll: [8 + 47, 8 + 47], keys: [8 + 47, 8 + 47], input: [8 + 47, 8 + 47] };
    for (const [k, v] of Object.entries(wantN)) if (JSON.stringify(n[k]) !== JSON.stringify(v)) throw new Error(`좁은 가로 ${k} ${JSON.stringify(n[k])} ≠ ${JSON.stringify(v)} (${JSON.stringify(n)})`);
    await narrow.getByRole("button", { name: "목록으로" }).click();
    const l = await measure(narrow);
    if (JSON.stringify(l.list) !== "[47,47]") throw new Error(`좁은 가로 목록 ${JSON.stringify(l.list)}`);
    await narrow.context().close();
  });

  await step("화면 켜짐 유지(herdr-web-ui keepScreenOn·wakeLock, 차이 목록 M6): 켜면 pane을 보는 동안 screen wake lock, 가려지면 놓고 다시 보이면 다시, 끄면 놓음, 이 기기에 저장", async () => {
    // wake lock은 headless에서 거절될 수 있다: 요청·놓기를 기록하는 것으로 바꾼다
    const init = () => {
      const log = [];
      Object.defineProperty(window, "__wakeLog", { value: log });
      Object.defineProperty(navigator, "wakeLock", {
        configurable: true,
        value: {
          request: async (type) => {
            log.push(`request ${type}`);
            return { release: async () => void log.push("release") };
          },
        },
      });
    };
    const wp = (await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 }, { init })).page;
    const log = () => wp.evaluate(() => window.__wakeLog.join(","));
    const hide = (hidden) =>
      wp.evaluate((h) => {
        Object.defineProperty(document, "visibilityState", { configurable: true, get: () => (h ? "hidden" : "visible") });
        document.dispatchEvent(new Event("visibilitychange"));
      }, hidden);
    await wp.getByTestId("pane-t1").click();
    // 앞 단계가 남긴 글에 기대지 않는다: 셸 프롬프트가 보이면 연결된 것이다
    await until("t1 화면", async () => (await screenText(wp)).includes("$"));
    await sleep(300);
    if ((await log()) !== "") throw new Error(`꺼진 설정인데 ${await log()}`);
    if (!(await wp.evaluate(() => window.isSecureContext))) throw new Error("시험 주소가 secure context가 아니다");
    const toggle = async () => {
      await (await openSettings(wp, "폰 및 기기")).getByRole("switch", { name: "화면 켜짐 유지" }).click();
      await wp.keyboard.press("Escape");
    };
    // 휴대폰은 pane을 보는 동안 목록 바닥 톱니가 가려진다: 목록으로 돌아가 켜고 다시 고른다
    const back = wp.getByRole("button", { name: "목록으로" });
    if (await back.isVisible()) await back.click();
    await toggle();
    if ((await wp.evaluate(() => localStorage.getItem("aaa:keep-screen-on"))) !== "1") throw new Error("켠 설정이 저장되지 않았다");
    await sleep(300);
    if ((await log()) !== "") throw new Error(`pane을 보지 않는데 ${await log()}`);
    await wp.getByTestId("pane-t1").click();
    await until("wake lock 요청", async () => (await log()) === "request screen");
    await hide(true);
    await until("가려지면 놓음", async () => (await log()) === "request screen,release");
    await hide(false);
    await until("다시 보이면 다시 요청", async () => (await log()) === "request screen,release,request screen");
    // 다시 열어도 이 기기 설정
    await wp.reload();
    // 주소(#pane=)가 그 pane을 다시 고른다. 목록이 보이면 고른다
    await until("다시 연 화면", async () => (await wp.getByTestId("pane-t1").isVisible()) || (await log()) !== "");
    if (await wp.getByTestId("pane-t1").isVisible()) await wp.getByTestId("pane-t1").click();
    await until("다시 연 화면도 요청", async () => (await log()) === "request screen");
    if (await back.isVisible()) await back.click();
    await until("목록으로 가면 놓음", async () => (await log()) === "request screen,release");
    await toggle();
    if ((await wp.evaluate(() => localStorage.getItem("aaa:keep-screen-on"))) !== null) throw new Error("끈 설정이 남았다");
    await wp.getByTestId("pane-t1").click();
    await sleep(300);
    if ((await log()) !== "request screen,release") throw new Error(`끈 뒤 ${await log()}`);
    await wp.context().close();
  });

  await step("앱 설치(herdr-web-ui install.ts·MachineSidebar·설정 「휴대폰 → 설치」, 차이 목록 M4): 설치 전엔 단추가 늘 있고, 신호가 없으면 누를 때 이 브라우저의 방법을 펼치며, 신호가 오면 그 창을 띄우고, 설치되면 숨는다", async () => {
    // 브라우저가 스스로 보내는 beforeinstallprompt는 시험이 보낸 것만 남긴다(headless가 보낼지 말지에 기대지 않는다)
    const init = () => {
      const log = [];
      Object.defineProperty(window, "__installLog", { value: log });
      window.addEventListener(
        "beforeinstallprompt",
        (e) => {
          if (!e.e2e) e.stopImmediatePropagation();
        },
        true,
      );
    };
    const offer = (p) =>
      p.evaluate(() => {
        const e = new Event("beforeinstallprompt", { cancelable: true });
        e.e2e = true;
        e.prompt = async () => void window.__installLog.push("prompt");
        e.userChoice = Promise.resolve({ outcome: "dismissed", platform: "" });
        window.dispatchEvent(e);
        return e.defaultPrevented;
      });
    const dialogRow = async (p) => {
      const row = (await openSettings(p, "폰 및 기기")).locator(".settings-row", { hasText: /^설치/ });
      const text = (await row.innerText()).replace(/\s+/g, " ");
      const button = await row.getByRole("button", { name: "앱 설치" }).count();
      await p.keyboard.press("Escape");
      return { text, button };
    };
    const ip = (await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 }, { init })).page;
    const btn = ip.getByTestId("install-app");
    await until("설치 단추", () => btn.isVisible());
    // 신호 없음(데스크톱 Chromium): 누르면 주소창 안내를 펼치고, 다시 누르면 접는다
    if ((await btn.getAttribute("aria-expanded")) !== "false") throw new Error(`aria-expanded ${await btn.getAttribute("aria-expanded")}`);
    await btn.click();
    const help = ip.getByTestId("install-help");
    if ((await help.innerText()) !== "주소창의 설치 아이콘이나 브라우저 메뉴의 설치 항목을 쓰세요.") throw new Error(`안내 ${await help.innerText()}`);
    if ((await btn.getAttribute("aria-expanded")) !== "true") throw new Error("펼친 뒤 aria-expanded");
    let row = await dialogRow(ip);
    if (!row.text.includes("주소창의 설치 아이콘") || row.button !== 0) throw new Error(`설정 줄(신호 없음) ${JSON.stringify(row)}`);
    await btn.click();
    if (await help.count()) throw new Error("다시 눌러도 안내가 남았다");
    // 신호가 오면 기본 창을 막아 두고(preventDefault), 단추가 그 창을 띄운다
    if (!(await offer(ip))) throw new Error("beforeinstallprompt 기본 동작을 막지 않았다");
    await until("신호 받은 단추", async () => (await btn.getAttribute("aria-expanded")) === null);
    row = await dialogRow(ip);
    if (row.button !== 1 || row.text.includes("주소창")) throw new Error(`설정 줄(신호 있음) ${JSON.stringify(row)}`);
    await btn.click();
    await until("설치 창", async () => (await ip.evaluate(() => window.__installLog.join(","))) === "prompt");
    // 거절하면 그 신호는 다 썼다: 다시 안내로
    await until("거절 뒤 안내로", async () => (await btn.getAttribute("aria-expanded")) === "false");
    // 설치되면 단추가 숨고 설정은 「설치됨」
    await ip.evaluate(() => window.dispatchEvent(new Event("appinstalled")));
    await until("설치 뒤 단추 숨김", async () => (await btn.count()) === 0);
    row = await dialogRow(ip);
    if (!row.text.includes("설치됨") || row.button !== 0) throw new Error(`설정 줄(설치됨) ${JSON.stringify(row)}`);
    await ip.context().close();
    // 브라우저마다 안내(참고 installHelp 시험의 기기들). iPadOS는 Mac UA라 터치 점 수로 가린다:
    // Chromium 터치 흉내는 maxTouchPoints가 1이라 실제 iPad처럼 5로 둔다(참고 시험도 5)
    const IPHONE = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1";
    const MAC_SAFARI = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15";
    const FIREFOX = "Mozilla/5.0 (X11; Linux x86_64; rv:130.0) Gecko/20100101 Firefox/130.0";
    const ANDROID = "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0 Mobile Safari/537.36";
    // addInitScript는 함수 글만 옮긴다(바깥 init을 부를 수 없다)
    const ipad = () => {
      window.addEventListener(
        "beforeinstallprompt",
        (e) => {
          if (!e.e2e) e.stopImmediatePropagation();
        },
        true,
      );
      Object.defineProperty(navigator, "maxTouchPoints", { get: () => 5 });
    };
    for (const [name, viewport, userAgent, touch, want, script] of [
      ["iPhone", { width: 390, height: 844 }, IPHONE, true, "공유 버튼을 누른 뒤 홈 화면에 추가를 선택하세요.", init],
      ["iPad(Mac UA·터치)", { width: 1024, height: 1366 }, MAC_SAFARI, true, "공유 버튼을 누른 뒤 홈 화면에 추가를 선택하세요.", ipad],
      ["Mac Safari", { width: 1280, height: 800 }, MAC_SAFARI, false, "파일 메뉴에서 Dock에 추가를 선택하세요.", init],
      ["Android", { width: 412, height: 915 }, ANDROID, true, "브라우저 메뉴에서 앱 설치 또는 홈 화면에 추가를 선택하세요.", init],
      ["데스크톱 Firefox", { width: 1280, height: 800 }, FIREFOX, false, "데스크톱 Firefox는 웹 앱을 설치할 수 없습니다. Chrome이나 Edge로 여세요.", init],
    ]) {
      const p = (await pairPage(browser, aaa("web", "pair").code, viewport, { userAgent, touch, init: script })).page;
      await until(`${name} 설치 단추`, () => p.getByTestId("install-app").isVisible());
      await p.getByTestId("install-app").click();
      const got = await p.getByTestId("install-help").innerText();
      if (got !== want) throw new Error(`${name} 안내: ${got}`);
      await p.context().close();
    }
    // 홈 화면 앱(iOS navigator.standalone)으로 열면 단추가 없고 설정은 「설치됨」
    const sp = (
      await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 }, {
        userAgent: IPHONE,
        init: () => Object.defineProperty(navigator, "standalone", { get: () => true }),
      })
    ).page;
    await until("홈 화면 앱 목록", () => sp.getByTestId("settings-open").isVisible());
    if (await sp.getByTestId("install-app").count()) throw new Error("설치된 앱에 설치 단추");
    row = await dialogRow(sp);
    if (!row.text.includes("설치됨")) throw new Error(`홈 화면 앱 설정 줄 ${JSON.stringify(row)}`);
    await sp.context().close();
  });

  await step("기기·pairing(herdr-web-ui DevicesPanel·AccessGate·PhonePanel, 차이 목록 P1·P2·P3, D6): 설정 → 기기에서 코드·QR, QR 주소는 바로 내고 주소에서 지움, 새 코드는 앞 코드를 버림, 틀린 코드는 말, 이름 바꾸기, 폐기하면 열린 화면이 바로 pairing 화면, 보기 기기는 목록만, 로그아웃", async () => {
    // 휴대폰이 열 주소는 host의 tailscale serve 상태(`web_status`)다. 시험 host의 실제 tailscale에 기대지 않게 답을 바꾼다
    const served = "https://mac-a.example.ts.net:8443";
    const fakePublish = async (route) => {
      const req = route.request();
      if (req.method() === "POST" && JSON.parse(req.postData() ?? "{}").op === "web_status") {
        const body = { ok: { kind: "publish", tailscale: "running", dns_name: "mac-a.example.ts.net", url: served, owned_port: null, web_port: 0, message: null } };
        return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(body) });
      }
      return route.continue();
    };
    const openDevices = async (p) => {
      const dialog = await openSettings(p, "폰 및 기기");
      await dialog.locator(".devices-panel").scrollIntoViewIfNeeded();
      return dialog;
    };
    const desk = (await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 }, { route: ["**/api/v1/call", fakePublish] })).page;
    const dlg = await openDevices(desk);
    // 폰: 공개 주소와 그 QR
    await dlg.getByRole("img", { name: `QR 코드: ${served}` }).waitFor();
    // 기기: 이 기기 표시와 역할
    const mine = await desk.evaluate(async () => (await (await fetch("/api/v1/me")).json()).ok.device);
    const mineRow = dlg.locator(`.devices-row[data-device="${mine}"]`);
    await mineRow.locator(".devices-current", { hasText: "이 기기" }).waitFor();
    await dlg.getByRole("button", { name: "기기 페어링" }).click();
    const codeText = async () => (await dlg.locator(".devices-code").textContent()).replace(/\s/g, "");
    await dlg.locator(".devices-code").waitFor();
    const first = await codeText();
    if (!/^\d{6}$/.test(first)) throw new Error(`코드 ${first}`);
    const qr = dlg.getByRole("img", { name: `코드 ${first}로 폰을 페어링하는 QR 코드` });
    await qr.waitFor();
    const link = await dlg.locator(".devices-link a").getAttribute("href");
    if (link !== `${served}/?pair=${first}`) throw new Error(`QR 주소 ${link}`);
    if (!/[0-9]:[0-5][0-9] 뒤 만료됩니다/.test(await dlg.locator(".devices-pairing").textContent())) throw new Error("남은 시간이 없다");
    await shot(desk, "settings-devices-desktop");
    // 새 코드는 앞 코드를 버린다(참고 startPairing)
    await dlg.getByRole("button", { name: "새 코드" }).click();
    await until("새 코드", async () => (await codeText()) !== first);
    const second = await codeText();
    const pr = await fetch(`${base}api/v1/pair`, { method: "POST", headers: { "content-type": "application/json", origin: base.slice(0, -1) }, body: JSON.stringify({ code: first, label: "x" }) });
    if (pr.status !== 401) throw new Error(`버린 코드가 ${pr.status}`);
    // 휴대폰이 QR 주소를 연다: 코드를 바로 내고, 주소에서 지운다(기록·referrer에 남지 않게)
    const phoneCtx = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true, userAgent: "Mozilla/5.0 (iPhone; CPU iPhone OS 18_7 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/27.0.1 Mobile/15E148 Safari/604.1" });
    const phone = await phoneCtx.newPage();
    openPages.add(phone);
    phone.on("close", () => openPages.delete(phone));
    await phone.goto(`${base}?pair=${second}`);
    await phone.getByTestId("machine-labA").waitFor();
    if (phone.url().includes("pair=")) throw new Error(`주소에 코드가 남았다: ${phone.url()}`);
    // 데스크톱 화면은 그 코드로 들어온 기기를 알아채고 코드를 닫는다
    await dlg.getByRole("status").filter({ hasText: "페어링됨: iPhone · Safari." }).waitFor();
    await dlg.locator(".devices-pairing").waitFor({ state: "detached" });
    const phoneId = await phone.evaluate(async () => (await (await fetch("/api/v1/me")).json()).ok.device);
    const phoneRow = dlg.locator(`.devices-row[data-device="${phoneId}"]`);
    await phoneRow.waitFor();
    // 이름 바꾸기
    await phoneRow.getByRole("button", { name: "iPhone · Safari 이름 바꾸기" }).click();
    await phoneRow.getByLabel("기기 이름").fill("  내   폰  ");
    await phoneRow.getByRole("button", { name: "이름 저장" }).click();
    await phoneRow.locator(".settings-label", { hasText: "내 폰" }).waitFor();
    const listed = aaa("web", "devices").devices.find((d) => d.id === phoneId);
    if (listed?.label !== "내 폰" || listed.role !== "controller") throw new Error(`CLI 목록 ${JSON.stringify(listed)}`);
    // 폰은 터미널을 연다(그 WebSocket도 바로 닫혀야 한다)
    await phone.getByTestId("pane-t1").click();
    await until("폰 t1 화면", async () => (await screenText(phone)).includes("$"));
    // 폐기: 두 번 눌러야 한다
    const revoke = phoneRow.getByRole("button", { name: "폐기" });
    await revoke.click();
    await phoneRow.getByRole("button", { name: "폐기할까요?" }).click();
    await phoneRow.waitFor({ state: "detached" });
    // 폰의 열린 화면(events·터미널 WebSocket)은 4401로 닫혀 바로 pairing 화면이 된다
    await phone.getByLabel("페어링 코드").waitFor();
    await shot(phone, "phone-pair-gate-after-revoke");
    const after = await phone.evaluate(async () => (await fetch("/api/v1/me")).status);
    if (after !== 401) throw new Error(`폐기한 기기가 ${after}`);
    // 틀린 코드는 말한다
    await phone.getByLabel("페어링 코드").fill("000000");
    await phone.getByRole("button", { name: "이 기기 페어링" }).click();
    await phone.getByRole("alert").filter({ hasText: "코드가 틀렸거나 이미 쓰였거나 만료되었습니다" }).waitFor();
    // 휴대폰·iPad의 설정 → 기기(코드가 나가 있는 모습)
    for (const [name, viewport, touch] of [
      ["settings-devices-phone", { width: 390, height: 844 }, true],
      ["settings-devices-ipad", { width: 820, height: 1180 }, true],
    ]) {
      const p = (await pairPage(browser, aaa("web", "pair").code, viewport, { touch, route: ["**/api/v1/call", fakePublish] })).page;
      const d = await openDevices(p);
      await d.getByRole("button", { name: "기기 페어링" }).click();
      await d.locator(".devices-pairing").scrollIntoViewIfNeeded();
      await d.getByRole("img", { name: /로 폰을 페어링하는 QR 코드$/ }).waitFor();
      // 화면 밖으로 넘치지 않는다
      const over = await p.evaluate(() => [...document.querySelectorAll(".devices-panel *")].filter((e) => e.getBoundingClientRect().right > window.innerWidth + 0.5).map((e) => e.className).slice(0, 3));
      if (over.length) throw new Error(`${name} 넘침: ${over}`);
      await shot(p, name);
      await p.context().close();
    }
    // 보기 기기: 목록은 보고, 페어링·이름·폐기는 못 한다(host도 막는다)
    const rd = (await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 390, height: 844 })).page;
    const rdlg = await openDevices(rd);
    await rdlg.getByText("보기 권한 기기에서는 기기를 페어링하거나 바꿀 수 없습니다.").waitFor();
    if (await rdlg.getByRole("button", { name: "기기 페어링" }).count()) throw new Error("보기 기기에 페어링 단추");
    const rdId = await rd.evaluate(async () => (await (await fetch("/api/v1/me")).json()).ok.device);
    const rdRow = rdlg.locator(`.devices-row[data-device="${rdId}"]`);
    await rdRow.getByText(/^보기만 · /).waitFor();
    if (!(await rdRow.getByRole("button", { name: "폐기" }).isDisabled())) throw new Error("보기 기기의 폐기 단추가 켜져 있다");
    const minted = await rd.evaluate(async () =>
      (await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ op: "web_pair", role: "controller" }) })).json(),
    );
    if (minted.err?.code !== "forbidden") throw new Error(`보기 기기가 코드를 만들었다: ${JSON.stringify(minted)}`);
    await rd.context().close();
    // 웹 controller도 보기 코드는 못 만든다(D6: 웹은 controller만)
    const readerCode = await desk.evaluate(async () =>
      (await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ op: "web_pair", role: "reader" }) })).json(),
    );
    if (readerCode.err?.code !== "forbidden") throw new Error(`웹이 보기 코드를 만들었다: ${JSON.stringify(readerCode)}`);
    // 로그아웃: 확인 창(취소가 focus) → 로그아웃 → pairing 화면. 기기 기록은 남는다(참고와 같다)
    await dlg.getByRole("button", { name: "로그아웃" }).click();
    const confirm = desk.getByRole("alertdialog", { name: "이 브라우저에서 로그아웃할까요?" });
    await confirm.waitFor();
    // focus는 창이 그려진 다음 frame에 취소로 간다(useFocusTrap): frame마다 본다
    await desk.waitForFunction(() => document.activeElement?.textContent === "취소" && document.activeElement.closest('[role="alertdialog"]') !== null, null, { polling: "raf", timeout: 5000 });
    await shot(desk, "settings-sign-out-confirm");
    await confirm.getByRole("button", { name: "로그아웃" }).click();
    await desk.getByLabel("페어링 코드").waitFor();
    if (!aaa("web", "devices").devices.some((d) => d.id === mine)) throw new Error("로그아웃한 기기 기록이 없어졌다");
    aaa("web", "revoke", mine);
    await desk.context().close();
    await phoneCtx.close();
  });

  await step("설정 쪽(herdr-web-ui SettingsDialog PAGES·settings-nav·settingsHistory, 차이 목록 T1): 넓은 창은 목록 옆에 쪽·화살표로 넘김, 휴대폰은 목록 → 쪽 → 뒤로, 시스템 뒤로는 한 걸음씩, Esc는 닫고 focus는 톱니로", async () => {
    const bad = [];
    const NAMES = ["모양", "터미널", "알림", "구독 사용량", "폰 및 기기"];
    const tabs = (dialog) => dialog.getByRole("tab").allInnerTexts();
    const focused = (p) => p.evaluate(() => `${document.activeElement?.getAttribute("role") ?? document.activeElement?.tagName}:${document.activeElement?.textContent?.trim() || document.activeElement?.getAttribute("aria-label")}`);
    const closed = (p) => until("설정 닫힘", async () => (await p.getByRole("dialog", { name: "설정" }).count()) === 0);
    // 넓은 창: 목록과 첫 쪽(모양)이 나란히, 고른 줄에 focus, 화살표·End가 목록을 걷고 쪽도 넘긴다
    const dp = (await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 })).page;
    await dp.getByTestId("settings-open").click();
    const dlg = dp.getByRole("dialog", { name: "설정" });
    await dlg.getByRole("tabpanel", { name: "모양" }).waitFor();
    if (JSON.stringify(await tabs(dlg)) !== JSON.stringify(NAMES)) bad.push(`넓은 창 목록 ${await tabs(dlg)}`);
    const side = await dp.evaluate(() => {
      // 참고 배치: 목록은 왼쪽 열 전체, 오른쪽 열은 머리(쪽 제목·닫기) 아래에 쪽
      const nav = document.querySelector(".settings-nav").getBoundingClientRect();
      const head = document.querySelector(".settings-header").getBoundingClientRect();
      const body = document.querySelector(".settings-body").getBoundingClientRect();
      return { navRight: Math.round(nav.right), bodyLeft: Math.round(body.left), top: Math.abs(nav.top - head.top) < 2 && body.top >= head.bottom - 1 };
    });
    if (side.navRight > side.bodyLeft || !side.top) bad.push(`넓은 창 목록·쪽이 나란하지 않다 ${JSON.stringify(side)}`);
    await until("고른 줄에 focus", async () => (await focused(dp)) === "tab:모양").catch(async (e) => bad.push(`${e.message}: ${await focused(dp)}`));
    await dp.keyboard.press("ArrowDown");
    await dlg.getByRole("tabpanel", { name: "터미널" }).waitFor();
    if ((await dlg.getByRole("tab", { name: "터미널" }).getAttribute("aria-selected")) !== "true") bad.push("화살표로 넘긴 줄이 골라지지 않았다");
    await dp.keyboard.press("End");
    await dlg.getByRole("tabpanel", { name: "폰 및 기기" }).waitFor();
    await dp.keyboard.press("Home");
    await dlg.getByRole("tabpanel", { name: "모양" }).waitFor();
    await dlg.getByRole("tab", { name: "알림" }).click();
    await dlg.getByRole("tabpanel", { name: "알림" }).waitFor();
    // 고른 줄의 바탕은 transition으로 옮겨 간다(참고 .settings-tab): 다 옮긴 뒤 고른 줄만 바탕이 있는지 보고 찍는다
    await dp.mouse.move(1200, 780);
    const marks = await dp.evaluate(async () => {
      await Promise.all(document.getAnimations().map((a) => a.finished.catch(() => null)));
      return [...document.querySelectorAll(".settings-tab")].map((t) => `${t.textContent.trim()}:${getComputedStyle(t).backgroundColor !== "rgba(0, 0, 0, 0)"}`);
    });
    if (marks.join() !== "모양:false,터미널:false,알림:true,구독 사용량:false,폰 및 기기:false") bad.push(`고른 줄 바탕 ${marks}`);
    await shot(dp, "settings-pages-desktop");
    await dp.keyboard.press("Escape");
    await closed(dp);
    if ((await dp.evaluate(() => document.activeElement?.dataset.testid)) !== "settings-open") bad.push(`닫은 뒤 focus ${await focused(dp)}`);
    // 넓은 창도 쪽마다 history 한 칸: 뒤로가 창을 닫는다
    await openSettings(dp, "터미널");
    await dp.goBack();
    await closed(dp);
    await dp.context().close();

    // 휴대폰: 목록으로 열고(쪽 없음), 고르면 쪽만, ←·시스템 뒤로는 목록으로, 그 줄에 focus
    const pp = (await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 }, { touch: true })).page;
    await pp.getByTestId("settings-open").click();
    const pd = pp.getByRole("dialog", { name: "설정" });
    await pd.getByRole("tab", { name: "모양" }).waitFor();
    if ((await pd.getByRole("tabpanel").count()) !== 0) bad.push("휴대폰이 목록과 쪽을 함께 보인다");
    if (JSON.stringify(await tabs(pd)) !== JSON.stringify(NAMES)) bad.push(`휴대폰 목록 ${await tabs(pd)}`);
    // sheet는 아래에서 올라온다(modal.css sheet-in): 다 올라온 뒤 잰다
    const fit = await pp.evaluate(async () => {
      const sheet = document.querySelector(".settings-dialog");
      await Promise.all(sheet.getAnimations().map((a) => a.finished));
      const r = sheet.getBoundingClientRect();
      return { left: Math.round(r.left), right: Math.round(r.right), bottom: Math.round(r.bottom), page: document.documentElement.scrollWidth };
    });
    if (fit.left < 0 || fit.right > 390 || fit.bottom > 844 || fit.page > 390) bad.push(`휴대폰 설정 창이 넘친다 ${JSON.stringify(fit)}`);
    await shot(pp, "settings-pages-phone-list");
    await pd.getByRole("tab", { name: "터미널" }).click();
    await pd.getByRole("tabpanel", { name: "터미널" }).waitFor();
    if ((await pd.getByRole("tab").count()) !== 0) bad.push("휴대폰 쪽에 목록이 남았다");
    if ((await pd.locator(".modal-title").innerText()) !== "터미널") bad.push(`휴대폰 쪽 제목 ${await pd.locator(".modal-title").innerText()}`);
    await until("쪽으로 들어간 focus", async () => (await focused(pp)) === "BUTTON:설정으로 돌아가기").catch(async (e) => bad.push(`${e.message}: ${await focused(pp)}`));
    await shot(pp, "settings-pages-phone-terminal");
    // 키 바 편집 → 뒤로 → 터미널 쪽(「키 바 편집」에 focus) → 뒤로 → 목록(「터미널」 줄에 focus) → 뒤로 → 닫힘
    await pd.getByRole("button", { name: "키 바 편집" }).click();
    await pp.getByRole("dialog", { name: "키 바" }).waitFor();
    await pp.goBack();
    await pd.getByRole("tabpanel", { name: "터미널" }).waitFor();
    await until("키 바에서 돌아온 focus", async () => (await focused(pp)) === "BUTTON:키 바 편집").catch(async (e) => bad.push(`${e.message}: ${await focused(pp)}`));
    await pp.goBack();
    await pd.getByRole("tab", { name: "터미널" }).waitFor();
    await until("목록으로 나온 focus", async () => (await focused(pp)) === "tab:터미널").catch(async (e) => bad.push(`${e.message}: ${await focused(pp)}`));
    await pp.goBack();
    await closed(pp);
    // ←는 목록으로, Esc는 쪽에서도 창을 닫는다
    await openSettings(pp, "알림");
    await pp.getByRole("button", { name: "설정으로 돌아가기" }).click();
    await pd.getByRole("tab", { name: "알림" }).waitFor();
    await pd.getByRole("tab", { name: "폰 및 기기" }).click();
    await pd.getByRole("tabpanel", { name: "폰 및 기기" }).waitFor();
    await pp.keyboard.press("Escape");
    await closed(pp);
    // X로 닫으면 쌓은 칸을 지운다(settingsHistory): 지금 칸은 설정이 아니다
    await openSettings(pp, "모양");
    await pp.getByRole("button", { name: "설정 닫기" }).click();
    await closed(pp);
    await until("설정 칸이 지워짐", () => pp.evaluate(() => history.state?.["aaa:settings"] == null)).catch((e) => bad.push(e.message));
    await pp.context().close();

    // iPad(WebKit 820×1180): 넓은 창 배치(목록 옆에 쪽)
    const wk = await webkit.launch();
    try {
      const ip = (await pairPage(wk, aaa("web", "pair").code, { width: 820, height: 1180 }, { touch: true })).page;
      const id = await openSettings(ip, "폰 및 기기");
      if ((await id.getByRole("tab").count()) !== NAMES.length) bad.push("iPad에 목록이 없다");
      const over = await ip.evaluate(() => [...document.querySelectorAll(".settings-dialog *")].filter((e) => e.getBoundingClientRect().right > window.innerWidth + 0.5).length);
      if (over) bad.push(`iPad 설정 창 넘침 ${over}개`);
      await shot(ip, "settings-pages-ipad");
      await ip.context().close();
    } finally {
      await wk.close();
    }
    if (bad.length) throw new Error(bad.join("; "));
  });

  await step("터미널 글꼴(herdr-web-ui 설정 「터미널 글꼴」, 차이 목록 「글꼴 종류」): 적은 목록을 거르고 기본 글꼴 앞에, face를 받은 뒤 cell을 재고, 찾아보기는 고정폭만, 비우면 기본", async () => {
    aaa("tab", "create", "--machine", "labA", "--label", "fontset", "--", process.execPath, `${root}/scroll.js`, "plain");
    await waitOutput("fontset", "s-200");
    // 시험 글꼴 「E2E Wide」: 넣은 Meslotendard를 다른 주소로 150% 크기로 받는다(깔린 글꼴에 기대지 않는다).
    // 깔린 글꼴 목록(Chromium queryLocalFonts)은 정해 둔 셋으로 바꾼다. Helvetica는 비례폭이라 거절해야 한다.
    const init = () => {
      try {
        document.fonts.add(new FontFace("E2E Wide", 'url("/fonts/meslotendard-0.3.1/Meslotendard-Regular.woff2?e2e-wide")', { sizeAdjust: "150%" }));
        window.queryLocalFonts = async () => [{ family: "Menlo" }, { family: "Helvetica" }, { family: "E2E Wide" }];
      } catch {}
    };
    const fp = (await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 }, { init })).page;
    // 그 face는 늦게 준다: 받기 전에 cell을 재면 대신 글꼴 너비가 남아 아래 1.5배 확인이 실패한다
    let wideFetches = 0;
    await fp.context().route((url) => url.search === "?e2e-wide", async (r) => {
      wideFetches += 1;
      await sleep(800);
      await r.continue();
    });
    await fp.getByTestId("pane-fontset").click();
    await until("fontset 화면", async () => (await screenText(fp)).includes("s-200"));
    // cell 너비 = 글자 영역 너비 / 열 수(머리의 cols×rows), 그리고 host PTY 열 수
    const cell = async () => {
      const m = await fp.evaluate(() => {
        const w = document.querySelector('[data-testid="terminal"] .xterm-screen').getBoundingClientRect().width;
        const cols = Number(/(\d+)×\d+/.exec(document.querySelector(".term-title .muted").textContent)?.[1]);
        return { w: w / cols, cols };
      });
      return { ...m, pty: aaa("pane", "list").panes.find((x) => x.name === "fontset").cols };
    };
    let last = null;
    const settle = (what, ok) =>
      until(what, async () => {
        last = await cell();
        return last.cols === last.pty && ok(last);
      }).catch((e) => {
        throw new Error(`${e.message} 마지막: ${JSON.stringify(last)}`);
      });
    await settle("처음 크기", () => true);
    const first = last;
    const field = () => fp.getByRole("dialog", { name: "설정" }).getByRole("textbox", { name: "터미널 글꼴" });
    const stored = () => fp.evaluate(() => localStorage.getItem("aaa:terminal-font-family"));
    // 찾아보기: 이름순, 검색, 비례폭 거절(값을 바꾸지 않음), 고정폭을 고르면 그것이 값
    const dlg = await openSettings(fp, "터미널");
    await dlg.getByRole("button", { name: "터미널 글꼴에 설치된 글꼴 찾아보기" }).click();
    const listed = await dlg.getByRole("list", { name: "설치된 글꼴" }).getByRole("button").allInnerTexts();
    if (JSON.stringify(listed) !== JSON.stringify(["E2E Wide", "Helvetica", "Menlo"])) throw new Error(`글꼴 목록 ${listed}`);
    await dlg.getByRole("searchbox", { name: "글꼴 검색" }).fill("hel");
    await dlg.getByRole("button", { name: "Helvetica" }).click();
    await dlg.getByText("Helvetica은(는) 고정폭 글꼴이 아닙니다.", { exact: false }).waitFor();
    await shot(fp, "desktop-terminal-font");
    if ((await field().inputValue()) !== "" || (await stored()) !== null) throw new Error("비례폭 글꼴을 받아들였다");
    // 검색칸의 Esc는 목록만 닫는다
    await dlg.getByRole("searchbox", { name: "글꼴 검색" }).press("Escape");
    await dlg.getByRole("list", { name: "설치된 글꼴" }).waitFor({ state: "detached" });
    await dlg.getByRole("button", { name: "터미널 글꼴에 설치된 글꼴 찾아보기" }).click();
    await dlg.getByRole("button", { name: "E2E Wide" }).click();
    if ((await field().inputValue()) !== '"E2E Wide"' || (await stored()) !== '"E2E Wide"') throw new Error(`고른 값 ${await field().inputValue()} / ${await stored()}`);
    // 그 face를 받은 뒤 넣으므로 cell은 1.5배, grid는 줄고 host PTY도 그 열 수
    await settle("넓은 글꼴로 다시 맞춤", (c) => Math.abs(c.w / first.w - 1.5) < 0.05 && c.cols < first.cols);
    if (wideFetches !== 1) throw new Error(`E2E Wide를 ${wideFetches}번 받았다`);
    // 적은 값은 거른다(세미콜론·중괄호를 빼고, 빈 이름을 버리고, 빈칸 있는 이름은 따옴표). Esc로 닫아도 저장한다
    await field().fill("'E2E Wide'; }, , Menlo");
    await fp.keyboard.press("Escape");
    await until("설정 닫힘", async () => (await fp.locator(".settings-dialog").count()) === 0);
    if ((await stored()) !== '"E2E Wide", Menlo') throw new Error(`거른 값 ${await stored()}`);
    // 다시 열어도 이 기기의 글꼴: 연 뒤 face를 받고 나서 잰다
    await fp.reload();
    await fp.getByTestId("pane-fontset").click();
    await settle("다시 연 화면도 넓은 글꼴", (c) => Math.abs(c.w / first.w - 1.5) < 0.05);
    if (wideFetches !== 2) throw new Error(`다시 열고 E2E Wide를 ${wideFetches}번째로 받지 않았다`);
    // 비우면(Enter) 기본 글꼴, 저장값도 지운다
    await openSettings(fp, "터미널");
    await field().fill("");
    await field().press("Enter");
    if ((await stored()) !== null) throw new Error(`비웠는데 저장값 ${await stored()}`);
    await fp.keyboard.press("Escape");
    await settle("기본 글꼴로 돌아옴", (c) => Math.abs(c.w - first.w) < 0.05 && c.cols === first.cols);
    await fp.context().close();
    aaa("pane", "kill", "fontset");
  });

  await step("구독 사용량·설정(herdr-web-ui UsageMeters·SettingsDialog): 목록 아래 톱니와 Claude·Codex 칩(herdr-web-ui AgentMark), 펼치면 한도·초기화까지, 다른 구독은 묻지 않음, host는 5분 캐시, 설정에서 남은 양·세션·숨기기·순서·입력 방식", async () => {
    const dp = (await pairPage(browser, aaa("web", "pair").code, { width: 1100, height: 760 })).page;
    const strip = dp.getByTestId("usage-strip");
    await strip.waitFor();
    const chips = () => strip.locator(".usage-chip").evaluateAll((els) => els.map((e) => `${e.dataset.provider} ${e.querySelector(".usage-chip-value").textContent}${e.classList.contains("is-high") ? " high" : ""}${e.classList.contains("has-problem") ? " problem" : ""}`));
    const bad = [];
    // 기본: 쓴 양·주간. 80% 넘으면 막힘 색. 늦추라고 한 구독은 숫자 없이 흐리게
    let got = JSON.stringify(await chips());
    // 구독은 Claude·Codex만 보인다: 이 집에 OpenCode 로그인이 있어도 묻지도 보이지도 않는다(사용자 요청 10-17)
    if (got !== JSON.stringify(["claude 95% high", "codex 73%"])) bad.push(`기본 칩 ${got}`);
    // 구독 표시는 herdr-web-ui AgentMark다: Claude는 주황 타일의 별, Codex는 글자색 매듭
    const marks = await strip.locator(".usage-chip .agent-mark").evaluateAll((els) =>
      els.map((e) => `${e.className} ${e.querySelector("svg").getAttribute("aria-label")} ${e.querySelector("svg").getAttribute("width")} ${e.querySelector("path").getAttribute("fill")}`),
    );
    if (JSON.stringify(marks) !== JSON.stringify(["agent-mark claude is-brand claude 14 #D77655", "agent-mark codex codex 14 currentColor"])) bad.push(`구독 표시 ${JSON.stringify(marks)}`);
    if ((await dp.getByTestId("settings-open").innerText()).trim() !== "설정") bad.push("톱니 「설정」이 없다");
    // 펼치면 구독마다 한도·초기화까지·계정·plan
    await strip.click();
    const pop = dp.locator(".usage-popover");
    await pop.waitFor();
    const claude = await pop.getByRole("region", { name: "Claude · me@example.com" }).innerText();
    for (const want of ["Max", "세션", "20%", "주간", "95%", "주간 · Sonnet", "40%", "뒤 초기화"]) if (!claude.includes(want)) bad.push(`펼친 Claude에 「${want}」가 없다: ${JSON.stringify(claude)}`);
    if ((await pop.getByRole("region", { name: "OpenCode" }).count()) !== 0) bad.push("펼친 창에 OpenCode가 있다");
    // 화면을 여러 번 열었어도 host는 구독마다 한 번만 물었다(5분 캐시). OpenCode에는 묻지 않았다
    await dp.reload();
    await strip.waitFor();
    await sleep(300);
    const hits = ["/api/oauth/usage", "/backend-api/wham/usage", "/zen/go/v1/usage"].map((p) => usageHits.get(p) ?? 0);
    if (JSON.stringify(hits) !== "[1,1,0]") bad.push(`구독에 물은 수 ${JSON.stringify(hits)}`);
    if (shots) await dp.screenshot({ path: path.join(shots, "usage-popover.png") });
    // 설정: 남은 양, 세션
    const dlg = await openSettings(dp, "구독 사용량");
    await dlg.getByRole("group", { name: "막대가 보이는 것" }).getByRole("button", { name: "남은 양" }).click();
    await until("남은 양", async () => JSON.stringify(await chips()) === JSON.stringify(["claude 5% high", "codex 27%"]));
    await dlg.getByRole("group", { name: "보이는 한도" }).getByRole("button", { name: "세션" }).click();
    await until("세션", async () => JSON.stringify(await chips()) === JSON.stringify(["claude 80%", "codex 90%"]));
    // 계정 숨기기와 순서
    const hideClaude = dlg.getByRole("switch", { name: "Claude · me@example.com 보이기" });
    await hideClaude.click();
    await until("숨기기", async () => JSON.stringify(await chips()) === JSON.stringify(["codex 90%"]));
    await hideClaude.click();
    await dlg.getByRole("button", { name: "Codex · me@example.com 위로" }).click();
    await until("순서", async () => JSON.stringify(await chips()) === JSON.stringify(["codex 90%", "claude 80%"]));
    if (shots) await dp.screenshot({ path: path.join(shots, "settings-dialog.png") });
    // 데스크톱에서 입력줄 고르기(D1에서 남은 빈 곳): 입력 방식은 「터미널」 쪽
    await dlg.getByRole("tab", { name: "터미널" }).click();
    await dlg.getByLabel("입력 방식").selectOption("line");
    await dp.keyboard.press("Escape");
    await until("설정 닫힘", async () => (await dp.getByRole("dialog", { name: "설정" }).count()) === 0);
    await dp.getByTestId("pane-t1").click();
    await until("입력줄", async () => (await dp.locator(".terminal-input").count()) === 1);
    // 다시 열어도 이 기기에 남는다. 끄면 막대도 사라진다
    await dp.reload();
    await until("남은 설정", async () => JSON.stringify(await chips()) === JSON.stringify(["codex 90%", "claude 80%"]));
    await (await openSettings(dp, "구독 사용량")).getByRole("switch", { name: "plan 한도 보이기" }).click();
    await until("막대 사라짐", async () => (await dp.getByTestId("usage-strip").count()) === 0);
    if (bad.length) throw new Error(bad.join("; "));
    await dp.context().close();
  });

  await step("agent 표시(herdr-web-ui AgentMark·Sidebar rowMark·App context-title, 사용자 요청 10-17): 목록 줄과 터미널 머리에 Claude는 주황 타일 별, Codex는 매듭, 셸은 터미널 표시", async () => {
    // hook으로 harness를 알린 pane(Claude·Codex)과 셸(t1)
    for (const [name, agent] of [["markc", "claude"], ["markx", "codex"]])
      aaa("tab", "create", "--machine", "labA", "--label", name, "--", "/bin/sh", "-c", `"$0" hook --agent ${agent} '{"hook_event_name":"Stop"}'; exec cat`, AA);
    const mp = (await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 })).page;
    for (const name of ["markc", "markx"]) await until(`${name} harness`, async () => (await mp.getByTestId(`agent-${name}`).count()) === 1);
    const markOf = (name) =>
      mp.getByTestId(`pane-${name}`).locator(".sidebar-mark").evaluate((m) => {
        const a = m.querySelector(".agent-mark");
        const svg = m.querySelector("svg");
        return {
          agent: m.dataset.agent,
          cls: a?.className ?? null,
          label: svg?.getAttribute("aria-label") ?? null,
          size: svg?.getAttribute("width") ?? null,
          tile: a?.querySelector(".agent-mark-tile")?.getAttribute("fill") ?? null,
          shell: svg?.classList.contains("lucide-terminal") ?? false,
        };
      });
    const bad = [];
    const c = await markOf("markc");
    if (c.cls !== "agent-mark claude is-brand" || c.label !== "claude" || c.size !== "18" || c.tile !== "#D77655") bad.push(`Claude 줄 ${JSON.stringify(c)}`);
    const x = await markOf("markx");
    if (x.cls !== "agent-mark codex" || x.label !== "codex" || x.size !== "18") bad.push(`Codex 줄 ${JSON.stringify(x)}`);
    // 셸은 agent 표시가 아니라 터미널 표시(참고 Sidebar.tsx rowMark: lucide Terminal)
    const sh = await markOf("t1");
    if (sh.agent !== "shell" || sh.cls !== null || !sh.shell) bad.push(`셸 줄 ${JSON.stringify(sh)}`);
    await shot(mp, "phone-agent-marks");
    // 터미널 머리: 제목 앞에 그 pane의 agent 표시(참고 App.tsx 1402줄, 18px)
    await mp.getByTestId("pane-markc").click();
    const head = mp.locator(".term-title .agent-mark");
    await head.waitFor();
    const hc = await head.evaluate((a) => `${a.className} ${a.querySelector("svg").getAttribute("width")}`);
    if (hc !== "agent-mark claude is-brand 18") bad.push(`머리 표시 ${hc}`);
    await shot(mp, "phone-agent-head");
    await mp.getByRole("button", { name: "목록으로" }).click();
    await mp.getByTestId("pane-t1").click();
    await mp.getByTestId("terminal").waitFor();
    if ((await mp.locator(".term-title .agent-mark").count()) !== 0) bad.push("셸 머리에 agent 표시가 있다");
    await mp.context().close();
    for (const name of ["markc", "markx"]) aaa("pane", "kill", name);
    if (bad.length) throw new Error(bad.join("; "));
  });

  await step("목록 바닥 줄은 늘 맨 아래(herdr-web-ui sidebar-footer, 사용자 요청 10-17): 목록이 길어도·굴려도 설정·사용량 줄은 화면 맨 아래, 휴대폰·iPad·데스크톱·화면 키보드", async () => {
    // 목록이 화면보다 길어야 시험이 된다: 짧으면 예전 판(바닥 줄이 목록 끝을 따라감)도 맨 아래에 있다
    const many = Array.from({ length: 24 }, (_, i) => `foot${i + 1}`);
    for (const name of many) aaa("tab", "create", "--machine", "labA", "--label", name, "--", "/bin/sleep", "600");
    const footer = (p) =>
      p.evaluate(() => {
        const r = (s) => document.querySelector(s).getBoundingClientRect();
        const body = document.querySelector(".list-body");
        return {
          view: Math.round(window.visualViewport?.height ?? window.innerHeight),
          app: Math.round(r(".app").bottom),
          list: Math.round(r(".list").bottom),
          foot: Math.round(r(".list-foot").bottom),
          gear: Math.round(r('[data-testid="settings-open"]').bottom),
          over: body.scrollHeight - body.clientHeight,
          top: Math.round(body.scrollTop),
        };
      });
    const bad = [];
    // 바닥 줄이 보이는 높이의 맨 아래에 붙어 있고(목록 끝을 따라가지 않고), 목록을 끝까지 굴려도 그 자리다
    const check = async (label, p) => {
      await until(`${label} 목록에 foot24`, async () => (await p.getByTestId("pane-foot24").count()) === 1);
      const at = await footer(p);
      if (at.over <= 0) bad.push(`${label}: 목록이 화면보다 짧아 시험이 안 된다 ${JSON.stringify(at)}`);
      if (at.top !== 0 || at.app !== at.view || at.list !== at.view || at.foot !== at.view || at.gear > at.view) bad.push(`${label}: 바닥 줄이 맨 아래가 아니다 ${JSON.stringify(at)}`);
      await p.locator(".list-body").evaluate((el) => (el.scrollTop = el.scrollHeight));
      const end = await footer(p);
      if (end.top === 0 || end.foot !== at.foot || end.gear !== at.gear) bad.push(`${label}: 목록을 굴리니 바닥 줄이 움직였다 ${JSON.stringify({ at, end })}`);
      await p.locator(".list-body").evaluate((el) => (el.scrollTop = 0));
      return at;
    };
    const phone = (await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 })).page;
    const ph = await check("휴대폰 390×844", phone);
    await phone.context().close();
    const desk = (await pairPage(browser, aaa("web", "pair").code, { width: 1100, height: 760 })).page;
    await check("데스크톱 1100×760", desk);
    await desk.context().close();
    const wk = await webkit.launch();
    try {
      const pad = (await pairPage(wk, aaa("web", "pair").code, { width: 820, height: 1180 }, { touch: true, init: fakeViewport })).page;
      const up = await check("iPad 820×1180", pad);
      if (up.view !== 1180) bad.push(`iPad 보이는 높이가 1180이 아니다(${up.view}): 키보드 시험이 안 된다`);
      // pane을 열고 화면 키보드를 올리면(보이는 높이 780) 바닥 줄은 키보드 바로 위에 남는다
      await pad.getByTestId("pane-t1").click();
      await pad.getByTestId("terminal").waitFor();
      // 키보드는 글 칸에 focus가 있을 때만 올라온다(herdr/viewport.ts)
      await pad.locator(".xterm-helper-textarea").focus();
      await softKeyboard(pad, 780);
      await until("iPad 키보드 위 바닥 줄", async () => (await footer(pad)).foot === 780).catch(async (e) => {
        throw new Error(`${e.message}: ${JSON.stringify(await footer(pad))}`);
      });
      await check("iPad 화면 키보드(보이는 높이 780)", pad);
      await softKeyboard(pad, 1180);
      await until("iPad 키보드 내림", async () => (await footer(pad)).foot === 1180);
      await pad.context().close();
    } finally {
      await wk.close();
    }
    for (const name of many) aaa("pane", "kill", name);
    if (bad.length) throw new Error(bad.join("; "));
    console.log(`     휴대폰 바닥 줄 아래 ${ph.foot}px(보이는 높이 ${ph.view}), 목록 넘침 ${ph.over}px`);
  });

  await step("글자 크기(herdr-web-ui terminalFontSize·핀치): 설정에서 한 칸씩, 휴대폰은 두 손가락으로 글자가 따라오고 손을 떼면 저장하고 PTY를 한 번 다시 건다", async () => {
    const t1 = () => aaa("pane", "list").panes.find((x) => x.name === "t1");
    const rowsFont = (pg) => pg.evaluate(() => getComputedStyle(document.querySelector('[data-testid="terminal"] .xterm-rows')).fontSize);
    /** 이 화면이 PTY 크기를 건 뒤 멈춘 열 수 */
    const settled = async (pg, what, ok) =>
      until(what, async () => {
        const a = t1().cols;
        await sleep(300);
        const b = t1().cols;
        return a === b && ok(b) ? b : null;
      });
    const dp = (await pairPage(browser, aaa("web", "pair").code, { width: 1100, height: 760 })).page;
    await dp.getByTestId("pane-t1").click();
    await until("13px로 연다", async () => (await rowsFont(dp)) === "13px");
    // 데스크톱(바로 입력)은 pane을 열면 터미널이 키보드를 받는다(herdr-web-ui PaneTerminal 2072줄). 입력줄이면 참고처럼 놓는다.
    await until("열면 터미널에 focus", () => dp.evaluate(() => document.activeElement?.classList.contains("xterm-helper-textarea")));
    await dp.getByTestId("terminal").click();
    const wide = await settled(dp, "데스크톱 크기", () => true);
    const dlg = await openSettings(dp, "터미널");
    for (let i = 0; i < 3; i++) await dlg.getByRole("button", { name: "글자 크게" }).click();
    if ((await dlg.getByRole("group", { name: "글자 크기" }).innerText()).trim() !== "16px") throw new Error("설정에 16px가 아니다");
    await until("16px", async () => (await rowsFont(dp)) === "16px");
    // 쓰는 화면이면 바뀐 글자로 PTY 크기를 다시 건다(13 → 16px이면 열이 줄어든다)
    const big = await settled(dp, "큰 글자 크기", (c) => c < wide * 0.9);
    if ((await dp.evaluate(() => localStorage.getItem("aaa:terminal-font-size"))) !== "16") throw new Error("16px 저장이 없다");
    for (let i = 0; i < 3; i++) await dlg.getByRole("button", { name: "글자 작게" }).click();
    await settled(dp, "13px로 돌아온 크기", (c) => c === wide).catch(async (e) => {
      // Term autoFit은 이 문서가 보이고 focus를 가질 때만 크기를 건다(inUse)
      const box = await dp.getByTestId("terminal").evaluate((el) => `${Math.round(el.getBoundingClientRect().width)}px, focus ${document.hasFocus()}, ${document.visibilityState}`);
      throw new Error(`${e.message}: 처음 ${wide}열, 지금 ${t1().cols}열, 글자 ${await rowsFont(dp)}, 설정 ${(await dlg.getByRole("group", { name: "글자 크기" }).innerText()).trim()}, 터미널 ${box}`);
    });
    // 기본값이면 저장하지 않는다
    if ((await dp.evaluate(() => localStorage.getItem("aaa:terminal-font-size"))) !== null) throw new Error("기본값을 저장했다");
    await dp.keyboard.press("Escape");
    await dp.context().close();
    console.log(`     글자 크기 ${wide}열(13px) → ${big}열(16px)`);

    // 휴대폰: 두 손가락을 벌리면 글자가 바로 커지고 크기를 띄운다. 손을 떼면 저장하고 PTY를 다시 건다.
    const mp = (await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 })).page;
    await mp.getByTestId("pane-t1").click();
    await until("휴대폰 13px", async () => (await rowsFont(mp)) === "13px");
    await mp.getByTestId("terminal").click();
    const phone = await settled(mp, "휴대폰 크기", (c) => c < wide);
    const r = await mp.getByTestId("terminal").boundingBox();
    const cx = r.x + r.width / 2;
    const cy = r.y + Math.min(r.height / 2, 200);
    const cdp = await mp.context().newCDPSession(mp);
    const touch = (type, d) =>
      cdp.send("Input.dispatchTouchEvent", {
        type,
        touchPoints: d === null ? [] : [{ x: cx - d / 2, y: cy, id: 1 }, { x: cx + d / 2, y: cy, id: 2 }],
      });
    await touch("touchStart", 100);
    await touch("touchMove", 130);
    await touch("touchMove", 150);
    // 13 × 150/100 = 19.5 → 20px. 손가락을 떼기 전에는 PTY도 저장도 그대로다
    await until("핀치 중 20px", async () => (await rowsFont(mp)) === "20px");
    const badge = await mp.getByTestId("pinch-badge").innerText();
    if (badge !== "20px") throw new Error(`핀치 표시 ${JSON.stringify(badge)}`);
    if (t1().cols !== phone) throw new Error("손을 떼기 전에 PTY가 바뀌었다");
    if ((await mp.evaluate(() => localStorage.getItem("aaa:terminal-font-size"))) !== null) throw new Error("손을 떼기 전에 저장했다");
    await touch("touchEnd", null);
    await until("핀치 표시 사라짐", async () => await mp.getByTestId("pinch-badge").isHidden());
    if ((await mp.evaluate(() => localStorage.getItem("aaa:terminal-font-size"))) !== "20") throw new Error("핀치 크기를 저장하지 않았다");
    const pinched = await settled(mp, "핀치 뒤 PTY", (c) => c < phone * 0.8);
    console.log(`     핀치 ${phone}열(13px) → ${pinched}열(20px)`);
    await mp.context().close();
  });

  await step("터미널 스크롤(herdr-web-ui PaneTerminal 휠·터치, herdr pane_input 휠 나누기, 사용자 요청 10-17): 열기 전 기록도 굴러가고, 출력이 와도 올려 둔 자리, 키를 치면 맨 아래, alternate screen은 화살표·mouse 보고면 프로그램에, 휴대폰·iPad는 한 손가락으로 손가락만큼", async () => {
    // 터치용 alt·mouse pane은 따로 둔다(휠로 받은 조각과 섞이지 않게)
    const names = ["plain", "alt", "mouse", "touch-alt", "touch-mouse"];
    for (const mode of names) aaa("tab", "create", "--machine", "labA", "--label", `scroll-${mode}`, "--", process.execPath, `${root}/scroll.js`, mode.replace("touch-", ""));
    for (const mode of names) await waitOutput(`scroll-${mode}`, "s-200");
    const got = (name) => aaa("pane", "read", name, "--source", "visible").text;
    /** 보이는 첫 줄의 번호(s-NNN), 없으면 null */
    const firstNo = (p) =>
      p.evaluate(() => {
        const m = /s-(\d{3})/.exec(document.querySelector(".xterm-rows")?.firstElementChild?.textContent ?? "");
        return m ? Number(m[1]) : null;
      });
    const bad = [];
    const dp = (await pairPage(browser, aaa("web", "pair").code, { width: 1100, height: 760 })).page;
    // 열기 전에 쓴 줄: host가 붙을 때 기록을 채워야 위로 굴러간다(전에는 보이는 줄만 받아 굴릴 것이 없었다)
    await dp.getByTestId("pane-scroll-plain").click();
    await until("plain 화면", async () => (await screenText(dp)).includes("s-200")).catch(async (e) => {
      // CI(macos-15)에서만 실패한 적이 있다(10-17 69224fd): 그때 화면과 grid·기록 상태를 남긴다
      const seen = {
        title: await dp.locator(".term-title").textContent(),
        history: await dp.locator(".xterm-viewport").evaluate((vp) => vp.scrollHeight - vp.clientHeight),
        ...(await termDiag(dp)),
      };
      throw new Error(`${e.message}: ${JSON.stringify(seen)} pane ${JSON.stringify(aaa("pane", "read", "scroll-plain", "--source", "visible").text.split("\n").filter(Boolean).slice(-3))}`);
    });
    const box = await dp.getByTestId("terminal").boundingBox();
    await dp.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await until("휠로 맨 위 s-001", async () => {
      await dp.mouse.wheel(0, -2000);
      return (await firstNo(dp)) === 1;
    }).catch(async (e) => {
      throw new Error(`${e.message}: 첫 줄 ${await firstNo(dp)}, xterm 기록 ${await dp.locator(".xterm-viewport").evaluate((v) => v.scrollHeight - v.clientHeight)}px`);
    });
    // 올려 둔 동안 출력이 와도 그 자리(키 입력이 아니다: 다른 곳에서 보낸 입력)
    for (let i = 0; i < 3; i++) aaa("pane", "send-text", "scroll-plain", "p");
    await until("more-3 출력", () => got("scroll-plain").includes("more-3"));
    await sleep(300);
    if ((await firstNo(dp)) !== 1) bad.push(`출력이 오자 올려 둔 자리를 잃었다(첫 줄 ${await firstNo(dp)})`);
    // 키를 치면 맨 아래로 돌아온다
    await dp.keyboard.press("x");
    // 맨 아래에는 올려 둔 동안 온 출력도 있다(그 출력이 이 화면에 왔는데도 자리를 지켰다는 뜻)
    await until("키를 치면 맨 아래", async () => {
      const text = await screenText(dp);
      return text.includes('GOT:"x"') && text.includes("more-3");
    });
    if ((await firstNo(dp)) === 1) bad.push("키를 쳤는데 맨 아래로 오지 않았다");
    // alternate screen에 mouse 보고가 없으면 휠은 화살표(herdr AlternateScroll), mouse 보고면 그 보고가 프로그램에 간다
    // 휠 한 번이 여러 줄이면 화살표도 여럿이 한 조각으로 온다
    for (const [mode, want] of [["alt", 'GOT:"\\u001b[A'], ["mouse", 'GOT:"\\u001b[<64;']]) {
      await dp.getByTestId(`pane-scroll-${mode}`).click();
      await until(`${mode} 화면`, async () => (await screenText(dp)).includes("s-200"));
      await dp.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
      await dp.mouse.wheel(0, -100);
      await until(`${mode} 휠 → ${want}`, () => got(`scroll-${mode}`).includes(want)).catch((e) => bad.push(`${e.message}: ${got(`scroll-${mode}`).split("\n").filter((l) => l.startsWith("GOT")).join(" ")}`));
    }
    // 휠 속도(herdr-web-ui terminalWheelSpeed): 3×면 휠 한 번에 보고 셋(xterm은 휠 사건마다 보고 하나)
    const reports = () => got("scroll-mouse").split("<64;").length - 1;
    await sleep(300);
    const before = reports();
    await dp.mouse.wheel(0, -100);
    await until("1× 보고 하나", () => reports() === before + 1);
    const dlg = await openSettings(dp, "터미널");
    for (let i = 0; i < 2; i++) await dlg.getByRole("button", { name: "휠 스크롤 빠르게" }).click();
    if ((await dlg.getByRole("group", { name: "휠 스크롤 속도" }).innerText()).trim() !== "3×") bad.push("설정이 3×가 아니다");
    if ((await dp.evaluate(() => localStorage.getItem("aaa:terminal-wheel-speed"))) !== "3") bad.push("휠 속도 저장이 없다");
    await dp.keyboard.press("Escape");
    await dp.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await sleep(300);
    const mid = reports();
    await dp.mouse.wheel(0, -100);
    await until("3× 보고 셋", () => reports() === mid + 3).catch((e) => bad.push(`${e.message}: ${reports() - mid}개`));
    await sleep(300);
    if (reports() !== mid + 3) bad.push(`3×에서 보고 ${reports() - mid}개`);
    await dp.context().close();

    // 한 손가락 끌기: 손가락이 한 칸 높이 내려갈 때마다 기록 한 줄(xterm 자기 터치 스크롤과 두 번 굴리지 않는다)
    const swipeCheck = async (label, p, swipe) => {
      await p.getByTestId("pane-scroll-plain").click();
      await until(`${label} 화면`, async () => (await screenText(p)).includes('GOT:"x"'));
      const cell = await p.evaluate(() => {
        const screen = document.querySelector(".xterm-screen").getBoundingClientRect();
        return { h: screen.height / document.querySelectorAll(".xterm-rows > div").length, top: screen.top, x: screen.left + screen.width / 2 };
      });
      const before = await firstNo(p);
      const travel = 240;
      await swipe(cell, travel);
      await sleep(200);
      const after = await firstNo(p);
      const want = Math.round(travel / cell.h);
      if (before === null || after === null || Math.abs(before - after - want) > 1)
        bad.push(`${label}: ${travel}px 끌어 첫 줄 ${before} → ${after}(기대 ${want}줄 위, 한 칸 ${cell.h.toFixed(1)}px)`);
      // alternate screen·mouse 보고 프로그램(Claude Code·Codex 같은)에서는 xterm 터치 스크롤이 아무것도 하지 않는다:
      // 끌기가 휠이 되어 화살표·mouse 보고로 프로그램에 가야 한다
      for (const [mode, want] of [["touch-alt", 'GOT:"\\u001b[A'], ["touch-mouse", 'GOT:"\\u001b[<64;']]) {
        if (label.startsWith("iPad") && mode === "touch-mouse") continue; // 한 기기에서 한 번이면 된다(iPad는 alt만)
        // 휴대폰은 pane을 열면 목록이 가려진다
        const back = p.getByRole("button", { name: "목록으로" });
        if (await back.isVisible()) await back.click();
        await p.getByTestId(`pane-scroll-${mode}`).click();
        // 그 프로그램의 화면이 그려졌는지는 글만으로 모른다: 앞 pane(plain)도 s-200을 보인다(10-18 전체 실행에서 끌기가 바뀌는
        // 사이 화면에 가 사라졌다). alternate screen이면 기록이 없고(viewport가 늘지 않는다), mouse면 xterm이 mouse 보고 중이다
        await until(`${label} ${mode} 화면`, async () =>
          (await screenText(p)).includes("s-200") &&
          (await p.evaluate((mode) => {
            const vp = document.querySelector(".xterm-viewport");
            const alt = vp && vp.scrollHeight <= vp.clientHeight + 1;
            return alt && (mode !== "touch-mouse" || !!document.querySelector(".xterm.enable-mouse-events"));
          }, mode)),
        );
        const c = await p.evaluate(() => {
          const r = document.querySelector(".xterm-screen").getBoundingClientRect();
          return { top: r.top, x: r.left + r.width / 2 };
        });
        await swipe({ ...cell, ...c }, travel);
        await until(`${label} ${mode} 끌기 → ${want}`, () => got(`scroll-${mode}`).includes(want)).catch(async (e) => {
          // 실패하면 그때 xterm이 mouse 보고 중이었는지(xterm Terminal.ts `enable-mouse-events`)와 viewport를 남긴다(t-0327 넘김 (마))
          const at = await p.evaluate(() => {
            const vp = document.querySelector(".xterm-viewport");
            return {
              mouse: !!document.querySelector(".xterm.enable-mouse-events"),
              viewport: vp ? [vp.scrollTop, vp.scrollHeight, vp.clientHeight] : null,
              log: (window.__aaaTermLog ?? []).slice(-12).map((x) => (x.kind ? `${x.at} bin×${x.bins}` : `${x.at} ${x.e}`)),
            };
          });
          bad.push(`${e.message}: ${got(`scroll-${mode}`).split("\n").filter((l) => l.startsWith("GOT")).join(" ")} ‖ ${JSON.stringify(at)}`);
        });
      }
      return { before, after, want };
    };
    const phone = (await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 })).page;
    const cdp = await phone.context().newCDPSession(phone);
    const ph = await swipeCheck("휴대폰(Chromium 터치)", phone, async (cell, travel) => {
      const y0 = cell.top + 40;
      await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x: cell.x, y: y0, id: 1 }] });
      for (let i = 1; i <= 12; i++) await cdp.send("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: [{ x: cell.x, y: y0 + (travel * i) / 12, id: 1 }] });
      await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
    });
    await phone.context().close();
    const wk = await webkit.launch();
    try {
      const pad = (await pairPage(wk, aaa("web", "pair").code, { width: 820, height: 1180 }, { touch: true })).page;
      await swipeCheck("iPad(WebKit 터치)", pad, (cell, travel) =>
        pad.evaluate(
          async ({ x, y0, travel }) => {
            const target = document.querySelector(".xterm-screen");
            // Playwright WebKit에는 Touch 생성자가 없다(Illegal constructor): 같은 이름의 사건에 손가락 목록을 붙인다
            const ev = (type, y) => {
              const t = { identifier: 7, target, clientX: x, clientY: y, pageX: x, pageY: y };
              const on = type === "touchend" ? [] : [t];
              const e = new Event(type, { bubbles: true, cancelable: true, composed: true });
              for (const [key, value] of [["touches", on], ["targetTouches", on], ["changedTouches", [t]]]) Object.defineProperty(e, key, { value });
              return e;
            };
            target.dispatchEvent(ev("touchstart", y0));
            for (let i = 1; i <= 12; i++) {
              target.dispatchEvent(ev("touchmove", y0 + (travel * i) / 12));
              await new Promise((r) => requestAnimationFrame(r));
            }
            target.dispatchEvent(ev("touchend", y0 + travel));
          },
          { x: cell.x, y0: cell.top + 40, travel },
        ),
      );
      await pad.context().close();
    } finally {
      await wk.close();
    }
    for (const mode of names) aaa("pane", "kill", `scroll-${mode}`);
    if (bad.length) throw new Error(bad.join("; "));
    console.log(`     휴대폰 240px 끌기: 첫 줄 s-${ph.before} → s-${ph.after}(${ph.want}줄)`);
  });

  await step("글꼴이 늦게 와도 맨 아래(넣은 WOFF2, plan §8.60): 터미널을 연 뒤 글꼴이 받아져 칸 높이가 바뀌어도 맨 아래를 보던 화면은 맨 아래에 남는다", async () => {
    // CI 러너와 처음 여는 기기에는 Meslotendard가 깔려 있지 않다: 대신 글꼴로 열고, 넣은 글꼴은 늦게 받는다.
    // 대신 글꼴은 칸 높이가 확실히 다른 것을 쓴다(같으면 이 시험은 아무것도 보지 않는다)
    aaa("tab", "create", "--machine", "labA", "--label", "fontjump", "--", process.execPath, `${root}/scroll.js`, "plain");
    await waitOutput("fontjump", "s-200");
    const init = () =>
      document.addEventListener("DOMContentLoaded", () => {
        const st = document.createElement("style");
        // 이 머신에 깔린 Meslotendard(local)를 쓰지 않고 WOFF2만 받게 같은 이름의 @font-face를 뒤에 둔다
        st.textContent = [400, 700]
          .map((w) => `@font-face{font-family:"Meslotendard Web";src:url("/fonts/meslotendard-0.3.1/Meslotendard-${w === 400 ? "Regular" : "Bold"}.woff2") format("woff2");font-weight:${w}}`)
          .concat(':root{--font-term:"Meslotendard Web", "Courier New", monospace !important}')
          .join("\n");
        document.head.append(st);
      });
    let release;
    const fontGate = new Promise((r) => (release = r));
    const hold = async (r) => {
      await fontGate;
      await r.continue();
    };
    const p = (await pairPage(browser, aaa("web", "pair").code, { width: 1100, height: 760 }, { init, route: ["**/fonts/**", hold] })).page;
    const rowHeight = () => p.evaluate(() => document.querySelector(".xterm-rows > div")?.getBoundingClientRect().height ?? 0);
    const atBottom = () =>
      p.evaluate(() => {
        const vp = document.querySelector(".xterm-viewport");
        return vp.scrollTop >= vp.scrollHeight - vp.clientHeight - 1;
      });
    await p.getByTestId("pane-fontjump").click();
    await until("대신 글꼴로 맨 아래 s-200", async () => (await screenText(p)).includes("s-200") && (await atBottom()));
    const before = await rowHeight();
    release();
    await until("넣은 글꼴로 칸 높이가 바뀜", async () => Math.abs((await rowHeight()) - before) >= 1);
    const after = await rowHeight();
    await until("글꼴이 바뀐 뒤에도 맨 아래 s-200", async () => (await screenText(p)).includes("s-200") && (await atBottom())).catch(async (e) => {
      throw new Error(`${e.message}: 칸 ${before.toFixed(1)}px → ${after.toFixed(1)}px, ${JSON.stringify(await termDiag(p))}`);
    });
    console.log(`     칸 높이 ${before.toFixed(1)}px → ${after.toFixed(1)}px`);
    await p.context().close();
    aaa("pane", "kill", "fontjump");
  });

  await step("줄 수가 줄면 위 줄을 기록으로(xterm·herdr처럼, plan §8.59): 작은 화면이 크기를 걸어도 host 화면에 마지막 줄·커서 줄이 남고, 다시 붙은 웹은 그 위를 굴려 본다", async () => {
    aaa("tab", "create", "--machine", "labA", "--label", "shrink", "--", process.execPath, `${root}/scroll.js`, "plain");
    await waitOutput("shrink", "s-200");
    // pty-size는 새 크기가 host에 닿은 뒤 돌아온다: 바로 읽어도 줄인 화면이다
    aaa("pane", "pty-size", "shrink", "80", "8");
    const r = aaa("pane", "read", "shrink", "--source", "visible").text.split("\n");
    // 커서는 s-200 다음 빈 줄: 그 위 일곱 줄이 s-194~s-200
    const want = Array.from({ length: 7 }, (_, i) => `s-${194 + i}`);
    if (JSON.stringify(r.slice(0, 7)) !== JSON.stringify(want)) throw new Error(`줄인 화면 ${JSON.stringify(r)}`);
    // 늦게 붙은 웹: 기록에 줄인 때 올린 줄까지 있어 위로 굴리면 s-001
    const dp = (await pairPage(browser, aaa("web", "pair").code, { width: 1100, height: 760 })).page;
    await dp.getByTestId("pane-shrink").click();
    await until("shrink 화면", async () => (await screenText(dp)).includes("s-200"));
    const box = await dp.getByTestId("terminal").boundingBox();
    await dp.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await until("기록 맨 위 s-001", async () => {
      await dp.mouse.wheel(0, -2000);
      return (await screenText(dp)).includes("s-001");
    });
    await dp.context().close();
    aaa("pane", "kill", "shrink");
  });

  await step("앞 기록(사용자 요청 10-17 「뒤로 스크롤하다 데이터가 부족해지면 받아오면 어때?」, plan §8.67): 끊긴 동안 쏟아진 출력을 다시 붙은 뒤 위로 굴리며 차례로 받아 보고, 붙일 때 보던 자리가 튀지 않는다", async () => {
    aaa("tab", "create", "--machine", "labA", "--label", "histapp", "--", process.execPath, `${root}/history.js`);
    await waitOutput("histapp", "hist>");
    // pane WebSocket 문: 닫혀 있으면 새 연결을 바로 끊는다(끊긴 동안). host의 앞 기록 답은 잠깐 붙잡아, 붙이기 전 화면을 잰다
    // 첫 답을 붙잡는 동안 온 출력도 붙잡았다가 답 바로 뒤에 보낸다: 다시 쓰는 사이에 출력이 오는 경우
    let gate = "open";
    let pipe = null;
    let replies = 0;
    let holdOutput = null;
    let heldOutput = 0;
    const route = (ws) => {
      if (gate === "closed") return void ws.close({ code: 1011, reason: "test offline" });
      const server = ws.connectToServer();
      pipe = { ws, server };
      ws.onMessage((m) => server.send(m));
      server.onMessage((m) => {
        if (typeof m === "string" && m.startsWith('{"t":"history"')) {
          replies += 1;
          if (replies === 1) holdOutput = [];
          return void setTimeout(() => {
            ws.send(m);
            for (const o of holdOutput ?? []) ws.send(o);
            heldOutput += holdOutput?.length ?? 0;
            holdOutput = null;
          }, replies === 1 ? 1500 : 600);
        }
        // 지금 연결의 출력만(닫은 앞 연결의 늦은 출력이 섞이지 않게)
        if (holdOutput && typeof m !== "string" && pipe?.server === server) return void holdOutput.push(m);
        ws.send(m);
      });
      server.onClose((code, reason) => ws.close({ code, reason }));
      ws.onClose((code, reason) => server.close({ code, reason }));
    };
    const hp = (await pairPage(browser, aaa("web", "pair").code, { width: 1100, height: 760 }, { wsRoute: [/\/api\/v1\/panes\//, route] })).page;
    await hp.getByTestId("pane-histapp").click();
    await until("histapp 화면", async () => (await screenText(hp)).includes("hist>"));
    const box = hp.getByTestId("terminal");
    const attr = (k) => box.evaluate((el, k) => el.dataset[k] ?? null, k);
    // 끊고, 끊긴 동안 6000줄
    gate = "closed";
    pipe.ws.close({ code: 1011, reason: "test offline" });
    pipe.server.close({ code: 1000, reason: "test offline" });
    aaa("pane", "send-text", "histapp", "h");
    await waitOutput("histapp", "history-done");
    gate = "open";
    await until("다시 붙음", async () => (await screenText(hp)).includes("history-done"), 30000);
    if ((await attr("historyMore")) !== "1") throw new Error(`다시 붙은 뒤 앞 기록 표시 ${await attr("historyMore")}`);
    const rows = () => hp.locator(".xterm-rows").innerText();
    const firstNum = (t) => Number(/h-(\d{5})/.exec(t)?.[1] ?? NaN);
    // 이 화면을 쓴다(누르면 이 화면 크기로 grid를 되찾는다: 남의 grid를 보는 동안은 휠이 칸을 굴린다)
    await box.click();
    await until("이 화면 grid", async () => (await hp.locator(".term-scroll[data-adopted-grid]").count()) === 0);
    await box.hover();
    let applied = 0;
    const seen = [];
    for (let round = 0; round < 10; round++) {
      // 받은 기록 맨 위 근처까지 굴린다: 요청이 나가면 멈추고 그 화면을 잰다
      await until(`위로 굴려 요청(${round + 1})`, async () => {
        if ((await attr("historyWaiting")) === "1" || (await attr("historyMore")) === "0") return true;
        await hp.mouse.wheel(0, -3000);
        return false;
      });
      if ((await attr("historyMore")) === "0" && (await attr("historyWaiting")) !== "1") break;
      const before = await rows();
      // 첫 덩어리를 기다리는 동안 pane에 새 줄(tick-1): 다시 쓰는 사이에 도착한다
      if (round === 0) {
        aaa("pane", "send-text", "histapp", "p");
        await waitOutput("histapp", "tick-1");
      }
      await until(`붙임(${round + 1})`, async () => Number(await attr("historyApplied")) === applied + 1);
      applied += 1;
      const after = await rows();
      if (after !== before) throw new Error(`붙일 때 화면이 튀었다(${round + 1}): ${before.slice(0, 80)} → ${after.slice(0, 80)}`);
      seen.push(firstNum(before));
    }
    if (applied < 2 || replies !== applied) throw new Error(`앞 기록을 ${applied}번 붙였다(답 ${replies})`);
    if (heldOutput === 0) throw new Error("다시 쓰는 사이에 온 출력이 없다(시험이 그 경우를 만들지 못했다)");
    if (!seen.every((n, i) => i === 0 || n < seen[i - 1])) throw new Error(`붙인 자리가 위로 가지 않았다: ${seen}`);
    // 버퍼 전체를 한 화면씩 훑어: h-00001~h-06000이 빠짐·겹침 없이 차례로(덩어리 경계도)
    const sweep = await hp.evaluate(async () => {
      const box = document.querySelector('[data-testid="terminal"]');
      const vp = box.querySelector(".xterm-viewport");
      const rows = () => [...box.querySelectorAll(".xterm-rows > div")];
      // 그 줄이 맨 위로 그려질 때까지(Term onRender가 data-view-top에 적는다). 걸음마다 맨 위 줄이 바뀌므로 그리기가 꼭 온다.
      // 오지 않으면 끝없이 기다리지 않고 그때 모습으로 실패한다(기다림의 끝은 사건이고, 10초는 고장 판정일 뿐이다)
      const drawnAt = (line) =>
        new Promise((done, fail) => {
          const ok = () => box.dataset.viewTop === String(line);
          if (ok()) return done();
          const stuck = setTimeout(() => {
            mo.disconnect();
            fail(new Error(`줄 ${line}이 그려지지 않았다: ${JSON.stringify({ top: box.dataset.viewTop, base: box.dataset.viewBase, scrollTop: vp.scrollTop, scrollHeight: vp.scrollHeight })}`));
          }, 10_000);
          const mo = new MutationObserver(() => ok() && (clearTimeout(stuck), mo.disconnect(), done()));
          mo.observe(box, { attributes: true, attributeFilter: ["data-view-top"] });
        });
      // xterm(5.5 Viewport)은 scrollTop을 줄 높이(css cell 높이 = 줄 div 높이)로 나눠 반올림한 줄을 맨 위로 그린다. scroll 영역은
      // 줄 높이×줄 수에 (viewport 높이 − canvas 높이)를 더한 높이라, scrollHeight를 줄 수로 나누면 줄이 커져 먼 줄에서 한 줄 넘는다
      const goTo = async (line) => {
        if (box.dataset.viewTop === String(line)) return;
        const rowPx = rows()[0].getBoundingClientRect().height;
        vp.scrollTop = line * rowPx;
        vp.dispatchEvent(new Event("scroll"));
        await drawnAt(line);
      };
      const nums = new Set();
      const bad = [];
      const ticks = new Map();
      let last = -1;
      const half = Math.floor(rows().length / 2);
      for (let line = 0; ; ) {
        await goTo(line);
        const divs = rows();
        const got = divs.map((d) => /^h-(\d{5})/.exec(d.textContent)?.[1]).filter(Boolean).map(Number);
        for (let i = 1; i < got.length; i++) if (got[i] !== got[i - 1] + 1) bad.push([got[i - 1], got[i]]);
        got.forEach((n) => nums.add(n));
        // tick-1·h-06000 줄의 버퍼 줄 번호(겹쳐 본 화면에서 같은 줄은 한 번 센다)
        divs.forEach((d, i) => {
          if (d.textContent.startsWith("tick-1")) ticks.set(line + i, "tick");
          if (d.textContent.startsWith("h-06000")) last = line + i;
        });
        const base = Number(box.dataset.viewBase);
        if (line >= base) break;
        line = Math.min(base, line + half);
      }
      const tickAt = [...ticks.keys()];
      return { count: nums.size, min: Math.min(...nums), max: Math.max(...nums), bad: bad.slice(0, 5), tickAt, last };
    });
    if (sweep.count !== 6000 || sweep.min !== 1 || sweep.max !== 6000 || sweep.bad.length) throw new Error(`버퍼 기록 ${JSON.stringify(sweep)}`);
    // 다시 쓰는 사이에 온 줄은 한 번, 기록 뒤에
    if (sweep.tickAt.length !== 1 || !(sweep.tickAt[0] > sweep.last)) throw new Error(`tick-1 자리 ${JSON.stringify(sweep)}`);
    // 다시 쓴 버퍼도 프로그램이 켠 입력 모드(bracketed paste)를 그대로 갖는다: 붙여 넣은 여러 줄이 감싸여 간다
    await box.click();
    await hp.locator(".xterm-helper-textarea").evaluate((el) => {
      const dt = new DataTransfer();
      dt.setData("text/plain", "pa\nste");
      el.dispatchEvent(new ClipboardEvent("paste", { clipboardData: dt, bubbles: true, cancelable: true }));
    });
    await waitOutput("histapp", 'GOT:"\\u001b[200~pa\\rste\\u001b[201~"');
    await hp.context().close();
    aaa("pane", "kill", "histapp");
  });

  await step("출력이 밀릴 때(herdr-web-ui OutputWindow·pty-ack·4008 「Reconnect」, D7, plan §8.62): ACK를 못 하는 화면은 host가 4008로 닫고 다시 보내지 않으며 「다시 연결」로 새로 붙는다. 따라오는 화면은 그대로 받는다", async () => {
    aaa("tab", "create", "--machine", "labA", "--label", "flood", "--", process.execPath, `${root}/flood.js`);
    await waitOutput("flood", "flood>");
    /** 이 화면의 pane WebSocket 기록(진단 기록과 같은 것): snapshot·닫힘·열림 */
    const log = (p) => p.evaluate(() => window.__aaaTermLog.map((x) => x.e ?? x.kind));
    const count = (l, prefix) => l.filter((e) => e.startsWith(prefix)).length;
    // ACK를 삼키는 화면: 브라우저가 쓴 뒤 알리는 것이 host에 닿지 않는다(느린 기기·멈춘 탭과 같다)
    let dropAcks = true;
    let dropped = 0;
    const swallow = (ws) => {
      const server = ws.connectToServer();
      ws.onMessage((m) => {
        if (dropAcks && typeof m === "string" && m.startsWith('{"t":"ack"')) return void dropped++;
        server.send(m);
      });
      server.onMessage((m) => ws.send(m));
      server.onClose((code, reason) => ws.close({ code, reason }));
      ws.onClose((code, reason) => server.close({ code, reason }));
    };
    const slow = (await pairPage(browser, aaa("web", "pair").code, { width: 1100, height: 760 }, { wsRoute: [/\/api\/v1\/panes\//, swallow] })).page;
    await slow.getByTestId("pane-flood").click();
    await until("느린 화면 flood>", async () => (await screenText(slow)).includes("flood>"));
    const fast = (await pairPage(browser, aaa("web", "pair").code, { width: 1100, height: 760 })).page;
    await fast.getByTestId("pane-flood").click();
    await until("따라오는 화면 flood>", async () => (await screenText(fast)).includes("flood>"));
    // ACK 전의 옛 웹(주소에 flow=ack가 없고 ACK도 없다, 다시 띄우기 전에 받은 화면): host는 보내기 줄로만 재므로
    // 잘 받는 동안은 닫지 않는다(참고의 ACK 없는 연결)
    const old = await fast.context().newPage();
    await old.goto(base);
    await old.evaluate((id) => {
      const w = new WebSocket(`ws://${location.host}/api/v1/panes/${encodeURIComponent(id)}/ws`);
      w.binaryType = "arraybuffer";
      window.oldClient = { bytes: 0, code: null, acks: 0, joined: false };
      w.onmessage = (m) => {
        if (typeof m.data !== "string") window.oldClient.bytes += m.data.byteLength;
        else if (m.data.startsWith('{"t":"snapshot"')) window.oldClient.joined = true;
      };
      w.onclose = (e) => (window.oldClient.code = e.code);
    }, aaa("pane", "list").panes.find((x) => x.name === "flood").id);
    // snapshot이 출력 구독의 시작이다. 그 전에 쏟으면 앞부분이 snapshot(text)에 들어가 bytes가 모자란다(t-0327, 착지 검증에서 1/8)
    await until("옛 웹 붙음", async () => await old.evaluate(() => window.oldClient.joined));
    // 400KB를 한 번에 쓴다(HIGH 256KiB를 넘는다)
    aaa("pane", "send-text", "flood", "f");
    await until("출력 정지 띠", async () => await slow.getByTestId("output-stalled").isVisible());
    const stalled = await log(slow);
    if (!stalled.includes("ws close 4008")) throw new Error(`4008로 닫히지 않았다: ${JSON.stringify(stalled.filter((e) => e !== "io"))}`);
    if (dropped === 0) throw new Error("삼킨 ACK가 없다: 화면이 ACK를 보내지 않는다");
    // 과부하는 다시 보내지 않는다: 닫히기 전 snapshot을 다시 받지 않았고, 닫힌 뒤 스스로 다시 붙지 않는다
    if (count(stalled, "snapshot") !== 1) throw new Error(`snapshot을 다시 받았다: ${JSON.stringify(stalled.filter((e) => e !== "io"))}`);
    await sleep(1500);
    const later = await log(slow);
    if (count(later, "ws open") !== 1 || !(await slow.getByTestId("output-stalled").isVisible())) throw new Error(`4008 뒤 스스로 다시 붙었다: ${JSON.stringify(later.filter((e) => e !== "io"))}`);
    // 따라오는 화면은 닫히지 않고 끝까지 받는다
    await until("따라오는 화면 flood-done", async () => (await screenText(fast)).includes("flood-done"));
    const kept = await log(fast);
    if (kept.some((e) => e.startsWith("ws close")) || count(kept, "snapshot") !== 1) throw new Error(`따라오는 화면이 끊겼다: ${JSON.stringify(kept.filter((e) => e !== "io"))}`);
    await until("옛 웹도 끝까지 받음", async () => (await old.evaluate(() => window.oldClient.bytes)) >= 400_000);
    const oldClient = await old.evaluate(() => window.oldClient);
    if (oldClient.code !== null) throw new Error(`ACK 없는 옛 웹이 닫혔다: ${JSON.stringify(oldClient)}`);
    await old.close();
    await shot(slow, "desktop-output-stalled");
    // 「다시 연결」: 새로 붙어 snapshot을 받고 입력도 간다
    dropAcks = false;
    await slow.getByTestId("output-reconnect").click();
    // 옛 화면에도 flood-done이 있다: 새 snapshot을 받은 뒤의 화면을 본다
    await until("다시 붙음", async () => count(await log(slow), "snapshot") === 2 && (await slow.getByTestId("output-stalled").count()) === 0);
    await until("다시 붙은 화면", async () => (await screenText(slow)).includes("flood-done"));
    const back = await log(slow);
    if (count(back, "ws open") !== 2) throw new Error(`다시 연결이 새로 붙지 않았다: ${JSON.stringify(back.filter((e) => e !== "io"))}`);
    await slow.locator(".xterm-helper-textarea").focus();
    await slow.keyboard.type("z");
    await waitOutput("flood", 'GOT:"z"');
    await slow.context().close();
    await fast.context().close();
    aaa("pane", "kill", "flood");
  });

  await step("키 처리(herdr-web-ui PaneTerminal, macOS): Shift+Enter ESC CR, Ctrl+Enter는 modifyOtherKeys대로, Cmd+⌫ ^U, Cmd+←/→ ^A/^E, Ctrl+V는 브라우저 몫, 선택 중 Ctrl+C는 복사, OSC 52, 늦게 붙은 화면도 모드를 안다", async () => {
    aaa("tab", "create", "--machine", "labA", "--label", "keysapp", "--", process.execPath, `${root}/keys.js`);
    const got = () => aaa("pane", "read", "keysapp", "--source", "visible").text;
    const gots = () => got().split("GOT:").length - 1;
    // 클립보드는 기록만 한다(headless 권한·focus와 상관없이 무엇을 썼는지 본다)
    const clip = () => {
      window.__clip = [];
      window.__copies = 0;
      Object.defineProperty(navigator, "clipboard", {
        configurable: true,
        value: { writeText: (t) => (window.__clip.push(t), Promise.resolve()), readText: () => Promise.resolve("") },
      });
      document.addEventListener("copy", () => window.__copies++, true);
    };
    const mac = await pairPage(browser, aaa("web", "pair").code, { width: 1100, height: 700 }, { platform: "MacIntel", init: clip });
    const mp = mac.page;
    await mp.getByTestId("pane-keysapp").click();
    await until("keys 화면", async () => (await screenText(mp)).includes("keys>"));
    await mp.getByTestId("terminal").click();
    const press = async (key, want) => {
      const n = gots();
      await mp.keyboard.press(key);
      await until(`${key} → ${want}`, () => gots() > n);
      const last = got().split("GOT:").at(-1).split("\n")[0];
      if (last !== want) throw new Error(`${key}가 ${last}로 갔다(기대 ${want})`);
      // 프로그램이 같은 조각에 쓴 모드 변경을 웹 터미널도 읽은 뒤에 다음 키를 누른다
      await until(`화면 ${want}`, async () => (await screenText(mp)).split("GOT:").at(-1).split("\n")[0] === want);
    };
    await press("Shift+Enter", '"\\u001b\\r"');
    // 화면을 열 때 이미 켜진 모드는 snapshot replay가 다시 말한다
    await press("Control+Enter", '"\\u001b[27;5;13~"');
    await press("n", '"n"');
    await press("Control+Enter", '"\\r"');
    await press("m", '"m"');
    await press("Control+Enter", '"\\u001b[27;5;13~"');
    await press("Meta+Backspace", '"\\u0015"');
    await press("Meta+ArrowLeft", '"\\u0001"');
    await press("Meta+ArrowRight", '"\\u0005"');
    await press("Enter", '"\\r"');
    // Ctrl+V는 xterm이 ^V로 보내지 않는다(브라우저 붙여넣기에 맡긴다): 뒤이은 x만 간다
    await mp.keyboard.press("Control+v");
    await press("x", '"x"');
    // 선택이 있으면 Ctrl+C는 복사하고 ^C를 보내지 않는다
    const rows = await mp.locator(".xterm-rows > div").first().boundingBox();
    await mp.mouse.move(rows.x + 2, rows.y + rows.height / 2);
    await mp.mouse.down();
    await mp.mouse.move(rows.x + 60, rows.y + rows.height / 2, { steps: 4 });
    await mp.mouse.up();
    await mp.keyboard.press("Control+c");
    await press("y", '"y"');
    if ((await mp.evaluate(() => window.__copies)) < 1) throw new Error("선택 중 Ctrl+C가 복사하지 않았다");
    // OSC 52: 프로그램이 쓴 글이 클립보드로
    await press("o", '"o"');
    await until("OSC 52 클립보드", async () => (await mp.evaluate(() => window.__clip)).includes("클립 52"));
    // 늦게 붙은 화면: 프로그램은 모드를 다시 말하지 않지만 replay가 말한다
    const late = await pairPage(browser, aaa("web", "pair").code, { width: 1100, height: 700 }, { platform: "MacIntel" });
    await late.page.getByTestId("pane-keysapp").click();
    await until("keys 화면(늦게 붙음)", async () => (await screenText(late.page)).includes("keys>"));
    await late.page.getByTestId("terminal").click();
    const n = gots();
    await late.page.keyboard.press("Control+Enter");
    await until("늦게 붙은 화면 Ctrl+Enter", () => gots() > n);
    const last = got().split("GOT:").at(-1).split("\n")[0];
    if (last !== '"\\u001b[27;5;13~"') throw new Error(`늦게 붙은 화면의 Ctrl+Enter가 ${last}`);
    if (mac.errors.length || late.errors.length) throw new Error([...mac.errors, ...late.errors].join("; "));
    await late.page.context().close();
    await mp.context().close();
    await page.getByTestId("pane-t1").click();
  });

  await step("그냥 끌어 고르기·제자리 클릭·크기 되찾기(herdr-web-ui PaneTerminal): 마우스 보고 중에도 왼쪽 끌기는 고르고 놓으면 복사, 제자리 클릭은 프로그램에, 키는 보내기 전에 이 화면 크기를 건다", async () => {
    aaa("tab", "create", "--machine", "labA", "--label", "mouseapp", "--", process.execPath, `${root}/mouse.js`);
    // [일련번호, 종류, 값]
    const events = () =>
      aaa("pane", "read", "mouseapp", "--source", "visible")
        .text.split("\n").map((l) => /^(GOT|SIZE):(\d+):(.*)$/.exec(l))
        .filter(Boolean)
        .map((m) => [Number(m[2]), m[1], m[1] === "GOT" ? JSON.parse(m[3]) : Number(m[3])]);
    const after = (n) => events().filter(([i]) => i > n);
    const lastSeq = () => Math.max(0, ...events().map(([i]) => i));
    const clip = () => {
      window.__copied = [];
      // xterm이 copy 사건에 넣은 글(bubble 끝에서 읽는다)
      window.addEventListener("copy", (e) => window.__copied.push(e.clipboardData?.getData("text/plain") ?? ""));
    };
    const dp = (await pairPage(browser, aaa("web", "pair").code, { width: 1100, height: 700 }, { platform: "MacIntel", init: clip })).page;
    // 이 화면이 WebSocket에 쓴 글 frame(보낸 순서를 본다)
    const frames = [];
    dp.on("websocket", (ws) =>
      ws.on("framesent", (f) => {
        if (typeof f.payload !== "string") return;
        try {
          frames.push(JSON.parse(f.payload));
        } catch {}
      }),
    );
    await dp.getByTestId("pane-mouseapp").click();
    await until("mouse 화면", async () => (await screenText(dp)).includes("pick-this-word"));
    const cell = async (col, row) => {
      const r = await dp.locator('[data-testid="terminal"] .xterm-screen').boundingBox();
      const p = aaa("pane", "list").panes.find((x) => x.name === "mouseapp");
      return { x: r.x + (col + 0.5) * (r.width / p.cols), y: r.y + (row + 0.5) * (r.height / p.rows) };
    };
    // 제자리 클릭은 프로그램에 간다(SGR 누름·뗌, 1부터 센 열·줄)
    let n = lastSeq();
    const c2 = await cell(2, 0);
    await dp.mouse.click(c2.x, c2.y);
    await until("클릭 보고", () => after(n).filter(([, k]) => k === "GOT").map(([, , v]) => v).join("").includes("\u001b[<0;3;1m"));
    const clicked = after(n).filter(([, k]) => k === "GOT").map(([, , v]) => v).join("");
    if (!clicked.includes("\u001b[<0;3;1M")) throw new Error(`클릭 누름 보고가 없다: ${JSON.stringify(clicked)}`);
    // 끌기는 고른다: 프로그램에 마우스 보고가 가지 않고, 놓으면 복사하고 알린다
    await sleep(300);
    n = lastSeq();
    const from = await cell(0, 0);
    // xterm은 끝을 가까운 칸 경계로 잡는다: 마지막 글자 다음 칸 가운데까지 끈다
    const to = await cell(14, 0);
    await dp.mouse.move(from.x, from.y);
    await dp.mouse.down();
    await dp.mouse.move(to.x, to.y, { steps: 6 });
    await dp.mouse.up();
    await until("복사 알림", async () => (await dp.getByTestId("clipboard-note").innerText()).includes("클립보드에 복사했다"));
    const copied = await dp.evaluate(() => window.__copied);
    if (!copied.some((t) => t.includes("pick-this-word"))) throw new Error(`복사한 글 ${JSON.stringify(copied)}`);
    await sleep(300);
    const leaked = after(n).filter(([, k, v]) => k === "GOT" && v.includes("\u001b[<"));
    if (leaked.length) throw new Error(`끌기가 프로그램에 갔다: ${JSON.stringify(leaked)}`);
    // 다른 화면이 크기를 가져간 뒤 이 화면에서 키를 치면, 키보다 먼저 이 화면 크기를 건다(첫 글의 답이 이 grid로 그려진다)
    const sp = (await pairPage(browser, aaa("web", "pair").code, { width: 760, height: 560 })).page;
    await sp.getByTestId("pane-mouseapp").click();
    await until("mouse 화면(둘째)", async () => (await screenText(sp)).includes("pick-this-word"));
    const pcols = () => aaa("pane", "list").panes.find((x) => x.name === "mouseapp").cols;
    const dcols = pcols();
    // 둘째 화면이 크기를 가져간다(키 막대 없는 데스크톱: 터미널 누름은 클릭 보고도 보낸다)
    await sp.getByTestId("terminal").click();
    await until("둘째 화면 크기", () => pcols() < dcols);
    await sleep(500);
    // 보낸 순서는 이 화면이 WebSocket에 쓴 순서로 본다(프로그램 쪽은 SIGWINCH와 입력 읽기의 순서가 정해지지 않는다)
    const mark = frames.length;
    n = lastSeq();
    await dp.keyboard.press("z");
    await until("z 받음", () => after(n).some(([, k, v]) => k === "GOT" && v === "z"));
    await until("크기 되찾음", () => pcols() === dcols);
    const order = frames.slice(mark).filter((f) => f.t === "resize" || (f.t === "input" && f.data === "z")).map((f) => f.t);
    if (order[0] !== "resize" || !order.includes("input")) throw new Error(`보낸 순서 ${JSON.stringify(frames.slice(mark))}`);
    await sp.context().close();
    await dp.context().close();
    aaa("pane", "kill", "mouseapp");
  });

  await step("두 controller(herdr 모델): 가져오기 없이 바로 입력, 입력·focus를 얻은 화면이 크기를 다시 건다, 가려지면 바로 놓는다", async () => {
    const other = await pairPage(browser, aaa("web", "pair").code, { width: 1000, height: 640 }, { line: true });
    const op = other.page;
    // 이 화면이 WebSocket에 쓴 글 frame
    const opFrames = [];
    op.on("websocket", (ws) =>
      ws.on("framesent", (f) => {
        if (typeof f.payload !== "string") return;
        try {
          opFrames.push(JSON.parse(f.payload));
        } catch {}
      }),
    );
    const t1size = () => {
      const p = aaa("pane", "list").panes.find((x) => x.name === "t1");
      return `${p.cols}x${p.rows}`;
    };
    const holder = () => aaa("pane", "list").panes.find((x) => x.name === "t1").lease?.label ?? null;
    // 원래 화면이 t1을 쓰는 중에서 시작한다(앞 단계가 t1에 입력했는지에 기대지 않는다)
    await page.locator(".xterm-screen").click();
    await page.keyboard.type("echo own-$((1+2))\n");
    await until("원래 화면 입력", async () => (await screenText(page)).includes("own-3"));
    await until("원래 화면이 쓰는 중", () => holder() !== null);
    const held = t1size();
    const first = holder();
    await op.getByTestId("pane-t1").click();
    await until("둘째 화면 터미널", async () => (await screenText(op)).includes("own-3"));
    // 「다른 화면이 크기를 정하는 중」 띠와 「화면에 맞추기」 단추는 참고에 없어 없앴다
    if ((await op.getByTestId("lease-banner").count()) !== 0) throw new Error("크기를 정하는 중 띠가 있다");
    if ((await op.getByRole("button", { name: "화면에 맞추기" }).count()) !== 0) throw new Error("화면에 맞추기 단추가 있다");
    // 입력 막대가 켜져 있다(보기 권한이 아니면 언제나 입력된다). 열기만 해서는 크기를 바꾸지 않는다
    if (await op.getByLabel("보낼 글").isDisabled()) throw new Error("다른 화면이 쓰는 중이라고 입력 막대가 꺼졌다");
    await sleep(600);
    if (t1size() !== held || holder() !== first) throw new Error(`열기만 했는데 크기·쓰는 화면이 바뀌었다: ${held} → ${t1size()}`);
    // 1) 입력하면 바로 들어가고, 그 화면이 지금 쓰는 화면이 되어 자기 칸 크기로 맞춘다
    await op.getByLabel("보낼 글").fill("echo direct-$((3*7))");
    await op.getByRole("button", { name: "보내기" }).click();
    await until("다른 화면 입력", async () => (await screenText(page)).includes("direct-21"));
    await until("입력한 화면 크기", () => t1size() !== held && holder() !== first);
    const otherSize = t1size();
    await shot(page, "desktop-lease-held");
    // 2) 번갈아 입력하면 크기가 그때마다 바뀐다(지금 규칙): 원래 화면 터미널에 직접 친다
    await page.locator(".xterm-screen").click();
    await page.keyboard.type("echo back-$((2*5))\n");
    await until("원래 화면 입력", async () => (await screenText(page)).includes("back-10"));
    await until("원래 화면 크기로", () => t1size() === held && holder() === first);
    await op.getByLabel("보낼 글").fill("echo again-$((4*4))");
    await op.getByRole("button", { name: "보내기" }).click();
    await until("다시 다른 화면 크기", () => t1size() === otherSize);
    // 3) focus를 얻은 화면이 크기를 다시 건다(입력 없이)
    await page.evaluate(() => window.dispatchEvent(new Event("focus")));
    await until("focus 얻은 화면 크기", () => t1size() === held && holder() === first);
    // 4) 쓰던 화면이 가려지면 바로 놓고(참고 keepSize(hidden), 전에는 1초 뒤), 보이고 focus 있는 화면이 자기 크기로 맞춘다
    await op.getByLabel("보낼 글").fill("echo hide");
    await op.getByRole("button", { name: "보내기" }).click();
    await until("다른 화면 크기", () => t1size() === otherSize);
    const beforeHide = opFrames.length;
    await op.evaluate(() => {
      Object.defineProperty(document, "visibilityState", { configurable: true, get: () => "hidden" });
      document.dispatchEvent(new Event("visibilitychange"));
    });
    // 1초 타이머가 아니라 바로 놓는다(700ms 안에 release frame)
    await until("가려지자 바로 놓음", () => opFrames.slice(beforeHide).some((f) => f.t === "release"), 700);
    await until("가려진 화면이 놓음 → 남은 화면 크기", () => t1size() === held && holder() === first, 5000);
    if (other.errors.length) throw new Error(other.errors.join("; "));
    await other.page.context().close();
    if (await page.getByLabel("보낼 글").isDisabled()) throw new Error("입력 막대가 꺼져 있다");
  });


  await step("reader(휴대폰 크기): 보기만 된다", async () => {
    const rd = await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 390, height: 844 });
    const rp = rd.page;
    if ((await rp.getByRole("button", { name: "새 셸" }).count()) !== 0) throw new Error("reader에게 새 셸 버튼");
    await shot(rp, "mobile-list");
    await rp.getByTestId("pane-t1").click();
    await until("reader 화면", async () => (await screenText(rp)).includes("한글-8"));
    // 휴대폰 폭(13px)에서도 한글이 두 칸에 꼭 맞는다
    const cols = aaa("pane", "list").panes.find((x) => x.name === "t1").cols;
    console.log("     글꼴 측정(390×844)", JSON.stringify(await fontOk(rp, cols)));
    const banner = await rp.getByTestId("readonly-banner").innerText();
    if (!banner.includes("보기 권한")) throw new Error(`reader 배너: ${banner}`);
    if ((await rp.getByLabel("보낼 글").count()) !== 0) throw new Error("reader에게 입력 막대");
    await rp.getByTestId("terminal").click();
    await rp.keyboard.type("echo reader-$((1+1))\n");
    await sleep(800);
    const lines = aaa("pane", "read", "t1", "--source", "visible").text;
    if (lines.includes("reader-")) throw new Error("reader 입력이 PTY에 들어갔다");
    await shot(rp, "mobile-reader");
    if (rd.errors.length) throw new Error(rd.errors.join("; "));
    await rd.page.context().close();
    // 낮은 화면의 보기 권한: 데스크톱 grid가 칸보다 높으면 칸이 굴러가고 커서 줄을 보이게 따라간다(herdr-web-ui followCursor)
    // t1 화면은 뒤 단계가 본다: 따로 pane을 띄운다(80×40, 줄 80개 뒤 커서는 맨 아래)
    aaa("tab", "create", "--machine", "labA", "--label", "follow", "--cols", "80", "--rows", "40", "--", "/bin/sh", "-c", "seq 1 80; echo follow-$((6*7)); exec cat");
    const low = await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 390, height: 420 });
    await low.page.getByTestId("pane-follow").click();
    const followed = () =>
      low.page.evaluate(() => {
        const scroll = document.querySelector(".term-scroll");
        const box = scroll.getBoundingClientRect();
        // 보기만 하는 화면은 focus가 없어 커서 칸이 따로 없다: 커서 바로 위 줄(follow-42)을 본다
        const row = [...document.querySelectorAll(".xterm-rows > div")].find((r) => r.textContent.includes("follow-42"));
        const cursor = row?.getBoundingClientRect();
        return {
          text: row !== undefined,
          scrollTop: scroll.scrollTop,
          scrollHeight: scroll.scrollHeight,
          clientHeight: scroll.clientHeight,
          box: [Math.round(box.top), Math.round(box.bottom)],
          cursor: cursor ? [Math.round(cursor.top), Math.round(cursor.bottom)] : null,
        };
      });
    let last = null;
    await until("커서 줄을 따라감", async () => {
      last = await followed();
      return last.text && last.cursor && last.scrollTop > 0 && last.cursor[0] >= last.box[0] && last.cursor[1] <= last.box[1] + 1;
    }).catch((e) => {
      throw new Error(`${e.message}: ${JSON.stringify(last)}`);
    });
    await low.page.context().close();
    aaa("pane", "kill", "follow");
  });

  await step("controller: 새 셸", async () => {
    const before = aaa("pane", "list").panes.length;
    await page.getByTestId("machine-labA").getByRole("button", { name: "새 셸" }).click();
    await until("새 pane", () => aaa("pane", "list").panes.length === before + 1);
    await page.getByTestId("pane-t1").click();
  });

  await step("확인창 질문: 카드·상태 표시 → reader는 못 답함 → 예", async () => {
    // 세션 안에서 Claude처럼 선택 창을 그리고 hook 사건(PermissionRequest)을 보낸다
    const script =
      'printf "Allow Bash(make test)?\\n\\342\\235\\257 1. Yes\\n  2. No\\nEsc to cancel\\n"; ' +
      '"$0" hook --agent claude \'{"hook_event_name":"PermissionRequest","tool_name":"Bash","tool_input":{"command":"make test"}}\'; ' +
      "stty -echo; exec cat > /dev/null";
    aaa("tab", "create", "--machine", "labA", "--label", "q1", "--", "/bin/sh", "-c", script, AA);
    // 목록에는 그 줄의 빨간 ?만(차이 목록 Q1·S2, 참고 Sidebar.tsx StatusBadge): 카드는 그 pane을 열면 터미널 아래에
    const state = (pg) => pg.getByTestId("agent-q1");
    await state(page).and(page.locator('[data-status="blocked"]')).waitFor();
    if ((await state(page).locator(".sidebar-status-mark").innerText()) !== "?") throw new Error("질문 줄에 ? 표시가 없다");
    if ((await state(page).getAttribute("aria-label")) !== "agent 답을 기다림") throw new Error(`상태 말: ${await state(page).getAttribute("aria-label")}`);
    if ((await page.locator(".question").count()) !== 0) throw new Error("pane을 열기 전에 목록에 질문 카드가 있다");
    await page.getByTestId("pane-q1").click();
    const q = aaa("question", "list").questions.find((x) => x.text.includes("make test"));
    const card = page.getByTestId(`dock-question-${q.id}`);
    await card.waitFor();
    if (!(await card.innerText()).includes("make test")) throw new Error(`카드 본문: ${await card.innerText()}`);
    const rd = await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 390, height: 844 });
    await state(rd.page).and(rd.page.locator('[data-status="blocked"]')).waitFor();
    await rd.page.getByTestId("pane-q1").click();
    const rcard = rd.page.getByTestId(`dock-question-${q.id}`);
    await rcard.waitFor();
    if ((await rcard.getByRole("button", { name: "예" }).count()) !== 0) throw new Error("reader에게 답 버튼");
    if (!(await rcard.innerText()).includes("보기 권한이라 답할 수 없다")) throw new Error(`reader 카드: ${await rcard.innerText()}`);
    await shot(page, "desktop-question");
    await card.getByRole("button", { name: "예" }).click();
    await card.waitFor({ state: "detached" });
    await rcard.waitFor({ state: "detached" });
    const all = aaa("question", "list", "--all").questions;
    if (all.length !== 1 || all[0].state !== "answered" || all[0].answer !== "yes") throw new Error(JSON.stringify(all));
    await state(page).and(page.locator('[data-status="working"]')).waitFor();
    if (rd.errors.length) throw new Error(rd.errors.join("; "));
    await rd.page.context().close();
  });

  await step("확인창 답이 실패하면 카드 안에 보인다(herdr-web-ui PromptCard.tsx pending·role=alert, QA t-0337 Q3): 휴대폰에서 답하는 동안 선택지가 막히고, host가 선택 창을 못 찾은 오류가 그 카드에 뜨며 다시 누를 수 있다", async () => {
    // 선택 창 끝 줄(「Esc to cancel」)이 없는 확인창: host는 MENU_WAIT 동안 찾다가 오류로 답한다
    const script =
      'printf "Allow Bash(make lint)?\\n\\342\\235\\257 1. Yes\\n  2. No\\n"; ' +
      '"$0" hook --agent claude \'{"hook_event_name":"PermissionRequest","tool_name":"Bash","tool_input":{"command":"make lint"}}\'; ' +
      "stty -echo; exec cat > /dev/null";
    aaa("tab", "create", "--machine", "labA", "--label", "q2", "--", "/bin/sh", "-c", script, AA);
    const ph = await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 });
    await ph.page.getByTestId("agent-q2").and(ph.page.locator('[data-status="blocked"]')).waitFor();
    await ph.page.getByTestId("pane-q2").click();
    const q = aaa("question", "list").questions.find((x) => x.text.includes("make lint"));
    const id = `dock-question-${q.id}`;
    const card = ph.page.getByTestId(id);
    await card.waitFor();
    const answered = ph.page.waitForResponse((r) => r.url().endsWith("/api/v1/call") && r.request().postDataJSON()?.op === "question_answer");
    await card.getByRole("button", { name: "예" }).click();
    // 답이 가는 동안: 카드는 aria-busy, 선택지는 막힘(누른 순간의 모습을 본다. 답은 MENU_WAIT 뒤에 온다)
    const busy = await card.evaluate((c) => [c.getAttribute("aria-busy"), [...c.querySelectorAll(".q-actions button")].map((b) => b.disabled)]);
    if (JSON.stringify(busy) !== JSON.stringify(["true", [true, true]])) throw new Error(`답하는 동안 카드 ${JSON.stringify(busy)}`);
    const res = await (await answered).json();
    if (res.ok !== undefined) throw new Error(`실패해야 할 답이 성공했다: ${JSON.stringify(res)}`);
    await domIs(ph.page, "카드 안 오류", (sel) => document.querySelector(sel)?.textContent.includes("터미널에서 직접 답한다"), `[data-testid="${id}"] [role=alert]`);
    const after = await card.evaluate((c) => [c.getAttribute("aria-busy"), [...c.querySelectorAll(".q-actions button")].map((b) => b.disabled)]);
    if (JSON.stringify(after) !== JSON.stringify(["false", [false, false]])) throw new Error(`오류 뒤 카드 ${JSON.stringify(after)}`);
    // 오류는 카드에만(휴대폰은 pane을 보는 동안 목록 머리 띠가 숨는다)
    if (!(await card.getByRole("alert").isVisible())) throw new Error("휴대폰에서 카드 오류가 보이지 않는다");
    await shot(ph.page, "phone-question-error");
    if (ph.errors.length) throw new Error(ph.errors.join("; "));
    await ph.page.context().close();
    aaa("pane", "kill", "q2");
  });

  await step("끝남(안 봄)(herdr PaneState::seen, 차이 목록 S1): 보는 화면 없이 턴이 끝나면 초록 점, 그 pane을 열면 사라지고, 보는 동안 끝난 턴은 남지 않으며, 가려진 동안 끝나면 다시 보일 때 사라진다", async () => {
    // 줄 하나를 받을 때마다 턴을 열고(UserPromptSubmit) 다음 줄에 닫는다(Stop): 시험이 턴의 끝을 정한다
    const script =
      'while read a; do "$0" hook --agent claude \'{"hook_event_name":"UserPromptSubmit"}\'; ' +
      'read b; "$0" hook --agent claude \'{"hook_event_name":"Stop"}\'; done';
    aaa("tab", "create", "--machine", "labA", "--label", "seen1", "--", "/bin/sh", "-c", script, AA);
    const inUse = (p, on) =>
      p.evaluate((on) => {
        if (on) delete document.hasFocus;
        else document.hasFocus = () => false;
        window.dispatchEvent(new Event(on ? "focus" : "blur"));
      }, on);
    const state = page.getByTestId("agent-seen1");
    const is = (s) => state.and(page.locator(`[data-status="${s}"]`)).waitFor();
    const using = async () => (await page.evaluate(async () => (await (await fetch("/api/v1/push")).json()).ok.in_use));
    const turn = async (end, inUseNow) => {
      aaa("pane", "run", "seen1", "go");
      await is("working");
      // presence는 턴을 열기 전에 보냈고 host를 한 바퀴 돈 working이 왔다: host가 이미 안다(기다리지 않고 확인만 한다)
      const n = await using();
      if ((n > 0) !== inUseNow) throw new Error(`host가 아는 쓰는 화면 수 ${n}(쓰는 중 ${inUseNow})`);
      aaa("pane", "run", "seen1", "end");
      await is(end);
    };
    await inUse(page, true);
    // 1) 다른 pane(q1)을 보는 동안 끝났다: 끝남(안 봄), 초록 점과 그 말
    await turn("done", true);
    if ((await state.locator(".sidebar-status-dot").count()) !== 1) throw new Error("끝남에 점이 없다");
    if ((await state.getAttribute("aria-label")) !== "agent 끝남(안 봄)") throw new Error(`상태 말: ${await state.getAttribute("aria-label")}`);
    await shot(page, "desktop-unseen-done");
    // 2) 그 pane을 열면 봤다
    await page.getByTestId("pane-seen1").click();
    await is("idle");
    // 3) 보는 동안 끝난 턴은 끝남으로 남지 않는다
    await turn("idle", true);
    // 4) 가려진(쓰는 화면이 아닌) 동안 끝났다: 열어 둔 pane이라도 끝남, 다시 쓰는 화면이 되면 봤다
    await inUse(page, false);
    await turn("done", false);
    await inUse(page, true);
    await is("idle");
    aaa("pane", "kill", "seen1");
  });

  await step("입력 막대·CLI send-text 뒤 send-keys enter: 글 뒤 Enter를 따로 보낸다(Codex식 붙여넣기 판정)", async () => {
    aaa("tab", "create", "--machine", "labA", "--label", "burst", "--", process.execPath, `${root}/burst.js`);
    await page.getByTestId("pane-burst").click();
    await until("burst 화면", async () => (await screenText(page)).includes("burst>"));
    await page.getByLabel("보낼 글").fill("hello web");
    await page.getByRole("button", { name: "보내기" }).click();
    await until("웹 제출", async () => (await screenText(page)).includes("SUBMIT:hello web"));
    // CLI는 글과 Enter를 따로 쓴다(herdr send-text·send-keys). `pane run`은 글+Enter를 한 번에 쓰니(herdr) 여기서는 쓰지 않는다
    aaa("pane", "send-text", "burst", "hello cli");
    aaa("pane", "send-keys", "burst", "enter");
    await waitOutput("burst", "SUBMIT:hello cli");
    if ((await screenText(page)).includes("[newline]")) throw new Error("Enter가 글과 한 덩어리로 갔다");
    // 가짜 입력창이 한 덩어리를 실제로 가려내는지(이 단언이 없으면 위 단언은 아무것도 보지 않는다)
    aaa("pane", "send-text", "burst", "one\r");
    await until("한 덩어리 판정", async () => (await screenText(page)).includes("[newline]"));
    aaa("pane", "kill", "burst");
  });

  await step("입력줄: 여러 줄은 bracketed paste, Enter 보내기·Shift+Enter 줄바꿈, 조합 중 Enter 무시, 초안 보존, 휴대폰도 Enter는 보내기", async () => {
    aaa("tab", "create", "--machine", "labA", "--label", "pasteapp", "--", process.execPath, `${root}/paste.js`);
    const got = () => aaa("pane", "read", "pasteapp", "--source", "visible").text;
    const gots = () => got().split("GOT:").length - 1;
    await page.getByTestId("pane-pasteapp").click();
    await until("paste 화면", async () => (await screenText(page)).includes("paste>"));
    const box = page.getByLabel("보낼 글");
    // 여러 줄: 프로그램이 bracketed paste를 켰으니 감싸서 한 덩어리로, 줄바꿈은 CR로. Enter는 따로.
    await box.fill("줄1\n줄2");
    await page.getByRole("button", { name: "보내기" }).click();
    await until("여러 줄 붙여넣기", () => got().includes('GOT:"\\u001b[200~줄1\\r줄2\\u001b[201~"'));
    await until("뒤 Enter", () => got().split('GOT:"\\r"').length - 1 === 1);
    // 데스크톱 Enter는 보낸다
    await box.fill("한 줄");
    await box.press("Enter");
    await until("Enter로 보냄", () => got().includes('GOT:"한 줄"') && got().split('GOT:"\\r"').length - 1 === 2);
    // Shift+Enter는 줄바꿈이고 보내지 않는다
    let n = gots();
    await box.press("a");
    await box.press("Shift+Enter");
    await box.press("b");
    if ((await box.inputValue()) !== "a\nb") throw new Error(JSON.stringify(await box.inputValue()));
    // 조합 중 Enter는 보내지 않는다. 같은 사건이 조합 중이 아니면 보낸다(이 단언이 실제로 가려내는지).
    await box.evaluate((el) => el.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", isComposing: true, bubbles: true })));
    await sleep(500);
    if (gots() !== n || (await box.inputValue()) !== "a\nb") throw new Error(`조합 중 Enter로 보냈다: ${got()}`);
    // 초안: 다시 열어도 남는다
    await page.reload();
    await page.getByTestId("machine-labA").waitFor();
    await until("초안 보존", async () => (await page.getByLabel("보낼 글").inputValue()) === "a\nb");
    // 다시 연 화면은 host의 readonly 알림이 올 때까지 입력줄이 꺼져 있고, 꺼진 동안의 Enter는 초안을 두고 보내지 않는다
    await until("입력줄 켜짐", () => page.getByLabel("보낼 글").isEnabled());
    await page.getByLabel("보낼 글").evaluate((el) => el.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })));
    // 여러 줄 붙여넣기 다음 Enter는 300ms 뒤 따로 온다(둘 다 와야 끝난다)
    await until("조합이 아닌 Enter는 보냄", () => gots() >= n + 2 && got().trimEnd().endsWith('GOT:"\\r"\npaste>'));
    if ((await page.getByLabel("보낼 글").inputValue()) !== "") throw new Error("보낸 뒤 글이 남았다");
    // 조합 중에 누른 보내기(herdr-web-ui scripts/terminal-input-regression.ts와 같은 네 경우)
    {
      const line = page.getByLabel("보낼 글");
      const send = page.getByRole("button", { name: "보내기" });
      const banner = () => page.locator(".terminal-input-note").evaluateAll((els) => els[0]?.textContent ?? null);
      const since = () => got().split('GOT:"MARK"').at(-1);
      const mark = async () => {
        aaa("pane", "send-text", "pasteapp", "MARK");
        await until("표지", () => got().includes('GOT:"MARK"'));
      };
      // 1) 실제 조합(Chrome IME 길): 누르는 사이 확정되어 바로 보낸다. 입력칸은 focus를 지킨다(키보드가 그대로)
      await mark();
      await line.fill("real 한");
      await line.focus();
      const cdp = await page.context().newCDPSession(page);
      await cdp.send("Input.imeSetComposition", { text: "글", selectionStart: 1, selectionEnd: 1 });
      await until("조합 중 값", async () => (await line.inputValue()) === "real 한글");
      await send.click();
      await until("조합 중 보내기", () => since().includes('GOT:"real 한글"') && since().includes('GOT:"\\r"'));
      await cdp.detach();
      if (!(await line.evaluate((el) => el === document.activeElement))) throw new Error("보낸 뒤 입력칸이 focus를 잃었다");
      // 2) 기다리는 사이 확정: 확정된 글을 한 번 보낸다(compositionend와 값 바꾸기가 같은 task여도)
      await mark();
      await line.fill("commit 한");
      await line.dispatchEvent("compositionstart", { data: "" });
      await send.click();
      await until("기다림 표시", async () => (await banner()) === "글자를 마치는 중…");
      if (since().includes("commit")) throw new Error("확정 전에 보냈다");
      await line.evaluate((el) => {
        const setValue = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value").set;
        el.dispatchEvent(new CompositionEvent("compositionend", { bubbles: true, data: "글" }));
        setValue.call(el, "commit 한글");
        el.dispatchEvent(new InputEvent("input", { bubbles: true, data: "글", inputType: "insertFromComposition" }));
      });
      await until("확정 뒤 보냄", () => since().includes('GOT:"commit 한글"') && since().includes('GOT:"\\r"'));
      // 3) 확정 전에 지워짐: 글 보내기였으니 Enter도 보내지 않는다
      await mark();
      await line.fill("한");
      await line.dispatchEvent("compositionstart", { data: "" });
      await send.click();
      await until("기다림 표시", async () => (await banner()) === "글자를 마치는 중…");
      await line.evaluate((el) => {
        const setValue = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value").set;
        el.dispatchEvent(new CompositionEvent("compositionend", { bubbles: true, data: "" }));
        setValue.call(el, "");
        el.dispatchEvent(new InputEvent("input", { bubbles: true, inputType: "deleteCompositionText" }));
      });
      await sleep(400);
      if (since().includes("GOT:")) throw new Error(`빈 확정에 보냈다: ${since()}`);
      // 4) 조합이 이어져도 누른 때부터의 기한을 넘기지 않는다
      await line.fill("chain 한");
      await line.dispatchEvent("compositionstart", { data: "" });
      await send.click();
      await until("기다림 표시", async () => (await banner()) === "글자를 마치는 중…");
      const t0 = Date.now();
      while (Date.now() - t0 < 450) {
        await line.evaluate((el) => {
          el.dispatchEvent(new CompositionEvent("compositionend", { bubbles: true, data: "한" }));
          el.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true, data: "" }));
        });
        await sleep(40);
      }
      if ((await banner()) !== "글자를 마친 뒤 보낸다") throw new Error(`기한 뒤 표시: ${await banner()}`);
      await line.dispatchEvent("compositionend", { data: "한" });
      await sleep(100);
      if (since().includes("GOT:")) throw new Error(`기한 넘은 조합을 보냈다: ${since()}`);
      await line.fill("");
    }
    // 휴대폰(터치)도 Enter는 보내기다(herdr-web-ui TerminalInput: enterKeyHint="send", Shift+Enter만 줄바꿈)
    const mob = await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 }, { line: true });
    await mob.page.getByTestId("pane-pasteapp").click();
    const mbox = mob.page.getByLabel("보낼 글");
    // 연결(snapshot)되기 전에는 읽기 전용이다: 화면이 온 뒤에 본다
    await until("휴대폰 paste 화면", async () => (await screenText(mob.page)).includes("paste>"));
    // 데스크톱 화면이 쓰는 중이어도 가져오기 없이 입력된다
    if (await mbox.isDisabled()) throw new Error("휴대폰 입력 막대가 꺼져 있다");
    if ((await mbox.getAttribute("enterkeyhint")) !== "send") throw new Error(`휴대폰 enterkeyhint ${await mbox.getAttribute("enterkeyhint")}`);
    n = gots();
    await mbox.fill("휴대폰");
    await mbox.press("Enter");
    await until("휴대폰 Enter로 보냄", () => got().includes('GOT:"휴대폰"') && gots() >= n + 2);
    if ((await mbox.inputValue()) !== "") throw new Error(`휴대폰 보낸 뒤 글이 남았다: ${JSON.stringify(await mbox.inputValue())}`);
    if (mob.errors.length) throw new Error(mob.errors.join("; "));
    await mob.page.context().close();
    aaa("pane", "kill", "pasteapp");
    await page.getByTestId("pane-t1").click();
    await until("t1 터미널", async () => (await screenText(page)).includes("한글-8"));
  });

  await step("보내는 동안 친 글 지키기(herdr-web-ui submit·submit-result, terminalDraft): 입력줄은 host 답을 받고서야 보낸 앞부분만 지운다. 그 사이 친 글은 남고, 거절·끊김이면 글을 두고 이유", async () => {
    aaa("tab", "create", "--machine", "labA", "--label", "submitapp", "--", process.execPath, `${root}/paste.js`);
    const got = () => aaa("pane", "read", "submitapp", "--source", "visible").text;
    // host의 답(submitted)을 붙잡아 두는 화면: 답이 오기 전 상태를 본다
    const held = [];
    let pipe = null;
    const holdReplies = (ws) => {
      const server = ws.connectToServer();
      pipe = { ws, server };
      ws.onMessage((m) => server.send(m));
      server.onMessage((m) => {
        if (typeof m === "string" && m.startsWith('{"t":"submitted"')) held.push(m);
        else ws.send(m);
      });
      server.onClose((code, reason) => ws.close({ code, reason }));
      ws.onClose((code, reason) => server.close({ code, reason }));
    };
    const sp = (await pairPage(browser, aaa("web", "pair").code, { width: 1100, height: 760 }, { line: true, wsRoute: [/\/api\/v1\/panes\//, holdReplies] })).page;
    await sp.getByTestId("pane-submitapp").click();
    await until("submitapp 화면", async () => (await screenText(sp)).includes("paste>"));
    const box = sp.getByLabel("보낼 글");
    const sendBtn = sp.getByRole("button", { name: "터미널에 보내기" });
    const note = () => sp.locator(".terminal-input-note").evaluateAll((els) => els[0]?.textContent ?? null);
    const reply = async () => {
      await until("host 답", () => held.length > 0);
      return JSON.parse(held.shift());
    };
    // 1) host가 글과 Enter를 쓴 뒤 답한다. 답 전에는 글이 남고 보내기는 잠긴다. 그 사이 덧붙인 글은 답 뒤에도 남는다
    await box.fill("첫 줄");
    await box.press("Enter");
    const first = await reply();
    if (!first.ok || !got().includes('GOT:"첫 줄"') || !got().includes('GOT:"\\r"')) throw new Error(`답 전에 글·Enter가 pane에 없다: ${JSON.stringify(first)} ${got()}`);
    if ((await box.inputValue()) !== "첫 줄" || !(await sendBtn.isDisabled())) throw new Error(`답 전 입력줄 ${JSON.stringify(await box.inputValue())} 잠김 ${await sendBtn.isDisabled()}`);
    await box.press("End");
    await box.pressSequentially(" 더");
    pipe.ws.send(JSON.stringify(first));
    await until("보낸 앞부분만 지움", async () => (await box.inputValue()) === " 더");
    if ((await note()) !== null) throw new Error(`알림 ${await note()}`);
    // 2) 그 사이 글을 바꿨으면(보낸 글로 시작하지 않으면) 지우지 않는다
    await box.fill("둘");
    await box.press("Enter");
    const second = await reply();
    await box.fill("다른 글");
    pipe.ws.send(JSON.stringify(second));
    await sleep(300);
    if ((await box.inputValue()) !== "다른 글") throw new Error(`바꾼 글을 지웠다: ${JSON.stringify(await box.inputValue())}`);
    // 3) host가 거절하면 글을 두고 이유를 보인다(답을 바꿔 보낸다. 실제 거절은 아래 끝난 pane)
    await box.fill("셋");
    await box.press("Enter");
    const third = await reply();
    pipe.ws.send(JSON.stringify({ ...third, ok: false, message: "시험 거절" }));
    await until("거절 이유", async () => (await note()) === "보내지 못했다: 시험 거절");
    if ((await box.inputValue()) !== "셋" || (await sendBtn.isDisabled())) throw new Error(`거절 뒤 입력줄 ${JSON.stringify(await box.inputValue())}`);
    // 4) 답 전에 끊기면 들어갔는지 모른다: 글을 두고 터미널을 보게 한다
    await box.fill("넷");
    await box.press("Enter");
    await reply();
    pipe.ws.close({ code: 1011, reason: "test" });
    await until("확인 못 함", async () => (await note()) === "확인하지 못했다. 다시 보내기 전에 터미널을 본다");
    if ((await box.inputValue()) !== "넷") throw new Error(`끊긴 뒤 입력줄 ${JSON.stringify(await box.inputValue())}`);
    // 5) 실제 host 거절: 끝난 pane에 보낸 submit은 ok:false와 이유로 답한다(답 없이 두지 않는다)
    aaa("tab", "create", "--machine", "labA", "--label", "submitend", "--", process.execPath, "-e", "process.exit(0)");
    await until("submitend 끝남", () => aaa("pane", "list").panes.find((x) => x.name === "submitend")?.state !== "running");
    // 붙잡는 route가 없는 화면에서 연다
    const refused = await page.evaluate(
      (id) =>
        new Promise((resolve) => {
          const w = new WebSocket(`ws://${location.host}/api/v1/panes/${encodeURIComponent(id)}/ws?flow=ack`);
          w.onmessage = (m) => {
            if (typeof m.data !== "string") return;
            const j = JSON.parse(m.data);
            if (j.t === "snapshot") w.send(JSON.stringify({ t: "submit", id: 7, data: "late" }));
            if (j.t === "submitted") resolve(j);
          };
          w.onclose = (e) => resolve({ closed: e.code });
          setTimeout(() => resolve({ timeout: true }), 5000);
        }),
      aaa("pane", "list").panes.find((x) => x.name === "submitend").id,
    );
    if (refused.id !== 7 || refused.ok !== false || !refused.message) throw new Error(`끝난 pane submit 답 ${JSON.stringify(refused)}`);
    await sp.context().close();
    aaa("pane", "kill", "submitapp");
  });

  await step("터미널 한글 IME(herdr-web-ui xterm 패치): 데스크톱 조합 사건·CDP 실제 IME, macOS Safari·iPad 기록 재생 → pane에는 완성된 글만", async () => {
    aaa("tab", "create", "--machine", "labA", "--label", "imeapp", "--", process.execPath, `${root}/paste.js`);
    aaa("tab", "create", "--machine", "labA", "--label", "imespin", "--", process.execPath, `${root}/spin.js`);
    const wk = await webkit.launch();
    const bad = [];
    const textarea = (q) => q.locator("textarea.xterm-helper-textarea");
    const press = async (q, ...keys) => {
      for (const k of keys) await textarea(q).press(k);
    };
    // 데스크톱(실제 브라우저 판)
    const desktop = [
      ["같은 task", (q) => imeType(q), "상태"],
      // 한글 입력기에서 숫자·문장부호: keydown(229) 뒤 조합 없이 글만 늘어난다
      [
        "조합 없는 229(숫자)",
        (q) =>
          textarea(q).evaluate(async (el) => {
            for (const d of ["7", "."]) {
              el.dispatchEvent(new KeyboardEvent("keydown", { key: "Process", keyCode: 229, which: 229, bubbles: true, cancelable: true }));
              el.value += d;
              el.dispatchEvent(new InputEvent("input", { data: d, inputType: "insertText", bubbles: true, composed: true }));
              el.dispatchEvent(new KeyboardEvent("keyup", { key: d, keyCode: 229, bubbles: true }));
              await new Promise((r) => setTimeout(r, 40));
            }
          }),
        "7.",
      ],
    ];
    const cdpIme = [
      "CDP 실제 IME",
      async (q) => {
        const cdp = await q.context().newCDPSession(q);
        for (const syl of [["ㅅ", "사", "상"], ["ㅌ", "태"]]) {
          for (const t of syl) await cdp.send("Input.imeSetComposition", { text: t, selectionStart: t.length, selectionEnd: t.length });
          await cdp.send("Input.insertText", { text: syl.at(-1) });
        }
        await cdp.detach();
      },
      "상태",
    ];
    // macOS Safari 26.6.2 기록(herdr-web-ui terminal-safari-ime-regression.ts)
    const safari = [
      [
        "영문 뒤 한글",
        async (q) => {
          await press(q, "a", "b", "c");
          await safariSyllable(q, ["ㅎ", "하", "한"]);
          await safariEdit(q, "insertReplacementText", "한", 0, 1);
          await safariSyllable(q, ["ㄱ", "그", "글"], 1);
          await safariEdit(q, "insertReplacementText", "글", 1, 2);
          await press(q, "Space");
        },
        "abc한글 ",
      ],
      [
        "Enter 앞 확정",
        async (q) => {
          await safariSyllable(q, ["ㅎ", "하", "한"]);
          await press(q, "Enter");
        },
        "한\r",
      ],
      [
        "받침 넘김",
        async (q) => {
          await safariSyllable(q, ["ㄱ", "가", "갑", "값"]);
          await safariEdit(q, "insertReplacementText", "갑", 0, 1);
          await safariSyllable(q, ["사"], 1);
          await press(q, "Space");
        },
        "갑사 ",
      ],
      [
        "확정 전 지우기",
        async (q) => {
          await safariSyllable(q, ["ㅎ", "하", "한"]);
          await safariEdit(q, "insertReplacementText", "하", 0, 1);
          await safariEdit(q, "insertReplacementText", "ㅎ", 0, 1);
          await safariEdit(q, "deleteContentBackward", "", 0, 1);
          await press(q, "a");
        },
        "a",
      ],
      [
        "Safari 실제 조합 사건",
        (q) =>
          textarea(q).evaluate(async (box) => {
            box.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true }));
            box.dispatchEvent(new InputEvent("beforeinput", { bubbles: true, inputType: "insertCompositionText", data: "한", isComposing: true }));
            box.value = "한";
            box.dispatchEvent(new CompositionEvent("compositionupdate", { bubbles: true, data: "한" }));
            box.dispatchEvent(new InputEvent("input", { bubbles: true, inputType: "insertCompositionText", data: "한", isComposing: true }));
            await new Promise((r) => requestAnimationFrame(() => r()));
            box.dispatchEvent(new CompositionEvent("compositionend", { bubbles: true, data: "한" }));
          }),
        "한",
      ],
    ];
    // iPadOS Safari + 하드웨어 키보드 기록(herdr-web-ui terminal-ipad-ime-regression.ts)
    const ipad = [
      [
        "영문 뒤 한글",
        async (q) => {
          await press(q, "a", "b", "c");
          await ipadFresh(q, "ㅎ");
          await ipadReplace(q, "ㅏ", "하");
          await ipadReplace(q, "ㄴ", "한");
          await ipadFresh(q, "ㄱ");
          await ipadReplace(q, "ㅡ", "그");
          await ipadReplace(q, "ㄹ", "글");
          await press(q, "Space");
        },
        "abc한글 ",
      ],
      [
        "두 음절 바꿔 넣기",
        async (q) => {
          await ipadFresh(q, "ㅁ");
          await ipadReplace(q, "ㅏ", "마");
          await ipadReplace(q, "ㅇ", "망");
          await ipadReplace(q, "ㅏ", "마아");
          await press(q, "Space");
        },
        "마아 ",
      ],
      [
        "Enter 앞 확정",
        async (q) => {
          await ipadFresh(q, "ㅎ");
          await ipadReplace(q, "ㅏ", "하");
          await ipadReplace(q, "ㄴ", "한");
          await press(q, "Enter");
        },
        "한\r",
      ],
    ];
    const SAFARI_UA = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 Version/26.6.2 Safari/605.1.15";
    const IPAD_UA = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/27.2 Safari/605.1.15";
    const wide = { width: 1280, height: 800 };
    const IPAD_PORTRAIT = { width: 820, height: 1180 };
    const runs = [
      ["Chromium", async () => page, [...desktop, cdpIme]],
      ["WebKit", async () => (await pairPage(wk, aaa("web", "pair").code, wide)).page, desktop],
      ["Safari 26.6.2 기록", async () => (await pairPage(browser, aaa("web", "pair").code, wide, { userAgent: SAFARI_UA, platform: "MacIntel" })).page, safari],
      ["iPad(MacIntel) 기록", async () => (await pairPage(browser, aaa("web", "pair").code, wide, { userAgent: IPAD_UA, platform: "MacIntel" })).page, ipad],
      ["iPad(iPad) 기록", async () => (await pairPage(browser, aaa("web", "pair").code, wide, { userAgent: IPAD_UA, platform: "iPad" })).page, ipad],
      // 화면 키보드가 올라온 iPad(사용자 보고 13:0x: 키보드 고침 뒤 자모가 다시 분리됐다). 키보드가 grid를 붙잡고
      // 터미널을 커서 줄로 옮긴 상태에서, 출력이 계속 오는 동안 같은 기록을 재생한다.
      ...[["iPad", ipad], ["Safari", safari]].map(([rec, ways]) => [
        `WebKit iPad 화면 키보드 ${rec} 기록`,
        async () => (await pairPage(wk, aaa("web", "pair").code, IPAD_PORTRAIT, { userAgent: IPAD_UA, platform: "iPad", touch: true, init: fakeViewport })).page,
        ways,
        async (p) => {
          await softKeyboard(p, IPAD_PORTRAIT.height - 400);
          await sleep(600);
          const up = await p.evaluate(() => document.documentElement.hasAttribute("data-keyboard"));
          const panned = await p.locator(".term-scroll").evaluate((el) => el.scrollTop);
          if (!up || panned <= 0) throw new Error(`화면 키보드 흉내가 안 됐다(data-keyboard ${up}, 옮긴 거리 ${panned})`);
          // iOS는 caret이 움직이면 보이는 영역을 옮기고 visualViewport scroll을 보낸다(viewport.ts가 그때마다 맞춘다)
          await p.evaluate(() => setInterval(() => window.viewportFixture.viewport.dispatchEvent(new Event("scroll")), 30));
        },
        "imespin",
      ]),
    ];
    try {
      for (const [engine, open, ways, keyboard, pane = "imeapp"] of runs) {
        const p = await open();
        await p.getByTestId(`pane-${pane}`).click();
        await until("ime 화면", async () => (await screenText(p)).includes("paste>"));
        // 다른 화면이 쓰던 pane이어도 터미널을 누르고 바로 친다
        await until("입력 가능", async () => (await p.getByTestId("readonly-banner").count()) === 0);
        await p.locator(".xterm-screen").click();
        if (keyboard) await keyboard(p);
        for (const [how, type, want] of ways) {
          aaa("pane", "send-text", pane, "MARK");
          await waitOutput(pane, 'GOT:"MARK"');
          // 기록 재생은 빈 편집기에서 시작한다(herdr-web-ui 재생과 같다)
          await textarea(p).evaluate((el) => {
            el.value = "";
          });
          await type(p);
          await sleep(700);
          const all = aaa("pane", "read", pane, "--source", "visible").text;
          const got = all
            .slice(all.lastIndexOf('GOT:"MARK"') + 10)
            .split("\n")
            .filter((l) => l.startsWith("GOT:"))
            .map((l) => JSON.parse(l.slice(4)))
            .join("");
          console.log(`     IME ${engine} ${how}: ${JSON.stringify(got)}`);
          if (got !== want) bad.push(`${engine} ${how}: pane이 받은 글 ${JSON.stringify(got)}(기대 ${JSON.stringify(want)})`);
        }
        if (p !== page) await p.context().close();
      }
    } finally {
      await wk.close();
    }
    if (bad.length) throw new Error(bad.join("; "));
    aaa("pane", "kill", "imeapp");
    aaa("pane", "kill", "imespin");
    await page.getByTestId("pane-t1").click();
    await until("t1 터미널", async () => (await screenText(page)).includes("한글-8"));
  });

  await step("iPhone 화면 키보드 사용자 기록 재생(10-17): 「한글」을 자모마다 keydown으로 친 뒤 blur → pane에는 완성된 음절만", async () => {
    aaa("tab", "create", "--machine", "labA", "--label", "iphoneA", "--", process.execPath, `${root}/spin.js`);
    const IPHONE_UA = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_7 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/27.0.1 Mobile/15E148 Safari/604.1";
    const IPHONE = { width: 430, height: 932 };
    const wk = await webkit.launch();
    const bad = [];
    try {
      for (const [label, b, keyboard] of [["WebKit iPhone 화면 키보드", wk, true], ["Chromium iPhone", browser, false]]) {
        const { page: p } = await pairPage(b, aaa("web", "pair").code, IPHONE, { userAgent: IPHONE_UA, platform: "iPhone", touch: true, init: fakeViewport });
        const sent = [];
        const frames = [];
        let bins = 0;
        let ready = false;
        await p.routeWebSocket(/\/api\/v1\/panes\/[^/]+\/ws/, (socket) => {
          const server = socket.connectToServer();
          server.onMessage((raw) => {
            if (typeof raw === "string") {
              const m = JSON.parse(raw);
              // snapshot은 replay 길이도(빈 화면이 빈 replay였는지 가린다)
              frames.push(m.t === "snapshot" ? `snapshot:${m.replay?.length ?? 0}` : m.t);
              if (m.t === "snapshot" || m.t === "readonly") ready = m.readonly === false;
            } else bins++;
            socket.send(raw);
          });
          socket.onMessage((raw) => {
            const m = typeof raw === "string" ? JSON.parse(raw) : null;
            if (m?.t === "input") sent.push(m.data);
            else server.send(raw);
          });
        });
        // 가로채기는 새로 연 문서부터 붙는다(참고 시험도 reset마다 다시 연다)
        await p.goto(base);
        await p.getByTestId("pane-iphoneA").evaluate((node) => node.click());
        await until("iPhone 준비", () => ready).catch(async (e) => {
          throw new Error(`${e.message}: ${label} 프레임 ${JSON.stringify(frames)} 화면 ${JSON.stringify((await p.locator("body").innerText()).slice(0, 300))}`);
        });
        await until("iPhone 화면", async () => (await screenText(p)).includes("paste>")).catch(async (e) => {
          // CI에서만 실패한 적이 있다(10-17 dbacbfe·372e85d·e784eac): 그때 화면이 어땠는지 남긴다
          const seen = await termDiag(p);
          throw new Error(`${e.message}: ${label} 프레임 ${JSON.stringify(frames.slice(-12))} 출력 ${bins}조각 ${JSON.stringify(seen)} pane ${JSON.stringify(aaa("pane", "read", "iphoneA", "--source", "visible").text.split("\n").filter(Boolean).slice(-2))}`);
        });
        const input = p.locator(".xterm-helper-textarea");
        await input.focus();
        // 기록처럼 화면 키보드가 올라온다(보이는 높이 873 → 485)
        if (keyboard) {
          await softKeyboard(p, 485);
          await until("화면 키보드", () => p.evaluate(() => document.documentElement.hasAttribute("data-keyboard")));
          await sleep(300);
        }
        await input.evaluate((box) => {
          box.value = "";
        });
        sent.length = 0;
        const fresh = (jamo) => iphoneKey(p, jamo, [["insert", jamo]]);
        const replace = (jamo, result) => iphoneKey(p, jamo, [["delete"], ["insert", result]]);
        await fresh("ㅎ");
        await replace("ㅏ", "하");
        await replace("ㄴ", "한");
        if (sent.join("") !== "") bad.push(`${label}: 조합 중 자모를 보냈다 ${JSON.stringify(sent)}`);
        await fresh("ㄱ");
        await replace("ㅡ", "그");
        await replace("ㄹ", "글");
        // 기록 끝: 키보드를 내리며 blur
        await input.evaluate((box) => box.blur());
        await until(`${label} blur 확정`, () => sent.join("").length >= 2, 3000).catch(() => {});
        await sleep(300);
        console.log(`     iPhone 기록 ${label}: ${JSON.stringify(sent.join(""))}`);
        if (sent.join("") !== "한글") bad.push(`${label}: pane이 받은 글 ${JSON.stringify(sent.join(""))}(기대 "한글")`);
        await p.goto("about:blank").catch(() => {});
        await p.context().close();
      }
    } finally {
      await wk.close();
    }
    aaa("pane", "kill", "iphoneA");
    if (bad.length) throw new Error(bad.join("; "));
  });

  await step("iPad IME 회귀(herdr-web-ui scripts/terminal-ipad-ime-regression.ts 그대로): Chromium MacIntel·iPad, 화면 키보드가 올라온 WebKit iPad", async () => {
    // 참고 시험처럼 pane WebSocket을 가로채 보낸 input 프레임을 그대로 본다(입력은 pane에 가지 않는다).
    // 두 pane은 참고처럼 조용한 입력창이다(출력이 오는 동안의 조합은 「터미널 한글 IME」 단계의 화면 키보드 판이 본다).
    for (const n of ["ipadA", "ipadB"]) aaa("tab", "create", "--machine", "labA", "--label", n, "--", process.execPath, `${root}/paste.js`);
    const ids = Object.fromEntries(aaa("pane", "list").panes.filter((x) => x.name.startsWith("ipad")).map((x) => [x.id, x.name]));
    const wk = await webkit.launch();
    const IPAD_UA = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/27.2 Safari/605.1.15";
    const IPAD_PORTRAIT = { width: 820, height: 1180 };
    const runs = [
      ["Chromium MacIntel", () => pairPage(browser, aaa("web", "pair").code, { width: 1100, height: 700 }, { userAgent: IPAD_UA, platform: "MacIntel" }), false],
      ["Chromium iPad", () => pairPage(browser, aaa("web", "pair").code, { width: 1100, height: 700 }, { userAgent: IPAD_UA, platform: "iPad" }), false],
      [
        "WebKit iPad 화면 키보드",
        () => pairPage(wk, aaa("web", "pair").code, IPAD_PORTRAIT, { userAgent: IPAD_UA, platform: "iPad", touch: true, init: fakeViewport }),
        true,
      ],
    ];
    const bad = [];
    try {
      for (const [label, open, keyboard] of runs) {
        const { page: p } = await open();
        const sent = [];
        let ready = false;
        // pane이 쓴 것처럼 화면에 보낼 출력(참고 toPage)
        let toPage = () => {};
        await p.routeWebSocket(/\/api\/v1\/panes\/[^/]+\/ws/, (socket) => {
          const pane = ids[decodeURIComponent(socket.url().split("/panes/")[1].split("/")[0])];
          const server = socket.connectToServer();
          toPage = (data) => socket.send(Buffer.from(data));
          server.onMessage((raw) => {
            if (typeof raw === "string") {
              const m = JSON.parse(raw);
              if (m.t === "snapshot" || m.t === "readonly") ready = m.readonly === false;
            }
            socket.send(raw);
          });
          socket.onMessage((raw) => {
            const m = typeof raw === "string" ? JSON.parse(raw) : null;
            if (m?.t === "input") sent.push({ pane, data: m.data });
            else server.send(raw);
          });
        });
        const input = p.locator(".xterm-helper-textarea");
        const select = async (name) => {
          ready = false;
          // 참고처럼 focus를 옮기지 않는 click(조합 중 글자가 blur로 확정되지 않게)
          await p.getByTestId(`pane-${name}`).evaluate((node) => node.click());
          await until(`${name} 준비`, () => ready);
          await until(`${name} 화면`, async () => (await screenText(p)).includes("paste>"));
          await input.focus();
          if (keyboard) {
            await softKeyboard(p, IPAD_PORTRAIT.height - 400);
            await until("화면 키보드", () => p.evaluate(() => document.documentElement.hasAttribute("data-keyboard")));
            await sleep(300);
          }
        };
        const reset = async () => {
          await p.goto(base);
          await select("ipadA");
          await input.evaluate((box) => {
            box.value = "";
          });
          sent.length = 0;
        };
        const fresh = (jamo) => ipadKey(p, jamo, [["insert", jamo]]);
        const replace = (jamo, result) => ipadKey(p, jamo, [["delete"], ["insert", result]]);
        const text = () => sent.map((f) => f.data).join("");
        const check = async (expected, what) => {
          await input.press("Control+g");
          await until(`${what}: ^G`, () => sent.at(-1)?.data === "\x07");
          if (text() !== expected + "\x07") bad.push(`${label} ${what}: ${JSON.stringify(text())}(기대 ${JSON.stringify(expected + "\x07")})`);
          if (!sent.every((f) => f.pane === "ipadA")) bad.push(`${label} ${what}: 다른 pane으로 갔다 ${JSON.stringify(sent)}`);
        };
        await reset();
        await input.press("a");
        await input.press("b");
        await input.press("c");
        if ((await fresh("ㅎ")) !== true) bad.push(`${label}: 한글 keydown을 편집기에 맡기지 않았다`);
        await replace("ㅏ", "하");
        await replace("ㄴ", "한");
        if (text() !== "abc") bad.push(`${label}: 조합 중 자모를 보냈다 ${JSON.stringify(text())}`);
        await fresh("ㄱ");
        await replace("ㅡ", "그");
        await replace("ㄹ", "글");
        await input.press("Space");
        await check("abc한글 ", "영문 뒤 한글은 음절마다 한 번씩 순서대로");
        // 조합 중 글자는 그 줄의 확정 글자 크기로 그린다
        toPage("\r\n한 ");
        await p.waitForFunction(() => [...document.querySelectorAll(".xterm-rows span")].some((span) => span.textContent?.includes("한")));
        await fresh("ㄱ");
        await replace("ㅡ", "그");
        const sizes = await p.evaluate(() => {
          const row = [...document.querySelectorAll(".xterm-rows span")].find((span) => span.textContent?.includes("한"));
          const view = document.querySelector(".composition-view");
          return { row: getComputedStyle(row).fontSize, view: getComputedStyle(view).fontSize, pending: view.classList.contains("active") ? view.textContent : null };
        });
        if (sizes.pending !== "그") bad.push(`${label}: 조합 중 글자 ${JSON.stringify(sizes.pending)}`);
        else if (Math.abs(parseFloat(sizes.view) - parseFloat(sizes.row)) >= 0.01) bad.push(`${label}: 조합 글자 크기 ${sizes.view} ≠ 줄 ${sizes.row}`);

        await reset();
        // 망 다음 ㅏ: 받침 ㅇ이 넘어가며 편집기가 두 음절을 한 번에 바꿔 넣는다
        await fresh("ㅁ");
        await replace("ㅏ", "마");
        await replace("ㅇ", "망");
        await replace("ㅏ", "마아");
        await input.press("Space");
        await check("마아 ", "두 음절 바꿔 넣기");

        for (const [key, expected] of [["Enter", "\r"], ["Control+c", "\x03"], ["Tab", "\t"]]) {
          await reset();
          await fresh("ㅎ");
          await replace("ㅏ", "하");
          await replace("ㄴ", "한");
          await input.press(key);
          await check("한" + expected, `${key} 앞 확정`);
        }

        await reset();
        await fresh("ㅎ");
        await replace("ㅏ", "하");
        await replace("ㄴ", "한");
        await input.evaluate((box) => box.blur());
        await until("blur 확정", () => sent.length > 0);
        await input.focus();
        await check("한", "blur가 조합 중 글자를 한 번 확정");

        await reset();
        await fresh("ㅎ");
        await replace("ㅏ", "하");
        await select("ipadB");
        await input.press("Control+g");
        await until("다른 pane ^G", () => sent.at(-1)?.data === "\x07");
        if (text() !== "\x07" || sent.some((f) => f.pane !== "ipadB"))
          bad.push(`${label}: 조합 중 글자가 다른 pane으로 갔다 ${JSON.stringify(sent)}`);
        console.log(`     iPad IME 회귀 ${label}: ${bad.length ? "실패 있음" : "통과"}`);
        await p.goto("about:blank").catch(() => {});
        await p.context().close();
      }
    } finally {
      await wk.close();
    }
    for (const n of ["ipadA", "ipadB"]) aaa("pane", "kill", n);
    if (bad.length) throw new Error(bad.join("; "));
  });

  await step("입력 기록(실기 키보드 진단): 켜면 터미널 사건·textarea 값·화면 키보드 상태·보낸 글을 모으고, 보내면 host diag/에 남는다", async () => {
    // pairing 없이는 남기지 않는다
    const anon = await fetch(`${base}api/v1/diag/input`, {
      method: "POST",
      headers: { "content-type": "application/json", origin: base.slice(0, -1) },
      body: "{}",
    });
    if (anon.status !== 401) throw new Error(`인증 없는 기록 보내기가 ${anon.status}`);
    aaa("tab", "create", "--machine", "labA", "--label", "traceapp", "--", process.execPath, `${root}/paste.js`);
    const tp = (await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 })).page;
    // 목록에는 진단 단추가 없고(참고 목록에 그런 자리가 없다) 설정 → 터미널 쪽 맨 끝 「진단」에 있다(사용자 화면 10-18, plan §8.82·§8.92)
    if ((await tp.getByText("입력 기록", { exact: false }).count()) !== 0) throw new Error("목록에 입력 기록 단추가 있다");
    const dialog = tp.getByRole("dialog", { name: "설정" });
    const traceSwitch = dialog.getByRole("switch", { name: "입력 기록" });
    await openSettings(tp, "터미널");
    await traceSwitch.click();
    if ((await traceSwitch.getAttribute("aria-checked")) !== "true") throw new Error("입력 기록이 켜지지 않았다");
    await tp.keyboard.press("Escape");
    await tp.getByTestId("pane-traceapp").click();
    await until("trace 화면", async () => (await screenText(tp)).includes("paste>"));
    await tp.locator(".xterm-screen").click();
    await imeType(tp);
    await waitOutput("traceapp", 'GOT:"상"');
    await sleep(300);
    await tp.getByRole("button", { name: "목록으로" }).click();
    await openSettings(tp, "터미널");
    await dialog.getByTestId("trace-send").click();
    await until("보냄 표시", async () => (await dialog.getByTestId("trace-msg").innerText()).startsWith("보냈다: input-"));
    const dir = `${root}/s/host/diag`;
    const files = fs.readdirSync(dir).filter((n) => /^input-\d+\.json$/.test(n));
    if (files.length !== 1) throw new Error(`diag 파일 ${files}`);
    const rec = JSON.parse(fs.readFileSync(path.join(dir, files[0]), "utf8"));
    const types = new Set(rec.events.map((e) => e.type));
    for (const t of ["keydown", "compositionstart", "compositionupdate", "compositionend", "input", "focus", "send"])
      if (!types.has(t)) throw new Error(`기록에 ${t}가 없다: ${[...types]}`);
    const sent = rec.events.filter((e) => e.type === "send").map((e) => e.data).join("");
    if (sent !== "상태") throw new Error(`기록의 보낸 글 ${JSON.stringify(sent)}`);
    const upd = rec.events.find((e) => e.type === "compositionupdate" && e.data === "사");
    if (!upd || typeof upd.value !== "string" || !Array.isArray(upd.sel) || typeof upd.kb !== "boolean") throw new Error(`상태가 빠졌다 ${JSON.stringify(upd)}`);
    // 보내면 비우고, 끄면 모으지 않는다
    await until("보낸 뒤 비움", async () => (await dialog.locator(".settings-row:has([role=switch][aria-label='입력 기록'])").innerText()).includes("0개 모았습니다"));
    await traceSwitch.click();
    if ((await traceSwitch.getAttribute("aria-checked")) !== "false") throw new Error("입력 기록이 꺼지지 않았다");
    await tp.context().close();
    aaa("pane", "kill", "traceapp");
  });

  await step("조합 상자(WebKit iPad·iPhone 크기, 어두운·밝은 테마): 크기는 herdr-web-ui fitView, 색은 그 칸 배경·글자색, 테두리 없음, 세로는 커서 줄에 맞춰 커서가 새지 않음", async () => {
    // 배경 있는 입력줄(Claude 입력줄처럼)과 배경 없는 줄, 커서는 줄 끝에 둔다
    const app = `${root}/compose.sh`;
    // 셋째 줄은 Codex 입력줄처럼 화면 맨 아래 배경 줄의 「› 」 뒤에서 「가」를 조합한다(사용자 캡처 12:05)
    fs.writeFileSync(
      app,
      "printf 'plain 상태 '; read a; printf '\\033[48;5;238m› 상태 \\033[K'; read b; printf '\\033[0m'; " +
        "i=0; while [ $i -lt 80 ]; do echo; i=$((i+1)); done; printf '\\033[48;5;238m› \\033[K'; cat >/dev/null\n",
    );
    const wk = await webkit.launch();
    const bad = [];
    try {
      const devices = [
        ["dark", "ipad", { width: 820, height: 1180 }, 2],
        ["light", "ipad", { width: 820, height: 1180 }, 2],
        ["dark", "ipad-land", { width: 1180, height: 820 }, 2],
        ["dark", "iphone", { width: 390, height: 844 }, 3],
      ];
      for (const [scheme, dev, viewport, deviceScaleFactor] of devices) {
        const name = `compose-${scheme}-${dev}`;
        aaa("tab", "create", "--machine", "labA", "--label", name, "--", "/bin/sh", app);
        const p = (await pairPage(wk, aaa("web", "pair").code, viewport, { colorScheme: scheme, deviceScaleFactor })).page;
        // 커서는 herdr-web-ui처럼 깜빡인다(cursorBlink). 꺼진 순간에 재면 커서색이 투명이라 샘을 볼 수 없다:
        // 깜빡임만 멈춰 커서가 그려진 상태에서 잰다
        await p.addStyleTag({ content: ".xterm .xterm-cursor-blink, .xterm .xterm-cursor-blink * { animation: none !important; }" });
        await p.getByTestId(`pane-${name}`).click();
        await until("조합 화면", async () => (await screenText(p)).includes("plain 상태"));
        await p.locator(".xterm-screen").click();
        for (const [line, ch] of [["plain", "ㅅ"], ["bg", "ㅅ"], ["bottom", "가"]]) {
          if (line === "bg") {
            aaa("pane", "send-keys", name, "enter");
            await until("배경 줄", async () => (await screenText(p)).includes("› 상태"));
          }
          if (line === "bottom") {
            aaa("pane", "send-keys", name, "enter");
            await until("맨 아래 줄", async () => !(await screenText(p)).includes("plain 상태"));
          }
          await sleep(300);
          const ta = p.locator("textarea.xterm-helper-textarea");
          await ta.evaluate(async (el, ch) => {
            el.focus();
            el.dispatchEvent(new CompositionEvent("compositionstart", { data: "", bubbles: true }));
            el.value += ch;
            el.dispatchEvent(new CompositionEvent("compositionupdate", { data: ch, bubbles: true }));
            el.dispatchEvent(new InputEvent("input", { data: ch, inputType: "insertCompositionText", isComposing: true, bubbles: true }));
            await new Promise((r) => setTimeout(r, 100));
          }, ch);
          const info = await p.evaluate(() => {
            const view = document.querySelector(".xterm .composition-view.active");
            if (!view) return null;
            const r = view.getBoundingClientRect();
            const st = getComputedStyle(view);
            const rows = document.querySelector(".xterm-rows");
            // 비교할 칸: 조합 상자 바로 왼쪽 칸(같은 줄의 「상태 」 뒤 빈칸)
            // 자리로 찾는다: 터치 화면은 줄이 터치를 받지 않아(pointer-events: none) elementsFromPoint가 건너뛴다
            const [x, y] = [r.left - 3, r.top + r.height / 2];
            const inside = (e) => {
              const b = e.getBoundingClientRect();
              return b.left <= x && x < b.right && b.top <= y && y < b.bottom;
            };
            const row = [...rows.children].find(inside);
            const left = row && ([...row.children].find(inside) ?? row);
            let bg = "";
            for (let e = left ?? rows; e && e !== document.documentElement; e = e.parentElement) {
              const c = getComputedStyle(e).backgroundColor;
              if (c && c !== "rgba(0, 0, 0, 0)" && c !== "transparent") {
                bg = c;
                break;
              }
            }
            const screen = document.querySelector(".xterm-screen");
            const cur = document.querySelector(".xterm-rows .xterm-cursor");
            const c = cur?.getBoundingClientRect();
            return {
              rect: { x: r.x, y: r.y, width: r.width, height: r.height },
              cursor: c ? { x: c.x, y: c.y, width: c.width, height: c.height, cls: cur.className, bg: getComputedStyle(cur).backgroundColor, outline: getComputedStyle(cur).outlineStyle, shadow: getComputedStyle(cur).boxShadow } : null,
              rowH: parseFloat(screen.style.height) / document.querySelectorAll(".xterm-rows > div").length,
              bg: st.backgroundColor,
              fg: st.color,
              border: st.borderTopWidth,
              outline: st.outlineStyle,
              cellBg: bg,
              cellFg: left ? getComputedStyle(left).color : "",
              screenW: parseFloat(screen.style.width),
            };
          });
          const cols = aaa("pane", "list").panes.find((x) => x.name === name).cols;
          // 조합 상자 둘레(위·아래·옆 3px)에 커서색 픽셀이 새는지 캡처로 센다(사용자 캡처 12:05: 「가」 위 가로줄)
          if (info?.cursor) {
            const m = 3;
            const png = await p.screenshot({ clip: { x: info.rect.x - m, y: info.rect.y - m, width: info.rect.width + 2 * m, height: info.rect.height + 2 * m } });
            info.leak = await p.evaluate(
              async ({ b64, color, m }) => {
                const img = new Image();
                img.src = `data:image/png;base64,${b64}`;
                await img.decode();
                const c = document.createElement("canvas");
                c.width = img.width;
                c.height = img.height;
                const g = c.getContext("2d");
                g.drawImage(img, 0, 0);
                const d = g.getImageData(0, 0, c.width, c.height).data;
                const [cr, cg, cb] = color.match(/\d+/g).map(Number);
                const k = c.width / (c.width / devicePixelRatio);
                const rows = new Map();
                for (let y = 0; y < c.height; y++)
                  for (let x = 0; x < c.width; x++) {
                    const i = (y * c.width + x) * 4;
                    if (Math.abs(d[i] - cr) + Math.abs(d[i + 1] - cg) + Math.abs(d[i + 2] - cb) < 60) rows.set(y, (rows.get(y) ?? 0) + 1);
                  }
                return { dpr: k, size: [c.width, c.height], inner: [Math.round(m * k), Math.round(c.height - m * k)], rows: [...rows] };
              },
              { b64: png.toString("base64"), color: info.cursor.bg, m },
            );
          }
          if (shots && info) {
            await p.screenshot({
              path: path.join(shots, `${name}-${line}.png`),
              clip: { x: Math.max(0, info.rect.x - 160), y: Math.max(0, info.rect.y - 24), width: 260, height: info.rect.height + 48 },
            });
          }
          console.log(`     조합 상자 ${scheme} ${dev} ${line}: ${JSON.stringify(info)}`);
          if (!info) bad.push(`${scheme} ${line}: 조합 상자가 없다`);
          else {
            const cell = info.screenW / cols;
            if (info.bg !== info.cellBg) bad.push(`${scheme} ${line}: 배경 ${info.bg} ≠ 칸 ${info.cellBg}`);
            if (info.fg !== info.cellFg) bad.push(`${scheme} ${line}: 글자 ${info.fg} ≠ 칸 ${info.cellFg}`);
            if (info.border !== "0px" || info.outline !== "none") bad.push(`${scheme} ${line}: 테두리 ${info.border} ${info.outline}`);
            if (Math.abs(info.rect.width - 2 * cell) > 1.5) bad.push(`${scheme} ${line}: 폭 ${info.rect.width} ≠ 두 칸 ${2 * cell}`);
            const at = `${scheme} ${dev} ${line}`;
            if (!info.cursor) bad.push(`${at}: 커서 칸이 없다`);
            else if (Math.abs(info.rect.y - info.cursor.y) > 0.02 || Math.abs(info.rect.height - info.cursor.height) > 0.02)
              bad.push(`${at}: 상자 세로 ${info.rect.y}+${info.rect.height} ≠ 커서 줄 ${info.cursor.y}+${info.cursor.height}`);
            if (info.leak?.rows.length) bad.push(`${at}: 상자 둘레에 커서색이 샌다(device 줄, 픽셀 수) ${JSON.stringify(info.leak.rows)}`);
          }
          // 조합을 끝내 지운다(보내지 않는다: 값을 비운 뒤 끝낸다)
          await ta.evaluate((el) => {
            el.value = el.value.slice(0, -1);
            el.dispatchEvent(new CompositionEvent("compositionend", { data: "", bubbles: true }));
          });
          await sleep(150);
        }
        await p.context().close();
        aaa("pane", "kill", name);
      }
    } finally {
      await wk.close();
    }
    if (bad.length) throw new Error(bad.join("; "));
  });

  await step("화면 키보드(WebKit iPad·iPhone 크기, visualViewport를 줄여 흉내): 커서 줄·입력 막대·키 막대가 키보드 위에 보이고, PTY 크기는 그대로", async () => {
    // herdr-web-ui scripts/keyboard-viewport-regression.ts처럼 플랫폼 입력(visualViewport·innerHeight·100lvh 자)만 가짜로 둔다.
    // 사용자 캡처(12:26): iPad 화면 키보드가 터미널 아래쪽(프롬프트 줄·커서·입력 막대·키 막대)을 가렸다.
    const init = fakeViewport;
    const app = `${root}/keyboard.sh`;
    fs.writeFileSync(app, "i=0; while [ $i -lt 200 ]; do echo kb-line-$i; i=$((i+1)); done; printf '› '; cat >/dev/null\n");
    const wk = await webkit.launch();
    const bad = [];
    try {
      for (const [dev, viewport, kb] of [["ipad", { width: 820, height: 1180 }, 400], ["iphone", { width: 390, height: 844 }, 336]]) {
        const name = `kb-${dev}`;
        aaa("tab", "create", "--machine", "labA", "--label", name, "--", "/bin/sh", app);
        const p = (await pairPage(wk, aaa("web", "pair").code, viewport, { touch: true, init })).page;
        const direct = p.locator('.key-bar .key[data-key="direct"]');
        await p.getByTestId(`pane-${name}`).click();
        await until("키보드 화면", async () => (await screenText(p)).includes("kb-line-199"));
        await sleep(500);
        let before = aaa("pane", "list").panes.find((x) => x.name === name);
        for (const field of ["terminal", "input-bar"]) {
          // 터미널은 바로 입력(기본)에서, 입력줄은 ⌨로 입력줄을 켠 뒤 본다
          if (field === "terminal") await p.locator(".xterm-screen").tap();
          else {
            // 입력줄 칸(한 줄)이 흐름에 들어와 그리드가 한 번 줄어든다(참고와 같다). 키보드가 바꾸는지는 그 뒤부터 본다.
            await direct.tap();
            await p.locator(".terminal-input").waitFor();
            await sleep(800);
            before = aaa("pane", "list").panes.find((x) => x.name === name);
            await p.getByLabel("보낼 글").focus();
          }
          // 화면 키보드가 올라온다: 보이는 높이만 줄고(iOS Safari) 레이아웃은 그대로다
          await softKeyboard(p, viewport.height - kb);
          await sleep(600);
          const seen = await p.evaluate(() => {
            const bottom = (sel) => document.querySelector(sel)?.getBoundingClientRect().bottom ?? null;
            const cur = document.querySelector(".xterm-rows .xterm-cursor")?.getBoundingClientRect();
            const area = document.querySelector(".term-scroll")?.getBoundingClientRect();
            return {
              keyboard: document.documentElement.hasAttribute("data-keyboard"),
              area: area ? { top: area.top, bottom: area.bottom } : null,
              visible: window.visualViewport.height,
              app: document.querySelector(".app")?.getBoundingClientRect().height,
              cursor: cur ? { top: cur.top, bottom: cur.bottom } : null,
              input: bottom(".terminal-input-text"),
              keys: bottom(".key-bar"),
              scroll: window.scrollY,
            };
          });
          const rows = aaa("pane", "list").panes.find((x) => x.name === name);
          console.log(`     화면 키보드 ${dev} ${field}: ${JSON.stringify(seen)} PTY ${before.cols}x${before.rows} → ${rows.cols}x${rows.rows}`);
          if (shots) await p.screenshot({ path: path.join(shots, `keyboard-${dev}-${field}.png`) });
          const at = `${dev} ${field}`;
          const vis = seen.visible;
          if (!seen.cursor || seen.cursor.top < 0 || seen.cursor.bottom > vis + 0.5) bad.push(`${at}: 커서 줄이 키보드에 가렸다 ${JSON.stringify(seen.cursor)} > ${vis}`);
          // 터미널 영역(입력 막대 위) 안에 보여야 한다: 영역 밖이면 잘려 보이지 않는다
          else if (!seen.area || seen.cursor.top < seen.area.top - 0.5 || seen.cursor.bottom > seen.area.bottom + 0.5)
            bad.push(`${at}: 커서 줄이 터미널 영역 밖이다 ${JSON.stringify(seen.cursor)} ∉ ${JSON.stringify(seen.area)}`);
          if (field === "input-bar" && (seen.input === null || seen.input > vis + 0.5)) bad.push(`${at}: 입력줄 아래 ${seen.input} > ${vis}`);
          if (field === "terminal" && seen.input !== null) bad.push(`${at}: 바로 입력인데 입력줄이 있다`);
          if (seen.keys === null || seen.keys > vis + 0.5) bad.push(`${at}: 키 막대 아래 ${seen.keys} > ${vis}`);
          if (rows.cols !== before.cols || rows.rows !== before.rows) bad.push(`${at}: 키보드에 PTY 크기가 바뀌었다 ${before.cols}x${before.rows} → ${rows.cols}x${rows.rows}`);
          // 키보드가 내려간다
          await softKeyboard(p, viewport.height);
          await p.evaluate(() => document.activeElement?.blur());
          await sleep(400);
        }
        await p.context().close();
        aaa("pane", "kill", name);
      }
    } finally {
      await wk.close();
    }
    if (bad.length) throw new Error(bad.join("; "));
  });

  await step("대화 기록(읽기 전용): Claude 기록에서 사람·답·도구 줄만, 생각·도구 결과는 빼고, 새 줄을 따라가고, 기록 폴더 밖은 거부", async () => {
    const dir = `${root}/claude/projects/-lab-chat`;
    fs.mkdirSync(dir, { recursive: true });
    const tr = `${dir}/s1.jsonl`;
    const L = (o) => JSON.stringify(o) + "\n";
    fs.writeFileSync(
      tr,
      L({ type: "user", message: { role: "user", content: "결제 지연을 줄여 줘" } }) +
        L({ type: "user", message: { role: "user", content: "<system-reminder>끼운 글</system-reminder>" } }) +
        L({
          type: "assistant",
          message: {
            content: [
              { type: "thinking", thinking: "속생각" },
              { type: "text", text: "캐시를 넣었다" },
              { type: "tool_use", name: "Bash", input: { command: "cargo test" } },
            ],
          },
        }) +
        L({ type: "user", message: { content: [{ type: "tool_result", content: "도구결과본문" }] } }),
    );
    const hookPane = (name, path) =>
      aaa("tab", "create", "--machine", "labA", "--label", name, "--", "/bin/sh", "-c", '"$0" hook --agent claude "$1"; exec cat', AA,
        JSON.stringify({ hook_event_name: "Stop", session_id: "s1", transcript_path: path }));
    hookPane("chat1", tr);
    await page.getByTestId("pane-chat1").click();
    await until("대화 단추", async () => (await page.getByTestId("chat-toggle").count()) === 1);
    await page.getByTestId("chat-toggle").click();
    const chat = page.getByTestId("chat");
    const roles = async () => chat.locator(".chat-msg").evaluateAll((els) => els.map((e) => [e.dataset.role, e.textContent]));
    await until("대화 줄", async () => (await roles()).length === 3);
    const want = [["user", "결제 지연을 줄여 줘"], ["assistant", "캐시를 넣었다"], ["tool", "Bash: cargo test"]];
    if (JSON.stringify(await roles()) !== JSON.stringify(want)) throw new Error(JSON.stringify(await roles()));
    const all = await chat.innerText();
    if (all.includes("속생각") || all.includes("도구결과본문") || all.includes("끼운 글")) throw new Error(`빼야 할 글이 보인다: ${all}`);
    // 새 줄이 생기면 따라간다
    fs.appendFileSync(tr, L({ type: "assistant", message: { content: [{ type: "text", text: "시험도 통과했다" }] } }));
    await until("새 줄", async () => (await roles()).length === 4);
    await shot(page, "desktop-chat");
    // 터미널로 돌아간다
    await page.getByTestId("chat-toggle").click();
    if (await page.getByTestId("terminal").isHidden()) throw new Error("터미널로 돌아가지 않았다");
    // CLI도 같은 기록을 본다
    const c = aaa("pane", "chat", "chat1");
    if (c.messages.length !== 4 || c.harness !== "claude") throw new Error(JSON.stringify(c));
    // 기록 폴더 밖을 가리키는 hook(세션 안 누구나 보낼 수 있다)은 읽지 않는다
    fs.writeFileSync(`${root}/secret.jsonl`, L({ type: "user", message: { role: "user", content: "비밀" } }));
    hookPane("chat2", `${root}/secret.jsonl`);
    await until("chat2 harness", () => aaa("pane", "list").panes.find((p) => p.name === "chat2")?.agent?.harness === "claude");
    let refused = "";
    try {
      aaa("pane", "chat", "chat2");
    } catch (e) {
      // 오류 JSON은 stderr로 온다(herdr와 같다)
      refused = String(e.stderr);
    }
    if (!refused.includes("기록 폴더 밖") || refused.includes("비밀")) throw new Error(`밖의 파일: ${refused}`);
    for (const n of ["chat1", "chat2"]) aaa("pane", "kill", n);
    await page.getByTestId("pane-t1").click();
    await until("t1 터미널", async () => (await screenText(page)).includes("한글-8"));
  });

  await step("그림 붙여 넣기·끌어 놓기(herdr-web-ui): 셸·harness 입력창에 따옴표 경로(Enter 없음), 글이 있으면 글, 여러 파일은 놓은 순서, 글 놓기, 확인창·형식·reader 거부", async () => {
    const png = fs.readFileSync(`${root}/pic.png`);
    const banner = page.getByTestId("attach-banner");
    // 1) 일반 셸(t1): 대상 머신 세션 폴더에 저장하고 셸에도 따옴표 경로와 빈칸을 넣는다(결정 20 D7, Enter 없음)
    await page.getByTestId("pane-t1").click();
    await until("t1 터미널", async () => (await screenText(page)).includes("$"));
    await pasteFile(page, png, "image/png");
    await until("셸 경로 배너", async () => (await banner.innerText()).includes("경로를 넣었다"));
    const p1 = bannerPath(await banner.innerText());
    if (!p1?.startsWith(`${root}/s/agent/sessions/`)) throw new Error(`저장 경로: ${await banner.innerText()}`);
    if (!fs.readFileSync(p1).equals(png)) throw new Error("저장한 그림이 다르다");
    if ((fs.statSync(p1).mode & 0o777) !== 0o600) throw new Error(`권한 ${(fs.statSync(p1).mode & 0o777).toString(8)}`);
    // 긴 경로는 화면에서 줄이 접힌다: 접힌 줄을 이어 본다(herdr recent-unwrapped)
    await waitOutput("t1", `'${p1}'`, "recent-unwrapped");
    // Enter가 없었다: 셸은 실행하지 않았다(그 줄을 지운다)
    await sleep(300);
    if (aaa("pane", "read", "t1", "--source", "visible").text.replaceAll("\n", "").includes("No such file")) throw new Error("셸이 경로를 실행했다");
    aaa("pane", "send-text", "t1", "\u0015");
    // 2) 형식이 다른 파일(SVG)은 host가 signature로 거부한다
    await pasteFile(page, Buffer.from("<svg xmlns='http://www.w3.org/2000/svg'/>"), "image/svg+xml", "x.svg");
    await until("형식 거부", async () => (await page.locator(".banner.err").innerText()).includes("PNG·JPEG·GIF·WebP"));

    // 3) harness pane(hook 보냄, 입력 대기): 여러 조각(1.2MB) 그림의 경로를 bracketed paste로 넣고 Enter는 보내지 않는다
    const big = Buffer.concat([png.subarray(0, 8), Buffer.alloc(1_200_000, 7)]);
    const recv = `${root}/h1.in`;
    aaa("tab", "create", "--machine", "labA", "--label", "h1", "--", "/bin/sh", "-c",
      '"$0" hook --agent claude \'{"hook_event_name":"Stop"}\'; stty raw -echo; printf "\\342\\235\\257 \\n"; exec cat > "$1"', AA, recv);
    await until("h1 harness 상태", async () => (await page.getByTestId("agent-h1").count()) === 1);
    await page.getByTestId("pane-h1").click();
    await until("h1 터미널", async () => (await screenText(page)).includes("❯"));
    await pasteFile(page, big, "image/png");
    await until("입력창 배너", async () => (await banner.innerText()).includes("경로를 넣었다"));
    const p2 = bannerPath(await banner.innerText());
    if (!fs.readFileSync(p2).equals(big)) throw new Error("여러 조각 그림이 다르다");
    await until("PTY가 받은 경로", () => fs.readFileSync(recv, "latin1") === `\x1b[200~'${p2}' \x1b[201~`);
    await shot(page, "desktop-attach");
    // 받은 것 가운데 이 뒤에 온 것
    let seen = fs.readFileSync(recv, "latin1").length;
    const recvNext = () => fs.readFileSync(recv, "latin1").slice(seen);
    // 3a) 클립보드에 글과 그림이 같이 있으면 글이다(복사한 경로도 글로 남는다)
    await page.evaluate(
      ({ b64 }) => {
        const dt = new DataTransfer();
        dt.setData("text/plain", "mixed-text");
        dt.items.add(new File([Uint8Array.from(atob(b64), (c) => c.charCodeAt(0))], "m.png", { type: "image/png" }));
        document.querySelector('[data-testid="terminal"] textarea').dispatchEvent(new ClipboardEvent("paste", { clipboardData: dt, bubbles: true, cancelable: true }));
      },
      { b64: png.toString("base64") },
    );
    await until("글만 붙여 넣음", () => recvNext() === "mixed-text");
    await sleep(300);
    if (recvNext() !== "mixed-text") throw new Error(`글과 그림: ${JSON.stringify(recvNext())}`);
    seen = fs.readFileSync(recv, "latin1").length;
    // 3b) 파일 여러 개를 끌어 놓으면 놓은 순서대로 올리고 경로를 차례로 넣는다
    const pa = Buffer.concat([png.subarray(0, 8), Buffer.alloc(300_000, 1)]);
    const pb = Buffer.concat([png.subarray(0, 8), Buffer.alloc(2_000, 2)]);
    const drop = (files, text) =>
      page.evaluate(
        ({ files, text }) => {
          const dt = new DataTransfer();
          for (const f of files) dt.items.add(new File([Uint8Array.from(atob(f.b64), (c) => c.charCodeAt(0))], f.name, { type: "image/png" }));
          if (text) dt.setData("text/plain", text);
          const at = document.querySelector('[data-testid="terminal"] .xterm-screen');
          const over = new DragEvent("dragover", { dataTransfer: dt, bubbles: true, cancelable: true });
          at.dispatchEvent(over);
          at.dispatchEvent(new DragEvent("drop", { dataTransfer: dt, bubbles: true, cancelable: true }));
          // dropEffect는 볼 수 없다: 지어낸 DataTransfer는 끌기 중이 아니라 dropEffect 대입을 버린다(Chromium에서 확인)
          return { prevented: over.defaultPrevented };
        },
        { files: files.map(([name, b]) => ({ name, b64: b.toString("base64") })), text },
      );
    const over = await drop([["a.png", pa], ["b.png", pb]]);
    if (!over.prevented) throw new Error(`끌어 오기를 받지 않았다 ${JSON.stringify(over)}`);
    const two = await until("두 경로", () => {
      const m = /^\x1b\[200~'([^']+)' \x1b\[201~\x1b\[200~'([^']+)' \x1b\[201~$/.exec(recvNext());
      return m ? [m[1], m[2]] : null;
    });
    if (!fs.readFileSync(two[0]).equals(pa) || !fs.readFileSync(two[1]).equals(pb)) throw new Error(`놓은 순서가 아니다: ${two}`);
    seen = fs.readFileSync(recv, "latin1").length;
    // 3c) 글을 끌어 놓으면 붙여 넣는다(Enter 없음)
    await drop([], "dropped-text");
    await until("놓은 글", () => recvNext() === "dropped-text");

    // 4) 확인창이 떠 있는 harness pane에는 넣지 않는다
    const recv2 = `${root}/h2.in`;
    aaa("tab", "create", "--machine", "labA", "--label", "h2", "--", "/bin/sh", "-c",
      'printf "Allow Bash(rm x)?\\n\\342\\235\\257 1. Yes\\n  2. No\\nEsc to cancel\\n"; ' +
        '"$0" hook --agent claude \'{"hook_event_name":"PermissionRequest","tool_name":"Bash","tool_input":{"command":"rm x"}}\'; ' +
        'stty raw -echo; exec cat > "$1"', AA, recv2);
    await page.getByTestId("agent-h2").and(page.locator('[data-status="blocked"]')).waitFor();
    await page.getByTestId("pane-h2").click();
    await until("h2 터미널", async () => (await screenText(page)).includes("Esc to cancel"));
    await pasteFile(page, png, "image/png");
    await until("확인창 배너", async () => (await banner.innerText()).includes("확인창이 떠 있어"));
    await sleep(300);
    if (fs.readFileSync(recv2, "latin1") !== "") throw new Error(`확인창에 입력이 갔다: ${fs.readFileSync(recv2, "latin1")}`);

    // 5) reader 연결은 화면 밖에서 직접 보내도 host가 거부한다
    const rd = await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 390, height: 844 });
    const h1id = aaa("pane", "list").panes.find((p) => p.name === "h1").id;
    const got = await rd.page.evaluate(
      async ({ id, b64 }) => {
        const ws = new WebSocket(`ws://${location.host}/api/v1/panes/${id}/ws`);
        return await new Promise((resolve) => {
          ws.onopen = () => ws.send(JSON.stringify({ t: "attach", offset: 0, total: 70, sha256: "0".repeat(64), data: b64 }));
          ws.onmessage = (m) => {
            if (typeof m.data !== "string") return;
            const msg = JSON.parse(m.data);
            if (msg.t === "error" || msg.t === "attached") resolve(msg);
          };
          setTimeout(() => resolve({ t: "timeout" }), 5000);
        });
      },
      { id: h1id, b64: png.toString("base64") },
    );
    if (got.t !== "error" || got.code !== "forbidden") throw new Error(`reader 첨부: ${JSON.stringify(got)}`);
    await rd.page.context().close();
    for (const n of ["h1", "h2"]) aaa("pane", "kill", n);
  });

  const pushDev = pushDevice("push/dev1");
  const goneDev = pushDevice("gone/dev2");
  await step("Web Push: 구독 검사 → 시험 알림이 암호화·서명돼 온다 → service worker가 알림을 띄운다", async () => {
    const api = (path, body) =>
      page.evaluate(
        async ({ path, body }) => {
          const r = await fetch(path, body ? { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) } : {});
          return { status: r.status, json: await r.json() };
        },
        { path, body },
      );
    const info = await api("/api/v1/push");
    if (!info.json.ok?.enabled || info.json.ok.subscribed !== 0) throw new Error(JSON.stringify(info.json));
    pushKey = info.json.ok.key;
    if (Buffer.from(pushKey, "base64url").length !== 65) throw new Error(`VAPID 공개키 ${pushKey}`);
    // 허용하지 않은 곳·loopback이라도 목록에 없는 이름·틀린 키는 받지 않는다
    for (const bad of [
      { ...pushDev.json, endpoint: "https://evil.example/push" },
      { ...pushDev.json, endpoint: `http://localhost:${pushPort}/x` },
      { ...pushDev.json, keys: { ...pushDev.json.keys, p256dh: b64u(Buffer.alloc(65, 1)).slice(0, 40) } },
    ]) {
      const r = await api("/api/v1/push/subscribe", bad);
      if (r.status !== 400) throw new Error(`거부하지 않았다: ${JSON.stringify(bad)} → ${r.status}`);
    }
    if ((await api("/api/v1/push/subscribe", pushDev.json)).status !== 200) throw new Error("구독 실패");
    const from = pushes.length;
    const t = await api("/api/v1/push/test", {});
    if (t.json.ok?.sent !== 1) throw new Error(JSON.stringify(t.json));
    const { msg, claims } = await nextPush(pushDev, from, "시험 알림");
    if (msg.title !== "aaa" || msg.body !== "알림이 켜졌다") throw new Error(JSON.stringify(msg));
    const now = Date.now() / 1000;
    if (claims.aud !== pushOrigin || claims.exp < now || claims.exp > now + 24 * 3600 || !/^(mailto:|https:)/.test(claims.sub)) {
      throw new Error(`claims ${JSON.stringify(claims)}`);
    }
    // 끄면 지워진다. 보고 알림을 받을 기기(dev1)와 브라우저가 버린 구독(dev2, 410)을 둔다.
    if (!(await api("/api/v1/push/unsubscribe", { endpoint: pushDev.json.endpoint })).json.ok?.removed) throw new Error("구독 해제");
    if ((await api("/api/v1/push")).json.ok.subscribed !== 0) throw new Error("해제 뒤에도 구독이 남았다");
    for (const d of [pushDev, goneDev]) {
      if ((await api("/api/v1/push/subscribe", d.json)).status !== 200) throw new Error("다시 구독 실패");
    }
    // service worker: push를 받으면 알림을 띄운다(push 서비스 없이 CDP로 직접 넣는다).
    // headless shell은 알림을 지원하지 않아 이 부분만 정식 Chromium(새 headless)으로 한다.
    const full = await chromium.launch({ channel: "chromium" });
    try {
      // 이 기기 알림은 꺼 둔다: 권한이 있으면 열 때마다 구독을 다시 등록하는데(참고), 그러면 정식 Chromium이 실제 push 서비스에 닿는다
      const mob = await pairPage(full, aaa("web", "pair", "--reader").code, { width: 390, height: 844 }, {
        init: () => {
          try {
            localStorage.setItem("aaa:alerts-on", "0");
          } catch {}
        },
      });
      const ctx = mob.page.context();
      await ctx.grantPermissions(["notifications"], { origin: base.slice(0, -1) });
      await mob.page.reload();
      await mob.page.getByTestId("machine-labA").waitFor();
      // 알림 켜기는 머리 「더 보기」 메뉴의 「알림」이다(참고 App.tsx moreItems). 보기 권한 기기도 알림은 받는다. 꺼져 있으면 메뉴 단추에 점
      await mob.page.getByTestId("more").click();
      await until("알림 항목", async () => (await mob.page.getByTestId("alerts-item").innerText()).includes("이 기기에서 꺼짐"));
      if ((await mob.page.getByTestId("more").getAttribute("aria-label")) !== "더 보기 · 알림 꺼짐") throw new Error("꺼진 알림 표시가 없다");
      await mob.page.keyboard.press("Escape");
      const cdp = await ctx.newCDPSession(mob.page);
      const regId = new Promise((r) =>
        cdp.on("ServiceWorker.workerRegistrationUpdated", (e) => {
          const reg = e.registrations.find((x) => x.scopeURL === base && !x.isDeleted);
          if (reg) r(reg.registrationId);
        }),
      );
      // 실패하면 service worker 판 상태를 남긴다(10-17 22:20 전체 실행에서 한 번 알림이 뜨지 않았고 따로는 재현되지 않았다)
      const versions = [];
      cdp.on("ServiceWorker.workerVersionUpdated", (e) => versions.push(...e.versions.map((v) => `${v.versionId}:${v.status}/${v.runningStatus}`)));
      cdp.on("ServiceWorker.workerErrorReported", (e) => versions.push(`error:${e.errorMessage.errorMessage}`));
      await cdp.send("ServiceWorker.enable");
      const data = JSON.stringify({ title: "aaa · demo", body: "t-0009 question: 확인창", url: "/#thread=demo%2Ft-0009", tag: "aaa-demo" });
      // 활성화된 worker에만 보낸다: 설치·활성화 중에 넣은 push는 사건 없이 버려질 수 있다(10-17 두 번 난 「알림 표시」 실패의 후보)
      await until("service worker 활성", () => mob.page.evaluate(async () => (await navigator.serviceWorker.getRegistration())?.active?.state === "activated"));
      const atDeliver = versions.slice(-4).join(",");
      await cdp.send("ServiceWorker.deliverPushMessage", { origin: base.slice(0, -1), registrationId: await regId, data });
      const shown = await until("알림 표시", async () => {
        const v = await mob.page.evaluate(async () =>
          (await (await navigator.serviceWorker.ready).getNotifications()).map((n) => [n.title, n.body, n.data?.url]),
        );
        return v.length ? v : null;
      }).catch(async (e) => {
        const state = await mob.page.evaluate(async () => {
          const reg = await navigator.serviceWorker.getRegistration();
          return {
            permission: Notification.permission,
            controller: !!navigator.serviceWorker.controller,
            active: reg?.active?.state ?? null,
            waiting: reg?.waiting?.state ?? null,
            installing: reg?.installing?.state ?? null,
            shown: reg ? (await reg.getNotifications()).length : null,
          };
        });
        throw new Error(`${e.message}: ${JSON.stringify(state)} 판 ${versions.slice(-8).join(",")} 보낼 때 판 ${atDeliver} 오류 ${mob.errors.join("; ")}`);
      });
      if (JSON.stringify(shown[0]) !== JSON.stringify(["aaa · demo", "t-0009 question: 확인창", "/#thread=demo%2Ft-0009"])) {
        throw new Error(JSON.stringify(shown));
      }
      if (mob.errors.length) throw new Error(`페이지 오류: ${mob.errors.join("; ")}`);
    } finally {
      await full.close();
    }
    await page.getByTestId("pane-t1").click();
    await until("t1 터미널", async () => (await screenText(page)).includes("한글-8"));
  });

  await step("service worker 알림(herdr-web-ui sw.js·pwa-notification.test.ts 사례, 차이 목록 N5·N7): 보이는 창이 있으면 소리 없이, 누르면 focus 전에 자리를 알리고, focus가 안 되면 새 창, 마지막 누름만, 파일 탭은 고르지 않음", async () => {
    // 내보내는 sw.js를 그대로 돌린다(가짜 self). 참고 시험과 같은 방법이다.
    const src = fs.readFileSync(path.join(repo, "web/public/sw.js"), "utf8");
    const APP = "https://aaa.example/";
    const deferred = () => {
      let resolve, reject;
      const promise = new Promise((y, n) => ((resolve = y), (reject = n)));
      return { promise, resolve, reject };
    };
    const worker = (windows, matchAll = async () => windows, openWindow = async () => null) => {
      const on = new Map();
      const opened = [];
      const shown = [];
      const self = {
        location: new URL(APP),
        addEventListener: (type, f) => on.set(type, f),
        clients: { matchAll, openWindow: async (u) => (opened.push(u), openWindow(u)), claim: async () => {} },
        registration: { showNotification: async (title, options) => void shown.push({ title, options }) },
        skipWaiting: async () => {},
      };
      const { isAppWindow } = new Function("self", "caches", `${src}\nreturn { isAppWindow };`)(self, {});
      const run = (type, ev) => {
        let done;
        on.get(type)({ ...ev, waitUntil: (p) => (done = p) });
        return done;
      };
      const push = (payload) => run("push", { data: { json: () => payload, text: () => JSON.stringify(payload) } });
      let closed = 0;
      const click = (url = "/#thread=demo%2Ft-0001") => run("notificationclick", { notification: { data: { url }, close: () => closed++ } });
      return { isAppWindow, push, click, opened, shown, closed: () => closed };
    };
    const client = (focus = async () => {}, focused = false, url = APP, visibilityState = "visible") => {
      const got = [];
      return { url, focused, focus, visibilityState, got, postMessage: (m) => got.push(m.hash) };
    };
    const eq = (what, a, b) => {
      if (JSON.stringify(a) !== JSON.stringify(b)) throw new Error(`${what}: ${JSON.stringify(a)} ≠ ${JSON.stringify(b)}`);
    };
    const A = "#thread=demo%2Ft-0001";
    const B = "#thread=demo%2Ft-0002";
    // 앱 창: host API가 연 파일·결과물 탭과 보내는 파일은 아니다
    const { isAppWindow } = worker([]);
    eq(
      "앱 창",
      [APP, `${APP}#pane=p1`, `${APP}thread/t-0001`, `${APP}index.html`, `${APP}api/v1/push`, `${APP}v/cap1/index.html`, `${APP}v/cap1/sub/`, `${APP}icon-192.png`, `${APP}manifest.webmanifest`, `${APP}sw.js`, `${APP}offline.html`, "not a url"].map(isAppWindow),
      [true, true, true, true, false, false, false, false, false, false, false, false],
    );
    // 보이는 창이 있으면 소리·진동 없이, 없으면 다시 울린다. 누를 때 쓸 주소를 단다
    const seen = worker([client()]);
    await seen.push({ title: "demo · 웹", body: "b", url: `/${A}`, tag: "aaa-demo/t-0001" });
    const hidden = worker([client(undefined, false, APP, "hidden")]);
    await hidden.push({ title: "t", body: "b" });
    eq("보는 중 알림", [seen.shown[0].title, seen.shown[0].options.silent, seen.shown[0].options.renotify, seen.shown[0].options.tag, seen.shown[0].options.data.url], ["demo · 웹", true, false, "aaa-demo/t-0001", `/${A}`]);
    eq("안 보는 중 알림", [hidden.shown[0].options.silent, hidden.shown[0].options.renotify, hidden.shown[0].options.tag, hidden.shown[0].options.data.url], [false, true, "aaa", "/"]);
    // focus가 늦어도 먼저 알리고, 끝나면 다시 알린다. 새 창은 열지 않는다
    {
      const d = deferred();
      const t = client(() => d.promise);
      const w = worker([t]);
      const done = w.click();
      await Promise.resolve();
      eq("focus 전에 알림", t.got, [A]);
      d.resolve();
      await done;
      eq("focus 뒤 다시", [t.got, w.opened, w.closed()], [[A, A], [], 1]);
    }
    // focus를 거절하면 그 자리로 새 창을 한 번 연다
    {
      const t = client(async () => {
        throw new Error("NotAllowedError");
      });
      const w = worker([t]);
      await w.click();
      eq("거절 뒤 새 창", [t.got, w.opened], [[A], [`/${A}`]]);
    }
    // focus된 창을 고른다. 창이 없으면 그 자리로 열고, 자리가 없는 알림은 맨 앞으로
    {
      const bg = client();
      const fg = client(undefined, true);
      const w = worker([bg, fg]);
      await w.click();
      eq("focus된 창", [bg.got.length, fg.got.length, w.opened], [0, 2, []]);
      const cold = worker([]);
      await cold.click();
      await cold.click("/");
      eq("창 없음", cold.opened, [`/${A}`, "/"]);
      let focused = 0;
      const g = client(async () => void focused++);
      const gw = worker([g]);
      await gw.click("/");
      eq("자리 없는 알림", [focused, g.got, gw.opened], [1, [], []]);
    }
    // 파일 탭(결과물)은 고르지 않는다: 앱 창이 받고, 앱 창이 없으면 연다
    {
      const file = client(undefined, true, `${APP}v/cap1/index.html`);
      const app = client();
      const w = worker([file, app]);
      await w.click();
      eq("파일 탭", [file.got, app.got.length, w.opened], [[], 2, []]);
      let focused = 0;
      const lone = client(async () => void focused++, true, `${APP}icon-192.png`);
      const cold = worker([lone]);
      await cold.click(`/${B}`);
      eq("파일 탭만", [focused, lone.got, cold.opened], [0, [], [`/${B}`]]);
    }
    // 늦게 끝난 앞 누름의 focus가 새 누름 자리를 바꾸지 못한다
    {
      const d = deferred();
      let n = 0;
      const t = client(() => (++n === 1 ? d.promise : Promise.resolve()));
      const w = worker([t]);
      const first = w.click(`/${A}`);
      await Promise.resolve();
      await w.click(`/${B}`);
      d.resolve();
      await first;
      eq("늦은 focus", [t.got, w.opened], [[A, B, B, B], []]);
    }
    // 밀려난 앞 누름의 focus 거절이 옛 자리로 새 창을 열지 못한다
    {
      const d = deferred();
      let n = 0;
      const t = client(() => (++n === 1 ? d.promise : Promise.resolve()));
      const w = worker([t]);
      const first = w.click(`/${A}`);
      await Promise.resolve();
      await w.click(`/${B}`);
      d.reject(new Error("NotAllowedError"));
      await first;
      eq("밀려난 거절", [t.got, w.opened], [[A, B, B], []]);
    }
    // 창 찾기가 느린 앞 누름은 새 누름 뒤에 옛 자리를 알리지 못한다
    {
      const d = deferred();
      let n = 0;
      const t = client();
      const w = worker([t], async () => {
        if (++n === 1) await d.promise;
        return [t];
      });
      const first = w.click(`/${A}`);
      await w.click(`/${B}`);
      d.resolve();
      await first;
      eq("느린 창 찾기", [t.got, w.opened], [[B, B], []]);
    }
    // 늦게 열린 새 창(거절 뒤·처음 열기)도 마지막 누름 자리를 받는다
    for (const fallback of [false, true]) {
      const opening = deferred();
      const ready = deferred();
      const refused = client(async () => {
        throw new Error("NotAllowedError");
      });
      const newer = client(undefined, true);
      const opened = client();
      let n = 0;
      const w = worker(
        [],
        async () => (++n > 1 ? [newer] : fallback ? [refused] : []),
        async () => {
          opening.resolve();
          await ready.promise;
          return opened;
        },
      );
      const first = w.click(`/${A}`);
      await opening.promise;
      await w.click(`/${B}`);
      ready.resolve();
      await first;
      eq(`늦게 열린 창(${fallback ? "거절 뒤" : "처음"})`, [opened.got, newer.got, w.opened], [[B], [B, B], [`/${A}`]]);
    }
  });

  const pushFrom = pushes.length;
  // 쓰는 화면이 있으면 host는 Web Push를 보내지 않는다(herdr-web-ui presence, plan §8.69). headless 화면은 늘 focus가
  // 있으므로, 보고 알림이 기기에 가는 것을 볼 동안 이 화면은 쓰지 않는 것으로 둔다(focus만 없앤다)
  const setInUse = (p, on) =>
    p.evaluate((on) => {
      if (on) delete document.hasFocus;
      else document.hasFocus = () => false;
      window.dispatchEvent(new Event(on ? "focus" : "blur"));
    }, on);
  const pushInUse = async () => (await page.evaluate(async () => (await (await fetch("/api/v1/push")).json()).ok.in_use));
  await step("thread 보고서·결과물: Markdown 보고, sandbox 결과물, agent에 남은 큰 파일, ack", async () => {
    await setInUse(page, false);
    await until("쓰는 화면 없음", async () => (await pushInUse()) === 0);
    aaa("project", "create", "demo");
    fs.writeFileSync(`${root}/task.md`, "웹 보기");
    const t = aaa("thread", "start", "demo", "--title", "웹 결과물", "--machine", "labA", "--profile", "fake", "--kind", "dir",
      "--task-file", `${root}/task.md`);
    await until("보고 회수", () => aaa("thread", "show", "demo", t.id).report_hash);
    const item = page.getByTestId(`thread-demo-${t.id}`);
    await until("thread 목록에 검토 대기", async () => (await item.innerText()).includes("검토 대기"));
    // thread 줄 표시는 그 thread pane의 agent(참고 Sidebar.tsx rowMark): 표시가 없는 profile(fake)은 머리글자 표시
    const tm = await item.locator(".sidebar-mark .agent-mark").evaluate((a) => `${a.className}|${a.textContent}`);
    if (tm !== "agent-mark fake|F") throw new Error(`thread 줄 표시 ${tm}`);
    // thread 줄은 그 agent pane을 열고(차이 목록 L4, herdr-projects: thread는 자기 agent 줄) 보고서는 터미널 머리 「보고」로
    await item.click();
    await page.getByTestId("open-report").click();
    const report = page.getByTestId("report");
    await until("보고 Markdown", async () => (await report.locator("strong").first().innerText()) === "확인");
    // herdr-web-ui Markdown대로 그린다(차이 목록 R1): 표·굵게 안 code·링크·번호 목록 안 목록·code block
    const md = await report.locator(".markdown").evaluate((m) => ({
      firstCode: m.querySelector("li code")?.textContent,
      strongCode: m.querySelector("strong > code")?.textContent,
      link: [m.querySelector("a[href='https://example.com/doc']")?.textContent, m.querySelector("a[href='https://example.com/doc']")?.target],
      head: [...m.querySelectorAll(".markdown-table-wrap table thead th")].map((x) => x.textContent),
      cells: [...m.querySelectorAll(".markdown-table-wrap table tbody tr")].map((r) => [...r.children].map((c) => c.innerHTML.replace(/<\/?span>/g, ""))),
      nested: m.querySelector("ol > li > ul > li code")?.textContent,
      ol: [...m.querySelectorAll(":scope > ol > li")].length,
      fence: [m.querySelector(".markdown-code")?.dataset.language, m.querySelector(".markdown-code pre code")?.textContent],
      raw: /\*\*|\|/.test(m.textContent),
      // code block 안 code는 바탕을 따로 갖지 않는다(참고 `.markdown :not(pre) > code`만 칩)
      fenceBg: getComputedStyle(m.querySelector(".markdown-code pre code")).backgroundColor,
    }));
    const want = {
      firstCode: "index.html",
      strongCode: "a1b2c3d",
      link: ["참고 문서", "_blank"],
      head: ["ID", "판정", "근거"],
      cells: [
        ["<code>T2</code>", "같음", "<code>prefs.rs</code>"],
        ["R1", "<strong>일부</strong>", "표와 <code>code</code>"],
      ],
      nested: "하나",
      ol: 2,
      fence: ["sh", "cargo test -p aaa"],
      raw: false,
      fenceBg: "rgba(0, 0, 0, 0)",
    };
    if (JSON.stringify(md) !== JSON.stringify(want)) throw new Error(`보고 Markdown ${JSON.stringify(md)}`);
    const frameEl = await page.getByTestId("library-frame").elementHandle();
    const frame = await until("결과물 frame", async () => frameEl && (await frameEl.contentFrame()));
    const probe = await until("결과물 HTML 실행", async () => {
      const d = await frame.evaluate(() => ({
        title: document.getElementById("t")?.textContent,
        pic: document.getElementById("pic")?.naturalWidth,
        ...document.body.dataset,
      }));
      return d.api ? d : null;
    });
    if (probe.title !== "결과 보고" || probe.pic !== 1) throw new Error(`결과물 표시: ${JSON.stringify(probe)}`);
    // opaque origin: 부모 문서·cookie·제어 API에 닿지 못한다
    if (probe.parent !== "blocked" || probe.cookie === "visible" || probe.api === "200") {
      throw new Error(`결과물이 격리되지 않았다: ${JSON.stringify(probe)}`);
    }
    const big = page.getByTestId("files").locator("li", { hasText: "big.bin" });
    if (!(await big.innerText()).includes("agent에서 읽음")) throw new Error(await big.innerText());
    const href = await big.locator("a").getAttribute("href");
    const n = await page.evaluate(async (u) => (await (await fetch(u)).arrayBuffer()).byteLength, href);
    if (n !== 2097152) throw new Error(`agent 파일 길이 ${n}`);
    await shot(page, "desktop-thread-library");
    // 휴대폰 폭: 넓은 표는 그 안에서만 옆으로 밀리고 화면은 밀리지 않는다(참고 .markdown-table-wrap)
    await page.setViewportSize({ width: 390, height: 844 });
    await report.locator(".markdown-table-wrap").scrollIntoViewIfNeeded();
    const wide = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
    if (wide > 0) throw new Error(`휴대폰 폭에서 화면이 ${wide}px 옆으로 밀린다`);
    await shot(page, "phone-report-markdown");
    await page.setViewportSize({ width: 1280, height: 800 });
    // 알림을 보내기 전에 확인하면 그 알림은 빠진다(plan §8.69): 다음 단계가 볼 보고 알림이 나간 뒤에 확인한다
    await until("보고 알림 보냄", () => pushes.slice(pushFrom).some((x) => x.path === new URL(pushDev.json.endpoint).pathname));
    await page.getByRole("button", { name: "확인(ack)" }).click();
    await until("ack 뒤 검토 대기 아님", async () => !(await item.innerText()).includes("검토 대기"));
    if (aaa("thread", "show", "demo", t.id).acked_hash !== aaa("thread", "show", "demo", t.id).report_hash) {
      throw new Error("ack가 기록되지 않았다");
    }
    // 다음 단계는 t1 터미널을 본다
    await page.getByTestId("pane-t1").click();
    await until("t1 터미널", async () => (await screenText(page)).includes("한글-8"));
  });

  await step("Web Push: 보고 준비 알림이 기기에 오고(제목·자리는 그 thread) 그 thread로 연다, 버려진 구독(410)은 지운다, 쓰는 화면이 있으면 어느 기기에도 보내지 않는다(차이 목록 N5·N6)", async () => {
    const { msg } = await nextPush(pushDev, pushFrom, "보고 알림");
    // 한 thread 알림: 제목은 「프로젝트 · thread 제목」, 자리(tag)는 그 thread(차이 목록 N6)
    if (msg.title !== "demo · 웹 결과물" || msg.tag !== "aaa-demo/t-0001" || !msg.body.includes("new report") || msg.url !== "/#thread=demo%2Ft-0001") {
      throw new Error(JSON.stringify(msg));
    }
    await until("410 구독 정리", async () =>
      (await page.evaluate(async () => (await (await fetch("/api/v1/push")).json()).ok.subscribed)) === 1,
    );
    const kinds = aaa("events", "demo").events.map((e) => e.kind);
    if (!kinds.includes("push_sent") || !kinds.includes("push_gone")) throw new Error(kinds.join(","));
    // 구독 주소(기기에 보내는 열쇠)는 기록에 남지 않는다
    if (JSON.stringify(aaa("events", "demo")).includes("dev1")) throw new Error("흐름 사건에 endpoint가 남았다");
    // 화면을 다시 쓰면(보이고 focus) 새 보고가 나와도 어느 기기에도 보내지 않는다(herdr-web-ui push.ts inUse, 차이 목록 N5)
    await setInUse(page, true);
    await until("쓰는 화면", async () => (await pushInUse()) === 1);
    const from = pushes.length;
    fs.writeFileSync(`${root}/task2.md`, "보는 중 보고");
    const t2 = aaa("thread", "start", "demo", "--title", "보는 중", "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/task2.md`);
    await until("보는 중 보고 회수", () => aaa("thread", "show", "demo", t2.id).report_hash);
    const skipped = await until("보내지 않음 기록", () =>
      aaa("events", "demo").events.find((e) => e.kind === "push_skipped" && e.detail.startsWith("쓰는 화면 1개")),
    );
    if (pushes.slice(from).some((x) => x.path === new URL(pushDev.json.endpoint).pathname)) throw new Error(`쓰는 화면이 있는데 보냈다 ${skipped.detail}`);
    // 보내지 않은 대신 쓰는 화면에는 앱 안 알림이 내려온다(plan §8.70)
    await until("쓰는 화면의 앱 안 알림", async () => (await page.locator(".droplet-title").allInnerTexts()).includes("보는 중"));
    // 화면을 닫으면(연결이 끊기면) 쓰는 화면이 아니다
    const extra = (await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 })).page;
    await until("둘째 화면도 씀", async () => (await pushInUse()) === 2);
    await extra.context().close();
    await until("닫은 화면은 빠짐", async () => (await pushInUse()) === 1);
    aaa("thread", "ack", "demo", t2.id);
  });

  await step("이 기기 알림(herdr-web-ui Droplet·alertSound·notifications·PushTestControls·더 보기, 차이 목록 N1·N2·N8·N9): 새 보고에 앱 안 알림이 내려오고 누르면 그 thread, 소리는 켜면 미리 듣고 알림마다, 가려지면 탭 알림, 앱 안을 끄면 안 내려옴, 메뉴로 끄면 아무것도, 시험 보내기는 구독이 없다고", async () => {
    // 소리·탭 알림·권한·push 구독은 기록하는 것으로 바꾼다(실제 소리·알림·push 서비스 없이)
    const init = () => {
      const log = [];
      Object.defineProperty(window, "__alertLog", { value: log });
      class FakeNotification {
        static permission = "granted";
        static async requestPermission() {
          return "granted";
        }
        constructor(t) {
          log.push(`new ${t}`);
        }
        addEventListener() {}
      }
      Object.defineProperty(window, "Notification", { configurable: true, value: FakeNotification });
      ServiceWorkerRegistration.prototype.showNotification = async function (t, o) {
        log.push(`tab ${t}|${o.tag}|${o.data?.url}`);
      };
      PushManager.prototype.subscribe = async () => {
        throw new DOMException("시험에서는 push 서비스가 없다", "AbortError");
      };
      PushManager.prototype.getSubscription = async () => null;
      class FakeAudio {
        state = "suspended";
        destination = {};
        // 소리 하나가 울리는 동안은 같은 소리를 겹쳐 울리지 않는다(참고 chime): 시계가 흘러야 한다
        get currentTime() {
          return performance.now() / 1000;
        }
        constructor() {
          window.__evLog?.push("ac new");
        }
        async resume() {
          window.__evLog?.push(`ac resume ${this.state}`);
          this.state = "running";
        }
        createOscillator() {
          const o = {
            type: "",
            frequency: { value: 0 },
            connect: (g) => g,
            start: (at) => {
              log.push(`tone ${o.frequency.value}`);
              // 이 소리가 끝나는 때(참고 NOTE_LENGTH_S 0.3초)
              window.__chimeEnd = Math.max(window.__chimeEnd ?? 0, at + 0.3);
              window.__evLog?.push(`osc ${o.frequency.value} at ${at.toFixed(2)} now ${this.currentTime.toFixed(2)}`);
            },
            stop() {},
          };
          return o;
        }
        createGain() {
          return { gain: { setValueAtTime() {}, exponentialRampToValueAtTime() {} }, connect: (d) => d };
        }
      }
      Object.defineProperty(window, "AudioContext", { configurable: true, value: FakeAudio });
      // iPhone·iPad Safari의 소리 범주(Audio Session API, plan §8.83): 바꿀 때마다 남긴다
      let sessionType = "auto";
      const session = {
        get type() {
          return sessionType;
        },
        set type(v) {
          sessionType = v;
          log.push(`session ${v}`);
        },
      };
      Object.defineProperty(navigator, "audioSession", { configurable: true, value: session });
      // 진단: 상태 사건 WS의 열림·닫힘과 thread 사건(보고 hash)
      const ev = [];
      Object.defineProperty(window, "__evLog", { value: ev });
      new MutationObserver((ms) => {
        for (const m of ms) for (const n of m.addedNodes) if (n.classList?.contains("droplet")) ev.push(`drop ${n.dataset.kind}`);
      }).observe(document, { childList: true, subtree: true });
      addEventListener("pagehide", () => ev.push("pagehide"));
      const RealBC = window.BroadcastChannel;
      window.BroadcastChannel = class extends RealBC {
        postMessage(m) {
          ev.push(`bc ${m?.type} @${(performance.now() / 1000).toFixed(2)}`);
          super.postMessage(m);
        }
      };
      const Real = window.WebSocket;
      window.WebSocket = class extends Real {
        constructor(url, p) {
          super(url, p);
          if (!String(url).includes("/api/v1/events")) return;
          const t0 = performance.now();
          const at = () => Math.round(performance.now() - t0);
          this.addEventListener("open", () => ev.push(`open`));
          this.addEventListener("close", (e) => ev.push(`close ${e.code} ${at()}`));
          this.addEventListener("message", (m) => {
            try {
              const d = JSON.parse(m.data);
              if (d.ev === "thread") ev.push(`thr ${d.id} ${String(d.report_hash).slice(0, 6)} ${String(d.acked_hash).slice(0, 6)} @${(performance.now() / 1000).toFixed(2)}`);
            } catch {}
          });
        }
      };
    };
    const apOpen = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 }, { init });
    const ap = apOpen.page;
    const log = () => ap.evaluate(() => window.__alertLog.join(","));
    const clear = () => ap.evaluate(() => void window.__alertLog.splice(0));
    // 기록을 기다린다. 실패하면 그때의 기록·고른 자리·소리 상태를 남긴다
    const waitLog = (what, ok) =>
      until(what, async () => ok(await log())).catch(async (e) => {
        const at = await ap.evaluate(() => ({ hash: location.hash, vis: document.visibilityState, sound: localStorage.getItem("aaa:alert-sound") }));
        const ev = await ap.evaluate(() => window.__evLog.slice(-16).join(","));
        throw new Error(`${e.message}: 기록 [${await log()}] ${JSON.stringify(at)} 앱 안 ${await droplet.count()} 오류 [${apOpen.errors.join("; ")}] 사건 [${ev}] 보고 ${String(aaa("thread", "show", "demo", t.id).report_hash).slice(0, 6)}`);
      });
    const droplet = ap.locator(".droplet");
    // 들어오는 움직임(참고 Droplet.css)이 끝난 모습
    const landed = (d) => d.evaluate((el) => Promise.all(el.getAnimations({ subtree: true }).map((a) => a.finished)));
    fs.writeFileSync(`${root}/task3.md`, "알림 시험");
    const t = aaa("thread", "start", "demo", "--title", "알림 시험", "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/task3.md`);
    const target = `#thread=demo%2F${t.id}`;
    // 첫 보고: 앱 안 알림(작업 완료). 누르면 그 thread로 가고 접힌다
    await until("앱 안 알림", async () => (await droplet.count()) === 1 && (await droplet.locator(".droplet-title").innerText()) === "알림 시험");
    if ((await droplet.getAttribute("data-kind")) !== "done" || (await droplet.locator(".droplet-detail").innerText()) !== "작업 완료") {
      throw new Error(`앱 안 알림 ${await droplet.getAttribute("data-kind")} ${await droplet.locator(".droplet-detail").innerText()}`);
    }
    await landed(droplet);
    await shot(ap, "desktop-droplet");
    await droplet.locator(".droplet-card").click();
    await until("누르면 그 thread", async () => (await ap.evaluate(() => location.hash)) === target);
    await until("접힘", async () => (await droplet.count()) === 0);
    const work = aaa("thread", "show", "demo", t.id).work_dir;
    let n = 0;
    // 새 보고를 낸다. 돌려주는 hash로 이 화면이 그 thread 사건을 받았는지 본다(evLog `thr`)
    const report = async (what, { chime = true } = {}) => {
      // 앞 소리가 울리는 동안 온 「완료」는 그 소리가 이미 알렸다고 보고 겹쳐 울리지 않는다(참고 alertSound.ts chime).
      // 알림마다 소리를 보려면 앞 소리가 끝난 뒤에 다음 보고를 낸다
      if (chime) await until("앞 소리 끝", () => ap.evaluate(() => performance.now() / 1000 > (window.__chimeEnd ?? 0)));
      const before = aaa("thread", "show", "demo", t.id).report_hash;
      fs.writeFileSync(`${work}/report.md`, `## Report\n- ${what} ${++n}\n`);
      return await until(`${what} 보고`, () => {
        const h = aaa("thread", "show", "demo", t.id).report_hash;
        return h !== before && h;
      });
    };
    // 이 화면이 그 보고 사건을 받아 처리하고(앱 처리기는 진단 기록 뒤에 돈다) 두 번 그린 뒤
    const handled = async (hash) => {
      await until("보고 사건 받음", () => ap.evaluate((h) => window.__evLog.some((e) => e.startsWith("thr ") && e.includes(` ${h.slice(0, 6)} `)), hash));
      await ap.evaluate(() => new Promise((r) => setTimeout(() => requestAnimationFrame(() => requestAnimationFrame(r)))));
    };
    const dropsSince = (i) => ap.evaluate((i) => window.__evLog.slice(i).filter((e) => e.startsWith("drop")).length, i);
    const evMark = () => ap.evaluate(() => window.__evLog.length);
    // 그 thread를 보고 있으면 띄우지 않는다: 다른 곳을 본다
    await ap.getByTestId("pane-t1").click();
    // 소리: 켜는 누름이 미리 듣기(작업 완료 음 880→660), 그 뒤 알림마다
    const dialog = await openSettings(ap, "알림");
    await dialog.getByRole("switch", { name: "소리" }).click();
    await waitLog("미리 듣기", (l) => l === "tone 880,tone 660");
    await ap.keyboard.press("Escape");
    if ((await ap.evaluate(() => localStorage.getItem("aaa:alert-sound"))) !== "1") throw new Error("소리 설정이 저장되지 않았다");
    await clear();
    await report("소리");
    await waitLog("알림 소리", (l) => l === "tone 880,tone 660");
    await until("소리와 함께 앱 안 알림", async () => (await droplet.count()) === 1);
    // 무음 모드에서도(사용자 보고 10-18 iPhone, plan §8.83): 끈 동안은 소리 범주를 건드리지 않는다(위 기록에 session이 없다).
    // 켜는 누름이 미리 듣기이고, 소리를 낼 때만 "playback"(무음 스위치를 무시)으로 두었다가 끝나면 "auto"로 되돌린다
    await openSettings(ap, "알림");
    const silent = dialog.getByRole("switch", { name: "무음 모드에서도" });
    await until("앞 소리 끝", () => ap.evaluate(() => performance.now() / 1000 > (window.__chimeEnd ?? 0)));
    await clear();
    await silent.click();
    await waitLog("무음에서도 미리 듣기", (l) => l === "session playback,tone 880,tone 660,session auto");
    await ap.keyboard.press("Escape");
    if ((await ap.evaluate(() => localStorage.getItem("aaa:alert-silent"))) !== "1") throw new Error("무음 모드 설정이 저장되지 않았다");
    await until("앞 알림 접힘", async () => (await droplet.count()) === 0);
    await clear();
    await report("무음에서도");
    await waitLog("무음에서도 알림 소리", (l) => l === "session playback,tone 880,tone 660,session auto");
    await openSettings(ap, "알림");
    await silent.click();
    await ap.keyboard.press("Escape");
    if ((await ap.evaluate(() => localStorage.getItem("aaa:alert-silent"))) !== null) throw new Error("끈 무음 모드 설정이 남았다");
    // 가려지면: 앱 안 알림은 없고, 소리와 탭 알림(이 기기에 push 구독이 없다)
    const hide = (h) =>
      ap.evaluate((h) => {
        Object.defineProperty(document, "visibilityState", { configurable: true, get: () => (h ? "hidden" : "visible") });
        Object.defineProperty(document, "hidden", { configurable: true, get: () => h });
        document.dispatchEvent(new Event("visibilitychange"));
      }, h);
    await until("앞 알림 접힘", async () => (await droplet.count()) === 0);
    await clear();
    await hide(true);
    await report("가려짐");
    await waitLog("탭 알림", (l) => l.includes(`tab demo · 알림 시험|aaa-demo/${t.id}|/${target}`));
    // 소리는 탭끼리 누가 울릴지 정하는 짧은 틈(참고 alertTurns WINDOW_MS) 뒤에 울린다
    await waitLog("가려진 동안 소리", (l) => l.includes("tone 880,tone 660"));
    if ((await droplet.count()) !== 0) throw new Error(`가려진 동안 앱 안 알림 ${await log()}`);
    await hide(false);
    // 앱 안을 끄면 소리만
    await openSettings(ap, "알림");
    await dialog.getByRole("switch", { name: "앱 안에서" }).click();
    await ap.keyboard.press("Escape");
    await clear();
    const offMark = await evMark();
    await handled(await report("앱 안 끔"));
    await waitLog("앱 안 끈 뒤 소리", (l) => l === "tone 880,tone 660");
    if ((await dropsSince(offMark)) !== 0 || (await droplet.count()) !== 0) throw new Error("앱 안을 껐는데 내려왔다");
    // 더 보기 → 알림: 권한이 있고 구독이 없으면 「이 탭에서 켜짐」. 누르면 꺼지고 메뉴 단추에 점
    await ap.getByTestId("more").click();
    const item = ap.getByTestId("alerts-item");
    if (!(await item.innerText()).includes("이 탭에서 켜짐") || (await item.getAttribute("aria-checked")) !== "true") throw new Error(`알림 항목 ${await item.innerText()}`);
    if ((await ap.getByTestId("more").getAttribute("aria-label")) !== "더 보기") throw new Error("켜진 알림인데 점");
    await shot(ap, "desktop-more-menu");
    await item.click();
    await until("메뉴 단추 점", async () => (await ap.getByTestId("more").getAttribute("aria-label")) === "더 보기 · 알림 꺼짐");
    if ((await ap.evaluate(() => localStorage.getItem("aaa:alerts-on"))) !== "0") throw new Error("끈 알림이 저장되지 않았다");
    // 끄면 아무것도: 이 화면이 그 사건을 받아 처리한 뒤에도 소리·소리 차례·앱 안 알림·탭 알림이 없다
    await clear();
    const quietMark = await evMark();
    await handled(await report("끔"));
    const turns = await ap.evaluate((i) => window.__evLog.slice(i).filter((e) => e.startsWith("bc ")).length, quietMark);
    if ((await log()) !== "" || (await dropsSince(quietMark)) !== 0 || turns !== 0) throw new Error(`끈 뒤 [${await log()}] 소리 차례 ${turns}`);
    // 시험 보내기: 이 기기에 구독이 없다고 하고 「알림 다시 켜기」를 권한다
    await openSettings(ap, "알림");
    if ((await dialog.getByTestId("push-test-result").innerText()) !== "알림 꺼짐") throw new Error(`시험 줄 ${await dialog.getByTestId("push-test-result").innerText()}`);
    await shot(ap, "desktop-settings-alerts-off");
    await dialog.getByRole("button", { name: "알림 다시 켜기" }).click();
    await until("다시 켠 알림", async () => (await ap.getByTestId("more").getAttribute("aria-label")) === "더 보기");
    await dialog.getByRole("button", { name: "테스트 보내기" }).click();
    await until("구독 없음", async () => (await dialog.getByTestId("push-test-result").innerText()) === "이 기기의 푸시 구독이 없습니다. 알림을 다시 켜세요.");
    await ap.keyboard.press("Escape");
    await ap.context().close();
    // 휴대폰(390×844): 앱 안 알림과 「더 보기」 메뉴가 화면 안, 머리 아래에 있다(겹침·잘림 없음)
    const pp = (await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 })).page;
    const inside = (r, what) => {
      if (!r || r.left < 0 || r.right > 390 || r.top < 0 || r.bottom > 844) throw new Error(`${what}가 화면 밖 ${JSON.stringify(r)}`);
    };
    const rect = (l) => l.evaluate((e) => e.getBoundingClientRect().toJSON());
    await report("휴대폰", { chime: false });
    const pd = pp.locator(".droplet");
    await until("휴대폰 앱 안 알림", async () => (await pd.count()) === 1);
    await landed(pd);
    const head = await rect(pp.locator(".list-head"));
    const card = await rect(pd.locator(".droplet-card"));
    inside(card, "앱 안 알림");
    if (card.top < head.bottom) throw new Error(`앱 안 알림이 머리를 가린다 ${card.top} < ${head.bottom}`);
    await shot(pp, "phone-droplet");
    await pd.locator(".droplet-close").click();
    await until("휴대폰 알림 닫힘", async () => (await pd.count()) === 0);
    await pp.getByTestId("more").click();
    inside(await rect(pp.locator(".more-menu")), "더 보기 메뉴");
    const it = await rect(pp.getByTestId("alerts-item"));
    const menu = await rect(pp.locator(".more-menu"));
    if (it.bottom > menu.bottom) throw new Error(`알림 항목이 메뉴 밖으로 나온다 ${it.bottom} > ${menu.bottom}`);
    await shot(pp, "phone-more-menu");
    await pp.context().close();
    aaa("thread", "ack", "demo", t.id);
  });

  await step("공유 설정(herdr-web-ui preferencesSync·shared/preferences.ts, 차이 목록 T2): 한 기기에서 바꾼 테마·색상·사이드바 행은 다른 기기에 곧 오고, 기기 설정(상태 표시)은 남으며, 여는 것만으로는 쓰지 않고, 보기 권한 기기의 쓰기는 거부되며 다음 공유 변경을 모두 받는다", async () => {
    // 기다림은 시간이 아니라 사건으로 끝낸다: <html> 속성은 MutationObserver, host 쓰기는 그 요청의 답
    const htmlIs = (p, name, value) =>
      p.evaluate(
        ([name, value]) =>
          new Promise((done) => {
            const ok = () => document.documentElement.getAttribute(name) === value;
            if (ok()) return done();
            const mo = new MutationObserver(() => ok() && (mo.disconnect(), done()));
            mo.observe(document.documentElement, { attributes: true, attributeFilter: [name] });
          }),
        [name, value],
      );
    const wrote = (p) =>
      p
        .waitForResponse((r) => r.url().endsWith("/api/v1/call") && r.request().postDataJSON()?.op === "preferences_set")
        .then(async (r) => ({ sent: r.request().postDataJSON(), ...(await r.json()) }));
    const keys = (o) => JSON.stringify(Object.keys(o).sort());
    const held = (p) =>
      p.evaluate(() =>
        fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ op: "preferences" }) })
          .then((r) => r.json())
          .then((o) => o.ok),
      );
    const settings = (p) => openSettings(p, "모양");
    const close = (p) => p.keyboard.press("Escape");
    const a = (await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 })).page;
    const b = (await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 })).page;
    await a.getByTestId("machine-labA").waitFor();
    await b.getByTestId("machine-labA").waitFor();
    // 여는 것만으로는 아무것도 쓰지 않는다
    const before = await held(a);
    if (before.revision !== 0) throw new Error(`열기 전에 공유 설정이 있다: ${JSON.stringify(before)}`);

    // 데스크톱에서 밝게 → 휴대폰도 밝게(새로 고치지 않음)
    let d = await settings(a);
    let sent = wrote(a);
    await d.getByRole("group", { name: "테마" }).getByRole("button", { name: "밝게" }).click();
    const first = await sent;
    // 바꾼 공유 설정만 보낸다. host가 아직 고정을 갖지 않아 이 기기 고정(빈 목록)이 함께 간다
    if (first.ok?.revision !== 1 || first.ok.settings.theme !== "light" || keys(first.sent.settings) !== keys({ theme: 1 }) || !Array.isArray(first.sent.pins))
      throw new Error(`첫 쓰기 ${JSON.stringify(first)}`);
    await htmlIs(b, "data-theme", "light");
    // 휴대폰에서 색상 Catppuccin과 상태 표시 「점」(기기 설정) → 데스크톱은 색상만 받는다
    d = await settings(b);
    await d.getByRole("group", { name: "상태 표시" }).getByRole("button", { name: "점" }).click();
    sent = wrote(b);
    await d.locator("#settings-palette").selectOption("catppuccin");
    const second = await sent;
    if (keys(second.sent.settings) !== keys({ palette: 1 }) || keys(second.ok.settings) !== keys({ palette: 1, theme: 1 }))
      throw new Error(`둘째 쓰기에 기기 설정이 섞였다: ${JSON.stringify(second)}`);
    await htmlIs(a, "data-palette", "catppuccin");
    const aLook = await a.evaluate(() => JSON.parse(localStorage.getItem("aaa:appearance") ?? "{}"));
    if (aLook.statusIndicators === "dots") throw new Error("기기 설정(상태 표시)이 다른 기기로 갔다");
    if (aLook.palette !== "catppuccin" || aLook.theme !== "light") throw new Error(`받은 값이 이 기기에 남지 않았다: ${JSON.stringify(aLook)}`);
    await close(b);
    await shot(a, "desktop-shared-light-catppuccin");

    // 보기 권한 기기: 열면 host 문서를 받고, 제 변경은 거부되어 이 기기에만 남다가 다음 공유 변경을 모두 받는다
    const r = (await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 390, height: 844 })).page;
    await htmlIs(r, "data-palette", "catppuccin");
    await htmlIs(r, "data-theme", "light");
    d = await settings(r);
    sent = wrote(r);
    await d.getByRole("group", { name: "테마" }).getByRole("button", { name: "어둡게" }).click();
    const refused = await sent;
    if (refused.err?.code !== "forbidden") throw new Error(`보기 권한 쓰기가 거부되지 않았다: ${JSON.stringify(refused)}`);
    await htmlIs(r, "data-theme", "dark");
    if ((await held(a)).revision !== 2) throw new Error("거부된 쓰기가 host 문서를 바꿨다");
    await close(r);
    // 데스크톱에서 사이드바 한 줄 → 휴대폰은 한 줄, 보기 권한 기기는 밝음까지 모두 받는다
    sent = wrote(a);
    d = a.getByRole("dialog", { name: "설정" });
    await d.getByRole("group", { name: "사이드바 행" }).getByRole("button", { name: "한 줄" }).click();
    if ((await sent).ok?.revision !== 3) throw new Error("셋째 쓰기");
    await htmlIs(r, "data-theme", "light");
    await b.locator(".machine-workspaces:not(.is-two-line)").first().waitFor();
    await shot(r, "phone-reader-shared");

    // 되돌린다(뒤 단계의 화면이 기본 모양이게)
    sent = wrote(a);
    await d.getByRole("group", { name: "테마" }).getByRole("button", { name: "어둡게" }).click();
    await d.locator("#settings-palette").selectOption("amber");
    await d.getByRole("group", { name: "사이드바 행" }).getByRole("button", { name: "두 줄" }).click();
    await sent;
    await htmlIs(b, "data-palette", "amber");
    await htmlIs(b, "data-theme", "dark");
    await b.locator(".machine-workspaces.is-two-line").first().waitFor();
    for (const p of [a, b, r]) await p.context().close();
  });

  await step("보는 동안은 push 대신 앱 안 알림(사용자 화면 10-18 00:27 iPhone 홈 화면 앱 배너, herdr-web-ui server/push.ts inUse·sendTest·App.tsx 다시 등록): 구독이 있는 휴대폰을 열고 다시 열어도(다시 등록) push 0건, 보는 동안 새 보고는 push 0건·앱 안 알림 1건, 가려지면 push 1건", async () => {
    // 이 기기(휴대폰)는 알림을 켜 두어 이미 구독이 있다(홈 화면 앱처럼). 브라우저 push 서비스 대신 가짜 push 서비스의 주소를 준다
    const dev = pushDevice("push/dev3");
    const devPath = new URL(dev.json.endpoint).pathname;
    const toDev = (from) => pushes.slice(from).filter((x) => x.path === devPath);
    const subInit = (a) => {
      const log = [];
      Object.defineProperty(window, "__pushLog", { value: log });
      class FakeNotification {
        static permission = "granted";
        static async requestPermission() {
          return "granted";
        }
        constructor(t) {
          log.push(`new ${t}`);
        }
        addEventListener() {}
      }
      Object.defineProperty(window, "Notification", { configurable: true, value: FakeNotification });
      ServiceWorkerRegistration.prototype.showNotification = async function (t) {
        log.push(`tab ${t}`);
      };
      const key = Uint8Array.from(atob(a.key.replace(/-/g, "+").replace(/_/g, "/")), (c) => c.charCodeAt(0)).buffer;
      const sub = {
        endpoint: a.sub.endpoint,
        options: { applicationServerKey: key },
        toJSON: () => a.sub,
        unsubscribe: async () => (log.push("unsubscribe"), true),
      };
      PushManager.prototype.getSubscription = async () => sub;
      PushManager.prototype.subscribe = async () => (log.push("subscribe"), sub);
      try {
        localStorage.setItem("aaa:alerts-on", "1");
      } catch {}
      // 이 화면이 받은 thread 사건(보고 hash)
      const Real = window.WebSocket;
      window.WebSocket = class extends Real {
        constructor(url, p) {
          super(url, p);
          if (!String(url).includes("/api/v1/events")) return;
          this.addEventListener("message", (m) => {
            try {
              const d = JSON.parse(m.data);
              if (d.ev === "thread") log.push(`thr ${String(d.report_hash).slice(0, 6)}`);
            } catch {}
          });
        }
      };
    };
    const init = `(${subInit})(${JSON.stringify({ key: pushKey, sub: dev.json })})`;
    // 다른 화면은 쓰지 않는 것으로 둔다: 이 휴대폰 하나만 본다
    await setInUse(page, false);
    await until("쓰는 화면 없음", async () => (await pushInUse()) === 0);
    const start = pushes.length;
    const phone = await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 }, { init });
    const pp = phone.page;
    const subscribed = () => pp.evaluate(async () => (await (await fetch("/api/v1/push")).json()).ok.subscribed);
    const bellPush = async () => {
      await pp.getByTestId("more").click();
      const ok = (await pp.getByTestId("alerts-item").innerText()).includes("켜짐, 이 기기로 푸시");
      await pp.keyboard.press("Escape");
      return ok;
    };
    // (1) 열면 권한·구독이 있는 기기는 조용히 다시 등록한다(참고 App.tsx 「re-registers on every load」). 다시 열어도 같다
    await until("열 때 다시 등록", async () => (await subscribed()) === 1 && (await bellPush()));
    await pp.reload();
    await pp.getByTestId("machine-labA").waitFor();
    await until("다시 열 때 다시 등록", () => bellPush());
    if ((await pp.evaluate(() => window.__pushLog.join(","))) !== "") throw new Error(`다시 등록이 구독을 바꿨다: ${await pp.evaluate(() => window.__pushLog.join(","))}`);
    await until("휴대폰을 봄", async () => (await pushInUse()) === 1);
    const work = (title) => {
      fs.writeFileSync(`${root}/task-${title}.md`, title);
      const t = aaa("thread", "start", "demo", "--title", title, "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/task-${title}.md`);
      const hash = until(`${title} 보고`, () => aaa("thread", "show", "demo", t.id).report_hash);
      return { t, hash };
    };
    const seen = (hash) => until("보고 사건 받음", () => pp.evaluate((h) => window.__pushLog.includes(`thr ${h.slice(0, 6)}`), hash));
    // (2) 보는 동안: host는 보내지 않고(push_skipped) 이 화면에 앱 안 알림이 하나 내려온다
    const skippedBefore = aaa("events", "demo").events.filter((e) => e.kind === "push_skipped").length;
    const a = work("휴대폰으로 보는 중");
    await seen(await a.hash);
    await until("보내지 않음 기록", () => aaa("events", "demo").events.filter((e) => e.kind === "push_skipped").length > skippedBefore);
    await until("보는 동안 앱 안 알림", async () => (await pp.locator(".droplet-title").allInnerTexts()).includes("휴대폰으로 보는 중"));
    if ((await pp.locator(".droplet").count()) !== 1) throw new Error(`앱 안 알림 ${await pp.locator(".droplet").count()}개`);
    if (toDev(start).length !== 0) throw new Error(`보는 동안·여는 동안 push ${toDev(start).length}건`);
    await pp.locator(".droplet-close").click();
    await until("앱 안 알림 닫힘", async () => (await pp.locator(".droplet").count()) === 0);
    // (3) 가려지면(홈 화면으로 나감): 쓰는 화면이 없어 이 기기에 push가 하나 간다. 앱 안 알림·탭 알림은 없다
    await pp.evaluate(() => {
      Object.defineProperty(document, "visibilityState", { configurable: true, get: () => "hidden" });
      Object.defineProperty(document, "hidden", { configurable: true, get: () => true });
      document.hasFocus = () => false;
      document.dispatchEvent(new Event("visibilitychange"));
    });
    await until("가려진 휴대폰", async () => (await pushInUse()) === 0);
    const b = work("휴대폰을 닫은 동안");
    const { msg } = await nextPush(dev, start, "가려진 동안 push");
    if (msg.title !== "demo · 휴대폰을 닫은 동안" || msg.tag !== `aaa-demo/${b.t.id}`) throw new Error(JSON.stringify(msg));
    await seen(await b.hash);
    await pp.evaluate(() => new Promise((r) => setTimeout(r)));
    if ((await pp.locator(".droplet").count()) !== 0) throw new Error("가려진 동안 앱 안 알림이 내려왔다");
    const tabs = (await pp.evaluate(() => window.__pushLog.filter((e) => !e.startsWith("thr ")).join(",")));
    if (tabs !== "") throw new Error(`push가 있는 기기에 탭 알림: ${tabs}`);
    // 처음부터 끝까지 이 기기에 간 push는 가려진 동안의 하나뿐(확인 push 「알림이 켜졌다」 없음)
    if (toDev(start).length !== 1) throw new Error(`이 기기에 push ${toDev(start).length}건`);
    if (phone.errors.length) throw new Error(`페이지 오류: ${phone.errors.join("; ")}`);
    await pp.context().close();
    // 머신마다 도는 thread 수에 한도가 있다(max_threads_per_machine): 다 본 thread는 정리한다
    for (const x of [a, b]) aaa("thread", "resolve", "demo", x.t.id);
    await setInUse(page, true);
    await until("본 화면 다시 씀", async () => (await pushInUse()) === 1);
  });

  await step("두 기기·죽은 연결(coordinator 실기 10-18 00:58 iPhone: push는 「쓰는 화면 1개」로 생략, 앱 안 알림도 없음, herdr-web-ui ws.ts Liveness·server/push.ts inUse): A가 쓰는 동안 보이기만 하는 B에 앱 안 알림이 내려오고 push는 없다, 사건이 오지 않는 연결은 웹이 ping으로 알아채 다시 붙고, 답하지 않는 연결은 host가 끊어 쓰는 화면에서 뺀다, ⋯ 메뉴는 열면 첫 항목에 focus·화살표로 옮기고 Esc로 닫힌다(herdr-web-ui RowMenu)", async () => {
    // B(휴대폰): 보이지만 focus는 없다. 상태 사건 소켓을 「얼릴」 수 있게 감싼다: 언 뒤에는 받은 것을 버리고 보낸 것을 삼킨다.
    // 브라우저의 WebSocket ping 응답(pong)은 그대로라 host는 그 연결이 살아 있다고 본다(잠든 휴대폰·바뀐 망의 반쯤 열린 연결)
    const freezeInit = () => {
      document.hasFocus = () => false;
      const log = [];
      Object.defineProperty(window, "__evSock", { value: log });
      const Real = window.WebSocket;
      window.WebSocket = class extends Real {
        constructor(url, p) {
          super(url, p);
          if (!String(url).includes("/api/v1/events")) return;
          log.push("new");
          const me = this;
          me.frozen = false;
          window.__freezeEvents = () => void (me.frozen = true);
          this.addEventListener("message", (e) => {
            if (!me.frozen) return;
            log.push(`dropped ${String(e.data).slice(0, 24)}`);
            e.stopImmediatePropagation();
          });
        }
        send(d) {
          if (String(d).startsWith('{"t":"ping"')) log.push("ping");
          if (this.frozen) {
            log.push(`swallowed ${String(d).slice(0, 24)}`);
            return;
          }
          super.send(d);
        }
      };
    };
    const B = await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 }, { init: freezeInit });
    const bp = B.page;
    await until("A만 쓰는 화면", async () => (await pushInUse()) === 1);
    const skips = () => aaa("events", "demo").events.filter((e) => e.kind === "push_skipped");
    const devicePushes = (from) => pushes.slice(from).filter((x) => !x.path.startsWith("/gone/"));
    let n = 0;
    const work = (title) => {
      fs.writeFileSync(`${root}/task-dead-${++n}.md`, title);
      return aaa("thread", "start", "demo", "--title", title, "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/task-dead-${n}.md`);
    };
    const ts = [];
    // (1) A(본 화면)가 쓰는 동안: 어느 기기에도 push가 없고(참고 inUse는 모든 기기), 보이기만 하는 B에는 앱 안 알림이 내려온다
    let from = pushes.length;
    let k = skips().length;
    ts.push(work("두 기기 A 쓰는 중"));
    const s1 = await until("A 때문에 보내지 않음", () => skips().slice(k)[0]);
    if (!/^쓰는 화면 1개\(기기 [0-9A-Za-z]{1,7}\) · 항목 1개$/.test(s1.detail)) throw new Error(`push_skipped ${s1.detail}`);
    await until("보이기만 하는 B의 앱 안 알림", async () => (await bp.locator(".droplet-title").allInnerTexts()).includes("두 기기 A 쓰는 중"));
    if (devicePushes(from).length !== 0) throw new Error(`A가 쓰는 동안 push ${devicePushes(from).length}건`);
    await bp.locator(".droplet-close").click();
    // 머신마다 도는 thread 수에 한도가 있다(max_threads_per_machine): 다 본 thread는 정리한다
    const done = () => {
      while (ts.length) aaa("thread", "resolve", "demo", ts.shift().id);
    };
    done();
    // (2) 이제 B만 쓰고, B의 연결이 반쯤 열린 채 죽는다: host는 B를 쓰는 화면으로 세어 push를 생략하고 B는 사건을 받지 못한다(실기와 같은 상태)
    await setInUse(page, false);
    await until("쓰는 화면 없음", async () => (await pushInUse()) === 0);
    await bp.evaluate(() => {
      delete document.hasFocus;
      window.dispatchEvent(new Event("focus"));
    });
    await until("B만 쓰는 화면", async () => (await pushInUse()) === 1 && (await bp.evaluate(() => document.hasFocus())));
    await bp.evaluate(() => window.__freezeEvents());
    k = skips().length;
    ts.push(work("죽은 연결 동안"));
    await until("죽은 B 때문에 보내지 않음", () => skips().slice(k)[0]);
    await until("B가 그 사건을 받지 못함", () => bp.evaluate(() => window.__evSock.some((e) => e.startsWith("dropped") && e.includes('"thread"'))));
    // 웹: 망이 바뀌거나(online) 다시 보이면 ping하고, 그 pong이 두 번 오지 않으면 끊고 바로 다시 붙는다
    await bp.evaluate(() => window.dispatchEvent(new Event("online")));
    await until("ping을 보냄", () => bp.evaluate(() => window.__evSock.some((e) => e.startsWith('swallowed {"t":"ping"'))));
    await until("다시 붙음", () => bp.evaluate(() => window.__evSock.filter((e) => e === "new").length === 2));
    await until("새 연결이 쓰는 화면", async () => (await pushInUse()) === 1);
    // 다시 붙으면 끊긴 동안 놓친 보고를 알린다(t3code afterSequence처럼 놓친 사건을 잇는다)
    await until("놓친 보고의 앱 안 알림", async () => (await bp.locator(".droplet-title").allInnerTexts()).includes("죽은 연결 동안"));
    await bp.locator(".droplet-close").click();
    await until("놓친 보고 알림 닫힘", async () => (await bp.locator(".droplet").count()) === 0);
    done();
    k = skips().length;
    ts.push(work("다시 붙은 뒤"));
    await until("다시 붙은 B의 앱 안 알림", async () => (await bp.locator(".droplet-title").allInnerTexts()).includes("다시 붙은 뒤"));
    await until("다시 붙은 B 때문에 보내지 않음", () => skips().slice(k)[0]);
    // 10초 넘게 가려졌다가 다시 보이면 묻지 않고 바로 새로 붙는다(t3code application-active-reconnect). 시계만 앞으로 돌린다
    const mark = await bp.evaluate(() => window.__evSock.length);
    await bp.evaluate(() => {
      Object.defineProperty(document, "visibilityState", { configurable: true, get: () => "hidden" });
      document.dispatchEvent(new Event("visibilitychange"));
      const real = Date.now;
      Date.now = () => real() + 11_000;
      Object.defineProperty(document, "visibilityState", { configurable: true, get: () => "visible" });
      document.dispatchEvent(new Event("visibilitychange"));
    });
    await until("오래 가려진 뒤 바로 다시 붙음", () => bp.evaluate(() => window.__evSock.filter((e) => e === "new").length === 3));
    const after = await bp.evaluate((i) => window.__evSock.slice(i), mark);
    if (after.indexOf("ping") !== -1 && after.indexOf("ping") < after.indexOf("new")) throw new Error(`오래 가려진 뒤 묻고 기다렸다: ${after.join(",")}`);
    await until("다시 쓰는 화면", async () => (await pushInUse()) === 1);
    // host: WebSocket ping에도 답하지 않는 연결(읽지 않는 클라이언트)은 끊어 쓰는 화면에서 뺀다
    const cookie = (await bp.context().cookies()).find((c) => c.name === "aaa" || c.name === "__Host-aaa");
    const u = new URL(base);
    const raw = await new Promise((resolve, reject) => {
      const req = http.request({
        host: u.hostname,
        port: u.port,
        path: "/api/v1/events",
        headers: {
          Connection: "Upgrade",
          Upgrade: "websocket",
          "Sec-WebSocket-Key": crypto.randomBytes(16).toString("base64"),
          "Sec-WebSocket-Version": "13",
          Origin: base.slice(0, -1),
          Cookie: `${cookie.name}=${cookie.value}`,
        },
      });
      req.on("upgrade", (_res, socket) => resolve(socket));
      req.on("response", (res) => reject(new Error(`upgrade 거절 ${res.statusCode}`)));
      req.on("error", reject);
      req.end();
    });
    const text = Buffer.from(JSON.stringify({ t: "presence", active: true }));
    const mask = crypto.randomBytes(4);
    raw.write(Buffer.concat([Buffer.from([0x81, 0x80 | text.length]), mask, Buffer.from(text.map((b, i) => b ^ mask[i % 4]))]));
    // 그 뒤로는 아무것도 보내지 않는다(pong도)
    raw.pause();
    await until("읽지 않는 연결도 쓰는 화면", async () => (await pushInUse()) === 2);
    const t0 = Date.now();
    await until("host가 답하지 않는 연결을 끊음", async () => (await pushInUse()) === 1);
    const gone = Date.now() - t0;
    raw.destroy();
    if (gone > 6000) throw new Error(`끊기까지 ${gone}ms(ping 1초·3초 기한)`);
    // ⋯ 메뉴(사용자 화면 10-18: 미러링 키보드 Esc로 닫히지 않음): 키보드 사건이 메뉴로 오도록 첫 항목에 focus
    const focused = () => bp.evaluate(() => document.activeElement?.getAttribute("data-testid") ?? document.activeElement?.textContent?.trim());
    await bp.getByTestId("more").click();
    await until("열면 첫 항목에 focus", async () => (await focused()) === "링크 복사");
    await bp.keyboard.press("ArrowDown");
    if ((await focused()) !== "alerts-item") throw new Error(`아래 화살표 뒤 focus ${await focused()}`);
    await bp.keyboard.press("Escape");
    await until("Esc로 닫힘", async () => (await bp.locator(".more-menu").count()) === 0);
    if ((await focused()) !== "more") throw new Error(`닫힌 뒤 focus ${await focused()}`);
    if (B.errors.length) throw new Error(`페이지 오류: ${B.errors.join("; ")}`);
    await bp.context().close();
    done();
    await setInUse(page, true);
    await until("본 화면 다시 씀", async () => (await pushInUse()) === 1);
  });

  await step("끝난 thread 보기(herdr-projects popup.rs Section::Threads, 차이 목록 2장 L3·3장 C1): 끝낸 thread는 목록에서 빠지고 「더 보기 → 프로젝트」에서 묶음마다 보이며 끝남은 맨 아래 흐리게, 거르기, 고르면 보고, 뒤로는 그 프로젝트", async () => {
    const $ = (id) => `[data-testid="${id}"]`;
    if ((await page.locator($("thread-demo-t-0001")).count()) !== 1) throw new Error("t-0001이 목록에 없다");
    aaa("thread", "resolve", "demo", "t-0001");
    await domIs(page, "끝낸 thread가 목록에서 빠짐(sidebar.rs)", (q) => !document.querySelector(q), $("thread-demo-t-0001"));
    // 앞 단계들도 demo thread를 끝낸다: 끝난 수는 host가 말하는 그대로
    const ended = aaa("thread", "list", "demo").threads.filter((t) => t.group === "resolved").length;
    // 프로젝트 고르기(popup 첫 화면): 프로젝트마다 열린 묶음 줄과 끝난 수
    await page.getByTestId("more").click();
    const toProjects = hashIs(page, "#projects");
    await page.getByTestId("projects-item").click();
    await toProjects;
    await domIs(page, "demo 줄의 끝난 수", ([q, n]) => document.querySelector(q)?.textContent.includes(`끝남 ${n}`), [$("project-open-demo"), ended]);
    const toDemo = hashIs(page, "#project=demo");
    await page.getByTestId("project-open-demo").click();
    await toDemo;
    const view = page.getByTestId("project-threads-demo");
    await domIs(page, "끝남 묶음", (q) => document.querySelector(q) !== null, `${$("project-threads-demo")} ${$("project-group-resolved")}`);
    const done = view.getByTestId("project-group-resolved");
    if ((await done.locator("h3").innerText()) !== `끝남 (${ended})`) throw new Error(`끝남 ${ended}: ${await done.innerHTML()}`);
    // 묶음은 popup DISPLAY_ORDER대로, 끝남은 맨 아래
    const order = await view.locator("[data-testid^=project-group-]").evaluateAll((xs) => xs.map((x) => x.dataset.testid.slice("project-group-".length)));
    const rank = ["waiting_on_you", "ready_for_review", "landing", "working", "idle", "resolved"];
    if (order.at(-1) !== "resolved" || order.some((g, i) => i > 0 && rank.indexOf(g) < rank.indexOf(order[i - 1]))) throw new Error(order.join(","));
    // 끝난 줄은 흐리다(popup group_color: Resolved는 DarkGrey)
    const r = view.getByTestId("project-thread-t-0001");
    const dim = await r.locator(".project-threads-name").evaluate((n) => {
      const probe = document.createElement("span");
      probe.style.color = "var(--text-dim)";
      document.body.append(probe);
      const want = getComputedStyle(probe).color;
      probe.remove();
      return [getComputedStyle(n).color, want, n.textContent];
    });
    if (dim[0] !== dim[1] || dim[2] !== "t-0001 · 웹 결과물") throw new Error(`끝난 줄 ${JSON.stringify(dim)}`);
    await shot(page, "desktop-project-threads");
    // popup 「/」 거르기처럼 이름(id·제목)에 든 글로 고른다
    const rows = `${$("project-threads-demo")} [data-testid^=project-thread-]`;
    const all = await page.locator(rows).count();
    const filter = view.getByRole("searchbox");
    await filter.fill("웹 결과");
    await domIs(page, "거른 줄 하나", (q) => document.querySelectorAll(q).length === 1, rows);
    await filter.fill("없는 이름");
    await domIs(page, "맞는 것 없음", (q) => document.querySelector(q)?.textContent === "맞는 thread 없음", `${$("project-threads-demo")} [role=status]`);
    await filter.fill("");
    await domIs(page, "거르기 풂", ([q, n]) => document.querySelectorAll(q).length === n, [rows, all]);
    // 고르면 보고(popup detail), 뒤로는 그 프로젝트(popup: detail에서 목록으로)
    const toReport = hashIs(page, "#thread=demo%2Ft-0001");
    await r.click();
    await toReport;
    await domIs(page, "끝난 thread 보고", (q) => document.querySelector(q)?.textContent === "확인", `${$("report")} .markdown strong`);
    const back = hashIs(page, "#project=demo");
    await page.getByTestId("thread-back").click();
    await back;
    // 휴대폰 폭: 목록 대신 이 화면, 옆으로 밀리지 않는다
    await page.setViewportSize({ width: 390, height: 844 });
    await domIs(page, "휴대폰 폭 프로젝트 보기", (q) => document.querySelector(q)?.getBoundingClientRect().width > 0, $("project-threads-demo"));
    const wide = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
    if (wide > 0) throw new Error(`휴대폰 폭에서 화면이 ${wide}px 옆으로 밀린다`);
    await shot(page, "phone-project-threads");
    const toPicker = hashIs(page, "#projects");
    await view.getByRole("button", { name: "프로젝트 목록" }).click();
    await toPicker;
    const closed = hashIs(page, "");
    await page.getByTestId("projects").getByRole("button", { name: "닫기" }).click();
    await closed;
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.getByTestId("pane-t1").click();
    await until("t1 터미널", async () => (await screenText(page)).includes("한글-8"));
  });

  await step("사후 평가자: 목록에 섞이지 않고 header badge로 보인다 → 누르면 평가자 화면", async () => {
    const v = aaa("evaluator", "assign", "demo", "--profile", "fake", "--machine", "labA").evaluators[0];
    const id = v.thread.id;
    const badge = page.getByTestId("evaluator-demo");
    await until("평가 badge", async () => (await badge.count()) === 1);
    await until("평가자 brief 전달", () => aaa("thread", "show", "demo", id).brief === "delivered");
    if ((await page.getByTestId(`thread-demo-${id}`).count()) !== 0) throw new Error("평가자가 thread 목록에 보인다");
    if ((await page.getByTestId(`pane-demo/${id}`).count()) !== 0) throw new Error("평가자가 머신 목록에 보인다");
    const text = await badge.innerText();
    if (!/평가\s*●\s*0/.test(text)) throw new Error(`badge: ${text}`);
    await badge.click();
    const pane = aaa("thread", "show", "demo", id).pane;
    await until("평가자 화면 선택", async () => (await page.evaluate(() => location.hash)) === `#pane=${pane}`);
    await until("평가자 터미널", async () => (await screenText(page)).includes("fake>"));
    await shot(page, "desktop-evaluator");
    aaa("evaluator", "unassign", "demo");
    await until("끝낸 평가자 badge 사라짐", async () => (await badge.count()) === 0);
    await page.getByTestId("pane-t1").click();
    await until("t1 터미널", async () => (await screenText(page)).includes("한글-8"));
  });

  await step("agent 끊김 → 읽기 전용 표시", async () => {
    await stop("agent");
    await until("끊김 배너", async () =>
      (await page.getByTestId("readonly-banner").innerText()).includes("agent와 연결이 끊겼다"),
    );
    // 끊긴 머신은 이름 아래 「끊김」, 그 줄들은 흐리고 누를 수 없다(herdr-web-ui MachineSidebar machine-state·machine-offline·inert)
    const labA = page.getByTestId("machine-labA");
    await labA.locator(".machine-state-word", { hasText: "끊김" }).waitFor();
    if (!(await labA.locator(".machine-offline[inert]").count())) throw new Error("끊긴 머신의 줄이 흐리게·누를 수 없게 되지 않았다");
    if ((await labA.locator('.sidebar-status[data-status]:not([data-status="unknown"])').count()) !== 0) throw new Error("끊긴 머신 줄이 상태를 말한다");
    // 입력줄은 그대로 쓸 수 있고 보내기만 막힌다(herdr-web-ui TerminalInput: 끊겨도 쓰던 글은 두고 보내기만 거부)
    if (await page.getByLabel("보낼 글").isDisabled()) throw new Error("끊겼는데 입력줄이 꺼졌다");
    if (!(await page.getByRole("button", { name: "보내기" }).isDisabled())) throw new Error("끊겼는데 보내기가 켜져 있다");
    // 끊긴 동안 터미널에 친 글은 붙잡는다(herdr-web-ui lib/draft.ts): Enter는 붙잡지 않고 그 자리를 빈칸으로 둔다(참고 규칙)
    await page.getByTestId("terminal").click();
    await page.keyboard.type("echo held-$((2+3))");
    await page.keyboard.press("Enter");
    await until("끊김 띠에 붙잡은 입력", async () =>
      (await page.getByTestId("readonly-banner").innerText()).includes("붙잡은 입력: “echo held-$((2+3)) ”"),
    );
    const stored = await page.evaluate(() =>
      Object.keys(localStorage).filter((k) => k.startsWith("herdr-web-ui:terminal-draft:")).map((k) => JSON.parse(localStorage.getItem(k))),
    );
    if (stored.length !== 1 || stored[0].text !== "echo held-$((2+3)) " || stored[0].leftOut !== true)
      throw new Error(`붙잡은 입력 저장: ${JSON.stringify(stored)}`);
    await shot(page, "desktop-disconnected");
  });

  await step("agent 복귀 → 다시 입력", async () => {
    start("agent2", "agent", "--host", "local", "--name", "labA");
    await until("배너 사라짐", async () => (await page.getByTestId("readonly-banner").count()) === 0);
    // 다시 붙어도 붙잡은 입력은 저절로 가지 않는다: 띠에서 사람이 고른다. Enter를 뺐다고 알린다.
    const draftBanner = page.getByTestId("draft-banner");
    await draftBanner.waitFor();
    const draftText = await draftBanner.innerText();
    if (!draftText.includes("echo held-$((2+3))") || !draftText.includes("Enter·줄바꿈은 뺐다"))
      throw new Error(`붙잡은 입력 띠: ${draftText}`);
    if ((await screenText(page)).includes("held-5")) throw new Error("붙잡은 입력이 저절로 보내졌다");
    // 붙잡은 입력이 있는 동안 입력줄 보내기는 거절한다(그 앞부분 없이 줄이 돌면 안 된다)
    await page.getByLabel("보낼 글").fill("echo again-$((9+1))");
    await page.getByRole("button", { name: "터미널에 보내기" }).click();
    await until("먼저 보내거나 버리라는 말", async () =>
      (await page.locator(".terminal-input-note").innerText()).includes("붙잡은 입력을 먼저 보내거나 버린다"),
    );
    if ((await page.getByLabel("보낼 글").inputValue()) !== "echo again-$((9+1))") throw new Error("거절된 입력줄 글이 사라졌다");
    await page.getByTestId("draft-send").click();
    await until("붙잡은 입력 띠 사라짐", async () => (await draftBanner.count()) === 0);
    // 보내기는 Enter를 누르지 않는다: 글만 줄에 들어가 있다. Enter는 사람이 누른다.
    await page.getByTestId("terminal").click();
    await page.keyboard.press("Enter");
    await until("붙잡은 입력 실행", async () => (await screenText(page)).includes("held-5"));
    const left = await page.evaluate(() => Object.keys(localStorage).filter((k) => k.startsWith("herdr-web-ui:terminal-draft:")));
    if (left.length !== 0) throw new Error(`보낸 뒤 저장이 남았다: ${left}`);
    await page.getByRole("button", { name: "터미널에 보내기" }).click();
    await until("복귀 뒤 입력", async () => (await screenText(page)).includes("again-10"));
  });

  await step("host 재시작 → 웹이 스스로 다시 붙는다", async () => {
    await stop("host");
    await until("host 끊김 표시", async () =>
      (await page.getByTestId("readonly-banner").innerText()).includes("host와 연결이 끊겼다"),
    );
    // host가 없을 때 새로 열면 service worker가 「연결 없음」 화면을 보인다(PWA)
    const off = await page.context().newPage();
    await off.goto(base);
    await until("연결 없음 화면", async () => (await off.locator("h1").innerText()) === "host에 연결할 수 없다");
    await off.close();
    start("host2", "host");
    await until("다시 붙음", async () => (await page.getByTestId("readonly-banner").count()) === 0, 30_000);
    // agent 재연결(입력 가능)과 화면 재구독(snapshot)은 따로 도착한다. 화면이 돌아오는 것을 본다.
    await until("재시작 뒤 화면 복원", async () => (await screenText(page)).includes("again-10"));
    await page.getByLabel("보낼 글").fill("echo host-back");
    await page.getByRole("button", { name: "보내기" }).click();
    await until("재시작 뒤 입력", async () => (await screenText(page)).split("host-back").length >= 3);
    await shot(page, "desktop-after-host-restart");
  });

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

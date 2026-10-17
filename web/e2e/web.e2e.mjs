// 웹 front E2E: 실제 aaa host·agent를 띄우고 headless Chromium으로 pairing·보기·입력·권한·끊김을 확인한다.
//
// 실행(저장소 루트): cargo build -p aaa && (cd web && npm run build) && cargo build -p aaa
//                    NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs [스크린샷 폴더]
// AAA_BIN으로 실행 파일을 바꿀 수 있다. 실패하면 실험실 폴더를 남기고 종료 코드 1로 끝난다.

import { spawn, execFileSync } from "node:child_process";
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
};
const procs = new Map();

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function aaa(...args) {
  const sep = args.indexOf("--");
  const a = sep < 0 ? [...args, "--json"] : [...args.slice(0, sep), "--json", ...args.slice(sep)];
  return JSON.parse(execFileSync(AA, a, { env, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }));
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
async function pairPage(browser, code, viewport, { userAgent, platform, colorScheme, deviceScaleFactor, touch, init, line } = {}) {
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
  const page = await ctx.newPage();
  const errors = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  await page.goto(base);
  await page.getByLabel("pairing 코드").fill(code);
  await page.getByRole("button", { name: "연결" }).click();
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
async function step(name, fn) {
  const t0 = Date.now();
  try {
    await fn();
    steps.push(`ok   ${name} (${Date.now() - t0}ms)`);
  } catch (e) {
    steps.push(`FAIL ${name}: ${e.message}`);
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
      printf '[완료]\\n\\n## Report\\n- 웹 보기 **확인**: \\140index.html\\140\\n\\n## Next\\n- 결과 확인\\n' > "$dir/report.md"
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
    aaa("pane", "run", "--machine", "labA", "--name", "t1", "--", "/bin/sh");
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
    if (man.name !== "aaa" || man.display !== "standalone" || man.start_url !== "/") throw new Error(JSON.stringify(man));
    for (const icon of [...man.icons.map((i) => i.src), "/apple-touch-icon.png", "/favicon-32.png"]) {
      const r = await fetch(new URL(icon, base));
      const buf = Buffer.from(await r.arrayBuffer());
      // PNG 서명(89 50 4E 47)
      if (r.status !== 200 || r.headers.get("content-type") !== "image/png" || buf.readUInt32BE(0) !== 0x89504e47) {
        throw new Error(`아이콘 ${icon}: ${r.status} ${r.headers.get("content-type")}`);
      }
    }
    const ctx = await browser.newContext();
    const page = await ctx.newPage();
    await page.goto(base);
    const cdp = await ctx.newCDPSession(page);
    const got = await cdp.send("Page.getAppManifest");
    if (got.errors.length || !got.url.endsWith("/manifest.webmanifest")) throw new Error(JSON.stringify(got.errors));
    // service worker(§8.26)가 이 화면을 맡고, 브라우저가 설치할 수 있다고 본다
    const sw = await fetch(`${base}sw.js`);
    if (sw.status !== 200 || !sw.headers.get("content-type").startsWith("text/javascript")) throw new Error(`sw.js ${sw.status}`);
    if ((await fetch(`${base}offline.html`)).status !== 200) throw new Error("offline.html");
    await until("service worker가 화면을 맡음", () => page.evaluate(() => !!navigator.serviceWorker.controller));
    const inst = await cdp.send("Page.getInstallabilityErrors");
    if (inst.installabilityErrors.length) throw new Error(`설치할 수 없다: ${JSON.stringify(inst.installabilityErrors)}`);
    await ctx.close();
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
    aaa("pane", "run", "--machine", "labA", "--name", "widths", "--", process.execPath, `${root}/widths.js`);
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
    aaa("pane", "run", "--machine", "labA", "--name", "barapp", "--", process.execPath, `${root}/bar.js`);
    // [일련번호, 받은 조각]
    const lines = () =>
      aaa("pane", "read", "barapp")
        .lines.map((l) => /^GOT:(\d+):(.*)$/.exec(l))
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
    aaa("pane", "run", "--machine", "labA", "--name", "lineapp", "--", process.execPath, `${root}/bar.js`);
    // [일련번호, 받은 조각]
    const lines = () =>
      aaa("pane", "read", "lineapp")
        .lines.map((l) => /^GOT:(\d+):(.*)$/.exec(l))
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
    await dp.getByTestId("settings-open").click();
    const dlg = dp.getByRole("dialog", { name: "설정" });
    await dlg.waitFor();
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
    // 데스크톱에서 입력줄 고르기(D1에서 남은 빈 곳)
    await dlg.getByLabel("입력 방식").selectOption("line");
    await dp.keyboard.press("Escape");
    await until("설정 닫힘", async () => (await dp.getByRole("dialog", { name: "설정" }).count()) === 0);
    await dp.getByTestId("pane-t1").click();
    await until("입력줄", async () => (await dp.locator(".terminal-input").count()) === 1);
    // 다시 열어도 이 기기에 남는다. 끄면 막대도 사라진다
    await dp.reload();
    await until("남은 설정", async () => JSON.stringify(await chips()) === JSON.stringify(["codex 90%", "claude 80%"]));
    await dp.getByTestId("settings-open").click();
    await dp.getByRole("switch", { name: "plan 한도 보이기" }).click();
    await until("막대 사라짐", async () => (await dp.getByTestId("usage-strip").count()) === 0);
    if (bad.length) throw new Error(bad.join("; "));
    await dp.context().close();
  });

  await step("목록 바닥 줄은 늘 맨 아래(herdr-web-ui sidebar-footer, 사용자 요청 10-17): 목록이 길어도·굴려도 설정·사용량 줄은 화면 맨 아래, 휴대폰·iPad·데스크톱·화면 키보드", async () => {
    // 목록이 화면보다 길어야 시험이 된다: 짧으면 예전 판(바닥 줄이 목록 끝을 따라감)도 맨 아래에 있다
    const many = Array.from({ length: 24 }, (_, i) => `foot${i + 1}`);
    for (const name of many) aaa("pane", "run", "--machine", "labA", "--name", name, "--", "/bin/sleep", "600");
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
    await dp.getByTestId("settings-open").click();
    const dlg = dp.getByRole("dialog", { name: "설정" });
    for (let i = 0; i < 3; i++) await dlg.getByRole("button", { name: "글자 크게" }).click();
    if ((await dlg.getByRole("group", { name: "글자 크기" }).innerText()).trim() !== "16px") throw new Error("설정에 16px가 아니다");
    await until("16px", async () => (await rowsFont(dp)) === "16px");
    // 쓰는 화면이면 바뀐 글자로 PTY 크기를 다시 건다(13 → 16px이면 열이 줄어든다)
    const big = await settled(dp, "큰 글자 크기", (c) => c < wide * 0.9);
    if ((await dp.evaluate(() => localStorage.getItem("aaa:terminal-font-size"))) !== "16") throw new Error("16px 저장이 없다");
    for (let i = 0; i < 3; i++) await dlg.getByRole("button", { name: "글자 작게" }).click();
    await settled(dp, "13px로 돌아온 크기", (c) => c === wide);
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

  await step("키 처리(herdr-web-ui PaneTerminal, macOS): Shift+Enter ESC CR, Ctrl+Enter는 modifyOtherKeys대로, Cmd+⌫ ^U, Cmd+←/→ ^A/^E, Ctrl+V는 브라우저 몫, 선택 중 Ctrl+C는 복사, OSC 52, 늦게 붙은 화면도 모드를 안다", async () => {
    aaa("pane", "run", "--machine", "labA", "--name", "keysapp", "--", process.execPath, `${root}/keys.js`);
    const got = () => aaa("pane", "read", "keysapp").lines.join("\n");
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
    aaa("pane", "run", "--machine", "labA", "--name", "mouseapp", "--", process.execPath, `${root}/mouse.js`);
    // [일련번호, 종류, 값]
    const events = () =>
      aaa("pane", "read", "mouseapp")
        .lines.map((l) => /^(GOT|SIZE):(\d+):(.*)$/.exec(l))
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
    const lines = aaa("pane", "read", "t1").lines.join("\n");
    if (lines.includes("reader-")) throw new Error("reader 입력이 PTY에 들어갔다");
    await shot(rp, "mobile-reader");
    if (rd.errors.length) throw new Error(rd.errors.join("; "));
    await rd.page.context().close();
    // 낮은 화면의 보기 권한: 데스크톱 grid가 칸보다 높으면 칸이 굴러가고 커서 줄을 보이게 따라간다(herdr-web-ui followCursor)
    // t1 화면은 뒤 단계가 본다: 따로 pane을 띄운다(80×40, 줄 80개 뒤 커서는 맨 아래)
    aaa("pane", "run", "--machine", "labA", "--name", "follow", "--cols", "80", "--rows", "40", "--", "/bin/sh", "-c", "seq 1 80; echo follow-$((6*7)); exec cat");
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
    aaa("pane", "run", "--machine", "labA", "--name", "q1", "--", "/bin/sh", "-c", script, AA);
    const card = page.getByTestId("questions").locator(".question");
    await until("질문 카드", async () => (await card.count()) === 1);
    if (!(await card.innerText()).includes("make test")) throw new Error(`카드 본문: ${await card.innerText()}`);
    await until("pane 질문 표시", async () => (await page.getByTestId("agent-q1").innerText()) === "질문");
    const rd = await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 390, height: 844 });
    await rd.page.getByTestId("questions").waitFor();
    if ((await rd.page.getByRole("button", { name: "예" }).count()) !== 0) throw new Error("reader에게 답 버튼");
    await shot(page, "desktop-question");
    await card.getByRole("button", { name: "예" }).click();
    await until("카드 사라짐", async () => (await page.getByTestId("questions").count()) === 0);
    await until("reader 카드도 사라짐", async () => (await rd.page.getByTestId("questions").count()) === 0);
    const all = aaa("question", "list", "--all").questions;
    if (all.length !== 1 || all[0].state !== "answered" || all[0].answer !== "yes") throw new Error(JSON.stringify(all));
    await until("pane 작업 중", async () => (await page.getByTestId("agent-q1").innerText()) === "작업 중");
    if (rd.errors.length) throw new Error(rd.errors.join("; "));
    await rd.page.context().close();
  });

  await step("입력 막대·pane send: 글 뒤 Enter를 따로 보낸다(Codex식 붙여넣기 판정)", async () => {
    aaa("pane", "run", "--machine", "labA", "--name", "burst", "--", process.execPath, `${root}/burst.js`);
    await page.getByTestId("pane-burst").click();
    await until("burst 화면", async () => (await screenText(page)).includes("burst>"));
    await page.getByLabel("보낼 글").fill("hello web");
    await page.getByRole("button", { name: "보내기" }).click();
    await until("웹 제출", async () => (await screenText(page)).includes("SUBMIT:hello web"));
    aaa("pane", "send", "burst", "--text", "hello cli", "--enter");
    await until("CLI 제출", () => aaa("pane", "read", "burst").lines.join("\n").includes("SUBMIT:hello cli"));
    if ((await screenText(page)).includes("[newline]")) throw new Error("Enter가 글과 한 덩어리로 갔다");
    // 가짜 입력창이 한 덩어리를 실제로 가려내는지(이 단언이 없으면 위 단언은 아무것도 보지 않는다)
    aaa("pane", "send", "burst", "--text", "one\r");
    await until("한 덩어리 판정", async () => (await screenText(page)).includes("[newline]"));
    aaa("pane", "kill", "burst");
  });

  await step("입력줄: 여러 줄은 bracketed paste, Enter 보내기·Shift+Enter 줄바꿈, 조합 중 Enter 무시, 초안 보존, 휴대폰도 Enter는 보내기", async () => {
    aaa("pane", "run", "--machine", "labA", "--name", "pasteapp", "--", process.execPath, `${root}/paste.js`);
    const got = () => aaa("pane", "read", "pasteapp").lines.join("\n");
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
        aaa("pane", "send", "pasteapp", "--text", "MARK");
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

  await step("터미널 한글 IME(herdr-web-ui xterm 패치): 데스크톱 조합 사건·CDP 실제 IME, macOS Safari·iPad 기록 재생 → pane에는 완성된 글만", async () => {
    aaa("pane", "run", "--machine", "labA", "--name", "imeapp", "--", process.execPath, `${root}/paste.js`);
    aaa("pane", "run", "--machine", "labA", "--name", "imespin", "--", process.execPath, `${root}/spin.js`);
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
          aaa("pane", "send", pane, "--text", "MARK");
          await until("표지", () => aaa("pane", "read", pane).lines.join("\n").includes('GOT:"MARK"'));
          // 기록 재생은 빈 편집기에서 시작한다(herdr-web-ui 재생과 같다)
          await textarea(p).evaluate((el) => {
            el.value = "";
          });
          await type(p);
          await sleep(700);
          const all = aaa("pane", "read", pane).lines.join("\n");
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
    aaa("pane", "run", "--machine", "labA", "--name", "iphoneA", "--", process.execPath, `${root}/spin.js`);
    const IPHONE_UA = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_7 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/27.0.1 Mobile/15E148 Safari/604.1";
    const IPHONE = { width: 430, height: 932 };
    const wk = await webkit.launch();
    const bad = [];
    try {
      for (const [label, b, keyboard] of [["WebKit iPhone 화면 키보드", wk, true], ["Chromium iPhone", browser, false]]) {
        const { page: p } = await pairPage(b, aaa("web", "pair").code, IPHONE, { userAgent: IPHONE_UA, platform: "iPhone", touch: true, init: fakeViewport });
        const sent = [];
        const frames = [];
        let ready = false;
        await p.routeWebSocket(/\/api\/v1\/panes\/[^/]+\/ws/, (socket) => {
          const server = socket.connectToServer();
          server.onMessage((raw) => {
            if (typeof raw === "string") {
              const m = JSON.parse(raw);
              frames.push(m.t);
              if (m.t === "snapshot" || m.t === "readonly") ready = m.readonly === false;
            }
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
          // CI에서만 실패한 적이 있다(10-17 dbacbfe·372e85d): 그때 화면이 어땠는지 남긴다
          const seen = await p.evaluate(() => {
            const rows = document.querySelector(".xterm-rows");
            const scroll = document.querySelector(".term-scroll");
            return {
              text: rows?.innerText.replace(/\s+/g, " ").slice(-300),
              rows: rows?.childElementCount,
              font: rows ? getComputedStyle(rows).fontSize : null,
              scroll: scroll ? [scroll.clientWidth, scroll.clientHeight, scroll.scrollTop, scroll.scrollHeight] : null,
              vis: document.visibilityState,
              focus: document.hasFocus(),
              banners: [...document.querySelectorAll(".banner")].map((b) => b.textContent),
            };
          });
          throw new Error(`${e.message}: ${label} 프레임 ${JSON.stringify(frames.slice(-12))} ${JSON.stringify(seen)}`);
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
    for (const n of ["ipadA", "ipadB"]) aaa("pane", "run", "--machine", "labA", "--name", n, "--", process.execPath, `${root}/paste.js`);
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
    aaa("pane", "run", "--machine", "labA", "--name", "traceapp", "--", process.execPath, `${root}/paste.js`);
    const tp = (await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 })).page;
    await tp.getByTestId("trace-toggle").click();
    if ((await tp.getByTestId("trace-toggle").getAttribute("aria-pressed")) !== "true") throw new Error("입력 기록이 켜지지 않았다");
    await tp.getByTestId("pane-traceapp").click();
    await until("trace 화면", async () => (await screenText(tp)).includes("paste>"));
    await tp.locator(".xterm-screen").click();
    await imeType(tp);
    await until("조합 글 도착", () => aaa("pane", "read", "traceapp").lines.join("\n").includes('GOT:"상"'));
    await sleep(300);
    await tp.getByRole("button", { name: "목록으로" }).click();
    await tp.getByTestId("trace-send").click();
    await until("보냄 표시", async () => (await tp.locator(".foot-msg").allInnerTexts()).some((t) => t.startsWith("보냈다: input-")));
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
    if ((await tp.getByTestId("trace-toggle").innerText()).includes("(0개") === false) throw new Error("보낸 뒤 비우지 않았다");
    await tp.getByTestId("trace-toggle").click();
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
        aaa("pane", "run", "--machine", "labA", "--name", name, "--", "/bin/sh", app);
        const p = (await pairPage(wk, aaa("web", "pair").code, viewport, { colorScheme: scheme, deviceScaleFactor })).page;
        // 커서는 herdr-web-ui처럼 깜빡인다(cursorBlink). 꺼진 순간에 재면 커서색이 투명이라 샘을 볼 수 없다:
        // 깜빡임만 멈춰 커서가 그려진 상태에서 잰다
        await p.addStyleTag({ content: ".xterm .xterm-cursor-blink, .xterm .xterm-cursor-blink * { animation: none !important; }" });
        await p.getByTestId(`pane-${name}`).click();
        await until("조합 화면", async () => (await screenText(p)).includes("plain 상태"));
        await p.locator(".xterm-screen").click();
        for (const [line, ch] of [["plain", "ㅅ"], ["bg", "ㅅ"], ["bottom", "가"]]) {
          if (line === "bg") {
            aaa("pane", "send", name, "--text", "", "--enter");
            await until("배경 줄", async () => (await screenText(p)).includes("› 상태"));
          }
          if (line === "bottom") {
            aaa("pane", "send", name, "--text", "", "--enter");
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
            const left = document.elementsFromPoint(r.left - 3, r.top + r.height / 2).find((e) => rows.contains(e) && e !== rows);
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
        aaa("pane", "run", "--machine", "labA", "--name", name, "--", "/bin/sh", app);
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
      aaa("pane", "run", "--machine", "labA", "--name", name, "--", "/bin/sh", "-c", '"$0" hook --agent claude "$1"; exec cat', AA,
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
      refused = String(e.stdout);
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
    await until("셸 줄에 따옴표 경로", () => aaa("pane", "read", "t1").lines.join("").includes(`'${p1}'`));
    // Enter가 없었다: 셸은 실행하지 않았다(그 줄을 지운다)
    await sleep(300);
    if (aaa("pane", "read", "t1").lines.join("").includes("No such file")) throw new Error("셸이 경로를 실행했다");
    aaa("pane", "send", "t1", "--text", "\u0015");
    // 2) 형식이 다른 파일(SVG)은 host가 signature로 거부한다
    await pasteFile(page, Buffer.from("<svg xmlns='http://www.w3.org/2000/svg'/>"), "image/svg+xml", "x.svg");
    await until("형식 거부", async () => (await page.locator(".banner.err").innerText()).includes("PNG·JPEG·GIF·WebP"));

    // 3) harness pane(hook 보냄, 입력 대기): 여러 조각(1.2MB) 그림의 경로를 bracketed paste로 넣고 Enter는 보내지 않는다
    const big = Buffer.concat([png.subarray(0, 8), Buffer.alloc(1_200_000, 7)]);
    const recv = `${root}/h1.in`;
    aaa("pane", "run", "--machine", "labA", "--name", "h1", "--", "/bin/sh", "-c",
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
    aaa("pane", "run", "--machine", "labA", "--name", "h2", "--", "/bin/sh", "-c",
      'printf "Allow Bash(rm x)?\\n\\342\\235\\257 1. Yes\\n  2. No\\nEsc to cancel\\n"; ' +
        '"$0" hook --agent claude \'{"hook_event_name":"PermissionRequest","tool_name":"Bash","tool_input":{"command":"rm x"}}\'; ' +
        'stty raw -echo; exec cat > "$1"', AA, recv2);
    await until("h2 질문", async () => (await page.getByTestId("agent-h2").innerText()) === "질문");
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
      const mob = await pairPage(full, aaa("web", "pair", "--reader").code, { width: 390, height: 844 });
      const ctx = mob.page.context();
      await ctx.grantPermissions(["notifications"], { origin: base.slice(0, -1) });
      await mob.page.reload();
      await mob.page.getByTestId("machine-labA").waitFor();
      // 알림을 받을 수 있는 브라우저면 목록 아래에 알림 단추가 보인다(보기 권한 기기도 알림은 받는다)
      await until("알림 단추", async () => (await mob.page.getByTestId("push-toggle").innerText()).includes("알림 켜기"));
      const cdp = await ctx.newCDPSession(mob.page);
      const regId = new Promise((r) =>
        cdp.on("ServiceWorker.workerRegistrationUpdated", (e) => {
          const reg = e.registrations.find((x) => x.scopeURL === base && !x.isDeleted);
          if (reg) r(reg.registrationId);
        }),
      );
      await cdp.send("ServiceWorker.enable");
      const data = JSON.stringify({ title: "aaa · demo", body: "t-0009 question: 확인창", url: "/#thread=demo%2Ft-0009", tag: "aaa-demo" });
      await cdp.send("ServiceWorker.deliverPushMessage", { origin: base.slice(0, -1), registrationId: await regId, data });
      const shown = await until("알림 표시", async () => {
        const v = await mob.page.evaluate(async () =>
          (await (await navigator.serviceWorker.ready).getNotifications()).map((n) => [n.title, n.body, n.data?.url]),
        );
        return v.length ? v : null;
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

  const pushFrom = pushes.length;
  await step("thread 보고서·결과물: Markdown 보고, sandbox 결과물, agent에 남은 큰 파일, ack", async () => {
    aaa("project", "create", "demo");
    fs.writeFileSync(`${root}/task.md`, "웹 보기");
    const t = aaa("thread", "start", "demo", "--title", "웹 결과물", "--machine", "labA", "--profile", "fake", "--kind", "dir",
      "--task-file", `${root}/task.md`);
    await until("보고 회수", () => aaa("thread", "show", "demo", t.id).report_hash);
    const item = page.getByTestId(`thread-demo-${t.id}`);
    await until("thread 목록에 검토 대기", async () => (await item.innerText()).includes("검토 대기"));
    await item.click();
    const report = page.getByTestId("report");
    await until("보고 Markdown", async () => (await report.locator("strong").innerText()) === "확인");
    if ((await report.locator("code").innerText()) !== "index.html") throw new Error(await report.innerHTML());
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
    await page.getByRole("button", { name: "확인(ack)" }).click();
    await until("ack 뒤 검토 대기 아님", async () => !(await item.innerText()).includes("검토 대기"));
    if (aaa("thread", "show", "demo", t.id).acked_hash !== aaa("thread", "show", "demo", t.id).report_hash) {
      throw new Error("ack가 기록되지 않았다");
    }
    // 다음 단계는 t1 터미널을 본다
    await page.getByTestId("pane-t1").click();
    await until("t1 터미널", async () => (await screenText(page)).includes("한글-8"));
  });

  await step("Web Push: 보고 준비 알림이 기기에 오고 그 thread로 연다, 버려진 구독(410)은 지운다", async () => {
    const { msg } = await nextPush(pushDev, pushFrom, "보고 알림");
    if (msg.title !== "aaa · demo" || !msg.body.includes("report ready") || msg.url !== "/#thread=demo%2Ft-0001") {
      throw new Error(JSON.stringify(msg));
    }
    await until("410 구독 정리", async () =>
      (await page.evaluate(async () => (await (await fetch("/api/v1/push")).json()).ok.subscribed)) === 1,
    );
    const kinds = aaa("events", "--project", "demo").events.map((e) => e.kind);
    if (!kinds.includes("push_sent") || !kinds.includes("push_gone")) throw new Error(kinds.join(","));
    // 구독 주소(기기에 보내는 열쇠)는 기록에 남지 않는다
    if (JSON.stringify(aaa("events", "--project", "demo")).includes("dev1")) throw new Error("흐름 사건에 endpoint가 남았다");
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
    await page.getByText("실행·끊김(읽기 전용)").first().waitFor();
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

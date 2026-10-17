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
const ipadReplace = (page, jamo, result) => ipadKey(page, jamo, [["delete"], ["insert", result]]);

async function pairPage(browser, code, viewport, { userAgent, platform, colorScheme, deviceScaleFactor } = {}) {
  const ctx = await browser.newContext({
    viewport,
    isMobile: viewport.width < 700,
    hasTouch: viewport.width < 700,
    ...(userAgent ? { userAgent } : {}),
    ...(colorScheme ? { colorScheme } : {}),
    ...(deviceScaleFactor ? { deviceScaleFactor } : {}),
  });
  if (platform) await ctx.addInitScript((p) => Object.defineProperty(navigator, "platform", { get: () => p }), platform);
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

  const ctl = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 });
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

  await step("controller: 입력 막대로 한글", async () => {
    await page.getByLabel("보낼 글").fill("echo 한글-$((2*4))");
    await page.getByRole("button", { name: "보내기" }).click();
    await until("한글 입력 결과", async () => (await screenText(page)).includes("한글-8"));
    await shot(page, "desktop-controller");
  });

  await step("키 줄: Ctrl·Alt 유지 키는 다음 입력 한 번에 붙고 풀린다", async () => {
    await page.getByLabel("보낼 글").fill("cat -v");
    await page.getByRole("button", { name: "보내기" }).click();
    await sleep(400);
    const ctrl = page.getByRole("button", { name: "Ctrl", exact: true });
    await ctrl.click();
    if ((await ctrl.getAttribute("aria-pressed")) !== "true") throw new Error("Ctrl이 켜지지 않았다");
    await page.getByTestId("terminal").click();
    await page.keyboard.type("a");
    if ((await ctrl.getAttribute("aria-pressed")) !== "false") throw new Error("Ctrl이 풀리지 않았다");
    await page.getByRole("button", { name: "Alt", exact: true }).click();
    await page.getByTestId("terminal").click();
    await page.keyboard.type("x");
    await page.keyboard.press("Enter");
    await until("cat -v 출력", async () => (await screenText(page)).includes("^A^[x"));
    // 조합으로 확정된 글에는 유지 키를 붙이지 않고, 유지 키는 다음 키를 기다린다(herdr-web-ui PaneTerminal과 같다)
    await ctrl.click();
    await page.getByTestId("terminal").click();
    const cdp = await page.context().newCDPSession(page);
    await cdp.send("Input.imeSetComposition", { text: "한", selectionStart: 1, selectionEnd: 1 });
    await cdp.send("Input.insertText", { text: "한" });
    await cdp.detach();
    await sleep(200);
    if ((await ctrl.getAttribute("aria-pressed")) !== "true") throw new Error("조합 확정이 Ctrl을 써 버렸다");
    await page.keyboard.type("b");
    await page.keyboard.press("Enter");
    // 터미널 echo 줄: 한은 그대로, 유지 키 Ctrl은 다음 키 b에 붙는다(한b가 아니라 한^B)
    await until("조합 뒤 유지 키", async () => (await screenText(page)).includes("\n한^B\n"));
    await page.getByRole("button", { name: "^C", exact: true }).click();
  });

  await step("두 controller(herdr 모델): 가져오기 없이 바로 입력, 입력·focus를 얻은 화면이 크기를 다시 건다, 가려지면 놓는다", async () => {
    const other = await pairPage(browser, aaa("web", "pair").code, { width: 1000, height: 640 });
    const op = other.page;
    const t1size = () => {
      const p = aaa("pane", "list").panes.find((x) => x.name === "t1");
      return `${p.cols}x${p.rows}`;
    };
    const holder = () => aaa("pane", "list").panes.find((x) => x.name === "t1").lease?.label ?? null;
    const held = t1size();
    const first = holder();
    await op.getByTestId("pane-t1").click();
    await until("다른 웹이 크기를 정하는 중 표시", async () =>
      /웹\(.{4}\)이 크기를 정하는 중/.test(await op.getByTestId("lease-banner").innerText()),
    );
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
    await until("원래 화면에 표시", async () => (await page.getByTestId("lease-banner").count()) === 1);
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
    // 4) 쓰던 화면이 1초 넘게 가려지면 놓고, 보이고 focus 있는 화면이 자기 크기로 맞춘다
    await op.getByLabel("보낼 글").fill("echo hide");
    await op.getByRole("button", { name: "보내기" }).click();
    await until("다른 화면 크기", () => t1size() === otherSize);
    await op.evaluate(() => {
      Object.defineProperty(document, "visibilityState", { configurable: true, get: () => "hidden" });
      document.dispatchEvent(new Event("visibilitychange"));
    });
    await until("가려진 화면이 놓음 → 남은 화면 크기", () => t1size() === held && holder() === first, 5000);
    if (other.errors.length) throw new Error(other.errors.join("; "));
    await other.page.context().close();
    await until("표시 없음", async () => (await page.getByTestId("lease-banner").count()) === 0);
    if (await page.getByLabel("보낼 글").isDisabled()) throw new Error("입력 막대가 꺼져 있다");
  });


  await step("reader(휴대폰 크기): 보기만 된다", async () => {
    const rd = await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 390, height: 844 });
    const rp = rd.page;
    if ((await rp.getByRole("button", { name: "새 셸" }).count()) !== 0) throw new Error("reader에게 새 셸 버튼");
    await shot(rp, "mobile-list");
    await rp.getByTestId("pane-t1").click();
    await until("reader 화면", async () => (await screenText(rp)).includes("한글-8"));
    // 휴대폰 폭(11px)에서도 한글이 두 칸에 꼭 맞는다
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

  await step("composer: 여러 줄은 bracketed paste, Enter 보내기·Shift+Enter 줄바꿈, 조합 중 Enter 무시, 초안 보존, 휴대폰 Enter는 줄바꿈", async () => {
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
      const banner = () => page.getByTestId("attach-banner").evaluateAll((els) => els[0]?.textContent ?? null);
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
    // 휴대폰(터치): Enter는 줄바꿈, 보내기는 단추
    const mob = await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 });
    await mob.page.getByTestId("pane-pasteapp").click();
    const mbox = mob.page.getByLabel("보낼 글");
    // 데스크톱 화면이 쓰는 중이어도 가져오기 없이 입력된다
    if (await mbox.isDisabled()) throw new Error("휴대폰 입력 막대가 꺼져 있다");
    n = gots();
    await mbox.fill("휴대폰");
    await mbox.press("Enter");
    await sleep(500);
    if (gots() !== n || (await mbox.inputValue()) !== "휴대폰\n")
      throw new Error(`휴대폰 Enter: ${JSON.stringify(await mbox.inputValue())} · 받은 것 ${got().split("GOT:").slice(n + 1).join(" | ")}`);
    await mob.page.getByRole("button", { name: "보내기" }).click();
    await until("휴대폰 보내기", () => got().includes("휴대폰"));
    if (mob.errors.length) throw new Error(mob.errors.join("; "));
    await mob.page.context().close();
    aaa("pane", "kill", "pasteapp");
    await page.getByTestId("pane-t1").click();
    await until("t1 터미널", async () => (await screenText(page)).includes("한글-8"));
  });

  await step("터미널 한글 IME(herdr-web-ui xterm 패치): 데스크톱 조합 사건·CDP 실제 IME, macOS Safari·iPad 기록 재생 → pane에는 완성된 글만", async () => {
    aaa("pane", "run", "--machine", "labA", "--name", "imeapp", "--", process.execPath, `${root}/paste.js`);
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
    const runs = [
      ["Chromium", async () => page, [...desktop, cdpIme]],
      ["WebKit", async () => (await pairPage(wk, aaa("web", "pair").code, wide)).page, desktop],
      ["Safari 26.6.2 기록", async () => (await pairPage(browser, aaa("web", "pair").code, wide, { userAgent: SAFARI_UA, platform: "MacIntel" })).page, safari],
      ["iPad(MacIntel) 기록", async () => (await pairPage(browser, aaa("web", "pair").code, wide, { userAgent: IPAD_UA, platform: "MacIntel" })).page, ipad],
      ["iPad(iPad) 기록", async () => (await pairPage(browser, aaa("web", "pair").code, wide, { userAgent: IPAD_UA, platform: "iPad" })).page, ipad],
    ];
    try {
      for (const [engine, open, ways] of runs) {
        const p = await open();
        await p.getByTestId("pane-imeapp").click();
        await until("ime 화면", async () => (await screenText(p)).includes("paste>"));
        // 다른 화면이 쓰던 pane이어도 터미널을 누르고 바로 친다
        await until("입력 막대", async () => p.getByLabel("보낼 글").isEnabled());
        await p.locator(".xterm-screen").click();
        for (const [how, type, want] of ways) {
          aaa("pane", "send", "imeapp", "--text", "MARK");
          await until("표지", () => aaa("pane", "read", "imeapp").lines.join("\n").includes('GOT:"MARK"'));
          // 기록 재생은 빈 편집기에서 시작한다(herdr-web-ui 재생과 같다)
          await textarea(p).evaluate((el) => {
            el.value = "";
          });
          await type(p);
          await sleep(700);
          const all = aaa("pane", "read", "imeapp").lines.join("\n");
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
    await page.getByTestId("pane-t1").click();
    await until("t1 터미널", async () => (await screenText(page)).includes("한글-8"));
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

  await step("그림 붙여 넣기: 셸은 저장만, harness 입력창에는 경로(Enter 없음), 확인창·형식·reader 거부", async () => {
    const png = fs.readFileSync(`${root}/pic.png`);
    const banner = page.getByTestId("attach-banner");
    // 1) 일반 셸(t1): 대상 머신 세션 폴더에 저장하고 PTY에는 아무것도 넣지 않는다
    await page.getByTestId("pane-t1").click();
    await until("t1 터미널", async () => (await screenText(page)).includes("$"));
    await pasteFile(page, png, "image/png");
    await until("셸 저장 배너", async () => (await banner.innerText()).includes("경로만 저장했다"));
    const p1 = bannerPath(await banner.innerText());
    if (!p1?.startsWith(`${root}/s/agent/sessions/`)) throw new Error(`저장 경로: ${await banner.innerText()}`);
    if (!fs.readFileSync(p1).equals(png)) throw new Error("저장한 그림이 다르다");
    if ((fs.statSync(p1).mode & 0o777) !== 0o600) throw new Error(`권한 ${(fs.statSync(p1).mode & 0o777).toString(8)}`);
    await sleep(300);
    if (aaa("pane", "read", "t1").lines.join("\n").includes("attachments")) throw new Error("셸에 경로가 들어갔다");
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
    await until("입력창 배너", async () => (await banner.innerText()).includes("입력창에 넣었다"));
    const p2 = bannerPath(await banner.innerText());
    if (!fs.readFileSync(p2).equals(big)) throw new Error("여러 조각 그림이 다르다");
    await until("PTY가 받은 경로", () => fs.readFileSync(recv, "latin1") === `\x1b[200~${p2}\x1b[201~`);
    await shot(page, "desktop-attach");

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
    if (!(await page.getByLabel("보낼 글").isDisabled())) throw new Error("끊겼는데 입력 막대가 켜져 있다");
    await shot(page, "desktop-disconnected");
  });

  await step("agent 복귀 → 다시 입력", async () => {
    start("agent2", "agent", "--host", "local", "--name", "labA");
    await until("배너 사라짐", async () => (await page.getByTestId("readonly-banner").count()) === 0);
    await page.getByLabel("보낼 글").fill("echo again-$((9+1))");
    await page.getByRole("button", { name: "보내기" }).click();
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

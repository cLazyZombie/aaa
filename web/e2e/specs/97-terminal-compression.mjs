// 실제 PTY·host·Worker·xterm. codec/ACK·입력·페이지는 실제 연결로 검사한다.
// 회선별 시간·FIFO는 RateLimitedWire의 주입 시계 owner가 맡는다.
import assert from "node:assert/strict";
import fs from "node:fs";
import { gunzipSync } from "node:zlib";
import { performance } from "node:perf_hooks";
import { RateLimitedWire } from "../lib/rate-limited-wire.mjs";
import { specCleanup, aaa, root, base, step, browser, webkit, pairPage, inList, waitOutput, domIs, inTime, screenText, shot, termDiag } from "../lib/harness.mjs";

function observeWorker() {
  window.__inflateHeld = false;
  window.__inflateReleased = false;
  const Native = window.Worker;
  window.Worker = class extends Native {
    constructor(url, options) { super(url, options); this.terminalPayload = options?.name === "aaa-terminal-inflate"; }
    postMessage(message, ...rest) {
      if (this.terminalPayload && !window.__inflateReleased) {
        window.__inflateHeld = true;
        document.documentElement.dataset.inflateHeld = "1";
        window.__releaseInflate = () => { window.__inflateReleased = true; super.postMessage(message, ...rest); };
      } else super.postMessage(message, ...rest);
    }
  };
}

function identityPeer() {
  const Native = window.WebSocket;
  window.WebSocket = class extends Native {
    constructor(url, ...rest) {
      const target = new URL(url);
      target.searchParams.delete("compression");
      super(target, ...rest);
    }
  };
}

function routeWire(stats, bps = 1e9, rtt = 1) {
  return async (socket) => {
    if (stats.pane && !socket.url().includes(encodeURIComponent(stats.pane))) { socket.connectToServer(); return; }
    // 재접속 시점의 snapshot이 시험 출력의 중간을 잡지 않게 실제 PTY 완료 사건으로 연결을 놓는다.
    if (stats.beforeConnect) await stats.beforeConnect;
    const server = socket.connectToServer();
    stats.started = performance.now();
    let echoTail = "";
    let binaryBytes = 0;
    const downstream = new RateLimitedWire((raw) => {
      socket.send(raw);
      stats.delivered.push({ kind: typeof raw === "string" ? JSON.parse(raw).t : "binary", at: performance.now() });
    }, bps, rtt);
    const upstream = new RateLimitedWire((raw) => {
      if (typeof raw === "string") {
        const msg = JSON.parse(raw);
        if (msg.t === "ack") { stats.acks.push({ stream: msg.stream, offset: msg.offset, at: performance.now() }); stats.onAck?.(); }
        if (msg.t === "input") stats.inputs.push({ at: performance.now(), bytes: Buffer.byteLength(msg.data), data: msg.data });
      } else stats.inputs.push({ at: performance.now(), bytes: raw.length, data: Buffer.from(raw).toString("utf8") });
      server.send(raw);
    }, bps, rtt);
    let held = [];
    stats.releaseHistory = () => {
      stats.holdHistory = false;
      for (const [raw, size] of held) downstream.push(raw, size);
      held = [];
    };
    server.onMessage((raw) => {
      const size = typeof raw === "string" ? Buffer.byteLength(raw) : raw.length;
      let kind = "binary";
      if (typeof raw === "string") {
        const msg = JSON.parse(raw); kind = msg.t;
        if (kind === "snapshot" || kind === "history") {
          const data = Buffer.from(kind === "snapshot" ? msg.replay : msg.data, "base64");
          const bytes = msg.compression ? gunzipSync(data) : data;
          assert.equal(msg.compression?.raw_len ?? bytes.length, bytes.length);
          stats.payloads.push({ kind, wire: size, raw: bytes.length, compression: msg.compression?.codec ?? null,
            stream: msg.stream, id: msg.id, lines: msg.lines, more: msg.more });
        }
      }
      if (kind === "binary") {
        binaryBytes += size;
        echoTail = (echoTail + Buffer.from(raw).toString("utf8")).slice(-2048);
        stats.binaryTail = echoTail; stats.binaryBytes = binaryBytes;
        if (stats.expectedEcho && echoTail.includes(stats.expectedEcho)) stats.echoOffset = binaryBytes;
      }
      // 실제 history와 뒤따르는 binary/state를 FIFO로 붙잡는다. 느린 회선의
      // 몇 초 대기 대신 이 사건을 쓰고, 반대 방향의 실제 입력은 계속 전달한다.
      if (stats.holdHistory && (kind === "history" || held.length)) {
        held.push([raw, size]);
        if (kind === "history") stats.onHistory?.();
      } else downstream.push(raw, size);
    });
    socket.onMessage((raw) => upstream.push(raw, typeof raw === "string" ? Buffer.byteLength(raw) : raw.length));
    const clear = () => { held = []; downstream.close(); upstream.close(); };
    server.onClose((code, reason) => { clear(); socket.close({ code, reason }); });
    socket.onClose((code, reason) => { clear(); server.close({ code, reason }); });
    stats.disconnect = () => { clear(); socket.close({ code: 1011, reason: "test reconnect" }); server.close(); };
  };
}

let corpusFixture;
let safari;
const safariBrowser = async () => (safari ??= specCleanup.own(() => webkit.launch({ headless: true }), value => value.close())).value;

async function workerLoad(engine, label, width, failure) {
  const sockets = [], workerRequests = [], diagnostics = [], messages = [];
  const dev = await pairPage(engine, aaa("web", "pair").code, { width, height: 844 }, {
    serviceWorkers: "block",
    init: { content: `(${function (failure) {
      const Native = window.Worker;
      window.Worker = class extends Native {
        constructor(url, options) {
          if (failure === "constructor") throw new DOMException("fixture Worker creation denied", "SecurityError");
          if (failure === "runtime") {
            const bad = URL.createObjectURL(new Blob(['throw new Error("fixture worker startup failure");'], { type: "text/javascript" }));
            super(bad, options);
            this.addEventListener("error", () => URL.revokeObjectURL(bad), { once: true });
          } else super(url, options);
        }
      };
    }} )(${JSON.stringify(failure)})` },
    onPage: page => {
      page.on("websocket", socket => { if (socket.url().includes("/panes/")) sockets.push(socket.url()); });
      page.on("console", message => { messages.push(message.text()); if (message.text().startsWith("터미널 압축 Worker 실패")) diagnostics.push(message.text()); });
    },
    setup: async context => context.route("**/assets/terminalInflate.worker-*.js", route => {
      workerRequests.push(route.request().url());
      return route.abort("accessdenied");
    }),
  });
  try {
    await dev.page.getByTestId("terminal").waitFor();
    // visible 출력의 행 경계를 넘지 않는 고유 표지로 실제 셸 계산 결과를 기다린다.
    const marker = `wf-${label[0]}-${width}-${failure[0]}`;
    aaa("pane", "run", "t1", `echo ${marker}`);
    await domIs(dev.page, "Worker 실패 뒤 실제 터미널 출력", text => document.querySelector(".xterm-rows")?.textContent.includes(text), marker);
    await dev.page.getByTestId("terminal").click();
    await dev.page.keyboard.type(`echo input-$((40+2))-${marker}`);
    await dev.page.keyboard.press("Enter");
    await waitOutput("t1", `input-42-${marker}`);
    await domIs(dev.page, "Worker 실패 뒤 키 입력 출력", text => document.querySelector(".xterm-rows")?.textContent.includes(text), `input-42-${marker}`);
    assert.equal(workerRequests.length, 0, "접근 거부·404가 가능한 외부 Worker 자산을 요청하지 않는다");
    assert.equal(sockets.length, 1, "연결을 한 번만 연다");
    assert.equal(new URL(sockets[0]).searchParams.has("compression"), failure === "access");
    assert.equal(diagnostics.length, failure === "access" ? 0 : 1, "Worker 실패 원인을 진단 로그에 남긴다");
    assert.deepEqual(dev.errors, [], `${label}/${width}/${failure}: Worker 오류를 처리한다`);
    console.log("terminal-worker-load", JSON.stringify({ label, width, failure, workerRequests: workerRequests.length, sockets: sockets.length, gzip: failure === "access", diagnostics: diagnostics.length }));
  } catch (error) {
    console.error("terminal-worker-failure", JSON.stringify({ label, width, failure, workerRequests, sockets, diagnostics, messages, errors: dev.errors }));
    throw error;
  } finally { await dev.page.context().close(); }
}

async function compressedHistory(name, engine, width) {
  const paneName = `compressed-${name}`;
  aaa("tab", "create", "--machine", "labA", "--label", paneName, "--", process.execPath, corpusFixture);
  await waitOutput(paneName, "FIXTURE_READY>");
  const pane = aaa("pane", "list").panes.find((item) => item.name === paneName).id;
  let expectedKeys = "A";
  const stats = { pane, payloads: [], delivered: [], acks: [], inputs: [] };
  const dev = await pairPage(engine, aaa("web", "pair").code, { width, height: 844 }, {
    serviceWorkers: "block", init: observeWorker,
    wsRoute: [/\/api\/v1\/panes\//, routeWire(stats)],
  });
  const page = dev.page;
  try {
    await inList(page, `pane-${paneName}`);
    await domIs(page, "작은 준비 화면", () => document.querySelector(".xterm-rows")?.textContent.includes("FIXTURE_READY>"));
    await page.getByTestId("terminal").click();
    await domIs(page, "기기 크기 적용", () => !document.querySelector(".term-scroll[data-adopted-grid]"));
    stats.payloads = []; stats.acks = []; stats.delivered = []; stats.inputs = [];
    let releaseConnect;
    stats.beforeConnect = new Promise((resolve) => { releaseConnect = resolve; });
    stats.disconnect();
    aaa("pane", "send-text", paneName, "l");
    try { await waitOutput(paneName, "COMPRESS_READY>"); }
    finally { stats.beforeConnect = null; releaseConnect(); }
    await domIs(page, "Worker가 snapshot 해제를 붙잡음", () => window.__inflateHeld === true);
    const snapshot = stats.payloads.find((item) => item.kind === "snapshot");
    assert.equal(snapshot.compression, "gzip");
    assert.equal(stats.acks.length, 0, "해제 전에는 ACK하지 않는다");
    assert.equal((await screenText(page)).includes("COMPRESS_READY>"), false);
    aaa("pane", "send-text", paneName, expectedKeys);
    await waitOutput(paneName, `KEY:${expectedKeys}`);
    await page.evaluate(() => window.__releaseInflate());
    await domIs(page, "snapshot 뒤 live 출력까지 표시", (keys) => document.querySelector(".xterm-rows")?.textContent.includes(`KEY:${keys}`), expectedKeys);
    // xterm 완료 callback이 ACK를 보낸다. 다음 실제 PTY 출력을 울타리로 쓴다.
    await page.getByTestId("terminal").click();
    await page.keyboard.type("xyz");
    expectedKeys += "xyz";
    await waitOutput(paneName, `KEY:${expectedKeys}`);
    await domIs(page, "입력 출력 표시", (keys) => document.querySelector(".xterm-rows")?.textContent.includes(`KEY:${keys}`), expectedKeys);
    assert(stats.acks.some((ack) => ack.stream === snapshot.stream && ack.offset >= snapshot.raw));
    assert(stats.inputs.length >= 1);
    // 줄 접힘과 색을 실제 DOM에서 확인한다.
    assert(await page.locator('.xterm-rows .xterm-fg-1, .xterm-rows span[style*="color"]').count() > 0);
    const term = page.getByTestId("terminal");
    for (let pageNo = 1; pageNo <= 2; pageNo++) {
      await term.evaluate((el) => { const vp = el.querySelector(".xterm-viewport"); vp.scrollTop = 0; vp.dispatchEvent(new Event("scroll")); });
      await domIs(page, "앞 기록 추가와 그린 위치 복원", (expected) => {
        const el = document.querySelector('[data-testid="terminal"]');
        return Number(el?.dataset.historyApplied) >= expected && Number(el?.dataset.viewTop) > 0;
      }, pageNo);
    }
    assert(stats.payloads.filter((item) => item.kind === "history" && item.compression === "gzip").length >= 2);
    await shot(page, `compression-${name}`);
    stats.disconnect();
    await domIs(page, "재연결 snapshot 표시", () => window.__aaaTermLog.filter((item) => item.e?.startsWith("snapshot")).length >= 2 && document.querySelector(".xterm-rows")?.textContent.includes("KEY:"));
    if (dev.errors.length) throw new Error(dev.errors.join("; "));
  } catch (error) {
    console.error("압축 대상 진단", JSON.stringify({ stats, terminal: await termDiag(page) }));
    throw error;
  } finally { await page.context().close(); }
}

async function networkSample(sample) {
  const result = [];
  const paneName = `network-${sample}`;
  aaa("tab", "create", "--machine", "labA", "--label", paneName, "--", process.execPath, corpusFixture, sample);
  await waitOutput(paneName, "FIXTURE_READY>");
  const pane = aaa("pane", "list").panes.find((item) => item.name === paneName).id;
  // 먼저 이 기기 크기를 건 뒤 기록을 만든다. 비교 쌍의 reflow·원시 snapshot이 같다.
  const prep = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 844 }, { serviceWorkers: "block" });
  await inList(prep.page, `pane-${paneName}`);
  await domIs(prep.page, "회선 표본 준비", () => document.querySelector(".xterm-rows")?.textContent.includes("FIXTURE_READY>"));
  await prep.page.getByTestId("terminal").click();
  await domIs(prep.page, "표본 grid 적용", () => !document.querySelector(".term-scroll[data-adopted-grid]"));
  await prep.page.context().close();
  aaa("pane", "send-text", paneName, "l");
  await waitOutput(paneName, "COMPRESS_READY>");
  let keys = "";
  // 세 회선의 전송 시간/FIFO는 rateLimitedWire.test의 주입 시계가 맡는다.
  // 이 연결은 실제 codec·raw offset·입력·xterm ACK 경계를 검사한다.
  const bps = 1e9, rtt = 1;

  for (const compressed of [false, true]) {
    const stats = { pane, payloads: [], delivered: [], acks: [], inputs: [] };
    const acknowledged = new Promise((resolve) => { stats.onAck = resolve; });
    const dev = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 844 }, {
      serviceWorkers: "block", ...(compressed ? {} : { init: identityPeer }),
      wsRoute: [/\/api\/v1\/panes\//, routeWire(stats, bps, rtt)],
    });
    try {
      await inList(dev.page, `pane-${paneName}`);
      await domIs(dev.page, "제한 회선 snapshot 표시", () => document.querySelector(".xterm-rows")?.textContent.includes("COMPRESS_READY>") || document.querySelector(".xterm-rows")?.textContent.includes("KEY:"));
      await acknowledged;
      const displayedMs = stats.acks[0].at - stats.started;
      const snapshot = stats.payloads.find((item) => item.kind === "snapshot");
      assert.equal(snapshot.compression, compressed ? "gzip" : null);
      assert(stats.acks.some((ack) => ack.stream === snapshot.stream && ack.offset >= snapshot.raw));
      await dev.page.getByTestId("terminal").click();
      stats.holdHistory = true;
      const historyReady = new Promise(resolve => { stats.onHistory = resolve; });
      await dev.page.getByTestId("terminal").evaluate((el) => { const vp = el.querySelector(".xterm-viewport"); vp.scrollTop = 0; vp.dispatchEvent(new Event("scroll")); });
      await inTime(historyReady, 20_000, "실제 history 전송 gate");
      const beforeHistory = await dev.page.getByTestId("terminal").getAttribute("data-history-applied");
      keys += "abcdefgh";
      stats.expectedEcho = `KEY:${keys}`;
      let echoDeadline;
      const applied = new Promise((resolve, reject) => {
        echoDeadline = setTimeout(() => reject(new Error("입력 echo의 xterm ACK가 20초 안에 오지 않았다")), 20_000);
        stats.onAck = () => {
          if (stats.echoOffset !== undefined && stats.acks.some((ack) => ack.stream === snapshot.stream && ack.offset >= snapshot.raw + stats.echoOffset)) { resolve(performance.now()); }
        };
      });
      const inputStarted = performance.now();
      const [inputPtyMs, appliedAt] = await Promise.all([
        dev.page.keyboard.type("abcdefgh").then(() => waitOutput(paneName, `KEY:${keys}`)).then(async () => {
          // 브라우저→WS→실제 PTY가 history 정체 중에도 진행해야 한다.
          assert.equal(await dev.page.getByTestId("terminal").getAttribute("data-history-applied"), beforeHistory);
          const elapsed = performance.now() - inputStarted;
          stats.releaseHistory();
          return elapsed;
        }),
        applied,
      ]).finally(() => { stats.releaseHistory(); clearTimeout(echoDeadline); stats.onAck = undefined; });
      const inputAppliedAckMs = appliedAt - inputStarted;
      assert.equal(stats.inputs.reduce((sum, input) => sum + input.bytes, 0), 8);
      assert.equal(stats.inputs.map((input) => input.data).join(""), "abcdefgh");
      await domIs(dev.page, "제한 회선 history prepend 완료", () => Number(document.querySelector('[data-testid="terminal"]')?.dataset.historyApplied) >= 1);
      const history = stats.payloads.find((item) => item.kind === "history");
      assert.equal(history.compression, compressed ? "gzip" : null);
      result.push({ sample, bps, rtt, compressed, wire: snapshot.wire, raw: snapshot.raw, displayedMs, historyWire: history.wire, historyRaw: history.raw, inputPtyMs, inputAppliedAckMs });
    } catch (error) {
      console.error("회선 대상 진단", JSON.stringify({ sample, bps, rtt, compressed, keys, stats, terminal: await termDiag(dev.page) }));
      throw error;
    } finally { await dev.page.context().close(); }
  }
  fs.writeFileSync(`${root}/terminal-network-${sample}.json`, JSON.stringify(result, null, 2));
  console.log(`터미널 codec/ACK 경계 ${JSON.stringify(result)}`);
}

export default async function ({ page }) {
  await step("압축 실험실 준비: Worker 정책·실제 PTY corpus", async () => {
    const response = await page.request.get(base);
    const policy = response.headers()["content-security-policy"].split(";").map(directive => directive.trim());
    assert.ok(policy.includes("worker-src 'self' blob:"));
    assert.ok(policy.includes("script-src 'self'"), "앱 스크립트 정책은 그대로 유지한다");
    const fixture = `${root}/compression.cjs`;
    corpusFixture = fixture;
    fs.writeFileSync(fixture, `process.stdin.setRawMode(true); process.stdin.resume();
    let text = ""; const mode=process.argv[2]||"mixed";
    for(let i=1;i<=6200;i++) {
     let line="c-"+String(i).padStart(5,"0")+" ";
     if(mode==="plain") line+="INFO worker="+(i%8)+" item="+(i*7919).toString(16)+" bytes="+(i%8000)+" done "+"x".repeat(40);
     else if(mode==="ansi") { for(let j=0;j<12;j++)line+="\\x1b[38;2;"+(i*j%256)+";"+(i%256)+";"+(j*19)+"m"+(i*7919+j).toString(16)+" "; line+="\\x1b[m"; }
     else if(mode==="korean") line+="작업 "+i+": 한글 파일 검사 완료 결과="+(i*7919).toString(16)+" 줄 접힘 확인";
     else line+="\\x1b[31m한글 색상\\x1b[m "+String(i*7919)+" 줄 접힘 확인 "+"x".repeat(85);
     text+=line+"\\r\\n";
    }
    process.stdout.write("FIXTURE_READY>\\r\\n");
    let keys = ""; process.stdin.on("data", data => {
     if(data.toString()==="l") { process.stdout.write(text+"COMPRESS_READY>\\r\\n"); return; }
     keys += data.toString(); process.stdout.write("KEY:"+keys+"\\r\\n"); });`);
  }, { always: true });
  try {
  await step("압축 Worker 로드: chromium 1280 access", () => workerLoad(browser, "chromium", 1280, "access"));
  await step("압축 Worker 로드: chromium 1280 constructor", () => workerLoad(browser, "chromium", 1280, "constructor"));
  await step("압축 Worker 로드: chromium 1280 runtime", () => workerLoad(browser, "chromium", 1280, "runtime"));
  await step("압축 Worker 로드: webkit 390 access", async () => workerLoad(await safariBrowser(), "webkit", 390, "access"));
  await step("압축 Worker 로드: webkit 390 constructor", async () => workerLoad(await safariBrowser(), "webkit", 390, "constructor"));
  await step("압축 Worker 로드: webkit 390 runtime", async () => workerLoad(await safariBrowser(), "webkit", 390, "runtime"));
  await step("압축 기록: chromium 1280 Worker·ACK·입력·재연결·두 쪽 prepend", async () => compressedHistory("chromium-1280", browser, 1280));
  await step("압축 기록: webkit 390 Worker·ACK·입력·재연결·두 쪽 prepend", async () => compressedHistory("webkit-390", await safariBrowser(), 390));
  await step("압축 회선 경계: korean 실제 PTY의 identity/gzip·입력 8키·최종 ACK", () => networkSample("korean"));
  } finally { await safari?.close(); }
}

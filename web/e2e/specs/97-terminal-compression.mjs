// 실제 PTY·host·Worker·xterm. 전송 계층만 회선 속도로 제한하며 ACK/DOM 사건으로 끝낸다.
import assert from "node:assert/strict";
import fs from "node:fs";
import { gunzipSync } from "node:zlib";
import { performance } from "node:perf_hooks";
import { RateLimitedWire } from "../lib/rate-limited-wire.mjs";
import { aaa, root, step, browser, webkit, pairPage, inList, waitOutput, domIs, screenText, shot, termDiag } from "../lib/harness.mjs";

function observeWorker() {
  window.__inflateHeld = false;
  window.__inflateReleased = false;
  const Native = window.Worker;
  window.Worker = class extends Native {
    constructor(url, options) { super(url, options); this.terminalPayload = String(url).includes("terminalInflate"); }
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
  return (socket) => {
    const server = socket.connectToServer();
    if (stats.pane && !socket.url().includes(encodeURIComponent(stats.pane))) return;
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
      downstream.push(raw, size);
    });
    socket.onMessage((raw) => upstream.push(raw, typeof raw === "string" ? Buffer.byteLength(raw) : raw.length));
    const clear = () => { downstream.close(); upstream.close(); };
    server.onClose((code, reason) => { clear(); socket.close({ code, reason }); });
    socket.onClose((code, reason) => { clear(); server.close({ code, reason }); });
    stats.disconnect = () => { clear(); socket.close({ code: 1011, reason: "test reconnect" }); server.close(); };
  };
}

export default async function () {
  let corpusFixture;
  await step("압축 기록: 실제 Worker gate·ACK 원시 길이·색상/한글·입력·재연결·두 쪽 prepend와 위치 보존", async () => {
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
    const wk = await webkit.launch();
    try {
      for (const [name, engine, width] of [["chromium-desktop", browser, 1280], ["chromium-phone", browser, 390], ["webkit-desktop", wk, 1280], ["webkit-phone", wk, 390]]) {
        const paneName = `compressed-${name}`;
        aaa("tab", "create", "--machine", "labA", "--label", paneName, "--", process.execPath, fixture);
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
          stats.disconnect();
          aaa("pane", "send-text", paneName, "l");
          await waitOutput(paneName, "COMPRESS_READY>");
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
    } finally { await wk.close(); }
  });

  await step("압축 회선 실측: 같은 PTY의 identity/gzip·1 Gbit/s/5 Mbit/s/1 Mbit/s·최종 xterm ACK", async () => {
    const result = [];
    for (const sample of ["plain", "ansi", "korean"]) {
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
      for (const [bps, rtt] of [[1e9, 1], [5e6, 50], [1e6, 100]]) {
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
            await dev.page.getByTestId("terminal").evaluate((el) => { const vp = el.querySelector(".xterm-viewport"); vp.scrollTop = 0; vp.dispatchEvent(new Event("scroll")); });
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
              dev.page.keyboard.type("abcdefgh").then(() => waitOutput(paneName, `KEY:${keys}`)).then(() => performance.now() - inputStarted),
              applied,
            ]).finally(() => { clearTimeout(echoDeadline); stats.onAck = undefined; });
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
      }
    }
    fs.writeFileSync(`${root}/terminal-network.json`, JSON.stringify(result, null, 2));
    console.log(`터미널 회선 실측 ${JSON.stringify(result)}`);
  });
}

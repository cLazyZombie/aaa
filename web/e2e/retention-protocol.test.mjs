// 실제 Playwright 1.64.1/Chromium 경계의 짧은 관측 대조. host·운영 페이지는 열지 않는다.
// NODE_PATH=<격리 node_modules> nice -n 15 node --test web/e2e/retention-protocol.test.mjs
import assert from "node:assert/strict";
import fs from "node:fs";
import { createRequire } from "node:module";
import os from "node:os";
import path from "node:path";
import { it } from "node:test";
import { launchChromium } from "./lib/chromium-launch.mjs";
import { retentionProtocol } from "./lib/retention-protocol.mjs";
import { retentionFailure } from "./lib/retention-failure.mjs";

const require = createRequire(path.join(process.env.NODE_PATH ?? process.cwd(), "noop.js"));
const { chromium } = require("playwright");
assert.equal(require("playwright/package.json").version, "1.64.1");

async function lab(t, mode) {
  const directory = process.env.AAA_CHAT_DIAGNOSTICS
    ? path.join(process.env.AAA_CHAT_DIAGNOSTICS, mode)
    : fs.mkdtempSync(path.join(os.tmpdir(), "aaa-protocol-browser-"));
  if (!process.env.AAA_CHAT_DIAGNOSTICS) t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  const browser = await launchChromium(chromium);
  t.after(() => browser.close());
  const page = await browser.newPage();
  await page.setContent('<div data-testid="chat" aria-busy="false"><span class="chat-row">synthetic row</span></div>');
  const observer = retentionProtocol(page, directory);
  const context = { engine: "chromium", width: 1280, role: "coordinator", trial: 2, pageNo: 4, phase: "lookup-before-state" };
  observer.at(context);
  const read = () => fs.readFileSync(path.join(directory, "retention-protocol.jsonl"), "utf8")
    .trim().split("\n").map(line => JSON.parse(line));
  return { page, observer, context, read };
}

it("실제 locator 조회·adoption·함수 반환의 DOM/Runtime 원문을 연결한다", { timeout: 5000 }, async t => {
  const { page, observer, read } = await lab(t, "normal");
  assert.equal(await page.getByTestId("chat").evaluate(el => el.querySelectorAll(".chat-row").length), 1);
  observer.at(null);
  const rows = read(), sends = rows.filter(r => r.event === "send");
  for (const method of ["DOM.describeNode", "DOM.resolveNode"]) {
    const send = sends.find(r => r.method === method);
    assert.ok(send, method);
    const reply = rows.find(r => r.event === "receive" && r.id === send.id);
    assert.ok(reply, `${method} response`);
    if (method === "DOM.describeNode") assert.ok(reply.protocol.result.node.backendNodeId);
    else assert.ok(reply.protocol.result.object.objectId);
  }
  assert.ok(sends.some(r => r.expression === "lookup-element"));
  assert.ok(sends.some(r => r.protocol.params?.arguments?.some(arg => arg.value?.includes?.('querySelectorAll(".chat-row")'))));
  assert.equal(rows.filter(r => r.event === "error").length, 0);
});

// 필요한 이유: 실제 transport 응답이 멈춰도 최초 실패 자료를 추가 evaluate보다 먼저 남겨야 한다.
// 응답 보류는 원인 재현이 아닌 통제 대조다. 정규 E103에는 보류나 우회 코드를 넣지 않는다.
for (const target of ["lookup-element", "DOM.resolveNode"]) {
  it(`실제 ${target} 응답 보류 중 실패 snapshot·살아 있는 페이지·종료 미응답을 구별한다`, { timeout: 5000 }, async t => {
    const { page, observer, context, read } = await lab(t, target.replaceAll(".", "-"));
    const client = page._connection.toImpl(page).delegate._mainFrameSession._client;
    const connection = client._connection, session = client._sessionId;
    const sender = connection._transport.send, receiver = connection._transport.onmessage;
    const held = Promise.withResolvers();
    let id, reply;
    connection._transport.send = message => {
      const selected = target === "lookup-element"
        ? message.method === "Runtime.callFunctionOn" && message.params?.arguments?.some(arg => arg.value === "(r) => r.element")
        : message.method === target;
      if (message.sessionId === session && selected && id === undefined) id = message.id;
      return sender.call(connection._transport, message);
    };
    connection._transport.onmessage = message => {
      if (id !== undefined && message.sessionId === session && message.id === id) {
        reply = message;
        held.resolve();
      } else return receiver(message);
    };
    const restore = () => {
      connection._transport.onmessage = receiver;
      connection._transport.send = sender;
    };
    t.after(restore);
    const evaluation = page.getByTestId("chat").evaluate(el => {
      window.__aaaRetentionEvaluation = "entered";
      return el.textContent;
    }).then(value => ({ value }), error => ({ error }));
    await held.promise;
    assert.ok(reply.result, "실제 browser 응답은 이미 도착했다");
    const original = new Error("controlled failure boundary");
    const records = [];
    await assert.rejects(retentionFailure(original, context, [], row => records.push(row), async () => {
      const saved = read();
      assert.equal(saved.find(r => r.event === "failure-boundary").phase, "lookup-before-state");
      assert.ok(saved.some(r => r.event === "pending-at-failure" && r.id === id));
      // recorder는 driver에 도착하지 않은 응답을 받았다고 기록하면 안 된다.
      assert.equal(saved.some(r => r.event === "receive" && r.id === id), false);
      context.phase = "diagnostic";
      return page.evaluate(() => ({ entered: window.__aaaRetentionEvaluation ?? null, rows: document.querySelectorAll(".chat-row").length }));
    }, () => observer.failed()), error => error === original);
    assert.deepEqual(records.at(-1).state, { entered: null, rows: 1 });
    // 기존 locator의 시한을 바꾸지 않는다. 실제 close 사건으로 즉시 끝낸다.
    await page.close();
    const result = await evaluation;
    assert.match(result.error.message, /closed/);
    const pending = read().find(r => r.event === "pending-at-close" && r.id === id);
    assert.ok(pending);
    assert.equal(pending.protocol.id, id);
    assert.equal(read().some(r => r.event === "send" && r.phase === "diagnostic"), true);
    restore();
  });
}

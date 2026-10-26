import assert from "node:assert/strict";
import fs from "node:fs";
import { createHash } from "node:crypto";
import { root, aaa, until, pairPage, waitOutput } from "./harness.mjs";
import { responseGate } from "./bulk-actions.mjs";

// 이전 답은 실제 host가 처리한다. HTTP 전달만 새 보고의 입력 뒤로 미룬다.
// 새 질문을 가짜 응답으로 만들거나 입력을 다시 채워 경합을 덮지 않는다.
export async function questionReplyScenario(engine, width) {
  const label = `${engine.browserType().name()}-${width}`;
  const trace = [];
  if (!aaa("project", "list").projects.some(p => p.name === "demo")) aaa("project", "create", "demo");
  fs.writeFileSync(`${root}/reply-task.md`, "늦은 질문 답 응답 회귀\n");
  const t = aaa("thread", "start", "demo", "--title", "직접 답 응답 순서", "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/reply-task.md`);
  await until("첫 보고", () => aaa("thread", "show", t.id).report_hash);
  const gate = responseGate();
  const { page, errors } = await pairPage(engine, aaa("web", "pair").code, { width, height: 900 }, {
    hash: `#question=${t.id}`, serviceWorkers: "block", setup: gate.setup,
    init: () => {
      window.__questionReplyTrace = [];
      window.__questionInputInterleavings = [];
      addEventListener("DOMContentLoaded", () => {
        // native input을 root capture가 받은 뒤, bubble onChange 전에 실제 부모
        // 갱신을 끼운다. 입력값·React state·host 응답은 직접 바꾸지 않는다.
        document.getElementById("root").addEventListener("input", event => {
          const element = event.target;
          if (element.dataset?.testid !== "thread-ask-text" || element.value !== "이전 답") return;
          const before = element.value;
          document.querySelector('[data-testid="activity-asks"]').dispatchEvent(new MouseEvent("click", { bubbles: true }));
          window.__questionInputInterleavings.push({ before, after: element.value,
            same: element === document.querySelector('[data-testid="thread-ask-text"]') });
        }, true);
        let previous;
        const observe = () => {
          const node = document.querySelector('.thread-ask-card');
          const state = { at: performance.now(), id: node?.dataset.testid,
            question: node?.querySelector("h3 code")?.textContent,
            input: node?.querySelector("input")?.value,
            disabled: node?.querySelector('[data-testid="thread-ask-send"]')?.disabled };
          const key = JSON.stringify({ ...state, at: 0 });
          if (key !== previous) window.__questionReplyTrace.push(state);
          previous = key;
        };
        new MutationObserver(observe).observe(document, { subtree: true, childList: true, attributes: true });
        document.addEventListener("input", observe, true);
      });
    },
    onPage: p => {
      p.on("websocket", ws => ws.on("framereceived", ({ payload }) => {
        if (!ws.url().endsWith("/api/v1/events")) return;
        const event = JSON.parse(payload);
        if (event.ev === "thread" && event.id === t.id) trace.push({ at: Date.now(), kind: "event",
          updated: event.updated_at_ms, hash: event.report_hash, revision: event.questions?.report_hash,
          questions: event.questions?.items.map(q => q.id), draft: event.questions?.draft });
      }));
      p.on("request", r => {
        if (r.url().endsWith("/api/v1/call") && r.postDataJSON()?.op === "thread_answer")
          trace.push({ at: Date.now(), kind: "request", body: r.postDataJSON() });
      });
    },
  });
  const card = page.getByTestId(`thread-ask-${t.id}`);
  const write = async name => {
    const body = `[결정 필요] 질문 1개: ${name}\n## Questions\n### 1. 진행\n${name}\n- **A 진행\n`;
    fs.writeFileSync(`${t.work_dir}/report.md`, body);
    const hash = createHash("sha256").update(body).digest("hex");
    const v = await until("해당 보고 수집", () => {
      const v = aaa("thread", "show", t.id);
      return v.report_hash === hash && v.questions && v;
    });
    // 비활성 여부만으로 이전 보고의 카드와 구별하지 않는다.
    await until("해당 질문 DOM", async () => (await card.locator("h3 code").textContent()) === v.questions.items[0].id);
    assert.equal(await card.getByRole("radio").count(), 0);
    assert.equal(await card.getByTestId("thread-ask-send").isDisabled(), true);
    return v.questions.report_hash;
  };
  let held;
  try {
    const previous = await write("이전 보고");
    await card.getByTestId("thread-ask-text").fill("이전 답");
    assert.equal(await card.getByTestId("thread-ask-text").inputValue(), "이전 답", "input capture와 bubble 사이 부모 갱신도 직접 답을 보존한다");
    assert.equal(await card.getByTestId("thread-ask-send").isEnabled(), true);
    assert.deepEqual(await page.evaluate(() => window.__questionInputInterleavings),
      [{ before: "이전 답", after: "이전 답", same: true }], "같은 입력칸에서 실제 부모 갱신을 한 번 검사한다");
    held = gate.arm("thread_answer");
    // 응답 시한과 실제 누름을 함께 관측한다. 누름이 진행 중이어도 거부를 즉시 처리해
    // 진단·실험실 정리 전에 unhandled rejection으로 프로세스가 끝나지 않게 한다.
    await Promise.all([card.getByTestId("thread-ask-send").click(), held.arrived]);
    await waitOutput(t.pane, "직접 쓴 답: 이전 답");
    const current = await write("새 보고");
    const text = "선택지가 없어도 진행";
    await card.getByTestId("thread-ask-text").fill(text);
    await card.evaluate(node => {
      window.__questionReply = { node, detached: false };
      const observer = new MutationObserver(() => {
        if (!node.isConnected) window.__questionReply.detached = true;
      });
      observer.observe(document, { subtree: true, childList: true });
      window.__questionReply.stop = () => observer.disconnect();
    });
    const finished = page.waitForEvent("requestfinished", {
      predicate: r => r.url().endsWith("/api/v1/call") && r.postDataJSON()?.op === "thread_answer",
    });
    held.release();
    await finished;
    // 본문 소비 뒤의 렌더 사건을 지나고, 실제 최신 thread 사건도 한 번 더 받는다.
    await page.evaluate(() => new Promise(resolve => requestAnimationFrame(resolve)));
    aaa("thread", "ack", t.id);
    await card.waitFor();
    const state = await card.evaluate(node => ({
      same: node === window.__questionReply.node, detached: window.__questionReply.detached,
      input: node.querySelector("input").value,
      disabled: node.querySelector('[data-testid="thread-ask-send"]').disabled,
    }));
    console.log("question-reply", JSON.stringify({ label, previous, current, ...state }));
    assert.deepEqual(state, { same: true, detached: false, input: text, disabled: false }, "이전 답 응답은 새 카드·입력·활성을 보존한다");
    await page.evaluate(() => window.__questionReply.stop());
    await card.getByTestId("thread-ask-send").click();
    await waitOutput(t.pane, `직접 쓴 답: ${text}`);
    await until("새 보고의 직접 답 저장", () => aaa("thread", "show", t.id).questions?.answer?.text === text);
    assert.deepEqual(gate.calls.map(r => [r.report_hash, r.text]), [[previous, "이전 답"], [current, text]], "각 보고에 맞는 답을 정확히 한 번 요청한다");
    assert.deepEqual(errors, []);
  } catch (error) {
    const dir = `${root}/diagnostics`;
    fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(`${dir}/question-reply-${label}.json`, JSON.stringify({
      label, error: String(error), calls: gate.calls, trace,
      dom: await page.evaluate(() => ({
        hash: location.hash, card: document.querySelector('.thread-ask-card')?.outerHTML,
        detached: window.__questionReply?.detached, trace: window.__questionReplyTrace,
        interleavings: window.__questionInputInterleavings,
      })).catch(String),
    }, null, 2));
    throw error;
  } finally {
    held?.release();
    gate.clear();
    await page.context().unrouteAll({ behavior: "wait" });
    await page.context().close();
    aaa("thread", "resolve", t.id);
  }
}

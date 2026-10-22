import test from "node:test";
import assert from "node:assert/strict";
import { setImmediate } from "node:timers/promises";
import { EventEmitter } from "node:events";
import { clickForPopup, observePopupLifecycle } from "../e2e/lib/popup.mjs";

const deferred = () => Promise.withResolvers();

test("popup은 클릭 전에 기다리고 클릭 완료까지 확인한다", async () => {
  const popup = deferred(), click = deferred(), tab = {};
  const events = [];
  const opened = clickForPopup({ waitForEvent: (event) => { events.push(event); return popup.promise; } }, {
    click: () => { events.push("click"); popup.resolve(tab); return click.promise; },
  });
  assert.deepEqual(events, ["popup", "click"]);
  let finished = false;
  opened.then(() => { finished = true; });
  await setImmediate();
  assert.equal(finished, false);
  click.resolve();
  assert.equal(await opened, tab);
});

test("클릭 실패 뒤 브라우저가 닫혀도 원래 오류와 처리된 popup 거부를 유지한다", async () => {
  const popup = deferred(), original = new Error("링크 클릭 실패");
  const events = [];
  const opened = clickForPopup({ waitForEvent: () => popup.promise }, { click: async () => { throw original; } },
    (event, detail) => events.push({ event, ...detail }));
  await assert.rejects(opened, (error) => error === original);
  popup.reject(new Error("context가 닫힘"));
  // Node test runner가 이 사건 뒤의 unhandledRejection도 실패로 처리한다.
  await setImmediate();
  assert.deepEqual(events, [
    { event: "popup.click", status: "rejected", error: "링크 클릭 실패" },
    { event: "popup.wait", status: "rejected", error: "context가 닫힘" },
  ]);
});

test("popup 실패 뒤 늦게 실패한 클릭도 처리한다", async () => {
  const click = deferred(), original = new Error("popup 대기 실패");
  const events = [];
  const opened = clickForPopup({ waitForEvent: async () => { throw original; } }, { click: () => click.promise },
    (event, detail) => events.push({ event, ...detail }));
  await assert.rejects(opened, (error) => error === original);
  click.reject(new Error("늦은 클릭 실패"));
  await setImmediate();
  assert.deepEqual(events, [
    { event: "popup.wait", status: "rejected", error: "popup 대기 실패" },
    { event: "popup.click", status: "rejected", error: "늦은 클릭 실패" },
  ]);
});

test("renderer와 context·browser 종료 사건의 순서를 보존하고 구독을 해제한다", () => {
  const page = new EventEmitter(), context = new EventEmitter(), browser = new EventEmitter();
  page.context = () => context; context.browser = () => browser;
  const events = [];
  const stop = observePopupLifecycle(page, (event) => events.push(event));
  page.emit("crash"); page.emit("close"); context.emit("close"); browser.emit("disconnected");
  assert.deepEqual(events, ["page.crash", "page.close", "context.close", "browser.disconnected"]);
  stop();
  for (const object of [page, context, browser]) assert.deepEqual(object.eventNames(), []);
});

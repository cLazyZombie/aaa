import test from "node:test";
import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import { observePopupLifecycle } from "../e2e/lib/popup.mjs";

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

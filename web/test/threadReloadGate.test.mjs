import assert from "node:assert/strict";
import { it } from "node:test";
import { ThreadReloadGate } from "../e2e/lib/thread-reload-gate.mjs";

it("reload 전 문서의 늦은 끝 응답·첫 요청은 새 문서의 gate를 끝내지 않는다", () => {
  const gate = new ThreadReloadGate(), final = { kind: "threads", next_after: null };
  assert.equal(gate.holdFinal("old", final), false);
  gate.arm("old");
  assert.equal(gate.holdFinal("old", final), false, "첫 착지 trace: 이전 문서의 마지막 응답이 먼저 도착");
  assert.equal(gate.held, false);
  assert.equal(gate.holdFinal("reload", { kind: "threads", next_after: "more" }), false);
  assert.equal(gate.holdFinal("reload", final), true);
  assert.equal(gate.document, "reload");
  assert.equal(gate.holdFirst("old", null), false);
  assert.equal(gate.holdFirst("reload", "more"), false);
  assert.equal(gate.holdFirst("reload", null), true);
  assert.equal(gate.holdFinal("reload", final), false);
});

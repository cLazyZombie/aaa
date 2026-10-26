import assert from "node:assert/strict";
import { it } from "node:test";
import { retentionFailure } from "../e2e/lib/retention-failure.mjs";

it("추가 진단 실패에도 최초 오류·페이지·직전 상태를 먼저 보존한다", async () => {
  const original = new Error("앞 쪽 commit 실패");
  const context = { role: "coordinator", trial: 0, pageNo: 4, before: 750, beforeState: { rows: 750, busy: "false" } };
  for (const diagnosticFails of [false, true]) {
    const records = [];
    const diagnose = async () => {
      assert.equal(records[0].original.stack, original.stack);
      assert.equal(records[0].pageNo, 4);
      assert.deepEqual(records[0].beforeState, context.beforeState);
      if (diagnosticFails) throw new Error("진단 evaluate 실패");
      return { absent: true };
    };
    await assert.rejects(retentionFailure(original, context, [], r => records.push(r), diagnose), error => error === original);
    assert.equal(records[0].original.message, original.message);
    assert.equal(records[1].stateSource, diagnosticFails ? "before" : "live");
    assert.equal(records[1].pageNo, 4);
  }
});

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

it("protocol snapshot은 추가 evaluate보다 먼저 실행하고 저장 오류도 최초 오류를 바꾸지 않는다", async () => {
  const original = new Error("lookup 실패");
  for (const snapshotFails of [false, true]) {
    const records = [];
    let snapshotted = false;
    await assert.rejects(retentionFailure(original, { phase: "lookup-before-state" }, [], row => records.push(row), async () => {
      assert.equal(snapshotted, true);
      assert.equal(records[0].original.stack, original.stack);
      if (snapshotFails) assert.match(records[1].protocolError, /write failed/);
      return { responsive: true };
    }, () => {
      assert.equal(records[0].event, "failure");
      snapshotted = true;
      if (snapshotFails) throw new Error("write failed");
    }), error => error === original);
    assert.equal(records.at(-1).event, "diagnostic");
  }
});

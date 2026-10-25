import assert from "node:assert/strict";
import { test } from "node:test";
import { runBatches } from "../src/bulkAction.ts";

test("1201개를 host 한도 안에서 한 묶음씩 처리하고 실제 완료만 센다", async () => {
  const items = Array.from({ length: 1201 }, (_, i) => i);
  const batches = [], progress = [];
  let release;
  const gate = new Promise(resolve => { release = resolve; });
  const task = runBatches(items, async batch => {
    batches.push(batch);
    if (batches.length === 1) await gate;
  }, value => progress.push(value));
  assert.equal(batches.length, 1);
  assert.deepEqual(progress, ["처리 중 0/1201"]);
  items.push(9999); // 시작 뒤 생긴 항목을 포함하지 않는다.
  release();
  await task;
  assert.deepEqual(batches.map(b => b.length), [500, 500, 201]);
  assert.deepEqual(batches.flat(), items.slice(0, 1201));
  assert.deepEqual(progress, ["처리 중 0/1201", "처리 중 500/1201", "처리 중 1000/1201", "처리 중 1201/1201"]);
});

test("중간 실패는 원인·완료 수를 남기고 다음 묶음이나 자동 재시도를 보내지 않는다", async () => {
  let calls = 0;
  const progress = [];
  await assert.rejects(runBatches(Array.from({ length: 1201 }), async () => {
    if (++calls === 2) throw new Error("host 연결 끊김");
  }, message => progress.push(message)), /1201개 중 500개 완료 후 중단: host 연결 끊김/);
  assert.equal(calls, 2);
  assert.deepEqual(progress, ["처리 중 0/1201", "처리 중 500/1201"]);
});

test("빈 대상은 쓰기 요청을 보내지 않는다", async () => {
  await runBatches([], async () => assert.fail("빈 요청"), () => {});
});

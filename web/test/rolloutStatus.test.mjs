import assert from "node:assert/strict";
import { test } from "node:test";
import { aaaVersion, machineRollout } from "../src/rolloutStatus.ts";
test("머신 전체 단계의 완료·현재·대기·실패 뒤 미진행을 구별한다", () => {
  const card = { state: "running", cursor: 1, steps: [
    { machine: "mac-a", completed: true }, { machine: "mac-b", completed: false },
    { machine: "mac-b", completed: false }, { machine: "mac-c", completed: false },
  ] };
  assert.deepEqual(machineRollout(card, "mac-a"), { label: "배포 완료", busy: false });
  assert.deepEqual(machineRollout(card, "mac-b"), { label: "배포 중", busy: true });
  assert.deepEqual(machineRollout(card, "mac-c"), { label: "배포 대기", busy: false });
  for (const [state, label, busy] of [["recovering", "복구 중", true], ["stopped", "배포 중단", false], ["stopped_rolled_back", "복구 완료", false], ["recovery_failed", "복구 실패", false]]) {
    assert.deepEqual(machineRollout({ ...card, state }, "mac-b"), { label, busy });
    assert.deepEqual(machineRollout({ ...card, state }, "mac-c"), { label: "미진행", busy: false });
  }
  assert.equal(machineRollout(card, "other"), null);
  assert.equal(machineRollout(null, "mac-a"), null);
});

test("aaa 접두사는 한 번만 표시한다", () => {
  assert.equal(aaaVersion("aaa 0.1.6"), "aaa 0.1.6");
  assert.equal(aaaVersion("0.1.6+demo"), "aaa 0.1.6+demo");
});

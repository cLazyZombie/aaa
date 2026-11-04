import assert from "node:assert/strict";
import { test } from "node:test";
import { usagePlanName } from "../src/usagePlan.ts";

// 필요: 요금제 칸 넘침의 원인인 월 금액만 빼고 이름·인증 상태와 감지 원문은 남는지 지킨다.
test("구독 사용량 요금제 칸은 월 금액만 뺀다", () => {
  assert.equal(usagePlanName("Max · $100/월"), "Max");
  assert.equal(usagePlanName("Ultra · $200/월 · 인증됨"), "Ultra · 인증됨");
  assert.equal(usagePlanName("Pro · $500/월"), "Pro");
  assert.equal(usagePlanName("Team Premium"), "Team Premium");
  assert.equal(usagePlanName("max"), "max");
});

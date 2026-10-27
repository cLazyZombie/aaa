import assert from "node:assert/strict";
import { test } from "node:test";
import { withRecommended } from "../src/questionAnswer.ts";

test("직접 답은 자기 질문만 채우고 다른 미응답은 추천 또는 미완료로 남긴다", () => {
  const q = { items: [{ choices: [{ recommended: true }] }, { choices: [{ recommended: false }] }] };
  assert.equal(withRecommended(q, [null, null], ["첫 질문 의견", null]), null);
  assert.deepEqual(withRecommended(q, [null, null], [null, "두번째 직접 답"]), [0, null]);
  assert.deepEqual(withRecommended(q, [0, null], ["선택과 의견", "둘째\n여러 줄"]), [0, null]);
  assert.equal(withRecommended(q, [0, null], [null, " \n "]), null);
  assert.deepEqual(withRecommended(q, [0, 0]), [0, 0]);
});

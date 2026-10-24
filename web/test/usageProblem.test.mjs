import assert from "node:assert/strict";
import { test } from "node:test";
import { usageProblemLabel, usageProblemText } from "../src/usageProblem.ts";

test("Claude 수집 실패는 빈 값 대신 원인을 표시하고 재로그인을 단정하지 않는다", () => {
  for (const problem of ["expired", "rate_limited", "locked", "unobserved", "stale", "disconnected", "unsupported", "denied", "failed"]) {
    const usage = { id: "claude", problem };
    assert.ok(usageProblemLabel(usage).length > 0);
    assert.ok(usageProblemText(usage).length > 0);
    assert.ok(!usageProblemText(usage).includes("다시 로그인"));
  }
  assert.equal(usageProblemLabel({ problem: null }), "한도 없음");
  assert.equal(usageProblemText({ problem: null }), null);
  assert.match(usageProblemText({ id: "claude", problem: "denied" }), /HTTP 403/);
  assert.match(usageProblemText({ id: "claude", problem: "unobserved" }), /상태줄/);
});

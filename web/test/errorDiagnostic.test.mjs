import assert from "node:assert/strict";
import { test } from "node:test";
import { ApiError } from "../src/apiOutcome.ts";
import { diagnosticReason, ignoredFailure } from "../src/errorDiagnostic.ts";

test("공개 서버 원인을 보존하고 브라우저·파서 원문과 임의 name은 진단에 넣지 않는다", () => {
  assert.equal(diagnosticReason(new ApiError({ code: "unavailable", message: "파일 읽기: os error 5" })), "unavailable: 파일 읽기: os error 5");
  assert.equal(diagnosticReason(new SyntaxError("private-json-body")), "SyntaxError");
  const error = new Error("private-message"); error.name = "private-error-name";
  assert.equal(diagnosticReason(error), "브라우저 작업 오류");
});

test("의도적 복구는 같은 값을 반환하며 원문 없이 한 번 기록한다", () => {
  const original = console.debug, events = [];
  console.debug = (...args) => events.push(args);
  try {
    const recover = ignoredFailure("선택 자료", () => 7);
    assert.equal(recover(new TypeError("private-response")), 7);
    assert.deepEqual(events, [["선택 자료: 기존 복구·기본값 사용", "TypeError"]]);
  } finally { console.debug = original; }
});

import assert from "node:assert/strict";
import test from "node:test";
import { receivedInput } from "../e2e/lib/received-input.mjs";

const down = "\x1b[<0;40;11M";
const up = "\x1b[<0;40;11m";
const expected = down + up + "z";

test("마우스 해제와 z가 한 GOT 조각에 합쳐져도 전체 입력 순서를 확인한다", () => {
  const events = [[1, "GOT", down], [2, "SIZE", 89], [3, "GOT", up + "z"]];
  // 구판의 단일 GOT === z 대조는 같은 정상 byte 흐름을 놓친다.
  assert.equal(events.some(([, kind, value]) => kind === "GOT" && value === "z"), false);
  assert.equal(receivedInput(events), expected);
});

test("CSI 중간 분할과 모든 read 경계에서 같은 정확한 입력을 확인한다", () => {
  for (let split = 0; split <= expected.length; split++) {
    const events = [[1, "GOT", expected.slice(0, split)], [2, "SIZE", 79], [3, "GOT", expected.slice(split)]];
    assert.equal(receivedInput(events), expected, `read 경계 ${split}`);
  }
  const bytes = [...expected].map((value, i) => [i + 1, "GOT", value]);
  assert.equal(receivedInput(bytes), expected);
});

test("해제 누락·z가 먼저 도착·중복 키·추가 bytes는 정상 순서로 인정하지 않는다", () => {
  for (const wrong of [down + "z", down + "z" + up, down + up + "zz", down + up + "xz"]) {
    assert.notEqual(receivedInput([[1, "GOT", wrong]]), expected);
  }
});

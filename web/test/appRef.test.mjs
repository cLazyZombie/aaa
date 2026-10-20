// 앱 안 참조(src/appRef.ts, plan §8.324): coordinator가 쓰는 `aaa:question/<thread>`·`aaa:report/<이름>`을 글에서 골라 칩으로
// 그린다. 대상 글자는 host `notify::app_ref`와 같고, 문장 끝 점·조사·괄호는 참조가 아니다.
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { appRefOf, splitAppRefs } from "../src/appRef.ts";

const q = (target) => ({ kind: "question", target });
const r = (target) => ({ kind: "report", target });

describe("앱 안 참조", () => {
  it("글 속의 참조를 보통 글과 나눈다", () => {
    assert.deepEqual(splitAppRefs("t-0512 질문 2개 · 추천: 1A 2A aaa:question/t-0512"), ["t-0512 질문 2개 · 추천: 1A 2A ", q("t-0512")]);
    assert.deepEqual(splitAppRefs("보고서 aaa:report/ui-v2.html와 aaa:question/t-1."), ["보고서 ", r("ui-v2.html"), "와 ", q("t-1"), "."]);
    assert.deepEqual(splitAppRefs("(aaa:report/demo_1)에"), ["(", r("demo_1"), ")에"]);
    assert.deepEqual(splitAppRefs("aaa:question/t-0001에 답한다"), [q("t-0001"), "에 답한다"]);
  });
  it("참조가 아닌 것은 글 그대로", () => {
    for (const text of ["", "aaa:question/", "aaa:thread/t-1", "xaaa:question/t-1", "https://h/aaa:report/x", "aaa:report/.", "aaa:question/한글"])
      assert.deepEqual(splitAppRefs(text), [text], text);
  });
  it("글 전체가 참조 하나일 때만(backtick·링크 주소)", () => {
    assert.deepEqual(appRefOf(" aaa:question/t-9 "), q("t-9"));
    assert.equal(appRefOf("aaa:question/t-9 보기"), null);
    assert.equal(appRefOf("https://example.ts.net/"), null);
  });
});

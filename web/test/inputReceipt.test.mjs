import assert from "node:assert/strict";
import { it } from "node:test";
import { receiptText, receiptMethod, slashCommand } from "../src/inputReceipt.ts";

it("수신 비교는 같은 id로 글 전체를 감싼 봉투만 걷는다", () => {
  const text = "첫 줄  \n두 번째 줄";
  for (const id of ["7f06", "arbitrary_1"]) {
    const wrapped = `<pasted_content id="${id}">\n${text}\n</pasted_content id="${id}">`;
    assert.equal(receiptText(wrapped), text);
    assert.equal(receiptMethod(text, {role:"user",text:wrapped,pending:false}), "pasted_content");
    assert.equal(receiptMethod(text, {role:"user",text:wrapped,pending:true}), "queue_operation");
  }
  for (const text of [
    '<pasted_content id="a">\nbody\n</pasted_content id="b">',
    'before\n<pasted_content id="a">\nbody\n</pasted_content id="a">',
    '<pasted_content id="a">\nunfinished',
    '<pasted_content id="a">\none\n</pasted_content id="a">\n<pasted_content id="a">\ntwo\n</pasted_content id="a">',
  ]) assert.equal(receiptText(text), text);
  assert.equal(receiptMethod("first\nsecond", {role:"assistant",text:"first\nsecond",pending:false}), null);
  assert.equal(receiptMethod("first \nsecond", {role:"user",text:"first\nsecond",pending:false}), null);
});

it("harness 명령 한 줄만 명령이다(경로·여러 줄·글 중간의 /는 사람 글)", () => {
  for (const t of ["/compact", " /compact ", "/model opus", "/review:pr 12"]) assert.equal(slashCommand(t), true, t);
  for (const t of ["/Users/me/a.txt 봐 줘", "/compact\n그리고 이것도", "compact", "이 /compact는?", "/", "/1"]) assert.equal(slashCommand(t), false, t);
});

import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { it } from "node:test";
import { receiptText, receiptMethod, receiptSeen, receiptHashes, slashCommand } from "../src/inputReceipt.ts";

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

it("새 수신 기록이 생긴 commit부터 낙관적 말을 숨기고 예전 같은 글·다른 해시·미확인은 유지한다", () => {
  const sent = { text: "추가 지시", hash: "new", before: 1 };
  const row = hash => ({ role: "user", text: sent.text, receipt: { sha256: hash, method: "normal" } });
  const previous = [row("new"), row("old"), { role: "assistant", text: sent.text }];
  assert.equal(receiptSeen(sent, previous), false);
  assert.equal(receiptSeen(sent, [...previous, { ...row("new"), pending: true }]), true);
  assert.equal(receiptSeen(sent, [...previous, row("new")]), true);
  assert.equal(receiptSeen({ ...sent, before: null }, [...previous, row("new")]), false);
  assert.equal(receiptSeen(sent, null), false);
  assert.equal(receiptSeen({ text: sent.text, before: 0 }, [{ role: "user", text: sent.text, pending: true }]), true);
});

it("탭을 공백 네 칸으로 기록한 긴 붙여넣기도 전체 해시로 확인한다", async () => {
  const text = "검증\t결과\t경과\n".repeat(500) + "실제 끝";
  const expanded = text.replace(/\t/g, "    ");
  const sha = value => createHash("sha256").update(value).digest("hex");
  const hashes = await receiptHashes(text);
  assert.equal(hashes.hash, sha(text));
  assert.equal(hashes.tabHash, sha(expanded));
  const row = (value, pending = false) => ({ role: "user", text: value.slice(0, 4000) + "…", pending,
    receipt: { sha256: sha(value), method: "pasted_content" } });
  const sent = { text, ...hashes, before: 1 };
  assert.equal(receiptSeen(sent, [row(expanded)]), false, "예전 같은 글은 새 수신이 아니다");
  assert.equal(receiptSeen(sent, [row(expanded), row(expanded.slice(0, -4) + "다른 끝")]), false);
  assert.equal(receiptSeen(sent, [row(expanded), row(expanded, true)]), true);
  assert.equal(receiptSeen(sent, [row(expanded), row(expanded)]), true);
  assert.equal(receiptSeen({ ...sent, before: 0 }, [row(text)]), true, "원래 탭을 보존한 기록도 확인한다");
  assert.equal(receiptSeen({ ...sent, before: null }, [row(expanded)]), false);
  assert.equal((await receiptHashes("탭 없는 글")).tabHash, undefined);
});

it("해시 없는 접속의 짧은 탭 입력은 봉투와 탭 변환만 맞추고 다른 공백은 보존한다", () => {
  const text = "검증\t결과\n다음 줄  끝";
  const wrapped = value => ({ role: "user", text: `<pasted_content id="tabs">\n${value}\n</pasted_content id="tabs">` });
  assert.equal(receiptMethod(text, wrapped(text.replace(/\t/g, "    "))), "pasted_content");
  assert.equal(receiptMethod(text, wrapped(text.replace(/\t/g, "   "))), null);
  assert.equal(receiptMethod(text, wrapped(text.replace(/\t/g, "    ").replace("  끝", " 끝"))), null);
});

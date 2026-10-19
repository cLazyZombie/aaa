// 보낸 글 다시 부르기(src/composerRecall.ts, plan §8.266): t3code `composerPromptHistory.test.ts`(1451e7cb2)의 목록·걸음 시험을 aaa 기록으로.
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { recallablePrompt, recallEntries, recallStep } from "../src/composerRecall.ts";

const msg = (role, text) => ({ role, text });
const entries = recallEntries([msg("user", "first"), msg("assistant", "reply"), msg("user", "second"), msg("user", "third")]);
const backward = (position, currentPrompt) => recallStep({ direction: "backward", entries, position, currentPrompt });
const forward = (position, currentPrompt) => recallStep({ direction: "forward", entries, position, currentPrompt });
const at = (prompt) => entries.find((e) => e.prompt === prompt);
const pos = (prompt) => ({ entryId: at(prompt).id, recalled: prompt });

describe("다시 부를 글", () => {
  it("Ultrathink 머리와 앞뒤 빈칸을 뺀다", () => {
    assert.equal(recallablePrompt("Ultrathink:\nInvestigate this\n"), "Investigate this");
    assert.equal(recallablePrompt("   "), "");
  });
  it("사람 글만, 오래된 것부터", () => {
    assert.deepEqual(entries.map((e) => e.prompt), ["first", "second", "third"]);
  });
  it("연달아 같은 글은 하나, 떨어진 같은 글은 둘 다 남고 자리 id가 다르다", () => {
    const collapsed = recallEntries([msg("user", "same"), msg("user", " same "), msg("user", "other"), msg("user", "same"), msg("user", "")]);
    assert.deepEqual(collapsed.map((e) => e.prompt), ["same", "other", "same"]);
    assert.notEqual(collapsed[0].id, collapsed[2].id);
  });
});

describe("걸음(참고 stepComposerPromptHistory)", () => {
  it("쓰던 글에서는 시작하지 않는다", () => {
    assert.equal(backward(null, "typing"), null);
  });
  it("가장 새 것부터 뒤로", () => {
    const first = backward(null, "");
    assert.deepEqual(first, { position: pos("third"), prompt: "third" });
    assert.equal(backward(first.position, "third")?.prompt, "second");
  });
  it("가장 오래된 글에서는 걷지 않아 커서가 움직인다", () => {
    assert.equal(backward(pos("first"), "first"), null);
  });
  it("앞으로 걷고 가장 새 것 너머는 빈 칸", () => {
    const newer = forward(pos("second"), "second");
    assert.equal(newer?.prompt, "third");
    assert.deepEqual(forward(newer.position, "third"), { position: null, prompt: "" });
  });
  it("고친 글은 새 초안이다", () => {
    assert.equal(backward(pos("third"), "third edited"), null);
    assert.equal(forward(pos("third"), "third edited"), null);
    assert.deepEqual(backward(pos("third"), "")?.position, pos("third"));
  });
  it("부르지 않았으면 ↓는 걷지 않는다", () => {
    assert.equal(forward(null, ""), null);
  });
  it("앞이 잘려도 자리를 id로 따라가고, id가 없으면 같은 글의 마지막 자리", () => {
    const full = recallEntries([msg("user", "zeroth"), msg("user", "A"), msg("user", "B"), msg("user", "A")]);
    const olderA = full[1];
    const trimmed = full.slice(1);
    assert.equal(recallStep({ direction: "backward", entries: full, position: { entryId: olderA.id, recalled: "A" }, currentPrompt: "A" })?.prompt, "zeroth");
    assert.equal(recallStep({ direction: "forward", entries: recallEntries(trimmed.map((e) => msg("user", e.prompt))), position: { entryId: olderA.id, recalled: "A" }, currentPrompt: "A" })?.prompt, "B");
    assert.equal(recallStep({ direction: "forward", entries: full, position: { entryId: "gone", recalled: "not sent" }, currentPrompt: "not sent" }), null);
    assert.equal(recallStep({ direction: "backward", entries: full, position: { entryId: "gone", recalled: "A" }, currentPrompt: "A" })?.prompt, "B");
  });
});

// HTTP 목록과 질문/답 사건의 순서 계약. 새 질문의 실제 DOM·전송 경계는 E2E 11이 본다.
import assert from "node:assert/strict";
import { it } from "node:test";
import { QuestionReload } from "../src/questionReload.ts";

const open = { id: "q1", state: "open", text: "권한 확인" };

it("목록을 읽는 동안 답한 질문은 옛 목록으로 되살아나지 않는다", () => {
  const state = new QuestionReload();
  const read = state.begin();
  state.note({ ...open, state: "answered", answer: "yes" });
  assert.deepEqual([...state.finish(read, [open]).values()], []);
});

it("같은 질문의 바뀐 메뉴는 사건을 따르고, 사건이 없는 질문은 새 목록을 따른다", () => {
  const state = new QuestionReload();
  const read = state.begin();
  const old = { ...open, menu: { id: "old" } };
  const changed = { ...open, menu: { id: "new" } };
  const other = { ...open, id: "q2" };
  state.note(changed);
  assert.deepEqual([...state.finish(read, [old, other]).values()], [changed, other]);
  assert.deepEqual([...state.finish(state.begin(), []).values()], []);
});

for (const obsolete of ["finish", "failed"]) {
  it(`다시 붙은 뒤 옛 요청의 ${obsolete}는 새 요청의 사건을 잃지 않는다`, () => {
    const state = new QuestionReload();
    const old = state.begin();
    const current = state.begin();
    state.note(open);
    if (obsolete === "finish") assert.equal(state.finish(old, []), null);
    else state.failed(old);
    assert.deepEqual(state.finish(current, []), new Map([["q1", open]]));
  });
}

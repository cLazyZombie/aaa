import assert from "node:assert/strict";
import { it } from "node:test";
import { ThreadReplies } from "../src/threadReplies.ts";

const old = { project: "demo", id: "t-0001", report_hash: "old", questions: { answer: null } };
const answered = { ...old, questions: { answer: { text: "이전 답" } } };
const fresh = { ...old, report_hash: "new" };

it("새 사건이 없으면 명령의 답 응답을 반영한다", () => {
  const state = new ThreadReplies();
  const reply = state.begin(old);
  assert.equal(reply.finish(answered), answered);
});

it("답 응답 전에 받은 새 보고와 뒤따른 상태를 보존한다", () => {
  const state = new ThreadReplies();
  const reply = state.begin(old);
  state.note(fresh);
  const copied = { ...fresh, copy: "complete" };
  state.note(copied);
  assert.equal(reply.finish(answered), copied);
});

it("같은 번호의 다른 프로젝트와 다른 thread 사건은 답 응답을 막지 않는다", () => {
  const state = new ThreadReplies();
  const reply = state.begin(old);
  state.note({ ...fresh, project: "other" });
  state.note({ ...fresh, id: "t-0002" });
  assert.equal(reply.finish(answered), answered);
});

it("겹친 요청은 각자 시작 뒤 받은 사건을 따르고 끝난 요청은 분리한다", () => {
  const state = new ThreadReplies();
  const first = state.begin(old);
  state.note(fresh);
  const second = state.begin(fresh);
  assert.equal(first.finish(answered), fresh);
  const currentAnswer = { ...answered, report_hash: "new" };
  assert.equal(second.finish(currentAnswer), currentAnswer);
  state.note(old);
  assert.equal(first.finish(answered), fresh);
});

it("실패한 요청을 해제하면 뒤의 사건과 새 요청에 남지 않는다", () => {
  const state = new ThreadReplies();
  const cancelled = state.begin(old);
  cancelled.cancel();
  state.note(fresh);
  assert.equal(cancelled.finish(answered), answered);
  assert.equal(state.begin(old).finish(answered), answered);
});

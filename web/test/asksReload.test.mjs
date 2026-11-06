import assert from "node:assert/strict";
import { it } from "node:test";
import { AsksReload } from "../src/asksReload.ts";
import { answerIndex, mergeAskPage, openAsks, questionKey, questionRefLook } from "../src/asksModel.ts";

const deferred = () => Promise.withResolvers();
it("첫 사건은 한 읽기이고, 읽는 동안의 여러 질문·답 사건은 끝의 한 읽기로 모인다", async () => {
  const first = deferred(), next = deferred(), started = deferred(), applied = [];
  let calls = 0;
  const read = new AsksReload(() => { if (++calls === 1) return first.promise; started.resolve(); return next.promise; }, r => applied.push(r), e => { throw e; });
  const done = read.refresh();
  assert.equal(calls, 1);
  read.refresh(); read.refresh();
  assert.equal(calls, 1);
  first.resolve("옛 질문");
  await started.promise;
  assert.deepEqual(applied, [], "사건 전 목록을 공개하지 않는다");
  next.resolve("새 질문과 답"); await done;
  assert.equal(calls, 2);
  assert.deepEqual(applied, ["새 질문과 답"]);
});
it("직접 답 응답과 닫힌 화면을 늦은 조회가 덮지 않고, 오류 뒤의 실제 사건으로 복구한다", async () => {
  const first = deferred(), applied = [], errors = [];
  const read = new AsksReload(() => first.promise, r => applied.push(r), e => errors.push(e));
  const done = read.refresh(); read.invalidate(); first.resolve("이전"); await done;
  assert.deepEqual(applied, []);
  read.close(); await read.refresh(); assert.deepEqual(applied, []);
  let fail = true;
  const recovered = new AsksReload(async () => { if (fail) throw Error("끊김"); return "재연결"; }, r => applied.push(r), e => errors.push(e));
  await recovered.refresh(); fail = false; await recovered.refresh();
  assert.equal(errors.length, 1); assert.deepEqual(applied, ["재연결"]);
});
it("원문을 아직 읽지 않은 지난 질문도 링크·배지·삭제·페이지 병합이 정확하다", () => {
  const index = [{ id: "old", kind: "question", subject: "t-0001", question_id: "q-0001", question_ids: ["q-0001"], summary: "1A", answer_summary: "1A", done_ms: 1 }];
  const data = { requests: [{id: "post", title: "확인 aaa:question/q-0001"}], done: [], index };
  assert.equal(questionKey("q-0001", [], answerIndex(data)), "d:old");
  assert.equal(questionRefLook("q-0001", [], index).state, "답함 · 1A");
  assert.deepEqual(openAsks([], data, []), []);
  const old = {id: "old", body: "원문"}, removed = {id: "removed"};
  assert.deepEqual(mergeAskPage([old, removed], [old], index), [old]);
  assert.deepEqual(mergeAskPage([old], [old], []), [], "늦은 페이지가 삭제 기록을 되살리지 않는다");
});

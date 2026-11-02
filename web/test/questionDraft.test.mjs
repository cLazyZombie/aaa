// 새 판의 내용 경계·서버의 일부 답·늦은 전송 성공·저장소 고장은 UI E2E와 별도로 지킨다.
import assert from "node:assert/strict";
import { test } from "node:test";
import { QuestionDraftStore, mergeQuestionDraft } from "../src/questionDraft.ts";
const question = (patch = {}) => ({ report_hash: "first", review: false, answer: null, items: [
  { id: "q-1000", text: "자리", body: "화면", choices: [{ label: "아래", description: null, recommended: true }, { label: "가운데", description: null, recommended: false }] },
  { id: "q-1001", text: "색", choices: [] },
], ...patch });
const local = { picks: [1, null], custom: "", comments: [null, "직접 답\n원래 색 유지"] };
const storage = () => {
  const values = new Map();
  return { getItem: k => values.get(k) ?? null, setItem: (k,v) => values.set(k,v), removeItem: k => values.delete(k), values };
};

test("새 q-ID·보고 hash만 바뀌면 새 페이지에서도 복원하고, 다른 소유자·질문 내용에는 옮기지 않는다", () => {
  const disk = storage(), store = new QuestionDraftStore(() => disk), q = question();
  store.write("project-a/thread-a", q, local);
  const repost = { ...q, report_hash: "repost", items: q.items.map((i,n) => ({ ...i, id: `q-${2000+n}` })) };
  const fresh = new QuestionDraftStore(() => disk);
  assert.deepEqual(fresh.read("project-a/thread-a", repost), local);
  assert.deepEqual(fresh.read("project-b/thread-a", repost), mergeQuestionDraft(repost));
  for (const changed of [
    question({ review: true }),
    question({ items: [...q.items].reverse() }),
    question({ items: [{ ...q.items[0], text: "다른 자리" }, q.items[1]] }),
    question({ items: [{ ...q.items[0], body: "다른 시안" }, q.items[1]] }),
    question({ items: [{ ...q.items[0], after: "새 근거" }, q.items[1]] }),
    question({ items: [{ ...q.items[0], choices: [...q.items[0].choices].reverse() }, q.items[1]] }),
  ]) assert.deepEqual(fresh.read("project-a/thread-a", changed), mergeQuestionDraft(changed));
});

test("서버의 일부 답을 반영해도 다른 질문의 직접 답과 명시적 선택 취소는 남는다", () => {
  const q = question({ draft: { picks: [0, null], comments: [null, null], text: null } });
  assert.deepEqual(mergeQuestionDraft(q, local), { ...local, picks: [0, null] });
  assert.deepEqual(mergeQuestionDraft(question(), { ...local, picks: [null, null] }).picks, [null, null]);
  const one = question({ items: [question().items[0]], draft: { picks: [null], comments: ["CLI 직접 답"], text: null } });
  assert.equal(mergeQuestionDraft(one, { ...local, custom: "옛 초안" }).custom, "CLI 직접 답");
});

test("이미 받은 일부 답을 다시 고친 선택도 목록 갱신·새로고침 뒤 유지한다", () => {
  const disk = storage(), store = new QuestionDraftStore(() => disk);
  const q = question({ draft: { picks: [0, null], comments: [null, null], text: null } });
  store.write("thread", q, local);
  assert.deepEqual(new QuestionDraftStore(() => disk).read("thread", q), local);
  const next = { ...q, draft: { ...q.draft, comments: [null, "CLI 새 답"] } };
  assert.deepEqual(store.read("thread", next), { ...local, comments: [null, "CLI 새 답"] });
});

test("성공한 답만 지우며 늦은 성공이 같은 질문의 새 입력·새 판 초안을 지우지 않는다", () => {
  const disk = storage(), store = new QuestionDraftStore(() => disk), q = question();
  store.write("thread", q, local);
  const sent = store.snapshot("thread");
  assert.deepEqual(store.read("thread", q), local, "실패 시 settle을 부르지 않아 초안이 남는다");
  store.write("thread", { ...q, report_hash: "repost" }, { ...local, custom: "새로 쓴 답" });
  store.settle("thread", sent);
  assert.equal(store.read("thread", q).custom, "새로 쓴 답");
  store.settle("thread", store.snapshot("thread"));
  assert.deepEqual(new QuestionDraftStore(() => disk).read("thread", q), mergeQuestionDraft(q));
});

test("저장소 차단에도 같은 페이지의 카드 왕복을 보존하며 손상된 초안은 답으로 쓰지 않는다", () => {
  const blocked = new QuestionDraftStore(() => { throw new DOMException("차단", "SecurityError"); });
  const q = question();
  blocked.write("thread", q, local);
  assert.deepEqual(blocked.read("thread", q), local);
  const disk = storage(), store = new QuestionDraftStore(() => disk);
  store.write("thread", q, local);
  const [key, raw] = [...disk.values][0];
  disk.setItem(key, "{");
  assert.deepEqual(store.read("thread", q), mergeQuestionDraft(q));
  disk.setItem(key, JSON.stringify({ ...JSON.parse(raw), value: { ...local, picks: [900, null] } }));
  assert.deepEqual(store.read("thread", q).picks, [null, null]);
});

test("저장 용량 초과는 디스크의 옛 초안보다 메모리의 최신 입력을 유지한다", () => {
  const disk = storage(), store = new QuestionDraftStore(() => disk), q = question();
  store.write("thread", q, local);
  disk.setItem = () => { throw new DOMException("용량 초과", "QuotaExceededError"); };
  store.write("thread", q, { ...local, custom: "최신 입력" });
  assert.equal(store.read("thread", q).custom, "최신 입력");
});

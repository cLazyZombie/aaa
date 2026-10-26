import assert from "node:assert/strict";
import { it } from "node:test";
import { readThreadPages, ThreadReload } from "../src/threadReload.ts";

const old = { project: "demo", id: "t-0001", title: "이전 보고", questions: { answer: null } };

it("모든 페이지를 읽는 동안 받은 새 사건을 보존하고 다른 프로젝트의 같은 번호도 유지한다", async () => {
  const reload = new ThreadReload();
  const read = reload.begin();
  const updated = { ...old, title: "새 보고" };
  const second = { ...old, project: "other" };
  let calls = 0;
  const snapshot = await readThreadPages(async request => {
    assert.equal(request.details, true); // 질문·보류 상세를 잃지 않는다
    if (++calls === 1) {
      assert.equal(request.after, undefined);
      return { kind: "threads", threads: [old], next_after: "cursor" };
    }
    assert.equal(request.after, "cursor");
    reload.note(updated);
    return { kind: "threads", threads: [second] };
  });
  assert.equal(calls, 2);
  assert.deepEqual([...reload.finish(read, snapshot).values()], [updated, second]);
});

it("뒤 페이지 오류와 반복 커서는 부분 목록을 성공으로 바꾸지 않는다", async () => {
  const cause = new Error("연결 끊김");
  let calls = 0;
  await assert.rejects(readThreadPages(async () => {
    if (++calls === 1) return { kind: "threads", threads: [old], next_after: "cursor" };
    throw cause;
  }), error => error === cause);
  await assert.rejects(readThreadPages(async () => ({ kind: "threads", threads: [old], next_after: "same" })), /커서가 나아가지/);
  await assert.rejects(readThreadPages(async () => ({ kind: "done" })), /목록 응답이 아니다/);
});

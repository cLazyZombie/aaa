// 할 일 화면의 행 상태·거르기·검색·배지 셈(src/tasksModel.ts, 할 일 보기 T1, plan §8.304).
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { findTask, ownerLabel, taskBadge, taskDepth, taskMatches, taskSections, taskState, unassigned } from "../src/tasksModel.ts";

const task = (over) => ({ list: "Backlog", title: "일", owner: null, thread: null, notes: [], done: false, line: 1, raw: "- [ ] 일", block_sha256: "", parent: null, problems: [], ...over });
const MISREAD = "owner 칸이 owner 꼴이 아님: t-0001 S4 뒤";
const groups = new Map([["t-0001", "working"], ["t-0002", "resolved"], ["t-0003", "waiting_on_you"]]);

describe("할 일 화면", () => {
  it("행 상태: 형식 어긋남 > 열린 thread와 이어짐 > 대기. 끝난·모르는 thread는 대기다", () => {
    assert.equal(taskState(task({ thread: "t-0001", problems: [MISREAD] }), groups), "problem");
    assert.equal(taskState(task({ thread: "t-0001" }), groups), "run");
    assert.equal(taskState(task({ thread: "t-0003" }), groups), "run");
    assert.equal(taskState(task({ thread: "t-0002" }), groups), "wait");
    assert.equal(taskState(task({ thread: "t-0099" }), groups), "wait");
    assert.equal(taskState(task({}), groups), "wait");
  });

  it("owner: 비움·agent는 미할당, me는 나. owner를 읽지 못한 형식 어긋남은 미할당으로 세지 않는다", () => {
    assert.equal(ownerLabel(task({})), "미할당");
    assert.equal(ownerLabel(task({ owner: "agent" })), "미할당");
    assert.equal(ownerLabel(task({ owner: "me" })), "나");
    assert.equal(ownerLabel(task({ owner: "codex@mac-b" })), "codex@mac-b");
    assert.equal(unassigned(task({ problems: [MISREAD] })), false);
    assert.equal(ownerLabel(task({ problems: [MISREAD] })), null);
    // `(미할당)`은 미할당으로 읽혔다(정규형 안내만 단다)
    assert.equal(unassigned(task({ problems: ["owner `미할당`는 미할당으로 읽음(정규형은 비움)"] })), true);
  });

  it("거르기: 전체·내 일·미할당·진행 중", () => {
    const ts = [task({ title: "a", owner: "me" }), task({ title: "b" }), task({ title: "c", thread: "t-0001", owner: "claude" }), task({ title: "d", problems: [MISREAD] })];
    const pick = (f) => ts.filter((t) => taskMatches(t, f, "", groups)).map((t) => t.title);
    assert.deepEqual(pick("all"), ["a", "b", "c", "d"]);
    assert.deepEqual(pick("mine"), ["a"]);
    assert.deepEqual(pick("unassigned"), ["b"]);
    assert.deepEqual(pick("running"), ["c"]);
  });

  it("검색은 제목·메모·owner에서 대소문자를 가리지 않는다", () => {
    const t = task({ title: "Login 고치기", owner: "Priya Rao", notes: ["- 테스트 먼저"] });
    for (const q of ["login", "LOGIN", "테스트", "priya", "  고치기  "]) assert.equal(taskMatches(t, "all", q, groups), true, q);
    assert.equal(taskMatches(t, "all", "배포", groups), false);
    assert.equal(taskMatches(t, "mine", "login", groups), false);
  });

  it("배지는 형식 어긋남 task 수다(진행 중·대기는 세지 않는다)", () => {
    assert.equal(taskBadge(null), 0);
    const board = { project: "p", sha256: "", lists: [], unlinked_threads: [], notes_files: [], tasks: [task({ problems: [MISREAD] }), task({ problems: ["제목이 김(메모로 옮길 후보)"] }), task({ thread: "t-0001" }), task({})] };
    assert.equal(taskBadge(board), 2);
  });

  it("하위 task 깊이와 목록별 묶음: task 없는 목록도 머리를 갖고, 거른 수와 전체 수를 따로 센다", () => {
    const ts = [task({ line: 3, title: "상위" }), task({ line: 4, title: "하위", parent: 3 }), task({ line: 5, title: "손자", parent: 4 }), task({ line: 9, title: "끊긴 상위", parent: 7 }), task({ line: 12, list: "Later", title: "나중", owner: "me" })];
    const byLine = new Map(ts.map((t) => [t.line, t]));
    assert.deepEqual(ts.map((t) => taskDepth(t, byLine)), [0, 1, 2, 0, 0]);
    const board = { project: "p", sha256: "", lists: [{ name: "Backlog", notes: ["설명"] }, { name: "빈 목록", notes: [] }, { name: "Later", notes: [] }], tasks: ts, unlinked_threads: [], notes_files: [] };
    const s = taskSections(board, "mine", "", groups);
    assert.deepEqual(s.map((x) => [x.name, x.total, x.tasks.length]), [["Backlog", 4, 0], ["빈 목록", 0, 0], ["Later", 1, 1]]);
    assert.deepEqual(s[0].notes, ["설명"]);
  });

  it("상세가 가리키는 task: 제목으로 찾고, 같은 제목이면 줄이 같은 것", () => {
    const board = { project: "p", sha256: "", lists: [], unlinked_threads: [], notes_files: [], tasks: [task({ line: 2, title: "같음" }), task({ line: 8, title: "같음" }), task({ line: 9, title: "다른" })] };
    assert.equal(findTask(board, { title: "같음", line: 8 }).line, 8);
    // 줄이 밀려도(위에 줄이 더해짐) 제목으로 따라간다
    assert.equal(findTask(board, { title: "다른", line: 3 }).line, 9);
    assert.equal(findTask(board, { title: "없음", line: 1 }), null);
    assert.equal(findTask(null, { title: "같음", line: 2 }), null);
  });
});

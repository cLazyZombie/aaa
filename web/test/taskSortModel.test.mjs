// 표시 정렬이 실행 차례·상하위·문서 원문을 바꾸지 않는 기능 계약이다.
import assert from "node:assert/strict";
import { it } from "node:test";
import { taskList } from "../src/tasksModel.ts";
import { ACTIVE_TASK_SORTS, readTaskSort, writeTaskSort, sortActiveTasks } from "../src/taskSortModel.ts";

const task = (line, over = {}) => ({ line, id: `id${line}`, short_id: `d-${line}`, title: `일 ${line}`, parent: null,
  done: false, problems: [], attention: null, thread: null, profile: null, machine: null, notes: [], owner: null, ...over });
const groups = new Map([["t-0001", "working"], ["t-0002", "parked"]]);
const titles = m => m.rest.map(r => r.t.title);

it("속성별 차례·빈 값·0채움 번호와 상태를 지키고 같은 값은 손 차례다", () => {
  const tasks = [
    task(1, { title: "다", short_id: "d-00965", profile: "sol", machine: "mac-b", thread: "t-0002" }),
    task(2, { title: "가", short_id: "d-10000", profile: "astra", machine: "mac-a", thread: "t-0001" }),
    task(3, { title: "나", short_id: "d-0965", profile: "sol", machine: "mac-b", attention: "확인" }),
    task(4, { title: "라", short_id: null }),
  ];
  const board = { tasks, queue: [] };
  const original = structuredClone(board);
  const model = taskList(board, "all", "", null, groups);
  const sorted = s => titles(sortActiveTasks(model, board, s, groups));
  assert.equal(sortActiveTasks(model, board, "manual", groups), model);
  assert.deepEqual(sorted("number"), ["가", "다", "나", "라"]);
  assert.deepEqual(sorted("title"), ["가", "나", "다", "라"]);
  assert.deepEqual(sorted("state"), ["나", "가", "라", "다"]);
  for (const s of ["profile", "machine"]) assert.deepEqual(sorted(s), ["가", "다", "나", "라"]);
  assert.deepEqual(board, original);
  assert.deepEqual(titles(model), ["다", "가", "나", "라"]);
});

it("예약과 숨은 예약 자리는 고정하고 하위는 상위와 함께, 거른 뒤에도 묶음을 보존한다", () => {
  const tasks = [
    task(1, { title: "하 예약", repo: "aaa" }),
    task(2, { title: "가 하위", parent: 1, repo: "aaa" }),
    task(3, { title: "숨은 예약", repo: "rc" }),
    task(4, { title: "나 예약", repo: "aaa" }),
    task(5, { title: "하 상위", repo: "aaa", owner: "me" }),
    task(6, { title: "가 손자", parent: 7, repo: "aaa" }),
    task(7, { title: "나 하위", parent: 5, repo: "aaa" }),
    task(8, { title: "가 하위", parent: 5, repo: "aaa" }),
    task(9, { title: "나 상위", repo: "aaa" }),
  ];
  // 실제 parser가 주는 상위→하위 순서를 쓴다.
  tasks.sort((a, b) => [1,2,3,4,5,7,6,8,9].indexOf(a.line) - [1,2,3,4,5,7,6,8,9].indexOf(b.line));
  const board = { tasks, queue: [tasks[0], tasks[2], tasks[3]].map((t, i) => ({ task_id: t.id, task: t, state: "queued", position: i + 1 })) };
  for (const s of ACTIVE_TASK_SORTS) {
    const model = taskList(board, "all", "", "aaa", groups);
    const sorted = sortActiveTasks(model, board, s.key, groups);
    assert.equal(sorted.top, model.top);
    assert.deepEqual(sorted.top.map(r => r.kind), ["reserved", "under", "gap", "reserved"]);
  }
  assert.deepEqual(titles(sortActiveTasks(taskList(board, "all", "", "aaa", groups), board, "title", groups)), ["나 상위", "하 상위", "가 하위", "나 하위", "가 손자"]);
  // 검색으로 상위가 없어도 하위 하나를 다른 상위 묶음 사이로 흩뜨리지 않는다.
  assert.deepEqual(titles(sortActiveTasks(taskList(board, "unassigned", "", "aaa", groups), board, "title", groups)), ["나 상위", "가 하위", "나 하위", "가 손자"]);
});

it("할 일·완료 선택을 독립하여 기억하고 모르는 저장값은 손 차례다", () => {
  const previous = Object.getOwnPropertyDescriptor(globalThis, "localStorage");
  const storage = new Map([["aaa:tasks:completed-sort", "started"]]);
  Object.defineProperty(globalThis, "localStorage", { configurable: true, value: { getItem: k => storage.get(k) ?? null, setItem: (k, v) => storage.set(k, v) } });
  try {
    assert.equal(readTaskSort("active"), "manual");
    writeTaskSort("active", "machine");
    assert.equal(readTaskSort("active"), "machine");
    assert.equal(storage.get("aaa:tasks:completed-sort"), "started");
    storage.set("aaa:tasks:active-sort", "unknown");
    assert.equal(readTaskSort("active"), "manual");
  } finally {
    if (previous) Object.defineProperty(globalThis, "localStorage", previous);
    else delete globalThis.localStorage;
  }
});

import assert from "node:assert/strict";
import { test } from "node:test";
import { paletteTargets, rememberTarget, loadRecent, RECENT_LIMIT } from "../src/palette.ts";
import { appScreenTarget, recentTargets, liveRecentTargets, visitedScreens, startRecentCycle, moveRecentCycle, recentScreenChord } from "../src/recentScreens.ts";

const machines = [{ id: "m", name: "mac-a", connection: "connected" }];
const pane = (id, extra = {}) => ({ id, machine_id: "m", name: id, argv: [], state: "running", ...extra });
const thread = (id, extra = {}) => ({ id, project: "demo", title: id, machine: "mac-a", pane: id, status: "open", group: "working", ...extra });

test("앱 목록·상세·설정도 방문 순서와 열 주소를 보존하며 pane 생명주기 필터와 분리한다", () => {
  const keys = ["reports:", "reports:demo/r-0001", "tasks:", "task:d-0001", "automation:demo/daily", "idea:n-0001", "idea:demo/n-0002", "settings:", "question:q-0001", "workspace:", "workspace:m/repo"];
  const panes = paletteTargets(machines, [pane("coord", { coordinator_project: "demo" }), pane("gone", { state: "exited" })], []);
  const targets = recentTargets(panes, [...keys, "reports:", "pane:gone", "https://example.com"]);
  assert.deepEqual(visitedScreens(targets, keys).map(t => t.open), keys);
  assert.equal(targets.length, keys.length + 1);
  assert.equal(targets[0].coordinator, true);
  assert.equal(appScreenTarget("idea:n-0001").title, "아이디어 · n-0001");
  for (const key of ["https://example.com", "pane:gone", "unknown:", "settings"]) assert.equal(appScreenTarget(key), null);
});

test("아이디어 목록은 옛 기록에서도 제외하고 오른쪽 노트만 순환한다", () => {
  const keys = ["idea:n-0002", "ideas:", "idea:n-0001", "tasks:"];
  assert.equal(appScreenTarget("ideas:"), null);
  const targets = recentTargets([], keys);
  const visited = visitedScreens(targets, keys).map(target => target.key);
  assert.deepEqual(visited, ["idea:n-0002", "idea:n-0001", "tasks:"]);
  assert.equal(startRecentCycle(visited, "idea:n-0002", 1).key, "idea:n-0001");
  assert.equal(startRecentCycle(visited, "idea:n-0002", -1).key, "tasks:");
});

test("자동화·답할 것 목록의 옛 기록을 제외하고 규칙·질문 본문만 순환한다", () => {
  const keys = ["question:q-0001", "asks:", "automation:demo/daily", "automations:", "ask:r:request", "ideas:"];
  assert.deepEqual([appScreenTarget("automations:"), appScreenTarget("asks:")], [null, null]);
  const visited = visitedScreens(recentTargets([], keys), keys).map(target => target.open);
  assert.deepEqual(visited, ["question:q-0001", "automation:demo/daily", "ask:r:request"]);
  assert.equal(startRecentCycle(visited, "question:q-0001", 1).key, "automation:demo/daily");
  assert.equal(startRecentCycle(visited, "question:q-0001", -1).key, "ask:r:request");
});

test("완료 보고의 쉼은 포함하고 종료·보류·해결·보관·연결 해제는 제외한다", () => {
  const panes = [pane("work", { agent: { activity: "idle", seen: false } }), pane("shell"), pane("coord", { coordinator_project: "demo", role: "coordinator" }), pane("gone", { state: "exited" }), pane("park"), pane("resolved"), pane("archived")];
  const threads = [thread("work"), thread("park", { status: "parked" }), thread("resolved", { status: "resolved" }), thread("archived", { project_status: "archived" })];
  const live = liveRecentTargets(paletteTargets(machines, panes, threads));
  assert.deepEqual(new Set(live.map((target) => target.pane.id)), new Set(["work", "shell", "coord"]));
  assert.deepEqual(liveRecentTargets(paletteTargets([{ ...machines[0], connection: "disconnected" }], panes, threads)), []);
  const work = live.find((target) => target.pane.id === "work");
  assert.deepEqual(work.keys, ["thread:demo/work", "pane:work"]);
  assert.deepEqual(visitedScreens(live, [work.key, work.key, "absent"]).map((target) => target.key), [work.key]);
});

test("Ctrl을 누른 동안 MRU를 고정해 셋 이상 순환하고 역방향으로 되짚는다", () => {
  const keys = ["c", "b", "a"];
  const live = new Set(keys);
  const first = startRecentCycle(keys, "c", 1);
  assert.equal(first.key, "b");
  const second = moveRecentCycle(first, live, 1);
  assert.equal(second.key, "a");
  assert.equal(moveRecentCycle(second, live, -1).key, "b");
  assert.equal(moveRecentCycle(second, live, 1).key, "c");
  assert.deepEqual(keys, ["c", "b", "a"]);
  assert.deepEqual(rememberTarget(second.key, keys), ["a", "c", "b"]);
});

test("목록 밖 화면·후보 0/1개·누른 동안 사라진 후보를 처리한다", () => {
  assert.equal(startRecentCycle([], null, 1), null);
  assert.equal(startRecentCycle(["a"], "a", 1), null);
  assert.equal(startRecentCycle(["a"], null, 1).key, "a");
  assert.equal(startRecentCycle(["a", "b"], null, -1).key, "b");
  const cycle = startRecentCycle(["c", "b", "a"], "c", 1);
  assert.equal(moveRecentCycle(cycle, new Set(["c", "a"]), 1), null);
  assert.deepEqual(moveRecentCycle(cycle, new Set(["b", "a", "new"]), 1), { keys: ["b", "a"], key: "a" });
});

test("방문 기록은 기존 8개 뒤도 남고 중복 없이 저장 한도를 지킨다", () => {
  let recent = [];
  for (let i = 0; i < RECENT_LIMIT + 5; i++) recent = rememberTarget(`pane:${i}`, recent);
  assert.equal(recent.length, RECENT_LIMIT);
  assert.ok(recent.includes("pane:5"));
  assert.deepEqual(loadRecent(() => JSON.stringify(recent)), recent);
  assert.deepEqual(loadRecent(() => JSON.stringify(["pane:a", 3, "pane:a"])), ["pane:a"]);
});

test("Ctrl+Tab과 역방향만 앱 후보이며 조합·AltGr·Meta는 제외한다", () => {
  const event = { key: "Tab", ctrlKey: true, altKey: false, metaKey: false };
  assert.equal(recentScreenChord(event), true);
  assert.equal(recentScreenChord({ ...event, shiftKey: true }), true);
  for (const extra of [{ ctrlKey: false }, { altKey: true }, { metaKey: true }, { isComposing: true }, { keyCode: 229 }, { key: "Enter" }]) assert.equal(recentScreenChord({ ...event, ...extra }), false);
});

import assert from "node:assert/strict";
import test from "node:test";
import { recentHistory, visitRecent, recentScreen, recentList, stepRecent, recentScreenChord, RECENT_SCREENS_MAX } from "../src/recentScreenModel.ts";

const pane = (id, fields = {}) => ({ id, name: id, machine: "mac-a", state: "running", connection: "connected", ...fields });
const thread = (fields = {}) => ({ project: "example", id: "t-0001", title: "작업", group: "working", project_status: "active", pane: "agent", ...fields });
const context = (fields = {}) => ({ panes: [], threads: [], tasks: [], reports: [], automations: [], ...fields });

test("기기 기록은 알려진 화면 키만, 중복 없이 최근 50개까지 기억한다", () => {
  assert.deepEqual(recentHistory([null, {}, 1, "https://example.com", "tasks:", "tasks:", "settings:", "pane:\nunsafe"]), ["tasks:", "settings:"]);
  assert.deepEqual(visitRecent(["tasks:", "settings:", "reports:"], "settings:"), ["settings:", "tasks:", "reports:"]);
  assert.equal(recentHistory(Array.from({ length: 80 }, (_, i) => `pane:${i}`)).length, RECENT_SCREENS_MAX);
});

test("thread의 대화·보고·agent pane은 한 작업이고 재개된 pane으로 연다", () => {
  const ctx = context({ panes: [pane("agent"), pane("shell")], threads: [thread()] });
  const resolve = (key) => recentScreen(key, ctx);
  const rows = recentList(["pane:agent", "thread:example/t-0001", "pane:shell"], resolve);
  assert.deepEqual(rows.map((s) => s.key), ["thread:example/t-0001", "pane:shell"]);
  assert.equal(rows[0].selection, "pane:agent");
  ctx.panes = [pane("resumed", { replaced: ["agent"] })];
  ctx.threads = [thread({ pane: "resumed" })];
  assert.equal(resolve("pane:agent").selection, "pane:resumed");
  assert.equal(resolve("thread:example/t-0001").selection, "pane:resumed");
});

test("닫힌 셸·끝난/보류 thread·보관 프로젝트는 빼고 미완료 보고는 남긴다", () => {
  for (const state of ["exited", "lost", "interrupted"]) assert.equal(recentScreen("pane:shell", context({ panes: [pane("shell", { state })] })), null);
  for (const fields of [{ group: "resolved" }, { group: "parked" }, { project_status: "archived" }]) {
    const ctx = context({ panes: [pane("agent")], threads: [thread(fields)] });
    assert.equal(recentScreen("pane:agent", ctx), null);
    assert.equal(recentScreen("thread:example/t-0001", ctx), null);
  }
  const ctx = context({ threads: [thread({ group: "ready_for_review", pane: null })] });
  assert.equal(recentScreen("thread:example/t-0001", ctx).selection, "thread:example/t-0001");
  assert.equal(recentScreen("pane:missing", ctx), null);
});

test("할 일·보고서·자동화·설정·작업 공간도 직접 방문한 대상만 목록에 넣는다", () => {
  const ctx = context({ tasks: [{ short_id: "d-0001", title: "할 일 제목" }], reports: [{ project: "example", name: "review", title: "검토" }], automations: [{ project: "example", rules: [{ name: "watch", title: "감시" }] }] });
  const keys = ["task:d-0001", "reports:example/review", "automation:example/watch", "settings:", "tasks:", "reports:", "asks:", "workspace:"];
  assert.equal(recentList(keys, (key) => recentScreen(key, ctx)).length, keys.length);
  assert.deepEqual(recentList(["tasks:"], (key) => recentScreen(key, ctx)).map((s) => s.title), ["할 일"]);
  assert.equal(recentScreen("task:d-0002", ctx), null);
  assert.equal(recentScreen("reports:example/deleted", ctx), null);
  assert.equal(recentScreen("automation:example/deleted", ctx), null);
});

test("세 화면 이상을 MRU 스냅샷으로 양방향 순회하고 한 화면이면 열지 않는다", () => {
  const keys = ["tasks:", "settings:", "reports:", "pane:shell"];
  let cycle = stepRecent(keys, "tasks:", 1);
  assert.equal(cycle.selected, "settings:");
  cycle = stepRecent(cycle.keys, cycle.selected, 1);
  assert.equal(cycle.selected, "reports:");
  cycle = stepRecent(cycle.keys, cycle.selected, -1);
  assert.equal(cycle.selected, "settings:");
  assert.equal(stepRecent(keys, "tasks:", -1).selected, "pane:shell");
  assert.deepEqual(keys, ["tasks:", "settings:", "reports:", "pane:shell"]);
  assert.equal(stepRecent(["tasks:"], "tasks:", 1), null);
  assert.equal(stepRecent(keys, null, -1).selected, "pane:shell");
});

test("앞 항목이 사라져도 ID로 선택을 유지하며 닫힌 선택은 확정하지 않는다", () => {
  const ctx = context({ panes: [pane("a"), pane("b"), pane("c")] });
  const cycle = stepRecent(["pane:a", "pane:b", "pane:c"], "pane:b", 1);
  ctx.panes.shift();
  assert.equal(cycle.selected, "pane:c");
  assert.deepEqual(recentList(cycle.keys, (key) => recentScreen(key, ctx)).map((s) => s.key), ["pane:b", "pane:c"]);
  ctx.panes.pop();
  assert.equal(recentScreen(cycle.selected, ctx), null);
});

test("정확한 Control+Tab만 처리하고 Alt·Command·IME·일반 Tab은 남긴다", () => {
  const event = { key: "Tab", ctrlKey: true, shiftKey: false, altKey: false, metaKey: false };
  assert.equal(recentScreenChord(event), true);
  assert.equal(recentScreenChord({ ...event, shiftKey: true }), true);
  for (const fields of [{ ctrlKey: false }, { altKey: true }, { metaKey: true }, { isComposing: true }, { keyCode: 229 }, { key: "x" }]) assert.equal(recentScreenChord({ ...event, ...fields }), false);
});

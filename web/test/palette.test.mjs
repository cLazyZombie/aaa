// 명령 팔레트의 검색·묶음(src/palette.ts). herdr-web-ui `src/lib/paletteSearch.test.ts`·`paletteSettings.test.ts`·
// `machinePalette.test.ts` 가운데 aaa에 옮긴 것: 빈 검색은 목록 차례, 이름·폴더·agent·머신으로 찾기, 글자 순서 찾기, `>` 동작만,
// 상태 칩과 그 수, 가장 잘 맞은 줄의 묶음이 먼저, 최근 줄(연 줄·없어진 줄 빼고), 동작·설정 쪽 이름 찾기, 설정 쪽 요청.
// 참고의 workspace 자리는 머신 안 프로젝트(thread 줄)와 pane 묶음이다.
import assert from "node:assert/strict";
import { beforeEach, describe, it } from "node:test";
import {
  ACTION_ALIASES,
  groupTargets,
  loadRecent,
  paletteTargets,
  parseQuery,
  rankByTitle,
  rankTargets,
  recentTargets,
  rememberTarget,
  requestedSettingsPage,
  requestSettingsPage,
  SETTINGS_PAGE_ENTRIES,
  statusCounts,
} from "../src/palette.ts";

const machine = (id, name, connection = "connected") => ({ id, name, connection });
const pane = (id, machine_id, { name = id, cwd = null, agent = null, label = null } = {}) => ({
  id,
  machine_id,
  machine: machine_id,
  name,
  label,
  argv: [],
  cwd,
  state: "running",
  role: null,
  agent,
});
const agent = (harness, activity, seen = true) => ({ harness, activity, seen, since_ms: 1 });
const thread = (project, id, title, pane, machineName) => ({ project, id, title, machine: machineName, profile: "claude", role: null, group: "working", pane, branch: `hp/${id}` });

const machines = [machine("m1", "mac-a"), machine("m2", "mac-b"), machine("m3", "mac-off", "disconnected")];
const panes = [
  pane("p1", "m1", { name: "dashboard", cwd: "/srv/front/dashboard", agent: agent("claude", "idle") }),
  pane("p2", "m1", { name: "server", cwd: "/srv/backend", agent: agent("codex", "working") }),
  pane("p3", "m2", { name: "frontend", cwd: "/srv/front", agent: agent("claude", "question") }),
  pane("p4", "m2", { name: "shell" }),
  pane("p5", "m3", { name: "away" }),
  pane("t1pane", "m1", { name: "t-0001", cwd: "/srv/work/demo", agent: agent("claude", "idle", false) }),
];
const threads = [thread("demo", "t-0001", "결제 지연 줄이기", "t1pane", "mac-a")];
const keys = (targets) => targets.map((t) => t.key);

describe("paletteTargets", () => {
  const targets = paletteTargets(machines, panes, threads);

  it("붙은 머신의 목록 줄을 목록 차례로: 머신마다 thread 줄, 그다음 pane", () => {
    assert.deepEqual(keys(targets), ["thread:demo/t-0001", "pane:p1", "pane:p2", "pane:p3", "pane:p4"]);
  });

  it("thread 줄은 그 agent pane을 열고 그 pane도 골랐다고 본다", () => {
    const [t] = targets;
    assert.deepEqual(t.open, { pane: "t1pane" });
    assert.deepEqual(t.keys, ["thread:demo/t-0001", "pane:t1pane"]);
    assert.equal(t.title, "t-0001 · 결제 지연 줄이기");
    assert.equal(t.project, "demo");
  });

  it("상태는 목록과 같은 말: 확인창 blocked, 턴 working, 안 본 쉼 done, 셸은 없음", () => {
    assert.deepEqual(targets.map((t) => t.status), ["done", "idle", "working", "blocked", "unknown"]);
  });
});

describe("rankTargets", () => {
  const targets = paletteTargets(machines, panes, threads);

  it("빈 검색은 목록 차례 그대로", () => {
    assert.deepEqual(keys(rankTargets("  ", targets, "all")), keys(targets));
  });

  it("이름·폴더·agent·thread 제목·머신 이름으로 찾는다", () => {
    assert.deepEqual(keys(rankTargets("backend", targets, "all")), ["pane:p2"]);
    assert.deepEqual(keys(rankTargets("codex", targets, "all")), ["pane:p2"]);
    assert.deepEqual(keys(rankTargets("결제", targets, "all")), ["thread:demo/t-0001"]);
    assert.deepEqual(keys(rankTargets("mac-b", targets, "all")), ["pane:p3", "pane:p4"]);
  });

  it("순서대로 고른 글자로도 찾고, 바로 맞은 것이 먼저", () => {
    assert.deepEqual(keys(rankTargets("dsh", targets, "all")), ["pane:p1"]);
    assert.deepEqual(keys(rankTargets("front", targets, "all")), ["pane:p3", "pane:p1"]);
    assert.deepEqual(rankTargets("zzq", targets, "all"), []);
  });

  it("상태 칩은 그 상태의 줄만, 수는 검색과 상관없이 전체에서", () => {
    assert.deepEqual(keys(rankTargets("", targets, "blocked")), ["pane:p3"]);
    assert.deepEqual(keys(rankTargets("", targets, "done")), ["thread:demo/t-0001"]);
    assert.deepEqual(statusCounts(targets), { all: 5, blocked: 1, working: 1, idle: 1, done: 1, waiting: 0 });
  });
});

describe("groupTargets", () => {
  const targets = paletteTargets(machines, panes, threads);

  it("머신의 프로젝트마다, 그다음 그 머신의 pane 묶음", () => {
    assert.deepEqual(
      groupTargets(targets).map((g) => [g.machine.name, g.project, keys(g.targets)]),
      [
        ["mac-a", "demo", ["thread:demo/t-0001"]],
        ["mac-a", null, ["pane:p1", "pane:p2"]],
        ["mac-b", null, ["pane:p3", "pane:p4"]],
      ],
    );
  });

  it("찾은 목록은 가장 잘 맞은 줄의 묶음이 먼저", () => {
    assert.deepEqual(
      groupTargets(rankTargets("front", targets, "all")).map((g) => g.machine.name),
      ["mac-b", "mac-a"],
    );
  });

  it("줄이 없으면 묶음도 없다", () => {
    assert.deepEqual(groupTargets([]), []);
  });
});

describe("최근 줄", () => {
  const targets = paletteTargets(machines, panes, threads);

  it("새것 먼저, 지금 연 줄(thread 줄은 그 pane으로도)과 없어진 줄은 빼고, 한도까지", () => {
    assert.deepEqual(keys(recentTargets(targets, ["pane:p4", "pane:gone", "pane:p1", "pane:p2"], "pane:p1", 3)), ["pane:p4", "pane:p2"]);
    assert.deepEqual(keys(recentTargets(targets, ["thread:demo/t-0001", "pane:p2"], "pane:t1pane", 3)), ["pane:p2"]);
    assert.deepEqual(keys(recentTargets(targets, ["pane:p4", "pane:p2", "pane:p1"], null, 2)), ["pane:p4", "pane:p2"]);
  });

  it("기억은 앞에 놓고 겹치지 않으며, 깨진 저장값은 빈 기록", () => {
    assert.deepEqual(rememberTarget("pane:p2", ["pane:p1", "pane:p2"]), ["pane:p2", "pane:p1"]);
    assert.deepEqual(loadRecent(() => '["pane:p1","pane:p1",3,"pane:p2"]'), ["pane:p1", "pane:p2"]);
    assert.deepEqual(loadRecent(() => "{"), []);
    assert.deepEqual(loadRecent(() => "{}"), []);
  });
});

describe("parseQuery", () => {
  it("앞의 >는 동작만, 나머지를 찾는다", () => {
    assert.deepEqual(parseQuery(">"), { actionsOnly: true, text: "" });
    assert.deepEqual(parseQuery("  > set"), { actionsOnly: true, text: "set" });
    assert.deepEqual(parseQuery(">theme "), { actionsOnly: true, text: "theme" });
  });

  it("다른 검색은 줄과 동작에", () => {
    assert.deepEqual(parseQuery(" dash "), { actionsOnly: false, text: "dash" });
    assert.deepEqual(parseQuery("a > b"), { actionsOnly: false, text: "a > b" });
  });
});

describe("rankByTitle", () => {
  const rows = [
    { item: "toggle-theme", title: "Toggle theme", aliases: "dark light" },
    { item: "settings", title: "Settings", aliases: "preferences theme" },
    { item: "theme", title: "Theme", aliases: "" },
    { item: "refresh", title: "Refresh", aliases: "reload" },
  ];

  it("빈 검색은 모든 줄", () => {
    assert.deepEqual(rankByTitle("  ", rows), ["toggle-theme", "settings", "theme", "refresh"]);
  });

  it("제목 전체, 제목 시작, 제목 안, 다른 말로만 맞은 것 차례", () => {
    assert.deepEqual(rankByTitle("theme", rows), ["theme", "toggle-theme", "settings"]);
    assert.deepEqual(rankByTitle("the", rows), ["theme", "toggle-theme", "settings"]);
  });

  it("제목에 없는 다른 말로 찾고, 낱말은 모두 맞아야 한다", () => {
    assert.deepEqual(rankByTitle("reload", rows), ["refresh"]);
    assert.deepEqual(rankByTitle("settings theme", rows), ["settings"]);
    assert.deepEqual(rankByTitle("settings nothing", rows), []);
  });

  it("설정 쪽을 테마·글꼴로 찾는다", () => {
    const pages = SETTINGS_PAGE_ENTRIES.map((entry) => ({ item: entry.id, title: entry.label, aliases: entry.aliases }));
    assert.deepEqual(rankByTitle("테마", pages), ["appearance"]);
    assert.deepEqual(rankByTitle("글꼴", pages), ["chat", "terminal"]);
    assert.ok(Object.keys(ACTION_ALIASES).includes("theme"));
  });
});

describe("설정 쪽 요청", () => {
  beforeEach(() => requestSettingsPage(null));
  it("지울 때까지 남는다", () => {
    requestSettingsPage("terminal");
    assert.equal(requestedSettingsPage(), "terminal");
    requestSettingsPage(null);
    assert.equal(requestedSettingsPage(), null);
  });
});

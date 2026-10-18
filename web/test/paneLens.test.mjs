// pane이 열리는 보기(src/paneLens.ts). herdr-web-ui `src/lib/paneLens.test.ts` 전부와 `settings.test.ts` 「default lens」의
// `forgetPaneViews` 시험을 옮겼다(bun:test → node:test, 열쇠 머리만 aaa).
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { defaultLens, forgetPaneViews, PaneLenses } from "../src/paneLens.ts";

describe("defaultLens", () => {
  it("설정을 따른다: agent(또는 아직 모르는 pane)는 대화, 셸은 터미널", () => {
    assert.equal(defaultLens(true, "chat", false), "chat");
    assert.equal(defaultLens(null, "chat", false), "chat");
    assert.equal(defaultLens(false, "chat", false), "terminal");
    assert.equal(defaultLens(true, "terminal", true), "terminal");
    assert.equal(defaultLens(true, "auto", false), "terminal");
    assert.equal(defaultLens(true, "auto", true), "chat");
    assert.equal(defaultLens(false, "auto", true), "terminal");
  });
});

describe("PaneLenses", () => {
  it("셸에서 agent가 떠도 쓰던 터미널 그대로(기본이 대화여도)", () => {
    const lenses = new PaneLenses("chat");
    assert.equal(lenses.of("m1:p2", false, false), "terminal");
    assert.equal(lenses.of("m1:p2", true, false), "terminal");
  });

  it("agent가 끝나도 그 pane의 대화 그대로", () => {
    const lenses = new PaneLenses("chat");
    assert.equal(lenses.of("m1:p1", true, false), "chat");
    assert.equal(lenses.of("m1:p1", false, false), "chat");
  });

  it("pane을 아직 모르는 동안은 정하지 않는다", () => {
    const lenses = new PaneLenses("chat");
    assert.equal(lenses.of("m1:p3", null, false), "chat");
    assert.equal(lenses.of("m1:p3", false, false), "terminal");
    assert.equal(lenses.of("m1:p3", true, false), "terminal");
  });

  it("이 화면이 agent를 띄운 pane은 첫 상태가 셸이라 해도 agent의 보기", () => {
    const lenses = new PaneLenses("chat");
    lenses.startedAgent("m2:p1", false);
    assert.equal(lenses.of("m2:p1", false, false), "chat");
  });

  it("pane마다 따로", () => {
    const lenses = new PaneLenses("chat");
    assert.equal(lenses.of("m1:p1", true, false), "chat");
    assert.equal(lenses.of("m2:p1", false, false), "terminal");
  });
});

it("pane마다 고른 보기만 잊고 다른 것은 둔다", () => {
  const data = new Map([
    ["aaa:view:m1:p1", "terminal"],
    ["aaa:view:m2:p1", "chat"],
    ["aaa:appearance", "{}"],
  ]);
  const storage = {
    get length() {
      return data.size;
    },
    key: (i) => [...data.keys()][i] ?? null,
    removeItem: (k) => void data.delete(k),
  };
  assert.equal(forgetPaneViews(storage), 2);
  assert.deepEqual([...data.keys()], ["aaa:appearance"]);
});

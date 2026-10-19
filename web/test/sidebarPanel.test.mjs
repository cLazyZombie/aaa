// 활동 막대 드러내기 규칙(src/sidebarPanel.ts revealPanel, plan §8.240·§8.280): 단축키·팔레트·알림이 연 줄의 패널.
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { revealPanel } from "../src/sidebarPanel.ts";

describe("연 줄의 패널", () => {
  it("「작업」 트리에 있는 줄이면 「작업」으로(VS Code explorer.autoReveal)", () => {
    assert.equal(revealPanel("work", true), "work");
    assert.equal(revealPanel("machines", true), "work");
    assert.equal(revealPanel("special", true), "work");
  });
  it("트리에 없는 줄(평가자·임무 pane)은 패널을 바꾸지 않는다", () => {
    assert.equal(revealPanel("machines", false), "machines");
    assert.equal(revealPanel("special", false), "special");
  });
});

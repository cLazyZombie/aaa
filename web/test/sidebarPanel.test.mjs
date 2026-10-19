// 활동 막대 드러내기 규칙(src/sidebarPanel.ts revealPanel, plan §8.238): 단축키·팔레트·알림이 연 줄의 패널. TUI도 같은 규칙을 따른다.
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { revealPanel } from "../src/sidebarPanel.ts";

describe("연 줄의 패널", () => {
  it("지금 패널에 그 줄이 있으면 그대로(VS Code explorer.autoReveal)", () => {
    assert.equal(revealPanel("projects", true, true), "projects");
    assert.equal(revealPanel("agents", true, true), "agents");
  });
  it("없으면 프로젝트 먼저, 그다음 에이전트", () => {
    assert.equal(revealPanel("machines", true, true), "projects");
    assert.equal(revealPanel("agents", true, false), "projects");
    assert.equal(revealPanel("projects", false, true), "agents");
    assert.equal(revealPanel("machines", false, true), "agents");
  });
  it("어느 패널에도 없는 줄(평가자 pane 등)은 패널을 바꾸지 않는다", () => {
    assert.equal(revealPanel("machines", false, false), "machines");
    assert.equal(revealPanel("agents", false, false), "agents");
  });
});

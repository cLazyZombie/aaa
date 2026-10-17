// 「이전·다음 패널」 단축키가 밟는 줄 차례(src/sidebarModel.ts machineRows): 목록에 보이는 차례와 같아야 한다.
// 목록(Sidebar MachineGroup)도 같은 machineRoster로 그리므로, 여기서는 차례 규칙과 thread 줄이 여는 것을 본다.
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { machineRows, paneProblem } from "../src/sidebarModel.ts";

const m = { id: "m1", name: "mac-a" };
const pane = (id, machine_id = "m1", state = "running", role = "pane") => ({ id, machine_id, state, role });
const thread = (project, id, group, pane = null, updated_at_ms = 0) => ({ project, id, group, pane, machine: "mac-a", updated_at_ms });

describe("단축키 줄 차례", () => {
  it("프로젝트 묶음(처음 나온 차례)의 thread, 그다음 thread에 딸리지 않은 pane", () => {
    const panes = [pane("p1"), pane("p2"), pane("t1p"), pane("other", "m2"), pane("eval", "m1", "running", "evaluator")];
    const threads = [
      thread("beta", "t-2", "working", null, 5),
      thread("alpha", "t-1", "waiting_on_you", "t1p"),
      thread("beta", "t-3", "waiting_on_you", null),
    ];
    const keys = machineRows(m, panes, threads).map((r) => r.keys[0]);
    // 사람 차례(waiting_on_you)가 먼저라 alpha가 먼저 나오고, beta 안에서도 t-3이 t-2보다 앞
    assert.deepEqual(keys, ["thread:alpha/t-1", "thread:beta/t-3", "thread:beta/t-2", "pane:p1", "pane:p2"]);
  });

  it("thread 줄은 agent pane이 돌면 그 pane을, 끝났으면 보고서를 연다. 골랐다고 보는 값은 thread와 pane 둘 다", () => {
    const rows = machineRows(m, [pane("a"), pane("b", "m1", "exited")], [thread("p", "t-1", "working", "a"), thread("p", "t-2", "working", "b")]);
    assert.deepEqual(rows[0], { open: { pane: "a" }, keys: ["thread:p/t-1", "pane:a"] });
    assert.equal("thread" in rows[1].open, true);
    assert.deepEqual(rows[1].keys, ["thread:p/t-2", "pane:b"]);
  });
});

// 재부팅 뒤 복원 실패(plan §8.105): 참고 RestoreErrorBadge처럼 경고 하나에 까닭을 싣는다
describe("중단 pane 경고", () => {
  it("복원하지 못한 까닭이 있으면 그것을, 없으면 중단만", () => {
    const p = { ...pane("p1", "m1", "interrupted"), restore_error: "다시 띄우지 못했다: 작업 폴더가 없다: /w" };
    assert.equal(paneProblem(p), "중단: 다시 띄우지 못했다: 작업 폴더가 없다: /w");
    assert.equal(paneProblem(pane("p2", "m1", "interrupted")), "중단: agent backend가 다시 떴을 때 이 세션이 없었다");
    assert.equal(paneProblem(pane("p3")), null);
  });
});

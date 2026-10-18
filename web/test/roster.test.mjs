// 「이전·다음 패널」 단축키가 밟는 줄 차례(src/sidebarModel.ts rosterRows): 목록에 보이는 차례와 같아야 한다.
// 목록(Sidebar Roster)도 같은 roster로 그리므로, 여기서는 차례 규칙과 thread 줄이 여는 것을 본다.
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { paneProblem, rosterRows } from "../src/sidebarModel.ts";

const ma = { id: "m1", name: "mac-a", connection: "connected" };
const mb = { id: "m2", name: "mac-b", connection: "connected" };
const pane = (id, machine_id = "m1", state = "running", role = "pane") => ({ id, machine_id, state, role });
const thread = (project, id, group, pane = null, updated_at_ms = 0, machine = "mac-a") => ({ project, id, group, pane, machine, updated_at_ms });

describe("단축키 줄 차례", () => {
  it("프로젝트를 먼저 묶는다(plan §8.195): 모든 머신의 thread를 처음 나온 프로젝트 차례로, 그다음 「기타」 pane을 머신 차례로", () => {
    const panes = [pane("p1"), pane("b1", "m2"), pane("p2"), pane("t1p"), pane("t4p", "m2"), pane("gone", "m9"), pane("eval", "m1", "running", "evaluator")];
    const threads = [
      thread("beta", "t-2", "working", null, 5),
      thread("alpha", "t-1", "waiting_on_you", "t1p"),
      // mac-b pane의 beta thread도 같은 beta 묶음(머신마다 나뉘지 않는다)
      thread("beta", "t-4", "ready_for_review", "t4p"),
      thread("beta", "t-3", "waiting_on_you", null, 0, "m2"),
      // 모르는 머신의 thread는 목록에 없다
      thread("gamma", "t-9", "waiting_on_you", null, 0, "mac-z"),
    ];
    const keys = rosterRows([ma, mb], panes, threads).map((r) => r.keys[0]);
    // 확인 필요(waiting_on_you)가 먼저라 alpha가 먼저 나오고, beta 안은 rank·최근 차례
    assert.deepEqual(keys, ["thread:alpha/t-1", "thread:beta/t-3", "thread:beta/t-4", "thread:beta/t-2", "pane:p1", "pane:p2", "pane:b1"]);
  });

  it("사람이 옮긴 「기타」 차례(paneOrder)가 머신을 넘어 먼저, 끊긴 머신의 줄은 열 수 없어 뺀다", () => {
    const panes = [pane("p1"), pane("p2"), pane("b1", "m2")];
    const keys = (machines, order) => rosterRows(machines, panes, [thread("p", "t-1", "working", null, 0, "mac-b")], order).map((r) => r.keys[0]);
    assert.deepEqual(keys([ma, mb], ["m2:b1", "m1:p2"]), ["thread:p/t-1", "pane:b1", "pane:p2", "pane:p1"]);
    assert.deepEqual(keys([ma, { ...mb, connection: "disconnected" }], ["m2:b1"]), ["pane:p1", "pane:p2"]);
  });

  it("thread 줄은 agent pane이 돌면 그 pane을, 끝났으면 보고서를 연다. 골랐다고 보는 값은 thread와 pane 둘 다", () => {
    const rows = rosterRows([ma], [pane("a"), pane("b", "m1", "exited")], [thread("p", "t-1", "working", "a"), thread("p", "t-2", "working", "b")]);
    assert.deepEqual(rows[0], { open: { pane: "a" }, keys: ["thread:p/t-1", "pane:a"] });
    assert.equal("thread" in rows[1].open, true);
    assert.deepEqual(rows[1].keys, ["thread:p/t-2", "pane:b"]);
  });
});

// 재부팅 뒤 복원 실패(plan §8.111): 참고 RestoreErrorBadge처럼 경고 하나에 까닭을 싣는다
describe("중단 pane 경고", () => {
  it("복원하지 못한 까닭이 있으면 그것을, 없으면 중단만", () => {
    const p = { ...pane("p1", "m1", "interrupted"), restore_error: "다시 띄우지 못했다: 작업 폴더가 없다: /w" };
    assert.equal(paneProblem(p), "중단: 다시 띄우지 못했다: 작업 폴더가 없다: /w");
    assert.equal(paneProblem(pane("p2", "m1", "interrupted")), "중단: agent backend가 다시 떴을 때 이 세션이 없었다");
    assert.equal(paneProblem(pane("p3")), null);
  });
});

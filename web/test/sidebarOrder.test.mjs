// Agents 「활동순」(src/sidebarModel.ts activityOrder)과 그 설정 값. herdr-web-ui `src/lib/sidebarOrder.test.ts`의
// 「activity order」와 기본값 시험을 옮겼다. 참고의 herdr `state_change_seq` 자리는 host `AgentStatus.since_ms`이고,
// 참고 상태 말(blocked·working·idle·done)은 aaa 상태(question·working·idle·안 본 idle)로 만든다(sidebarModel paneAgentStatus).
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { activityOrder } from "../src/sidebarModel.ts";
import { sanitizeAppearance } from "../src/theme.ts";

const ACTIVITY = { blocked: ["question", true], working: ["working", true], idle: ["idle", true], done: ["idle", false] };
const pane = (id, status, since_ms) => {
  const [activity, seen] = ACTIVITY[status];
  return { id, state: "running", agent: since_ms === undefined ? null : { activity, seen, since_ms } };
};
const ids = (rows) => rows.map((p) => p.id);

describe("활동순", () => {
  const seqs = { idle: 50, working: 10, blocked: 1, "done-old": 20, "done-new": 30 };
  const agents = [
    pane("idle", "idle", seqs.idle),
    pane("working", "working", seqs.working),
    pane("blocked", "blocked", seqs.blocked),
    pane("done-old", "done", seqs["done-old"]),
    pane("done-new", "done", seqs["done-new"]),
    pane("nocount", "idle"),
  ];
  const order = (rows) => ids(activityOrder(rows, (p) => p));

  it("blocked가 먼저, 그다음 상태와 상관없이 가장 최근 변화", () => {
    assert.deepEqual(order(agents), ["blocked", "idle", "done-new", "done-old", "working", "nocount"]);
  });

  it("방금 일을 시킨 agent는 도는 동안에도 끝난 뒤에도 위에 남는다", () => {
    const sent = agents.map((p) => (p.id === "done-old" ? pane("done-old", "working", 60) : p));
    assert.deepEqual(order(sent).slice(0, 2), ["blocked", "done-old"]);
    const finished = agents.map((p) => (p.id === "done-old" ? pane("done-old", "done", 61) : p));
    assert.deepEqual(order(finished).slice(0, 2), ["blocked", "done-old"]);
  });

  it("새 agent는 위로, 같으면 받은 차례", () => {
    const fresh = [...agents, pane("new", "idle", 99), pane("tie", "idle")];
    assert.deepEqual(order(fresh).slice(0, 2), ["blocked", "new"]);
    // 변화 시각이 없는 줄은 받은 차례대로 맨 끝
    assert.deepEqual(order([...agents, pane("new", "idle"), pane("tie", "idle")]).slice(-3), ["nocount", "new", "tie"]);
  });
});

it("기본은 워크스페이스순이고 아는 값만 받는다", () => {
  assert.equal(sanitizeAppearance({}).agentOrder, "workspace");
  assert.equal(sanitizeAppearance({ agentOrder: "activity" }).agentOrder, "activity");
  assert.equal(sanitizeAppearance({ agentOrder: "recent" }).agentOrder, "workspace");
});

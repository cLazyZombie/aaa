import assert from "node:assert/strict";
import { it } from "node:test";
import { providerFailure } from "../src/providerFailure.ts";
import { paneProblem } from "../src/sidebarModel.ts";

it("native 원인·코드·실행/완료 시각을 표시하고 화면 오류와 구독 고갈을 섞지 않는다", () => {
  const agent = { activity: "idle", provider_turn: { generation_ms: 1000, started_ms: 2000, at_ms: 3000, finished_ms: 3000, error: { kind: "overloaded", code: "server_overloaded", http_status: 529 } } };
  for (const [kind, reason] of [["overloaded", "공급자 과부하"], ["authentication", "로그인 필요"], ["allowance", "구독·결제 한도"], ["model", "모델 오류"], ["rate_limit", "요청 한도"], ["unknown", "공급자 오류 · 원인 미확인"]]) {
    const value = providerFailure({ ...agent, provider_turn: { ...agent.provider_turn, error: { kind } } });
    assert.equal(value.reason, reason);
  }
  const value = providerFailure(agent);
  assert.match(value.evidence, /server_overloaded · HTTP 529 · 완료 .+ · 실행 /);
  assert.match(paneProblem({ state: "running", agent }), /^공급자 과부하/);
  assert.equal(providerFailure(null), null);
  assert.equal(providerFailure({ activity: "error", message: "usage limit" }), null);
  assert.equal(providerFailure({ ...agent, provider_turn: { ...agent.provider_turn, finished_ms: null } }), null);
  assert.equal(providerFailure({ ...agent, provider_turn: { ...agent.provider_turn, error: null } }), null);
  assert.equal(paneProblem({ state: "lost", agent }), "유실: 세션 기록을 찾지 못했다");
});

import type { AgentStatus } from "./gen/proto";

const REASON = {
  overloaded: "공급자 과부하",
  authentication: "로그인 필요",
  allowance: "구독·결제 한도",
  model: "모델 오류",
  rate_limit: "요청 한도",
  unknown: "공급자 오류 · 원인 미확인",
} as const;

/** 분류는 native 기록을 읽은 agent가 정한다. 화면 글·구독 수치로 추측하지 않는다. */
export function providerFailure(agent: AgentStatus | null | undefined): { reason: string; evidence: string; kind: string } | null {
  const turn = agent?.provider_turn;
  if (!turn?.error || turn.finished_ms == null) return null;
  const { error } = turn;
  const time = (at: number) => new Date(at).toLocaleString();
  return {
    reason: REASON[error.kind] ?? REASON.unknown,
    kind: error.kind,
    evidence: [error.code, error.http_status ? `HTTP ${error.http_status}` : null, `완료 ${time(turn.finished_ms)}`, `실행 ${time(turn.generation_ms)}`].filter(Boolean).join(" · "),
  };
}

// 특수 agent 패널(사용자 결정 10-19 「특수 에이전트도 … 왼쪽 아이콘에 등록」, plan §8.256): 임무·사후 평가자 줄의 상태와
// 활동 막대 배지. 배지는 사람이 볼 것만 센다: 막힘·오류·승인 필요 임무와 확인창이 걸린 평가자.
import type { EvaluatorView, MissionView, PaneView } from "./gen/proto";

/** 줄 색: 막힘(빨강)·일함(파랑)·사람 차례(승인, 노랑)·그 밖(회색) */
export type SpecialTone = "blocked" | "working" | "need" | "quiet";

export function missionTone(m: MissionView): SpecialTone {
  if (m.error || m.state === "막힘") return "blocked";
  if (!m.approved) return "need";
  return m.state === "일함" || m.state === "시작 중" ? "working" : "quiet";
}

/** 둘째 줄 첫 낱말: 오류·승인 필요가 host 상태 낱말보다 먼저 */
export function missionWord(m: MissionView): string {
  if (m.error) return "오류";
  return m.approved ? m.state : "승인 필요";
}

/** 평가자 pane의 확인창(질문) */
export const evaluatorAsks = (v: EvaluatorView, panes: readonly PaneView[]): boolean =>
  v.running && panes.some((p) => p.id === v.thread.pane && p.state === "running" && p.agent?.activity === "question");

export function evaluatorState(v: EvaluatorView, panes: readonly PaneView[]): { tone: SpecialTone; word: string } {
  const pane = panes.find((p) => p.id === v.thread.pane && p.state === "running");
  if (!v.running || !pane) return { tone: "quiet", word: "세션 끝남" };
  const act = pane.agent?.activity;
  if (act === "question") return { tone: "blocked", word: "막힘" };
  return act === "working" ? { tone: "working", word: "평가 중" } : { tone: "quiet", word: "대기" };
}

/** 활동 막대 「특수」 배지 수 */
export function specialAttention(missions: readonly MissionView[], evaluators: readonly EvaluatorView[], panes: readonly PaneView[]): number {
  return missions.filter((m) => missionTone(m) === "blocked" || missionTone(m) === "need").length + evaluators.filter((v) => evaluatorAsks(v, panes)).length;
}

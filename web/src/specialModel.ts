// 특수 agent 패널(사용자 결정 10-19 「특수 에이전트도 … 왼쪽 아이콘에 등록」, plan §8.258): 임무·사후 평가자 줄의 상태와
// 활동 막대 배지. 배지는 사람이 볼 것만 센다: 막힘·오류·승인 필요 임무와 확인창이 걸린 평가자.
import type { EvaluatorView, MissionView, PaneView, MissionBlockReason } from "./gen/proto";

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
  const reasons: Record<MissionBlockReason, string> = { question: "확인창", message_unknown: "입력 전달 모름", wake_unknown: "재시작 중 깨움 모름", brief_unknown: "brief 전달 모름", restart_failed: "다시 시작 실패", legacy_unknown: "이유 모름" };
  if (!m.approved) return "승인 필요";
  return m.state === "막힘" && m.blocked_reason ? `막힘 · ${reasons[m.blocked_reason]}` : m.state;
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

/** 무제한은 분모를 숨긴다. 옛 호스트의 u32::MAX도 호환한다. */
export function missionCount(value: number | bigint, cap = 0): string {
  return cap > 0 && cap < 4294967295 ? `${value}/${cap}` : String(value);
}
export function missionWork(m: MissionView): string {
  return `일한 분 ${m.work_minutes}${m.max_minutes > 0 && m.max_minutes < 4294967295 ? ` · 깨움 작업 ${missionCount(m.wake_work_minutes, m.max_minutes)}분` : ""}`;
}

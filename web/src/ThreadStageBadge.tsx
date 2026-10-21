import type { ThreadStage } from "./gen/proto";
import "./ThreadStageBadge.css";

export const STAGE_WORD: Record<ThreadStage, string> = { merge_ready: "병합 대기", landing: "착지 중" };

/** host가 회수한 첫 줄의 단계만 표시한다. harness 상태·진행률과 별개다. */
export function ThreadStageBadge({ stage }: { stage?: ThreadStage | null }) {
  if (!stage) return null;
  return <span className={`thread-stage is-${stage}`} data-testid="thread-stage" data-stage={stage}
    title={stage === "merge_ready" ? "구현·확인 완료, 착지 창 대기 · 실행 한도에서 제외" : "통합·검증·병합과 그 실패 수리 · 착지 한 자리만 실행 한도에서 제외"}>{STAGE_WORD[stage]}</span>;
}

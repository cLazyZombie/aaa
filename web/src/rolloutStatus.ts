import type { RolloutCard, RolloutState } from "./gen/proto";
export const rolloutLabels: Record<RolloutState, string> = {
  running: "배포 중", recovering: "복구 중", done: "배포 완료", stopped: "배포 중단",
  stopped_rolled_back: "복구 완료", recovery_failed: "복구 실패",
};
export const rolloutBusy = (state: RolloutState) => state === "running" || state === "recovering";
/** 완료는 그 머신의 모든 단계가 끝났을 때만 표시한다. 대상 판으로 실행 판을 추정하지 않는다. */
export function machineRollout(card: RolloutCard | null, machine: string) {
  const steps = card?.steps.filter((step) => step.machine === machine);
  if (!card || !steps?.length) return null;
  if (steps.every((step) => step.completed)) return { label: "배포 완료", busy: false };
  if (card.steps[card.cursor]?.machine === machine)
    return { label: rolloutLabels[card.state], busy: rolloutBusy(card.state) };
  return { label: card.state === "running" ? "배포 대기" : "미진행", busy: false };
}

export function aaaVersion(software: string) {
  return `aaa ${software.replace(/^aaa\s+/, "")}`;
}

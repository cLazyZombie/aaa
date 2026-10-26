import type { MetricPoint, MetricRun } from "./gen/proto";

/** 날짜별 점의 run_id는 p50 대표 표본이다. 기준 실행은 같은 날짜·머신·profile 묶음으로 찾는다. */
export function referencePoint(points: MetricPoint[], run: MetricRun | undefined, daily: boolean, utc: boolean): MetricPoint | undefined {
  if (!run || (daily && run.outcome !== "succeeded")) return;
  const day = (ms: number) => Math.floor((ms + (utc ? 0 : 540 * 60000)) / 86400000);
  return points.find((p) => daily
    ? p.machine === run.machine && p.profile === run.profile && day(p.at_ms) === day(run.observed_at_ms)
    : p.run_id === run.run_id);
}

export function referenceNotice(run: MetricRun | undefined, requested: boolean, visible: boolean, metric: "verify" | "build", daily: boolean): string | null {
  if (!run) return requested ? "기준 실행을 이 workspace 기록에서 찾을 수 없습니다. 다른 실행을 고르세요." : null;
  if (!visible) return "기준 실행이 현재 그래프 범위 밖입니다. 기간·머신·profile 조건을 확인하세요.";
  if ((metric === "build" ? run.build_ms : run.verification_ms) == null) return `기준 실행의 ${metric === "build" ? "빌드" : "검증"} 시간이 미수집입니다.`;
  if (daily && run.outcome !== "succeeded") return "기준 실행은 성공한 실행이 아니므로 날짜별 p50에서 제외됩니다. 시각별에서 확인하세요.";
  return null;
}

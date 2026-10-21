// 표시 판단만 한다. 수집·계정 귀속·창 합치기는 host가 맡는다(A2).
import type { AccountQuotaWindow, QuotaProblem } from "./gen/proto";

export const quotaProblem: Record<QuotaProblem, string> = {
  unknown: "사용량 모름", expired: "인증 만료", rate_limited: "조회 제한", locked: "인증 잠김",
  disconnected: "머신 연결 끊김", unsupported: "조회 지원 안 됨", failed: "조회 실패", clock_skew: "머신 시각 차이",
};
export function quotaLabel(w: AccountQuotaWindow): string {
  const period = w.duration_mins === 300 ? "5시간" : w.duration_mins === 10080 ? "주간" : w.duration_mins ? `${w.duration_mins}분 창` : "기타 창";
  return w.scope ? `${period} · ${w.scope}` : period;
}
export function quotaState(w: AccountQuotaWindow, now: number): "fresh" | "stale" | "unknown" {
  if (!Number.isFinite(w.used_percent) || w.used_percent < 0 || w.used_percent > 100 || w.problem
      || !w.observed_at_ms || w.observed_at_ms > now + 30_000 || now - w.observed_at_ms > 600_000
      || (w.resets_at_ms !== null && w.resets_at_ms <= now)) return "unknown";
  if (w.freshness === "unknown") return "unknown";
  return w.freshness === "stale" || now - w.observed_at_ms > 120_000 ? "stale" : "fresh";
}
export function quotaLeft(w: AccountQuotaWindow): string {
  return Number.isFinite(w.used_percent) && w.used_percent >= 0 && w.used_percent <= 100 ? `${Math.round((100 - w.used_percent) * 10) / 10}% 남음` : "사용량 모름";
}
export function quotaTime(ms: number | null): string {
  return ms && Number.isFinite(ms) && Number.isFinite(new Date(ms).getTime()) ? new Date(ms).toLocaleString("ko-KR", { month: "numeric", day: "numeric", hour: "2-digit", minute: "2-digit", hour12: false }) : "모름";
}

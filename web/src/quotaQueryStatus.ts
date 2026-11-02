import type { UsageProblem } from "./gen/proto";

export function quotaQueryStatus(status: UsageProblem | null | undefined): string | null {
  if (!status) return null;
  const labels: Partial<Record<UsageProblem, string>> = {
    unobserved: "조회 인증 필요", denied: "조회 접근 거부", expired: "조회 인증 만료",
    rate_limited: "조회 대기 중", failed: "조회 실패", locked: "조회 인증 잠김",
  };
  return labels[status] ?? "조회 실패";
}

import type { ProviderUsage } from "./gen/proto";

/** 수치가 없는 칩에도 읽지 못한 까닭을 남긴다. */
export function usageProblemLabel(usage: ProviderUsage): string {
  switch (usage.problem) {
    case "expired": return "인증 만료";
    case "rate_limited": return "조회 제한";
    case "locked": return "인증 잠김";
    case "unobserved": return "관측 대기";
    case "stale": return "오래된 관측";
    case "disconnected": return "연결 끊김";
    case "unsupported": return "수집 미지원";
    case "denied": return "접근 거부";
    case "failed": return "조회 실패";
    default: return "한도 없음";
  }
}

export function usageProblemText(usage: ProviderUsage): string | null {
  switch (usage.problem) {
    case "expired": return usage.id === "claude"
      ? "사용량 조회 인증이 만료됐다. Claude Code의 인증 갱신이 필요하다."
      : "로그인이 끝났다. Codex를 열어 다시 로그인한다.";
    case "rate_limited": return "사용량 조회가 제한됐다. 잠시 뒤 다시 읽는다.";
    case "locked": return "사용량 인증을 host가 읽을 수 없다.";
    case "unobserved": return "이 계정의 Claude Code 상태줄 사용량을 아직 관측하지 못했다.";
    case "stale": return "상태줄 관측이 오래됐거나 한도 초기화 시각이 지났다. 마지막 숫자다.";
    case "disconnected": return "이 계정의 사용량을 수집하는 머신의 연결이 끊겼다.";
    case "unsupported": return "이 머신은 Claude Code 상태줄 사용량 수집을 지원하지 않는다.";
    case "denied": return "Claude 사용량 API가 접근을 거부했다(HTTP 403). 인증 만료 여부는 확인되지 않았다.";
    case "failed": return "사용량을 읽지 못했다. host의 수집 오류를 확인한다.";
    default: return null;
  }
}

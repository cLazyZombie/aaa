import { ApiError } from "./apiOutcome.ts";

/** 서버의 공개 오류만 메시지를 쓴다. 브라우저/파서 오류의 message에는 입력·URL·인증 본문이 들어갈 수 있다. */
export function diagnosticReason(error: unknown): string {
  if (error instanceof ApiError) return `${error.body.code}: ${error.message}`;
  if (error instanceof Error) {
    const allowed = ["Error", "TypeError", "SyntaxError", "RangeError", "AbortError", "NotAllowedError", "NotSupportedError", "SecurityError", "QuotaExceededError", "InvalidStateError", "NotFoundError", "NetworkError"];
    return allowed.includes(error.name) ? error.name : "브라우저 작업 오류";
  }
  return "원인 형식을 알 수 없는 오류";
}

/** 선택 자료·정리가 실패해도 기존 fallback을 유지한다. 진단에는 요청 인자나 원문을 덧붙이지 않는다. */
export function logIgnoredFailure(error: unknown, stage: string): void {
  console.debug(`${stage}: 기존 복구·기본값 사용`, diagnosticReason(error));
}

export function ignoredFailure<T>(stage: string, recover: () => T): (error: unknown) => T {
  return (error) => { logIgnoredFailure(error, stage); return recover(); };
}

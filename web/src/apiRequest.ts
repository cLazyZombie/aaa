/** 페이지 종료 뒤 예약된 조회가 새 HTTP를 시작하지 않게 하는 앱 API 전송 경계.
 * 숨긴 탭은 그대로 동작한다. pagehide만 닫고, BFCache의 pageshow는 새 요청을 다시 허용한다.
 * 떠날 때 명시한 keepalive 저장은 유지하며 취소한 쓰기를 복귀 때 자동 재생하지 않는다. */
let pageClosed = false;
if (typeof window !== "undefined") {
  window.addEventListener("pagehide", () => { pageClosed = true; }, { capture: true });
  window.addEventListener("pageshow", () => { pageClosed = false; }, { capture: true });
}

export function apiFetch(input: RequestInfo | URL, init?: RequestInit): Promise<Response> {
  if (pageClosed && !init?.keepalive) {
    return Promise.reject(new DOMException("페이지가 종료되어 요청을 취소했습니다.", "AbortError"));
  }
  return fetch(input, init);
}

/** cap·query·origin을 표시/진단에 남기지 않고 같은 host의 보고서 파일만 확인한다. */
export function reportMediaTarget(src: string, origin: string): { url: string; path: string } | null {
  try {
    const url = new URL(src, origin);
    const path = /^\/v\/[^/]+\/(.+)$/.exec(url.pathname)?.[1];
    if (url.origin !== origin || url.username || url.password || !path) return null;
    url.hash = "";
    return { url: url.href, path: decodeURIComponent(path) };
  } catch { return null; }
}

export type MediaStatus = { status: number | null; size: number | null; message: string };

/** 사용자 누름에서 HEAD 한 번만 보낸다. 원문 오류·Location·쿠키·cap은 읽거나 저장하지 않는다. */
export async function readReportMediaStatus(url: string, signal: AbortSignal, request: typeof fetch = fetch): Promise<MediaStatus> {
  const controller = new AbortController();
  const abort = () => controller.abort();
  signal.addEventListener("abort", abort, { once: true });
  if (signal.aborted) abort();
  const timer = setTimeout(abort, 5000);
  try {
    const response = await request(url, { method: "HEAD", credentials: "omit", cache: "no-store", redirect: "error", signal: controller.signal });
    const length = response.headers.get("content-length");
    const size = length !== null && /^\d+$/.test(length) && Number.isSafeInteger(Number(length)) ? Number(length) : null;
    const message = response.status === 401 ? "보기 권한이 끝났습니다. 보고서나 질문을 다시 열어 주세요."
      : response.status === 404 ? "이 보고 판에 파일이 없습니다."
      : response.status === 403 ? "이 파일을 읽을 수 없습니다."
      : response.ok ? "파일 응답 정상" : "파일을 읽지 못했습니다.";
    return { status: response.status, size: response.ok ? size : null, message };
  } catch (error) {
    if (signal.aborted) throw error;
    return { status: null, size: null, message: "응답을 받지 못했습니다. 연결을 확인해 주세요." };
  } finally {
    clearTimeout(timer); signal.removeEventListener("abort", abort);
  }
}

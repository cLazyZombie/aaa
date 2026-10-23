import type { ErrorBody } from "./gen/proto";

export class ApiError extends Error {
  body: ErrorBody;
  constructor(body: ErrorBody) { super(body.message); this.body = body; }
}

/** 서버가 보낸 안전한 오류는 그대로 보인다. HTML·손상된 JSON 원문은 인증 자료를 포함할 수 있어 표시하지 않는다. */
export async function readApiResponse<T>(res: Response): Promise<T> {
  let out: unknown;
  try {
    out = await res.json();
  } catch (error) {
    const reason = error instanceof SyntaxError ? "응답 JSON 형식 오류" : "응답 본문 읽기 실패";
    throw new ApiError({ code: "internal", message: `HTTP ${res.status}: ${reason}` });
  }
  if (typeof out === "object" && out !== null && "err" in out) {
    const error = out.err as Partial<ErrorBody> | null;
    if (error && typeof error.code === "string" && typeof error.message === "string") {
      throw new ApiError(error as ErrorBody);
    }
  }
  if (!res.ok || typeof out !== "object" || out === null || !("ok" in out) || "err" in out) {
    throw new ApiError({ code: "internal", message: `HTTP ${res.status}: host 응답 형식 오류` });
  }
  return out.ok as T;
}

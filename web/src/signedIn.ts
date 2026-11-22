// 앱을 다시 열 때(t-2150) 이 브라우저가 마지막으로 확인한 기기(역할·ID)를 기억한다. 있으면 App은 기기 확인(GET /api/v1/me)
// 응답을 기다리지 않고 바로 본 화면을 띄워 상태 연결·목록 조회를 함께 시작한다. 쿠키가 실제 인증이고 host가 매 요청을
// 판정하므로 여기 값은 화면을 고르는 데만 쓴다. 확인 응답이 다르면(취소·역할 바뀜) App이 그 응답으로 고친다.
import type { DeviceRole } from "./gen/proto";

export type SignedIn = { role: DeviceRole; device: string };

const KEY = "aaa:signed-in";
const ROLES: readonly DeviceRole[] = ["reader", "controller", "local"];

function local(): Storage | null {
  try {
    return typeof localStorage === "undefined" ? null : localStorage;
  } catch {
    return null; // 사생활 보호 창·막힌 저장소
  }
}

/** 마지막으로 확인한 기기. 없거나 읽을 수 없으면 null(그때는 확인 응답을 기다린다) */
export function rememberedSignIn(storage: Storage | null = local()): SignedIn | null {
  try {
    const raw = storage?.getItem(KEY);
    if (!raw) return null;
    const v = JSON.parse(raw) as Partial<SignedIn>;
    if (typeof v.device !== "string" || v.device === "" || !ROLES.includes(v.role as DeviceRole)) return null;
    return { role: v.role as DeviceRole, device: v.device };
  } catch {
    return null;
  }
}

/** 확인 응답을 기억한다. null(401·로그아웃·취소)이면 지운다 */
export function rememberSignIn(signedIn: SignedIn | null, storage: Storage | null = local()): void {
  try {
    if (signedIn) storage?.setItem(KEY, JSON.stringify({ role: signedIn.role, device: signedIn.device }));
    else storage?.removeItem(KEY);
  } catch {
    /* 저장하지 못하면 다음에도 확인 응답을 기다린다 */
  }
}

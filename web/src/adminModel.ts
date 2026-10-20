// 머신 등록·기기·웹 공개·보고서 올리기·키 교체의 순수 규칙(관리 GUI W5b, plan §8.352). 판정의 정본은 host다
// (웹 조작 기기만, 마지막 조작 기기·host 머신 폐기 거부, 등록 코드 3번 틀리면 버림, 올리기 경로·상한). 화면은 고를 수 있는 것만 보인다.
import { apiAtLeast } from "./automationModel.ts";
import type { DeviceView, LinkView, PublishState } from "./gen/proto";

/** 웹의 머신 등록·공개 바꾸기·보고서 올리기·키 교체는 1.80 이상 host만 안다 */
export const canAdmin = (frontApi: string | null | undefined): boolean => apiAtLeast(frontApi, 1, 80);

/** 새 머신 화면의 확인 코드: 숫자만 6자리(사이 공백·하이픈은 host도 뺀다) */
export function enrollCode(text: string): string | null {
  const digits = text.replace(/\D/g, "");
  return digits.length === 6 ? digits : null;
}

/** 등록 창이 닫히기까지(올림 분). 닫혔으면 null */
export function enrollLeft(openUntilMs: number | undefined, now: number): number | null {
  if (openUntilMs === undefined || openUntilMs <= now) return null;
  return Math.ceil((openUntilMs - now) / 60_000);
}

/** 이 기기가 마지막 조작 기기인가(웹에서는 폐기하지 못한다, 사용자 답 2A) */
export function lastController(devices: DeviceView[], id: string): boolean {
  const controllers = devices.filter((d) => d.role === "controller");
  return controllers.length === 1 && controllers[0]!.id === id;
}

/** 이 화면이 공개 주소로 열려 있나: 끄면 이 화면이 끊길 수 있다(사용자 답 3A) */
export function onPublicAddress(pageOrigin: string, publish: PublishState | null): boolean {
  if (!publish?.url) return false;
  try {
    return new URL(publish.url).origin === new URL(pageOrigin).origin;
  } catch {
    return false;
  }
}

/**
 * 공개를 끈 뒤에도 남는 접속 경로(사람이 확인할 것). host는 loopback과 공개 주소의 Host만 받으므로(DNS rebinding 방어) 다른 기기는
 * host Mac으로 포트를 전달해 loopback 주소로 연다(새 주소라 다시 페어링한다)
 */
export function otherRoutes(publish: PublishState): string[] {
  const port = publish.web_port;
  return [
    `host Mac에서 http://127.0.0.1:${port}`,
    `다른 기기에서 ssh -L ${port}:127.0.0.1:${port} <host Mac> 뒤 같은 주소(다시 페어링)`,
    "다시 공개: host Mac 터미널의 aaa web publish",
  ];
}

/** host `name_ok`와 같다: 소문자·숫자·-·_ 1~100자, `t-`로 시작하지 않는다 */
export function reportNameOk(name: string): boolean {
  return /^[a-z0-9_-]{1,100}$/.test(name) && !name.startsWith("t-") && !["con", "prn", "aux", "nul"].includes(name) && !/^(com|lpt)[1-9]$/.test(name);
}

export const FILE_MAX = 16 << 20;
export const TOTAL_MAX = 64 << 20;

export type Picked = { name: string; relative: string; size: number };
/**
 * 고른 파일들의 보고서 안 경로. 폴더를 고르면(`webkitRelativePath` = `폴더/…`) 맨 위 폴더 이름을 뺀다. 숨은 이름(`.`)은 빼고
 * (host도 거부한다), 상한을 넘으면 까닭을 준다
 */
export function uploadPlan(files: Picked[]): { paths: { path: string; index: number }[]; skipped: string[]; error: string | null } {
  const paths: { path: string; index: number }[] = [];
  const skipped: string[] = [];
  let total = 0;
  files.forEach((f, index) => {
    const rel = f.relative ? f.relative.split("/").slice(1).join("/") : f.name;
    if (!rel || rel.split("/").some((c) => c === "" || c.startsWith("."))) {
      skipped.push(f.relative || f.name);
      return;
    }
    paths.push({ path: rel, index });
    total += f.size;
  });
  // 뺀 숨은 파일(`.git` 묶음 등)은 크기를 따지지 않는다
  const big = paths.map((p) => files[p.index]!).find((f) => f.size > FILE_MAX);
  const error =
    paths.length === 0
      ? "올릴 파일이 없다"
      : big
        ? `${big.name}: 파일 하나는 16MB까지다`
        : total > TOTAL_MAX
          ? "합계 64MB까지 올린다"
          : paths.length > 1000
            ? "파일은 1000개까지다"
            : null;
  return { paths, skipped, error };
}

/** 키를 바꾸면 끊길 공유 링크: 폐기되지 않고 만료 전인 것 */
export const liveLinks = (links: LinkView[], now: number): LinkView[] => links.filter((l) => !l.revoked && l.expires_at_ms > now);

/** 올리기 이름(host `valid_upload`: 소문자 hex 32자) */
export function newUploadId(): string {
  const b = new Uint8Array(16);
  crypto.getRandomValues(b);
  return Array.from(b, (x) => x.toString(16).padStart(2, "0")).join("");
}

import { logIgnoredFailure } from "./errorDiagnostic.ts";
// 보고서 그림 크게 보기(plan §8.353). 보고서 HTML은 opaque origin sandbox iframe이라 앱이 그 안을 보지 못한다. host가 HTML 탐색
// 응답 끝에 붙인 알림(aaa-host `report_images.js`)이 누른 그림과 그 문서의 그림 주소를 보낸다. 보고서 제 script도 같은 꼴을
// 보낼 수 있으므로 믿지 않고 거른다: 그 보고서의 보기 경로(`/v/<cap>/`) 아래 같은 origin 주소만 받는다(data:·blob:·다른
// 보고서·API 주소는 버린다). 그래서 흉내 낸 메시지가 할 수 있는 일은 「그 보고서 제 그림을 앱 크게 보기로 연다」뿐이다.

import { askFileUrl } from "./asksModel.ts";

export type LightboxItem = { src: string; alt: string };

/** 한 번에 넘기는 그림 수(보고서가 그 이상이면 누른 그림 둘레만) */
export const MAX_REPORT_IMAGES = 500;
const MAX_ALT = 300;

/** 보기 경로 주소(`/v/<cap>/…`, 상대·절대)의 그 보고서 뿌리 `/v/<cap>/`. 같은 origin 보기 경로가 아니면 null */
export function viewRoot(src: string, origin: string): string | null {
  let url: URL;
  try {
    url = new URL(src, origin);
  } catch (ignoredError) { logIgnoredFailure(ignoredError, "reportImages.ts:20");
    return null;
  }
  const m = /^\/v\/([^/]+)\//.exec(url.pathname);
  return url.origin === origin && m ? `/v/${m[1]}/` : null;
}

/** `base`(그 보고서의 보기 경로, 상대·절대 모두) 아래 같은 origin 주소면 그 주소, 아니면 null */
export function reportImageUrl(src: unknown, base: string, origin: string): string | null {
  if (typeof src !== "string" || src === "") return null;
  let url: URL;
  let root: URL;
  try {
    root = new URL(base, origin);
    url = new URL(src, root);
  } catch (ignoredError) { logIgnoredFailure(ignoredError, "reportImages.ts:35");
    return null;
  }
  if (url.origin !== origin || root.origin !== origin) return null;
  // URL이 `..`·`%2e%2e`를 풀어 둔 경로로 본다
  if (!root.pathname.startsWith("/v/") || !root.pathname.endsWith("/") || !url.pathname.startsWith(root.pathname)) return null;
  if (url.pathname === root.pathname) return null;
  url.hash = "";
  return url.href;
}

/** 보고서 iframe이 보낸 메시지 → 크게 볼 그림들과 누른 자리. 꼴이 틀리거나 누른 그림이 그 보고서 것이 아니면 null */
export function reportImages(data: unknown, base: string, origin: string): { items: LightboxItem[]; at: number } | null {
  if (data === null || typeof data !== "object") return null;
  const m = data as Record<string, unknown>;
  if (m.type !== "aaa:report-image" || !Array.isArray(m.images) || typeof m.at !== "number" || !Number.isInteger(m.at)) return null;
  let items: LightboxItem[] = [];
  let at = -1;
  for (const [i, raw] of m.images.entries()) {
    if (raw === null || typeof raw !== "object") continue;
    const r = raw as Record<string, unknown>;
    const src = reportImageUrl(r.src, base, origin);
    if (src === null) continue;
    if (i === m.at) at = items.length;
    items.push({ src, alt: typeof r.alt === "string" ? r.alt.slice(0, MAX_ALT) : "" });
  }
  if (at < 0) return null;
  if (items.length > MAX_REPORT_IMAGES) {
    const start = Math.min(Math.max(0, at - MAX_REPORT_IMAGES / 2), items.length - MAX_REPORT_IMAGES);
    items = items.slice(start, start + MAX_REPORT_IMAGES);
    at -= start;
  }
  return { items, at };
}

/** 보고 글(report.md)의 그림 `library/…` → 그 보고서 보기 경로(`base`, library 폴더가 뿌리) 아래 주소. 그 밖·`..`·숨은 파일은 null */
export function reportMarkdownImage(base: string, src: string): string | null {
  // 질문 본문과 같은 거름(asksModel `askFileUrl`): `/library/<인코딩한 경로>`를 받아 보기 경로로 옮긴다
  const url = askFileUrl("/", src);
  return url?.startsWith("/library/") ? base + url.slice("/library/".length) : null;
}

/** 앱이 그린 보고 글(Markdown) 안 그림을 눌렀을 때: 그 글의 그림들과 누른 자리. 링크·단추 안 그림은 그 링크·단추가 받는다 */
export function markdownImages(root: Element, target: EventTarget | null): { items: LightboxItem[]; at: number } | null {
  const img = target instanceof Element ? target.closest("img") : null;
  if (!img || !root.contains(img) || img.closest("a[href], button")) return null;
  const list = Array.from(root.querySelectorAll("img")).filter((i) => !i.closest("a[href], button"));
  return { items: list.map((i) => ({ src: i.currentSrc || i.src, alt: i.alt })), at: list.indexOf(img) };
}

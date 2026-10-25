import { normalizeAppTarget } from "./appIds.ts";
import { logIgnoredFailure } from "./errorDiagnostic.ts";
// 앱 안 참조(plan §8.329): coordinator가 채팅·post 제목에 쓰는 `aaa:question/<thread>`(그 thread의 질문 카드)와
// `aaa:report/<이름>`(보고서 패널의 그 보고서). 터미널에서도 눌리게 쓰는 같은 host 링크 `<웹 주소>/#question=<thread>`·
// `<웹 주소>/#report=<이름>`(plan §8.345)도 같은 참조다(`appLinkOf`). 웹 대화는 이것을 누르면 앱 안에서 여는 칩으로 그리고, 터미널에서는 글 그대로 읽힌다.
// 대상 글자는 host `notify::app_ref`와 같다(영문·숫자·`-`·`_`·`.`). 글 속에서는 앞이 낱말 글자가 아니어야 하고, 꼬리의 `.`는
// 문장 끝으로 본다(「…aaa:report/demo.」). 한국어 조사(「aaa:question/t-0001에」)는 대상 글자가 아니라 저절로 끊긴다.

export type AppRef = { kind: "question" | "report"; target: string; rawTarget?: string };

const REF = /(?<![A-Za-z0-9_.:/-])aaa:(question|report)\/([A-Za-z0-9_.-]+)/g;

/** 글을 보통 글과 참조로 나눈다. 참조가 없으면 글 하나 그대로다 */
export function splitAppRefs(text: string): (string | AppRef)[] {
  const out: (string | AppRef)[] = [];
  let at = 0;
  for (const m of text.matchAll(REF)) {
    const target = m[2]!.replace(/\.+$/, "");
    if (target === "") continue;
    const start = m.index;
    const end = start + "aaa:".length + m[1]!.length + 1 + target.length;
    if (start > at) out.push(text.slice(at, start));
    const normalized = normalizeAppTarget(target, m[1] === "question" ? "qt" : "rt");
    out.push({ kind: m[1] as AppRef["kind"], target: normalized, ...(m[1] === "report" && normalized !== target ? { rawTarget: target } : {}) });
    at = end;
  }
  if (at < text.length) out.push(text.slice(at));
  return out.length === 0 ? [text] : out;
}

/** 글 전체가 참조 하나인가(backtick에 넣은 참조, Markdown 링크 주소) */
export function appRefOf(text: string): AppRef | null {
  const parts = splitAppRefs(text.trim());
  return parts.length === 1 && typeof parts[0] !== "string" ? parts[0]! : null;
}


/**
 * 같은 host 앱 주소 링크의 참조(plan §8.345): `https://<이 화면의 origin>/#question=<thread>`·`/#report=<이름>`. 앱 자신의
 * `#report=<프로젝트>/<이름>`도 이름으로 읽는다. 다른 origin·다른 경로·모르는 주소는 null(보통 링크)
 */
export function appLinkOf(href: string, origin: string): AppRef | null {
  let u: URL;
  try {
    u = new URL(href.trim(), origin);
  } catch (ignoredError) { logIgnoredFailure(ignoredError, "appRef.ts:45");
    return null;
  }
  if (u.origin !== origin || (u.pathname !== "/" && u.pathname !== "") || u.search !== "") return null;
  const m = /^#(question|report)=(.+)$/.exec(u.hash);
  if (!m) return null;
  let target: string;
  try {
    target = decodeURIComponent(m[2]!);
  } catch (ignoredError) { logIgnoredFailure(ignoredError, "appRef.ts:54");
    return null;
  }
  if (!/^(?:[a-z0-9_][a-z0-9_-]{0,47}\/)?[A-Za-z0-9_.-]+$/.test(target)) return null;
  const normalized = normalizeAppTarget(target, m[1] === "question" ? "qt" : "rt");
  return { kind: m[1] as AppRef["kind"], target: normalized, ...(m[1] === "report" && normalized !== target ? { rawTarget: target } : {}) };
}

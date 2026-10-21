// 짧은 ID는 host가 알려 준 대상만 앱 안 링크로 그린다. 주소·경로 속 ID는 건드리지 않는다.
export type AppId = { kind: "thread" | "task" | "question" | "report"; target: string };
const KINDS = { t: "thread", d: "task", q: "question", r: "report" } as const;
const ID = /(?<![A-Za-z0-9_./:@%?#=&+-])([tdqr])-\d{4,}(?![A-Za-z0-9_/-]|\.[A-Za-z0-9])/g;

export function splitAppIds(text: string, exists: (id: AppId) => boolean): (string | AppId)[] {
  const out: (string | AppId)[] = [];
  let at = 0;
  for (const m of text.matchAll(ID)) {
    const id: AppId = { kind: KINDS[m[1] as keyof typeof KINDS], target: m[0] };
    if (!exists(id)) continue;
    if (m.index > at) out.push(text.slice(at, m.index));
    out.push(id);
    at = m.index + m[0].length;
  }
  if (at < text.length) out.push(text.slice(at));
  return out.length ? out : [text];
}

/** 프로젝트를 붙인 주소도 받는다. 잘못된 주소는 다른 할 일을 고르지 않는다. */
export function taskTarget(value: string): { project: string | null; id: string } | null {
  const m = /^(?:([^/]+)\/)?(d-\d{4,})$/.exec(value);
  return m ? { project: m[1] ?? null, id: m[2]! } : null;
}

export const appIdHash = (id: AppId): string => `#${id.kind}=${encodeURIComponent(id.target)}`;

// 짧은 ID는 host가 알려 준 대상만 앱 안 링크로 그린다. 주소·경로 속 ID는 건드리지 않는다.
export type AppId = { kind: "thread" | "task" | "question" | "report" | "automation" | "idea"; target: string; label?: string };
const KINDS = { t: "thread", d: "task", q: "question", r: "report", a: "automation", n: "idea" } as const;
const ID = /(?<![A-Za-z0-9_./:@%?#=&+-])([tdqran])-\d+(?![A-Za-z0-9_/-]|\.[A-Za-z0-9])/g;

export function splitAppIds(text: string, exists: (id: AppId) => boolean): (string | AppId)[] {
  const out: (string | AppId)[] = [];
  let at = 0;
  for (const m of text.matchAll(ID)) {
    const target = canonicalShortId(m[0], m[1]!);
    if (target === null) continue;
    const id: AppId = { kind: KINDS[m[1] as keyof typeof KINDS], target, ...(target === m[0] ? {} : { label: m[0] }) };
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
  const m = /^(?:([a-z0-9_][a-z0-9_-]{0,47})\/)?(d-\d+)$/.exec(value);
  const id = m ? canonicalShortId(m[2]!, "d") : null;
  return m && id ? { project: m[1] ?? null, id } : null;
}

export const appIdHash = (id: AppId): string => `#${id.kind}=${encodeURIComponent(id.target)}`;

/** JS Number로 바꾸지 않아 SQLite i64 끝에서도 숫자를 반올림하지 않는다. */
export function canonicalShortId(value: string, prefix: string): string | null {
  if (!/^[tdqrapn]$/.test(prefix) || !value.startsWith(`${prefix}-`)) return null;
  const digits = value.slice(2);
  if (!/^\d+$/.test(digits)) return null;
  const n = digits.replace(/^0+/, "");
  if (!n || n.length > 19 || n.length === 19 && n > "9223372036854775807") return null;
  return `${prefix}-${n.padStart(prefix === "p" ? 8 : 4, "0")}`;
}

/** 프로젝트가 있으면 그대로 둔다. 경로나 다른 종류의 번호는 고치지 않는다. */
export function normalizeAppTarget(value: string, prefixes: string): string {
  const m = /^(?:([a-z0-9_][a-z0-9_-]{0,47})\/)?([a-z]-\d+)$/.exec(value);
  if (!m || !prefixes.includes(m[2]![0]!)) return value;
  const id = canonicalShortId(m[2]!, m[2]![0]!);
  return id ? `${m[1] ? `${m[1]}/` : ""}${id}` : value;
}

/** 명시 프로젝트 열쇠를 유지하고, 여러 프로젝트의 같은 짧은 번호는 고르지 않는다. */
export function uniqueAppTargets<T>(rows: readonly T[], project: (row: T) => string, id: (row: T) => string): Map<string, T> {
  const out = new Map<string, T>();
  const owners = new Map<string, string>();
  for (const row of rows) {
    const key = id(row), scope = project(row);
    out.set(`${scope}/${key}`, row);
    if (!owners.has(key)) { owners.set(key, scope); out.set(key, row); }
    else if (owners.get(key) !== scope) out.delete(key);
  }
  return out;
}

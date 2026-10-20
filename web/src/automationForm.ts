// 자동화 편집 초안의 순수 규칙(관리 GUI W2, plan §8.342). 폼 값은 host 정본 타입(`automations/*.toml`)과 같은 모양이다:
// 화면은 이 값을 고치고, host(`automation_validate`)가 바뀐 칸만 원문 TOML에 합친다. 기본값 채우기·검사의 정본은 host다.
import { apiAtLeast } from "./automationModel.ts";

export type ConditionKind = "schedule" | "command" | "thread" | "pr" | "ci" | "git";
export type ActionKind = "inbox" | "message" | "start" | "deploy";
export type Form = Record<string, unknown> & { on?: Record<string, Record<string, unknown>>; do?: Record<string, Record<string, unknown>> | "deploy"; limits?: Record<string, unknown> };

/** 편집·지금 실행·지운 목록은 1.76 이상 host만 안다(옛 host는 모르는 요청으로 거절한다) */
export const canEditAutomations = (frontApi: string | null | undefined): boolean => apiAtLeast(frontApi, 1, 76);

export const conditionKinds: Array<{ kind: ConditionKind; label: string }> = [
  { kind: "schedule", label: "일정" },
  { kind: "command", label: "명령 출력 변화" },
  { kind: "thread", label: "thread 사건" },
  { kind: "pr", label: "PR 사건" },
  { kind: "ci", label: "CI 결과" },
  { kind: "git", label: "Git push" },
];
export const actionKinds: Array<{ kind: ActionKind; label: string }> = [
  { kind: "inbox", label: "coordinator 안내" },
  { kind: "message", label: "thread에 글" },
  { kind: "start", label: "새 thread 시작" },
  { kind: "deploy", label: "배포 제안" },
];
export const PR_EVENTS = ["opened", "checks-failed", "review", "merged"];
export const CI_EVENTS = ["failed", "stuck", "fixed", "passed"];
export const GIT_EVENTS = ["pushed", "forced", "deleted"];
/** 새 규칙의 사용량 문턱 기본값(계획 Q3: 5시간 창 5%·주간 10% 아래면 쉼) */
export const USAGE_DEFAULTS = { usage_session_below: 5, usage_week_below: 10 };

/** 새 규칙의 첫 초안: 매일 아침 coordinator 안내, 사용량 문턱 기본값을 채운다 */
export function newForm(): Form {
  return { title: "", on: { schedule: { daily: "09:00" } }, do: { inbox: { text: "" } }, limits: { ...USAGE_DEFAULTS } };
}

export function conditionKind(form: Form): ConditionKind | null {
  const key = Object.keys(form.on ?? {})[0];
  return conditionKinds.some((c) => c.kind === key) ? (key as ConditionKind) : null;
}
export function actionKind(form: Form): ActionKind | null {
  if (form.do === "deploy") return "deploy";
  const key = Object.keys(form.do ?? {})[0];
  return actionKinds.some((a) => a.kind === key) ? (key as ActionKind) : null;
}
export const condition = (form: Form): Record<string, unknown> => {
  const kind = conditionKind(form);
  return kind ? form.on![kind] ?? {} : {};
};
export const action = (form: Form): Record<string, unknown> => {
  const kind = actionKind(form);
  return kind && kind !== "deploy" && typeof form.do === "object" ? form.do[kind] ?? {} : {};
};

const pick = (from: Record<string, unknown>, keys: string[]): Record<string, unknown> =>
  Object.fromEntries(keys.filter((k) => from[k] !== undefined && from[k] !== null).map((k) => [k, from[k]]));

/** 조건 종류를 바꾼다. 겹치는 칸(일정·저장소·branch·머신)은 이어 쓴다 */
export function setCondition(form: Form, kind: ConditionKind): Form {
  const old = condition(form);
  const timing = pick(old, ["every", "daily", "catch_up"]);
  const target = pick(old, ["repo", "branches", "machine"]);
  if (!timing.every && !timing.daily) timing.daily = "09:00";
  const next: Record<ConditionKind, Record<string, unknown>> = {
    schedule: timing,
    command: { ...timing, command: old.command ?? "", ...pick(old, ["machine"]) },
    thread: { events: Array.isArray(old.events) && conditionKind(form) === "thread" ? old.events : ["thread_failed"] },
    pr: { events: ["checks-failed", "review"] },
    ci: { repo: "", branches: ["main"], ...target, events: ["failed"] },
    git: { repo: "", branches: ["main"], ...target, events: ["pushed"] },
  };
  return { ...form, on: { [kind]: next[kind] } };
}

/** 동작 종류를 바꾼다. 본문 글(text)은 이어 쓴다. 배포 제안은 main Git push 조건에만 쓴다(host가 검사한다) */
export function setAction(form: Form, kind: ActionKind): Form {
  if (kind === "deploy") return { ...form, do: "deploy" };
  const old = action(form);
  const text = typeof old.text === "string" ? old.text : "";
  const next: Record<Exclude<ActionKind, "deploy">, Record<string, unknown>> = {
    inbox: { text },
    message: { thread: typeof old.thread === "string" ? old.thread : "owner", text },
    start: { title: typeof old.title === "string" ? old.title : "", profiles: [], machines: [], text },
  };
  return { ...form, do: { [kind]: next[kind] } };
}

/** 칸 하나를 고친다(빈 글·빈 목록은 칸을 지워 host 기본값을 쓴다) */
export function setField(form: Form, part: "on" | "do" | "limits", key: string, value: unknown): Form {
  const empty = value === "" || value === null || value === undefined || (Array.isArray(value) && value.length === 0 && key !== "profiles" && key !== "machines" && key !== "branches");
  if (part === "limits") {
    const limits = { ...(form.limits ?? {}) };
    if (empty) delete limits[key];
    else limits[key] = value;
    return { ...form, limits };
  }
  const kind = part === "on" ? conditionKind(form) : actionKind(form);
  if (!kind || kind === "deploy") return form;
  const table = { ...(part === "on" ? condition(form) : action(form)) };
  if (empty) delete table[key];
  else table[key] = value;
  return { ...form, [part]: { [kind]: table } };
}

/** 본문: 규칙 안 글(text)인가, 따로 둔 파일(text_file·brief_file)인가 */
export function bodyOf(form: Form): { inline: string | null; file: string | null } {
  const a = action(form);
  const file = typeof a.brief_file === "string" ? a.brief_file : typeof a.text_file === "string" ? a.text_file : null;
  return { inline: file ? null : typeof a.text === "string" ? a.text : actionKind(form) === "deploy" ? null : "", file };
}

export const listText = (value: unknown): string => (Array.isArray(value) ? value.join(", ") : "");
export const parseList = (text: string): string[] => text.split(/[,\n]/).map((s) => s.trim()).filter(Boolean);
/** 사용량 문턱 칸: 빈 칸은 문턱 없음, 숫자는 0~100 */
export function parsePercent(text: string): number | null | "bad" {
  if (text.trim() === "") return null;
  const n = Number(text);
  return Number.isFinite(n) && n >= 0 && n <= 100 ? n : "bad";
}

export type DiffLine = { kind: "same" | "add" | "del"; text: string };
/** 줄 단위 차이(LCS). 충돌 창이 「내 초안」과 「지금 파일」의 다른 줄을 보인다. 자동화 파일은 작아서 O(n·m)이면 된다 */
export function lineDiff(from: string, to: string): DiffLine[] {
  const a = from.split("\n");
  const b = to.split("\n");
  const n = a.length;
  const m = b.length;
  const lcs = Array.from({ length: n + 1 }, () => new Array<number>(m + 1).fill(0));
  for (let i = n - 1; i >= 0; i--) for (let j = m - 1; j >= 0; j--) lcs[i][j] = a[i] === b[j] ? lcs[i + 1][j + 1] + 1 : Math.max(lcs[i + 1][j], lcs[i][j + 1]);
  const out: DiffLine[] = [];
  let i = 0;
  let j = 0;
  while (i < n || j < m) {
    if (i < n && j < m && a[i] === b[j]) { out.push({ kind: "same", text: a[i] }); i++; j++; }
    else if (j < m && (i >= n || lcs[i][j + 1] >= lcs[i + 1][j])) { out.push({ kind: "add", text: b[j] }); j++; }
    else { out.push({ kind: "del", text: a[i] }); i++; }
  }
  return out;
}

/** 이름: 파일 이름이 된다(host `valid_project`와 같다) */
export const validName = (name: string): boolean => /^[a-z0-9_][a-z0-9_-]{0,47}$/.test(name);

/** 지금 실행 key: 같은 누름은 한 번만 받는다(창을 연 동안 같은 key) */
export const runKey = (): string => `web-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;

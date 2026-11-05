// 자동화 편집 폼의 고르기 칸 값(profile·머신·저장소)과 설정 보기 흐름 카드(t-1538 시안 1A)의 순수 규칙. 목록의 정본은 host다:
// profile은 `assignable`(머신마다 쓸 수 있는 이름, safety 허용 반영)과 `profiles`(host 머신의 모델·생각 수준), 머신은 `assignable`,
// 저장소는 `project_settings` repos 경로의 마지막 이름이다(host auto_start `matches_repo`와 같다). 저장 값은 늘 이름 그대로다.
import type { OwnerChoice, ProfileView, RepoChoice } from "./gen/proto";
import { action, actionKind, condition, conditionKind, type Form } from "./automationForm.ts";

export type ProfileChoice = { name: string; model: string; effort: string; known: boolean };
export type MachineChoice = { name: string; connected: boolean };
export type Choices = { profiles: ProfileChoice[]; machines: MachineChoice[]; repos: string[] };

/** 모델 칸 글: 모델이 없는 기본 profile(claude·codex)은 harness 이름이다 */
export const modelLabel = (p: Pick<ProfileView, "model" | "harness">): string => p.model?.trim() || p.harness;
/** 생각 수준 칸 글: 정하지 않은 profile은 「기본」이다 */
export const effortLabel = (p: Pick<ProfileView, "effort">): string => p.effort?.trim() || "기본";

/**
 * 고를 수 있는 profile: `assignable`이 준 이름(어느 머신에서든)에 host 머신 profile의 모델·생각 수준을 붙인다.
 * `assignable`이 비면(옛 host·끊김) host 머신 profile만 쓴다. 모델을 모르는 이름은 모델 칸이 「다른 머신」이다.
 */
export function profileChoices(host: readonly ProfileView[], owners: readonly OwnerChoice[] | null): ProfileChoice[] {
  const detail = new Map(host.map((p) => [p.name, p]));
  const names = owners?.some((o) => o.profiles.length > 0) ? [...new Set(owners.flatMap((o) => o.profiles))] : host.map((p) => p.name);
  return names.map((name) => {
    const p = detail.get(name);
    return p ? { name, model: modelLabel(p), effort: effortLabel(p), known: true } : { name, model: "다른 머신", effort: "기본", known: true };
  }).sort((a, b) => a.model.localeCompare(b.model) || a.effort.localeCompare(b.effort) || a.name.localeCompare(b.name));
}

export const machineChoices = (owners: readonly OwnerChoice[] | null): MachineChoice[] =>
  (owners ?? []).map((o) => ({ name: o.machine, connected: o.connected }));

/** 저장소 이름: 경로의 마지막 이름, 겹치지 않게 적힌 차례 */
export function repoNames(repos: readonly RepoChoice[]): string[] {
  const out: string[] = [];
  for (const r of repos) {
    const name = r.path.replace(/[\\/]+$/, "").split(/[\\/]/).pop() ?? "";
    if (name && !out.includes(name)) out.push(name);
  }
  return out;
}

export const modelsOf = (choices: readonly ProfileChoice[]): string[] => [...new Set(choices.map((c) => c.model))];
export const effortsOf = (choices: readonly ProfileChoice[], model: string): string[] => [...new Set(choices.filter((c) => c.model === model).map((c) => c.effort))];
export const matching = (choices: readonly ProfileChoice[], model: string, effort: string): ProfileChoice[] =>
  choices.filter((c) => c.model === model && c.effort === effort);

/** 칩 하나: 이름과 붙일 글. 목록에 없는 기존 값도 지우지 않고 「목록에 없음」 표지로 보인다 */
export function profileChip(name: string, choices: readonly ProfileChoice[] | null): ProfileChoice {
  return choices?.find((c) => c.name === name) ?? { name, model: "", effort: "", known: false };
}

/** 목록의 i번째를 한 칸 앞으로(차례가 고르는 차례다: profile_pick first) */
export function moveEarlier<T>(list: readonly T[], i: number): T[] {
  if (i <= 0 || i >= list.length) return [...list];
  const out = [...list];
  [out[i - 1], out[i]] = [out[i], out[i - 1]];
  return out;
}

const UNIT: Record<string, string> = { s: "초", m: "분", h: "시간", d: "일" };
/** `30m`·`2h` 같은 길이를 사람 말로. 모르는 꼴은 그대로 */
export const span = (text: unknown): string => {
  const s = typeof text === "string" ? text.trim() : "";
  const m = /^(\d+)([smhd])$/.exec(s);
  return m ? `${m[1]}${UNIT[m[2]]}` : s;
};
const list = (v: unknown): string[] => (Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : []);
const str = (v: unknown): string | null => (typeof v === "string" && v.trim() ? v : null);

export type FlowRow = { label: string; chips?: string[]; kind?: "profile" | "machine" | "repo" | "text"; empty?: string; code?: boolean };
export type FlowCard = { title: string; notes: string[]; rows: FlowRow[] };
export type Flow = { when: FlowCard; what: FlowCard; where: FlowCard; limits: string[] };

/** 일정 글: 매일 HH:MM · N분마다. 없으면 null */
function timing(c: Record<string, unknown>): string | null {
  if (str(c.daily)) return `매일 ${c.daily}`;
  if (str(c.every)) return `${span(c.every)}마다`;
  return null;
}

/**
 * 설정 보기의 흐름 카드(언제 → 무엇을 → 어디서)와 한도 알약. host가 읽은 폼(`automation_read` check.form)에서만 만든다.
 * 폼으로 읽지 못한 조건·동작(새 종류)은 null이다: 화면은 그때 host 낱말 줄을 그대로 보인다.
 */
export function flowOf(form: Form): Flow | null {
  const ck = conditionKind(form);
  const ak = actionKind(form);
  if (!ck || !ak) return null;
  const c = condition(form);
  const a = action(form);
  const when: FlowCard = { title: "", notes: [], rows: [] };
  const time = timing(c);
  switch (ck) {
    case "schedule": when.title = time ?? "일정"; break;
    case "command":
      when.title = time ? `${time} 명령 출력 변화` : "명령 출력 변화";
      when.rows.push({ label: "명령", chips: str(c.command) ? [c.command as string] : [], code: true, empty: "없음" });
      when.rows.push({ label: "명령 머신", chips: str(c.machine) ? [c.machine as string] : [], kind: "machine", empty: "host 머신" });
      break;
    case "thread":
      when.title = "thread 사건";
      when.rows.push({ label: "사건", chips: list(c.events), code: true, empty: "없음" });
      when.rows.push({ label: "저장소", chips: list(c.repos), kind: "repo", empty: "모든 저장소" });
      if (c.include_automation === true) when.notes.push("자동화 thread의 사건도 받음");
      break;
    case "ci": case "git":
      when.title = ck === "ci" ? "CI 결과" : "Git push";
      when.rows.push({ label: "저장소", chips: str(c.repo) ? [c.repo as string] : [], kind: "repo", empty: "없음" });
      when.rows.push({ label: "branch", chips: list(c.branches), code: true, empty: "모든 branch" });
      if (ck === "ci") when.rows.push({ label: "workflow", chips: list(c.workflows), code: true, empty: "모든 workflow" });
      when.rows.push({ label: "사건", chips: list(c.events), code: true, empty: "없음" });
      if (str(c.machine)) when.rows.push({ label: "조회 머신", chips: [c.machine as string], kind: "machine" });
      break;
  }
  if (ck === "schedule" || ck === "command") {
    when.notes.push(c.catch_up === "skip" ? "놓친 차례는 기록하고 넘김" : "놓친 차례는 한 번으로");
    if (typeof c.min_open_threads === "number") when.notes.push(`열린 일반 thread ${c.min_open_threads}개 이상일 때`);
  }

  const what: FlowCard = { title: "", notes: [], rows: [] };
  const where: FlowCard = { title: "", notes: [], rows: [] };
  switch (ak) {
    case "inbox":
      what.title = "coordinator 안내";
      if (a.notify === true) what.notes.push("휴대폰에도 알림");
      where.rows.push({ label: "받는 곳", chips: ["coordinator"], kind: "text" });
      break;
    case "message":
      what.title = "thread에 글";
      where.rows.push({ label: "받는 thread", chips: str(a.thread) ? [a.thread as string] : [], kind: "text", empty: "없음" });
      break;
    case "start":
      what.title = "새 thread 시작";
      if (str(a.title)) what.notes.push(`「${a.title}」`);
      what.notes.push(a.reuse === "open" ? "열린 thread에 이어 보냄" : "사건마다 새 thread");
      if (a.reuse === "open" && a.memory === true) what.notes.push("규칙 기억 씀");
      if (a.reuse === "open" && str(a.close_idle)) what.notes.push(`조용하면 ${span(a.close_idle)} 뒤 끝냄`);
      if (a.reports === true) what.notes.push("사건 thread 보고 붙임");
      where.rows.push({ label: "profile", chips: list(a.profiles), kind: "profile", empty: "없음" });
      where.rows.push({ label: "머신", chips: list(a.machines), kind: "machine", empty: "자동" });
      where.rows.push({ label: "저장소", chips: str(a.repo) ? [a.repo as string] : [], kind: "repo", empty: "작업 폴더" });
      break;
    case "command":
      what.title = "승인된 명령 실행";
      where.rows.push({ label: "명령", chips: str(a.command) ? [a.command as string] : [], code: true, empty: "없음" });
      where.rows.push({ label: "실행 머신", chips: str(a.machine) ? [a.machine as string] : [], kind: "machine", empty: "host 머신" });
      break;
    case "deploy":
      what.title = "배포 제안";
      where.rows.push({ label: "받는 곳", chips: ["coordinator"], kind: "text" });
      break;
  }

  const l = form.limits ?? {};
  const limits: string[] = [];
  if (typeof l.usage_session_below === "number") limits.push(`5시간 창 ${l.usage_session_below}% 아래면 쉼`);
  if (typeof l.usage_week_below === "number") limits.push(`주간 ${l.usage_week_below}% 아래면 쉼`);
  if (str(l.batch)) limits.push(`입력 ${span(l.batch)} 묶음`);
  if (str(l.min_gap)) limits.push(`실행 사이 ${span(l.min_gap)}`);
  if (ak === "start") limits.push(`동시 ${typeof l.max_running === "number" ? l.max_running : 1}`);
  limits.push(typeof l.max_per_day === "number" ? `하루 ${l.max_per_day}번` : "하루 상한 없음");
  if (ak === "deploy") limits.push(`새 커밋 ${typeof l.min_commits === "number" ? l.min_commits : 1}개`);
  return { when, what, where, limits };
}

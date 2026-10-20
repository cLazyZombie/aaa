// 자동화 상세·변환·막힌 큐의 판정(관리 GUI W1, plan §8.302). 화면은 이 결과만 그린다. 정본은 host다.
import type { MissionView, RoutineRun, RoutineView } from "./gen/proto";

/** host가 이 판(FRONT_API) 이상인가. 옛 host는 `/api/v1/me`에 판을 싣지 않는다(빈 값 = 옛 host) */
export function apiAtLeast(have: string | null | undefined, major: number, minor: number): boolean {
  const m = /^(\d+)\.(\d+)$/.exec(have ?? "");
  if (!m) return false;
  const [a, b] = [Number(m[1]), Number(m[2])];
  return a === major && b >= minor;
}
/** 변환 미리보기(`automation_convert` dry_run): 옛 host는 그 칸을 버리고 변환하므로 1.58 이상만 */
export const canPreviewConvert = (frontApi: string | null | undefined): boolean => apiAtLeast(frontApi, 1, 58);

/** 웹이 고치지 않는 옛 `.md` 형식. 규칙은 `routines/*.md`, 임무는 front matter(`+++`)로 시작하는 md다 */
export const legacyRule = (r: Pick<RoutineView, "file">): boolean => /^(routines|missions)\/[^/]+\.md$/.test(r.file);
export const legacyMission = (m: Pick<MissionView, "file_text">): boolean => m.file_text.startsWith("+++\n");
/** 변환할 옛 파일 수: 옛 임무의 조건 줄(`mission:<이름>`, 파일 missions/<이름>.md)과 그 임무를 한 파일로 센다 */
export const legacyFiles = (rules: readonly Pick<RoutineView, "file">[], missions: readonly Pick<MissionView, "name" | "file_text">[]): string[] =>
  [...new Set([...rules.filter(legacyRule).map((r) => r.file), ...missions.filter(legacyMission).map((m) => `missions/${m.name}.md`)])].sort();

/** 보낸 결과를 모르는 `do.message` 묶음: 사람이 대상 thread를 보고 푼다 */
export const stuckBatch = (r: Pick<RoutineView, "queue">) => (r.queue?.inflight?.unknown ? r.queue.inflight : null);

export type RuleTone = "off" | "failed" | "stuck" | "done" | "skipped" | "waiting" | "ready";
/** 줄의 점·배지 색: 꺼짐 → 파일 오류 → 막힌 큐 → 마지막 결과 */
export function ruleTone(r: Pick<RoutineView, "enabled" | "error" | "queue" | "last_result">): RuleTone {
  if (!r.enabled) return "off";
  if (r.error) return "failed";
  if (stuckBatch(r)) return "stuck";
  const s = r.last_result?.status;
  return s === "done" || s === "skipped" || s === "waiting" || s === "failed" ? s : "ready";
}

export const runWord: Record<string, string> = { done: "처리됨", skipped: "건너뜀", waiting: "대기", failed: "실패" };
export const runTone = (run: Pick<RoutineRun, "status">): "done" | "failed" | "notice" | "quiet" =>
  run.status === "done" ? "done" : run.status === "failed" ? "failed" : run.status === "waiting" || run.status === "skipped" ? "notice" : "quiet";

/** 동작이 닿는 곳(대상). host 낱말(`action`)과 승인 범위·명령 머신에서 고른다 */
export function ruleTargets(r: Pick<RoutineView, "action" | "machine" | "command" | "start_scope" | "queue">): string[] {
  const out: string[] = [];
  if (r.action) out.push(r.action);
  if (r.command) out.push(`명령 실행 머신: ${r.machine ?? "host 머신"}`);
  // 승인 범위 글(host auto_start)의 조건 줄은 「설정」에 있다: 시작 후보 머신·저장소와 profile만 대상이다
  if (r.start_scope)
    out.push(...r.start_scope.split("\n").map((l) => l.trim().replace(/^- /, "")).filter((l) => l && !l.startsWith("조건:") && l !== "시작 후보:"));
  const thread = r.queue?.inflight?.thread;
  if (thread) out.push(`보내던 thread: ${thread}`);
  return out;
}

export type ConvertGroup = { key: "rules" | "derived" | "missions" | "bodies" | "archived" | "schema"; title: string; files: string[] };
/**
 * 변환 계획 목록을 사람이 읽는 묶음으로 나눈다. 임무 조건에서 생기는 규칙(`<임무>-events`·`<임무>-schedule`)은
 * 그 임무의 새 파일이 같은 계획에 있을 때만 「생기는 규칙」으로 본다.
 */
export function convertGroups(files: readonly string[]): ConvertGroup[] {
  const missions = new Set(files.map((f) => /^missions\/([^/]+)\.toml$/.exec(f)?.[1]).filter((n): n is string => !!n));
  const groups: ConvertGroup[] = [
    { key: "rules", title: "새 규칙 파일", files: [] },
    { key: "derived", title: "임무 조건에서 생기는 규칙", files: [] },
    { key: "missions", title: "새 임무 파일", files: [] },
    { key: "bodies", title: "따로 둔 긴 본문", files: [] },
    { key: "archived", title: "옛 파일 보존 사본(.converted/)", files: [] },
    { key: "schema", title: "편집기 스키마", files: [] },
  ];
  const at = (k: ConvertGroup["key"]) => groups.find((g) => g.key === k)!.files;
  for (const f of files) {
    if (f.includes("/.converted/")) at("archived").push(f);
    else if (f.endsWith(".schema.json")) at("schema").push(f);
    else if (f.startsWith("missions/")) at("missions").push(f);
    else if (/^automations\/[^/]+\.md$/.test(f)) at("bodies").push(f);
    else {
      const name = /^automations\/([^/]+)\.toml$/.exec(f)?.[1] ?? "";
      const derived = [...missions].some((m) => name === `${m}-events` || name === `${m}-schedule`);
      at(derived ? "derived" : "rules").push(f);
    }
  }
  return groups.filter((g) => g.files.length > 0);
}
/** 보존 사본이 놓인 폴더들(`routines/.converted/`·`missions/.converted/`) */
export const archiveDirs = (files: readonly string[]): string[] =>
  [...new Set(files.filter((f) => f.includes("/.converted/")).map((f) => f.slice(0, f.indexOf("/.converted/") + "/.converted/".length)))];

/** 마지막 건너뜀·대기: host가 준 값, 없으면 기록에서 */
export const lastSkip = (r: Pick<RoutineView, "last_skip">, runs: readonly RoutineRun[]): RoutineRun | null =>
  r.last_skip ?? runs.find((run) => run.status === "skipped" || run.status === "waiting") ?? null;

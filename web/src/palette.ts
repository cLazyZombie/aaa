// 명령 팔레트의 검색·묶음(herdr-web-ui aa624cb src/lib/paletteSearch.ts·machinePalette.ts·paletteSettings.ts를 aaa 목록에 맞춰 옮김).
// 참고는 PC마다 workspace 묶음에 pane 줄을 둔다. aaa의 workspace 자리는 머신 안 프로젝트(thread 줄)와 thread에 딸리지 않은
// pane 묶음이고, 줄 하나는 목록 줄 하나다(sidebarModel `rosterRows`: thread 줄은 그 agent pane이 돌면 pane을, 아니면 보고를 연다).
// 목록은 프로젝트 먼저지만(plan §8.195) 팔레트는 참고대로 PC마다 묶고, PC 안의 차례는 목록 차례(`roster`)를 따른다.
// 찾는 점수(`fuzzyScore`)·`>` 동작만·상태 칩과 그 글자 키·동작 이름 찾기(`rankByTitle`)·최근 줄은 참고 그대로다. plan §8.132.
import type { MachineView, PaneView, ThreadView } from "./gen/proto";
import { knownStatus, type KnownStatus } from "./herdr/status.ts";
import { paneAgentStatus, roster, shortPath, threadKey } from "./sidebarModel.ts";

export type PaletteStatusFilter = "all" | Exclude<KnownStatus, "unknown">;

export const STATUS_FILTERS: readonly PaletteStatusFilter[] = ["all", "blocked", "working", "idle", "done", "waiting"];

/**
 * herdr's picker keys as they are: b blocked, w working, i idle, d done, a all. The palette
 * listens for them only while the focus is outside its search field, so a query never loses a
 * letter to them. BG has no key in herdr and gets none here.
 */
export const FILTER_KEYS: Readonly<Record<string, PaletteStatusFilter>> = { a: "all", b: "blocked", w: "working", i: "idle", d: "done" };

export interface PaletteQuery {
  actionsOnly: boolean;
  text: string;
}

/** A query that starts with `>` searches the actions alone, as T3 Code's palette reads that prefix. */
export function parseQuery(query: string): PaletteQuery {
  const trimmed = query.trimStart();
  if (trimmed.startsWith(">")) return { actionsOnly: true, text: trimmed.slice(1).trim() };
  return { actionsOnly: false, text: query.trim() };
}

export function fuzzyScore(query: string, candidate: string): number | null {
  const needle = query.trim().toLocaleLowerCase();
  if (needle.length === 0) return 0;
  const haystack = candidate.toLocaleLowerCase();
  const direct = haystack.indexOf(needle);
  if (direct >= 0) return 1000 - direct * 2 - (haystack.length - needle.length);

  let queryIndex = 0;
  let first = -1;
  let previous = -2;
  let runs = 0;
  for (let index = 0; index < haystack.length && queryIndex < needle.length; index += 1) {
    if (haystack[index] !== needle[queryIndex]) continue;
    if (first < 0) first = index;
    if (index !== previous + 1) runs += 1;
    previous = index;
    queryIndex += 1;
  }
  if (queryIndex !== needle.length) return null;
  return 500 - first * 2 - (previous - first) - runs * 12;
}

/** 팔레트 줄 하나(참고 `MachinePane`): 목록 줄과 같은 것을 연다 */
export interface PaletteTarget {
  /** 목록의 선택 값(`pane:…`·`thread:…`), 최근 기록의 열쇠 */
  key: string;
  /** 이 줄이 골랐다고 보는 선택 값들(thread 줄은 그 pane도) */
  keys: string[];
  open: { pane: string } | { thread: ThreadView };
  machine: MachineView;
  /** 묶음: 프로젝트 이름, thread에 딸리지 않은 pane은 null */
  project: string | null;
  title: string;
  /** 줄 둘째 줄(참고 `placeLine`: 탭 · 폴더) */
  place: string;
  /** 바닥 줄 경로 */
  path: string;
  status: KnownStatus;
  pane: PaneView | undefined;
  thread: ThreadView | undefined;
  /** 찾는 글(참고 `searchableText`) */
  search: string[];
}

const paneTitle = (p: PaneView) => p.name || p.argv.join(" ") || p.id.slice(0, 8);

/** 붙은 머신의 목록 줄을 목록 차례로(참고 `rankMachinePanes`의 입력: 끊긴 머신의 남은 줄은 갈 곳이 아니다) */
export function paletteTargets(machines: readonly MachineView[], panes: readonly PaneView[], threads: readonly ThreadView[], paneOrder: readonly string[] = []): PaletteTarget[] {
  const targets: PaletteTarget[] = [];
  const all = roster(machines, panes, threads, paneOrder);
  for (const m of machines) {
    if (m.connection !== "connected") continue;
    for (const project of all.projects) {
      for (const p of all.coordinators.filter((p) => p.coordinator_project === project && p.machine_id === m.id)) addPane(p, project);
      for (const t of all.threads.filter((x) => x.project === project && all.machineOf(x)?.id === m.id)) {
        const pane = t.pane ? panes.find((p) => p.id === t.pane) : undefined;
        const running = pane?.state === "running";
        const path = pane?.cwd ? shortPath(pane.cwd) : "";
        targets.push({
          key: `thread:${threadKey(t)}`,
          keys: [`thread:${threadKey(t)}`, ...(pane ? [`pane:${pane.id}`] : [])],
          open: running && pane ? { pane: pane.id } : { thread: t },
          machine: m,
          project,
          // herdr-projects thread_display: 「id · 제목」(목록 ThreadRow와 같다)
          title: `${t.id} · ${t.title}`,
          place: [t.profile, path].filter(Boolean).join(" · "),
          path,
          status: knownStatus(running && pane ? paneAgentStatus(pane) : undefined),
          pane,
          thread: t,
          search: [t.id, t.title, t.project, t.profile, t.branch ?? "", pane ? paneTitle(pane) : "", pane?.cwd ?? "", pane?.agent?.harness ?? "", m.name],
        });
      }
    }
    for (const p of all.panes.filter((x) => x.machine_id === m.id)) addPane(p, null);
    function addPane(p: PaneView, project: string | null) {
      const path = p.cwd ? shortPath(p.cwd) : "";
      targets.push({
        key: `pane:${p.id}`,
        keys: [`pane:${p.id}`],
        open: { pane: p.id },
        machine: m,
        project,
        title: paneTitle(p),
        place: [p.agent?.harness, path].filter(Boolean).join(" · "),
        path,
        status: knownStatus(paneAgentStatus(p)),
        pane: p,
        thread: undefined,
        search: [project ?? "", p.label ?? "", p.name, p.argv.join(" "), p.cwd ?? "", p.agent?.harness ?? "", m.name],
      });
    }
  }
  return targets;
}

/** How many rows each chip stands for, counted over the whole roster: a query does not change them. */
export function statusCounts(targets: readonly PaletteTarget[]): Record<PaletteStatusFilter, number> {
  const counts: Record<PaletteStatusFilter, number> = { all: targets.length, blocked: 0, working: 0, idle: 0, done: 0, waiting: 0 };
  for (const target of targets) if (target.status !== "unknown") counts[target.status] += 1;
  return counts;
}

/** Fuzzy search across everything visible in a palette row. Ties retain roster order (참고 `scorePanes`). */
export function rankTargets(query: string, targets: readonly PaletteTarget[], filter: PaletteStatusFilter): PaletteTarget[] {
  const shown = filter === "all" ? targets : targets.filter((target) => target.status === filter);
  if (query.trim().length === 0) return [...shown];
  return shown
    .map((target, index) => {
      let score: number | null = null;
      for (const candidate of [target.title, ...target.search]) {
        const candidateScore = fuzzyScore(query, candidate);
        if (candidateScore !== null && (score === null || candidateScore > score)) score = candidateScore;
      }
      return { target, index, score };
    })
    .filter((entry): entry is typeof entry & { score: number } => entry.score !== null)
    .sort((a, b) => b.score - a.score || a.index - b.index)
    .map(({ target }) => target);
}

export interface PaletteGroup {
  id: string;
  machine: MachineView;
  project: string | null;
  targets: PaletteTarget[];
}

/**
 * 머신의 프로젝트마다 묶음 하나, 그다음 그 머신의 pane 묶음(참고 `groupMachinePanes`). 찾지 않은 목록은 목록 차례 그대로이고,
 * 찾은 목록은 첫 줄이 선 자리에 묶음이 서서 가장 잘 맞은 줄이 첫 줄로 남는다. 묶음 안 줄은 차례를 지킨다.
 */
export function groupTargets(targets: readonly PaletteTarget[]): PaletteGroup[] {
  const groups = new Map<string, PaletteGroup>();
  for (const target of targets) {
    const id = JSON.stringify([target.machine.id, target.project]);
    let group = groups.get(id);
    if (!group) {
      group = { id, machine: target.machine, project: target.project, targets: [] };
      groups.set(id, group);
    }
    group.targets.push(target);
  }
  return [...groups.values()];
}

export const RECENT_KEY = "aaa:palette-recent";
const RECENT_LIMIT = 8;

export function rememberTarget(key: string, current: readonly string[]): string[] {
  return [key, ...current.filter((item) => item !== key)].slice(0, RECENT_LIMIT);
}

/** The rows the user went to last, newest first, those still listed and other than the one open now (참고 `recentTargets`). */
export function recentTargets(targets: readonly PaletteTarget[], recent: readonly string[], selected: string | null, limit: number): PaletteTarget[] {
  const byKey = new Map(targets.map((target) => [target.key, target]));
  return recent
    .flatMap((key) => {
      const target = byKey.get(key);
      return target && !(selected !== null && target.keys.includes(selected)) ? [target] : [];
    })
    .slice(0, limit);
}

export function loadRecent(read: (key: string) => string | null): string[] {
  try {
    const value: unknown = JSON.parse(read(RECENT_KEY) ?? "[]");
    if (!Array.isArray(value)) return [];
    return [...new Set(value.filter((item): item is string => typeof item === "string"))].slice(0, RECENT_LIMIT);
  } catch {
    return [];
  }
}

export type SettingsPageId = "appearance" | "chat" | "terminal" | "alerts" | "usage" | "accounts" | "safety" | "profiles" | "shortcuts" | "devices" | "machines" | "project" | "repos";

/** What the palette shows for one Settings page: its name and the words a user may look for it by. */
export interface SettingsPageEntry {
  id: SettingsPageId;
  label: string;
  aliases: string;
}

/** 설정 쪽(참고 `settingsPageEntries` 가운데 aaa에 있는 쪽, 이름과 다른 말은 참고 i18n.ko.ts) */
export const SETTINGS_PAGE_ENTRIES: readonly SettingsPageEntry[] = [
  { id: "appearance", label: "모양", aliases: "테마 다크 라이트 모드 색상 팔레트 밀도 언어 사이드바 로고" },
  { id: "chat", label: "채팅", aliases: "채팅 글꼴 글자 크기 메시지 너비 입력창 엔터 보내기 생각 코드" },
  { id: "terminal", label: "터미널", aliases: "터미널 글꼴 글자 크기 키 바 키보드 클립보드" },
  { id: "alerts", label: "알림", aliases: "알림 소리 푸시 벨 입력 필요 완료" },
  { id: "usage", label: "구독 사용량", aliases: "요금제 한도 할당량 미터" },
  { id: "accounts", label: "계정", aliases: "계정 로그인 다시 로그인 기본 계정 메일 인증 끊김 만료 claude codex" },
  { id: "safety", label: "안전", aliases: "safety yolo 권한 건너뛰기 start_threads trust_screens 신뢰 창 배포 coordinator_deploy" },
  { id: "profiles", label: "profile", aliases: "profile 프로필 harness model effort 인자 허용 목록 기본 profile claude codex" },
  { id: "shortcuts", label: "단축키", aliases: "키보드 키 단축키 바인딩" },
  { id: "devices", label: "폰 및 기기", aliases: "휴대폰 설치 화면 켜짐 유지 앱 웹 공개 tailscale 공개 끄기" },
  { id: "machines", label: "머신", aliases: "머신 등록 승인 폐기 agent enroll 확인 코드 작업 폴더 work root hook codex 연동" },
  { id: "project", label: "이 aaa", aliases: "프로젝트 목표 profile 멈춤 재개 nudge mute 정리 sweep 보고서 사본 공유 링크 키 교체" },
  { id: "repos", label: "저장소", aliases: "저장소 repo workspaces 머신 경로 작업 폴더" },
];

/** Other words for the palette's actions, by action id: a name the user may remember instead of the label (참고 i18n.ko.ts). */
export const ACTION_ALIASES: Readonly<Record<string, string>> = {
  new: "만들기 워크스페이스 세션 시작",
  view: "보기 채팅 터미널 대화",
  theme: "다크 라이트 모드 외관 색상",
  settings: "환경설정 옵션 구성",
  // 예전 「프로젝트 thread 보기」(plan §8.291): 예전 말로도 찾는다
  threads: "프로젝트 thread 끝남 보고 목록",
  // 보고서는 설정 쪽이었다가 활동 막대의 본문 화면이 됐다(plan §8.240): 예전 말로도 찾는다
  reports: "보고서 결과물 library 공유 링크",
  sidebar: "사이드바 목록 접기 펴기 숨기기 보이기 패널",
};

export interface SearchableRow<T> {
  item: T;
  title: string;
  aliases?: string;
}

function normalize(text: string): string {
  return text.toLocaleLowerCase().replace(/\s+/g, " ").trim();
}

/**
 * Rows a query finds, best first. A title that holds every word of the query is a strong match
 * (the whole title, then its start, then anywhere in it); a row found only through its aliases is a
 * weak match and comes after every strong one, as T3 Code sorts a secondary hit last. Within a rank
 * the rows keep the order they came in. An empty query keeps every row as it is.
 */
export function rankByTitle<T>(query: string, rows: readonly SearchableRow<T>[]): T[] {
  const needle = normalize(query);
  if (needle === "") return rows.map((row) => row.item);
  const words = needle.split(" ");
  const found: { item: T; index: number; rank: number }[] = [];
  rows.forEach((row, index) => {
    const title = normalize(row.title);
    const all = `${title} ${normalize(row.aliases ?? "")}`;
    if (words.every((word) => title.includes(word))) {
      found.push({ item: row.item, index, rank: title === needle ? 3 : title.startsWith(needle) ? 2 : 1 });
    } else if (words.every((word) => all.includes(word))) {
      found.push({ item: row.item, index, rank: 0 });
    }
  });
  return found.sort((a, b) => b.rank - a.rank || a.index - b.index).map((entry) => entry.item);
}

/**
 * The page the palette asked Settings to open. The dialog mounts per opening and reads it when it
 * mounts; it clears it once it has, and the palette clears it on its own opening, so a request the
 * dialog never saw cannot steer a later opening.
 */
let requested: SettingsPageId | null = null;

export function requestSettingsPage(page: SettingsPageId | null): void {
  requested = page;
  if (page !== null && typeof window !== "undefined") window.dispatchEvent(new Event("aaa:settings-page"));
}

export function requestedSettingsPage(): SettingsPageId | null {
  return requested;
}

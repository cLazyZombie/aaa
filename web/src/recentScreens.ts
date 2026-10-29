import type { PaletteTarget } from "./palette.ts";

export interface RecentTarget {
  key: string;
  keys: string[];
  title: string;
  detail: string;
  open: string;
  coordinator: boolean;
}

/** 앱 본문 주소만 허용한다. 임의 URL·팔레트 명령은 방문 대상이 아니다. */
export function appScreenTarget(key: string): RecentTarget | null {
  const at = key.indexOf(":");
  const kind = key.slice(0, at), value = key.slice(at + 1);
  const labels: Record<string, string> = { reports: "보고서", tasks: "할 일", task: "할 일", automation: "자동화", idea: "아이디어", settings: "설정", ask: "답할 것", question: "답할 것", workspace: "작업 공간", threads: "모든 thread" };
  const label = labels[kind];
  if (at < 0 || !label) return null;
  return { key, keys: [key], title: value ? `${label} · ${value}` : label, detail: "앱 화면", open: key, coordinator: false };
}

export function recentTargets(panes: readonly PaletteTarget[], keys: readonly string[]): RecentTarget[] {
  return [
    ...liveRecentTargets(panes).map(target => ({ key: target.key, keys: target.keys, title: target.title,
      detail: [target.machine.name, target.project].filter(Boolean).join(" · "), open: `pane:${target.pane!.id}`, coordinator: !!target.pane?.coordinator_project })),
    ...[...new Set(keys)].flatMap(key => { const target = appScreenTarget(key); return target ? [target] : []; }),
  ];
}

/** 완료 보고의 done(쉼)은 종료가 아니다. pane 실행 상태와 thread 생명주기를 기준으로 삼는다. */
export function liveRecentTargets(targets: readonly PaletteTarget[]): PaletteTarget[] {
  return targets.filter(({ pane, thread }) => pane?.state === "running" &&
    (!thread || (thread.status === "open" && thread.project_status !== "archived")));
}

export function visitedScreens<T extends { key: string }>(targets: readonly T[], recent: readonly string[]): T[] {
  const byKey = new Map(targets.map((target) => [target.key, target]));
  return [...new Set(recent)].flatMap((key) => { const target = byKey.get(key); return target ? [target] : []; });
}

export interface RecentCycle { keys: string[]; key: string; }

/** 방문 순서를 고정한다. 현재 화면이 목록 밖이면 정방향은 맨 앞, 역방향은 맨 뒤부터. */
export function startRecentCycle(keys: string[], current: string | null, direction: -1 | 1): RecentCycle | null {
  if (keys.length === 0 || (keys.length === 1 && keys[0] === current)) return null;
  const at = current === null ? -1 : keys.indexOf(current);
  const index = at < 0 ? (direction === 1 ? 0 : keys.length - 1) : (at + direction + keys.length) % keys.length;
  return { keys: [...keys], key: keys[index]! };
}

/** 누른 동안 새로 방문한 것은 끼우지 않고, 종료·연결 해제된 화면은 즉시 제외한다. */
export function moveRecentCycle(cycle: RecentCycle, live: ReadonlySet<string>, direction: -1 | 1): RecentCycle | null {
  if (!live.has(cycle.key)) return null;
  const keys = cycle.keys.filter((key) => live.has(key));
  const index = (keys.indexOf(cycle.key) + direction + keys.length) % keys.length;
  return { keys, key: keys[index]! };
}

export function recentScreenChord(event: { key: string; ctrlKey: boolean; shiftKey?: boolean; altKey: boolean; metaKey: boolean; isComposing?: boolean; keyCode?: number }): boolean {
  return event.key === "Tab" && event.ctrlKey && !event.altKey && !event.metaKey && !event.isComposing && event.keyCode !== 229;
}

export function standaloneWindow(): boolean {
  return window.matchMedia("(display-mode: standalone)").matches || (navigator as { standalone?: boolean }).standalone === true;
}

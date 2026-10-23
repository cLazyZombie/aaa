// 최근에 실제 연 화면만 기록한다. VS Code·Windows Terminal의 MRU/해제 확정 방식을 참고했다(MIT).
import type { LibraryReport, PaneView, TaskView, ThreadView } from "./gen/proto.ts";

export const RECENT_SCREENS_KEY = "aaa:recent-screens";
export const RECENT_SCREENS_MAX = 50;
export interface RecentScreen {
  key: string;
  title: string;
  detail: string;
  selection: string;
}
export interface RecentContext {
  panes: readonly PaneView[];
  threads: readonly ThreadView[];
  tasks: readonly TaskView[] | null;
  reports: readonly LibraryReport[];
  automations: readonly { project: string; rules: readonly { name: string; title: string }[] }[];
}

export function recentScreenKey(value: unknown): value is string {
  return typeof value === "string" && value.length <= 600 &&
    /^(?:pane:.+|thread:.+\/.+|task:.+|reports:.*|automation:.+\/.*|workspace:.*|ask:.+|question:.+|tasks:|threads:|asks:|settings:)$/.test(value) &&
    !/[\u0000-\u001f\u007f]/.test(value);
}
export function recentHistory(raw: unknown): string[] {
  return Array.isArray(raw) ? [...new Set(raw.filter(recentScreenKey))].slice(0, RECENT_SCREENS_MAX) : [];
}
export function storedRecentScreens(): string[] {
  try { return recentHistory(JSON.parse(localStorage.getItem(RECENT_SCREENS_KEY) ?? "null")); }
  catch { return []; }
}
export function visitRecent(history: readonly string[], key: string): string[] {
  return recentHistory([key, ...history]);
}

/** pane와 thread 보고는 같은 작업 한 항목이다. 열 때는 지금의 실행 pane을 다시 찾는다. */
export function recentScreen(key: string | null, ctx: RecentContext): RecentScreen | null {
  if (!recentScreenKey(key)) return null;
  const screen = (title: string, detail: string, selection = key): RecentScreen => ({ key, title, detail, selection });
  const threadScreen = (t: ThreadView): RecentScreen | null => {
    if (t.group === "resolved" || t.group === "parked" || t.project_status === "archived") return null;
    const pane = ctx.panes.find((p) => p.id === t.pane && p.state === "running" && p.connection === "connected");
    const canonical = `thread:${t.project}/${t.id}`;
    return { key: canonical, title: t.title || t.id, detail: `${t.id} · ${t.project}`, selection: pane ? `pane:${pane.id}` : canonical };
  };
  if (key.startsWith("pane:")) {
    const id = key.slice(5);
    const pane = ctx.panes.find((p) => p.id === id || p.replaced?.includes(id));
    if (!pane || pane.state !== "running" || pane.connection !== "connected") return null;
    const thread = ctx.threads.find((t) => t.pane === pane.id);
    if (thread) return threadScreen(thread);
    return { key: `pane:${pane.id}`, title: pane.coordinator_project ? "coordinator" : pane.name || "셸", detail: `${pane.machine} · ${pane.coordinator_project ? "대화" : "터미널"}`, selection: `pane:${pane.id}` };
  }
  if (key.startsWith("thread:")) {
    const t = ctx.threads.find((t) => `${t.project}/${t.id}` === key.slice(7));
    return t ? threadScreen(t) : null;
  }
  if (key.startsWith("task:")) {
    const task = ctx.tasks?.find((t) => t.short_id === key.slice(5) || t.id === key.slice(5));
    return task ? screen(task.title, `할 일 · ${task.short_id ?? ""}`) : null;
  }
  if (key.startsWith("reports:") && key !== "reports:") {
    const report = ctx.reports.find((r) => `${r.project}/${r.name}` === key.slice(8));
    return report ? screen(report.title || report.name, "보고서") : null;
  }
  if (key.startsWith("automation:")) {
    const route = key.slice(11), slash = route.indexOf("/");
    const project = route.slice(0, slash), name = route.slice(slash + 1);
    if (!name) return screen("새 자동화", project);
    const rule = ctx.automations.find((r) => r.project === project)?.rules.find((r) => r.name === name);
    return rule ? screen(rule.title || rule.name, `자동화 · ${project}`) : null;
  }
  if (key.startsWith("workspace:")) return screen("작업 공간", "분석");
  if (key.startsWith("ask:") || key.startsWith("question:")) return screen("답할 것", key.slice(key.indexOf(":") + 1));
  const titles: Record<string, string> = { "tasks:": "할 일", "reports:": "보고서", "threads:": "모든 thread", "asks:": "답할 것", "settings:": "설정" };
  return titles[key] ? screen(titles[key], "앱 화면") : null;
}

/** 갱신된 대상만 읽되 순서는 유지한다. 하나의 작업을 가리키는 옛 pane 키도 합친다. */
export function recentList(history: readonly string[], resolve: (key: string) => RecentScreen | null): RecentScreen[] {
  const seen = new Set<string>();
  return history.flatMap((key) => {
    const target = resolve(key);
    if (!target || seen.has(target.key)) return [];
    seen.add(target.key);
    return [target];
  });
}

export interface RecentCycle { keys: string[]; selected: string }
export function stepRecent(keys: readonly string[], selected: string | null, direction: 1 | -1): RecentCycle | null {
  if (keys.length < 2) return null;
  const index = selected === null ? -1 : keys.indexOf(selected);
  const next = index < 0 ? (direction === 1 ? 0 : keys.length - 1) : (index + direction + keys.length) % keys.length;
  return { keys: [...keys], selected: keys[next]! };
}

export function standaloneApp(): boolean {
  return window.matchMedia("(display-mode: standalone)").matches || window.matchMedia("(display-mode: window-controls-overlay)").matches ||
    (navigator as { standalone?: boolean }).standalone === true;
}
export function recentScreenChord(event: { key: string; ctrlKey: boolean; shiftKey: boolean; altKey: boolean; metaKey: boolean; isComposing?: boolean; keyCode?: number }): boolean {
  return event.key === "Tab" && event.ctrlKey && !event.altKey && !event.metaKey && !event.isComposing && event.keyCode !== 229;
}

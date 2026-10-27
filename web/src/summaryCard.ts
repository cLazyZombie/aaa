// thread 작업 요약 카드의 규칙(plan §8.259). 화면(ThreadSummary.tsx)과 나눠 node --test로 본다.
import type { SummaryEntry, SummaryMark, ThreadSummaryView, ThreadView } from "./gen/proto";
import { formatWorkingDuration } from "./sidebarModel.ts";

/** 익숙한 모델 계열만 줄인다. 알 수 없는 이름은 원문을 유지한다. */
export function modelName(model: string): string {
  const codex = /^gpt-6(?:\.\d+)?-(astra|sol|luna)$/.exec(model);
  if (codex) return codex[1]!;
  const claude = /^claude-(opus|sonnet|haiku|fable)-(\d+)-(\d+)(?:-\d{8})?(?:\[1m\])?$/.exec(model);
  if (claude) return `${claude[1]} ${claude[2]}.${claude[3]}`;
  return model;
}

/** /model 선택 직후 기록이 없을 수 있으므로 「최근 확인」과 시작 profile을 분명히 나눈다. */
export function summaryModel(t: ThreadView): { text: string; title: string } {
  const observed = t.agent?.model_observation;
  if (observed) {
    const effort = observed.effort ?? "생각 수준 미확인";
    return {
      text: `최근 확인 모델: ${modelName(observed.model)} ${effort}`,
      title: `${observed.model} · ${effort} · ${new Date(observed.at_ms).toLocaleString()} 확인`,
    };
  }
  return { text: `현재 모델 미확인 · 시작 profile: ${t.profile}`, title: "이 실행에서 아직 모델을 확인하지 못했습니다." };
}

/** host가 기억한 활동 시작을 쓴다. 마지막 보고 시각을 활동 시작으로 추측하지 않는다. */
export function summaryActivity(t: ThreadView, now: number): { activity: string; time: string } {
  const a = t.agent;
  const finished = t.status === "resolved" || a?.activity === "exited" || a?.percent === 100;
  const label = finished ? "마지막 활동" : "지금 활동";
  const percent = a?.progress_at_ms != null ? (a.percent != null ? `~${a.percent}%` : "진행률 미정") : null;
  const activity = `${label}: ${a?.progress ?? "아직 보고 없음"}${percent ? ` · ${percent}` : ""}`;
  const end = t.status === "resolved" ? t.updated_at_ms : now;
  const since = a?.progress_since_ms;
  const activityEnd = finished ? a?.progress_at_ms ?? end : end;
  const activityTime = since != null ? `활동 ${formatWorkingDuration(activityEnd - since)}` : "활동 시간 없음";
  return { activity, time: `시작 후 ${formatWorkingDuration(end - t.created_at_ms)} · ${activityTime}` };
}

/** 요약과 CLI 보고는 따로 갱신된다. 요약의 목표만 고쳐도 시각은 바뀌므로 목록 자체의 갱신 시각으로 부르지 않는다. */
export function summaryUpdates(t: ThreadView, v: ThreadSummaryView, now: number): string {
  const age = (at: number) => `${formatWorkingDuration(now - at)} 전`;
  const summary = v.at_ms !== null ? age(v.at_ms) : "없음";
  const report = t.agent?.progress_at_ms;
  return `요약 갱신 ${summary} · 진행 보고 ${report != null ? age(report) : "없음"}`;
}

/** hover 카드가 보이는 「지금 할 것」 줄 수. 카드는 훑어보는 것이라 6줄 안팎이다(10-19 사용자) */
export const CARD_NOW = 3;

export interface CardLines {
  /** 목표(요약이 없으면 할 일 첫 줄, 그것도 없으면 제목) */
  goal: string;
  /** 요약 파일이 없다 */
  auto: boolean;
  now: string[];
  /** 카드에 못 실은 「지금」 줄 수 */
  moreNow: number;
  /** 가장 최근에 바뀐 것: `## 바뀜` 맨 아래에서 그어지지 않은 줄 */
  latest: SummaryEntry | null;
  pending: number;
}

export function cardLines(v: ThreadSummaryView, title: string): CardLines {
  const auto = v.text === null;
  const goal = v.goal ?? v.task ?? title;
  const now = v.now.slice(0, CARD_NOW);
  const latest = [...v.changed].reverse().find((e) => !e.dropped) ?? null;
  return { goal, auto, now, moreNow: v.now.length - now.length, latest, pending: v.pending };
}

/** 정한 것: coordinator 요약의 줄 뒤에 agent 보고 `## Decisions` 줄(같은 글은 한 번) */
export function decidedLines(v: ThreadSummaryView): SummaryEntry[] {
  const seen = new Set(v.decided.map((e) => e.text));
  const agent = v.decisions.filter((d) => !seen.has(d)).map((text) => ({ who: "agent", at: null, text, dropped: false }));
  return [...v.decided, ...agent];
}

/** 「요약」 단추의 점: 요약 뒤에 지시가 있다(요약이 없는데 지시가 쌓인 것도) */
export const summaryStale = (m: SummaryMark | undefined): boolean => (m?.pending ?? 0) > 0;

/** 칩의 「누가」: coordinator는 짧게 */
export const whoWord = (who: string | null): string => (who === "coordinator" ? "coord" : (who ?? ""));

export interface Box {
  top: number;
  left: number;
  right: number;
  bottom: number;
}
export interface Size {
  width: number;
  height: number;
}

/**
 * hover 카드 자리(t3code SidebarThreadTooltip: 줄 오른쪽, 위 끝 맞춤, 4px 띄움). 오른쪽이 모자라면 줄 왼쪽으로,
 * 아래가 모자라면 위로 민다. 화면 가장자리는 t3code tooltip.tsx `COLLISION_PADDING`(5px)만큼 남긴다.
 */
export function hoverPlace(row: Box, card: Size, view: Size, gap = 4, edge = 5): { top: number; left: number } {
  let left = row.right + gap;
  if (left + card.width > view.width - edge) left = Math.max(edge, row.left - gap - card.width);
  const top = Math.max(edge, Math.min(row.top, view.height - edge - card.height));
  return { top, left };
}

/** 머리 단추 팝오버 자리: 단추 아래, 오른쪽 끝 맞춤(RowMenu align end와 같다). 왼쪽이 모자라면 가장자리에 붙인다 */
export function popoverPlace(button: Box, card: Size, view: Size, gap = 4, edge = 5): { top: number; left: number } {
  const left = Math.max(edge, Math.min(button.right - card.width, view.width - edge - card.width));
  return { top: button.bottom + gap, left };
}

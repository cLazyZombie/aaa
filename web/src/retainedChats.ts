// 화면과 pane 연결의 수명은 같다. 여기에는 본문 사본 없이 수명·예산만 둔다.
import type { PaneView } from "./gen/proto";

export const CHAT_LIMIT = 3;
export const CHAT_BYTES = 8 * 1024 * 1024;
export const CHAT_IDLE_MS = 5 * 60_000;
export interface RetainedChat {
  key: string;
  bornAt: number;
  session: string | null;
  bytes: number;
  hiddenAt: number | null;
}
export const chatKey = (pane: PaneView) => `${pane.machine_id}:${pane.id}`;
export const hasChat = (pane: PaneView) => pane.agent?.harness === "claude" || pane.agent?.harness === "codex";

/** 최근 사용 순. host의 최신 pane으로 삭제·확인된 세션 교체를 판정한다. */
export function retainChats(previous: readonly RetainedChat[], panes: readonly PaneView[], current: string | null, now: number): readonly RetainedChat[] {
  const live = new Map(panes.filter(hasChat).map(p => [chatKey(p), p]));
  let next = previous.flatMap(entry => {
    const pane = live.get(entry.key);
    if (!pane || (entry.session && pane.agent?.harness_session && entry.session !== pane.agent.harness_session)
      || (entry.hiddenAt !== null && now - entry.hiddenAt >= CHAT_IDLE_MS)) return [];
    const hiddenAt = entry.key === current ? null : entry.hiddenAt ?? now;
    const session = pane.agent?.harness_session ?? entry.session;
    return [hiddenAt === entry.hiddenAt && session === entry.session ? entry : { ...entry, hiddenAt, session }];
  });
  if (current && live.has(current)) {
    const held = next.find(e => e.key === current);
    next = [held ?? { key: current, bornAt: now, session: live.get(current)!.agent?.harness_session ?? null, bytes: 0, hiddenAt: null }, ...next.filter(e => e.key !== current)];
  }
  // 활성 대화 단독 초과는 허용한다. 떠나면 즉시 버리고, 비활성 LRU부터 예산을 비운다.
  // 일반 셸을 보고 있을 때도 pane 연결 총수는 4를 넘기지 않는다.
  const ordinaryLimit = CHAT_LIMIT - (current && !live.has(current) ? 1 : 0);
  let ordinary = 0, coordinators = 0;
  next = next.filter(e => {
    if (e.key !== current && e.bytes > CHAT_BYTES) return false;
    return live.get(e.key)?.role === "coordinator" ? ++coordinators <= 1 : ++ordinary <= ordinaryLimit;
  });
  let bytes = next.reduce((sum, e) => sum + e.bytes, 0);
  for (let i = next.length - 1; bytes > CHAT_BYTES && i >= 0; i--) {
    const entry = next[i]!;
    if (entry.key !== current) { bytes -= entry.bytes; next.splice(i, 1); }
  }
  return next.length === previous.length && next.every((e, i) => e === previous[i]) ? previous : next;
}

export function chatExpiry(entries: readonly RetainedChat[]): number | null {
  const times = entries.flatMap(e => e.hiddenAt === null ? [] : [e.hiddenAt + CHAT_IDLE_MS]);
  return times.length ? Math.min(...times) : null;
}

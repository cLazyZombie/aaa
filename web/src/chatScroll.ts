// 대화 보기의 굴림: 보던 줄 고정(plan §8.262)과 끝 판정·thread마다 읽던 자리 기억(plan §8.266). t3code(MIT, 1451e7cb2)를 옮겼다:
// - 끝 판정 문턱 40px: `apps/web/src/components/chat/MessagesTimeline.logic.ts:324` `TIMELINE_FOLLOW_REARM_THRESHOLD_PX`·326줄 `resolveTimelineIsAtEnd`
// - 기준 줄(지금 굴림 위치에 걸친 줄)과 그 줄 안 거리: `packages/client-runtime/src/work-log/scrollAnchor.ts` `resolveWorkGroupScrollAnchor`
// - 자리 기억(최근 100 thread, 따라가는 중이면 끝으로 기억): `apps/web/src/components/chat/timelineScrollAnchoring.ts:143~179`
// 새 agent는 파일/byte/블록 ID를 보낸다(§8.298). ID가 없는 옛 agent의 fallback은 말한 이·글이 같은 줄 가운데
// 끝에서 몇 번째인지와 글의 hash로 둔다: 앞이 잘려도 그대로고, 같은 글이 뒤에 또 붙을 때만 바뀌어 그때는 참고처럼 굴림 거리로 돌아간다.
import type { ChatMessage } from "./gen/proto";

export const CHAT_FOLLOW_REARM_PX = 40;

export function chatIsAtEnd(el: { scrollHeight: number; scrollTop: number; clientHeight: number }): boolean {
  return el.scrollHeight - el.scrollTop - el.clientHeight <= CHAT_FOLLOW_REARM_PX;
}

function hash(text: string): string {
  let h = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) h = Math.imul(h ^ text.charCodeAt(i), 0x01000193);
  return (h >>> 0).toString(36);
}

/** 기록 줄마다 id(말한 이·같은 줄 가운데 끝에서 몇 번째·글 hash) */
export function chatRowIds(messages: readonly Pick<ChatMessage, "role" | "text" | "id">[]): string[] {
  const seen = new Map<string, number>();
  const ids: string[] = [];
  for (let i = messages.length - 1; i >= 0; i--) {
    const m = messages[i]!;
    if (m.id) { ids[i] = m.id; continue; }
    const key = `${m.role}:${m.text.length}:${hash(m.text)}`;
    const n = seen.get(key) ?? 0;
    seen.set(key, n + 1);
    ids[i] = `${key}:${n}`;
  }
  return ids;
}

/** 굴림 위치에 걸친 줄(위가 굴림 위치보다 위인 마지막 줄)과 그 줄 안 거리. `tops`는 줄 위 끝(내용 기준), 오름차순 */
export function chatScrollAnchor(ids: readonly string[], tops: readonly number[], scroll: number):
  { rowId: string; offsetWithinRow: number; scrollOffset: number } | undefined {
  if (ids.length === 0 || tops.length !== ids.length || !Number.isFinite(scroll)) return undefined;
  const scrollOffset = Math.max(0, scroll);
  let low = 0;
  let high = ids.length - 1;
  let index = 0;
  let rowTop = tops[0]!;
  while (low <= high) {
    const middle = Math.floor((low + high) / 2);
    const top = tops[middle]!;
    if (top <= scrollOffset) {
      index = middle;
      rowTop = top;
      low = middle + 1;
    } else high = middle - 1;
  }
  return { rowId: ids[index]!, offsetWithinRow: Math.max(0, scrollOffset - rowTop), scrollOffset };
}

export interface ChatPosition {
  readonly rowId: string;
  readonly offsetWithinRow: number;
  readonly scrollOffset: number;
  readonly atEnd: boolean;
}

const remembered = new Map<string, ChatPosition>();

export function readChatPosition(key: string): ChatPosition | undefined {
  return remembered.get(key);
}

export function rememberChatPosition(key: string, position: ChatPosition, liveFollow: boolean): void {
  remembered.delete(key);
  remembered.set(key, { ...position, atEnd: liveFollow || position.atEnd });
  if (remembered.size > 100) {
    const oldest = remembered.keys().next().value;
    if (oldest !== undefined) remembered.delete(oldest);
  }
}

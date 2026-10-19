// 목록 줄의 작업 시간이 함께 보는 초 시계(plan §8.266). t3code(MIT) `apps/web/src/hooks/useNowSecond.ts`(1451e7cb2, 커밋 6ac3bc553
// 「sidebar working timers share one clock」)를 그대로 옮겼다: 벽시계 초 경계에 맞춘 타이머 하나를 useSyncExternalStore로 나눠
// 작업 중인 모든 줄의 시간이 같은 frame에 바뀐다. 보는 곳이 없으면 타이머도 없고, 그때 읽으면 지금 초를 준다.
import { useSyncExternalStore } from "react";

function currentSecond(): number {
  return Math.floor(Date.now() / 1000) * 1000;
}

let nowSecond = currentSecond();
let timerId: number | null = null;
const listeners = new Set<() => void>();

function scheduleTick(): void {
  timerId = window.setTimeout(
    () => {
      nowSecond = currentSecond();
      for (const listener of listeners) listener();
      scheduleTick();
    },
    1000 - (Date.now() % 1000),
  );
}

function subscribe(listener: () => void): () => void {
  if (listeners.size === 0) scheduleTick();
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0 && timerId !== null) {
      window.clearTimeout(timerId);
      timerId = null;
    }
  };
}

function getSnapshot(): number {
  // 타이머가 도는 동안은 알린 값 그대로다(useSyncExternalStore는 알림 사이에 같은 값을 요구한다)
  if (timerId === null) nowSecond = currentSecond();
  return nowSecond;
}

export function useNowSecond(): number {
  return useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
}

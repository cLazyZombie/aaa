// 입력칸 컨텍스트 게이지의 규칙(plan §8.321). 값은 agent가 대화 기록에서 읽어 `AgentStatus.context`로 보낸다(%는 그 harness 식).
// 사용자 10-20 「컨텍스트 사용량 (%, over하면 실제 k)」: 기본은 사용량 %, 창을 넘었거나 창을 모르면 실제 토큰 수를 k로 보인다.
import type { ContextUsage } from "./gen/proto";

/** 위험 색이 시작하는 %: 구독 사용량 칩(`usage.ts` HIGH_PERCENT)과 같은 기준(이 파일은 node 시험이 읽어 React 모듈을 부르지 않는다) */
export const CONTEXT_HIGH_PERCENT = 80;

/** normal·high(80% 이상)·over(창을 넘음)·unknown(창을 모름) */
export type ContextLevel = "normal" | "high" | "over" | "unknown";

export interface ContextView {
  /** 게이지 옆 글: `38%` 또는 `230k` */
  text: string;
  level: ContextLevel;
  /** 링을 채울 만큼(0~100). 창을 모르면 0 */
  fill: number;
  /** title·화면 읽기 글 */
  title: string;
}

/** 토큰 수를 k로(1,000k를 넘어도 k: 사용자 10-20 「1050k처럼」) */
export function formatK(tokens: number): string {
  return `${tokens > 0 ? Math.max(1, Math.round(tokens / 1000)) : 0}k`;
}

export function contextView(c: ContextUsage): ContextView {
  const used = formatK(c.used);
  if (c.window !== null && c.used > c.window) {
    return { text: used, level: "over", fill: 100, title: `컨텍스트 ${used} 사용 · 창 ${formatK(c.window)}를 넘음` };
  }
  if (c.window === null || c.percent === null) {
    return { text: used, level: "unknown", fill: 0, title: `컨텍스트 ${used} 사용 · 이 모델의 창 크기를 모른다` };
  }
  return {
    text: `${c.percent}%`,
    level: c.percent >= CONTEXT_HIGH_PERCENT ? "high" : "normal",
    fill: c.percent,
    title: `컨텍스트 ${c.percent}% 사용 · ${used} / ${formatK(c.window)}`,
  };
}

/**
 * 글을 치는 동안 숨길까(사용자 10-20 「타이핑시에는 보이지 않게」). 80% 이상·창 넘음은 치는 중에도 보인다: 보내기 전에 정리할지
 * 정할 때이고(t3code는 높을 때 보내기 옆에 「Compact N」 칩을 띄운다), 참고 UI 어디도 높은 값을 숨기지 않는다.
 */
export function hiddenWhileTyping(level: ContextLevel, typing: boolean): boolean {
  return typing && (level === "normal" || level === "unknown");
}

/** 정리된 자리 구분선의 전후 크기: `164k → 28k` */
export function compactionText(before: number, after: number): string {
  return `${formatK(before)} → ${formatK(after)}`;
}

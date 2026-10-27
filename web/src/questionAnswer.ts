import type { ThreadQuestions } from "./gen/proto";

/** 직접 답은 자기 질문만 채운다. 나머지는 고른 값 또는 기존 추천을 쓴다. */
export function withRecommended(q: ThreadQuestions, picks: (number | null)[], comments: (string | null)[] = []): (number | null)[] | null {
  const out: (number | null)[] = [];
  for (const [n, item] of q.items.entries()) {
    if (picks[n] == null && comments[n]?.trim()) {
      out.push(null);
      continue;
    }
    const pick = picks[n] ?? item.choices.findIndex((c) => c.recommended);
    if (pick < 0) return null;
    out.push(pick);
  }
  return out;
}

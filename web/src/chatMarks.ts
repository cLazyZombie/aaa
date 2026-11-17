// 기획 대화 사건 줄의 자리(t-1894, t-1950). 사건의 `after`는 그 사건 전에 보낸 마지막 사람 말의 앞부분이다.
import { unwrapPasted } from "./pastedContent.ts";

/**
 * 사건마다 놓을 자리: 그 말(사람 말의 기록 순번)의 답 뒤. `after`가 없으면 맨 앞(-1)이다. 짝 말을 불러온 기록에서 못 찾으면
 * 시각 차례대로 바로 앞 사건 자리에 둔다(옛 사건이 맨 끝에 몰려 방금 일 같았다, t-1950). 아직 기록에 오지 않은 보내는 중인
 * 말의 사건만 맨 끝(Infinity)이다. `marks`는 시각 차례다
 */
export function markPlaces(
  marks: readonly { after: string | null }[],
  messages: readonly { role: string; text: string }[],
  pending: readonly { text: string }[],
): number[] {
  const norm = (t: string) => t.replace(/\s+/g, " ").trim();
  const users = messages.flatMap((m, i) => (m.role === "user" ? [i] : [])).reverse();
  let last = -1;
  return marks.map((mk) => {
    let place = -1;
    if (mk.after !== null) {
      const want = norm(mk.after);
      const hit = users.find((i) => norm(unwrapPasted(messages[i]!.text)).startsWith(want));
      place = hit ?? (pending.some((p) => norm(p.text).startsWith(want)) ? Number.POSITIVE_INFINITY : last);
    }
    last = place;
    return place;
  });
}

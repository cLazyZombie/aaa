// 대기 메시지 「지금 보내기」(plan §8.271)의 화면 규칙. 대기 말(harness 대기열에 이미 있는 글) 맨 끝에 단추 하나를 둔다.
// 참고 herdr-web-ui PendingMessages.tsx(57-76행, ArrowUp + 「지금 보내기」, 보내는 중 aria-busy)와 PaneTerminal.tsx `actOnPending`
// (터미널 draft면 held_input 거절, 서버가 sent를 알리면 줄을 지운다). aaa는 글을 다시 보내지 않고 host가 고른 키만 보낸다
// (Claude sendNow, Codex Esc). 그 키는 대기열 전부를 보내므로 단추는 묶음에 하나다. React·브라우저를 쓰지 않아 node 시험이 부른다.
import type { SteerRefusal } from "./gen/proto";
import type { AgentStatus } from "./herdr/status.ts";

/** 보낸 뒤 이만큼 대기 말이 줄지 않으면 오래 걸림으로 알린다(입력창 SLOW_MS와 같다) */
export const STEER_SLOW_MS = 10_000;

export type SteerState =
  | { phase: "idle" }
  /** host 답을 기다린다 */
  | { phase: "sending"; id: number }
  /** 키가 들어갔다: 대기 말이 기록에서 빠지기를 기다린다. `before`는 보낼 때 대기 말 수 */
  | { phase: "sent"; at: number; before: number }
  /** 보내지 않았거나 확인하지 못했다. `inspect`면 터미널 보기를 준다 */
  | { phase: "failed"; text: string; inspect: boolean };

export const STEER_IDLE: SteerState = { phase: "idle" };

/** 다른 글을 보내는 중이라 잠깐 못 누르는 까닭. 대기 말 단추와 이 기기 보관함 단추가 같은 말을 쓴다 */
export const STEER_BUSY = "보내는 중인 글이 전달된 뒤 누를 수 있습니다";

/** 대화 정리 중(plan §8.306)에는 harness가 대기 글을 읽지 않는다: 대기 말 단추의 까닭. 이 기기 보관함 단추는 「차례대로 보냅니다」다(`heldSend`) */
export const STEER_COMPACTING = "대화 정리가 끝나면 대기 글이 이어서 읽힙니다";

/** 단추를 보일까. 쉬는 agent는 대기 글을 곧 읽으므로 숨기고, 질문 중·대화 정리 중·다른 글을 보내는 중이면 까닭과 함께 끈다.
 * `hint`는 단추 아래 안내다: 질문 중·정리 중만 둔다(보내는 중은 0.3~0.5초라 줄이 깜박이지 않게 title로만).
 * 정리 중은 t3code처럼 steering을 받지 않는다(`CommandPolicy.ts` 「Compaction … turns cannot take steering input」) */
export function steerAvailability(status: AgentStatus | undefined, busy: boolean, compacting = false): "hidden" | { blocked: string | null; hint: string | null } {
  if (status === "blocked") {
    const why = "질문에 답하면 대기 글이 이어서 읽힙니다";
    return { blocked: why, hint: why };
  }
  if (status !== "working") return "hidden";
  if (compacting) return { blocked: STEER_COMPACTING, hint: STEER_COMPACTING };
  return { blocked: busy ? STEER_BUSY : null, hint: null };
}

const HELD_STEER = {
  claude: "기다리지 않고 지금 턴에 넣습니다. Claude가 하던 일을 끊을 수 있습니다 (Ctrl+Enter와 같음)",
  codex: "기다리지 않고 지금 턴에 넣습니다. Codex는 다음 도구 호출 뒤 읽습니다 (Ctrl+Enter와 같음)",
};

/** 이 기기 보관함(composer-queue)의 「지금 보내기」(plan §8.273, 사용자 결정 10-19 19:36): 대기 말 단추와 같은 뜻이 되게 작업 중이면
 * Ctrl+Enter처럼 steer로 보내 지금 턴에 넣고(`steer`), 쉬면 보통 보내기다. 못 누르는 까닭은 대기 말 단추와 같은 차례·말이다:
 * 질문 중이면 끄고 단추 아래에 안내(`hint`, 글이 질문의 답으로 들어가지 않게), 끊김·다른 글을 보내는 중·빈 글은 title로만 */
export function heldSend(o: { working: boolean; asking: boolean; compacting?: boolean; connected: boolean; busy: boolean; empty: boolean; harness: "claude" | "codex" }): {
  steer: boolean;
  blocked: string | null;
  hint: string | null;
  title: string;
} {
  const steer = o.working;
  const title = steer ? HELD_STEER[o.harness] : "보통 보내기와 같습니다";
  if (o.asking) {
    const why = "질문에 답한 뒤 보낼 수 있습니다";
    return { steer, blocked: why, hint: why, title: why };
  }
  // 정리 중: 보내도 harness가 정리 뒤에 읽는다. 끝나면 입력창이 차례대로 보낸다(plan §8.306)
  if (o.compacting) {
    const why = "대화 정리가 끝나면 차례대로 보냅니다";
    return { steer, blocked: why, hint: why, title: why };
  }
  const blocked = !o.connected ? "연결되면 보낼 수 있습니다" : o.busy ? STEER_BUSY : o.empty ? "빈 메시지는 보낼 수 없습니다" : null;
  return { steer, blocked, hint: null, title: blocked ?? title };
}

export function steerLabel(count: number): string {
  return count > 1 ? `${count}개 지금 보내기` : "지금 보내기";
}

/** host가 보내지 않은 까닭(그리고 답 없이 연결이 끊긴 것: `reason` null) */
export function refusalText(reason: SteerRefusal | null, message: string | null, agent: string): { text: string; inspect: boolean } {
  switch (reason) {
    case "not_working":
      return { text: `${agent}가 작업을 마쳐 대기 글을 곧 읽습니다`, inspect: false };
    case "question":
      return { text: "질문에 답하면 대기 글이 이어서 읽힙니다", inspect: false };
    case "draft":
      return { text: "보내지 못함 · 터미널 입력창에 쓰던 글이 있습니다", inspect: true };
    case "no_queue":
      return { text: `보내지 못함 · ${agent} 화면에 대기 글이 보이지 않습니다(이미 읽었을 수 있습니다)`, inspect: true };
    case "unknown_screen":
      return { text: `보내지 못함 · ${agent} 입력창을 확인하지 못했습니다`, inspect: true };
    case "unsupported":
      return { text: "이 프로그램에는 지금 보내기를 쓸 수 없습니다", inspect: false };
    case "failed":
      return { text: `보내지 못했습니다: ${message ?? "알 수 없는 오류"}`, inspect: true };
    case "compacting":
      return { text: STEER_COMPACTING, inspect: false };
    case null:
      return { text: "연결이 끊겨 보냈는지 확인하지 못했습니다", inspect: true };
    // 새 host가 더한 까닭(옛 웹)
    default:
      return { text: `보내지 못했습니다: ${message ?? String(reason)}`, inspect: true };
  }
}

/** 보낸 뒤: 대기 말이 줄면 끝(기록이 그 글을 보통 사람 말로 보인다), 오래 줄지 않으면 알린다. 그 밖은 그대로.
 * 대화 정리 중(`compacting`)은 harness가 대기 글을 읽지 않으므로 오래 걸림을 세지 않는다: 정리가 끝난 때부터 다시 잰다(plan §8.306) */
export function steerSettled(state: SteerState, pending: number, now: number, compacting = false): SteerState {
  if (state.phase !== "sent") return state;
  if (pending < state.before) return STEER_IDLE;
  if (compacting) return state;
  if (now - state.at >= STEER_SLOW_MS) {
    return { phase: "failed", text: "보냈지만 10초가 지나도 대기 글이 그대로입니다 · 터미널을 확인하세요", inspect: true };
  }
  return state;
}

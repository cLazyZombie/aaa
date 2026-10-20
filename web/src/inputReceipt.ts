import type { ChatMessage } from "./gen/proto";

import { pasteText } from "./pasteText.ts";
import { unwrapPasted } from "./pastedContent.ts";

/** 대화 보기와 같은 공용 봉투 판독. receipt에서는 줄 끝을 맞추고 겹친 전체 봉투도 걷는다. */
export function receiptText(text: string): string {
  let value = text.replace(/\r\n?/g, "\n").trim();
  for (;;) {
    const next = unwrapPasted(value).trim();
    if (next === value) return value;
    value = next;
  }
}

export function receiptMethod(text: string, message: ChatMessage, hash?: string): "normal" | "pasted_content" | "queue_operation" | null {
  if (message.role !== "user") return null;
  if (hash && message.receipt) {
    if (hash !== message.receipt.sha256) return null;
    return message.pending ? "queue_operation" : message.receipt.method === "pasted_content" ? "pasted_content" : "normal";
  }
  if (receiptText(message.text) !== receiptText(text)) return null;
  return message.pending ? "queue_operation" : message.text.trim() === text.trim() ? "normal" : "pasted_content";
}

/** harness 명령 한 줄(`/compact`, `/model opus`). Claude는 `<command-name>` 줄로, Codex는 기록하지 않아 사람 글 증거가 오지 않는다.
 * 경로(`/Users/…`)는 명령이 아니다 */
export function slashCommand(text: string): boolean {
  return /^\/[a-z][\w:-]*(?:[ \t][^\n]*)?$/i.test(text.trim());
}

export async function receiptHash(text: string): Promise<string | undefined> {
  if (!crypto.subtle) return undefined;
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(receiptText(pasteText(text))));
  return Array.from(new Uint8Array(digest), (b) => b.toString(16).padStart(2, "0")).join("");
}

/** 진단 ID는 보안 token이 아니다. 일반 HTTP에서도 getRandomValues는 사용할 수 있다. */
export function inputTraceId(): string {
  if (typeof crypto.randomUUID === "function") return crypto.randomUUID();
  return Array.from(crypto.getRandomValues(new Uint8Array(16)), (b) => b.toString(16).padStart(2, "0")).join("");
}

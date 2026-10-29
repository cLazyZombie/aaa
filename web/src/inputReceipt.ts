import type { ChatMessage } from "./gen/proto";

import { pasteText } from "./pasteText.ts";
import { unwrapPasted } from "./pastedContent.ts";

export interface ReceiptHashes {
  hash?: string;
  /** Claude는 붙여넣은 탭 하나를 공백 네 칸으로 기록한다. 원문 해시도 함께 보존한다. */
  tabHash?: string;
}

/** 대화 보기와 같은 공용 봉투 판독. receipt에서는 줄 끝을 맞추고 겹친 전체 봉투도 걷는다. */
export function receiptText(text: string): string {
  let value = text.replace(/\r\n?/g, "\n").trim();
  for (;;) {
    const next = unwrapPasted(value).trim();
    if (next === value) return value;
    value = next;
  }
}

export function receiptMethod(text: string, message: ChatMessage, hash?: string, tabHash?: string): "normal" | "pasted_content" | "queue_operation" | null {
  if (message.role !== "user") return null;
  if (hash && message.receipt) {
    if (hash !== message.receipt.sha256 && tabHash !== message.receipt.sha256) return null;
    return message.pending ? "queue_operation" : message.receipt.method === "pasted_content" ? "pasted_content" : "normal";
  }
  const original = receiptText(text);
  const recorded = receiptText(message.text);
  if (recorded !== original && recorded !== original.replace(/\t/g, "    ")) return null;
  return message.pending ? "queue_operation" : message.text.trim() === text.trim() ? "normal" : "pasted_content";
}

/** 보낼 때보다 같은 사람 글의 수신 기록이 늘었다. 낙관적 말 표시와 상태 정리가 같은 증거를 쓴다. */
export function receiptSeen(item: ReceiptHashes & { text: string; before: number | null }, messages: readonly ChatMessage[] | null): boolean {
  return item.before !== null && (messages ?? []).filter(m => receiptMethod(item.text, m, item.hash, item.tabHash) !== null).length > item.before;
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

/** 실제 전송 글은 바꾸지 않고 harness가 기록할 수 있는 두 형태의 전체 해시를 준비한다. */
export async function receiptHashes(text: string): Promise<ReceiptHashes> {
  const [hash, tabHash] = await Promise.all([
    receiptHash(text),
    text.includes("\t") ? receiptHash(text.replace(/\t/g, "    ")) : undefined,
  ]);
  return { hash, tabHash };
}

/** 진단 ID는 보안 token이 아니다. 일반 HTTP에서도 getRandomValues는 사용할 수 있다. */
export function inputTraceId(): string {
  if (typeof crypto.randomUUID === "function") return crypto.randomUUID();
  return Array.from(crypto.getRandomValues(new Uint8Array(16)), (b) => b.toString(16).padStart(2, "0")).join("");
}

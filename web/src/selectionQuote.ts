// 고른 글을 출처와 함께 그 pane의 입력 초안 끝에 붙인다(t-1204).
// 참고: herdr-web-ui `src/lib/terminalQuote.ts`(MIT, 터미널 「Quote in message」)의 머리·울타리·붙이기 규칙,
// t3code `apps/web/src/lib/assistantTextSelection.ts`(MIT, 답 「Cite」)의 단추·글자 아닌 것 빼기.
// aaa 입력창은 칩 없는 글이라 받는 agent도 그대로 읽는 Markdown 글로 넣는다.

/** CRLF를 LF로, 앞뒤 빈 줄을 뺀다. 남는 것이 없으면 null(참고 `quotedText`) */
export function quotedText(text: string): string | null {
  const normalized = text.replace(/\r\n?/g, "\n").replace(/^\n+|\n+$/g, "");
  return normalized.trim() === "" ? null : normalized;
}

/** 출처 이름: 줄바꿈·겹친 공백을 한 칸으로 */
function sourceName(source: string, fallback: string): string {
  return source.replace(/\s+/g, " ").trim() || fallback;
}

/** 「t-1204 터미널 12~15줄」(줄 번호는 편집기처럼 1부터). 줄을 모르면 줄 없이 */
export function terminalQuoteSource(source: string, range: { start: number; end: number } | null): string {
  const name = `${sourceName(source, "pane")} 터미널`;
  if (!range) return name;
  const start = Math.max(1, Math.floor(range.start) + 1);
  const end = Math.max(start, Math.floor(range.end) + 1);
  return start === end ? `${name} ${start}줄` : `${name} ${start}~${end}줄`;
}

/** 터미널 글: 머리와, 글 안의 backtick 줄이 닫지 못하는 울타리(참고 `formatTerminalQuote`) */
export function formatTerminalQuote(quote: { source: string; range: { start: number; end: number } | null; text: string }): string | null {
  const text = quotedText(quote.text);
  if (text === null) return null;
  let fence = "```";
  for (const run of text.match(/`{3,}/g) ?? []) if (run.length >= fence.length) fence = "`".repeat(run.length + 1);
  return `인용: ${terminalQuoteSource(quote.source, quote.range)}\n${fence}\n${text}\n${fence}`;
}

export type QuoteSpeaker = "user" | "assistant" | "command" | "command_output" | "thinking" | "tool" | "mixed";

/** 대화 속 누구의 글인가. 여러 말에 걸치면 「대화」만 */
export function chatQuoteSource(source: string, speaker: QuoteSpeaker, harness: "claude" | "codex"): string {
  const name = `${sourceName(source, "pane")} 대화`;
  const who = speaker === "user" ? "사람 글"
    : speaker === "assistant" ? `${harness === "codex" ? "Codex" : "Claude"} 답`
    : speaker === "command" ? "명령"
    : speaker === "command_output" ? "명령 출력"
    : speaker === "thinking" ? "생각"
    : speaker === "tool" ? "도구"
    : null;
  return who ? `${name} · ${who}` : name;
}

/** 대화 글: 머리와 줄마다 `> `(빈 줄은 `>`) */
export function formatChatQuote(quote: { source: string; speaker: QuoteSpeaker; harness: "claude" | "codex"; text: string }): string | null {
  const text = quotedText(quote.text);
  if (text === null) return null;
  const body = text.split("\n").map((line) => (line.trim() === "" ? ">" : `> ${line.trimEnd()}`)).join("\n");
  return `인용: ${chatQuoteSource(quote.source, quote.speaker, quote.harness)}\n${body}`;
}

/**
 * 지금 초안 뒤에 빈 줄을 두고 붙이고, 그 아래에 이어 쓰도록 줄을 하나 더 둔다. 쓰던 글은 그대로다.
 * `max` 글자를 넘으면 null(참고 `appendQuote`)
 */
export function appendQuote(draft: string, block: string, max: number): string | null {
  const gap = draft === "" ? "" : draft.endsWith("\n\n") ? "" : draft.endsWith("\n") ? "\n" : "\n\n";
  const next = `${draft}${gap}${block}\n`;
  return next.length > max ? null : next;
}

// ── 대화 기록에서 고른 글(브라우저) ──

/** 복사·접기 단추, 보이지 않는 글, 말 아래 단추·표지 줄, 기록 안내 줄은 고른 글이 아니다(참고 EXCLUDED_SELECTOR) */
const EXCLUDED = "button, input, textarea, select, [role=button], [hidden], [aria-hidden=true], svg, .chat-turn-meta, .chat-bubble-foot, .chat-pending-tag, .chat-note";
const BLOCK = "address, article, aside, blockquote, dd, div, dl, dt, figcaption, figure, footer, h1, h2, h3, h4, h5, h6, header, hr, li, main, nav, ol, p, pre, section, table, td, th, tr, ul, details, summary";
const SPEAKERS = new Set<QuoteSpeaker>(["user", "assistant", "command", "command_output", "thinking", "tool"]);

/**
 * 대화 칸 안에서 고른 글과 그 글의 화자. 대화 밖·입력칸 안·빈 선택이면 null.
 * 글은 화면에 보이는 글자이고 블록 사이는 줄바꿈이다(Markdown 기호는 그려진 뒤라 없다)
 */
export function chatSelection(container: HTMLElement, selection: Selection | null): { text: string; speaker: QuoteSpeaker; range: Range } | null {
  if (!selection || selection.isCollapsed || selection.rangeCount !== 1) return null;
  const range = selection.getRangeAt(0);
  if (!container.contains(range.commonAncestorContainer)) return null;
  const parts: string[] = [];
  const speakers = new Set<string>();
  let lastBlock: Element | null = null;
  const root = range.commonAncestorContainer;
  const walker = document.createTreeWalker(root.nodeType === Node.TEXT_NODE ? root.parentNode! : root, NodeFilter.SHOW_TEXT);
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    if (!range.intersectsNode(node)) continue;
    const parent = node.parentElement;
    if (!parent || parent.closest(EXCLUDED)) continue;
    const text = node as Text;
    const start = node === range.startContainer ? range.startOffset : 0;
    const end = node === range.endContainer ? range.endOffset : text.length;
    if (start >= end) continue;
    const block = parent.closest(BLOCK);
    if (parts.length > 0 && block !== lastBlock) parts.push("\n");
    lastBlock = block;
    parts.push(text.data.slice(start, end));
    const role = parent.closest<HTMLElement>("[data-role]")?.dataset.role;
    if (role) speakers.add(role);
  }
  const text = parts.join("");
  if (text.trim() === "") return null;
  const only = speakers.size === 1 ? ([...speakers][0] as QuoteSpeaker) : null;
  return { text, speaker: only && SPEAKERS.has(only) ? only : "mixed", range };
}

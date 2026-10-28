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

/** 글 안의 backtick 줄이 닫지 못하는 울타리 */
function fenceFor(text: string): string {
  let fence = "```";
  for (const run of text.match(/`{3,}/g) ?? []) if (run.length >= fence.length) fence = "`".repeat(run.length + 1);
  return fence;
}

/** 터미널 글: 머리와, 글 안의 backtick 줄이 닫지 못하는 울타리(참고 `formatTerminalQuote`) */
export function formatTerminalQuote(quote: { source: string; range: { start: number; end: number } | null; text: string }): string | null {
  const text = quotedText(quote.text);
  if (text === null) return null;
  const fence = fenceFor(text);
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

/** 줄마다 `> `(빈 줄은 `>`) */
function quoteLines(text: string): string {
  return text.split("\n").map((line) => (line.trim() === "" ? ">" : `> ${line.trimEnd()}`)).join("\n");
}

/** 대화 글: 머리와 줄마다 `> `(빈 줄은 `>`) */
export function formatChatQuote(quote: { source: string; speaker: QuoteSpeaker; harness: "claude" | "codex"; text: string }): string | null {
  const text = quotedText(quote.text);
  if (text === null) return null;
  return `인용: ${chatQuoteSource(quote.source, quote.speaker, quote.harness)}\n${quoteLines(text)}`;
}

/** 보고 글(t-1222): 「t-1222 보고 r-4001」. 판(r-ID)은 바뀌지 않는 보고 판이다 */
export function formatReportQuote(quote: { source: string; report: string; text: string }): string | null {
  const text = quotedText(quote.text);
  if (text === null) return null;
  return `인용: ${sourceName(quote.source, "thread")} 보고 ${quote.report}\n${quoteLines(text)}`;
}

/** diff 줄의 쪽: 새(바뀐 뒤)·옛(바뀌기 전) */
export type DiffQuoteSide = "additions" | "deletions";
export interface DiffQuoteRange { start: number; side: DiffQuoteSide; end: number; endSide: DiffQuoteSide }

const SIDE_LABEL: Record<DiffQuoteSide, string> = { additions: "새", deletions: "옛" };

/** 「새 12~15줄」·「새 12줄」, 쪽이 갈리면 「옛 12~새 14줄」 */
export function diffRangeLabel(range: DiffQuoteRange): string {
  if (range.side !== range.endSide) return `${SIDE_LABEL[range.side]} ${range.start}~${SIDE_LABEL[range.endSide]} ${range.end}줄`;
  return range.start === range.end ? `${SIDE_LABEL[range.side]} ${range.start}줄` : `${SIDE_LABEL[range.side]} ${range.start}~${range.end}줄`;
}

/** 변경 기준: 「origin/main 대비 abcd1234」(merge-base 앞 8자), 미커밋은 「미커밋」 */
export function diffScopeLabel(scope: "branch" | "uncommitted", base: string, baseSha: string): string {
  if (scope === "uncommitted") return "미커밋";
  const sha = baseSha.slice(0, 8);
  return sha === "" ? `${base} 대비` : `${base} 대비 ${sha}`;
}

/**
 * diff 줄(t-1222, 참고 Orca `formatDiffComments`의 파일·줄·원문): 「t-1222 diff origin/main 대비 abcd1234 · src/a.rs 새 12~15줄」과
 * 줄마다 ` `·`-`·`+`를 붙인 diff 울타리
 */
export function formatDiffQuote(quote: { source: string; scope: string; path: string; range: DiffQuoteRange; rows: readonly { kind: " " | "-" | "+"; text: string }[] }): string | null {
  if (quote.rows.length === 0) return null;
  const body = quote.rows.map((row) => `${row.kind}${row.text}`.trimEnd()).join("\n");
  const fence = fenceFor(body);
  return `인용: ${sourceName(quote.source, "thread")} diff ${quote.scope} · ${quote.path} ${diffRangeLabel(quote.range)}\n${fence}diff\n${body}\n${fence}`;
}

/** 인용 아래에 단 의견(t-1222, 참고 Orca `User comment`): 「의견: 」 뒤에 쓴 그대로. 빈 의견은 붙이지 않는다 */
export function withComment(block: string, comment: string): string {
  const text = comment.replace(/\r\n?/g, "\n").replace(/^\s*\n|\s+$/g, "");
  return text.trim() === "" ? block : `${block}\n의견: ${text}`;
}

// ── 보낸 글의 인용 머리 읽기(인용 위치로 돌아가기) ──

export type QuoteRef =
  | { kind: "chat"; source: string; who: string | null; text: string }
  | { kind: "terminal"; source: string; range: { start: number; end: number } | null; text: string }
  | { kind: "report"; source: string; report: string; text: string }
  | { kind: "diff"; source: string; scope: "branch" | "uncommitted"; path: string; range: DiffQuoteRange; text: string };

const SIDE_OF: Record<string, DiffQuoteSide> = { 새: "additions", 옛: "deletions" };

type QuoteHead = QuoteRef extends infer R ? (R extends QuoteRef ? Omit<R, "text"> : never) : never;

function quoteHead(head: string): QuoteHead | null {
  let m = /^(.+) 보고 (r-\d+)$/.exec(head);
  if (m) return { kind: "report", source: m[1]!, report: m[2]! };
  m = /^(.+?) diff (.+?) · (.+) (새|옛) (\d+)(?:~(?:(새|옛) )?(\d+))?줄$/.exec(head);
  if (m) {
    const side = SIDE_OF[m[4]!]!;
    const start = Number(m[5]);
    return { kind: "diff", source: m[1]!, scope: m[2] === "미커밋" ? "uncommitted" : "branch", path: m[3]!,
      range: { start, side, end: m[7] ? Number(m[7]) : start, endSide: m[6] ? SIDE_OF[m[6]]! : side } };
  }
  m = /^(.+) 터미널(?: (\d+)(?:~(\d+))?줄)?$/.exec(head);
  if (m) return { kind: "terminal", source: m[1]!, range: m[2] ? { start: Number(m[2]) - 1, end: Number(m[3] ?? m[2]) - 1 } : null };
  m = /^(.+) 대화(?: · (.+))?$/.exec(head);
  if (m) return { kind: "chat", source: m[1]!, who: m[2] ?? null };
  return null;
}

/** 돌아가기 단추의 이름: 「보고 r-4001」·「a.rs 새 12~15줄」·「대화」·「터미널 3~4줄」 */
export function quoteRefLabel(ref: QuoteRef): string {
  if (ref.kind === "report") return `보고 ${ref.report}`;
  if (ref.kind === "diff") return `${ref.path.split("/").at(-1)} ${diffRangeLabel(ref.range)}`;
  if (ref.kind === "terminal") return terminalQuoteSource("", ref.range).replace(/^pane /, "");
  return "대화";
}

/**
 * 글 속 인용(이 파일이 만든 꼴): 머리 줄 `인용: …`과 그 아래 `>` 줄 또는 울타리. 원문은 `>`·울타리·diff 표지를 뗀 글이다.
 * 사람이 고쳐 꼴이 깨진 인용은 건너뛴다
 */
export function parseQuotes(text: string): QuoteRef[] {
  const lines = text.replace(/\r\n?/g, "\n").split("\n");
  const out: QuoteRef[] = [];
  for (let i = 0; i < lines.length; i++) {
    const m = /^인용: (.+)$/.exec(lines[i]!);
    const head = m ? quoteHead(m[1]!.trim()) : null;
    if (head === null) continue;
    const body: string[] = [];
    const fence = /^(`{3,})(diff)?$/.exec(lines[i + 1] ?? "");
    if (fence) {
      let j = i + 2;
      for (; j < lines.length && lines[j] !== fence[1]; j++) body.push(fence[2] ? lines[j]!.slice(1) : lines[j]!);
      if (j >= lines.length) continue;
      i = j;
    } else {
      let j = i + 1;
      for (; j < lines.length && /^>( |$)/.test(lines[j]!); j++) body.push(lines[j]!.replace(/^> ?/, ""));
      i = j - 1;
    }
    const quoted = quotedText(body.join("\n"));
    if (quoted !== null) out.push({ ...head, text: quoted } as QuoteRef);
  }
  return out;
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

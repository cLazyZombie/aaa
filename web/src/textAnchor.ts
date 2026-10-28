// 고른 글의 자리를 글자 차례로 남기고 다시 찾는다(t-1222 인용 위치로 돌아가기). 참고 t3code
// `apps/web/src/lib/assistantTextSelection.ts`(MIT, 4b1024da): 블록 사이를 줄바꿈으로 이은 글에서 공백을 접은 차례(start·end)와
// 앞뒤 문맥(prefix·suffix)을 남기고, 다시 찾을 때는 남긴 차례가 맞으면 그 자리, 아니면 문맥이 맞는 유일한 자리, 그것도 없으면
// 글이 하나뿐일 때 그 자리를 고른다. 다른 곳: aaa 보고 글은 칩이 없는 Markdown이라 받는 쪽 Range를 그대로 고른다(선택).

export interface TextAnchor {
  /** 고른 글(블록 사이는 줄바꿈) */
  text: string;
  /** 공백을 접은 글에서의 차례(UTF-16) */
  start: number;
  end: number;
  prefix: string;
  suffix: string;
}

const CONTEXT = 32;
const EXCLUDED = "button, input, textarea, select, [role=button], [contenteditable], [hidden], [aria-hidden=true], script, style, template, noscript, svg";
const BLOCK = "address, article, aside, blockquote, dd, div, dl, dt, figcaption, figure, footer, h1, h2, h3, h4, h5, h6, header, hr, li, main, nav, ol, p, pre, section, table, td, th, tr, ul";

const fold = (text: string) => text.replace(/\s+/g, " ");

/** 이은 글에서 고른 차례 → 남길 자리(참고 createAssistantTextSelector). 고른 것이 공백뿐이면 null */
export function anchorOf(text: string, rawStart: number, rawEnd: number): TextAnchor | null {
  const quote = text.slice(rawStart, rawEnd);
  if (quote.trim() === "") return null;
  const folded = fold(text);
  let start = fold(text.slice(0, rawStart)).length;
  if (rawStart > 0 && /\s/.test(text[rawStart - 1]!) && /\s/.test(text[rawStart]!)) start -= 1;
  const end = fold(text.slice(0, rawEnd)).length;
  return { text: quote, start, end, prefix: folded.slice(Math.max(0, start - CONTEXT), start), suffix: folded.slice(end, end + CONTEXT) };
}

/** 남긴 자리를 글에서 다시 찾는다(참고 findAssistantCitationText). 문맥이 두 곳에 맞거나 글이 여럿인데 맞는 곳이 없으면 null */
export function findAnchor(text: string, anchor: Pick<TextAnchor, "text" | "start" | "end" | "prefix" | "suffix">): { start: number; end: number } | null {
  const folded = fold(text);
  const quote = fold(anchor.text);
  if (quote.trim() === "") return null;
  const prefix = fold(anchor.prefix);
  const suffix = fold(anchor.suffix);
  const fits = (start: number, end: number) =>
    folded.slice(Math.max(0, start - prefix.length), start) === prefix && folded.slice(end, end + suffix.length) === suffix;
  let match = anchor.start >= 0 && anchor.end - anchor.start === quote.length && folded.slice(anchor.start, anchor.end) === quote && fits(anchor.start, anchor.end)
    ? { start: anchor.start, end: anchor.end } : null;
  // 보낸 글에서 읽은 인용은 문맥이 없다: 처음 나온 곳이다
  if (prefix === "" && suffix === "") {
    const first = folded.indexOf(quote);
    return match ?? (first < 0 ? null : { start: first, end: first + quote.length });
  }
  let only: { start: number; end: number } | null = null;
  let count = 0;
  for (let start = folded.indexOf(quote); start !== -1; start = folded.indexOf(quote, start + 1)) {
    const end = start + quote.length;
    count += 1;
    only = { start, end };
    if (!fits(start, end)) continue;
    if (match !== null && match.start !== start) return null;
    match = { start, end };
  }
  return match ?? (count === 1 ? only : null);
}

interface Chunk { node: Text; start: number; end: number }

/** 보이는 글자 node를 블록 사이 줄바꿈으로 잇는다(참고 readAssistantText) */
function readText(root: Element): { text: string; chunks: Chunk[] } {
  const parts: string[] = [];
  const chunks: Chunk[] = [];
  let length = 0;
  let separator = false;
  const visit = (node: Node) => {
    if (node.nodeType === 3) {
      const text = node as Text;
      if (text.length === 0) return;
      if (separator && length > 0) {
        parts.push("\n");
        length += 1;
      }
      separator = false;
      chunks.push({ node: text, start: length, end: length + text.length });
      parts.push(text.data);
      length += text.length;
      return;
    }
    if (node.nodeType !== 1) return;
    const element = node as Element;
    if (element.matches(EXCLUDED)) return;
    const block = element.matches(BLOCK);
    if (block || element.tagName === "BR") separator = true;
    for (const child of element.childNodes) visit(child);
    if (block) separator = true;
  };
  visit(root);
  return { text: parts.join(""), chunks };
}

const excluded = (node: Node) => (node.nodeType === 1 ? (node as Element) : node.parentElement)?.closest(EXCLUDED) ?? null;

/** 고른 글이 root 안에 있으면 그 글과 자리(참고 captureAssistantTextSelection) */
export function captureAnchor(root: Element, selection: Selection | null): { anchor: TextAnchor; range: Range } | null {
  if (!selection || selection.isCollapsed || selection.rangeCount !== 1) return null;
  const range = selection.getRangeAt(0);
  if (!root.contains(range.commonAncestorContainer) || excluded(range.startContainer) || excluded(range.endContainer)) return null;
  const stream = readText(root);
  let rawStart: number | null = null;
  let rawEnd = 0;
  for (const chunk of stream.chunks) {
    if (!range.intersectsNode(chunk.node)) continue;
    const start = range.startContainer === chunk.node ? range.startOffset : 0;
    const end = range.endContainer === chunk.node ? range.endOffset : chunk.node.length;
    if (start >= end) continue;
    rawStart ??= chunk.start + start;
    rawEnd = chunk.start + end;
  }
  if (rawStart === null) return null;
  const anchor = anchorOf(stream.text, rawStart, rawEnd);
  return anchor && { anchor, range };
}

/** 접은 차례 → 이은 글 차례(참고 rawTextOffset) */
function rawOffset(text: string, offset: number): number {
  let at = 0;
  for (const m of text.matchAll(/\s+|\S+/g)) {
    const space = /\s/.test(m[0][0]!);
    const length = space ? 1 : m[0].length;
    if (offset <= at + length) return m.index + (space && offset > at ? m[0].length : offset - at);
    at += length;
  }
  return text.length;
}

/** 남긴 자리의 Range(참고 resolveAssistantCitationRange). 글이 바뀌어 찾지 못하면 null */
export function resolveAnchor(root: Element, anchor: Pick<TextAnchor, "text" | "start" | "end" | "prefix" | "suffix">): Range | null {
  const stream = readText(root);
  const match = findAnchor(stream.text, anchor);
  if (match === null) return null;
  const start = rawOffset(stream.text, match.start);
  const end = rawOffset(stream.text, match.end);
  const first = stream.chunks.find((c) => c.end > start);
  let last: Chunk | undefined;
  for (const c of stream.chunks) if (c.start < end) last = c;
  if (!first || !last) return null;
  const range = root.ownerDocument.createRange();
  range.setStart(first.node, Math.max(0, start - first.start));
  range.setEnd(last.node, Math.min(last.node.length, end - last.start));
  return range;
}

/** 찾은 자리를 고르고 화면 가운데로 데려온다 */
export function revealRange(range: Range): void {
  const selection = window.getSelection();
  selection?.removeAllRanges();
  selection?.addRange(range);
  const node = range.startContainer.nodeType === 1 ? (range.startContainer as Element) : range.startContainer.parentElement;
  node?.scrollIntoView({ block: "center", behavior: "instant" });
}

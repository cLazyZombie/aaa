// 대화 안 찾기(plan §8.308, t3code `chat/threadFind.ts`·`useThreadFind.ts`·`threadFindHighlights.ts`·`threadFindPosition.ts`,
// `packages/shared/src/threadSearch.ts`·`threadFindText.ts`). 참고는 서버가 thread 전체에서 세고(ThreadFind.ts) 화면은 그린 글에
// CSS Highlight를 칠한다. aaa는 받아 둔 기록(chat.messages)을 이 화면에서 센다: 화면 밖(가상화로 본문이 없는) 줄도 센다. 그래서
// 데이터에서 뽑는 글이 Markdown.tsx가 그리는 DOM 글과 같은 차례·같은 블록 경계여야 한다(참고 `markdownThreadFindText`가 렌더와 같은
// pipeline을 쓰는 까닭). 도구 줄·생각은 참고처럼 찾지 않는다(참고는 사람 말·답·계획만).
import { foldCode, parseMarkdown, type InlineNode, type MarkdownBlock } from "./herdr/markdown.ts";
import { fileUriPath } from "./herdr/terminalFileLinks.ts";
import { normalizeCode } from "./herdr/highlight.ts";

/** 찾은 글이 든 곳(DOM): 사람 말 본문·답. 이 안의 글만 센다(참고 `[data-thread-find-text]`) */
export const FIND_TEXT_ATTR = "data-chat-find-text";
/** 그 안이라도 세지 않는 머리·단추(참고 `[data-thread-find-ignore]`) */
export const FIND_IGNORE_ATTR = "data-chat-find-ignore";
/** 접혀 가린 글(긴 사람 말·긴 code 꼬리): 활성 찾음이 거기 있으면 `beforematch`로 편다(참고 `[data-thread-find-fold]`) */
export const FIND_FOLD_ATTR = "data-chat-find-fold";

// 참고 THREAD_FIND_BLOCK_TAGS: 글자 단위 감싸개(문법 강조 span)는 찾는 말을 끊지 않고 블록은 끊는다
const BLOCK_TAGS = new Set(["address", "article", "aside", "blockquote", "br", "dd", "details", "div", "dl", "dt", "figcaption", "figure", "footer",
  "h1", "h2", "h3", "h4", "h5", "h6", "header", "hr", "li", "main", "nav", "ol", "p", "pre", "section", "summary", "table", "tbody", "td", "th",
  "thead", "tr", "ul"]);

/** 참고 `foldThreadSearchCase`: 대소문자를 접되 길이가 바뀌는 글자는 두어 offset이 원문과 맞는다 */
export function foldFindCase(text: string): string {
  const lowered = text.toLowerCase().replace(/ς/g, "σ");
  if (lowered.length === text.length) return lowered;
  let folded = "";
  for (const character of text) {
    const one = character.toLowerCase().replace(/ς/g, "σ");
    folded += one.length === character.length ? one : character;
  }
  return folded;
}

/** 참고 `scanThreadSearchOccurrences`: 겹치지 않는 찾음의 시작 offset */
export function findOccurrences(text: string, query: string): number[] {
  const offsets: number[] = [];
  if (text.length === 0 || query.length === 0) return offsets;
  const hay = foldFindCase(text);
  const needle = foldFindCase(query);
  let cursor = 0;
  while (cursor <= hay.length - needle.length) {
    const offset = hay.indexOf(needle, cursor);
    if (offset === -1) break;
    offsets.push(offset);
    cursor = offset + needle.length;
  }
  return offsets;
}

/** 참고 `stepThreadFindIndex`: 끝에서 처음으로 돈다 */
export function stepFindIndex(index: number, total: number, delta: number): number {
  if (total <= 0) return 0;
  const clamped = clampFindIndex(index, total);
  return (((clamped + delta) % total) + total) % total;
}

function clampFindIndex(index: number, total: number): number {
  if (total <= 0 || !Number.isFinite(index) || index < 0) return 0;
  return Math.min(Math.trunc(index), total - 1);
}

/** 참고 `formatThreadFindCount` */
export function formatFindCount(index: number, total: number): string {
  return total <= 0 ? "0/0" : `${clampFindIndex(index, total) + 1}/${total}`;
}

/**
 * Markdown.tsx가 그리는 글을 블록마다 하나씩(DOM walk가 블록 태그에서 끊는 것과 같다). `canOpen`은 OpenFileContext가 있는지:
 * 없으면 파일 이름 뒤에 「 (경로)」가, 있으면 `file://` code가 경로로 그려진다. code 안 줄바꿈은 두고 나머지는 빈칸이다(pre 밖 DOM).
 */
export function markdownFindSegments(markdown: string, canOpen: boolean): string[] {
  const segments: string[] = [];
  let text = "";
  const flush = () => {
    if (text.trim()) segments.push(text);
    text = "";
  };
  const inline = (nodes: InlineNode[], open: boolean) => {
    for (const node of nodes) {
      switch (node.type) {
        case "text":
          text += node.value.replace(/\r?\n/g, " ");
          break;
        case "code": {
          const file = open ? fileUriPath(node.value) : null;
          text += (file ?? node.value).replace(/\r?\n/g, " ");
          break;
        }
        case "math":
          text += `\\(${node.value}\\)`.replace(/\r?\n/g, " ");
          break;
        case "image":
          // 대화에는 그림 resolver가 없다: 대체 글(「!」 + 이름)
          inline(node.fallback, open);
          break;
        case "strong":
        case "em":
        case "del":
          inline(node.children, open);
          break;
        case "link":
          inline(node.children, false);
          break;
        case "file":
          inline(node.children, false);
          if (!open) text += ` (${node.path})`.replace(/\r?\n/g, " ");
          break;
      }
    }
  };
  const blocks = (list: MarkdownBlock[]) => {
    for (const block of list) {
      flush();
      switch (block.type) {
        case "heading":
          inline(block.content, canOpen);
          break;
        case "paragraph":
          // 줄 사이 <br>이 블록 경계다
          for (const line of block.lines) {
            inline(line, canOpen);
            flush();
          }
          break;
        case "list":
          for (const item of block.items) {
            flush();
            inline(item.content, canOpen);
            flush();
            if (item.blocks) blocks(item.blocks);
          }
          break;
        case "blockquote":
          blocks(block.blocks);
          break;
        case "code": {
          // 접힌 code는 머리(HighlightedCode)와 꼬리(가린 pre)가 다른 블록이다. 찾는 말에 줄바꿈이 없어 줄 경계를 넘지 않는다
          const value = normalizeCode(block.value);
          const fold = foldCode(value);
          if (fold === null) text += value;
          else {
            text += fold.head;
            flush();
            text += value.split("\n").slice(fold.head.split("\n").length).join("\n");
          }
          break;
        }
        case "math":
          text += `\\[${block.value}\\]`.replace(/\r?\n/g, " ");
          break;
        case "hr":
          break;
        case "table":
          for (const cell of block.header) {
            inline(cell, canOpen);
            flush();
          }
          for (const row of block.rows) {
            for (const cell of row) {
              inline(cell, canOpen);
              flush();
            }
          }
          break;
      }
      flush();
    }
  };
  blocks(parseMarkdown(markdown));
  flush();
  return segments;
}

/**
 * 한 말(사람·답)의 찾을 글. 기록 전체를 쿼리마다 다시 뽑지 않게 그 말(받아 둔 기록의 객체)에 붙여 기억한다. 기록에서 빠진 말과
 * 함께 사라지므로 수만 줄을 앞까지 받아도 상한에 걸려 처음부터 다시 뽑지 않는다(plan §8.310)
 */
const segmentCache = new WeakMap<object, { text: string; canOpen: boolean; segments: string[] }>();
export function messageFindSegments(message: object, text: string, canOpen: boolean): string[] {
  const cached = segmentCache.get(message);
  if (cached !== undefined && cached.text === text && cached.canOpen === canOpen) return cached.segments;
  const segments = markdownFindSegments(text, canOpen);
  segmentCache.set(message, { text, canOpen, segments });
  return segments;
}

export function countInSegments(segments: readonly string[], query: string): number {
  let count = 0;
  for (const segment of segments) count += findOccurrences(segment, query).length;
  return count;
}

/** 찾은 자리: 줄(row id)과 그 줄 안 차례(참고 ThreadFindMatch의 entryId·occurrence) */
export interface FindMatch {
  rowId: string;
  occurrence: number;
}

/** 줄마다 개수에서 전체 차례 → 자리. 개수 0이면 null */
export function matchAt(rows: readonly { id: string; count: number }[], index: number): FindMatch | null {
  let rest = index;
  for (const row of rows) {
    if (rest < row.count) return { rowId: row.id, occurrence: rest };
    rest -= row.count;
  }
  return null;
}

/** 자리 → 전체 차례. 그 줄이 없거나(기록이 바뀜) 개수가 줄었으면 가까운 쪽으로 */
export function indexOf(rows: readonly { id: string; count: number }[], match: FindMatch): number | null {
  let before = 0;
  for (const row of rows) {
    if (row.id === match.rowId) return row.count === 0 ? null : before + Math.min(match.occurrence, row.count - 1);
    before += row.count;
  }
  return null;
}

/**
 * 찾기를 열거나 말을 바꿀 때의 시작(참고 readThreadFindPosition → ThreadFind selectMatch의 start): 보이는 첫 줄의 보이는 첫 찾음부터.
 * `firstVisible`은 화면 맨 위에 걸친 줄과 그 줄에서 화면 위로 나간 찾음 수다. 그 뒤에 없으면 처음으로
 */
export function startIndex(rows: readonly { id: string; count: number }[], firstVisible: { rowId: string; passed: number } | null): number {
  const total = rows.reduce((sum, row) => sum + row.count, 0);
  if (total === 0 || firstVisible === null) return 0;
  let before = 0;
  for (const row of rows) {
    if (row.id === firstVisible.rowId) {
      const index = before + Math.min(firstVisible.passed, row.count);
      return index >= total ? 0 : index;
    }
    before += row.count;
  }
  return 0;
}

export interface FindRange {
  occurrence: number;
  range: Range;
}

/**
 * 한 줄 DOM에서 찾은 Range(참고 collectThreadFindRanges): 찾을 글 영역 안의 글자 node를 블록 경계마다 이어 붙여 찾고, 그 offset을
 * node 위치로 되돌린다. svg·세지 않는 곳은 건너뛰며 끊는다. pre 밖 줄바꿈은 빈칸이다(그려진 글과 같게)
 */
export function collectFindRanges(row: Element, query: string): FindRange[] {
  const ranges: FindRange[] = [];
  if (query.length === 0) return ranges;
  let occurrence = 0;
  for (const scope of row.querySelectorAll(`[${FIND_TEXT_ATTR}]`)) {
    if (scope.parentElement?.closest(`[${FIND_TEXT_ATTR}]`)) continue;
    let text = "";
    let nodes: { node: Node; start: number; end: number }[] = [];
    const flush = () => {
      let first = 0;
      let last = 0;
      for (const offset of findOccurrences(text, query)) {
        while (first < nodes.length && nodes[first]!.end <= offset) first++;
        if (last < first) last = first;
        while (last < nodes.length && nodes[last]!.end < offset + query.length) last++;
        const start = nodes[first];
        const end = nodes[last];
        if (!start || !end) continue;
        const range = row.ownerDocument.createRange();
        range.setStart(start.node, offset - start.start);
        range.setEnd(end.node, offset + query.length - end.start);
        ranges.push({ occurrence: occurrence++, range });
      }
      text = "";
      nodes = [];
    };
    const visit = (node: Node, inPre: boolean) => {
      const element = node.nodeType === 1 ? (node as Element) : null;
      if (element?.matches("svg")) return;
      if (element?.hasAttribute(FIND_IGNORE_ATTR)) {
        flush();
        return;
      }
      const tag = element?.tagName.toLowerCase() ?? "";
      const block = BLOCK_TAGS.has(tag);
      if (block) flush();
      if (node.nodeType === 3) {
        const value = node.nodeValue ?? "";
        const start = text.length;
        text += inPre ? value : value.replace(/\r?\n/g, " ");
        nodes.push({ node, start, end: text.length });
      }
      for (const child of node.childNodes) visit(child, inPre || tag === "pre");
      if (block) flush();
    };
    visit(scope, false);
    flush();
  }
  return ranges;
}

/** Range를 가린 접힘(참고 foldsHiding): `hidden`이거나 잘린 아래에 있다 */
export function foldsHiding(range: Range, container: Element): Element[] {
  const folds: Element[] = [];
  for (let fold = range.startContainer.parentElement?.closest(`[${FIND_FOLD_ATTR}], [hidden]`); fold && container.contains(fold);
    fold = fold.parentElement?.closest(`[${FIND_FOLD_ATTR}], [hidden]`)) {
    if (fold.hasAttribute("hidden") || range.getBoundingClientRect().bottom > fold.getBoundingClientRect().bottom) folds.push(fold);
  }
  return folds;
}

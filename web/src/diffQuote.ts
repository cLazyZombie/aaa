// diff에서 고른 줄의 원문(t-1222). `@pierre/diffs`의 줄 고르기는 줄 번호와 쪽(새·옛)만 준다: 그 사이 줄을 한 줄 보기 차례로
// 다시 세워 ` `·`-`·`+` 표지와 함께 꺼낸다(참고 Orca `diff-comment-types.ts`의 startLine·lineNumber, 원문은 Markdown 의견의
// selectedText처럼 의견을 단 때의 글을 남긴다). 펼친 맥락(전체 파일을 읽은 diff)은 hunk 사이 줄도 센다.
import type { FileDiffMetadata } from "@pierre/diffs";
import type { DiffQuoteRange, DiffQuoteSide } from "./selectionQuote.ts";

export interface DiffRow {
  kind: " " | "-" | "+";
  /** 옛 파일 줄 번호(더한 줄은 없음) */
  old: number | null;
  /** 새 파일 줄 번호(지운 줄은 없음) */
  new: number | null;
  text: string;
}

const strip = (line: string | undefined) => (line ?? "").replace(/\r?\n$/, "");

/** 한 줄 보기 차례의 줄들. 일부만 읽은 patch는 hunk 사이 줄을 모른다(그 줄은 화면에도 없다) */
export function diffRows(d: FileDiffMetadata): DiffRow[] {
  const rows: DiffRow[] = [];
  let oldNo = 1;
  let newNo = 1;
  const gap = (until: number) => {
    // 전체 파일을 읽었으면 새 파일의 그 줄이 맥락 줄이다
    for (; newNo < until; oldNo++, newNo++) if (!d.isPartial) rows.push({ kind: " ", old: oldNo, new: newNo, text: strip(d.additionLines[newNo - 1]) });
  };
  for (const hunk of d.hunks) {
    gap(hunk.additionStart);
    oldNo = Math.max(hunk.deletionStart, 1);
    newNo = Math.max(hunk.additionStart, 1);
    for (const c of hunk.hunkContent) {
      if (c.type === "context") {
        for (let i = 0; i < c.lines; i++) rows.push({ kind: " ", old: oldNo++, new: newNo++, text: strip(d.additionLines[c.additionLineIndex + i]) });
      } else {
        for (let i = 0; i < c.deletions; i++) rows.push({ kind: "-", old: oldNo++, new: null, text: strip(d.deletionLines[c.deletionLineIndex + i]) });
        for (let i = 0; i < c.additions; i++) rows.push({ kind: "+", old: null, new: newNo++, text: strip(d.additionLines[c.additionLineIndex + i]) });
      }
    }
  }
  if (!d.isPartial) gap(d.additionLines.length + 1);
  return rows;
}

function rowOf(rows: readonly DiffRow[], line: number, side: DiffQuoteSide): number {
  return rows.findIndex((r) => (side === "deletions" ? r.old === line && r.kind !== "+" : r.new === line && r.kind !== "-"));
}

/** 고른 범위(양 끝 포함)의 줄. 끝을 찾지 못하면(그 뒤 diff가 바뀜) 빈 목록 */
export function pickDiffRows(rows: readonly DiffRow[], range: DiffQuoteRange): DiffRow[] {
  const a = rowOf(rows, range.start, range.side);
  const b = rowOf(rows, range.end, range.endSide);
  if (a < 0 || b < 0) return [];
  return rows.slice(Math.min(a, b), Math.max(a, b) + 1);
}

/** 라이브러리 범위(쪽이 없을 수 있다)를 위에서 아래 차례로 */
export function diffQuoteRange(rows: readonly DiffRow[], range: { start: number; side?: DiffQuoteSide; end: number; endSide?: DiffQuoteSide }): DiffQuoteRange {
  const side = range.side ?? "additions";
  const endSide = range.endSide ?? side;
  const a = rowOf(rows, range.start, side);
  const b = rowOf(rows, range.end, endSide);
  return a >= 0 && b >= 0 && b < a ? { start: range.end, side: endSide, end: range.start, endSide: side } : { start: range.start, side, end: range.end, endSide };
}

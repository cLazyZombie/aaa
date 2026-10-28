// 보고 글·diff 줄 의견과 인용 위치로 돌아가기(t-1222): 인용 꼴(보고 판·diff 기준·파일·줄·의견), 보낸 글에서 인용 머리를 다시 읽기,
// diff에서 고른 줄의 원문, 고른 글 자리의 문맥 찾기, 의견 모음의 차례·저장.
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { parsePatchFiles } from "@pierre/diffs";
import { diffRangeLabel, diffScopeLabel, formatChatQuote, formatDiffQuote, formatReportQuote, formatTerminalQuote, parseQuotes, quoteRefLabel, withComment } from "../src/selectionQuote.ts";
import { diffQuoteRange, diffRows, pickDiffRows } from "../src/diffQuote.ts";
import { anchorOf, findAnchor } from "../src/textAnchor.ts";
import { formatNotes, readNotes, sortNotes, writeNotes } from "../src/reviewNoteStore.ts";

const PATCH = `diff --git a/src/a.rs b/src/a.rs
index 1111111..2222222 100644
--- a/src/a.rs
+++ b/src/a.rs
@@ -10,4 +10,4 @@ fn main() {
 let a = 1;
-let b = 2;
+let b = 3;
 let c = \`x\`;
 let d = 4;
`;

describe("보고·diff 인용의 글 꼴", () => {
  it("보고 글은 thread와 보고 판, diff 줄은 기준·파일·줄과 표지 붙은 울타리, 의견은 「의견:」 줄이다", () => {
    assert.equal(formatReportQuote({ source: "t-1222", report: "r-4001", text: "첫 줄\n\n둘째 줄  " }), "인용: t-1222 보고 r-4001\n> 첫 줄\n>\n> 둘째 줄");
    assert.equal(diffScopeLabel("branch", "origin/main", "abcdef0123456789"), "origin/main 대비 abcdef01");
    assert.equal(diffScopeLabel("uncommitted", "HEAD", "abcdef01"), "미커밋");
    assert.equal(diffRangeLabel({ start: 12, side: "additions", end: 15, endSide: "additions" }), "새 12~15줄");
    assert.equal(diffRangeLabel({ start: 3, side: "deletions", end: 3, endSide: "deletions" }), "옛 3줄");
    assert.equal(diffRangeLabel({ start: 11, side: "deletions", end: 12, endSide: "additions" }), "옛 11~새 12줄");
    assert.equal(formatDiffQuote({ source: "t-1222", scope: "origin/main 대비 abcdef01", path: "src/a.rs", range: { start: 11, side: "deletions", end: 11, endSide: "additions" },
      rows: [{ kind: "-", text: "let b = 2;" }, { kind: "+", text: "let b = 3;" }, { kind: " ", text: "```" }] }),
      "인용: t-1222 diff origin/main 대비 abcdef01 · src/a.rs 옛 11~새 11줄\n````diff\n-let b = 2;\n+let b = 3;\n ```\n````");
    assert.equal(withComment("인용: x", "  \n이렇게 바꿔\n둘째 줄\n\n"), "인용: x\n의견: 이렇게 바꿔\n둘째 줄");
    assert.equal(withComment("인용: x", " \n "), "인용: x");
  });

  it("보낸 글의 인용 머리를 다시 읽는다: 네 꼴 모두, 원문은 표지를 뗀 글이고 깨진 인용은 건너뛴다", () => {
    const text = [
      "앞 글",
      formatReportQuote({ source: "t-1222", report: "r-4001", text: "보고 글 한 줄" }),
      "의견: 고쳐 줘",
      "",
      formatDiffQuote({ source: "t-1222", scope: "미커밋", path: "web/src/a b.ts", range: { start: 4, side: "additions", end: 6, endSide: "additions" },
        rows: [{ kind: " ", text: "a" }, { kind: "+", text: "b" }, { kind: "+", text: "c" }] }),
      formatTerminalQuote({ source: "mac-a build", range: { start: 2, end: 3 }, text: "$ ls\nfile" }),
      formatChatQuote({ source: "t-1204", speaker: "assistant", harness: "claude", text: "답 첫 줄\n답 둘째 줄" }),
      "인용: t-1 터미널 2줄",
      "```",
      "닫히지 않은 울타리",
    ].join("\n");
    assert.deepEqual(parseQuotes(text), [
      { kind: "report", source: "t-1222", report: "r-4001", text: "보고 글 한 줄" },
      { kind: "diff", source: "t-1222", scope: "uncommitted", path: "web/src/a b.ts", range: { start: 4, side: "additions", end: 6, endSide: "additions" }, text: "a\nb\nc" },
      { kind: "terminal", source: "mac-a build", range: { start: 2, end: 3 }, text: "$ ls\nfile" },
      { kind: "chat", source: "t-1204", who: "Claude 답", text: "답 첫 줄\n답 둘째 줄" },
    ]);
    const [diff] = parseQuotes(formatDiffQuote({ source: "t-9", scope: "origin/main 대비 0123abcd", path: "a.rs", range: { start: 11, side: "deletions", end: 12, endSide: "additions" }, rows: [{ kind: "-", text: "x" }] }));
    assert.deepEqual(diff.range, { start: 11, side: "deletions", end: 12, endSide: "additions" });
    assert.equal(diff.scope, "branch");
    assert.deepEqual(parseQuotes("인용: 이상한 머리\n> 글").length, 0);
    assert.deepEqual(parseQuotes(text).map(quoteRefLabel), ["보고 r-4001", "a b.ts 새 4~6줄", "터미널 3~4줄", "대화"]);
  });
});

describe("diff에서 고른 줄", () => {
  const [file] = parsePatchFiles(PATCH, "t").flatMap((p) => p.files);
  const rows = diffRows(file);

  it("hunk를 한 줄 보기 차례로 세우고 옛·새 줄 번호를 매긴다", () => {
    assert.deepEqual(rows.map((r) => [r.kind, r.old, r.new, r.text]), [
      [" ", 10, 10, "let a = 1;"], ["-", 11, null, "let b = 2;"], ["+", null, 11, "let b = 3;"], [" ", 12, 12, "let c = `x`;"], [" ", 13, 13, "let d = 4;"],
    ]);
  });

  it("고른 범위의 줄을 꺼내고, 아래에서 위로 고른 범위는 위에서 아래로 바로잡고, 없는 줄이면 비운다", () => {
    const range = diffQuoteRange(rows, { start: 12, side: "additions", end: 11, endSide: "deletions" });
    assert.deepEqual(range, { start: 11, side: "deletions", end: 12, endSide: "additions" });
    assert.deepEqual(pickDiffRows(rows, range).map((r) => r.kind + r.text), ["-let b = 2;", "+let b = 3;", " let c = `x`;"]);
    assert.deepEqual(pickDiffRows(rows, { start: 10, side: "deletions", end: 10, endSide: "additions" }).map((r) => r.text), ["let a = 1;"]);
    assert.deepEqual(pickDiffRows(rows, { start: 40, side: "additions", end: 41, endSide: "additions" }), []);
  });

  it("전체 파일을 읽은 diff는 hunk 사이·뒤 맥락 줄도 센다", () => {
    const full = { ...file, isPartial: false, additionLines: Array.from({ length: 14 }, (_, i) => `n${i + 1}\n`), deletionLines: Array.from({ length: 14 }, (_, i) => `o${i + 1}\n`),
      hunks: [{ ...file.hunks[0], hunkContent: [{ type: "context", lines: 1, additionLineIndex: 9, deletionLineIndex: 9 }, { type: "change", deletions: 1, deletionLineIndex: 10, additions: 1, additionLineIndex: 10 },
        { type: "context", lines: 2, additionLineIndex: 11, deletionLineIndex: 11 }] }] };
    const all = diffRows(full);
    assert.equal(all.length, 15);
    assert.deepEqual(all[0], { kind: " ", old: 1, new: 1, text: "n1" });
    assert.deepEqual(all.at(-1), { kind: " ", old: 14, new: 14, text: "n14" });
  });
});

describe("고른 글 자리", () => {
  const text = "머리\n같은 말이다. 첫째\n같은 말이다. 둘째\n끝";

  it("문맥으로 같은 글 둘 중 고른 곳을 찾고, 글이 바뀌어도 문맥이 맞으면 찾는다", () => {
    const start = text.indexOf("같은 말이다", 10);
    const anchor = anchorOf(text, start, start + "같은 말이다".length);
    assert.equal(anchor.text, "같은 말이다");
    assert.deepEqual(findAnchor(text, anchor), { start: anchor.start, end: anchor.end });
    const moved = `앞에 더한 줄\n${text}`;
    const found = findAnchor(moved, anchor);
    assert.equal(moved.replace(/\s+/g, " ").slice(found.start, found.end), "같은 말이다");
    assert.ok(found.start > moved.indexOf("둘째") - 10, "둘째 줄의 것을 골라야 한다");
    assert.equal(anchorOf(text, 2, 3), null);
  });

  it("문맥 없는 인용(보낸 글에서 읽음)은 처음 나온 곳, 없으면 null", () => {
    assert.deepEqual(findAnchor(text, { text: "같은  말이다.\n첫째", start: -1, end: -1, prefix: "", suffix: "" }), { start: 3, end: 13 });
    assert.equal(findAnchor(text, { text: "없는 글", start: -1, end: -1, prefix: "", suffix: "" }), null);
  });
});

describe("의견 모음", () => {
  const anchor = (text, start) => ({ text, start, end: start + text.length, prefix: "", suffix: "" });
  const diffNote = (id, path, start, at) => ({ id, at, comment: `${id} 의견`, source: { kind: "diff", scope: "branch", base: "origin/main", base_sha: "abcdef0123", path,
    range: { start, side: "additions", end: start, endSide: "additions" }, rows: [{ kind: "+", text: `${path}:${start}` }] } });
  const notes = [
    diffNote("d2", "src/b.rs", 3, 1),
    { id: "r2", at: 2, comment: "", source: { kind: "report", report: "r-4001", anchor: anchor("둘째", 20) } },
    diffNote("d1", "src/a.rs", 9, 3),
    { id: "r1", at: 4, comment: "보고 의견", source: { kind: "report", report: "r-4001", anchor: anchor("첫째", 5) } },
  ];

  it("보고 글이 먼저(글 차례), diff는 파일·줄 차례로 빈 줄을 두고 잇는다", () => {
    assert.deepEqual(sortNotes(notes).map((n) => n.id), ["r1", "r2", "d1", "d2"]);
    assert.equal(formatNotes("t-1222", notes), [
      "인용: t-1222 보고 r-4001\n> 첫째\n의견: 보고 의견",
      "인용: t-1222 보고 r-4001\n> 둘째",
      "인용: t-1222 diff origin/main 대비 abcdef01 · src/a.rs 새 9줄\n```diff\n+src/a.rs:9\n```\n의견: d1 의견",
      "인용: t-1222 diff origin/main 대비 abcdef01 · src/b.rs 새 3줄\n```diff\n+src/b.rs:3\n```\n의견: d2 의견",
    ].join("\n\n"));
  });

  it("thread마다 저장하고 다시 읽으며, 깨진 항목은 버린다", () => {
    const store = new Map();
    const storage = { getItem: (k) => store.get(k) ?? null, setItem: (k, v) => store.set(k, v), removeItem: (k) => store.delete(k) };
    assert.equal(writeNotes("p", "t-1", notes, storage), true);
    store.set("aaa:review-notes:p/t-2", JSON.stringify([{ id: "x" }, notes[0]]));
    assert.deepEqual(readNotes("p", "t-1", storage), notes);
    assert.deepEqual(readNotes("p", "t-2", storage), [notes[0]]);
    assert.equal(readNotes("p", "t-1", storage), readNotes("p", "t-1", storage), "바뀌지 않은 목록은 같은 객체(useSyncExternalStore)");
    writeNotes("p", "t-1", [], storage);
    assert.equal(store.has("aaa:review-notes:p/t-1"), false);
  });
});

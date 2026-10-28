// 고른 글 인용의 글 꼴(src/selectionQuote.ts, t-1204): 출처 머리, 터미널 울타리·대화 `>` 줄, 쓰던 초안 뒤에 붙이기.
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { appendQuote, chatQuoteSource, formatChatQuote, formatTerminalQuote, terminalQuoteSource } from "../src/selectionQuote.ts";

describe("고른 글 인용", () => {
  it("터미널 글은 출처·줄과 글 안 backtick이 닫지 못하는 울타리로 싼다", () => {
    assert.equal(terminalQuoteSource("t-1204", { start: 11, end: 14 }), "t-1204 터미널 12~15줄");
    assert.equal(terminalQuoteSource("t-1204", { start: 3, end: 3 }), "t-1204 터미널 4줄");
    assert.equal(terminalQuoteSource("  ", null), "pane 터미널");
    assert.equal(formatTerminalQuote({ source: "mac-a build", range: { start: 0, end: 1 }, text: "\r\n$ ls\r\n```x```\r\n\r\n" }),
      "인용: mac-a build 터미널 1~2줄\n````\n$ ls\n```x```\n````");
    assert.equal(formatTerminalQuote({ source: "a", range: null, text: "\n \n" }), null);
  });

  it("대화 글은 화자를 밝히고 줄마다 > 를 붙인다", () => {
    assert.equal(chatQuoteSource("t-1204", "assistant", "codex"), "t-1204 대화 · Codex 답");
    assert.equal(chatQuoteSource("coordinator", "user", "claude"), "coordinator 대화 · 사람 글");
    assert.equal(chatQuoteSource("t-1204", "mixed", "claude"), "t-1204 대화");
    assert.equal(formatChatQuote({ source: "t-1204", speaker: "assistant", harness: "claude", text: "첫 줄  \n\n둘째 줄\n" }),
      "인용: t-1204 대화 · Claude 답\n> 첫 줄\n>\n> 둘째 줄");
  });

  it("쓰던 초안은 그대로 두고 빈 줄 뒤에 붙이며, 넘치면 붙이지 않는다", () => {
    assert.equal(appendQuote("", "B", 100), "B\n");
    assert.equal(appendQuote("요청:", "B", 100), "요청:\n\nB\n");
    assert.equal(appendQuote("요청:\n", "B", 100), "요청:\n\nB\n");
    assert.equal(appendQuote("A\n\n", "B", 100), "A\n\nB\n");
    assert.equal(appendQuote("1234", "56", 7), null);
  });
});

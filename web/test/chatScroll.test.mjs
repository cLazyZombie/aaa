// 대화 굴림의 끝 판정·기준 줄·자리 기억(src/chatScroll.ts, plan §8.262·§8.266). t3code `MessagesTimeline.logic.ts` resolveTimelineIsAtEnd,
// `scrollAnchor.ts` resolveWorkGroupScrollAnchor, `timelineScrollAnchoring.ts` rememberTimelinePosition을 aaa 기록으로.
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { chatAnchorTop, chatIsAtEnd, chatRowIds, chatScrollAnchor, readChatPosition, rememberChatPosition } from "../src/chatScroll.ts";

const msg = (role, text) => ({ role, text });

describe("끝 판정(참고 40px)", () => {
  it("남은 거리가 40px 이하면 끝", () => {
    assert.equal(chatIsAtEnd({ scrollHeight: 1000, clientHeight: 400, scrollTop: 560 }), true);
    assert.equal(chatIsAtEnd({ scrollHeight: 1000, clientHeight: 400, scrollTop: 559 }), false);
  });
});

describe("줄 id", () => {
  it("앞이 잘려도 남은 줄의 id는 그대로다", () => {
    const all = [msg("user", "a"), msg("tool", "Read"), msg("assistant", "b"), msg("tool", "Read")];
    assert.deepEqual(chatRowIds(all.slice(1)), chatRowIds(all).slice(1));
  });
  it("같은 글의 줄도 서로 다르고, 말한 이가 다르면 다르다", () => {
    const ids = chatRowIds([msg("tool", "Read"), msg("tool", "Read"), msg("user", "Read")]);
    assert.equal(new Set(ids).size, 3);
  });
});

describe("기준 줄(참고 resolveWorkGroupScrollAnchor)", () => {
  const ids = ["a", "b", "c"];
  const tops = [10, 110, 400];
  it("굴림 위치에 걸친 줄과 그 줄 안 거리", () => {
    assert.deepEqual(chatScrollAnchor(ids, tops, 150), { rowId: "b", offsetWithinRow: 40, scrollOffset: 150 });
    assert.deepEqual(chatScrollAnchor(ids, tops, 0), { rowId: "a", offsetWithinRow: 0, scrollOffset: 0 });
    assert.deepEqual(chatScrollAnchor(ids, tops, 900), { rowId: "c", offsetWithinRow: 500, scrollOffset: 900 });
  });
  it("줄이 없거나 잰 수가 다르면 없다", () => {
    assert.equal(chatScrollAnchor([], [], 0), undefined);
    assert.equal(chatScrollAnchor(ids, [0, 1], 0), undefined);
  });
});

describe("자리 기억(참고 rememberTimelinePosition)", () => {
  const at = { rowId: "r", offsetWithinRow: 3, scrollOffset: 50, atEnd: false };
  it("따라가는 중이면 끝으로 기억한다", () => {
    rememberChatPosition("p-follow", at, true);
    assert.equal(readChatPosition("p-follow").atEnd, true);
    rememberChatPosition("p-free", at, false);
    assert.deepEqual(readChatPosition("p-free"), at);
  });
  it("최근 100개만 둔다(가장 오래 안 쓴 것부터 버린다)", () => {
    for (let i = 0; i < 101; i++) rememberChatPosition(`lru-${i}`, at, false);
    assert.equal(readChatPosition("lru-0"), undefined);
    assert.notEqual(readChatPosition("lru-100"), undefined);
  });
});

describe("안내 높이와 맨 위", () => {
  const at = { offsetWithinRow: -48, scrollOffset: 0 };
  it("안내가 1~2px 커져도 맨 위는 정확히 0", () => {
    assert.equal(chatAnchorTop(49, true, at), 0);
    assert.equal(chatAnchorTop(50, true, at), 0);
  });
  it("앞 페이지가 붙거나 중간을 읽던 자리는 줄을 따라간다", () => {
    assert.equal(chatAnchorTop(1000, false, at), 952);
    assert.equal(chatAnchorTop(1000, true, { offsetWithinRow: 12, scrollOffset: 1012 }), 1012);
  });
});

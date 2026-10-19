// 대화 굴림의 줄 id·기준 줄(src/chatScroll.ts, plan §8.262). t3code `scrollAnchor.ts` resolveWorkGroupScrollAnchor를 aaa 기록으로.
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { chatRowIds, chatScrollAnchor } from "../src/chatScroll.ts";

const msg = (role, text) => ({ role, text });

describe("줄 id", () => {
  it("앞이 잘려도 남은 줄의 id는 그대로다(agent가 끝 150개로 자른다)", () => {
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

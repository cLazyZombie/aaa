// 입력칸 컨텍스트 게이지 규칙(src/contextUsage.ts, plan §8.319): 사용량 %, 넘거나 창을 모르면 k, 80%부터 위험, 치는 중 숨김.
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { compactionText, contextView, formatK, hiddenWhileTyping } from "../src/contextUsage.ts";

const usage = (used, window, percent) => ({ used, window, percent, compaction: null });

describe("컨텍스트 게이지 글(사용자 「%, over하면 실제 k」)", () => {
  it("창을 알고 넘지 않으면 agent가 낸 %", () => {
    assert.deepEqual(contextView(usage(76_000, 200_000, 38)), { text: "38%", level: "normal", fill: 38, title: "컨텍스트 38% 사용 · 76k / 200k" });
    assert.equal(contextView(usage(160_000, 200_000, 80)).level, "high");
    assert.equal(contextView(usage(159_000, 200_000, 79)).level, "normal");
  });
  it("창을 넘으면 실제 토큰 k, 링은 가득", () => {
    assert.deepEqual(contextView(usage(230_000, 200_000, null)), { text: "230k", level: "over", fill: 100, title: "컨텍스트 230k 사용 · 창 200k를 넘음" });
  });
  it("창을 모르면 k, 링은 비움", () => {
    assert.deepEqual(contextView(usage(84_000, null, null)), { text: "84k", level: "unknown", fill: 0, title: "컨텍스트 84k 사용 · 이 모델의 창 크기를 모른다" });
  });
  it("k는 1,000k를 넘어도 k(1050k)이고 0이 아닌 값은 1k 이상", () => {
    assert.equal(formatK(1_050_000), "1050k");
    assert.equal(formatK(400), "1k");
    assert.equal(formatK(0), "0k");
    assert.equal(compactionText(163_600, 28_400), "164k → 28k");
  });
});

describe("치는 중 숨김(사용자 「타이핑시에는 보이지 않게」)", () => {
  it("보통·창 모름은 치는 동안 숨고, 80% 이상·넘침은 보인다", () => {
    assert.equal(hiddenWhileTyping("normal", true), true);
    assert.equal(hiddenWhileTyping("unknown", true), true);
    assert.equal(hiddenWhileTyping("high", true), false);
    assert.equal(hiddenWhileTyping("over", true), false);
    for (const level of ["normal", "high", "over", "unknown"]) assert.equal(hiddenWhileTyping(level, false), false);
  });
});

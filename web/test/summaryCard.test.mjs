// thread 작업 요약 카드(src/summaryCard.ts, plan §8.259): 카드는 6줄 안팎(목표·지금 3줄·최근 바뀐 것·상태), 자리는 줄 오른쪽에서
// 모자라면 왼쪽·위로, 단추 점은 요약 뒤 지시가 있을 때.
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { CARD_NOW, cardLines, decidedLines, hoverPlace, modelName, popoverPlace, summaryActivity, summaryModel, summaryStale } from "../src/summaryCard.ts";

const entry = (text, extra = {}) => ({ who: null, at: null, text, dropped: false, ...extra });
const view = (over = {}) => ({
  project: "demo", thread: "t-0001", text: "# 목표\n...", goal: "웹 패널 다듬기", origin: null,
  now: ["하나", "둘", "셋", "넷"], decided: [], changed: [], done: [], decisions: [],
  at_ms: 1, by: "coordinator", follow_ups: 3, pending: 0, pending_lines: [], task: null, ...over,
});

describe("작업 요약 카드", () => {
  it("시작 profile을 현재 모델로 추측하지 않고 관측한 모델과 effort만 함께 보인다", () => {
    const t = { profile: "sol-high", agent: null };
    assert.equal(summaryModel(t).text, "현재 모델 미확인 · 시작 profile: sol-high");
    const observed = { model: "gpt-6-astra", effort: "high", at_ms: 1000 };
    assert.equal(summaryModel({ ...t, agent: { model_observation: observed } }).text, "최근 확인 모델: astra high");
    assert.equal(summaryModel({ ...t, agent: { model_observation: { ...observed, effort: null } } }).text, "최근 확인 모델: astra 생각 수준 미확인");
    assert.equal(summaryModel({ profile: "astra-high", agent: {} }).text, "현재 모델 미확인 · 시작 profile: astra-high");
    assert.equal(modelName("gpt-6.1-sol"), "sol");
    assert.equal(modelName("gpt-6-sol"), "sol");
    assert.equal(modelName("claude-opus-5-5"), "opus 5.5");
    assert.equal(modelName("custom-model"), "custom-model");
  });
  it("활동 글·진행률·시작 후 시간·활동 시간을 각각 표시하고 옛 host의 시작을 추측하지 않는다", () => {
    const t = { status: "open", created_at_ms: 1000, updated_at_ms: 2000, agent: { activity: "working", progress: "코드 확인", percent: 25, progress_at_ms: 65000, progress_since_ms: 61000 } };
    assert.deepEqual(summaryActivity(t, 126000), { activity: "지금 활동: 코드 확인 · ~25%", time: "시작 후 2분 · 활동 1분" });
    assert.equal(summaryActivity({ ...t, agent: { ...t.agent, percent: null } }, 126000).activity, "지금 활동: 코드 확인 · 진행률 미정");
    assert.equal(summaryActivity({ ...t, agent: { ...t.agent, progress_since_ms: undefined } }, 126000).time, "시작 후 2분 · 활동 시간 없음");
    assert.deepEqual(summaryActivity({ ...t, agent: null }, 0), { activity: "지금 활동: 아직 보고 없음", time: "시작 후 0초 · 활동 시간 없음" });
    assert.equal(summaryActivity({ ...t, status: "resolved", updated_at_ms: 126000 }, 999000).time, "시작 후 2분 · 활동 4초");
  });
  it("지금은 3줄까지, 남은 수를 센다", () => {
    const c = cardLines(view(), "제목");
    assert.equal(CARD_NOW, 3);
    assert.deepEqual(c.now, ["하나", "둘", "셋"]);
    assert.equal(c.moreNow, 1);
    assert.equal(c.goal, "웹 패널 다듬기");
    assert.equal(c.auto, false);
  });

  it("최근 바뀐 것은 맨 아래의 그어지지 않은 줄", () => {
    const changed = [entry("앞"), entry("최근"), entry("무효", { dropped: true })];
    assert.equal(cardLines(view({ changed }), "t").latest.text, "최근");
    assert.equal(cardLines(view({ changed: [entry("x", { dropped: true })] }), "t").latest, null);
  });

  it("요약이 없으면 할 일 첫 줄, 그것도 없으면 제목", () => {
    const none = { text: null, goal: null, now: [] };
    assert.deepEqual([cardLines(view({ ...none, task: "할 일" }), "제목").goal, cardLines(view(none), "제목").goal], ["할 일", "제목"]);
    assert.equal(cardLines(view(none), "제목").auto, true);
  });

  it("정한 것 뒤에 agent Decisions를 붙이되 같은 글은 한 번", () => {
    const v = view({ decided: [entry("8px", { who: "사용자" })], decisions: ["8px", "host 한 곳"] });
    assert.deepEqual(decidedLines(v).map((e) => [e.who, e.text]), [["사용자", "8px"], ["agent", "host 한 곳"]]);
  });

  it("단추 점은 요약 뒤 지시가 있을 때만", () => {
    assert.equal(summaryStale(undefined), false);
    assert.equal(summaryStale({ at_ms: 1, pending: 0 }), false);
    assert.equal(summaryStale({ at_ms: null, pending: 2 }), true);
  });
});

describe("카드 자리", () => {
  const view = { width: 1280, height: 800 };
  const card = { width: 320, height: 160 };
  it("줄 오른쪽, 위 끝 맞춤", () => {
    assert.deepEqual(hoverPlace({ top: 100, left: 80, right: 500, bottom: 140 }, card, view), { top: 100, left: 504 });
  });
  it("오른쪽이 모자라면 줄 왼쪽, 아래가 모자라면 위로", () => {
    const p = hoverPlace({ top: 760, left: 1000, right: 1200, bottom: 790 }, card, view);
    assert.deepEqual(p, { top: 800 - 5 - 160, left: 1000 - 4 - 320 });
  });
  it("팝오버는 단추 아래 오른쪽 끝 맞춤, 화면 안", () => {
    assert.deepEqual(popoverPlace({ top: 10, left: 1100, right: 1180, bottom: 34 }, { width: 400, height: 300 }, view), { top: 38, left: 780 });
    assert.equal(popoverPlace({ top: 10, left: 10, right: 60, bottom: 34 }, { width: 400, height: 300 }, view).left, 5);
  });
});

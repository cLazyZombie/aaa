// thread 줄 상태 말 색의 차례와 작업 시간(src/sidebarModel.ts stateTone·workingStartedAt·formatWorkingDuration, plan §8.266).
// t3code `Sidebar.logic.test.ts`의 resolveThreadStatusPill·formatWorkingDurationLabel 시험을 aaa 상태로.
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { formatWorkingDuration, stateTone, workingStartedAt } from "../src/sidebarModel.ts";

const agent = (activity, more = {}) => ({ activity, since_ms: 1000, harness: "claude", seen: true, background_wait: false, ...more });
const thread = (group, status = "open") => ({ group, pr: null, status, agent: null, updated_at_ms: 0 });
const pane = (a) => ({ state: "running", agent: a });

describe("상태 말 색의 차례(참고 resolveThreadStatusPill)", () => {
  it("확인창이 가장 앞, 그다음 작업 중, 배경 대기", () => {
    assert.equal(stateTone(thread("working", "failed"), pane(agent("question"))), "question");
    assert.equal(stateTone(thread("waiting_on_you", "failed"), pane(agent("working"))), "working");
    assert.equal(stateTone(thread("working"), pane(agent("idle", { background_wait: true }))), "background");
  });
  it("쉬는 agent면 실패 → 확인 필요 → 검토·병합 중 → 끝나고 안 봄 → 쉼", () => {
    const idle = pane(agent("idle"));
    assert.equal(stateTone(thread("waiting_on_you", "failed"), idle), "failed");
    assert.equal(stateTone(thread("waiting_on_you"), idle), "attention");
    assert.equal(stateTone(thread("ready_for_review"), pane(agent("idle", { seen: false }))), "review");
    assert.equal(stateTone(thread("landing"), undefined), "review");
    assert.equal(stateTone(thread("idle"), pane(agent("idle", { seen: false }))), "unseen");
    assert.equal(stateTone(thread("idle"), idle), "idle");
    assert.equal(stateTone(thread("resolved"), undefined), "idle");
  });
  it("사용량 한도(참고 limited, 실패 자리)는 실패 다음이고 풀리는 때가 지나면 걷힌다", () => {
    const limited = (group, until, status = "open") => ({ ...thread(group, status), limited_until_ms: until });
    const idle = pane(agent("idle"));
    assert.equal(stateTone(limited("waiting_on_you", 5000), idle, 4999), "limited");
    assert.equal(stateTone(limited("idle", 5000), undefined, 4999), "limited");
    assert.equal(stateTone(limited("waiting_on_you", 5000), idle, 5000), "attention");
    assert.equal(stateTone(limited("idle", 5000, "failed"), idle, 0), "failed");
    // 일하는 agent는 한도에 막히지 않았다: 사용량 숫자보다 화면이 앞이다
    assert.equal(stateTone(limited("working", 5000), pane(agent("working")), 0), "working");
    assert.equal(stateTone(limited("working", 5000), pane(agent("question")), 0), "question");
  });
});

describe("작업 시간(참고 formatWorkingDurationLabel)", () => {
  it("60초 밑은 초, 60분 밑은 분, 그 위는 시간과 분", () => {
    assert.deepEqual([0, 43_000, 59_999, 60_000, 185_000, 3_599_999, 3_600_000, 3_900_000, -5, NaN].map(formatWorkingDuration),
      ["0초", "43초", "59초", "1분", "3분", "59분", "1시간 0분", "1시간 5분", "0초", "0초"]);
  });
  it("작업 중·배경 대기만 시작 시각이 있다(상태가 바뀐 때)", () => {
    assert.equal(workingStartedAt("working", pane(agent("working"))), 1000);
    assert.equal(workingStartedAt("background", pane(agent("idle", { background_wait: true }))), 1000);
    assert.equal(workingStartedAt("question", pane(agent("question"))), null);
    assert.equal(workingStartedAt("working", undefined), null);
  });
});

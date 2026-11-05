import assert from "node:assert/strict";
import test from "node:test";
import { dropletContent, inboxContent, notificationSummary } from "../src/notificationSummary.ts";

test("알림 요약은 첫 내용 줄·핵심 문구를 남기고 긴 원문과 이모지를 안전하게 줄인다", () => {
  assert.equal(notificationSummary("\n## [완료] **알림**을 [수정](library/details.md)했다\n긴 검증 기록"), "알림을 수정했다");
  assert.equal(notificationSummary("앞👨‍👩‍👧‍👦뒤끝", 4), "앞👨‍👩‍👧‍👦뒤끝");
  assert.equal(notificationSummary("앞👨‍👩‍👧‍👦뒤끝추가", 4), "앞👨‍👩‍👧‍👦뒤…");
  assert.equal(notificationSummary(" \n "), "");
  assert.equal(notificationSummary("요약 ".repeat(60)).length <= 48, true);
});

test("앱 안 알림은 종류·대상·내용을 구별하고 공개 URL이 내용을 대신하지 않는다", () => {
  const base = { kind: "done", target: "thread=demo%2Ft-0001", title: "앱 알림을 짧게 바꿨다", machine: null, agent: "codex" };
  const report = dropletContent({ ...base, topic: "reports", subject: "t-0001", libraryUrl: "https://example.com/v/long-link/" });
  assert.deepEqual(report, { topic: "reports", label: "완료 보고", summary: "t-0001 · 앱 알림을 짧게 바꿨다", full: "완료 보고 · t-0001 · 앱 알림을 짧게 바꿨다" });
  const original = "어느 아이콘을 쓸까요?\n- A 지금 아이콘\n- B 새 아이콘";
  const question = dropletContent({ ...base, kind: "blocked", topic: "asks", label: "질문", summary: original });
  assert.equal(question.summary, "어느 아이콘을 쓸까요?");
  assert.ok(question.full.includes(original), "줄인 질문의 원문은 툴팁에 남는다");
  assert.equal(dropletContent({ ...base, target: "idea=n-0001" }).topic, "ideas");
  assert.equal(dropletContent({ ...base, target: "task=d-0001" }).topic, "tasks");
  assert.equal(dropletContent(base).label, "작업 완료");
  assert.equal(dropletContent({ ...base, kind: "reset" }).label, "한도 풀림");
});

test("알림 목록은 원문 속 단어 대신 구조화된 종류·사건으로 질문·보고·할 일을 구별한다", () => {
  const item = (item_kind, event) => ({ item_kind, event, subject: "t-0001", summary: "질문·보고·할 일이란 단어가 든 내용\n긴 원문" });
  for (const [kind, event, topic, label] of [
    ["thread-state", "new report", "reports", "새 보고"],
    ["thread-state", "message not delivered", "work", "메시지 수락 확인 못함"],
    ["coordinator-check", "question card missing", "asks", "질문 카드 확인"],
    ["coordinator-check", "question card outdated", "asks", "질문 카드 갱신"],
    ["thread-state", "report needs user", "asks", "확인 필요"],
    ["thread-state", "blocked on a prompt", "asks", "입력 필요"],
    ["thread-state", "resolved", "work", "작업 종료"],
    ["task-followup", "task needs check", "tasks", "할 일 확인"],
    ["idea", "updated", "ideas", "updated"],
    ["routine", "ran", "automations", "ran"],
    ["outage", "down", "machines", "연결 끊김"],
    ["system", "unknown event", "workspace", "unknown event"],
  ]) {
    const content = inboxContent(item(kind, event));
    assert.equal(content.topic, topic);
    assert.equal(content.label, label);
    assert.equal(content.summary, "질문·보고·할 일이란 단어가 든 내용");
  }
});

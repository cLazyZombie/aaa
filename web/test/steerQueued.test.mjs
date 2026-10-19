import assert from "node:assert/strict";
import { it } from "node:test";
import { refusalText, steerAvailability, steerLabel, steerSettled, STEER_IDLE, STEER_SLOW_MS } from "../src/steerQueued.ts";

it("단추는 작업 중에만 보이고, 질문 중·다른 글을 보내는 중이면 까닭과 함께 꺼진다", () => {
  assert.deepEqual(steerAvailability("working", false), { blocked: null, hint: null });
  assert.match(steerAvailability("working", true).blocked, /전달된 뒤/);
  assert.equal(steerAvailability("working", true).hint, null, "잠깐 끄는 까닭은 줄로 깜박이지 않는다");
  assert.match(steerAvailability("blocked", false).blocked, /질문에 답하면/);
  assert.match(steerAvailability("blocked", false).hint, /질문에 답하면/);
  for (const status of ["idle", "done", "waiting", "unknown", undefined]) assert.equal(steerAvailability(status, false), "hidden", String(status));
});

it("여럿이면 개수를 단다", () => {
  assert.equal(steerLabel(1), "지금 보내기");
  assert.equal(steerLabel(3), "3개 지금 보내기");
});

it("까닭마다 다시 확인할 곳이 있으면 터미널 보기를 준다", () => {
  for (const [reason, inspect] of [["draft", true], ["no_queue", true], ["unknown_screen", true], ["failed", true], [null, true], ["question", false], ["not_working", false], ["unsupported", false]]) {
    const r = refusalText(reason, "pane이 끝났다", "Claude");
    assert.equal(r.inspect, inspect, String(reason));
    assert.ok(r.text.length > 0);
  }
  assert.match(refusalText("draft", null, "Claude").text, /터미널 입력창에 쓰던 글/);
  assert.match(refusalText("failed", "pane이 끝났다", "Codex").text, /pane이 끝났다/);
  assert.match(refusalText("no_queue", null, "Codex").text, /Codex 화면/);
});

it("보낸 뒤 대기 말이 줄면 끝나고, 10초 줄지 않으면 알린다", () => {
  const sent = { phase: "sent", at: 1_000, before: 2 };
  assert.equal(steerSettled(sent, 2, 1_000 + STEER_SLOW_MS - 1), sent);
  assert.equal(steerSettled(sent, 0, 2_000), STEER_IDLE);
  assert.equal(steerSettled(sent, 1, 2_000), STEER_IDLE);
  const slow = steerSettled(sent, 2, 1_000 + STEER_SLOW_MS);
  assert.equal(slow.phase, "failed");
  assert.equal(slow.inspect, true);
  // 보내는 중·실패는 대기 말 수로 바꾸지 않는다
  const sending = { phase: "sending", id: 1 };
  assert.equal(steerSettled(sending, 0, 99_999), sending);
});

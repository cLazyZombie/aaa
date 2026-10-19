import assert from "node:assert/strict";
import { it } from "node:test";
import { heldSend, refusalText, STEER_BUSY, steerAvailability, steerLabel, steerSettled, STEER_IDLE, STEER_SLOW_MS } from "../src/steerQueued.ts";

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

it("이 기기 보관함 「지금 보내기」: 작업 중이면 steer(지금 턴), 쉬면 보통 보내기, 못 누르는 까닭은 대기 말 단추와 같다", () => {
  const base = { working: true, asking: false, connected: true, busy: false, empty: false, harness: "claude" };
  const working = heldSend(base);
  assert.deepEqual([working.steer, working.blocked, working.hint], [true, null, null]);
  assert.match(working.title, /지금 턴에 넣습니다.*Claude가 하던 일을 끊을 수/);
  assert.match(heldSend({ ...base, harness: "codex" }).title, /Codex는 다음 도구 호출 뒤/);
  const idle = heldSend({ ...base, working: false });
  assert.deepEqual([idle.steer, idle.blocked], [false, null]);
  // 질문 중: 글이 답으로 들어가지 않게 끄고 아래에 안내한다(쉬는 상태로 보여도)
  const asking = heldSend({ ...base, working: false, asking: true, busy: true });
  assert.equal(asking.blocked, asking.hint);
  assert.match(asking.blocked, /질문에 답한 뒤/);
  assert.equal(heldSend({ ...base, busy: true }).blocked, STEER_BUSY, "대기 말 단추와 같은 말");
  assert.equal(heldSend({ ...base, busy: true }).hint, null, "잠깐 끄는 까닭은 줄로 깜박이지 않는다");
  assert.match(heldSend({ ...base, connected: false, busy: true }).blocked, /연결되면/);
  assert.match(heldSend({ ...base, empty: true }).blocked, /빈 메시지/);
  assert.equal(heldSend({ ...base, empty: true }).title, heldSend({ ...base, empty: true }).blocked);
});

import assert from "node:assert/strict";
import { aaa, until, waitOutput, shot } from "./harness.mjs";

// 실제 보고 수집·카드·답 전달을 잇는다. index를 원문의 글자로 오인한 두 사건의 회귀다.
export async function questionLettersScenario(page, t, write, label) {
  const card = page.getByTestId(`thread-ask-${t.id}`);
  const source = `[결정 필요] 질문 1개: 글자 ${label}\n## Questions\n### 1. 글자 ${label}\n| 안 | 뜻 |\n| --- | --- |\n| B | 새 안 |\n| A | 원래 |\n\n- B 새 안 (추천)\n- A 원래\n- C 보류\n`;
  const first = await write(source);
  const id = first.questions.items[0].id;
  assert.equal(first.questions.format_warning, undefined);
  assert.deepEqual(first.questions.items[0].choices.map(c => c.letter), ["B", "A", "C"]);
  await page.evaluate(id => { location.hash = `question=${id}`; }, id);
  await card.getByTestId("thread-ask-pick-1B").waitFor();
  assert.deepEqual(await card.locator(".prompt-card-number [aria-hidden]").allTextContents(), ["B", "A", "C"]);
  assert.match(await card.getByTestId("thread-ask-pick-1B").textContent(), /새 안/);
  await shot(page, `53-source-letters-${label}`);
  await card.getByTestId("thread-ask-pick-1B").click();
  await card.getByTestId("thread-ask-send").click();
  await waitOutput(t.pane, `1B. ${id} 글자 ${label} — 새 안`);
  await until("원문 B 답 보존", () => aaa("thread", "show", t.id).questions.answer?.summary === "1B");
  const records = () => page.evaluate(async () => {
    const response = await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ op: "asks" }) });
    return (await response.json()).ok;
  });
  // question_answer는 전송 전 claim이다. 실제 답 원문 보존이 끝난 사건 뒤 개수를 잰다.
  const before = await until("전달 완료 답 원문 기록", async () => {
    const done = (await records()).done.filter(r => r.subject === t.id);
    return done.some(r => r.items.some(q => q.id === id) && r.answer?.summary === "1B") && done.length;
  });
  const repeated = await write(source + "\n## Report\n질문 밖의 진행 글만 바뀜\n");
  assert.equal(repeated.questions.items[0].id, id);
  assert.equal(repeated.questions.answer.summary, "1B");
  await waitOutput(t.pane, `${id}: 같은 질문의 답과 ID를 유지했다.`);
  assert.equal((await records()).done.filter(r => r.subject === t.id).length, before);
  assert.equal(aaa("question", "list").questions.some(q => q.id === id), false);
  await page.getByTestId("ask-done-card").waitFor();
  assert.match(await page.getByTestId("ask-done-picked").textContent(), /B.*새 안/s);

  const second = `### 2. 새 색 ${label}\n- B 회색 (추천)\n- A 파랑\n- C 없음\n`;
  const mixed = await write(source.replace("질문 1개", "질문 2개") + second);
  assert.equal(mixed.questions.items.length, 1);
  assert.equal(mixed.questions.items[0].source_number, 2);
  await page.evaluate(id => { location.hash = `question=${id}`; }, mixed.questions.items[0].id);
  await card.getByTestId("thread-ask-pick-2A").waitFor();
  assert.equal(await card.getByTestId("thread-ask-pick-1B").count(), 0);
  aaa("thread", "answer", t.id, "2A");
  await waitOutput(t.pane, "사용자 답(2A)");
  assert.equal(aaa("thread", "show", t.id).questions.answer.picks[0], 1);

  const byId = await write(source.replaceAll(`글자 ${label}`, `개별 ${label}`));
  aaa("question", "answer", byId.questions.items[0].id, "B");
  await waitOutput(t.pane, `1B. ${byId.questions.items[0].id} 개별 ${label} — 새 안`);
  const broken = await write(source.replace("- C 보류", "- B 중복"));
  assert.equal(broken.questions.items[0].choices.length, 0);
  await page.evaluate(id => { location.hash = `question=${id}`; }, broken.questions.items[0].id);
  await card.getByTestId("question-format-warning").waitFor();
  assert.match(await card.getByTestId("question-format-warning").textContent(), /형식 오류/);
  await waitOutput(t.pane, `보고 판: ${broken.report_hash.slice(0, 12)}`);
  assert.equal(await card.getByRole("radio").count(), 0);
}

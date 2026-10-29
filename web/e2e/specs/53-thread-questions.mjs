// 실제 thread 답·원문 보존과 새 보고/늦은 응답 경합을 검사한다.
import fs from "node:fs";
import { fileURLToPath } from "node:url";
import { createHash } from "node:crypto";
import { root, aaa, until, step, browser, webkit, waitOutput, shot } from "../lib/harness.mjs";
import { questionReplyScenario } from "../lib/question-reply.mjs";

const pictures = (p, sel) =>
  p.evaluate((sel) => {
    const imgs = [...document.querySelectorAll(`${sel} img`)];
    return imgs.length > 0 && imgs.every((i) => i.complete && i.naturalWidth > 0) ? imgs.length : 0;
  }, sel);

export default async function ({ page }) {
  let wk;

  await step("늦은 답 응답: 새 보고의 직접 답·카드·보내기 활성을 보존하고 각 보고에 한 번씩 전달한다", async () => {
    wk = await webkit.launch();
    try {
      await questionReplyScenario(browser, 1280);
      await questionReplyScenario(wk, 390);
    } finally { await wk.close(); wk = null; }
  });

  await step("t-0621 원문: 선택지 뒤 E·F 설명·그림과 A~D 선택지 그림을 보존하고 두 질문·4/3개 선택지를 답한다", async () => {
    if (!aaa("project", "list").projects.some((p) => p.name === "demo")) aaa("project", "create", "demo");
    fs.writeFileSync(`${root}/task-icon-notes.md`, "선택지 뒤 설명 회귀 시험\n");
    const t = aaa("thread", "start", "demo", "--title", "단추 아이콘 원문", "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/task-icon-notes.md`);
    await until("첫 아이콘 보고", () => aaa("thread", "show", t.id).report_hash);
    const original = fs.readFileSync(fileURLToPath(new URL("../../../crates/aaa-proto/tests/fixtures/t0621-questions.md", import.meta.url)), "utf8");
    for (const name of new Set([...original.matchAll(/library\/([a-z-]+\.png)/g)].map((m) => m[1]))) {
      // 실제 보고의 글은 그대로, 그림 bytes는 독립 실험실의 fixture다.
      fs.copyFileSync(`${root}/pic.png`, `${t.work_dir}/library/${name}`);
    }
    const body = "[결정 필요] 질문 2개: 단추 아이콘 원문\n\n" + original;
    fs.writeFileSync(`${t.work_dir}/report.md`, body);
    const hash = createHash("sha256").update(body).digest("hex");
    const v = await until("아이콘 원문 두 질문", () => { const v = aaa("thread", "show", t.id); return v.report_hash === hash && v.questions && v; });
    if (v.questions.format_warning || JSON.stringify(v.questions.items.map((q) => q.choices.length)) !== "[4,3]" || !v.questions.items[0].after?.includes("E·F안 그림:")) throw new Error(`아이콘 원문: ${JSON.stringify(v.questions)}`);
    await page.evaluate((id) => { location.hash = `question=${id}`; }, t.id);
    const card = page.getByTestId(`thread-ask-${t.id}`);
    await card.getByTestId("thread-ask-pick-1D").waitFor();
    await until("선택지와 E/F 그림", async () => (await pictures(page, `[data-testid="thread-ask-${t.id}"] .ask-choice-picture`)) === 7 && (await pictures(page, `[data-testid="thread-ask-${t.id}"] .ask-after`)) === 2);
    if ((await card.locator('[role="radiogroup"]').count()) !== 2 || (await card.locator('[role="radio"]').count()) !== 7 || (await card.getByTestId("thread-ask-pick-1E").count()) !== 0) throw new Error("E/F를 선택지로 위조하거나 질문을 빠뜨렸다");
    await shot(page, "53-t0621-icon-question");
    await card.locator(".ask-after").scrollIntoViewIfNeeded();
    await shot(page, "53-t0621-after-options");
    await card.getByTestId("thread-ask-pick-2B").click();
    await card.getByTestId("thread-ask-text-1").fill("E: TowerControl\n원래 픽셀 밀도 유지");
    await card.getByTestId("thread-ask-text-2").fill("선택한 B에 색만 더한다");
    await card.getByTestId("thread-ask-send").click();
    await waitOutput(t.pane, "1번 직접 쓴 답: E: TowerControl");
    await waitOutput(t.pane, "2번 직접 쓴 답(의견): 선택한 B에 색만 더한다");
    await until("답한 아이콘 카드", async () => (await page.getByTestId("ask-done-card").count()) === 1 && (await pictures(page, '[data-testid="ask-done-card"] .ask-after')) === 2);
    const answered = aaa("thread", "show", t.id).questions.answer;
    if (answered.picks[0] !== null || answered.picks[1] !== 1 || answered.text !== null
      || JSON.stringify(answered.comments) !== JSON.stringify(["E: TowerControl\n원래 픽셀 밀도 유지", "선택한 B에 색만 더한다"])) throw new Error(`E 직접 답: ${JSON.stringify(answered)}`);
    if (!(await page.getByTestId("ask-done-text-1").innerText()).includes("원래 픽셀 밀도 유지")
      || !(await page.getByTestId("ask-done-text-2").innerText()).includes("선택한 B에 색만 더한다")) throw new Error("질문별 직접 답이 답한 카드에 없다");
    const followups = fs.readFileSync(`${root}/projects/demo/threads/${t.id}.task.md`, "utf8");
    if (!followups.includes("1번 직접 쓴 답: E: TowerControl\n원래 픽셀 밀도 유지")
      || !followups.includes("2번 직접 쓴 답(의견): 선택한 B에 색만 더한다")) throw new Error("질문별 원문이 Follow-ups에 없다");
    const notice = aaa("inbox", "list", "demo", "--all", "--peek").items.find(i => i.event === "question answered" && i.subject === t.id);
    if (!notice?.summary.includes("1직접 2B+의견") || !notice.summary.includes("1 직접: E: TowerControl")
      || !notice.summary.includes("2B + 의견: 선택한 B에 색만 더한다") || !notice.body.includes("원래 픽셀 밀도 유지")
      || !notice.body.includes("2번 직접 쓴 답(의견): 선택한 B에 색만 더한다")) throw new Error(`질문별 inbox 원문: ${JSON.stringify(notice)}`);
    await shot(page, "53-t0621-answered-notes");
    aaa("thread", "resolve", t.id);
  });

}

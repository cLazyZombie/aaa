// 짧은 ID: 실제 host 목록으로 대화·보고서 자동 링크, 할 일 깊은 주소·삭제·보관·휴대폰·Reader를 확인한다.
import assert from "node:assert/strict";
import fs from "node:fs";
import { createHash } from "node:crypto";
import { root, AA, aaa, step, until, domIs, waitOutput, coordinatorPost, browser, webkit, pairPage, shot } from "../lib/harness.mjs";
import { pairDevice } from "../lib/devices.mjs";

const go = (p, hash) => p.evaluate((hash) => { location.hash = hash; }, hash);
const link = (p, id) => p.locator(`[data-testid="app-id"][data-target="${id}"]`);
const picked = async (p, id) => {
  try {
    await domIs(p, "주소의 할 일 상세와 강조", (id) => document.querySelector('[data-testid="tasks-view"]')?.dataset.shown === "true"
      && document.querySelector('.task-row.is-selected')?.dataset.shortId === id
      && document.querySelector('[data-testid="task-detail"] .task-short-id')?.textContent === id, id);
  } catch (error) {
    throw new Error(`${error.message}: ${JSON.stringify(await p.evaluate(() => ({ hash: location.hash, tasks: document.querySelector('[data-testid="tasks-view"]')?.dataset.shown,
      row: document.querySelector('.task-row.is-selected')?.dataset.shortId, detail: document.querySelector('[data-testid="task-detail"] .task-short-id')?.textContent, body: document.querySelector("main")?.innerText.slice(0, 1800) })))}`);
  }
};
let coordinatorPane;
const chat = async (p, pane = coordinatorPane) => {
  await go(p, `pane=${pane}`);
  if (await p.getByTestId("chat").count() === 0) { await p.getByTestId("chat-toggle").waitFor(); await p.getByTestId("chat-toggle").click(); }
  await p.getByTestId("chat").waitFor();
};

export default async function ({ page }) {
  let t, d, other, q, threadQ, r, text, reportHash;
  await step("준비: 실제 thread·할 일·coordinator 질문·보고서 번호와 대화 원문", async () => {
    aaa("project", "create", "demo");
    aaa("project", "set", "demo", "repos.add", "/srv/rc@labA");
    fs.writeFileSync(`${root}/projects/demo/TASKS.md`, "# Tasks\n\n## Backlog\n- [ ] 첫 할 일 [작업 폴더]\n- [ ] 링크 대상 [rc]\n  상세 메모\n");
    [other, d] = aaa("task", "list").tasks.map(t => t.short_id);
    fs.writeFileSync(`${root}/task-ids.md`, "질문 카드 시험\n");
    t = aaa("thread", "start", "demo", "--title", "ID 참조", "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/task-ids.md`);
    await until("thread 보고 회수", () => aaa("thread", "show", t.id).questions);
    await coordinatorPost({ request: true, title: "ID 질문", body: "## Questions\n### 1. 진행 방향\n- A 진행 (추천) — 지금 범위\n- B 보류 — 다음에\n" });
    // 질문 목록의 원본 필드 대신 host가 보낸 요청 카드의 영속 번호를 쓴다.
    q = await page.evaluate(async () => {
      const r = await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ op: "asks" }) });
      return (await r.json()).ok.requests.find(r => r.title === "ID 질문").question_id;
    });
    assert.match(q, /^q-\d{4,}$/);
    const input = `${root}/id-report`; fs.mkdirSync(input);
    fs.writeFileSync(`${input}/index.html`, "<!doctype html><meta charset=utf-8><h1>번호로 연 보고서</h1>");
    // 보고서 이름이 없는 할 일 ID와 같아도 그 할 일을 만들지는 않는다.
    r = aaa("library", "publish", input, "--name", "d-9999").report_id;
    text = `참조 ${t.id}과 ${d}, ${q}, ${r}.\n\n인라인 \`${d}\`\n\n없는 번호 t-9999 d-9999 q-9999 r-9999\n\n\`\`\`text\n${t.id} ${d} ${q} ${r}\n\`\`\`\n\nhttps://example.ts.net/${d} [${t.id}](https://example.ts.net/) 경로 demo/${d} ${d}.md`;
    const report = fs.readFileSync(`${t.work_dir}/report.md`, "utf8") + "\n## 참조 확인\n" + text;
    fs.writeFileSync(`${t.work_dir}/report.md`, report);
    reportHash = createHash("sha256").update(report).digest("hex");
    await until("링크 본문 회수", () => aaa("thread", "show", t.id).report_hash === reportHash);
    threadQ = aaa("thread", "show", t.id).questions.items[0].id;
    const chatText = text + `\n\nthread의 질문 ${threadQ}`;
    coordinatorPane = aaa("project", "open", "demo", "--profile", "fake").pane.id;
    await waitOutput(coordinatorPane, "fake>");
    for (const [pane, tag, session] of [[coordinatorPane, "coordinator", "6a1f0c2e-3b4d-4e5f-8a9b-0c1d2e3f4a77"], [t.pane, "thread", "6a1f0c2e-3b4d-4e5f-8a9b-0c1d2e3f4a78"]]) {
      const dir = `${root}/claude/projects/-lab-ids-${tag}`; fs.mkdirSync(dir, { recursive: true });
      const tr = `${dir}/s.jsonl`;
      fs.writeFileSync(tr, JSON.stringify({ type: "assistant", message: { content: [{ type: "text", text: chatText }] } }) + "\n");
      const hook = JSON.stringify({ hook_event_name: "SessionStart", session_id: session, source: "startup", transcript_path: tr });
      fs.writeFileSync(`${dir}/hook.sh`, `'${AA}' hook --agent claude '${hook}'\nprintf 'id-hooked-${tag}\\n'\n`);
      aaa("pane", "run", pane, `post-probe ${dir}/hook.sh`);
      await waitOutput(pane, `id-hooked-${tag}`);
    }
    await page.reload();
  });

  await step("할 일 깊은 주소: 거르기를 풀고 행·상세 표시, 새로고침·프로젝트 주소·다른 프로젝트·없는 번호", async () => {
    await go(page, "tasks");
    await page.getByTestId("tasks-repo").click();
    await page.getByRole("menuitemcheckbox", { name: /^작업 폴더/ }).click();
    await page.getByTestId("tasks-filter-mine").click();
    await page.getByTestId("tasks-search").fill("일치하지 않는 검색");
    await go(page, `task=${d}`); await picked(page, d);
    assert.equal(await page.getByTestId("tasks-search").inputValue(), "");
    assert.equal(await page.getByTestId("tasks-repo").getAttribute("data-repo"), "");
    assert.equal(await page.getByTestId("tasks-filter-all").getAttribute("aria-pressed"), "true");
    await page.reload(); await picked(page, d);
    await go(page, `task=demo/${d}`); await picked(page, d);
    for (const target of [`other/${d}`, "d-9999"]) {
      await go(page, `task=${target}`);
      await page.getByTestId("task-missing").waitFor();
      assert.match(await page.getByTestId("task-missing").textContent(), /없음/);
      assert.equal(await page.getByTestId("task-detail").count(), 0);
    }
    await go(page, `task=${d}`); await picked(page, d);
    await shot(page, "77-task-deep-link");
  });

  await step("대화의 네 ID는 짧은 글 그대로 앱 안 이동, 코드·주소·없는 번호 제외, 뒤로와 가운데 누름 새 탭", async () => {
    await chat(page);
    const c = page.getByTestId("chat");
    await domIs(page, "네 ID·thread 질문·인라인 코드만 링크", () => document.querySelectorAll('[data-testid="chat"] [data-testid="app-id"]').length === 6);
    assert.equal(await c.locator('pre [data-testid="app-id"], a a, [data-testid="app-id"][data-target$="9999"]').count(), 0);
    assert.equal(await c.locator(`a[href="https://example.ts.net/${d}"]`).count(), 1);
    const origin = await page.evaluate(() => location.origin);
    assert.equal(await link(c, d).first().getAttribute("href"), `#task=${d}`);
    const popupEvent = page.context().waitForEvent("page");
    await link(c, d).first().click({ button: "middle" });
    const popup = await popupEvent;
    await popup.waitForURL((url) => url.hash === `#task=${d}`);
    await popup.waitForLoadState("domcontentloaded");
    await picked(popup, d); await popup.close();
    for (const id of [d, t.id, q, threadQ, r]) {
      await link(c, id).first().click();
      if (id === d) await picked(page, d);
      if (id === t.id) {
        try { await domIs(page, "thread로 이동", () => !!document.querySelector('[data-testid="thread-view"]')); }
        catch (error) { throw new Error(`${error.message}: ${JSON.stringify(await page.evaluate(() => ({ hash: location.hash, body: document.querySelector("main")?.innerText })))}`); }
      }
      if (id === q || id === threadQ) await page.getByTestId("thread-ask-send").waitFor();
      if (id === r) await page.frameLocator('[data-testid="report-frame"]').getByRole("heading", { name: "번호로 연 보고서" }).waitFor();
      assert.equal(await page.evaluate(() => location.origin), origin);
      await page.goBack(); await c.waitFor();
    }
    aaa("question", "answer", q, "A");
    await link(c, q).click(); await page.getByTestId("ask-done-card").waitFor();
    await page.goBack(); await c.waitFor();
    await shot(page, "77-chat-ids");
    await chat(page, t.pane);
    await domIs(page, "thread 대화도 같은 ID 링크", () => document.querySelectorAll('[data-testid="chat"] [data-testid="app-id"]').length === 6);
    await link(page.getByTestId("chat"), d).first().click(); await picked(page, d);
  });

  await step("보고서 본문의 ID도 링크, 삭제·자동 보관된 할 일은 없음과 일반 글", async () => {
    await go(page, `thread=${t.id}`);
    const body = page.locator(".report-markdown");
    await link(body, d).first().waitFor();
    assert.equal(await body.locator('[data-testid="app-id"]').count(), 5);
    await link(body, d).first().click(); await picked(page, d);
    aaa("task", "rm", d);
    await page.getByTestId("task-missing").waitFor();
    await chat(page);
    await domIs(page, "지운 번호는 글 그대로", (d) => !document.querySelector(`[data-testid="chat"] [data-testid="app-id"][data-target="${d}"]`), d);
    assert.ok((await page.getByTestId("chat").textContent()).includes(d));
    aaa("task", "restore", d);
    await domIs(page, "복원한 번호는 다시 링크", (d) => !!document.querySelector(`[data-testid="chat"] [data-testid="app-id"][data-target="${d}"]`), d);
    aaa("thread", "resolve", t.id);
    await chat(page);
    await domIs(page, "끝난 thread의 보존하지 않은 질문은 글 그대로", (id) => !document.querySelector(`[data-testid="chat"] [data-testid="app-id"][data-target="${id}"]`), threadQ);
    assert.ok((await page.getByTestId("chat").textContent()).includes(threadQ));
    fs.writeFileSync(`${root}/archive-task.md`, "완료 보관 시험");
    const completed = aaa("thread", "start", "demo", "--title", "보관할 일", "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/archive-task.md`);
    aaa("task", "edit", other, "--thread", completed.id);
    await until("보관할 완료 보고", () => aaa("thread", "show", completed.id).report_hash);
    aaa("thread", "resolve", completed.id); aaa("ticker", "tick");
    assert.ok(aaa("task", "archive").entries.some(e => e.task.short_id === other));
    await go(page, `task=${other}`); await page.getByTestId("task-missing").waitFor();
  });

  await step("WebKit 휴대폰과 Reader의 첫 주소·상세 닫기·보고서 링크·가로 넘침 없음", async () => {
    const wk = await webkit.launch();
    const phone = await pairDevice(wk, aaa("web", "pair").code, "iphone", { hash: `#task=demo/${d}` });
    const reader = await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 390, height: 844 }, { hash: `#task=${d}` });
    try {
      for (const [name, p] of [["phone", phone.page], ["reader", reader.page]]) {
        await picked(p, d);
        assert.ok(await p.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
        await shot(p, `77-task-${name}`);
        await p.getByTestId("task-detail-close").click();
        await domIs(p, "닫으면 할 일 목록", () => location.hash === "#tasks" && !document.querySelector('[data-testid="task-detail"]'));
        await go(p, `thread=${t.id}`);
        await link(p.locator(".report-markdown"), d).first().click(); await picked(p, d);
      }
      assert.equal(await reader.page.getByTestId("task-save").count(), 0);
      assert.deepEqual([...phone.errors, ...reader.errors], []);
    } finally { await phone.page.context().close(); await reader.page.context().close(); await wk.close(); }
  });
}

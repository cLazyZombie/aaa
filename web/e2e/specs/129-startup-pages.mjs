// 실제 host의 큰 목록을 늦춰 첫 화면·과거 직접 링크·복원·답변 페이지를 검사한다. 고정 sleep은 없다.
import assert from "node:assert/strict";
import fs from "node:fs";
import { aaa, step, browser, webkit, pairPage, domIs, until, root, showPanel, base, drawerSettled } from "../lib/harness.mjs";
import { seedHost } from "../lib/bulk-fixture.mjs";
import { ThreadRoutes } from "../lib/thread-routes.mjs";

export default async function () {
  await step("첫 화면 규모 자료 준비", async () => {
    aaa("project", "create", "demo");
    fs.writeFileSync(`${root}/task.md`, "첫 화면 시험\n");
    const t = aaa("thread", "start", "--title", "먼저 열 작업", "--profile", "fake", "--machine", "labA", "--kind", "dir", "--task-file", `${root}/task.md`);
    await until("첫 보고 수집", () => aaa("thread", "show", t.id).report_hash);
    await seedHost(db => {
      const original = JSON.parse(db.prepare("SELECT data FROM threads WHERE id=?").get(t.id).data);
      const put = db.prepare("INSERT INTO threads(project,id,status,data,updated_at) VALUES('demo',?,?,?,1)");
      for (let i = 2000; i < 3570; i++) {
        const status = i === 3568 ? "parked" : i === 3569 ? "failed" : "resolved";
        const record = { ...original, id: `t-${i}`, title: `이전 작업 ${i}`, status,
          pane: null, questions: [], question_answer: null, question_draft: null, report_hash: null, report_copy_hash: null, acked_hash: null,
          work_dir: null, cwd: null, launch: null, resolve_pending: null, park: null, next: [], error: status === "failed" ? "시험 실패" : null,
          task_clean: { ...original.task_clean, followups: [], followup_revisions: [] } };
        put.run(record.id, status, JSON.stringify(record));
      }
      const ask = db.prepare("INSERT INTO asks(project,id,kind,subject,done_ms,data) VALUES('demo',?,'request',?,?,?)");
      for (let i = 0; i < 250; i++) {
        const record = { id: `history-${i}`, question_id: `q-${9000+i}`, kind: "request", subject: `fixture-${i}`, title: `답변 ${i}`, asked_ms: Date.now(), done_ms: Date.now(), by: "시험", summary: "답함", items: [], body: `원문-${i} ` + "측정용 답변 본문. ".repeat(329) };
        ask.run(record.id, record.subject, record.done_ms, JSON.stringify(record));
      }
    });
  }, { always: true });
  const wk = await webkit.launch({ headless: true });
  try {
    for (const [engine, name, width] of [[browser, "chromium", 1280], [wk, "webkit", 390]]) {
      await step(`첫 표시·과거 링크·복원·답 이력 ${name}`, async () => {
        const routes = new ThreadRoutes();
        let hold = Promise.withResolvers(), context;
        const gates = [hold];
        try {
          const { page, errors } = await pairPage(engine, aaa("web", "pair").code, { width, height: 844 }, {
            serviceWorkers: "block",
            init: () => {
              window.startupReads = [];
              const fetch = window.fetch;
              window.fetch = function(input, init) {
                try { const req = JSON.parse(init?.body); if (["asks", "ask_history", "threads"].includes(req.op)) window.startupReads.push(req); } catch {}
                return fetch.call(this, input, init);
              };
            },
            setup: async ctx => {
              context = ctx;
              await ctx.route("**/api/v1/call", route => routes.run(route, async () => {
                const req = route.request().postDataJSON();
                if (req.op === "threads" && !req.open) await hold.promise;
                await route.continue();
              }));
            },
          });
          await domIs(page, "전체 목록 전 열린 작업", () => document.querySelector('.app')?.dataset.threadsReady === "1");
          assert.equal(await page.locator('.app').getAttribute('data-threads-sync'), "0");
          await page.evaluate(() => { location.hash = "thread=t-2000"; });
          await domIs(page, "지난 작업 단건 직접 열기", () => document.querySelector('.thread-view')?.textContent.includes("이전 작업 2000"));
          assert.equal(await page.locator('.app').getAttribute('data-threads-sync'), "0");
          hold.resolve();
          await domIs(page, "뒤 목록 끝", () => document.querySelector('.app')?.dataset.threadsSync === "1");
          await page.evaluate(() => { location.hash = "threads"; });
          await domIs(page, "보류·실패·끝난 작업 모두", () => ["t-3568", "t-3569", "t-2000"].every(id => document.querySelector(`[data-testid="project-thread-${id}"]`)));
          await page.getByRole('searchbox', { name: 'thread 거르기' }).fill('이전 작업 2000');
          assert.equal(await page.locator('[data-testid="all-threads"] [data-testid^="project-thread-"]').count(), 1);
          await page.evaluate(() => { location.hash = "thread=t-2000"; });
          await domIs(page, "선택 저장", () => sessionStorage.getItem("aaa:selection") === "thread:demo/t-2000");
          hold = Promise.withResolvers(); gates.push(hold);
          await page.goto(base);
          await domIs(page, "지난 선택도 먼저 복원", () => document.querySelector('.thread-view')?.textContent.includes("이전 작업 2000"));
          assert.equal(await page.locator('.app').getAttribute('data-threads-sync'), "0");
          hold.resolve();
          await domIs(page, "재방문 목록과 답변 색인", () => document.querySelector('.app')?.dataset.threadsSync === "1" && window.startupReads.filter(r => r.op === "asks").length > 0);
          assert.equal(await page.evaluate(() => window.startupReads.filter(r => r.op === "asks").length), 1, "연결 열림과 첫 사건의 중복 조회 없음");
          await page.evaluate(() => { location.hash = "question=q-9000"; });
          await domIs(page, "첫 페이지 밖 과거 답 원문", () => document.querySelector('[data-testid="ask-done-card"]')?.textContent.includes("원문-0"));
          assert(await page.evaluate(() => window.startupReads.some(r => r.op === "ask_history" && r.id === "history-0")));
          if (width < 700) {
            await page.locator('[data-testid="asks-view"] [data-testid="drawer-toggle"]').click();
            await drawerSettled(page);
          } else await showPanel(page, "asks");
          await domIs(page, "전체 답변 개수", () => document.querySelector('[data-testid="asks-done-count"]')?.textContent === "250");
          // 더 보기마다 실제 페이지 응답이 온 뒤 다음 쪽을 누른다.
          while (await page.getByTestId('asks-done-more').count()) {
            const more = page.getByTestId('asks-done-more');
            await more.click();
            await domIs(page, "이력 페이지 반영", () => !document.querySelector('[data-testid="asks-done-more"]')?.disabled);
          }
          assert.equal(await page.locator('[data-testid="ask-row"].is-done').count(), 250);
          assert.deepEqual(errors, []);
        } finally {
          for (const gate of gates) gate.resolve();
          if (context) { await routes.close(context); await context.close(); }
        }
      });
    }
  } finally { await wk.close(); }
}

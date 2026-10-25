// 9999 앞에서 실제 번호를 생성하고 CLI와 두 브라우저의 별칭 입력을 확인한다.
import assert from "node:assert/strict";
import fs from "node:fs";
import { DatabaseSync } from "node:sqlite";
import { root, AA, aaa, step, until, domIs, browser, webkit, pairPage, shot, waitOutput } from "../lib/harness.mjs";

const go = (p, hash) => p.evaluate(hash => { location.hash = hash; }, hash);
export default async function () {
  let high, task, question, report;
  await step("짧은 ID: 실제 생성기 9999→10000, CLI의 숫자 동치", async () => {
    aaa("project", "create", "demo");
    fs.writeFileSync(`${root}/ids-task.md`, "번호 경계 시험\n");
    fs.writeFileSync(`${root}/report-fixture.md`, "[결정 필요] 번호 확인\n\n## Questions\n### 1. 번호 경계\n- A 확인 (추천) — 같은 대상\n- B 보류 — 나중에\n\n## Next\n- 답을 기다린다\n");
    const start = title => aaa("thread", "start", "--title", title, "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/ids-task.md`);
    const seed = start("기준 thread");
    await until("기준 보고 회수", () => aaa("thread", "show", seed.id).questions);
    for (const id of ["t-1", "t-0001", "t-00001"]) assert.equal(aaa("thread", "show", id).id, seed.id);
    // 그 thread의 보고가 모두 회수된 뒤 원장의 마지막 번호만 준비한다.
    const db = new DatabaseSync(`${root}/s/host/host.db`);
    const original = JSON.parse(db.prepare("SELECT data FROM threads WHERE project='demo' AND id=?").get(seed.id).data);
    Object.assign(original, { id: "t-9998", status: "resolved", pane: null, work_dir: null, cwd: null, launch: null,
      questions: [], report_hash: null, report_copy_hash: null, copy: null, next: [] });
    db.prepare("INSERT INTO threads(project,id,status,data,updated_at) VALUES('demo','t-9998','resolved',?,1)").run(JSON.stringify(original));
    db.prepare("INSERT INTO task_ids VALUES('demo','boundary-seed',9998)").run();
    db.prepare("INSERT INTO automation_ids VALUES('demo','boundary-seed',9998)").run();
    db.prepare("INSERT INTO question_ids(project,seq,id,kind,subject,revision,position) VALUES('demo',9998,'q-9998','thread','boundary-seed','v',0)").run();
    db.prepare("INSERT INTO report_ids(project,seq,id,source,revision) VALUES('demo',9998,'r-9998','boundary-seed','v')").run();
    db.close();
    const previous = start("9999 thread");
    assert.equal(previous.id, "t-9999");
    await until("9999 보고 회수", () => aaa("thread", "show", previous.id).questions);
    high = start("10000 thread");
    assert.equal(high.id, "t-10000");
    const view = await until("10000 보고 회수", () => { const t = aaa("thread", "show", high.id); return t.questions && t; });
    question = view.questions.items[0].id;
    assert.equal(question, "q-10000");
    task = aaa("task", "list").tasks.find(t => t.thread === high.id).short_id;
    assert.equal(task, "d-10000");
    for (const id of ["t-10000", "t-010000", "t-0010000"]) assert.equal(aaa("thread", "show", id).id, high.id);
    aaa("task", "edit", "d-010000", "--note", "별칭으로 붙인 메모");
    assert.ok(aaa("task", "list").tasks.find(t => t.short_id === task).notes.includes("별칭으로 붙인 메모"));
    const source = `${root}/id-html`; fs.mkdirSync(source);
    fs.writeFileSync(`${source}/index.html`, "<!doctype html><meta charset=utf-8><h1>다섯 자리 보고서</h1>");
    report = aaa("library", "publish", source, "--name", "id-html").report_id;
    assert.ok(Number(report.slice(2)) > 10000);
    assert.equal(aaa("library", "show", `r-0${report.slice(2)}`).thread, report);
    fs.writeFileSync(`${root}/projects/demo/automations/id-rule.toml`, 'title = "번호 자동화"\n[on.schedule]\nevery = "1d"\n[do.inbox]\ntext = "번호 시험"\n');
    assert.equal(aaa("routine", "list").routines.find(r => r.name === "id-rule").short_id, "a-9999");
    assert.equal(aaa("automation", "show", "a-09999").doc.name, "id-rule");
    const transcriptDir = `${root}/claude/projects/-lab-short-ids`; fs.mkdirSync(transcriptDir, { recursive: true });
    const transcript = `${transcriptDir}/s.jsonl`;
    const text = `t-010000 d-0010000 q-010000 r-0${report.slice(2)} a-09999\n\n경계 xt-010000 t-010000x demo/t-010000 t-010000.md\n\n\`\`\`text\nt-010000 d-0010000\n\`\`\``;
    fs.writeFileSync(transcript, JSON.stringify({ type: "assistant", message: { content: [{ type: "text", text }] } }) + "\n");
    const hook = JSON.stringify({ hook_event_name: "SessionStart", session_id: "6a1f0c2e-3b4d-4e5f-8a9b-0c1d2e3f4112", source: "startup", transcript_path: transcript });
    fs.writeFileSync(`${transcriptDir}/hook.sh`, `'${AA}' hook --agent claude '${hook}'\nprintf 'short-id-hooked\\n'\n`);
    await waitOutput(high.pane, "fake>");
    aaa("pane", "run", high.pane, `post-probe ${transcriptDir}/hook.sh`);
    await waitOutput(high.pane, "short-id-hooked");
  });

  await step("다섯 자리 깊은 주소: Chromium·WebKit, 정본·0 추가·프로젝트 경계", async () => {
    const safari = await webkit.launch({ headless: true });
    try {
      for (const [engine, label] of [[browser, "chromium"], [safari, "webkit"]]) {
        const { page, errors } = await pairPage(engine, aaa("web", "pair").code, { width: 390, height: 844 });
        try {
          await go(page, `pane=${high.pane}`);
          if (await page.getByTestId("chat").count() === 0) await page.getByTestId("chat-toggle").click();
          await domIs(page, "별칭 링크 네 개와 질문 칩", () => document.querySelectorAll('[data-testid="chat"] [data-testid="app-id"]').length === 4
            && document.querySelectorAll('[data-testid="chat"] [data-testid="app-ref"]').length === 1);
          const chat = page.getByTestId("chat");
          assert.equal(await chat.locator('pre [data-testid="app-id"]').count(), 0);
          assert.equal(await chat.locator('[data-target="t-10000"]').innerText(), "t-010000");
          assert.equal(await chat.locator('[data-target="d-10000"]').getAttribute("href"), "#task=d-10000");
          await chat.locator('[data-target="t-10000"]').click();
          await domIs(page, "별칭 링크의 thread 본문", () => document.querySelector('[data-testid="thread-view"]')?.textContent.includes("10000 thread"));
          for (const digits of ["10000", "010000", "0010000"]) {
            await go(page, `thread=t-${digits}`);
            await domIs(page, "같은 thread 본문", () => document.querySelector('[data-testid="thread-view"]')?.textContent.includes("10000 thread"));
            await go(page, `task=demo/d-${digits}`);
            await domIs(page, "같은 할 일 상세", () => document.querySelector('[data-testid="task-detail"] .task-short-id')?.textContent === "d-10000");
            await go(page, `question=q-${digits}`);
            await page.getByTestId("thread-ask-send").waitFor();
          }
          await go(page, `report=demo/r-0${report.slice(2)}`);
          await page.frameLocator('[data-testid="report-frame"]').getByRole("heading", { name: "다섯 자리 보고서" }).waitFor();
          await page.reload();
          await page.frameLocator('[data-testid="report-frame"]').getByRole("heading", { name: "다섯 자리 보고서" }).waitFor();
          await go(page, "automation=a-09999");
          await page.getByTestId("automation-detail").waitFor();
          assert.ok((await page.getByTestId("automation-detail").innerText()).includes("번호 자동화"));
          await go(page, "task=other/d-010000");
          await page.getByTestId("task-missing").waitFor();
          await go(page, "question=other/q-010000");
          await page.getByTestId("asks-missing").waitFor();
          await shot(page, `short-ids-${label}`);
          assert.deepEqual(errors, []);
        } finally { await page.context().close(); }
      }
    } finally { await safari.close(); }
  });
}

// 확인 필요 카드를 휴대폰·데스크톱 Chromium/WebKit에서 바로 처리하고 실제 보관·예약·답 기록을 확인한다.
import assert from "node:assert/strict";
import fs from "node:fs";
import { createHash } from "node:crypto";
import { finishFixtureFollowups, aaa, step, browser, webkit, pairPage, domIs, until, root, shot } from "../lib/harness.mjs";

const call = (page, request) => page.evaluate(async request => (await fetch("/api/v1/call", {
  method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(request),
})).json(), request);

export default async function () {
  await step("할 일 확인 카드 준비", async () => {
    aaa("project", "create", "demo");
    aaa("project", "set", "demo", "thread_profile", "fake");
    fs.writeFileSync(`${root}/report-fixture.md`, "[진행 중] 카드 처리 시험\n\n## Next\n");
  }, { always: true });
  const wk = await webkit.launch({ headless: true });
  try {
    for (const [engine, name] of [[browser, "chromium"], [wk, "webkit"]]) {
      for (const width of [1280, 390]) {
        for (const action of ["complete", "reassign", "unlink"]) {
          await step(`할 일 카드 ${action} ${name} ${width}`, async () => {
            const title = `카드 ${name} ${width} ${action}`;
            fs.appendFileSync(`${root}/projects/demo/TASKS.md`, `\n- [ ] ${title} (fake@labA) [작업 폴더]\n  카드 처리 범위\n`);
            const t = aaa("thread", "start", "demo", "--from-task", title, "--link-task", "--kind", "dir");
            await until("첫 보고 수집", () => aaa("thread", "show", t.id).report_hash);
            // 보고 첫 줄이 표지가 아닌 실제 문제를 재현한다. 다른 저장소의 착지 문구도 자동 완료로 믿지 않는다.
            const report = "착지: engine main abc1234 · push함\n\n## Report\n다른 저장소 반영을 확인한다\n\n## Next\n";
            const hash = createHash("sha256").update(report).digest("hex");
            fs.writeFileSync(`${t.work_dir}/report.md`, report);
            await until("표지 없는 보고 수집", () => aaa("thread", "show", t.id).report_hash === hash);
            finishFixtureFollowups(t.id);
            aaa("thread", "resolve", t.id);
            aaa("ticker", "tick");
            const task = aaa("task", "list").tasks.find(t => t.title === title);
            assert(task.attention.includes("보고는 있지만 첫 줄에 표지가 없다"));
            assert.equal(task.attention_evidence.report_first_line, report.split("\n")[0]);
            const p = (await pairPage(engine, aaa("web", "pair").code, { width, height: 844 }, { hash: `#ask=t:${title}` })).page;
            try {
              await domIs(p, "확인 카드와 근거", ({ title }) => {
                const card = document.querySelector('[data-testid="ask-card"][data-kind="task"]');
                return card?.textContent.includes(title) && card.textContent.includes("착지: engine main abc1234 · push함") && card.textContent.includes("끝남");
              }, { title });
              const evidence = p.getByTestId("ask-task-evidence");
              assert((await evidence.textContent()).includes("작업 폴더"));
              if (action === "complete") await shot(p, `task-attention-${name}-${width}`);
              await p.getByTestId(`ask-task-${action}`).click();
              await domIs(p, "처리한 카드가 답 기록으로 이동", () => !!document.querySelector('[data-testid="ask-done-card"]'));
              const active = aaa("task", "list").tasks.find(t => t.title === title);
              if (action === "complete") {
                assert.equal(active, undefined);
                const archived = aaa("task", "archive").entries.find(t => t.task_id === task.id);
                assert.equal(archived.reason, "사용자 완료 확인");
                assert.equal(archived.report, `threads/${t.id}.md`);
              } else if (action === "reassign") {
                const entries = aaa("task", "queue").entries;
                assert(entries.some(q => q.task_id === task.id), "재배정 실행 예약");
              } else {
                assert.equal(active.thread, null);
                assert.equal(active.attention, null);
                assert(!aaa("task", "queue").entries.some(q => q.task_id === task.id));
              }
              const done = (await call(p, { op: "asks" })).ok.done.find(r => r.subject === title);
              assert.equal(done.summary, { complete: "완료 보관", reassign: "다시 맡김", unlink: "연결만 풂" }[action]);
              assert(done.body.includes("착지: engine main abc1234 · push함"));
            } finally { await p.context().close(); }
            // 예약은 실행되어도 이 실험실의 새 thread만 남는다. 다음 조합의 수용량을 확보한다.
            if (action === "reassign") {
              aaa("ticker", "tick");
              const current = await until("재배정 thread 연결", () => {
                const task = aaa("task", "list").tasks.find(t => t.title === title);
                return task?.thread && task.thread !== t.id && task;
              });
              await until("재배정 보고 수집", () => aaa("thread", "show", current.thread).report_hash);
              finishFixtureFollowups(current.thread);
              aaa("thread", "resolve", current.thread);
            }
          });
        }
      }
    }
  } finally { await wk.close(); }
}

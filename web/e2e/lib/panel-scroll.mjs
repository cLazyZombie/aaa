// E2E26: 실제 격리 host의 긴 목록. DOM 복제나 높이 강제 없이 마지막 행의 접근·선택을 검증한다.
import { panelRows } from "./panel-rows.mjs";
import { installPanelScrollTrace, PanelScrollDiagnostics } from "./panel-scroll-diagnostics.mjs";
import { deferTaskCapabilities } from "./panel-scroll-capabilities.mjs";
import assert from "node:assert/strict";
import fs from "node:fs";
import { DatabaseSync } from "node:sqlite";
import { root, aaa, step, browser, webkit, pairPage, stop, start, until, domIs, hashIs, inList, drawerSettled, shot, inTime } from "./harness.mjs";

/** scrollIntoView/click의 자동 스크롤 전에 칸의 경계와 직접 스크롤을 확인한다. */
async function reachEnd(p, selector, rowSelector, wheel, afterEnd) {
  const list = p.locator(selector), last = list.locator(rowSelector).last();
  await last.waitFor();
  await panelRows(p, `${selector} ${rowSelector}`);
  const geometry = await list.evaluate((el, value) => {
    window.__panelScrollTrace.begin(value);
    const r = el.getBoundingClientRect();
    const geometry = { scrollTop: el.scrollTop, h: el.clientHeight, sh: el.scrollHeight, sw: el.scrollWidth, w: el.clientWidth, top: r.top, bottom: r.bottom, viewport: innerHeight, overflow: getComputedStyle(el).overflowY };
    window.__panelScrollTrace.mark("geometry", { geometry }, false);
    return geometry;
  }, { selector, rowSelector, wheel });
  assert.ok(geometry.h > 0 && geometry.sh > geometry.h + 100, `긴 목록의 제한된 높이: ${selector} ${JSON.stringify(geometry)}`);
  assert.ok(geometry.top >= 0 && geometry.bottom <= geometry.viewport + 1, `화면 안 목록: ${selector} ${JSON.stringify(geometry)}`);
  assert.ok(geometry.sw <= geometry.w + 1, `가로 넘침: ${selector}`);
  assert.match(geometry.overflow, /auto|scroll/);
  await list.evaluate((el) => { window.__panelScrollTrace.phase("reset"); el.scrollTop = 0; window.__panelScrollTrace.mark("reset-assigned", {}, false); });
  if (wheel) {
    const r = await list.boundingBox();
    await p.mouse.move(r.x + r.width / 2, r.y + r.height / 2);
    // scrollTop은 DOM mutation이 아니다. 다른 화면 갱신에 기대지 않고 scroll 사건을 기다린다.
    const moved = inTime(list.evaluate((el) => new Promise((resolve, reject) => {
      const finish = () => {
        if (el.scrollTop <= 100) return;
        clearTimeout(limit);
        el.removeEventListener("scroll", finish);
        resolve();
      };
      const limit = setTimeout(() => {
        el.removeEventListener("scroll", finish);
        reject(new Error("휠 뒤 목록이 움직이지 않았다"));
      }, 20_000);
      el.addEventListener("scroll", finish);
      window.__panelScrollTrace.phase("wheel");
      finish();
    })), 20_000, "휠의 scroll 사건");
    await p.mouse.wheel(0, 700);
    await moved;
  }
  // WebKit 휴대폰에서도 실제 overflow 컨테이너를 굴린다(합성 touchmove는 native 스크롤을 하지 않는다).
  await list.evaluate((el) => { window.__panelScrollTrace.phase("end-request"); const requested = el.scrollHeight; el.scrollTop = requested; window.__panelScrollTrace.mark("end-assigned", { requested }, false); });
  if (afterEnd) await afterEnd();
  await domIs(p, "목록 끝 도착", (s) => { const e = document.querySelector(s); return e.scrollTop > 100 && Math.abs(e.scrollHeight - e.clientHeight - e.scrollTop) <= 2; }, selector);
  await domIs(p, `마지막 행이 목록 안에 보이고 눌린다: ${selector}`, ({ selector, rowSelector }) => {
    // React 목록 교체와 locator 평가 사이에 분리된 옛 행을 잡지 않는다. 같은 DOM 평가에서 현재 행·부모를 함께 읽는다.
    const parentEl = document.querySelector(selector), el = parentEl?.querySelectorAll(rowSelector).item(parentEl.querySelectorAll(rowSelector).length - 1);
    if (!el || !parentEl) return false;
    const r = el.getBoundingClientRect(), parent = parentEl.getBoundingClientRect();
    const x = r.left + r.width / 2, y = Math.min(r.bottom, parent.bottom) - 3;
    return r.top >= parent.top - 1 && r.bottom <= parent.bottom + 1 && el.contains(document.elementFromPoint(x, y));
  }, { selector, rowSelector });
  if (await list.evaluate((el) => !!el.closest("#sidebar-column"))) {
    const fixed = await p.evaluate(() => [".list-head", ".list-foot"].every((s) => {
      const r = document.querySelector(`#sidebar-column ${s}`).getBoundingClientRect();
      return r.top >= 0 && r.bottom <= innerHeight + 1;
    }));
    assert.ok(fixed, "스크롤 뒤에도 패널 머리와 바닥은 화면 안");
  }
  await panelRows(p, `${selector} ${rowSelector}`);
  assert.equal(await p.evaluate(() => window.scrollY), 0, "앱 문서는 스크롤하지 않는다");
  return last;
}

async function delayedCapabilities(instance, engine, name, viewport, touch, position) {
  await step(`할 일 권한 지연 ${engine} ${name} ${position}: 실제 응답 뒤 보기 높이가 줄어도 끝·읽던 행 유지`, async () => {
    const { page: p, errors } = await pairPage(instance, aaa("web", "pair").code, viewport, {
      touch, hash: "#tasks", init: installPanelScrollTrace,
      setup: ctx => ctx.addInitScript(deferTaskCapabilities),
    });
    const diagnostic = new PanelScrollDiagnostics(root, p, engine, `${name}-capabilities-${position}`);
    const selector = '[data-testid="tasks-list"]';
    try {
      await domIs(p, "실제 권한 응답보다 79행이 먼저 표시", () => window.__taskCapabilityGate?.held > 0 &&
        document.querySelector('[data-testid="tasks-view"]')?.dataset.canEdit === "pending" &&
        document.querySelectorAll('[data-testid="tasks-list"] .task-row').length === 79);
      await p.evaluate(value => window.__panelScrollTrace.begin(value), { selector, rowSelector: ".task-row", wheel: !touch });
      const release = async () => {
        const before = await p.locator(selector).evaluate(el => ({ h: el.clientHeight, sh: el.scrollHeight, top: el.scrollTop }));
        await p.evaluate(() => { window.__panelScrollTrace.mark("capabilities-before"); window.__taskCapabilityGate.release(); });
        await domIs(p, "실제 권한 응답과 편집 도구 반영", () => document.querySelector('[data-testid="tasks-view"]')?.dataset.canEdit === "true");
        const after = await p.locator(selector).evaluate(el => ({ h: el.clientHeight, sh: el.scrollHeight, top: el.scrollTop }));
        await p.evaluate(() => window.__panelScrollTrace.mark("capabilities-after"));
        if (!touch) assert.ok(after.h < before.h, `편집 도구가 실제 보기 높이를 줄인다: ${JSON.stringify({ before, after })}`);
      };
      if (position === "end") {
        // 기존 끝 대입·도착 단언 사이에 실제 Response를 전달한다. fixture가 먼저 준비되기를 기다리지 않는다.
        await reachEnd(p, selector, ".task-row", !touch, release);
      } else {
        await p.locator(selector).evaluate(el => { el.scrollTop = 700; });
        await domIs(p, "권한 응답 전 중간 위치", () => document.querySelector('[data-testid="tasks-list"]')?.scrollTop === 700);
        const anchor = await p.locator(selector).evaluate(el => {
          const box = el.getBoundingClientRect();
          const row = [...el.querySelectorAll(".task-row")].find(row => row.getBoundingClientRect().top >= box.top);
          return { id: row.dataset.testid, offset: row.getBoundingClientRect().top - box.top };
        });
        await p.evaluate(anchor => window.__panelScrollTrace.mark("reading-anchor", { anchor }, false), anchor);
        await release();
        await p.evaluate(({ id }) => {
          const el = document.querySelector('[data-testid="tasks-list"]'), row = document.querySelector(`[data-testid="${id}"]`);
          window.__panelScrollTrace.mark("reading-anchor-after", { id, offset: row.getBoundingClientRect().top - el.getBoundingClientRect().top }, false);
        }, anchor);
        await domIs(p, "권한 응답 뒤에도 중간의 같은 행·위치", ({ id, offset }) => {
          const el = document.querySelector('[data-testid="tasks-list"]'), row = document.querySelector(`[data-testid="${id}"]`);
          return el.scrollTop > 100 && el.scrollHeight - el.clientHeight - el.scrollTop > 100 &&
            Math.abs(row.getBoundingClientRect().top - el.getBoundingClientRect().top - offset) <= 1;
        }, anchor);
      }
      assert.deepEqual(errors, []);
      if (process.env.AAA_E2E_PANEL_TRACE) await diagnostic.save();
    } catch (error) {
      try { await diagnostic.save(error); }
      catch (captureError) { console.error("권한 지연 목록 진단 저장 오류", String(captureError)); }
      throw error;
    } finally {
      await p.evaluate(() => window.__taskCapabilityGate.release()).catch(() => {});
      await p.context().close();
    }
  });
}

export async function longPanelLists() {
  await step("긴 목록 준비: 답한 것 79개·할 일 79개·완료 79개·자동화 40개(실제 격리 파일과 DB)", async () => {
    const dir = `${root}/projects/rv`;
    fs.writeFileSync(`${dir}/TASKS.md`, `## Backlog\n${Array.from({ length: 79 }, (_, i) => `- [ ] 스크롤 할 일 ${String(i + 1).padStart(2, "0")} (me)`).join("\n")}\n`);
    aaa("task", "list");
    for (let i = 1; i <= 40; i++) fs.writeFileSync(`${dir}/routines/scroll-${String(i).padStart(2, "0")}.md`, '+++\nschedule = "every 1h"\nenabled = false\n+++\n긴 목록 스크롤 검사\n');
    // 외부 writer는 host를 멈춘 동안에만 쓴다(E2E86과 같은 격리 경계).
    await stop("host");
    const db = new DatabaseSync(`${root}/s/host/host.db`);
    try {
      const ask = db.prepare("INSERT INTO asks(project,id,kind,subject,done_ms,data) VALUES('rv',?,'request',?,?,?)");
      const archive = db.prepare("INSERT INTO task_archive(project,task_id,data) VALUES('rv',?,?)");
      db.exec("BEGIN");
      const now = Date.now();
      for (let i = 1; i <= 79; i++) {
        const id = `scroll-${String(i).padStart(2, "0")}`;
        const record = { id, question_id: `q-${1130 + i}`, kind: "request", subject: id, title: `스크롤 질문 ${String(i).padStart(2, "0")}`, asked_ms: now - i, done_ms: now - i, by: "CLI", summary: "답함으로 표시", items: [], body: "마지막 질문 원문도 열 수 있다." };
        ask.run(id, id, record.done_ms, JSON.stringify(record));
        const taskId = String(i).padStart(26, "0");
        const title = `스크롤 완료 ${String(i).padStart(2, "0")}`;
        archive.run(taskId, JSON.stringify({ task_id: taskId, task: { id: taskId, list: "Backlog", title, owner: "me", thread: null, notes: [`완료 메모 ${i}`], done: true, repo: null }, body: `- [x] ${title} (me)`, reason: "완료 체크 [x]", report: null, completed_ms: now - i, restored: false, committed: true, removed_sha256: "", history: [] }));
      }
      db.exec("COMMIT");
    } finally { db.close(); }
    start("host", "host");
    await until("긴 목록 host 준비", () => aaa("task", "list").tasks.length === 79);
  });
  const wk = await webkit.launch();
  try {
    for (const [engine, instance] of [["chromium", browser], ["webkit", wk]]) {
      for (const [name, viewport, touch] of [["desktop", { width: 1280, height: 800 }, false], ["phone", { width: 390, height: 844 }, true]]) {
        await step(`긴 목록 ${engine} ${name}: 답한 것·보고서·할 일·자동화·완료의 끝까지 스크롤하고 선택`, async () => {
          const { page: p, errors } = await pairPage(instance, aaa("web", "pair").code, viewport, { touch, init: installPanelScrollTrace });
          const diagnostic = new PanelScrollDiagnostics(root, p, engine, name);
          try {
            await inList(p, "activity-asks");
            await drawerSettled(p);
            await domIs(p, "답한 것 79", () => document.querySelector('[data-testid="asks-done-count"]')?.textContent === "79");
            await p.getByRole("button", { name: "답한 것 더 보기", exact: true }).click();
            await domIs(p, "답한 것 79개 표시", () => document.querySelectorAll('[data-testid="asks-panel"] .ask-row.is-done').length === 79);
            const ask = await reachEnd(p, '[data-testid="asks-panel"]', '.ask-row', !touch);
            await shot(p, `panel-scroll-${engine}-${name}-asks`);
            await ask.click();
            await hashIs(p, "#ask=d%3Ascroll-79");
            await p.getByTestId("asks-view").getByText("마지막 질문 원문도 열 수 있다.", { exact: true }).waitFor();
            // 선택 뒤에도 머리·바닥 조작이 보이고 다시 다른 패널로 갈 수 있다.
            await inList(p, "activity-automations");
            await drawerSettled(p);
            await p.getByTestId("automation-rv-scroll-40").waitFor();
            const automation = await reachEnd(p, '[data-testid="automations"]', '.automation-card', !touch);
            await automation.click();
            await p.getByTestId("automation-detail").waitFor();
            // 선택·hover 배경이 앞 행의 둘째 줄을 덮지 않는지 실제 선택 후 다시 본다.
            if (touch) {
              await p.locator(".view [data-testid='drawer-toggle']:visible").click();
              await drawerSettled(p, true);
            } else await automation.hover();
            await panelRows(p, ".automation-card");
            await shot(p, `panel-scroll-${engine}-${name}-automation-selected`);
            await inList(p, "activity-reports");
            await domIs(p, "보고서 준비", () => document.querySelector('[data-testid="reports-panel"]')?.dataset.status === "ready");
            const report = await reachEnd(p, '[data-testid="reports-scroll"]', '.report-card', !touch);
            await report.getByTestId("report-open").click();
            await p.getByTestId("report-frame").waitFor();
            await inList(p, "activity-tasks");
            await domIs(p, "할 일 79행", () => document.querySelectorAll('[data-testid="tasks-list"] .task-row').length === 79);
            const task = await reachEnd(p, '[data-testid="tasks-list"]', '.task-row', !touch);
            await task.locator(".task-title").click();
            await p.getByTestId("task-detail").waitFor();
            // 휴대폰 상세 시트는 닫은 뒤 완료 탭을 누른다.
            if (touch) await p.getByTestId("task-detail-close").click();
            await p.getByTestId("tasks-tab-completed").click();
            await p.locator(".task-completed-row").first().waitFor();
            await reachEnd(p, '[data-testid="task-archive-list"]', '.task-completed-row', !touch);
            const more = p.getByTestId("task-archive-more");
            while (await more.count()) {
              const count = await p.locator(".task-completed-row").count();
              await more.click();
              await domIs(p, "완료 다음 페이지", (before) => document.querySelectorAll('.task-completed-row').length > before, count);
              await reachEnd(p, '[data-testid="task-archive-list"]', '.task-completed-row', !touch);
            }
            const last = p.locator(".task-completed-row").last();
            await last.locator("summary").click();
            await last.getByText("완료 메모 79", { exact: true }).waitFor();
            await shot(p, `panel-scroll-${engine}-${name}-completed`);
            assert.deepEqual(errors, []);
            if (process.env.AAA_E2E_PANEL_TRACE) await diagnostic.save();
          } catch (error) {
            try { await diagnostic.save(error); }
            catch (captureError) { console.error("목록 실패 진단 저장 오류", String(captureError)); }
            throw error;
          } finally { await p.context().close(); }
        });
      }
    }
    for (const [engine, instance] of [["chromium", browser], ["webkit", wk]])
      for (const [name, viewport, touch] of [["desktop", { width: 1280, height: 800 }, false], ["phone", { width: 390, height: 844 }, true]])
        for (const position of ["end", "middle"])
          await delayedCapabilities(instance, engine, name, viewport, touch, position);
  } finally { await wk.close(); }
}

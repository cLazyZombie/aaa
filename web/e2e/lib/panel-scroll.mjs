// E2E26: 실제 격리 host의 긴 목록. DOM 복제나 높이 강제 없이 마지막 행의 접근·선택을 검증한다.
import assert from "node:assert/strict";
import fs from "node:fs";
import { DatabaseSync } from "node:sqlite";
import { root, aaa, step, browser, webkit, pairPage, stop, start, until, domIs, hashIs, inList, drawerSettled, shot, inTime } from "./harness.mjs";

/** scrollIntoView/click의 자동 스크롤 전에 칸의 경계와 직접 스크롤을 확인한다. */
async function reachEnd(p, selector, rowSelector, wheel) {
  const list = p.locator(selector), last = list.locator(rowSelector).last();
  await last.waitFor();
  const geometry = await list.evaluate((el) => {
    const r = el.getBoundingClientRect();
    return { h: el.clientHeight, sh: el.scrollHeight, sw: el.scrollWidth, w: el.clientWidth, top: r.top, bottom: r.bottom, viewport: innerHeight, overflow: getComputedStyle(el).overflowY };
  });
  assert.ok(geometry.h > 0 && geometry.sh > geometry.h + 100, `긴 목록의 제한된 높이: ${selector} ${JSON.stringify(geometry)}`);
  assert.ok(geometry.top >= 0 && geometry.bottom <= geometry.viewport + 1, `화면 안 목록: ${selector} ${JSON.stringify(geometry)}`);
  assert.ok(geometry.sw <= geometry.w + 1, `가로 넘침: ${selector}`);
  assert.match(geometry.overflow, /auto|scroll/);
  await list.evaluate((el) => { el.scrollTop = 0; });
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
      finish();
    })), 20_000, "휠의 scroll 사건");
    await p.mouse.wheel(0, 700);
    await moved;
  }
  // WebKit 휴대폰에서도 실제 overflow 컨테이너를 굴린다(합성 touchmove는 native 스크롤을 하지 않는다).
  await list.evaluate((el) => { el.scrollTop = el.scrollHeight; });
  await domIs(p, "목록 끝 도착", (s) => { const e = document.querySelector(s); return e.scrollTop > 100 && Math.abs(e.scrollHeight - e.clientHeight - e.scrollTop) <= 2; }, selector);
  const visible = await last.evaluate((el, selector) => {
    const r = el.getBoundingClientRect(), parent = el.closest(selector).getBoundingClientRect();
    const x = r.left + r.width / 2, y = Math.min(r.bottom, parent.bottom) - 3;
    return r.top >= parent.top - 1 && r.bottom <= parent.bottom + 1 && el.contains(document.elementFromPoint(x, y));
  }, selector);
  assert.ok(visible, `마지막 행이 목록 안에 보이고 눌린다: ${selector}`);
  if (await list.evaluate((el) => !!el.closest("#sidebar-column"))) {
    const fixed = await p.evaluate(() => [".list-head", ".list-foot"].every((s) => {
      const r = document.querySelector(`#sidebar-column ${s}`).getBoundingClientRect();
      return r.top >= 0 && r.bottom <= innerHeight + 1;
    }));
    assert.ok(fixed, "스크롤 뒤에도 패널 머리와 바닥은 화면 안");
  }
  assert.equal(await p.evaluate(() => window.scrollY), 0, "앱 문서는 스크롤하지 않는다");
  return last;
}

export async function longPanelLists() {
  await step("긴 목록 준비: 답한 것 79개·할 일 79개·기록 79개·자동화 40개(실제 격리 파일과 DB)", async () => {
    const dir = `${root}/projects/rv`;
    fs.writeFileSync(`${dir}/TASKS.md`, `## Backlog\n${Array.from({ length: 79 }, (_, i) => `- [ ] 스크롤 할 일 ${String(i + 1).padStart(2, "0")} (me)`).join("\n")}\n`);
    aaa("task", "list");
    for (let i = 1; i <= 40; i++) fs.writeFileSync(`${dir}/routines/scroll-${String(i).padStart(2, "0")}.md`, '+++\nschedule = "every 1h"\nenabled = false\n+++\n긴 목록 스크롤 검사\n');
    // 외부 writer는 host를 멈춘 동안에만 쓴다(E2E86과 같은 격리 경계).
    await stop("host");
    const db = new DatabaseSync(`${root}/s/host/host.db`);
    try {
      const ask = db.prepare("INSERT INTO asks(project,id,kind,subject,done_ms,data) VALUES('rv',?,'request',?,?,?)");
      const log = db.prepare("INSERT INTO task_log(project,at_ms,actor,action,list,title,before,after) VALUES('rv',?,'file','raw','Backlog',?,NULL,?)");
      db.exec("BEGIN");
      const now = Date.now();
      for (let i = 1; i <= 79; i++) {
        const id = `scroll-${String(i).padStart(2, "0")}`;
        const record = { id, question_id: `q-${1130 + i}`, kind: "request", subject: id, title: `스크롤 질문 ${String(i).padStart(2, "0")}`, asked_ms: now - i, done_ms: now - i, by: "CLI", summary: "답함으로 표시", items: [], body: "마지막 질문 원문도 열 수 있다." };
        ask.run(id, id, record.done_ms, JSON.stringify(record));
        log.run(now - i, `스크롤 기록 ${String(i).padStart(2, "0")}`, `기록 본문 ${i}`);
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
        await step(`긴 목록 ${engine} ${name}: 답한 것·보고서·할 일·자동화·기록의 끝까지 스크롤하고 선택`, async () => {
          const { page: p, errors } = await pairPage(instance, aaa("web", "pair").code, viewport, { touch });
          try {
            await inList(p, "activity-asks");
            await drawerSettled(p);
            await domIs(p, "답한 것 79", () => document.querySelector('[data-testid="asks-done-count"]')?.textContent === "79");
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
            // 휴대폰 상세 시트는 닫은 뒤 기록 탭을 누른다.
            if (touch) await p.getByTestId("task-detail-close").click();
            await p.getByTestId("tasks-tab-log").click();
            await p.locator('[data-testid="task-log"] .task-log-row').first().waitFor();
            const log = await reachEnd(p, '[data-testid="task-log"]', '.task-log-row', !touch);
            await log.locator(".task-log-toggle").click();
            await log.getByLabel("전후 차이").waitFor();
            // fixture 79개 앞에는 TASKS.md를 읽어 만든 실제 기록도 있다. 더 보기가 없어질 때까지 간다.
            const more = p.getByTestId("task-log").getByRole("button", { name: "더 보기", exact: true });
            while (await more.count()) {
              const count = await p.locator('[data-testid="task-log"] .task-log-row').count();
              await more.click();
              await domIs(p, "기록 다음 페이지", (before) => document.querySelectorAll('[data-testid="task-log"] .task-log-row').length > before, count);
              await reachEnd(p, '[data-testid="task-log"]', '.task-log-row', !touch);
            }
            const lastLog = p.locator('[data-testid="task-log"] .task-log-row').last();
            await lastLog.locator(".task-log-toggle").click();
            await lastLog.getByLabel("전후 차이").waitFor();
            await shot(p, `panel-scroll-${engine}-${name}-history`);
            assert.deepEqual(errors, []);
          } finally { await p.context().close(); }
        });
      }
    }
  } finally { await wk.close(); }
}

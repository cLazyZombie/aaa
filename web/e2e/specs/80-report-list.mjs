// 운영 폴더 대신 1,118개 보고 사본·큰 manifest를 만든다. 첨부 원문 바이트는 목록이 읽을 이유가 없다.
import fs from "node:fs";
import { DatabaseSync } from "node:sqlite";
import { root, aaa, inList, domIs, step, shots, browser, webkit, pairPage, inTime } from "../lib/harness.mjs";

export default async function ({ page }) {
  await step("보고서 목록 규모: 1118개 폴더·954개 첨부·보존 판·공유 링크의 API와 첫 화면 시간", async () => {
    aaa("project", "create", "demo");
    const db = new DatabaseSync(`${root}/s/host/host.db`);
    // harness가 명시한 격리 projects root만 쓴다.
    const projectDir = `${root}/projects/demo`;
    const put = db.prepare("INSERT INTO library_reports(project,name,updated,to_user,title) VALUES('demo',?,?,?,?)");
    const version = db.prepare("INSERT INTO report_ids(project,seq,id,source,revision,ready) VALUES('demo',?,?,?,'fixture',1)");
    const link = db.prepare("INSERT INTO library_links(id,project,thread,created,expires,revoked) VALUES(?,'demo',?,1,9999999999999,0)");
    db.exec("BEGIN");
    for (let i = 1; i <= 559; i++) {
      const name = `sample-${i}`, id = `r-${String(i).padStart(4, "0")}`;
      const files = Array.from({ length: i === 1 ? 954 : 8 }, (_, j) => ({ path: j === 0 ? "index.html" : `file-${j}.bin`, size: 1024 ** 2, on_agent: false, mime: "application/octet-stream", kind: "binary" }));
      for (const n of [name, id]) {
        fs.mkdirSync(`${projectDir}/library/${n}`, { recursive: true });
        fs.writeFileSync(`${projectDir}/threads/${n}.files.json`, JSON.stringify(files));
        put.run(n, i, i === 1 && n === name ? 1 : 0, n === name ? `보고서 ${i}` : null);
      }
      version.run(i, id, name);
      link.run(i.toString(16).padStart(16, "0"), name);
    }
    for (let i = 1; i <= 120; i++) {
      const task = `fixture-${i}`;
      const row = { task_id: task, task: { id: task, list: "Backlog", title: "보관", notes: [], done: true }, body: `- [x] 보관\n  [자료](library/r-${String(i).padStart(4, "0")}/index.html)`, reason: "완료", completed_ms: 1, restored: false, committed: true, removed_sha256: "", history: [] };
      db.prepare("INSERT INTO task_archive(project,task_id,data) VALUES('demo',?,?)").run(task, JSON.stringify(row));
      db.prepare("INSERT INTO task_ids(project,task_id,number) VALUES('demo',?,?)").run(task, i);
    }
    db.exec("COMMIT"); db.close();
    await page.evaluate(() => {
      performance.clearResourceTimings();
      window.__reportRequests = [];
      const original = window.fetch;
      window.fetch = async (...args) => {
        const op = args[1]?.body && JSON.parse(args[1].body).op;
        if (op !== "library_reports" && op !== "library_usage") return original(...args);
        const start = performance.now();
        const response = await original(...args);
        const json = response.json.bind(response);
        response.json = async () => {
          const value = await json();
          window.__reportRequests.push({ op, responseMs: performance.now() - start, reports: value.ok?.reports?.length });
          return value;
        };
        return response;
      };
      window.__reportsStart = performance.now();
    });
    await inList(page, "activity-reports");
    await domIs(page, "큰 목록 첫 화면", () => document.querySelector('[data-testid="reports-panel"]')?.dataset.status === "ready");
    await page.getByTestId("report-demo-sample-1").waitFor();
    const firstPaintMs = await page.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(() => done(performance.now() - window.__reportsStart)))));
    await page.getByTestId("reports-usage").waitFor();
    const requests = await page.evaluate(() => window.__reportRequests);
    const metrics = { fixture: { folders: 1118, sources: 559, largestManifest: 954, archives: 120 }, firstPaintMs, requests };
    console.log(`보고서 목록 측정 ${JSON.stringify(metrics)}`);
    if (shots) fs.writeFileSync(`${shots}/report-list-timing.json`, JSON.stringify(metrics, null, 2));
    if (requests.find((r) => r.op === "library_reports")?.reports !== 559) throw new Error("보존 판을 별도 줄로 세었다");
    await page.getByTestId("reports-tab-work").click();
    await domIs(page, "첫 50개만 그리기", () => document.querySelectorAll('.report-row').length === 50);
    // 화면 밖까지 포함해 558개 중 보관으로 보호한 119개를 빼고 고른다.
    await page.getByTestId("reports-pick-done").click();
    const picked = await page.getByTestId("reports-picked").innerText();
    if (!picked.includes("439개 선택")) throw new Error(`전체 선택이 첫 화면만 골랐다: ${picked}`);
    await page.getByTestId("reports-more").click();
    await domIs(page, "다음 50개", () => document.querySelectorAll('.report-row').length === 100);
    for (let i = 0; i < 10; i++) await page.getByTestId("reports-more").click();
    await domIs(page, "작업 보고 전체", () => document.querySelectorAll('.report-row').length === 558);
    await page.getByTestId("report-demo-sample-2").waitFor();
    if (await page.getByTestId("reports-more").count()) throw new Error("끝난 목록에 더 보기");
    const safari = await webkit.launch({ headless: true });
    try {
      for (const [engine, width, failUsage] of [[browser, 1280, true], [safari, 390, false]]) {
        let release, reached;
        const gate = new Promise((done) => { release = done; });
        const held = new Promise((done) => { reached = done; });
        const paired = await pairPage(engine, aaa("web", "pair", "--reader").code, { width, height: 844 }, {
          serviceWorkers: "block",
          setup: (ctx) => ctx.route("**/api/v1/call", async (route) => {
            if (route.request().postDataJSON()?.op !== "library_usage") return route.fallback();
            const response = await route.fetch();
            reached();
            await gate;
            return failUsage
              ? route.fulfill({ status: 503, contentType: "application/json", body: JSON.stringify({ err: { code: "unavailable", message: "실험실 용량 조회 실패" } }) })
              : route.fulfill({ response });
          }),
        });
        try {
          await inList(paired.page, "activity-reports");
          await inTime(held, 20_000, "용량 응답 보류");
          // 용량 답이 없는 동안 실제 host에서 온 목록은 그려져야 한다.
          await paired.page.getByTestId("report-demo-sample-1").waitFor();
          if (await paired.page.getByTestId("reports-usage").count()) throw new Error("보류한 용량을 추측해 보였다");
          release();
          if (failUsage) await paired.page.getByRole("alert").filter({ hasText: "실험실 용량 조회 실패" }).waitFor();
          else await paired.page.getByTestId("reports-usage").waitFor();
          await paired.page.getByTestId("report-demo-sample-1").waitFor();
          await paired.page.getByTestId("reports-tab-work").click();
          await domIs(paired.page, "보기 기기도 50개씩", () => document.querySelectorAll('.report-row').length === 50);
          if (await paired.page.getByTestId("reports-pick-done").count()) throw new Error("Reader에 삭제 선택");
          await domIs(paired.page, "목록 가로 넘침 없음", () => document.documentElement.scrollWidth <= innerWidth);
          if (paired.errors.length) throw new Error(paired.errors.join("; "));
        } finally {
          release();
          await inTime(paired.page.context().unrouteAll({ behavior: "wait" }), 20_000, "보류 응답 해제");
          await paired.page.context().close();
        }
      }
    } finally { await safari.close(); }

  });
}

// 실제 목록 API: thread 출처·활성 할 일 칸·보관 판을 잇고 화면과 일괄 조작 범위를 검사한다.
import assert from "node:assert/strict";
import fs from "node:fs";
import { DatabaseSync } from "node:sqlite";
import { root, aaa, inList, domIs, step, shot, browser, webkit, pairPage } from "../lib/harness.mjs";

export default async function () {
  await step("보고서 저장소: 세 탭·기기 기억·읽음 범위·선택 해제·세 화면 폭", async () => {
    aaa("project", "create", "demo");
    const dir = `${root}/projects/demo`;
    fs.writeFileSync(`${dir}/TASKS.md`, "# Tasks\n\n## Backlog\n- [ ] 첫 저장소 [aaa] · t-9001\n- [ ] 둘째 저장소 [rc] · t-9002\n");
    const db = new DatabaseSync(`${root}/s/host/host.db`);
    const put = db.prepare("INSERT INTO library_reports(project,name,updated,to_user,title,sent_at,from_thread) VALUES('demo',?,1,?,?,1,?)");
    for (const tab of ["mine", "work", "archive"]) {
      for (const [i, repo] of ["aaa", "rc", "none"].entries()) {
        const name = tab === "archive" ? `r-900${i + 1}` : `${tab}-${repo}`;
        fs.mkdirSync(`${dir}/library/${name}`, { recursive: true });
        fs.writeFileSync(`${dir}/threads/${name}.files.json`, "[]");
        put.run(name, tab === "work" ? 0 : 1, `${repo === "none" ? "직접 올린" : repo} 보고서`, i < 2 ? `t-900${i + 1}` : null);
        if (tab === "archive") {
          db.prepare("INSERT INTO report_ids(project,seq,id,source,revision,ready) VALUES('demo',?,?,?,'fixture',1)").run(9001 + i, name, `source-${i}`);
          db.prepare("INSERT INTO library_archives(project,name) VALUES('demo',?)").run(name);
        }
      }
    }
    const safari = await webkit.launch({ headless: true });
    try {
      for (const [engine, label] of [[browser, "chromium"], [safari, "webkit"]]) {
        for (const width of [1280, 820, 390]) {
          db.exec("UPDATE library_reports SET seen_at=NULL");
          const paired = await pairPage(engine, aaa("web", "pair").code, { width, height: 900 });
          const page = paired.page;
          try {
            await inList(page, "activity-reports");
            await page.getByTestId("report-demo-mine-aaa").waitFor();
            await shot(page, `report-repos-${label}-${width}-all`);
            const choose = async (name) => {
              await page.getByTestId("reports-repo").click();
              await page.locator('.row-menu, .row-sheet').getByText(name, { exact: true }).click();
              await page.locator('.row-menu, .row-sheet').waitFor({ state: "hidden" });
              await page.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))));
            };
            await page.getByTestId("reports-repo").click();
            await page.locator('.row-menu, .row-sheet').waitFor();
            await page.evaluate(async () => {
              await Promise.all(document.getAnimations().filter((a) => a.effect?.getComputedTiming().iterations !== Infinity).map((a) => a.finished.catch(() => {})));
            });
            await shot(page, `report-repos-${label}-${width}-menu`);
            await page.keyboard.press("Escape");
            await domIs(page, "필터 초점 복귀", () => document.activeElement?.getAttribute("data-testid") === "reports-repo");
            await page.evaluate(() => {
              localStorage.setItem("aaa:tasks:repo:demo", "rc");
              localStorage.setItem("aaa:reports:repo:other", "rc");
            });
            await choose("aaa");
            for (const tab of ["mine", "work", "archive"]) {
              await page.getByTestId(`reports-tab-${tab}`).click();
              await domIs(page, "저장소 한 개만", () => document.querySelectorAll('.report-card, .report-row').length === 1);
              assert.match(await page.locator('#reports-tabpanel').innerText(), /aaa 보고서/);
              await shot(page, `report-repos-${label}-${width}-${tab}`);
            }
            await page.getByTestId("reports-tab-work").click();
            await page.getByTestId("reports-pick-done").click();
            assert.match(await page.getByTestId("reports-picked").innerText(), /1개 선택/);
            await choose("rc");
            assert.match(await page.getByTestId("reports-picked").innerText(), /0개 선택/);
            await choose("aaa");
            assert.match(await page.getByTestId("reports-picked").innerText(), /0개 선택/);
            await page.getByTestId("reports-tab-mine").click();
            await page.getByTestId("reports-mark-read").click();
            await page.getByRole("alertdialog").getByRole("button", { name: "모두 읽음", exact: true }).click();
            await page.getByRole("alertdialog").waitFor({ state: "hidden" });
            assert.equal(db.prepare("SELECT count(*) AS n FROM library_reports WHERE seen_at IS NOT NULL").get().n, 1);
            await page.reload();
            await page.getByTestId("report-demo-mine-aaa").waitFor();
            assert.equal(await page.getByTestId("reports-repo").getAttribute("data-repo"), "aaa");
            assert.deepEqual(await page.evaluate(() => [localStorage.getItem("aaa:tasks:repo:demo"), localStorage.getItem("aaa:reports:repo:other")]), ["rc", "rc"]);
            await choose("정하지 않음");
            await page.getByTestId("report-demo-mine-none").waitFor();
            assert.equal(await page.locator('.report-card').count(), 1);
            await choose("전체");
            assert.equal(await page.locator('.report-card').count(), 3);
            await domIs(page, "가로 넘침 없음", () => document.documentElement.scrollWidth <= innerWidth);
            assert.deepEqual(paired.errors, []);
          } finally { await page.context().close(); }
        }
      }
      const reader = await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 390, height: 900 });
      try {
        await inList(reader.page, "activity-reports");
        await reader.page.getByTestId("reports-repo").click();
        await reader.page.locator('.row-menu, .row-sheet').getByText("rc", { exact: true }).click();
        await reader.page.getByTestId("report-demo-mine-rc").waitFor();
        assert.equal(await reader.page.locator('.report-card').count(), 1);
        assert.equal(await reader.page.getByTestId("reports-mark-read").count(), 0);
      } finally { await reader.page.context().close(); }
    } finally { await safari.close(); db.close(); }
  });
}

// 실제 목록 API: thread 출처·활성 할 일 칸·보관 판을 잇고 화면과 일괄 조작 범위를 검사한다.
import assert from "node:assert/strict";
import fs from "node:fs";
import { DatabaseSync } from "node:sqlite";
import { specCleanup, root, aaa, inList, domIs, focusIs, step, shot, browser, webkit, pairPage } from "../lib/harness.mjs";
import { clickDialogButton, clickMenuItem } from "../lib/menu.mjs";

// 새로고침 첫 DOM부터 본다. 최종 필터만 기다리면 중간의 전체 목록 노출을 놓친다.
function observeRepoRestore() {
  window.repoRestore = [];
  const sample = () => {
    if (!document.querySelector('[data-testid="report-demo-mine-aaa"]')) return;
    const state = {
      repo: document.querySelector('[data-testid="reports-repo"]')?.getAttribute('data-repo'),
      cards: document.querySelectorAll('.report-card').length,
      stored: localStorage.getItem('aaa:reports:repo:demo'),
      hash: location.hash,
    };
    if (JSON.stringify(window.repoRestore.at(-1)?.state) !== JSON.stringify(state)) window.repoRestore.push({ at: performance.now(), state });
  };
  new MutationObserver(sample).observe(document, { childList: true, subtree: true, attributes: true });
}

export default async function () {
  let db;
  let safari;
  const engine = async name => name === "chromium" ? browser : (safari ??= specCleanup.own(() => webkit.launch({ headless: true }), value => value.close())).value;

  async function reportRepo(engine, label, width) {
    db.exec("UPDATE library_reports SET seen_at=NULL");
    const paired = await pairPage(engine, aaa("web", "pair").code, { width, height: 900 });
    const page = paired.page;
    try {
      await inList(page, "activity-reports");
      await page.getByTestId("report-demo-mine-aaa").waitFor();
      await shot(page, `report-repos-${label}-${width}-all`);
      let repeatWithKeyboard = false;
      const choose = async (name) => {
        await page.getByTestId("reports-repo").click();
        const item = page.locator('.row-menu, .row-sheet').getByText(name, { exact: true });
        // 접근성 이름에는 보고서 수 hint도 붙는다. 글을 가진 실제 단추를 활성화한다.
        if (repeatWithKeyboard) {
          // RowMenu의 첫 rAF 초점이 Enter 대상의 초점을 뒤늦게 빼앗지 않게 한다.
          await focusIs(page, "저장소 시트 초기 항목", ".row-sheet-items > .row-sheet-item:first-child");
          await item.locator("xpath=ancestor-or-self::button[1]").press("Enter");
        } else await clickMenuItem(item);
        await page.locator('.row-menu, .row-sheet').waitFor({ state: "hidden" });
        await domIs(page, "저장소 시트 퇴장 완료", () => !document.querySelector('body > [inert][aria-hidden="true"]'));
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
      // 최초 열기·Esc 초점 복귀·실제 포인터 선택과 퇴장은 그대로 지킨다.
      // 휴대폰의 반복 필터는 실제 Enter 활성화를 써서 매번 포인터의 안정성을
      // 기다리지 않는다. 기본 애니메이션·퇴장·모든 필터/원장 단언을 유지한다.
      repeatWithKeyboard = width < 700;
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
      await clickDialogButton(page.getByRole("alertdialog").getByRole("button", { name: "모두 읽음", exact: true }));
      await page.getByRole("alertdialog").waitFor({ state: "hidden" });
      assert.equal(db.prepare("SELECT count(*) AS n FROM library_reports WHERE seen_at IS NOT NULL").get().n, 1);
      await page.context().addInitScript(observeRepoRestore);
      await page.reload();
      await page.getByTestId("report-demo-mine-aaa").waitFor();
      try {
        assert.equal(await page.getByTestId("reports-repo").getAttribute("data-repo"), "aaa");
        const restored = await page.evaluate(() => window.repoRestore);
        assert.ok(restored.length > 0, "새로고침 첫 카드 관측");
        assert.deepEqual(restored.filter(({ state }) => state.repo !== "aaa" || state.cards !== 1), [], "기억한 필터로 첫 카드부터 표시");
      } catch (error) {
        console.error("보고서 필터 복원", label, width, await page.evaluate(() => window.repoRestore));
        throw error;
      }
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

  try {
  await step("보고서 저장소 fixture", async () => {
    aaa("project", "create", "demo");
    const dir = `${root}/projects/demo`;
    fs.writeFileSync(`${dir}/TASKS.md`, "# Tasks\n\n## Backlog\n- [ ] 첫 저장소 [aaa] · t-9001\n- [ ] 둘째 저장소 [rc] · t-9002\n");
    db = new DatabaseSync(`${root}/s/host/host.db`);
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
  }, { always: true });
  await step("보고서 저장소: chromium 1280 · 세 탭·기억·읽음·선택", async () => {
    await reportRepo(await engine("chromium"), "chromium", 1280);
  });
  await step("보고서 저장소: chromium 820 · 세 탭·기억·읽음·선택", async () => {
    await reportRepo(await engine("chromium"), "chromium", 820);
  });
  await step("보고서 저장소: chromium 390 · 세 탭·기억·읽음·선택", async () => {
    await reportRepo(await engine("chromium"), "chromium", 390);
  });
  await step("보고서 저장소: webkit 1280 · 세 탭·기억·읽음·선택", async () => {
    await reportRepo(await engine("webkit"), "webkit", 1280);
  });
  await step("보고서 저장소: webkit 820 · 세 탭·기억·읽음·선택", async () => {
    await reportRepo(await engine("webkit"), "webkit", 820);
  });
  await step("보고서 저장소: webkit 390 · 세 탭·기억·읽음·선택", async () => {
    await reportRepo(await engine("webkit"), "webkit", 390);
  });
  await step("보고서 저장소: Reader 390 읽음 권한", async () => {
    const reader = await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 390, height: 900 });
    try {
      await inList(reader.page, "activity-reports");
      await reader.page.getByTestId("reports-repo").click();
      await clickMenuItem(reader.page.locator('.row-menu, .row-sheet').getByText("rc", { exact: true }));
      await reader.page.getByTestId("report-demo-mine-rc").waitFor();
      assert.equal(await reader.page.locator('.report-card').count(), 1);
      assert.equal(await reader.page.getByTestId("reports-mark-read").count(), 0);
    } finally { await reader.page.context().close(); }
  });
  } finally {
    try { if (safari) await safari.close(); } finally { db?.close(); }
  }
}

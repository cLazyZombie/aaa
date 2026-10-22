// thread 보류(plan §8.421, 사용자 10-22 1A 2A 3A 4A): thread 화면 「관리 → 보류」는 thread에게 정리(커밋·push·Handoff)를 맡기고,
// thread가 첫 줄 `[보류 준비됨]` 보고를 쓰면 host가 pane을 닫고 자리를 돌려준다. 보류한 thread는 목록(sidebar)에서 빠지고 「모든 thread」의
// 「보류」 묶음·할 일 줄의 「보류」 표지와 거르기 칩·thread 화면의 보류 띠에 보인다. 「이어 하기」 창은 host가 푼 머신별 길(같은 머신은
// 같은 대화 또는 같은 자리의 새 대화, 다른 머신은 새 worktree)을 보이고, 고르면 다시 띄운다. 휴대폰 폭에서도 창이 넘치지 않는다.
// 시간은 기다리지 않는다: host 기록·`ticker tick`·화면 표지를 본다.
import fs from "node:fs";
import { root, aaa, until, step, domIs, shot, openSettings, pairPage, webkit } from "../lib/harness.mjs";

const TASKS = "# Tasks\n\n## Backlog\n- [ ] 조명 검수 (fake@labA) [작업 폴더]\n  검수만 남음\n";
const show = (id) => aaa("thread", "show", "demo", id);

/** thread 화면을 열고 그 관리 메뉴 항목을 고른다(56과 같다: 데스크톱 popover, 휴대폰 sheet) */
async function manage(p, id, item) {
  await p.evaluate((h) => (location.hash = h), `#thread=demo%2F${id}`);
  await p.getByTestId("thread-view").waitFor();
  await domIs(p, `${id} 화면`, (id) => document.querySelector('[data-testid="thread-view"] .term-title strong')?.textContent?.endsWith(id), id);
  await p.getByTestId("thread-manage").click();
  await p.locator('[role="menuitem"], .row-sheet-item').filter({ hasText: item }).first().click();
}

/** 창이 화면 폭 안에 있고 옆으로 넘치지 않는다 */
const fits = (p, selector) =>
  p.evaluate((sel) => {
    const el = document.querySelector(sel);
    const body = el?.querySelector(".modal-body") ?? el;
    const box = el?.getBoundingClientRect();
    return el && box.left >= 0 && box.right <= innerWidth + 0.5 && body.scrollWidth <= body.clientWidth + 1 ? true : JSON.stringify({ box, sw: body?.scrollWidth, cw: body?.clientWidth, w: innerWidth });
  }, selector);

export default async function ({ page }) {
  let id;
  await step("보류: 「정리 맡기고 보류」 → thread의 [보류 준비됨] 보고 → pane을 닫고 보류 띠", async () => {
    // 가짜 thread가 곧 턴을 끝내 앱 안 알림이 화면 위를 덮는다: 이 기기는 앱 안 알림을 끈다(47과 같다)
    const settings = await openSettings(page, "알림");
    await settings.getByRole("switch", { name: "앱 안에서" }).click();
    await page.keyboard.press("Escape");
    aaa("project", "create", "demo");
    aaa("project", "set", "demo", "thread_profile", "fake");
    fs.writeFileSync(`${root}/projects/demo/TASKS.md`, TASKS);
    id = aaa("thread", "start", "demo", "--from-task", "조명 검수", "--link-task").id;
    await until(`${id} 보고·brief`, () => {
      const t = show(id);
      return t.report_hash && t.brief === "delivered";
    });
    await page.getByTestId(`thread-demo-${id}`).waitFor();
    await manage(page, id, "보류");
    const dialog = page.getByTestId("park-dialog");
    await dialog.getByTestId("park-note").fill("사람 검수 대기");
    await shot(page, "park-desktop-dialog");
    await dialog.getByTestId("park-ask").click();
    await dialog.waitFor({ state: "detached" });
    await until(`${id} 보류`, () => {
      aaa("ticker", "tick");
      // 「보류」로 바꾼 뒤 pane 정리를 마친 때가 `parked_at_ms`다
      const t = show(id);
      return t.status === "parked" && t.park.parked_at_ms != null;
    });
    const t = show(id);
    if (t.park.note !== "사람 검수 대기" || t.park.immediate || t.pane) throw new Error(`보류 기록: ${JSON.stringify(t.park)} pane ${t.pane}`);
    const banner = page.getByTestId("park-banner");
    await domIs(page, "보류 띠", () => document.querySelector('[data-testid="park-banner"]')?.textContent?.includes("사람 검수 대기") && document.querySelector('[data-testid="park-banner"] strong')?.textContent === "보류");
    await banner.getByTestId("park-banner-unpark").waitFor();
    // 목록(sidebar)에서는 빠진다
    await domIs(page, "목록에서 빠짐", (id) => document.querySelector(`[data-testid="thread-demo-${id}"]`) === null, id);
    await shot(page, "park-desktop-banner");
  });

  await step("보류 목록: 할 일 줄 「보류」 표지·거르기 칩, 「모든 thread」의 「보류」 묶음", async () => {
    await page.getByTestId("activity-tasks").click();
    await domIs(page, "할 일 줄 보류", () => document.querySelector('.task-row[data-title="조명 검수"]')?.dataset.taskState === "parked");
    await page.getByTestId("tasks-filter-parked").click();
    await domIs(page, "보류 칩 수·거른 줄", () => document.querySelector('[data-testid="tasks-filter-parked"] .tasks-count')?.textContent === "1" && document.querySelectorAll(".task-row[data-title]").length === 1);
    const chip = page.getByTestId(`task-thread-${id}`);
    if ((await chip.textContent()) !== `${id} · 보류`) throw new Error(`thread 칩: ${await chip.textContent()}`);
    await shot(page, "park-desktop-tasks");
    await page.getByTestId("tasks-filter-all").click();
    await page.evaluate(() => (location.hash = "#threads"));
    const group = page.getByTestId("project-group-parked");
    await group.getByTestId(`project-thread-${id}`).waitFor();
    if (!(await group.innerText()).includes("보류 (1)")) throw new Error(`보류 묶음: ${await group.innerText()}`);
    await shot(page, "park-desktop-all-threads");
  });

  await step("이어 하기: 창이 머신별 길·자리 수를 보이고, 같은 머신이면 같은 자리에서 다시 띄운다", async () => {
    await page.getByTestId(`project-thread-${id}`).click();
    await page.getByTestId("park-banner-unpark").click();
    const dialog = page.getByTestId("unpark-dialog");
    const own = dialog.getByTestId("unpark-option-labA");
    await own.waitFor();
    const text = await own.innerText();
    // custom harness는 대화 ID로 잇지 못한다: 같은 자리의 새 대화(이어 하기 요약)
    if (!text.includes("보류한 머신") || !text.includes("같은 자리에서 새 대화")) throw new Error(`labA 길: ${text}`);
    if (!(await own.locator("input").isChecked())) throw new Error("보류한 머신이 기본이 아니다");
    if (!(await dialog.getByTestId("unpark-slots").innerText()).includes("열린 thread 0/")) throw new Error("자리 수");
    await shot(page, "park-desktop-unpark");
    await dialog.getByTestId("unpark-confirm").click();
    await dialog.waitFor({ state: "detached" });
    await until(`${id} 다시 열림`, () => show(id).status === "open" && show(id).pane);
    await page.getByTestId("park-banner").waitFor({ state: "detached" });
    const brief = fs.readFileSync(`${show(id).work_dir}/brief.md`, "utf8");
    if (!brief.includes("# 보류에서 이어 하기") || !brief.includes("사람 검수 뒤 이어 간다")) throw new Error(`이어 하기 brief:\n${brief}`);
    await page.getByTestId(`thread-demo-${id}`).waitFor();
  });

  await step("휴대폰(WebKit 390): 보류 창·이어 하기 창이 폭 안에 들고 바로 보류가 된다", async () => {
    await until(`${id} brief 다시 받음`, () => show(id).brief === "delivered");
    const wk = await webkit.launch({ headless: true });
    try {
      const dev = await pairPage(wk, aaa("web", "pair").code, { width: 390, height: 844 }, { touch: true });
      const p = dev.page;
      try {
        await manage(p, id, "보류");
        const dialog = p.getByTestId("park-dialog");
        await dialog.getByTestId("park-now").waitFor();
        const ok = await fits(p, '[data-testid="park-dialog"]');
        if (ok !== true) throw new Error(`보류 창 넘침 ${ok}`);
        await shot(p, "park-phone-dialog");
        await dialog.getByTestId("park-now").click();
        await dialog.waitFor({ state: "detached" });
        await until(`${id} 바로 보류`, () => show(id).status === "parked" && show(id).park.immediate && show(id).park.parked_at_ms != null);
        await p.getByTestId("park-banner-unpark").click();
        const unpark = p.getByTestId("unpark-dialog");
        await unpark.getByTestId("unpark-option-labA").waitFor();
        const fit = await fits(p, '[data-testid="unpark-dialog"]');
        if (fit !== true) throw new Error(`이어 하기 창 넘침 ${fit}`);
        await shot(p, "park-phone-unpark");
        await p.keyboard.press("Escape");
        await unpark.waitFor({ state: "detached" });
      } finally {
        await p.context().close();
      }
    } finally {
      await wk.close();
    }
    // 보류한 thread도 끝낼 수 있다(보고와 결과물은 남는다)
    aaa("thread", "resolve", "demo", id);
    if (show(id).status !== "resolved") throw new Error("보류 thread 끝내기");
  });
}

// 할 일·thread별 자동 병합(plan §8.530, t-1214, 사용자 답 1A 2A): 값은 할 일 하나에 하나이고 host DB가 정본이다. 할 일 상세의 스위치,
// 「바로 시작」·예약 확인·새 할 일 폼의 칸, thread 화면의 스위치가 같은 값을 바꾸고, 할 일 줄·thread 목록·thread 제목이 「자동 병합」 표지로
// 보인다(「작업」 목록 줄은 둘째 줄 아이콘, t-1293). CLI(`thread auto-merge`)로 바꾼 값은 열린 화면에 사건으로 따라온다. brief 머리에 시작 때 값이 실린다. 휴대폰 폭에서 넘치지 않는다.
// 시간은 기다리지 않는다: host 기록(`task list`·`thread show`·`task log`)과 화면 표지를 본다.
import fs from "node:fs";
import { root, aaa, until, step, domIs, shot, openSettings, pairPage, webkit } from "../lib/harness.mjs";

const TASKS = "# Tasks\n\n## Backlog\n- [ ] 자동 착지 시험 (fake@labA) [작업 폴더]\n  병합 준비되면 묻지 않고 착지\n- [ ] 승인 착지 시험 (fake@labA) [작업 폴더]\n  예약해 둔다\n";
const row = (p, title) => p.locator(`.task-row[data-title="${title}"]`);
const pick = (p, title) => row(p, title).locator(".task-pick").click();
const task = (title) => aaa("task", "list").tasks.find((t) => t.title === title);
const merged = (title) => task(title)?.auto_merge === true;
const show = (id) => aaa("thread", "show", "demo", id);
/** 스위치(herdr Toggle)의 지금 값 */
/** 「작업」 목록 thread 줄(t-1293 사용자 답): 첫 줄은 「번호 · 제목」만, 아이콘은 둘째 줄 상태 말 앞 */
const rowMarkIs = (p, id, on) =>
  domIs(p, `작업 목록 아이콘 ${on ? "on" : "off"}`, ([id, want]) => {
    const row = document.querySelector(`[data-testid="thread-demo-${id}"]`);
    const first = row?.querySelector(".workspace-line");
    if (!first || first.querySelector(".thread-stage, .auto-merge-mark")) return false;
    const mark = row.querySelector('.thread-place > .thread-chips > [data-testid="auto-merge-mark"]');
    return want ? mark?.nextElementSibling === null && mark.parentElement.nextElementSibling?.classList.contains("thread-state") : mark === null && row.querySelector('[data-testid="auto-merge-mark"]') === null;
  }, [id, on]);
const switchIs = (p, testid, on) =>
  domIs(p, `${testid} ${on ? "on" : "off"}`, ([id, want]) => {
    const s = document.querySelector(`[data-testid="${id}"]`);
    return s?.dataset.busy === "false" && s.querySelector('[role="switch"]')?.getAttribute("aria-checked") === String(want);
  }, [testid, on]);

export default async function ({ page }) {
  let id;
  await step("할 일 상세의 스위치로 켜면 host 값·기록·coordinator inbox·줄 표지가 같다", async () => {
    // 가짜 thread가 곧 턴을 끝내 앱 안 알림이 화면 위를 덮는다: 이 기기는 앱 안 알림을 끈다(85와 같다)
    const settings = await openSettings(page, "알림");
    await settings.getByRole("switch", { name: "앱 안에서" }).click();
    await page.keyboard.press("Escape");
    aaa("project", "create", "demo");
    aaa("project", "set", "demo", "thread_profile", "fake");
    fs.writeFileSync(`${root}/projects/demo/TASKS.md`, TASKS);
    if (aaa("task", "list").tasks.some((t) => t.auto_merge)) throw new Error("기본값이 off가 아니다");
    await page.getByTestId("activity-tasks").click();
    await pick(page, "자동 착지 시험");
    await switchIs(page, "task-field-auto-merge", false);
    await page.getByTestId("task-field-auto-merge").getByRole("switch").click();
    await until("host 값 on", () => merged("자동 착지 시험"));
    await switchIs(page, "task-field-auto-merge", true);
    if (merged("승인 착지 시험")) throw new Error("다른 할 일까지 켰다");
    await domIs(page, "줄 표지", () =>
      document.querySelector('.task-row[data-title="자동 착지 시험"] [data-testid="auto-merge-badge"]') !== null &&
      document.querySelector('.task-row[data-title="승인 착지 시험"] [data-testid="auto-merge-badge"]') === null);
    // 바꾼 기록(누가·off→on)과 coordinator가 다음 턴에 볼 inbox
    const log = aaa("task", "log", "자동 착지 시험").entries.filter((e) => e.action === "auto_merge");
    if (log.length !== 1 || log[0].before !== "off" || log[0].after !== "on" || !log[0].actor.startsWith("device:")) throw new Error(`기록 ${JSON.stringify(log)}`);
    const inbox = aaa("inbox", "list", "demo").items.filter((i) => i.event === "auto-merge on");
    if (inbox.length !== 1 || !inbox[0].summary.includes("묻지 않고 착지 창")) throw new Error(`inbox ${JSON.stringify(inbox)}`);
    await shot(page, "auto-merge-task-detail");
  });

  await step("바로 시작 창의 칸은 할 일 값으로 열리고, 시작한 thread의 brief·목록·제목에 같은 값이 보인다", async () => {
    await page.getByTestId("task-detail").getByTestId("task-handoff-start").click();
    const dialog = page.getByTestId("task-start");
    await domIs(page, "바로 시작 값", () => document.querySelector('[data-testid="task-start"] form')?.dataset.startState === "ready");
    if (!(await dialog.getByTestId("task-start-auto-merge").isChecked())) throw new Error("시작 창 칸이 할 일 값(on)이 아니다");
    await shot(page, "auto-merge-start-dialog");
    await dialog.getByTestId("task-start-go").click();
    await page.getByTestId("task-started").waitFor();
    id = await until("시작한 thread", () => task("자동 착지 시험")?.thread);
    await until(`${id} brief`, () => show(id).brief === "delivered");
    const t = show(id);
    if (!t.auto_merge) throw new Error("thread가 이어진 할 일 값을 보이지 않는다");
    const brief = fs.readFileSync(`${t.work_dir}/brief.md`, "utf8");
    if (!brief.includes("- 자동 병합: on(병합 준비되면 사용자에게 묻지 않고 착지 창)")) throw new Error(`brief 머리:\n${brief.slice(0, 600)}`);
    await page.evaluate(() => (location.hash = "#threads"));
    await domIs(page, "thread 목록 표지", (id) => document.querySelector(`[data-testid="project-thread-${id}"] [data-testid="auto-merge-badge"]`) !== null, id);
  });

  await step("thread 화면 스위치로 끄면 할 일 값도 꺼지고, CLI로 켜면 열린 화면이 따라온다", async () => {
    await page.evaluate((h) => (location.hash = h), `#thread=demo%2F${id}`);
    await page.getByTestId("thread-view").waitFor();
    await switchIs(page, "auto-merge-switch", true);
    await page.getByTestId("auto-merge-switch").getByRole("switch").click();
    await until("thread 화면에서 끔", () => !show(id).auto_merge && !merged("자동 착지 시험"));
    await switchIs(page, "auto-merge-switch", false);
    await rowMarkIs(page, id, false);
    const back = aaa("thread", "auto-merge", id, "on");
    if (back.kind !== "auto_merge" || !back.on || !back.changed || back.thread !== id) throw new Error(JSON.stringify(back));
    await switchIs(page, "auto-merge-switch", true);
    await rowMarkIs(page, id, true);
    await shot(page, "auto-merge-thread-desktop");
    // 같은 값은 기록을 늘리지 않는다
    if (aaa("thread", "auto-merge", id, "on").changed) throw new Error("같은 값을 다시 썼다");
    const log = aaa("task", "log", "자동 착지 시험").entries.filter((e) => e.action === "auto_merge");
    if (log.map((e) => e.after).join(",") !== "on,off,on") throw new Error(`기록 ${log.map((e) => e.after)}`);
  });

  await step("새 할 일 폼·예약 확인의 칸은 만들거나 예약할 때 값을 저장한다", async () => {
    await page.getByTestId("activity-tasks").click();
    await page.getByTestId("tasks-add").click();
    const detail = page.getByTestId("task-detail");
    await detail.getByTestId("task-field-title").fill("새로 켠 일");
    await detail.getByTestId("task-field-auto-merge-new").check();
    await detail.getByTestId("task-save").click();
    await until("새 할 일 on", () => merged("새로 켠 일"));
    await pick(page, "승인 착지 시험");
    await page.getByTestId("task-reserve-preview").click();
    const confirm = page.getByTestId("task-reserve-confirm");
    await confirm.waitFor();
    if (await confirm.getByTestId("task-reserve-auto-merge").isChecked()) throw new Error("예약 칸이 할 일 값(off)이 아니다");
    await confirm.getByTestId("task-reserve-auto-merge").check();
    await shot(page, "auto-merge-reserve");
    await confirm.getByTestId("task-reserve-go").click();
    await confirm.waitFor({ state: "detached" });
    if (!merged("승인 착지 시험")) throw new Error("예약 칸 값을 저장하지 않았다");
    if (!aaa("task", "queue").entries.some((q) => q.task.title === "승인 착지 시험")) throw new Error("예약하지 않았다");
  });

  await step("휴대폰(WebKit 390): thread 화면 스위치가 폭 안에 있고 누르면 바뀐다", async () => {
    const wk = await webkit.launch({ headless: true });
    try {
      const dev = await pairPage(wk, aaa("web", "pair").code, { width: 390, height: 844 }, { touch: true });
      const p = dev.page;
      try {
        await p.evaluate((h) => (location.hash = h), `#thread=demo%2F${id}`);
        await p.getByTestId("thread-view").waitFor();
        await switchIs(p, "auto-merge-switch", true);
        await domIs(p, "가로 넘침 없음", () => {
          const r = document.querySelector('[data-testid="auto-merge-switch"]')?.getBoundingClientRect();
          return r && r.left >= 0 && r.right <= innerWidth + 0.5 && document.documentElement.scrollWidth <= innerWidth;
        });
        await shot(p, "auto-merge-thread-phone");
        await p.getByTestId("auto-merge-switch").getByRole("switch").tap();
        await until("휴대폰에서 끔", () => !show(id).auto_merge);
      } finally {
        await p.context().close();
      }
    } finally {
      await wk.close();
    }
  });
}

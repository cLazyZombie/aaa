// 자동화 보기의 주 경계: 실제 일정 판정 → DB 기록 → CLI/웹, 파일 enabled 편집, Reader, 휴대폰.
import fs from "node:fs";
import { root, aaa, step, browser, pairPage, until, domIs, drawerSettled, shot } from "../lib/harness.mjs";
export const labEnv = { AAA_TEST_ROUTINE_SECONDS: "1" };
let original;
const file = () => `${root}/projects/auto-demo/routines/check.md`;
export default async function ({ page }) {
  await step("자동화: 활동 막대에 규칙과 임무 조건을 함께 보고 host가 기록한 건너뜀 까닭·최근 기록을 읽는다", async () => {
    aaa("project", "create", "auto-demo");
    original = '+++\n# 보존할 주석\nschedule = "every 1s"\nenabled = true\n+++\n자료를 확인한다.\n';
    fs.writeFileSync(file(), original);
    fs.mkdirSync(`${root}/projects/auto-demo/missions`, { recursive: true });
    fs.writeFileSync(`${root}/projects/auto-demo/missions/watch.md`, '+++\nprofile="fake"\nevents=["git_pushed"]\nrepos=["app"]\nwithdrawal="none"\n+++\n자료를 분류한다.\n');
    await until("coordinator 없는 실제 차례", () => {
      aaa("ticker", "tick");
      return aaa("routine", "runs", "auto-demo", "check").runs.some((r) => r.detail === "coordinator 없음");
    });
    await page.getByTestId("activity-automations").click();
    const row = page.getByTestId("automation-auto-demo-check");
    await row.waitFor();
    await domIs(page, "건너뜀 기록", () => document.querySelector('[data-testid="automation-auto-demo-check"]')?.textContent.includes("마지막 건너뜀: coordinator 없음"));
    const mission = page.getByTestId("automation-auto-demo-mission:watch");
    await mission.waitFor();
    if (!(await mission.textContent()).includes("임무 파일에서") || !(await mission.textContent()).includes("git_pushed")) throw new Error("임무 조건 출처가 없다");
    await row.locator("summary").click();
    await row.locator("ol li").first().waitFor();
    if (!(await row.locator("ol").textContent()).includes("건너뜀: coordinator 없음")) throw new Error("최근 기록이 없다");
    await shot(page, "automations-desktop");
  });
  await step("자동화: 토글은 enabled만 바꾸고 주석·일정·본문을 지킨다. 임무 파일 토글도 다른 칸을 바꾸지 않는다", async () => {
    const button = page.getByRole("switch", { name: "auto-demo/check 켜기", exact: true });
    await button.click();
    await domIs(page, "규칙 꺼짐", () => document.querySelector('[data-testid="automation-auto-demo-check"] [role="switch"]')?.getAttribute("aria-checked") === "false");
    if (fs.readFileSync(file(), "utf8") !== original.replace("enabled = true", "enabled = false")) throw new Error("enabled 외 파일 변경");
    const missionPath = `${root}/projects/auto-demo/missions/watch.md`;
    const before = fs.readFileSync(missionPath, "utf8");
    await page.getByRole("switch", { name: "auto-demo/mission:watch 켜기", exact: true }).click();
    await domIs(page, "임무 조건 꺼짐", () => document.querySelector('[data-testid="automation-auto-demo-mission:watch"] [role="switch"]')?.getAttribute("aria-checked") === "false");
    const after = fs.readFileSync(missionPath, "utf8");
    if (after.replace('enabled = false\n', '') !== before) throw new Error("임무 enabled 외 파일 변경");
  });
  await step("자동화: Reader는 기록을 읽되 토글할 수 없고 휴대폰 drawer 안 목록·토글이 화면 폭 안에 있다", async () => {
    const dev = await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 390, height: 844 }, { touch: true });
    const p = dev.page;
    try {
      await p.locator(".view [data-testid='drawer-toggle']").click();
      await p.locator("#workspace-drawer.is-open").waitFor();
      await drawerSettled(p);
      await p.getByTestId("activity-automations").click();
      const row = p.getByTestId("automation-auto-demo-check");
      await row.waitFor();
      const toggle = row.getByRole("switch");
      if (!(await toggle.isDisabled())) throw new Error("Reader 토글 활성");
      const forbidden = await p.evaluate(async () => (await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ op: "routine_toggle", project: "auto-demo", name: "check", on: true }) })).json());
      if (forbidden.err?.code !== "forbidden") throw new Error(`Reader 쓰기 ${JSON.stringify(forbidden)}`);
      await row.locator("summary").click();
      await row.locator("ol li").first().waitFor();
      const geometry = await p.evaluate(() => {
        const panel = document.querySelector('.automations');
        const button = document.querySelector('[data-testid="automation-auto-demo-check"] [role="switch"]').getBoundingClientRect();
        return { overflow: panel.scrollWidth > panel.clientWidth + 1, left: button.left, right: button.right, width: innerWidth };
      });
      if (geometry.overflow || geometry.left < 0 || geometry.right > geometry.width) throw new Error(`휴대폰 폭 ${JSON.stringify(geometry)}`);
      await shot(p, "automations-phone-reader");
      if (dev.errors.length) throw new Error(dev.errors.join("\n"));
    } finally { await p.context().close(); }
  });
}

// 실제 host·브라우저에서 CI의 늦은 사건 경계를 고정한다. child.mjs로 실행한다.
import assert from "node:assert/strict";
import { aaa, browser, webkit, pairPage, step, inTime, inList, sentCount, sentFrames, root } from "./lib/harness.mjs";
import { modalKeyboardReady, initialPaneReady } from "./lib/interaction-ready.mjs";
import { failureDiagnosticsCase } from "./lib/failure-diagnostics-case.mjs";

export const labEnv = { AAA_E2E_FAILURE_DIAGNOSTICS: "1" };

export default async function () {
  const pane = aaa("pane", "list").panes.find(p => p.name === "t1").id;
  for (const type of ["chromium", "webkit"]) {
    const engine = type === "webkit" ? await webkit.launch() : browser;
    try {
      await step(`${type}: 첫 전체 목록 전에는 pane 머리 준비를 끝내지 않는다`, async () => {
        let release, arrive;
        const held = new Promise(resolve => { release = resolve; });
        const arrived = new Promise(resolve => { arrive = resolve; });
        let page;
        try {
          ({ page } = await pairPage(engine, aaa("web", "pair").code, { width: 390, height: 844 }, {
            hash: `#pane=${pane}`, serviceWorkers: "block",
            route: ["**/api/v1/call", async route => {
              if (route.request().postDataJSON()?.op !== "threads") return route.continue();
              const response = await route.fetch();
              arrive(); await held;
              await route.fulfill({ response });
            }],
          }));
          await inTime(arrived, 20_000, "첫 threads 응답 보류");
          let ready = false;
          const waiting = initialPaneReady(page, pane).then(() => { ready = true; });
          waiting.catch(() => {});
          await page.evaluate(() => document.querySelector(".app")?.dataset.threadsSync);
          assert.equal(ready, false, "machine 행 부착만으로 준비되지 않는다");
          release(); await waiting;
          await inList(page, "settings-open");
          await page.getByRole("region", { name: "설정" }).waitFor();
        } finally { release(); await page?.context().close(); }
      });

      await step(`${type}: 창이 보여도 초점 frame 전에는 키보드 준비를 끝내지 않는다`, async () => {
        const { page } = await pairPage(engine, aaa("web", "pair").code, { width: 1280, height: 800 }, { hash: `#pane=${pane}` });
        try {
          await initialPaneReady(page, pane);
          await page.getByTestId("terminal").click();
          await page.evaluate(() => {
            const frame = requestAnimationFrame;
            const held = [];
            window.__releaseModalFocus = () => { window.requestAnimationFrame = frame; for (const fn of held) frame(fn); };
            window.requestAnimationFrame = fn => { held.push(fn); return 0; };
          });
          const before = await sentCount(page);
          await page.keyboard.press("ControlOrMeta+Shift+O");
          const dialog = page.getByRole("dialog", { name: "새 pane · labA" });
          await dialog.waitFor();
          let ready = false;
          const waiting = modalKeyboardReady(page).then(() => { ready = true; });
          waiting.catch(() => {});
          assert.equal(await page.evaluate(() => document.querySelector(".new-session-modal").contains(document.activeElement)), false);
          assert.equal(ready, false, "dialog 부착만으로 키보드가 준비되지 않는다");
          await page.evaluate(() => window.__releaseModalFocus());
          await waiting;
          await page.keyboard.press("Escape");
          await dialog.waitFor({ state: "detached" });
          assert.equal((await sentFrames(page, before)).filter(f => f.t === "input" && f.data.includes("\x1b")).length, 0);
        } finally {
          await page.evaluate(() => window.__releaseModalFocus?.()).catch(() => {});
          await page.context().close();
        }
      });
      await step(`${type}: context 정리 뒤에도 최초 실패 진단을 보존한다`, () => failureDiagnosticsCase(engine, type, `${root}/diagnostics`));
    } finally { if (type === "webkit") await engine.close(); }
  }
}

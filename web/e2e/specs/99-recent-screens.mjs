// 설치 창의 OS 키 전달은 별도 실기 항목이다. 여기서는 standalone 조건을 주입하고
// Playwright 입력(Chromium CDP·WebKit protocol)으로 실제 앱·host·PTY의 경계를 검사한다.
import assert from "node:assert/strict";
import { root, aaa, step, browser, webkit, pairPage, inList, openSettings, until, screenText, paneHash, sentCount, sentFrames, sentText, shot } from "../lib/harness.mjs";

export default async function () {
  await step("PWA 최근 화면: 고정 MRU·역방향·확정·Esc·blur·종료·팔레트 공유·설정 끄기·터미널 입력 경계 (Chromium·WebKit)", async () => {
    const wk = await webkit.launch({ headless: true });
    try {
      for (const [engine, name, platform] of [[browser, "chromium", "Win32"], [wk, "webkit", "MacIntel"]]) {
        const names = ["a", "b", "c"].map((s) => `recent-${name}-${s}`);
        for (const label of names) aaa("tab", "create", "--machine", "labA", "--label", label, "--", process.execPath, `${root}/keys.js`);
        const ids = names.map((name) => aaa("pane", "list").panes.find((pane) => pane.name === name).id);
        const { page: p, errors } = await pairPage(engine, aaa("web", "pair").code, { width: 1100, height: 800 }, {
          platform, hash: paneHash(names[0]),
          init: () => {
            // 네이티브 예약 키를 우회하는 조건 주입이다. 실기 성공으로 세지 않는다.
            const media = window.matchMedia.bind(window);
            window.matchMedia = (query) => { const value = media(query); if (query === "(display-mode: standalone)") Object.defineProperty(value, "matches", { value: true }); return value; };
          },
        });
        try {
          const modal = p.getByTestId("recent-switcher");
          const selected = () => modal.getByRole("option", { selected: true }).locator("span").innerText();
          const history = () => p.evaluate(() => JSON.parse(localStorage.getItem("aaa:palette-recent") ?? "[]"));
          const address = () => p.evaluate(() => decodeURIComponent(location.hash));
          const at = async (index) => until(`최근 화면 ${names[index]}`, async () => (await address()) === `#pane=${ids[index]}`);
          const focus = async () => { await until("키 프로그램 준비", async () => (await screenText(p)).includes("keys>")); await p.locator(".xterm-screen").click(); };
          const open = async (index) => { await inList(p, `pane-${names[index]}`); await at(index); await focus(); };
          const noLeak = async (from) => assert.equal(sentText(await sentFrames(p, from)), "", `${name}: 전환 키가 PTY로 샜다`);
          await open(0); await open(1); await open(2);
          const original = ids.slice().reverse().map((id) => `pane:${id}`);
          assert.deepEqual(await history(), original);

          let from = await sentCount(p);
          await p.keyboard.down("Control"); await p.keyboard.press("Tab");
          await modal.waitFor();
          assert.equal(await selected(), names[1]); await at(2);
          await p.keyboard.press("Tab"); assert.equal(await selected(), names[0]);
          await p.keyboard.press("Shift+Tab"); assert.equal(await selected(), names[1]);
          assert.deepEqual(await history(), original);
          assert.equal(await modal.getByRole("option").evaluateAll((rows) => rows.every((row) => {
            const box = row.getBoundingClientRect();
            return [...row.children].every((child) => { const text = child.getBoundingClientRect(); return text.top >= box.top && text.bottom <= box.bottom; }) && row.scrollWidth <= row.clientWidth;
          })), true, "두 줄 항목의 글이 행 밖으로 넘친다");
          await shot(p, `recent-${name}`);
          await p.keyboard.press("Escape"); await p.keyboard.up("Control");
          await modal.waitFor({ state: "detached" }); await at(2);
          assert.deepEqual(await history(), original); await noLeak(from);
          assert.equal(await p.evaluate(() => document.activeElement?.classList.contains("xterm-helper-textarea")), true);

          // 확정한 것만 앞에 놓는다. 다음 짧은 Ctrl+Tab은 바로 전 화면으로 돌아간다.
          from = await sentCount(p);
          await p.keyboard.press("Control+Tab"); await at(1);
          assert.deepEqual(await history(), [`pane:${ids[1]}`, `pane:${ids[2]}`, `pane:${ids[0]}`]);
          await focus(); await p.keyboard.press("Control+Tab"); await at(2);
          assert.deepEqual(await history(), original); await noLeak(from);
          await focus(); await p.keyboard.press("Control+Shift+Tab"); await at(0);
          const afterReverse = [`pane:${ids[0]}`, `pane:${ids[2]}`, `pane:${ids[1]}`];
          assert.deepEqual(await history(), afterReverse);

          // Shift 뗌은 확정하지 않는다. 창을 벗어나면 취소하며 늦은 Ctrl 뗌도 무시한다.
          await focus(); from = await sentCount(p);
          await p.keyboard.down("Control"); await p.keyboard.press("Shift+Tab"); await modal.waitFor();
          assert.equal(await selected(), names[1]);
          await p.evaluate(() => window.dispatchEvent(new Event("blur")));
          await modal.waitFor({ state: "detached" }); await p.keyboard.up("Control");
          await at(0); assert.deepEqual(await history(), afterReverse); await noLeak(from);

          // 새로고침 후에도 이 기기의 기록을 쓴다. 팔레트도 확정된 순서를 공유한다.
          await p.reload(); await focus();
          assert.deepEqual(await history(), afterReverse);
          await p.keyboard.press(platform === "MacIntel" ? "Meta+Shift+K" : "Control+Shift+K");
          const palette = p.getByTestId("command-palette"); await palette.waitFor();
          // 현재 화면을 뺀 최근 두 줄이 먼저 보인다.
          const firstRows = palette.locator('[data-target]');
          assert.deepEqual(await firstRows.evaluateAll((rows) => rows.slice(0, 2).map((row) => row.dataset.target)), [`pane:${ids[2]}`, `pane:${ids[1]}`]);
          await p.keyboard.press("Control+Tab"); assert.equal(await modal.count(), 0); await at(0);
          await p.keyboard.press("Escape"); await palette.waitFor({ state: "detached" });

          // 설정에서 끄면 전환 창 대신 원래 xterm 경로로 보낸다. 다시 켜고 종료 경계를 검사한다.
          const settings = await openSettings(p, "단축키");
          const toggle = settings.getByRole("switch", { name: "최근 화면 전환", exact: true });
          await toggle.click(); assert.equal(await toggle.getAttribute("aria-checked"), "false");
          await settings.getByRole("button", { name: "설정 닫기" }).click(); await focus();
          from = await sentCount(p); await p.keyboard.press("Control+Tab");
          assert.equal(await modal.count(), 0); await at(0);
          const disabledBytes = sentText(await sentFrames(p, from));
          assert.notEqual(disabledBytes, "", "꺼진 키가 xterm에 전달되지 않았다");
          const settingsAgain = await openSettings(p, "단축키");
          await settingsAgain.getByRole("switch", { name: "최근 화면 전환", exact: true }).click();
          await settingsAgain.getByRole("button", { name: "설정 닫기" }).click(); await focus();

          from = await sentCount(p); await p.keyboard.down("Control"); await p.keyboard.press("Tab"); await modal.waitFor();
          assert.equal(await selected(), names[2]);
          aaa("pane", "kill", names[2]);
          await modal.waitFor({ state: "detached" }); await p.keyboard.up("Control");
          await at(0); assert.deepEqual(await history(), afterReverse); await noLeak(from);
          await focus(); await p.keyboard.down("Control"); await p.keyboard.press("Tab"); await modal.waitFor();
          assert.deepEqual(await modal.getByRole("option").locator("span").allTextContents(), [names[0], names[1]]);
          await p.keyboard.press("Escape"); await p.keyboard.up("Control");

          // 일반 탭에서도 똑같이 누른 키가 기존 xterm 경로로 간다(standalone 주입 없음).
          const normal = (await pairPage(engine, aaa("web", "pair").code, { width: 1100, height: 800 }, { platform, hash: paneHash(names[1]) })).page;
          try {
            await until("일반 탭 키 프로그램", async () => (await screenText(normal)).includes("keys>"));
            await normal.locator(".xterm-screen").click();
            const n = await sentCount(normal); await normal.keyboard.press("Control+Tab");
            assert.equal(await normal.getByTestId("recent-switcher").count(), 0);
            assert.equal(await normal.evaluate(() => decodeURIComponent(location.hash)), `#pane=${ids[1]}`);
            assert.equal(sentText(await sentFrames(normal, n)), disabledBytes);
          } finally { await normal.context().close(); }
          if (errors.length) throw new Error(errors.join("; "));
        } finally { await p.context().close(); }
      }
    } finally { await wk.close(); }
  });
}

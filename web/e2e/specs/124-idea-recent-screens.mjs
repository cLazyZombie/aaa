// 실제 host의 아이디어 둘을 방문하고 standalone 조건에서 Ctrl+Tab 등록·정방향·역방향·목록 복원을 본다.
// OS 예약 키 전달은 기존 E2E105처럼 조건을 주입하며, 모든 대기는 화면·방문 기록 사건으로 끝낸다.
import assert from "node:assert/strict";
import { aaa, step, browser, webkit, pairPage, inList } from "../lib/harness.mjs";

async function ideaRecentCase(engine, width, ids) {
  const { page: p, errors } = await pairPage(engine, aaa("web", "pair").code, { width, height: 844 }, {
    hash: "#tasks",
    init: () => {
      const media = window.matchMedia.bind(window);
      window.matchMedia = query => {
        const value = media(query);
        if (query === "(display-mode: standalone)") Object.defineProperty(value, "matches", { value: true });
        return value;
      };
    },
  });
  try {
    const modal = p.getByTestId("recent-switcher");
    const panel = p.locator('#sidebar-panel-ideas:not([hidden])');
    const history = () => p.evaluate(() => JSON.parse(localStorage.getItem("aaa:palette-recent") ?? "[]"));
    const visited = key => p.waitForFunction(key => JSON.parse(localStorage.getItem("aaa:palette-recent") ?? "[]")[0] === key, key);
    const note = async index => {
      await p.getByTestId("idea-title").getByText(`최근 노트 ${index + 1}`, { exact: true }).waitFor();
      assert.equal(await p.evaluate(() => decodeURIComponent(location.hash)), `#idea=${ids[index]}`);
      await visited(`idea:${ids[index]}`);
    };
    const selected = () => modal.getByRole("option", { selected: true }).locator(":scope > span").innerText();
    await p.getByTestId("tasks-view").waitFor();
    await visited("tasks:");
    await inList(p, "activity-ideas");
    await panel.waitFor();
    await visited("ideas:");
    for (let i = 0; i < ids.length; i++) {
      await inList(p, p.locator(`[data-testid="idea-row"][data-id="${ids[i]}"]`));
      await note(i);
    }
    const original = [`idea:${ids[1]}`, `idea:${ids[0]}`, "ideas:", "tasks:"];
    assert.deepEqual(await history(), original);
    await p.keyboard.down("Control"); await p.keyboard.press("Tab"); await modal.waitFor();
    assert.equal(await selected(), `아이디어 · ${ids[0]}`);
    await p.keyboard.press("Tab"); assert.equal(await selected(), "아이디어");
    await p.keyboard.press("Shift+Tab"); assert.equal(await selected(), `아이디어 · ${ids[0]}`);
    assert.deepEqual(await history(), original, "Ctrl 유지 중 방문 순서 고정");
    await p.keyboard.up("Control"); await modal.waitFor({ state: "detached" }); await note(0);
    await p.keyboard.press("Control+Tab"); await note(1);
    await p.keyboard.press("Control+Shift+Tab");
    await p.getByTestId("tasks-view").waitFor(); await visited("tasks:");
    await p.keyboard.down("Control"); await p.keyboard.press("Tab"); await modal.waitFor();
    await p.keyboard.press("Tab"); await p.keyboard.press("Tab");
    assert.equal(await selected(), "아이디어");
    await p.keyboard.up("Control"); await modal.waitFor({ state: "detached" });
    await panel.waitFor(); await visited("ideas:");
    assert.equal(await p.getByTestId("activity-ideas").getAttribute("aria-selected"), "true");
    assert.equal(await p.evaluate(() => location.hash), "#tasks", "목록 복귀는 보던 본문 주소 유지");
    await p.keyboard.press("Control+Tab"); await visited("tasks:");
    await p.getByTestId("tasks-view").waitFor();
    assert.deepEqual(errors, []);
  } finally { await p.context().close(); }
}

export default async function () {
  aaa("project", "create", "demo");
  const ids = [1, 2].map(n => aaa("idea", "new", `최근 노트 ${n}`).note.idea.short_id);
  await step("아이디어 최근 화면 Chromium 1280: 목록·노트 둘·정방향·역방향 복귀", () => ideaRecentCase(browser, 1280, ids));
  await step("아이디어 최근 화면 Chromium 390: 목록·노트 둘·정방향·역방향 복귀", () => ideaRecentCase(browser, 390, ids));
  const wk = await webkit.launch({ headless: true });
  try {
    await step("아이디어 최근 화면 WebKit 1280: 목록·노트 둘·정방향·역방향 복귀", () => ideaRecentCase(wk, 1280, ids));
    await step("아이디어 최근 화면 WebKit 390: 목록·노트 둘·정방향·역방향 복귀", () => ideaRecentCase(wk, 390, ids));
  } finally { await wk.close(); }
}

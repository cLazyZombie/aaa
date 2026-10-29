// 실제 host의 아이디어 둘을 방문하고 standalone 조건에서 Ctrl+Tab 등록·정방향·역방향 전환과 목록 제외를 본다.
// OS 예약 키 전달은 기존 E2E105처럼 조건을 주입하며, 모든 대기는 화면·방문 기록 사건으로 끝낸다.
import assert from "node:assert/strict";
import fs from "node:fs";
import { root, aaa, step, browser, webkit, pairPage, inList, coordinatorPost } from "../lib/harness.mjs";

async function ideaRecentCase(engine, width, ids) {
  const { page: p, errors } = await pairPage(engine, aaa("web", "pair").code, { width, height: 844 }, {
    hash: "#tasks",
    init: () => {
      // 이전 판의 목록 방문 기록도 전환 후보에 남으면 안 된다.
      localStorage.setItem("aaa:palette-recent", JSON.stringify(["ideas:"]));
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
    assert.deepEqual(await history(), ["tasks:", "ideas:"], "목록을 열어도 본문 방문 기록 유지");
    for (let i = 0; i < ids.length; i++) {
      await inList(p, p.locator(`[data-testid="idea-row"][data-id="${ids[i]}"]`));
      await note(i);
    }
    // 펼친 목록에서도 현재 화면은 오른쪽 노트여야 한다.
    if (width >= 769) {
      await p.getByTestId("activity-ideas").click();
      await inList(p, "activity-ideas");
    } else {
      // 선택한 패널 단추는 서랍을 닫으므로 본문의 열기 단추를 쓴다.
      await p.locator('.view [data-testid="drawer-toggle"]:visible').click();
      await p.locator("#workspace-drawer.is-open").waitFor();
    }
    await panel.waitFor();
    const original = [`idea:${ids[1]}`, `idea:${ids[0]}`, "tasks:", "ideas:"];
    assert.deepEqual(await history(), original);
    await p.keyboard.down("Control"); await p.keyboard.press("Tab"); await modal.waitFor();
    assert.equal(await selected(), `아이디어 · ${ids[0]}`);
    assert.deepEqual(await modal.getByRole("option").locator(":scope > span").allTextContents(),
      [`아이디어 · ${ids[1]}`, `아이디어 · ${ids[0]}`, "할 일"], "옛 기록의 목록도 후보에서 제외");
    await p.keyboard.press("Tab"); assert.equal(await selected(), "할 일");
    await p.keyboard.press("Shift+Tab"); assert.equal(await selected(), `아이디어 · ${ids[0]}`);
    assert.deepEqual(await history(), original, "Ctrl 유지 중 방문 순서 고정");
    await p.keyboard.up("Control"); await modal.waitFor({ state: "detached" }); await note(0);
    await p.keyboard.press("Control+Tab"); await note(1);
    await p.keyboard.press("Control+Shift+Tab");
    await p.getByTestId("tasks-view").waitFor(); await visited("tasks:");
    await p.keyboard.press("Control+Tab"); await note(1);
    await p.keyboard.press("Control+Shift+Tab"); await note(0);
    assert.deepEqual(errors, []);
  } finally { await p.context().close(); }
}

async function panelRecentCase(engine, width, question, request) {
  const { page: p, errors } = await pairPage(engine, aaa("web", "pair").code, { width, height: 844 }, {
    hash: "#automation=demo/recent",
    init: () => {
      localStorage.setItem("aaa:palette-recent", JSON.stringify(["automations:", "asks:", "ideas:"]));
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
    const history = () => p.evaluate(() => JSON.parse(localStorage.getItem("aaa:palette-recent") ?? "[]"));
    const visited = key => p.waitForFunction(key => JSON.parse(localStorage.getItem("aaa:palette-recent") ?? "[]")[0] === key, key);
    const rule = async () => { await p.getByTestId("automation-detail").waitFor(); await visited("automation:demo/recent"); };
    const card = async () => { await p.getByTestId(`request-ask-${request}`).waitFor(); await visited(`question:${question}`); };
    await rule();
    await p.evaluate(id => { location.hash = `question=${id}`; }, question);
    await card();
    for (const panel of ["automations", "asks"]) {
      // 현재 카드가 자동으로 펼친 답할 것 패널을 다시 눌러 닫지 않도록 다른 목록에서 연다.
      await inList(p, "activity-machines");
      await inList(p, `activity-${panel}`);
      await p.locator(`#sidebar-panel-${panel}:not([hidden])`).waitFor();
      assert.equal(await p.evaluate(() => decodeURIComponent(location.hash)), `#question=${question}`, "목록 열기는 오른쪽 카드 유지");
      const original = [`question:${question}`, "automation:demo/recent", "automations:", "asks:", "ideas:"];
      assert.deepEqual(await history(), original, "목록을 새 방문으로 기록하지 않는다");
      await p.keyboard.down("Control"); await p.keyboard.press("Tab"); await modal.waitFor();
      const selected = () => modal.getByRole("option", { selected: true }).locator(":scope > span").innerText();
      assert.deepEqual(await modal.getByRole("option").locator(":scope > span").allTextContents(),
        [`답할 것 · ${question}`, "자동화 · demo/recent"], "옛 세 목록 기록을 모두 제외한다");
      assert.equal(await selected(), "자동화 · demo/recent", "펼친 목록 뒤 첫 전환은 이전 본문");
      await p.keyboard.press("Shift+Tab"); assert.equal(await selected(), `답할 것 · ${question}`);
      await p.keyboard.press("Tab"); assert.equal(await selected(), "자동화 · demo/recent");
      assert.deepEqual(await history(), original, "Ctrl 유지 중 MRU 고정");
      await p.keyboard.up("Control"); await modal.waitFor({ state: "detached" }); await rule();
      assert.equal(await p.evaluate(() => decodeURIComponent(location.hash)), "#automation=demo/recent");
      await p.keyboard.press("Control+Shift+Tab"); await card();
    }
    assert.deepEqual(errors, []);
  } finally { await p.context().close(); }
}

export default async function ({ page }) {
  aaa("project", "create", "demo");
  const ids = [1, 2].map(n => aaa("idea", "new", `최근 노트 ${n}`).note.idea.short_id);
  await step("아이디어 최근 화면 Chromium 1280: 목록 제외·노트 둘·정방향·역방향 복귀", () => ideaRecentCase(browser, 1280, ids));
  await step("아이디어 최근 화면 Chromium 390: 목록 제외·노트 둘·정방향·역방향 복귀", () => ideaRecentCase(browser, 390, ids));
  fs.mkdirSync(`${root}/projects/demo/automations`, { recursive: true });
  fs.writeFileSync(`${root}/projects/demo/automations/recent.toml`, 'title="최근 규칙"\n[on.schedule]\nevery="1d"\n[do.inbox]\ntext="최근 화면 시험"\n');
  await coordinatorPost({ request: true, title: "최근 질문", body: "## Questions\n### 1. 진행할까요?\n- A 진행 (추천) — 확인\n- B 보류 — 대기\n" });
  const request = await page.evaluate(async () => {
    const r = await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ op: "asks" }) });
    return (await r.json()).ok.requests.find(r => r.title === "최근 질문");
  });
  const question = request.questions.items[0].id;
  await step("사이드바 목록 제외 Chromium 1280: 옛 기록·본문·정역방향", () => panelRecentCase(browser, 1280, question, request.id));
  await step("사이드바 목록 제외 Chromium 390: 옛 기록·본문·정역방향", () => panelRecentCase(browser, 390, question, request.id));
  const wk = await webkit.launch({ headless: true });
  try {
    await step("아이디어 최근 화면 WebKit 1280: 목록 제외·노트 둘·정방향·역방향 복귀", () => ideaRecentCase(wk, 1280, ids));
    await step("아이디어 최근 화면 WebKit 390: 목록 제외·노트 둘·정방향·역방향 복귀", () => ideaRecentCase(wk, 390, ids));
    await step("사이드바 목록 제외 WebKit 1280: 옛 기록·본문·정역방향", () => panelRecentCase(wk, 1280, question, request.id));
    await step("사이드바 목록 제외 WebKit 390: 옛 기록·본문·정역방향", () => panelRecentCase(wk, 390, question, request.id));
  } finally { await wk.close(); }
}

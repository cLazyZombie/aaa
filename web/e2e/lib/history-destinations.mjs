// E2E39: 같은 문서의 hash 이동과 hash가 그대로인 설정 기록을 함께 순회한다.
import fs from "node:fs";
import { aaa, root, inList, domIs, inTime, drawerSettled, shot } from "./harness.mjs";
import { pairDevice } from "./devices.mjs";

let thread;
const drawerIsOpen = (p) => p.locator("#workspace-drawer").evaluate((el) => el.classList.contains("is-open"));

// 제품보다 먼저 설치해 기록 쓰기·실제 브라우저 이동의 순서를 보존한다.
// pairing 주소의 인증 자료는 남기지 않고 이 시험이 쓰는 목적지만 기록한다.
function traceHistory() {
  if (window !== window.top) return;
  const trace = window.__historyDestinations = [];
  const record = (event, detail) => trace.push({
    event, at: Math.round(performance.now()), detail, length: history.length, hash: location.hash,
    settings: new URLSearchParams(location.search).get("settings"),
    entry: history.state?.["aaa:settings"] ?? null,
    sheet: history.state?.["aaa:sheet"] ?? null,
  });
  window.__markHistoryDestination = record;
  for (const method of ["pushState", "replaceState"]) {
    const write = history[method].bind(history);
    history[method] = (...args) => { write(...args); record(method); };
  }
  for (const event of ["popstate", "hashchange"]) addEventListener(event, () => record(event));
  document.addEventListener("load", (event) => {
    if (event.target?.tagName === "IFRAME") record("iframe-load");
  }, true);
}

async function setDrawer(p, open) {
  if ((await drawerIsOpen(p)) !== open) {
    if (open) await p.locator(".view [data-testid='drawer-toggle']:visible").click();
    else await p.locator(".scrim.is-open").click({ position: { x: 380, y: 600 } });
  }
  await domIs(p, "이동 전 메뉴", (open) => document.querySelector("#workspace-drawer")?.classList.contains("is-open") === open, open);
  await drawerSettled(p, true);
}

async function shown(p, { hash, settings, selector }) {
  await domIs(p, `이동한 본문 ${hash} ${settings ?? ""}`, ({ hash, settings, selector }) => {
    const query = new URLSearchParams(location.search);
    if (location.hash !== hash || query.get("settings") !== settings) return false;
    if (settings !== null) return !!document.querySelector("#settings-screen") && !!document.querySelector(selector);
    return !document.querySelector("#settings-screen") && !!document.querySelector(selector);
  }, { hash, settings, selector }).catch(async (error) => {
    const actual = await p.evaluate((selector) => ({
      hash: location.hash,
      settings: new URLSearchParams(location.search).get("settings"),
      selector,
      found: !!document.querySelector(selector),
      shown: [...document.querySelectorAll("[data-shown='true']")].map((el) => el.getAttribute("data-testid")),
      cards: [...document.querySelectorAll(".ask-card")].map((el) => el.getAttribute("data-testid")),
    }), selector);
    throw new Error(`${error.message}: ${JSON.stringify(actual)}`);
  });
}

export async function historyDestinations(browser) {
  if (!thread) {
    fs.writeFileSync(`${root}/history-task.md`, "뒤로·앞으로의 목적지 시험\n질문 카드 시험\n");
    thread = aaa("thread", "start", "demo", "--title", "이동 목적지", "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/history-task.md`);
  }
  const engine = browser.browserType().name();
  const { page: p } = await pairDevice(browser, aaa("web", "pair").code, "iphone", { init: traceHistory });
  try {
    const entries = [];
    const remember = async (selector) => {
      const entry = await p.evaluate((selector) => ({ hash: location.hash, settings: new URLSearchParams(location.search).get("settings"), selector }), selector);
      await shown(p, entry);
      entries.push(entry);
    };
    await inList(p, "pane-bk1");
    await remember('[data-testid="pane-bk1"][aria-current="true"]');
    const route = async (hash, selector) => {
      await p.evaluate((hash) => { location.hash = hash; }, hash);
      await shown(p, { hash: `#${hash}`, settings: null, selector });
      await remember(selector);
    };
    await route("threads", '[data-testid="all-threads"]');
    await route(`thread=${thread.id}`, '[data-testid="thread-view"]');
    await p.getByTestId("report").waitFor();
    await route("report=demo/backrep", '[data-testid="report-detail"][data-status="ready"]');
    await route("reports", '[data-testid="reports-view"][data-shown="true"] .reports-list:not([hidden])');
    await route("tasks", '[data-testid="tasks-view"][data-shown="true"]');
    await route(`question=${thread.id}`, `[data-testid="asks-view"][data-shown="true"] [data-testid="thread-ask-${thread.id}"]`);
    await route("asks", '[data-testid="asks-view"][data-shown="true"]');
    // 설정 목록·쪽은 pushState를 써 hash가 바뀌지 않는다. hashchange만 듣는 수리는 이 경로를 놓친다.
    await inList(p, "settings-open");
    await p.getByRole("region", { name: "설정", exact: true }).waitFor();
    await remember("#settings-screen .settings-nav");
    await p.getByRole("tab", { name: "모양", exact: true }).click();
    await p.getByRole("tabpanel", { name: "모양", exact: true }).waitFor();
    await remember('#settings-panel[aria-label="모양"]');
    const length = await p.evaluate(() => { window.__markHistoryDestination("순회 시작"); return history.length; });

    for (const open of [true, false]) {
      for (const direction of [-1, 1]) {
        const targets = direction < 0 ? entries.slice(0, -1).reverse() : entries.slice(1);
        for (const entry of targets) {
          await setDrawer(p, open);
          await inTime(p.evaluate(({ direction, open, entry }) => new Promise((done) => {
            window.__markHistoryDestination("이동 전", { open, direction, entry });
            addEventListener("popstate", done, { once: true });
            if (direction < 0) history.back(); else history.forward();
          }), { direction, open, entry }), 20_000, "목적지 순회의 popstate");
          await shown(p, entry);
          // React effect와 drawer transition까지 끝난 상태를 검사한다.
          await inTime(p.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done)))), 20_000, "목적지 그리기");
          await drawerSettled(p, true);
          if (await drawerIsOpen(p)) throw new Error(`${engine}: ${open ? "열고" : "닫고"} ${direction < 0 ? "뒤로" : "앞으로"} ${JSON.stringify(entry)}에서 메뉴 열림`);
        }
      }
      await shot(p, `39-history-${engine}-${open ? "open" : "closed"}`);
    }
    const actualLength = await p.evaluate(() => { window.__markHistoryDestination("순회 끝"); return history.length; });
    if (actualLength !== length) throw new Error(`${engine}: 메뉴 토글·기록 순회가 기록 칸 수를 바꿨다: 기대=${length}, 실제=${actualLength}, 순서=${JSON.stringify(await p.evaluate(() => window.__historyDestinations))}`);

    // 빈 첫 칸은 기존 규칙대로 보던 pane으로 정규화한다. 이 조기 return 경로도 메뉴를 닫아야 한다.
    await setDrawer(p, true);
    await inList(p, "pane-bk2");
    if (await drawerIsOpen(p)) throw new Error(`${engine}: 목록 줄 직접 선택 뒤 메뉴 열림`);
    await route("threads", '[data-testid="all-threads"]');
    await p.getByTestId("all-threads").getByRole("button", { name: "닫기", exact: true }).click();
    await p.getByTestId("empty-open-list").waitFor();
    if (await drawerIsOpen(p)) throw new Error(`${engine}: 빈 목록을 보였는데 메뉴 열림`);
    await p.getByTestId("empty-open-list").click();
    await domIs(p, "빈 목록에서 메뉴 단추", () => document.querySelector("#workspace-drawer")?.classList.contains("is-open"));
    await drawerSettled(p, true);
    await inList(p, "pane-bk3");
    const pane = await p.evaluate(() => location.hash);
    for (const direction of [-1, 1]) {
      await setDrawer(p, true);
      await inTime(p.evaluate((direction) => new Promise((done) => {
        addEventListener("popstate", done, { once: true });
        history.go(direction);
      }), direction), 20_000, "빈 칸 순회의 popstate");
      await shown(p, { hash: pane, settings: null, selector: '[data-testid="pane-bk3"][aria-current="true"]' });
      await drawerSettled(p, true);
      if (await drawerIsOpen(p)) throw new Error(`${engine}: 빈 칸 순회 뒤 메뉴 열림`);
    }
  } catch (error) {
    try {
      const trace = await p.evaluate(() => window.__historyDestinations);
      fs.mkdirSync(`${root}/diagnostics`, { recursive: true });
      fs.writeFileSync(`${root}/diagnostics/history-destinations-${engine}.json`, JSON.stringify(trace, null, 2));
    } catch (diagnosticError) {
      console.log(`기록 순회 진단 저장 실패: ${diagnosticError.message}`);
    }
    await shot(p, `39-history-${engine}-failure`);
    throw error;
  } finally {
    await p.context().close();
  }
  await taskSheetHistory(browser);
}

// 상세는 시트 전용 기록을 갖는다. 일반 본문 순회와 별도 페이지에서 그 수명을 확인한다.
async function taskSheetHistory(browser) {
  const engine = browser.browserType().name();
  const { page: p } = await pairDevice(browser, aaa("web", "pair").code, "iphone", { hash: "#tasks" });
  try {
    await shown(p, { hash: "#tasks", settings: null, selector: '[data-testid="tasks-view"][data-shown="true"]' });
    for (const open of [true, false]) {
      await p.getByTestId("tasks-list").locator(".task-pick").first().click();
      await p.getByTestId("task-detail").waitFor();
      await inTime(p.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done)))), 20_000, "상세 시트 기록 반영");
      if (!(await p.evaluate(() => typeof history.state?.["aaa:sheet"] === "string"))) throw new Error(`${engine}: 상세 시트 기록이 없다`);
      for (const direction of [-1, 1]) {
        if (open) {
          await p.keyboard.press("Meta+Shift+b");
          await domIs(p, "시트 순회 전 메뉴 열림", () => document.querySelector("#workspace-drawer")?.classList.contains("is-open"));
          await drawerSettled(p, true);
        }
        await inTime(p.evaluate((direction) => new Promise((done) => {
          addEventListener("popstate", done, { once: true });
          if (direction < 0) history.back(); else history.forward();
        }), direction), 20_000, "할 일 시트 순회의 popstate");
        await p.getByTestId("task-detail").waitFor({ state: "detached" });
        await shown(p, { hash: "#tasks", settings: null, selector: '[data-testid="tasks-view"][data-shown="true"]' });
        await drawerSettled(p, true);
        if (await drawerIsOpen(p)) throw new Error(`${engine}: 할 일 시트 순회 뒤 메뉴 열림`);
      }
    }
  } catch (error) {
    await shot(p, `39-task-sheet-${engine}-failure`);
    throw error;
  } finally {
    await p.context().close();
  }
}

// 설정 닫기의 내부 되감기를 늦춰 사용자가 메뉴를 연 뒤 도착하는 순서를 재현한다.
// 대체하는 것은 history.go의 전달 시점뿐이다. 실제 history·popstate·제품 설정 복귀를 쓴다.
import { aaa, inList, domIs, inTime, drawerSettled, shot, paneHash } from "./harness.mjs";
import { pairDevice } from "./devices.mjs";

function holdSettingsRewind() {
  if (window !== window.top) return;
  const trace = window.__settingsDrawerTrace = [];
  const record = (event) => {
    const drawer = document.getElementById("workspace-drawer");
    trace.push({ event, at: Math.round(performance.now()), hash: location.hash,
      drawer: drawer?.className, right: drawer?.getBoundingClientRect().right,
      visibility: drawer && getComputedStyle(drawer).visibility });
    if (trace.length > 200) trace.shift();
  };
  for (const event of ["popstate", "hashchange", "pointerdown", "click", "transitionstart", "transitionend"])
    addEventListener(event, () => record(event), true);
  addEventListener("DOMContentLoaded", () => {
    new MutationObserver((mutations) => {
      if (mutations.some(({ target }) => target.id === "workspace-drawer")) record("drawer-mutation");
    }).observe(document.documentElement, { subtree: true, attributes: true, attributeFilter: ["class", "style"] });
  }, { once: true });
  const go = history.go.bind(history);
  window.__settingsRewind = { hold: false, pending: null };
  history.go = (by) => {
    if (window.__settingsRewind.hold && by < 0) {
      if (window.__settingsRewind.pending !== null) throw new Error("설정 되감기 중복 요청");
      window.__settingsRewind.pending = by;
      dispatchEvent(new Event("settings-rewind-requested"));
    } else go(by);
  };
  window.__releaseSettingsRewind = () => new Promise((done) => {
    const state = window.__settingsRewind;
    if (state.pending === null) throw new Error("설정 되감기 요청이 없다");
    addEventListener("popstate", done, { once: true });
    const by = state.pending;
    state.pending = null;
    state.hold = false;
    go(by);
  });
}

const painted = (p) => inTime(p.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done)))), 20_000, "설정 복귀 그리기");
const openDrawer = async (p) => {
  await p.locator(".view [data-testid='drawer-toggle']:visible").click();
  await p.locator("#workspace-drawer.is-open").waitFor();
  await drawerSettled(p, true);
};
const drawerOpen = (p) => p.locator("#workspace-drawer").evaluate((el) => el.classList.contains("is-open"));

// 머신 행 부착 뒤에도 첫 threads 응답은 늦게 올 수 있다. 빈 주소의 자동 첫 선택(choose)은
// 그때 사용자가 연 메뉴까지 닫으므로, 이 시험의 시작 목적지를 처음부터 명시한다.
function pairSettingsPage(browser, diagnostics, close, options = {}) {
  return pairDevice(browser, aaa("web", "pair").code, "iphone", {
    ...options, hash: paneHash("bk1"), init: holdSettingsRewind,
    onPage: (p) => diagnostics?.watch(p, browser.browserType().name(), close),
  });
}

// 실제 threads 응답을 메뉴 열림 뒤까지 보류한다. 첫 전체 목록 전에는 pane 행이 없으므로
// 메뉴만 먼저 열고 목록 반영 뒤 실제 줄을 누른다. 자동 첫 선택이 메뉴를 닫는지도 그대로 검사한다.
export async function settingsPanePreparation(browser, diagnostics) {
  let release, requested, delivered;
  const gate = new Promise((done) => { release = done; });
  const seen = new Promise((done) => { requested = done; });
  const sent = new Promise((done) => { delivered = done; });
  const { page: p } = await pairSettingsPage(browser, diagnostics, "preparation", {
    // Service Worker가 맡은 요청은 context.route를 지나지 않는다. gate를 쓰는 이 대조만 막는다.
    serviceWorkers: "block",
    route: ["**/api/v1/call", async (route) => {
      if (route.request().postDataJSON()?.op !== "threads") return route.continue();
      const response = await route.fetch();
      requested();
      await gate;
      await route.fulfill({ response });
      delivered();
    }],
  });
  try {
    await inTime(seen, 20_000, "첫 threads 응답 보류");
    await domIs(p, "첫 전체 목록 전 로딩과 미확정 pane 숨김", () =>
      Number(document.querySelector(".app")?.dataset.threadsSync) === 0 &&
      !!document.querySelector('[data-testid="work-list-loading"]') &&
      !document.querySelector('[data-testid="pane-bk1"]'));
    await openDrawer(p);
    diagnostics?.phase("메뉴 열림 뒤 첫 threads 응답 전달");
    release();
    await inTime(sent, 20_000, "첫 threads 응답 전달");
    await domIs(p, "첫 전체 목록 반영 뒤 pane 행", () =>
      Number(document.querySelector(".app")?.dataset.threadsSync) > 0 &&
      !!document.querySelector('[data-testid="pane-bk1"]'));
    await painted(p);
    if (!await drawerOpen(p)) throw new Error("첫 threads 목록 반영이 사용자가 연 메뉴를 닫았다");
    // inList로 메뉴를 다시 열면 자동 첫 선택의 닫기 회귀를 숨긴다. 열린 메뉴의 실제 줄만 누른다.
    diagnostics?.phase("첫 전체 목록 반영 뒤 bk1 클릭");
    await p.getByTestId("pane-bk1").click();
    await domIs(p, "늦은 threads 뒤 bk1 선택", () => document.querySelector('[data-testid="pane-bk1"]')?.getAttribute("aria-current") === "true");
    console.log(`설정 준비 ${browser.browserType().name()}: 메뉴 열림 뒤 threads 전달·bk1 클릭 완료`);
  } catch (error) {
    await diagnostics?.save(error);
    throw error;
  } finally { release(); await p.context().close(); }
}

export async function settingsDrawerHistory(browser, diagnostics) {
  const engine = browser.browserType().name();
  for (const close of ["panel", "x"]) {
    const { page: p } = await pairSettingsPage(browser, diagnostics, close);
    try {
      await inList(p, "pane-bk1");
      diagnostics?.phase("설정 열기");
      await inList(p, "settings-open");
      await p.getByRole("navigation", { name: "설정", exact: true }).getByRole("tab", { name: "모양", exact: true }).click();
      await p.getByRole("tabpanel", { name: "모양", exact: true }).waitFor();
      await painted(p);
      const depth = await p.evaluate(() => history.state?.["aaa:settings"]?.depth);
      if (depth !== 2) throw new Error(`${engine}: 휴대폰 설정 목록·쪽 기록 ${depth} ≠ 2`);
      await p.evaluate(() => { window.__settingsRewind.hold = true; });
      diagnostics?.phase("설정 닫기·내부 되감기 보류");
      if (close === "panel") {
        // 실제 목록 조작: 설정을 연 채 메뉴 → 작업 패널. 그 다음 pane 줄을 누르려는 상태다.
        await openDrawer(p);
        await p.getByTestId("activity-work").click();
      } else {
        await p.getByRole("button", { name: "설정 닫기", exact: true }).click();
      }
      await p.getByRole("region", { name: "설정", exact: true }).waitFor({ state: "detached" });
      await inTime(p.evaluate(() => new Promise((done) => {
        if (window.__settingsRewind.pending !== null) return done();
        addEventListener("settings-rewind-requested", done, { once: true });
      })), 20_000, "설정 닫기의 되감기 요청");
      if (close === "x") await openDrawer(p);
      diagnostics?.phase("내부 되감기 해제·메뉴 유지");
      if (!await drawerOpen(p)) throw new Error(`${engine} ${close}: 내부 복귀 전 사용자가 연 메뉴가 닫혔다`);
      await inTime(p.evaluate(() => window.__releaseSettingsRewind()), 20_000, "설정 내부 복귀 popstate");
      await painted(p);
      await drawerSettled(p, true);
      if (await p.evaluate(() => history.state?.["aaa:settings"] !== undefined)) throw new Error(`${engine}: 설정 내부 복귀가 끝나지 않았다`);
      await shot(p, `39-settings-rewind-${engine}-${close}`);
      if (!await drawerOpen(p)) throw new Error(`${engine} ${close}: 브라우저 이동 없이 설정 내부 복귀가 사용자가 연 메뉴를 닫았다`);
      // 재개방 도우미 없이 실제로 줄을 눌러 본다. 메뉴가 남아 있어야 다음 선택을 할 수 있다.
      await p.getByTestId("pane-bk2").click();
      diagnostics?.phase("bk2 선택 완료");
      await domIs(p, "bk2 선택", () => document.querySelector('[data-testid="pane-bk2"]')?.getAttribute("aria-current") === "true");
      await drawerSettled(p, true);
      if (await drawerOpen(p)) throw new Error(`${engine}: 목록 줄 선택 뒤 메뉴가 열려 있다`);

      // 내부 복귀의 예외가 다음 사용자 Back/Forward에 남지 않아야 한다.
      for (const direction of [-1, 1]) {
        diagnostics?.phase(direction < 0 ? "사용자 뒤로" : "사용자 앞으로");
        await openDrawer(p);
        await inTime(p.evaluate((direction) => new Promise((done) => {
          addEventListener("popstate", done, { once: true });
          if (direction < 0) history.back(); else history.forward();
        }), direction), 20_000, "사용자 뒤로·앞으로 popstate");
        const label = direction < 0 ? "bk1" : "bk2";
        await domIs(p, `${label} 선택`, (label) => document.querySelector(`[data-testid="pane-${label}"]`)?.getAttribute("aria-current") === "true", label);
        await painted(p);
        await drawerSettled(p, true);
        if (await drawerOpen(p)) throw new Error(`${engine}: 내부 복귀 뒤 사용자 이동에서 메뉴가 닫히지 않았다`);
      }
    } catch (error) {
      await diagnostics?.save(error);
      throw new Error(`${engine}/${close}: ${error.message}`, { cause: error });
    } finally {
      await p.context().close();
    }
  }
}

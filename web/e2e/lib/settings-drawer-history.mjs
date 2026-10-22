// 설정 닫기의 내부 되감기를 늦춰 사용자가 메뉴를 연 뒤 도착하는 순서를 재현한다.
// 대체하는 것은 history.go의 전달 시점뿐이다. 실제 history·popstate·제품 설정 복귀를 쓴다.
import { aaa, inList, domIs, inTime, drawerSettled, shot } from "./harness.mjs";
import { pairDevice } from "./devices.mjs";

function holdSettingsRewind() {
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

export async function settingsDrawerHistory(browser) {
  const engine = browser.browserType().name();
  for (const close of ["panel", "x"]) {
    const { page: p } = await pairDevice(browser, aaa("web", "pair").code, "iphone", { init: holdSettingsRewind });
    try {
      await inList(p, "pane-bk1");
      await inList(p, "settings-open");
      await p.getByRole("navigation", { name: "설정", exact: true }).getByRole("tab", { name: "모양", exact: true }).click();
      await p.getByRole("tabpanel", { name: "모양", exact: true }).waitFor();
      await painted(p);
      const depth = await p.evaluate(() => history.state?.["aaa:settings"]?.depth);
      if (depth !== 2) throw new Error(`${engine}: 휴대폰 설정 목록·쪽 기록 ${depth} ≠ 2`);
      await p.evaluate(() => { window.__settingsRewind.hold = true; });
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
      if (!await drawerOpen(p)) throw new Error(`${engine} ${close}: 내부 복귀 전 사용자가 연 메뉴가 닫혔다`);
      await inTime(p.evaluate(() => window.__releaseSettingsRewind()), 20_000, "설정 내부 복귀 popstate");
      await painted(p);
      await drawerSettled(p, true);
      if (await p.evaluate(() => history.state?.["aaa:settings"] !== undefined)) throw new Error(`${engine}: 설정 내부 복귀가 끝나지 않았다`);
      await shot(p, `39-settings-rewind-${engine}-${close}`);
      if (!await drawerOpen(p)) throw new Error(`${engine} ${close}: 브라우저 이동 없이 설정 내부 복귀가 사용자가 연 메뉴를 닫았다`);
      // 재개방 도우미 없이 실제로 줄을 눌러 본다. 메뉴가 남아 있어야 다음 선택을 할 수 있다.
      await p.getByTestId("pane-bk2").click();
      await domIs(p, "bk2 선택", () => document.querySelector('[data-testid="pane-bk2"]')?.getAttribute("aria-current") === "true");
      await drawerSettled(p, true);
      if (await drawerOpen(p)) throw new Error(`${engine}: 목록 줄 선택 뒤 메뉴가 열려 있다`);

      // 내부 복귀의 예외가 다음 사용자 Back/Forward에 남지 않아야 한다.
      for (const direction of [-1, 1]) {
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
    } finally {
      await p.context().close();
    }
  }
}

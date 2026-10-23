import { watchScroll } from "./scrollDiagnostics.mjs";
import { domIs, until } from "./harness.mjs";

const readScroll = (page) => page.evaluate(() => {
  const el = document.querySelector('[data-testid="terminal"]');
  const rows = document.querySelector(".xterm-rows");
  const vp = document.querySelector(".xterm-viewport");
  return {
    first: Number(/s-(\d{3})/.exec(rows.firstElementChild.textContent)?.[1]),
    rows: rows.childElementCount,
    top: Number(el.dataset.viewTop), base: Number(el.dataset.viewBase),
    nativeTop: vp.scrollTop, bottom: vp.scrollHeight - vp.clientHeight,
  };
});

export async function pageScrollRace(page) {
  await domIs(page, "재연결·resize 뒤 맨 아래 렌더", () => {
    const el = document.querySelector('[data-testid="terminal"]');
    const vp = document.querySelector(".xterm-viewport");
    return el?.dataset.viewTop === el?.dataset.viewBase && Number(el?.dataset.viewBase) > 0 &&
      vp.scrollTop >= vp.scrollHeight - vp.clientHeight - 1 && document.querySelector(".xterm-rows")?.textContent.includes("s-200");
  });
  await watchScroll(page, ".xterm-viewport");
  const before = await readScroll(page);
  // PageUp이 쓴 DOM 위치의 native scroll 사건이 xterm에 도착하기 직전에 다음 키를 처리한다.
  // scrollTop·버퍼·frame은 바꾸지 않는다. 앱의 실제 단축키 처리기만 호출해 이 순서를 고정한다.
  await page.evaluate(() => {
    const vp = document.querySelector(".xterm-viewport");
    const rows = document.querySelector(".xterm-rows").childElementCount;
    const cell = document.querySelector(".xterm-screen").getBoundingClientRect().height / rows;
    const want = Math.round((vp.scrollHeight - vp.clientHeight) / cell) - (rows - 1);
    window.__pageScrollRace = new Promise((resolve) => {
      const timer = setTimeout(() => {
        document.removeEventListener("scroll", onScroll, true);
        resolve({ error: "20000ms 안에 PageUp native scroll이 오지 않았다", want, cell, rows, states: window.takeScrollStates() });
      }, 20_000);
      const onScroll = (event) => {
        if (event.target !== vp || Math.round(vp.scrollTop / cell) !== want) return;
        clearTimeout(timer);
        document.removeEventListener("scroll", onScroll, true);
        const key = new KeyboardEvent("keydown", { key: "PageDown", code: "PageDown", ctrlKey: true, shiftKey: true, bubbles: true, cancelable: true });
        document.body.dispatchEvent(key);
        resolve({ native: event.isTrusted, taken: key.defaultPrevented, top: vp.scrollTop, want });
      };
      document.addEventListener("scroll", onScroll, true);
    });
  });
  await page.keyboard.press("Control+Shift+PageUp");
  const event = await page.evaluate(() => window.__pageScrollRace);
  if (!event.native || !event.taken) throw new Error(`native scroll과 앱 PageDown 처리 확인: ${JSON.stringify(event)}`);
  await domIs(page, "늦은 PageUp scroll 뒤 PageDown → 맨 아래", (before) => {
    const el = document.querySelector('[data-testid="terminal"]');
    const vp = document.querySelector(".xterm-viewport");
    const first = Number(/s-(\d{3})/.exec(document.querySelector(".xterm-rows").firstElementChild.textContent)?.[1]);
    return first === before.first && Number(el.dataset.viewTop) === before.base && Number(el.dataset.viewBase) === before.base &&
      vp.scrollTop >= vp.scrollHeight - vp.clientHeight - 1;
  }, before).catch(async (error) => {
    throw new Error(`${error.message}: 전 ${JSON.stringify(before)}, 사건 ${JSON.stringify(event)}, 후 ${JSON.stringify(await readScroll(page))}, 사건들 ${JSON.stringify(await page.evaluate(() => window.takeScrollStates()))}`);
  });
  // 자체 동기화 무시 표지를 무조건 소비하는 과거 구현으로 되돌리면 첫 사용자 이동을 버린다.
  // 실제 native overflow 스크롤 뒤 DOM과 xterm의 맨 위가 함께 옮겨야 한다.
  await page.locator(".xterm-viewport").evaluate((vp) => { vp.scrollTop = 0; });
  await until("사용자 scroll → 첫 줄 s-001", async () => (await readScroll(page)).first === 1);
}

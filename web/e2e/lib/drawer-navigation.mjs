// 사용자가 제시한 A → 메뉴로 B → 가장자리 뒤로. OS 미리보기 이미지는 headless가 제공하지 않는다.
// 대신 기록을 쓰는 순간의 실제 DOM 경계와 손가락/이동 사건의 순서를 따로 검사한다.
import { aaa, inList, domIs, inTime, drawerSettled, shot } from "./harness.mjs";
import { pairDevice } from "./devices.mjs";

const painted = (p) => inTime(p.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done)))), 20_000, "쓸기 뒤 그리기");
const closed = async (p, why) => {
  await painted(p);
  await drawerSettled(p, true);
  const state = await p.locator("#workspace-drawer").evaluate((el) => ({ open: el.classList.contains("is-open"), inline: el.style.transform, right: el.getBoundingClientRect().right }));
  if (state.open || state.inline || state.right > 1) throw new Error(`${why}: 메뉴가 남았다 ${JSON.stringify(state)}`);
};

// Touch 객체 생성만 호환 처리한다. 실제 제품의 document capture 듣기에 전달한다.
function stroke({ type, x, at }) {
  const target = document.querySelector(".view") ?? document.body;
  const init = { identifier: 805, target, clientX: x, clientY: 500, pageX: x, pageY: 500, screenX: x, screenY: 500 };
  const touch = typeof document.createTouch === "function" ? document.createTouch(window, target, 805, x, 500, x, 500) : new Touch(init);
  const ended = type === "touchend" || type === "touchcancel";
  const touches = typeof document.createTouchList === "function" ? document.createTouchList(...(ended ? [] : [touch])) : ended ? [] : [touch];
  const changedTouches = typeof document.createTouchList === "function" ? document.createTouchList(touch) : [touch];
  const event = new TouchEvent(type, { touches, targetTouches: touches, changedTouches, bubbles: true, cancelable: true });
  Object.defineProperty(event, "timeStamp", { value: at });
  target.dispatchEvent(event);
}

export async function drawerNavigation(browser) {
  const engine = browser.browserType().name();
  const { page: p } = await pairDevice(browser, aaa("web", "pair").code, "iphone");
  let phase = "대화 준비";
  try {
    if (await p.getByTestId("drawer-diagnostics").count()) throw new Error("일반 탭에 쓸기 진단이 나타났다");
    const url = new URL(p.url());
    url.searchParams.set("drawer-debug", "1");
    await p.goto(url.href);
    await domIs(p, "대화 준비", () => /^#pane=/.test(location.hash) && document.querySelector(".view [data-testid='drawer-toggle']"));
    await inList(p, "pane-bk1");
    await domIs(p, "A 대화 선택 완료", () => document.querySelector('[data-testid="pane-bk1"]')?.getAttribute("aria-current") === "true");
    if (await p.getByTestId("chat").count() === 0) await p.getByTestId("chat-toggle").click();
    await p.getByTestId("chat").waitFor();
    // 제품의 rAF와 별도로 실제 브라우저 frame을 관측한다. 메뉴 선택의 첫 frame에도
    // 주소·본문은 A이고 메뉴만 닫혀 있어야 한다. style 계산만 하고 즉시 쓰던 코드는 실패한다.
    await p.getByTestId("pane-bk2").evaluate((row) => row.addEventListener("click", () => {
      const hash = location.hash;
      window.__drawerPaintProbe = { before: hash, sample: null };
      requestAnimationFrame(() => {
        const drawer = document.getElementById("workspace-drawer");
        window.__drawerPaintProbe.sample = { hash: location.hash, currentA: document.querySelector('[data-testid="pane-bk1"]')?.getAttribute("aria-current") === "true",
          open: drawer.classList.contains("is-open"), right: drawer.getBoundingClientRect().right };
      });
    }, { capture: true, once: true }));
    await inList(p, "pane-bk2");
    await domIs(p, "B 대화 선택 완료", () => document.querySelector('[data-testid="pane-bk2"]')?.getAttribute("aria-current") === "true");
    if (await p.getByTestId("chat").count() === 0) await p.getByTestId("chat-toggle").click();
    await p.getByTestId("chat").waitFor();
    await closed(p, "메뉴로 B 선택");
    phase = "닫힘 frame 순서";
    const paint = await p.evaluate(() => window.__drawerPaintProbe);
    if (!paint.sample || paint.sample.hash !== paint.before || !paint.sample.currentA || paint.sample.open || paint.sample.right > 1)
      throw new Error(`${engine}: 메뉴가 없는 이전 화면의 frame 전에 기록을 썼다 ${JSON.stringify(paint)}`);
    const panel = p.getByTestId("drawer-diagnostics");
    await panel.locator("summary").click();
    await panel.getByRole("button", { name: "기록 읽기", exact: true }).click();
    const trace = JSON.parse(await panel.getByRole("textbox").inputValue());
    const writes = trace.entries.filter((entry) => entry.event === "history-write");
    if (writes.length < 2 || writes.some((entry) => entry.open || entry.right > 1))
      throw new Error(`${engine}: 메뉴 선택이 열린 화면을 기록 경계에 남겼다 ${JSON.stringify(writes)}`);
    const write = writes.at(-1);
    const ready = trace.entries.findLast((entry) => entry.event === "navigation-ready" && entry.seq < write.seq);
    const first = trace.entries.findLast((entry) => entry.event === "navigation-frame" && entry.generation === ready?.generation);
    const commit = trace.entries.findLast((entry) => entry.event === "drawer-commit" && entry.value === false && entry.seq < first?.seq);
    if (!commit || !first || !ready || !(commit.seq < first.seq && first.seq < ready.seq && ready.seq < write.seq))
      throw new Error(`${engine}: 닫힘 commit→첫 frame→다음 frame→history 순서 누락 ${JSON.stringify({ commit, first, ready, write })}`);
    await panel.locator("summary").click();

    // 정확한 사용자 순서: 왼쪽 끝에서 취소 + native Back. 실제 Safari 미리보기 검증과 구분한다.
    phase = "A로 가장자리 뒤로";
    await p.evaluate(stroke, { type: "touchstart", x: 4, at: 0 });
    await p.evaluate(stroke, { type: "touchmove", x: 250, at: 30 });
    await p.evaluate(stroke, { type: "touchcancel", x: 250, at: 40 });
    await inTime(p.evaluate(() => new Promise((done) => { addEventListener("popstate", done, { once: true }); history.back(); })), 20_000, "A로 뒤로");
    await domIs(p, "A 선택", () => document.querySelector('[data-testid="pane-bk1"]')?.getAttribute("aria-current") === "true");
    await closed(p, "A로 가장자리 뒤로");

    // 24px 밖 빠른 손가락을 브라우저가 취소하고, 이동 자체도 취소한 경우.
    phase = "빠른 몸짓 취소";
    await p.evaluate(stroke, { type: "touchstart", x: 25, at: 100 });
    await p.evaluate(stroke, { type: "touchmove", x: 61, at: 120 });
    await p.evaluate(stroke, { type: "touchcancel", x: 61, at: 130 });
    await closed(p, "빠른 몸짓 취소");

    // popstate가 먼저 닫아도 뒤따르는 touchend/move가 다시 열어서는 안 된다.
    for (const end of ["touchend", "touchcancel"]) {
      phase = `뒤로 뒤 늦은 ${end}`;
      await inList(p, "pane-bk2");
      await p.evaluate(stroke, { type: "touchstart", x: 25, at: 200 });
      await p.evaluate(stroke, { type: "touchmove", x: 250, at: 220 });
      await inTime(p.evaluate(() => new Promise((done) => { addEventListener("popstate", done, { once: true }); history.back(); })), 20_000, "쓸기 도중 뒤로");
      await p.evaluate(stroke, { type: "touchmove", x: 270, at: 230 });
      await p.evaluate(stroke, { type: end, x: 270, at: 240 });
      await closed(p, `뒤로 뒤 늦은 ${end}`);
    }

    // bfcache의 사건 경계만 주입한다. 실제 OS 복원/스냅숏을 재현했다고 하지 않는다.
    phase = "페이지 보관·복원";
    await p.locator(".view [data-testid='drawer-toggle']:visible").click();
    await p.locator("#workspace-drawer.is-open").waitFor();
    await p.evaluate(() => dispatchEvent(new PageTransitionEvent("pagehide", { persisted: true })));
    await closed(p, "pagehide 직전 정리");
    await p.evaluate(() => dispatchEvent(new PageTransitionEvent("pageshow", { persisted: true })));
    await closed(p, "pageshow 복귀");
    await panel.locator("summary").click();
    await panel.getByRole("button", { name: "정상", exact: true }).click();
    await panel.getByRole("button", { name: "미리보기만", exact: true }).click();
    await panel.getByRole("button", { name: "계속 열림", exact: true }).click();
    const final = JSON.parse(await panel.getByRole("textbox").inputValue());
    if (final.counts["result:normal"] !== 1 || final.counts["result:preview-only"] !== 1 || final.counts["result:stays-open"] !== 1 || !final.counts.touchcancel || !final.counts.popstate || !final.counts.pagehide || !final.counts.pageshow)
      throw new Error(`${engine}: 실기 진단의 사건·빈도 누락 ${JSON.stringify(final.counts)}`);
    if (final.entries.some((entry, i) => i && entry.seq <= final.entries[i - 1].seq)) throw new Error("진단 사건 순서 역전");
    await shot(p, `39-drawer-diagnostics-${engine}`);
  } catch (error) {
    throw new Error(`${engine}/${phase}: ${error.message}`, { cause: error });
  } finally {
    await p.context().close();
  }
}

// 실제 agent의 표시 정보와 구판·끊김 화면, 네 브라우저 조합의 오른쪽 패널.
import assert from "node:assert/strict";
import { aaa, step, browser, webkit, pairPage, inList, openSettings, until, shot, domIs, inTime, drawerSettled } from "../lib/harness.mjs";
import { observeHistory, assertHistoryUnchanged } from "../lib/history-observer.mjs";

// E2E39와 같은 WebKit touch 생성·document capture 경로. 목표를 실제 손가락 아래 요소로 잡아 겹침도 검사한다.
function stroke({ type, x, at }) {
  const target = window.__machineTouchTarget = type === "touchstart"
    ? document.elementFromPoint(x, 500) : window.__machineTouchTarget;
  const init = { identifier: 806, target, clientX: x, clientY: 500, pageX: x, pageY: 500, screenX: x, screenY: 500 };
  const touch = typeof document.createTouch === "function" ? document.createTouch(window, target, 806, x, 500, x, 500) : new Touch(init);
  const ended = type === "touchend" || type === "touchcancel";
  const list = items => typeof document.createTouchList === "function" ? document.createTouchList(...items) : items;
  const touches = list(ended ? [] : [touch]);
  const event = new TouchEvent(type, { touches, targetTouches: touches, changedTouches: list([touch]), bubbles: true, cancelable: true });
  Object.defineProperty(event, "timeStamp", { value: at });
  target.dispatchEvent(event);
}

async function traverse(p, direction) {
  await inTime(p.evaluate(direction => new Promise(done => {
    addEventListener("popstate", done, { once: true });
    history[direction]();
  }), direction), 20_000, `기기 정보 ${direction}`);
}

export default async function () {
  await step("휴대폰 기기 정보: 주소 한 칸·가장자리 뒤로·앞으로·반 스와이프의 목록 노출 (Chromium·WebKit 390)", async () => {
    const wk = await webkit.launch({ headless: true });
    try {
      for (const [engine, name] of [[browser, "chromium"], [wk, "webkit"]]) {
        const p = (await pairPage(engine, aaa("web", "pair").code, { width: 390, height: 844 }, { hash: "#threads" })).page;
        const observer = await observeHistory(p);
        try {
          const original = await observer.snapshot();
          await inList(p, p.getByRole("button", { name: "labA 기기 정보", exact: true, includeHidden: true }));
          const panel = p.getByTestId("machine-info-panel");
          await panel.waitFor();
          const id = aaa("machine", "list").machines.find(m => m.name === "labA").id;
          const hash = `#machine=${encodeURIComponent(id)}`;
          assert.equal(await p.evaluate(() => location.hash), hash);
          const opened = await observer.snapshot();
          assert.equal(opened.length, original.length + 1);
          assert.equal(await panel.evaluate(el => el.closest(".view-content") !== null), true);
          assert.equal(await panel.evaluate(el => getComputedStyle(el).position), "relative");
          await drawerSettled(p, true);
          for (let i = 0; i < 2; i++) {
            await p.evaluate(stroke, { type: "touchstart", x: 4, at: 0 });
            await p.evaluate(stroke, { type: "touchmove", x: 300, at: 30 });
            await p.evaluate(stroke, { type: "touchcancel", x: 300, at: 40 });
            await traverse(p, "back");
            await domIs(p, "기기 정보 이전 화면", () => location.hash === "#threads" && !document.querySelector('[data-testid="machine-info-panel"]'));
            await traverse(p, "forward");
            await panel.waitFor();
            assert.equal(await p.evaluate(() => location.hash), hash);
            await drawerSettled(p, true);
          }
          assertHistoryUnchanged(opened, await observer.snapshot());

          // 손가락을 놓기 전 중간 위치와 실제 위에 보이는 목록을 검사한다(단순 is-open 검사가 아니다).
          await p.evaluate(stroke, { type: "touchstart", x: 40, at: 100 });
          await p.evaluate(stroke, { type: "touchmove", x: 180, at: 500 });
          const preview = await p.locator("#workspace-drawer").evaluate(el => ({
            right: el.getBoundingClientRect().right,
            follows: el.style.transition === "none",
            onTop: el.contains(document.elementFromPoint(100, 500)),
          }));
          assert.equal(Math.round(preview.right), 140);
          assert.equal(preview.follows, true);
          assert.equal(preview.onTop, true, "기기 정보가 목록 미리보기를 가리면 안 된다");
          await shot(p, `machine-info-${name}-390-half-swipe`);
          await p.evaluate(stroke, { type: "touchcancel", x: 180, at: 510 });
          await drawerSettled(p, true);
          assertHistoryUnchanged(opened, await observer.snapshot());
          await panel.getByTestId("drawer-toggle").click();
          await p.locator("#workspace-drawer.is-open").waitFor();
          await panel.getByTestId("drawer-toggle").click();
          await domIs(p, "기기 정보 목록 닫힘", () => !document.querySelector("#workspace-drawer.is-open"));
          await drawerSettled(p, true);
          await p.reload();
          await panel.getByRole("heading", { name: "labA", exact: true }).waitFor();
          assert.equal(await p.evaluate(() => location.hash), hash);
          await panel.getByRole("button", { name: "기기 정보 닫기" }).click();
          await panel.waitFor({ state: "detached" });
        } finally { await observer.close(); await p.context().close(); }
      }
    } finally { await wk.close(); }
  });

  await step("기기 정보: 실제 수집·판 표시·오른쪽 패널·휴대폰·Esc·전환·구판·끊김 (Chromium·WebKit 1280·390)", async () => {
    const info = await until("실제 agent 정보", () => aaa("machine", "list").machines.find(m => m.name === "labA")?.health?.info);
    const wk = await webkit.launch({ headless: true });
    try {
      for (const [engine, name] of [[browser, "chromium"], [wk, "webkit"]]) {
        for (const width of [1280, 390]) {
          const route = ws => {
            const server = ws.connectToServer();
            ws.onMessage(m => server.send(m));
            server.onMessage(m => {
              if (typeof m === "string") {
                const e = JSON.parse(m);
                if (e.ev === "state") {
                  const real = e.machines.find(m => m.name === "labA");
                  e.machines.push({ ...real, id: "info-old", name: "old-agent", local: false, connection: "disconnected", health: null, software: null });
                  e.machines.push({ ...real, id: "info-offline", name: "offline-agent", local: false, connection: "disconnected" });
                  m = JSON.stringify(e);
                }
              }
              ws.send(m);
            });
          };
          const p = (await pairPage(engine, aaa("web", "pair").code, { width, height: 844 }, { hash: "#threads", wsRoute: [/\/api\/v1\/events/, route] })).page;
          try {
            const open = async machine => {
              await inList(p, p.getByRole("button", { name: `${machine} 기기 정보`, exact: true, includeHidden: true }));
              await p.getByRole("heading", { name: machine, exact: true }).waitFor();
            };
            const panel = p.getByTestId("machine-info-panel");
            const before = await p.evaluate(() => location.hash);
            await open("labA");
            if (width === 1280) {
              assert.equal(await p.evaluate(() => location.hash), before, "데스크톱은 원래 주소의 오른쪽 패널을 유지한다");
              assert.equal(await panel.evaluate(el => getComputedStyle(el).position), "fixed");
            }
            const box = await panel.boundingBox();
            assert.equal(Math.round(box.x + box.width), width);
            assert.equal(Math.round(box.width), width === 390 ? 390 : 420);
            assert.equal(await panel.locator('[data-label="작업 폴더"] dd').textContent(), info.work_root);
            assert.equal(await panel.locator('[data-label="CPU 모델"] dd').textContent(), info.cpu_model ?? "모름");
            assert.equal(await panel.locator('[data-label="Claude Code"] dd').textContent(), info.claude ?? "모름");
            assert.equal(await panel.locator('[data-label="Codex"] dd').textContent(), info.codex ?? "모름");
            assert.notEqual(await panel.locator('[data-label="정보 수집"] dd').textContent(), "모름");
            await p.keyboard.press("Escape");
            await panel.waitFor({ state: "detached" });
            for (const palette of ["amber", "catppuccin", "github-dark"]) {
              const settings = await openSettings(p, "모양");
              await settings.locator("#settings-palette").selectOption(palette);
              await settings.getByRole("button", { name: "설정 닫기" }).click();
              await open("labA");
              assert.equal(await panel.evaluate(el => el.scrollWidth > el.clientWidth), false);
              await shot(p, `machine-info-${name}-${width}-${palette}`);
              await panel.getByRole("button", { name: "기기 정보 닫기" }).click();
            }
            await open("labA");
            if (width === 1280) {
              // 패널을 닫지 않고 왼쪽 목록의 다른 머신을 고른다.
              await p.getByRole("button", { name: "old-agent 기기 정보", exact: true }).click();
            } else {
              await panel.getByRole("button", { name: "기기 정보 닫기" }).click();
              await open("old-agent");
            }
            await panel.getByRole("heading", { name: "old-agent", exact: true }).waitFor();
            assert.equal(await panel.locator('[data-label="Codex"] dd').textContent(), "모름");
            await panel.getByText("연결이 끊겼습니다. 마지막으로 받은 정보입니다.").waitFor();
            await panel.getByRole("button", { name: "기기 정보 닫기" }).click();
            await open("offline-agent");
            assert.notEqual(await panel.locator('[data-label="정보 수집"] dd').textContent(), "모름");
            assert.notEqual(await panel.locator('[data-label="작업 폴더"] dd').textContent(), "모름");
            await panel.getByRole("button", { name: "기기 정보 닫기" }).click();
          } finally { await p.context().close(); }
        }
      }
    } finally { await wk.close(); }
  });
}

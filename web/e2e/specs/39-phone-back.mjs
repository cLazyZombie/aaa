// 웹 E2E 휴대폰 뒤로 기록(사용자 10-20 「아이폰에서 왼쪽 스와이프를 처음부터 끝까지 하면 저 화면이 나와」, plan §8.301).
// iPhone(WebKit 390·430, Safari 탭·홈 화면 앱 흉내)에서 pane을 열고 다른 pane으로 가고 보고서를 연 뒤 브라우저 뒤로(iOS 가장자리
// 쓸기와 같은 history.back)를 끝까지 하면, 열어 본 화면을 거꾸로 지나 첫 pane에서 멈추고 빈 「하나를 고른다」로 떨어지지 않는다.
// 왼쪽 가장자리에서 오른쪽 끝까지 쓸기는 Safari의 가장자리 뒤로 몫이다. 앱의 목록 drawer 쓸기(herdr/edgeSwipe.ts)는 화면 안쪽에서
// 시작한 쓸기만 받는다(plan §8.408, 전에는 두 몸짓이 함께 돌아 뒤로 간 화면에 목록이 열려 남았다). 가장자리 쓸기를 따로, 그리고
// 뒤로와 겹쳐(시스템이 손가락을 가져가 touchcancel, 그 뒤 뒤로) 흉내 낸다. 손가락은 WebKit의 document.createTouch로 만든다.
// 실제 iOS가 가장자리 뒤로 몸짓에서 페이지에 어떤 터치 사건을 주는지는 헤드리스가 보이지 않는다(실기 확인).
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 39-phone-back(새 실험실).

import fs from "node:fs";
import { aaa, step, webkit, inList, showPanel, drawerSettled, domIs, inTime, shot, root, AA, base } from "../lib/harness.mjs";
import { pairDevice } from "../lib/devices.mjs";

/** 보고서 링크가 든 Claude 대화 pane(38-chat-report-close와 같은 길: hook으로 기록 자리를 알린다) */
function chatPane(label, text) {
  const dir = `${root}/claude/projects/-lab-${label}`;
  fs.mkdirSync(dir, { recursive: true });
  const tr = `${dir}/s1.jsonl`;
  const L = (o) => JSON.stringify(o) + "\n";
  fs.writeFileSync(tr, L({ type: "user", message: { role: "user", content: "보고서는?" } }) + L({ type: "assistant", message: { content: [{ type: "text", text }] } }));
  aaa("tab", "create", "--machine", "labA", "--label", label, "--", "/bin/sh", "-c", '"$0" hook --agent claude "$1"; exec cat', AA,
    JSON.stringify({ hook_event_name: "SessionStart", session_id: "6a5b4c3d-2e1f-4a0b-9c8d-7e6f5a4b3c39", source: "startup", transcript_path: tr }));
}

/** 지금 화면: 주소, 빈 화면인지, 파일 보기, 목록 drawer, 기록 칸 수 */
const screen = (p) =>
  p.evaluate(() => ({
    hash: location.hash,
    empty: document.querySelector(".empty-view") !== null,
    viewer: document.querySelector('[data-testid="file-viewer"]') !== null,
    drawer: document.getElementById("workspace-drawer")?.classList.contains("is-open") === true,
    len: history.length,
  }));

/** 앱이 그 사건을 다 그렸다: 두 frame 뒤(React가 상태를 그리고 effect가 돈 뒤) */
const painted = (p) => inTime(p.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(() => r())))), 20_000, "두 frame");

/** 브라우저 뒤로(iOS 가장자리 쓸기의 뒤로와 같다): 그 popstate가 온 뒤 앱이 그린 화면 */
async function back(p) {
  await inTime(p.evaluate(() => new Promise((r) => { addEventListener("popstate", () => r(), { once: true }); history.back(); })), 20_000, "뒤로의 popstate");
  await painted(p);
  return screen(p);
}

/**
 * 왼쪽 가장자리(x 4, `from`을 주면 그 자리)에서 오른쪽 끝까지 한 손가락. `end`가 touchend면 손을 뗀 것, touchcancel이면
 * 시스템(Safari 가장자리 뒤로)이 손가락을 가져간 것이다. `back`이면 손가락을 뗀 뒤 그 몸짓의 뒤로가 온다(iOS는 손을 떼면 이동한다).
 */
async function edgeStroke(p, { from = 4, end = "touchend", back: after = false } = {}) {
  await inTime(
    p.evaluate(
      ({ from, end, after }) =>
        new Promise((resolve) => {
          const y = 500;
          const target = document.elementFromPoint(from, y) ?? document.body;
          const fire = (type, x) => {
            const t = document.createTouch(window, target, 1, x, y, x, y);
            const on = type === "touchend" || type === "touchcancel" ? document.createTouchList() : document.createTouchList(t);
            target.dispatchEvent(new TouchEvent(type, { touches: on, targetTouches: on, changedTouches: document.createTouchList(t), bubbles: true, cancelable: true }));
          };
          const goBack = () => new Promise((r) => { addEventListener("popstate", () => r(), { once: true }); history.back(); });
          (async () => {
            fire("touchstart", from);
            const width = innerWidth;
            for (let i = 1; i <= 12; i++) {
              fire("touchmove", from + ((width - 4 - from) * i) / 12);
              await new Promise((r) => requestAnimationFrame(() => r()));
            }
            fire(end, width - 4);
            if (after) await goBack();
            resolve();
          })();
        }),
      { from, end, after },
    ),
    20_000,
    "가장자리 쓸기",
  );
  await drawerSettled(p, true);
  await painted(p);
  return screen(p);
}

const paneHashOf = (label) => `#pane=${encodeURIComponent(aaa("pane", "list").panes.find((x) => x.name === label).id)}`;
/** 이 페이지(앱 문서)가 열린 기록 칸: 그 앞은 헤드리스의 about:blank, 실기의 앞 페이지다. 뒤로 「끝까지」는 이 칸까지다 */
const pageEntry = () => {
  if (window === window.top) Object.defineProperty(window, "__aaaPageEntry", { value: history.length });
};
const fmt = (s) => `${s.hash || "(주소 없음)"}${s.viewer ? "+보기" : ""}${s.drawer ? "+목록" : ""}${s.empty ? "+빈 화면" : ""}`;

export default async function () {
  let url;
  await step("준비: 보고서 publish, 보고서 링크가 든 대화 pane과 셸 pane 둘", async () => {
    aaa("project", "create", "demo");
    const input = `${root}/report-39`;
    fs.mkdirSync(input, { recursive: true });
    fs.writeFileSync(`${input}/index.html`, "<!doctype html><html lang=ko><meta charset=utf-8><h1>뒤로 보고서</h1></html>");
    url = new URL(aaa("library", "publish", input, "--project", "demo", "--name", "backrep", "--to-user").base, base).href;
    chatPane("bk1", `보기: ${url}`);
    for (const label of ["bk2", "bk3", "bk4"]) aaa("tab", "create", "--machine", "labA", "--label", label, "--", "/bin/sh", "-c", "exec cat");
  });

  await step("휴대폰 뒤로 끝까지: pane → 다른 pane → 보고서 → 뒤로를 거꾸로 지나 첫 pane에서 멈추고 빈 화면이 없다(WebKit 390·430, Safari·홈 화면 앱)", async () => {
    const wk = await webkit.launch();
    try {
      for (const width of [390, 430]) for (const standalone of [false, true]) {
        const tag = `${width}-${standalone ? "app" : "safari"}`;
        const { page: p } = await pairDevice(wk, aaa("web", "pair").code, "iphone", { standalone, init: pageEntry });
        try {
          if (width !== 390) await p.setViewportSize({ width, height: 932 });
          // 첫 화면: 앱이 스스로 고른 pane(목록 첫 줄)
          await domIs(p, "첫 pane", () => /^#pane=/.test(location.hash) && document.querySelector('[data-testid="terminal"], [data-testid="chat"]') !== null);
          const first = await screen(p);
          const bottom = await p.evaluate(() => window.__aaaPageEntry);
          const visited = [first.hash];
          for (const label of ["bk3", "bk2", "bk1"]) {
            await inList(p, `pane-${label}`);
            await domIs(p, `${label} 주소`, (h) => location.hash === h, paneHashOf(label));
            const h = paneHashOf(label);
            if (visited.at(-1) !== h) visited.push(h);
          }
          // bk1의 대화에서 보고서를 연다(파일 보기, 기록 한 칸: plan §8.300)
          if ((await p.getByTestId("chat").count()) === 0) await p.getByTestId("chat-toggle").click();
          await p.getByTestId("chat").getByRole("link", { name: url }).first().click();
          await p.getByTestId("file-viewer").waitFor();
          const top = await screen(p);
          const seen = [fmt(top)];
          // 앱 문서의 칸까지 뒤로 끝까지 간다. 앱이 스스로 고른 첫 pane은 칸을 더하지 않아(그 칸을 바꾼다) 끝은 첫 pane이다
          const pushed = top.len - bottom;
          for (let i = 0; i < pushed; i++) {
            const s = await back(p);
            seen.push(fmt(s));
            if (s.empty) await shot(p, `phone-back-${tag}-empty`);
          }
          const want = [`${visited.at(-1)}+보기`, ...visited.slice().reverse()];
          if (seen.join(" ← ") !== want.join(" ← ") || seen.some((s) => s.includes("빈 화면")))
            throw new Error(`${tag}: 뒤로 화면 차례 ${seen.join(" ← ")} ≠ ${want.join(" ← ")} (첫 칸 ${bottom}, 위 칸 ${top.len})`);
          await shot(p, `phone-back-${tag}-bottom`);
        } finally {
          await p.context().close();
        }
      }
    } finally {
      await wk.close();
    }
  });

  // t-0541, plan §8.340: 메뉴 상태는 창별 기록이 아니라 이동 직전의 상태다.
  await step("메뉴를 연 채 뒤로·앞으로: 이동 직전의 열림·닫힘 유지(WebKit 390·430, Safari·홈 화면 앱)", async () => {
    const wk = await webkit.launch();
    const bad = [];
    try {
      for (const width of [390, 430]) for (const standalone of [false, true]) {
        const tag = `${width}-${standalone ? "app" : "safari"}`;
        const { page: p } = await pairDevice(wk, aaa("web", "pair").code, "iphone", { standalone });
        try {
          if (width !== 390) await p.setViewportSize({ width, height: 932 });
          const hashes = Object.fromEntries(["bk1", "bk2", "bk3"].map((label) => [label, paneHashOf(label)]));
          // 1→2→3은 목록에서 직접 고른다: 줄 선택은 지금처럼 메뉴를 닫고 기록을 한 칸씩 만든다.
          for (const label of ["bk1", "bk2", "bk3"]) {
            await inList(p, `pane-${label}`);
            await domIs(p, `${label} 선택`, (label) => document.querySelector(`[data-testid="pane-${label}"]`)?.getAttribute("aria-current") === "true", label);
          }
          await p.locator(".view [data-testid='drawer-toggle']").click();
          await p.locator("#workspace-drawer.is-open").waitFor();
          await drawerSettled(p, true);
          await shot(p, `phone-menu-${tag}-3-open`);
          // 시스템 쓸기의 실제 이동은 history 순회다. popstate 뒤 선택된 줄(hashchange 처리 완료)과 drawer 움직임을 기다린다.
          const move = async (direction, label, open) => {
            await inTime(p.evaluate((direction) => new Promise((r) => {
              addEventListener("popstate", () => r(), { once: true });
              history.go(direction);
            }), direction), 20_000, "쓸기 이동의 popstate");
            await domIs(p, `${label} 선택`, (label) => document.querySelector(`[data-testid="pane-${label}"]`)?.getAttribute("aria-current") === "true", label);
            await drawerSettled(p, true);
            const s = await screen(p);
            if (s.hash !== hashes[label] || s.drawer !== open || s.empty)
              bad.push(`${tag} ${direction < 0 ? "뒤로" : "앞으로"}: ${fmt(s)} (원함 ${hashes[label]}, 메뉴 ${open ? "열림" : "닫힘"})`);
          };
          const len = (await screen(p)).len;
          // 메뉴가 이미 열린 상태에서 왼쪽 가장자리 쓸기 + Safari 뒤로(touchcancel)도 같은 규칙이다.
          const swiped = await edgeStroke(p, { end: "touchcancel", back: true });
          await domIs(p, "bk2 선택", () => document.querySelector('[data-testid="pane-bk2"]')?.getAttribute("aria-current") === "true");
          if (swiped.hash !== hashes.bk2 || !swiped.drawer || swiped.empty)
            bad.push(`${tag} 메뉴 열고 쓸기 뒤로: ${fmt(swiped)} (원함 ${hashes.bk2}, 메뉴 열림)`);
          await shot(p, `phone-menu-${tag}-2-back`);
          // 창마다 기억하지 않는다: 메뉴가 닫혔던 1·2·3으로 돌아가도 지금 열어 둔 상태를 쓴다.
          await move(-1, "bk1", true);
          await move(1, "bk2", true);
          await move(1, "bk3", true);
          await shot(p, `phone-menu-${tag}-3-forward`);
          // 메뉴를 닫은 뒤에는 열려 있던 창으로 뒤로·앞으로 가도 닫힌 채다.
          if ((await screen(p)).drawer) await p.locator(".scrim.is-open").click({ position: { x: width - 10, y: 600 } });
          await drawerSettled(p, true);
          await move(-1, "bk2", false);
          await move(1, "bk3", false);
          if ((await screen(p)).len !== len) bad.push(`${tag}: 메뉴 토글·기록 순회가 기록 칸 수를 바꿨다`);
        } finally {
          await p.context().close();
        }
      }
      if (bad.length) throw new Error(bad.join("; "));
    } finally {
      await wk.close();
    }
  });

  await step("왼쪽 가장자리에서 오른쪽 끝까지 쓸기는 Safari 뒤로 몫: 목록을 열지 않고, 뒤로와 겹치면 앞 화면에 목록 없이 간다. 안쪽 쓸기는 목록을 연다(plan §8.408)", async () => {
    const wk = await webkit.launch();
    try {
      for (const width of [390, 430]) for (const standalone of [false, true]) {
        const tag = `${width}-${standalone ? "app" : "safari"}`;
        const { page: p } = await pairDevice(wk, aaa("web", "pair").code, "iphone", { standalone });
        try {
          if (width !== 390) await p.setViewportSize({ width, height: 932 });
          await domIs(p, "첫 pane", () => /^#pane=/.test(location.hash) && document.querySelector('[data-testid="terminal"], [data-testid="chat"]') !== null);
          const bad = [];
          // 1. 가장자리 쓸기만(뒤로 칸이 없거나 시스템이 가져가지 않은 손가락): 목록이 열리지 않고 주소·기록 그대로
          const before = await screen(p);
          const edge = await edgeStroke(p);
          if (edge.drawer || edge.hash !== before.hash || edge.len !== before.len || edge.empty)
            bad.push(`가장자리 쓸기 ${fmt(before)} → ${fmt(edge)} (기록 ${before.len} → ${edge.len})`);
          // 2. 안쪽(x 60)에서 쓸기: 목록이 열리고 주소·기록 그대로
          const inner = await edgeStroke(p, { from: 60 });
          if (!inner.drawer || inner.hash !== before.hash || inner.len !== before.len || inner.empty)
            bad.push(`안쪽 쓸기 ${fmt(before)} → ${fmt(inner)} (기록 ${before.len} → ${inner.len})`);
          await shot(p, `phone-edge-${tag}-drawer`);
          await p.locator(".scrim.is-open").click({ position: { x: width - 10, y: 600 } });
          await drawerSettled(p, true);
          // 다른 pane 둘을 열어 뒤로 칸을 만든다
          await inList(p, "pane-bk3");
          await inList(p, "pane-bk2");
          const h3 = paneHashOf("bk3");
          const h2 = paneHashOf("bk2");
          await domIs(p, "bk2 주소", (h) => location.hash === h, h2);
          // 3. 겹침(시스템이 손가락을 가져감 → touchcancel, 그 뒤 뒤로): 앞 pane으로, 목록은 닫힌 채(사용자가 본 결함)
          const a = await edgeStroke(p, { end: "touchcancel", back: true });
          if (a.hash !== h3 || a.drawer || a.empty) bad.push(`취소 뒤 뒤로: ${fmt(a)} (원함 ${h3}, 목록 닫힘)`);
          await shot(p, `phone-edge-${tag}-back`);
          await inList(p, "pane-bk2");
          // 4. 가장자리 쓸기가 끝난 뒤 그 몸짓의 뒤로가 온다: 뒤로 하나만, 목록 닫힘
          const c = await edgeStroke(p, { back: true });
          if (c.hash !== h3 || c.drawer || c.empty) bad.push(`쓸기 끝난 뒤 뒤로: ${fmt(c)} (원함 ${h3}, 목록 닫힘)`);
          if (bad.length) throw new Error(`${tag}: ${bad.join("; ")}`);
        } finally {
          await p.context().close();
        }
      }
    } finally {
      await wk.close();
    }
  });

  await step("넓은 화면 뒤로·앞으로: 데스크톱·패드의 사이드바 펼침·접힘 유지(WebKit 1280·820)", async () => {
    const wk = await webkit.launch();
    try {
      for (const profile of ["desktop", "ipad"]) {
        const { page: p } = await pairDevice(wk, aaa("web", "pair").code, profile);
        try {
          for (const label of ["bk1", "bk2"]) await inList(p, `pane-${label}`);
          await showPanel(p, "work");
          for (const collapsed of [false, true]) {
            if (collapsed) {
              await p.getByTestId("activity-work").click();
              await p.locator(".app.sidebar-collapsed").waitFor();
            }
            for (const [direction, label] of [[-1, "bk1"], [1, "bk2"]]) {
              await inTime(p.evaluate((direction) => new Promise((r) => {
                addEventListener("popstate", () => r(), { once: true });
                history.go(direction);
              }), direction), 20_000, "넓은 화면 popstate");
              await domIs(p, `${label} 선택`, (label) => document.querySelector(`[data-testid="pane-${label}"]`)?.getAttribute("aria-current") === "true", label);
              const actual = await p.locator(".app").evaluate((el) => el.classList.contains("sidebar-collapsed"));
              if (actual !== collapsed || (await screen(p)).empty)
                throw new Error(`${profile}: 뒤로·앞으로 사이드바 접힘 ${actual} ≠ ${collapsed}`);
            }
          }
        } finally {
          await p.context().close();
        }
      }
    } finally {
      await wk.close();
    }
  });

  await step("고른 것이 없는 화면: 휴대폰은 「목록에서 하나를 고른다」와 「목록 열기」 단추, 기록 칸을 더하지 않고 뒤로는 앞 화면, 데스크톱은 「왼쪽에서」 그대로", async () => {
    const wk = await webkit.launch();
    try {
      const { page: p } = await pairDevice(wk, aaa("web", "pair").code, "iphone", { standalone: false });
      try {
        await domIs(p, "첫 pane", () => /^#pane=/.test(location.hash) && document.querySelector('[data-testid="terminal"], [data-testid="chat"]') !== null);
        // 닫힌 pane의 칸에 뒤로가 닿으면 지금 pane에 머물고 그렇다고 말한다(참고 onPop: pane이 없으면 그대로)
        await inList(p, "pane-bk4");
        const h4 = paneHashOf("bk4");
        await domIs(p, "bk4 주소", (h) => location.hash === h, h4);
        await inList(p, "pane-bk2");
        const h2 = paneHashOf("bk2");
        await domIs(p, "bk2 주소", (h) => location.hash === h, h2);
        aaa("pane", "close", "bk4");
        await p.getByTestId("pane-bk4").waitFor({ state: "detached" });
        const gone = await back(p);
        if (gone.hash !== h2 || gone.empty) throw new Error(`닫힌 pane 칸으로 뒤로: ${fmt(gone)} (원함 ${h2})`);
        await domIs(p, "닫힌 pane 알림", () => document.querySelector('[data-testid="head-note"]')?.textContent === "그 pane은 닫혔습니다");
        const pane = await screen(p);
        // 모든 thread 화면을 열고(기록 한 칸) 닫는다: 고른 것 없음은 그 칸을 바꾼다
        await p.evaluate(() => (location.hash = "threads"));
        await p.getByTestId("all-threads").waitFor();
        const pushed = (await screen(p)).len;
        await p.getByTestId("all-threads").getByRole("button", { name: "닫기" }).click();
        const view = p.locator(".empty-view");
        await view.waitFor();
        const head = await view.locator("h2").textContent();
        if (head !== "목록에서 하나를 고른다") throw new Error(`휴대폰 빈 화면 제목 「${head}」`);
        const open = p.getByTestId("empty-open-list");
        const box = await open.boundingBox();
        if (!box || box.height < 44 || box.x < 0 || box.x + box.width > 390) throw new Error(`목록 열기 단추 ${JSON.stringify(box)}`);
        await shot(p, "phone-empty-list");
        const closed = await screen(p);
        if (closed.len !== pushed || closed.hash !== "") throw new Error(`고른 것 없음이 기록 칸을 더했다(${pushed} → ${closed.len}, 주소 ${closed.hash})`);
        await open.click();
        await p.locator("#workspace-drawer.is-open").waitFor();
        await drawerSettled(p, true);
        await p.locator(".scrim.is-open").click({ position: { x: 380, y: 600 } });
        await drawerSettled(p, true);
        // 뒤로: 모든 thread 칸이던 곳(이제 빈 칸)에 머물지 않고 그 앞 pane으로
        const s = await back(p);
        if (s.hash !== pane.hash || s.empty) throw new Error(`빈 화면에서 뒤로: ${fmt(s)} (원함 ${pane.hash})`);
      } finally {
        await p.context().close();
      }
      const { page: d } = await pairDevice(wk, aaa("web", "pair").code, "desktop");
      try {
        await domIs(d, "첫 pane", () => /^#pane=/.test(location.hash));
        await d.evaluate(() => (location.hash = "threads"));
        await d.getByTestId("all-threads").getByRole("button", { name: "닫기" }).click();
        const head = await d.locator(".empty-view h2").textContent();
        if (head !== "왼쪽에서 하나를 고른다" || (await d.getByTestId("empty-open-list").count()) !== 0) throw new Error(`데스크톱 빈 화면 「${head}」`);
      } finally {
        await d.context().close();
      }
    } finally {
      await wk.close();
    }
  });
}

// 웹 E2E 「답할 것」 화면 닫기·막대 자리(사용자 10-21 07:4x iPhone 캡처, plan §8.359). 「답할 것」을 연 채 대화·터미널 pane을 부르면
// 위 절반에 「답할 것」이 남고 아래 절반에 pane이 떠 닫을 수 없었다: 숨긴 「답할 것」(is-hidden)이 보고서·할 일 화면처럼 본문 칸 위에
// 숨지 않았다. 보고서 화면과 같은 규칙인지 본다: 다른 화면을 고르면 본문 전체가 그 화면, 휴대폰 뒤로는 「답할 것」으로 돌아가고 다시
// 뒤로는 그 앞 화면. 막대에서 「답할 것」은 맨 위가 아니라 패널 단추 끝, 보고서 단추 바로 위다.
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 62-asks-close(새 실험실).

import fs from "node:fs";
import { root, AA, aaa, step, browser, webkit, pairPage, inList, domIs, drawerSettled, inTime, shot } from "../lib/harness.mjs";
import { pairDevice } from "../lib/devices.mjs";

/**
 * 본문 칸(.view)을 무엇이 차지하나: 칸 가운데·위 1/4·아래 1/4 세 점에서 맨 위 요소가 든 화면(답할 것·보고서·pane·그 밖), 주소,
 * 두 화면의 data-shown, 그리고 pane 머리의 위쪽 끝(본문 칸 위쪽 끝과의 차). 반으로 나뉘면 위 점은 「답할 것」, 아래 점은 pane이다
 */
const viewIs = (p) =>
  p.evaluate(() => {
    const view = document.querySelector(".view").getBoundingClientRect();
    const owner = (y) => {
      const el = document.elementFromPoint(view.left + view.width / 2, y);
      if (!el) return "없음";
      if (el.closest('[data-testid="asks-view"]')) return "답할 것";
      if (el.closest('[data-testid="reports-view"]')) return "보고서";
      if (el.closest(".term-view, [data-testid='terminal'], [data-testid='chat']") || el.closest(".view > :not(.asks-view):not(.reports-view)")) return "pane";
      return el.className || el.tagName;
    };
    const asks = document.querySelector('[data-testid="asks-view"]');
    const ask = asks?.getBoundingClientRect();
    return {
      hash: location.hash,
      top: owner(view.top + view.height / 4),
      mid: owner(view.top + view.height / 2),
      bottom: owner(view.top + (view.height * 3) / 4),
      asks: asks?.dataset.shown ?? "없음",
      asksVisible: asks ? getComputedStyle(asks).visibility : "없음",
      asksInFlow: ask ? Math.round(ask.height) > 0 && getComputedStyle(asks).position !== "absolute" : false,
      reports: document.querySelector('[data-testid="reports-view"]')?.dataset.shown ?? "없음",
    };
  });
const fmt = (s) => JSON.stringify(s);
/** 본문 전체가 한 화면이다(세 점이 같다) */
const whole = (s, what) => s.top === what && s.mid === what && s.bottom === what;
/** 주소 순회를 App이 반영했다는 제품 표지를 기다린다. */
const selected = (p) => domIs(p, "주소에 맞는 본문", () => {
  const asks = /^#(?:asks|ask=|question=)/.test(location.hash);
  return asks
    ? document.querySelector('[data-testid="asks-view"]')?.dataset.shown === "true"
    : document.querySelector('[data-testid="asks-view"]')?.dataset.shown !== "true"
      && document.querySelector('[aria-current="true"][data-testid^="pane-"]') !== null;
});
/** 브라우저 뒤로(iOS 가장자리 쓸기의 뒤로와 같다): popstate가 온 뒤 앱이 그린 화면 */
async function back(p) {
  await inTime(p.evaluate(() => new Promise((r) => { addEventListener("popstate", () => r(), { once: true }); history.back(); })), 20_000, "뒤로의 popstate");
  await selected(p);
  return viewIs(p);
}
/** 확인 필요 줄을 눌러 「답할 것」 카드를 연다(휴대폰은 목록 drawer의 「답할 것」 패널에서) */
async function openAsk(p) {
  await inList(p, "activity-asks");
  await p.locator('#sidebar-column [data-panel="asks"]:not([hidden])').waitFor();
  const row = p.locator('[data-testid="ask-row"][data-key^="r:"]');
  await row.waitFor();
  await inList(p, row);
  await domIs(p, "「답할 것」 카드", () => /^#ask=r/.test(location.hash) && document.querySelector('[data-testid="asks-view"]')?.dataset.shown === "true"
    && document.querySelector('[data-testid="ask-card"][data-kind="request"]') !== null);
  await selected(p);
}
/** pane을 부른다(목록 「작업」의 그 줄): 그 pane 주소와 본문을 기다린다 */
async function openPane(p, label) {
  await inList(p, `pane-${label}`);
  await domIs(p, `${label} pane`, (label) => document.querySelector(`[data-testid="pane-${label}"]`)?.getAttribute("aria-current") === "true"
    && document.querySelector('[data-testid="terminal"], [data-testid="chat"]') !== null, label);
  await selected(p);
}

export default async function () {
  await step("준비: 확인 필요 post 하나(「답할 것」 줄)와 셸·대화 pane", async () => {
    aaa("project", "create", "demo");
    const body = `${root}/request-62.md`;
    fs.writeFileSync(body, "닫기 시험 확인 필요\n");
    aaa("post", "demo", "user", "--request", "--needs-user", "--topic", "user-action", "--title", "닫기 시험", "--body-file", body);
    aaa("tab", "create", "--machine", "labA", "--label", "ak1", "--", "/bin/sh", "-c", "exec cat");
    const dir = `${root}/claude/projects/-lab-ak2`;
    fs.mkdirSync(dir, { recursive: true });
    const transcript = `${dir}/s.jsonl`;
    fs.writeFileSync(transcript, JSON.stringify({ type: "assistant", message: { content: [{ type: "text", text: "대화로 돌아왔습니다." }] } }) + "\n");
    aaa("tab", "create", "--machine", "labA", "--label", "ak2", "--", "/bin/sh", "-c", '\"$0\" hook --agent claude \"$1\"; exec cat', AA,
      JSON.stringify({ hook_event_name: "SessionStart", session_id: "6a5b4c3d-2e1f-4a0b-9c8d-7e6f5a4b3c62", source: "startup", transcript_path: transcript }));
  });

  await step("휴대폰 「답할 것」을 연 채 pane을 부르면 본문 전체가 pane이고(반으로 나뉘지 않음, 보고서 화면과 같음), 뒤로는 「답할 것」 → 그 앞 pane(WebKit 390·430, Safari·홈 화면 앱)", async () => {
    const wk = await webkit.launch();
    const bad = [];
    try {
      for (const width of [390, 430]) for (const standalone of [false, true]) {
        const tag = `${width}-${standalone ? "app" : "safari"}`;
        const { page: p } = await pairDevice(wk, aaa("web", "pair").code, "iphone", { standalone });
        try {
          if (width !== 390) await p.setViewportSize({ width, height: 932 });
          await openPane(p, "ak1");
          const paneHash = (await viewIs(p)).hash;
          await openAsk(p);
          const asked = await viewIs(p);
          if (!whole(asked, "답할 것")) bad.push(`${tag} 「답할 것」 열기: ${fmt(asked)}`);
          await shot(p, `62-phone-${tag}-asks`);
          // 사용자 캡처의 길: 「답할 것」을 연 채 다른 화면(pane)을 부른다
          await openPane(p, "ak2");
          const called = await viewIs(p);
          await shot(p, `62-phone-${tag}-pane-after-asks`);
          if (!whole(called, "pane") || called.asks !== "false" || called.asksVisible !== "hidden" || called.asksInFlow) throw new Error(`${tag} 「답할 것」 뒤 pane: ${fmt(called)}`);
          // 휴대폰 뒤로: 「답할 것」으로 돌아가고(본문 전체), 다시 뒤로는 그 앞 pane(본문 전체)
          const b1 = await back(p);
          if (!/^#ask=r/.test(b1.hash) || !whole(b1, "답할 것")) bad.push(`${tag} 뒤로 1: ${fmt(b1)}`);
          const b2 = await back(p);
          if (b2.hash !== paneHash || !whole(b2, "pane") || b2.asksVisible !== "hidden") bad.push(`${tag} 뒤로 2: ${fmt(b2)}`);
          await shot(p, `62-phone-${tag}-back`);
          // 같은 길의 보고서 화면: 보고서를 연 채 pane을 부르면 본문 전체가 pane이다(「답할 것」이 따를 규칙)
          await inList(p, "activity-reports");
          await domIs(p, "보고서 화면", () => document.querySelector('[data-testid="reports-view"]')?.dataset.shown === "true");
          await openPane(p, "ak2");
          const rep = await viewIs(p);
          if (!whole(rep, "pane") || rep.reports !== "false") bad.push(`${tag} 보고서 뒤 pane: ${fmt(rep)}`);
        } finally {
          await p.context().close();
        }
      }
    } finally {
      await wk.close();
    }
    if (bad.length) throw new Error(`「답할 것」이 보고서 화면처럼 닫히지 않는다:\n${bad.join("\n")}`);
  });

  await step("데스크톱 「답할 것」을 연 채 pane을 고르면 본문 전체가 pane이다(1280)", async () => {
    const dev = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 });
    const p = dev.page;
    try {
      await p.getByTestId("pane-ak1").waitFor();
      await openAsk(p);
      const asked = await viewIs(p);
      if (!whole(asked, "답할 것")) throw new Error(`「답할 것」 열기: ${fmt(asked)}`);
      await openPane(p, "ak2");
      const called = await viewIs(p);
      await shot(p, "62-desktop-pane-after-asks");
      if (!whole(called, "pane") || called.asksVisible !== "hidden" || called.asksInFlow) throw new Error(`「답할 것」 뒤 pane: ${fmt(called)}`);
    } finally {
      await p.context().close();
    }
  });

  await step("닫기 단추는 앞 화면으로, Safari 가장자리 쓸기 뒤로는 목록을 열지 않음(WebKit 390·데스크톱 1280)", async () => {
    const wk = await webkit.launch();
    try {
      for (const phone of [true, false]) {
        const dev = phone
          ? await pairDevice(wk, aaa("web", "pair").code, "iphone")
          : await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 });
        const p = dev.page;
        try {
          await openPane(p, "ak2");
          const previous = (await viewIs(p)).hash;
          await openAsk(p);
          const close = p.getByRole("button", { name: "답할 것 닫기", exact: true });
          await close.click();
          await selected(p);
          const closed = await viewIs(p);
          if (closed.hash !== previous || !whole(closed, "pane")) throw new Error(`닫기 뒤 앞 대화: ${fmt(closed)}`);
          await openPane(p, "ak2");
          await openAsk(p);
          if (phone) {
            // Safari가 손가락을 가져가 touchcancel 뒤 popstate를 내는 길. 왼쪽 끝 쓸기는 브라우저 뒤로 몫이라 목록이 열리지 않는다(plan §8.408).
            await inTime(p.evaluate(async () => {
              const target = document.elementFromPoint(4, 500) ?? document.body;
              const fire = (type, x) => {
                const t = document.createTouch(window, target, 1, x, 500, x, 500);
                const on = type === "touchcancel" ? document.createTouchList() : document.createTouchList(t);
                target.dispatchEvent(new TouchEvent(type, { touches: on, targetTouches: on, changedTouches: document.createTouchList(t), bubbles: true, cancelable: true }));
              };
              fire("touchstart", 4);
              for (let i = 1; i <= 12; i++) {
                fire("touchmove", 4 + (innerWidth - 8) * i / 12);
                await new Promise((r) => requestAnimationFrame(r));
              }
              fire("touchcancel", innerWidth - 4);
              await new Promise((r) => { addEventListener("popstate", r, { once: true }); history.back(); });
            }), 20_000, "쓸기 뒤 popstate");
            await selected(p);
            await drawerSettled(p, true);
            if (await p.locator("#workspace-drawer.is-open").count()) throw new Error("가장자리 쓸기 뒤로에 drawer가 열렸다");
          } else await back(p);
          if ((await viewIs(p)).hash !== previous || !whole(await viewIs(p), "pane")) throw new Error("뒤로가 앞 대화를 채우지 않는다");
          await shot(p, phone ? "62-phone-swipe-back-chat" : "62-desktop-close-chat");
          if (dev.errors.length) throw new Error(dev.errors.join("\n"));
        } finally { await p.context().close(); }
      }
    } finally { await wk.close(); }
  });

  await step("답할 것이 없는 화면도 닫기를 누르면 앞서 보던 보고서로 돌아간다(WebKit 390·데스크톱 1280)", async () => {
    const wk = await webkit.launch();
    try {
      for (const phone of [true, false]) {
        const dev = phone
          ? await pairDevice(wk, aaa("web", "pair").code, "iphone")
          : await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 });
        const p = dev.page;
        try {
          // 실제로 남은 요청을 처리한다. #asks는 열린 요청이 있으면 첫 카드를 보이므로 빈 fixture를 먼저 만든다.
          if (phone) {
            await openAsk(p);
            await p.getByTestId("ask-handle").click();
            await domIs(p, "답할 것 비움", () => document.querySelector('[data-testid="asks-open-count"]')?.textContent === "0");
          }
          await inList(p, "activity-reports");
          await domIs(p, "보고서 목록", () => document.querySelector('[data-testid="reports-view"]')?.dataset.shown === "true");
          await p.evaluate(() => { location.hash = "asks"; });
          await domIs(p, "빈 답할 것 화면", () => document.querySelector('[data-testid="asks-view"]')?.dataset.shown === "true"
            && document.querySelector('[data-testid="asks-empty"]') !== null);
          await shot(p, phone ? "62-phone-empty-close" : "62-desktop-empty-close");
          await p.getByRole("button", { name: "답할 것 닫기", exact: true }).click();
          await domIs(p, "닫기 뒤 보고서", () => location.hash === "#reports" && document.querySelector('[data-testid="reports-view"]')?.dataset.shown === "true"
            && document.querySelector('[data-testid="asks-view"]')?.dataset.shown === "false");
          if (!whole(await viewIs(p), "보고서")) throw new Error("답할 것을 닫은 뒤 보고서가 본문을 채우지 않는다");
          if (dev.errors.length) throw new Error(dev.errors.join("\n"));
        } finally { await p.context().close(); }
      }
    } finally { await wk.close(); }
  });

  await step("막대 자리: 맨 위 「coordinator」, 「답할 것」은 패널 단추 끝, 보고서 단추 바로 위(데스크톱 1280·휴대폰 390 drawer)", async () => {
    const order = (p) =>
      p.evaluate(() => {
        const items = [...document.querySelectorAll(".activity-bar .activity-item")].filter((e) => e.closest(".activity-bottom") === null);
        const box = (id) => document.querySelector(`[data-testid="${id}"]`).getBoundingClientRect();
        const asks = box("activity-asks");
        const reports = box("activity-reports");
        const ideas = box("activity-ideas");
        return {
          ids: items.map((e) => e.dataset.testid).join(","),
          // 이웃한 두 단추 사이 틈: 아이디어↔답할 것과 답할 것↔보고서
          gapAbove: Math.round(asks.top - ideas.bottom),
          gapBelow: Math.round(reports.top - asks.bottom),
        };
      });
    // 맨 위는 「coordinator」(t-0621). 새 검색은 할 일 다음이며 답할 것·보고서 인접 배치는 유지한다(t-1205).
    // 아이디어(t-1260)는 패널 단추 안 「답할 것」 바로 위다
    const want = "activity-coordinator,activity-work,activity-workspace,activity-machines,activity-automations,activity-ideas,activity-asks,activity-reports,activity-tasks,activity-search";
    const dev = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 });
    try {
      await dev.page.getByTestId("activity-bar").waitFor();
      const o = await order(dev.page);
      if (o.ids !== want || o.gapBelow < 0 || o.gapBelow > o.gapAbove + 16) throw new Error(`데스크톱 막대: ${fmt(o)}`);
      await shot(dev.page, "62-desktop-bar");
    } finally {
      await dev.page.context().close();
    }
    const wk = await webkit.launch();
    try {
      const { page: p } = await pairDevice(wk, aaa("web", "pair").code, "iphone");
      await p.locator(".view [data-testid='drawer-toggle']:visible").click();
      await p.locator("#workspace-drawer.is-open").waitFor();
      await drawerSettled(p, true);
      const o = await order(p);
      await shot(p, "62-phone-bar");
      if (o.ids !== want || o.gapBelow < 0 || o.gapBelow > o.gapAbove + 16) throw new Error(`휴대폰 막대: ${fmt(o)}`);
    } finally {
      await wk.close();
    }
  });
}

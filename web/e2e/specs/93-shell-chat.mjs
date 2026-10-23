// 기본 셸 → hook 없는 harness → 실제 프로세스/기록 연결 → host → 대화 페이지/스크롤.
import assert from "node:assert/strict";
import fs from "node:fs";
import {
  root, aaa, step, until, waitOutput, inList, domIs, shot, pairPage, browser, webkit, inTime,
} from "../lib/harness.mjs";

export const labEnv = { CODEX_HOME: `${root}/codex` };
fs.mkdirSync(`${root}/shellbin`, { recursive: true });
for (const name of ["codex", "claude"]) {
  fs.copyFileSync(new URL("../lib/fake-shell-harness.py", import.meta.url), `${root}/shellbin/${name}`);
  fs.chmodSync(`${root}/shellbin/${name}`, 0o755);
}
const sid = (n) => `01900000-0000-7000-8000-${n.toString(16).padStart(12, "0")}`;
let serial = 0;

export default async function () {
  const check = async (engine, width, harness) => {
    const n = ++serial;
    const name = `shell-${harness}-${n}`;
    const id = sid(n * 3), next = sid(n * 3 + 1), decoy = sid(n * 3 + 2);
    const page = (await pairPage(engine, aaa("web", "pair").code, { width, height: 844 }, { touch: width < 500 })).page;
    try {
      // 본문과 cwd가 비슷한 더 새 기록도 다른 PID의 것이면 고르지 않는다.
      const dir = harness === "codex" ? `${root}/codex/sessions/2026/10/23` : `${root}/claude/projects/shell`;
      fs.mkdirSync(dir, { recursive: true });
      fs.writeFileSync(`${dir}/${harness === "codex" ? "rollout-decoy-" : ""}${decoy}.jsonl`, "{}\n");
      if (harness === "codex") {
        const backup = `${root}/codex/sessions/2026/10/22`;
        fs.mkdirSync(backup, { recursive: true });
        fs.writeFileSync(`${backup}/rollout-backup-${id}.jsonl`, JSON.stringify({ type: "session_meta", payload: { id, source: "cli" } }) + "\n");
      }
      const pane = aaa("tab", "create", "--machine", "labA", "--label", name).tabs.at(-1).root.pane;
      await inList(page, `pane-${name}`);
      aaa("pane", "run", pane, `${root}/shellbin/${harness} ${root} ${id}`);
      await waitOutput(pane, "foreground-harness-ready");
      await until("직접 실행한 harness 감지", () => aaa("pane", "list").panes.find((p) => p.id === pane)?.agent?.harness === harness);
      const state = () => aaa("pane", "list").panes.find((p) => p.id === pane);
      assert.equal(state().agent.harness_session, null, "최근 기록을 추측해 연결했다");
      const toggle = page.getByTestId("chat-toggle");
      await toggle.waitFor();
      if ((await toggle.getAttribute("aria-pressed")) !== "true") await toggle.click();
      const chat = page.getByTestId("chat");
      await chat.locator(".banner.warn").waitFor();
      // 첫 기록이 늦게 생겨도 사용자 새로 고침 없이 연결하고 push로 갱신한다.
      aaa("pane", "run", pane, "open");
      await until("실제 PID의 기록 연결", () => state().agent?.harness_session === id);
      await chat.getByText(`${id} 기록 359 ·`, { exact: false }).first().waitFor();
      await domIs(page, "대화 스크롤 범위", () => {
        const c = document.querySelector('[data-testid="chat"]');
        return c && c.scrollHeight > c.clientHeight;
      });
      // 두 쪽 이상을 실제 host에서 읽고 처음까지 도달한다. scroll 사건이 앞 쪽 요청을 끝낸다.
      for (let part = 0; part < 5; part++) {
        if (await chat.getByText("처음이다", { exact: true }).count()) break;
        const response = page.waitForResponse((r) => {
          if (!r.url().endsWith("/api/v1/call")) return false;
          const b = r.request().postDataJSON();
          return b?.op === "pane_chat" && Boolean(b.before);
        });
        await chat.evaluate((c) => { c.scrollTop = 0; c.dispatchEvent(new Event("scroll", { bubbles: true })); });
        await response;
        await domIs(page, "앞 기록 읽기 완료", () => !document.querySelector('[data-testid="chat"] [data-testid="chat-older-error"]') && !document.querySelector('[data-testid="chat"] button.chat-older:disabled'));
      }
      await chat.evaluate((c) => { c.scrollTop = 0; });
      await chat.getByText(`${id} 기록 0 ·`, { exact: false }).first().waitFor();
      await chat.getByText("처음이다", { exact: true }).waitFor();
      await shot(page, `shell-chat-${harness}-${engine.browserType().name()}-${width}`);
      // 데스크톱 휠과 Chromium 휴대폰의 실제 터치를 확인한다. 앞 쪽 갱신은 읽던 위치를 지킨다.
      const top = await chat.evaluate((c) => c.scrollTop);
      const box = await chat.boundingBox();
      if (width < 500 && engine === browser) {
        await chat.evaluate((c) => {
          window.shellScrollEnd = new Promise((done) => c.addEventListener("scrollend", () => done(), { once: true }));
        });
        const cdp = await page.context().newCDPSession(page);
        const x = Math.round(box.x + box.width / 2), y = Math.round(box.y + box.height - 50);
        await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x, y, id: 1 }] });
        for (let i = 1; i <= 8; i++) await cdp.send("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: [{ x, y: y - i * 25, id: 1 }] });
        await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
        await inTime(page.evaluate(() => window.shellScrollEnd), 20_000, "손을 뗀 뒤 관성 스크롤 종료");
        await cdp.detach();
      } else if (width < 500) {
        // Playwright의 mobile WebKit은 wheel 입력을 제공하지 않는다. 같은 브라우저 scroll 경로를 쓴다.
        await chat.evaluate((c) => c.scrollBy(0, 300));
      } else {
        await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
        await page.mouse.wheel(0, 300);
      }
      await page.waitForFunction((before) => document.querySelector('[data-testid="chat"]').scrollTop > before, top);
      await chat.evaluate((c) => { c.scrollTop = 0; c.dispatchEvent(new Event("scroll", { bubbles: true })); });
      const settled = await chat.evaluate((c) => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(() => done({ top: c.scrollTop, first: c.querySelector(".chat-row")?.getBoundingClientRect().top })))));
      // 실제 두 frame에서 가상 row의 위치 측정을 끝낸 뒤 새 답변을 쓴다.
      assert.equal(settled.top, 0, `${name}: 새 답변을 쓰기 전 자리`);
      const appended = page.waitForResponse(async (r) => {
        if (!r.url().endsWith("/api/v1/call") || r.request().postDataJSON()?.op !== "pane_chat") return false;
        return (await r.json()).ok?.messages?.some((m) => m.text === `${id} 새 답변`);
      });
      aaa("pane", "run", pane, "append");
      const reply = await (await appended).json();
      const added = reply.ok.messages.find((m) => m.text === `${id} 새 답변`).id;
      // 화면 밖 본문은 가상화된다. 새 row의 commit으로 갱신을 확인한다.
      await chat.locator(`[data-chat-id="${added}"]`).waitFor({ state: "attached" });
      assert.equal(await chat.evaluate((c) => c.scrollTop), 0, "갱신이 읽던 위치를 옮겼다");
      aaa("pane", "run", pane, `switch ${next}`);
      await until("같은 프로세스의 새 대화 연결", () => state().agent?.harness_session === next);
      await chat.getByText(`${next} 기록 359 ·`, { exact: false }).first().waitFor();
      assert.equal(await chat.getByText(`${id} 기록`, { exact: false }).count(), 0, "이전 세션을 섞었다");
      aaa("pane", "run", pane, "quit");
      await waitOutput(pane, "foreground-harness-exited");
      await until("셸 복귀 때 연결 해제", () => !state().agent);
      await page.getByTestId("chat-toggle").waitFor({ state: "detached" });
      assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
    } finally {
      await page.context().close();
    }
  };
  await step("셸에서 직접 연 Codex·Claude의 대화 연결과 스크롤(데스크톱 Chromium·WebKit)", async () => {
    const safari = await webkit.launch({ headless: true });
    try {
      for (const engine of [browser, safari]) for (const harness of ["codex", "claude"]) await check(engine, 1280, harness);
    } finally { await safari.close(); }
  });
  await step("셸에서 직접 연 Codex·Claude의 대화 연결과 스크롤(휴대폰 Chromium·WebKit 390px)", async () => {
    const safari = await webkit.launch({ headless: true });
    try {
      for (const engine of [browser, safari]) for (const harness of ["codex", "claude"]) await check(engine, 390, harness);
    } finally { await safari.close(); }
  });
}

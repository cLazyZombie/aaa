// 목록·탭·그리기 지킴(t-0403, plan §8.266, t3code 참고): thread 줄 상태 색의 차례와 작업 시간(Sidebar.logic.ts resolveThreadStatusPill·
// formatWorkingDurationLabel, 같은 초 시계 useNowSecond), 창에 focus가 없는 동안 알린 수의 탭 아이콘 배지(ThreadNotificationCoordinator·
// threadNotifications.ts setNotificationBadge), 그리기 오류를 그 자리에 가두는 오류 경계(RenderErrorBoundary).
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 31-render-guards(새 실험실). 준비(host·agent·t1·첫 화면)는 lib/harness.mjs runSpec.
import fs from "node:fs";
import { root, aaa, AA, step, inList, until, domIs, waitOutput, shot, claudeSh, claudeWorking, claudeIdle } from "../lib/harness.mjs";

// 화면 감지 유예 0(가짜 화면은 뜨자마자 그대로다), 긴 턴 기준 0(턴마다 「작업 완료」를 알린다, 기본값 길은 19-turn-alerts)
export const labEnv = { AAA_TEST_DETECT_GRACE_MS: "0", AAA_TEST_LONG_TURN_MS: "0" };

export default async function ({ page }) {
  await step("thread 줄 상태 색(t3code resolveThreadStatusPill 차례): 사람을 기다리는 thread는 「확인 필요」 색이고 작업 시간이 없다. 그 pane의 Claude가 일하면 색이 작업 중이 되고 상태 말 뒤에 초 단위 작업 시간이 붙어 늘며, 쉬면 시간이 사라진다", async () => {
    aaa("project", "create", "demo");
    fs.writeFileSync(`${root}/tone-task.md`, "# 색 보기\n\n사람을 기다림\n");
    const id = aaa("thread", "start", "demo", "--title", "색 보기", "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/tone-task.md`).id;
    await until("brief 전달", () => aaa("thread", "show", "demo", id).brief === "delivered");
    const shown = aaa("thread", "show", "demo", id);
    const pane = shown.pane?.id ?? shown.pane;
    await inList(page, `thread-demo-${id}`);
    const state = page.getByTestId(`thread-state-demo-${id}`);
    await domIs(page, "확인 필요 색, 시간 없음", (q) => {
      const s = document.querySelector(q);
      return s?.dataset.tone === "attention" && !s.querySelector('[data-testid="working-duration"]');
    }, `[data-testid="thread-state-demo-${id}"]`);
    // thread pane 안에서 Claude가 일하는 화면을 띄운다: fake.sh의 post-probe가 그 pane에서 sh로 돌린 셸이 job control(set -m)로
    // `sh …/claude`를 새 프로세스 그룹의 앞 job으로 띄운다. agent는 셸 pane의 앞 그룹이 바뀌면 그 이름(sh의 script 이름 claude)으로
    // Claude를 찾는다(detect/foreground.rs, herdr 규칙). 그 화면은 다음 줄을 받으면 쉬고, 그다음 줄에 끝난다
    fs.mkdirSync(`${root}/tonebin`);
    fs.writeFileSync(`${root}/tonebin/claude`, `${claudeWorking}\nread _\n${claudeIdle}\nread _\n`);
    const probe = `${root}/tone-front.sh`;
    fs.writeFileSync(probe, `set -m\nsh ${root}/tonebin/claude\n`);
    aaa("pane", "run", pane, `post-probe ${probe}`);
    const paneNow = () => aaa("pane", "list").panes.find((p) => p.id === pane);
    await until("pane의 Claude가 작업 중", () => paneNow()?.agent?.activity === "working").catch((e) => {
      throw new Error(`${e.message}: pane ${pane} ${JSON.stringify(paneNow())}`);
    });
    await domIs(page, "작업 중 색과 작업 시간", (q) => {
      const s = document.querySelector(q);
      return s?.dataset.tone === "working" && /^\d+초$/.test(s.querySelector('[data-testid="working-duration"]')?.textContent ?? "");
    }, `[data-testid="thread-state-demo-${id}"]`);
    const first = await state.getByTestId("working-duration").textContent();
    await until("작업 시간이 는다", async () => (await state.getByTestId("working-duration").textContent()) !== first);
    await shot(page, "thread-tone-working");
    aaa("pane", "run", pane, "end");
    await domIs(page, "쉬면 시간이 사라진다", (q) => {
      const s = document.querySelector(q);
      return s?.dataset.tone !== "working" && !s?.querySelector('[data-testid="working-duration"]');
    }, `[data-testid="thread-state-demo-${id}"]`);
    aaa("pane", "run", pane, "done");
    await waitOutput(pane, "fake>");
  });

  await step("탭 아이콘 배지(t3code ThreadNotificationCoordinator·setNotificationBadge): 창에 focus가 없는 동안 알린 자리(pane·thread)를 하나씩 세어 favicon을 빨간 원과 수로 바꾸고, 같은 자리는 다시 세지 않으며, 창에 focus가 오면 원래 아이콘으로 돌아온다", async () => {
    // 줄 하나를 받을 때마다 턴을 열고(일하는 화면) 다음 줄에 닫는다(입력 상자, 19-turn-alerts와 같은 화면)
    const script = `stty -echo; ${claudeIdle}; while read a; do ${claudeWorking}; read b; ${claudeIdle}; done`;
    for (const name of ["fb1", "fb2"]) aaa("tab", "create", "--machine", "labA", "--label", name, "--", claudeSh, "-c", script);
    const is = (name, s) => page.getByTestId(`agent-${name}`).and(page.locator(`[data-status="${s}"]`)).waitFor();
    await is("fb1", "idle");
    await is("fb2", "idle");
    // 내려온 앱 안 알림을 센다: 알림 하나가 이 화면에서 처리된 울타리다(배지는 같은 raiseAlert에서 먼저 센다)
    await page.evaluate(() => {
      const drops = [];
      Object.defineProperty(window, "__badgeDrops", { value: drops });
      new MutationObserver((ms) => {
        for (const m of ms) for (const n of m.addedNodes) if (n.classList?.contains("droplet")) drops.push(n.querySelector(".droplet-title")?.textContent);
      }).observe(document, { childList: true, subtree: true });
      document.hasFocus = () => false;
      window.dispatchEvent(new Event("blur"));
    });
    const original = await page.locator('link[rel="icon"]').getAttribute("href");
    const icons = () => page.evaluate(() => [...document.querySelectorAll('link[rel="icon"]')].map((l) => l.dataset.badge ?? l.getAttribute("href")));
    const turn = async (name, drops) => {
      aaa("pane", "run", name, "go");
      await is(name, "working");
      aaa("pane", "run", name, "end");
      await is(name, "done");
      await until(`앱 안 알림 ${drops}개`, async () => (await page.evaluate(() => window.__badgeDrops.length)) >= drops);
    };
    await turn("fb1", 1);
    if (JSON.stringify(await icons()) !== JSON.stringify(["1"])) throw new Error(`배지 1: ${await icons()}`);
    const image = await page.locator('link[rel="icon"]').getAttribute("href");
    if (!image?.startsWith("data:image/png")) throw new Error(`배지 그림: ${image?.slice(0, 30)}`);
    await shot(page, "favicon-badge");
    await turn("fb2", 2);
    if (JSON.stringify(await icons()) !== JSON.stringify(["2"])) throw new Error(`배지 2: ${await icons()}`);
    await turn("fb1", 3);
    if (JSON.stringify(await icons()) !== JSON.stringify(["2"])) throw new Error(`같은 자리는 다시 세지 않는다: ${await icons()}`);
    await page.evaluate(() => {
      delete document.hasFocus;
      window.dispatchEvent(new Event("focus"));
    });
    if (JSON.stringify(await icons()) !== JSON.stringify([original])) throw new Error(`focus가 오면 원래 아이콘: ${await icons()}`);
    for (const name of ["fb1", "fb2"]) aaa("pane", "kill", name);
  });

  await step("그리기 오류 경계(t3code RenderErrorBoundary): 대화 보기가 그리다 터지면 본문 자리만 「본문을 그리지 못했다」로 바뀌고 목록은 그대로 쓴다. 「다시 그리기」로 다시 그리고, 다른 pane으로 가면 저절로 다시 그린다", async () => {
    // 긴 대화 기록을 가진 Claude pane(hook으로 기록 자리를 알린다, 22-chat-surface와 같은 길)
    const dir = `${root}/claude/projects/-lab-eb1`;
    fs.mkdirSync(dir, { recursive: true });
    const transcript = `${dir}/s1.jsonl`;
    fs.writeFileSync(transcript, JSON.stringify({ type: "user", message: { role: "user", content: "그려 줘" } }) + "\n");
    aaa("tab", "create", "--machine", "labA", "--label", "eb1", "--", "/bin/sh", "-c", '"$0" hook --agent claude "$1"; exec cat', AA,
      JSON.stringify({ hook_event_name: "SessionStart", session_id: "8b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e", source: "startup", transcript_path: transcript }));
    // 대화 칸을 지켜보기 시작할 때(ResizeObserver.observe) 터지게 심는다
    await page.evaluate(() => {
      const observe = ResizeObserver.prototype.observe;
      window.__breakChat = true;
      ResizeObserver.prototype.observe = function (target, ...rest) {
        if (window.__breakChat && target instanceof Element && target.matches('[data-testid="chat"]')) throw new Error("시험이 심은 그리기 오류");
        return observe.call(this, target, ...rest);
      };
    });
    const broken = page.locator('[data-testid="render-error"][data-area="본문"]');
    // 대화 보기를 연다: 이 pane은 대화 보기로 열릴 수 있고, 그러면 곧바로 터져 대화 단추도 함께 사라진다
    const toggle = page.getByTestId("chat-toggle");
    const openChat = async () => {
      await inList(page, "pane-eb1");
      await until("대화 단추나 그리기 오류", async () => (await broken.count()) + (await toggle.count()) > 0);
      if ((await broken.count()) === 0 && (await toggle.getAttribute("aria-pressed")) !== "true") await toggle.click();
    };
    await openChat();
    await broken.waitFor();
    if (!(await broken.innerText()).includes("시험이 심은 그리기 오류")) throw new Error(`오류 글: ${await broken.innerText()}`);
    await shot(page, "render-error");
    // 목록은 그대로 쓴다: 다른 pane으로 가면 본문이 다시 그려진다(경계의 resetKeys)
    await inList(page, "pane-t1");
    await broken.waitFor({ state: "detached" });
    await page.getByTestId("terminal").waitFor();
    await openChat();
    await broken.waitFor();
    await page.evaluate(() => (window.__breakChat = false));
    await broken.getByRole("button", { name: "다시 그리기" }).click();
    await broken.waitFor({ state: "detached" });
    await openChat();
    await until("대화 기록", async () => (await page.locator('[data-testid="chat"] .chat-msg').count()) >= 1);
    aaa("pane", "kill", "eb1");
  });
}

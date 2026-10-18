// 웹 E2E 앱 틀: 화면 켜짐 유지·앱 설치·agent 표시·목록 바닥 줄·명령 팔레트.
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 04-app-shell(새 실험실). 준비(host·agent·t1·첫 화면)는 lib/harness.mjs runSpec.

import {
  webkit, AA, sleep, aaa, until, screenText, shot, fakeViewport, softKeyboard, pairPage, step, browser,
  openSettings,
} from "../lib/harness.mjs";

export default async function ({ ctl, page }) {
  await step("화면 켜짐 유지(herdr-web-ui keepScreenOn·wakeLock, 차이 목록 M6): 켜면 pane을 보는 동안 screen wake lock, 가려지면 놓고 다시 보이면 다시, 끄면 놓음, 이 기기에 저장", async () => {
    // wake lock은 headless에서 거절될 수 있다: 요청·놓기를 기록하는 것으로 바꾼다
    const init = () => {
      const log = [];
      Object.defineProperty(window, "__wakeLog", { value: log });
      Object.defineProperty(navigator, "wakeLock", {
        configurable: true,
        value: {
          request: async (type) => {
            log.push(`request ${type}`);
            return { release: async () => void log.push("release") };
          },
        },
      });
    };
    const wp = (await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 }, { init })).page;
    const log = () => wp.evaluate(() => window.__wakeLog.join(","));
    const hide = (hidden) =>
      wp.evaluate((h) => {
        Object.defineProperty(document, "visibilityState", { configurable: true, get: () => (h ? "hidden" : "visible") });
        document.dispatchEvent(new Event("visibilitychange"));
      }, hidden);
    // 「요청하지 않았다」는 App이 wake lock effect를 그 상태로 돌린 뒤 다는 표지(data-wake-lock: 설정 걸지 보는 것) 뒤에 본다
    const t1 = `pane:${aaa("pane", "list").panes.find((p) => p.name === "t1").id}`;
    const decided = (want) => until(`wake lock 판정 ${want}`, async () => (await wp.evaluate(() => document.documentElement.dataset.wakeLock)) === want);
    await wp.getByTestId("pane-t1").click();
    // 앞 단계가 남긴 글에 기대지 않는다: 셸 프롬프트가 보이면 연결된 것이다
    await until("t1 화면", async () => (await screenText(wp)).includes("$"));
    await decided(`off free ${t1}`);
    if ((await log()) !== "") throw new Error(`꺼진 설정인데 ${await log()}`);
    if (!(await wp.evaluate(() => window.isSecureContext))) throw new Error("시험 주소가 secure context가 아니다");
    const toggle = async () => {
      await (await openSettings(wp, "폰 및 기기")).getByRole("switch", { name: "화면 켜짐 유지" }).click();
      await wp.keyboard.press("Escape");
    };
    // 휴대폰은 pane을 보는 동안 목록 바닥 톱니가 가려진다: 목록으로 돌아가 켜고 다시 고른다
    const back = wp.getByRole("button", { name: "목록으로" });
    if (await back.isVisible()) await back.click();
    await toggle();
    if ((await wp.evaluate(() => localStorage.getItem("aaa:keep-screen-on"))) !== "1") throw new Error("켠 설정이 저장되지 않았다");
    await decided("on free -");
    if ((await log()) !== "") throw new Error(`pane을 보지 않는데 ${await log()}`);
    await wp.getByTestId("pane-t1").click();
    await until("wake lock 요청", async () => (await log()) === "request screen");
    await hide(true);
    await until("가려지면 놓음", async () => (await log()) === "request screen,release");
    await hide(false);
    await until("다시 보이면 다시 요청", async () => (await log()) === "request screen,release,request screen");
    // 다시 열어도 이 기기 설정
    await wp.reload();
    // 주소(#pane=)가 그 pane을 다시 고른다. 목록이 보이면 고른다
    await until("다시 연 화면", async () => (await wp.getByTestId("pane-t1").isVisible()) || (await log()) !== "");
    if (await wp.getByTestId("pane-t1").isVisible()) await wp.getByTestId("pane-t1").click();
    await until("다시 연 화면도 요청", async () => (await log()) === "request screen");
    if (await back.isVisible()) await back.click();
    await until("목록으로 가면 놓음", async () => (await log()) === "request screen,release");
    await toggle();
    if ((await wp.evaluate(() => localStorage.getItem("aaa:keep-screen-on"))) !== null) throw new Error("끈 설정이 남았다");
    await wp.getByTestId("pane-t1").click();
    await decided(`off free ${t1}`);
    if ((await log()) !== "request screen,release") throw new Error(`끈 뒤 ${await log()}`);
    await wp.context().close();
  });

  await step("앱 설치(herdr-web-ui install.ts·MachineSidebar·설정 「휴대폰 → 설치」, 차이 목록 M4): 설치 전엔 단추가 늘 있고, 신호가 없으면 누를 때 이 브라우저의 방법을 펼치며, 신호가 오면 그 창을 띄우고, 설치되면 숨는다", async () => {
    // 브라우저가 스스로 보내는 beforeinstallprompt는 시험이 보낸 것만 남긴다(headless가 보낼지 말지에 기대지 않는다)
    const init = () => {
      const log = [];
      Object.defineProperty(window, "__installLog", { value: log });
      window.addEventListener(
        "beforeinstallprompt",
        (e) => {
          if (!e.e2e) e.stopImmediatePropagation();
        },
        true,
      );
    };
    const offer = (p) =>
      p.evaluate(() => {
        const e = new Event("beforeinstallprompt", { cancelable: true });
        e.e2e = true;
        e.prompt = async () => void window.__installLog.push("prompt");
        e.userChoice = Promise.resolve({ outcome: "dismissed", platform: "" });
        window.dispatchEvent(e);
        return e.defaultPrevented;
      });
    const dialogRow = async (p) => {
      const row = (await openSettings(p, "폰 및 기기")).locator(".settings-row", { hasText: /^설치/ });
      const text = (await row.innerText()).replace(/\s+/g, " ");
      const button = await row.getByRole("button", { name: "앱 설치" }).count();
      await p.keyboard.press("Escape");
      return { text, button };
    };
    const ip = (await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 }, { init })).page;
    const btn = ip.getByTestId("install-app");
    await until("설치 단추", () => btn.isVisible());
    // 신호 없음(데스크톱 Chromium): 누르면 주소창 안내를 펼치고, 다시 누르면 접는다
    if ((await btn.getAttribute("aria-expanded")) !== "false") throw new Error(`aria-expanded ${await btn.getAttribute("aria-expanded")}`);
    await btn.click();
    const help = ip.getByTestId("install-help");
    if ((await help.innerText()) !== "주소창의 설치 아이콘이나 브라우저 메뉴의 설치 항목을 쓰세요.") throw new Error(`안내 ${await help.innerText()}`);
    if ((await btn.getAttribute("aria-expanded")) !== "true") throw new Error("펼친 뒤 aria-expanded");
    let row = await dialogRow(ip);
    if (!row.text.includes("주소창의 설치 아이콘") || row.button !== 0) throw new Error(`설정 줄(신호 없음) ${JSON.stringify(row)}`);
    await btn.click();
    if (await help.count()) throw new Error("다시 눌러도 안내가 남았다");
    // 신호가 오면 기본 창을 막아 두고(preventDefault), 단추가 그 창을 띄운다
    if (!(await offer(ip))) throw new Error("beforeinstallprompt 기본 동작을 막지 않았다");
    await until("신호 받은 단추", async () => (await btn.getAttribute("aria-expanded")) === null);
    row = await dialogRow(ip);
    if (row.button !== 1 || row.text.includes("주소창")) throw new Error(`설정 줄(신호 있음) ${JSON.stringify(row)}`);
    await btn.click();
    await until("설치 창", async () => (await ip.evaluate(() => window.__installLog.join(","))) === "prompt");
    // 거절하면 그 신호는 다 썼다: 다시 안내로
    await until("거절 뒤 안내로", async () => (await btn.getAttribute("aria-expanded")) === "false");
    // 설치되면 단추가 숨고 설정은 「설치됨」
    await ip.evaluate(() => window.dispatchEvent(new Event("appinstalled")));
    await until("설치 뒤 단추 숨김", async () => (await btn.count()) === 0);
    row = await dialogRow(ip);
    if (!row.text.includes("설치됨") || row.button !== 0) throw new Error(`설정 줄(설치됨) ${JSON.stringify(row)}`);
    await ip.context().close();
    // 브라우저마다 안내(참고 installHelp 시험의 기기들). iPadOS는 Mac UA라 터치 점 수로 가린다:
    // Chromium 터치 흉내는 maxTouchPoints가 1이라 실제 iPad처럼 5로 둔다(참고 시험도 5)
    const IPHONE = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1";
    const MAC_SAFARI = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15";
    // addInitScript는 함수 글만 옮긴다(바깥 init을 부를 수 없다)
    const ipad = () => {
      window.addEventListener(
        "beforeinstallprompt",
        (e) => {
          if (!e.e2e) e.stopImmediatePropagation();
        },
        true,
      );
      Object.defineProperty(navigator, "maxTouchPoints", { get: () => 5 });
    };
    for (const [name, viewport, userAgent, touch, want, script] of [
      ["iPhone", { width: 390, height: 844 }, IPHONE, true, "공유 버튼을 누른 뒤 홈 화면에 추가를 선택하세요.", init],
      ["iPad(Mac UA·터치)", { width: 1024, height: 1366 }, MAC_SAFARI, true, "공유 버튼을 누른 뒤 홈 화면에 추가를 선택하세요.", ipad],
    ]) {
      const p = (await pairPage(browser, aaa("web", "pair").code, viewport, { userAgent, touch, init: script })).page;
      await until(`${name} 설치 단추`, () => p.getByTestId("install-app").isVisible());
      await p.getByTestId("install-app").click();
      const got = await p.getByTestId("install-help").innerText();
      if (got !== want) throw new Error(`${name} 안내: ${got}`);
      await p.context().close();
    }
    // 홈 화면 앱(iOS navigator.standalone)으로 열면 단추가 없고 설정은 「설치됨」
    const sp = (
      await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 }, {
        userAgent: IPHONE,
        init: () => Object.defineProperty(navigator, "standalone", { get: () => true }),
      })
    ).page;
    await until("홈 화면 앱 목록", () => sp.getByTestId("settings-open").isVisible());
    if (await sp.getByTestId("install-app").count()) throw new Error("설치된 앱에 설치 단추");
    row = await dialogRow(sp);
    if (!row.text.includes("설치됨")) throw new Error(`홈 화면 앱 설정 줄 ${JSON.stringify(row)}`);
    await sp.context().close();
  });

  await step("agent 표시(herdr-web-ui AgentMark·Sidebar rowMark·App context-title, 사용자 요청 10-17): 목록 줄과 터미널 머리에 Claude는 주황 타일 별, Codex는 매듭, 셸은 터미널 표시", async () => {
    // 세션 ID hook으로 harness를 알린 pane(Claude SessionStart·Codex notify)과 셸(t1)
    const idHook = { claude: '{"hook_event_name":"SessionStart","session_id":"m1","source":"startup"}', codex: '{"type":"agent-turn-complete","thread-id":"m2"}' };
    for (const [name, agent] of [["markc", "claude"], ["markx", "codex"]])
      aaa("tab", "create", "--machine", "labA", "--label", name, "--", "/bin/sh", "-c", `"$0" hook --agent ${agent} '${idHook[agent]}'; exec cat`, AA);
    const mp = (await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 })).page;
    for (const name of ["markc", "markx"]) await until(`${name} harness`, async () => (await mp.getByTestId(`agent-${name}`).count()) === 1);
    const markOf = (name) =>
      mp.getByTestId(`pane-${name}`).locator(".sidebar-mark").evaluate((m) => {
        const a = m.querySelector(".agent-mark");
        const svg = m.querySelector("svg");
        return {
          agent: m.dataset.agent,
          cls: a?.className ?? null,
          label: svg?.getAttribute("aria-label") ?? null,
          tile: a?.querySelector(".agent-mark-tile")?.getAttribute("fill") ?? null,
          shell: svg?.classList.contains("lucide-terminal") ?? false,
        };
      });
    const bad = [];
    const c = await markOf("markc");
    if (c.cls !== "agent-mark claude is-brand" || c.label !== "claude" || c.tile !== "#D77655") bad.push(`Claude 줄 ${JSON.stringify(c)}`);
    const x = await markOf("markx");
    if (x.cls !== "agent-mark codex" || x.label !== "codex") bad.push(`Codex 줄 ${JSON.stringify(x)}`);
    // 셸은 agent 표시가 아니라 터미널 표시(참고 Sidebar.tsx rowMark: lucide Terminal)
    const sh = await markOf("t1");
    if (sh.agent !== "shell" || sh.cls !== null || !sh.shell) bad.push(`셸 줄 ${JSON.stringify(sh)}`);
    await shot(mp, "phone-agent-marks");
    // 터미널 머리: 제목 앞에 그 pane의 agent 표시(참고 App.tsx 1402줄, 18px)
    await mp.getByTestId("pane-markc").click();
    const head = mp.locator(".term-title .agent-mark");
    await head.waitFor();
    const hc = await head.evaluate((a) => a.className);
    if (hc !== "agent-mark claude is-brand") bad.push(`머리 표시 ${hc}`);
    await shot(mp, "phone-agent-head");
    await mp.getByRole("button", { name: "목록으로" }).click();
    await mp.getByTestId("pane-t1").click();
    await mp.getByTestId("terminal").waitFor();
    if ((await mp.locator(".term-title .agent-mark").count()) !== 0) bad.push("셸 머리에 agent 표시가 있다");
    await mp.context().close();
    for (const name of ["markc", "markx"]) aaa("pane", "kill", name);
    if (bad.length) throw new Error(bad.join("; "));
  });

  await step("목록 바닥 줄은 늘 맨 아래(herdr-web-ui sidebar-footer, 사용자 요청 10-17): 목록이 길어도·굴려도 설정·사용량 줄은 화면 맨 아래, 휴대폰·iPad·데스크톱·화면 키보드", async () => {
    // 목록이 화면보다 길어야 시험이 된다: 짧으면 예전 판(바닥 줄이 목록 끝을 따라감)도 맨 아래에 있다
    const many = Array.from({ length: 24 }, (_, i) => `foot${i + 1}`);
    for (const name of many) aaa("tab", "create", "--machine", "labA", "--label", name, "--", "/bin/sleep", "600");
    const footer = (p) =>
      p.evaluate(() => {
        const r = (s) => document.querySelector(s).getBoundingClientRect();
        const body = document.querySelector(".list-body");
        return {
          view: Math.round(window.visualViewport?.height ?? window.innerHeight),
          app: Math.round(r(".app").bottom),
          list: Math.round(r(".list").bottom),
          foot: Math.round(r(".list-foot").bottom),
          gear: Math.round(r('[data-testid="settings-open"]').bottom),
          over: body.scrollHeight - body.clientHeight,
          top: Math.round(body.scrollTop),
        };
      });
    const bad = [];
    // 바닥 줄이 보이는 높이의 맨 아래에 붙어 있고(목록 끝을 따라가지 않고), 목록을 끝까지 굴려도 그 자리다
    const check = async (label, p) => {
      await until(`${label} 목록에 foot24`, async () => (await p.getByTestId("pane-foot24").count()) === 1);
      const at = await footer(p);
      if (at.over <= 0) bad.push(`${label}: 목록이 화면보다 짧아 시험이 안 된다 ${JSON.stringify(at)}`);
      if (at.top !== 0 || at.app !== at.view || at.list !== at.view || at.foot !== at.view || at.gear > at.view) bad.push(`${label}: 바닥 줄이 맨 아래가 아니다 ${JSON.stringify(at)}`);
      await p.locator(".list-body").evaluate((el) => (el.scrollTop = el.scrollHeight));
      const end = await footer(p);
      if (end.top === 0 || end.foot !== at.foot || end.gear !== at.gear) bad.push(`${label}: 목록을 굴리니 바닥 줄이 움직였다 ${JSON.stringify({ at, end })}`);
      await p.locator(".list-body").evaluate((el) => (el.scrollTop = 0));
      return at;
    };
    const phone = (await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 })).page;
    const ph = await check("휴대폰 390×844", phone);
    await phone.context().close();
    const desk = (await pairPage(browser, aaa("web", "pair").code, { width: 1100, height: 760 })).page;
    await check("데스크톱 1100×760", desk);
    await desk.context().close();
    const wk = await webkit.launch();
    try {
      const pad = (await pairPage(wk, aaa("web", "pair").code, { width: 820, height: 1180 }, { touch: true, init: fakeViewport })).page;
      const up = await check("iPad 820×1180", pad);
      if (up.view !== 1180) bad.push(`iPad 보이는 높이가 1180이 아니다(${up.view}): 키보드 시험이 안 된다`);
      // pane을 열고 화면 키보드를 올리면(보이는 높이 780) 바닥 줄은 키보드 바로 위에 남는다
      await pad.getByTestId("pane-t1").click();
      await pad.getByTestId("terminal").waitFor();
      // 키보드는 글 칸에 focus가 있을 때만 올라온다(herdr/viewport.ts)
      await pad.locator(".xterm-helper-textarea").focus();
      await softKeyboard(pad, 780);
      await until("iPad 키보드 위 바닥 줄", async () => (await footer(pad)).foot === 780).catch(async (e) => {
        throw new Error(`${e.message}: ${JSON.stringify(await footer(pad))}`);
      });
      await check("iPad 화면 키보드(보이는 높이 780)", pad);
      await softKeyboard(pad, 1180);
      await until("iPad 키보드 내림", async () => (await footer(pad)).foot === 1180);
      await pad.context().close();
    } finally {
      await wk.close();
    }
    for (const name of many) aaa("pane", "kill", name);
    if (bad.length) throw new Error(bad.join("; "));
    console.log(`     휴대폰 바닥 줄 아래 ${ph.foot}px(보이는 높이 ${ph.view}), 목록 넘침 ${ph.over}px`);
  });

  await step("명령 팔레트(herdr-web-ui CommandPalette.tsx, 차이 목록 L12): 머리 돋보기·Mod+Shift+K로 열고, 이름으로 찾아 Enter면 그 pane, 다시 열면 「최근」이 앞, `>`는 pane 없이 동작(「테마 전환」), 찾으면 설정 쪽(「터미널」은 그 쪽으로), 휴대폰엔 돋보기가 없다", async () => {
    for (const name of ["pala", "palb"]) aaa("tab", "create", "--machine", "labA", "--label", name, "--", "/bin/cat");
    const dp = (await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 })).page;
    for (const name of ["pala", "palb"]) await dp.getByTestId(`pane-${name}`).waitFor();
    const palette = dp.getByTestId("command-palette");
    const search = palette.getByRole("searchbox", { name: "패널과 동작 검색" });
    const bad = [];
    const current = (name) => dp.getByTestId(`pane-${name}`).getAttribute("aria-current");
    const headings = () => palette.locator(".palette-section-name").allTextContents();
    const firstOption = () => palette.getByRole("option").first().innerText();

    // 돋보기로 열면 검색 칸이 focus를 받고, 찾지 않은 목록은 머신의 pane 묶음과 동작이다
    await dp.getByTestId("palette-open").click();
    await search.waitFor();
    if (!(await search.evaluate((e) => e === document.activeElement))) bad.push("열어도 검색 칸에 focus가 없다");
    const opened = await headings();
    if (!opened.includes("pane") || opened.at(-1) !== "동작") bad.push(`처음 묶음 ${JSON.stringify(opened)}`);
    const all = await palette.locator('.palette-filter[data-status="all"] .palette-filter-count').innerText();
    if (Number(all) !== (await palette.locator(".palette-pane").count())) bad.push(`「전체」 칩 수 ${all}가 줄 수와 다르다`);
    await search.fill("pal");
    // 이름에 바로 맞은 두 줄이 앞(경로에 p·a·l이 차례로 있는 줄도 글자 순서 찾기로 뒤에 걸린다)
    const titles = () => palette.locator(".palette-pane .palette-row-title").allInnerTexts();
    await until("pal로 찾은 두 줄이 앞", async () => (await titles()).slice(0, 2).sort().join() === "pala,palb");
    await shot(dp, "desktop-palette");
    // 이름으로 찾아 Enter: 그 pane이 열리고 팔레트는 닫힌다
    await search.fill("palb");
    await until("palb가 첫 줄", async () => (await firstOption()).includes("palb"));
    await dp.keyboard.press("Enter");
    await palette.waitFor({ state: "detached" });
    await until("palb 열림", async () => (await current("palb")) === "true");
    await dp.getByTestId("terminal").waitFor();
    // Mod+Shift+K(터미널에 focus가 있어도 앱 몫): pala를 열고, 다시 열면 「최근」 맨 앞이 앞서 연 palb(지금 연 pala는 빠진다)
    await dp.keyboard.press("ControlOrMeta+Shift+K");
    await search.waitFor();
    await search.fill("pala");
    await until("pala가 첫 줄", async () => (await firstOption()).includes("pala"));
    await dp.keyboard.press("Enter");
    await until("pala 열림", async () => (await current("pala")) === "true");
    await dp.keyboard.press("ControlOrMeta+Shift+K");
    await search.waitFor();
    const recent = await headings();
    if (recent[0] !== "최근" || !(await firstOption()).includes("palb")) bad.push(`다시 연 목록 ${JSON.stringify(recent)} 첫 줄 ${await firstOption()}`);
    // `>`는 동작만(칩도 숨는다): 「테마 전환」을 Enter면 테마가 바뀐다
    const theme = () => dp.evaluate(() => document.documentElement.dataset.theme);
    const before = await theme();
    await search.fill(">테마");
    // pane 줄은 빠지고 동작이 먼저(「테마」를 다른 말로 가진 설정 쪽 「모양」은 그 뒤에)
    await until("동작만", async () => (await palette.locator(".palette-pane").count()) === 0 && (await headings())[0] === "동작");
    if ((await palette.locator(".palette-filters").count()) !== 0) bad.push("`>`인데 상태 칩이 있다");
    if (!(await firstOption()).includes("테마 전환")) bad.push(`>테마 첫 줄 ${await firstOption()}`);
    await dp.keyboard.press("Enter");
    await until("테마 바뀜", async () => (await theme()) !== before);
    // 찾으면 설정 쪽이 나온다: 「터미널」을 누르면 설정 창이 그 쪽으로 열린다
    await dp.keyboard.press("ControlOrMeta+Shift+K");
    await search.fill("터미널");
    const termPage = palette.locator('[data-section="settings-pages"]').getByRole("option", { name: "터미널" });
    await termPage.click();
    const dialog = dp.getByRole("dialog", { name: "설정" });
    await dialog.getByRole("tabpanel", { name: "터미널" }).waitFor();
    await dp.keyboard.press("Escape");
    await dialog.waitFor({ state: "detached" });
    // 테마를 되돌린다(공유 설정이라 뒤 단계의 화면도 바꾼다)
    await dp.keyboard.press("ControlOrMeta+Shift+K");
    await search.fill(">테마");
    await until("테마 전환 첫 줄", async () => (await firstOption()).includes("테마 전환"));
    await dp.keyboard.press("Enter");
    await until("테마 되돌림", async () => (await theme()) === before);
    await dp.context().close();
    // 휴대폰(터치·좁은 화면)에는 머리 돋보기가 없다(참고 styles.css `.palette-button`)
    const mp = (await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 })).page;
    await mp.getByTestId("pane-t1").waitFor();
    if (await mp.getByTestId("palette-open").isVisible()) bad.push("휴대폰에 돋보기가 보인다");
    await mp.context().close();
    for (const name of ["pala", "palb"]) aaa("pane", "kill", name);
    if (bad.length) throw new Error(bad.join("; "));
  });
}

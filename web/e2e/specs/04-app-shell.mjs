// 웹 E2E 앱 틀: 안전 영역·화면 켜짐 유지·앱 설치·agent 표시·목록 바닥 줄·목록 굴림 영역·명령 팔레트.
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 04-app-shell(새 실험실). 준비(host·agent·t1·첫 화면)는 lib/harness.mjs runSpec.

import fs from "node:fs";
import {
  webkit, AA, sleep, aaa, until, screenText, shot, fakeViewport, softKeyboard, pairPage, step, browser,
  openSettings, focusIs, inList, paneHash, base, root, showPanel,
} from "../lib/harness.mjs";

export default async function ({ ctl, page }) {
  await step("안전 영역(herdr-web-ui styles.css safe-area-inset, 차이 목록 M3): 가로로 둔 휴대폰의 노치를 목록은 왼쪽, 터미널 칸은 오른쪽(좁으면 양쪽)으로 비킨다", async () => {
    // 가로 휴대폰(844×390, 두 칸)과 좁은 가로(667×375, 한 칸). Chromium이 env(safe-area-inset-*)를 이 값으로 준다
    const measure = (p) =>
      p.evaluate(() => {
        const pad = (sel) => {
          const el = document.querySelector(sel);
          if (!el || !el.getClientRects().length) return null;
          const cs = getComputedStyle(el);
          return [parseFloat(cs.paddingLeft), parseFloat(cs.paddingRight)];
        };
        return { list: pad(".list"), head: pad(".term-head"), scroll: pad(".term-scroll"), keys: pad(".key-bar"), input: pad(".terminal-input") };
      });
    const open = async (viewport, line) => {
      const { page: p } = await pairPage(browser, aaa("web", "pair").code, viewport, { touch: true, line });
      const cdp = await p.context().newCDPSession(p);
      await cdp.send("Emulation.setSafeAreaInsetsOverride", { insets: { top: 0, left: 47, right: 47, bottom: 21 } });
      await inList(p, "pane-t1");
      await until("t1 화면", async () => (await screenText(p)).includes("$"));
      return p;
    };
    const wide = await open({ width: 844, height: 390 });
    const w = await measure(wide);
    const want = { list: [47, 0], head: [16, 12 + 47], scroll: [16, 16 + 47], keys: [8, 8 + 47] };
    for (const [k, v] of Object.entries(want)) if (JSON.stringify(w[k]) !== JSON.stringify(v)) throw new Error(`가로 두 칸 ${k} ${JSON.stringify(w[k])} ≠ ${JSON.stringify(v)} (${JSON.stringify(w)})`);
    await shot(wide, "phone-landscape-safe-area");
    await wide.context().close();
    // 한 칸씩 보이는 좁은 가로: 터미널 칸이 양쪽을 비킨다(입력줄 화면)
    const narrow = await open({ width: 667, height: 375 }, true);
    const n = await measure(narrow);
    const wantN = { head: [8 + 47, 8 + 47], scroll: [8 + 47, 8 + 47], keys: [8 + 47, 8 + 47], input: [8 + 47, 8 + 47] };
    for (const [k, v] of Object.entries(wantN)) if (JSON.stringify(n[k]) !== JSON.stringify(v)) throw new Error(`좁은 가로 ${k} ${JSON.stringify(n[k])} ≠ ${JSON.stringify(v)} (${JSON.stringify(n)})`);
    // 목록은 pane 위의 drawer다(참고 styles.css .sidebar): 왼쪽 끝에 붙어 왼쪽 노치만 비킨다(오른쪽 끝에 닿지 않는다)
    await narrow.locator(".view [data-testid='drawer-toggle']").click();
    await narrow.locator("#workspace-drawer.is-open").waitFor();
    const l = await measure(narrow);
    if (JSON.stringify(l.list) !== "[47,0]") throw new Error(`좁은 가로 목록 ${JSON.stringify(l.list)}`);
    await narrow.context().close();
  });

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
    await inList(wp, "pane-t1");
    // 앞 단계가 남긴 글에 기대지 않는다: 셸 프롬프트가 보이면 연결된 것이다
    await until("t1 화면", async () => (await screenText(wp)).includes("$"));
    await decided(`off free ${t1}`);
    if ((await log()) !== "") throw new Error(`꺼진 설정인데 ${await log()}`);
    if (!(await wp.evaluate(() => window.isSecureContext))) throw new Error("시험 주소가 secure context가 아니다");
    const toggle = async () => {
      await (await openSettings(wp, "폰 및 기기")).getByRole("switch", { name: "화면 켜짐 유지" }).click();
      await wp.keyboard.press("Escape");
    };
    // 보는 pane이 있으니(참고 App.tsx 842줄 `selectedPane !== null`) 켜면 바로 요청한다
    await toggle();
    if ((await wp.evaluate(() => localStorage.getItem("aaa:keep-screen-on"))) !== "1") throw new Error("켠 설정이 저장되지 않았다");
    await decided(`on hold ${t1}`);
    await until("wake lock 요청", async () => (await log()) === "request screen");
    // 목록 drawer가 열려 있어도(설정은 drawer 바닥 톱니에서 열었고 닫으면 그 drawer로 돌아온다) pane은 골라진 채다: 놓지 않는다
    await wp.locator("#workspace-drawer.is-open").waitFor();
    await decided(`on hold ${t1}`);
    if ((await log()) !== "request screen") throw new Error(`drawer를 열었는데 ${await log()}`);
    await wp.keyboard.press("Escape");
    await hide(true);
    await until("가려지면 놓음", async () => (await log()) === "request screen,release");
    await hide(false);
    await until("다시 보이면 다시 요청", async () => (await log()) === "request screen,release,request screen");
    // 다시 열어도 이 기기 설정. 주소(#pane=)가 그 pane을 다시 연다
    await wp.reload();
    await until("다시 연 화면도 요청", async () => (await log()) === "request screen");
    await toggle();
    await until("끄면 놓음", async () => (await log()) === "request screen,release");
    if ((await wp.evaluate(() => localStorage.getItem("aaa:keep-screen-on"))) !== null) throw new Error("끈 설정이 남았다");
    await inList(wp, "pane-t1");
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
      // 설치 단추는 목록에 있다(휴대폰은 drawer 안)
      await inList(p, "install-app");
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
    await sp.getByTestId("settings-open").waitFor({ state: "attached" });
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
    // 목록은 drawer다: 열어 자리에 선 뒤 찍는다
    await mp.locator(".view [data-testid='drawer-toggle']").click();
    await mp.locator("#workspace-drawer.is-open").waitFor();
    await until("drawer 자리", () => mp.evaluate(() => document.getElementById("workspace-drawer").getAnimations().length === 0));
    await shot(mp, "phone-agent-marks");
    // 터미널 머리: 제목 앞에 그 pane의 agent 표시(참고 App.tsx 1402줄, 18px)
    await inList(mp, "pane-markc");
    const head = mp.locator(".term-title .agent-mark");
    await head.waitFor();
    const hc = await head.evaluate((a) => a.className);
    if (hc !== "agent-mark claude is-brand") bad.push(`머리 표시 ${hc}`);
    await shot(mp, "phone-agent-head");
    await inList(mp, "pane-t1");
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
      await inList(pad, "pane-t1");
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

  await step("목록 두 굴림 영역(사용자 10-19 「projects와 agents의 스크롤바 위치가 달라」): 위 목록과 Agents 목록이 둘 다 넘칠 때 두 굴림 영역이 사이드바 같은 x에 서서 막대가 같은 자리에 온다", async () => {
    // 참고 AgentSidebar.css는 Agents 묶음에 좌우 여백을 줘 그 굴림 영역(과 막대)이 위 목록보다 여백만큼 안쪽이었다(Sidebar.css).
    // 둘은 활동 막대의 다른 패널이다(plan §8.238): 패널을 바꿔도 막대가 같은 x에 있는지 패널마다 잰다. 에이전트 패널은 사이드바
    // 높이를 다 가져 넘치려면 agent가 24개 있어야 한다
    const agent = `${root}/fakebin/claude`;
    fs.mkdirSync(`${root}/fakebin`, { recursive: true });
    fs.writeFileSync(agent, "#!/bin/sh\nexec /bin/sleep 600\n", { mode: 0o755 });
    const names = [...Array.from({ length: 16 }, (_, i) => `lane${i + 1}`), ...Array.from({ length: 24 }, (_, i) => `lagent${i + 1}`)];
    for (const name of names) aaa("tab", "create", "--machine", "labA", "--label", name, "--", ...(name.startsWith("lagent") ? [agent] : ["/bin/sleep", "600"]));
    const desk = (await pairPage(browser, aaa("web", "pair").code, { width: 1100, height: 760 })).page;
    await until("두 목록에 마지막 줄", async () => (await desk.getByTestId("pane-lane16").count()) === 1 && (await desk.locator(".agent-list .agent-item").count()) >= 24);
    const lane = (s) =>
      desk.evaluate((s) => {
        const e = document.querySelector(s);
        const r = e.getBoundingClientRect();
        return { s, left: Math.round(r.left), right: Math.round(r.right), over: e.scrollHeight - e.clientHeight };
      }, s);
    const lanes = [];
    for (const [panel, s] of [["projects", ".machine-list"], ["agents", ".agent-list-contents"]]) {
      await showPanel(desk, panel);
      lanes.push(await lane(s));
    }
    await desk.context().close();
    for (const name of names) aaa("pane", "kill", name);
    const [list, agents] = lanes;
    if (list.over <= 0 || agents.over <= 0) throw new Error(`두 목록이 다 넘쳐야 시험이 된다: ${JSON.stringify(lanes)}`);
    if (list.left !== agents.left || list.right !== agents.right) throw new Error(`두 굴림 영역의 x가 다르다: ${JSON.stringify(lanes)}`);
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

    // 돋보기로 열면 검색 칸이 focus를 받고, 찾지 않은 목록은 머신의 pane 묶음과 동작이다. focus는 연 다음 frame에 온다
    // (useFocusTrap requestAnimationFrame): 보이자마자 보면 부하 아래 그 frame 앞이다(ci-local 10-18 09:2x)
    await dp.getByTestId("palette-open").click();
    await search.waitFor();
    await focusIs(dp, "열면 검색 칸", '[data-testid="command-palette"] input[type="search"]');
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
    await mp.getByTestId("pane-t1").waitFor({ state: "attached" });
    if (await mp.getByTestId("palette-open").isVisible()) bad.push("휴대폰에 돋보기가 보인다");
    await mp.context().close();
    for (const name of ["pala", "palb"]) aaa("pane", "kill", name);
    if (bad.length) throw new Error(bad.join("; "));
  });

  await step("목록 drawer(herdr-web-ui App.tsx drawerOpen·edgeSwipe.ts·styles.css 768px, 차이 목록 M1·D2): 768px 이하는 목록이 pane 위 drawer, 머리 단추·왼쪽 가장자리 쓸기로 열고 바깥 누름·왼쪽 쓸기·Esc로 닫으며, 줄을 고르면 닫히고, 넓어지면 옆 칸", async () => {
    const bad = [];
    const P = await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 });
    const pp = P.page;
    const toggle = pp.locator(".view [data-testid='drawer-toggle']");
    const drawer = pp.locator("#workspace-drawer");
    // 열림: 단추가 말하고(aria-expanded), drawer가 보이고(visibility), 자리에 섰다(transform 끝)
    const shown = () =>
      pp.evaluate(() => {
        const d = document.getElementById("workspace-drawer");
        const r = d.getBoundingClientRect();
        return { open: d.classList.contains("is-open"), visible: getComputedStyle(d).visibility === "visible", left: Math.round(r.left), width: Math.round(r.width) };
      });
    const settled = (want, why = "") =>
      until(`${want ? "drawer 열림" : "drawer 닫힘"}${why}`, async () => {
        const s = await shown();
        const animating = await pp.evaluate(() => document.getElementById("workspace-drawer").getAnimations().length > 0);
        return !animating && s.open === want && (want ? s.visible && s.left === 0 : !s.visible);
      });
    await toggle.waitFor();
    if ((await toggle.getAttribute("aria-expanded")) !== "false") bad.push("처음부터 drawer가 열렸다");
    await settled(false, " #1");
    // 단추로 열고 닫기
    await toggle.click();
    await settled(true, " #2");
    if ((await toggle.getAttribute("aria-label")) !== "목록 닫기") bad.push(`연 단추 이름 ${await toggle.getAttribute("aria-label")}`);
    const w = (await shown()).width;
    if (w > Math.round(390 * 0.88) + 1) bad.push(`drawer 너비 ${w}(88vw 넘음)`);
    // 바닥 줄(설정 톱니)까지 화면 안: drawer 높이는 머리 아래부터 화면 끝까지다(.sidebar-shell height:100%가 밀어내지 않게)
    const foot = await pp.evaluate(() => [document.getElementById("workspace-drawer"), document.querySelector("[data-testid='settings-open']")].map((e) => Math.round(e.getBoundingClientRect().bottom)));
    if (foot.some((b) => b > 844)) bad.push(`drawer·톱니 아래 끝 ${foot} > 844`);
    await shot(pp, "phone-drawer-open");
    // 바깥(scrim) 누름
    await pp.locator(".scrim.is-open").click({ position: { x: 380, y: 600 } });
    await settled(false, " #3");
    // Esc: drawer 안 focus는 단추로 돌아온다
    await toggle.click();
    await settled(true, " #4");
    await pp.getByTestId("pane-t1").focus();
    await pp.keyboard.press("Escape");
    await settled(false, " #5");
    if (!(await pp.evaluate(() => document.activeElement?.dataset?.testid === "drawer-toggle"))) bad.push("Esc로 닫은 뒤 focus가 단추가 아니다");
    // 왼쪽 가장자리에서 쓸어 열고, 왼쪽으로 쓸어 닫는다(참고 EDGE_PX 24·SWIPE_PX 56)
    const cdp = await pp.context().newCDPSession(pp);
    const swipe = async (x0, x1) => {
      await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x: x0, y: 500, id: 1 }] });
      for (let i = 1; i <= 10; i++) await cdp.send("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: [{ x: x0 + ((x1 - x0) * i) / 10, y: 500, id: 1 }] });
      await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
    };
    await swipe(4, 300);
    await settled(true, " #6").catch((e) => bad.push(`가장자리 쓸기로 열리지 않았다: ${e.message}`));
    await swipe(300, 20);
    await settled(false, " #7").catch((e) => bad.push(`왼쪽 쓸기로 닫히지 않았다: ${e.message}`));
    // 가장자리가 아닌 곳의 옆 쓸기는 drawer 것이 아니다
    await swipe(120, 360);
    if ((await shown()).open) bad.push("가장자리가 아닌 쓸기로 열렸다");
    // 줄을 고르면 그 pane을 열고 닫힌다
    await toggle.click();
    await settled(true, " #8");
    await pp.getByTestId("pane-t1").click();
    await settled(false, " #9");
    await until("t1 화면", async () => (await screenText(pp)).includes("t1-42"));
    // 넓어지면 drawer가 아니라 옆 칸, 다시 좁아져도 닫힌 채(참고 wideScreen)
    await toggle.click();
    await settled(true, " #10");
    await pp.setViewportSize({ width: 1024, height: 768 });
    // 옆 칸은 CSS가 곧 보이지만 drawer를 닫는 것은 앱이 media query 사건을 받은 뒤다: 그것까지 기다리고 좁힌다
    await until("넓은 창 옆 칸", async () => !(await toggle.isVisible()) && (await pp.getByTestId("pane-t1").isVisible()) && !(await drawer.evaluate((d) => d.classList.contains("is-open"))));
    await pp.setViewportSize({ width: 390, height: 844 });
    await settled(false, " #11");
    await P.page.context().close();
    // iPad 세로(768)는 drawer, 가로(1024)는 옆 칸
    const I = await pairPage(browser, aaa("web", "pair").code, { width: 768, height: 1024 }, { touch: true });
    const ip = I.page;
    await inList(ip, "pane-t1");
    await until("iPad t1", async () => (await screenText(ip)).includes("t1-42"));
    await ip.locator(".view [data-testid='drawer-toggle']").click();
    await ip.locator("#workspace-drawer.is-open").waitFor();
    await until("iPad drawer 자리", () => ip.evaluate(() => document.getElementById("workspace-drawer").getAnimations().length === 0));
    await shot(ip, "ipad-drawer-open");
    await ip.setViewportSize({ width: 1024, height: 768 });
    await until("iPad 가로 옆 칸", async () => !(await ip.locator(".view [data-testid='drawer-toggle']").isVisible()) && (await ip.getByTestId("pane-t1").isVisible()));
    await shot(ip, "ipad-landscape-sidebar");
    await I.page.context().close();
    if (bad.length) throw new Error(bad.join("; "));
  });

  await step("첫 화면(herdr-web-ui App.tsx SELECTION_KEY·fallback, 차이 목록 M1): 주소 없이 열면 이 기기가 마지막으로 연 pane, 없거나 닫혔으면 붙은 머신의 첫 줄을 연다. 휴대폰은 키보드를 올리지 않고 데스크톱은 터미널이 키보드를 받는다", async () => {
    const bad = [];
    const typing = (p) => p.evaluate(() => document.activeElement?.classList.contains("xterm-helper-textarea") === true);
    const firstRow = (p) => p.evaluate(() => document.querySelector(".machine-list [data-testid^='pane-']")?.dataset.testid.slice(5));
    const opened = (p, name) => until(`첫 화면 ${name}`, async () => (await p.evaluate(() => location.hash)) === paneHash(name) && (await p.locator('[data-testid="terminal"]').getAttribute("data-pane-name")) === name);
    // 휴대폰: 기억한 것이 없으면 첫 줄
    const P = await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 }, { touch: true });
    const pp = P.page;
    await pp.locator(".machine-list [data-testid^='pane-']").first().waitFor({ state: "attached" });
    const first = await firstRow(pp);
    await opened(pp, first);
    if ((await pp.locator("#workspace-drawer").getAttribute("class")).includes("is-open")) bad.push("첫 화면에 drawer가 열렸다");
    if (await typing(pp)) bad.push("휴대폰 첫 화면이 키보드를 올렸다");
    await shot(pp, "phone-first-screen");
    // 다른 pane을 열면 이 기기가 기억한다: 같은 기기의 새 탭(주소 없음)은 그 pane
    aaa("tab", "create", "--machine", "labA", "--label", "firstpick", "--", "/bin/sh");
    await inList(pp, "pane-firstpick");
    await opened(pp, "firstpick");
    const tab = await pp.context().newPage();
    await tab.goto(base);
    await opened(tab, "firstpick");
    if (await typing(tab)) bad.push("기억한 pane을 연 휴대폰이 키보드를 올렸다");
    await tab.close();
    // 그 pane이 끝났으면(목록에는 끝난 줄로 남는다) 첫 줄
    aaa("pane", "kill", "firstpick");
    await until("firstpick 끝남", () => aaa("pane", "list").panes.find((x) => x.name === "firstpick")?.state !== "running");
    const again = await pp.context().newPage();
    await again.goto(base);
    await opened(again, first);
    await P.page.context().close();
    // 데스크톱: 첫 줄을 열고 바로 친다(참고 PaneTerminal 2072줄, 고른 것이 없던 첫 화면은 autoSelected가 아니다)
    const dp = (await pairPage(browser, aaa("web", "pair").code, { width: 1100, height: 760 })).page;
    await opened(dp, first);
    await until("데스크톱 첫 화면 터미널 focus", () => typing(dp)).catch((e) => bad.push(e.message));
    await dp.context().close();
    if (bad.length) throw new Error(bad.join("; "));
  });

  await step("「더 보기」와 닫힌 pane 주소(herdr-web-ui App.tsx header-more·resolveLink·headerNote·autoSelected): 「더 보기」는 화면 머리 맨 끝이라 휴대폰도 drawer 없이 닿고, 주소가 가리킨 pane이 없으면 첫 줄을 열며 머리에 말하고, 키보드는 사용자가 고를 때까지 둔다", async () => {
    const bad = [];
    const typing = (p) => p.evaluate(() => document.activeElement?.classList.contains("xterm-helper-textarea") === true);
    const firstRow = (p) => p.evaluate(() => document.querySelector(".machine-list [data-testid^='pane-']")?.dataset.testid.slice(5));
    const landed = async (p, what) => {
      await p.locator(".machine-list [data-testid^='pane-']").first().waitFor({ state: "attached" });
      const first = await firstRow(p);
      await until(`${what} 첫 줄`, async () => (await p.evaluate(() => location.hash)) === paneHash(first) && (await screenText(p)).includes("$"));
      const note = p.getByTestId("head-note");
      await note.waitFor();
      if ((await note.innerText()) !== "그 링크의 pane은 닫혔습니다") bad.push(`${what} 머리 알림 ${await note.innerText()}`);
      return first;
    };
    const moreAt = (p) =>
      p.getByTestId("more").evaluate((e) => ({ head: e.closest(".view header") !== null, drawer: e.closest("#workspace-drawer") !== null, right: Math.round(e.getBoundingClientRect().right) }));
    // 휴대폰: 닫힌 pane 주소로 연다
    const pp = (await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 }, { touch: true, hash: "#pane=gone" })).page;
    await landed(pp, "휴대폰");
    if (await typing(pp)) bad.push("휴대폰이 키보드를 올렸다");
    await pp.getByTestId("more").waitFor();
    const pm = await moreAt(pp);
    if (!pm.head || pm.drawer || pm.right < 390 - 48) bad.push(`휴대폰 「더 보기」 자리 ${JSON.stringify(pm)}`);
    if ((await pp.locator("#workspace-drawer").getAttribute("class")).includes("is-open")) bad.push("drawer가 열렸다");
    await pp.getByTestId("more").click();
    // 화면 안이고, 아래 칸에 덮이지 않았다(메뉴 가운데를 누르면 메뉴가 받는다)
    const menu = await pp.getByRole("menu", { name: "더 보기" }).evaluate((e) => {
      const r = e.getBoundingClientRect();
      const hit = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
      return { ...r.toJSON(), mine: e.contains(hit), hit: hit?.className ?? null };
    });
    if (menu.left < 0 || menu.right > 390 || menu.bottom > 844 || !menu.mine) bad.push(`휴대폰 메뉴가 화면 밖이거나 덮였다 ${JSON.stringify(menu)}`);
    await shot(pp, "phone-head-more");
    await pp.keyboard.press("Escape");
    // drawer가 열린 채(톱니로 연 설정을 닫은 뒤처럼)여도 메뉴는 drawer 위다(참고 RowMenu.css drawer + 1)
    await pp.locator(".view [data-testid='drawer-toggle']").click();
    await pp.locator("#workspace-drawer.is-open").waitFor();
    await pp.getByTestId("more").click();
    const over = await pp.getByTestId("alerts-item").evaluate((e) => {
      const r = e.getBoundingClientRect();
      return e.contains(document.elementFromPoint(r.left + 8, r.top + r.height / 2));
    });
    if (!over) bad.push("열린 drawer가 「더 보기」 메뉴를 덮는다");
    await pp.keyboard.press("Escape");
    await pp.context().close();
    // 데스크톱: 첫 줄을 열되 키보드는 두고, 사용자가 그 줄을 고르면 터미널이 받는다(참고 PaneTerminal 2209~2216줄)
    const dp = (await pairPage(browser, aaa("web", "pair").code, { width: 1100, height: 760 }, { hash: "#pane=gone" })).page;
    const first = await landed(dp, "데스크톱");
    if (await typing(dp)) bad.push("스스로 고른 pane이 키보드를 가져갔다");
    const dm = await moreAt(dp);
    if (!dm.head || dm.drawer) bad.push(`데스크톱 「더 보기」 자리 ${JSON.stringify(dm)}`);
    await shot(dp, "desktop-head-note");
    await inList(dp, `pane-${first}`);
    await until("고르면 터미널 focus", () => typing(dp)).catch((e) => bad.push(e.message));
    await dp.context().close();
    if (bad.length) throw new Error(bad.join("; "));
  });
}

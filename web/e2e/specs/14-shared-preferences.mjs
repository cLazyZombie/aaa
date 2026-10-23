// 웹 E2E 공유 설정: 기기 사이 테마·색상·사이드바 행, 보기 권한 거부(빈 공유 설정 문서에서 시작).
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 14-shared-preferences(새 실험실). 준비(host·agent·t1·첫 화면)는 lib/harness.mjs runSpec.

import { aaa, shot, pairPage, step, browser, webkit, openSettings, domIs, until, sentFrames, inList, screenText } from "../lib/harness.mjs";

export default async function ({ ctl, page }) {
  await step("공유 설정(herdr-web-ui preferencesSync·shared/preferences.ts, 차이 목록 T2): 한 기기에서 바꾼 테마·색상·사이드바 행은 다른 기기에 곧 오고, 기기 설정(상태 표시)은 남으며, 여는 것만으로는 쓰지 않고, 보기 권한 기기의 쓰기는 거부되며 다음 공유 변경을 모두 받는다, 바꾸고 곧 닫아도 간다(QA Q11)", async () => {
    // 기다림은 시간이 아니라 사건으로 끝낸다: <html> 속성은 MutationObserver, host 쓰기는 그 요청의 답
    const htmlIs = (p, name, value) =>
      p.evaluate(
        ([name, value]) =>
          new Promise((done) => {
            const ok = () => document.documentElement.getAttribute(name) === value;
            if (ok()) return done();
            const mo = new MutationObserver(() => ok() && (mo.disconnect(), done()));
            mo.observe(document.documentElement, { attributes: true, attributeFilter: [name] });
          }),
        [name, value],
      );
    const wrote = (p) =>
      p
        .waitForResponse((r) => r.url().endsWith("/api/v1/call") && r.request().postDataJSON()?.op === "preferences_set")
        .then(async (r) => ({ sent: r.request().postDataJSON(), ...(await r.json()) }));
    const keys = (o) => JSON.stringify(Object.keys(o).sort());
    const held = (p) =>
      p.evaluate(() =>
        fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ op: "preferences" }) })
          .then((r) => r.json())
          .then((o) => o.ok),
      );
    const settings = (p) => openSettings(p, "모양");
    const close = (p) => p.keyboard.press("Escape");
    const a = (await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 })).page;
    const b = (await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 })).page;
    await a.getByTestId("machine-labA").waitFor({ state: "attached" });
    await b.getByTestId("machine-labA").waitFor({ state: "attached" });
    // 여는 것만으로는 아무것도 쓰지 않는다
    const before = await held(a);
    if (before.revision !== 0) throw new Error(`열기 전에 공유 설정이 있다: ${JSON.stringify(before)}`);

    // 데스크톱에서 밝게 → 휴대폰도 밝게(새로 고치지 않음)
    let d = await settings(a);
    let sent = wrote(a);
    await d.getByRole("group", { name: "테마" }).getByRole("button", { name: "밝게" }).click();
    const first = await sent;
    // 무엇을 보내는지(바꾼 것만·처음 고정)는 web/test/preferences.test.mjs가 본다
    if (first.ok?.revision !== 1 || first.ok.settings.theme !== "light") throw new Error(`첫 쓰기 ${JSON.stringify(first)}`);
    await htmlIs(b, "data-theme", "light");
    // 휴대폰에서 색상 Catppuccin과 상태 표시 「점」(기기 설정) → 데스크톱은 색상만 받는다
    d = await settings(b);
    await d.getByRole("group", { name: "상태 표시" }).getByRole("button", { name: "점" }).click();
    sent = wrote(b);
    await d.locator("#settings-palette").selectOption("catppuccin");
    const second = await sent;
    if (keys(second.ok.settings) !== keys({ palette: 1, theme: 1 })) throw new Error(`host 문서에 기기 설정이 섞였다: ${JSON.stringify(second)}`);
    await htmlIs(a, "data-palette", "catppuccin");
    await close(b);
    await shot(a, "desktop-shared-light-catppuccin");

    // 보기 권한 기기: 열면 host 문서를 받고, 제 변경은 거부되어 이 기기에만 남다가 다음 공유 변경을 모두 받는다
    const r = (await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 390, height: 844 })).page;
    await htmlIs(r, "data-palette", "catppuccin");
    await htmlIs(r, "data-theme", "light");
    d = await settings(r);
    sent = wrote(r);
    await d.getByRole("group", { name: "테마" }).getByRole("button", { name: "어둡게" }).click();
    const refused = await sent;
    if (refused.err?.code !== "forbidden") throw new Error(`보기 권한 쓰기가 거부되지 않았다: ${JSON.stringify(refused)}`);
    await htmlIs(r, "data-theme", "dark");
    if ((await held(a)).revision !== 2) throw new Error("거부된 쓰기가 host 문서를 바꿨다");
    await close(r);
    // 데스크톱에서 사이드바 한 줄 → 휴대폰은 한 줄, 보기 권한 기기는 밝음까지 모두 받는다
    sent = wrote(a);
    d = a.getByRole("region", { name: "설정" });
    await d.getByRole("group", { name: "사이드바 행" }).getByRole("button", { name: "한 줄" }).click();
    if ((await sent).ok?.revision !== 3) throw new Error("셋째 쓰기");
    await htmlIs(r, "data-theme", "light");
    // 설정을 닫으면 본문으로 돌아온다. 공유된 목록 모양은 drawer를 열어 실제로 본다.
    await b.locator(".view [data-testid='drawer-toggle']").click();
    await b.locator(".machine-workspaces:not(.is-two-line)").first().waitFor();
    await shot(r, "phone-reader-shared");

    // 바꾸고 곧바로 닫아도(보내기를 미루는 600ms 안) 값이 간다(QA t-0337 Q11): 떠날 때(pagehide) keepalive로 지금 보낸다
    const closing = [];
    const closeDiagnostics = {
      init: () => {
        const record = (event, extra = {}) => console.debug("preferences-leave " + JSON.stringify({ at: performance.now(), event, theme: document.documentElement.dataset.theme, ...extra }));
        for (const name of ["beforeunload", "pagehide"]) window.addEventListener(name, () => record(name));
        document.addEventListener("visibilitychange", () => record("visibility", { state: document.visibilityState }));
        const original = window.fetch;
        window.fetch = function (input, init) {
          try {
            const body = typeof init?.body === "string" ? JSON.parse(init.body) : null;
            if (body?.op === "preferences_set") record("send", { theme: body.settings?.theme, keepalive: init.keepalive });
          } catch { /* 진단은 보내기를 막지 않는다. */ }
          return original.apply(this, arguments);
        };
      },
      onPage: p => {
        p.on("console", m => { if (m.text().startsWith("preferences-leave ")) closing.push(JSON.parse(m.text().slice(18))); });
        p.on("close", () => closing.push({ event: "closed" }));
        p.on("requestfailed", r => { if (r.url().endsWith("/api/v1/call") && r.postDataJSON()?.op === "preferences_set") closing.push({ event: "failed", reason: r.failure()?.errorText }); });
      },
    };
    const q = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 }, closeDiagnostics);
    await htmlIs(q.page, "data-theme", "light");
    const qd = await settings(q.page);
    await qd.getByRole("group", { name: "테마" }).getByRole("button", { name: "어둡게" }).click();
    await q.page.close({ runBeforeUnload: true });
    // host 문서를 먼저 본다: 실패가 보내는 쪽(host에 안 닿음)인지 받는 쪽(host에는 왔는데 다른 기기가 못 받음)인지 가른다(spec 14 판정)
    const arrives = async (theme, what) => {
      let doc = null;
      try {
        await until(`${what} host 문서에`, async () => (doc = await held(a))?.settings.theme === theme);
      } catch (e) {
        const other = await b.evaluate(() => document.documentElement.getAttribute("data-theme"));
        throw new Error(`${e.message}: 보내는 쪽 — host 문서 revision ${doc?.revision} theme ${doc?.settings.theme}, 다른 기기 ${other}, 닫힘 ${JSON.stringify(closing)}`);
      }
      try {
        await domIs(b, `${what} 다른 기기에`, (t) => document.documentElement.getAttribute("data-theme") === t, theme);
      } catch (e) {
        throw new Error(`${e.message}: 받는 쪽 — host 문서에는 왔다(revision ${doc.revision})`);
      }
    };
    await arrives("dark", "닫힌 기기에서 바꾼 테마가");
    await q.page.context().close();
    // 미룬 보내기(600ms)가 이미 나간 뒤 닫아도 간다: 느린 CI에서는 누름과 닫음 사이가 600ms를 넘을 수 있다. 그때 나간 보통 요청은 문서와
    // 함께 끊기고, 떠날 때 보내기는 「가는 중」이라 다시 보내지 않았다(코드로 찾은 빈틈, plan §8.173). 그래서 보내기는 늘 keepalive다.
    // 로컬 host는 몇 ms에 답해 고치기 전에도 이 경우는 통과한다(결정적 재현 아님): 그 요청이 나간 것을 본 뒤 닫는 꼴만 지킨다
    const w = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 }, closeDiagnostics);
    await htmlIs(w.page, "data-theme", "dark");
    const wd = await settings(w.page);
    const issued = w.page.waitForRequest((x) => x.url().endsWith("/api/v1/call") && x.postDataJSON()?.op === "preferences_set");
    await wd.getByRole("group", { name: "테마" }).getByRole("button", { name: "밝게" }).click();
    await issued;
    await w.page.close({ runBeforeUnload: true });
    await arrives("light", "보내는 중에 닫힌 기기에서 바꾼 테마가");
    await w.page.context().close();
    // 클릭과 같은 task에서 마지막 저장 기회가 와도 React의 다음 effect를 기다리지 않는다.
    // 저장 타이머·응답은 늦추지 않고, 실제 fetch 경계에서 즉시 최신 값 한 번을 보냈는지 본다.
    for (const event of ["pagehide", "beforeunload", "visibilitychange"]) {
      const next = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 });
      const p = next.page;
      try {
        await htmlIs(p, "data-theme", "light");
        const dialog = await settings(p);
        const sent = await dialog.getByRole("group", { name: "테마" }).getByRole("button", { name: "어둡게" }).evaluate((button, event) => {
          const sends = [];
          const original = window.fetch;
          window.fetch = function (input, init) {
            const body = typeof init?.body === "string" ? JSON.parse(init.body) : null;
            if (body?.op === "preferences_set") sends.push({ body, keepalive: init.keepalive });
            return original.apply(this, arguments);
          };
          button.click();
          const leave = () => {
            if (event === "visibilitychange") {
              Object.defineProperty(document, "visibilityState", { configurable: true, value: "hidden" });
              document.dispatchEvent(new Event(event));
            } else window.dispatchEvent(new Event(event));
          };
          leave();
          leave(); // 같은 종료의 다른 알림이 가는 중인 요청을 중복 전송하지 않는다.
          return { theme: document.documentElement.dataset.theme, sends };
        }, event);
        if (sent.theme !== "dark" || sent.sends.length !== 1 || sent.sends[0].body.settings?.theme !== "dark" || sent.sends[0].keepalive !== true)
          throw new Error(`같은 task의 ${event} 저장: ${JSON.stringify(sent)}`);
        await arrives("dark", `${event}에서 바꾼 테마가`);
      } finally { await p.context().close(); }
      const restored = wrote(a);
      await a.getByRole("region", { name: "설정" }).getByRole("group", { name: "테마" }).getByRole("button", { name: "밝게" }).click();
      await restored;
      await htmlIs(b, "data-theme", "light");
    }
    for (const p of [a, b, r]) await p.context().close();
  });
  await step("GitHub 세 팔레트: 데스크톱·휴대폰에서 선택하고 다시 열어도 UI·ANSI 색과 설정이 남는다", async () => {
    const pane = aaa("pane", "list").panes.find((p) => p.name === "t1").id;
    aaa("pane", "run", pane, "printf '\\033[2J\\033[HGitHub theme / ANSI 16 colors\\n'; for i in 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15; do printf '\\033[38;5;%sm Color %s \\033[0m\\n' $i $i; done");
    const wk = await webkit.launch({ headless: true });
    try {
      for (const [device, engine, viewport] of [
        ["chromium-desktop", browser, { width: 1280, height: 800 }], ["chromium-phone", browser, { width: 390, height: 844 }],
        ["webkit-desktop", wk, { width: 1280, height: 800 }], ["webkit-phone", wk, { width: 390, height: 844 }],
      ]) {
        const p = (await pairPage(engine, aaa("web", "pair").code, viewport)).page;
        for (const [name, mode, bg] of [["github-light", "light", "#ffffff"], ["github-dark", "dark", "#24292e"], ["github-dark-dimmed", "dark", "#22272e"]]) {
          const d = await openSettings(p, "모양");
          const sent = p.waitForResponse((r) => r.url().endsWith("/api/v1/call") && r.request().postDataJSON()?.op === "preferences_set");
          await d.locator("#settings-palette").selectOption(name, { timeout: 3000 });
          const response = await sent;
          if (!response.ok()) throw new Error(`${name} 설정 저장 ${response.status()}`);
          await domIs(p, "html", (want) => document.documentElement.dataset.palette === want, name);
          const look = await p.evaluate(() => {
            const css = getComputedStyle(document.documentElement);
            const token = (name) => css.getPropertyValue(name).trim().replace(/^#([0-9a-f])([0-9a-f])([0-9a-f])$/i, "#$1$1$2$2$3$3");
            return { mode: document.documentElement.dataset.theme, bg: token("--term-bg"), ansi: Array.from({ length: 16 }, (_, i) => token(`--ansi-${i}`)), palette: JSON.parse(localStorage.getItem("aaa:appearance")).palette };
          });
          if (look.mode !== mode || look.bg !== bg || look.palette !== name || look.ansi.some((c) => !/^#[0-9a-f]{6}$/i.test(c))) throw new Error(`${name}: ${JSON.stringify(look)}`);
          const colors = (await sentFrames(p, 0)).filter((f) => f.t === "colors");
          if (!colors.length || colors.some((f) => !/^[0-9a-f]{6}$/i.test(f.fg) || !/^[0-9a-f]{6}$/i.test(f.bg))) throw new Error(`${name} 터미널 기본색 protocol: ${JSON.stringify(colors)}`);
          const contrasts = await p.evaluate((palette) => {
            const css = getComputedStyle(document.documentElement);
            const color = (name) => css.getPropertyValue(`--${name}`).trim().replace(/^#([0-9a-f])([0-9a-f])([0-9a-f])$/i, "#$1$1$2$2$3$3");
            const lum = (hex) => hex.slice(1).match(/../g).map((x) => parseInt(x, 16) / 255).map((v) => v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4).reduce((s, v, i) => s + v * [0.2126, 0.7152, 0.0722][i], 0);
            const ratio = (a, b) => (Math.max(lum(a), lum(b)) + 0.05) / (Math.min(lum(a), lum(b)) + 0.05);
            const pairs = ["text", "text-dim", "accent", "status-idle", "status-working", "status-blocked", "status-done", "notice"].flatMap((fg) => ["bg", "bg-panel", "bg-hover"].map((bg) => [fg, bg]));
            pairs.push(["primary-text", "primary"], ["primary-text", "primary-hover"]);
            // 사용자 지정 GitHub Dark는 ANSI 원색을 보존한다. UI 대비는 다른 팔레트와 같은 기준이다.
            if (palette !== "github-dark") pairs.push(...Array.from({ length: 16 }, (_, i) => [`ansi-${i}`, "term-bg"]));
            return pairs.map(([fg, bg]) => ({ fg, bg, ratio: ratio(color(fg), color(bg)) }));
          }, name);
          if (contrasts.some((c) => c.ratio < 4.5)) throw new Error(`${name} 대비: ${JSON.stringify(contrasts.filter((c) => c.ratio < 4.5))}`);
          console.log(`     ${device} ${name}: 최소 대비 ${Math.min(...contrasts.map((c) => c.ratio)).toFixed(2)}:1`);
          await shot(p, `${device}-${name}-settings`);
          await d.getByRole("button", { name: "설정 닫기" }).click();
          await d.waitFor({ state: "detached" });
          await inList(p, "pane-t1");
          await until("ANSI 화면", async () => (await screenText(p)).includes("Color 15"));
          await shot(p, `${device}-${name}`);
          // 새로 읽기는 localStorage뿐 아니라 host 공유 설정을 다시 받는 실제 경로다.
          await p.reload();
          await domIs(p, "html", (want) => document.documentElement.dataset.palette === want, name);
        }
        await p.context().close();
      }
    } finally { await wk.close(); }
  });


  await step("설정 되돌리기: host 사건은 왔지만 이전 HTTP 응답만 유실돼도 다시 켠 자동 전송은 남아 다음 쓰기로 간다", async () => {
    let offSeen = false;
    const client = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 }, {
      serviceWorkers: "block",
      onPage: p => p.on("websocket", ws => ws.on("framereceived", frame => {
        if (typeof frame.payload !== "string") return;
        try { const v = JSON.parse(frame.payload); if (v.ev === "preferences" && v.settings?.autoSendQueued === false) offSeen = true; } catch {}
      })),
    });
    const p = client.page;
    let release;
    const gate = new Promise(resolve => { release = resolve; });
    const hold = async route => {
      const v = route.request().postDataJSON();
      if (v?.op === "preferences_set" && v.settings?.autoSendQueued === false) {
        await route.fetch(); // host에 실제 적용하고 events도 보내되 HTTP 답만 잃는다.
        await gate;
        await route.abort("failed");
      } else await route.continue();
    };
    await p.route("**/api/v1/call", hold);
    try {
      const d = await openSettings(p, "채팅");
      const toggle = d.getByRole("switch", { name: "대기 메시지 자동 전송" });
      if (await toggle.getAttribute("aria-checked") !== "true") throw new Error("초기 자동 전송이 켜져 있지 않다");
      await toggle.click();
      await until("host의 끔 사건 수신", () => offSeen);
      await toggle.click();
      let resent;
      p.on("request", r => { const v = r.url().endsWith("/api/v1/call") ? r.postDataJSON() : null; if (v?.op === "preferences_set" && v.settings?.autoSendQueued === true) resent = r; });
      const failed = p.waitForEvent("requestfailed", r => r.url().endsWith("/api/v1/call") && r.postDataJSON()?.settings?.autoSendQueued === false);
      release();
      await failed;
      await until("유실 응답 뒤 로컬 변경 처리", async () => !!resent || await toggle.getAttribute("aria-checked") !== "true");
      if (await toggle.getAttribute("aria-checked") !== "true") throw new Error("유실된 끔 응답의 보관 사건이 새 켬을 덮었다");
      const response = await resent.response();
      const saved = await response.json();
      if (saved.ok?.settings?.autoSendQueued !== true) throw new Error("되돌린 켬을 host에 다시 쓰지 못했다");
      console.log('quick-reply-lost-response {"keptOn":true,"resentOn":true,"hostOn":true}');
    } finally {
      release();
      await p.unroute("**/api/v1/call", hold);
      await p.context().close();
    }
  });

}

// 웹 E2E 설정: 터미널 글꼴·구독 사용량·글자 크기·설정 쪽(넓은 창·휴대폰·iPad).
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 06-settings(새 실험실). 준비(host·agent·t1·첫 화면)는 lib/harness.mjs runSpec.

import path from "node:path";
import {
  shots, root, sleep, aaa, waitOutput, until, screenText, shot, pairPage, step, usageHits, browser, openSettings, webkit,
} from "../lib/harness.mjs";

export default async function ({ ctl, page }) {
  await step("터미널 글꼴(herdr-web-ui 설정 「터미널 글꼴」, 차이 목록 「글꼴 종류」): 적은 목록을 거르고 기본 글꼴 앞에, face를 받은 뒤 cell을 재고, 찾아보기는 고정폭만, 비우면 기본", async () => {
    aaa("tab", "create", "--machine", "labA", "--label", "fontset", "--", process.execPath, `${root}/scroll.js`, "plain");
    await waitOutput("fontset", "s-200");
    // 시험 글꼴 「E2E Wide」: 넣은 Meslotendard를 다른 주소로 150% 크기로 받는다(깔린 글꼴에 기대지 않는다).
    // 깔린 글꼴 목록(Chromium queryLocalFonts)은 정해 둔 셋으로 바꾼다. Helvetica는 비례폭이라 거절해야 한다.
    const init = () => {
      try {
        document.fonts.add(new FontFace("E2E Wide", 'url("/fonts/meslotendard-0.3.1/Meslotendard-Regular.woff2?e2e-wide")', { sizeAdjust: "150%" }));
        window.queryLocalFonts = async () => [{ family: "Menlo" }, { family: "Helvetica" }, { family: "E2E Wide" }];
      } catch {}
    };
    const fp = (await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 }, { init })).page;
    // 그 face는 늦게 준다: 받기 전에 cell을 재면 대신 글꼴 너비가 남아 아래 1.5배 확인이 실패한다
    let wideFetches = 0;
    await fp.context().route((url) => url.search === "?e2e-wide", async (r) => {
      wideFetches += 1;
      await sleep(800);
      await r.continue();
    });
    await fp.getByTestId("pane-fontset").click();
    await until("fontset 화면", async () => (await screenText(fp)).includes("s-200"));
    // cell 너비 = 글자 영역 너비 / 열 수(머리의 cols×rows), 그리고 host PTY 열 수
    const cell = async () => {
      const m = await fp.evaluate(() => {
        const w = document.querySelector('[data-testid="terminal"] .xterm-screen').getBoundingClientRect().width;
        const cols = Number(/(\d+)×\d+/.exec(document.querySelector(".term-title .muted").textContent)?.[1]);
        return { w: w / cols, cols };
      });
      return { ...m, pty: aaa("pane", "list").panes.find((x) => x.name === "fontset").cols };
    };
    let last = null;
    const settle = (what, ok) =>
      until(what, async () => {
        last = await cell();
        return last.cols === last.pty && ok(last);
      }).catch((e) => {
        throw new Error(`${e.message} 마지막: ${JSON.stringify(last)}`);
      });
    await settle("처음 크기", () => true);
    const first = last;
    const field = () => fp.getByRole("dialog", { name: "설정" }).getByRole("textbox", { name: "터미널 글꼴" });
    const stored = () => fp.evaluate(() => localStorage.getItem("aaa:terminal-font-family"));
    // 찾아보기: 이름순, 검색, 비례폭 거절(값을 바꾸지 않음), 고정폭을 고르면 그것이 값
    const dlg = await openSettings(fp, "터미널");
    await dlg.getByRole("button", { name: "터미널 글꼴에 설치된 글꼴 찾아보기" }).click();
    const listed = await dlg.getByRole("list", { name: "설치된 글꼴" }).getByRole("button").allInnerTexts();
    if (JSON.stringify(listed) !== JSON.stringify(["E2E Wide", "Helvetica", "Menlo"])) throw new Error(`글꼴 목록 ${listed}`);
    await dlg.getByRole("searchbox", { name: "글꼴 검색" }).fill("hel");
    await dlg.getByRole("button", { name: "Helvetica" }).click();
    await dlg.getByText("Helvetica은(는) 고정폭 글꼴이 아닙니다.", { exact: false }).waitFor();
    await shot(fp, "desktop-terminal-font");
    if ((await field().inputValue()) !== "" || (await stored()) !== null) throw new Error("비례폭 글꼴을 받아들였다");
    // 검색칸의 Esc는 목록만 닫는다
    await dlg.getByRole("searchbox", { name: "글꼴 검색" }).press("Escape");
    await dlg.getByRole("list", { name: "설치된 글꼴" }).waitFor({ state: "detached" });
    await dlg.getByRole("button", { name: "터미널 글꼴에 설치된 글꼴 찾아보기" }).click();
    await dlg.getByRole("button", { name: "E2E Wide" }).click();
    if ((await field().inputValue()) !== '"E2E Wide"' || (await stored()) !== '"E2E Wide"') throw new Error(`고른 값 ${await field().inputValue()} / ${await stored()}`);
    // 그 face를 받은 뒤 넣으므로 cell은 1.5배, grid는 줄고 host PTY도 그 열 수
    await settle("넓은 글꼴로 다시 맞춤", (c) => Math.abs(c.w / first.w - 1.5) < 0.05 && c.cols < first.cols);
    if (wideFetches !== 1) throw new Error(`E2E Wide를 ${wideFetches}번 받았다`);
    // 적은 값은 거른다(세미콜론·중괄호를 빼고, 빈 이름을 버리고, 빈칸 있는 이름은 따옴표). Esc로 닫아도 저장한다
    await field().fill("'E2E Wide'; }, , Menlo");
    await fp.keyboard.press("Escape");
    await until("설정 닫힘", async () => (await fp.locator(".settings-dialog").count()) === 0);
    if ((await stored()) !== '"E2E Wide", Menlo') throw new Error(`거른 값 ${await stored()}`);
    // 다시 열어도 이 기기의 글꼴: 연 뒤 face를 받고 나서 잰다
    await fp.reload();
    await fp.getByTestId("pane-fontset").click();
    await settle("다시 연 화면도 넓은 글꼴", (c) => Math.abs(c.w / first.w - 1.5) < 0.05);
    if (wideFetches !== 2) throw new Error(`다시 열고 E2E Wide를 ${wideFetches}번째로 받지 않았다`);
    // 비우면(Enter) 기본 글꼴, 저장값도 지운다
    await openSettings(fp, "터미널");
    await field().fill("");
    await field().press("Enter");
    if ((await stored()) !== null) throw new Error(`비웠는데 저장값 ${await stored()}`);
    await fp.keyboard.press("Escape");
    await settle("기본 글꼴로 돌아옴", (c) => Math.abs(c.w - first.w) < 0.05 && c.cols === first.cols);
    await fp.context().close();
    aaa("pane", "kill", "fontset");
  });

  await step("구독 사용량·설정(herdr-web-ui UsageMeters·SettingsDialog): 목록 아래 톱니와 Claude·Codex 칩(herdr-web-ui AgentMark), 펼치면 한도·초기화까지, 다른 구독은 묻지 않음, host는 5분 캐시, 설정에서 남은 양·세션·숨기기·순서·입력 방식", async () => {
    const dp = (await pairPage(browser, aaa("web", "pair").code, { width: 1100, height: 760 })).page;
    const strip = dp.getByTestId("usage-strip");
    await strip.waitFor();
    const chips = () => strip.locator(".usage-chip").evaluateAll((els) => els.map((e) => `${e.dataset.provider} ${e.querySelector(".usage-chip-value").textContent}${e.classList.contains("is-high") ? " high" : ""}${e.classList.contains("has-problem") ? " problem" : ""}`));
    const bad = [];
    // 기본: 쓴 양·주간. 80% 넘으면 막힘 색. 늦추라고 한 구독은 숫자 없이 흐리게
    let got = JSON.stringify(await chips());
    // 구독은 Claude·Codex만 보인다: 이 집에 OpenCode 로그인이 있어도 묻지도 보이지도 않는다(사용자 요청 10-17)
    if (got !== JSON.stringify(["claude 95% high", "codex 73%"])) bad.push(`기본 칩 ${got}`);
    // 구독 표시는 herdr-web-ui AgentMark다: Claude는 주황 타일의 별, Codex는 글자색 매듭
    const marks = await strip.locator(".usage-chip .agent-mark").evaluateAll((els) =>
      els.map((e) => `${e.className} ${e.querySelector("svg").getAttribute("aria-label")} ${e.querySelector("svg").getAttribute("width")} ${e.querySelector("path").getAttribute("fill")}`),
    );
    if (JSON.stringify(marks) !== JSON.stringify(["agent-mark claude is-brand claude 14 #D77655", "agent-mark codex codex 14 currentColor"])) bad.push(`구독 표시 ${JSON.stringify(marks)}`);
    if ((await dp.getByTestId("settings-open").innerText()).trim() !== "설정") bad.push("톱니 「설정」이 없다");
    // 펼치면 구독마다 한도·초기화까지·계정·plan
    await strip.click();
    const pop = dp.locator(".usage-popover");
    await pop.waitFor();
    const claude = await pop.getByRole("region", { name: "Claude · me@example.com" }).innerText();
    for (const want of ["Max", "세션", "20%", "주간", "95%", "주간 · Sonnet", "40%", "뒤 초기화"]) if (!claude.includes(want)) bad.push(`펼친 Claude에 「${want}」가 없다: ${JSON.stringify(claude)}`);
    if ((await pop.getByRole("region", { name: "OpenCode" }).count()) !== 0) bad.push("펼친 창에 OpenCode가 있다");
    // 화면을 여러 번 열었어도 host는 구독마다 한 번만 물었다(5분 캐시). OpenCode에는 묻지 않았다
    await dp.reload();
    await strip.waitFor();
    await sleep(300);
    const hits = ["/api/oauth/usage", "/backend-api/wham/usage", "/zen/go/v1/usage"].map((p) => usageHits.get(p) ?? 0);
    if (JSON.stringify(hits) !== "[1,1,0]") bad.push(`구독에 물은 수 ${JSON.stringify(hits)}`);
    if (shots) await dp.screenshot({ path: path.join(shots, "usage-popover.png") });
    // 설정: 남은 양, 세션
    const dlg = await openSettings(dp, "구독 사용량");
    await dlg.getByRole("group", { name: "막대가 보이는 것" }).getByRole("button", { name: "남은 양" }).click();
    await until("남은 양", async () => JSON.stringify(await chips()) === JSON.stringify(["claude 5% high", "codex 27%"]));
    await dlg.getByRole("group", { name: "보이는 한도" }).getByRole("button", { name: "세션" }).click();
    await until("세션", async () => JSON.stringify(await chips()) === JSON.stringify(["claude 80%", "codex 90%"]));
    // 계정 숨기기와 순서
    const hideClaude = dlg.getByRole("switch", { name: "Claude · me@example.com 보이기" });
    await hideClaude.click();
    await until("숨기기", async () => JSON.stringify(await chips()) === JSON.stringify(["codex 90%"]));
    await hideClaude.click();
    await dlg.getByRole("button", { name: "Codex · me@example.com 위로" }).click();
    await until("순서", async () => JSON.stringify(await chips()) === JSON.stringify(["codex 90%", "claude 80%"]));
    if (shots) await dp.screenshot({ path: path.join(shots, "settings-dialog.png") });
    // 데스크톱에서 입력줄 고르기(D1에서 남은 빈 곳): 입력 방식은 「터미널」 쪽
    await dlg.getByRole("tab", { name: "터미널" }).click();
    await dlg.getByLabel("입력 방식").selectOption("line");
    await dp.keyboard.press("Escape");
    await until("설정 닫힘", async () => (await dp.getByRole("dialog", { name: "설정" }).count()) === 0);
    await dp.getByTestId("pane-t1").click();
    await until("입력줄", async () => (await dp.locator(".terminal-input").count()) === 1);
    // 다시 열어도 이 기기에 남는다. 끄면 막대도 사라진다
    await dp.reload();
    await until("남은 설정", async () => JSON.stringify(await chips()) === JSON.stringify(["codex 90%", "claude 80%"]));
    await (await openSettings(dp, "구독 사용량")).getByRole("switch", { name: "plan 한도 보이기" }).click();
    await until("막대 사라짐", async () => (await dp.getByTestId("usage-strip").count()) === 0);
    if (bad.length) throw new Error(bad.join("; "));
    await dp.context().close();
  });

  await step("글자 크기(herdr-web-ui terminalFontSize·핀치): 설정에서 한 칸씩, 휴대폰은 두 손가락으로 글자가 따라오고 손을 떼면 저장하고 PTY를 한 번 다시 건다", async () => {
    const t1 = () => aaa("pane", "list").panes.find((x) => x.name === "t1");
    const rowsFont = (pg) => pg.evaluate(() => getComputedStyle(document.querySelector('[data-testid="terminal"] .xterm-rows')).fontSize);
    /** 이 화면이 PTY 크기를 건 뒤 멈춘 열 수 */
    const settled = async (pg, what, ok) =>
      until(what, async () => {
        const a = t1().cols;
        await sleep(300);
        const b = t1().cols;
        return a === b && ok(b) ? b : null;
      });
    const dp = (await pairPage(browser, aaa("web", "pair").code, { width: 1100, height: 760 })).page;
    await dp.getByTestId("pane-t1").click();
    await until("13px로 연다", async () => (await rowsFont(dp)) === "13px");
    // 데스크톱(바로 입력)은 pane을 열면 터미널이 키보드를 받는다(herdr-web-ui PaneTerminal 2072줄). 입력줄이면 참고처럼 놓는다.
    await until("열면 터미널에 focus", () => dp.evaluate(() => document.activeElement?.classList.contains("xterm-helper-textarea")));
    await dp.getByTestId("terminal").click();
    const wide = await settled(dp, "데스크톱 크기", () => true);
    const dlg = await openSettings(dp, "터미널");
    for (let i = 0; i < 3; i++) await dlg.getByRole("button", { name: "글자 크게" }).click();
    if ((await dlg.getByRole("group", { name: "글자 크기" }).innerText()).trim() !== "16px") throw new Error("설정에 16px가 아니다");
    await until("16px", async () => (await rowsFont(dp)) === "16px");
    // 쓰는 화면이면 바뀐 글자로 PTY 크기를 다시 건다(13 → 16px이면 열이 줄어든다)
    const big = await settled(dp, "큰 글자 크기", (c) => c < wide * 0.9);
    if ((await dp.evaluate(() => localStorage.getItem("aaa:terminal-font-size"))) !== "16") throw new Error("16px 저장이 없다");
    for (let i = 0; i < 3; i++) await dlg.getByRole("button", { name: "글자 작게" }).click();
    await settled(dp, "13px로 돌아온 크기", (c) => c === wide).catch(async (e) => {
      // Term autoFit은 이 문서가 보이고 focus를 가질 때만 크기를 건다(inUse)
      const box = await dp.getByTestId("terminal").evaluate((el) => `${Math.round(el.getBoundingClientRect().width)}px, focus ${document.hasFocus()}, ${document.visibilityState}`);
      throw new Error(`${e.message}: 처음 ${wide}열, 지금 ${t1().cols}열, 글자 ${await rowsFont(dp)}, 설정 ${(await dlg.getByRole("group", { name: "글자 크기" }).innerText()).trim()}, 터미널 ${box}`);
    });
    // 기본값이면 저장하지 않는다
    if ((await dp.evaluate(() => localStorage.getItem("aaa:terminal-font-size"))) !== null) throw new Error("기본값을 저장했다");
    await dp.keyboard.press("Escape");
    await dp.context().close();
    console.log(`     글자 크기 ${wide}열(13px) → ${big}열(16px)`);

    // 휴대폰: 두 손가락을 벌리면 글자가 바로 커지고 크기를 띄운다. 손을 떼면 저장하고 PTY를 다시 건다.
    const mp = (await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 })).page;
    await mp.getByTestId("pane-t1").click();
    await until("휴대폰 13px", async () => (await rowsFont(mp)) === "13px");
    await mp.getByTestId("terminal").click();
    const phone = await settled(mp, "휴대폰 크기", (c) => c < wide);
    const r = await mp.getByTestId("terminal").boundingBox();
    const cx = r.x + r.width / 2;
    const cy = r.y + Math.min(r.height / 2, 200);
    const cdp = await mp.context().newCDPSession(mp);
    const touch = (type, d) =>
      cdp.send("Input.dispatchTouchEvent", {
        type,
        touchPoints: d === null ? [] : [{ x: cx - d / 2, y: cy, id: 1 }, { x: cx + d / 2, y: cy, id: 2 }],
      });
    await touch("touchStart", 100);
    await touch("touchMove", 130);
    await touch("touchMove", 150);
    // 13 × 150/100 = 19.5 → 20px. 손가락을 떼기 전에는 PTY도 저장도 그대로다
    await until("핀치 중 20px", async () => (await rowsFont(mp)) === "20px");
    const badge = await mp.getByTestId("pinch-badge").innerText();
    if (badge !== "20px") throw new Error(`핀치 표시 ${JSON.stringify(badge)}`);
    if (t1().cols !== phone) throw new Error("손을 떼기 전에 PTY가 바뀌었다");
    if ((await mp.evaluate(() => localStorage.getItem("aaa:terminal-font-size"))) !== null) throw new Error("손을 떼기 전에 저장했다");
    await touch("touchEnd", null);
    await until("핀치 표시 사라짐", async () => await mp.getByTestId("pinch-badge").isHidden());
    if ((await mp.evaluate(() => localStorage.getItem("aaa:terminal-font-size"))) !== "20") throw new Error("핀치 크기를 저장하지 않았다");
    const pinched = await settled(mp, "핀치 뒤 PTY", (c) => c < phone * 0.8);
    console.log(`     핀치 ${phone}열(13px) → ${pinched}열(20px)`);
    await mp.context().close();
  });

  await step("설정 쪽(herdr-web-ui SettingsDialog PAGES·settings-nav·settingsHistory, 차이 목록 T1): 넓은 창은 목록 옆에 쪽·화살표로 넘김, 휴대폰은 목록 → 쪽 → 뒤로, 시스템 뒤로는 한 걸음씩, Esc는 닫고 focus는 톱니로", async () => {
    const bad = [];
    const NAMES = ["모양", "터미널", "알림", "구독 사용량", "단축키", "폰 및 기기"];
    const tabs = (dialog) => dialog.getByRole("tab").allInnerTexts();
    const focused = (p) => p.evaluate(() => `${document.activeElement?.getAttribute("role") ?? document.activeElement?.tagName}:${document.activeElement?.textContent?.trim() || document.activeElement?.getAttribute("aria-label")}`);
    const closed = (p) => until("설정 닫힘", async () => (await p.getByRole("dialog", { name: "설정" }).count()) === 0);
    // 넓은 창: 목록과 첫 쪽(모양)이 나란히, 고른 줄에 focus, 화살표·End가 목록을 걷고 쪽도 넘긴다
    const dp = (await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 })).page;
    await dp.getByTestId("settings-open").click();
    const dlg = dp.getByRole("dialog", { name: "설정" });
    await dlg.getByRole("tabpanel", { name: "모양" }).waitFor();
    if (JSON.stringify(await tabs(dlg)) !== JSON.stringify(NAMES)) bad.push(`넓은 창 목록 ${await tabs(dlg)}`);
    const side = await dp.evaluate(() => {
      // 참고 배치: 목록은 왼쪽 열 전체, 오른쪽 열은 머리(쪽 제목·닫기) 아래에 쪽
      const nav = document.querySelector(".settings-nav").getBoundingClientRect();
      const head = document.querySelector(".settings-header").getBoundingClientRect();
      const body = document.querySelector(".settings-body").getBoundingClientRect();
      return { navRight: Math.round(nav.right), bodyLeft: Math.round(body.left), top: Math.abs(nav.top - head.top) < 2 && body.top >= head.bottom - 1 };
    });
    if (side.navRight > side.bodyLeft || !side.top) bad.push(`넓은 창 목록·쪽이 나란하지 않다 ${JSON.stringify(side)}`);
    await until("고른 줄에 focus", async () => (await focused(dp)) === "tab:모양").catch(async (e) => bad.push(`${e.message}: ${await focused(dp)}`));
    await dp.keyboard.press("ArrowDown");
    await dlg.getByRole("tabpanel", { name: "터미널" }).waitFor();
    if ((await dlg.getByRole("tab", { name: "터미널" }).getAttribute("aria-selected")) !== "true") bad.push("화살표로 넘긴 줄이 골라지지 않았다");
    await dp.keyboard.press("End");
    await dlg.getByRole("tabpanel", { name: "폰 및 기기" }).waitFor();
    await dp.keyboard.press("Home");
    await dlg.getByRole("tabpanel", { name: "모양" }).waitFor();
    await dlg.getByRole("tab", { name: "알림" }).click();
    await dlg.getByRole("tabpanel", { name: "알림" }).waitFor();
    // 고른 줄의 바탕은 transition으로 옮겨 간다(참고 .settings-tab): 다 옮긴 뒤 고른 줄만 바탕이 있는지 보고 찍는다
    await dp.mouse.move(1200, 780);
    const marks = await dp.evaluate(async () => {
      await Promise.all(document.getAnimations().map((a) => a.finished.catch(() => null)));
      return [...document.querySelectorAll(".settings-tab")].map((t) => `${t.textContent.trim()}:${getComputedStyle(t).backgroundColor !== "rgba(0, 0, 0, 0)"}`);
    });
    if (marks.join() !== "모양:false,터미널:false,알림:true,구독 사용량:false,단축키:false,폰 및 기기:false") bad.push(`고른 줄 바탕 ${marks}`);
    await shot(dp, "settings-pages-desktop");
    await dp.keyboard.press("Escape");
    await closed(dp);
    if ((await dp.evaluate(() => document.activeElement?.dataset.testid)) !== "settings-open") bad.push(`닫은 뒤 focus ${await focused(dp)}`);
    // 넓은 창도 쪽마다 history 한 칸: 뒤로가 창을 닫는다
    await openSettings(dp, "터미널");
    await dp.goBack();
    await closed(dp);
    await dp.context().close();

    // 휴대폰: 목록으로 열고(쪽 없음), 고르면 쪽만, ←·시스템 뒤로는 목록으로, 그 줄에 focus
    const pp = (await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 }, { touch: true })).page;
    await pp.getByTestId("settings-open").click();
    const pd = pp.getByRole("dialog", { name: "설정" });
    await pd.getByRole("tab", { name: "모양" }).waitFor();
    if ((await pd.getByRole("tabpanel").count()) !== 0) bad.push("휴대폰이 목록과 쪽을 함께 보인다");
    if (JSON.stringify(await tabs(pd)) !== JSON.stringify(NAMES)) bad.push(`휴대폰 목록 ${await tabs(pd)}`);
    // sheet는 아래에서 올라온다(modal.css sheet-in): 다 올라온 뒤 잰다
    const fit = await pp.evaluate(async () => {
      const sheet = document.querySelector(".settings-dialog");
      await Promise.all(sheet.getAnimations().map((a) => a.finished));
      const r = sheet.getBoundingClientRect();
      return { left: Math.round(r.left), right: Math.round(r.right), bottom: Math.round(r.bottom), page: document.documentElement.scrollWidth };
    });
    if (fit.left < 0 || fit.right > 390 || fit.bottom > 844 || fit.page > 390) bad.push(`휴대폰 설정 창이 넘친다 ${JSON.stringify(fit)}`);
    await shot(pp, "settings-pages-phone-list");
    await pd.getByRole("tab", { name: "터미널" }).click();
    await pd.getByRole("tabpanel", { name: "터미널" }).waitFor();
    if ((await pd.getByRole("tab").count()) !== 0) bad.push("휴대폰 쪽에 목록이 남았다");
    if ((await pd.locator(".modal-title").innerText()) !== "터미널") bad.push(`휴대폰 쪽 제목 ${await pd.locator(".modal-title").innerText()}`);
    await until("쪽으로 들어간 focus", async () => (await focused(pp)) === "BUTTON:설정으로 돌아가기").catch(async (e) => bad.push(`${e.message}: ${await focused(pp)}`));
    await shot(pp, "settings-pages-phone-terminal");
    // 키 바 편집 → 뒤로 → 터미널 쪽(「키 바 편집」에 focus) → 뒤로 → 목록(「터미널」 줄에 focus) → 뒤로 → 닫힘
    await pd.getByRole("button", { name: "키 바 편집" }).click();
    await pp.getByRole("dialog", { name: "키 바" }).waitFor();
    await pp.goBack();
    await pd.getByRole("tabpanel", { name: "터미널" }).waitFor();
    await until("키 바에서 돌아온 focus", async () => (await focused(pp)) === "BUTTON:키 바 편집").catch(async (e) => bad.push(`${e.message}: ${await focused(pp)}`));
    await pp.goBack();
    await pd.getByRole("tab", { name: "터미널" }).waitFor();
    await until("목록으로 나온 focus", async () => (await focused(pp)) === "tab:터미널").catch(async (e) => bad.push(`${e.message}: ${await focused(pp)}`));
    await pp.goBack();
    await closed(pp);
    // ←는 목록으로, Esc는 쪽에서도 창을 닫는다
    await openSettings(pp, "알림");
    await pp.getByRole("button", { name: "설정으로 돌아가기" }).click();
    await pd.getByRole("tab", { name: "알림" }).waitFor();
    await pd.getByRole("tab", { name: "폰 및 기기" }).click();
    await pd.getByRole("tabpanel", { name: "폰 및 기기" }).waitFor();
    await pp.keyboard.press("Escape");
    await closed(pp);
    // X로 닫으면 쌓은 칸을 지운다(settingsHistory): 지금 칸은 설정이 아니다
    await openSettings(pp, "모양");
    await pp.getByRole("button", { name: "설정 닫기" }).click();
    await closed(pp);
    await until("설정 칸이 지워짐", () => pp.evaluate(() => history.state?.["aaa:settings"] == null)).catch((e) => bad.push(e.message));
    await pp.context().close();

    // iPad(WebKit 820×1180): 넓은 창 배치(목록 옆에 쪽)
    const wk = await webkit.launch();
    try {
      const ip = (await pairPage(wk, aaa("web", "pair").code, { width: 820, height: 1180 }, { touch: true })).page;
      const id = await openSettings(ip, "폰 및 기기");
      if ((await id.getByRole("tab").count()) !== NAMES.length) bad.push("iPad에 목록이 없다");
      const over = await ip.evaluate(() => [...document.querySelectorAll(".settings-dialog *")].filter((e) => e.getBoundingClientRect().right > window.innerWidth + 0.5).length);
      if (over) bad.push(`iPad 설정 창 넘침 ${over}개`);
      await shot(ip, "settings-pages-ipad");
      await ip.context().close();
    } finally {
      await wk.close();
    }
    if (bad.length) throw new Error(bad.join("; "));
  });
}

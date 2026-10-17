// 웹 E2E 설정: 터미널 글꼴·구독 사용량·글자 크기·공유 설정.
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 06-settings(새 실험실). 준비(host·agent·t1·첫 화면)는 lib/harness.mjs runSpec.

import path from "node:path";
import {
  shots, root, sleep, aaa, waitOutput, until, screenText, shot, pairPage, step, usageHits, browser,
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
    await fp.getByTestId("settings-open").click();
    const dlg = fp.getByRole("dialog", { name: "설정" });
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
    await fp.getByTestId("settings-open").click();
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
    await dp.getByTestId("settings-open").click();
    const dlg = dp.getByRole("dialog", { name: "설정" });
    await dlg.waitFor();
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
    // 데스크톱에서 입력줄 고르기(D1에서 남은 빈 곳)
    await dlg.getByLabel("입력 방식").selectOption("line");
    await dp.keyboard.press("Escape");
    await until("설정 닫힘", async () => (await dp.getByRole("dialog", { name: "설정" }).count()) === 0);
    await dp.getByTestId("pane-t1").click();
    await until("입력줄", async () => (await dp.locator(".terminal-input").count()) === 1);
    // 다시 열어도 이 기기에 남는다. 끄면 막대도 사라진다
    await dp.reload();
    await until("남은 설정", async () => JSON.stringify(await chips()) === JSON.stringify(["codex 90%", "claude 80%"]));
    await dp.getByTestId("settings-open").click();
    await dp.getByRole("switch", { name: "plan 한도 보이기" }).click();
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
    await dp.getByTestId("settings-open").click();
    const dlg = dp.getByRole("dialog", { name: "설정" });
    for (let i = 0; i < 3; i++) await dlg.getByRole("button", { name: "글자 크게" }).click();
    if ((await dlg.getByRole("group", { name: "글자 크기" }).innerText()).trim() !== "16px") throw new Error("설정에 16px가 아니다");
    await until("16px", async () => (await rowsFont(dp)) === "16px");
    // 쓰는 화면이면 바뀐 글자로 PTY 크기를 다시 건다(13 → 16px이면 열이 줄어든다)
    const big = await settled(dp, "큰 글자 크기", (c) => c < wide * 0.9);
    if ((await dp.evaluate(() => localStorage.getItem("aaa:terminal-font-size"))) !== "16") throw new Error("16px 저장이 없다");
    for (let i = 0; i < 3; i++) await dlg.getByRole("button", { name: "글자 작게" }).click();
    await settled(dp, "13px로 돌아온 크기", (c) => c === wide);
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

  await step("공유 설정(herdr-web-ui preferencesSync·shared/preferences.ts, 차이 목록 T2): 한 기기에서 바꾼 테마·색상·사이드바 행은 다른 기기에 곧 오고, 기기 설정(상태 표시)은 남으며, 여는 것만으로는 쓰지 않고, 보기 권한 기기의 쓰기는 거부되며 다음 공유 변경을 모두 받는다", async () => {
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
    const settings = async (p) => {
      await p.getByTestId("settings-open").click();
      return p.getByRole("dialog", { name: "설정" });
    };
    const close = (p) => p.keyboard.press("Escape");
    const a = (await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 })).page;
    const b = (await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 })).page;
    await a.getByTestId("machine-labA").waitFor();
    await b.getByTestId("machine-labA").waitFor();
    // 여는 것만으로는 아무것도 쓰지 않는다
    const before = await held(a);
    if (before.revision !== 0) throw new Error(`열기 전에 공유 설정이 있다: ${JSON.stringify(before)}`);

    // 데스크톱에서 밝게 → 휴대폰도 밝게(새로 고치지 않음)
    let d = await settings(a);
    let sent = wrote(a);
    await d.getByRole("group", { name: "테마" }).getByRole("button", { name: "밝게" }).click();
    const first = await sent;
    // 바꾼 공유 설정만 보낸다. host가 아직 고정을 갖지 않아 이 기기 고정(빈 목록)이 함께 간다
    if (first.ok?.revision !== 1 || first.ok.settings.theme !== "light" || keys(first.sent.settings) !== keys({ theme: 1 }) || !Array.isArray(first.sent.pins))
      throw new Error(`첫 쓰기 ${JSON.stringify(first)}`);
    await htmlIs(b, "data-theme", "light");
    // 휴대폰에서 색상 Catppuccin과 상태 표시 「점」(기기 설정) → 데스크톱은 색상만 받는다
    d = await settings(b);
    await d.getByRole("group", { name: "상태 표시" }).getByRole("button", { name: "점" }).click();
    sent = wrote(b);
    await d.locator("#settings-palette").selectOption("catppuccin");
    const second = await sent;
    if (keys(second.sent.settings) !== keys({ palette: 1 }) || keys(second.ok.settings) !== keys({ palette: 1, theme: 1 }))
      throw new Error(`둘째 쓰기에 기기 설정이 섞였다: ${JSON.stringify(second)}`);
    await htmlIs(a, "data-palette", "catppuccin");
    const aLook = await a.evaluate(() => JSON.parse(localStorage.getItem("aaa:appearance") ?? "{}"));
    if (aLook.statusIndicators === "dots") throw new Error("기기 설정(상태 표시)이 다른 기기로 갔다");
    if (aLook.palette !== "catppuccin" || aLook.theme !== "light") throw new Error(`받은 값이 이 기기에 남지 않았다: ${JSON.stringify(aLook)}`);
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
    d = a.getByRole("dialog", { name: "설정" });
    await d.getByRole("group", { name: "사이드바 행" }).getByRole("button", { name: "한 줄" }).click();
    if ((await sent).ok?.revision !== 3) throw new Error("셋째 쓰기");
    await htmlIs(r, "data-theme", "light");
    await b.locator(".machine-workspaces:not(.is-two-line)").first().waitFor();
    await shot(r, "phone-reader-shared");

    // 되돌린다(뒤 단계의 화면이 기본 모양이게)
    sent = wrote(a);
    await d.getByRole("group", { name: "테마" }).getByRole("button", { name: "어둡게" }).click();
    await d.locator("#settings-palette").selectOption("amber");
    await d.getByRole("group", { name: "사이드바 행" }).getByRole("button", { name: "두 줄" }).click();
    await sent;
    await htmlIs(b, "data-palette", "amber");
    await htmlIs(b, "data-theme", "dark");
    await b.locator(".machine-workspaces.is-two-line").first().waitFor();
    for (const p of [a, b, r]) await p.context().close();
  });
}

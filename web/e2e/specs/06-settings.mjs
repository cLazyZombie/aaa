// 웹 E2E 설정: 터미널 글꼴·구독 사용량·글자 크기·설정 쪽(넓은 창·휴대폰·iPad)·밀도와 인터페이스 글자 크기.
// 혼자 돌리기: node web/e2e/web.e2e.mjs --spec 06-settings(새 실험실). 준비(host·agent·t1·첫 화면)는 lib/harness.mjs runSpec.

import { panelRows } from "../lib/panel-rows.mjs";
import path from "node:path";
import {
  shots, root, aaa, domIs, webkit, waitOutput, until, screenText, shot, pairPage, step, browser, openSettings, sentFrames, sentCount, inList, paneHash,
} from "../lib/harness.mjs";

export default async function ({ ctl, page }) {
  await step("터미널 색 테마: ANSI·여백·OSC 색 즉시 반영, 앱 팔레트와 독립·기기별 저장·다른 탭·재열기·기본 복원(Chromium 1280·WebKit 390)", async () => {
    const mobile = await webkit.launch({ headless: true });
    const name = "terminal-colors";
    const ansi = ["000000", "f78166", "56d364", "e3b341", "6ca4f8", "db61a2", "2b7489", "ffffff", "4d4d4d", "f78166", "56d364", "e3b341", "6ca4f8", "db61a2", "2b7489", "ffffff"];
    const output = ansi.map((_, n) => `\x1b[38;5;${n}mANSI_${n}\x1b[0m\r\n`).join("");
    aaa("tab", "create", "--machine", "labA", "--label", name, "--", process.execPath, "-e", `process.stdout.write(${JSON.stringify(output)});process.stdin.resume()`);
    try {
      for (const [engine, width, device] of [[browser, 1280, "desktop"], [mobile, 390, "phone"]]) {
        const pg = (await pairPage(engine, aaa("web", "pair").code, { width, height: 900 }, { hash: paneHash(name) })).page;
        try {
          await until("ANSI 출력", async () => (await screenText(pg)).includes("ANSI_15"));
          await pg.evaluate(async () => { await document.fonts.ready; });
          const read = () => pg.evaluate(() => {
            const style = getComputedStyle(document.querySelector(".term"));
            const rows = document.querySelector(".xterm-rows");
            const css = getComputedStyle(rows);
            return {
              body: getComputedStyle(document.body).backgroundColor,
              bg: getComputedStyle(document.querySelector(".term-scroll")).backgroundColor,
              xtermBg: (() => {
                // DOM renderer는 투명하다. 실제 글자 뒤에 처음 칠해진 조상의 바탕을 검사한다.
                for (let e = document.querySelector(".xterm"); e; e = e.parentElement) {
                  const bg = getComputedStyle(e).backgroundColor;
                  if (bg !== "transparent" && bg !== "rgba(0, 0, 0, 0)") return bg;
                }
                return null;
              })(),
              fg: css.color, font: css.fontFamily, size: css.fontSize,
              ansi: Array.from({ length: 16 }, (_, i) => style.getPropertyValue(`--ansi-${i}`).trim()),
              colors: [...rows.querySelectorAll("span")].filter((e) => /^ANSI_\d+$/.test(e.textContent)).map((e) => [e.textContent, getComputedStyle(e).color]),
              saved: localStorage.getItem("aaa:terminal-color-theme"),
            };
          });
          const baseline = await read();
          const close = async () => {
            await pg.getByRole("button", { name: "설정 닫기", exact: true }).click();
            await until("설정 기록 닫힘", () => !new URL(pg.url()).searchParams.has("settings"));
          };
          const choose = async (id) => {
            const dialog = await openSettings(pg, "터미널");
            const select = dialog.getByLabel("터미널 색 테마", { exact: true });
            if (id === "wezterm-github-dark-user" && await select.inputValue() !== "app") throw new Error("새 기기의 기본 테마가 앱이 아니다");
            if (await select.locator('option[value="wezterm-github-dark-user"]').textContent() !== "GitHub Dark") throw new Error("터미널 표시 이름 불일치");
            await select.selectOption(id);
            const rect = await select.boundingBox();
            if (!rect || rect.x < 0 || rect.x + rect.width > width) throw new Error(`${device}: 색 테마 선택 넘침 ${JSON.stringify(rect)}`);
            await close();
          };
          const mark = await sentCount(pg);
          await choose("wezterm-github-dark-user");
          await domIs(pg, "선택한 터미널 배경", () => getComputedStyle(document.querySelector(".term-scroll")).backgroundColor === "rgb(36, 41, 46)");
          await until("OSC 색 변경 전송", async () => (await sentFrames(pg, mark)).some((f) => f.t === "colors" && f.fg === "c9d1d9" && f.bg === "24292e"));
          const selected = await read();
          if (selected.saved !== "wezterm-github-dark-user" || selected.xtermBg !== selected.bg || selected.body !== baseline.body || selected.font !== baseline.font || selected.size !== baseline.size) throw new Error(`${device}: 테마 적용 ${JSON.stringify(selected)}`);
          const rgb = (hex) => `rgb(${hex.match(/../g).map((n) => parseInt(n, 16)).join(", ")})`;
          for (let i = 0; i < 16; i++) {
            if (selected.ansi[i].toLowerCase() !== `#${ansi[i]}` || !selected.colors.some(([text, color]) => text === `ANSI_${i}` && color === rgb(ansi[i]))) throw new Error(`${device}: ANSI ${i} 원색 미보존 ${JSON.stringify(selected)}`);
          }
          const appearance = await openSettings(pg, "모양");
          await appearance.locator("#settings-palette").selectOption("charcoal");
          await domIs(pg, "앱 팔레트만 변경", () => document.documentElement.dataset.palette === "charcoal" && getComputedStyle(document.querySelector(".term-scroll")).backgroundColor === "rgb(36, 41, 46)");
          await appearance.locator("#settings-palette").selectOption("amber");
          await domIs(pg, "앱 팔레트 복원", () => document.documentElement.dataset.palette === "amber");
          await close();
          await shot(pg, `${device}-terminal-color-theme`);
          const tab = await pg.context().newPage();
          await tab.goto(pg.url());
          await domIs(tab, "다른 탭 선택 복원", () => getComputedStyle(document.querySelector(".term-scroll")).backgroundColor === "rgb(36, 41, 46)");
          await pg.reload();
          await domIs(pg, "새로고침 선택 복원", () => getComputedStyle(document.querySelector(".term-scroll")).backgroundColor === "rgb(36, 41, 46)");
          const independent = (await pairPage(engine, aaa("web", "pair").code, { width, height: 900 })).page;
          try {
            if (await independent.evaluate(() => localStorage.getItem("aaa:terminal-color-theme")) !== null) throw new Error("다른 기기로 색 테마가 공유됐다");
          } finally { await independent.context().close(); }
          await choose("app");
          await domIs(tab, "다른 탭도 기본 복원", () => localStorage.getItem("aaa:terminal-color-theme") === null && !document.querySelector(".term").style.getPropertyValue("--term-bg"));
          await tab.close();
          await domIs(pg, "앱 팔레트 터미널 복원", (bg) => getComputedStyle(document.querySelector(".term-scroll")).backgroundColor === bg, baseline.bg);
          if ((await read()).saved !== null) throw new Error("기본 복원 뒤 저장 값이 남았다");
        } finally { await pg.context().close(); }
      }
    } finally { await mobile.close(); aaa("pane", "kill", name); }
  });

  await step("설정 본문·passkey 긴 값: Chromium·WebKit 밝음/어두움·1280/980/390/320px, 넘침 없음·딥링크·새로고침·뒤로/앞으로·사이드바 이동", async () => {
    const wk = await webkit.launch({ headless: true });
    try {
      for (const [engine, width, theme, name] of [
        [browser, 1280, "light", "desktop-light"], [browser, 1280, "dark", "desktop-dark"],
        [browser, 980, "light", "narrow-desktop"], [browser, 320, "dark", "phone-small"],
        [wk, 1280, "light", "webkit-desktop-light"], [wk, 1280, "dark", "webkit-desktop-dark"],
        [wk, 390, "light", "phone-light"], [wk, 390, "dark", "phone-dark"],
      ]) {
        const pg = (await pairPage(engine, aaa("web", "pair").code, { width, height: 900 }, { colorScheme: theme, serviceWorkers: "block" })).page;
        try {
          await pg.getByTestId("terminal").waitFor();
          const home = pg.url();
          // 실제 host의 /me 응답에서 passkey 상태만 대체한다. 10분 경과를 자면서 기다리지 않는다.
          let passkey = { registered: false, rp_id: "example.ts.net", register_until_ms: 0 };
          await pg.route("**/api/v1/me", async (route) => {
            const response = await route.fetch();
            const json = await response.json();
            json.ok.passkey = passkey;
            // 새 JSON의 길이·인코딩으로 응답한다. 원 응답의 전송 헤더를 재사용하지 않는다.
            await route.fulfill({ status: response.status(), contentType: "application/json", json });
          });
          const appearance = await openSettings(pg, "모양");
          await appearance.getByRole("group", { name: "테마", exact: true }).getByRole("button", { name: theme === "light" ? "밝게" : "어둡게", exact: true }).click();
          await until("테마 적용", () => pg.evaluate((want) => document.documentElement.dataset.theme === want, theme));
          // 메뉴는 앱 왼쪽 패널에, 내용만 main 전체 폭에 놓인다. 조작의 실제 hit target도 검사한다.
          const layout = await pg.evaluate(() => {
            const nav = document.querySelector('nav[aria-label="설정"]');
            const body = document.querySelector(".settings-body");
            const main = document.querySelector("main").getBoundingClientRect();
            const bounds = body.getBoundingClientRect();
            const shell = document.querySelector(".sidebar-shell");
            const controls = [...body.querySelectorAll('[aria-label="테마"] button, #settings-palette, [aria-label="밀도"] button')];
            return {
              sidebar: !!nav?.closest("#sidebar-column"),
              title: shell.querySelector(".list-title").textContent,
              color: nav && getComputedStyle(nav).backgroundColor === getComputedStyle(shell).backgroundColor,
              full: Math.abs(bounds.left - main.left) < 1 && Math.abs(bounds.right - main.right) < 1,
              controls: controls.length,
              clipped: controls.some((el) => {
                const r = el.getBoundingClientRect();
                const hit = document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2);
                return r.width <= 0 || r.left < main.left || r.right > main.right || !el.contains(hit);
              }),
            };
          });
          if (!layout.full || layout.controls < 6 || layout.clipped || (width > 768 && (!layout.sidebar || layout.title !== "설정" || !layout.color)))
            throw new Error(`${name}: 설정 메뉴·조작 배치 ${JSON.stringify(layout)}`);
          if (width > 768) await panelRows(pg, ".settings-tab");
          await shot(pg, `settings-appearance-${name}`);
          await appearance.getByRole("button", { name: "설정 닫기" }).click();
          await until("테마 설정 닫힘", () => !new URL(pg.url()).searchParams.has("settings"));
          let screen = await openSettings(pg, "폰 및 기기");
          const status = screen.getByTestId("passkey-status");
          const served = await pg.evaluate(async () => (await (await fetch("/api/v1/me")).json()).ok.passkey);
          if (!served || served.register_until_ms !== 0) throw new Error(`${name}: 시험용 상태 응답 ${JSON.stringify(served)}`);
          await status.filter({ hasText: "10분이 지나" }).waitFor();
          await screen.locator(".phone-panel [role=status]").waitFor({ state: "detached" });
          await screen.locator(".devices-list").waitFor();
          const fit = async () => screen.evaluate((el) => {
            const view = el.closest("main").getBoundingClientRect();
            const r = el.getBoundingClientRect();
            const body = el.querySelector(".settings-body");
            const row = el.querySelector('[data-testid="passkey-status"]')?.closest(".settings-row");
            const text = row?.querySelector(".settings-row-text").getBoundingClientRect();
            return {
              modal: !!el.closest(".modal-scrim") || el.hasAttribute("aria-modal"),
              fills: Math.abs(r.left - view.left) < 1 && Math.abs(r.right - view.right) < 1 && Math.abs(r.bottom - view.bottom) < 1,
              overflow: body.scrollWidth > body.clientWidth + 1 || document.documentElement.scrollWidth > innerWidth + 1,
              textWidth: text?.width,
              rowWidth: row?.clientWidth,
              squeezed: [...el.querySelectorAll(".settings-row-text")].some((node) => node.getBoundingClientRect().width < 100),
            };
          });
          const check = async () => {
            const f = await fit();
            if (f.modal || !f.fills || f.overflow || f.squeezed || f.textWidth < f.rowWidth * 0.7) throw new Error(`${name}: 설정 배치 ${JSON.stringify(f)}`);
          };
          await check();
          await screen.locator(".settings-body").evaluate((el) => { el.scrollTop = el.scrollHeight; });
          await shot(pg, `settings-full-${name}`);
          const link = pg.url();
          if (new URL(link).searchParams.get("settings") !== "devices") throw new Error(`${name}: 설정 주소 없음`);
          // 새로고침에도 같은 쪽. 새 탭의 주소도 설정을 연다.
          await pg.reload();
          screen = pg.getByRole("region", { name: "설정", exact: true });
          await screen.getByTestId("passkey-status").filter({ hasText: "10분이 지나" }).waitFor();
          await check();
          const linked = await pg.context().newPage();
          const shared = new URL(link);
          shared.hash = "";
          await linked.goto(shared.href);
          await linked.getByRole("tabpanel", { name: "폰 및 기기" }).waitFor();
          await linked.getByRole("button", { name: "설정 닫기" }).click();
          await until("공유 주소 닫기", () => !new URL(linked.url()).searchParams.has("settings"));
          await linked.close();
          // 확인 모달만 Esc로 닫힌다. 설정은 본문에 남는다.
          await screen.getByRole("button", { name: "로그아웃", exact: true }).click();
          const confirm = pg.getByRole("alertdialog", { name: "이 브라우저에서 로그아웃할까요?" });
          await confirm.waitFor();
          await pg.keyboard.press("Escape");
          await confirm.waitFor({ state: "detached" });
          if (!(await screen.isVisible())) throw new Error(`${name}: 확인 창 Esc가 설정도 닫았다`);
          await screen.getByRole("button", { name: "설정 닫기" }).click();
          await until("닫기 주소 복귀", () => pg.url() === home);
          await pg.goForward();
          await screen.waitFor();
          // 앞으로는 휴대폰에서 목록부터 복원될 수 있다.
          if (await screen.page().getByRole("navigation", { name: "설정", exact: true }).getByRole("tab", { name: "폰 및 기기" }).isVisible()) await screen.page().getByRole("navigation", { name: "설정", exact: true }).getByRole("tab", { name: "폰 및 기기" }).click();
          await screen.getByRole("tabpanel", { name: "폰 및 기기" }).waitFor();
          await screen.getByRole("button", { name: "설정 닫기" }).click();
          await until("다시 닫기 주소 복귀", () => pg.url() === home);
          // 긴 도메인의 등록 상태도 동일한 폭 안에서 접힌다.
          passkey = { ...passkey, registered: true, rp_id: `${"long-domain-".repeat(18)}example.ts.net` };
          screen = await openSettings(pg, "폰 및 기기");
          await screen.getByTestId("passkey-status").filter({ hasText: "등록됨" }).waitFor();
          await check();
          if (width === 1280) {
            // Tab은 본문에 갇히지 않고 앱 목록으로 나간다.
            await screen.getByRole("button", { name: "설정 닫기" }).focus();
            await pg.keyboard.press("Shift+Tab");
            if (!await pg.evaluate(() => document.getElementById("workspace-drawer").contains(document.activeElement))) throw new Error("설정 본문에서 Tab으로 앱 목록에 나가지 못했다");
            await pg.getByTestId("palette-open").click();
            const palette = pg.getByRole("dialog", { name: "명령 팔레트" });
            await palette.getByRole("searchbox").fill("알림");
            await palette.locator('[data-section="settings-pages"]').getByRole("option", { name: "알림", exact: true }).click();
            await screen.getByRole("tabpanel", { name: "알림" }).waitFor();
            await until("열린 설정 팔레트 이동", () => new URL(pg.url()).searchParams.get("settings") === "alerts");
          }
          // 설정 밖의 같은 pane 선택으로도 본문 복귀. history가 뒤늦게 덮지 않는다.
          await inList(pg, "pane-t1");
          await screen.waitFor({ state: "detached" });
          await pg.getByTestId("terminal").waitFor();
          await until("사이드바 이동 주소", () => !new URL(pg.url()).searchParams.has("settings"));
          // 다른 쪽의 긴 값·조작들도 본문을 넘치지 않는다.
          for (const name of ["모양", "터미널", "단축키", "알림"]) {
            screen = await openSettings(pg, name);
            if (name === "모양") await screen.getByRole("textbox", { name: "화면 글꼴", exact: true }).fill("긴글꼴이름".repeat(40));
            const f = await fit();
            if (f.overflow || f.squeezed) throw new Error(`${width}px ${name}: 다른 설정 줄 넘침 ${JSON.stringify(f)}`);
            await screen.getByRole("button", { name: "설정 닫기" }).click();
            await until("설정 기록 해제", () => !new URL(pg.url()).searchParams.has("settings"));
          }
          if (width === 1280) {
            screen = await openSettings(pg, "터미널");
            await screen.getByRole("button", { name: "키 바 편집" }).click();
            await pg.reload();
            const editor = pg.getByRole("region", { name: "키 바", exact: true });
            await editor.waitFor();
            await pg.goBack();
            await pg.getByRole("tabpanel", { name: "터미널" }).waitFor();
            await pg.getByRole("button", { name: "설정 닫기" }).click();
            await until("키 바 닫기", () => !new URL(pg.url()).searchParams.has("settings"));
          }
        } catch (error) {
          console.log(`설정 실패 ${name}: ${pg.url()} / ${await pg.locator(".settings-dialog").innerText().catch(() => "설정 없음")}`);
          await shot(pg, `settings-failure-${name}`);
          throw error;
        } finally { await pg.context().close(); }
      }
    } finally { await wk.close(); }
  });

  await step("화면·고정폭·터미널 글꼴: 즉시 적용·없는 글꼴 대체·아이콘 우선·기기별 저장·새로고침·기본값 복원(Chromium 1280·WebKit 390)", async () => {
    const mobileBrowser = await webkit.launch({ headless: true });
    try {
      for (const [engine, width, name] of [[browser, 1280, "desktop"], [mobileBrowser, 390, "phone"]]) {
        const pg = (await pairPage(engine, aaa("web", "pair").code, { width, height: 900 })).page;
        try {
          const fields = [
            { label: "화면 글꼴", page: "모양", key: "aaa:ui-font-family", variable: "--font-ui", family: '"E2E Missing UI", serif' },
            { label: "고정폭 글꼴", page: "모양", key: "aaa:mono-font-family", variable: "--font-mono", family: '"E2E Missing Mono", monospace' },
            { label: "터미널 글꼴", page: "터미널", key: "aaa:terminal-font-family", family: '"E2E Missing Term", monospace' },
          ];
          const read = () => pg.evaluate(() => {
            const style = getComputedStyle(document.documentElement);
            const code = document.createElement("code");
            code.textContent = "M0 경로";
            document.body.append(code);
            const mono = getComputedStyle(code).fontFamily;
            // 기대하는 가족·순서·generic을 그대로 CSSOM에 직렬화한다. WebKit 27은 공백 있는 이름의 따옴표를 생략한다.
            const serialize = (family) => { code.style.fontFamily = family; return getComputedStyle(code).fontFamily; };
            const expected = {
              ui: serialize('"E2E Missing UI", serif'),
              mono: serialize('"Symbols Nerd Font Mono", "E2E Missing Mono", monospace'),
              term: serialize('"E2E Missing Term", monospace'),
              otherTab: serialize('"E2E Other Tab", serif'),
            };
            code.remove();
            return {
              expected,
              ui: getComputedStyle(document.body).fontFamily,
              mono,
              term: getComputedStyle(document.querySelector(".xterm-rows")).fontFamily,
              uiDefault: style.getPropertyValue("--font-ui-default").trim(),
              monoDefault: style.getPropertyValue("--font-mono-default").trim(),
            };
          });
          await pg.locator(".xterm-rows").waitFor();
          await pg.evaluate(async () => { await document.fonts.ready; await new Promise(requestAnimationFrame); });
          const baseline = await read();
          const close = async () => {
            await pg.keyboard.press("Escape");
            await pg.getByRole("region", { name: "설정" }).waitFor({ state: "detached" });
          };
          for (const f of fields) {
            const dialog = await openSettings(pg, f.page);
            const input = dialog.getByRole("textbox", { name: f.label, exact: true });
            if (await input.inputValue() !== "") throw new Error(`${name}: 다른 기기의 ${f.label}이 넘어왔다`);
            // 없는 이름만 적으면 기존 목록과 같은 폭으로 그린다.
            await input.fill('"E2E Missing Font"');
            await input.press("Enter");
            if (f.variable) {
              const metrics = await pg.evaluate((variable) => {
                const style = getComputedStyle(document.documentElement);
                const canvas = document.createElement("canvas").getContext("2d");
                const size = (family) => { canvas.font = `16px ${family}`; return canvas.measureText("MWii 012 한글").width; };
                const base = style.getPropertyValue(`${variable}-default`);
                return [size(style.getPropertyValue(variable)), size(base)];
              }, f.variable);
              if (Math.abs(metrics[0] - metrics[1]) > 0.01) throw new Error(`${name}: ${f.label} 기본 목록 대체 실패 ${metrics}`);
            }
            await input.fill(f.family);
            await input.press("Enter");
            if (await pg.evaluate((key) => localStorage.getItem(key), f.key) !== f.family) throw new Error(`${name}: ${f.label} 저장 실패`);
            await close();
          }
          await domIs(pg, "터미널 글꼴 적용", () => getComputedStyle(document.querySelector(".xterm-rows")).fontFamily.includes("E2E Missing Term"));
          const chosen = await read();
          const check = (value) => {
            if (!value.ui.startsWith(`${baseline.expected.ui},`) || !value.ui.endsWith(baseline.ui)) throw new Error(`${name}: 화면 목록 ${value.ui}`);
            if (!value.mono.startsWith(`${baseline.expected.mono},`) || !value.mono.endsWith(baseline.mono)) throw new Error(`${name}: 아이콘·기본 목록 ${value.mono}`);
            if (!value.term.includes(baseline.expected.term) || !value.term.includes("Meslotendard")) throw new Error(`${name}: 터미널 목록 ${value.term}`);
          };
          check(chosen);
          // 같은 브라우저의 다른 탭은 storage 사건을 따르고, 새 기기는 값을 물려받지 않는다.
          const tab = await pg.context().newPage();
          await tab.goto(pg.url());
          await tab.getByTestId("machine-labA").waitFor({ state: "attached" });
          const dialog = await openSettings(pg, "모양");
          await dialog.getByRole("textbox", { name: "화면 글꼴", exact: true }).fill('"E2E Other Tab", serif');
          await dialog.getByRole("textbox", { name: "화면 글꼴", exact: true }).press("Enter");
          await domIs(tab, "다른 탭 화면 글꼴 적용", (family) => getComputedStyle(document.body).fontFamily.startsWith(`${family},`), baseline.expected.otherTab);
          await dialog.getByRole("textbox", { name: "화면 글꼴", exact: true }).fill(fields[0].family);
          await dialog.getByRole("textbox", { name: "화면 글꼴", exact: true }).press("Enter");
          await close();
          await tab.close();
          const other = (await pairPage(engine, aaa("web", "pair").code, { width, height: 900 })).page;
          try {
            const values = await other.evaluate((keys) => keys.map((key) => localStorage.getItem(key)), fields.map((f) => f.key));
            if (values.some((v) => v !== null)) throw new Error(`${name}: 다른 기기에 글꼴이 공유됐다`);
          } finally { await other.context().close(); }
          await pg.reload();
          await pg.getByTestId("machine-labA").waitFor({ state: "attached" });
          await domIs(pg, "재접속 터미널 글꼴", () => getComputedStyle(document.querySelector(".xterm-rows")).fontFamily.includes("E2E Missing Term"));
          check(await read());
          for (const section of ["모양", "터미널"]) {
            const dialog = await openSettings(pg, section);
            const last = fields.filter((f) => f.page === section).at(-1);
            await dialog.getByRole("textbox", { name: last.label, exact: true }).scrollIntoViewIfNeeded();
            await dialog.evaluate((el) => Promise.all(el.getAnimations({ subtree: true }).map((a) => a.finished.catch(() => null))));
            const overflow = await dialog.evaluate((el) => [...el.querySelectorAll("input, button")].filter((e) => {
              const r = e.getBoundingClientRect(); return r.width && (r.right > innerWidth + 0.5 || r.left < -0.5);
            }).map((e) => e.getAttribute("aria-label") ?? e.textContent));
            if (overflow.length) throw new Error(`${name}: 설정 가로 넘침 ${overflow}`);
            await shot(pg, `${name}-font-families-${section === "모양" ? "appearance" : "terminal"}`);
            for (const f of fields.filter((f) => f.page === section)) {
              const input = dialog.getByRole("textbox", { name: f.label, exact: true });
              if (await input.inputValue() !== f.family) throw new Error(`${name}: ${f.label} 새로고침 복원 실패`);
              await dialog.getByRole("button", { name: `${f.label} 기본값으로`, exact: true }).click();
              if (await input.inputValue() !== "" || await pg.evaluate((key) => localStorage.getItem(key), f.key) !== null) throw new Error(`${name}: ${f.label} 기본값 복원 실패`);
            }
            await close();
          }
          await domIs(pg, "기본 터미널 복원", (family) => getComputedStyle(document.querySelector(".xterm-rows")).fontFamily === family, baseline.term);
          const restored = await read();
          if (restored.ui !== baseline.ui || restored.mono !== baseline.mono) throw new Error(`${name}: 기본 화면·고정폭 복원 실패`);
          await pg.reload();
          await pg.getByTestId("machine-labA").waitFor({ state: "attached" });
          await domIs(pg, "새로고침 뒤 기본 터미널", (family) => getComputedStyle(document.querySelector(".xterm-rows")).fontFamily === family, baseline.term);
          if ((await read()).ui !== baseline.ui || (await read()).mono !== baseline.mono) throw new Error(`${name}: 기본값 새로고침 복원 실패`);
        } finally {
          await pg.context().close();
        }
      }
    } finally {
      await mobileBrowser.close();
    }
  });

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
    // service worker는 /fonts/를 캐시 먼저로 맡아(plan §8.115) 다시 연 화면의 face가 아래 route를 거치지 않는다.
    // 다시 연 뒤에도 늦게 온 face를 받은 다음 재는지 보려고 이 기기는 worker 없이 연다(worker 캐시는 01-http-pwa가 본다)
    const fp = (await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 }, { init, serviceWorkers: "block" })).page;
    // 그 face는 시험이 놓을 때 준다. 800ms 뒤면 충분하다는 어림 없이, 받기 전/후를 따로 확인한다.
    let wideFetches = 0;
    let fontGate = Promise.withResolvers();
    await fp.context().route((url) => url.search === "?e2e-wide", async (r) => {
      wideFetches += 1;
      await fontGate.promise;
      await r.continue();
    });
    await inList(fp, "pane-fontset");
    await until("fontset 화면", async () => (await screenText(fp)).includes("s-200"));
    // cell 너비 = 글자 영역 너비 / 열 수(머리의 cols×rows), 그리고 host PTY 열 수
    const cell = async () => {
      const m = await fp.evaluate(() => {
        const w = document.querySelector('[data-testid="terminal"] .xterm-screen').getBoundingClientRect().width;
        const size = /(\d+)×(\d+)/.exec(document.querySelector(".term-title .muted").textContent);
        const cols = Number(size?.[1]);
        const rows = Number(size?.[2]);
        const term = document.querySelector('[data-testid="terminal"]');
        const drawn = term.querySelector(".xterm-rows");
        const style = getComputedStyle(drawn);
        const measure = document.createElement("canvas").getContext("2d");
        measure.font = `${style.fontSize} ${style.fontFamily}`;
        return {
          w: w / cols, cols, rows, drawnRows: drawn.childElementCount,
          advance: measure.measureText("M").width, family: style.fontFamily,
          fit: term.dataset.fit, pending: term.dataset.fitPending,
        };
      });
      return { ...m, pty: aaa("pane", "list").panes.find((x) => x.name === "fontset").cols };
    };
    let last = null;
    let first = null;
    const settle = (what, ok) =>
      until(what, async () => {
        last = await cell();
        // 제목/fit은 state 수신 때 먼저 바뀌고 xterm resize는 출력 큐 뒤다. 실제 그리드도 맞은 뒤 잰다.
        // 0.02px는 공용 fontOk의 cell/영문 advance 단언과 같다.
        return last.fit === "same" && !last.pending && last.cols === last.pty &&
          last.drawnRows === last.rows && Math.abs(last.w - last.advance) <= 0.02 && ok(last);
      }).catch((e) => {
        throw new Error(`${e.message} 기준: ${JSON.stringify(first)} 마지막: ${JSON.stringify(last)}`);
      });
    await settle("처음 크기", () => true);
    first = last;
    const field = () => fp.getByRole("region", { name: "설정" }).getByRole("textbox", { name: "터미널 글꼴" });
    const stored = () => fp.evaluate(() => localStorage.getItem("aaa:terminal-font-family"));
    // 찾아보기: 이름순, 검색, 비례폭 거절(값을 바꾸지 않음), 고정폭을 고르면 그것이 값
    const dlg = await openSettings(fp, "터미널");
    await dlg.getByRole("button", { name: "터미널 글꼴에 설치된 글꼴 찾아보기" }).click();
    const listed = await dlg.getByRole("list", { name: "설치된 글꼴" }).getByRole("button").allInnerTexts();
    if (JSON.stringify(listed) !== JSON.stringify(["E2E Wide", "Helvetica", "Menlo"])) throw new Error(`글꼴 목록 ${listed}`);
    await dlg.getByRole("searchbox", { name: "글꼴 검색" }).fill("hel");
    await dlg.getByRole("button", { name: "Helvetica" }).click();
    await dlg.getByText("Helvetica은(는) 고정폭 글꼴이 아닙니다.", { exact: false }).waitFor();
    if ((await field().inputValue()) !== "" || (await stored()) !== null) throw new Error("비례폭 글꼴을 받아들였다");
    // 검색칸의 Esc는 목록만 닫는다
    await dlg.getByRole("searchbox", { name: "글꼴 검색" }).press("Escape");
    await dlg.getByRole("list", { name: "설치된 글꼴" }).waitFor({ state: "detached" });
    await dlg.getByRole("button", { name: "터미널 글꼴에 설치된 글꼴 찾아보기" }).click();
    await dlg.getByRole("button", { name: "E2E Wide" }).click();
    if ((await field().inputValue()) !== '"E2E Wide"' || (await stored()) !== '"E2E Wide"') throw new Error(`고른 값 ${await field().inputValue()} / ${await stored()}`);
    await until("넓은 글꼴 요청을 붙잡음", () => wideFetches === 1);
    const loading = await fp.evaluate(() => [...document.fonts].find((f) => f.family.replaceAll('"', "") === "E2E Wide")?.status);
    const waiting = await cell();
    if (loading !== "loading" || waiting.family !== first.family || waiting.cols !== first.cols || Math.abs(waiting.w - first.w) >= 0.05)
      throw new Error(`face를 받기 전에 크기를 바꿨다: ${loading} ${JSON.stringify(waiting)}`);
    fontGate.resolve();
    // 그 face를 받은 뒤 넣으므로 cell은 1.5배, grid는 줄고 host PTY도 그 열 수
    await settle("넓은 글꼴로 다시 맞춤", (c) => Math.abs(c.w / first.w - 1.5) < 0.05 && c.cols < first.cols);
    if (wideFetches !== 1) throw new Error(`E2E Wide를 ${wideFetches}번 받았다`);
    // screenshot도 document.fonts.ready를 기다린다. 시험이 붙잡은 face를 놓고 그리드까지 맞춘 뒤 캡처한다.
    await shot(fp, "desktop-terminal-font");
    // 적은 값은 거른다(세미콜론·중괄호를 빼고, 빈 이름을 버리고, 빈칸 있는 이름은 따옴표). Esc로 닫아도 저장한다
    await field().fill("'E2E Wide'; }, , Menlo");
    await fp.keyboard.press("Escape");
    await until("설정 닫힘", async () => (await fp.locator(".settings-dialog").count()) === 0);
    if ((await stored()) !== '"E2E Wide", Menlo') throw new Error(`거른 값 ${await stored()}`);
    // 다시 열어도 이 기기의 글꼴: 연 뒤 face를 받고 나서 잰다
    fontGate = Promise.withResolvers();
    const reloaded = fp.reload();
    await until("다시 연 넓은 글꼴 요청을 붙잡음", () => wideFetches === 2);
    fontGate.resolve();
    await reloaded;
    await inList(fp, "pane-fontset");
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
    // 펼치면 구독마다 한도·초기화까지·계정·plan
    await strip.click();
    const pop = dp.locator(".usage-popover");
    await pop.waitFor();
    const claude = await pop.getByRole("region", { name: "Claude · me@example.com" }).innerText();
    for (const want of ["Max", "세션", "20%", "주간", "95%", "주간 · Sonnet", "40%", "뒤 초기화"]) if (!claude.includes(want)) bad.push(`펼친 Claude에 「${want}」가 없다: ${JSON.stringify(claude)}`);
    // 한도 다섯 줄(Claude 셋·Codex 둘)을 펼쳐도 낮다(사용자 요청 10-19 「창의 높이가 높아」, plan §8.270: 289px → 237px)
    const popHeight = await pop.evaluate((e) => e.getBoundingClientRect().height);
    if (popHeight > 245) bad.push(`펼친 창 높이 ${popHeight}px`);
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
    await dlg.page().getByRole("navigation", { name: "설정", exact: true }).getByRole("tab", { name: "터미널" }).click();
    await dlg.getByLabel("입력 방식").selectOption("line");
    await dp.keyboard.press("Escape");
    await until("설정 닫힘", async () => (await dp.getByRole("region", { name: "설정" }).count()) === 0);
    await inList(dp, "pane-t1");
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
    /**
     * 이 화면이 PTY 크기를 건 뒤 멈춘 열 수: 맞추기 판단(Term data-fit)이 same(이 화면이 받은 host 크기가 맞출 크기와 같다)이고
     * 크기 바뀜 뒤 기다리는 맞추기(data-fit-pending)가 없을 때의 host 열 수. 300ms 사이 두 번 읽어 멈춤을 어림하지 않는다
     */
    const settled = async (pg, what, ok) => {
      const term = pg.getByTestId("terminal");
      const now = async () => ({ fit: await term.getAttribute("data-fit"), pending: await term.getAttribute("data-fit-pending"), cols: t1().cols });
      return until(what, async () => {
        const n = await now();
        return n.fit === "same" && n.pending === null && (await ok(n.cols)) ? n.cols : null;
      }).catch(async (e) => {
        throw new Error(`${e.message}: ${JSON.stringify(await now())}`);
      });
    };
    /**
     * 이 화면의 터미널 칸을 PTY `cols`열이 채운다: 이 화면이 자기 칸으로 크기를 건 뒤다(xterm은 host가 알린 PTY 크기를 따른다).
     * 앞 단계 화면이 건 크기(넓은 창이면 116열)가 남아 있는 동안은 넘친다(02-terminal 「넓은 창」과 같은 판정)
     */
    const fills = (pg, cols) =>
      pg.evaluate((cols) => {
        const box = document.querySelector('[data-testid="terminal"]');
        const sw = box.querySelector(".xterm-screen").getBoundingClientRect().width;
        const cs = getComputedStyle(box);
        const w = box.clientWidth - parseFloat(cs.paddingLeft) - parseFloat(cs.paddingRight);
        // 오른쪽에는 xterm 세로 scrollbar 자리(기본 14px)가 남는다
        return sw <= w + 1 && w - sw < sw / cols + 15;
      }, cols);
    // 주소가 t1을 가리켜 첫 화면이 t1이다(이미 고른 줄을 다시 누르면 focus가 그 줄로 간다, 참고 selectTarget)
    const dp = (await pairPage(browser, aaa("web", "pair").code, { width: 1100, height: 760 }, { hash: paneHash("t1") })).page;
    await until("13px로 연다", async () => (await rowsFont(dp)) === "13px");
    // 데스크톱(바로 입력)은 pane을 열면 터미널이 키보드를 받는다(herdr-web-ui PaneTerminal 2072줄). 입력줄이면 참고처럼 놓는다.
    await until("열면 터미널에 focus", () => dp.evaluate(() => document.activeElement?.classList.contains("xterm-helper-textarea")));
    await dp.getByTestId("terminal").click();
    const wide = await settled(dp, "데스크톱 크기", (c) => fills(dp, c));
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
    await inList(mp, "pane-t1");
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
    const beforePinch = await sentCount(mp);
    await touch("touchStart", 100);
    await touch("touchMove", 130);
    await touch("touchMove", 150);
    // 13 × 150/100 = 19.5 → 20px. 손가락을 떼기 전에는 PTY도 저장도 그대로다
    await until("핀치 중 20px", async () => (await rowsFont(mp)) === "20px");
    const badge = await mp.getByTestId("pinch-badge").innerText();
    if (badge !== "20px") throw new Error(`핀치 표시 ${JSON.stringify(badge)}`);
    // 핀치 사건 처리 안에서 크기를 걸었다면 이미 보냈다(host에 닿기 전이어도)
    const pinchResize = (await sentFrames(mp, beforePinch)).filter((f) => f.t === "resize");
    if (pinchResize.length || t1().cols !== phone) throw new Error(`손을 떼기 전에 PTY가 바뀌었다: ${phone}열 → ${t1().cols}열, 보낸 크기 ${JSON.stringify(pinchResize)}`);
    const beforeEnd = await sentCount(mp);
    if ((await mp.evaluate(() => localStorage.getItem("aaa:terminal-font-size"))) !== null) throw new Error("손을 떼기 전에 저장했다");
    await touch("touchEnd", null);
    await until("핀치 표시 사라짐", async () => await mp.getByTestId("pinch-badge").isHidden());
    if ((await mp.evaluate(() => localStorage.getItem("aaa:terminal-font-size"))) !== "20") throw new Error("핀치 크기를 저장하지 않았다");
    const pinched = await settled(mp, "핀치 뒤 PTY", (c) => c < phone * 0.8);
    const endResize = (await sentFrames(mp, beforeEnd)).filter((f) => f.t === "resize");
    if (endResize.length !== 1) throw new Error(`손을 뗀 뒤 PTY를 한 번 걸지 않았다: ${JSON.stringify(endResize)}`);
    console.log(`     핀치 ${phone}열(13px) → ${pinched}열(20px)`);
    await mp.context().close();
  });

  await step("설정 쪽(herdr-web-ui SettingsDialog PAGES·settings-nav·settingsHistory, 차이 목록 T1): 넓은 창은 목록 옆에 쪽·화살표로 넘김, 휴대폰은 목록 → 쪽 → 뒤로, 시스템 뒤로는 한 걸음씩, Esc는 닫고 focus는 톱니로", async () => {
    const bad = [];
    const focused = (p) => p.evaluate(() => `${document.activeElement?.getAttribute("role") ?? document.activeElement?.tagName}:${document.activeElement?.textContent?.trim() || document.activeElement?.getAttribute("aria-label")}`);
    const closed = (p) => until("설정 닫힘", async () => (await p.getByRole("region", { name: "설정" }).count()) === 0);
    // 넓은 창: 목록과 첫 쪽(모양)이 나란히, 고른 줄에 focus, 화살표·End가 목록을 걷고 쪽도 넘긴다
    const dp = (await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 })).page;
    await inList(dp, "settings-open");
    const dlg = dp.getByRole("region", { name: "설정" });
    await dlg.getByRole("tabpanel", { name: "모양" }).waitFor();
    await until("고른 줄에 focus", async () => (await focused(dp)) === "tab:모양").catch(async (e) => bad.push(`${e.message}: ${await focused(dp)}`));
    await dp.keyboard.press("ArrowDown");
    await dlg.getByRole("tabpanel", { name: "채팅" }).waitFor();
    if ((await dlg.page().getByRole("navigation", { name: "설정", exact: true }).getByRole("tab", { name: "채팅" }).getAttribute("aria-selected")) !== "true") bad.push("화살표로 넘긴 줄이 골라지지 않았다");
    // 마지막 쪽은 「저장소」(관리 GUI W4, plan §8.343: 「폰 및 기기」 뒤에 「이 aaa」·「저장소」)
    await dp.keyboard.press("End");
    await dlg.getByRole("tabpanel", { name: "저장소" }).waitFor();
    await dp.keyboard.press("Home");
    await dlg.getByRole("tabpanel", { name: "모양" }).waitFor();
    await dlg.page().getByRole("navigation", { name: "설정", exact: true }).getByRole("tab", { name: "알림" }).click();
    await dlg.getByRole("tabpanel", { name: "알림" }).waitFor();
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
    await inList(pp, "settings-open");
    const pd = pp.getByRole("region", { name: "설정" });
    await pd.getByRole("tab", { name: "모양" }).waitFor();
    if ((await pd.getByRole("tabpanel").count()) !== 0) bad.push("휴대폰이 목록과 쪽을 함께 보인다");
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
    await until("쪽으로 들어간 focus", async () => (await focused(pp)) === "BUTTON:설정으로 돌아가기").catch(async (e) => bad.push(`${e.message}: ${await focused(pp)}`));
    await shot(pp, "settings-pages-phone-terminal");
    // 키 바 편집 → 뒤로 → 터미널 쪽(「키 바 편집」에 focus) → 뒤로 → 목록(「터미널」 줄에 focus) → 뒤로 → 닫힘
    await pd.getByRole("button", { name: "키 바 편집" }).click();
    await pp.getByRole("region", { name: "키 바" }).waitFor();
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
    await pp.context().close();

    if (bad.length) throw new Error(bad.join("; "));
  });

  await step("밀도·인터페이스 글자 크기(herdr-web-ui settings.ts density·uiFontSize·typography.ts, 차이 목록 T4): 촘촘이면 한 단계 작고, 글자 크기는 --ui-scale로 목록·창·단추가 함께 바뀌며 터미널은 그대로, 이 기기에만 남는다", async () => {
    const bad = [];
    /** 문서의 밀도·배율과 실제 글자 크기(본문, 목록 줄 이름, 설정 줄 이름, 터미널 줄) */
    const look = (p) =>
      p.evaluate(() => {
        const px = (sel) => {
          const el = document.querySelector(sel);
          return el ? getComputedStyle(el).fontSize : null;
        };
        return {
          density: document.documentElement.dataset.density,
          scale: document.documentElement.style.getPropertyValue("--ui-scale"),
          body: getComputedStyle(document.body).fontSize,
          row: px('[data-testid="pane-t1"]'),
          setting: px(".settings-row-text"),
          term: px('[data-testid="terminal"] .xterm-rows'),
        };
      });
    const dp = (await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 })).page;
    await inList(dp, "pane-t1");
    await until("13px 터미널", async () => (await look(dp)).term === "13px");
    const start = await look(dp);
    if (start.density !== "comfortable" || start.scale !== "1" || start.body !== "14px") bad.push(`처음 모양 ${JSON.stringify(start)}`);
    const dlg = await openSettings(dp, "모양");
    const size = dlg.getByRole("group", { name: "인터페이스 글자 크기" });
    if ((await size.innerText()).trim() !== "14px") bad.push(`처음 인터페이스 글자 크기 ${await size.innerText()}`);
    // 촘촘: 본문이 13px, 고른 크기가 없으니 글자 크기 칸도 밀도를 따라 13px
    await dlg.getByRole("group", { name: "밀도" }).getByRole("button", { name: "촘촘" }).click();
    await until("촘촘", async () => (await look(dp)).body === "13px");
    const compact = await look(dp);
    if (compact.density !== "compact" || compact.scale !== "1" || parseFloat(compact.setting) >= parseFloat(start.setting)) bad.push(`촘촘 ${JSON.stringify(compact)} ← ${JSON.stringify(start)}`);
    if ((await size.innerText()).trim() !== "13px") bad.push(`촘촘의 글자 크기 칸 ${await size.innerText()}`);
    // 18px까지: 배율 18/13, 목록·설정 줄 글자도 같은 배율로 커지고 터미널은 13px 그대로
    for (let i = 0; i < 5; i++) await dlg.getByRole("button", { name: "인터페이스 글자 크기 키우기" }).click();
    await until("18px", async () => (await look(dp)).body === "18px");
    const big = await look(dp);
    const ratio = (a, b) => parseFloat(a) / parseFloat(b);
    if (Math.abs(Number(big.scale) - 18 / 13) > 1e-6) bad.push(`배율 ${big.scale}`);
    if (Math.abs(ratio(big.row, compact.row) - 18 / 13) > 0.02 || Math.abs(ratio(big.setting, compact.setting) - 18 / 13) > 0.02) bad.push(`목록·설정이 배율을 따르지 않는다 ${JSON.stringify(big)} ← ${JSON.stringify(compact)}`);
    if (big.term !== "13px") bad.push(`터미널 글자가 바뀌었다 ${big.term}`);
    await shot(dp, "settings-ui-font-18-desktop");
    // 20px이 끝: 키우기가 꺼진다
    for (let i = 0; i < 2; i++) await dlg.getByRole("button", { name: "인터페이스 글자 크기 키우기" }).click();
    if (!(await dlg.getByRole("button", { name: "인터페이스 글자 크기 키우기" }).isDisabled())) bad.push("20px에서 키우기가 켜져 있다");
    // 이 기기에 남고 host 공유 설정에는 가지 않는다(참고 SETTING_SCOPE density·uiFontSize "device")
    const saved = await dp.evaluate(() => JSON.parse(localStorage.getItem("aaa:appearance") ?? "{}"));
    if (saved.density !== "compact" || saved.uiFontSize !== 20) bad.push(`저장 ${JSON.stringify(saved)}`);
    await dp.keyboard.press("Escape");
    await dp.reload();
    await dp.getByTestId("machine-labA").waitFor({ state: "attached" });
    const again = await look(dp);
    if (again.density !== "compact" || again.body !== "20px") bad.push(`다시 연 뒤 ${JSON.stringify(again)}`);
    await dp.context().close();
    // 휴대폰 20px: 목록·설정 쪽이 가로로 넘치지 않는다. 다른 기기는 제 밀도(여유) 그대로
    const pp = (await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 }, { touch: true })).page;
    await pp.evaluate(() => localStorage.setItem("aaa:appearance", JSON.stringify({ ...JSON.parse(localStorage.getItem("aaa:appearance") ?? "{}"), uiFontSize: 20 })));
    await pp.reload();
    // 목록은 drawer다: 열고 잰다
    await pp.locator(".view [data-testid='drawer-toggle']").click();
    await pp.locator("#workspace-drawer.is-open").waitFor();
    await until("drawer 자리", () => pp.evaluate(() => document.getElementById("workspace-drawer").getAnimations().length === 0));
    const wide = () => pp.evaluate(() => document.documentElement.scrollWidth);
    if ((await wide()) > 390) bad.push(`휴대폰 20px 목록 넘침 ${await wide()}`);
    await shot(pp, "phone-ui-font-20-list");
    const pd = await openSettings(pp, "모양");
    await pd.evaluate((el) => Promise.all(el.getAnimations({ subtree: true }).map((a) => a.finished.catch(() => null))));
    const over = await pp.evaluate(() => [...document.querySelectorAll(".settings-dialog *")].filter((e) => e.getBoundingClientRect().right > window.innerWidth + 0.5).map((e) => e.className || e.tagName).slice(0, 3));
    if (over.length) bad.push(`휴대폰 20px 설정 넘침 ${over}`);
    await shot(pp, "phone-ui-font-20-settings");
    await pp.context().close();
    if (bad.length) throw new Error(bad.join("; "));
  });

  await step("설정 닫는 길·조작 크기(QA t-0337 Q10·Q12, herdr-web-ui SettingsDialog 본문 focus·settingsHistory, primitives·SettingsControls 터치 크기): 휴대폰 시스템 뒤로는 설정만 한 걸음씩 닫고 앱을 나가지 않으며, 아래로 밀어도 본문에 남고, 닫으면 focus는 연 곳으로, 단축키로 연 설정을 Esc로 닫으면 친 글은 터미널로, 조작 대상은 참고 크기", async () => {
    const bad = [];
    const closed = (p) => until("설정 닫힘", async () => (await p.getByRole("region", { name: "설정" }).count()) === 0);
    const pp = (await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 }, { touch: true })).page;
    await pp.getByTestId("terminal").waitFor();
    await domIs(pp, "처음 pane 주소 선택", () => location.hash.startsWith("#pane="));
    const home = pp.url();
    // 뒤로 두 번: 쪽 → 목록 → 닫힘. 앱은 그 자리(주소·목록)에 남고, focus는 톱니로
    await openSettings(pp, "터미널");
    await pp.goBack();
    await pp.getByRole("region", { name: "설정" }).getByRole("tab", { name: "터미널" }).waitFor();
    await pp.goBack();
    await closed(pp);
    if (pp.url() !== home || await pp.locator("#workspace-drawer.is-open").count()) bad.push(`뒤로가 앱을 나갔다: ${pp.url()}`);
    await until("휴대폰 닫은 뒤 focus는 본문 목록 단추", () => pp.evaluate(() => document.activeElement?.dataset.testid === "drawer-toggle")).catch((e) => bad.push(e.message));
    // 조작 크기(390×844 터치): 쪽 줄은 --keybar-h, 머리·한 칸씩의 아이콘 단추는 --touch-target, 글 단추는 그 높이, 스위치는 48×40
    const sizes = (p, sel) =>
      p.locator(".settings-dialog").evaluate((d, sel) => [...d.querySelectorAll(sel)].filter((e) => e.getClientRects().length).map((e) => [Math.round(e.getBoundingClientRect().width), Math.round(e.getBoundingClientRect().height)]), sel);
    await inList(pp, "settings-open");
    const dlg = pp.getByRole("region", { name: "설정" });
    await dlg.page().getByRole("navigation", { name: "설정", exact: true }).getByRole("tab", { name: "터미널" }).waitFor();
    const tabs = await sizes(pp, ".settings-tab");
    if (!tabs.length || tabs.some(([, h]) => h < 48)) bad.push(`쪽 줄 ${JSON.stringify(tabs)}`);
    await dlg.page().getByRole("navigation", { name: "설정", exact: true }).getByRole("tab", { name: "터미널" }).click();
    await dlg.getByRole("tabpanel", { name: "터미널" }).waitFor();
    const icons = await sizes(pp, ".icon-button");
    const btns = await sizes(pp, ".btn");
    const toggles = await sizes(pp, ".settings-toggle");
    if (!icons.length || icons.some(([w, h]) => w < 40 || h < 40)) bad.push(`아이콘 단추 ${JSON.stringify(icons)}`);
    if (!btns.length || btns.some(([, h]) => h < 40)) bad.push(`글 단추 ${JSON.stringify(btns)}`);
    if (!toggles.length || toggles.some(([w, h]) => w < 48 || h < 40)) bad.push(`스위치 ${JSON.stringify(toggles)}`);
    await dlg.getByRole("button", { name: "설정으로 돌아가기" }).click();
    await dlg.page().getByRole("navigation", { name: "설정", exact: true }).getByRole("tab", { name: "모양" }).click();
    const segs = await sizes(pp, ".segmented > button");
    console.log(`     휴대폰 설정 조작: 쪽 줄 ${tabs[0]}, 아이콘 ${icons[0]}, 글 단추 ${btns[0]}, 스위치 ${toggles[0]}, 여럿 중 하나 ${segs[0]}(참고 --control-h − 6px)`);
    // 설정은 본문이다. 머리를 아래로 밀어도 닫히거나 주소가 바뀌지 않는다.
    await dlg.evaluate((el) => Promise.all(el.getAnimations().map((a) => a.finished.catch(() => null))));
    const head = await dlg.locator(".settings-header").boundingBox();
    const cdp = await pp.context().newCDPSession(pp);
    const x = head.x + head.width / 2;
    const y0 = head.y + head.height / 2;
    await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x, y: y0, id: 1 }] });
    for (let i = 1; i <= 12; i++) await cdp.send("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: [{ x, y: y0 + (420 * i) / 12, id: 1 }] });
    await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
    if (!(await dlg.isVisible()) || new URL(pp.url()).searchParams.get("settings") !== "appearance") bad.push("본문을 아래로 밀자 설정이 닫혔다");
    await dlg.getByRole("button", { name: "설정 닫기" }).click();
    await closed(pp);
    await until("닫은 뒤 원래 주소", () => pp.url() === home);
    await pp.context().close();
    // 데스크톱: 터미널에서 단축키로 연 설정을 Esc로 닫으면 focus는 터미널로 돌아가 친 글이 pane에 닿는다(Mod = Ctrl인 platform)
    const dp = (await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 }, { platform: "Linux x86_64" })).page;
    await inList(dp, "pane-t1");
    await dp.getByTestId("terminal").click();
    await until("터미널에 focus", () => dp.evaluate(() => document.activeElement?.classList.contains("xterm-helper-textarea")));
    await dp.keyboard.press("Control+Shift+Comma");
    await dp.getByRole("region", { name: "설정" }).waitFor();
    await dp.keyboard.press("Escape");
    await closed(dp);
    await until("Esc 뒤 터미널에 focus", () => dp.evaluate(() => document.activeElement?.classList.contains("xterm-helper-textarea"))).catch((e) => bad.push(e.message));
    await dp.keyboard.type("echo q10-$((3+4))");
    await dp.keyboard.press("Enter");
    await waitOutput("t1", "q10-7").catch((e) => bad.push(`친 글이 터미널에 닿지 않았다: ${e.message}`));
    await dp.context().close();
    if (bad.length) throw new Error(bad.join("; "));
  });
}

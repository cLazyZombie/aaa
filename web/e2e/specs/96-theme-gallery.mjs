// herdr의 역할별 색·밝기 짝과 실제 host 저장·터미널 색 전송을 확인한다.
import assert from "node:assert/strict";
import fs from "node:fs";
import { specCleanup, browser, webkit, aaa, step, pairPage, openSettings, domIs, shot, sentFrames, until } from "../lib/harness.mjs";
import { waitForSettingsExit } from "../lib/settings-closed.mjs";

const themes = JSON.parse(fs.readFileSync(new URL("../../public/theme-catalog/sources.json", import.meta.url))).themes;
// 모든 원색/ANSI/역할 대비·밝기 짝은 theme.test가 전체 카탈로그로 검사한다.
// 실제 저장·재연결·DOM token 연결은 밝음/어두움 대표를 두 엔진·두 폭에서 본다.
const selected = ["tokyo-night-day", "one-dark"].map(id => {
  const theme = themes.find(theme => theme.id === id);
  assert.ok(theme, `E2E 대표 팔레트 없음: ${id}`);
  return theme;
});

async function check(engine, label, width) {
  const connections = [];
  const p = (await pairPage(engine, aaa("web", "pair").code, { width, height: 900 }, { onPage: (p) => p.on("websocket", (ws) => connections.push(ws.url())) })).page;
  try {
    let d = await openSettings(p, "모양");
    await d.getByRole("button", { name: "어둡게", exact: true }).click();
    const gallery = d.locator(".theme-gallery");
    await gallery.locator("summary").click();
    assert.equal(await gallery.locator(".theme-card").count(), 18);
    const broken = await gallery.locator(".theme-card").evaluateAll((cards) => cards.filter((card) => {
      const box = card.getBoundingClientRect(), preview = card.querySelector(".theme-card-preview").getBoundingClientRect(), label = card.querySelector(".theme-card-label").getBoundingClientRect();
      return box.height < 120 || preview.top < box.top || preview.bottom > label.top || label.bottom > box.bottom;
    }).map((card) => card.textContent));
    assert.deepEqual(broken, [], "미리보기와 이름은 카드 안에서 겹치지 않는다");
    const faint = await gallery.locator(".theme-card-preview").evaluateAll((previews) => {
      const lum = (rgb) => rgb.slice(0, 3).map((v) => v / 255).map((v) => v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4).reduce((s, v, i) => s + v * [0.2126, 0.7152, 0.0722][i], 0);
      const rgb = (value) => value.match(/[\d.]+/g).map(Number);
      return previews.flatMap((preview) => [...preview.children].flatMap((child) => {
        const css = getComputedStyle(child), bg = rgb(css.backgroundColor);
        const a = lum(rgb(css.color)), b = lum(bg[3] === 0 ? rgb(getComputedStyle(preview).backgroundColor) : bg);
        const ratio = (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
        return ratio < 4.5 ? [{ text: child.textContent, ratio }] : [];
      }));
    });
    assert.deepEqual(faint, [], "견본 글자·코드도 자기 바탕에서 4.5:1 이상");
    await gallery.getByRole("searchbox", { name: "테마 찾기" }).fill("rose");
    // Rosé의 악센트를 제거한 검색도 지원한다.
    assert.equal(await gallery.locator(".theme-card").count(), 2);
    await gallery.getByRole("button", { name: "Rosé Pine Dawn" }).click();
    await domIs(p, "갤러리 선택", () => document.documentElement.dataset.palette === "rose-pine-dawn");
    await gallery.getByRole("searchbox", { name: "테마 찾기" }).fill("없는테마");
    await gallery.getByRole("status").getByText("맞는 테마가 없습니다.").waitFor();
    await gallery.getByRole("searchbox", { name: "테마 찾기" }).fill("");
    await shot(p, `gallery-${label}-${width}-top`);
    await gallery.getByRole("button", { name: "Rosé Pine Dawn" }).scrollIntoViewIfNeeded();
    await shot(p, `gallery-${label}-${width}-bottom`);

    for (const theme of selected) {
      const saved = p.waitForResponse((r) => r.url().endsWith("/api/v1/call") && r.request().postDataJSON()?.op === "preferences_set");
      await d.locator("#settings-palette").selectOption(theme.id);
      assert.ok((await saved).ok());
      await domIs(p, "새 팔레트 적용", (id) => document.documentElement.dataset.palette === id, theme.id);
      const look = await p.evaluate(() => {
        const css = getComputedStyle(document.documentElement);
        const hex = (name) => css.getPropertyValue(`--${name}`).trim().replace(/^#([a-f0-9])([a-f0-9])([a-f0-9])$/i, "#$1$1$2$2$3$3");
        const lum = (h) => h.slice(1).match(/../g).map((c) => parseInt(c, 16) / 255).map((v) => v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4).reduce((s, v, i) => s + v * [0.2126, 0.7152, 0.0722][i], 0);
        const ratio = (a, b) => (Math.max(lum(a), lum(b)) + 0.05) / (Math.min(lum(a), lum(b)) + 0.05);
        const roles = ["text", "text-dim", "text-strong", "accent", "status-idle", "status-working", "status-blocked", "status-done", "danger-text", "notice", "syntax-comment", "syntax-keyword", "syntax-string", "syntax-function", "syntax-number", "syntax-type", "syntax-variable", "syntax-meta"];
        const pairs = roles.flatMap((fg) => ["bg", "bg-panel", "bg-elevated", "bg-hover", "bg-input"].map((bg) => [fg, bg]));
        pairs.push(["primary-text", "primary"], ["primary-text", "primary-hover"]);
        return {
          bg: hex("bg"), termBg: hex("term-bg"), termFg: hex("term-fg"), mode: document.documentElement.dataset.theme,
          pwa: document.querySelector('meta[name="theme-color"]').content,
          ansi: Array.from({ length: 16 }, (_, i) => hex(`ansi-${i}`)),
          low: pairs.map(([fg, bg]) => ({ fg, bg, ratio: ratio(hex(fg), hex(bg)) })).filter((pair) => pair.ratio < 4.5),
          overflow: document.documentElement.scrollWidth > innerWidth,
        };
      });
      assert.equal(look.bg, theme.colors.panel_bg);
      assert.equal(look.pwa, theme.colors.panel_bg);
      assert.equal(look.mode, theme.mode);
      assert.equal(look.termBg, theme.terminal.background);
      assert.equal(look.termFg, theme.terminal.foreground);
      assert.deepEqual(look.ansi, [...theme.terminal.ansi, ...theme.terminal.brights]);
      assert.deepEqual(look.low, [], `${theme.id} UI 대비`);
      assert.equal(look.overflow, false);
      await until(`${theme.id} 변경 기본색 전송`, async () => (await sentFrames(p, 0)).some((f) => f.t === "colors" && f.bg === theme.terminal.background.slice(1) && f.fg === theme.terminal.foreground.slice(1)));
      await d.getByRole("button", { name: "설정 닫기" }).click();
      await p.evaluate(waitForSettingsExit);
      // 실제 host 공유 설정과 기기 사본을 모두 다시 읽는다.
      connections.length = 0;
      await p.reload();
      await domIs(p, "재열기 팔레트 보존", (id) => document.documentElement.dataset.palette === id, theme.id);
      await domIs(p, "실제 터미널 렌더", () => !!document.querySelector(".xterm-rows"));
      await until(`${theme.id} 재연결 기본색`, () => connections.some((url) => { const u = new URL(url); return u.pathname.includes("/panes/") && u.searchParams.get("bg") === theme.terminal.background.slice(1) && u.searchParams.get("fg") === theme.terminal.foreground.slice(1); }));
      await shot(p, `theme-${label}-${width}-${theme.id}`);
      d = await openSettings(p, "모양");
    }
    // OS 변경은 저장한 밝음·어두움 짝을 적용한다.
    await d.getByRole("button", { name: "시스템", exact: true }).click();
    await d.locator("#settings-palette-light").selectOption("tokyo-night-day");
    await d.locator("#settings-palette-dark").selectOption("one-dark");
    await p.emulateMedia({ colorScheme: "light" });
    await domIs(p, "OS 밝음", () => document.documentElement.dataset.palette === "tokyo-night-day");
    await p.emulateMedia({ colorScheme: "dark" });
    await domIs(p, "OS 어두움", () => document.documentElement.dataset.palette === "one-dark");
    await d.getByRole("button", { name: "어둡게", exact: true }).click();
    await d.locator("#settings-palette").selectOption("github-dark");
    await domIs(p, "이전 CSS 팔레트 복귀", () => document.documentElement.dataset.palette === "github-dark" && !document.documentElement.style.getPropertyValue("--bg"));
  } finally { await p.context().close(); }
}

export default async function () {
  await step("테마 갤러리 Chromium 데스크톱: 밝음·어두움 저장·ANSI·대비·OS 짝", async () => check(browser, "chromium", 1280));
  await step("테마 갤러리 Chromium 휴대폰: 밝음·어두움 저장·ANSI·대비·OS 짝", async () => check(browser, "chromium", 390));
  let safari;
  const safariBrowser = async () => (safari ??= specCleanup.own(() => webkit.launch({ headless: true }), value => value.close())).value;
  try {
  await step("테마 갤러리 WebKit 데스크톱: 밝음·어두움 저장·ANSI·대비·OS 짝", async () => check(await safariBrowser(), "webkit", 1280));
  await step("테마 갤러리 WebKit 휴대폰: 밝음·어두움 저장·ANSI·대비·OS 짝", async () => check(await safariBrowser(), "webkit", 390));
  } finally { await safari?.close(); }
}

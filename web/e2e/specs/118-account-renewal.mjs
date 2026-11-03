// 화면 계약은 가짜 제공자 응답으로, 권한·실제 자식 정리는 account_login::remote::tests가 검사한다.
import assert from "node:assert/strict";
import { token } from "../../../scripts/fixture-secret.mjs";
import { aaa, step, browser, webkit, pairPage, inList, openSettings, shot, domIs } from "../lib/harness.mjs";

const manualCode = `${token("manual-code")}#${token("manual-state")}`;

// macOS의 mobile WebKit은 iOS의 UIKit 입력 확대를 재현하지 않는다.
// 실제 DOM의 확대 유발 조건과 완료 뒤 viewport·닫기 조작을 각각 지킨다.
async function phoneInput(page, input, width) {
  if (width >= 700) return;
  const original = await page.evaluate(() => ({ density: document.documentElement.getAttribute("data-density"), scale: document.documentElement.style.getPropertyValue("--ui-scale") }));
  try {
    for (const [density, size] of [[null, 14], ["compact", 13], ["compact", 11], [null, 20]]) {
      await page.evaluate(({ density, size }) => {
        const root = document.documentElement;
        if (density) root.setAttribute("data-density", density); else root.removeAttribute("data-density");
        root.style.setProperty("--ui-scale", String(size / (density ? 13 : 14)));
      }, { density, size });
      const font = await input.evaluate(el => parseFloat(getComputedStyle(el).fontSize));
      assert.ok(font >= 16, `휴대폰 로그인 입력 글자 하한: ${font}px (${density ?? "기본"}/${size})`);
      if (size === 20) assert.equal(font, 20, "큰 글자 설정을 줄이지 않는다");
    }
    await page.evaluate(() => {
      document.documentElement.setAttribute("data-density", "compact");
      document.documentElement.style.setProperty("--ui-scale", String(11 / 13));
    });
    await page.setViewportSize({ width: 844, height: 390 });
    await input.tap();
    assert.ok(await input.evaluate(el => parseFloat(getComputedStyle(el).fontSize)) >= 16, "휴대폰 가로 화면에서도 입력 글자 하한");
  } finally {
    await page.setViewportSize({ width, height: 900 });
    await page.evaluate(original => {
      const root = document.documentElement;
      if (original.density) root.setAttribute("data-density", original.density); else root.removeAttribute("data-density");
      if (original.scale) root.style.setProperty("--ui-scale", original.scale); else root.style.removeProperty("--ui-scale");
    }, original);
  }
  await input.tap();
}

async function closeCompleted(page, dialog, width) {
  const button = dialog.getByRole("button", { name: "닫기", exact: true });
  if (width < 700) {
    const viewport = await button.evaluate(el => {
      const v = window.visualViewport;
      const r = el.getBoundingClientRect();
      return { scale: v?.scale ?? 1, width: v?.width ?? innerWidth, height: v?.height ?? innerHeight,
        left: r.left - (v?.offsetLeft ?? 0), right: r.right - (v?.offsetLeft ?? 0), top: r.top - (v?.offsetTop ?? 0), bottom: r.bottom - (v?.offsetTop ?? 0),
        hit: el.contains(document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2)), overflow: document.documentElement.scrollWidth > innerWidth };
    });
    assert.equal(viewport.scale, 1, "완료 뒤 화면 배율 유지");
    assert.ok(viewport.left >= 0 && viewport.right <= viewport.width && viewport.top >= 0 && viewport.bottom <= viewport.height, `닫기가 viewport 안에 있다: ${JSON.stringify(viewport)}`);
    assert.ok(viewport.hit && !viewport.overflow, "확대·가로 넘침 없이 닫기를 누를 수 있다");
    assert.doesNotMatch(await page.locator('meta[name="viewport"]').getAttribute("content"), /user-scalable\s*=\s*(no|0)|maximum-scale\s*=\s*1(?:[.,\s]|$)/, "사용자 확대를 막지 않는다");
    await button.tap();
  } else await button.click();
  await dialog.waitFor({ state: "detached" });
  // DOM을 거둔 뒤에도 sheet의 back()은 아직 도착하지 않을 수 있다.
  // 실제 복귀를 마친 뒤 다음 인증이나 reload를 시작한다.
  await domIs(page, "인증 창 닫기 기록 복귀", () => !history.state?.["aaa:sheet"], undefined, 20_000, ["popstate"]);
}

function provider(registered = false) {
  let accounts = [
    { name: "sample-codex", harness: "codex", version: 1, health: "expired" },
    { name: "sample-claude", harness: "claude", version: 1, health: "expiring", email: registered ? "me@example.com" : undefined },
    { name: "healthy", harness: "codex", version: 1, health: "ok" },
  ];
  let login = null, prompt = null, failQuery = false;
  const codes = [], starts = [], seen = [];
  const route = async route => {
    const request = route.request().postDataJSON();
    seen.push(request.op);
    let value;
    if (request.op === "account_list") value = { kind: "accounts", accounts };
    else if (["account_login_remote", "account_usage_login"].includes(request.op)) {
      const usage = request.op === "account_usage_login";
      const harness = usage ? "claude" : request.harness;
      if (usage) { assert.equal(request.remote, true); assert.equal(request.email, accounts.find(a => a.name === request.name)?.email ?? "me@example.com"); }
      if (request.name === "personal") assert.equal(request.email, "new@example.com");
      starts.push(usage ? "query" : harness);
      login = { id: `fixture-${starts.length}`, account: request.name, harness, usage_only: usage || undefined, state: "waiting", expect_email: request.email };
      prompt = harness === "codex"
        ? { kind: "device_code", url: "https://auth.openai.com/codex/device", user_code: "ABCD-1234" }
        : { kind: "paste_code", url: "https://claude.com/cai/oauth/authorize?code=true&state=fixture" };
      value = { kind: "account_login", login };
    } else if (request.op === "account_login_interaction") value = { kind: "account_login_interaction", login: request.id || login?.state === "waiting" ? login : null, prompt, expires_ms: Date.now() + 300_000 };
    else if (request.op === "account_login_code") {
      codes.push(request.code); login.state = "succeeded"; prompt = null;
      if (login.usage_only && failQuery) { login.state = "failed"; login.error = "공식 Claude CLI 인증이 실패했습니다"; failQuery = false; }
      accounts = accounts.map(a => a.name === login.account ? { ...a, health: "ok", email: login.expect_email ?? a.email } : a);
      if (!accounts.some(a => a.name === login.account)) accounts.push({ name: login.account, harness: login.harness, version: 1, health: "ok", email: login.expect_email });
      value = { kind: "done" };
    } else if (request.op === "account_email") {
      assert.equal(request.email, "changed@example.com");
      accounts = accounts.map(a => a.name === request.name ? { ...a, email: request.email } : a);
      value = { kind: "done" };
    } else if (request.op === "account_login_cancel") {
      login.state = "cancelled"; prompt = null; value = { kind: "account_login", login };
    } else return route.continue();
    await route.fulfill({ json: { ok: value } });
  };
  return { route, starts, codes, seen, failNextQuery: () => { failQuery = true; } };
}

async function check(name, engine, width) {
  // WebKit의 service worker가 fetch를 중계하면 Playwright route를 우회한다.
  // 이 시험은 제공자 응답 fixture를 쓰므로 해당 context에서만 worker를 막는다.
  const fixture = provider();
  const { page, errors } = await pairPage(engine, aaa("web", "pair").code, { width, height: 900 }, {
    serviceWorkers: "block", route: ["**/api/v1/call", fixture.route],
  });
  try {
    {
      const menu = page.getByTestId("account-renewal");
      await inList(page, menu.getByRole("button", { name: "sample-codex 인증 갱신", includeHidden: true }));
      const dialog = page.getByTestId("account-login-dialog");
      await dialog.getByText("ABCD-1234", { exact: true }).waitFor();
      assert.equal(await menu.getByRole("button", { includeHidden: true }).count(), 2);
      assert.equal(await dialog.getByRole("link", { name: "인증 열기" }).getAttribute("href"), "https://auth.openai.com/codex/device");
      await shot(page, `118-codex-${name}-${width}`);
      await page.reload();
      await dialog.getByText("ABCD-1234", { exact: true }).waitFor();
      await dialog.getByRole("button", { name: "취소", exact: true }).click();
      await dialog.waitFor({ state: "detached" });
    }
    {
      const settings = await openSettings(page, "계정");
      await settings.getByRole("button", { name: "sample-claude 다시 로그인", exact: true }).click();
      const dialog = page.getByTestId("account-login-dialog");
      const input = dialog.getByLabel("인증 코드", { exact: true });
      await phoneInput(page, input, width);
      await input.fill(manualCode);
      const read = page.waitForResponse(r => r.request().postData()?.includes('"account_login_interaction"'));
      await page.evaluate(() => window.dispatchEvent(new Event("focus")));
      await read;
      assert.equal(await input.inputValue(), manualCode, "상태 재조회가 입력을 지우지 않는다");
      assert.equal(await input.getAttribute("type"), "password");
      assert.match(await dialog.getByRole("link", { name: "인증 열기" }).getAttribute("href"), /^https:\/\/claude.com\/cai\/oauth\/authorize/);
      await shot(page, `118-claude-${name}-${width}`);
      await dialog.getByRole("button", { name: "확인", exact: true }).click();
      await dialog.getByText("로그인 완료", { exact: true }).waitFor();
      await closeCompleted(page, dialog, width);
      assert.deepEqual(fixture.starts, ["codex", "claude"]);
      assert.deepEqual(fixture.codes, [manualCode]);
      assert.equal(await page.getByTestId("account-renewal").getByRole("button", { includeHidden: true }).count(), 1);
      await settings.getByRole("button", { name: "sample-claude 사용량 조회 로그인", exact: true }).click();
      await dialog.getByLabel("조회 계정 메일", { exact: true }).waitFor();
      await phoneInput(page, dialog.getByLabel("조회 계정 메일", { exact: true }), width);
      assert.deepEqual(fixture.starts, ["codex", "claude"], "기대 메일을 입력하기 전에 로그인을 시작하지 않는다");
      await dialog.getByLabel("조회 계정 메일", { exact: true }).fill("me@example.com");
      await dialog.getByRole("button", { name: "조회 로그인", exact: true }).click();
      await dialog.getByRole("link", { name: "인증 열기", exact: true }).waitFor();
      await page.reload();
      await dialog.getByRole("heading", { name: /사용량 조회 로그인/ }).waitFor();
      await phoneInput(page, dialog.getByLabel("인증 코드", { exact: true }), width);
      await dialog.getByLabel("인증 코드", { exact: true }).fill(manualCode);
      await dialog.getByRole("button", { name: "확인", exact: true }).click();
      await dialog.getByText("로그인 완료", { exact: true }).waitFor();
      assert.deepEqual(fixture.starts, ["codex", "claude", "query"]);
      assert.deepEqual(fixture.codes, [manualCode, manualCode]);
      await shot(page, `118-query-${name}-${width}`);
      await closeCompleted(page, dialog, width);
      assert.deepEqual(errors, []);
    }
  } catch (e) {
    e.message += ` · 진단 ${JSON.stringify({ errors, calls: fixture.seen, ui: await page.locator("body").innerText() })}`;
    throw e;
  } finally { await page.context().close(); }
}

// 메일 등록·수정은 독립 fixture에서 확인한다. 갱신·복원 시험과 긴 로그인 사슬을 만들지 않는다.
async function checkEmail(name, engine, width) {
  const fixture = provider(true);
  const { page, errors } = await pairPage(engine, aaa("web", "pair").code, { width, height: 900 }, {
    serviceWorkers: "block", route: ["**/api/v1/call", fixture.route],
  });
  try {
    const settings = await openSettings(page, "계정");
    const dialog = page.getByTestId("account-login-dialog");
    const row = settings.locator('[data-account="sample-claude"]');
    await row.getByTestId("account-email").getByText("me@example.com", { exact: true }).waitFor();
    await row.getByRole("button", { name: "sample-claude 메일 수정", exact: true }).click();
    const editor = row.getByTestId("account-email-edit");
    assert.equal(await editor.getByLabel("sample-claude 메일", { exact: true }).inputValue(), "me@example.com");
    await editor.getByLabel("sample-claude 메일", { exact: true }).fill("changed@example.com");
    await shot(page, `118-email-edit-${name}-${width}`);
    await editor.getByRole("button", { name: "메일 저장", exact: true }).click();
    await editor.waitFor({ state: "detached" });
    await row.getByTestId("account-email").getByText("changed@example.com", { exact: true }).waitFor();
    fixture.failNextQuery();
    await settings.getByRole("button", { name: "sample-claude 사용량 조회 로그인", exact: true }).click();
    await dialog.getByRole("link", { name: "인증 열기", exact: true }).waitFor();
    assert.equal(await dialog.getByLabel("조회 계정 메일", { exact: true }).count(), 0, "저장된 메일로 바로 시작한다");
    await dialog.getByLabel("인증 코드", { exact: true }).fill(manualCode);
    await dialog.getByRole("button", { name: "확인", exact: true }).click();
    await dialog.getByText("공식 Claude CLI 인증이 실패했습니다", { exact: true }).waitFor();
    assert.ok(!(await dialog.innerText()).includes("같은 계정의 조회 인증을 저장하지 못했습니다"));
    await closeCompleted(page, dialog, width);
    await settings.getByTestId("account-alias").fill("personal");
    await settings.getByTestId("account-registration-email").fill("new@example.com");
    await shot(page, `118-email-registration-${name}-${width}`);
    await settings.getByRole("button", { name: "로그인", exact: true }).click();
    await dialog.getByLabel("인증 코드", { exact: true }).fill(manualCode);
    await dialog.getByRole("button", { name: "확인", exact: true }).click();
    await dialog.getByText("로그인 완료", { exact: true }).waitFor();
    await closeCompleted(page, dialog, width);
    await page.reload();
    await settings.getByTestId("settings-accounts").waitFor();
    await row.getByTestId("account-email").getByText("changed@example.com", { exact: true }).waitFor();
    await settings.locator('[data-account="personal"]').getByTestId("account-email").getByText("new@example.com", { exact: true }).waitFor();
    assert.deepEqual(errors, []);
  } finally { await page.context().close(); }
}

// 회전 때 matchMedia effect와 sheet 닫기 timer가 모두 기록 이동을 요청한 뒤
// 브라우저가 도착하게 한다. 시간을 자거나 실제 기록·popstate를 대체하지 않는다.
function holdRotationHistory() {
  const rotation = window.__rotationHistory = { hold: false, moves: [], sheetRequested: null };
  for (const method of ["go", "back"]) {
    const original = history[method].bind(history);
    history[method] = (...args) => {
      if (!rotation.hold || (method === "go" && !(args[0] < 0))) return original(...args);
      rotation.moves.push(() => original(...args));
      if (method === "back") rotation.sheetRequested();
    };
  }
}

async function checkRotation(engine) {
  const fixture = provider();
  const { page, errors } = await pairPage(engine, aaa("web", "pair").code, { width: 390, height: 900 }, {
    serviceWorkers: "block", route: ["**/api/v1/call", fixture.route], init: holdRotationHistory,
  });
  try {
    const settings = await openSettings(page, "계정");
    await settings.getByRole("button", { name: "sample-claude 다시 로그인", exact: true }).click();
    const dialog = page.getByTestId("account-login-dialog");
    await dialog.getByLabel("인증 코드", { exact: true }).waitFor();
    await page.evaluate(() => {
      const rotation = window.__rotationHistory;
      rotation.hold = true;
      rotation.ready = new Promise(done => { rotation.sheetRequested = done; });
    });
    await page.setViewportSize({ width: 844, height: 390 });
    await page.evaluate(async () => {
      const rotation = window.__rotationHistory;
      await rotation.ready;
      await new Promise(done => requestAnimationFrame(() => requestAnimationFrame(done)));
      rotation.hold = false;
      for (const move of rotation.moves) {
        const landed = new Promise(done => addEventListener("popstate", done, { once: true }));
        move(); await landed;
      }
    });
    // 새로 그린 설정 탭은 인증 창 밑에 있어도 선택 상태를 보존해야 한다.
    const accountTab = page.getByRole("tab", { name: "계정", exact: true, includeHidden: true });
    assert.equal(await accountTab.getAttribute("aria-selected"), "true", "가로 회전은 계정 쪽을 바꾸지 않는다");
    await page.setViewportSize({ width: 390, height: 900 });
    await dialog.getByLabel("인증 코드", { exact: true }).fill(manualCode);
    await dialog.getByRole("button", { name: "확인", exact: true }).click();
    await dialog.getByText("로그인 완료", { exact: true }).waitFor();
    await closeCompleted(page, dialog, 390);
    await settings.getByTestId("settings-accounts").waitFor();
    assert.deepEqual(fixture.starts, ["claude"], "회전·닫기는 새 인증을 시작하지 않는다");
    await settings.getByRole("button", { name: "sample-claude 사용량 조회 로그인", exact: true }).click();
    await dialog.getByLabel("조회 계정 메일", { exact: true }).fill("me@example.com");
    await dialog.getByRole("button", { name: "조회 로그인", exact: true }).click();
    await dialog.getByLabel("인증 코드", { exact: true }).waitFor();
    assert.deepEqual(fixture.starts, ["claude", "query"], "복귀한 계정 쪽에서 요청을 한 번 보낸다");
    await dialog.getByRole("button", { name: "취소", exact: true }).click();
    await dialog.waitFor({ state: "detached" });
    assert.deepEqual(errors, []);
  } finally { await page.context().close(); }
}

export default async function () {
  await step("휴대폰 Chromium 인증 창 회전·설정 기록 경합", async () => { await checkRotation(browser); });
  await step("휴대폰 WebKit 인증 창 회전·설정 기록 경합", async () => { const wk = await webkit.launch(); try { await checkRotation(wk); } finally { await wk.close(); } });
  await step("PC Chromium 메뉴·설정 원격 인증", async () => { await check("chromium", browser, 1280); });
  await step("휴대폰 Chromium 메뉴·설정 원격 인증", async () => { await check("chromium", browser, 390); });
  await step("PC WebKit 메뉴·설정 원격 인증", async () => { const wk = await webkit.launch(); try { await check("webkit", wk, 1280); } finally { await wk.close(); } });
  await step("휴대폰 WebKit 메뉴·설정 원격 인증", async () => { const wk = await webkit.launch(); try { await check("webkit", wk, 390); } finally { await wk.close(); } });
  await step("PC Chromium 메일 등록·수정", async () => { await checkEmail("chromium", browser, 1280); });
  await step("휴대폰 Chromium 메일 등록·수정", async () => { await checkEmail("chromium", browser, 390); });
  await step("PC WebKit 메일 등록·수정", async () => { const wk = await webkit.launch(); try { await checkEmail("webkit", wk, 1280); } finally { await wk.close(); } });
  await step("휴대폰 WebKit 메일 등록·수정", async () => { const wk = await webkit.launch(); try { await checkEmail("webkit", wk, 390); } finally { await wk.close(); } });
}

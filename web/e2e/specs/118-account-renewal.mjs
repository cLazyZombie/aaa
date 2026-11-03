// 화면 계약은 가짜 제공자 응답으로, 권한·실제 자식 정리는 account_login::remote::tests가 검사한다.
import assert from "node:assert/strict";
import { token } from "../../../scripts/fixture-secret.mjs";
import { aaa, step, browser, webkit, pairPage, inList, openSettings, shot } from "../lib/harness.mjs";

const manualCode = `${token("manual-code")}#${token("manual-state")}`;

function provider(registered = false) {
  let accounts = [
    { name: "work-codex", harness: "codex", version: 1, health: "expired" },
    { name: "work-claude", harness: "claude", version: 1, health: "expiring", email: registered ? "me@example.com" : undefined },
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
      await inList(page, menu.getByRole("button", { name: "work-codex 인증 갱신", includeHidden: true }));
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
      await settings.getByRole("button", { name: "work-claude 다시 로그인", exact: true }).click();
      const dialog = page.getByTestId("account-login-dialog");
      const input = dialog.getByLabel("인증 코드", { exact: true });
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
      await dialog.getByRole("button", { name: "닫기", exact: true }).click();
      await dialog.waitFor({ state: "detached" });
      assert.deepEqual(fixture.starts, ["codex", "claude"]);
      assert.deepEqual(fixture.codes, [manualCode]);
      assert.equal(await page.getByTestId("account-renewal").getByRole("button", { includeHidden: true }).count(), 1);
      await settings.getByRole("button", { name: "work-claude 사용량 조회 로그인", exact: true }).click();
      await dialog.getByLabel("조회 계정 메일", { exact: true }).waitFor();
      assert.deepEqual(fixture.starts, ["codex", "claude"], "기대 메일을 입력하기 전에 로그인을 시작하지 않는다");
      await dialog.getByLabel("조회 계정 메일", { exact: true }).fill("me@example.com");
      await dialog.getByRole("button", { name: "조회 로그인", exact: true }).click();
      await dialog.getByRole("link", { name: "인증 열기", exact: true }).waitFor();
      await page.reload();
      await dialog.getByRole("heading", { name: /사용량 조회 로그인/ }).waitFor();
      await dialog.getByLabel("인증 코드", { exact: true }).fill(manualCode);
      await dialog.getByRole("button", { name: "확인", exact: true }).click();
      await dialog.getByText("로그인 완료", { exact: true }).waitFor();
      assert.deepEqual(fixture.starts, ["codex", "claude", "query"]);
      assert.deepEqual(fixture.codes, [manualCode, manualCode]);
      await shot(page, `118-query-${name}-${width}`);
      await dialog.getByRole("button", { name: "닫기", exact: true }).click();
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
    const row = settings.locator('[data-account="work-claude"]');
    await row.getByTestId("account-email").getByText("me@example.com", { exact: true }).waitFor();
    await row.getByRole("button", { name: "work-claude 메일 수정", exact: true }).click();
    const editor = row.getByTestId("account-email-edit");
    assert.equal(await editor.getByLabel("work-claude 메일", { exact: true }).inputValue(), "me@example.com");
    await editor.getByLabel("work-claude 메일", { exact: true }).fill("changed@example.com");
    await shot(page, `118-email-edit-${name}-${width}`);
    await editor.getByRole("button", { name: "메일 저장", exact: true }).click();
    await editor.waitFor({ state: "detached" });
    await row.getByTestId("account-email").getByText("changed@example.com", { exact: true }).waitFor();
    fixture.failNextQuery();
    await settings.getByRole("button", { name: "work-claude 사용량 조회 로그인", exact: true }).click();
    await dialog.getByRole("link", { name: "인증 열기", exact: true }).waitFor();
    assert.equal(await dialog.getByLabel("조회 계정 메일", { exact: true }).count(), 0, "저장된 메일로 바로 시작한다");
    await dialog.getByLabel("인증 코드", { exact: true }).fill(manualCode);
    await dialog.getByRole("button", { name: "확인", exact: true }).click();
    await dialog.getByText("공식 Claude CLI 인증이 실패했습니다", { exact: true }).waitFor();
    assert.ok(!(await dialog.innerText()).includes("같은 계정의 조회 인증을 저장하지 못했습니다"));
    await dialog.getByRole("button", { name: "닫기", exact: true }).click();
    await settings.getByTestId("account-alias").fill("personal");
    await settings.getByTestId("account-registration-email").fill("new@example.com");
    await shot(page, `118-email-registration-${name}-${width}`);
    await settings.getByRole("button", { name: "로그인", exact: true }).click();
    await dialog.getByLabel("인증 코드", { exact: true }).fill(manualCode);
    await dialog.getByRole("button", { name: "확인", exact: true }).click();
    await dialog.getByText("로그인 완료", { exact: true }).waitFor();
    await dialog.getByRole("button", { name: "닫기", exact: true }).click();
    await page.reload();
    await settings.getByTestId("settings-accounts").waitFor();
    await settings.locator('[data-account="personal"]').getByTestId("account-email").getByText("new@example.com", { exact: true }).waitFor();
    assert.deepEqual(errors, []);
  } finally { await page.context().close(); }
}

export default async function () {
  await step("PC Chromium 메뉴·설정 원격 인증", async () => { await check("chromium", browser, 1280); });
  await step("휴대폰 Chromium 메뉴·설정 원격 인증", async () => { await check("chromium", browser, 390); });
  await step("PC WebKit 메뉴·설정 원격 인증", async () => { const wk = await webkit.launch(); try { await check("webkit", wk, 1280); } finally { await wk.close(); } });
  await step("휴대폰 WebKit 메뉴·설정 원격 인증", async () => { const wk = await webkit.launch(); try { await check("webkit", wk, 390); } finally { await wk.close(); } });
  await step("PC Chromium 메일 등록·수정", async () => { await checkEmail("chromium", browser, 1280); });
  await step("휴대폰 Chromium 메일 등록·수정", async () => { await checkEmail("chromium", browser, 390); });
  await step("PC WebKit 메일 등록·수정", async () => { const wk = await webkit.launch(); try { await checkEmail("webkit", wk, 1280); } finally { await wk.close(); } });
  await step("휴대폰 WebKit 메일 등록·수정", async () => { const wk = await webkit.launch(); try { await checkEmail("webkit", wk, 390); } finally { await wk.close(); } });
}

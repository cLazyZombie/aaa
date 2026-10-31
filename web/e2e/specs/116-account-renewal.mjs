// 화면 계약은 가짜 제공자 응답으로, 권한·실제 자식 정리는 account_login::remote::tests가 검사한다.
import assert from "node:assert/strict";
import { token } from "../../../scripts/fixture-secret.mjs";
import { aaa, step, browser, webkit, pairPage, inList, openSettings, shot } from "../lib/harness.mjs";

const manualCode = `${token("manual-code")}#${token("manual-state")}`;

function provider() {
  let accounts = [
    { name: "work-codex", harness: "codex", version: 1, health: "expired" },
    { name: "work-claude", harness: "claude", version: 1, health: "expiring" },
    { name: "healthy", harness: "codex", version: 1, health: "ok" },
  ];
  let login = null, prompt = null;
  const codes = [], starts = [], seen = [];
  const route = async route => {
    const request = route.request().postDataJSON();
    seen.push(request.op);
    let value;
    if (request.op === "account_list") value = { kind: "accounts", accounts };
    else if (request.op === "account_login_remote") {
      starts.push(request.harness);
      login = { id: `fixture-${starts.length}`, account: request.name, harness: request.harness, state: "waiting" };
      prompt = request.harness === "codex"
        ? { kind: "device_code", url: "https://auth.openai.com/codex/device", user_code: "ABCD-1234" }
        : { kind: "paste_code", url: "https://claude.com/cai/oauth/authorize?code=true&state=fixture" };
      value = { kind: "account_login", login };
    } else if (request.op === "account_login_interaction") value = { kind: "account_login_interaction", login: request.id || login?.state === "waiting" ? login : null, prompt, expires_ms: Date.now() + 300_000 };
    else if (request.op === "account_login_code") {
      codes.push(request.code); login.state = "succeeded"; prompt = null;
      accounts = accounts.map(a => a.name === login.account ? { ...a, health: "ok" } : a);
      value = { kind: "done" };
    } else if (request.op === "account_login_cancel") {
      login.state = "cancelled"; prompt = null; value = { kind: "account_login", login };
    } else return route.continue();
    await route.fulfill({ json: { ok: value } });
  };
  return { route, starts, codes, seen };
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
      await shot(page, `116-codex-${name}-${width}`);
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
      await shot(page, `116-claude-${name}-${width}`);
      await dialog.getByRole("button", { name: "확인", exact: true }).click();
      await dialog.getByText("로그인 완료", { exact: true }).waitFor();
      await dialog.getByRole("button", { name: "닫기", exact: true }).click();
      await dialog.waitFor({ state: "detached" });
      assert.deepEqual(fixture.starts, ["codex", "claude"]);
      assert.deepEqual(fixture.codes, [manualCode]);
      assert.equal(await page.getByTestId("account-renewal").getByRole("button", { includeHidden: true }).count(), 1);
      assert.deepEqual(errors, []);
    }
  } catch (e) {
    e.message += ` · 진단 ${JSON.stringify({ errors, calls: fixture.seen, ui: await page.locator("body").innerText() })}`;
    throw e;
  } finally { await page.context().close(); }
}

export default async function () {
  await step("PC Chromium 메뉴·설정 원격 인증", async () => { await check("chromium", browser, 1280); });
  await step("휴대폰 Chromium 메뉴·설정 원격 인증", async () => { await check("chromium", browser, 390); });
  await step("PC WebKit 메뉴·설정 원격 인증", async () => { const wk = await webkit.launch(); try { await check("webkit", wk, 1280); } finally { await wk.close(); } });
  await step("휴대폰 WebKit 메뉴·설정 원격 인증", async () => { const wk = await webkit.launch(); try { await check("webkit", wk, 390); } finally { await wk.close(); } });
}

// 필요: 웹이 10분 뒤 마지막 숫자를 모름으로 바꾸거나 리셋 뒤 잔량을 합성하는 회귀를 막는다.
import assert from "node:assert/strict";
import { aaa, step, browser, webkit, pairPage, openSettings, inList } from "../lib/harness.mjs";

async function check(engine, width) {
  const observed = Date.now() - 45 * 60_000;
  let reset = Date.now() + 3_600_000;
  let failed = false;
  const route = async route => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/api/v1/account-usage") {
      if (failed) return route.fulfill({ status: 503, json: { err: { code: "unavailable", message: "시험 조회 실패" } } });
      return route.fulfill({ json: { ok: { accounts: [{
        account: "work", harness: "claude", problem: null, retry_at_ms: null,
        windows: [{ limit_id: "claude", window_id: "seven_day", scope: null, duration_mins: 10080,
          used_percent: 35, observed_at_ms: observed, resets_at_ms: reset, machine: "mac-a",
          source: "claude_statusline", received_at_ms: Date.now(), freshness: "stale", problem: null, conflict: false }],
      }] } } });
    }
    if (path === "/api/v1/usage") return route.fulfill({ json: { ok: { providers: [{
      id: "claude", key: "claude:registered:work", account: "work", plan: null, problem: "stale",
      checked_at: new Date(observed).toISOString(), windows: [{ kind: "week", scope: null,
        used_percent: 35, resets_at: new Date(reset).toISOString(), starts_at: null }],
    }] } } });
    if (path === "/api/v1/call" && route.request().postDataJSON().op === "account_list") {
      return route.fulfill({ json: { ok: { kind: "accounts", accounts: [{ name: "work", harness: "claude", version: 1, health: "ok" }] } } });
    }
    return route.continue();
  };
  const { page, errors } = await pairPage(engine, aaa("web", "pair").code, { width, height: 900 }, {
    serviceWorkers: "block", route: ["**/api/v1/**", route],
  });
  try {
    await inList(page, page.getByTestId("usage-strip"));
    const usage = page.getByRole("dialog", { name: "구독 사용량", exact: true });
    await usage.getByText(/45분 전 관측/).waitFor();
    assert.match(await usage.innerText(), /35%/);
    await page.keyboard.press("Escape");
    const settings = await openSettings(page, "계정");
    const window = settings.getByTestId("quota-window");
    await window.getByText("65% 남음", { exact: true }).waitFor();
    assert.equal(await window.getAttribute("data-state"), "stale");
    assert.match(await window.innerText(), /45분 전 관측/);
    assert.ok(!(await window.innerText()).includes("모름"));
    reset = Date.now() - 1;
    await settings.getByRole("button", { name: "사용량 다시 읽기", exact: true }).click();
    await window.getByText("마지막 65% 남음", { exact: true }).waitFor();
    assert.equal(await window.getAttribute("data-state"), "unknown");
    failed = true;
    await settings.getByRole("button", { name: "사용량 다시 읽기", exact: true }).click();
    await settings.getByTestId("quota-problem").filter({ hasText: "시험 조회 실패" }).waitFor();
    assert.match(await window.innerText(), /마지막 65% 남음/);
    assert.deepEqual(errors, []);
  } finally { await page.context().close(); }
}

export default async function () {
  await step("PC Chromium 마지막 사용량과 경과 시간", async () => { await check(browser, 1280); });
  await step("휴대폰 Chromium 마지막 사용량과 경과 시간", async () => { await check(browser, 390); });
  await step("PC WebKit 마지막 사용량과 경과 시간", async () => {
    const wk = await webkit.launch();
    try { await check(wk, 1280); } finally { await wk.close(); }
  });
  await step("휴대폰 WebKit 마지막 사용량과 경과 시간", async () => {
    const wk = await webkit.launch();
    try { await check(wk, 390); } finally { await wk.close(); }
  });
}

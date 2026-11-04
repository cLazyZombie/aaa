// 필요: 실제 host 저장·재조회와 PC/휴대폰 드롭다운·요금제별 창 표시를 함께 지킨다.
import assert from "node:assert/strict";
import fs from "node:fs";
import { token } from "../../../scripts/fixture-secret.mjs";
import { aaa, root, step, browser, webkit, pairPage, openSettings, inList, shot } from "../lib/harness.mjs";

async function check(engine, width) {
  const { page, errors } = await pairPage(engine, aaa("web", "pair").code, { width, height: 900 });
  try {
    let settings = await openSettings(page, "계정");
    let select = settings.getByLabel("sample 요금제", { exact: true });
    await select.selectOption("ultra");
    const row = settings.locator('[data-account="sample"]');
    await row.getByTestId("quota-session-status").filter({ hasText: "해당 없음" }).waitFor();
    const plans = await select.locator("option").allTextContents();
    assert.ok(plans.some(p => p.includes("Max") && p.includes("$100")));
    assert.ok(plans.some(p => p.includes("Ultra") && p.includes("$200")));
    assert.ok(!plans.some(p => p.includes("Plus")), "공급자별 목록만 표시");
    assert.ok(await select.evaluate(el => el.getBoundingClientRect().right <= innerWidth), "드롭다운이 좁은 화면 안에 있다");
    if (width === 1280) {
      const reader = (await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 1280, height: 900 })).page;
      try {
        const result = await reader.evaluate(async () => (await fetch("/api/v1/usage")).json());
        const registered = result.ok.providers.find(p => p.account === "sample");
        assert.equal(registered.plan, null, "보기 전용에는 등록 계정 요금제 이름을 보내지 않는다");
        assert.equal(registered.session_limit, false, "창 상태는 같은 정책을 따른다");
        const settings = await openSettings(reader, "계정");
        assert.equal(await settings.getByRole("combobox", { name: "sample 요금제", exact: true }).count(), 0);
      } finally { await reader.context().close(); }
    }
    await page.reload();
    // 새로고침은 계정 상세 경로를 복원한다. 좁은 화면에는 목록 탭이 없다.
    settings = page.getByRole("region", { name: "설정" });
    select = settings.getByLabel("sample 요금제", { exact: true });
    await select.waitFor();
    assert.equal(await select.inputValue(), "ultra", "새 페이지에서도 host 선택 유지");
    await settings.getByTestId("quota-session-status").filter({ hasText: "해당 없음" }).waitFor();
    await shot(page, `account-plans-${engine.browserType().name()}-${width}`);
    await settings.getByRole("button", { name: "설정 닫기", exact: true }).click();
    await inList(page, page.getByTestId("usage-strip"));
    const usage = page.getByRole("dialog", { name: "구독 사용량", exact: true }).getByRole("region", { name: "Claude · sample" });
    await usage.getByTestId("usage-session-status").filter({ hasText: "해당 없음" }).waitFor();
    // 요금제 칸은 금액 없이 한 줄이고 계정 이름과 겹치지 않는다(11-04 넘침).
    const plan = usage.getByTestId("usage-plan");
    assert.equal(await plan.textContent(), "Ultra · 인증됨");
    const box = await plan.evaluate(el => {
      const text = document.createRange(); text.selectNodeContents(el);
      const r = el.getBoundingClientRect(), a = el.parentElement.querySelector(".usage-account").getBoundingClientRect();
      return { lines: new Set([...text.getClientRects()].map(t => Math.round(t.top))).size, overlap: r.right > a.left, out: a.right > innerWidth };
    });
    assert.ok(box.lines === 1 && !box.overlap && !box.out, `요금제 칸 한 줄: ${JSON.stringify(box)}`);
    await shot(page, `usage-plan-${engine.browserType().name()}-${width}`);
    await page.keyboard.press("Escape");
    settings = await openSettings(page, "계정");
    select = settings.getByLabel("sample 요금제", { exact: true });
    await select.selectOption("ultra_pending");
    await settings.getByText("5시간 · 사용량 모름", { exact: true }).waitFor();
    await select.selectOption("pro");
    await settings.getByText("5시간 · 사용량 모름", { exact: true }).waitFor();
    const cleared = page.waitForResponse(async r => r.url().endsWith("/api/v1/call") && r.request().postDataJSON()?.op === "account_list"
      && (await r.json()).ok.accounts.some(a => a.name === "sample" && !a.plan_override));
    await select.selectOption("");
    // 저장 완료와 목록 재조회 응답을 울타리로 삼아 다음 기기의 변경과 겹치지 않는다.
    await cleared;
    await select.locator('option[value=""]:checked').waitFor({ state: "attached" });
    assert.equal(await select.inputValue(), "");
    assert.deepEqual(errors, []);
  } finally { await page.context().close(); }
}

export default async function () {
  await step("시험 계정 등록", async () => {
    const path = `${root}/sample-auth`;
    fs.writeFileSync(path, token("account-plan-e2e"));
    aaa("account", "import", "sample", "--harness", "claude", "--file", path);
  });
  await step("PC Chromium 요금제 저장과 5시간 창", async () => { await check(browser, 1280); });
  await step("휴대폰 WebKit 요금제 저장과 5시간 창", async () => {
    const wk = await webkit.launch();
    try { await check(wk, 390); } finally { await wk.close(); }
  });
}

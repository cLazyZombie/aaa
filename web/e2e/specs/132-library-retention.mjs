// 필요: 실제 웹 입력→host.toml 저장→재조회와 정리 미리보기가 같은 host 전체 한도를 쓰는 계약을 지킨다(t-1710).
import assert from "node:assert/strict";
import fs from "node:fs";
import { root, step, webkit, aaa, pairPage, openSettings, shot } from "../lib/harness.mjs";

const call = (page, request) => page.evaluate(async (request) => {
  const r = await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(request) });
  return r.json();
}, request);

async function check(page, phone) {
  const settings = await openSettings(page, "보고서");
  const days = settings.getByRole("spinbutton", { name: "보존 기간 (일)", exact: true });
  const mb = settings.getByRole("spinbutton", { name: "사본 용량 (MiB)", exact: true });
  const save = settings.getByRole("button", { name: "저장", exact: true });
  await days.waitFor();
  assert.equal(await days.inputValue(), "30");
  assert.equal(await mb.inputValue(), "20480");
  await days.fill("-1");
  await settings.getByRole("alert").waitFor();
  assert.equal(await save.isDisabled(), true);
  await days.fill("0.5");
  assert.equal(await save.isDisabled(), true);
  await days.fill("1");
  await mb.fill("1");
  await save.click();
  await settings.getByRole("status").filter({ hasText: "저장됨" }).waitFor();
  assert.match(fs.readFileSync(`${root}/c/host.toml`, "utf8"), /library_max_days = 1\n/);
  assert.match(fs.readFileSync(`${root}/c/host.toml`, "utf8"), /library_max_mb = 1\n/);
  const usage = await call(page, { op: "library_usage" });
  assert.equal(usage.ok.max_days, 1);
  assert.equal(usage.ok.max_size, 1048576);
  const plan = await call(page, { op: "library_prune_preview", project: null, older_than_days: null });
  assert.equal(plan.ok.max_days, 1);
  assert.equal(plan.ok.max_size, 1048576);
  await shot(page, `library-retention-${phone ? "phone" : "pc"}`);
  await days.fill("0");
  await mb.fill("0");
  await save.click();
  await settings.getByRole("status").filter({ hasText: "저장됨" }).waitFor();
  await settings.getByRole("button", { name: "설정 닫기", exact: true }).click();
  await settings.waitFor({ state: "detached" });
  await page.reload();
  const reopened = await openSettings(page, "보고서");
  const restored = reopened.getByRole("spinbutton", { name: "보존 기간 (일)", exact: true });
  await restored.waitFor();
  assert.equal(await restored.inputValue(), "0");
  assert.equal(await reopened.getByRole("spinbutton", { name: "사본 용량 (MiB)", exact: true }).inputValue(), "0");
  await restored.fill("30");
  await reopened.getByRole("spinbutton", { name: "사본 용량 (MiB)", exact: true }).fill("20480");
  await reopened.getByRole("button", { name: "저장", exact: true }).click();
  await reopened.getByRole("status").filter({ hasText: "저장됨" }).waitFor();
  await reopened.getByRole("button", { name: "설정 닫기", exact: true }).click();
  await reopened.waitFor({ state: "detached" });
}

export default async function ({ ctl, page }) {
  await step("PC에서 보존 한도 입력·저장·새로 읽기", async () => {
    await check(page, false);
    assert.deepEqual(ctl.errors, []);
  });
  await step("휴대폰 WebKit에서 보존 한도 입력·저장·새로 읽기", async () => {
    const wk = await webkit.launch();
    try {
      const phone = await pairPage(wk, aaa("web", "pair").code, { width: 390, height: 844 });
      try {
        await check(phone.page, true);
        assert.deepEqual(phone.errors, []);
      } finally { await phone.page.context().close(); }
    } finally { await wk.close(); }
  });
}

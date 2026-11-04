// 필요: 머신 agent가 알린 Codex 목록 → 웹 「모델」 켜기·끄기·모두 끄기 → host.toml 저장 → profile model 거절까지 한 흐름을 지킨다(t-1509).
import assert from "node:assert/strict";
import fs from "node:fs";
import { root, step, webkit, aaa, pairPage, openSettings, shot } from "../lib/harness.mjs";

// 실제 ~/.codex를 읽지 않게 agent의 Codex 폴더를 실험실 안으로 둔다
export const labEnv = { CODEX_HOME: `${root}/codex` };

const hostToml = () => fs.readFileSync(`${root}/c/host.toml`, "utf8");
// 설정 창을 닫고 그 쪽으로 다시 연다(휴대폰은 쪽을 연 뒤 목록 탭이 없다)
async function reopen(page, name) {
  const open = page.getByRole("region", { name: "설정" });
  if (await open.count()) {
    await open.getByRole("button", { name: "설정 닫기", exact: true }).click();
    await open.waitFor({ state: "detached" });
  }
  return openSettings(page, name);
}
const row = (settings, id) => settings.getByTestId("models-codex").locator(`li[data-model="${id}"]`);

async function check(page, phone) {
  let settings = await reopen(page, "모델");
  const codex = settings.getByTestId("models-codex");
  await row(settings, "gpt-a").waitFor();
  assert.deepEqual(await codex.locator("li").evaluateAll((l) => l.map((e) => e.dataset.model)), ["gpt-a", "gpt-b"], "숨긴 모델은 싣지 않는다");
  assert.ok(await settings.getByTestId("models-claude").locator('li[data-model="opus"]').count());

  await settings.getByRole("switch", { name: "gpt-b 끄기", exact: true }).click();
  await row(settings, "gpt-b").and(page.locator('[data-enabled="off"]')).waitFor();
  assert.match(hostToml(), /\[models\.codex\]\ndisabled = \["gpt-b"\]/);
  if (!phone) {
    await page.reload();
    settings = await reopen(page, "모델");
    await row(settings, "gpt-b").and(page.locator('[data-enabled="off"]')).waitFor();
  }
  await shot(page, `model-toggles-${phone ? "phone" : "pc"}`);

  settings = await reopen(page, "profile");
  await settings.getByTestId("profile-add").click();
  const dialog = page.getByRole("dialog", { name: "profile 더하기" });
  await dialog.getByTestId("profile-name").fill(`m${phone ? "p" : "c"}`);
  await dialog.getByTestId("profile-harness").selectOption("codex");
  // 켜진 모델만 후보다
  await dialog.locator('#profile-model-choices option[value="gpt-a"]').waitFor({ state: "attached" });
  assert.equal(await dialog.locator('#profile-model-choices option[value="gpt-b"]').count(), 0);
  await dialog.getByTestId("profile-model").fill("gpt-b");
  await dialog.getByTestId("profile-preview").click();
  await dialog.getByRole("alert").filter({ hasText: "꺼져 있다" }).waitFor();
  await dialog.getByRole("button", { name: "닫기", exact: true }).click();

  settings = await reopen(page, "모델");
  const codex2 = settings.getByTestId("models-codex");
  const bulk = settings.getByTestId("models-codex-bulk");
  await bulk.filter({ hasText: "모두 끄기" }).click();
  await bulk.filter({ hasText: "모두 켜기" }).waitFor();
  assert.equal(await codex2.locator('li[data-enabled="on"]').count(), 0);
  await bulk.click();
  await bulk.filter({ hasText: "모두 끄기" }).waitFor();
  assert.equal(await codex2.locator('li[data-enabled="off"]').count(), 0);
  assert.ok(!hostToml().includes("[models"), "모두 켜면 표가 빠진다");
}

export default async function ({ ctl, page }) {
  await step("머신의 Codex 모델 목록", async () => {
    fs.mkdirSync(`${root}/codex`, { recursive: true });
    fs.writeFileSync(
      `${root}/codex/models_cache.json`,
      JSON.stringify({
        models: [
          { slug: "gpt-a", display_name: "GPT-A", visibility: "list" },
          { slug: "gpt-hidden", display_name: "Hidden", visibility: "hide" },
          { slug: "gpt-b", display_name: "GPT-B", visibility: "list" },
        ],
      }),
    );
  });
  await step("PC 켜기·끄기와 profile 거절", async () => {
    await check(page, false);
    assert.deepEqual(ctl.errors, []);
  });
  await step("휴대폰 WebKit 켜기·끄기와 profile 거절", async () => {
    const wk = await webkit.launch();
    try {
      const phone = await pairPage(wk, aaa("web", "pair").code, { width: 390, height: 844 });
      try {
        await check(phone.page, true);
        assert.deepEqual(phone.errors, []);
      } finally {
        await phone.page.context().close();
      }
    } finally {
      await wk.close();
    }
  });
}

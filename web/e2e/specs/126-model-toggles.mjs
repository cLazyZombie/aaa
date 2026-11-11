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

async function checkBody(page, phone) {
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
  await armProfileTrace(page, phone);
  const name = `m${phone ? "p" : "c"}`;
  await dialog.getByTestId("profile-name").fill(name);
  assert.equal(await dialog.getByTestId("profile-name").inputValue(), name, "다른 필드 갱신도 입력 중인 이름을 보존한다");
  assert.equal(await dialog.getByTestId("profile-harness").inputValue(), "codex", "이름 입력도 함께 바꾼 harness를 되돌리지 않는다");
  assert.deepEqual(await page.evaluate(() => window.__profileInterleavings),
    [{ before: name, after: name, same: true }], "input capture 사이 실제 harness 변경을 한 번 검사한다");
  await dialog.getByTestId("profile-harness").selectOption("codex");
  // 켜진 모델만 후보다
  await dialog.locator('#profile-model-choices option[value="gpt-a"]').waitFor({ state: "attached" });
  assert.equal(await dialog.locator('#profile-model-choices option[value="gpt-b"]').count(), 0);
  await dialog.getByTestId("profile-model").fill("gpt-b");
  assert.equal(await dialog.getByTestId("profile-name").inputValue(), name);
  assert.equal(await dialog.getByTestId("profile-model").inputValue(), "gpt-b");
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

async function armProfileTrace(page, phone) {
  await page.evaluate(name => {
    window.__profileInterleavings = [];
    window.__profileTrace = [];
    const snapshot = () => {
      const name = document.querySelector('[data-testid="profile-name"]');
      return { at: performance.now(), name: name?.value, nameDisabled: name?.disabled,
        harness: document.querySelector('[data-testid="profile-harness"]')?.value,
        model: document.querySelector('[data-testid="profile-model"]')?.value,
        disabled: document.querySelector('[data-testid="profile-preview"]')?.disabled };
    };
    const input = event => {
      const element = event.target;
      if (element.dataset?.testid !== "profile-name" || element.value !== name || window.__profileInterleavings.length) return;
      const before = element.value;
      const harness = document.querySelector('[data-testid="profile-harness"]');
      harness.value = "codex";
      harness.dispatchEvent(new Event("change", { bubbles: true }));
      window.__profileInterleavings.push({ before, after: element.value,
        same: element === document.querySelector('[data-testid="profile-name"]') });
    };
    const root = document; // AdminDialog는 body portal에 그린다.
    root.addEventListener("input", input, true);
    let before;
    const observer = new MutationObserver(() => {
      const value = snapshot();
      const key = JSON.stringify({ ...value, at: 0 });
      if (key !== before) window.__profileTrace.push(value);
      before = key;
    });
    observer.observe(document, { subtree: true, attributes: true, childList: true });
    window.__stopProfileTrace = () => { root.removeEventListener("input", input, true); observer.disconnect(); };
  }, `m${phone ? "p" : "c"}`);
}

// native input과 React bubble 사이에 같은 폼의 실제 harness 변경을 끼운다.
// 이름·React state를 대신 채우거나 실패한 이름 입력을 다시 넣지 않는다.
async function check(page, phone) {
  const requests = [];
  const onRequest = request => {
    if (request.url().endsWith("/api/v1/call") && request.postDataJSON()?.op === "profile_plan") requests.push(request.postDataJSON());
  };
  page.on("request", onRequest);
  try {
    await checkBody(page, phone);
    assert.deepEqual(requests.map(r => [r.change.name, r.change.harness, r.change.model]),
      [[`m${phone ? "p" : "c"}`, "codex", "gpt-b"]], "입력한 이름과 꺼진 모델의 미리보기를 한 번 요청한다");
  } catch (error) {
    try {
      const dir = `${root}/diagnostics`;
      fs.mkdirSync(dir, { recursive: true });
      fs.writeFileSync(`${dir}/profile-input-${phone ? "phone" : "pc"}.json`, JSON.stringify({
        error: { message: error.message, stack: error.stack, actual: error.actual, expected: error.expected }, requests,
        dom: await page.evaluate(() => ({ interleavings: window.__profileInterleavings, trace: window.__profileTrace,
          dialog: document.querySelector('[aria-label="profile 더하기"]')?.outerHTML })).catch(String),
      }, null, 2));
    } catch (diagnostic) { console.log(`profile 입력 실패 진단 저장 오류: ${diagnostic}`); }
    throw error;
  } finally {
    page.off("request", onRequest);
    await page.evaluate(() => window.__stopProfileTrace?.()).catch(() => {});
  }
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

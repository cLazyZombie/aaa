// 할 일 편집에서 검수 설정을 저장해 다시 열고 각 칸을 기본값으로 돌린다.
import assert from "node:assert/strict";
import fs from "node:fs";
import { token } from "../../../scripts/fixture-secret.mjs";
import { aaa, step, browser, webkit, pairPage, root } from "../lib/harness.mjs";

const config = `${root}/c/host.toml`;
fs.writeFileSync(config, fs.readFileSync(config, "utf8")
  .replace('allow_profiles = ["fake"]', 'allow_profiles = ["fake", "review-claude"]')
  + '\n[profiles.review-claude]\nagent = "claude"\nmodel = "review-model"\n');

export default async function () {
  await step("검수 profile·등록 계정과 할 일 준비", async () => {
    const auth = `${root}/review-auth`;
    fs.writeFileSync(auth, token("task-review-account-e2e"));
    aaa("account", "import", "review-sample", "--harness", "claude", "--file", auth);
    aaa("project", "create", "demo");
    aaa("project", "set", "demo", "thread_profile", "fake");
    aaa("task", "add", "검수할 일", "--profile", "fake", "--note", "원래 메모");
  }, { always: true });

  const wk = await webkit.launch({ headless: true });
  try {
    for (const [engine, name, width] of [[browser, "데스크톱 Chromium", 1280], [wk, "휴대폰 WebKit", 393]]) {
      await step(`검수 설정 선택·유지·초기화 ${name}`, async () => {
        const { page } = await pairPage(engine, aaa("web", "pair").code, { width, height: 844 }, { hash: "#tasks" });
        try {
          const task = () => aaa("task", "list").tasks.find(t => t.title === "검수할 일");
          const row = page.locator('.task-row[data-title="검수할 일"] .task-pick');
          await row.click();
          const model = page.getByTestId("task-field-review-profile");
          const account = page.getByTestId("task-field-review-account");
          await model.waitFor();
          assert.equal(await model.inputValue(), "");
          assert.equal(await account.inputValue(), "");
          async function save(field, value) {
            const edited = page.waitForResponse(response => response.url().endsWith("/api/v1/call")
              && response.request().method() === "POST"
              && response.request().postDataJSON().op === "task_edit");
            await field.selectOption(value);
            const result = await (await edited).json();
            assert.equal(result.ok?.kind, "task_edited", JSON.stringify(result));
            assert.equal(result.ok.conflict, null);
          }
          await save(model, "review-claude");
          await save(account, "review-sample");
          assert.deepEqual(task().review, { profile: "review-claude", account: "review-sample" });
          await page.getByTestId("task-detail-close").click();
          await model.waitFor({ state: "hidden" });
          await row.click();
          await model.waitFor();
          assert.equal(await model.inputValue(), "review-claude");
          assert.equal(await account.inputValue(), "review-sample");
          await save(account, "");
          assert.deepEqual(task().review, { profile: "review-claude" });
          await save(model, "");
          assert.equal(task().review, undefined);
          assert.deepEqual(task().notes, ["원래 메모"]);
        } finally {
          await page.context().close();
        }
      });
    }
  } finally {
    await wk.close();
  }
}

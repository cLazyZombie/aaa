// 다중 선행 선택·보존·남은 이유·갱신을 실제 host와 두 브라우저에서 확인한다.
import assert from "node:assert/strict";
import fs from "node:fs";
import { token } from "../../../scripts/fixture-secret.mjs";
import { aaa, step, browser, webkit, pairPage, domIs, root, shot } from "../lib/harness.mjs";

// 같은 가짜 프로그램을 Claude harness로 실행해 계정 선택과 선행 조건을 함께 검사한다.
const config = `${root}/c/host.toml`;
fs.writeFileSync(config, fs.readFileSync(config, "utf8")
  .replace('allow_profiles = ["fake"]', 'allow_profiles = ["fake", "fake-claude"]')
  + `\n[profiles.fake-claude]\nagent = "claude"\ncommand = ["/bin/sh", "${root}/fake.sh"]\n`);

export default async function () {
  await step("다중 선행 예약 준비", async () => {
    const auth = `${root}/reservation-auth`;
    fs.writeFileSync(auth, token("reservation-account-e2e"));
    aaa("account", "import", "sample", "--harness", "claude", "--file", auth);
    aaa("project", "create", "demo");
    aaa("project", "set", "demo", "thread_profile", "fake");
  }, { always: true });
  const wk = await webkit.launch({ headless: true });
  try {
    for (const [engine, name] of [[browser, "chromium"], [wk, "webkit"]]) {
      for (const width of [1280, 390]) {
        await step(`여러 앞 할 일 선택·갱신 ${name} ${width}`, async () => {
          fs.writeFileSync(`${root}/projects/demo/TASKS.md`, "## Backlog\n- [ ] 첫째 (fake-claude@labA) [작업 폴더]\n  첫 범위\n- [ ] 둘째 (fake-claude@labA) [작업 폴더]\n  둘 범위\n- [ ] 합류 (fake-claude@labA) [작업 폴더]\n  합류 범위\n");
          const tasks = aaa("task", "list").tasks;
          const first = tasks.find(t => t.title === "첫째"), second = tasks.find(t => t.title === "둘째"), join = tasks.find(t => t.title === "합류");
          const p = (await pairPage(engine, aaa("web", "pair").code, { width, height: 844 }, { hash: "#tasks" })).page;
          try {
            await p.locator('.task-row[data-title="합류"] .task-pick').click();
            await p.getByTestId("task-reserve-preview").click();
            const account = p.getByRole("combobox", { name: "예약 계정" });
            assert.equal(await account.inputValue(), "");
            await account.selectOption("sample");
            const choices = p.getByTestId("task-reserve-after");
            await choices.getByRole("checkbox", { name: `${first.short_id} · 첫째` }).check();
            await choices.getByRole("checkbox", { name: `${second.short_id} · 둘째` }).check();
            await p.getByTestId("task-reserve-go").click();
            await p.getByTestId("task-reserve-confirm").waitFor({ state: "hidden" });
            aaa("ticker", "tick");
            await domIs(p, "여러 선행 대기 이유", () => document.querySelector('[data-testid="task-queue-why"]')?.textContent?.includes("완료 대기"));
            assert.deepEqual(aaa("task", "queue").entries.find(q => q.task_id === join.id).after_all, [first.id, second.id]);
            assert.equal(aaa("task", "queue").entries.find(q => q.task_id === join.id).account, "sample");
            const why = await p.getByTestId("task-queue-why").textContent();
            assert(why.includes(first.short_id) && why.includes(second.short_id));
            await shot(p, `task-dependencies-${name}-${width}`);
            await p.getByTestId("task-reserve-preview").click();
            await account.waitFor();
            assert.equal(await account.inputValue(), "sample");
            assert.equal(await choices.getByRole("checkbox", { name: `${first.short_id} · 첫째` }).isChecked(), true);
            assert.equal(await choices.getByRole("checkbox", { name: `${second.short_id} · 둘째` }).isChecked(), true);
            await choices.getByRole("checkbox", { name: `${first.short_id} · 첫째` }).uncheck();
            await p.getByTestId("task-reserve-go").click();
            await p.getByTestId("task-reserve-confirm").waitFor({ state: "hidden" });
            const updated = aaa("task", "queue").entries.find(q => q.task_id === join.id);
            assert.deepEqual(updated.after_all, [second.id]);
            assert.equal(updated.account, "sample");
            assert.equal(updated.thread, null);
            aaa("task", "unreserve", join.short_id);
          } finally { await p.context().close(); }
        });
      }
    }
  } finally { await wk.close(); }
}

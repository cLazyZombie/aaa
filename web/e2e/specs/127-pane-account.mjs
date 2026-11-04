// 필요: 계정으로 띄운 thread의 터미널 머리 줄과 작업 요약 창이 크기(63×39) 대신 그 계정을 보이고,
// 계정을 모르는 pane은 아무것도 보이지 않으며, 휴대폰 폭에서도 계정이 잘리지 않는 계약을 지킨다(t-1531).
import assert from "node:assert/strict";
import fs from "node:fs";
import { token } from "../../../scripts/fixture-secret.mjs";
import { root, aaa, until, step, browser, webkit, pairPage, shot } from "../lib/harness.mjs";

const config = `${root}/c/host.toml`;
// 계정 인증은 harness가 같아야 붙는다: 가짜 agent(fake.sh)를 Claude profile로 띄운다
fs.writeFileSync(config, fs.readFileSync(config, "utf8")
  .replace('allow_profiles = ["fake"]', 'allow_profiles = ["fake", "fake-claude"]')
  + `\n[profiles.fake-claude]\nagent = "claude"\ncommand = ["/bin/sh", "${root}/fake.sh"]\n`);

let thread = null;

async function check(engine, width) {
  const { page, errors } = await pairPage(engine, aaa("web", "pair").code, { width, height: 844 }, { hash: `#pane=${thread.pane}` });
  try {
    const head = page.locator(`section.term[data-pane="${thread.pane}"] .term-title .term-sub`);
    const account = head.getByTestId("pane-account");
    await account.waitFor();
    assert.equal((await account.innerText()).trim(), width > 640 ? "· 계정 sample-claude" : "sample-claude");
    // 크기는 화면에 쓰지 않는다
    assert.doesNotMatch(await head.textContent(), /\d+×\d+/);
    await shot(page, `pane-account-${engine.browserType().name()}-${width}`);
    // 머리 줄은 화면 밖으로 넘치지 않는다. 넓은 창은 계정 전체를, 휴대폰(단추가 많아 제목 칸이 좁다)은 위치 글을 빼고
    // 계정이 줄 맨 앞부터 그 칸을 쓴다(긴 이름은 말줄임, 전체는 요약 시트)
    const fit = await account.evaluate((el) => {
      const a = el.getBoundingClientRect(), line = el.parentElement.getBoundingClientRect();
      return { whole: el.scrollWidth <= el.clientWidth, first: a.left - line.left < 1 && a.width > 0,
        inside: a.right <= line.right + 0.5 && line.right <= window.innerWidth };
    });
    assert.deepEqual(fit, width > 640 ? { whole: true, first: false, inside: true } : { whole: fit.whole, first: true, inside: true });

    await page.locator(`section.term[data-pane="${thread.pane}"]`).getByTestId("summary-open").click();
    const summary = page.getByTestId("summary-account");
    await summary.waitFor();
    assert.equal(await summary.textContent(), "계정 sample-claude");
    await shot(page, `pane-account-summary-${engine.browserType().name()}-${width}`);
    assert.deepEqual(errors, []);
  } finally { await page.context().close(); }
}

export default async function () {
  await step("계정으로 thread 시작", async () => {
    const path = `${root}/sample-auth`;
    fs.writeFileSync(path, token("pane-account-e2e"));
    aaa("account", "import", "sample-claude", "--harness", "claude", "--file", path);
    if (!aaa("project", "list").projects.some((p) => p.name === "demo")) aaa("project", "create", "demo");
    fs.writeFileSync(`${root}/account-task.md`, "계정 표시 확인\n");
    const t = aaa("thread", "start", "demo", "--title", "계정 표시", "--machine", "labA", "--profile", "fake-claude",
      "--account", "sample-claude", "--kind", "dir", "--task-file", `${root}/account-task.md`);
    thread = await until("thread pane", () => {
      const v = aaa("thread", "show", t.id);
      return v.pane && v.account === "sample-claude" && v;
    });
  });
  await step("계정을 모르는 pane은 계정을 보이지 않는다", async () => {
    const shell = aaa("pane", "list").panes.find((p) => p.name === "t1");
    const { page, errors } = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 }, { hash: `#pane=${shell.id}` });
    try {
      const term = page.locator(`section.term[data-pane="${shell.id}"]`);
      await term.locator(".term-title .term-sub").waitFor();
      assert.equal(await term.getByTestId("pane-account").count(), 0);
      assert.deepEqual(errors, []);
    } finally { await page.context().close(); }
  });
  await step("PC Chromium 머리 줄·요약 창의 계정", async () => { await check(browser, 1280); });
  await step("휴대폰 WebKit 머리 줄·요약 시트의 계정", async () => {
    const wk = await webkit.launch();
    try { await check(wk, 390); } finally { await wk.close(); }
  });
}

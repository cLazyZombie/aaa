// 필요: 계정으로 띄운 thread의 터미널 머리 줄과 작업 요약 창이 크기(63×39) 대신 그 계정을 보이고,
// 계정을 모르는 pane은 아무것도 보이지 않으며, 휴대폰 폭에서도 계정이 잘리지 않는 계약을 지킨다(t-1531).
import assert from "node:assert/strict";
import fs from "node:fs";
import { token } from "../../../scripts/fixture-secret.mjs";
import { root, AA, aaa, until, step, browser, webkit, pairPage, shot, waitOutput } from "../lib/harness.mjs";

const config = `${root}/c/host.toml`;
// 계정 인증은 harness가 같아야 붙는다: 가짜 agent(fake.sh)를 Claude profile로 띄운다
fs.writeFileSync(config, fs.readFileSync(config, "utf8")
  .replace('allow_profiles = ["fake"]', 'allow_profiles = ["fake", "fake-claude"]')
  + `\n[profiles.fake-claude]\nagent = "claude"\ncommand = ["/bin/sh", "${root}/fake.sh"]\n`);

let thread = null;

// 머리 줄 폭 520px 이하(t-1987, Term.tsx TIGHT_HEAD)는 보고·요약·자동 병합을 「더 보기」 맨 위로 모은다. 모든 단추가 머리 안이고
// 제목 칸이 남는다(320px 휴대폰은 전에 제목 0px·「더 보기」가 화면 밖, 목록이 붙은 창도 같았다)
async function headFits(page, tight) {
  const head = page.locator(`section.term[data-pane="${thread.pane}"] > .term-head`);
  const got = await head.evaluate((h) => {
    const box = h.getBoundingClientRect();
    const shown = (sel) => !!h.querySelector(sel)?.getClientRects().length;
    return {
      tight: h.hasAttribute("data-tight"),
      title: Math.round(h.querySelector(".term-title").getBoundingClientRect().width),
      over: [...h.children].filter((c) => c.getClientRects().length && c.getBoundingClientRect().right > Math.min(box.right, window.innerWidth) + 0.5).map((c) => c.className),
      buttons: [shown("[data-testid='open-report']"), shown(".summary-btn"), shown(".pane-auto-merge [role=switch]")],
    };
  });
  assert.deepEqual({ tight: got.tight, over: got.over, buttons: got.buttons, titleShown: got.title >= 80 },
    { tight, over: [], buttons: [!tight, !tight, !tight], titleShown: true }, JSON.stringify(got));
  if (!tight) return;
  await head.getByTestId("more").click();
  const items = await page.locator(".more-menu").evaluate((m) => [...m.querySelectorAll("[role^=menuitem]")].slice(0, 3).map((b) => b.textContent.trim()));
  assert.deepEqual(items, ["보고", "작업 요약", "자동 병합"]);
  await page.keyboard.press("Escape");
  await page.locator(".more-menu").waitFor({ state: "detached" });
}

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
    if (width > 640) assert.deepEqual(fit, { whole: true, first: false, inside: true });
    else assert.deepEqual({ first: fit.first, inside: fit.inside }, { first: true, inside: true });
    // 자동 병합이 있는 좁은 머리줄(t-1968 q-4124 1A)은 상태 표시를 진행률 숫자만 두어 제목 칸을 남긴다
    const chip = await page.locator(`section.term[data-pane="${thread.pane}"] > .term-head .chip.agent`).innerText();
    assert.deepEqual({ numberOnly: chip.trim() === "50%", pct: chip.trim().endsWith("50%") }, { numberOnly: width <= 700, pct: true }, chip);
    const tight = width <= 520;
    await headFits(page, tight);

    // 좁은 머리는 요약을 「더 보기」 항목으로 연다
    if (tight) {
      await page.locator(`section.term[data-pane="${thread.pane}"] > .term-head`).getByTestId("more").click();
      await page.getByTestId("summary-item").click();
    } else await page.locator(`section.term[data-pane="${thread.pane}"]`).getByTestId("summary-open").click();
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
    // 머리줄 상태 표시의 진행률(실제 세션의 진행 보고)
    fs.writeFileSync(`${root}/account-progress.sh`, `"${AA}" report --percent 50 --activity '계정 표시'\nprintf 'account-progress-done\\n'\n`);
    aaa("pane", "run", thread.pane, `post-probe ${root}/account-progress.sh`);
    await waitOutput(thread.pane, "account-progress-done");
    thread = await until("진행률 50%", () => { const v = aaa("thread", "show", t.id); return v.agent?.percent === 50 && v; });
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
  // 목록이 옆에 붙은 창(t-1968·t-1987): 769px는 머리줄 400px라 전에 「더 보기」가 밖으로 나갔다. 좁은 머리는 휴대폰처럼 모으고,
  // 그보다 넓은 머리(1000px 창, 머리 631px)는 단추를 그대로 두고 아이콘·숫자로만 줄인다
  await step("목록이 붙은 Chromium 창의 머리 줄", async () => {
    for (const [width, tight] of [[769, true], [1000, false]]) {
      const { page, errors } = await pairPage(browser, aaa("web", "pair").code, { width, height: 800 }, { hash: `#pane=${thread.pane}` });
      try {
        const head = page.locator(`section.term[data-pane="${thread.pane}"] > .term-head`);
        await head.locator(".pane-auto-merge[data-narrow]").waitFor({ state: "attached" });
        assert.equal((await head.locator(".chip.agent").innerText()).trim(), "50%");
        await headFits(page, tight);
        await shot(page, `pane-head-chromium-${width}`);
        assert.deepEqual(errors, []);
      } finally { await page.context().close(); }
    }
  });
  await step("320px 휴대폰 Chromium 머리 줄·요약 시트", async () => { await check(browser, 320); });
  // 작업 요약 단추가 「⋯」로 들어간 좁은 머리는 그 단추의 「반영 안 된 지시 있음」 점을 「⋯」 단추가 단다(t-1987 q-4182 1B).
  // 넓은 머리는 전처럼 요약 단추의 점이다. 지시는 brief를 받은 thread에 보낸 메시지(Follow-ups)다
  await step("반영 안 된 지시가 있으면 좁은 머리의 「⋯」에 점", async () => {
    fs.writeFileSync(`${root}/dot-task.md`, "점 확인\n");
    const t = aaa("thread", "start", "demo", "--title", "점 표시", "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/dot-task.md`);
    const v = await until("brief 전달", () => { const x = aaa("thread", "show", t.id); return x.pane && x.brief === "delivered" && x; });
    fs.writeFileSync(`${root}/dot-msg.md`, "색만 바꾼다\n");
    aaa("thread", "message", t.id, "--text-file", `${root}/dot-msg.md`);
    await until("반영 안 된 지시 1개", () => aaa("thread", "show", t.id).summary?.pending === 1);
    for (const [width, tight] of [[320, true], [1280, false]]) {
      const { page, errors } = await pairPage(browser, aaa("web", "pair").code, { width, height: 800 }, { hash: `#pane=${v.pane}` });
      try {
        const head = page.locator(`section.term[data-pane="${v.pane}"] > .term-head`);
        await head.locator(".summary-btn.is-stale").waitFor({ state: "attached" });
        const got = await head.evaluate((h) => ({
          more: !!h.querySelector("[data-testid='more-dot'].is-notice")?.getClientRects().length,
          summary: !!h.querySelector(".summary-btn .summary-dot")?.getClientRects().length,
          label: h.querySelector("[data-testid='more']").getAttribute("aria-label"),
        }));
        assert.deepEqual(got, tight
          ? { more: true, summary: false, label: "더 보기 · 반영 안 된 지시 있음" }
          : { more: false, summary: true, label: "더 보기" });
        if (tight) {
          await head.getByTestId("more").click();
          assert.equal(await page.getByTestId("summary-item").locator(".more-hint").textContent(), "반영 안 된 지시 있음");
        }
        await shot(page, `pane-head-stale-${width}`);
        assert.deepEqual(errors, []);
      } finally { await page.context().close(); }
    }
  });
  await step("휴대폰 WebKit 머리 줄·요약 시트의 계정", async () => {
    const wk = await webkit.launch();
    try { await check(wk, 390); } finally { await wk.close(); }
  });
}

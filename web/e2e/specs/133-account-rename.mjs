// 필요: 사용 중 이름 저장 성공·기존/새 thread 이름 분리와 화면 밖으로 숨었던 저장 오류를 실제 host로 지킨다.
import assert from "node:assert/strict";
import fs from "node:fs";
import { token } from "../../../scripts/fixture-secret.mjs";
import { root, aaa, until, step, browser, webkit, pairPage, openSettings, domIs, waitOutput, shot } from "../lib/harness.mjs";

const config = `${root}/c/host.toml`;
fs.writeFileSync(config, fs.readFileSync(config, "utf8")
  .replace('allow_profiles = ["fake"]', 'allow_profiles = ["fake", "fake-claude"]')
  + `\n[profiles.fake-claude]\nagent = "claude"\ncommand = ["/bin/sh", "${root}/fake.sh"]\n`);
const auth = `${root}/rename-auth`;
fs.writeFileSync(auth, token("account-rename-e2e"));
const task = `${root}/rename-task.md`;
fs.writeFileSync(task, "사람을 기다림\n");
const register = name => aaa("account", "import", name, "--harness", "claude", "--file", auth);

async function startThread(name) {
  const t = aaa("thread", "start", "demo", "--title", "이름 변경", "--machine", "labA", "--profile", "fake-claude",
    "--account", name, "--kind", "dir", "--task-file", task);
  const rec = await until("계정 thread pane", () => { const r = aaa("thread", "show", t.id); return r.pane && r; });
  await waitOutput(rec.pane, "fake>");
  return rec;
}

async function check(engine, width) {
  const suffix = `${engine.browserType().name()}-${width}`;
  const before = `sample-${suffix}`, after = `team-${suffix}`, collision = `taken-${suffix}`;
  register(before);
  aaa("account", "default", "claude", before);
  const existing = await startThread(before);
  const { page, errors } = await pairPage(engine, aaa("web", "pair").code, { width, height: 844 }, { serviceWorkers: "block" });
  try {
    const settings = await openSettings(page, "계정");
    let row = settings.locator(`[data-account="${before}"]`);
    await row.getByRole("button", { name: `${before} 이름 바꾸기`, exact: true }).click();
    await row.getByLabel(`${before} 새 이름`, { exact: true }).fill(after);
    await row.getByRole("button", { name: "이름 저장", exact: true }).click();
    row = settings.locator(`[data-account="${after}"]`);
    await row.getByTestId("account-renamed").waitFor();
    assert.equal(aaa("thread", "show", existing.id).account, before, "기존 스레드 이름 유지");
    assert.equal(aaa("thread", "show", existing.id).pane, existing.pane, "기존 실행을 교체하지 않는다");
    assert.equal(aaa("account", "default", "claude").defaults[0].account, after);
    assert.ok(aaa("account", "pool", "list").pool.members.some(m => m.account === after));
    assert.ok(!aaa("account", "pool", "list").pool.members.some(m => m.account === before));
    const next = await startThread(after);
    assert.equal(next.account, after, "새 스레드는 새 이름 사용");
    assert.equal(aaa("pane", "list").panes.find(p => p.id === existing.pane).account, before);
    assert.equal(aaa("pane", "list").panes.find(p => p.id === next.pane).account, after);

    await row.getByRole("button", { name: `${after} 이름 바꾸기`, exact: true }).click();
    const input = row.getByLabel(`${after} 새 이름`, { exact: true });
    await input.fill(collision);
    // 목록 조회 뒤 전송 직전에 다른 조작이 이름을 차지한다. 실제 host가 NameTaken으로 거부한다.
    let intercepted = false, releaseList;
    const listGate = new Promise(resolve => { releaseList = resolve; });
    const route = async route => {
      const request = route.request().postDataJSON();
      if (request.op === "account_rename" && request.to === collision) {
        intercepted = true;
        register(collision);
      }
      // 오류를 표시한 다음 새 행을 반영해, 뒤늦은 목록 변경으로 밀리는 순서를 고정한다.
      if (request.op === "account_list" && intercepted) await listGate;
      await route.continue();
    };
    await page.route("**/api/v1/call", route);
    try {
      const rejected = page.waitForResponse(r => r.url().endsWith("/api/v1/call") && r.request().postDataJSON()?.op === "account_rename");
      await row.getByRole("button", { name: "이름 저장", exact: true }).click();
      const reply = await (await rejected).json();
      assert.ok(intercepted, "실제 전송 직전에 충돌 계정을 등록했다");
      assert.equal(reply.err?.code, "name_taken", JSON.stringify(reply));
      const form = row.getByTestId("account-rename");
      const error = form.getByRole("alert");
      await error.waitFor();
      assert.equal(await error.textContent(), "이미 쓰는 계정 이름이다");
      assert.equal(await input.inputValue(), collision, "실패 뒤 입력 유지");
      assert.match(await input.getAttribute("aria-describedby"), /account-rename-error/);
      releaseList();
      await settings.locator(`[data-account="${collision}"]`).waitFor({ state: "attached" });
      await domIs(page, "오류가 입력 아래·저장 위·현재 화면 안에 표시", () => {
        const form = document.querySelector('[data-testid="account-rename"]');
        const alert = form?.querySelector('[role="alert"]');
        const input = form?.querySelector("input");
        const button = form?.querySelector('button[type="submit"]');
        if (!alert || !input || !button) return false;
        const a = alert.getBoundingClientRect(), i = input.getBoundingClientRect(), b = button.getBoundingClientRect();
        return a.top >= i.bottom && a.bottom <= b.top && a.top >= 0 && a.bottom <= innerHeight
          && alert.contains(document.elementFromPoint(a.left + a.width / 2, a.top + a.height / 2));
      });
      await shot(page, `account-rename-error-${suffix}`);
      await form.getByRole("button", { name: "취소", exact: true }).click();
      await form.waitFor({ state: "detached" });
      await row.getByRole("button", { name: `${after} 이름 바꾸기`, exact: true }).click();
      assert.equal(await row.getByTestId("account-rename").getByRole("alert").count(), 0, "새 편집에 옛 오류가 남지 않는다");
      assert.deepEqual(errors, []);
    } finally { releaseList(); await page.unroute("**/api/v1/call", route); }
  } catch (error) {
    const diagnostics = `${root}/diagnostics`;
    fs.mkdirSync(diagnostics, { recursive: true });
    await page.screenshot({ path: `${diagnostics}/rename-${suffix}.png`, animations: "disabled" });
    const position = await page.evaluate(() => {
      const form = document.querySelector('[data-testid="account-rename"]');
      const rect = el => {
        if (!el) return null;
        const r = el.getBoundingClientRect();
        return { top: r.top, bottom: r.bottom, left: r.left, right: r.right,
          hit: document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2)?.outerHTML.slice(0, 300) };
      };
      return { input: rect(form?.querySelector("input")), alert: rect(form?.querySelector('[role="alert"]')),
        button: rect(form?.querySelector('button[type="submit"]')), height: innerHeight };
    });
    throw new Error(`${error.message}; 이름 변경 위치: ${JSON.stringify(position)}`);
  } finally { await page.context().close(); }
}

export default async function () {
  await step("이름 변경 실험실 준비", async () => {
    if (!aaa("project", "list").projects.some(p => p.name === "demo")) aaa("project", "create", "demo");
    for (const name of ["alpha", "bravo", "charlie"]) register(name);
  });
  await step("PC Chromium 실행 중 이름 변경과 입력 아래 오류", async () => { await check(browser, 1280); });
  await step("Mac WebKit 실행 중 이름 변경과 입력 아래 오류", async () => {
    const wk = await webkit.launch();
    try { await check(wk, 1280); } finally { await wk.close(); }
  });
  await step("휴대폰 WebKit 실행 중 이름 변경과 입력 아래 오류", async () => {
    const wk = await webkit.launch();
    try { await check(wk, 390); } finally { await wk.close(); }
  });
}

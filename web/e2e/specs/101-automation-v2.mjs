// schema 2 이관·동작 영수증·원장 UI. 실제 host/SQLite를 사용한다.
import assert from "node:assert/strict";
import fs from "node:fs";
import { root, aaa, step, until, shot, webkit, pairPage, drawerSettled } from "../lib/harness.mjs";
const project = "auto-v2";
const call = (p, req) => p.evaluate(async body => (await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) })).json(), req);
export default async function ({ page }) {
  let run;
  await step("현재 설정을 보존해 이관하고 안내 전달을 작업 완료와 구별한다", async () => {
    aaa("project", "create", project);
    const dir = `${root}/projects/${project}/automations`;
    const old = 'title="야간 검사 안내"\n[on.schedule]\ndaily="03:00"\n[do.inbox]\ntext="coordinator가 기존 방식으로 검사를 시작한다"\n';
    fs.writeFileSync(`${dir}/night.toml`, old);
    fs.writeFileSync(`${dir}/watch.toml`, 'enabled=false\n[on.ci]\nrepo="repo"\nbranches=["main"]\nevents=["failed"]\n[do.start]\ntitle="감시"\nprofiles=["current-profile"]\nmachines=["local"]\ntext="기존 본문"\nreuse="open"\nmemory=true\n');
    fs.mkdirSync(`${dir}/watch.memory`);
    fs.writeFileSync(`${dir}/watch.memory/processed.txt`, "old-input.json\n");
    const preview = await call(page, { op: "automation_convert", project, dry_run: true });
    assert.ok(preview.ok?.plan_sha256, JSON.stringify(preview));
    assert.equal(fs.readFileSync(`${dir}/night.toml`, "utf8"), old);
    const changed = old.replace('daily="03:00"', 'daily="03:15"');
    fs.writeFileSync(`${dir}/night.toml`, changed);
    const stale = await call(page, { op: "automation_convert", project, plan_sha256: preview.ok.plan_sha256 });
    assert.ok(stale.err, "이관 계획의 오래된 hash를 거부한다");
    const fresh = await call(page, { op: "automation_convert", project, dry_run: true });
    const applied = await call(page, { op: "automation_convert", project, plan_sha256: fresh.ok.plan_sha256 });
    assert.ok(applied.ok, JSON.stringify(applied));
    const night = fs.readFileSync(`${dir}/night.toml`, "utf8");
    assert.match(night, /schema = 2/);
    assert.match(night, /daily = "03:15"/);
    assert.match(night, /kind = "notice"/);
    const watch = fs.readFileSync(`${dir}/watch.toml`, "utf8");
    assert.match(watch, /current-profile/);
    assert.match(watch, /enabled = false/);
    assert.equal(fs.readFileSync(`${dir}/watch.memory/processed.txt`, "utf8"), "old-input.json\n");
    const result = await call(page, { op: "automation_run", project, name: "night", key: "night-once" });
    assert.ok(result.ok, JSON.stringify(result));
    run = await until("안내 동작 영수증", async () => {
      aaa("ticker", "tick");
      const answer = await call(page, { op: "automation_executions", project, name: "night" });
      return answer.ok?.runs.find(r => r.receipt === "notice_created");
    });
    assert.equal(run.state, "succeeded");
    assert.equal(run.outcome, null);
    assert.equal(run.processed, false);
    assert.ok(run.events.some(e => e.stage === "trigger"));
    assert.ok(run.events.some(e => e.stage === "action"));
    const detail = await call(page, { op: "automation_execution", project, id: run.id });
    assert.equal(detail.ok.run.id, run.id);
    await page.getByTestId("activity-automations").click();
    await page.getByTestId(`automation-${project}-night`).click();
    const ledger = page.getByTestId("automation-executions");
    await ledger.waitFor();
    assert.match(await ledger.textContent(), /안내 전달됨/);
    await ledger.locator("summary").first().click();
    assert.match(await ledger.textContent(), /후속 결과미연결/);
    assert.ok(await ledger.evaluate(e => e.scrollWidth <= e.clientWidth + 1), "데스크톱 실행 원장 폭");
    await shot(page, "automation-v2-ledger");
  });
  await step("휴대폰 WebKit에서도 단계와 후속 결과를 따로 읽는다", async () => {
    const browser = await webkit.launch();
    try {
      const dev = await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 }, { touch: true });
      await dev.page.locator(".view [data-testid='drawer-toggle']:visible").click();
      await drawerSettled(dev.page);
      await dev.page.getByTestId("activity-automations").click();
      await dev.page.getByTestId(`automation-${project}-night`).click();
      const ledger = dev.page.getByTestId("automation-executions");
      await ledger.waitFor();
      assert.match(await ledger.textContent(), /안내 전달됨/);
      await ledger.locator("summary").first().click();
      assert.match(await ledger.textContent(), /미확인/);
      assert.ok(await ledger.evaluate(e => e.scrollWidth <= e.clientWidth + 1), "휴대폰 실행 원장 폭");
      await shot(dev.page, "automation-v2-phone");
      assert.deepEqual(dev.errors, []);
    } finally { await browser.close(); }
  });
}

import assert from "node:assert/strict";
import fs from "node:fs";
import { DatabaseSync } from "node:sqlite";
import { root, aaa, inList, domIs, pairPage } from "./harness.mjs";
import { responseGate, responsive, count } from "./bulk-actions.mjs";
import { clickDialogButton } from "./menu.mjs";
import { seedHost } from "./bulk-fixture.mjs";
import { BulkConversionDiagnostics } from "./bulk-conversion-diagnostics.mjs";

export async function automationQueueScenario(engine, label, width) {
  console.log(`bulk queue ${label} ${width}`);
  const dir = `${root}/projects/demo`;
  const event = id => ({ id, at_ms: 1000, owner: "t-9000", body: "{}", action: { message: { thread: "owner", text: "확인" } }, source: "events" });
  await seedHost(db => {
    const put = db.prepare("INSERT OR REPLACE INTO meta(key,value) VALUES(?,?)");
    for (const discard of [false, true]) {
      const name = `queue-${label}-${width}-${discard}`;
      // 꺼진 규칙은 사람이 푼 뒤에도 ticker가 남은 입력을 소비하지 않는다.
      fs.writeFileSync(`${dir}/automations/${name}.toml`, 'schema=2\nenabled=false\n[trigger]\nkind="event"\nsource="thread"\n[condition]\nevents=["thread_failed"]\n[action]\nkind="message"\nthread="owner"\ntext="확인"\n');
      put.run(`automation:demo:${name}`, JSON.stringify({
        pending: Array.from({ length: count }, (_, i) => event(`pending-${i}`)),
        inflight: { id: "seen-batch", events: Array.from({ length: count }, (_, i) => event(`unknown-${i}`)), day: 0, unknown: true },
      }));
    }
  });
  // 실제 동작 뒤 상태 확인에는 읽기 전용 연결만 쓴다.
  const db = new DatabaseSync(`${root}/s/host/host.db`, { readOnly: true });
  try { for (const discard of [false, true]) {
    const name = `queue-${label}-${width}-${discard}`;
    const stateKey = `automation:demo:${name}`;
    const gate = responseGate();
    const paired = await pairPage(engine, aaa("web", "pair").code, { width, height: 900 }, { serviceWorkers: "block", setup: gate.setup });
    const page = paired.page;
    try {
      await page.getByTestId("terminal").waitFor();
      await inList(page, "activity-automations");
      await inList(page, `automation-demo-${name}`);
      const detail = page.getByTestId("automation-detail");
      await detail.getByText("뒤에 쌓인 입력 500건", { exact: true }).waitFor();
      await detail.getByRole("button", { name: discard ? "확인함 · 남은 입력 버리기" : "확인함 · 남은 입력 계속 보내기", exact: true }).click();
      const dialog = page.getByRole("alertdialog");
      const summary = await dialog.locator(".confirm-body").textContent();
      const held = gate.arm("automation_unblock");
      await clickDialogButton(dialog.getByRole("button", { name: discard ? "버리기" : "계속 보내기", exact: true }));
      try {
        await held.arrived;
        // host는 반영했지만 HTTP 응답은 아직 보류 중이다. 포커스 복귀가 실제 목록을 먼저 갱신하게 한다.
        // 목록의 막힘이 사라져도 이 요청의 확인창·진행 표시·처음 고른 묶음은 응답까지 남아야 한다.
        const refreshed = page.waitForResponse(response => response.url().endsWith("/api/v1/call") && response.request().postDataJSON()?.op === "routines");
        await page.evaluate(() => window.dispatchEvent(new Event("focus")));
        await refreshed;
        await detail.getByTestId("automation-stuck").waitFor({ state: "hidden" });
        await responsive(page, dialog);
        assert.equal(await dialog.locator(".confirm-body").textContent(), summary);
      }
      finally { held.release(); }
      await dialog.waitFor({ state: "hidden" });
      await detail.getByTestId("automation-stuck").waitFor({ state: "hidden" });
      assert.equal(gate.calls.length, 1);
      const stored = JSON.parse(db.prepare("SELECT value FROM meta WHERE key=?").get(stateKey).value);
      assert.equal(stored.inflight, null);
      assert.equal(stored.pending.length, discard ? 0 : count);
      assert.deepEqual(paired.errors, []);
    } finally { await page.context().close(); }
  } } finally { db.close(); }
}

export async function automationConvertScenario(engine, label, width) {
  console.log(`bulk convert ${label} ${width}`);
  const hostOffset = fs.statSync(`${root}/host.log`).size;
  const dir = `${root}/projects/demo`;
  const prefix = `legacy-${label}-${width}-`;
  const source = '+++\nschedule="every 1h"\nenabled=false\n+++\n보존할 원문\n';
  for (let i = 0; i < count; i++) fs.writeFileSync(`${dir}/routines/${prefix}${i}.md`, source);
  const diagnostic = new BulkConversionDiagnostics(root, label, width, hostOffset);
  const gate = responseGate(diagnostic.record);
  const paired = await pairPage(engine, aaa("web", "pair").code, { width, height: 900 }, { serviceWorkers: "block", setup: gate.setup });
  const page = paired.page;
  diagnostic.watch(page, engine);
  diagnostic.phase = "preview";
  diagnostic.record("pair.ready");
  try {
    await diagnostic.observeClick();
    await page.getByTestId("terminal").waitFor();
    await inList(page, "activity-automations");
    await page.getByTestId("automation-legacy-demo").getByRole("button", { name: "새 형식으로 바꾸기", exact: true }).click();
    const sheet = page.getByTestId("automation-convert");
    await domIs(page, "변환 계획 규칙 500개", () => document.querySelectorAll('[data-group="rules"] li').length === 500);
    diagnostic.phase = "convert";
    diagnostic.record("preview.ready");
    const held = gate.arm(req => req.op === "automation_convert" && !req.dry_run);
    diagnostic.record("click.start");
    await sheet.getByRole("button", { name: "변환", exact: true }).click();
    diagnostic.record("click.end");
    try {
      await held.arrived;
      await responsive(page);
      await page.keyboard.press("Escape");
      assert.equal(await sheet.count(), 1);
      assert.equal(await sheet.getByRole("button", { name: "닫기", exact: true }).isDisabled(), true);
    } finally { held.release(); }
    await sheet.waitFor({ state: "hidden" });
    await inList(page, page.getByTestId("convert-archived"));
    await page.getByTestId("automation-legacy-demo").waitFor({ state: "hidden" });
    assert.equal(gate.calls.length, 1);
    for (let i = 0; i < count; i++) {
      assert.equal(fs.existsSync(`${dir}/routines/${prefix}${i}.md`), false);
      assert.equal(fs.readFileSync(`${dir}/routines/.converted/${prefix}${i}.md`, "utf8"), source);
      assert.equal(fs.existsSync(`${dir}/automations/${prefix}${i}.toml`), true);
    }
    assert.deepEqual(paired.errors, []);
  } catch (e) { await diagnostic.capture(e); throw e; }
  finally {
    // 성공한 실험실도 정리 전에 제품의 단계별 비용을 전체 gate 로그에 보존한다.
    for (const line of fs.readFileSync(`${root}/host.log`).subarray(hostOffset).toString().split("\n")) {
      if (line.includes("자동화 변환 단계 비용")) console.log(`[bulk-conversion ${label} ${width}] ${line}`);
    }
    await diagnostic.close(() => page.context().close());
  }
}

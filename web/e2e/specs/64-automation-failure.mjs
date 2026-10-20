// 실제 host의 실행 실패와 파일 파싱 실패를 CLI·웹에서 구분한다. 운영 파일·DB는 쓰지 않는다.
import fs from "node:fs";
import { root, aaa, step, until, domIs, drawerSettled, shot, webkit, pairPage } from "../lib/harness.mjs";

const project = "auto-failure";
const call = (p, req) => p.evaluate(async (body) => (await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) })).json(), req);

export default async function ({ page }) {
  await step("정상 파일의 실행 실패와 깨진 파일은 CLI·웹 목록·상세에서 다르게 보인다", async () => {
    aaa("project", "create", project);
    const dir = `${root}/projects/${project}/automations`;
    fs.mkdirSync(dir, { recursive: true });
    // 존재하지 않는 대상의 실행 실패: 파일 자체는 올바른 TOML이다.
    fs.writeFileSync(`${dir}/failed.toml`, 'title="실행 실패 재현"\n[on.thread]\nevents=["report_ready"]\n[do.message]\nthread="t-9999"\ntext="확인 자료"\n');
    fs.writeFileSync(`${dir}/broken.toml`, 'title = [\n');
    const accepted = await call(page, { op: "automation_run", project, name: "failed", key: "failure-repro" });
    if (accepted.err) throw new Error(JSON.stringify(accepted));
    const failed = await until("실제 host 실행 실패", () => {
      aaa("ticker", "tick");
      return aaa("routine", "list", project).routines.find((r) => r.name === "failed" && r.last_result?.status === "failed");
    });
    if (failed.error !== null || !failed.last_result.detail.includes("t-9999")) throw new Error(`CLI 실행 실패 ${JSON.stringify(failed)}`);
    const broken = aaa("routine", "list", project).routines.find((r) => r.name === "broken");
    if (!broken.error || broken.enabled) throw new Error(`CLI 파일 오류 ${JSON.stringify(broken)}`);
    const api = await call(page, { op: "routines", project });
    const viewed = api.ok?.routines.find((r) => r.name === "failed");
    if (viewed?.error !== failed.error || viewed?.last_result.status !== failed.last_result.status || viewed?.last_result.detail !== failed.last_result.detail) throw new Error(`CLI와 웹 응답 차이 ${JSON.stringify(api)}`);

    await page.getByTestId("activity-automations").click();
    const row = page.getByTestId(`automation-${project}-failed`);
    await row.locator('[aria-label="실행 실패"]').waitFor();
    await domIs(page, "실행 실패 상태", () => document.querySelector('[data-testid="automation-auto-failure-failed"]')?.dataset.status === "failed");
    if ((await row.textContent()).includes("파일 오류") || !(await row.textContent()).includes(failed.last_result.detail)) throw new Error("실행 실패 줄의 이유 또는 분류");
    await row.getByRole("button", { name: "실행 실패 재현 자세히", exact: true }).click();
    const sheet = page.getByTestId("automation-detail");
    await sheet.locator(".automation-badge.is-failed").first().waitFor();
    if (await sheet.locator(".automation-badge.is-failed").first().textContent() !== "실행 실패" || !(await sheet.getByLabel("요약").textContent()).includes(failed.last_result.detail)) throw new Error("상세 실행 실패");
    await page.keyboard.press("Escape");
    await sheet.waitFor({ state: "detached" });
    const brokenRow = page.getByTestId(`automation-${project}-broken`);
    await brokenRow.locator('[aria-label="파일 오류"]').waitFor();
    await brokenRow.getByRole("button", { name: "broken 자세히", exact: true }).click();
    await sheet.locator(".automation-badge.is-file-error").waitFor();
    if (await sheet.locator(".automation-badge.is-file-error").textContent() !== "파일 오류" || !(await sheet.getByLabel("요약").textContent()).includes(broken.error)) throw new Error("상세 파일 오류");
    await page.keyboard.press("Escape");
    await sheet.waitFor({ state: "detached" });
  });

  await step("휴대폰 WebKit 상세에서도 실행 실패·원인과 파일 오류를 구분한다", async () => {
    const phone = await webkit.launch();
    try {
      const dev = await pairPage(phone, aaa("web", "pair").code, { width: 390, height: 844 }, { touch: true });
      const p = dev.page;
      await p.locator(".view [data-testid='drawer-toggle']").click();
      await drawerSettled(p);
      await p.getByTestId("activity-automations").click();
      const row = p.getByTestId(`automation-${project}-failed`);
      await row.getByRole("button", { name: "실행 실패 재현 자세히", exact: true }).click();
      const sheet = p.getByTestId("automation-detail");
      await sheet.getByTestId("automation-source").locator("pre").first().waitFor();
      if (await sheet.locator(".automation-badge.is-failed").first().textContent() !== "실행 실패") throw new Error("휴대폰 실행 실패 칩");
      await shot(p, "automation-execution-failure-phone");
      await sheet.getByRole("button", { name: "닫기", exact: true }).first().click();
      await sheet.waitFor({ state: "detached" });
      await p.getByTestId(`automation-${project}-broken`).getByRole("button", { name: "broken 자세히", exact: true }).click();
      await sheet.locator(".automation-badge.is-file-error").waitFor();
      await shot(p, "automation-file-error-phone");
      if (dev.errors.length) throw new Error(dev.errors.join("\n"));
    } finally { await phone.close(); }
  });
}

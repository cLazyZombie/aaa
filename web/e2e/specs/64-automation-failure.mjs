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
    // 카드는 둘 다 「오류」 표지다. 실행 실패와 파일 오류의 구분·까닭은 본문 설정의 색 상자에 있다(t-0628)
    const row = page.getByTestId(`automation-${project}-failed`);
    await row.locator('[aria-label="오류"]').waitFor();
    await domIs(page, "실행 실패 상태", () => document.querySelector('[data-testid="automation-auto-failure-failed"]')?.dataset.status === "error");
    await row.click();
    const sheet = page.getByTestId("automation-detail");
    const failedBox = sheet.getByTestId("automation-failed");
    await failedBox.waitFor();
    if (!(await failedBox.textContent()).includes(failed.last_result.detail) || await sheet.getByTestId("automation-file-error").count()) throw new Error("상세 실행 실패");
    if (!(await sheet.getByLabel("요약").textContent()).includes(failed.last_result.detail)) throw new Error("요약 지난 실행");
    const brokenRow = page.getByTestId(`automation-${project}-broken`);
    await brokenRow.locator('[aria-label="오류"]').waitFor();
    await brokenRow.click();
    const fileError = sheet.getByTestId("automation-file-error");
    await fileError.waitFor();
    if (!(await fileError.textContent()).includes(broken.error) || await sheet.getByTestId("automation-failed").count()) throw new Error("상세 파일 오류");
    await sheet.getByTestId("automation-close").click();
    await sheet.waitFor({ state: "hidden" });
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
      await row.click();
      const sheet = p.getByTestId("automation-detail");
      await sheet.getByTestId("automation-source").locator("pre").first().waitFor();
      if (!(await sheet.getByTestId("automation-failed").textContent()).startsWith("마지막 실행 실패")) throw new Error("휴대폰 실행 실패 상자");
      await shot(p, "automation-execution-failure-phone");
      await sheet.getByTestId("automation-close").click();
      await sheet.waitFor({ state: "hidden" });
      // 닫으면 앞 화면이다: 목록은 서랍을 다시 열어 본다
      await p.locator(".view [data-testid='drawer-toggle']:visible").click();
      await drawerSettled(p);
      await p.getByTestId(`automation-${project}-broken`).click();
      await sheet.getByTestId("automation-file-error").waitFor();
      await shot(p, "automation-file-error-phone");
      if (dev.errors.length) throw new Error(dev.errors.join("\n"));
    } finally { await phone.close(); }
  });
}

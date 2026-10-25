import { aaa, step, browser, webkit } from "../lib/harness.mjs";
import { askScenario, reportScenario, maintenanceScenario } from "../lib/bulk-actions.mjs";

export default async function () {
  aaa("project", "create", "demo");
  const safari = await webkit.launch({ headless: true });
  try {
  await step("대량 답 삭제: Chromium 데스크톱·휴대폰의 500개 원문·그림·실패 복구", async () => {
    for (const width of [1280, 390]) await askScenario(browser, "chromium", width);
  });
  await step("대량 답 삭제: WebKit 데스크톱·휴대폰의 500개 원문·그림·실패 복구", async () => {
    for (const width of [1280, 390]) await askScenario(safari, "webkit", width);
  });
  await step("대량 보고서: Chromium 601개 보관·세 탭 모두 읽음·삭제", async () => {
    for (const width of [1280, 390]) await reportScenario(browser, "chromium", width);
  });
  await step("대량 보고서: WebKit 601개 보관·세 탭 모두 읽음·삭제", async () => {
    for (const width of [1280, 390]) await reportScenario(safari, "webkit", width);
  });
  await step("대량 사본·공유 링크: Chromium 500개 정리·무효화", async () => {
    for (const width of [1280, 390]) await maintenanceScenario(browser, "chromium", width);
  });
  await step("대량 사본·공유 링크: WebKit 500개 정리·무효화", async () => {
    for (const width of [1280, 390]) await maintenanceScenario(safari, "webkit", width);
  });
  } finally { await safari.close(); }
}

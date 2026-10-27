import { aaa, step, browser, webkit } from "../lib/harness.mjs";
import { askScenario, reportScenario, maintenanceScenario } from "../lib/bulk-actions.mjs";

export default async function () {
  aaa("project", "create", "demo");
  const safari = await webkit.launch({ headless: true });
  try {
  await step("대량 답 삭제: chromium 1280", () => askScenario(browser, "chromium", 1280));
  await step("대량 답 삭제: chromium 390", () => askScenario(browser, "chromium", 390));
  await step("대량 답 삭제: webkit 1280", () => askScenario(safari, "webkit", 1280));
  await step("대량 답 삭제: webkit 390", () => askScenario(safari, "webkit", 390));
  await step("대량 보고서: chromium 1280", () => reportScenario(browser, "chromium", 1280));
  await step("대량 보고서: chromium 390", () => reportScenario(browser, "chromium", 390));
  await step("대량 보고서: webkit 1280", () => reportScenario(safari, "webkit", 1280));
  await step("대량 보고서: webkit 390", () => reportScenario(safari, "webkit", 390));
  await step("대량 사본·공유 링크: chromium 1280", () => maintenanceScenario(browser, "chromium", 1280));
  await step("대량 사본·공유 링크: chromium 390", () => maintenanceScenario(browser, "chromium", 390));
  await step("대량 사본·공유 링크: webkit 1280", () => maintenanceScenario(safari, "webkit", 1280));
  await step("대량 사본·공유 링크: webkit 390", () => maintenanceScenario(safari, "webkit", 390));
  } finally { await safari.close(); }
}

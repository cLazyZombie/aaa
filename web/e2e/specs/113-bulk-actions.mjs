import { aaa, step, browser, webkit } from "../lib/harness.mjs";
import { askScenario, reportScenario, maintenanceScenario } from "../lib/bulk-actions.mjs";

export default async function () {
  aaa("project", "create", "demo");
  const safari = await webkit.launch({ headless: true });
  try {
  await step("대량 답 삭제: webkit 390", () => askScenario(safari, "webkit", 390));
  await step("대량 보고서: chromium 1280", () => reportScenario(browser, "chromium", 1280));
  await step("대량 사본·공유 링크: webkit 390", () => maintenanceScenario(safari, "webkit", 390));
  } finally { await safari.close(); }
}

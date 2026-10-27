import { aaa, step, browser, webkit } from "../lib/harness.mjs";
import { automationQueueScenario, automationConvertScenario } from "../lib/bulk-automation.mjs";
import { inboxScenario, uploadScenario, uploadFailureScenario, sweepScenario } from "../lib/bulk-extra.mjs";

export default async function () {
  aaa("project", "create", "demo");
  const safari = await webkit.launch({ headless: true });
  try {
  await step("대량 inbox: chromium 1280", () => inboxScenario(browser, "chromium", 1280));
  await step("대량 inbox: chromium 390", () => inboxScenario(browser, "chromium", 390));
  await step("대량 inbox: webkit 1280", () => inboxScenario(safari, "webkit", 1280));
  await step("대량 inbox: webkit 390", () => inboxScenario(safari, "webkit", 390));
  await step("대량 올리기: chromium 1280", () => uploadScenario(browser, "chromium", 1280));
  await step("대량 올리기: chromium 390", () => uploadScenario(browser, "chromium", 390));
  await step("대량 올리기: webkit 1280", () => uploadScenario(safari, "webkit", 1280));
  await step("대량 올리기: webkit 390", () => uploadScenario(safari, "webkit", 390));
  await step("올리기 부분 실패: chromium 1280", () => uploadFailureScenario(browser, "chromium", 1280));
  await step("올리기 부분 실패: webkit 390", () => uploadFailureScenario(safari, "webkit", 390));
  await step("대량 자리 정리: chromium 1280", () => sweepScenario(browser, "chromium", 1280));
  await step("대량 자리 정리: chromium 390", () => sweepScenario(browser, "chromium", 390));
  await step("대량 자리 정리: webkit 1280", () => sweepScenario(safari, "webkit", 1280));
  await step("대량 자리 정리: webkit 390", () => sweepScenario(safari, "webkit", 390));
  await step("대량 자동화 입력: chromium 1280", () => automationQueueScenario(browser, "chromium", 1280));
  await step("대량 자동화 입력: chromium 390", () => automationQueueScenario(browser, "chromium", 390));
  await step("대량 자동화 입력: webkit 1280", () => automationQueueScenario(safari, "webkit", 1280));
  await step("대량 자동화 입력: webkit 390", () => automationQueueScenario(safari, "webkit", 390));
  await step("대량 자동화 변환: chromium 1280", () => automationConvertScenario(browser, "chromium", 1280));
  await step("대량 자동화 변환: chromium 390", () => automationConvertScenario(browser, "chromium", 390));
  await step("대량 자동화 변환: webkit 1280", () => automationConvertScenario(safari, "webkit", 1280));
  await step("대량 자동화 변환: webkit 390", () => automationConvertScenario(safari, "webkit", 390));
  } finally { await safari.close(); }
}

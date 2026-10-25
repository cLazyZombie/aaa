import { aaa, step, browser, webkit } from "../lib/harness.mjs";
import { automationQueueScenario, automationConvertScenario } from "../lib/bulk-automation.mjs";
import { inboxScenario, uploadScenario, uploadFailureScenario, sweepScenario } from "../lib/bulk-extra.mjs";

export default async function () {
  aaa("project", "create", "demo");
  const safari = await webkit.launch({ headless: true });
  try {
  await step("대량 inbox: 숨겨진 컴포넌트도 Chromium·WebKit에서 500개 완료·새 항목 보존", async () => {
    for (const [engine, label] of [[browser, "chromium"], [safari, "webkit"]]) {
      for (const width of [1280, 390]) await inboxScenario(engine, label, width);
    }
  });
  await step("대량 올리기: Chromium·WebKit 500개 파일 진행·완료·목록", async () => {
    for (const [engine, label] of [[browser, "chromium"], [safari, "webkit"]]) {
      for (const width of [1280, 390]) await uploadScenario(engine, label, width);
    }
  });
  await step("보고서 올리기 부분 실패: 진행 중 요청 완료·성공 경로 보존·명시적 재개", async () => {
    await uploadFailureScenario(browser, "chromium", 1280);
    await uploadFailureScenario(safari, "webkit", 390);
  });
  await step("대량 자리 정리: Chromium·WebKit 500개 잔여 기록과 실제 worktree 삭제", async () => {
    for (const [engine, label] of [[browser, "chromium"], [safari, "webkit"]]) {
      for (const width of [1280, 390]) await sweepScenario(engine, label, width);
    }
  });
  await step("대량 자동화 입력: Chromium·WebKit 500건 버리기·계속 보내기와 목록 갱신", async () => {
    for (const [engine, label] of [[browser, "chromium"], [safari, "webkit"]]) {
      for (const width of [1280, 390]) await automationQueueScenario(engine, label, width);
    }
  });
  await step("대량 자동화 변환: Chromium·WebKit 500개 원문 보존·창 닫힘·목록 갱신", async () => {
    for (const [engine, label] of [[browser, "chromium"], [safari, "webkit"]]) {
      for (const width of [1280, 390]) await automationConvertScenario(engine, label, width);
    }
  });
  } finally { await safari.close(); }
}

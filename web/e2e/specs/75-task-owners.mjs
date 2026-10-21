// 실제 agent의 끊김 동안 할 일을 처음 연 뒤 복귀 사건으로 owner 목록이 갱신되는지 검사한다.
// 실험실 프로세스와 실제 assignable 응답만 쓰며, DOM·연결·HTTP 사건으로 끝낸다.
import fs from "node:fs";
import { aaa, start, stop, until, domIs, step, root, browser, webkit, pairPage } from "../lib/harness.mjs";

const machineIs = (p, connected) => domIs(p, `머신 ${connected ? "연결" : "끊김"}`, (up) => {
  const o = document.querySelector('[data-testid="task-field-machine"] option[value="labA"]');
  return !!o && o.disabled === !up && o.textContent === (up ? "labA" : "labA(끊김)");
}, connected);
const refreshes = async (page, action) => {
  const requested = page.waitForRequest((r) => r.url().endsWith("/api/v1/call") && r.postDataJSON()?.op === "assignable");
  await action();
  const response = await (await requested).response();
  if (!response?.ok()) throw new Error("owner 재조회 실패");
  await response.finished();
};

export default async function () {
  let wk;
  const pages = [];
  try {
  await step("할 일 머신: 끊긴 동안 처음 읽은 목록이 agent 복귀·끊김을 따라가고 편집 중인 값은 남는다", async () => {
    aaa("project", "create", "demo");
    aaa("project", "set", "demo", "thread_profile", "fake");
    fs.writeFileSync(`${root}/projects/demo/TASKS.md`, "# Tasks\n\n## Backlog\n- [ ] 연결 확인\n");
    aaa("task", "list");
    await stop("agent");
    await until("agent 끊김", () => aaa("machine", "list").machines.some((m) => m.name === "labA" && m.connection !== "connected"));
    wk = await webkit.launch({ headless: true });
    for (const [engine, viewport] of [[browser, { width: 1280, height: 800 }], [wk, { width: 390, height: 844 }]]) {
      const ctl = await pairPage(engine, aaa("web", "pair").code, viewport, { hash: "#tasks", serviceWorkers: "block" });
      pages.push(ctl);
      await ctl.page.locator('.task-row[data-title="연결 확인"] .task-pick').click();
      await machineIs(ctl.page, false);
      await ctl.page.getByTestId("task-field-title").fill("편집 중인 제목");
    }
    start("agent2", "agent", "--host", "local", "--name", "labA");
    for (const { page } of pages) {
      await machineIs(page, true);
      if (await page.getByTestId("task-field-title").inputValue() !== "편집 중인 제목") throw new Error("연결 갱신이 고치던 제목을 지웠다");
    }
    await stop("agent2");
    for (const { page } of pages) await machineIs(page, false);
    start("agent3", "agent", "--host", "local", "--name", "labA");
    for (const { page } of pages) await machineIs(page, true);
  });

  await step("할 일 머신: 상세·고르기·바로 시작을 열 때 실제 owner 목록을 다시 읽는다", async () => {
    for (const { page } of pages) {
      // 같은 상세를 닫았다 열어도 캐시만 쓰지 않는다.
      await page.getByTestId("task-field-title").fill("연결 확인");
      await page.getByTestId("task-detail-close").click();
      await refreshes(page, () => page.locator('.task-row[data-title="연결 확인"] .task-pick').click());
      await refreshes(page, () => page.getByTestId("task-field-machine").focus());
      await refreshes(page, () => page.getByTestId("task-handoff-start").click());
      await domIs(page, "시작 머신 목록", () => [...document.querySelectorAll('[data-testid="task-start-machine"] option')].some((o) => o.value === "labA"));
      await page.getByTestId("task-start").getByRole("button", { name: "취소", exact: true }).click();
    }
  });
    for (const { errors } of pages) {
      if (errors.length) throw new Error(`페이지 오류: ${errors.join("; ")}`);
    }
  } finally {
    for (const { page } of pages) await page.context().close();
    await wk?.close();
  }
}

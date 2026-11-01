// 실제 보고 수집·답 전달 뒤 작업창 표지가 갱신되는 계약과 작은 표지의 배치를 본다.
import assert from "node:assert/strict";
import fs from "node:fs";
import { createHash } from "node:crypto";
import { root, AA, aaa, until, step, browser, webkit, pairPage, inList, domIs, shot, waitOutput, drawerSettled } from "../lib/harness.mjs";

const config = `${root}/c/host.toml`;
fs.writeFileSync(config, fs.readFileSync(config, "utf8")
  .replace('allow_profiles = ["fake"]', 'allow_profiles = ["fake", "fake-claude", "fake-codex"]')
  + ['claude', 'codex'].map(agent => `\n[profiles.fake-${agent}]\nagent = "custom"\ncommand = ["/bin/sh", "${root}/fake.sh"]\n`).join(''));

async function scenario(engine, width, agent, rows) {
  const label = `${engine.browserType().name()}-${width}-${agent}`;
  if (!aaa("project", "list").projects.some(p => p.name === "demo")) aaa("project", "create", "demo");
  fs.writeFileSync(`${root}/mark-task.md`, "질문 표지 확인\n");
  const t = aaa("thread", "start", "demo", "--title", "질문 표지", "--machine", "labA", "--profile", `fake-${agent}`, "--kind", "dir", "--task-file", `${root}/mark-task.md`);
  await until("첫 보고", () => aaa("thread", "show", t.id).report_hash);
  // 작업 중 판정은 보고 첫 줄이 아니라 실제 세션의 미완료 진행 보고로 정한다.
  fs.writeFileSync(`${root}/mark-progress.sh`, `"${AA}" report --percent 50 --activity '질문 표지 시험'\nprintf 'mark-progress-done\\n'\n`);
  aaa("pane", "run", t.pane, `post-probe ${root}/mark-progress.sh`);
  await waitOutput(t.pane, "mark-progress-done");
  await until("작업 중 진행 보고", () => aaa("thread", "show", t.id).agent?.percent === 50);
  const write = async body => {
    fs.writeFileSync(`${t.work_dir}/report.md`, body);
    const hash = createHash("sha256").update(body).digest("hex");
    return until("새 보고 수집", () => {
      const v = aaa("thread", "show", t.id);
      return v.report_hash === hash && v;
    });
  };
  await write("[진행 중] 질문 없이 작업 중\n");
  const { page, errors } = await pairPage(engine, aaa("web", "pair").code, { width, height: 844 });
  try {
    const result = await page.evaluate(async sidebarRows => (await fetch("/api/v1/call", {
      method: "POST", headers: { "content-type": "application/json" },
      body: JSON.stringify({ op: "preferences_set", settings: { sidebarRows }, pins: null }),
    })).json(), rows);
    assert.equal(result.err, undefined);
    const row = page.getByTestId(`thread-demo-${t.id}`);
    await inList(page, row);
    // 휴대폰에서 줄 선택은 서랍을 닫는다. 표시를 검사할 때는 목록을 다시 연다.
    if (width <= 768) {
      await page.locator('.view [data-testid="drawer-toggle"]:visible').click();
      await page.locator("#workspace-drawer.is-open").waitFor();
      await drawerSettled(page, true);
    }
    await domIs(page, "질문 없는 줄 반영", id => {
      const row = document.querySelector(`[data-testid="thread-demo-${id}"]`);
      return row?.textContent.includes("작업 중") && !row.querySelector(".thread-question");
    }, t.id);
    const source = `[결정 필요] 질문 2개: ${label}\n\n## Questions\n### 1. 진행\n- A 진행\n- B 보류\n### 2. 색\n- A 회색\n- B 파랑\n`;
    const v = await write(source);
    const badge = row.locator(".sidebar-mark .thread-question");
    await badge.waitFor({ state: "visible" });
    assert.equal(await badge.textContent(), "?");
    assert.equal(await row.locator(`.sidebar-mark[data-agent="${agent}"]`).count(), 1);
    assert.match(await row.ariaSnapshot(), /질문 대기/);
    const placement = await badge.evaluate(el => {
      const badge = el.getBoundingClientRect(), mark = el.parentElement.getBoundingClientRect();
      return { width: badge.width, height: badge.height, right: mark.right - badge.right, bottom: mark.bottom - badge.bottom,
        visible: getComputedStyle(el).color !== getComputedStyle(el).backgroundColor };
    });
    assert.deepEqual(placement, { width: 12, height: 12, right: 1, bottom: -3, visible: true });
    await shot(page, `question-mark-${label}`);
    // 일부만 답했을 때 남은 질문이 있으면 표지를 유지한다.
    aaa("question", "answer", v.questions.items[0].id, "A");
    await until("첫 질문 답 저장", () => aaa("thread", "show", t.id).questions?.draft?.picks[0] === 0);
    await domIs(page, "일부 답 뒤 질문 표지 유지", id => document.querySelector(`[data-testid="thread-demo-${id}"] .thread-question`) !== null, t.id);
    aaa("question", "answer", v.questions.items[1].id, "B");
    await until("모든 질문 답 저장", () => aaa("thread", "show", t.id).questions?.answer);
    await badge.waitFor({ state: "detached" });
    // 같은 질문을 다시 보고해도 이미 답했다면 표지가 되살아나지 않는다.
    await write(source + "\n## Report\n답을 반영한다.\n");
    aaa("thread", "ack", t.id);
    await domIs(page, "답한 질문 재보고 뒤 표지 없음", id => {
      const row = document.querySelector(`[data-testid="thread-demo-${id}"]`);
      return row?.textContent.includes("작업 중") && !row.querySelector(".thread-question");
    }, t.id);
    assert.deepEqual(errors, []);
  } finally {
    await page.context().close();
    aaa("thread", "resolve", t.id);
  }
}

export default async function () {
  await step("질문 표지: Chromium 데스크톱 Codex 두 줄", () => scenario(browser, 1280, "codex", "two"));
  await step("질문 표지: Chromium 휴대폰 Claude 한 줄", () => scenario(browser, 390, "claude", "one"));
  const wk = await webkit.launch();
  try {
    await step("질문 표지: WebKit 데스크톱 Claude 두 줄", () => scenario(wk, 1280, "claude", "two"));
    await step("질문 표지: WebKit 휴대폰 Codex 한 줄", () => scenario(wk, 390, "codex", "one"));
  } finally { await wk.close(); }
}

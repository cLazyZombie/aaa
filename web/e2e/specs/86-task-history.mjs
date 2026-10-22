// 실제 격리 DB의 4MiB 초과 기록. 검색·저장소·상태는 host가 처리하고 UI는 작은 페이지만 더한다.
import assert from "node:assert/strict";
import fs from "node:fs";
import { DatabaseSync } from "node:sqlite";
import { root, aaa, step, shot, domIs, browser, webkit, pairPage, stop, start, until } from "../lib/harness.mjs";

const request = (page, value) => page.evaluate(async (value) => (await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(value) })).json(), value);
const archive = (page, view) => request(page, { op: "task_archive", restore: null, view });
const rows = (page) => page.locator('[data-testid^="archive-"]');

export default async function ({ page }) {
  let restoredId;
  await step("기록 규모: 4MiB 초과 보관도 CLI 전체 출력과 host 페이지의 크기·검색 조합을 지킨다", async () => {
    aaa("project", "create", "demo");
    fs.writeFileSync(`${root}/projects/demo/TASKS.md`, "## Backlog\n- [ ] 활성 할 일 (me) [작업 폴더]\n  메모\n");
    aaa("task", "list");
    // fixture를 외부 SQLite writer로 넣는 동안 실제 host writer와 경합하지 않는다.
    await stop("host");
    const db = new DatabaseSync(`${root}/s/host/host.db`);
    const put = db.prepare("INSERT INTO task_archive(project,task_id,data) VALUES('demo',?,?)");
    let storedBytes = 0;
    db.exec("BEGIN");
    for (let i = 1; i <= 130; i++) {
      const id = String(i).padStart(26, "0"), repo = i === 130 ? null : i % 2 ? "engine" : "rc";
      const title = `보관 ${String(i).padStart(3, "0")}`;
      const body = `- [x] ${title} (me)${repo ? ` [${repo}]` : ""} <!-- aaa-task:${id} -->\n  needle-${i} 결과 메모\n`;
      const history = [{ id: i, at_ms: i, actor: "file", by: "파일 직접", action: "raw", list: "Backlog", from_list: null, title, before: "자료".repeat(6000), after: null, thread: null, restored_by: null }];
      const row = { task_id: id, task: { id, list: "Backlog", title, owner: "me", thread: null, notes: [`needle-${i} 결과 메모`], done: true, repo }, body, reason: ["PR 병합", "origin/main 병합", "보고 [완료]"][(i - 1) % 3], report: null, completed_ms: i, restored: i > 128, committed: true, removed_sha256: "", history };
      const data = JSON.stringify(row); storedBytes += Buffer.byteLength(data); put.run(id, data);
    }
    db.exec("COMMIT"); db.close();
    start("host", "host");
    await until("기록 fixture 뒤 host 준비", () => aaa("task", "list").tasks.length === 1);
    await domIs(page, "캡처 전 host 웹 재연결", () => ![...document.querySelectorAll(".banner.warn")].some((e) => e.textContent.includes("host와 연결이 끊겼다")));
    assert.ok(storedBytes > 4 * 1024 ** 2);
    const cli = aaa("task", "archive");
    assert.equal(cli.entries.length, 128, "CLI는 기본 페이지 뒤까지 합쳐 기존 전체 출력을 유지한다");
    assert.ok(cli.entries.every((e) => e.history.length === 0));
    const seen = [], pageBytes = [];
    let after = null;
    do {
      const r = await archive(page, { search: null, filter: null, after, limit: 50 });
      assert.ok(r.ok, JSON.stringify(r.err));
      pageBytes.push(Buffer.byteLength(JSON.stringify(r)));
      assert.ok(pageBytes.at(-1) < 1024 ** 2);
      seen.push(...r.ok.entries.map((e) => e.task_id)); after = r.ok.next;
    } while (after);
    assert.equal(seen.length, 128); assert.equal(new Set(seen).size, 128);
    const filtered = await archive(page, { search: { query: "  NEEDLE-100  ", repo: "rc" }, filter: "pr_merged", after: null, limit: 50 });
    assert.deepEqual(filtered.ok.entries.map((e) => e.task.title), ["보관 100"]);
    const none = await archive(page, { search: { query: "needle-100", repo: "engine" }, filter: "pr_merged", after: null, limit: 50 });
    assert.equal(none.ok.entries.length, 0);
    console.log(`기록 규모 ${JSON.stringify({ storedBytes, pageBytes, cli: cli.entries.length })}`);
  });

  await step("기록 화면: 활성 UI와 같은 검색·저장소·완료 상태 조합과 다음 페이지·복원을 유지한다", async () => {
    aaa("ticker", "tick");
    await page.getByTestId("activity-tasks").click();
    await page.getByTestId("tasks-search").waitFor();
    await shot(page, "history-desktop-active");
    await page.getByTestId("task-archive-open").click();
    await domIs(page, "보관 첫 50개", () => document.querySelectorAll('[data-testid^="archive-"]').length === 50);
    await shot(page, "history-desktop-records");
    await page.getByTestId("task-archive-more").click();
    await domIs(page, "보관 다음 50개", () => document.querySelectorAll('[data-testid^="archive-"]').length === 100);
    await page.getByTestId("task-log-search").fill("needle-100");
    await page.getByTestId("task-log-filter-pr_merged").click();
    await page.getByTestId("task-log-repo").click();
    await page.getByRole("menuitemcheckbox").filter({ has: page.locator(".menu-item-main").filter({ hasText: /^rc$/ }) }).click();
    await domIs(page, "세 조건 한 건", () => document.querySelectorAll('[data-testid^="archive-"]').length === 1 && document.querySelector('[data-testid^="archive-"]')?.textContent.includes("보관 100"));
    await shot(page, "history-desktop-filtered");
    const r = await archive(page, { search: { query: "needle-100", repo: "rc" }, filter: null, after: null, limit: 50 });
    restoredId = r.ok.entries[0].task_id;
    await rows(page).getByRole("button", { name: "활성 목록으로 복원", exact: true }).click();
    await domIs(page, "복원 뒤 현재 조건 빈 목록", () => document.querySelector('[data-testid="task-archive-list"]')?.textContent.includes("조건에 맞는 완료 보관이 없다"));
    const restored = aaa("task", "list").tasks.find((t) => t.id === restoredId);
    assert.ok(restored && !restored.done && restored.thread === null);
    assert.equal(aaa("task", "queue").entries.length, 0);
    await page.getByTestId("task-log-filter-archive_restored").click();
    await rows(page).filter({ hasText: "보관 100" }).waitFor();
    assert.equal(await rows(page).getByRole("button", { name: "활성 목록으로 복원" }).count(), 0);
    // 실제 삭제 기록에도 같은 세 조건이 걸리고 이전 페이지 커서는 남지 않는다.
    aaa("task", "rm", restored.short_id);
    await page.getByTestId("task-log-filter-deleted").click();
    await domIs(page, "삭제 기록 검색", () => document.querySelectorAll('.task-log-row[data-action="delete"]').length === 1);
    await page.getByTestId("task-log").getByRole("button", { name: "되살리기", exact: true }).click();
    await page.getByTestId("task-log-filter-restored").click();
    await domIs(page, "복원 기록 검색", () => document.querySelectorAll('.task-log-row[data-action="restore"]').length === 1);
  });

  await step("휴대폰과 Reader: 320·390px의 같은 메뉴·상태·검색과 읽기 권한을 지킨다", async () => {
    const safari = await webkit.launch({ headless: true });
    try {
      for (const [engine, width] of [[browser, 320], [safari, 390]]) {
        const paired = await pairPage(engine, aaa("web", "pair", "--reader").code, { width, height: 844 }, { hash: "#tasks", touch: true });
        try {
          const p = paired.page;
          await p.getByTestId("tasks-search").waitFor();
          await shot(p, `history-${width}-active`);
          await p.getByTestId("task-archive-open").click();
          await rows(p).first().waitFor();
          await shot(p, `history-${width}-records`);
          await p.getByTestId("task-log-search").fill("needle-100");
          await p.getByTestId("task-log-filter-archive_restored").click();
          await rows(p).filter({ hasText: "보관 100" }).waitFor();
          await p.getByTestId("task-log-repo").click();
          await p.getByRole("dialog", { name: "저장소로 거르기" }).getByRole("button").filter({ has: p.locator(".row-sheet-label").filter({ hasText: /^rc$/ }) }).click();
          await p.getByRole("dialog", { name: "저장소로 거르기" }).waitFor({ state: "detached" });
          // 공통 sheet의 퇴장 화면은 닫힌 shadow root에 있다. 바깥 host의 제거 사건 뒤 촬영한다.
          await domIs(p, "저장소 시트 퇴장 완료", () => !document.querySelector('body > [inert][aria-hidden="true"]'));
          await rows(p).filter({ hasText: "보관 100" }).waitFor();
          assert.ok(await p.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1));
          assert.equal(await p.getByRole("button", { name: "활성 목록으로 복원" }).count(), 0);
          const denied = await request(p, { op: "task_archive", restore: restoredId });
          assert.equal(denied.err?.code, "forbidden");
          await shot(p, `history-${width}-filtered`);
        } finally { await paired.page.context().close(); }
      }
    } finally { await safari.close(); }
  });
}

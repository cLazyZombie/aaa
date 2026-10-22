// 실제 격리 DB의 4MiB 초과 기록. 검색·저장소·상태는 host가 처리하고 UI는 작은 페이지만 더한다.
import assert from "node:assert/strict";
import fs from "node:fs";
import { DatabaseSync } from "node:sqlite";
import { root, aaa, step, shot, domIs, browser, webkit, pairPage, stop, start, until } from "../lib/harness.mjs";

const request = (page, value) => page.evaluate(async (value) => (await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(value) })).json(), value);
const archive = (page, view) => request(page, { op: "task_archive", restore: null, view });
const rows = (page) => page.locator('[data-testid^="archive-"]');


async function choose(page, field, text) {
  await page.getByTestId(`task-completed-${field}`).click();
  const desktop = page.getByRole("menuitemcheckbox").filter({ has: page.locator(".menu-item-main").filter({ hasText: new RegExp(`^${text}$`) }) });
  if (await desktop.count()) await desktop.click();
  else await page.getByRole("dialog").getByRole("button").filter({ has: page.locator(".row-sheet-label").filter({ hasText: new RegExp(`^${text}$`) }) }).click();
}

// 탭과 검색이 같은 flex 부모를 쓰고, 탭 전환에도 검색의 시작 위치·높이가 같다.
async function toolbarLayout(page, searchId, expected = null) {
  await domIs(page, "검색줄 배치 완료", ({ searchId }) => {
    const view = document.querySelector('[data-testid="tasks-view"]');
    const boardReady = view?.getAttribute("data-tasks-sha") && view?.getAttribute("data-can-edit") !== "pending";
    const input = document.querySelector(`[data-testid="${searchId}"]`);
    const search = input?.closest(".tasks-search"), tabs = document.querySelector(".tasks-tabs");
    return boardReady && search?.parentElement === tabs?.parentElement && search?.getBoundingClientRect().width > 0;
  }, { searchId });
  const layout = await page.evaluate((searchId) => {
    const search = document.querySelector(`[data-testid="${searchId}"]`).closest(".tasks-search");
    const toolbar = search.parentElement;
    const tabs = toolbar.querySelector(".tasks-tabs");
    const chips = toolbar.querySelector(".tasks-chips");
    const a = search.getBoundingClientRect(), b = tabs.getBoundingClientRect(), c = chips.getBoundingClientRect();
    return { x: a.x, y: a.y, h: a.height, tabsY: b.y, tabsH: b.height, chipsY: c.y, gap: getComputedStyle(toolbar).gap,
      searchStyle: ["padding", "borderRadius", "backgroundColor"].map((k) => getComputedStyle(search)[k]),
      overflow: document.documentElement.scrollWidth > innerWidth + 1 };
  }, searchId);
  assert.equal(layout.overflow, false);
  if (expected) {
    for (const k of ["x", "y", "h", "tabsY", "tabsH", "gap", "searchStyle"]) assert.deepEqual(layout[k], expected[k], `탭 전환 뒤 ${k}`);
  }
  if (page.viewportSize().width >= 1920) {
    assert.ok(Math.abs(layout.y + layout.h / 2 - layout.tabsY - layout.tabsH / 2) <= 1);
    assert.ok(layout.chipsY < layout.y + layout.h, "넓은 화면의 칩도 탭·검색과 같은 줄");
  } else if (page.viewportSize().width <= 640) {
    assert.ok(layout.y >= layout.tabsY + layout.tabsH, "좁은 화면 검색은 탭 다음 줄");
    assert.ok(layout.chipsY >= layout.y + layout.h, "좁은 화면 칩은 검색 다음 줄");
  }
  return layout;
}

export default async function ({ page }) {
  let restoredId;
  await step("보관 규모: 4MiB 초과 보관도 CLI 전체 출력과 host 페이지의 크기·검색 조합을 지킨다", async () => {
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
      const row = { task_id: id, task: { id, list: "Backlog", title, owner: i === 100 ? "fake@mac-a" : "me", thread: null, notes: [`needle-${i} 결과 메모`], done: true, repo }, body, reason: ["PR 병합", "origin/main 병합", "보고 [완료]"][(i - 1) % 3], report: null, completed_ms: i === 100 ? Date.now() - 86400000 : i, restored: i > 128, committed: true, removed_sha256: "", history };
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

  await step("완료 화면: 활성 UI와 같은 검색·저장소·완료 상태 조합과 다음 페이지·복원을 유지한다", async () => {
    aaa("ticker", "tick");
    await page.getByTestId("activity-tasks").click();
    await page.getByTestId("tasks-search").waitFor();
    await page.setViewportSize({ width: 1920, height: 1000 });
    const activeLayout = await toolbarLayout(page, "tasks-search");
    await page.getByTestId("tasks-tab-completed").click();
    await toolbarLayout(page, "task-completed-search", activeLayout);
    await page.getByTestId("tasks-tab-tasks").click();
    await toolbarLayout(page, "tasks-search", activeLayout);
    await shot(page, "history-desktop-active");
    await page.getByTestId("tasks-tab-completed").click();
    await domIs(page, "보관 첫 50개", () => document.querySelectorAll('[data-testid^="archive-"]').length === 50);
    await toolbarLayout(page, "task-completed-search", activeLayout);
    await shot(page, "history-desktop-records");
    await page.getByTestId("task-archive-more").click();
    await domIs(page, "보관 다음 50개", () => document.querySelectorAll('[data-testid^="archive-"]').length === 100);
    await page.getByTestId("task-completed-search").fill("needle-100");
    await choose(page, "reason", "PR 병합");
    await page.getByTestId("task-completed-repo").click();
    await page.getByRole("menuitemcheckbox").filter({ has: page.locator(".menu-item-main").filter({ hasText: /^rc$/ }) }).click();
    await domIs(page, "세 조건 한 건", () => document.querySelectorAll('[data-testid^="archive-"]').length === 1 && document.querySelector('[data-testid^="archive-"]')?.textContent.includes("보관 100"));
    await choose(page, "profile", "fake");
    await choose(page, "machine", "mac-a");
    await choose(page, "period", "최근 7일");
    await domIs(page, "완료 여섯 조건", () => {
      const list = document.querySelector('[data-testid="task-archive-list"]');
      return list?.getAttribute("aria-busy") === "false" && list.querySelectorAll('[data-testid^="archive-"]').length === 1 && list.textContent.includes("모델: fake") && list.textContent.includes("머신: mac-a");
    });
    assert.equal(await page.getByRole("tab", { name: "기록", exact: true }).count(), 0);
    await shot(page, "completed-desktop-dark");
    await page.evaluate(() => document.documentElement.setAttribute("data-theme", "light"));
    await shot(page, "completed-desktop-light");
    await page.evaluate(() => document.documentElement.setAttribute("data-theme", "dark"));
    await choose(page, "profile", "전체");
    await choose(page, "machine", "전체");
    await choose(page, "period", "전체");
    const r = await archive(page, { search: { query: "needle-100", repo: "rc" }, filter: null, after: null, limit: 50 });
    restoredId = r.ok.entries[0].task_id;
    // 쓰기 응답만 보류한다. 실제 host 복원·사건·다른 검색은 그대로 진행한다.
    let releaseRestore, restoreReached;
    const release = new Promise((resolve) => { releaseRestore = resolve; });
    const reached = new Promise((resolve) => { restoreReached = resolve; });
    await page.route("**/api/v1/call", async (route) => {
      const payload = route.request().postDataJSON();
      if (payload?.op !== "task_archive" || payload.restore !== restoredId) return route.continue();
      const response = await route.fetch();
      restoreReached();
      await release;
      await route.fulfill({ response });
    });
    await rows(page).locator("summary").click();
    await rows(page).getByRole("button", { name: "되살리기", exact: true }).click();
    await reached;
    await page.getByTestId("task-completed-search").fill("needle-102");
    await choose(page, "reason", "전체");
    await rows(page).filter({ hasText: "보관 102" }).waitFor();
    const restoredResponse = page.waitForResponse((r) => r.request().postDataJSON()?.restore === restoredId);
    releaseRestore();
    await restoredResponse;
    await domIs(page, "복원 응답 뒤 새 검색 보존", () => {
      const list = document.querySelector('[data-testid="task-archive-list"]');
      return list?.getAttribute("aria-busy") === "false";
    });
    assert.equal(await rows(page).count(), 1);
    assert.match(await rows(page).textContent(), /보관 102/);
    await page.unroute("**/api/v1/call");
    await page.getByTestId("task-completed-search").fill("needle-100");
    await domIs(page, "복원 뒤 현재 조건 빈 목록", () => document.querySelector('[data-testid="task-archive-list"]')?.textContent.includes("조건에 맞는 완료한 할 일이 없다"));
    const restored = aaa("task", "list").tasks.find((t) => t.id === restoredId);
    assert.ok(restored && !restored.done && restored.thread === null);
    assert.equal(aaa("task", "queue").entries.length, 0);
    await choose(page, "reason", "되살린 항목");
    await rows(page).filter({ hasText: "보관 100" }).waitFor();
    assert.equal(await rows(page).getByRole("button", { name: "되살리기" }).count(), 0);

  });

  await step("휴대폰과 Reader: 320·390px의 같은 메뉴·상태·검색과 읽기 권한을 지킨다", async () => {
    const safari = await webkit.launch({ headless: true });
    try {
      for (const [engine, width] of [[browser, 320], [safari, 390]]) {
        const paired = await pairPage(engine, aaa("web", "pair", "--reader").code, { width, height: 844 }, { hash: "#tasks", touch: true });
        try {
          const p = paired.page;
          await p.getByTestId("tasks-search").waitFor();
          const activeLayout = await toolbarLayout(p, "tasks-search");
          await shot(p, `history-${width}-active`);
          await p.getByTestId("tasks-tab-completed").click();
          await rows(p).first().waitFor();
          await toolbarLayout(p, "task-completed-search", activeLayout);
          await shot(p, `history-${width}-records`);
          await p.getByTestId("task-completed-search").fill("needle-100");
          await choose(p, "reason", "되살린 항목");
          await rows(p).filter({ hasText: "보관 100" }).waitFor();
          await p.getByTestId("task-completed-repo").click();
          await p.getByRole("dialog", { name: "저장소로 거르기" }).getByRole("button").filter({ has: p.locator(".row-sheet-label").filter({ hasText: /^rc$/ }) }).click();
          await p.getByRole("dialog", { name: "저장소로 거르기" }).waitFor({ state: "detached" });
          // 공통 sheet의 퇴장 화면은 닫힌 shadow root에 있다. 바깥 host의 제거 사건 뒤 촬영한다.
          await domIs(p, "저장소 시트 퇴장 완료", () => !document.querySelector('body > [inert][aria-hidden="true"]'));
          await rows(p).filter({ hasText: "보관 100" }).waitFor();
          assert.ok(await p.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1));
          assert.equal(await p.getByRole("button", { name: "되살리기" }).count(), 0);
          const denied = await request(p, { op: "task_archive", restore: restoredId });
          assert.equal(denied.err?.code, "forbidden");
          await rows(p).locator("summary").click();
          assert.equal(await p.getByText("기록 보기", { exact: true }).count(), 0);
          await shot(p, `completed-${width}-dark`);
          await p.evaluate(() => document.documentElement.setAttribute("data-theme", "light"));
          await shot(p, `completed-${width}-light`);
        } finally { await paired.page.context().close(); }
      }
    } finally { await safari.close(); }
  });

  await step("칩과 완료 체크: 모델만 정한 대기·칩 합·직접 체크 보관과 복원을 유지한다", async () => {
    // 운영 파일 대신 이 실험실의 파일에서 모델 지정·thread 없음과 완료 상하위를 만든다.
    fs.appendFileSync(`${root}/projects/demo/TASKS.md`, "- [ ] 모델 대기 (fake) [작업 폴더]\n");
    aaa("ticker", "tick");
    await page.getByTestId("tasks-tab-tasks").click();
    await page.getByTestId("tasks-filter-waiting").click();
    await page.locator(".task-row").filter({ hasText: "모델 대기" }).waitFor();
    const counts = await page.locator(".tasks-filter .tasks-count").allTextContents();
    assert.equal(counts.slice(1).reduce((n, c) => n + Number(c), 0), Number(counts[0]));
    await shot(page, "history-desktop-waiting");
    const source = fs.readFileSync(`${root}/projects/demo/TASKS.md`, "utf8");
    fs.writeFileSync(`${root}/projects/demo/TASKS.md`, source.replace("- [ ] 모델 대기", "- [x] 모델 대기") + "- [x] 완료 상위 (fake) [작업 폴더]\n  보관 메모\n  - [X] 완료 하위\n");
    aaa("ticker", "tick");
    const tasks = aaa("task", "list").tasks;
    assert.ok(!tasks.some((t) => ["모델 대기", "완료 상위", "완료 하위"].includes(t.title)));
    const completed = aaa("task", "archive").entries.filter((a) => a.reason === "완료 체크 [x]");
    assert.equal(completed.length, 3);
    await page.getByTestId("tasks-tab-completed").click();
    await choose(page, "reason", "전체");
    await page.getByTestId("task-completed-search").fill("완료 상위");
    await rows(page).filter({ hasText: "완료 상위" }).waitFor();
    await shot(page, "history-desktop-checked");
    const item = completed.find((a) => a.task.title === "완료 상위");
    // 복원 요청 때도 목록을 비우므로 행 없음만으로는 완료가 아니다. 같은 ID의 쓰기 응답을 먼저 확인한다.
    const restoreResponse = page.waitForResponse((response) => {
      if (!response.url().endsWith("/api/v1/call") || response.request().method() !== "POST") return false;
      const request = response.request().postDataJSON();
      return request?.op === "task_archive" && request.restore === item.task_id;
    });
    await rows(page).filter({ hasText: "완료 상위" }).locator("summary").click();
    await rows(page).filter({ hasText: "완료 상위" }).getByRole("button", { name: "되살리기", exact: true }).click();
    const response = await (await restoreResponse).json();
    assert.equal(response.ok?.kind, "task_archive", JSON.stringify(response.err));
    await domIs(page, "완료 체크 복원 뒤 빈 보관 검색", () => {
      const list = document.querySelector('[data-testid="task-archive-list"]');
      return list?.getAttribute("aria-busy") === "false" && list.textContent.includes("조건에 맞는 완료한 할 일이 없다");
    });
    const restored = aaa("task", "list").tasks.find((t) => t.id === item.task_id);
    assert.ok(restored && !restored.done && restored.thread === null);
    assert.equal(restored.owner, "fake");
    assert.equal(aaa("task", "queue").entries.length, 0);
  });

}

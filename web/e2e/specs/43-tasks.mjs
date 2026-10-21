// 할 일 보기(T1, 사용자 10-20 「하기로 했던 기능들을 다 볼수 있고 (왼쪽에 아이콘이 있어서 거기서 들어가게)」, plan §8.306). host가
// TASKS.md를 읽어 목록·형식 어긋남·task 없는 열린 thread를 주고, 웹은 활동 막대 「할 일」에서 그것을 보인다. 파일을 직접 고치면 ticker가
// 바뀐 hash를 `tasks` 사건으로 알리고 화면이 다시 읽는다. 시간은 기다리지 않는다: 화면 표지 `data-tasks-sha`·`data-task-state`를 기다린다.
import fs from "node:fs";
import crypto from "node:crypto";
import { root, aaa, step, browser, pairPage, domIs, hashIs, shot, openSettings } from "../lib/harness.mjs";
import { pairDevice } from "../lib/devices.mjs";

const dir = () => `${root}/projects/demo`;
const sha = (text) => crypto.createHash("sha256").update(text).digest("hex");
/** TASKS.md를 쓰고 그 hash(화면 `data-tasks-sha`가 이것이 되기를 기다린다) */
const write = (text) => {
  fs.writeFileSync(`${dir()}/TASKS.md`, text);
  // 화면 관측 전에 식별 주석 이주를 마친 fixture를 준비한다.
  return aaa("task", "list").sha256;
};
// 운영에서 나온 오독 꼴 셋은 같은 꼴의 가짜 글이다(끝 괄호 상태 메모, `(미할당)`, 본문 속 `· t-NNNN(…)`)
const tasksMd = (run) => `# Tasks

## In progress
- [ ] 로그인 고치기 (fake@labA) · ${run}
  - 메모: **테스트** 먼저
  - [ ] 하위 단계 하나 (me)
## Backlog
나중 일은 memory/later.md에 모은다
- [ ] 정리(선택) (t-0101 S4 뒤, 사용자 지시 시)
  사용자 지시 뒤에 한다
- [ ] 실패 조사 (미할당)
- [ ] 웹 화면 W1~W8(계획 · t-0102(opus-high), 이어서 W2~W8 차례로)
- [ ] 배포 (me)
- [ ] 원격 일 (codex@labA)
## 빈 목록
`;
const LINE = { login: 4, sub: 6, tidy: 9, probe: 11, web: 12, deploy: 13, remote: 14 };
const shaIs = (p, want) => domIs(p, `할 일 hash ${want.slice(0, 8)}`, (w) => document.querySelector('[data-testid="tasks-view"]')?.dataset.tasksSha === w, want);
const stateOf = (p, line) => p.getByTestId(`task-${line}`).getAttribute("data-task-state");

/** 상세의 자리: 화면 안 상자와 목록 칸 */
const detailBox = (p) =>
  p.evaluate(() => {
    const d = document.querySelector('[data-testid="task-detail"]')?.getBoundingClientRect();
    const l = document.querySelector('[data-testid="tasks-list"]')?.getBoundingClientRect();
    return d && l && { d: { left: d.left, right: d.right, top: d.top, bottom: d.bottom, width: d.width }, l: { left: l.left, right: l.right, width: l.width }, w: innerWidth, h: innerHeight, wide: document.documentElement.scrollWidth > innerWidth + 1 };
  });

export default async function ({ page }) {
  let run, side, md;
  await step("할 일 보기: 막대 「할 일」이 목록·행 상태·형식 어긋남 배지·상세·task 없는 열린 thread를 보이고, thread 칩이 그 thread로 간다", async () => {
    // 가짜 thread가 곧 턴을 끝내 앱 안 알림이 화면 위를 덮는다(사라지는 것은 시간이다): 이 기기는 앱 안 알림을 끈다
    const dialog = await openSettings(page, "알림");
    await dialog.getByRole("switch", { name: "앱 안에서" }).click();
    await page.keyboard.press("Escape");
    aaa("project", "create", "demo");
    fs.writeFileSync(`${root}/task-41.md`, "할 일 보기");
    run = aaa("thread", "start", "demo", "--title", "로그인 고치기", "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/task-41.md`).id;
    side = aaa("thread", "start", "demo", "--title", "따로 도는 일", "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/task-41.md`).id;
    md = write(tasksMd(run));
    // 기준을 심는다(첫 tick은 기억만 한다)
    aaa("ticker", "tick");
    await page.getByTestId("activity-tasks").click();
    await shaIs(page, md);
    const view = page.getByTestId("tasks-view");
    if ((await view.getAttribute("data-shown")) !== "true") throw new Error("할 일 화면이 보이지 않는다");
    if (!(await page.evaluate(() => location.hash === "#tasks"))) throw new Error("주소가 #tasks가 아니다");
    // 행 상태: 열린 thread와 이은 task는 진행 중, 오독 꼴 셋은 형식 어긋남, 나머지는 대기
    const want = { [LINE.login]: "run", [LINE.sub]: "wait", [LINE.tidy]: "problem", [LINE.probe]: "problem", [LINE.web]: "problem", [LINE.deploy]: "wait", [LINE.remote]: "wait" };
    for (const [line, state] of Object.entries(want)) if ((await stateOf(page, line)) !== state) throw new Error(`${line}줄 상태 ${await stateOf(page, line)} ≠ ${state}`);
    if ((await page.getByTestId("activity-badge-tasks").textContent()) !== "3") throw new Error("배지는 형식 어긋남 3");
    // 목록 머리·설명 줄·하위 들여쓰기, task 없는 목록도 머리가 있다
    const heads = await page.locator(".task-list-name").allTextContents();
    if (heads.join("|") !== "In progress|Backlog|빈 목록") throw new Error(`목록 머리 ${heads}`);
    if (!(await page.getByTestId("task-list-Backlog").textContent()).includes("나중 일은 memory/later.md에 모은다")) throw new Error("목록 설명 줄");
    const pad = (line) => page.getByTestId(`task-${line}`).evaluate((el) => parseFloat(getComputedStyle(el).paddingLeft));
    if (!((await pad(LINE.sub)) > (await pad(LINE.login)))) throw new Error("하위 task 들여쓰기");
    // owner를 오독하지 않는다: 형식 어긋남 칩이고 사람 칩(「t-0101 S4 뒤, …」)이 없다
    const tidy = page.getByTestId(`task-${LINE.tidy}`);
    if (!(await tidy.textContent()).includes("형식 어긋남") || (await tidy.locator(".task-owner").count()) !== 0) throw new Error(`정리 줄 ${await tidy.textContent()}`);
    // task 없는 열린 thread는 다른 thread 하나다
    const unlinked = await page.getByTestId("tasks-unlinked").locator("[data-testid^='tasks-unlinked-']").evaluateAll((els) => els.map((e) => e.dataset.testid));
    if (unlinked.join() !== `tasks-unlinked-${side}`) throw new Error(`task 없는 thread ${unlinked}`);
    // 상세: 원문 줄·형식 어긋남·메모
    await tidy.locator(".task-pick").click();
    const detail = page.getByTestId("task-detail");
    await detail.waitFor();
    const tidyId = aaa("task", "list").tasks.find(t => t.line === LINE.tidy).id;
    if ((await detail.getByTestId("task-raw").textContent()) !== `- [ ] 정리(선택) (t-0101 S4 뒤, 사용자 지시 시) <!-- aaa-task:${tidyId} -->`) throw new Error("원문 줄");
    const problems = await detail.getByTestId("task-problems").textContent();
    if (!problems.includes("owner 칸이 owner 꼴이 아님") || !problems.includes("제목 안에 thread 번호가 묻힘: t-0101")) throw new Error(problems);
    if (!(await detail.textContent()).includes("사용자 지시 뒤에 한다")) throw new Error("메모");
    const box = await detailBox(page);
    if (!(box.l.right <= box.d.left + 1) || box.wide) throw new Error(`데스크톱은 목록 오른쪽 칸 ${JSON.stringify(box)}`);
    await shot(page, "tasks-desktop");
    // 이은 task의 메모는 Markdown으로 그린다
    await page.getByTestId(`task-${LINE.login}`).locator(".task-pick").click();
    await detail.locator(".task-notes strong", { hasText: "테스트" }).waitFor();
    // thread 칩 → 그 thread, 막대 「할 일」로 돌아오면 고른 task가 그대로
    await page.getByTestId(`task-${LINE.login}`).getByTestId(`task-thread-${run}`).click();
    await hashIs(page, `#thread=${run}`);
    await page.getByTestId("activity-tasks").click();
    await hashIs(page, "#tasks");
    await domIs(page, "고른 task 상세", (t) => document.querySelector('[data-testid="task-detail"]')?.getAttribute("aria-label") === `할 일 ${t}`, "로그인 고치기");
  });

  await step("파일 직접 편집 → ticker tick → `tasks` 사건 → 화면이 새 hash로 다시 읽는다. 고치지 않은 tick은 hash를 바꾸지 않는다", async () => {
    aaa("ticker", "tick");
    if ((await page.getByTestId("tasks-view").getAttribute("data-tasks-sha")) !== md) throw new Error("고치지 않았는데 hash가 바뀌었다");
    md = write(tasksMd(run).replace("- [ ] 배포 (me)\n", "- [ ] 배포 (me)\n- [ ] 새로 더한 일\n"));
    aaa("ticker", "tick");
    await shaIs(page, md);
    if (!(await page.getByTestId("task-list-Backlog").textContent()).includes("새로 더한 일")) throw new Error("새 줄이 없다");
    // 고른 task(제목으로 따라간다)는 줄이 밀려도 그대로 열려 있다
    if ((await page.getByTestId("task-detail").getAttribute("aria-label")) !== "할 일 로그인 고치기") throw new Error("고른 task");
  });

  await step("거르기·검색: 내 일·미할당·진행 중 칩과 제목·메모·owner 검색", async () => {
    const shown = () => page.locator(".task-row[data-task-state]").evaluateAll((els) => els.map((e) => e.dataset.testid));
    await page.getByTestId("tasks-filter-mine").click();
    await domIs(page, "내 일", () => document.querySelectorAll(".task-row[data-task-state]").length === 2);
    if ((await shown()).join() !== `task-${LINE.sub},task-${LINE.deploy}`) throw new Error(`내 일 ${await shown()}`);
    await page.getByTestId("tasks-filter-running").click();
    await domIs(page, "진행 중", () => document.querySelectorAll(".task-row[data-task-state]").length === 1);
    await page.getByTestId("tasks-filter-all").click();
    await page.getByTestId("tasks-search").fill("지시 뒤");
    await domIs(page, "메모 검색", () => document.querySelectorAll(".task-row[data-task-state]").length === 1);
    if ((await shown()).join() !== `task-${LINE.tidy}`) throw new Error(`검색 ${await shown()}`);
    await page.getByTestId("tasks-search").fill("");
  });

  await step("보기 권한 기기도 할 일 화면을 본다", async () => {
    const r = await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 1280, height: 800 });
    try {
      await r.page.getByTestId("activity-tasks").click();
      await shaIs(r.page, md);
      if ((await stateOf(r.page, LINE.login)) !== "run") throw new Error("Reader 행 상태");
      if (r.errors.length) throw new Error(r.errors.join("\n"));
    } finally { await r.page.context().close(); }
  });

  await step("휴대폰 폭: 상세는 전체 화면 시트이고 Back이 닫는다(할 일 화면에 남는다)", async () => {
    const { page: p } = await pairDevice(browser, aaa("web", "pair").code, "iphone", { hash: "#tasks" });
    try {
      await shaIs(p, md);
      await shot(p, "tasks-phone");
      await p.getByTestId(`task-${LINE.web}`).locator(".task-pick").click();
      await p.getByTestId("task-detail").waitFor();
      const box = await detailBox(p);
      if (Math.abs(box.d.left) > 1 || Math.abs(box.d.right - box.w) > 1 || box.d.bottom < box.h - 1 || box.wide) throw new Error(`휴대폰 상세는 화면 전체 ${JSON.stringify(box)}`);
      await shot(p, "tasks-phone-detail");
      await p.evaluate(() => new Promise((r) => { addEventListener("popstate", () => r(), { once: true }); history.back(); }));
      await p.getByTestId("task-detail").waitFor({ state: "detached" });
      if ((await p.evaluate(() => location.hash)) !== "#tasks") throw new Error("Back이 할 일 화면을 떠났다");
    } finally { await p.context().close(); }
  });

  await step("패드: 세로는 오른쪽 시트, 가로는 두 칸", async () => {
    const { page: p } = await pairDevice(browser, aaa("web", "pair").code, "ipad", { hash: "#tasks" });
    try {
      await shaIs(p, md);
      await p.getByTestId(`task-${LINE.login}`).locator(".task-pick").click();
      await p.getByTestId("task-detail").waitFor();
      let box = await detailBox(p);
      if (Math.abs(box.d.right - box.w) > 1 || box.d.width >= box.l.width || box.d.left <= box.l.left || box.wide) throw new Error(`패드 세로 시트 ${JSON.stringify(box)}`);
      await shot(p, "tasks-pad-portrait");
      await p.setViewportSize({ width: 1180, height: 820 });
      await domIs(p, "패드 가로 두 칸", () => {
        const d = document.querySelector('[data-testid="task-detail"]')?.getBoundingClientRect();
        const l = document.querySelector('[data-testid="tasks-list"]')?.getBoundingClientRect();
        return d && l && l.right <= d.left + 1;
      });
      box = await detailBox(p);
      if (box.wide) throw new Error("패드 가로 넘침");
      await shot(p, "tasks-pad-landscape");
    } finally { await p.context().close(); }
  });

  await step("「나중」 탭: 설정이 비면 없고, task_notes를 더하면 생겨 `##` 절을 접는 묶음으로 보이며, 없는 파일은 「없음」이다", async () => {
    if ((await page.getByTestId("tasks-tab-later").count()) !== 0) throw new Error("설정이 비었는데 「나중」 탭");
    fs.mkdirSync(`${dir()}/memory`, { recursive: true });
    fs.writeFileSync(`${dir()}/memory/later.md`, "# 나중 작업 모음\n\n## UI\n- 건설 안내 문구를 UI로\n- `카드` 기울기\n\n## 밸런스\n- 포병이 약함\n");
    aaa("project", "set", "demo", "task_notes.add", "memory/later.md");
    aaa("ticker", "tick");
    await page.getByTestId("tasks-tab-later").waitFor();
    await page.getByTestId("tasks-tab-later").click();
    const file = page.getByTestId("tasks-notes-memory/later.md");
    const names = await file.locator("summary .task-list-name").allTextContents();
    if (names.join("|") !== "UI|밸런스") throw new Error(`절 ${names}`);
    await file.locator("summary", { hasText: "UI" }).click();
    await file.locator(".task-notes code", { hasText: "카드" }).waitFor();
    await shot(page, "tasks-later");
    aaa("project", "set", "demo", "task_notes.add", "memory/none.md");
    aaa("ticker", "tick");
    await domIs(page, "없는 파일", () => document.querySelector('[data-testid="tasks-notes-memory/none.md"] [data-testid="tasks-notes-error"]')?.textContent === "없음");
    // 「나중」 파일을 고치면 다시 읽는다
    fs.appendFileSync(`${dir()}/memory/later.md`, "- 마석 사용처\n");
    aaa("ticker", "tick");
    await domIs(page, "고친 「나중」", () => document.querySelector('[data-testid="tasks-notes-memory/later.md"]')?.textContent.includes("밸런스2"));
    for (const p of ["memory/later.md", "memory/none.md"]) aaa("project", "set", "demo", "task_notes.remove", p);
    aaa("ticker", "tick");
    await page.getByTestId("tasks-tab-later").waitFor({ state: "detached" });
    await page.getByTestId("tasks-list").waitFor();
  });
}

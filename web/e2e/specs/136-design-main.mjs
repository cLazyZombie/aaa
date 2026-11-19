// 실제 host·agent·Git에서 기획 편집과 구현 맡기기가 일반 작업 branch를 쓰는지 확인한다.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import { AA, aaa, root, step, browser, webkit, pairPage, inList, domIs, until, shot } from "../lib/harness.mjs";

const LOOP = `# 게임 루프

탑 앞 캠프에서 1층부터 오른다.

## 탑

지금 20층까지 있다.

## 바뀐 것

- 11-15: 처음 정함.
`;

/** 블록 표지 문서(3-1): 넘긴 할 일 셋(일함·기다림·끝남)과 없는 d-9999, 아직 안 넘긴 계획 둘, 미정 하나 */
const EXIT = (d) => `# 나가기

탑에서 캠프로 돌아오는 길.

## 통로

지금 통로는 계단 하나다.

- 계단
- 귀환 두루마리

  두루마리는 상자에서만 나온다.

> [!계획] ${d.work}
> 처음 1칸, 스킬 트리로 최대 6칸.

> [!계획] ${d.wait}
> 스크롤·랜덤 통로.

> [!미정]
> 스킬 무기가 나오는 확률

## 상한

> [!계획] ${d.done}
> 층 표 상한을 99로 둔다.

> [!계획] d-9999
> 나간 뒤 가방 정리 화면.

> [!계획]
> **캠프 지도** — 시설 자리.

> [!계획]
> **상점 위치** — 캠프 입구 옆.

## 바뀐 것

- 11-16: 통로를 ${d.work}로 넘김.
`;

const git = (cwd, ...args) => execFileSync("git", ["-c", "user.name=demo", "-c", "user.email=demo@example.com", "-c", "init.defaultBranch=main", ...args], {cwd,encoding:"utf8"}).trim();
const ORIGIN = () => `${root}/origins/app.git`;
const mainHead = () => git(root, "--git-dir", ORIGIN(), "rev-parse", "refs/heads/main");

export default async function () {
  const tasks = {};
  await step("기획 문서와 저장소", async () => {
    const origin = ORIGIN(), seed = `${root}/seed/app`, clone = `${root}/app`;
    for (const d of [`${root}/origins`, seed]) fs.mkdirSync(d, { recursive: true });
    git(root, "init", "--bare", "-q", origin);
    git(seed, "init", "-q");
    fs.mkdirSync(`${seed}/docs/loop`, { recursive: true });
    fs.writeFileSync(`${seed}/docs/README.md`, "# 개요\n");
    fs.writeFileSync(`${seed}/docs/loop/README.md`, LOOP);
    git(seed, "add", ".");
    git(seed, "commit", "-q", "-m", "처음");
    git(seed, "remote", "add", "origin", origin);
    git(seed, "push", "-q", "origin", "HEAD:main");
    git(root, "clone", "-q", origin, clone);
    git(clone, "config", "user.name", "demo");
    git(clone, "config", "user.email", "demo@example.com");
    aaa("project", "create", "demo", "--thread-profile", "fake");
    // aaa.toml이 있으면 그것이 프로젝트 설정이다: thread profile도 여기에 둔다(대화·구현 thread가 쓴다)
    fs.writeFileSync(`${root}/projects/demo/aaa.toml`, `schema = 1\nthread_profile = "fake"\n\n[[workspaces]]\npath = "${clone}"\ndesign = "docs"\ndesign_media = "mockups"\n`);
    for (const [k, title] of [["work", "가호 칸 6칸"], ["wait", "나가기 통로"], ["done", "층 표 상한"]]) {
      aaa("task", "add", title, "--list", "Backlog");
      tasks[k] = aaa("task", "list").tasks.find((t) => t.title === title).short_id;
    }
    fs.writeFileSync(`${seed}/docs/exit.md`, EXIT(tasks));
    git(seed, "add", ".");
    git(seed, "commit", "-q", "-m", "표지");
    git(seed, "push", "-q", "origin", "HEAD:main");
  }, { always: true });
  await step("절을 편집하는 일반 thread와 수정본", async () => {
    const { page } = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 900 }, {
      hash: `#design=${encodeURIComponent("app/docs/loop/README.md")}`,
    });
    try {
      await page.getByTestId("design-edit").click();
      const dialog = page.getByRole("dialog", {name:"편집 요청"});
      await dialog.getByLabel("편집할 부분").selectOption({label:"탑"});
      await dialog.getByLabel("요청 내용").fill("탑을 30층까지 늘려 주세요.");
      const base = mainHead();
      await dialog.getByRole("button", {name:"작업 요청"}).click();
      await dialog.getByRole("status").filter({hasText:"시작 예약됨"}).waitFor();
      const id = await dialog.getByRole("link").textContent();
      const thread = await until("편집 thread", () => aaa("task", "list").tasks.find(t => t.short_id === id)?.thread, 60_000);
      const shown = await until("편집 worktree", () => { const t = aaa("thread","show",thread); return t.work_dir && fs.existsSync(`${t.work_dir}/brief.md`) && t; }, 60_000);
      const brief = fs.readFileSync(`${shown.work_dir}/brief.md`, "utf8");
      assert.ok(brief.includes("지금 20층까지 있다.") && brief.includes("30층까지") && brief.includes("mockups"), brief);
      const tree = shown.work_dir.replace(/\/\.aaa\/[^/]+\/?$/, "");
      assert.match(git(tree,"branch","--show-current"), /^aaa\//);
      fs.writeFileSync(`${tree}/docs/loop/README.md`, LOOP.replace("20층","30층"));
      git(tree,"commit","-qam","탑 편집");
      const dest = `${shown.work_dir}/library/review`;
      execFileSync(AA,["plan","preview","docs/loop/README.md","--base",base,"--output",dest],{cwd:tree});
      const html = fs.readFileSync(`${dest}/index.html`, "utf8");
      assert.match(html,/20층/); assert.match(html,/30층/); assert.match(html,/수정 전 보기/);
      assert.equal(mainHead(),base);
    } finally { await page.context().close(); }
  });
  await step("블록 표지: 넘긴 할 일 칩·이상 표시·글 속 d-ID·보기 셋·트리 수·블록 편집 요청", async () => {
    // 넘긴 할 일 하나는 thread가 일하고, 하나는 끝나 보관됐다(표지 남음). d-9999는 어디에도 없다(할 일 없음)
    const t = aaa("thread", "start", "demo", "--from-task", tasks.work, "--machine", "labA", "--profile", "fake", "--kind", "dir");
    aaa("task", "edit", tasks.done, "--done");
    const { page: p } = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 900 });
    try {
      await inList(p, "activity-design");
      const exit = p.locator('[data-testid="design-doc"][data-path="docs/exit.md"]');
      await exit.waitFor();
      assert.deepEqual(await exit.locator("[data-testid=design-marks] span").allTextContents(), ["4", "2", "1"]);
      await p.getByTestId("design-todo-on").click();
      await p.locator('[data-testid="design-doc"][data-path="docs/README.md"]').waitFor({ state: "detached" });
      assert.match(await p.getByTestId("design-plain").textContent(), /표지 없는 문서 \d+/);
      await exit.click();
      await p.getByTestId("design-body").waitFor();
      const state = (id) => p.locator(`[data-testid=design-callout][data-task="${id}"] [data-testid=design-state]`);
      await domIs(p, "할 일 상태", () => !!document.querySelector("[data-testid=design-callout] [data-testid=design-state]"));
      await domIs(p, "보관 확인", () => [...document.querySelectorAll("[data-testid=design-state]")].some((e) => e.textContent.startsWith("표지 남음")));
      assert.equal((await state(tasks.wait).textContent()).trim(), `대기 ${tasks.wait}`);
      assert.equal((await state(tasks.done).textContent()).trim(), `표지 남음 ${tasks.done}`);
      assert.equal((await state("d-9999").textContent()).trim(), "할 일 없음 d-9999");
      const chips = await p.evaluate(id => ({
        block: document.querySelector(`[data-testid=design-callout][data-task="${id}"] [data-testid=design-state]`)?.textContent.trim(),
        inline: document.querySelector('[data-testid=design-inline-state]')?.textContent.trim(),
      }), tasks.work);
      assert.match(chips.block, new RegExp(`^(작업 중|검수 대기) ${tasks.work} · ${t.id}$`), "thread가 맡은 할 일");
      assert.equal(await state(tasks.work).getAttribute("href"), `#task=${tasks.work}`);
      assert.equal(await p.getByTestId("design-body").locator("li", { hasText: "두루마리는 상자에서만 나온다" }).count(), 1, "목록 안의 문단");
      assert.equal(chips.inline, chips.block, "글 속 d-ID는 같은 칩");
      assert.deepEqual(
        await p.locator("[data-testid=design-callout]").evaluateAll((els) => els.map((e) => e.dataset.kind)),
        ["plan", "plan", "open", "plan", "plan", "plan", "plan"],
      );
      assert.equal((await p.getByTestId("design-count-work").textContent()).trim(), "진행 4");
      assert.equal((await p.getByTestId("design-count-plan").textContent()).trim(), "계획 2");
      assert.equal((await p.getByTestId("design-count-open").textContent()).trim(), "미정 1");
      await p.locator("[data-testid=design-look][data-look=now]").click();
      await domIs(p, "지금 모습", () => !document.querySelector("[data-testid=design-callout]"));
      const now = await p.getByTestId("design-body").textContent();
      assert.ok(now.includes("지금 통로는 계단 하나다") && now.includes("바뀐 것") && !now.includes("상한"), now);
      await p.locator("[data-testid=design-look][data-look=left]").click();
      await p.getByTestId("design-left").waitFor();
      assert.deepEqual(await p.locator(".design-left-group h3").allTextContents(), ["통로일부 구현", "상한계획만"]);
      assert.equal(await p.getByTestId("design-left-row").count(), 7);
      await p.getByTestId("design-left").getByTestId("design-discuss").click();
      const edit = p.getByRole("dialog", {name:"편집 요청"});
      await edit.getByText("스킬 무기가 나오는 확률", {exact:false}).first().waitFor();
      await edit.getByRole("button",{name:"취소"}).click();
    } finally {
      await p.context().close();
    }
  });
  await step("구현 맡기기의 모델·블록 선택과 검토한 판 확인", async () => {
    const landFixture = (text) => {
      const seed = `${root}/seed/app`;
      fs.writeFileSync(`${seed}/docs/exit.md`, text);
      git(seed, "commit", "-qam", "기획 문서 갱신");
      git(seed, "push", "-q", "origin", "HEAD:main");
      git(`${root}/app`, "fetch", "-q", "origin");
      aaa("plan", "list");
    };
    const d = aaa("plan", "show", "app", "docs/exit.md").doc;
    landFixture(d.text.replace("## 바뀐 것", "> [!계획]\n> **층 이벤트** — 다섯 층마다 이벤트 방.\n\n## 바뀐 것"));
    // 최신 트리 없이 캐시에서 연 창을 확인한다. 기다림은 응답을 풀거나 상태가 나타날 때 끝난다.
    const fresh = Promise.withResolvers();
    const assignable = Promise.withResolvers();
    let modelReply = "live";
    const { page: p } = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 900 }, {
      hash: `#design=${encodeURIComponent("app/docs/exit.md")}`,
      serviceWorkers: "block",
      route: ["**/api/v1/call", async (route) => {
        const req = route.request().postDataJSON();
        if (req.op === "design_tree" && !req.cached) await fresh.promise;
        if (req.op === "assignable") {
          if (modelReply === "empty") {
            return route.fulfill({ json: { ok: { kind: "assignable", owners: [
              { machine: "labA", connected: true, profiles: [] },
              { machine: "labB", connected: true, profiles: ["other-model"] },
            ], valid: null } } });
          }
          if (modelReply === "failed") return route.abort("failed");
          await assignable.promise;
        }
        await route.continue();
      }],
    });
    try {
      await p.getByTestId("design-body").getByText("층 이벤트").first().waitFor();
      assert.equal(await p.getByTestId("design-handoff").count(), 3, "넘기지 않은 계획만(캠프 지도·상점 위치·층 이벤트)");
      const openHandoff = () => p.getByTestId("design-handoff").first().click();
      const first = p.getByTestId("design-hand-dialog");
      const firstModel = first.getByTestId("design-hand-model");
      const modelStatus = first.getByRole("status");
      const closeHandoff = () => first.getByRole("button", { name: "닫기" }).click();
      await openHandoff();
      await modelStatus.filter({ hasText: "모델 불러오는 중" }).waitFor();
      assert.equal(await firstModel.locator('option[value=""]').textContent(), "프로젝트 기본 · labA");
      assignable.resolve();
      await firstModel.locator('option[value="fake"]').waitFor({ state: "attached" });
      assert.deepEqual(await firstModel.locator("option").evaluateAll((options) => options.map((option) => option.value)), ["", "fake"]);
      await closeHandoff();

      modelReply = "empty";
      await openHandoff();
      await modelStatus.filter({ hasText: "선택할 수 있는 모델이 없습니다" }).waitFor();
      assert.equal(await firstModel.locator("option").count(), 1, "다른 머신의 모델을 고르지 않는다");
      await closeHandoff();

      modelReply = "failed";
      await openHandoff();
      await modelStatus.filter({ hasText: "모델 목록을 불러오지 못했습니다" }).waitFor();
      await closeHandoff();

      modelReply = "live";
      fresh.resolve();
      // 창을 연 뒤 그 문서가 바뀌면 맡기지 않는다: 창은 연 때의 판으로 보내고 host가 다시 본다
      await openHandoff();
      await p.getByTestId("design-hand-main").waitFor();
      assert.match(await p.getByTestId("design-hand-main").textContent(), /선택한 블록/);
      const now = aaa("plan", "show", "app", "docs/exit.md").doc;
      landFixture(now.text.replace("- 11-16: 통로를", "- 11-16: 상한 표지를 정리함.\n- 11-16: 통로를"));
      await p.getByTestId("design-hand-submit").click();
      await first.locator(".confirm-error").first().waitFor();
      assert.match(await first.locator(".confirm-error").first().textContent(), /그 사이 main 문서가 바뀌었다/);
      assert.ok(!aaa("task", "list").tasks.some((t) => t.title === "캠프 지도"), "할 일을 만들지 않았다");
      await first.getByRole("button", { name: "닫기" }).click();
      await first.waitFor({ state: "detached" });
      await p.getByTestId("design-doc-reload").click();
      await domIs(p, "새 판", () => !!document.querySelector("[data-testid=design-body]")?.textContent.includes("상한 표지를 정리함"));
      await p.locator("[data-testid=design-look][data-look=left]").click();
      const checks = p.getByTestId("design-left-check");
      assert.equal(await checks.count(), 3, "미정·넘긴 줄은 고르지 않는다");
      await checks.nth(0).check();
      await checks.nth(2).check();
      await p.getByTestId("design-left-handoff").click();
      const dialog = p.getByTestId("design-hand-dialog");
      await dialog.waitFor();
      const title = "상한 — 캠프 지도 · 층 이벤트";
      assert.equal(await p.getByTestId("design-hand-title").inputValue(), title);
      assert.ok(await p.getByTestId("design-hand-reserve").isChecked(), "예약이 기본(q-3957 2A)");
      await dialog.getByTestId("design-hand-model").selectOption("fake");
      const before = mainHead();
      await p.getByTestId("design-hand-submit").click();
      await dialog.waitFor({ state: "detached" });
      const task = aaa("task", "list").tasks.find((t) => t.title === title);
      const id = task?.short_id;
      assert.ok(id, "할 일 하나");
      assert.equal(task.profile, "fake");
      assert.equal(task.machine, "labA");
      assert.equal(mainHead(), before);
      await p.locator("[data-testid=design-look][data-look=all]").click();
      await domIs(p, "넘긴 두 블록", (task) => document.querySelectorAll(`[data-testid=design-callout][data-task="${task}"]`).length === 2, id);
      assert.equal(await p.locator(".design-pending").count(), 2);
      assert.equal(await p.getByTestId("design-handoff").count(), 1);
    } finally {
      fresh.resolve();
      assignable.resolve();
      await p.context().close();
    }
  });

  for (const name of ["chromium", "webkit"]) await step(`휴대폰: 트리·문서 칸 ${name} 390px`, async () => {
    const wk = name === "webkit" ? await webkit.launch({ headless: true }) : null;
    const { page: p } = await pairPage(wk ?? browser, aaa("web", "pair").code, { width: 390, height: 844 });
    try {
      await p.goto(`${new URL(p.url()).origin}/#design=${encodeURIComponent("app/docs/loop/README.md")}`);
      await p.getByTestId("design-body").waitFor();
      await p.getByTestId("design-history").waitFor();
      if (name === "chromium") await shot(p, "design-main-phone-doc");
      assert.equal(await p.getByTestId("design-page-chat").count(), 0);
      await p.getByTestId("design-page-tree").click();
      await p.locator('[data-testid="design-repo"][data-repo="app"]').last().waitFor();
    } finally {
      await p.context().close();
      await wk?.close();
    }
  });
}

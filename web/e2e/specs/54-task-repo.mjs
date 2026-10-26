import { taskContent } from "../lib/task-content.mjs";
// task 저장소 칸(plan §8.330, 사용자 10-20 「506에 workspace 추가」): 할 일 폼에서 project.toml repos 이름이나 「작업 폴더」를 고르면
// task 줄 owner 뒤에 `[이름]`으로 적는다. 「바로 시작」은 그 저장소로 고정하고 그 저장소가 있는 머신을 기본으로 고른다(덜 붐비는
// 머신 고르기는 m13 `task_repo_field_picks_machine_and_path`가 두 머신으로 본다). 칸이 빈 task는 저장소나 작업 폴더를 고르기 전에는
// 시작하지 못하고, 고른 저장소로 띄우되 그 값은 task 줄에 적지 않는다. 모델·머신 칸(plan §8.335, 사용자 10-20 「모델과 머신 따로
// 설정하게」)은 같은 폼에서 따로 고치고 표기 `(모델@머신)` 그대로 적는다. 시간은 기다리지 않는다: 화면 표지(`data-tasks-sha`·
// `data-start-state`)와 파일 글을 본다.
import fs from "node:fs";
import crypto from "node:crypto";
import { execFileSync } from "node:child_process";
import { root, aaa, until, step, browser, domIs, shot } from "../lib/harness.mjs";
import { pairDevice } from "../lib/devices.mjs";

const file = () => `${root}/projects/demo/TASKS.md`;
const sha = (text) => crypto.createHash("sha256").update(text).digest("hex");
const read = () => fs.readFileSync(file(), "utf8");
const write = (text) => {
  fs.writeFileSync(file(), text);
  // 화면 관측 전에 식별 주석 이주를 마친 fixture를 준비한다.
  return aaa("task", "list").sha256;
};
const START = `# Tasks

## Backlog
- [ ] 저장소 일 (fake@labA)
- [ ] 빈 칸 일 (fake@labA)
- [ ] 모델 일
`;
const shaIs = (p, want) => domIs(p, `할 일 hash ${want.slice(0, 8)}`, (w) => document.querySelector('[data-testid="tasks-view"]')?.dataset.tasksSha === w, want);
const row = (p, title) => p.locator(`.task-row[data-title="${title}"]`);
const pick = (p, title) => row(p, title).locator(".task-pick").click();
const ready = (p) => domIs(p, "바로 시작 값", () => document.querySelector('[data-testid="task-start"] form')?.dataset.startState === "ready");

export default async function ({ page }) {
  const repo = `${root}/app`;
  await step("할 일 폼: 저장소를 고르면 task 줄 owner 뒤에 [이름]으로 적고 목록 줄에 칩으로 보인다", async () => {
    fs.mkdirSync(repo);
    const git = (...args) => execFileSync("git", ["-C", repo, "-c", "user.name=Test", "-c", "user.email=test@example.com", ...args], { stdio: "pipe" });
    git("init", "-q", "-b", "main");
    fs.writeFileSync(`${repo}/README.md`, "app\n");
    git("add", ".");
    git("commit", "-qm", "fixture");
    aaa("project", "create", "demo");
    aaa("project", "set", "demo", "thread_profile", "fake");
    aaa("project", "set", "demo", "repos.add", repo);
    const md = write(START);
    aaa("ticker", "tick");
    let releaseOwners;
    const gate = new Promise((resolve) => { releaseOwners = resolve; });
    const holdOwners = async (route) => {
      if (route.request().postDataJSON()?.op !== "assignable") return route.fallback();
      await gate;
      await route.continue();
    };
    const form = page.getByTestId("task-form");
    await page.route("**/api/v1/call", holdOwners);
    try {
      await page.getByTestId("activity-tasks").click();
      await shaIs(page, md);
      const requested = page.waitForRequest((r) => r.url().endsWith("/api/v1/call") && r.postDataJSON()?.op === "assignable");
      await pick(page, "저장소 일");
      const request = await requested;
      const options = await form.getByTestId("task-field-repo").locator("option").allTextContents();
      if (JSON.stringify(options) !== JSON.stringify(["정하지 않음", "app", "작업 폴더(저장소 없음)"])) throw new Error(`저장소 고르기 목록: ${options}`);
      // owner 목록의 DOM 반영 직후 change를 보낸다. 고정 sleep 없이 passive effect와 겹치는 선택을 재현한다.
      await page.evaluate(() => {
        const observer = new MutationObserver(() => {
          const machine = document.querySelector('[data-testid="task-field-machine"] option[value="labA"]');
          if (machine?.textContent !== "labA") return;
          observer.disconnect();
          const select = document.querySelector('[data-testid="task-field-repo"]');
          select.value = "app";
          select.dispatchEvent(new Event("change", { bubbles: true }));
          select.dataset.ownerBoundary = "selected";
        });
        observer.observe(document, { subtree: true, childList: true, characterData: true });
      });
      releaseOwners();
      const response = await request.response();
      if (!response?.ok()) throw new Error("owner 조회 실패");
      await response.finished();
      await domIs(page, "owner 목록을 그린 직후 저장소 선택", () => document.querySelector('[data-testid="task-field-repo"]')?.dataset.ownerBoundary === "selected");
      await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
      if (await form.getByTestId("task-field-repo").inputValue() !== "app") throw new Error("owner 응답이 고른 저장소를 초기화했다");
    } finally {
      releaseOwners();
      await page.unroute("**/api/v1/call", holdOwners);
    }
    await shot(page, "repo-desktop-form");
    // 속성 칸은 고르면 바로 저장한다(사용자 10-26 q-1382 3B): 「저장」 없이 파일에 쓰인다
    await until("저장소 칸", () => taskContent(read()).includes("- [ ] 저장소 일 (fake@labA) [app]\n"));
    await shaIs(page, sha(read()));
    const chip = await row(page, "저장소 일").locator(".task-repo").textContent();
    if (chip?.trim() !== "app") throw new Error(`저장소 칩: ${chip}`);
  });

  await step("모델·머신 칸: 담당 agent의 모델과 머신을 따로 고치고 다른 칸은 그대로, 목록 줄에 칩 둘, 바로 시작은 정한 칸만 잠근다", async () => {
    await pick(page, "모델 일");
    const form = page.getByTestId("task-form");
    const value = (id) => form.getByTestId(id).inputValue();
    if ((await value("task-field-owner")) !== "agent" || (await value("task-field-profile")) !== "" || (await value("task-field-machine")) !== "") throw new Error("미할당 task는 agent에 두 칸이 비어야 한다");
    await domIs(page, "모델 목록", () => [...document.querySelectorAll('[data-testid="task-field-profile"] option')].some((o) => o.value === "fake"));
    await shot(page, "who-desktop-unassigned");
    // 속성 칸은 고르면 바로 저장한다(q-1382 3B). 저장 뒤 폼은 손대지 않은 폼이라 「저장」 단추가 없다
    const save = async (want) => {
      const before = sha(read());
      await until(want, () => taskContent(read()).includes(`- [ ] ${want}\n`));
      await domIs(page, "새 판", (b) => document.querySelector('[data-testid="tasks-view"]')?.dataset.tasksSha !== b && document.querySelector('[data-task-saving="true"]') === null, before);
      if (await form.getByTestId("task-save").count()) throw new Error("바로 저장한 뒤에도 저장 단추가 남았다");
    };
    await form.getByTestId("task-field-machine").selectOption("labA");
    await save("모델 일 (@labA)");
    await form.getByTestId("task-field-profile").selectOption("fake");
    await save("모델 일 (fake@labA)");
    const r = row(page, "모델 일");
    const chips = [await r.locator(".task-model").textContent(), await r.locator(".task-machine").textContent(), await r.locator(".task-owner").count()];
    if (JSON.stringify(chips) !== JSON.stringify(["fake", "labA", 0])) throw new Error(`목록 줄 칩: ${chips}`);
    await shot(page, "who-desktop-form");
    // 머신만 비우면 모델은 남는다. 바로 시작은 모델만 잠그고 머신은 덜 붐비는 곳을 기본으로 둔다
    await form.getByTestId("task-field-machine").selectOption("");
    await save("모델 일 (fake)");
    if ((await r.locator(".task-machine").count()) !== 0) throw new Error("비운 머신 칩이 남았다");
    await page.getByTestId("task-detail").getByTestId("task-handoff-start").click();
    await ready(page);
    const profile = page.getByTestId("task-start-profile");
    if ((await profile.inputValue()) !== "fake" || !(await profile.isDisabled())) throw new Error("task 모델로 잠그지 않았다");
    await page.getByTestId("task-start").getByRole("button", { name: "취소" }).click();
    // 사람 할 일로 바꾸면 모델·머신 칸은 숨고 담당 칩 하나다
    await form.getByTestId("task-field-owner").selectOption("me");
    if ((await form.getByTestId("task-field-profile").count()) !== 0) throw new Error("나(me)인데 모델 칸이 보인다");
    await save("모델 일 (me)");
    if ((await r.locator(".task-owner").textContent())?.trim() !== "나") throw new Error("담당 칩");
  });

  await step("바로 시작: task 저장소로 고정하고 그 저장소가 있는 머신으로 worktree를 띄운다", async () => {
    await pick(page, "저장소 일");
    await page.getByTestId("task-detail").getByTestId("task-handoff-start").click();
    await ready(page);
    const repoSelect = page.getByTestId("task-start-repo");
    if ((await repoSelect.inputValue()) !== "app" || !(await repoSelect.isDisabled())) throw new Error("task 저장소로 고정하지 않았다");
    if ((await page.getByTestId("task-start-machine").inputValue()) !== "labA") throw new Error("저장소가 있는 머신을 고르지 않았다");
    await shot(page, "repo-desktop-start");
    await page.getByTestId("task-start-go").click();
    await page.getByTestId("task-started").waitFor();
    const id = await until("이은 thread", () => /저장소 일 .* · (t-\d{4,})$/m.exec(taskContent(read()))?.[1]);
    const t = aaa("thread", "show", "demo", id);
    // project.toml에는 실제 경로로 적힌다(/tmp → /private/tmp)
    if (t.thread_kind !== "worktree" || t.repo !== fs.realpathSync(repo) || t.machine !== "labA") throw new Error(`저장소로 띄우지 않았다: ${t.thread_kind} ${t.repo} ${t.machine}`);
  });

  await step("칸이 빈 task: 저장소나 작업 폴더를 고르기 전에는 시작하지 못하고, 고른 저장소로 띄우되 task 줄에 적지 않는다(iPhone)", async () => {
    const before = read();
    const { page: p } = await pairDevice(browser, aaa("web", "pair").code, "iphone", { hash: "#tasks" });
    await shaIs(p, sha(before));
    await pick(p, "빈 칸 일");
    await shot(p, "who-iphone-form");
    await p.getByTestId("task-handoff-start").click();
    await ready(p);
    if ((await p.getByTestId("task-start-repo").inputValue()) !== "") throw new Error("빈 칸인데 저장소를 골라 두었다");
    if (!(await p.getByTestId("task-start-go").isDisabled())) throw new Error("고르기 전에 시작할 수 있다");
    await shot(p, "repo-iphone-start-pick");
    await p.getByTestId("task-start-repo").selectOption("작업 폴더");
    if (await p.getByTestId("task-start-go").isDisabled()) throw new Error("작업 폴더를 골라도 시작할 수 없다");
    await p.getByTestId("task-start-repo").selectOption("app");
    await p.getByTestId("task-start-go").click();
    await p.getByTestId("task-started").waitFor();
    const id = await until("이은 thread", () => /빈 칸 일 \(fake@labA\) · (t-\d{4,})$/m.exec(taskContent(read()))?.[1]);
    const t = aaa("thread", "show", "demo", id);
    if (t.thread_kind !== "worktree" || t.repo !== fs.realpathSync(repo)) throw new Error(`고른 저장소로 띄우지 않았다: ${t.thread_kind} ${t.repo}`);
    if (taskContent(read()).includes("빈 칸 일 (fake@labA) [")) throw new Error("고른 값을 task 줄에 적었다");
    await p.close();
  });
}

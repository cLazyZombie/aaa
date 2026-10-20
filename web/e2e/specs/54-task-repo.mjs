// task 저장소 칸(plan §8.330, 사용자 10-20 「506에 workspace 추가」): 할 일 폼에서 project.toml repos 이름이나 「작업 폴더」를 고르면
// task 줄 owner 뒤에 `[이름]`으로 적는다. 「바로 시작」은 그 저장소로 고정하고 그 저장소가 있는 머신을 기본으로 고른다(덜 붐비는
// 머신 고르기는 m13 `task_repo_field_picks_machine_and_path`가 두 머신으로 본다). 칸이 빈 task는 저장소나 작업 폴더를 고르기 전에는
// 시작하지 못하고, 고른 저장소로 띄우되 그 값은 task 줄에 적지 않는다. 시간은 기다리지 않는다: 화면 표지(`data-tasks-sha`·`data-start-state`)와 파일 글을 본다.
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
  return sha(text);
};
const START = `# Tasks

## Backlog
- [ ] 저장소 일 (fake@labA)
- [ ] 빈 칸 일 (fake@labA)
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
    await page.getByTestId("activity-tasks").click();
    await shaIs(page, md);
    await pick(page, "저장소 일");
    const form = page.getByTestId("task-form");
    const options = await form.getByTestId("task-field-repo").locator("option").allTextContents();
    if (JSON.stringify(options) !== JSON.stringify(["정하지 않음(시작할 때 고른다)", "app", "작업 폴더(저장소 없음)"])) throw new Error(`저장소 고르기 목록: ${options}`);
    await form.getByTestId("task-field-repo").selectOption("app");
    await shot(page, "repo-desktop-form");
    await form.getByTestId("task-save").click();
    await until("저장소 칸", () => read().includes("- [ ] 저장소 일 (fake@labA) [app]\n"));
    await shaIs(page, sha(read()));
    const chip = await row(page, "저장소 일").locator(".task-repo").textContent();
    if (chip?.trim() !== "app") throw new Error(`저장소 칩: ${chip}`);
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
    const id = await until("이은 thread", () => /저장소 일 .* · (t-\d{4})$/m.exec(read())?.[1]);
    const t = aaa("thread", "show", "demo", id);
    // project.toml에는 실제 경로로 적힌다(/tmp → /private/tmp)
    if (t.thread_kind !== "worktree" || t.repo !== fs.realpathSync(repo) || t.machine !== "labA") throw new Error(`저장소로 띄우지 않았다: ${t.thread_kind} ${t.repo} ${t.machine}`);
  });

  await step("칸이 빈 task: 저장소나 작업 폴더를 고르기 전에는 시작하지 못하고, 고른 저장소로 띄우되 task 줄에 적지 않는다(iPhone)", async () => {
    const before = read();
    const { page: p } = await pairDevice(browser, aaa("web", "pair").code, "iphone", { hash: "#tasks" });
    await shaIs(p, sha(before));
    await pick(p, "빈 칸 일");
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
    const id = await until("이은 thread", () => /빈 칸 일 \(fake@labA\) · (t-\d{4})$/m.exec(read())?.[1]);
    const t = aaa("thread", "show", "demo", id);
    if (t.thread_kind !== "worktree" || t.repo !== fs.realpathSync(repo)) throw new Error(`고른 저장소로 띄우지 않았다: ${t.thread_kind} ${t.repo}`);
    if (read().includes("빈 칸 일 (fake@labA) [")) throw new Error("고른 값을 task 줄에 적었다");
    await p.close();
  });
}

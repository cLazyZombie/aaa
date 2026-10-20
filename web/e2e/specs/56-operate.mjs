// 운영 화면(관리 GUI W4, plan §8.333): 설정 창 「저장소」·「이 aaa」, thread 관리(끝내기·다시 열기·다시 시작·brief·Next·요약),
// 활동 막대 「알림함」(inbox·흐름 사건·post), 정리 미리보기 → 확인 창 실행, 보기 권한 기기. 실제 host·agent·git 저장소로 본다.
// 끝내기 창은 host가 미리 읽은 커밋하지 않은 파일·병합되지 않은 변경·사본 상태로 「남기기」 기본을 정한다: 위험한 thread는 켠 채,
// 깨끗한 thread는 끈 채 열린다. 시간은 기다리지 않는다: 파일 글·thread 기록·화면 표지를 본다.
import fs from "node:fs";
import { execFileSync } from "node:child_process";
import { root, aaa, step, browser, pairPage, until, domIs, shot, openSettings, showPanel, inList } from "../lib/harness.mjs";

const settingsFile = () => `${root}/projects/demo/project.toml`;
const git = (dir, ...a) => execFileSync("git", ["-C", dir, "-c", "user.name=e2e", "-c", "user.email=e2e@example.com", ...a], { encoding: "utf8" }).trim();
const show = (id) => aaa("thread", "show", "demo", id);
const openInbox = () => aaa("inbox", "list", "demo", "--peek").items.filter((i) => !i.done);
/** thread 화면을 열고 그 관리 메뉴 항목을 고른다 */
async function manage(p, id, item) {
  await p.evaluate((h) => (location.hash = h), `#thread=demo%2F${id}`);
  await p.getByTestId("thread-view").waitFor();
  await domIs(p, `${id} 화면`, (id) => document.querySelector('[data-testid="thread-view"] .term-title strong')?.textContent?.endsWith(id), id);
  await p.getByTestId("thread-manage").click();
  // 데스크톱은 popover의 menuitem, 휴대폰은 아래 sheet의 단추(RowMenu)
  await p.locator('[role="menuitem"], .row-sheet-item').filter({ hasText: item }).first().click();
}
/** 창이 휴대폰 폭 안에 있고 옆으로 넘치지 않는다 */
const fits = (p, selector) => p.evaluate((sel) => {
  const el = document.querySelector(sel);
  const body = el?.querySelector(".modal-body") ?? el;
  const box = el?.getBoundingClientRect();
  return el && box.left >= 0 && box.right <= innerWidth + 0.5 && body.scrollWidth <= body.clientWidth + 1 ? true : JSON.stringify({ box, sw: body?.scrollWidth, cw: body?.clientWidth, w: innerWidth });
}, selector);
async function startRepoThread(repo, title) {
  fs.writeFileSync(`${root}/op-task.md`, `${title}\n`);
  const id = aaa("thread", "start", "demo", "--title", title, "--machine", "labA", "--profile", "fake", "--repo", repo, "--task-file", `${root}/op-task.md`).id;
  await until(`${id} 보고 회수`, () => show(id).report_hash);
  const cwd = show(id).cwd;
  // 가짜 agent가 보고 뒤 worktree에 쓰는 새 파일 둘(커밋하지 않은 파일)
  await until(`${id} 가짜 agent의 새 파일`, () => fs.existsSync(`${cwd}/src/app.ts`));
  return { id, cwd };
}

export default async function ({ page }) {
  const repo = `${root}/repos/opgame`;
  let risky = { id: "", cwd: "" };
  await step("저장소 페이지: 머신별 경로를 보고, 더하고, 제자리에서 고치고, 빼며, 할 일 저장소 칸과 같은 목록에 저장소 없는 작업 폴더가 끝에 있다", async () => {
    fs.mkdirSync(repo, { recursive: true });
    git(repo, "init", "-q", "-b", "main");
    fs.writeFileSync(`${repo}/README.md`, "# 게임\n");
    git(repo, "add", "-A");
    git(repo, "commit", "-qm", "first");
    aaa("project", "create", "demo");
    aaa("project", "set", "demo", "thread_profile", "fake");
    aaa("project", "set", "demo", "repos.add", "/srv/lib@mac-a");
    await page.setViewportSize({ width: 1280, height: 800 });
    const dialog = await openSettings(page, "저장소");
    const pane = dialog.getByTestId("settings-repos");
    await pane.getByTestId("repo-group-lib").waitFor();
    await pane.getByTestId("repo-add-path").fill(repo);
    await pane.getByTestId("repo-add-save").click();
    await pane.getByTestId("repo-group-opgame").waitFor();
    // 머신 없는 경로는 host 머신(labA)의 자리로 보인다
    const row = await pane.getByTestId("repo-group-opgame").innerText();
    if (!row.includes("labA") || !row.includes(repo)) throw new Error(`host 머신 저장소 줄: ${row}`);
    // 제자리 고치기: 차례(lib 다음 opgame)는 그대로, 머신을 붙인다
    await pane.getByRole("button", { name: "/srv/lib@mac-a 고치기" }).click();
    await pane.getByTestId("repo-edit-input").fill("/srv/lib2@mac-b");
    await pane.getByRole("button", { name: "저장", exact: true }).click();
    await until("고친 저장소", () => fs.readFileSync(settingsFile(), "utf8").includes('path = "/srv/lib2"\nmachine = "mac-b"'));
    const text = fs.readFileSync(settingsFile(), "utf8");
    if (text.indexOf("/srv/lib2") > text.indexOf(repo)) throw new Error(`고치기가 차례를 바꿨다:\n${text}`);
    await pane.getByTestId("repo-group-lib2").waitFor();
    if (!(await pane.getByTestId("repo-workdir").innerText()).includes("작업 폴더(저장소 없음)")) throw new Error("작업 폴더 줄");
    await shot(page, "56-repos-desktop");
    // 빼기는 확인 창을 거친다
    await pane.getByRole("button", { name: "/srv/lib2@mac-b 빼기" }).click();
    await page.getByRole("alertdialog", { name: "저장소 빼기" }).getByRole("button", { name: "빼기" }).click();
    await pane.getByTestId("repo-group-lib2").waitFor({ state: "detached" });
    // 할 일 저장소 칸과 같은 목록(task board `repos`)
    const board = aaa("task", "list");
    if (JSON.stringify(board.repos) !== JSON.stringify(["opgame"])) throw new Error(`할 일 저장소 목록: ${JSON.stringify(board.repos)}`);
    await page.keyboard.press("Escape");
  });

  await step("이 aaa 페이지: 목표·숫자·mute를 고치면 설정 파일에 쓰고, 범위 밖 숫자는 저장하지 못하며, 확인 창으로 멈추고 다시 돌린다", async () => {
    const dialog = await openSettings(page, "이 aaa");
    const pane = dialog.getByTestId("settings-this-aaa");
    await pane.getByTestId("set-goal").fill("게임 만들기");
    await pane.getByTestId("set-goal-save").click();
    await until("목표", () => fs.readFileSync(settingsFile(), "utf8").includes('goal = "게임 만들기"'));
    await pane.getByTestId("set-max-parallel").fill("0");
    if (!(await pane.getByTestId("set-max-parallel-save").isDisabled())) throw new Error("범위 밖 숫자를 저장할 수 있다");
    await pane.getByTestId("set-max-parallel").fill("3");
    await pane.getByTestId("set-max-parallel-save").click();
    await until("동시 thread 경고", () => fs.readFileSync(settingsFile(), "utf8").includes("max_parallel_threads = 3"));
    await pane.getByRole("switch", { name: "밖으로 가는 알림 끄기" }).click();
    await until("mute", () => fs.readFileSync(settingsFile(), "utf8").includes("mute = true"));
    await domIs(page, "mute 켬 표시", () => document.querySelector('[role="switch"][aria-label="밖으로 가는 알림 끄기"]')?.getAttribute("aria-checked") === "true");
    await pane.getByTestId("project-pause").click();
    await page.getByRole("alertdialog", { name: "멈추기" }).getByRole("button", { name: "멈추기" }).click();
    await until("멈춤", () => aaa("project", "list").projects.find((p) => p.name === "demo")?.status === "paused");
    await domIs(page, "멈춤 표시", () => document.querySelector('[data-testid="project-pause"]')?.textContent === "다시 돌리기");
    await shot(page, "56-this-aaa-desktop");
    await pane.getByTestId("project-pause").click();
    await page.getByRole("alertdialog", { name: "다시 돌리기" }).getByRole("button", { name: "다시 돌리기" }).click();
    await until("다시 돌림", () => aaa("project", "list").projects.find((p) => p.name === "demo")?.status === "active");
    aaa("project", "set", "demo", "mute", "false");
    await page.keyboard.press("Escape");
  });

  await step("위험한 thread 끝내기: 커밋하지 않은 파일·병합되지 않은 커밋·사본에 빠진 것을 보이고 남기기를 켠 채 열리며, 끝내도 worktree와 branch가 남는다", async () => {
    risky = await startRepoThread(repo, "위험한 끝내기");
    const { id, cwd } = risky;
    fs.writeFileSync(`${cwd}/README.md`, "# 게임\n\n고친 글\n");
    git(cwd, "commit", "-qam", "branch change");
    // 사본에 빠진 것: 결과물의 symlink는 복사하지 않는다. 보고를 다시 써 사본을 다시 받게 한다
    const work = `${cwd}/.aaa/demo-${id}`;
    fs.symlinkSync(`${work}/report.md`, `${work}/library/link`);
    fs.appendFileSync(`${work}/report.md`, "\n- 다시 씀\n");
    await until(`${id} 사본에 빠진 것`, () => show(id).copy === "partial");
    await manage(page, id, "끝내기");
    const sheet = page.getByTestId("resolve-dialog");
    await sheet.getByTestId("resolve-facts").waitFor();
    const risks = await sheet.getByTestId("resolve-risks").innerText();
    for (const want of ["커밋하지 않은 파일 2개", "병합되지 않은 변경", "사본에 빠진 것"]) if (!risks.includes(want)) throw new Error(`위험 ${want}: ${risks}`);
    if (!(await sheet.getByTestId("resolve-keep").isChecked())) throw new Error("위험한데 남기기가 꺼져 있다");
    if (show(id).status !== "open") throw new Error("미리보기가 끝냈다");
    await shot(page, "56-resolve-risky-desktop");
    await sheet.getByTestId("resolve-confirm").click();
    await sheet.waitFor({ state: "detached" });
    await until(`${id} 끝남`, () => show(id).status === "resolved");
    if (!fs.existsSync(`${cwd}/src/app.ts`)) throw new Error("남기기로 끝냈는데 worktree가 지워졌다");
    if (!git(repo, "branch", "--list", `aaa/demo/${id}*`)) throw new Error("branch가 지워졌다");
  });

  await step("깨끗한 thread 끝내기: 잃을 것이 없으면 남기기를 끈 채 열리고, 끝내면 worktree를 지운다", async () => {
    const { id, cwd } = await startRepoThread(repo, "깨끗한 끝내기");
    fs.rmSync(`${cwd}/src`, { recursive: true });
    fs.rmSync(`${cwd}/notes`, { recursive: true });
    // 가짜 agent의 결과물 big.bin(2MB)은 복사 상한(1MB)을 넘어 사본이 partial이다: 치우고 보고를 다시 써 사본을 다 받는다
    const work = `${cwd}/.aaa/demo-${id}`;
    fs.rmSync(`${work}/library/big.bin`);
    fs.appendFileSync(`${work}/report.md`, "\n- 다시 씀\n");
    await until(`${id} 사본 다 옴`, () => show(id).copy === "complete");
    await manage(page, id, "끝내기");
    const sheet = page.getByTestId("resolve-dialog");
    await sheet.getByTestId("resolve-facts").waitFor();
    const facts = await sheet.getByTestId("resolve-facts").innerText();
    if (!/커밋하지 않은 파일\s*0개/.test(facts) || !/base 대비 바뀐 파일\s*0개/.test(facts)) throw new Error(`깨끗한 thread 미리보기: ${facts}`);
    if (await sheet.getByTestId("resolve-keep").isChecked()) throw new Error(`잃을 것이 없는데 남기기가 켜져 있다: ${await sheet.innerText()}`);
    await sheet.getByTestId("resolve-confirm").click();
    await until(`${id} 끝남`, () => show(id).status === "resolved");
    await until("지운 worktree", () => !fs.existsSync(cwd));
  });

  await step("다시 열기·다시 시작·Next·요약: 끝낸 thread는 다시 열기만, 도는 세션엔 다시 시작·받은 brief 다시 보내기가 없고, 멈추면 다시 시작, Next 줄을 더하고 보내며, 요약을 통째로 고친다", async () => {
    await manage(page, risky.id, "다시 열기");
    await page.getByRole("alertdialog", { name: "끝낸 thread 다시 열기" }).getByRole("button", { name: "다시 열기" }).click();
    await until("다시 연 thread", () => show(risky.id).status !== "resolved");
    fs.writeFileSync(`${root}/op-dir.md`, "작업 폴더 일\n");
    const id = aaa("thread", "start", "demo", "--title", "관리 동작", "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/op-dir.md`).id;
    await until(`${id} brief`, () => show(id).brief === "delivered" && show(id).report_hash);
    await manage(page, id, "Next");
    const next = page.getByTestId("next-dialog");
    await next.getByTestId("next-lines").waitFor();
    await next.getByTestId("next-add").fill("결과를 다시 본다");
    await next.getByRole("button", { name: "더하기" }).click();
    await until("더한 Next 줄", async () => (await next.getByTestId("next-lines").innerText()).includes("결과를 다시 본다"));
    await next.getByRole("button", { name: "보내기" }).first().click();
    await until("보낸 Next", async () => (await next.innerText()).includes("1번째 줄을 보냈다"));
    await page.keyboard.press("Escape");
    await next.waitFor({ state: "detached" });
    // 도는 세션은 다시 시작하지 않고, 받은 brief는 다시 보내지 않는다(host도 거부한다)
    await page.getByTestId("thread-manage").click();
    await page.getByRole("menuitem", { name: "Next" }).waitFor();
    for (const name of ["다시 시작", "brief 다시 보내기"]) if ((await page.getByRole("menuitem", { name }).count()) !== 0) throw new Error(`도는 세션에 ${name}`);
    await page.keyboard.press("Escape");
    await manage(page, id, "작업 요약 고치기");
    const sum = page.getByTestId("summary-dialog");
    await domIs(page, "요약 읽음", () => document.querySelector('[data-testid="summary-text"]')?.disabled === false);
    await sum.getByTestId("summary-text").fill("# 목표\n관리 동작 시험\n");
    await sum.getByTestId("summary-save").click();
    await sum.waitFor({ state: "detached" });
    await until("고친 요약", () => aaa("thread", "summary", "demo", id).text?.includes("관리 동작 시험"));
    aaa("pane", "kill", show(id).pane);
    await until(`${id} 세션 멈춤`, () => !aaa("pane", "list").panes.some((p) => p.id === show(id).pane && p.state === "running"));
    await manage(page, id, "다시 시작");
    const restart = page.getByTestId("restart-dialog");
    await restart.getByTestId("restart-confirm").click();
    await restart.waitFor({ state: "detached" });
    await until(`${id} 다시 띄움`, () => aaa("pane", "list").panes.some((p) => p.id === show(id).pane && p.state === "running"));
  });

  await step("알림함: 끝내지 않은 항목 수가 막대 배지이고, 한 건 끝내면 배지가 준다. 흐름 사건은 최근 것이 위, post 탭이 있다", async () => {
    const before = openInbox().length;
    if (before < 2) throw new Error(`끝낸 thread의 알림이 없다: ${before}`);
    await showPanel(page, "inbox");
    await domIs(page, `알림함 배지 ${before}`, (n) => document.querySelector('[data-testid="activity-badge-inbox"]')?.textContent === String(n), before);
    const panel = page.getByTestId("inbox-panel");
    await panel.getByTestId("inbox-list").waitFor();
    await panel.getByTestId("inbox-item").first().getByRole("button", { name: /끝내기$/ }).click();
    await until("한 건 끝냄", () => openInbox().length === before - 1);
    await domIs(page, `알림함 배지 ${before - 1}`, (n) => document.querySelector('[data-testid="activity-badge-inbox"]')?.textContent === String(n), before - 1);
    // 화면이 읽어도(peek) 「봤다」가 되지 않는다: coordinator 묶음 알림이 그대로 간다
    if (openInbox().some((i) => i.seen)) throw new Error("알림함 화면이 항목을 봤다로 바꿨다");
    await shot(page, "56-inbox-desktop");
    await panel.getByTestId("inbox-tab-events").click();
    await panel.getByTestId("event-list").waitFor();
    const first = await panel.getByTestId("event-item").first().innerText();
    const last = aaa("events", "demo").events.at(-1);
    if (!first.includes(last.kind)) throw new Error(`흐름 사건 맨 위가 최근 것이 아니다: ${first} / ${last.kind}`);
    await panel.getByTestId("inbox-tab-posts").click();
    await until("post 탭", async () => /post가 없다|post-item/.test(await panel.innerHTML()));
  });

  await step("보고서 정리와 끝난 thread 자리 정리: 미리 보고 확인 창으로 실행한다", async () => {
    const dialog = await openSettings(page, "이 aaa");
    const pane = dialog.getByTestId("settings-this-aaa");
    await pane.getByTestId("sweep-preview").click();
    await pane.getByTestId("sweep-notes").waitFor();
    await pane.getByTestId("prune-preview").click();
    await pane.getByTestId("prune-total").waitFor();
    // 기본 기한(30일) 안이라 지울 사본이 없다: 실행 단추를 그리지 않는다
    if ((await pane.getByTestId("prune-run").count()) !== 0) throw new Error("지울 것이 없는데 실행 단추가 있다");
    await shot(page, "56-clean-desktop");
    await page.keyboard.press("Escape");
  });

  await step("휴대폰: 알림함·끝내기 창·저장소 페이지가 폭 안에 들어간다", async () => {
    const { page: ph } = await pairPage(browser, aaa("web", "pair").code, { width: 390, height: 844 });
    await inList(ph, "activity-inbox");
    await showPanel(ph, "inbox");
    await ph.getByTestId("inbox-list").waitFor();
    const inbox = await fits(ph, '[data-testid="inbox-panel"]');
    if (inbox !== true) throw new Error(`알림함이 넘친다: ${inbox}`);
    await shot(ph, "56-inbox-phone");
    await manage(ph, risky.id, "끝내기");
    await ph.getByTestId("resolve-facts").waitFor();
    const ok = await fits(ph, '[data-testid="resolve-dialog"]');
    if (ok !== true) throw new Error(`끝내기 창이 넘친다: ${ok}`);
    await shot(ph, "56-resolve-phone");
    await ph.getByRole("button", { name: "취소" }).click();
    await ph.getByTestId("resolve-dialog").waitFor({ state: "detached" });
    const dialog = await openSettings(ph, "저장소");
    await dialog.getByTestId("repo-group-opgame").waitFor();
    const repos = await fits(ph, '[data-testid="settings-repos"]');
    if (repos !== true) throw new Error(`저장소 페이지가 넘친다: ${repos}`);
    await shot(ph, "56-repos-phone");
    await ph.context().close();
  });

  await step("같은 시작 두 번: 할 일 「바로 시작」을 두 번 눌러도 thread는 하나다", async () => {
    aaa("task", "add", "두 번 누름", "--owner", "fake@labA", "--repo", "opgame");
    await page.getByTestId("activity-tasks").click();
    await page.locator('.task-row[data-title="두 번 누름"]').locator(".task-pick").click();
    await page.getByTestId("task-detail").getByTestId("task-handoff-start").click();
    await domIs(page, "바로 시작 값", () => document.querySelector('[data-testid="task-start"] form')?.dataset.startState === "ready");
    await page.getByTestId("task-start-go").dblclick();
    await page.getByTestId("task-started").waitFor();
    await until("시작한 thread", () => aaa("thread", "list", "demo").threads.some((t) => t.title === "두 번 누름"));
    aaa("ticker", "tick");
    const same = aaa("thread", "list", "demo").threads.filter((t) => t.title === "두 번 누름");
    if (same.length !== 1) throw new Error(`두 번 눌러 thread ${same.length}개: ${same.map((t) => t.id)}`);
  });

  await step("보기 권한 기기: thread 관리·알림함 끝내기·post 탭·설정 쓰기 단추가 없다", async () => {
    const { page: rd } = await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 1280, height: 800 });
    await rd.evaluate((h) => (location.hash = h), `#thread=demo%2F${risky.id}`);
    await rd.getByTestId("thread-view").waitFor();
    if ((await rd.getByTestId("thread-manage").count()) !== 0) throw new Error("보기 기기에 thread 관리");
    await showPanel(rd, "inbox");
    await rd.getByTestId("inbox-list").waitFor();
    if ((await rd.getByTestId("inbox-panel").getByRole("button", { name: /끝내기$/ }).count()) !== 0) throw new Error("보기 기기에 끝내기");
    if ((await rd.getByTestId("inbox-tab-posts").count()) !== 0) throw new Error("보기 기기에 post 탭");
    const dialog = await openSettings(rd, "이 aaa");
    const pane = dialog.getByTestId("settings-this-aaa");
    await pane.getByTestId("set-goal").waitFor();
    if (!(await pane.getByTestId("set-goal").isDisabled()) || (await pane.getByTestId("project-pause").count()) !== 0 || (await pane.getByTestId("sweep-run").count()) !== 0) throw new Error("보기 기기에 설정 쓰기");
    await rd.context().close();
  });
}

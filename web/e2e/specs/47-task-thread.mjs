// 할 일 thread 연결과 자동 정리(T3, 사용자 10-20 「작업이 시작되면 (끝나면? 지금 정책에 따라) 삭제도 되고」, plan §8.316). 상세의
// 「바로 시작」은 host `thread_start_preview`로 채운 확인창에서 시작하고 그 task 줄 끝에 thread를 잇는다(`link_task`). 이어진 thread가
// 끝나면 ticker가 한 번 판정한다: 보고 [완료]면 줄을 지우고(자동 정리, 되살리기 알림 줄), 보고가 없으면 줄을 남기고 「확인 필요」다.
// 「coordinator에게 맡기기」는 coordinator 입력칸을 채워 그 pane을 연다. 「task 없는 열린 thread」는 「task에 잇기」·「task로 만들기」.
// 시간은 기다리지 않는다: 화면 표지(`data-tasks-sha`·`data-task-state`·`data-start-state`)와 파일 글, `ticker tick`을 본다.
import fs from "node:fs";
import crypto from "node:crypto";
import { root, aaa, until, step, browser, domIs, shot, openSettings } from "../lib/harness.mjs";
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
- [ ] 로그인 고치기 (fake@labA)
  테스트 먼저 쓴다
- [ ] 문서 정리 (fake@labA)
  사람을 기다림
- [ ] 배포 (me)
- [ ] 원격 일
## 나중
- [ ] 큰 일 W1~W3
  - [ ] W1 화면 (fake@labA)
`;
const shaIs = (p, want) => domIs(p, `할 일 hash ${want.slice(0, 8)}`, (w) => document.querySelector('[data-testid="tasks-view"]')?.dataset.tasksSha === w, want);
const row = (p, title) => p.locator(`.task-row[data-title="${title}"]`);
const stateIs = (p, title, want) =>
  domIs(p, `「${title}」 ${want}`, ([t, w]) => document.querySelector(`.task-row[data-title="${t}"]`)?.dataset.taskState === w, [title, want]);
const gone = (p, title) => domIs(p, `「${title}」 없음`, (t) => document.querySelector(`.task-row[data-title="${t}"]`) === null && document.querySelector('[data-testid="tasks-view"]') !== null, title);
const pick = (p, title) => row(p, title).locator(".task-pick").click();
/** 줄 끝 thread 번호(` · t-NNNN`)를 파일에서 */
const linked = (title) => new RegExp(`^\\s*- \\[ \\] ${title}.* · (t-\\d{4})$`, "m").exec(read())?.[1] ?? null;

/** 상세 「바로 시작」 → 확인창이 host 값으로 채워지면 시작. 시작한 thread 번호 */
async function startFromDetail(p, title, shotName) {
  await pick(p, title);
  const detail = p.getByTestId("task-detail");
  await detail.getByTestId("task-handoff-start").click();
  const dialog = p.getByTestId("task-start");
  await domIs(p, "바로 시작 값", () => document.querySelector('[data-testid="task-start"] form')?.dataset.startState === "ready");
  // 저장소 칸이 빈 task는 고르기 전에는 시작하지 못한다(plan §8.330): 이 spec의 task는 작업 폴더로 띄운다
  await dialog.getByTestId("task-start-repo").selectOption("작업 폴더");
  if (shotName) await shot(p, shotName);
  await dialog.getByTestId("task-start-go").click();
  await p.getByTestId("task-started").waitFor();
  const id = await until(`「${title}」 줄 끝 thread`, () => linked(title));
  return id;
}

export default async function ({ page }) {
  let md;
  let done = "";
  await step("바로 시작: 확인창이 owner의 profile·머신과 메모 붙은 할 일을 보이고, 시작하면 그 task 줄 끝에 thread를 잇는다", async () => {
    // 가짜 thread가 곧 턴을 끝내 앱 안 알림이 화면 위를 덮는다: 이 기기는 앱 안 알림을 끈다(43과 같다)
    const dialog = await openSettings(page, "알림");
    await dialog.getByRole("switch", { name: "앱 안에서" }).click();
    await page.keyboard.press("Escape");
    aaa("project", "create", "demo");
    aaa("project", "set", "demo", "thread_profile", "fake");
    md = write(START);
    aaa("ticker", "tick");
    await page.getByTestId("activity-tasks").click();
    await shaIs(page, md);
    await domIs(page, "편집 가능", () => document.querySelector('[data-testid="tasks-view"]')?.dataset.canEdit === "true");
    await pick(page, "로그인 고치기");
    await page.getByTestId("task-handoff").waitFor();
    await shot(page, "thread-desktop-handoff");
    // me task에는 맡기기가 없다(host `--from-task`가 거부한다)
    await pick(page, "배포");
    await page.getByTestId("task-form").waitFor();
    if (await page.getByTestId("task-handoff").count()) throw new Error("me task에 맡기기 단추가 있다");
    done = await startFromDetail(page, "로그인 고치기", "thread-desktop-start");
    const preview = aaa("thread", "show", "demo", done);
    if (preview.profile !== "fake" || preview.machine !== "labA") throw new Error(`owner로 정하지 않았다: ${preview.profile}@${preview.machine}`);
    const task = fs.readFileSync(`${root}/projects/demo/threads/${done}.task.md`, "utf8");
    if (!task.includes("테스트 먼저 쓴다")) throw new Error(`메모가 할 일에 붙지 않았다: ${task}`);
    await stateIs(page, "로그인 고치기", "run");
    await shot(page, "thread-desktop-running");
    // 확인창을 연 뒤 그 task 블록이 파일에서 바뀌면 시작하지 않는다(화면이 본 블록 hash, 사용자 10-20 결정)
    await pick(page, "원격 일");
    await page.getByTestId("task-detail").getByTestId("task-handoff-start").click();
    await domIs(page, "바로 시작 값", () => document.querySelector('[data-testid="task-start"] form')?.dataset.startState === "ready");
    const kept = read();
    const threads = aaa("thread", "list", "demo").threads.length;
    write(kept.replace("- [ ] 원격 일\n", "- [ ] 원격 일\n  그사이 더한 메모\n"));
    await page.getByTestId("task-start-repo").selectOption("작업 폴더");
    await page.getByTestId("task-start-go").click();
    const error = page.getByTestId("task-start-error");
    await error.waitFor();
    if (!(await error.textContent()).includes("그사이 바뀌었다")) throw new Error(`바뀐 블록 거부 글: ${await error.textContent()}`);
    if (aaa("thread", "list", "demo").threads.length !== threads || linked("원격 일") !== null) throw new Error("바뀐 블록으로 시작했다");
    await page.getByTestId("task-start").getByRole("button", { name: "취소" }).click();
    md = write(kept);
    aaa("ticker", "tick");
    await shaIs(page, md);
  });

  await step("자동 정리: 보고 [완료]로 끝난 thread의 task는 다음 tick에 지워지고 알림 줄 「되살리기」로 돌아오면 「확인 필요」다", async () => {
    await until(`${done} 보고`, () => aaa("thread", "show", "demo", done).report_hash);
    aaa("thread", "resolve", "demo", done);
    aaa("ticker", "tick");
    if (linked("로그인 고치기") !== null || read().includes("로그인 고치기")) throw new Error(`자동 정리하지 않았다:\n${read()}`);
    const log = aaa("task", "log", "--limit", "5").entries;
    const auto = log.find((e) => e.action === "auto_clean" && e.title === "로그인 고치기");
    if (!auto || auto.by !== "host 자동 정리" || auto.thread !== done) throw new Error(`자동 정리 기록: ${JSON.stringify(log)}`);
    await gone(page, "로그인 고치기");
    const note = page.getByTestId("task-auto-clean");
    await note.waitFor();
    await shot(page, "thread-desktop-auto-clean");
    await note.getByTestId("task-auto-clean-restore").click();
    await stateIs(page, "로그인 고치기", "check");
    await note.waitFor({ state: "detached" });
    const detail = page.getByTestId("task-detail");
    await detail.getByTestId("task-attention").waitFor();
    if (!(await detail.getByTestId("task-attention").textContent()).includes("되살렸다")) throw new Error("되살린 task의 까닭이 다르다");
    // 같은 thread는 한 번만 판정한다: 다시 tick해도 되살린 줄은 남는다
    aaa("ticker", "tick");
    if (!read().includes(`로그인 고치기 (fake@labA) · ${done}`)) throw new Error(`되살린 줄이 다시 지워졌다:\n${read()}`);
  });

  await step("확인 필요: 보고 없이 끝난 thread의 task는 줄이 남고 ⚠, 「연결만 풀기」면 대기로 돌아간다", async () => {
    const id = await startFromDetail(page, "문서 정리", null);
    // 가짜 agent는 「사람을 기다림」 brief에 보고 없이 사람을 기다린다고만 알린다
    await until(`${id} 브리프`, () => aaa("thread", "show", "demo", id).brief === "delivered");
    aaa("thread", "resolve", "demo", id);
    aaa("ticker", "tick");
    if (linked("문서 정리") !== id) throw new Error(`확인 필요 줄이 바뀌었다:\n${read()}`);
    await stateIs(page, "문서 정리", "check");
    await page.getByTestId("tasks-filter-check").click();
    await pick(page, "문서 정리");
    const detail = page.getByTestId("task-detail");
    await detail.getByTestId("task-attention").waitFor();
    if (!(await detail.getByTestId("task-attention").textContent()).includes("보고가 없다")) throw new Error("확인 필요 까닭이 다르다");
    await shot(page, "thread-desktop-check");
    const before = await page.getByTestId("tasks-view").getAttribute("data-tasks-sha");
    await detail.getByTestId("task-attention-unlink").click();
    await domIs(page, "새 판", (b) => document.querySelector('[data-testid="tasks-view"]')?.dataset.tasksSha !== b, before);
    if (!read().includes("- [ ] 문서 정리 (fake@labA)\n")) throw new Error(`연결을 풀지 않았다:\n${read()}`);
    await page.getByTestId("tasks-filter-all").click();
    await stateIs(page, "문서 정리", "wait");
  });

  await step("하위가 모두 끝난 상위: 하위 task를 자동 정리하면 상위에 그 thread를 이어 「확인 필요」로 올린다(상위는 지우지 않는다)", async () => {
    const id = await startFromDetail(page, "W1 화면", null);
    await until(`${id} 보고`, () => aaa("thread", "show", "demo", id).report_hash);
    aaa("thread", "resolve", "demo", id);
    aaa("ticker", "tick");
    if (read().includes("W1 화면") || linked("큰 일 W1~W3") !== id) throw new Error(`상위를 올리지 않았다:\n${read()}`);
    await stateIs(page, "큰 일 W1~W3", "check");
  });

  await step("task 없는 열린 thread: 「task에 잇기」로 있는 task에, 「task로 만들기」로 새 task에 잇는다", async () => {
    fs.writeFileSync(`${root}/task-47.md`, "사람을 기다림");
    const a = aaa("thread", "start", "demo", "--title", "따로 도는 일", "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/task-47.md`).id;
    const b = aaa("thread", "start", "demo", "--title", "새로 생긴 일", "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/task-47.md`).id;
    await page.getByTestId(`task-unlinked-row-${a}`).waitFor();
    await page.getByTestId(`task-unlinked-row-${b}`).waitFor();
    await shot(page, "thread-desktop-unlinked");
    await page.getByTestId(`task-unlinked-link-${a}`).click();
    const dialog = page.getByTestId("task-link");
    await dialog.getByTestId("task-link-search").fill("원격");
    await shot(page, "thread-desktop-link");
    await dialog.locator('[data-testid="task-link-pick"][data-title="원격 일"]').click();
    await dialog.waitFor({ state: "detached" });
    await until("원격 일에 이음", () => linked("원격 일") === a);
    await stateIs(page, "원격 일", "run");
    await page.getByTestId(`task-unlinked-make-${b}`).click();
    const detail = page.getByTestId("task-detail");
    await detail.getByTestId("task-new-thread").waitFor();
    if ((await detail.getByTestId("task-field-title").inputValue()) !== "새로 생긴 일") throw new Error("thread 제목을 채우지 않았다");
    await detail.getByTestId("task-save").click();
    await until("새 task에 이음", () => linked("새로 생긴 일") === b);
    await page.getByTestId("tasks-unlinked").waitFor({ state: "detached" });
    for (const id of [a, b]) aaa("thread", "resolve", "demo", id);
    aaa("ticker", "tick");
  });

  await step("coordinator에게 맡기기: coordinator 입력칸에 맡기는 글을 채우고 그 pane을 연다(보내지는 않는다)", async () => {
    const opened = aaa("project", "open", "demo", "--profile", "fake");
    await pick(page, "원격 일");
    // 원격 일은 끝난 thread와 이어져 확인 필요다: 「다시 맡기기」가 coordinator에게 맡긴다
    const detail = page.getByTestId("task-detail");
    await detail.getByTestId("task-attention-again").click();
    await domIs(page, "coordinator pane", (id) => location.hash === `#pane=${encodeURIComponent(id)}`, opened.pane.id);
    const drafts = await page.evaluate(
      ([m, mid, id]) => [localStorage.getItem(`herdr-web-ui:terminal-line:${m}:${id}`), localStorage.getItem(`herdr-web-ui:composer-draft:${mid}:${id}`)],
      [opened.pane.machine, opened.pane.machine_id, opened.pane.id],
    );
    for (const d of drafts) if (!d?.startsWith("「원격 일」 task를 thread로 다시 맡겨 줘")) throw new Error(`입력칸 초안: ${JSON.stringify(drafts)}`);
    await shot(page, "thread-desktop-coordinator");
  });

  await step("휴대폰·패드: 상세 시트의 맡기기와 확인 필요, 바로 시작 확인창과 한도 넘김 확인", async () => {
    aaa("project", "set", "demo", "max_parallel_threads", "1");
    fs.writeFileSync(`${root}/task-47b.md`, "사람을 기다림");
    const busy = aaa("thread", "start", "demo", "--title", "한도 채우는 일", "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/task-47b.md`).id;
    for (const kind of ["iphone", "ipad"]) {
      const { page: p } = await pairDevice(browser, aaa("web", "pair").code, kind, { hash: "#tasks" });
      await shaIs(p, sha(read()));
      await pick(p, "큰 일 W1~W3");
      await p.getByTestId("task-attention").waitFor();
      await shot(p, `thread-${kind}-check`);
      await p.getByTestId("task-detail-close").click();
      await pick(p, "배포");
      await p.getByTestId("task-form").waitFor();
      await p.getByTestId("task-detail-close").click();
      // 미할당 task: 맡기기 단추
      await pick(p, "문서 정리");
      await p.getByTestId("task-handoff").waitFor();
      await shot(p, `thread-${kind}-handoff`);
      await p.getByTestId("task-handoff-start").click();
      await domIs(p, "바로 시작 값", () => document.querySelector('[data-testid="task-start"] form')?.dataset.startState === "ready");
      await p.getByTestId("task-start-limit").waitFor();
      await p.getByTestId("task-start-repo").selectOption("작업 폴더");
      if (!(await p.getByTestId("task-start-go").isDisabled())) throw new Error("한도를 넘기는데 확인 없이 시작할 수 있다");
      await shot(p, `thread-${kind}-start-limit`);
      await p.getByTestId("task-start-over").check();
      if (await p.getByTestId("task-start-go").isDisabled()) throw new Error("한도 넘김을 확인해도 시작할 수 없다");
      await p.keyboard.press("Escape");
      await p.getByTestId("task-start").waitFor({ state: "detached" });
      await p.close();
    }
    aaa("thread", "resolve", "demo", busy);
  });
}

import { editTaskContent } from "../lib/task-content.mjs";
// 할 일 thread 연결과 자동 정리(T3, 사용자 10-20 「작업이 시작되면 (끝나면? 지금 정책에 따라) 삭제도 되고」, plan §8.316). 상세의
// 「바로 시작」은 host `thread_start_preview`로 채운 확인창에서 시작하고 그 task 줄 끝에 thread를 잇는다(`link_task`). 이어진 thread가
// 끝나면 ticker가 한 번 판정한다: 보고 [완료]면 줄을 지우고(자동 정리, 되살리기 알림 줄), 보고가 없으면 줄을 남기고 「확인 필요」다.
// 「coordinator에게 맡기기」는 coordinator 입력칸을 채워 그 pane을 연다. 「task 없는 열린 thread」는 「task에 잇기」·「task로 만들기」.
// 시간은 기다리지 않는다: 화면 표지(`data-tasks-sha`·`data-task-state`·`data-start-state`)와 파일 글, `ticker tick`을 본다.
import fs from "node:fs";
import crypto from "node:crypto";
import { execFileSync } from "node:child_process";
import { root, aaa, until, step, browser, domIs, shot, openSettings } from "../lib/harness.mjs";
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
const linked = (title) => new RegExp(`^\\s*- \\[ \\] ${title}.* · (t-\\d{4})(?: <!-- aaa-task:[A-Z0-9]{26} -->)?$`, "m").exec(read())?.[1] ?? null;

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
    write(editTaskContent(kept, "- [ ] 원격 일\n", "- [ ] 원격 일\n  그사이 더한 메모\n"));
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

  await step("완료 보관: 같은 ID·본문·결과를 보관하고 복원하면 실행 연결 없이 대기로 돌아온다", async () => {
    await until(`${done} 보고`, () => aaa("thread", "show", "demo", done).report_hash);
    aaa("thread", "resolve", "demo", done);
    aaa("ticker", "tick");
    if (read().includes("로그인 고치기")) throw new Error(`완료 보관하지 않았다:\n${read()}`);
    const log=aaa("task","log","--limit","5").entries;
    const completed=log.find(e=>e.action==="completed"&&e.title==="로그인 고치기");
    const stored=aaa("task","archive").entries.find(a=>a.task.title==="로그인 고치기");
    if(!completed||!stored||stored.task.thread!==done||!stored.body.includes("테스트 먼저 쓴다")||!stored.report)throw new Error(`완료 보관 계약: ${JSON.stringify({completed,stored})}`);
    await gone(page,"로그인 고치기");
    await page.getByTestId("task-detail-close").click();
    await page.getByTestId("task-archive-open").click();
    const archived=page.getByTestId(`archive-${stored.task_id}`);
    await archived.waitFor();
    await shot(page,"thread-desktop-archive");
    await archived.getByRole("button",{name:"활성 목록으로 복원"}).click();
    await archived.waitFor({state:"detached"});
    // 완료 보관은 기록 탭에서 연다. 복원한 활성 행은 할 일 탭에서 확인한다.
    await page.getByTestId("tasks-tab-tasks").click();
    await stateIs(page,"로그인 고치기","wait");
    const t=aaa("task","list").tasks.find(t=>t.title==="로그인 고치기");
    if(t.id!==stored.task_id||t.thread!==null||aaa("task","queue").entries.length)throw new Error(`복원 계약: ${JSON.stringify(t)}`);
    aaa("ticker","tick");
    if(!read().includes("로그인 고치기 (fake@labA) <!-- aaa-task:"))throw new Error(`복원한 ID를 잃었다:\n${read()}`);
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
    if (!read().includes("- [ ] 문서 정리 (fake@labA) <!-- aaa-task:")) throw new Error(`연결을 풀지 않았다:\n${read()}`);
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
    // 새 일반 기동은 자동 연결된다. 지운 할 일/옛 버전에서 온 thread의 복구 UI도 계속 검증한다.
    for (const [id, title] of [[a, "따로 도는 일"], [b, "새로 생긴 일"]]) {
      if (linked(title) !== id) throw new Error(`일반 기동의 할 일 자동 연결 누락: ${title}`);
      aaa("task", "rm", title);
    }
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
    const taskId = aaa("task", "list").tasks.find(t => t.title === "원격 일").short_id;
    for (const d of drafts) if (!d?.startsWith(`${taskId} task를 thread로 다시 맡겨 줘`)) throw new Error(`입력칸 초안: ${JSON.stringify(drafts)}`);
    await shot(page, "thread-desktop-coordinator");
  });

  await step("데스크톱·휴대폰·패드: 한도 무시 시작 재확인·출처 표지와 예약 대기 유지", async () => {
    aaa("project", "set", "demo", "max_parallel_threads", "1");
    fs.writeFileSync(`${root}/task-47b.md`, "사람을 기다림");
    const busy = aaa("thread", "start", "demo", "--title", "한도 채우는 일", "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/task-47b.md`).id;
    aaa("task", "add", "강제 동안 예약", "--profile", "fake", "--machine", "labA", "--repo", "작업 폴더", "--note", "사람을 기다림");
    aaa("task", "reserve", "강제 동안 예약");
    const waitingId = aaa("task", "queue").entries.find(q => q.task.title === "강제 동안 예약").task_id;
    for (const kind of ["desktop", "iphone", "ipad"]) {
      const title = `강제 시작 ${kind}`;
      aaa("task", "add", title, "--profile", "fake", "--machine", "labA", "--repo", "작업 폴더", "--note", "사람을 기다림");
      aaa("ticker", "tick");
      const p = kind === "desktop" ? page : (await pairDevice(browser, aaa("web", "pair").code, kind, { hash: "#tasks" })).page;
      if (kind === "desktop") {
        await p.getByTestId("activity-tasks").click();
        if (await p.getByTestId("task-detail").isVisible()) await p.getByTestId("task-detail-close").click();
      }
      await shaIs(p, sha(read()));
      await pick(p, "큰 일 W1~W3");
      await p.getByTestId("task-attention").waitFor();
      await shot(p, `thread-${kind}-check`);
      await p.getByTestId("task-detail-close").click();
      await pick(p, "배포");
      await p.getByTestId("task-form").waitFor();
      await p.getByTestId("task-detail-close").click();
      // 미할당 task: 맡기기 단추
      await pick(p, title);
      await p.getByTestId("task-handoff").waitFor();
      await shot(p, `thread-${kind}-handoff`);
      await p.getByTestId("task-handoff-start").click();
      await domIs(p, "바로 시작 값", () => document.querySelector('[data-testid="task-start"] form')?.dataset.startState === "ready");
      await p.getByTestId("task-start-limit").waitFor();

      if (!(await p.getByTestId("task-start-go").isDisabled())) throw new Error("한도를 넘기는데 확인 없이 시작할 수 있다");
      await shot(p, `thread-${kind}-start-limit`);
      const count = aaa("thread", "list", "demo").threads.length;
      await p.getByTestId("task-start-over").click();
      await p.getByTestId("task-start-over-confirm").waitFor();
      await shot(p, `thread-${kind}-over-confirm`);
      if (aaa("thread", "list", "demo").threads.length !== count) throw new Error("재확인 전에 기동했다");
      await p.getByRole("button", { name: "돌아가기", exact: true }).click();
      await p.getByTestId("task-start-over-confirm").waitFor({ state: "detached" });
      if (aaa("thread", "list", "demo").threads.length !== count) throw new Error("돌아가기에서 기동했다");
      await p.getByTestId("task-start-over").click();
      await p.getByTestId("task-start-over-go").click();
      await p.getByTestId("task-started").waitFor();
      const id = await until("강제 시작 task 연결", () => linked(title));
      const record = aaa("thread", "show", "demo", id);
      if (!record.over_limit?.actor.startsWith("device:") || !record.over_limit.at_ms) throw new Error(`강제 시작 출처 없음: ${JSON.stringify(record)}`);
      await p.getByTestId("task-start").waitFor({ state: "detached" });
      await p.keyboard.press("Escape");
      await p.getByTestId("task-detail").waitFor({ state: "detached" });
      await row(p, title).getByTestId("thread-over-limit").waitFor();
      await row(p, title).scrollIntoViewIfNeeded();
      await row(p, title).locator(".task-pick").click({ trial: true });
      await shot(row(p, title), `thread-${kind}-over-started`);
      aaa("ticker", "tick");
      const q = aaa("task", "queue").entries.find(q => q.task.title === "강제 동안 예약");
      if (q.state !== "queued" || q.thread) throw new Error("한도 초과 동안 예약을 기동했다");
      // 웹 시작 응답·task 연결은 비동기 기동의 접수다. 준비 작업이 끝나 brief를 전달한 뒤 한 번 정리한다.
      await until(`${id} 브리프`, () => aaa("thread", "show", "demo", id).brief === "delivered");
      aaa("thread", "resolve", "demo", id);
      if (kind !== "desktop") await p.close();
    }
    aaa("task", "unreserve", waitingId);
    aaa("thread", "resolve", "demo", busy);
  });

  await step("직접 병합: [착지]여도 origin/main 조상이면 완료 보관하고 답할 것에 넣지 않는다", async () => {
    const repo = `${root}/merged-app`;
    const origin = `${root}/merged-origin.git`;
    const git = (cwd, ...args) => execFileSync("git", ["-C", cwd, "-c", "user.name=Test", "-c", "user.email=test@example.com", ...args], { encoding: "utf8", stdio: "pipe" }).trim();
    fs.mkdirSync(repo);
    git(root, "init", "-q", "--bare", "-b", "main", origin);
    git(repo, "init", "-q", "-b", "main");
    git(repo, "commit", "--allow-empty", "-qm", "fixture");
    git(repo, "remote", "add", "origin", origin);
    git(repo, "push", "-qu", "origin", "main");
    fs.writeFileSync(`${root}/landing-task.md`, "사람을 기다림\n");
    const threads = [];
    for (const title of ["직접 병합 완료", "직접 미병합"]) {
      const t = aaa("thread", "start", "demo", "--title", title, "--profile", "fake", "--machine", "labA", "--repo", repo, "--task-file", `${root}/landing-task.md`);
      await until(`${title} brief`, () => aaa("thread", "show", "demo", t.id).brief === "delivered");
      const rec = aaa("thread", "show", "demo", t.id);
      git(rec.cwd, "commit", "--allow-empty", "-qm", title);
      if (title === "직접 병합 완료") {
        git(repo, "merge", "--no-ff", "-m", "병합: 시험", rec.branch);
        git(repo, "push", "-q", "origin", "main");
      }
      fs.writeFileSync(`${rec.work_dir}/report.md`, "[착지] 직접 병합 확인\n\n## Report\n결과\n\n## Next\n");
      if (linked(title) !== t.id) throw new Error(`직접 시작의 할 일 연결 누락: ${title}`);
      aaa("thread", "resolve", "demo", t.id, "--keep-worktree");
      threads.push(t.id);
    }
    aaa("ticker", "tick");
    await gone(page, "직접 병합 완료");
    await stateIs(page, "직접 미병합", "check");
    const archived = aaa("task", "archive").entries.find((a) => a.task.title === "직접 병합 완료");
    if (archived?.reason !== "origin/main 병합") throw new Error(`직접 병합 보관 근거: ${JSON.stringify(archived)}`);
    const notices = aaa("inbox", "list", "demo", "--all", "--peek").items.filter((i) => i.item_kind === "task" && i.subject === threads[0]);
    if (notices.length !== 1 || notices[0].event !== "task auto-cleaned") throw new Error(`병합 확인 알림: ${JSON.stringify(notices)}`);
    await page.getByTestId("activity-asks").click();
    await page.locator('[data-testid="ask-row"][data-key="t:직접 미병합"]').waitFor();
    if (await page.locator('[data-testid="ask-row"][data-key="t:직접 병합 완료"]').count()) throw new Error("병합된 할 일에 확인 카드가 있다");
    await shot(page, "thread-direct-merge-asks");
    await page.getByTestId("activity-tasks").click();
  });
}

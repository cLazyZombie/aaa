import { taskContent, editTaskContent } from "../lib/task-content.mjs";
// 할 일 편집(T2, 사용자 10-20 「필요하면 수정, 삭제 할수 있고」, plan §8.311). 웹이 host `task_edit`로 TASKS.md를 고친다: 바뀐 블록만
// 쓰고(나머지 바이트 그대로), 대상 블록 hash가 그사이 바뀌었으면 쓰지 않고 충돌 창을 띄운다. 지운 task는 기록에서 되살린다. 저장만으로는
// thread가 시작되지 않는다. 시간은 기다리지 않는다: 화면 표지 `data-tasks-sha`·`data-task-saving`과 파일 글을 본다.
import fs from "node:fs";
import crypto from "node:crypto";
import { root, aaa, step, browser, webkit, pairPage, domIs, shot, openSettings } from "../lib/harness.mjs";
import { pairDevice } from "../lib/devices.mjs";

const dir = () => `${root}/projects/demo`;
const file = () => `${dir()}/TASKS.md`;
const sha = (text) => crypto.createHash("sha256").update(text).digest("hex");
const read = () => fs.readFileSync(file(), "utf8");
const write = (text) => {
  fs.writeFileSync(file(), text);
  // 화면 관측 전에 식별 주석 이주를 마친 fixture를 준비한다.
  return aaa("task", "list").sha256;
};
const START = `# Tasks

## In progress
- [ ] 로그인 고치기 (me)
  - 메모: 테스트 먼저
## Backlog
나중 일은 memory/later.md에 모은다
- [ ] 정리(선택) (t-0101 S4 뒤, 사용자 지시 시)
  사용자 지시 뒤에 한다
- [ ] 배포 (me)
- [ ] 원격 일
- [ ] 문서 정리
<!-- 모르는 줄 -->
## 빈 목록
`;
const shaIs = (p, want) => domIs(p, `할 일 hash ${want.slice(0, 8)}`, (w) => document.querySelector('[data-testid="tasks-view"]')?.dataset.tasksSha === w, want);
/** 화면이 지금 판이 아닌 새 판을 그릴 때까지(저장 응답 또는 `tasks` 사건) 기다리고, 파일이 그 판인지 본다 */
const saved = async (p, before) => {
  await domIs(p, "새 판", (b) => {
    const v = document.querySelector('[data-testid="tasks-view"]');
    return v && v.dataset.tasksSha !== b && document.querySelector('[data-task-saving="true"]') === null;
  }, before);
  const text = read();
  const now = await p.getByTestId("tasks-view").getAttribute("data-tasks-sha");
  if (now !== sha(text)) throw new Error(`화면 판 ${now?.slice(0, 8)} ≠ 파일 판 ${sha(text).slice(0, 8)}`);
  return text;
};
const shaOf = (p) => p.getByTestId("tasks-view").getAttribute("data-tasks-sha");
const row = (p, title) => p.locator(`.task-row[data-title="${title}"]`);
const pickRow = (p, title) => row(p, title).locator(".task-pick").click();
/** 상세 자리: 화면 안 상자와 목록 칸(43-tasks와 같다) */
const detailBox = (p) =>
  p.evaluate(() => {
    const d = document.querySelector('[data-testid="task-detail"]')?.getBoundingClientRect();
    const l = document.querySelector('[data-testid="tasks-list"]')?.getBoundingClientRect();
    return d && l && { d: { left: d.left, right: d.right, top: d.top, bottom: d.bottom, width: d.width }, l: { left: l.left, width: l.width }, w: innerWidth, h: innerHeight, wide: document.documentElement.scrollWidth > innerWidth + 1 };
  });
const expectFile = (got, want, what) => {
  if (taskContent(got) !== taskContent(want)) throw new Error(`${what}: 파일이 다르다\n--- 기대\n${want}\n--- 실제\n${got}`);
};

export default async function ({ page }) {
  let md;
  await step("편집 폼: 새 task 더하기, 제목·owner(사람으로 저장) 바꾸기(목록 칸은 없다, §8.393). 파일은 바뀐 블록만 바뀐다", async () => {
    // 앱 안 알림이 도구줄을 덮지 않게(43-tasks와 같다)
    const dialog = await openSettings(page, "알림");
    await dialog.getByRole("switch", { name: "앱 안에서" }).click();
    await page.keyboard.press("Escape");
    aaa("project", "create", "demo");
    md = write(START);
    aaa("ticker", "tick");
    await page.getByTestId("activity-tasks").click();
    await shaIs(page, md);
    await domIs(page, "편집 가능", () => document.querySelector('[data-testid="tasks-view"]')?.dataset.canEdit === "true");
    await shot(page, "edit-desktop-list");
    // 새 task: 도구줄 「+ 할 일」 → 폼 → 더하기. Backlog 끝(마지막 글 줄 뒤)에 붙는다
    await page.getByTestId("tasks-add").click();
    const detail = page.getByTestId("task-detail");
    await detail.getByTestId("task-field-title").fill("새 일");
    await detail.getByTestId("task-field-owner").selectOption("me");
    await detail.getByTestId("task-field-notes").fill("첫 줄\n둘째 줄");
    await shot(page, "edit-desktop-new");
    let before = await shaOf(page);
    await detail.getByTestId("task-save").click();
    let text = await saved(page, before);
    let want = START.replace("<!-- 모르는 줄 -->\n", "<!-- 모르는 줄 -->\n- [ ] 새 일 (me)\n  첫 줄\n  둘째 줄\n");
    expectFile(text, want, "더하기");
    await domIs(page, "더한 task 상세", () => document.querySelector('[data-testid="task-detail"]')?.getAttribute("aria-label") === "할 일 새 일");
    // 있는 task: 제목·owner(사람)를 한 번에 고친다. 한 목록이라 폼에 목록 칸이 없고 자리는 그대로다
    await pickRow(page, "배포");
    await detail.getByTestId("task-field-title").fill("배포 준비");
    await detail.getByTestId("task-field-owner").selectOption("person");
    await detail.getByTestId("task-field-person").fill("Priya Rao");
    if ((await detail.getByTestId("task-field-list").count()) !== 0) throw new Error("폼에 목록 칸이 남았다");
    await shot(page, "edit-desktop-form");
    before = await shaOf(page);
    await detail.getByTestId("task-save").click();
    text = await saved(page, before);
    want = want.replace("- [ ] 배포 (me)\n", "- [ ] 배포 준비 (Priya Rao)\n");
    expectFile(text, want, "제자리 고치기");
    // 사람 이름을 「사람으로 저장」 없이 owner로 쓰면 거부하고 폼 안에 까닭을 보인다
    // 상세를 열며 시작한 owner 조회를 사람 선택 뒤에 돌려준다. 빈 사람 이름도 고치던 폼이다.
    let releaseOwners;
    const ownerGate = new Promise((resolve) => { releaseOwners = resolve; });
    const holdOwners = async (route) => {
      if (route.request().postDataJSON()?.op !== "assignable") return route.fallback();
      await ownerGate;
      await route.continue();
    };
    await page.route("**/api/v1/call", holdOwners);
    try {
      const requested = page.waitForRequest((r) => r.url().endsWith("/api/v1/call") && r.postDataJSON()?.op === "assignable");
      await pickRow(page, "원격 일");
      const request = await requested;
      await detail.getByTestId("task-field-owner").selectOption("person");
      await detail.getByTestId("task-field-person").waitFor();
      releaseOwners();
      const response = await request.response();
      if (!response?.ok()) throw new Error("사람 선택 뒤 owner 조회 실패");
      await response.finished();
      // 조회 응답 뒤 React가 폼을 다시 그리는 frame을 지난 다음에도 이름 칸이 남아야 한다.
      await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
      await detail.getByTestId("task-field-person").fill("me");
    } finally {
      releaseOwners();
      await page.unroute("**/api/v1/call", holdOwners);
    }
    await detail.getByTestId("task-save").click();
    await detail.getByTestId("task-form-error").waitFor();
    if (!(await detail.getByTestId("task-form-error").textContent()).includes("사람 이름으로 쓰지 않는다")) throw new Error(await detail.getByTestId("task-form-error").textContent());
    expectFile(read(), want, "거부한 저장은 파일을 바꾸지 않는다");
    md = sha(want);
  });

  await step("정리 제안: 형식 어긋남 task에 정규형 후보를 보이고 「이대로 저장」이 바꾼다(배지가 준다)", async () => {
    if ((await page.getByTestId("activity-badge-tasks").textContent()) !== "1") throw new Error("배지 1");
    await pickRow(page, "정리(선택) (t-0101 S4 뒤, 사용자 지시 시)");
    const s = page.getByTestId("task-suggestion");
    await s.waitFor();
    if (!(await s.textContent()).includes("- [ ] 정리")) throw new Error(await s.textContent());
    await shot(page, "edit-desktop-suggestion");
    const before = await shaOf(page);
    await s.getByTestId("task-suggestion-apply").click();
    const text = await saved(page, before);
    if (!taskContent(text).includes("- [ ] 정리\n  선택\n  t-0101 S4 뒤, 사용자 지시 시\n  사용자 지시 뒤에 한다\n")) throw new Error(text);
    await page.getByTestId("activity-badge-tasks").waitFor({ state: "detached" });
    md = sha(text);
  });

  await step("행 메뉴·끌기·Alt+↓: 한 목록 안 순서. 옛 `##` 머리를 넘어 옮기면 앞 이웃의 목록에 들고, 목록 머리 조작은 없다", async () => {
    const order = (text, list) => {
      const block = taskContent(text).split("## ").find((b) => b.startsWith(`${list}\n`)) ?? "";
      return block.split("\n").filter((l) => l.startsWith("- [")).map((l) => l.replace(/^- \[.\] /, "").replace(/ \(.*$/, ""));
    };
    const lists = (text) => `${order(text, "In progress").join("|")} / ${order(text, "Backlog").join("|")}`;
    // 「문서 정리」 ⋯ → 위로
    let before = await shaOf(page);
    await row(page, "문서 정리").locator("[data-testid^='task-menu-']").click();
    await shot(page, "edit-desktop-row-menu");
    await page.getByRole("menuitem", { name: "위로", exact: true }).click();
    let text = await saved(page, before);
    if (lists(text) !== "로그인 고치기 / 정리|배포 준비|문서 정리|원격 일|새 일") throw new Error(`위로 ${lists(text)}`);
    // 끌기: 「원격 일」 손잡이를 「정리」 위 절반에 놓으면 그 앞. 앞 이웃(로그인 고치기)과 목록이 갈리는 틈이라 앞 목록 끝에 든다
    before = await shaOf(page);
    const target = row(page, "정리");
    const box = await target.boundingBox();
    await row(page, "원격 일").locator(".task-grip").dragTo(target, { targetPosition: { x: 40, y: 3 } });
    text = await saved(page, before);
    if (lists(text) !== "로그인 고치기|원격 일 / 정리|배포 준비|문서 정리|새 일") throw new Error(`끌기 ${lists(text)} (${JSON.stringify(box)})`);
    // Alt+↓: 키보드로 한 칸 아래
    before = await shaOf(page);
    await row(page, "원격 일").locator(".task-pick").focus();
    await page.keyboard.press("Alt+ArrowDown");
    text = await saved(page, before);
    if (lists(text) !== "로그인 고치기 / 정리|원격 일|배포 준비|문서 정리|새 일") throw new Error(`Alt+↓ ${lists(text)}`);
    // 옛 목록 머리를 넘어 위로: 「정리」는 「로그인 고치기」 앞(In progress)으로
    before = await shaOf(page);
    await row(page, "정리").locator("[data-testid^='task-menu-']").click();
    await page.getByRole("menuitem", { name: "위로", exact: true }).click();
    text = await saved(page, before);
    if (lists(text) !== "정리|로그인 고치기 / 원격 일|배포 준비|문서 정리|새 일") throw new Error(`머리 넘어 위로 ${lists(text)}`);
    // 다시 한 칸 아래: 두 이웃의 목록이 다르면 앞 이웃(로그인 고치기)의 목록 끝에 남는다. 화면 차례는 로그인 고치기 → 정리 → 원격 일
    before = await shaOf(page);
    await row(page, "정리").locator(".task-pick").focus();
    await page.keyboard.press("Alt+ArrowDown");
    text = await saved(page, before);
    if (lists(text) !== "로그인 고치기|정리 / 원격 일|배포 준비|문서 정리|새 일") throw new Error(`머리 넘어 아래로 ${lists(text)}`);
    const shown = await page.getByTestId("tasks-rest").locator(".task-row").evaluateAll((els) => els.map((e) => e.dataset.title.replace(/ \(.*$/, "")));
    if (shown.slice(0, 3).join("|") !== "로그인 고치기|정리|원격 일") throw new Error(`화면 차례 ${shown}`);
    // 한 목록: 목록 머리·머리 메뉴·「새 목록」이 없고, 파일의 옛 머리(task 없는 「빈 목록」 포함)는 그대로 남는다
    for (const sel of ['[data-testid^="task-list-menu-"]', '[data-testid="tasks-list-add"]', ".task-list-toggle"]) {
      if ((await page.getByTestId("tasks-list").locator(sel).count()) !== 0) throw new Error(`목록 머리 조작 ${sel}`);
    }
    if (!taskContent(text).includes("## In progress\n") || !taskContent(text).endsWith("## 빈 목록\n")) throw new Error(text);
    md = sha(text);
  });

  await step("충돌: 두 브라우저가 같은 task를 고치면 늦게 저장한 쪽이 충돌 창(내 고침·지금 파일), 「최신으로 다시 열기」", async () => {
    const two = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 }, { hash: "#tasks" });
    try {
      const p2 = two.page;
      await shaIs(p2, md);
      await pickRow(page, "로그인 고치기");
      await pickRow(p2, "로그인 고치기");
      // 둘째가 먼저 폼을 고친다(아직 저장 안 함)
      await p2.getByTestId("task-field-title").fill("로그인 다시 고치기");
      // 다른 기기의 저장 전에 React가 둘째 폼을 실제로 dirty로 반영했는지 확인한다.
      await domIs(p2, "둘째 폼 편집 반영", () => document.querySelector('[data-testid="task-field-title"]')?.value === "로그인 다시 고치기" && document.querySelector('[data-testid="task-save"]')?.disabled === false);
      // 첫째가 메모를 고쳐 저장한다(메모가 있는 task는 그린 메모부터 보이고 「고치기」가 입력 칸으로 바꾼다)
      const before = await shaOf(page);
      await page.getByTestId("task-notes-preview").locator("li", { hasText: "메모: 테스트 먼저" }).waitFor();
      await page.getByTestId("task-notes-toggle").click();
      await page.getByTestId("task-field-notes").fill("첫째가 고친 메모");
      await page.getByTestId("task-save").click();
      const text = await saved(page, before);
      if (!taskContent(text).includes("- [ ] 로그인 고치기 (me)\n  첫째가 고친 메모\n")) throw new Error(text);
      // 둘째 화면은 새 판을 받고, 손댄 폼은 그대로 「파일에서 바뀌었다」를 보인다
      await shaIs(p2, sha(text));
      await p2.getByTestId("task-form-stale").waitFor();
      await p2.getByTestId("task-save").click();
      const c = p2.getByTestId("task-conflict");
      await c.waitFor();
      if (!(await c.getByTestId("task-conflict-mine").textContent()).includes("로그인 다시 고치기")) throw new Error("내 고침");
      if (!(await c.getByTestId("task-conflict-latest").textContent()).includes("첫째가 고친 메모")) throw new Error("지금 파일");
      expectFile(read(), text, "충돌이면 쓰지 않는다");
      await shot(p2, "edit-desktop-conflict");
      await c.getByTestId("task-conflict-reopen").click();
      await c.waitFor({ state: "detached" });
      if ((await p2.getByTestId("task-field-title").inputValue()) !== "로그인 고치기") throw new Error("최신으로 다시 열기: 제목");
      if ((await p2.getByTestId("task-notes-preview").textContent()) !== "첫째가 고친 메모") throw new Error("최신으로 다시 열기: 메모");
      if (two.errors.length) throw new Error(two.errors.join("\n"));
      md = sha(text);
    } finally {
      await two.page.context().close();
    }
  });

  await step("대상만 충돌: 폼을 고치는 동안 다른 task가 파일에서 바뀌어도 저장된다(두 고침이 모두 남는다)", async () => {
    await pickRow(page, "배포 준비");
    await page.getByTestId("task-field-notes").fill("폼에서 더한 메모");
    // 그사이 다른 task(정리)의 메모를 파일에서 직접 고친다
    const direct = editTaskContent(read(), "- [ ] 정리\n  선택\n", "- [ ] 정리\n  선택(직접 고침)\n");
    if (direct === read()) throw new Error("직접 고칠 줄이 없다");
    write(direct);
    aaa("ticker", "tick");
    await shaIs(page, sha(direct));
    if ((await page.getByTestId("task-form-stale").count()) !== 0) throw new Error("다른 task가 바뀌었는데 이 폼이 「파일에서 바뀌었다」");
    const before = await shaOf(page);
    await page.getByTestId("task-save").click();
    const text = await saved(page, before);
    expectFile(text, editTaskContent(direct, "- [ ] 배포 준비 (Priya Rao)\n", "- [ ] 배포 준비 (Priya Rao)\n  폼에서 더한 메모\n"), "대상만 충돌");
    md = sha(text);
  });

  await step("속성 칸은 고르면 바로 저장하고(q-1382 3B), 고치던 제목은 남아 이어서 「저장」해도 충돌하지 않는다", async () => {
    await pickRow(page, "문서 정리");
    const detail = page.getByTestId("task-detail");
    await detail.getByTestId("task-field-title").fill("문서 정리하기");
    let before = await shaOf(page);
    await detail.getByTestId("task-field-owner").selectOption("me");
    let text = await saved(page, before);
    if (!taskContent(text).includes("- [ ] 문서 정리 (me)\n")) throw new Error(`담당만 바로 저장: ${text}`);
    if ((await detail.getByTestId("task-field-title").inputValue()) !== "문서 정리하기") throw new Error("바로 저장이 고치던 제목을 지웠다");
    if ((await page.getByTestId("task-form-stale").count()) !== 0) throw new Error("내 바로 저장을 「파일에서 바뀌었다」로 보였다");
    before = await shaOf(page);
    await detail.getByTestId("task-save").click();
    text = await saved(page, before);
    if (!taskContent(text).includes("- [ ] 문서 정리하기 (me)\n")) throw new Error(`이어 저장: ${text}`);
    if ((await page.getByTestId("task-conflict").count()) !== 0) throw new Error("바로 저장 뒤 제목 저장이 충돌했다");
    md = sha(text);
  });

  await step("지우기·되살리기: 끝남으로 지우면 줄이 빠지고 「되살리기」 줄에서 되살리기", async () => {
    let before = await shaOf(page);
    await pickRow(page, "원격 일");
    // 지우기는 상세 아래 고정 줄의 ⋯에 있다(t-1097)
    await page.getByTestId("task-detail-more").click();
    await page.getByRole("menuitem", { name: "끝남으로 지우기", exact: true }).click();
    let text = await saved(page, before);
    if (text.includes("원격 일")) throw new Error(text);
    await page.getByTestId("task-undo").waitFor();
    await shot(page, "edit-desktop-undo");
    before = await shaOf(page);
    await page.getByTestId("task-undo-restore").click();
    text = await saved(page, before);
    if (!taskContent(text).includes("- [ ] 새 일 (me)\n  첫 줄\n  둘째 줄\n- [ ] 원격 일\n")) throw new Error(`원래 목록 끝으로: ${text}`);
    await page.getByTestId("task-undo").waitFor({ state: "detached" });
    md = sha(text);
  });

  await step("저장만으로는 thread가 시작되지 않는다(tick 울타리 뒤 흐름 사건·thread 없음)", async () => {
    const events = () => aaa("events", "demo").events.length;
    const was = events();
    const before = await shaOf(page);
    await pickRow(page, "새 일");
    await page.getByTestId("task-notes-toggle").click();
    await page.getByTestId("task-field-notes").fill("저장만 한다");
    await page.getByTestId("task-save").click();
    md = sha(await saved(page, before));
    aaa("ticker", "tick");
    if (events() !== was) throw new Error("저장 뒤 흐름 사건이 생겼다");
    if (aaa("thread", "list", "demo").threads.length !== 0) throw new Error("저장이 thread를 시작했다");
  });

  await step("보기 권한 기기: 편집 단추·끌기·「+」·⋯가 없고 상세는 읽기 전용이다", async () => {
    const r = await pairPage(browser, aaa("web", "pair", "--reader").code, { width: 1280, height: 800 }, { hash: "#tasks" });
    try {
      await shaIs(r.page, md);
      await domIs(r.page, "편집 판정", () => document.querySelector('[data-testid="tasks-view"]')?.dataset.canEdit === "false");
      for (const sel of ['[data-testid="tasks-add"]', '[data-testid="tasks-fab"]', '[data-testid^="task-menu-"]', ".task-grip", '[data-testid^="task-list-menu-"]', '[data-testid="tasks-list-add"]', '[data-testid="tasks-raw"]']) {
        if ((await r.page.locator(sel).count()) !== 0) throw new Error(`보기 권한에 ${sel}`);
      }
      await pickRow(r.page, "새 일");
      await r.page.getByTestId("task-detail").waitFor();
      if ((await r.page.getByTestId("task-form").count()) !== 0) throw new Error("보기 권한에 편집 폼");
      if (r.errors.length) throw new Error(r.errors.join("\n"));
    } finally {
      await r.page.context().close();
    }
  });

  await step("명령 팔레트 「할 일 더하기」: 할 일 화면을 연 적 없는 기기에서도 화면을 열고 새 task 폼을 연다", async () => {
    const c = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 });
    try {
      const p = c.page;
      await p.getByTestId("palette-open").click();
      const palette = p.getByTestId("command-palette");
      await palette.getByRole("searchbox", { name: "패널과 동작 검색" }).fill("할 일 더하기");
      await palette.getByRole("option", { name: "할 일 더하기" }).click();
      await p.getByTestId("task-form").waitFor();
      if ((await p.getByTestId("task-detail").getAttribute("aria-label")) !== "새 할 일") throw new Error("새 task 폼이 아니다");
      if ((await p.getByTestId("task-field-title").inputValue()) !== "") throw new Error("새 폼 제목");
      if (c.errors.length) throw new Error(c.errors.join("\n"));
    } finally {
      await c.page.context().close();
    }
  });

  await step("휴대폰: 떠 있는 「+」로 더하고, 행 메뉴(아래 시트)의 맨 위로로 순서를 바꾼다. 상세는 전체 화면 시트", async () => {
    const { page: p } = await pairDevice(browser, aaa("web", "pair").code, "iphone", { hash: "#tasks" });
    try {
      await shaIs(p, md);
      await shot(p, "edit-phone-list");
      await p.getByTestId("tasks-fab").click();
      await p.getByTestId("task-field-title").fill("휴대폰 일");
      const box = await detailBox(p);
      if (Math.abs(box.d.left) > 1 || Math.abs(box.d.right - box.w) > 1 || box.d.bottom < box.h - 1 || box.wide) throw new Error(`휴대폰 폼은 화면 전체 ${JSON.stringify(box)}`);
      await shot(p, "edit-phone-form");
      let before = await shaOf(p);
      await p.getByTestId("task-save").click();
      let text = await saved(p, before);
      if (!taskContent(text).includes("- [ ] 원격 일\n- [ ] 휴대폰 일\n")) throw new Error(text);
      await p.evaluate(() => new Promise((r) => { addEventListener("popstate", () => r(), { once: true }); history.back(); }));
      await p.getByTestId("task-detail").waitFor({ state: "detached" });
      await row(p, "휴대폰 일").locator("[data-testid^='task-menu-']").click();
      const sheet = p.locator(".row-sheet");
      await sheet.waitFor();
      await shot(p, "edit-phone-menu");
      before = await shaOf(p);
      await sheet.getByRole("button", { name: "맨 위로" }).click();
      text = await saved(p, before);
      if (!taskContent(text).includes("## In progress\n- [ ] 휴대폰 일\n- [ ] 로그인 고치기 (me)\n")) throw new Error(text);
      md = sha(text);
    } finally {
      await p.context().close();
    }
  });

  await step("터치 입력 글자: 할 일 검색·편집·붙여넣기·원문은 iOS 초점 확대 하한을 지킨다", async () => {
    const wk = await webkit.launch();
    try {
      for (const [name, engine] of [["chromium", browser], ["webkit", wk]]) {
        const { page: p } = await pairDevice(engine, aaa("web", "pair").code, "iphone", { hash: "#tasks" });
        // iOS의 키보드·초점 확대는 headless에 없다. 실제 렌더된 글자의 하한을 검사한다.
        const readable = async (scope) => {
          const fields = scope.locator('input.input:visible, select.input:visible, textarea.input:visible, [contenteditable="true"]:visible');
          const sizes = await fields.evaluateAll((els) => els.map((el) => ({ field: el.getAttribute("data-testid"), size: parseFloat(getComputedStyle(el).fontSize) })));
          if (!sizes.length || sizes.some(({ size }) => size < 16)) throw new Error(`${name} 입력 글자: ${JSON.stringify(sizes)}`);
        };
        try {
          await shaIs(p, md);
          await readable(p.getByTestId("tasks-view"));
          await p.getByTestId("tasks-fab").click();
          await p.getByTestId("task-field-title").fill("입력 확대 확인");
          await p.getByTestId("task-field-notes").fill("메모 입력 확인");
          await p.getByTestId("task-field-owner").selectOption("person");
          await p.getByTestId("task-field-person").fill("담당자");
          await readable(p.getByTestId("task-detail"));
          await shot(p, `input-font-${name}-phone`);
          await p.setViewportSize({ width: 844, height: 390 });
          await readable(p.getByTestId("task-detail"));
          await shot(p, `input-font-${name}-landscape`);
          await p.getByTestId("task-detail-close").click();
          await p.getByTestId("tasks-raw").click();
          await p.getByTestId("task-raw-text").fill("# 원문 입력 확인\n");
          await readable(p.getByTestId("task-raw-editor"));
          await p.keyboard.press("Escape");
        } finally {
          await p.context().close();
        }
      }
    } finally {
      await wk.close();
    }
  });

  await step("패드 세로: 편집 폼은 오른쪽 시트", async () => {
    const { page: p } = await pairDevice(browser, aaa("web", "pair").code, "ipad", { hash: "#tasks" });
    try {
      await shaIs(p, md);
      await pickRow(p, "로그인 고치기");
      await p.getByTestId("task-form").waitFor();
      const box = await detailBox(p);
      if (Math.abs(box.d.right - box.w) > 1 || box.d.width >= box.l.width || box.d.left <= box.l.left || box.wide) throw new Error(`패드 세로 폼은 오른쪽 시트 ${JSON.stringify(box)}`);
      await shot(p, "edit-pad-portrait");
    } finally {
      await p.context().close();
    }
  });

  await step("「나중」 항목 메뉴 「할 일로 옮기기」가 새 task 폼을 채우고, 더하면 Backlog에 들어간다(「나중」 파일은 그대로)", async () => {
    fs.mkdirSync(`${dir()}/memory`, { recursive: true });
    const later = "# 나중 작업 모음\n\n## UI\n- 건설 안내 문구를 UI로\n  취소 버튼(터치)은 필요\n- `카드` 기울기\n";
    fs.writeFileSync(`${dir()}/memory/later.md`, later);
    aaa("project", "set", "demo", "task_notes.add", "memory/later.md");
    aaa("ticker", "tick");
    await page.getByTestId("tasks-tab-later").click();
    await page.locator("summary", { hasText: "UI" }).click();
    await page.getByTestId("tasks-notes-menu-0-0").click();
    await shot(page, "edit-desktop-later-menu");
    await page.getByRole("menuitem", { name: "할 일로 옮기기", exact: true }).click();
    const title = page.getByTestId("task-field-title");
    if ((await title.inputValue()) !== "건설 안내 문구를 UI로") throw new Error(await title.inputValue());
    const before = await shaOf(page);
    await page.getByTestId("task-save").click();
    const text = await saved(page, before);
    if (!taskContent(text).includes("- [ ] 건설 안내 문구를 UI로\n  취소 버튼(터치)은 필요\n  (나중 memory/later.md · UI에서 옮김)\n")) throw new Error(text);
    // 「나중」 파일은 고치지 않는다(출처는 메모 끝 줄에만 남는다)
    if (fs.readFileSync(`${dir()}/memory/later.md`, "utf8") !== later) throw new Error("「나중」 파일이 바뀌었다");
    md = sha(text);
  });

  await step("원문 편집: 연 판 그대로일 때만 저장하고, 그사이 바뀌었으면 쓰지 않고 「최신으로 다시 열기」", async () => {
    await page.getByTestId("tasks-raw").click();
    const editor = page.getByTestId("task-raw-editor");
    const area = editor.getByTestId("task-raw-text");
    await area.fill(`${await area.inputValue()}- [ ] 원문으로 더함\n`);
    // 그사이 파일을 직접 고친다
    const direct = editTaskContent(read(), "- [ ] 원격 일\n", "- [ ] 원격 일 (me)\n");
    write(direct);
    await editor.getByTestId("task-raw-save").click();
    await editor.getByTestId("task-raw-stale").waitFor();
    expectFile(read(), direct, "판이 다르면 원문 편집을 쓰지 않는다");
    aaa("ticker", "tick");
    await shaIs(page, sha(direct));
    await editor.getByRole("button", { name: "최신으로 다시 열기" }).click();
    if ((await area.inputValue()) !== direct) throw new Error("최신 원문");
    await area.fill(`${direct}- [ ] 원문으로 더함\n`);
    const before = await shaOf(page);
    await editor.getByTestId("task-raw-save").click();
    await editor.waitFor({ state: "detached" });
    expectFile(await saved(page, before), `${direct}- [ ] 원문으로 더함\n`, "원문 편집");
  });
}

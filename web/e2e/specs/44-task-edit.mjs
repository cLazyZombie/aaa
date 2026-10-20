// 할 일 편집(T2, 사용자 10-20 「필요하면 수정, 삭제 할수 있고」, plan §8.307). 웹이 host `task_edit`로 TASKS.md를 고친다: 바뀐 블록만
// 쓰고(나머지 바이트 그대로), 대상 블록 hash가 그사이 바뀌었으면 쓰지 않고 충돌 창을 띄운다. 지운 task는 기록에서 되살린다. 저장만으로는
// thread가 시작되지 않는다. 시간은 기다리지 않는다: 화면 표지 `data-tasks-sha`·`data-task-saving`과 파일 글을 본다.
import fs from "node:fs";
import crypto from "node:crypto";
import { root, aaa, step, browser, pairPage, domIs, shot, openSettings } from "../lib/harness.mjs";
import { pairDevice } from "../lib/devices.mjs";

const dir = () => `${root}/projects/demo`;
const file = () => `${dir()}/TASKS.md`;
const sha = (text) => crypto.createHash("sha256").update(text).digest("hex");
const read = () => fs.readFileSync(file(), "utf8");
const write = (text) => {
  fs.writeFileSync(file(), text);
  return sha(text);
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
const expectFile = (got, want, what) => {
  if (got !== want) throw new Error(`${what}: 파일이 다르다\n--- 기대\n${want}\n--- 실제\n${got}`);
};

export default async function ({ page }) {
  let md;
  await step("편집 폼: 새 task 더하기, 제목·owner(사람으로 저장)·목록 바꾸기. 파일은 바뀐 블록만 바뀐다", async () => {
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
    // 있는 task: 제목·owner(사람)·목록 → 한 번에 고치고 옮긴다
    await pickRow(page, "배포");
    await detail.getByTestId("task-field-title").fill("배포 준비");
    await detail.getByTestId("task-field-owner").selectOption("person");
    await detail.getByTestId("task-field-person").fill("Priya Rao");
    await detail.getByTestId("task-field-list").selectOption("In progress");
    await shot(page, "edit-desktop-form");
    before = await shaOf(page);
    await detail.getByTestId("task-save").click();
    text = await saved(page, before);
    want = want.replace("- [ ] 배포 (me)\n", "").replace("  - 메모: 테스트 먼저\n", "  - 메모: 테스트 먼저\n- [ ] 배포 준비 (Priya Rao)\n");
    expectFile(text, want, "고치고 옮기기");
    // 사람 이름을 「사람으로 저장」 없이 owner로 쓰면 거부하고 폼 안에 까닭을 보인다
    await pickRow(page, "원격 일");
    await detail.getByTestId("task-field-owner").selectOption("person");
    await detail.getByTestId("task-field-person").fill("me");
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
    if (!text.includes("- [ ] 정리\n  선택\n  t-0101 S4 뒤, 사용자 지시 시\n  사용자 지시 뒤에 한다\n")) throw new Error(text);
    await page.getByTestId("activity-badge-tasks").waitFor({ state: "detached" });
    md = sha(text);
  });

  await step("행 메뉴·끌기·Alt+↑: 같은 상위 안 순서와 목록 옮기기, 목록 머리 메뉴: 이름 바꾸기·빈 목록 지우기·새 목록", async () => {
    const order = (text, list) => {
      const block = text.split("## ").find((b) => b.startsWith(`${list}\n`)) ?? "";
      return block.split("\n").filter((l) => l.startsWith("- [")).map((l) => l.replace(/^- \[.\] /, "").replace(/ \(.*$/, ""));
    };
    // 「문서 정리」 ⋯ → 위로
    let before = await shaOf(page);
    await row(page, "문서 정리").locator("[data-testid^='task-menu-']").click();
    await shot(page, "edit-desktop-row-menu");
    await page.getByRole("menuitem", { name: "위로", exact: true }).click();
    let text = await saved(page, before);
    if (order(text, "Backlog").join("|") !== "정리|문서 정리|원격 일|새 일") throw new Error(`위로 ${order(text, "Backlog")}`);
    // 끌기: 「원격 일」 손잡이를 「정리」 위 절반에 놓으면 그 앞
    before = await shaOf(page);
    const target = row(page, "정리");
    const box = await target.boundingBox();
    await row(page, "원격 일").locator(".task-grip").dragTo(target, { targetPosition: { x: 40, y: 3 } });
    text = await saved(page, before);
    if (order(text, "Backlog").join("|") !== "원격 일|정리|문서 정리|새 일") throw new Error(`끌기 ${order(text, "Backlog")} (${JSON.stringify(box)})`);
    // Alt+↓: 키보드로 한 칸 아래
    before = await shaOf(page);
    await row(page, "원격 일").locator(".task-pick").focus();
    await page.keyboard.press("Alt+ArrowDown");
    text = await saved(page, before);
    if (order(text, "Backlog").join("|") !== "정리|원격 일|문서 정리|새 일") throw new Error(`Alt+↓ ${order(text, "Backlog")}`);
    // 목록 머리 ⋯ → 이름 바꾸기
    before = await shaOf(page);
    await page.getByTestId("task-list-menu-빈 목록").click();
    await page.getByRole("menuitem", { name: "이름 바꾸기", exact: true }).click();
    await page.getByTestId("task-name-input").fill("보관");
    await page.getByTestId("task-name-save").click();
    text = await saved(page, before);
    if (!text.endsWith("## 보관\n") || text.includes("## 빈 목록")) throw new Error(text);
    // 빈 목록 지우기, 새 목록
    before = await shaOf(page);
    await page.getByTestId("task-list-menu-보관").click();
    await page.getByRole("menuitem", { name: "빈 목록 지우기", exact: true }).click();
    text = await saved(page, before);
    if (text.includes("## 보관")) throw new Error(text);
    before = await shaOf(page);
    await page.getByTestId("tasks-list-add").click();
    await page.getByTestId("task-name-input").fill("다음 주");
    await page.getByTestId("task-name-save").click();
    text = await saved(page, before);
    if (!text.endsWith("\n## 다음 주\n")) throw new Error(text);
    // 다른 목록으로(메뉴)
    before = await shaOf(page);
    await row(page, "문서 정리").locator("[data-testid^='task-menu-']").click();
    await page.getByRole("menuitem", { name: "다음 주(으)로 옮기기", exact: true }).click();
    text = await saved(page, before);
    if (!text.endsWith("## 다음 주\n- [ ] 문서 정리\n")) throw new Error(text);
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
      // 첫째가 메모를 고쳐 저장한다(메모가 있는 task는 그린 메모부터 보이고 「고치기」가 입력 칸으로 바꾼다)
      const before = await shaOf(page);
      await page.getByTestId("task-notes-preview").locator("li", { hasText: "메모: 테스트 먼저" }).waitFor();
      await page.getByTestId("task-notes-toggle").click();
      await page.getByTestId("task-field-notes").fill("첫째가 고친 메모");
      await page.getByTestId("task-save").click();
      const text = await saved(page, before);
      if (!text.includes("- [ ] 로그인 고치기 (me)\n  첫째가 고친 메모\n")) throw new Error(text);
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

  await step("지우기·되살리기: 끝남으로 지우면 줄이 빠지고 「되살리기」 줄, 기록 화면의 삭제만 거르기와 되살리기", async () => {
    let before = await shaOf(page);
    await pickRow(page, "원격 일");
    await page.getByTestId("task-delete").click();
    let text = await saved(page, before);
    if (text.includes("원격 일")) throw new Error(text);
    await page.getByTestId("task-undo").waitFor();
    await shot(page, "edit-desktop-undo");
    before = await shaOf(page);
    await page.getByTestId("task-undo-restore").click();
    text = await saved(page, before);
    if (!text.includes("- [ ] 새 일 (me)\n  첫 줄\n  둘째 줄\n- [ ] 원격 일\n")) throw new Error(`원래 목록 끝으로: ${text}`);
    await page.getByTestId("task-undo").waitFor({ state: "detached" });
    // 다시 지우고 기록 화면에서 되살린다
    before = await shaOf(page);
    await pickRow(page, "원격 일");
    await page.getByTestId("task-delete").click();
    await saved(page, before);
    await page.getByTestId("tasks-tab-log").click();
    const log = page.getByTestId("task-log");
    await log.getByTestId("task-log-filter-deleted").click();
    await domIs(page, "삭제만", () => {
      const rows = [...document.querySelectorAll('[data-testid="task-log"] [data-action]')];
      return rows.length === 2 && rows.every((r) => r.dataset.action === "delete");
    });
    await log.getByTestId("task-log-filter-all").click();
    await domIs(page, "모든 기록", () => document.querySelectorAll('[data-testid="task-log"] [data-action]').length > 2);
    await log.locator(".task-log-row").first().locator(".task-log-toggle").click();
    await shot(page, "edit-desktop-log");
    before = await shaOf(page);
    await log.locator("[data-testid^='task-log-restore-']").first().click();
    text = await saved(page, before);
    if (!text.includes("- [ ] 원격 일\n")) throw new Error(text);
    await page.getByTestId("tasks-tab-tasks").click();
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

  await step("휴대폰: 떠 있는 「+」로 더하고, 행 메뉴(아래 시트)의 맨 위로로 순서를 바꾼다. 상세는 전체 화면 시트", async () => {
    const { page: p } = await pairDevice(browser, aaa("web", "pair").code, "iphone", { hash: "#tasks" });
    try {
      await shaIs(p, md);
      await shot(p, "edit-phone-list");
      await p.getByTestId("tasks-fab").click();
      await p.getByTestId("task-field-title").fill("휴대폰 일");
      await shot(p, "edit-phone-form");
      let before = await shaOf(p);
      await p.getByTestId("task-save").click();
      let text = await saved(p, before);
      if (!text.includes("- [ ] 원격 일\n- [ ] 휴대폰 일\n")) throw new Error(text);
      await p.evaluate(() => new Promise((r) => { addEventListener("popstate", () => r(), { once: true }); history.back(); }));
      await p.getByTestId("task-detail").waitFor({ state: "detached" });
      await row(p, "휴대폰 일").locator("[data-testid^='task-menu-']").click();
      const sheet = p.locator(".row-sheet");
      await sheet.waitFor();
      await shot(p, "edit-phone-menu");
      before = await shaOf(p);
      await sheet.getByRole("button", { name: "맨 위로" }).click();
      text = await saved(p, before);
      if (!text.includes("나중 일은 memory/later.md에 모은다\n- [ ] 휴대폰 일\n- [ ] 정리\n")) throw new Error(text);
      md = sha(text);
    } finally {
      await p.context().close();
    }
  });

  await step("패드 세로: 편집 폼은 오른쪽 시트", async () => {
    const { page: p } = await pairDevice(browser, aaa("web", "pair").code, "ipad", { hash: "#tasks" });
    try {
      await shaIs(p, md);
      await pickRow(p, "로그인 고치기");
      await p.getByTestId("task-form").waitFor();
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
    if (!text.includes("- [ ] 건설 안내 문구를 UI로\n  취소 버튼(터치)은 필요\n  (나중 memory/later.md · UI에서 옮김)\n")) throw new Error(text);
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
    const direct = read().replace("- [ ] 원격 일\n", "- [ ] 원격 일 (me)\n");
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

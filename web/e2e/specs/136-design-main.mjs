// 기획 main 직접 저장(t-2002, 확정 시안 library/mock, q-4227 1A): 실제 host·agent·git origin으로 기획 웹 화면을 본다.
// 트리·문서에 branch 표시가 없다 → 고치기와 그림 붙이기(main에 바로 commit, 그림은 그 commit 주소) → 저장 실패 상자(그 사이 다른
// 판, 글은 남는다) → 대화 고르기·새 대화(branch 칸 없음) → 대화 입력창 그림 → 이력과 되돌리기 → 대화 끝내기(main은 그대로)
// → 블록 표지(넘긴 할 일 칩·보기 셋·대화로 정하기) → 구현 맡기기(표지를 main에 바로 적고 예약한 구현 thread는 main에서) →
// 넘긴 블록 고침 알림 → 휴대폰 칸. 저장소 문서 검사가 저장을 거절하는 것은 m13 `design_saves_go_straight_to_main_with_checks_and_history`가 본다.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import zlib from "node:zlib";
import { aaa, root, step, browser, webkit, pairPage, inList, domIs, until, shot } from "../lib/harness.mjs";

/** 단색 PNG(w×h) */
function png(w, h, [r, g, b]) {
  const crcTable = Array.from({ length: 256 }, (_, n) => { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; return c >>> 0; });
  const crc = (buf) => { let c = 0xffffffff; for (const x of buf) c = crcTable[(c ^ x) & 0xff] ^ (c >>> 8); return (c ^ 0xffffffff) >>> 0; };
  const chunk = (type, data) => {
    const body = Buffer.concat([Buffer.from(type), data]);
    const out = Buffer.alloc(body.length + 8);
    out.writeUInt32BE(data.length, 0); body.copy(out, 4); out.writeUInt32BE(crc(body), body.length + 4);
    return out;
  };
  const head = Buffer.alloc(13);
  head.writeUInt32BE(w, 0); head.writeUInt32BE(h, 4); head[8] = 8; head[9] = 2;
  const row = Buffer.concat([Buffer.from([0]), Buffer.from(Array.from({ length: w }, () => [r, g, b]).flat())]);
  return Buffer.concat([Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), chunk("IHDR", head),
    chunk("IDAT", zlib.deflateSync(Buffer.concat(Array(h).fill(row)))), chunk("IEND", Buffer.alloc(0))]);
}

const git = (cwd, ...args) =>
  execFileSync("git", ["-c", "user.name=demo", "-c", "user.email=demo@example.com", "-c", "init.defaultBranch=main", ...args], { cwd, stdio: ["ignore", "pipe", "pipe"] }).toString().trim();

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

const ORIGIN = () => `${root}/origins/app.git`;
const branches = () => aaa("plan", "list").repos.find((r) => r.repo === "app").branches;
/** origin main 판의 파일 글 */
const onMain = (path) => git(root, "--git-dir", ORIGIN(), "show", `refs/heads/main:${path}`);
const mainHead = () => git(root, "--git-dir", ORIGIN(), "rev-parse", "refs/heads/main");
/** CLI 저장(사람): 바로 origin main에 들어간다. `chat`을 주면 그 대화에 사건을 남긴다 */
const save = (path, text, base, chat) => {
  fs.writeFileSync(`${root}/draft.md`, text);
  return aaa("plan", "save", "app", path, ...(chat ? ["--branch", chat] : []), ...(base ? ["--base", base] : ["--new"]), "--file", `${root}/draft.md`);
};

export default async function () {
  let page;
  const tasks = {};
  // 구현 맡기기에서 맡긴 할 일과 그 구현 thread(고침 알림이 쓴다)
  const handed = {};
  await step("기획 대화 둘: 저장은 바로 origin main에 들어간다", async () => {
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
    fs.writeFileSync(`${root}/projects/demo/aaa.toml`, `schema = 1\nthread_profile = "fake"\n\n[[workspaces]]\npath = "${clone}"\nmachine = "labA"\ndesign = "docs"\ndesign_media = "mockups"\n`);
    for (const [k, title] of [["work", "가호 칸 6칸"], ["wait", "나가기 통로"], ["done", "층 표 상한"]]) {
      aaa("task", "add", title, "--list", "Backlog");
      tasks[k] = aaa("task", "list").tasks.find((t) => t.title === title).short_id;
    }
    fs.writeFileSync(`${seed}/docs/exit.md`, EXIT(tasks));
    git(seed, "add", ".");
    git(seed, "commit", "-q", "-m", "표지");
    git(seed, "push", "-q", "origin", "HEAD:main");
    const checkout = git(clone, "rev-parse", "HEAD");
    aaa("plan", "new", "app", "shop", "--title", "중간 상점");
    aaa("plan", "new", "app", "floors", "--title", "100층까지");
    const blob = aaa("plan", "show", "app", "docs/loop/README.md").doc.blob;
    const saved = save("docs/loop/README.md", LOOP.replace("지금 20층까지 있다.\n", "지금 20층까지 있다.\n\n> [!계획]\n> **중간 상점** — 다섯 층마다 한 번.\n"), blob, "shop");
    assert.equal(mainHead(), saved.commit, "저장은 곧 origin main");
    assert.match(onMain("docs/loop/README.md"), /중간 상점/);
    assert.equal(git(clone, "rev-parse", "HEAD"), checkout, "저장소 checkout은 그대로");
    assert.deepEqual(branches().map((b) => b.slug), ["shop", "floors"]);
    assert.equal(git(root, "--git-dir", ORIGIN(), "branch", "--list", "design/*"), "", "branch를 만들지 않는다");
  }, { always: true });

  await step("트리와 문서: branch 표시·판 고르기 없이 main 하나", async () => {
    ({ page } = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 900 }, { serviceWorkers: "block" }));
    await page.evaluate(() => localStorage.setItem("herdr-web-ui:design-chat:app", "shop"));
    await inList(page, "activity-design");
    await inList(page, page.locator('[data-testid="design-dir"][data-path="docs/loop"]'));
    const readme = page.locator('[data-testid="design-doc"][data-path="docs/loop/README.md"]');
    await readme.click();
    await page.getByTestId("design-body").getByText("중간 상점").first().waitFor();
    assert.equal(await page.getByTestId("design-repo-branches").count(), 0);
    assert.equal(await page.getByTestId("design-doc-branch").count(), 0);
    assert.equal(await page.getByTestId("design-tab").count(), 0, "판 고르기 없음");
    assert.match(await page.getByTestId("design-meta").textContent(), /main [0-9a-f]{7}/);
    await page.getByTestId("design-history").waitFor();
    assert.equal(await page.getByTestId("design-apply").count(), 0, "「main에 반영」 없음");
    await page.getByTestId("design-mark").filter({ hasText: /^사람이 README\.md 고침 · main [0-9a-f]{7}/ }).first().waitFor();
    await shot(page, "design-main-wide");
  });

  // t-1975: 대화 목록의 바깥 누름은 선택을 바꾸지 않으며, 누른 곳의 동작도 그대로 실행한다.
  for (const name of ["chromium", "webkit"]) for (const width of [1280, 390]) {
    await step(`대화 목록 닫기 ${name} ${width}px: 바깥 누름·Esc·제목 재누름·선택·새 대화`, async () => {
      const wk = name === "webkit" ? await webkit.launch({ headless: true }) : null;
      let p;
      try {
        ({ page: p } = await pairPage(wk ?? browser, aaa("web", "pair").code, { width, height: 900 }, {
          hash: `#design=${encodeURIComponent("app/docs/loop/README.md")}`,
          init: () => localStorage.setItem("herdr-web-ui:design-chat:app", "shop"),
        }));
        const press = (locator) => width === 390 ? locator.tap() : locator.click();
        if (width === 390) await press(p.getByTestId("design-page-chat"));
        const button = p.getByTestId("design-pick"), menu = p.getByTestId("design-pick-menu");
        const title = () => p.evaluate(() => document.querySelector("[data-testid=design-pick] strong")?.textContent);
        await domIs(p, "대화 shop 선택", () => document.querySelector("[data-testid=design-pick] strong")?.textContent === "중간 상점");
        const open = async () => { await press(button.locator("svg").first()); await menu.waitFor(); };
        const closed = async () => {
          await menu.waitFor({ state: "detached" });
          assert.equal(await button.getAttribute("aria-expanded"), "false");
          assert.equal(await title(), "중간 상점", "닫기만 하면 대화 선택을 유지한다");
        };
        await open();
        await press(menu.locator(".design-pick-label"));
        assert.equal(await menu.isVisible(), true, "목록 안 빈 곳은 닫지 않는다");
        await press(p.getByTestId("design-chat-idle"));
        await closed();
        await open();
        await press(button.locator("strong"));
        await closed();
        await open();
        await p.keyboard.press("Escape");
        await closed();
        assert.equal(await button.evaluate((b) => document.activeElement === b), true, "Esc 뒤 제목 단추에 초점");
        await open();
        await press(p.getByTestId("design-chat-more"));
        await closed();
        await p.getByTestId("design-discard-open").waitFor();
        assert.equal(await p.getByTestId("design-chat-more").getAttribute("aria-expanded"), "true", "바깥 단추가 한 번에 실행된다");
        await press(p.getByTestId("design-chat-more"));
        await open();
        await press(p.getByTestId("design-pick-row").filter({ hasText: "100층까지" }));
        await menu.waitFor({ state: "detached" });
        await domIs(p, "대화 floors 선택", () => document.querySelector("[data-testid=design-pick] strong")?.textContent === "100층까지");
        await open();
        await press(p.getByTestId("design-pick-row").filter({ hasText: "중간 상점" }));
        await closed();
        await open();
        await press(p.getByTestId("design-new"));
        await closed();
        const dialog = p.getByTestId("design-new-dialog");
        await dialog.waitFor();
        await press(dialog.getByRole("button", { name: "닫기", exact: true }));
        await dialog.waitFor({ state: "detached" });
        await shot(p, `design-pick-closed-${name}-${width}`);
      } finally {
        await p?.context().close();
        await wk?.close();
      }
    });
  }

  await step("고치기와 그림 붙이기: main에 바로 commit하고 그림은 그 commit 주소로 보인다", async () => {
    await page.getByTestId("design-edit").click();
    const editor = page.getByTestId("design-editor");
    await editor.waitFor();
    assert.match(await page.getByTestId("design-save-note").textContent(), /바로 main에 들어간다/);
    assert.match(await page.getByTestId("design-edit-save").textContent(), /main에 저장/);
    await editor.evaluate((t) => { t.focus(); t.setSelectionRange(t.value.length, t.value.length); });
    const before = mainHead();
    await page.getByTestId("design-attach-input").setInputFiles({ name: "지도.png", mimeType: "image/png", buffer: png(200, 100, [80, 120, 200]) });
    // 저장소 손 그림 폴더(`design_media = "mockups"`, 문서 폴더 아래)에 들어오고 문서 기준 상대 경로로 넣는다
    await domIs(page, "그림 줄", () => /!\[지도\]\(mockups\/[0-9a-f]{16}\.png\)/.test(document.querySelector("[data-testid=design-editor]").value));
    const pic = (await editor.inputValue()).match(/\((mockups\/[0-9a-f]{16}\.png)\)/)[1];
    assert.notEqual(mainHead(), before, "그림은 main에 따로 commit한다");
    assert.match(git(root, "--git-dir", ORIGIN(), "log", "-1", "--format=%s", "main"), /^기획: 그림 [0-9a-f]{16}\.png 붙임 \(사람\)$/);
    await shot(page, "design-main-edit");
    await page.getByTestId("design-edit-save").click();
    await page.getByTestId("design-body").waitFor();
    await domIs(page, "그 판의 그림", () => { const i = document.querySelector("[data-testid=design-body] img"); return !!i && i.complete && i.naturalWidth === 200; });
    assert.match(await page.getByTestId("design-body").locator("img").getAttribute("src"), /\?c=[0-9a-f]{40}$/, "branch 없는 commit 주소");
    assert.ok(onMain("docs/loop/README.md").includes(`(${pic})`), "글도 main에 들어갔다");
    const shop = branches().find((b) => b.slug === "shop");
    assert.ok(shop.edited.includes("docs/loop/README.md"), "사람이 고친 문서는 다음 말 앞에 알린다");
    assert.deepEqual(shop.events.filter((e) => e.kind === "edit").map((e) => e.text).slice(-2), [`docs/loop/${pic}`, "docs/loop/README.md"]);
    assert.ok(git(root, "--git-dir", ORIGIN(), "ls-tree", "--name-only", "main", `docs/loop/${pic}`), "손 그림 폴더에 commit");
    await page.getByTestId("design-context").getByText("사람이 고침").waitFor();
  });

  await step("저장 실패 상자: 그 사이 다른 판이면 main은 그대로이고 쓴 글은 남는다", async () => {
    await page.getByTestId("design-edit").click();
    const editor = page.getByTestId("design-editor");
    await editor.waitFor();
    await editor.evaluate((t) => { t.focus(); t.setSelectionRange(t.value.length, t.value.length); });
    await editor.pressSequentially("\n웹에서 덧붙임\n");
    // 다른 기기(CLI)가 먼저 저장했다
    const now = aaa("plan", "show", "app", "docs/loop/README.md").doc;
    save("docs/loop/README.md", now.text.replace("- 11-15: 처음 정함.", "- 11-18: 다른 기기가 고침.\n- 11-15: 처음 정함."), now.blob);
    const head = mainHead();
    await page.getByTestId("design-edit-save").click();
    const box = page.getByTestId("design-save-error");
    await box.waitFor();
    assert.match(await box.textContent(), /저장하지 않았다[\s\S]*다른 판/);
    assert.equal(mainHead(), head, "main은 그대로");
    assert.match(await editor.inputValue(), /웹에서 덧붙임/, "쓴 글은 남는다");
    await shot(page, "design-main-save-error");
    await page.getByTestId("design-edit-cancel").click();
    await page.getByTestId("design-body").getByText("다른 기기가 고침").waitFor();
  });

  await step("대화 고르기와 새 대화: branch 칸 없이 주제와 이름", async () => {
    await page.getByTestId("design-pick").click();
    assert.equal(await page.getByTestId("design-pick-row").count(), 2);
    assert.match(await page.getByTestId("design-pick-row").first().textContent(), /지금 · 저장 \d+/);
    await page.getByTestId("design-new").click();
    const dialog = page.getByTestId("design-new-dialog");
    await page.getByTestId("design-new-title").fill("Boss notes");
    assert.equal(await page.getByTestId("design-new-slug").inputValue(), "boss-notes", "영문 주제는 이름을 짓는다");
    assert.doesNotMatch(await dialog.textContent(), /design\//, "branch 칸 없음");
    assert.match(await dialog.textContent(), /바로 app main에 들어간다/);
    await page.getByTestId("design-new-slug").fill("Bad_Name");
    assert.ok(await page.getByTestId("design-new-submit").isDisabled());
    await page.getByTestId("design-new-slug").fill("boss");
    await page.getByTestId("design-new-submit").click();
    await dialog.waitFor({ state: "detached" });
    assert.ok(branches().some((b) => b.slug === "boss" && b.title === "Boss notes"));
    await domIs(page, "boss 대화", () => document.querySelector("[data-testid=design-pick] strong")?.textContent === "Boss notes");
    assert.match(await page.getByTestId("design-mark").first().textContent(), /^새 대화 · main [0-9a-f]{7}$/);
  });

  await step("대화 입력창 그림: main에 commit하고 보낼 말 앞에 「(그림: 경로)」, 보는 문서를 이 대화가 고치지 않았으면 알린다", async () => {
    aaa("plan", "new", "app", "pics", "--title", "그림 대화");
    await page.getByTestId("design-reload").first().click();
    await page.getByTestId("design-pick").click();
    await page.getByTestId("design-pick-row").filter({ hasText: "그림 대화" }).click();
    await domIs(page, "pics 대화", () => document.querySelector("[data-testid=design-pick] strong")?.textContent === "그림 대화");
    const chat = page.getByTestId("design-chat");
    const before = mainHead();
    await chat.locator("input[type=file]").setInputFiles({ name: "시안.png", mimeType: "image/png", buffer: png(120, 80, [200, 90, 60]) });
    await domIs(page, "초안의 그림 경로", () => /@docs\/loop\/mockups\/[0-9a-f]{16}\.png /.test(document.querySelector("[data-testid=design-chat] textarea").value));
    await domIs(page, "고치지 않은 문서 칩", () => document.querySelector("[data-testid=design-viewing]")?.textContent.includes("이 대화가 고치지 않음"));
    const pic = (await chat.locator("textarea").inputValue()).match(/@(docs\/loop\/mockups\/[0-9a-f]{16}\.png)/)[1];
    assert.notEqual(mainHead(), before, "보는 문서 폴더에 main commit");
    assert.ok(git(root, "--git-dir", ORIGIN(), "ls-tree", "--name-only", "main", pic), "그림이 main에 있다");
    assert.deepEqual(branches().find((b) => b.slug === "pics").edited, [], "말 앞 「(그림: …)」이 알리므로 「사람이 고친 문서」에 넣지 않는다");
    await chat.locator("textarea").press("End");
    await chat.locator("textarea").pressSequentially("이 그림으로 시안");
    await chat.locator("textarea").press("Enter");
    const thread = await until("그림 대화 thread", () => branches().find((b) => b.slug === "pics").thread, 60_000);
    const log = `${root}/fake-lines/${thread}.log`;
    await until("대화가 받은 그림 말", () => fs.existsSync(log) && fs.readFileSync(log, "utf8").includes("이 그림으로 시안"), 60_000);
    const lines = fs.readFileSync(log, "utf8").split("\n");
    const at = lines.findIndex((l) => l === "이 그림으로 시안");
    assert.deepEqual(lines.slice(at - 2, at + 1), ["(보는 문서: docs/loop/README.md)", `(그림: ${pic})`, "이 그림으로 시안"]);
    // 대화 thread는 main 기획 자리에서 그 파일을 읽는다(brief)
    const slot = git(`${root}/app`, "worktree", "list", "--porcelain").split("\n\n").find((b) => /demo-design-main$/m.test(b))?.match(/^worktree (.+)$/m)?.[1];
    assert.ok(slot && fs.existsSync(`${slot}/${pic}`), `main 기획 자리에 그림 파일: ${slot}`);
    await chat.getByText(`그림 ${pic.split("/").pop()} 붙임`).first().waitFor();
  });

  await step("이력과 되돌리기: 판마다 누가·때, 고른 판과의 차이, 되돌리면 새 저장으로 main에", async () => {
    await page.getByTestId("design-pick").click();
    await page.getByTestId("design-pick-row").filter({ hasText: "중간 상점" }).click();
    await page.getByTestId("design-history").click();
    const rows = page.getByTestId("design-history-row");
    await domIs(page, "이력 줄", () => document.querySelectorAll("[data-testid=design-history-row]").length >= 4)
      .catch(async (e) => { e.message += ` 화면=${JSON.stringify((await page.getByTestId("design-view").textContent()).slice(0, 600))}`; throw e; });
    const log = aaa("plan", "log", "app", "docs/loop/README.md").entries;
    assert.equal(await rows.count(), log.length, "CLI 이력과 같은 줄");
    assert.match(await rows.first().textContent(), /지금 판/);
    // 「중간 상점」을 넣은 판(가장 옛 판 바로 뒤): 그 뒤 웹에서 붙인 그림 줄과 다른 기기 고침은 되돌리면 사라진다
    const pickRow = log.length - 2;
    assert.equal(log[log.length - 1].other, true, "가장 옛 판은 기획 저장이 아닌 처음 commit");
    await rows.nth(pickRow).click();
    await page.getByTestId("design-diff-gone").first().waitFor();
    const gone = await page.getByTestId("design-diff-gone").allTextContents();
    assert.ok(gone.some((t) => t.includes("다른 기기가 고침")), JSON.stringify(gone));
    await shot(page, "design-main-history");
    await page.getByTestId("design-revert").click();
    const dialog = page.getByTestId("design-revert-dialog");
    await dialog.waitFor();
    assert.equal(await dialog.locator(".design-file-row").count(), pickRow, "고른 판 뒤의 저장은 사라짐으로 보인다");
    assert.match(await dialog.textContent(), /넘긴 블록 표지[\s\S]*그대로 둔다/);
    await shot(page, "design-main-revert");
    const old = aaa("plan", "show", "app", "docs/loop/README.md", "--at", log[pickRow].commit).doc.text;
    await page.getByTestId("design-revert-submit").click();
    await dialog.waitFor({ state: "detached" });
    await page.getByTestId("design-body").waitFor();
    assert.equal(onMain("docs/loop/README.md"), old.trimEnd(), "옛 판 글을 새 저장으로");
    assert.match(git(root, "--git-dir", ORIGIN(), "log", "-1", "--format=%s", "main"), /^기획: docs\/loop\/README\.md 되돌림 ← [0-9a-f]{7} \(사람\)$/);
    await page.getByTestId("design-mark").filter({ hasText: "되돌림 ←" }).first().waitFor();
  });

  await step("대화 끝내기: 대화 thread와 기록을 거두고 main은 그대로", async () => {
    const head = mainHead();
    await page.getByTestId("design-pick").click();
    await page.getByTestId("design-pick-row").filter({ hasText: "그림 대화" }).click();
    await page.getByTestId("design-chat-more").click();
    await page.getByTestId("design-discard-open").click();
    const dialog = page.getByTestId("design-discard-dialog");
    assert.match(await dialog.textContent(), /main에 저장한 글은 그대로/);
    await page.getByTestId("design-discard-submit").click();
    await dialog.waitFor({ state: "detached" });
    assert.ok(!branches().some((b) => b.slug === "pics"));
    assert.equal(mainHead(), head);
    await page.context().close();
  });

  await step("블록 표지: 넘긴 할 일 칩·이상 표시·글 속 d-ID·보기 셋·트리 수·대화로 정하기", async () => {
    // 넘긴 할 일 하나는 thread가 일하고, 하나는 끝나 보관됐다(표지 남음). d-9999는 어디에도 없다(할 일 없음)
    const t = aaa("thread", "start", "demo", "--from-task", tasks.work, "--machine", "labA", "--profile", "fake", "--kind", "dir");
    aaa("task", "edit", tasks.done, "--done");
    const { page: p } = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 900 });
    try {
      await p.evaluate(() => localStorage.setItem("herdr-web-ui:design-chat:app", "boss"));
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
      assert.equal((await p.getByTestId("design-picked").textContent()).trim(), "고른 블록 통로 · 스킬 무기가 나오는 확률");
      await p.getByRole("button", { name: "고른 블록 빼기" }).click();
      await p.getByTestId("design-picked").waitFor({ state: "detached" });
    } finally {
      await p.context().close();
    }
  });

  await step("구현 맡기기: 고른 계획 블록을 할 일 하나로 넘기고 표지를 main에 바로 적으며, 예약한 구현 thread는 main에서 갈라진다", async () => {
    const d = aaa("plan", "show", "app", "docs/exit.md").doc;
    save("docs/exit.md", d.text.replace("## 바뀐 것", "> [!계획]\n> **층 이벤트** — 다섯 층마다 이벤트 방.\n\n## 바뀐 것"), d.blob, "boss");
    const { page: p } = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 900 }, {
      hash: `#design=${encodeURIComponent("app/docs/exit.md")}`,
      init: () => localStorage.setItem("herdr-web-ui:design-chat:app", "boss"),
    });
    try {
      await p.getByTestId("design-body").getByText("층 이벤트").first().waitFor();
      assert.equal(await p.getByTestId("design-handoff").count(), 3, "넘기지 않은 계획만(캠프 지도·상점 위치·층 이벤트)");
      // 창을 연 뒤 그 문서가 바뀌면 맡기지 않는다: 창은 연 때의 판으로 보내고 host가 다시 본다
      await p.getByTestId("design-handoff").first().click();
      const first = p.getByTestId("design-hand-dialog");
      await first.waitFor();
      await p.getByTestId("design-hand-main").waitFor();
      assert.match(await p.getByTestId("design-hand-main").textContent(), /main에 바로 저장/);
      const now = aaa("plan", "show", "app", "docs/exit.md").doc;
      save("docs/exit.md", now.text.replace("- 11-16: 통로를", "- 11-16: 상한 표지를 정리함.\n- 11-16: 통로를"), now.blob);
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
      const before = mainHead();
      await p.getByTestId("design-hand-submit").click();
      await dialog.waitFor({ state: "detached" });
      const id = aaa("task", "list").tasks.find((t) => t.title === title)?.short_id;
      assert.ok(id, "할 일 하나");
      // 고른 두 블록 표지에 같은 d-ID를 적어 main에 바로 저장했다
      const text = onMain("docs/exit.md");
      assert.ok(text.includes(`> [!계획] ${id}\n> **캠프 지도**`) && text.includes(`> [!계획] ${id}\n> **층 이벤트**`), text);
      assert.equal(git(root, "--git-dir", ORIGIN(), "rev-parse", "main^"), before, "표지는 한 commit");
      assert.equal(git(root, "--git-dir", ORIGIN(), "log", "-1", "--format=%s", "main"), `기획: 구현 맡김 ${id} (사람)`);
      const boss = branches().find((b) => b.slug === "boss");
      assert.ok(boss.events.some((e) => e.kind === "handoff" && e.text === `${id} · 블록 2`), JSON.stringify(boss.events));
      await p.locator("[data-testid=design-look][data-look=all]").click();
      await domIs(p, "넘긴 블록 칩", (task) => document.querySelectorAll(`[data-testid=design-callout][data-task="${task}"]`).length === 2, id);
      assert.equal(await p.getByTestId("design-handoff").count(), 1, "남은 계획(상점 위치)만");
      assert.equal(await p.locator(".design-pending").count(), 0, "표지가 main에 있어 「표지는 … 에」 칩이 없다");
      await p.getByTestId("design-mark").filter({ hasText: `구현 맡기기 ${id} · 블록 2 · main에 표지` }).waitFor();
      await shot(p, "design-main-handoff");
      // 예약한 할 일은 자리가 나면 host가 띄운다. 그 thread는 main에서 갈라지고 brief에 맡긴 자리가 있다
      const thread = await until("구현 thread", () => aaa("task", "list").tasks.find((t) => t.short_id === id)?.thread, 60_000)
        .catch((e) => { e.message += ` 예약=${JSON.stringify(aaa("task", "queue")).slice(0, 1500)}`; throw e; });
      const shown = await until("thread 작업 폴더", () => aaa("thread", "show", thread).work_dir && aaa("thread", "show", thread), 60_000);
      const brief = fs.readFileSync(`${shown.work_dir}/brief.md`, "utf8");
      assert.ok(brief.includes("## 기획에서 맡긴 일") && brief.includes(`main 문서에 표지 \`[!계획] ${id}\``) && !brief.includes("첫 commit"), brief);
      const tree = shown.work_dir.replace(/\/\.aaa\/[^/]+\/?$/, "");
      git(tree, "merge-base", "--is-ancestor", mainHead(), "HEAD");
      Object.assign(handed, { id, thread });
    } finally {
      await p.context().close();
    }
  });

  await step("넘긴 블록 고침 알림: main에서 맡긴 블록을 고치면 그 할 일의 구현 thread에 보낸다", async () => {
    const d = aaa("plan", "show", "app", "docs/exit.md").doc;
    save("docs/exit.md", d.text.replace("**층 이벤트** — 다섯 층마다 이벤트 방.", "**층 이벤트** — 다섯 층마다 이벤트 방. 보스 층에는 없다."), d.blob, "boss");
    const note = `${handed.id} → ${handed.thread}`;
    await until("고침 알림", () => branches().find((b) => b.slug === "boss").events.some((e) => e.kind === "relay" && e.text.startsWith(note)), 60_000);
  });

  for (const name of ["chromium", "webkit"]) await step(`휴대폰: 트리·문서·대화 칸 ${name} 390px`, async () => {
    const wk = name === "webkit" ? await webkit.launch({ headless: true }) : null;
    const { page: p } = await pairPage(wk ?? browser, aaa("web", "pair").code, { width: 390, height: 844 });
    try {
      await p.goto(`${new URL(p.url()).origin}/#design=${encodeURIComponent("app/docs/loop/README.md")}`);
      await p.getByTestId("design-body").waitFor();
      await p.getByTestId("design-history").waitFor();
      if (name === "chromium") await shot(p, "design-main-phone-doc");
      assert.equal(await p.getByTestId("design-chat-pane").isVisible(), false, "좁은 화면은 한 칸씩");
      await p.getByTestId("design-page-chat").click();
      await p.getByTestId("design-chat").waitFor();
      assert.equal(await p.getByTestId("design-body").isVisible(), false);
      await p.getByTestId("design-page-tree").click();
      await p.locator('[data-testid="design-repo"][data-repo="app"]').last().waitFor();
    } finally {
      await p.context().close();
      await wk?.close();
    }
  });
}

// 기획 2-3(t-1894, 확정 시안 library/mock2): 실제 host·agent·git origin으로 기획 branch 웹 화면을 본다.
// 트리 branch 표시(저장소 수·문서 칩·branch에서 새로 만든 문서) → branch 판 보기(판 고르기·main과 다른 덩어리·다른 branch 알림)
// → 고치기와 그림 붙이기(그 branch에 commit, 그림은 그 판 주소) → 대화 고르기·새 대화 → main에 반영(반영 thread) → 버리기
// → 블록 표지(3-1, 확정 시안 library/mock4: 넘긴 할 일 칩·이상 표시·글 속 d-ID·보기 셋·트리 수와 「할 것」·대화로 정하기)
// → 구현 맡기기(3-2, 확정 시안 library/mock5: 고른 블록 → 할 일 하나·표지 commit·바로 올림·예약한 구현 thread는 그 기획 branch에서)
// → 넘긴 블록 고침 알림·main 문서에서 바로 맡기기(3-3, 확정 시안 library/mock6) → 휴대폰 칸.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import zlib from "node:zlib";
import { aaa, root, step, browser, webkit, pairPage, inList, domIs, until } from "../lib/harness.mjs";

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

/** 블록 표지 문서(3-1): 넘긴 할 일 셋(일함·기다림·끝남)과 없는 d-9999, 아직 안 넘긴 계획 하나, 미정 하나 */
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

## 바뀐 것

- 11-16: 통로를 ${d.work}로 넘김.
`;

const branches = () => aaa("plan", "list").repos.find((r) => r.repo === "app").branches;
const save = (slug, path, text, base) => {
  fs.writeFileSync(`${root}/draft.md`, text);
  return aaa("plan", "save", "app", path, "--branch", slug, ...(base ? ["--base", base] : ["--new"]), "--file", `${root}/draft.md`);
};

export default async function () {
  let page;
  const tasks = {};
  // 3-2에서 맡긴 할 일과 그 구현 thread(3-3 고침 알림이 쓴다)
  const handed = {};
  await step("기획 branch 둘: 같은 문서를 고치고 한쪽은 새 문서를 더한다", async () => {
    const origin = `${root}/origins/app.git`, seed = `${root}/seed/app`, clone = `${root}/app`;
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
    // aaa.toml이 있으면 그것이 프로젝트 설정이다: thread profile도 여기에 둔다(반영 thread가 쓴다)
    fs.writeFileSync(`${root}/projects/demo/aaa.toml`, `schema = 1\nthread_profile = "fake"\n\n[[workspaces]]\npath = "${clone}"\nmachine = "labA"\ndesign = "docs"\n`);
    // 표지 문서는 첫 트리 읽기 전에 origin main에 올린다(agent는 같은 저장소를 1분에 한 번 받는다)
    for (const [k, title] of [["work", "가호 칸 6칸"], ["wait", "나가기 통로"], ["done", "층 표 상한"]]) {
      aaa("task", "add", title, "--list", "Backlog");
      tasks[k] = aaa("task", "list").tasks.find((t) => t.title === title).short_id;
    }
    fs.writeFileSync(`${seed}/docs/exit.md`, EXIT(tasks));
    git(seed, "add", ".");
    git(seed, "commit", "-q", "-m", "표지");
    git(seed, "push", "-q", "origin", "HEAD:main");
    aaa("plan", "new", "app", "shop", "--title", "중간 상점");
    const blob = aaa("plan", "show", "app", "docs/loop/README.md", "--branch", "shop").doc.blob;
    save("shop", "docs/loop/README.md", LOOP.replace("지금 20층까지 있다.\n", "지금 20층까지 있다.\n\n> [!계획]\n> **중간 상점** — 다섯 층마다 한 번.\n"), blob);
    aaa("plan", "new", "app", "floors", "--title", "100층까지");
    save("floors", "docs/loop/README.md", LOOP.replace("지금 20층까지 있다.", "지금 20층까지 있다. 100층까지 늘린다."), blob);
    save("floors", "docs/loop/floors.md", "# 층 표\n\n100층까지.\n");
    assert.deepEqual(branches().map((b) => [b.slug, b.changed.length]), [["shop", 1], ["floors", 2]]);
  }, { always: true });

  await step("트리 branch 표시와 branch 판 보기", async () => {
    ({ page } = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 900 }));
    await page.evaluate(() => localStorage.setItem("herdr-web-ui:design-chat:app", "shop"));
    await inList(page, "activity-design");
    const repoRow = page.locator('[data-testid="design-repo"][data-repo="app"]');
    assert.equal((await repoRow.getByTestId("design-repo-branches").first().textContent()).trim(), "2");
    await inList(page, page.locator('[data-testid="design-dir"][data-path="docs/loop"]'));
    const readme = page.locator('[data-testid="design-doc"][data-path="docs/loop/README.md"]');
    assert.equal((await readme.getByTestId("design-doc-branch").textContent()).trim(), "2", "두 branch가 고친 문서");
    const added = page.locator('[data-testid="design-doc"][data-path="docs/loop/floors.md"]');
    assert.equal((await added.getByTestId("design-doc-branch").textContent()).trim(), "floors", "branch에서 새로 만든 문서");
    await readme.click();
    const tabs = page.getByTestId("design-tab");
    await domIs(page, "판 고르기", () => document.querySelectorAll("[data-testid=design-tab]").length === 3);
    assert.deepEqual(await tabs.evaluateAll((els) => els.map((e) => e.dataset.tab)), ["main", "shop", "floors"]);
    // 고른 대화(shop)가 고친 문서는 그 판으로 연다: main과 다른 덩어리(중간 상점)만 표시하고 다른 branch를 알린다
    await page.getByTestId("design-body").getByText("중간 상점").first().waitFor();
    assert.match(await page.getByTestId("design-meta").textContent(), /design\/shop/);
    assert.equal(await page.getByTestId("design-changed").count(), 1);
    assert.match(await page.getByTestId("design-other").textContent(), /floors도 이 문서를 고침/);
    await tabs.filter({ hasText: "main" }).click();
    await domIs(page, "main 판", () => !document.querySelector("[data-testid=design-changed]") && !document.querySelector("[data-testid=design-body]")?.textContent.includes("중간 상점"));
  });

  await step("고치기와 그림 붙이기: 그 branch에 commit하고 그림은 그 판 주소로 보인다", async () => {
    await page.getByTestId("design-tab").filter({ hasText: "shop" }).click();
    await page.getByTestId("design-body").getByText("중간 상점").first().waitFor();
    await page.getByTestId("design-edit").click();
    const editor = page.getByTestId("design-editor");
    await editor.waitFor();
    await editor.evaluate((t) => { t.focus(); t.setSelectionRange(t.value.length, t.value.length); });
    const before = branches().find((b) => b.slug === "shop").head;
    await page.getByTestId("design-attach-input").setInputFiles({ name: "지도.png", mimeType: "image/png", buffer: png(200, 100, [80, 120, 200]) });
    await domIs(page, "그림 줄", () => /!\[지도\]\([0-9a-f]{16}\.png\)/.test(document.querySelector("[data-testid=design-editor]").value));
    const pic = (await editor.inputValue()).match(/\(([0-9a-f]{16}\.png)\)/)[1];
    const shop = branches().find((b) => b.slug === "shop");
    assert.notEqual(shop.head, before, "그림은 그 branch에 따로 commit한다");
    assert.ok(shop.changed.some((c) => c.path === `docs/loop/${pic}` && c.status === "A"), JSON.stringify(shop.changed));
    await page.getByTestId("design-edit-save").click();
    await page.getByTestId("design-body").waitFor();
    await domIs(page, "그 판의 그림", () => { const i = document.querySelector("[data-testid=design-body] img"); return !!i && i.complete && i.naturalWidth === 200; });
    assert.match(await page.getByTestId("design-body").locator("img").getAttribute("src"), /\?c=[0-9a-f]{40}&b=shop$/);
    const after = branches().find((b) => b.slug === "shop");
    assert.ok(after.edited.includes("docs/loop/README.md"), "사람이 고친 문서는 다음 말 앞에 알린다");
    // 준비 단계의 CLI 저장도 사람이 고친 것이다: 그 뒤 그림과 웹 저장이 차례로 붙는다
    assert.deepEqual(after.events.filter((e) => e.kind === "edit").map((e) => e.text).slice(-2), [`docs/loop/${pic}`, "docs/loop/README.md"]);
    await page.getByTestId("design-context").getByText("사람이 고침").waitFor();
  });

  await step("대화 고르기와 새 대화", async () => {
    await page.getByTestId("design-pick").click();
    assert.equal(await page.getByTestId("design-pick-row").count(), 2);
    await page.getByTestId("design-new").click();
    const dialog = page.getByTestId("design-new-dialog");
    await page.getByTestId("design-new-title").fill("Boss notes");
    assert.equal(await page.getByTestId("design-new-slug").inputValue(), "boss-notes", "영문 주제는 branch 이름을 짓는다");
    await page.getByTestId("design-new-slug").fill("Bad_Name");
    assert.ok(await page.getByTestId("design-new-submit").isDisabled());
    await page.getByTestId("design-new-slug").fill("boss");
    await page.getByTestId("design-new-submit").click();
    await dialog.waitFor({ state: "detached" });
    assert.ok(branches().some((b) => b.slug === "boss" && b.title === "Boss notes"));
    await page.getByTestId("design-chat-branch").filter({ hasText: "boss" }).waitFor();
    assert.match(await page.getByTestId("design-mark").first().textContent(), /새 대화 · design\/boss · main [0-9a-f]{7} 에서/);
  });

  await step("main에 반영: 고친 파일·줄 수를 보이고 그 branch에서 갈라진 thread를 띄운다", async () => {
    await page.getByTestId("design-pick").click();
    await page.getByTestId("design-pick-row").filter({ hasText: "중간 상점" }).click();
    await page.getByTestId("design-apply").click();
    const dialog = page.getByTestId("design-apply-dialog");
    await dialog.waitFor();
    assert.match(await dialog.getByTestId("design-files").textContent(), /loop\/README\.md\+\d+−\d+/);
    await page.getByTestId("design-apply-submit").click();
    await domIs(page, "반영 요청 끝", () => !!document.querySelector("[data-testid=design-applying]") || !!document.querySelector("[data-testid=design-apply-dialog] .confirm-error"));
    const problem = (await dialog.locator(".confirm-error").count()) ? await dialog.locator(".confirm-error").textContent() : null;
    assert.equal(problem, null, "반영 요청이 거부되지 않는다");
    await page.getByTestId("design-applying").waitFor();
    const shop = branches().find((b) => b.slug === "shop");
    assert.ok(shop.apply, "반영 thread");
    assert.equal(shop.pushed, shop.head, "올린 뒤 띄운다");
    const t = aaa("thread", "show", shop.apply);
    assert.equal(t.thread_kind, "worktree");
  });

  await step("버리기: 그 대화의 branch를 지우고 트리에서 사라진다", async () => {
    await page.getByTestId("design-pick").click();
    await page.getByTestId("design-pick-row").filter({ hasText: "100층까지" }).click();
    await page.getByTestId("design-chat-more").click();
    await page.getByTestId("design-discard-open").click();
    const dialog = page.getByTestId("design-discard-dialog");
    assert.match(await dialog.textContent(), /main에 없는 commit 2개/);
    await page.getByTestId("design-discard-submit").click();
    await dialog.waitFor({ state: "detached" });
    assert.ok(!branches().some((b) => b.slug === "floors"));
    await page.locator('[data-testid="design-doc"][data-path="docs/loop/floors.md"]').waitFor({ state: "detached" });
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
      // 트리 줄 끝 수: 진행(같은 d-ID 한 번)·계획·미정. 「할 것」은 표지 없는 문서를 한 줄로 접는다
      const exit = p.locator('[data-testid="design-doc"][data-path="docs/exit.md"]');
      await exit.waitFor();
      assert.deepEqual(await exit.locator("[data-testid=design-marks] span").allTextContents(), ["4", "1", "1"]);
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
      const working = (await state(tasks.work).textContent()).trim();
      assert.match(working, new RegExp(`^(작업 중|검수 대기) ${tasks.work} · ${t.id}$`), "thread가 맡은 할 일");
      assert.equal(await state(tasks.work).getAttribute("href"), `#task=${tasks.work}`);
      // main 판은 표지 사이의 보통 글을 한 번에 그린다: 빈 줄 뒤 목록의 이어지는 문단이 덩어리마다 끊기지 않는다
      assert.equal(await p.getByTestId("design-body").locator("li", { hasText: "두루마리는 상자에서만 나온다" }).count(), 1, "목록 안의 문단");
      // 글 속 d-ID는 같은 칩이다
      assert.equal((await p.getByTestId("design-inline-state").textContent()).trim(), working);
      assert.deepEqual(
        await p.locator("[data-testid=design-callout]").evaluateAll((els) => els.map((e) => e.dataset.kind)),
        ["plan", "plan", "open", "plan", "plan", "plan"],
      );
      assert.equal((await p.getByTestId("design-count-work").textContent()).trim(), "진행 4");
      assert.equal((await p.getByTestId("design-count-plan").textContent()).trim(), "계획 1");
      assert.equal((await p.getByTestId("design-count-open").textContent()).trim(), "미정 1");
      // 지금 모습: 블록과 블록만 남은 절(상한)을 숨긴다
      await p.locator("[data-testid=design-look][data-look=now]").click();
      await domIs(p, "지금 모습", () => !document.querySelector("[data-testid=design-callout]"));
      const now = await p.getByTestId("design-body").textContent();
      assert.ok(now.includes("지금 통로는 계단 하나다") && now.includes("바뀐 것") && !now.includes("상한"), now);
      // 남은 것: 절마다 「일부 구현」·「계획만」, 블록마다 한 줄
      await p.locator("[data-testid=design-look][data-look=left]").click();
      await p.getByTestId("design-left").waitFor();
      assert.deepEqual(await p.locator(".design-left-group h3").allTextContents(), ["통로일부 구현", "상한계획만"]);
      assert.equal(await p.getByTestId("design-left-row").count(), 6);
      // 대화로 정하기: 고른 블록을 대화 입력의 맥락 칩으로 넣고, 빼면 사라진다
      await p.getByTestId("design-left").getByTestId("design-discuss").click();
      assert.equal((await p.getByTestId("design-picked").textContent()).trim(), "고른 블록 통로 · 스킬 무기가 나오는 확률");
      await p.getByRole("button", { name: "고른 블록 빼기" }).click();
      await p.getByTestId("design-picked").waitFor({ state: "detached" });
    } finally {
      await p.context().close();
    }
  });

  await step("구현 맡기기: 고른 계획 블록을 할 일 하나로 넘기고 표지 commit을 올리며, 예약한 구현 thread는 그 기획 branch에서 갈라진다", async () => {
    // boss 대화 branch에서 표지 문서에 계획 하나를 더한다(넘기지 않은 계획: 캠프 지도·층 이벤트)
    const d = aaa("plan", "show", "app", "docs/exit.md", "--branch", "boss").doc;
    save("boss", "docs/exit.md", d.text.replace("## 바뀐 것", "> [!계획]\n> **층 이벤트** — 다섯 층마다 이벤트 방.\n\n## 바뀐 것"), d.blob);
    const { page: p } = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 900 });
    try {
      await p.evaluate(() => localStorage.setItem("herdr-web-ui:design-chat:app", "boss"));
      await p.goto(`${new URL(p.url()).origin}/#design=${encodeURIComponent("app/docs/exit.md")}`);
      // main 판에도 맡기기 단추가 있다(3-3): boss 판으로 바뀐 뒤에 센다
      await domIs(p, "boss 판", () => !!document.querySelector("[data-testid=design-meta]")?.textContent.includes("design/boss"));
      await p.getByTestId("design-handoff").first().waitFor();
      assert.equal(await p.getByTestId("design-handoff").count(), 2, "기획 branch 판의 넘기지 않은 계획만");
      // 창을 연 뒤 그 문서가 바뀌면(대화 thread·다른 기기의 저장) 맡기지 않는다: 창은 연 때의 판으로 보내고 host가 다시 본다
      await p.getByTestId("design-handoff").first().click();
      const first = p.getByTestId("design-hand-dialog");
      await first.waitFor();
      const now = aaa("plan", "show", "app", "docs/exit.md", "--branch", "boss").doc;
      save("boss", "docs/exit.md", now.text.replace("- 11-16: 통로를", "- 11-16: 상한 표지를 정리함.\n- 11-16: 통로를"), now.blob);
      await p.getByTestId("design-hand-submit").click();
      await first.locator(".confirm-error").first().waitFor();
      assert.match(await first.locator(".confirm-error").first().textContent(), /그 사이 문서가 바뀌었다/);
      assert.ok(!aaa("task", "list").tasks.some((t) => t.title === "캠프 지도"), "할 일을 만들지 않았다");
      await first.getByRole("button", { name: "닫기" }).click();
      await first.waitFor({ state: "detached" });
      // 새 판을 읽은 뒤 고른다
      await p.getByTestId("design-doc-reload").click();
      await domIs(p, "새 판", () => !!document.querySelector("[data-testid=design-body]")?.textContent.includes("상한 표지를 정리함"));
      await p.locator("[data-testid=design-look][data-look=left]").click();
      const checks = p.getByTestId("design-left-check");
      assert.equal(await checks.count(), 2, "미정·넘긴 줄은 고르지 않는다");
      await checks.nth(0).check();
      await checks.nth(1).check();
      await p.getByTestId("design-left-handoff").click();
      const dialog = p.getByTestId("design-hand-dialog");
      await dialog.waitFor();
      const title = "상한 — 캠프 지도 · 층 이벤트";
      assert.equal(await p.getByTestId("design-hand-title").inputValue(), title);
      assert.match(await p.getByTestId("design-hand-memo").inputValue(), /^\[기획 app docs\/exit\.md 「상한」\]\(design\/app\/docs\/exit\.md#상한\)\n- \*\*캠프 지도\*\*/);
      assert.ok(await p.getByTestId("design-hand-reserve").isChecked(), "예약이 기본(q-3957 2A)");
      await p.getByTestId("design-hand-submit").click();
      await dialog.waitFor({ state: "detached" });
      const id = aaa("task", "list").tasks.find((t) => t.title === title)?.short_id;
      assert.ok(id, "할 일 하나");
      // 고른 두 블록 표지에 같은 d-ID를 적어 그 branch에 commit하고 바로 올렸다
      const text = aaa("plan", "show", "app", "docs/exit.md", "--branch", "boss").doc.text;
      assert.ok(text.includes(`> [!계획] ${id}\n> **캠프 지도**`) && text.includes(`> [!계획] ${id}\n> **층 이벤트**`), text);
      const boss = branches().find((b) => b.slug === "boss");
      assert.equal(boss.pushed, boss.head, "바로 올린다");
      assert.ok(boss.events.some((e) => e.kind === "handoff" && e.text === `${id} · 블록 2`), JSON.stringify(boss.events));
      // 화면: 두 블록이 같은 할 일의 칩이 되고 대화에 「구현 맡김」 줄이 낀다
      await p.locator("[data-testid=design-look][data-look=all]").click();
      await domIs(p, "넘긴 블록 칩", (task) => document.querySelectorAll(`[data-testid=design-callout][data-task="${task}"]`).length === 2, id);
      assert.equal(await p.getByTestId("design-handoff").count(), 0);
      await p.getByTestId("design-mark").filter({ hasText: `구현 맡김 ${id} · 블록 2` }).waitFor();
      // 예약한 할 일은 자리가 나면 host가 띄운다. 그 thread는 기획 branch 끝에서 갈라지고 brief에 맡긴 자리가 있다
      const thread = await until("구현 thread", () => aaa("task", "list").tasks.find((t) => t.short_id === id)?.thread, 60_000)
        .catch((e) => { e.message += ` 예약=${JSON.stringify(aaa("task", "queue")).slice(0, 1500)}`; throw e; });
      const shown = await until("thread 작업 폴더", () => aaa("thread", "show", thread).work_dir && aaa("thread", "show", thread), 60_000);
      const brief = fs.readFileSync(`${shown.work_dir}/brief.md`, "utf8");
      assert.ok(brief.includes("## 기획에서 맡긴 일") && brief.includes(`기획 branch \`design/boss\``) && brief.includes(`\`[!계획] ${id}\` 표지를 풀고`), brief);
      const tree = shown.work_dir.replace(/\/\.aaa\/[^/]+\/?$/, "");
      git(tree, "merge-base", "--is-ancestor", boss.head, "HEAD");
      Object.assign(handed, { id, thread });
    } finally {
      await p.context().close();
    }
  });

  await step("넘긴 블록 고침 알림: 기획 branch에서 맡긴 블록을 고치면 올린 뒤 그 할 일의 구현 thread에 보낸다", async () => {
    const d = aaa("plan", "show", "app", "docs/exit.md", "--branch", "boss").doc;
    save("boss", "docs/exit.md", d.text.replace("**층 이벤트** — 다섯 층마다 이벤트 방.", "**층 이벤트** — 다섯 층마다 이벤트 방. 보스 층에는 없다."), d.blob);
    const note = `${handed.id} → ${handed.thread}`;
    await until("고침 알림", () => branches().find((b) => b.slug === "boss").events.some((e) => e.kind === "relay" && e.text === note), 60_000);
    const boss = branches().find((b) => b.slug === "boss");
    assert.equal(boss.pushed, boss.head, "알리기 전에 올린다");
  });

  await step("main 문서에서 바로 맡기기: 기획 branch 없이 할 일·예약, 착지 전 main 판에는 「표지는 구현 branch에」 칩", async () => {
    const { page: p } = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 900 });
    try {
      // 고른 대화(shop)는 이 문서를 고치지 않았다: main 판으로 연다
      await p.evaluate(() => localStorage.setItem("herdr-web-ui:design-chat:app", "shop"));
      await p.goto(`${new URL(p.url()).origin}/#design=${encodeURIComponent("app/docs/exit.md")}`);
      await p.getByTestId("design-handoff").first().waitFor();
      assert.equal(await p.getByTestId("design-handoff").count(), 1, "main 판의 넘기지 않은 계획(캠프 지도)");
      await p.getByTestId("design-handoff").click();
      const dialog = p.getByTestId("design-hand-dialog");
      await dialog.waitFor();
      await p.getByTestId("design-hand-main").waitFor();
      await p.getByTestId("design-hand-title").fill("main에서 캠프 지도");
      const before = branches().map((b) => [b.slug, b.head]);
      await p.getByTestId("design-hand-submit").click();
      await dialog.waitFor({ state: "detached" });
      const id = aaa("task", "list").tasks.find((t) => t.title === "main에서 캠프 지도")?.short_id;
      assert.ok(id, "할 일");
      assert.deepEqual(branches().map((b) => [b.slug, b.head]), before, "main 맡김은 어느 branch에도 commit하지 않는다");
      await domIs(p, "main 맡김 칩", (task) => !!document.querySelector(`[data-testid=design-callout][data-task="${task}"] .design-pending`), id);
      assert.equal(await p.getByTestId("design-handoff").count(), 0, "맡긴 블록은 다시 맡기지 않는다");
      // 예약한 구현 thread: main에서 갈라지고 brief가 첫 commit으로 표지를 적으라 한다
      const thread = await until("구현 thread", () => aaa("task", "list").tasks.find((t) => t.short_id === id)?.thread, 60_000);
      const shown = await until("thread 작업 폴더", () => aaa("thread", "show", thread).work_dir && aaa("thread", "show", thread), 60_000);
      const brief = fs.readFileSync(`${shown.work_dir}/brief.md`, "utf8");
      assert.ok(brief.includes("main 문서에서 맡김") && brief.includes(`\`[!계획] ${id}\``) && brief.includes("**캠프 지도**"), brief);
    } finally {
      await p.context().close();
    }
  });

  for (const name of ["chromium", "webkit"]) await step(`휴대폰: 트리·문서·대화 칸 ${name} 390px`, async () => {
    const wk = name === "webkit" ? await webkit.launch({ headless: true }) : null;
    const { page: p } = await pairPage(wk ?? browser, aaa("web", "pair").code, { width: 390, height: 844 });
    try {
      await p.goto(`${new URL(p.url()).origin}/#design=${encodeURIComponent("app/docs/loop/README.md")}`);
      await p.getByTestId("design-body").waitFor();
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

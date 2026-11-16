// 기획 2-3(t-1894, 확정 시안 library/mock2): 실제 host·agent·git origin으로 기획 branch 웹 화면을 본다.
// 트리 branch 표시(저장소 수·문서 칩·branch에서 새로 만든 문서) → branch 판 보기(판 고르기·main과 다른 덩어리·다른 branch 알림)
// → 고치기와 그림 붙이기(그 branch에 commit, 그림은 그 판 주소) → 대화 고르기·새 대화 → main에 반영(반영 thread) → 버리기 → 휴대폰 칸.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import zlib from "node:zlib";
import { aaa, root, step, browser, webkit, pairPage, inList, domIs } from "../lib/harness.mjs";

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

const branches = () => aaa("plan", "list").repos.find((r) => r.repo === "app").branches;
const save = (slug, path, text, base) => {
  fs.writeFileSync(`${root}/draft.md`, text);
  return aaa("plan", "save", "app", path, "--branch", slug, ...(base ? ["--base", base] : ["--new"]), "--file", `${root}/draft.md`);
};

export default async function () {
  let page;
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

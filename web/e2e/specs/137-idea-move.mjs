// 기획 4단계(t-1894, 확정 시안 library/mock7 q-3980, 규칙 q-3981~q-3985): 아이디어는 저장소 기획 문서로 옮겼다. 옛 아이디어(옛 판이 만든
// 폴더 견본 web/e2e/fixtures/ideas)는 읽기 전용이고, 다듬던 것은 「기획으로 옮기기」로 그 저장소 기획 대화에 넘긴다.
// 쓰기 거절(CLI·Front) → 기획 트리 「옛 아이디어」와 읽기 전용 화면 → 옮기기(그림 commit·대화가 받은 글·옮기는 중) → 버리면 풀림
// → main에 들어가면 「옮김 → 문서」 → 들어간 뒤 곧 버려도 옮김 → 할 일 메모의 아이디어 링크 → 휴대폰.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import { DatabaseSync } from "node:sqlite";
import { aaa, root, base, step, browser, webkit, pairPage, inList, domIs, until, finishFixtureFollowups, start, stop } from "../lib/harness.mjs";

const git = (cwd, ...args) =>
  execFileSync("git", ["-c", "user.name=demo", "-c", "user.email=demo@example.com", "-c", "init.defaultBranch=main", ...args], { cwd, stdio: ["ignore", "pipe", "pipe"] }).toString().trim();

const call = (page, request) => page.evaluate(async (request) => {
  const r = await fetch("/api/v1/call", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(request) });
  return r.json();
}, request);

const branches = () => aaa("plan", "list").repos.find((r) => r.repo === "app").branches;
const ideas = () => aaa("idea", "list").ideas;
const idea = (short) => ideas().find((i) => i.short_id === short);
const fixtures = new URL("../fixtures/ideas/", import.meta.url).pathname;
const CAMP_PIC = "a6d715956dab435e.png";

export default async function () {
  const ids = {};
  let page, seed, campSlug, campPic;
  await step("옛 아이디어 셋(그림 있는 판 2·옛 쓰기 칸이 남은 것·완료)과 기획 폴더 저장소", async () => {
    const origin = `${root}/origins/app.git`, clone = `${root}/app`;
    seed = `${root}/seed/app`;
    for (const d of [`${root}/origins`, seed]) fs.mkdirSync(d, { recursive: true });
    git(root, "init", "--bare", "-q", origin);
    git(seed, "init", "-q");
    fs.mkdirSync(`${seed}/docs/loop`, { recursive: true });
    fs.writeFileSync(`${seed}/docs/README.md`, "# 개요\n");
    fs.writeFileSync(`${seed}/docs/loop/README.md`, "# 게임 루프\n\n탑 앞 캠프에서 1층부터 오른다.\n");
    git(seed, "add", ".");
    git(seed, "commit", "-q", "-m", "처음");
    git(seed, "remote", "add", "origin", origin);
    git(seed, "push", "-q", "origin", "HEAD:main");
    git(root, "clone", "-q", origin, clone);
    aaa("project", "create", "demo", "--thread-profile", "fake");
    fs.writeFileSync(`${root}/projects/demo/aaa.toml`, `schema = 1\nthread_profile = "fake"\n\n[[workspaces]]\npath = "${clone}"\nmachine = "labA"\ndesign = "docs"\n`);
    fs.cpSync(fixtures, `${root}/projects/demo/ideas`, { recursive: true });
    for (const i of ideas()) ids[{ "캠프 게시판 의뢰": "camp", "보스 연속 층": "boss", "초반 튜토리얼": "tutorial" }[i.title]] = i.short_id;
    assert.deepEqual(Object.keys(ids).sort(), ["boss", "camp", "tutorial"]);
    assert.deepEqual([idea(ids.camp).status, idea(ids.camp).rev, idea(ids.tutorial).status], ["open", 2, "done"]);
    campSlug = `idea-${ids.camp}`;
  }, { always: true });

  await step("아이디어 쓰기는 CLI·Front 모두 까닭과 함께 거절하고 읽기는 남는다", async () => {
    for (const args of [["idea", "new", "새 아이디어"], ["idea", "save", ids.camp, "--base", "2", "--file", "x.md"], ["idea", "say", ids.camp, "말"], ["idea", "attach", ids.camp, "x.png"], ["idea", "model", ids.camp, "fake"]]) {
      assert.throws(() => aaa(...args), /옛 아이디어는 읽기 전용이다.*aaa plan/s, args.join(" "));
    }
    const shown = aaa("idea", "show", ids.camp).note;
    assert.equal(shown.rev, 2);
    assert.deepEqual(shown.media.map((m) => m.name), [CAMP_PIC]);
    ({ page } = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 900 }));
    for (const request of [{ op: "idea_save", idea: ids.camp, base_rev: 2, text: "# 고침\n" }, { op: "idea_say", idea: ids.camp, text: "말" }, { op: "idea_create", title: "새", text: "" }]) {
      const r = await call(page, request);
      assert.match(JSON.stringify(r.error ?? r), /옛 아이디어는 읽기 전용이다/, request.op);
    }
    assert.equal(idea(ids.camp).rev, 2, "노트는 그대로");
  });

  await step("기획 트리 「옛 아이디어」와 읽기 전용 아이디어 화면", async () => {
    await inList(page, "activity-design");
    assert.equal(await page.getByTestId("activity-ideas").count(), 0, "활동 막대 「아이디어」는 기획에 합쳤다");
    const group = page.locator("[data-testid=design-ideas]:visible").first();
    await group.waitFor();
    const row = (short) => group.locator(`[data-testid=design-idea][title^="${short} "]`);
    assert.equal(await group.getByTestId("design-idea").count(), 3);
    assert.deepEqual([await row(ids.camp).getAttribute("data-state"), await row(ids.boss).getAttribute("data-state"), await row(ids.tutorial).getAttribute("data-state")], ["open", "open", "done"]);
    assert.equal(await group.getByTestId("design-idea-move").count(), 2, "다듬는 중만 옮긴다");
    assert.equal((await row(ids.tutorial).getByTestId("design-idea-state").textContent()).trim(), "완료");
    await row(ids.camp).locator(".design-idea-open").click();
    const view = page.getByTestId("idea-view");
    await domIs(page, "아이디어 화면", () => document.querySelector("[data-testid=idea-view]")?.dataset.shown === "true");
    assert.equal(await page.getByTestId("idea-legacy").getAttribute("data-state"), "open");
    assert.equal((await page.getByTestId("idea-status").textContent()).trim(), "다듬는 중");
    await page.getByTestId("idea-move").waitFor();
    for (const gone of ["idea-edit", "idea-handoff", "idea-media-add", "idea-model"]) assert.equal(await view.getByTestId(gone).count(), 0, gone);
    assert.equal(await view.locator("[data-testid=idea-chat] textarea").count(), 0, "대화 입력창이 없다");
    await view.getByTestId("idea-note").locator("img").evaluate((img) => img.decode());
    // 옛 쓰기 칸(합치는 중·보낼 말·까닭)이 남은 노트도 다듬는 중으로 읽힌다
    await row(ids.boss).locator(".design-idea-open").click();
    await domIs(page, "보스 노트", () => document.querySelector("[data-testid=idea-title]")?.textContent === "보스 연속 층");
    assert.equal((await page.getByTestId("idea-status").textContent()).trim(), "다듬는 중");
    assert.equal(await view.getByTestId("idea-waiting").count(), 0);
    // 넓은 창의 노트·대화 칸 조절은 그대로다
    const grip = page.getByRole("separator", { name: "노트와 대화 크기" });
    await grip.focus();
    await page.keyboard.press("Home");
    assert.equal(await grip.getAttribute("aria-valuenow"), "25");
    await grip.dblclick();
    assert.equal(await grip.getAttribute("aria-valuenow"), "60");
  });

  await step("옛 thread 기록이 없는 library 그림 링크는 그대로 두고 노트를 읽는다", async () => {
    // 옛 판은 전용 thread의 library 그림을 노트 폴더로 옮겨 왔다. 그 thread 기록(t-0009)이 없으면 링크를 두고 글은 읽힌다
    const folder = `${root}/projects/demo/ideas/${idea(ids.boss).id}`;
    const before = fs.readFileSync(`${folder}/note.md`, "utf8");
    const old = "# 보스 연속 층\n읽을 글\n\n![없는 그림](library/missing.png)\n";
    try {
      for (const path of ["note.md", "revs/0001.md"]) fs.writeFileSync(`${folder}/${path}`, old);
      await page.goto(`${base}/#idea=${ids.boss}`);
      await page.getByTestId("idea-note").getByText("읽을 글", { exact: true }).waitFor();
      assert.equal(await page.getByRole("alert").filter({ hasText: "그림을 옮기지 못했다" }).count(), 0);
      assert.equal(fs.readFileSync(`${folder}/note.md`, "utf8"), old, "읽기는 원문을 바꾸지 않는다");
    } finally {
      for (const path of ["note.md", "revs/0001.md"]) fs.writeFileSync(`${folder}/${path}`, before);
    }
  });

  await step("기획으로 옮기기: 그림을 그 branch 위치 폴더에 commit하고 노트 본문·그림 경로·옮기는 법을 새 기획 대화가 받는다", async () => {
    // 다른 대화를 고른 채로 두어도 옮긴 뒤에는 그 기획 대화를 연다(기억한 대화보다 앞선다)
    aaa("plan", "new", "app", "other", "--title", "다른 대화");
    await page.evaluate(() => localStorage.setItem("herdr-web-ui:design-chat:app", "other"));
    await page.goto(`${base}/#idea=${ids.camp}`);
    await page.getByTestId("idea-move").click();
    const dialog = page.getByTestId("idea-move-dialog");
    await dialog.waitFor();
    // 창은 기획 트리가 오기 전에도 열린다: 트리가 오면 저장소를 고른다
    await domIs(page, "저장소 고름", () => document.querySelector("[data-testid=idea-move-repo]")?.value === "app");
    assert.equal(await page.getByTestId("idea-move-slug").inputValue(), campSlug, "한글 제목은 아이디어 번호로 branch를 짓는다");
    await page.getByTestId("idea-move-where").fill("camp");
    await page.getByTestId("idea-move-submit").click();
    await domIs(page, "옮기기 끝", () => !document.querySelector("[data-testid=idea-move-dialog]") || !!document.querySelector("[data-testid=idea-move-problem]"), null, 90_000);
    assert.equal(await page.getByTestId("idea-move-problem").count() ? await page.getByTestId("idea-move-problem").textContent() : null, null, "옮기기가 거절되지 않는다");
    const b = branches().find((x) => x.slug === campSlug);
    assert.equal(b?.title, "옮김: 캠프 게시판 의뢰");
    const pic = b.changed.find((c) => /^docs\/camp\/[0-9a-f]{16}\.png$/.test(c.path) && c.status === "A");
    assert.ok(pic, JSON.stringify(b.changed));
    campPic = pic.path;
    const moved = idea(ids.camp).moved;
    assert.deepEqual([moved.repo, moved.branch, moved.path, moved.done, moved.rev], ["app", campSlug, "docs/camp/", false, 2]);
    // 옮긴 뒤에는 그 기획 대화가 열린다
    await page.getByTestId("design-chat-branch").filter({ hasText: campSlug }).waitFor();
    const thread = await until("기획 대화 thread", () => branches().find((x) => x.slug === campSlug).thread, 60_000);
    const log = `${root}/fake-lines/${thread}.log`;
    const got = await until("대화가 받은 옮기기 말", () => fs.existsSync(log) && fs.readFileSync(log, "utf8").includes("노트 본문은 아래에 있다") && fs.readFileSync(log, "utf8"), 60_000);
    for (const want of [`옛 아이디어 ${ids.camp} 「캠프 게시판 의뢰」(판 2)를 기획 폴더로 옮긴다.`, "- 위치: `docs/camp/`", `  - \`${CAMP_PIC}\` → \`${campPic}\``,
      "✗, 넘긴 할 일 있음 → `[!계획] d-NNNN`", `노트 ${ids.camp} 판 2:`, "캠프 게시판에서 의뢰를 받아 층을 오른다. 의뢰를 마치면 Gold와 마석을 받는다.", `![게시판 시안](${CAMP_PIC})`]) {
      assert.ok(got.includes(want), `${want}\n---\n${got}`);
    }
    assert.ok(!got.includes("(문서 고침:"), "옮기며 commit한 그림은 「문서 고침」으로 따로 알리지 않는다");
    // 트리와 아이디어 화면: 옮기는 중(읽기 전용), 옮기기 단추 없음
    const row = page.locator("[data-testid=design-ideas]:visible").first().locator(`[data-testid=design-idea][title^="${ids.camp} "]`);
    await domIs(page, "옮기는 중 줄", (short) => document.querySelector(`[data-testid=design-idea][title^="${short} "]`)?.dataset.state === "moving", ids.camp);
    assert.equal(await row.getByTestId("design-idea-move").count(), 0);
    await page.goto(`${base}/#idea=${ids.camp}`);
    await domIs(page, "옮기는 중 띠", () => document.querySelector("[data-testid=idea-legacy]")?.dataset.state === "moving");
    assert.equal((await page.getByTestId("idea-status").textContent()).trim(), "옮기는 중");
    assert.equal(await page.getByTestId("idea-move").count(), 0);
    await page.getByTestId("idea-open-design").click();
    await page.getByTestId("design-chat-branch").filter({ hasText: campSlug }).waitFor();
  });

  await step("대화를 버리면 다시 다듬는 중이 되어 옮길 수 있다", async () => {
    await page.goto(`${base}/#idea=${ids.boss}`);
    await page.getByTestId("idea-move").click();
    await page.getByTestId("idea-move-submit").click();
    await page.getByTestId("idea-move-dialog").waitFor({ state: "detached" });
    assert.equal(idea(ids.boss).moved?.branch, `idea-${ids.boss}`);
    aaa("plan", "discard", "app", `idea-${ids.boss}`);
    assert.equal(idea(ids.boss).moved, undefined, "버리면 풀어 준다");
    await page.goto(`${base}/#idea=${ids.boss}`);
    await page.getByTestId("idea-move").waitFor();
  });

  await step("그 branch가 main에 들어가면 「옮김 → 문서」이고 그 문서를 연다", async () => {
    // 대화가 위치 폴더에 문서를 쓴다(그림은 같은 폴더 이름으로 잇는다)
    const name = campPic.split("/").pop();
    fs.writeFileSync(`${root}/board.md`, `# 캠프 게시판\n\n의뢰는 셋 중에서 고른다.\n\n![게시판 시안](${name})\n`);
    aaa("plan", "save", "app", "docs/camp/board.md", "--branch", campSlug, "--new", "--file", `${root}/board.md`);
    await until("옮긴 경로를 고친 문서로", () => idea(ids.camp).moved?.path === "docs/camp/board.md", 90_000);
    // 착지: 그 branch를 main에 병합해 올린다(반영 thread의 착지와 같은 결과)
    git(seed, "fetch", "-q", `${root}/app`, `design/${campSlug}`);
    git(seed, "merge", "--no-ff", "-q", "-m", "병합: 캠프 게시판 옮김", "FETCH_HEAD");
    git(seed, "push", "-q", "origin", "HEAD:main");
    await until("옮김", () => idea(ids.camp).moved?.done === true, 180_000);
    assert.equal(idea(ids.camp).moved.path, "docs/camp/board.md");
    await page.goto(`${base}/#idea=${ids.camp}`);
    await domIs(page, "옮김 띠", () => document.querySelector("[data-testid=idea-legacy]")?.dataset.state === "moved");
    assert.match(await page.getByTestId("idea-legacy").textContent(), /app docs\/camp\/board\.md/);
    assert.equal((await page.getByTestId("idea-status").textContent()).trim(), "옮김");
    const row = page.locator("[data-testid=design-ideas]:visible").first().locator(`[data-testid=design-idea][title^="${ids.camp} "]`);
    assert.equal((await row.getByTestId("design-idea-state").textContent()).trim(), "옮김 → app");
    await page.getByTestId("design-reload").first().click();
    await page.getByTestId("idea-open-design").click();
    await domIs(page, "옮긴 문서", () => location.hash === `#design=${encodeURIComponent("app/docs/camp/board.md")}`);
    await page.getByTestId("design-body").getByText("의뢰는 셋 중에서 고른다.").waitFor();
    await domIs(page, "옮긴 그림", () => { const i = document.querySelector("[data-testid=design-body] img"); return !!i && i.complete && i.naturalWidth > 0; });
  });

  await step("main에 들어간 뒤 곧 버려도(origin을 아직 받지 않았어도) 「옮김」으로 남고, 같은 이름으로 다시 만든 대화는 앞 옮기기를 잇지 않는다", async () => {
    // 앞에서 버린 대화와 같은 이름(idea-n-…)으로 다시 만든다. 노트는 긴 것(약 45KB)으로 바꿔 대화가 끝까지 받는지 본다
    const slug = `idea-${ids.boss}`;
    const folder = `${root}/projects/demo/ideas/${idea(ids.boss).id}`;
    const long = ["# 보스 연속 층", "", ...Array.from({ length: 900 }, (_, i) => `- ${i + 1}층 보스 방: 보스 하나와 졸개 둘`), "- 끝 줄: 100층 보스", ""].join("\n");
    for (const path of ["note.md", "revs/0001.md"]) fs.writeFileSync(`${folder}/${path}`, long);
    await page.goto(`${base}/#idea=${ids.boss}`);
    await page.getByTestId("idea-move").click();
    await domIs(page, "저장소 고름", () => document.querySelector("[data-testid=idea-move-repo]")?.value === "app");
    await page.getByTestId("idea-move-where").fill("boss");
    await page.getByTestId("idea-move-submit").click();
    await page.getByTestId("idea-move-dialog").waitFor({ state: "detached" });
    assert.equal(idea(ids.boss).moved?.done, false);
    const chat = await until("다시 만든 대화 thread", () => branches().find((x) => x.slug === slug)?.thread, 60_000);
    const log = `${root}/fake-lines/${chat}.log`;
    const got = await until("긴 노트 끝까지", () => fs.existsSync(log) && fs.readFileSync(log, "utf8").includes("- 끝 줄: 100층 보스") && fs.readFileSync(log, "utf8"), 90_000);
    assert.ok(got.includes("- 450층 보스 방: 보스 하나와 졸개 둘") && !got.includes("바이트만 실었다"), "자르지 않고 다 실었다");
    // 옛 판이 남긴 보내지 못한 말(견본 idea.json `pending`)도 싣는다
    assert.ok(got.includes("닿지 못한 말") && got.includes("91층 보스는 둘로"), got.slice(-800));
    fs.writeFileSync(`${root}/boss.md`, "# 보스 연속 층\n\n91~100층은 방 하나에 보스 하나.\n");
    aaa("plan", "save", "app", "docs/boss/README.md", "--branch", slug, "--new", "--file", `${root}/boss.md`);
    git(seed, "fetch", "-q", "origin", "main");
    git(seed, "reset", "-q", "--hard", "origin/main");
    git(seed, "fetch", "-q", `${root}/app`, `design/${slug}`);
    git(seed, "merge", "--no-ff", "-q", "-m", "병합: 보스 연속 층 옮김", "FETCH_HEAD");
    git(seed, "push", "-q", "origin", "HEAD:main");
    aaa("plan", "discard", "app", slug);
    assert.equal(idea(ids.boss).moved?.done, true, "버리기 전에 origin main을 새로 물어 들어간 것을 안다");
    assert.ok(!branches().some((b) => b.slug === slug));
  });

  await step("할 일 메모의 아이디어 링크는 노트와 그림을 thread 작업 폴더로 옮긴다", async () => {
    // 옛 판에서 넘긴 할 일의 메모 첫 줄(아이디어 링크)은 그대로 쓰인다
    aaa("task", "add", "간판 만들기", "--list", "Backlog", "--note", `[아이디어 ${ids.camp}](ideas/${ids.camp})`);
    const started = aaa("thread", "start", "--from-task", "간판 만들기", "--link-task", "--machine", "labA", "--profile", "fake", "--kind", "dir");
    const thread = aaa("thread", "show", started.id);
    const folder = `${root}/projects/demo/ideas/${idea(ids.camp).id}`;
    assert.deepEqual(fs.readFileSync(`${thread.work_dir}/attachments/${CAMP_PIC}`), fs.readFileSync(`${folder}/${CAMP_PIC}`));
    const text = fs.readFileSync(`${root}/projects/demo/threads/${started.id}.task.md`, "utf8");
    const link = /\[아이디어 n-\d+\]\(attachments\/([0-9a-f]{16}\.md)\)/.exec(text);
    assert.ok(link, text);
    assert.ok(fs.readFileSync(`${thread.work_dir}/attachments/${link[1]}`, "utf8").includes(`](${CAMP_PIC})`));
    await until("thread 보고", () => aaa("thread", "show", started.id).report_hash, 60_000);
    finishFixtureFollowups(started.id);
    aaa("thread", "resolve", started.id);
    await page.context().close();
  });

  await step("열린 옛 아이디어 thread는 host가 끝내고, 끝내기 전에 그 작업 폴더 library 그림을 노트로 옮기며 작업 폴더는 남긴다", async () => {
    // 새 판에는 아이디어 thread를 만드는 길이 없다: 작업 폴더 thread를 띄운 뒤 host를 멈추고 그 기록에 아이디어 표지를 단다
    fs.writeFileSync(`${root}/old-idea-task.md`, "옛 아이디어 대화\n");
    const t = aaa("thread", "start", "demo", "--title", "옛 아이디어 대화", "--machine", "labA", "--profile", "fake", "--kind", "dir", "--task-file", `${root}/old-idea-task.md`);
    const shown = await until("가짜 thread 보고와 library 그림", () => { const x = aaa("thread", "show", t.id); return x.report_hash && fs.existsSync(`${x.work_dir}/library/pic.png`) && x; }, 60_000);
    const tut = idea(ids.tutorial);
    const folder = `${root}/projects/demo/ideas/${tut.id}`;
    const text = "# 초반 튜토리얼\n\n1~3층 안내.\n\n![시안](library/pic.png)\n";
    for (const path of ["note.md", "revs/0001.md"]) fs.writeFileSync(`${folder}/${path}`, text);
    const meta = JSON.parse(fs.readFileSync(`${folder}/idea.json`, "utf8"));
    meta.thread = t.id;
    fs.writeFileSync(`${folder}/idea.json`, JSON.stringify(meta, null, 2));
    await stop("host");
    const db = new DatabaseSync(`${root}/s/host/host.db`);
    try {
      const rec = JSON.parse(db.prepare("SELECT data FROM threads WHERE project='demo' AND id=?").get(t.id).data);
      rec.idea = tut.id;
      db.prepare("UPDATE threads SET data=? WHERE project='demo' AND id=?").run(JSON.stringify(rec), t.id);
    } finally { db.close(); }
    start("host", "host");
    await until("host와 agent 재연결", () => aaa("machine", "list").machines.some((m) => m.name === "labA" && m.connection === "connected"), 60_000);
    await until("옛 아이디어 thread 끝남", () => aaa("thread", "show", t.id).status === "resolved", 90_000);
    assert.match(fs.readFileSync(`${folder}/note.md`, "utf8"), /!\[시안\]\([0-9a-f]{16}\.png\)/, "끝내기 전에 library 그림을 노트 폴더로 옮겼다");
    assert.ok(fs.existsSync(`${shown.work_dir}/library/pic.png`), "작업 폴더는 남긴다");
  });

  for (const name of ["chromium", "webkit"]) await step(`휴대폰: 옛 아이디어 노트·대화 칸 ${name} 390px`, async () => {
    const wk = name === "webkit" ? await webkit.launch({ headless: true }) : null;
    const { page: p } = await pairPage(wk ?? browser, aaa("web", "pair").code, { width: 390, height: 844 }, { touch: true, hash: `#idea=${ids.camp}` });
    try {
      await p.getByTestId("idea-note").locator("img").waitFor();
      assert.equal(await p.getByTestId("idea-resizer").count(), 0);
      await p.getByTestId("idea-note-pane").getByTestId("idea-open-design").waitFor();
      await p.getByTestId("idea-tab-chat").click();
      await p.getByTestId("idea-chat").waitFor();
      assert.equal(await p.getByTestId("idea-note-pane").isVisible(), false, "한 칸씩");
      await p.getByTestId("idea-tab-note").click();
      await p.getByTestId("idea-note").waitFor();
    } finally {
      await p.context().close();
      await wk?.close();
    }
  });
}

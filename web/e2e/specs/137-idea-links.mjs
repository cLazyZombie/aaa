// 옛 아이디어를 지웠다(t-1959): 기획 트리 아래 「옛 아이디어」 묶음·아이디어 화면·「기획으로 옮기기」·`aaa idea`가 없다.
// 프로젝트 `idea-links.toml`(n-ID → 옮긴 기획 문서)만 남아, 글 속 `n-NNNN`과 옛 할 일 메모 링크(`ideas/n-NNNN`)·옛 주소(`#idea=`)는
// 그 문서로 잇고, 표에 없거나 기획 폴더가 없는 저장소를 가리키면 글자로 둔다(빈 화면으로 가지 않는다).
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import { aaa, root, step, browser, webkit, pairPage, inList, domIs, drawerSettled, hashIs, shot } from "../lib/harness.mjs";

const git = (cwd, ...args) =>
  execFileSync("git", ["-c", "user.name=demo", "-c", "user.email=demo@example.com", "-c", "init.defaultBranch=main", ...args], { cwd, stdio: ["ignore", "pipe", "pipe"] }).toString().trim();

/** origin(bare)과 그 clone. files는 처음 commit에 넣을 파일 */
function repo(name, files) {
  const origin = `${root}/origins/${name}.git`, seed = `${root}/seed/${name}`, clone = `${root}/${name}`;
  fs.mkdirSync(seed, { recursive: true });
  git(`${root}/origins`, "init", "--bare", "-q", origin);
  git(seed, "init", "-q");
  for (const [path, text] of Object.entries(files)) {
    fs.mkdirSync(`${seed}/${path.split("/").slice(0, -1).join("/")}`, { recursive: true });
    fs.writeFileSync(`${seed}/${path}`, text);
  }
  git(seed, "add", ".");
  git(seed, "commit", "-q", "-m", "처음");
  git(seed, "remote", "add", "origin", origin);
  git(seed, "push", "-q", "origin", "HEAD:main");
  git(root, "clone", "-q", origin, clone);
  return clone;
}

const README = "# 개요\n\n캠프 게시판은 아이디어 n-0003에서 왔다. 엔진 구조는 n-0004, 버린 것은 n-0009.\n\n- [아이디어 n-0003](ideas/n-0003)\n";

export default async function () {
  await step("기획 폴더 저장소(app)와 기획 폴더 없는 저장소(eng), n-ID 링크 표, 남은 옛 노트 폴더", async () => {
    fs.mkdirSync(`${root}/origins`, { recursive: true });
    const app = repo("app", { "docs/README.md": README, "docs/camp.md": "# 캠프 게시판\n\n의뢰는 셋 중에서 고른다.\n" });
    const eng = repo("eng", { "doc/design/structure.md": "# 엔진 구조\n" });
    aaa("project", "create", "demo");
    fs.writeFileSync(`${root}/projects/demo/aaa.toml`,
      `schema = 1\n\n[[workspaces]]\npath = "${app}"\nmachine = "labA"\ndesign = "docs"\n\n[[workspaces]]\npath = "${eng}"\nmachine = "labA"\n`);
    // n-0004는 기획 폴더가 없는 저장소를 가리켜 싣지 않는다(열 수 없는 문서로 잇지 않는다)
    fs.writeFileSync(`${root}/projects/demo/idea-links.toml`, `"n-0003" = "app/docs/camp.md"\n"n-0004" = "eng/doc/design/structure.md"\n`);
    // 지우기 전 옛 노트 폴더가 남아 있어도 host는 읽지 않는다
    fs.mkdirSync(`${root}/projects/demo/ideas/01JD0000000000000000000000`, { recursive: true });
    fs.writeFileSync(`${root}/projects/demo/ideas/01JD0000000000000000000000/note.md`, "# 옛 노트\n");
  }, { always: true });

  await step("CLI: `aaa idea`는 없고 `aaa plan links`가 열 수 있는 링크만 보인다", async () => {
    assert.throws(() => aaa("idea", "list"));
    assert.deepEqual(aaa("plan", "links").links, { "n-0003": "app/docs/camp.md" });
  });

  for (const width of [1280, 390]) await step(`기획 트리에 옛 아이디어가 없고, 문서 속 n-ID는 옮긴 문서로 잇는다: ${width}px`, async () => {
    const { page: p } = await pairPage(browser, aaa("web", "pair").code, { width, height: 844 }, width < 600 ? { touch: true } : {});
    try {
      await inList(p, "activity-design");
      await p.locator('[data-testid="design-repo"][data-repo="app"]').waitFor({ state: "attached" });
      await drawerSettled(p);
      assert.equal(await p.getByTestId("design-ideas").count(), 0);
      assert.equal(await p.getByText("옛 아이디어").count(), 0);
      assert.equal(await p.getByTestId("activity-ideas").count(), 0);
      await shot(p, `idea-links-${width}-tree`);
      // 문서의 n-0003(글·옛 메모 링크)은 링크, 표에 없거나 열 수 없는 n-0004·n-0009는 글자다
      await p.evaluate((h) => { location.hash = h; }, `#design=${encodeURIComponent("app/docs/README.md")}`);
      const body = p.getByTestId("design-body");
      await body.getByText("캠프 게시판은 아이디어", { exact: false }).waitFor();
      const links = body.locator('[data-testid="app-id"][data-kind="idea"]');
      await domIs(p, "n-0003 링크", () => document.querySelectorAll('[data-testid="design-body"] [data-testid="app-id"][data-kind="idea"]').length === 1);
      assert.equal(await links.first().getAttribute("data-target"), "n-0003");
      for (const plain of ["n-0004", "n-0009"]) assert.equal(await body.locator(`[data-target="${plain}"]`).count(), 0, plain);
      assert.equal(await body.getByTestId("idea-link").count(), 1);
      await links.first().click();
      await hashIs(p, `#design=${encodeURIComponent("app/docs/camp.md")}`);
      await p.getByTestId("design-body").getByText("의뢰는 셋 중에서 고른다.").waitFor();
    } finally {
      await p.context().close();
    }
  });

  for (const name of ["chromium", "webkit"]) await step(`옛 주소 #idea=는 옮긴 문서로, 표에 없으면 처음 화면으로 바뀐다: ${name}`, async () => {
    const wk = name === "webkit" ? await webkit.launch({ headless: true }) : null;
    const { page: p } = await pairPage(wk ?? browser, aaa("web", "pair").code, { width: 1280, height: 844 }, { hash: "#idea=n-0003" });
    try {
      // 넘기기는 링크 표가 온 뒤 replaceState로 주소를 바꾸고(hashchange가 없다) 그다음 문서를 그린다. 그래서 hashIs가 아니라
      // 문서가 그려지기를 기다리고 그때 주소를 본다. hashIs는 표가 늦게 오는 부하에서 바뀐 주소를 놓쳤다(t-1959 착지 gate)
      await p.getByTestId("design-body").getByText("의뢰는 셋 중에서 고른다.").waitFor();
      assert.equal(await p.evaluate(() => location.hash), `#design=${encodeURIComponent("app/docs/camp.md")}`);
      // 앱 안 주소 바꿈(알림·링크)과 같은 길이다
      await p.evaluate(() => { location.hash = "#idea=n-0009"; });
      await domIs(p, "옛 주소를 지움", () => !location.hash.startsWith("#idea="));
      assert.equal(await p.getByTestId("idea-view").count(), 0);
    } finally {
      await p.context().close();
      await wk?.close();
    }
  });

  // 옛 137(옮기기)이 보던 t-1950 계약을 옮겼다: 대화의 commit이 모두 main에 들어가면 「main에 반영」 대신 「main에 들어감」
  await step("기획 대화 branch가 main에 병합되면 「main에 들어감」이고 반영 단추가 없다", async () => {
    const seed = `${root}/seed/app`, clone = `${root}/app`;
    aaa("plan", "new", "app", "camp", "--title", "캠프 게시판");
    const blob = aaa("plan", "show", "app", "docs/camp.md", "--branch", "camp").doc.blob;
    fs.writeFileSync(`${root}/camp.md`, "# 캠프 게시판\n\n의뢰는 셋 중에서 고른다. 게시판은 문 옆에 선다.\n");
    aaa("plan", "save", "app", "docs/camp.md", "--branch", "camp", "--base", blob, "--file", `${root}/camp.md`);
    // 착지(반영 thread와 같은 결과): 그 branch를 origin main에 병합해 올린다. agent의 1분 받기 대신 clone이 origin을 바로 받는다
    git(seed, "fetch", "-q", clone, "design/camp");
    git(seed, "merge", "--no-ff", "-q", "-m", "병합: 캠프 게시판", "FETCH_HEAD");
    git(seed, "push", "-q", "origin", "HEAD:main");
    git(clone, "fetch", "-q", "origin");
    const { page: p } = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 844 }, { hash: `#design=${encodeURIComponent("app/docs/camp.md")}` });
    try {
      await p.getByTestId("design-body").getByText("게시판은 문 옆에 선다.").waitFor();
      await p.getByTestId("design-chat-branch").filter({ hasText: "camp" }).waitFor();
      await p.getByTestId("design-merged").waitFor();
      assert.equal(await p.getByTestId("design-apply").count(), 0);
    } finally {
      await p.context().close();
    }
  });
}

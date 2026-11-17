// 기획서 ID(t-1957): 기획 문서마다 `k-NNNN`. 표지(`<!-- aaa-id: k-NNNN -->`)가 있는 문서는 그 번호, 없는 문서는 host 원장 번호다.
// 문서 머리의 ID·숨은 표지 줄·글 속 `k-NNNN` 링크(앱 안에서 그 문서를 연다)·`#design=k-NNNN` 주소·`aaa plan show k-NNNN`·
// `aaa plan ids`(한 번에 달기: 기획 branch `design/aaa-ids` 한 commit)를 실제 git origin·agent로 본다. 고정 sleep 없이 DOM·응답을 기다린다.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import { aaa, root, step, browser, pairPage, inList, shot } from "../lib/harness.mjs";

const git = (cwd, ...args) =>
  execFileSync("git", ["-c", "user.name=demo", "-c", "user.email=demo@example.com", "-c", "init.defaultBranch=main", ...args], { cwd, stdio: ["ignore", "pipe", "pipe"] }).toString().trim();

function write(dir, files) {
  for (const [rel, body] of Object.entries(files)) {
    fs.mkdirSync(`${dir}/${rel}`.replace(/\/[^/]+$/, ""), { recursive: true });
    fs.writeFileSync(`${dir}/${rel}`, body);
  }
}

// 개요는 다른 host가 단 표지(k-0007)를 가졌다. 게임 루프는 표지가 없고 제목 바로 아래 빈 줄 없이 계획 블록이 붙어 있다
const DOCS = {
  "README.md": "# app\n",
  "docs/README.md": "# 개요\n<!-- aaa-id: k-0007 -->\n\n작은 견본 게임. 흐름은 k-0008, 없는 문서 k-0999.\n",
  "docs/loop.md": "# 게임 루프\n> [!계획]\n> 성 안 보스를 더한다.\n\n마을에서 출발한다. 개요는 k-7.\n",
  "docs/skills.md": "# 기술 목록\n\n| 이름 | 효과 |\n| --- | --- |\n| 돌진 | 앞으로 세 칸 |\n",
  // vendor 폴더는 번호만 받고 표지는 달지 않는다(남의 원문, q-4090 1A)
  "docs/vendor/LICENSE.md": "# License\n\nMIT\n",
};

export default async function () {
  let clone;
  let origin;

  await step("기획서 ID: 표지는 그 번호, 없는 문서는 원장 번호", async () => {
    origin = `${root}/origins/app.git`;
    const seed = `${root}/seed/app`;
    clone = `${root}/app`;
    fs.mkdirSync(seed, { recursive: true });
    git(root, "init", "--bare", "-q", origin);
    git(seed, "init", "-q");
    write(seed, DOCS);
    git(seed, "add", ".");
    git(seed, "commit", "-q", "-m", "처음");
    git(seed, "remote", "add", "origin", origin);
    git(seed, "push", "-q", "origin", "HEAD:main");
    git(root, "clone", "-q", origin, clone);
    aaa("project", "create", "demo");
    fs.writeFileSync(`${root}/projects/demo/aaa.toml`,
      `schema = 1\n\n[[workspaces]]\npath = "${clone}"\nmachine = "labA"\ndesign = "docs"\n`);
    const t = aaa("plan", "list").repos[0].tree;
    assert.equal(t.ids, true);
    assert.deepEqual(t.docs.map((d) => [d.path, d.id, d.id_marked]), [
      ["docs/README.md", "k-0007", true],
      ["docs/loop.md", "k-0008", false],
      ["docs/skills.md", "k-0009", false],
      ["docs/vendor/LICENSE.md", "k-0010", false],
    ]);
    // 다시 읽어도 같은 번호
    assert.deepEqual(aaa("plan", "list").repos[0].tree.docs.map((d) => d.id), ["k-0007", "k-0008", "k-0009", "k-0010"]);
    const shown = aaa("plan", "show", "k-8");
    assert.equal(shown.doc.path, "docs/loop.md");
    assert.equal(shown.doc.id, "k-0008", "표지 없는 문서는 원장 번호");
  }, { always: true });

  await step("웹: 문서 머리 ID·숨은 표지·글 속 k-NNNN 링크·ID 주소", async () => {
    const { page: p } = await pairPage(browser, aaa("web", "pair").code, { width: 1280, height: 800 });
    try {
      await inList(p, "activity-design");
      await inList(p, p.locator('[data-testid="design-doc"][data-path="docs/README.md"]'));
      const body = p.getByTestId("design-body");
      await body.getByRole("heading", { name: "개요" }).waitFor();
      assert.equal(await p.getByTestId("design-id").textContent(), "k-0007");
      assert.doesNotMatch(await body.textContent(), /aaa-id/, "표지 줄은 그리지 않는다");
      // 지금 트리의 ID만 링크다(k-0999는 글 그대로)
      const link = body.locator('[data-testid="app-id"][data-kind="design"]');
      await link.first().waitFor();
      assert.deepEqual(await link.allTextContents(), ["k-0008"]);
      await shot(p, "design-id-readme");
      await link.first().click();
      await body.getByRole("heading", { name: "게임 루프" }).waitFor();
      assert.equal(await p.evaluate(() => location.hash), `#design=${encodeURIComponent("app/docs/loop.md")}`);
      assert.equal(await p.getByTestId("design-id").textContent(), "k-0008");
      await shot(p, "design-id-loop");
      // ID 주소(`#design=k-0009`)는 그 문서를 연다. 주소는 ID 그대로 두고(옮겨도 이어지는 링크), 새로 고쳐도 같은 문서다
      await p.evaluate(() => { location.hash = "#design=k-9"; });
      await body.getByRole("heading", { name: "기술 목록" }).waitFor();
      await p.reload();
      await p.getByTestId("design-body").getByRole("heading", { name: "기술 목록" }).waitFor();
      assert.equal(await p.getByTestId("design-id").textContent(), "k-0009");
    } finally { await p.context().close(); }
  });

  await step("한 번에 달기: 표지 없는 문서만 design/aaa-ids 한 commit, 다시 해도 그대로", async () => {
    const r = aaa("plan", "ids", "app");
    assert.equal(r.branch, "aaa-ids");
    assert.equal(r.marked, 2, JSON.stringify(r));
    const show = (path) => git(clone, "show", `design/aaa-ids:${path}`);
    assert.equal(show("docs/loop.md"), "# 게임 루프\n<!-- aaa-id: k-0008 -->\n\n> [!계획]\n> 성 안 보스를 더한다.\n\n마을에서 출발한다. 개요는 k-7.");
    assert.equal(show("docs/README.md"), DOCS["docs/README.md"].trimEnd(), "있던 표지는 그대로");
    assert.equal(git(clone, "rev-list", "--count", "main..design/aaa-ids"), "1", "한 commit");
    assert.deepEqual(git(clone, "diff", "--name-only", "main", "design/aaa-ids").split("\n"), ["docs/loop.md", "docs/skills.md"], "vendor는 그대로");
    // 표지 블록 수는 그대로(표지 줄이 계획 블록을 삼키지 않는다)
    const doc = aaa("plan", "show", "app", "docs/loop.md", "--branch", "aaa-ids");
    assert.equal(doc.doc.id, "k-0008");
    const again = aaa("plan", "ids", "app");
    assert.equal(again.commit, null, "main에 아직 없으니 고를 문서는 있지만 branch에는 이미 있다");
    assert.equal(git(clone, "rev-list", "--count", "main..design/aaa-ids"), "1");
  });
}

// 기획 트리(t-1894): 문서 목록을 폴더 트리로 묶는 차례와 문서 안 상대 경로 풀기. 트리 차례는 사이드바 줄 차례이고,
// 경로 풀기는 그림 주소·앱 안 문서 열기가 저장소 밖을 가리키지 않게 하는 계약이다.
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { addedDocs, ancestors, branchesByPath, changedBlocks, designFileUrl, designMarks, designTree, docLabel, mdBlocks, resolveDocPath, slugSuggest, SLUG, splitTarget } from "../src/designModel.ts";

const doc = (path, title = "") => ({ path, title, bytes: 1 });
const names = (nodes) => nodes.map((n) => (n.kind === "dir" ? `${n.name}/` : n.name));

describe("기획 트리", () => {
  it("폴더 먼저, README 먼저, 폴더 제목은 그 README", () => {
    const nodes = designTree("docs", [
      doc("docs/zeta.md"),
      doc("docs/README.md", "개요"),
      doc("docs/alpha.md", "알파"),
      doc("docs/loop/skill-tree.md", "스킬 트리"),
      doc("docs/loop/README.md", "게임 루프"),
      doc("docs/art/a/deep.md"),
      doc("other/out.md"),
    ]);
    assert.deepEqual(names(nodes), ["art/", "loop/", "README.md", "alpha.md", "zeta.md"]);
    const loop = nodes[1];
    assert.equal(loop.title, "게임 루프");
    assert.deepEqual(names(loop.children), ["README.md", "skill-tree.md"]);
    assert.deepEqual(names(nodes[0].children), ["a/"], "중간 폴더도 만든다");
    assert.equal(nodes[0].title, "");
    assert.equal(docLabel(doc("docs/loop/plan-v2.md")), "plan-v2", "제목 없는 문서는 파일 이름");
    assert.deepEqual(ancestors("docs/loop/skill-tree.md"), ["docs", "docs/loop"]);
  });

  it("문서 안 상대 경로", () => {
    const at = "docs/loop/README.md";
    assert.equal(resolveDocPath(at, "media/a.png"), "docs/loop/media/a.png");
    assert.equal(resolveDocPath(at, "./skill-tree.md#보상"), "docs/loop/skill-tree.md");
    assert.equal(resolveDocPath(at, "../art/%EA%B7%B8%EB%A6%BC.png?x=1"), "docs/art/그림.png");
    assert.equal(resolveDocPath(at, "../../README.md"), "README.md");
    for (const bad of ["../../../x.md", "a%2F..%2F..%2F..%2F..%2Fx", "/etc/passwd", "https://example.com/a.png", "data:image/png;base64,AA", "#절", " "]) {
      assert.equal(resolveDocPath(at, bad), null, bad);
    }
  });

  it("그림 주소와 앱 주소", () => {
    assert.equal(
      designFileUrl("de mo", "app", "docs/loop/그림 1.png", "0123456789abcdef0123456789abcdef01234567"),
      "/api/v1/projects/de%20mo/design/app/docs/loop/%EA%B7%B8%EB%A6%BC%201.png?c=0123456789abcdef0123456789abcdef01234567",
    );
    assert.deepEqual(splitTarget("app/docs/a.md"), { repo: "app", path: "docs/a.md" });
    assert.equal(splitTarget("app/"), null);
    assert.equal(splitTarget("/docs/a.md"), null);
  });
});

// 2-3 기획 화면(t-1894): 트리의 branch 표시, branch 판의 main과 다른 덩어리, branch 이름 짓기, 대화 사이 사건 줄
describe("기획 branch 화면", () => {
  const branch = (slug, changed, extra = {}) => ({ slug, changed, events: [], state: "open", ...extra });
  const repo = {
    tree: { docs: [doc("docs/a.md"), doc("docs/b.md")] },
    branches: [
      branch("one", [{ path: "docs/a.md", status: "M" }, { path: "docs/new.md", status: "A" }, { path: "docs/pic.png", status: "A" }]),
      branch("two", [{ path: "docs/a.md", status: "M" }, { path: "docs/new.md", status: "A" }]),
    ],
  };
  it("문서마다 고친 branch(만든 차례), main에 없는 새 문서는 먼저 만든 branch 하나로(그림은 문서가 아니다)", () => {
    assert.deepEqual(branchesByPath(repo).get("docs/a.md").map((b) => b.slug), ["one", "two"]);
    assert.equal(branchesByPath(repo).get("docs/b.md"), undefined);
    assert.deepEqual(addedDocs(repo).map((a) => [a.path, a.branch.slug]), [["docs/new.md", "one"]]);
  });
  it("branch 이름: 영문·숫자만 남기고, 꼴은 소문자·숫자·-(끝은 -가 아님)", () => {
    assert.equal(slugSuggest("Boss Dex 2!"), "boss-dex-2");
    assert.equal(slugSuggest("보스 도감"), "");
    for (const ok of ["a", "boss-dex", "floors-100"]) assert.ok(SLUG.test(ok), ok);
    for (const bad of ["", "-a", "a-", "A", "a_b", "a--b", "main", "x".repeat(49)]) assert.ok(!SLUG.test(bad), bad);
    assert.equal(slugSuggest("Main"), "main-1", "판 고르기의 main과 겹치지 않게");
  });
  it("덩어리는 빈 줄로 나누고 제목 줄은 홀로, 코드 울타리 안 빈 줄은 나누지 않는다. main에 없는 덩어리만 표시", () => {
    const text = "# 제목\n본문 하나\n\n```\n코드\n\n끝\n```\n\n## 절\n새 글\n";
    const blocks = mdBlocks(text);
    assert.deepEqual(blocks, ["# 제목", "본문 하나", "```\n코드\n\n끝\n```", "## 절", "새 글"]);
    assert.deepEqual(changedBlocks("# 제목\n본문 하나\n\n## 절\n옛 글\n", blocks), [false, false, true, false, true]);
    assert.deepEqual(changedBlocks(null, blocks), blocks.map(() => true), "main에 없는 새 문서는 모두");
    assert.deepEqual(mdBlocks("````\n```\n안\n```\n````\n\n끝\n"), ["````\n```\n안\n```\n````", "끝"], "긴 울타리 안의 짧은 울타리는 글이다");
  });
  it("대화 사건: 같은 말 뒤 thread 저장은 한 줄로 묶고, 새 대화·올림·사람 고침은 따로", () => {
    const ev = (kind, text, after, commit = "abcdef1234") => ({ kind, text, after, commit, at_ms: 0 });
    const marks = designMarks([
      ev("open", "", null, "8ebc07ffff"),
      ev("save", "docs/a.md", "보스 도감에"),
      ev("create", "docs/new.md", "보스 도감에", "4e1a2bffff"),
      ev("push", "", "보스 도감에"),
      ev("edit", "docs/a.md", "보스 도감에", "7c9d01ffff"),
      ev("save", "docs/b.md", "다음 말"),
    ], "boss-dex");
    assert.deepEqual(marks.map((m) => [m.kind, m.after]), [["open", null], ["save", "보스 도감에"], ["push", "보스 도감에"], ["edit", "보스 도감에"], ["save", "다음 말"]]);
    assert.equal(marks[0].text, "새 대화 · design/boss-dex · main 8ebc07f 에서");
    assert.equal(marks[1].text, "문서 1 고침 · 1 새로 · 4e1a2bf");
    assert.equal(marks[3].text, "사람이 a.md 고침 · 7c9d01f");
  });
  it("그림 주소: branch 판이면 slug를 싣는다", () => {
    assert.equal(designFileUrl("p", "r", "docs/x y.png", "abc", "boss-dex"), "/api/v1/projects/p/design/r/docs/x%20y.png?c=abc&b=boss-dex");
    assert.equal(designFileUrl("p", "r", "docs/x.png", "abc"), "/api/v1/projects/p/design/r/docs/x.png?c=abc");
  });
});

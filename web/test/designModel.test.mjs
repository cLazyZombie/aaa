// 기획 트리(t-1894): 문서 목록을 폴더 트리로 묶는 차례와 문서 안 상대 경로 풀기. 트리 차례는 사이드바 줄 차례이고,
// 경로 풀기는 그림 주소·앱 안 문서 열기가 저장소 밖을 가리키지 않게 하는 계약이다.
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { ancestors, designFileUrl, designTree, docLabel, resolveDocPath, splitTarget } from "../src/designModel.ts";

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

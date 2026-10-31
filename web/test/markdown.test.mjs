import assert from "node:assert/strict";
import { test } from "node:test";
import { parseInline } from "../src/herdr/markdown.ts";

// 빈 alt도 미디어로 전달해야 카드·보고의 기존 resolver가 같은 주소를 읽는다.
test("빈 alt 그림·영상과 기존 alt는 같은 미디어 경로를 보존한다", () => {
  for (const src of ["library/shot.png", "./library/preview/a.mp4#t=1", "library/a.webm", "library/%EC%8B%9C%EC%95%88.svg"]) {
    for (const alt of ["", "시안 영상"]) {
      const nodes = parseInline(`앞 ![${alt}](${src}) 뒤`);
      assert.deepEqual(nodes.map(n => n.type), ["text", "image", "text"]);
      assert.equal(nodes[0].value, "앞 ");
      assert.equal(nodes[1].src, src);
      assert.equal(nodes[1].alt, alt);
      assert.equal(nodes[2].value, " 뒤");
    }
  }
  assert.deepEqual(parseInline("![](library/a.png)![](library/b.mp4)").map(n => n.type), ["image", "image"]);
});

test("빈 일반 링크는 사라지거나 재생기로 바뀌지 않고 원문을 남긴다", () => {
  for (const src of ["library/a.png", "library/a.mp4", "https://example.com/a", "javascript:alert", "#here"]) {
    const text = `[](${src})`;
    assert.deepEqual(parseInline(text), [{ type: "text", value: text }]);
  }
  const image = parseInline("![](library/a.png)")[0];
  assert.equal(image.fallback.map(n => n.value).join(""), "![](library/a.png)");
});

test("code·깨진 미디어 문법은 글로, 기존 링크·강조는 그대로 읽는다", () => {
  for (const text of ["![]()", "![](library/a.png", "![](library/a b.png)", "![\n](library/a.png)"]) {
    assert.deepEqual(parseInline(text), [{ type: "text", value: text }]);
  }
  assert.deepEqual(parseInline("`![](library/a.png)`"), [{ type: "code", value: "![](library/a.png)" }]);
  assert.deepEqual(parseInline("[시안](library/a.png)"), [{ type: "file", path: "library/a.png", children: [{ type: "text", value: "시안" }] }]);
  const strong = parseInline("**![](library/a.png)**")[0];
  assert.equal(strong.type, "strong");
  assert.equal(strong.children[0].type, "image");
});

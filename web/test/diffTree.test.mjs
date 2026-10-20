// 변경 파일 카드의 트리·숫자(src/diffTree.ts, plan §8.305). t3code 4b1024da `turnDiffTree.ts`·`DiffStatLabel.tsx`의 규칙.
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { buildDiffTree, formatCompactDiffCount, summarizeDiffStats, visibleRows } from "../src/diffTree.ts";

const f = (path, additions, deletions) => ({ path, status: "modified", additions, deletions });

describe("변경 파일 트리", () => {
  it("폴더 먼저·자연수 차례·한 자식 폴더 합침·폴더 합계, 바이너리는 숫자 없이 합계에서 빠진다", () => {
    const files = [f("b.md", 1, 0), f("src/a10.rs", 2, 1), f("src/a2.rs", 1, 1), f("assets/img/hero/a.png", null, null), f("assets/img/hero/b.txt", 3, 0), f("A.md", 0, 2)];
    const tree = buildDiffTree(files);
    assert.deepEqual(tree.map((n) => [n.kind, n.name]), [["directory", "assets/img/hero"], ["directory", "src"], ["file", "A.md"], ["file", "b.md"]]);
    assert.deepEqual(tree[0].stat, { additions: 3, deletions: 0 });
    assert.deepEqual(tree[1].children.map((n) => n.name), ["a2.rs", "a10.rs"]);
    assert.equal(tree[0].children.find((n) => n.name === "a.png").stat, null);
    assert.deepEqual(summarizeDiffStats(files), { additions: 7, deletions: 4 });
  });
  it("보이는 줄: 기본 접힘, 폴더마다 바꾼 것, 모두 펼침", () => {
    const tree = buildDiffTree([f("src/x/a.rs", 1, 0), f("src/b.rs", 1, 0), f("c.rs", 1, 0)]);
    const names = (rows) => rows.map((r) => `${r.depth}:${r.node.name}`);
    assert.deepEqual(names(visibleRows(tree, {}, false)), ["0:src", "0:c.rs"]);
    assert.deepEqual(names(visibleRows(tree, { src: true }, false)), ["0:src", "1:x", "1:b.rs", "0:c.rs"]);
    assert.deepEqual(names(visibleRows(tree, { "src/x": false }, true)), ["0:src", "1:x", "1:b.rs", "0:c.rs"]);
  });
  it("숫자 줄임: 1000 밑은 그대로, k·m·b, 반올림이 1000이면 다음 단위", () => {
    assert.deepEqual([0, 999, 1000, 1250, 9950, 12_345, 999_499, 999_950, 1_500_000, 2e9].map(formatCompactDiffCount), ["0", "999", "1k", "1.3k", "9.9k", "12k", "999k", "1m", "1.5m", "2b"]);
  });
});

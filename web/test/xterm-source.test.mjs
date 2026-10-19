import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { applyPatch } from "../xterm-source.mjs";

// checkout의 패치(CRLF)와 npm 소스(LF)가 달라도 같은 결과여야 한다.
// 실제 회귀 경계인 파일 적용기를 실행하고, 문맥이 틀린 패치는 계속 거부한다.
test("xterm 패치는 checkout과 npm 소스의 줄 끝에 관계없이 엄격히 적용한다", (t) => {
  const root = mkdtempSync(join(tmpdir(), "aaa-xterm-patch-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const patch = "diff --git a/sample.ts b/sample.ts\n--- a/sample.ts\n+++ b/sample.ts\n@@ -1,3 +1,3 @@\n before\n-old\n+new\n after\n";
  for (const sourceEol of ["\n", "\r\n"]) {
    for (const patchEol of ["\n", "\r\n"]) {
      const file = join(root, "sample.ts");
      writeFileSync(file, ["before", "old", "after", ""].join(sourceEol));
      applyPatch(root, patch.replace(/\n/g, patchEol));
      assert.equal(readFileSync(file, "utf8"), "before\nnew\nafter\n");
      assert.throws(() => applyPatch(root, patch.replace(/\n/g, patchEol)), /문맥이 없다/);
    }
  }
});

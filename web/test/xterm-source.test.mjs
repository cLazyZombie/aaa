import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { applyPatch } from "../xterm-source.mjs";
import ts from "typescript-api";

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

// 빌드에 쓰는 실제 upstream 메서드에 같은 patch를 붙여 실행한다. DOM scroll 사건이 합쳐진 상태를 결정적으로 만든다.
test("xterm 자체 동기화 뒤 합쳐진 사용자 scroll은 다른 줄이면 버리지 않는다", (t) => {
  const root = mkdtempSync(join(tmpdir(), "aaa-xterm-scroll-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  mkdirSync(join(root, "src/browser"), { recursive: true });
  const file = join(root, "src/browser/Viewport.ts");
  const original = readFileSync(new URL("../node_modules/@xterm/xterm/src/browser/Viewport.ts", import.meta.url), "utf8");
  writeFileSync(file, original);
  applyPatch(root, readFileSync(new URL("../patches/xterm-5.5.0-viewport.patch", import.meta.url), "utf8"));
  const compile = (text) => {
    const source = ts.createSourceFile("Viewport.ts", text, ts.ScriptTarget.Latest, true);
    const viewport = source.statements.find(ts.isClassDeclaration);
    const method = viewport.members.find((m) => m.name?.getText(source) === "_handleScroll");
    const code = ts.transpileModule(`export class Probe { ${method.getText(source)} }`, { compilerOptions: { module: ts.ModuleKind.CommonJS } }).outputText;
    const module = {};
    new Function("exports", code)(module);
    return module.Probe;
  };
  const patched = compile(readFileSync(file, "utf8"));
  const run = (top, rowHeight = 18, implementation = patched) => {
    const requests = [];
    const state = {
      _viewportElement: { scrollTop: top, offsetParent: {} }, _currentRowHeight: rowHeight,
      _bufferService: { buffer: { ydisp: 43 } }, _ignoreNextScrollEvent: true,
      _onRequestScrollLines: { fire: (request) => requests.push(request) },
    };
    implementation.prototype._handleScroll.call(state, new Event("scroll"));
    assert.equal(state._ignoreNextScrollEvent, false);
    return requests;
  };
  assert.deepEqual(run(774), [{ amount: 0, suppressScrollEvent: true }], "자체 동기화 사건");
  assert.deepEqual(run(0, 18, compile(original)), [{ amount: 0, suppressScrollEvent: true }], "upstream 대조는 같은 이동을 버린다");
  assert.deepEqual(run(0), [{ amount: -43, suppressScrollEvent: true }], "같은 차례의 실제 위로 이동");
  assert.deepEqual(run(0, 0), [], "reset 중에는 줄 높이를 기다린다");
});

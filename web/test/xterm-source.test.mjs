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
function viewportProbes(t) {
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
    const methods = viewport.members.filter((m) => ["reset", "_handleScroll", "_innerRefresh", "_refresh", "syncScrollArea"].includes(m.name?.getText(source)));
    const code = ts.transpileModule(`export class Probe { ${methods.map((m) => m.getText(source)).join("\n")} }`, { compilerOptions: { module: ts.ModuleKind.CommonJS } }).outputText;
    const module = {};
    new Function("exports", code)(module);
    return module.Probe;
  };
  return { patched: compile(readFileSync(file, "utf8")), upstream: compile(original) };
}

test("xterm 자체 동기화 뒤 합쳐진 사용자 scroll은 다른 줄이면 버리지 않는다", (t) => {
  const { patched, upstream } = viewportProbes(t);
  const run = (top, rowHeight = 18, implementation = patched) => {
    const requests = [];
    const state = {
      _viewportElement: { scrollTop: top, offsetParent: {} }, _currentRowHeight: rowHeight,
      _bufferService: { buffer: { ydisp: 43 } }, _ignoreNextScrollEvent: true, _ignoredScrollTop: 774,
      _onRequestScrollLines: { fire: (request) => requests.push(request) },
      syncScrollArea: () => {},
    };
    implementation.prototype._handleScroll.call(state, new Event("scroll"));
    assert.equal(state._ignoreNextScrollEvent, false);
    return requests;
  };
  assert.deepEqual(run(774), [{ amount: 0, suppressScrollEvent: true }], "자체 동기화 사건");
  assert.deepEqual(run(0, 18, upstream), [{ amount: 0, suppressScrollEvent: true }], "upstream 대조는 같은 이동을 버린다");
  assert.deepEqual(run(0), [{ amount: -43, suppressScrollEvent: true }], "같은 차례의 실제 위로 이동");
  assert.deepEqual(run(0, 0), [], "reset 중에는 줄 높이를 기다린다");
});

test("xterm PageUp의 늦은 native scroll이 다음 PageDown을 되돌리지 않는다", (t) => {
  const { patched } = viewportProbes(t);
  for (const rowHeight of [19, 18.5]) {
    const requests = [];
    const frames = [];
    let nativeTop = 0;
    const state = {
      _viewportElement: {
        // 브라우저가 scrollTop을 반올림한 실제 값을 되읽어야 한다.
        get scrollTop() { return nativeTop; },
        set scrollTop(value) { nativeTop = Math.round(value); },
        offsetParent: {}, offsetHeight: 570,
      },
      _charSizeService: { height: 13 },
      _renderDimensions: { device: { cell: { height: rowHeight } }, css: { canvas: { height: 570 } } },
      _coreBrowserService: { dpr: 1, window: { requestAnimationFrame: (callback) => { frames.push(callback); return frames.length; } } },
      _bufferService: { buffer: { ydisp: 142, lines: { length: 201 } } },
      _lastRecordedBufferLength: 201, _lastScrollTop: 171 * rowHeight,
      _scrollArea: { style: {} }, _refreshAnimationFrame: null,
      _onRequestScrollLines: { fire: (request) => requests.push(request) },
    };
    Object.setPrototypeOf(state, patched.prototype);
    state._activeBuffer = state._bufferService.buffer;
    state._renderService = { dimensions: state._renderDimensions };
    // PageUp의 동기화 frame → PageDown의 논리 위치 변경 → 앞선 native scroll.
    patched.prototype._innerRefresh.call(state);
    assert.equal(nativeTop, Math.round(142 * rowHeight));
    state._bufferService.buffer.ydisp = 171;
    state.syncScrollArea();
    assert.equal(frames.length, 0, "다음 키 시점에는 이전 native 사건 전이라 _lastScrollTop이 옛 끝이다");
    patched.prototype._handleScroll.call(state, new Event("scroll"));
    assert.deepEqual(requests.splice(0), [{ amount: 0, suppressScrollEvent: true }], "앞선 자체 동기화는 새 목표를 덮지 않는다");
    assert.equal(frames.length, 1, "native 사건의 위치를 읽은 뒤 새 목표의 DOM 동기화를 건다");
    frames.shift()();
    assert.equal(nativeTop, Math.round(171 * rowHeight), "다음 frame에 DOM도 새 목표로 간다");
    // 다음 frame이 새 목표를 반영하고, 같은 차례에 사용자가 맨 위로 굴린 사건은 살린다.
    nativeTop = 0;
    patched.prototype._handleScroll.call(state, new Event("scroll"));
    assert.deepEqual(requests.splice(0), [{ amount: -171, suppressScrollEvent: true }], "동기화와 합쳐진 실제 사용자 이동");
  }
});

test("xterm reset frame·생성 task·refresh는 dispose 뒤 renderer를 읽지 않는다", (t) => {
  const { patched, upstream } = viewportProbes(t);
  for (const implementation of [upstream, patched]) {
    const frames = [];
    let reads = 0;
    const state = {
      _isDisposed: false,
      _coreBrowserService: { window: { requestAnimationFrame: (callback) => frames.push(callback) } },
      _bufferService: { buffer: { lines: { length: 0 } } },
      _renderService: { get dimensions() { reads++; throw new TypeError("renderer disposed"); } },
      _charSizeService: { get height() { reads++; throw new TypeError("disposed refresh"); } },
    };
    Object.setPrototypeOf(state, implementation.prototype);
    state.reset();
    state._isDisposed = true; // upstream Disposable.dispose가 설정하는 수명 표지
    for (const callback of [frames.shift(), () => state.syncScrollArea(), () => state._innerRefresh()]) {
      if (implementation === upstream) assert.throws(callback, TypeError, "구판은 renderer 제거 뒤에도 읽는다");
      else assert.doesNotThrow(callback);
    }
    assert.equal(reads, implementation === upstream ? 3 : 0);
  }
});

import assert from 'node:assert/strict';
import test from 'node:test';
import { cpSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import ts from 'typescript-api';
import { applyPatch } from '../xterm-source.mjs';

function helper(t, anchored) {
  const root = mkdtempSync(join(tmpdir(), 'aaa-preedit-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  cpSync(new URL('../node_modules/@xterm/xterm/src', import.meta.url), join(root, 'src'), { recursive: true });
  for (const name of ['xterm-5.5.0.patch', 'xterm-5.5.0-mobile-input.patch', ...(anchored ? ['xterm-5.5.0-composition-anchor.patch'] : [])]) {
    applyPatch(root, readFileSync(new URL(`../patches/${name}`, import.meta.url), 'utf8'));
  }
  const code = ts.transpileModule(readFileSync(join(root, 'src/browser/input/CompositionHelper.ts'), 'utf8'), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, experimentalDecorators: true },
  }).outputText;
  const exports = {};
  new Function('require', 'exports', code)(id => id === 'common/Platform' ? {} : new Proxy({}, { get: () => () => {} }), exports);
  const markers = new Set();
  const buffer = { x: 8, y: 5, ybase: 10, ydisp: 10, isCursorInViewport: true, addMarker(line) {
    const marker = { line, isDisposed: false, dispose() { this.isDisposed = true; markers.delete(this); } };
    markers.add(marker); return marker;
  } };
  const service = { buffer, cols: 80, rows: 24 };
  const input = { value: '', selectionEnd: 0, style: {} };
  const view = { style: {}, classList: { add() {}, remove() {} }, getBoundingClientRect: () => ({ width: 16, height: 20 }) };
  const sent = [];
  const h = new exports.CompositionHelper(input, view, service, { rawOptions: { fontSize: 13, fontFamily: 'monospace' } }, { triggerDataEvent: data => sent.push(data) }, { dimensions: { css: { cell: { width: 8, height: 20 } } } });
  const compose = text => { h.compositionstart(); input.value += text; input.selectionEnd = input.value.length; h.compositionupdate({ data: text }); };
  return { h, input, view, buffer, service, markers, sent, compose };
}

test('실제 xterm 조합은 분리된 상태 줄 출력 커서 대신 시작한 칸에 남고 확정은 한 번 보낸다', t => {
  t.mock.timers.enable({ apis: ['setTimeout'] });
  for (const anchored of [false, true]) {
    const { h, view, buffer, sent, compose, markers } = helper(t, anchored);
    compose('한'); t.mock.timers.runAll();
    assert.equal(view.style.top, '100px');
    buffer.x = 12; buffer.y = 2;
    h.updateCompositionElements(); t.mock.timers.runAll();
    assert.equal(view.style.top, anchored ? '100px' : '40px');
    assert.equal(view.style.left, anchored ? '64px' : '96px');
    assert.deepEqual(sent, []);
    h.compositionend(); t.mock.timers.runAll();
    assert.deepEqual(sent, ['한']);
    assert.equal(markers.size, 0);
    buffer.x = 3; buffer.y = 7;
    compose('글'); t.mock.timers.runAll();
    assert.equal(view.style.left, '24px'); assert.equal(view.style.top, '140px');
    h.reset(); t.mock.timers.runAll();
    assert.equal(markers.size, 0); assert.deepEqual(sent, ['한']);
  }
});

test('조합의 줄 표지는 스크롤·열 축소를 따르고 화면 밖과 다른 버퍼에는 남지 않는다', t => {
  t.mock.timers.enable({ apis: ['setTimeout'] });
  const { h, view, input, buffer, service, markers, compose } = helper(t, true);
  compose('한');
  buffer.ydisp = 12; service.cols = 4;
  h.updateCompositionElements();
  assert.equal(view.style.top, '60px'); assert.equal(input.style.left, '24px');
  buffer.ydisp = 16; h.updateCompositionElements();
  assert.equal(view.style.visibility, 'hidden');
  buffer.ydisp = 10; service.buffer = { ...buffer }; h.updateCompositionElements();
  assert.equal(view.style.visibility, 'hidden');
  service.buffer = buffer; h.updateCompositionElements();
  assert.equal(view.style.visibility, '');
  [...markers][0].dispose(); h.updateCompositionElements();
  assert.equal(view.style.visibility, 'hidden');
  h.reset(); t.mock.timers.runAll();
  assert.equal(markers.size, 0);
});

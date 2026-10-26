import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import ts from 'typescript-api';

function helper() {
  const code = ts.transpileModule(readFileSync(new URL('../xterm/CompositionHelper.ts', import.meta.url), 'utf8'), {
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
  const view = { style: {}, classList: { toggle() {} }, getBoundingClientRect: () => ({ width: 16, height: 20 }) };
  const sent = [];
  const h = new exports.CompositionHelper(input, view, service, { rawOptions: { fontSize: 13, fontFamily: 'monospace' } }, { triggerDataEvent: data => sent.push(data) }, { dimensions: { css: { cell: { width: 8, height: 20 } } } }, { getStringCellWidth: char => /[㄰-㆏가-힯]/.test(char) ? 2 : 1 });
  const compose = text => { h.compositionstart(); input.value += text; input.selectionEnd = input.value.length; h.compositionupdate({ data: text }); };
  return { h, input, view, buffer, service, markers, sent, compose };
}

test('실제 xterm 조합은 분리된 상태 줄 출력 커서 대신 시작한 칸에 남고 확정은 한 번 보낸다', t => {
  t.mock.timers.enable({ apis: ['setTimeout'] });
  {
    const { h, view, buffer, sent, compose, markers } = helper();
    compose('한'); t.mock.timers.runAll();
    assert.equal(view.style.top, '100px');
    buffer.x = 12; buffer.y = 2;
    h.updateCompositionElements(); t.mock.timers.runAll();
    assert.equal(view.style.top, '100px');
    assert.equal(view.style.left, '64px');
    assert.deepEqual(sent, []);
    h.compositionend(); t.mock.timers.runAll();
    assert.deepEqual(sent, ['한']);
    assert.equal(markers.size, 1, '단어 안 다음 음절은 같은 줄 표지를 쓴다');
    h.keydown({ keyCode: 39 });
    assert.equal(markers.size, 0, '방향키는 연속 입력을 끝낸다');
    buffer.x = 3; buffer.y = 7;
    compose('글'); t.mock.timers.runAll();
    assert.equal(view.style.left, '24px'); assert.equal(view.style.top, '140px');
    h.reset(); t.mock.timers.runAll();
    assert.equal(markers.size, 0); assert.deepEqual(sent, ['한']);
  }
});

test('조합의 줄 표지는 스크롤·열 축소를 따르고 화면 밖과 다른 버퍼에는 남지 않는다', t => {
  t.mock.timers.enable({ apis: ['setTimeout'] });
  const { h, view, input, buffer, service, markers, compose } = helper();
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

// 꼭 필요: 이전 확정의 에코가 오기 전 다음 음절을 시작하는 경계는 기존 상태 줄 시험에 없었다.
test('연속 음절·받침 넘김은 DOM 확정 범위로 진행하고 지연 에코·상태 줄은 위치를 바꾸지 않는다', t => {
  t.mock.timers.enable({ apis: ['setTimeout'] });
  const { h, input, buffer, view, sent, markers } = helper();
  const sending = [];
  h.observeState(() => { if (h.state.sending) sending.push(h.state.text); });
  input.value = 'x\u00a0';
  let prefix = '';
  for (const syllable of ['가', '나', '다', '라']) {
    h.compositionstart();
    // WebKit native는 앞의 NBSP도 같은 길이 SPACE로 정규화한다. 조합 범위는 그대로다.
    input.value = 'x ' + prefix + syllable; input.selectionEnd = input.value.length;
    h.compositionupdate({ data: syllable });
    assert.equal(view.style.left, `${64 + prefix.length * 16}px`, '에코 전에도 이전 음절 뒤다');
    buffer.x = 12; buffer.y = 2;
    h.updateCompositionElements();
    assert.equal(view.style.top, '100px', '분리된 상태 줄로 가지 않는다');
    assert.equal(view.style.left, `${64 + prefix.length * 16}px`);
    h.compositionend();
    prefix += syllable;
    // 다음 begin 전에 큐가 비워지든 뒤에 비워지든 같은 전이여야 한다.
    if (syllable === '나') t.mock.timers.runAll();
    assert.equal(markers.size, 1);
  }
  h.keydown({ keyCode: 32 }); t.mock.timers.runAll();
  assert.equal(sent.join(''), '가나다라');
  assert.equal(markers.size, 0);
  assert.ok(sending.length > 0, '실제 송신도 앱이 같은 상태에서 관측한다');

  h.reset(); sent.length = 0; buffer.x = 8; buffer.y = 5;
  h.compositionstart(); input.value = '각'; input.selectionEnd = 1;
  h.compositionupdate({ data: '각' });
  // 받침 ㄱ이 다음 음절로 넘어갈 때 end.data가 각이어도 DOM 확정분은 가다.
  input.value = '가'; h.compositionend(); h.compositionstart();
  input.value = '가가'; input.selectionEnd = 2; h.compositionupdate({ data: '가' });
  assert.equal(view.style.left, '80px');
  h.compositionend(); t.mock.timers.runAll();
  assert.equal(sent.join(''), '가가');
  h.reset();
  assert.equal(markers.size, 0);
  sent.length = 0;
  h.compositionstart(); input.value = '한'; input.selectionEnd = 1;
  h.compositionupdate({ data: '한' });
  assert.equal(h.keydown({ keyCode: 8, isComposing: true }), false);
  assert.deepEqual(sent, [], 'native Backspace는 확정하지 않는다');
  // OS paste/blur나 앱 paste가 조합 중 들어와도 같은 controller가 먼저 한 번만 확정한다.
  h.finishInput(); h.compositionend(); t.mock.timers.runAll();
  assert.deepEqual(sent, ['한']);
  assert.equal(markers.size, 0);
  h.reset(); sent.length = 0;
  h.compositionstart(); input.value = '한'; h.compositionupdate({ data: '한' });
  input.value = ''; h.compositionend(); t.mock.timers.runAll();
  assert.deepEqual(sent, [], '취소는 보낼 글이 없다');
  assert.equal(markers.size, 0, 'native 취소도 입력 연속성을 끝낸다');
});

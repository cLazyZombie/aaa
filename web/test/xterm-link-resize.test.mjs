import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, posix } from 'node:path';
import test from 'node:test';
import ts from 'typescript-api';
import { applyPatch } from '../xterm-source.mjs';

// 생성자의 실제 resize listener·render 해제·provider 조회까지 실행한다.
// mouseup 메서드만 추출하면 resize가 currentLink를 없애는 경계를 놓친다.
function linkifier(t) {
  const root = mkdtempSync(join(tmpdir(), 'aaa-link-resize-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  mkdirSync(join(root, 'src/browser'), { recursive: true });
  for (const file of ['Linkifier.ts', 'Types.d.ts'])
    writeFileSync(join(root, 'src/browser', file), readFileSync(new URL(`../node_modules/@xterm/xterm/src/browser/${file}`, import.meta.url)));
  applyPatch(root, readFileSync(new URL('../patches/xterm-5.5.0-link-press.patch', import.meta.url), 'utf8'));
  const modules = new Map();
  const load = id => {
    if (modules.has(id)) return modules.get(id);
    const source = id === 'browser/Linkifier'
      ? readFileSync(join(root, 'src/browser/Linkifier.ts'), 'utf8')
      : readFileSync(new URL(`../node_modules/@xterm/xterm/src/${id}.ts`, import.meta.url), 'utf8');
    const code = ts.transpileModule(source, { compilerOptions: {
      module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022,
      experimentalDecorators: true, useDefineForClassFields: false,
    } }).outputText;
    const exports = {}; modules.set(id, exports);
    new Function('require', 'exports', code)(name => load(name.startsWith('.') ? posix.join(posix.dirname(id), name) : name), exports);
    return exports;
  };
  return load('browser/Linkifier').Linkifier;
}

function signal() {
  const listeners = new Set();
  return {
    event: fn => { listeners.add(fn); return { dispose: () => listeners.delete(fn) }; },
    fire: value => { for (const fn of [...listeners]) fn(value); },
    get size() { return listeners.size; },
  };
}

test('resize가 누른 링크와 render 감시를 지워도 놓을 때 같은 현재 대상만 한 번 연다', { timeout: 5000 }, t => {
  const Linkifier = linkifier(t);
  const run = ({ change = () => {}, leave = false, deferred = false, outside = false } = {}) => {
    const resize = signal(), render = signal(), classes = new Set(['xterm']);
    const element = Object.assign(new EventTarget(), { classList: {
      add: value => classes.add(value), remove: value => classes.delete(value), contains: value => classes.has(value),
    } });
    let opened = 0, requests = 0, reply;
    let position = [8, 1];
    const state = { text: 'src/big.ts:2500', start: { x: 3, y: 1 }, end: { x: 17, y: 1 }, provider: 0 };
    const providers = [0, 1].map(index => ({ provideLinks: (line, callback) => {
      requests++;
      const links = index === state.provider ? [{
        text: state.text, range: { start: { ...state.start }, end: { ...state.end } }, activate: () => opened++,
      }] : undefined;
      if (deferred && index === 0 && requests > 2) reply = () => callback(links);
      else callback(links);
    } }));
    const instance = new Linkifier(element, { getCoords: () => position },
      { onRenderedViewportChange: render.event }, { onResize: resize.event, cols: 80, rows: 32, buffer: { ydisp: 0 } },
      { linkProviders: providers });
    element.dispatchEvent(new Event('mousemove'));
    assert.ok(classes.has('xterm-cursor-pointer'));
    element.dispatchEvent(new Event('mousedown'));
    assert.equal(render.size, 1);
    resize.fire({ cols: 80, rows: 33 });
    assert.equal(instance.currentLink, undefined);
    assert.equal(render.size, 0, 'resize는 재그리기로 링크를 복구할 listener도 해제한다');
    assert.equal(classes.has('xterm-cursor-pointer'), false);
    change(state);
    render.fire({ start: 0, end: 0 }); // 실제 PTY가 같은 행을 그려도 listener가 없다.
    if (leave) element.dispatchEvent(new Event('mouseleave'));
    if (outside) position = [30, 1];
    element.dispatchEvent(new Event('mouseup'));
    const once = opened;
    reply?.(); // 사용자 놓음이 끝난 뒤 도착한 응답으로 뒤늦게 열지 않는다.
    element.dispatchEvent(new Event('mouseup'));
    instance.dispose();
    assert.equal(resize.size, 0);
    assert.equal(render.size, 0);
    return { once, total: opened };
  };
  assert.deepEqual(run(), { once: 1, total: 1 });
  for (const change of [
    state => { state.text = 'src/else.ts:25'; },
    state => { state.provider = 1; },
    state => { state.provider = -1; },
    ...['start', 'end'].flatMap(edge => ['x', 'y'].map(axis => state => { state[edge][axis]++; })),
  ]) assert.deepEqual(run({ change }), { once: 0, total: 0 }, '대상·provider·전체 범위 변경은 거부');
  for (const options of [{ leave: true }, { outside: true }, { deferred: true }])
    assert.deepEqual(run(options), { once: 0, total: 0 }, '밖에서 놓음·늦은 응답·추가 놓음은 열지 않는다');
});

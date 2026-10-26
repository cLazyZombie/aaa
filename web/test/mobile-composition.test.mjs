import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import ts from "typescript-api";

// 모사 구현 대신 배포와 같은 upstream 소스·패치를 실행한다.
function helper(t, platform = { isSafari: true, isIphone: true }, screenReaderMode = false) {
  const source = readFileSync(new URL('../xterm/CompositionHelper.ts', import.meta.url), 'utf8');
  const code = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, experimentalDecorators: true } }).outputText;
  const exports = {};
  new Function("require", "exports", code)(id => {
    if (id === "common/Platform") return platform;
    if (id === "common/data/EscapeSequences") return { C0: { DEL: "\x7f" } };
    return new Proxy({}, { get: () => () => {} });
  }, exports);
  const box = { value: "", selectionEnd: 0 };
  const view = { textContent: "", classList: { add() {}, remove() {} } };
  const sent = [];
  const buffer = { x: 0, y: 0, ybase: 0, addMarker: () => ({ dispose() {} }) };
  const h = new exports.CompositionHelper(box, view, { buffer }, { rawOptions: { screenReaderMode } }, { triggerDataEvent: data => sent.push(data) }, {});
  h.updateCompositionElements = () => {}; // 배치만 제외, input/queue/watermark는 실제 코드
  const edit = (data, type = "insertCompositionText", value = data) => {
    const ev = { data, inputType: type, isComposing: type === "insertCompositionText" };
    h.beforeinput(ev);
    box.value = value; box.selectionEnd = value.length;
    h.input(ev);
  };
  return { h, box, sent, edit };
}

test("iPhone 영어 조합은 마지막 글자까지 바로 보내고 확정은 중복하지 않는다", t => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  {
    const { h, box, sent, edit } = helper(t);
    for (const ch of "status") {
      if (box.value) h.compositionend();
      h.compositionstart();
      h.compositionupdate({ data: ch });
      edit(ch, "insertCompositionText", box.value + ch);
      t.mock.timers.runAll();
    }
    assert.equal(sent.join(""), "status");
    h.compositionend(); t.mock.timers.runAll();
    assert.equal(sent.join(""), "status", "끝 사건이 이미 보낸 마지막 글자를 중복하지 않는다");
  }
});

test("영어 예측 단어 교체·지우기·확정 뒤 replacement는 보낸 꼬리만 고친다", t => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const { h, sent, edit } = helper(t);
  h.compositionstart();
  edit("teh"); edit("the");
  assert.equal(sent.join(""), "teh\x7f\x7fhe");
  h.compositionend(); t.mock.timers.runAll();
  edit("they", "insertReplacementText");
  edit(null, "deleteContentBackward", "the");
  assert.equal(sent.join(""), "teh\x7f\x7fhey\x7f");
  h.reset(); t.mock.timers.runAll();
  assert.equal(sent.join(""), "teh\x7f\x7fhey\x7f", "reset은 추가 입력이 없다");
});

test("한글은 확정까지 보류하고 영문→한글 전환은 보낸 영문만 되돌린다", t => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const { h, sent, edit } = helper(t);
  h.compositionstart(); edit("ㅎ"); edit("하"); edit("한");
  assert.equal(sent.join(""), "");
  h.compositionend(); t.mock.timers.runAll();
  assert.equal(sent.join(""), "한");
  h.reset(); sent.length = 0;
  h.compositionstart(); edit("h"); edit("ㅎ"); edit("한");
  assert.equal(sent.join(""), "h\x7f");
  h.compositionend(); t.mock.timers.runAll();
  assert.equal(sent.join(""), "h\x7f한");
});

test("데스크톱·화면 읽기 모드·한글 replacement는 기존 보류 경로를 쓴다", t => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  for (const [platform, reader] of [[{ isSafari: true, isMac: true }, false], [{ isSafari: true, isIphone: true }, true]]) {
    const { h, sent, edit } = helper(t, platform, reader);
    h.compositionstart(); edit("status");
    assert.equal(sent.join(""), "");
    h.compositionend(); t.mock.timers.runAll();
    assert.equal(sent.join(""), "status");
  }
  const { h, sent, edit } = helper(t);
  edit("ㅎ", "insertText"); edit("한", "insertReplacementText");
  assert.equal(sent.join(""), "");
  h.finishInput();
  assert.equal(sent.join(""), "한");
});

test("같은 task의 한글 확정→영문 시작도 PTY 순서를 지킨다", t => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const { h, box, sent, edit } = helper(t);
  h.compositionstart(); edit("한"); h.compositionend();
  h.compositionstart(); edit("s", "insertCompositionText", box.value + "s");
  assert.equal(sent.join(""), "한s");
  t.mock.timers.runAll();
  assert.equal(sent.join(""), "한s");
  h.reset(); sent.length = 0;
  h.compositionstart(); edit("한"); h.compositionend();
  // native WebKit은 조합 확정 직후, 같은 timer 차례 전에 일반 insertText를 낼 수 있다.
  h.beforeinput({ data: 'abc', inputType: 'insertText', isComposing: false });
  assert.equal(sent.join(''), '한', '새 DOM 삽입 전에 앞 확정을 먼저 보낸다');
  box.value += 'abc';
  t.mock.timers.runAll();
  assert.equal(sent.join(''), '한', '일반 삽입은 Terminal의 경계에서 한 번만 보낸다');
});

test("확정 뒤 교정의 keydown 229 fallback은 같은 교정을 다시 보내지 않는다", t => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const { h, sent, edit } = helper(t);
  h.compositionstart(); edit("teh"); h.compositionend(); t.mock.timers.runAll();
  h.keydown({ keyCode: 229 });
  edit("the", "insertReplacementText");
  t.mock.timers.runAll();
  assert.equal(sent.join(""), "teh\x7f\x7fhe");
});

test("DOM 삭제 뒤 늦은 Safari Backspace는 DEL을 중복하지 않는다", t => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const { h, sent, edit } = helper(t);
  h.compositionstart(); edit("status"); h.compositionend(); t.mock.timers.runAll();
  edit(null, "deleteContentBackward", "statu");
  let prevented = false;
  const accepted = h.keydown({ keyCode: 8, preventDefault: () => { prevented = true; } });
  assert.equal(accepted, false);
  assert.equal(prevented, true);
  t.mock.timers.runAll();
  assert.equal(sent.join(""), "status\x7f");
});

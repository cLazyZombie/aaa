import assert from "node:assert/strict";
import test from "node:test";
import { runInNewContext } from "node:vm";
import { pageScrollStart } from "../e2e/lib/page-scroll-ready.mjs";
import { selectInputAndWait } from "../e2e/lib/input-selection.mjs";
import { fittedTerminalGrid } from "../e2e/lib/terminal-grid-ready.mjs";

test("파일 링크 기준은 설정의 옛 fit·ACK·38행을 복귀한 35행 칸으로 기억하지 않는다", () => {
  // 원본 CI 순서: 설정 112×38 → 복귀 112×35 → 높이 19px 증가 112×36.
  // ResizeObserver가 아직 pending을 달기 전에는 옛 same·제목·DOM 행 수가 모두 일치한다.
  const data = { fit: 'same' };
  const rows = { childElementCount: 38, firstElementChild: { getBoundingClientRect: () => ({ height: 19 }) } };
  const title = { textContent: 'labA · 112×38' };
  const xterm = {};
  const term = {
    dataset: data, hasAttribute: () => Boolean(data.fitPending),
    querySelector: (s) => ({ '.xterm-rows': rows, '.xterm': xterm })[s],
    closest: () => ({ querySelector: () => title }),
  };
  const context = { document: { querySelector: () => term }, getComputedStyle: (el) => el === term ? { height: '679px' } : { paddingTop: '0px', paddingBottom: '0px' } };
  const ready = () => runInNewContext(`(${fittedTerminalGrid})()`, context);
  assert.equal(Number(/×(\d+)/.exec(title.textContent)[1]), rows.childElementCount, '옛 조건은 통과했다');
  assert.equal(ready(), false);
  title.textContent = 'labA · 112×35';
  assert.equal(ready(), false, 'ACK만 새 크기로 온 때도 거절한다');
  rows.childElementCount = 35;
  assert.equal(ready().rows, 35);
  data.fitPending = '1';
  assert.equal(ready(), false);
});

test("키 스크롤 기준은 resize의 옛 바닥 render를 현재 버퍼로 기억하지 않는다", () => {
  // 원본 CI: 본문은 s-172·30행, 파싱은 171, 옛 render 표지는 169.
  const data = { readonly: "false", fit: "same", viewTop: "169", viewBase: "169", parsedBase: "171" };
  const rows = { childElementCount: 30, firstElementChild: { textContent: "s-172" }, textContent: "s-172 s-200" };
  const vp = { scrollTop: 3249, scrollHeight: 3819, clientHeight: 570 };
  const screen = { getBoundingClientRect: () => ({ height: 570 }) };
  const el = { dataset: data, querySelector: selector => ({ ".xterm-rows": rows, ".xterm-viewport": vp, ".xterm-screen": screen })[selector] };
  const document = { querySelector: selector => selector === '[data-testid="terminal"]' ? el : { hasAttribute: () => false } };
  const ready = () => runInNewContext(`(${pageScrollStart})()`, { document });
  assert.equal(data.viewTop, data.viewBase);
  assert.equal(vp.scrollTop, vp.scrollHeight - vp.clientHeight, "옛 준비 조건은 이 순간을 통과시켰다");
  assert.equal(ready(), false);
  data.viewTop = data.viewBase = "171"; // 뒤에 도착한 실제 render 사건
  const before = ready();
  assert.equal(before.base, 171);
  assert.equal(before.top, 171);
  assert.equal(before.first, 172);
  data.fit = "sent";
  assert.equal(ready(), false, "크기 요청만 보낸 때도 기준을 잡지 않는다");
  data.fit = "same";
  vp.scrollTop = 3211;
  assert.equal(ready(), false, "본문보다 늦은 native viewport도 기다린다");
  vp.scrollTop = 3249;
  assert.equal(ready().nativeTop, 3249);
});

test("입력 선택 helper는 다음 전환 전에 비동기 native select까지 완료한다", async () => {
  const el = new EventTarget();
  Object.assign(el, { selectionStart: 12, selectionEnd: 12, selectionDirection: "forward", focus() { this.focused = true; } });
  let release, sets = 0, observed = 0, done = false;
  el.addEventListener("select", () => observed++);
  el.setSelectionRange = (start, end, direction) => {
    sets++;
    Object.assign(el, { selectionStart: start, selectionEnd: end, selectionDirection: direction });
    release = () => el.dispatchEvent(new Event("select"));
  };
  const selected = selectInputAndWait(el, [3, 8, "backward"]).then(() => { done = true; });
  await Promise.resolve();
  assert.equal(el.focused, true);
  assert.equal(sets, 1);
  assert.equal(done, false, "setSelectionRange 반환은 사건 완료가 아니다");
  release();
  await selected;
  assert.equal(observed, 1);
  assert.equal(done, true);
  // WebKit은 같은 범위·방향에 사건을 다시 만들지 않는다.
  await selectInputAndWait(el, [3, 8, "backward"]);
  assert.equal(sets, 1);
  assert.equal(observed, 1);
});

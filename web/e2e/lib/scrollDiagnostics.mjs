// CI 실패에서 키·native scroll·앱의 위치 쓰기를 구별한다. 관측만 하며 값·사건 순서는 바꾸지 않는다.
export async function watchScroll(page, selector) {
  await page.locator(selector).evaluate((el) => {
    const events = [];
    const read = () => {
      const term = document.querySelector('[data-testid="terminal"]');
      const rows = document.querySelector('.xterm-rows');
      return { top: el.scrollTop, height: el.scrollHeight, client: el.clientHeight,
        note: el.querySelector('.chat-note')?.getBoundingClientRect().height,
        first: el.querySelector('.chat-row')?.getBoundingClientRect().top,
        term: term && { ...term.dataset }, rows: rows?.childElementCount,
        firstLine: rows?.firstElementChild?.textContent };
    };
    const record = (event, extra = {}) => {
      events.push({ at: performance.now(), event, ...extra, ...read() });
      if (events.length > 192) events.shift();
    };
    window.takeScrollStates = () => [...events];
    const native = Object.getOwnPropertyDescriptor(Element.prototype, 'scrollTop');
    Object.defineProperty(el, 'scrollTop', { configurable: true, get() { return native.get.call(this); }, set(value) {
      const before = native.get.call(this);
      native.set.call(this, value);
      record('write', { before, value, stack: new Error().stack });
    } });
    for (const type of ['scroll', 'scrollend', 'touchstart', 'touchend']) el.addEventListener(type, e => record(type, { trusted: e.isTrusted }), { capture: true, passive: true });
    document.addEventListener('keydown', e => queueMicrotask(() => record('key', { key: e.key, ctrl: e.ctrlKey, meta: e.metaKey, shift: e.shiftKey, taken: e.defaultPrevented, target: e.target?.tagName })), true);
    record('watch');
  });
}

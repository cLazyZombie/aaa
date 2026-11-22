// Node에서 받은 사건이라 renderer가 죽어도 남는다. 명시적 정리 시작은 호출 쪽에서 같은 record로 남긴다.
export function observePopupLifecycle(page, record) {
  const listeners = [];
  const on = (object, event, name) => {
    const listener = () => record(name);
    object.on(event, listener);
    listeners.push(() => object.off(event, listener));
  };
  on(page, "crash", "page.crash");
  on(page, "close", "page.close");
  on(page.context(), "close", "context.close");
  on(page.context().browser(), "disconnected", "browser.disconnected");
  return () => { for (const remove of listeners) remove(); };
}

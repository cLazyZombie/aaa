// 먼저 거부된 작업만으로 원인을 정하지 않는다. 정리 뒤 늦게 끝나는 작업도 따로 기록한다.
export async function clickForPopup(page, link, record = () => {}) {
  const observe = (name, promise) => promise.then((value) => {
    record(name, { status: "fulfilled" });
    return value;
  }, (error) => {
    record(name, { status: "rejected", error: error.message });
    throw error;
  });
  const [tab] = await Promise.all([
    observe("popup.wait", page.waitForEvent("popup")),
    observe("popup.click", link.click()),
  ]);
  return tab;
}

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

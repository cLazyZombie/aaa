// 가상 diff가 같은 구분선을 다시 그려도 사용자의 다음 키는 그 단추로 간다.
const focusedButtons = new WeakMap<HTMLElement, { button: HTMLElement; key: string }>();
function buttonKey(node: HTMLElement, button: HTMLElement): string {
  return JSON.stringify([
    node.shadowRoot?.querySelector("[data-title]")?.textContent,
    button.closest("[data-expand-index]")?.getAttribute("data-expand-index"),
    ["data-expand-up", "data-expand-down", "data-expand-both", "data-expand-all-button"].filter((attr) => button.hasAttribute(attr)),
  ]);
}

/** 기본 펼치기 UI에 한글 이름과 키보드 조작을 보탠다. 펼치기는 라이브러리가 맡는다. */
export function labelContextButtons(node: HTMLElement, _instance?: unknown, phase?: string) {
  if (phase === "unmount") { focusedButtons.delete(node); return; }
  const previous = focusedButtons.get(node);
  for (const lines of node.shadowRoot?.querySelectorAll<HTMLElement>("[data-unmodified-lines]") ?? []) {
    const count = lines.textContent?.match(/^(\d+) unmodified lines?$/)?.[1];
    if (count) lines.textContent = `변경 없는 ${count}줄`;
    else if (lines.textContent === "More unchanged context may be available") lines.textContent = "맥락 펼치기";
  }
  for (const button of node.shadowRoot?.querySelectorAll<HTMLElement>("[data-expand-button]") ?? []) {
    const all = button.hasAttribute("data-expand-all-button");
    const label = all ? "모두 펼치기" : button.hasAttribute("data-expand-up") ? "아래로 20줄 펼치기" : button.hasAttribute("data-expand-down") ? "위로 20줄 펼치기" : "양쪽 20줄 펼치기";
    if (all && button.textContent !== label) button.textContent = label;
    button.setAttribute("aria-label", label);
    button.title = label;
    button.tabIndex = 0;
    const key = buttonKey(node, button);
    button.onfocus = () => focusedButtons.set(node, { button, key });
    button.onblur = () => queueMicrotask(() => {
      // 실제 다른 곳으로 옮긴 focus는 버린다. 같은 render에서 제거된 단추만 복구한다.
      if (button.isConnected && focusedButtons.get(node)?.button === button) focusedButtons.delete(node);
    });
    button.onkeydown = (event) => {
      if (event.key === "Enter" || event.key === " ") {
        event.preventDefault();
        button.click();
      }
    };
  }
  if (previous && !previous.button.isConnected && node.isConnected
      && document.activeElement === document.body && document.hasFocus()) {
    const next = [...(node.shadowRoot?.querySelectorAll<HTMLElement>("[data-expand-button]") ?? [])]
      .find((button) => buttonKey(node, button) === previous.key && button.getClientRects().length > 0);
    next?.focus({ preventScroll: true });
  }
}

export interface InputSelection {
  start: number;
  end: number;
  direction: "forward" | "backward" | "none";
  top: number;
  left: number;
}

const restoring = new WeakSet<HTMLElement>();
export const restoringFocus = (node: HTMLElement): boolean => restoring.has(node);

export function inputSelection(node: HTMLElement): InputSelection | null {
  if (!(node instanceof HTMLTextAreaElement || node instanceof HTMLInputElement) || node.selectionStart === null || node.selectionEnd === null) return null;
  return { start: node.selectionStart, end: node.selectionEnd, direction: node.selectionDirection ?? "none", top: node.scrollTop, left: node.scrollLeft };
}

/** WebKit은 focus만 되돌리면 선택을 잃을 수 있으므로 focus 뒤에 범위와 스크롤을 복원한다. */
export function focusWithSelection(node: HTMLElement, selection: InputSelection | null | undefined): void {
  restoring.add(node);
  try {
    node.focus({ preventScroll: true });
    if (selection && (node instanceof HTMLTextAreaElement || node instanceof HTMLInputElement)) {
      node.setSelectionRange(selection.start, selection.end, selection.direction);
      node.scrollTop = selection.top;
      node.scrollLeft = selection.left;
    }
  } finally { restoring.delete(node); }
}

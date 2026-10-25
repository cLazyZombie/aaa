import { useLayoutEffect, useRef, type RefObject } from "react";
import { focusWithSelection, inputSelection, restoringFocus, type InputSelection } from "./focusPosition";

interface ChatFocus {
  target: WeakRef<HTMLElement>;
  composer: boolean;
  selection: InputSelection | null;
}

// 초안 본문은 기존 composerDrafts가 맡는다. DOM 보관이 끝나도 입력 위치만 작게 남긴다.
// WeakRef로 만료된 대화 DOM·연결을 붙잡지 않고, pane 기록 수도 제한한다.
const remembered = new Map<string, ChatFocus>();
const LIMIT = 128;
const COMPOSER = "textarea.composer-text";

function rememberTarget(owner: string, node: HTMLElement, selection = inputSelection(node)): void {
  remembered.delete(owner);
  remembered.set(owner, { target: new WeakRef(node), composer: node.matches(COMPOSER), selection });
  if (remembered.size > LIMIT) remembered.delete(remembered.keys().next().value!);
}

/** 보관 대화마다 마지막 focus·선택을 기억한다. 터치 전환에서는 자판을 열지 않는다. */
export function useChatFocus(surface: RefObject<HTMLElement | null>, owner: string, active: boolean, autofocus: boolean, request: number): void {
  const enabled = useRef(active);
  enabled.current = active;
  useLayoutEffect(() => {
    const root = surface.current;
    if (!root) return;
    const remember = (event: Event) => {
      const node = event.target;
      if (!(node instanceof HTMLElement) || node.closest("[aria-modal='true'], dialog[open]") || node.closest(".term-stack")) return;
      // focusin 시점의 WebKit 임시 커서 0으로 복원 중인 선택을 덮지 않는다.
      if (restoringFocus(node)) return;
      if (event.type !== "focusout" && node !== document.activeElement) return;
      // hidden/inert 반영 뒤 focusout가 올 수 있고 select 사건은 그보다 늦다.
      // 이탈 직전 대상의 focusout까지 받아 마지막 선택을 확정한다.
      if (!enabled.current && !(event.type === "focusout" && remembered.get(owner)?.target.deref() === node)) return;
      const selection = inputSelection(node);
      const previous = remembered.get(owner)?.selection;
      // display:none 뒤에는 scrollTop이 0으로 보인다. 숨기기 전 키/스크롤 사건의 값을 쓴다.
      if (selection && previous && root.hidden) {
        selection.top = previous.top;
        selection.left = previous.left;
      }
      rememberTarget(owner, node, selection);
    };
    // focusout는 Ctrl+Tab 창이 focus를 가져가기 직전의 선택을 남긴다.
    const events = ["focusin", "focusout", "select", "keydown", "keyup", "pointerup", "input", "scroll"];
    for (const event of events) root.addEventListener(event, remember, true);
    return () => { for (const event of events) root.removeEventListener(event, remember, true); };
  }, [surface, owner]);

  useLayoutEffect(() => {
    const root = surface.current;
    if (!active || !autofocus || !root || document.querySelector("[aria-modal='true'], dialog[open]")) return;
    const saved = remembered.get(owner);
    const previous = saved?.target.deref();
    const valid = previous && root.contains(previous) && !previous.closest("[hidden], [inert]") && !previous.matches(":disabled") && previous.getClientRects().length > 0;
    const target = valid ? previous : root.querySelector<HTMLTextAreaElement>(COMPOSER);
    if (!target) return;
    const selection = valid || saved?.composer ? saved?.selection : null;
    focusWithSelection(target, selection);
    rememberTarget(owner, target);
  }, [surface, owner, active, autofocus, request]);
}

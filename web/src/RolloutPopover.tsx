// t-0963 A안: RowMenu의 앵커 아래 팝업·휴대폰 시트 구성만 참고한다. 외부 코드 복사 없음.
import { useLayoutEffect, useRef, type RefObject } from "react";
import { createPortal } from "react-dom";
import { X } from "lucide-react";
import { RolloutCard, type RolloutSnapshot } from "./RolloutCard";
import { useFocusTrap } from "./herdr/useFocusTrap";
import "./RolloutPopover.css";

export function RolloutPopover({ snapshot, connected, anchor, opener, onClose }: {
  snapshot: RolloutSnapshot; connected: boolean; anchor: RefObject<HTMLElement | null>; opener: HTMLElement; onClose: () => void;
}) {
  const close = useRef<HTMLButtonElement>(null);
  const dialog = useFocusTrap<HTMLDialogElement>(true, { initialFocus: close, shouldRestore: () => false });
  useLayoutEffect(() => {
    const node = dialog.current!;
    const place = () => {
      const rect = anchor.current?.getBoundingClientRect();
      node.style.setProperty("--rollout-left", `${Math.max(8, Math.min(rect?.left ?? 8, innerWidth - 508))}px`);
      node.style.setProperty("--rollout-top", `${Math.max(8, Math.min((rect?.bottom ?? 40) + 8, innerHeight - 200))}px`);
    };
    place();
    node.showModal();
    close.current?.focus();
    window.addEventListener("resize", place);
    return () => {
      window.removeEventListener("resize", place);
      // native modal을 먼저 닫아 배경 inert를 푼 뒤, Safari에서도 실제 누른 단추로 돌린다.
      node.close();
      if (opener.isConnected) opener.focus({ preventScroll: true });
    };
  }, [anchor, dialog, opener]);
  return createPortal(<dialog ref={dialog} className="rollout-popover" aria-label="최근 배포" aria-modal="true" tabIndex={-1}
    onCancel={(event) => { event.preventDefault(); event.stopPropagation(); onClose(); }}
    onKeyDown={(event) => { if (event.key === "Escape") event.stopPropagation(); }}
    onClick={(event) => {
      if (event.target !== event.currentTarget) return;
      const rect = event.currentTarget.getBoundingClientRect();
      if (event.clientX < rect.left || event.clientX > rect.right || event.clientY < rect.top || event.clientY > rect.bottom) onClose();
    }}>
    <header className="rollout-popover-heading"><h2>최근 배포</h2><button ref={close} type="button" className="icon-button" aria-label="배포 상세 닫기" onClick={onClose}><X aria-hidden="true" /></button></header>
    <RolloutCard snapshot={snapshot} connected={connected} help={false} collapsible={false} />
    <p className="rollout-readonly">읽기 전용 · 머신 줄은 확인된 실행 판, 이 창은 배포 대상 판입니다.</p>
  </dialog>, document.body);
}

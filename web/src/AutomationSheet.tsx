// 자동화 상세·변환 창의 틀: 데스크톱은 가운데 창, 휴대폰은 아래 sheet(t3code ScheduledTasksSettings의 Dialog → 모바일 sheet,
// 모양·움직임은 herdr modal.css). Esc·바깥 누름·닫기(X)·휴대폰 「뒤로」·아래로 밀기가 같은 닫기다. 위에 확인창을 띄운 동안
// (`covered`)에는 그 창이 Esc와 뒤로를 갖는다.
import { X } from "lucide-react";
import { useEffect, useId, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { nativeModalOver, useFocusTrap } from "./herdr/useFocusTrap.ts";
import { useSheetSwipe } from "./herdr/sheets.ts";
import { usePhoneSheetBack } from "./herdr/sheetHistory.ts";

export function AutomationSheet({ title, subtitle, onClose, covered = false, busy = false, testid, footer, children }: {
  title: string;
  subtitle?: ReactNode;
  onClose: () => void;
  /** 위에 다른 창이 떠 있다 */
  covered?: boolean;
  /** 일하는 중: 닫지 않는다 */
  busy?: boolean;
  testid?: string;
  footer?: ReactNode;
  children: ReactNode;
}) {
  const id = useId();
  const surface = useFocusTrap<HTMLDivElement>(!covered);
  const close = busy ? null : onClose;
  useSheetSwipe(surface, covered ? null : close);
  usePhoneSheetBack(() => {
    if (busy) return false;
    onClose();
  }, !covered);
  useEffect(() => {
    if (covered) return;
    const onKey = (event: KeyboardEvent): void => {
      if (event.key !== "Escape" || nativeModalOver(surface.current)) return;
      event.stopPropagation();
      event.preventDefault();
      if (!busy) onClose();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onClose, busy, covered]); // eslint-disable-line react-hooks/exhaustive-deps
  return createPortal(
    <div className="modal-scrim" onMouseDown={(event) => { if (event.target === event.currentTarget && !busy && !covered) onClose(); }}>
      <div ref={surface} className="modal automation-sheet" role="dialog" aria-modal="true" aria-labelledby={`${id}-title`} tabIndex={-1} data-testid={testid}>
        <header className="modal-header">
          <div className="automation-sheet-heading">
            <h2 className="modal-title" id={`${id}-title`}>{title}</h2>
            {subtitle && <div className="automation-sheet-subtitle">{subtitle}</div>}
          </div>
          <button type="button" className="icon-button" aria-label="닫기" title="닫기" disabled={busy} onClick={onClose}><X /></button>
        </header>
        <div className="modal-body automation-sheet-body">{children}</div>
        {footer && <footer className="modal-footer">{footer}</footer>}
      </div>
    </div>,
    document.body,
  );
}

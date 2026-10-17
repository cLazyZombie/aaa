// 되돌릴 수 없는 일 앞의 예·아니오(herdr-web-ui aa624cb src/components/ConfirmDialog.tsx, 모양 herdr/ConfirmDialog.css 그대로).
// 취소가 focus를 받아 Enter는 「아니오」다. Esc·바깥 누름도 아니오, Tab은 두 단추 사이에 머문다. 휴대폰에서는 아래로 밀어 닫고
// 시스템 「뒤로」도 닫는다. 일은 여기서 하므로 실패는 창 안에 보인다. 다른 곳: 참고의 escalation(「그래도 지우기」)은 쓰는 곳이 없어 뺐다.
import "./herdr/ConfirmDialog.css";

import { useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { nativeModalOver, useFocusTrap } from "./herdr/useFocusTrap.ts";
import { useSheetSwipe } from "./herdr/sheets.ts";
import { usePhoneSheetBack } from "./herdr/sheetHistory.ts";

export function ConfirmDialog({
  title,
  body,
  confirmLabel,
  onConfirm,
  onClose,
}: {
  title: string;
  body: string;
  confirmLabel: string;
  /** 일이 끝나면 resolve한다. 그 뒤 창을 내리는 것은 부른 쪽이다 */
  onConfirm: () => Promise<void>;
  onClose: () => void;
}) {
  const id = useId();
  const cancel = useRef<HTMLButtonElement>(null);
  const done = useRef(false);
  const surface = useFocusTrap<HTMLDivElement>(true, { initialFocus: cancel, shouldRestore: () => !done.current });
  const [pending, setPending] = useState(false);
  useSheetSwipe(surface, pending ? null : onClose);
  usePhoneSheetBack(() => {
    if (pending) return false;
    onClose();
  });
  const [error, setError] = useState<string | null>(null);

  // 일하는 동안 두 단추가 꺼지면 focus가 페이지로 떨어진다: 창이 쥔다
  useEffect(() => {
    if (pending) surface.current?.focus();
  }, [pending]); // eslint-disable-line react-hooks/exhaustive-deps
  // 창이 떠 있는 동안 Esc는 이 창의 것이다(설정 창이 같이 닫히지 않게 capture에서 멈춘다)
  useEffect(() => {
    const onKey = (event: KeyboardEvent): void => {
      if (event.key !== "Escape" || nativeModalOver(surface.current)) return;
      event.stopPropagation();
      event.preventDefault();
      if (!pending) onClose();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onClose, pending]); // eslint-disable-line react-hooks/exhaustive-deps

  const confirm = async (): Promise<void> => {
    setPending(true);
    setError(null);
    try {
      await onConfirm();
      done.current = true;
    } catch (reason: unknown) {
      setError(reason instanceof Error ? reason.message : String(reason));
      setPending(false);
    }
  };

  return createPortal(
    <div
      className="modal-scrim"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && !pending) onClose();
      }}
    >
      <div ref={surface} className="modal confirm-dialog" role="alertdialog" aria-modal="true" aria-labelledby={`${id}-title`} aria-describedby={`${id}-body`} tabIndex={-1}>
        <header className="modal-header">
          <h2 className="modal-title" id={`${id}-title`}>
            {title}
          </h2>
        </header>
        <div className="modal-body">
          <p className="confirm-body" id={`${id}-body`}>
            {body}
          </p>
          {error && (
            <p className="confirm-error" role="alert">
              {error}
            </p>
          )}
        </div>
        <footer className="modal-footer">
          <button ref={cancel} type="button" className="btn" disabled={pending} onClick={onClose}>
            취소
          </button>
          <button type="button" className="btn btn-danger" disabled={pending} onClick={() => void confirm()}>
            {confirmLabel}
          </button>
        </footer>
      </div>
    </div>,
    document.body,
  );
}

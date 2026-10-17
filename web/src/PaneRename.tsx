// pane에 사람이 붙이는 이름(herdr-web-ui aa624cb src/components/PaneCanvas.tsx `PaneRename`, 모양 herdr/PaneRename.css).
// 이름 칸에 focus(글 전체 선택)·Enter 저장·Esc와 바깥 누름은 취소, 비우고 저장하면 붙인 이름을 지운다(host가 띄울 때 이름으로 돌림).
// 휴대폰에서는 시스템 「뒤로」로 닫는다. 실패는 창 안에 보이고 친 이름은 남는다. 다른 곳: 문구는 aaa 화면 말(「패널」 대신 pane).
import "./herdr/PaneRename.css";

import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { nativeModalOver, useFocusTrap } from "./herdr/useFocusTrap.ts";
import { usePhoneSheetBack } from "./herdr/sheetHistory.ts";

/** host·agent와 같은 한도(aaa-proto `MAX_PANE_LABEL_CHARS`, 참고 입력 `maxLength={80}`) */
const MAX_PANE_LABEL_CHARS = 80;

export function PaneRename({ label, onClose, onSave }: { label: string | null; onClose: () => void; onSave: (label: string) => Promise<void> }) {
  const input = useRef<HTMLInputElement>(null);
  const done = useRef(false);
  const surface = useFocusTrap<HTMLDivElement>(true, { initialFocus: input, shouldRestore: () => !done.current });
  const [value, setValue] = useState(label ?? "");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  usePhoneSheetBack(() => {
    if (busy) return false;
    onClose();
  });
  useEffect(() => {
    const key = (event: KeyboardEvent): void => {
      if (event.key !== "Escape" || nativeModalOver(surface.current)) return;
      event.preventDefault();
      event.stopPropagation();
      if (!busy) onClose();
    };
    window.addEventListener("keydown", key, true);
    return () => window.removeEventListener("keydown", key, true);
  }, [busy, onClose]); // eslint-disable-line react-hooks/exhaustive-deps
  return createPortal(
    <div
      className="modal-scrim"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && !busy) onClose();
      }}
    >
      <div ref={surface} className="modal" role="dialog" aria-modal="true" aria-label="pane 이름 바꾸기">
        <form
          className="pane-rename-form"
          onSubmit={(event) => {
            event.preventDefault();
            if (busy) return;
            setBusy(true);
            setError(null);
            void onSave(value.trim()).then(
              () => {
                done.current = true;
                onClose();
              },
              (reason: unknown) => {
                setError(reason instanceof Error ? reason.message : String(reason));
                setBusy(false);
              },
            );
          }}
        >
          <header className="modal-header">
            <h2 className="modal-title">pane 이름 바꾸기</h2>
          </header>
          <div className="modal-body">
            <label>
              pane 이름
              <input
                ref={input}
                className="input"
                value={value}
                disabled={busy}
                maxLength={MAX_PANE_LABEL_CHARS}
                onFocus={(event) => event.currentTarget.select()}
                onChange={(event) => setValue(event.target.value)}
              />
            </label>
            {error && <p role="alert">{error}</p>}
          </div>
          <footer className="modal-footer">
            <button type="button" className="btn" disabled={busy} onClick={onClose}>
              취소
            </button>
            <button className="btn btn-primary" disabled={busy}>
              저장
            </button>
          </footer>
        </form>
      </div>
    </div>,
    document.body,
  );
}

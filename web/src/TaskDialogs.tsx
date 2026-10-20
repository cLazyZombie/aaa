// 할 일 편집의 창(할 일 편집 T2, plan §8.310): 충돌 창·목록 이름·원문 편집. 창 틀은 PaneRename(herdr-web-ui PaneCanvas.tsx
// `PaneRename`)과 같다: focus 가두기, Esc·바깥 누름은 닫기(일하는 동안 제외), 휴대폰은 시스템 「뒤로」로 닫는다. 충돌 창은 참고 넷에
// 없다(t-0469 §1.3): 나란히 보기는 herdr-web-ui RepoDiffViewer.tsx 118–174의 좁은 화면 한 줄 접기를 따른다.
import "./herdr/PaneRename.css";

import { useEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import type { TaskConflict } from "./gen/proto";
import { nativeModalOver, useFocusTrap } from "./herdr/useFocusTrap.ts";
import { usePhoneSheetBack } from "./herdr/sheetHistory.ts";
import { canonicalLine } from "./tasksModel";

function TaskModal({ label, busy, onClose, wide, children, testid }: { label: string; busy: boolean; onClose: () => void; wide?: boolean; children: ReactNode; testid: string }) {
  const done = useRef(false);
  const surface = useFocusTrap<HTMLDivElement>(true, { shouldRestore: () => !done.current });
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
      <div ref={surface} className={`modal task-modal${wide ? " is-wide" : ""}`} role="dialog" aria-modal="true" aria-label={label} data-testid={testid}>
        {children}
      </div>
    </div>,
    document.body,
  );
}

/** 내 고침(저장하려던 것) */
export interface MyEdit {
  title: string;
  owner: string | null;
  thread: string | null;
  notes: string[];
}

/**
 * 충돌 창: 저장하려던 task가 그사이 바뀌었거나 지워졌다(host가 쓰지 않았다). 내 고침과 지금 파일의 원문을 나란히 보이고 「최신으로
 * 다시 열기」(폼을 지금 원문으로) 또는 「닫기」(내 고침을 폼에 둔 채, 옮겨 적을 수 있게)를 고른다. 자동 병합은 하지 않는다(t-0469 §2.3).
 */
export function ConflictDialog({ conflict, mine, onReopen, onClose }: { conflict: TaskConflict; mine: MyEdit | null; onReopen: () => void; onClose: () => void }) {
  const latest = conflict.latest;
  return (
    <TaskModal label="저장하지 못했다: 그사이 바뀜" busy={false} onClose={onClose} wide testid="task-conflict">
      <header className="modal-header">
        <h2 className="modal-title">그사이 바뀌어 저장하지 않았다</h2>
      </header>
      <div className="modal-body">
        <p className="task-conflict-why">{conflict.reason}</p>
        <div className="task-conflict-sides">
          <section aria-label="내 고침">
            <h3>내 고침</h3>
            {mine ? (
              <pre className="task-raw" data-testid="task-conflict-mine">
                <code>{[canonicalLine(mine.title, mine.owner, mine.thread), ...mine.notes.map((n) => (n ? `  ${n}` : ""))].join("\n")}</code>
              </pre>
            ) : (
              <p className="muted">목록 동작</p>
            )}
          </section>
          <section aria-label="지금 파일">
            <h3>지금 파일</h3>
            {latest ? (
              <pre className="task-raw" data-testid="task-conflict-latest">
                <code>{[latest.raw, ...latest.notes.map((n) => `  ${n}`)].join("\n")}</code>
              </pre>
            ) : (
              <p className="muted" data-testid="task-conflict-latest">
                지워졌다(기록에서 되살릴 수 있다)
              </p>
            )}
          </section>
        </div>
      </div>
      <footer className="modal-footer">
        <button type="button" className="btn" onClick={onClose}>
          닫기
        </button>
        <button type="button" className="btn btn-primary" data-testid="task-conflict-reopen" onClick={onReopen}>
          {latest ? "최신으로 다시 열기" : "목록으로"}
        </button>
      </footer>
    </TaskModal>
  );
}

/** 이름 하나를 받는 창(새 목록·목록 이름 바꾸기). 실패는 창 안에 보이고 친 이름은 남는다 */
export function NameDialog({ title, label, initial, action, onSave, onClose }: { title: string; label: string; initial: string; action: string; onSave: (name: string) => Promise<void>; onClose: () => void }) {
  const [value, setValue] = useState(initial);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  return (
    <TaskModal label={title} busy={busy} onClose={onClose} testid="task-name-dialog">
      <form
        className="pane-rename-form"
        onSubmit={(event) => {
          event.preventDefault();
          if (busy || value.trim() === "") return;
          setBusy(true);
          setError(null);
          void onSave(value.trim()).then(onClose, (reason: unknown) => {
            setError(reason instanceof Error ? reason.message : String(reason));
            setBusy(false);
          });
        }}
      >
        <header className="modal-header">
          <h2 className="modal-title">{title}</h2>
        </header>
        <div className="modal-body">
          <label>
            {label}
            {/* eslint-disable-next-line jsx-a11y/no-autofocus */}
            <input className="input" value={value} disabled={busy} autoFocus onFocus={(e) => e.currentTarget.select()} onChange={(e) => setValue(e.target.value)} data-testid="task-name-input" />
          </label>
          {error && <p role="alert">{error}</p>}
        </div>
        <footer className="modal-footer">
          <button type="button" className="btn" disabled={busy} onClick={onClose}>
            취소
          </button>
          <button className="btn btn-primary" disabled={busy || value.trim() === ""} data-testid="task-name-save">
            {action}
          </button>
        </footer>
      </form>
    </TaskModal>
  );
}

/**
 * 원문 편집(고급, t-0469 §2.3): 파일 전체를 고친다. 연 판(sha256)이 그대로일 때만 저장하고, 그사이 바뀌었으면 쓰지 않고 「최신으로
 * 다시 열기」를 보인다. 블록 단위 충돌 감지가 없어 늘 쓰는 길이 아니다.
 */
export function RawEditor({ text, sha256, onSave, onClose }: { text: string; sha256: string; onSave: (text: string, sha256: string) => Promise<"saved" | "conflict">; onClose: () => void }) {
  const [base, setBase] = useState({ text, sha256 });
  const [value, setValue] = useState(text);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [stale, setStale] = useState(false);
  return (
    <TaskModal label="TASKS.md 원문 편집" busy={busy} onClose={onClose} wide testid="task-raw-editor">
      <form
        className="task-raw-form"
        onSubmit={(event) => {
          event.preventDefault();
          if (busy) return;
          setBusy(true);
          setError(null);
          void onSave(value, base.sha256).then(
            (r) => {
              setBusy(false);
              if (r === "saved") onClose();
              else setStale(true);
            },
            (reason: unknown) => {
              setError(reason instanceof Error ? reason.message : String(reason));
              setBusy(false);
            },
          );
        }}
      >
        <header className="modal-header">
          <h2 className="modal-title">TASKS.md 원문 편집</h2>
        </header>
        <div className="modal-body">
          <p className="muted">파일 전체를 바꾼다. 연 뒤 파일이 바뀌었으면 저장하지 않는다.</p>
          <textarea className="input task-raw-text" value={value} disabled={busy} spellCheck={false} onChange={(e) => setValue(e.target.value)} aria-label="TASKS.md 원문" data-testid="task-raw-text" />
          {stale && (
            <p role="alert" data-testid="task-raw-stale">
              연 뒤 파일이 바뀌어 저장하지 않았다.{" "}
              <button
                type="button"
                className="linkish"
                onClick={() => {
                  setBase({ text, sha256 });
                  setValue(text);
                  setStale(false);
                }}
              >
                최신으로 다시 열기
              </button>
            </p>
          )}
          {error && <p role="alert">{error}</p>}
        </div>
        <footer className="modal-footer">
          <button type="button" className="btn" disabled={busy} onClick={onClose}>
            취소
          </button>
          <button className="btn btn-primary" disabled={busy || value === base.text} data-testid="task-raw-save">
            저장
          </button>
        </footer>
      </form>
    </TaskModal>
  );
}

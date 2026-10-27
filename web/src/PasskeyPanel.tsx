// 조작 기기 passkey(관리 GUI W7): 짝지은 뒤 10분 안 등록 안내 줄(본문 위)과 설정 → 폰 및 기기의 이 기기 passkey 상태.
// 상태는 `/api/v1/me`의 `passkey`(공개 주소로 연 화면만)다. 누름이 지난 뒤의 WebAuthn을 위한 「passkey로 승인」 창도 여기 있다. host Mac에서 연 loopback 화면은 passkey 없이 승인한다.
import { useCallback, useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { DeviceRole, PasskeyState } from "./gen/proto";
import { ApiError, me, registerPasskey } from "./api";
import "./herdr/ConfirmDialog.css";
import { useFocusTrap } from "./herdr/useFocusTrap.ts";
import { PasskeyCancelled, canRegister, minutesLeft, setPasskeyGesture } from "./passkey";

const message = (e: unknown): string => (e instanceof ApiError || e instanceof Error ? e.message : String(e));

/** 이 화면의 passkey 상태: undefined는 읽는 중, null은 passkey를 쓰지 않는 화면(loopback·옛 host) */
export function usePasskeyState(): [PasskeyState | null | undefined, () => void] {
  const [state, setState] = useState<PasskeyState | null | undefined>(undefined);
  const load = useCallback(() => {
    me().then(
      (m) => setState(m?.passkey ?? null),
      () => setState(null),
    );
  }, []);
  useEffect(load, [load]);
  // 남은 분을 다시 그리고, 시한이 지나면 안내를 거둔다
  const [, tick] = useState(0);
  const open = canRegister(state);
  useEffect(() => {
    if (!open) return;
    const t = window.setInterval(() => tick((n) => n + 1), 30_000);
    return () => window.clearInterval(t);
  }, [open]);
  return [state, load];
}

function RegisterButton({ onDone, primary }: { onDone: () => void; primary?: boolean }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const run = async () => {
    setBusy(true);
    setError(null);
    try {
      await registerPasskey();
      onDone();
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <>
      <button type="button" className={primary ? "btn btn-primary" : "btn"} disabled={busy} onClick={() => void run()} data-testid="passkey-register">
        {busy ? "등록 중" : "passkey 등록"}
      </button>
      {error && (
        <span className="op-warn" role="alert">
          {error}
        </span>
      )}
    </>
  );
}

/** 본문 위 한 줄: 짝지은 지 10분이 안 된 공개 주소의 조작 기기에 passkey가 없다 */
export function PasskeyNotice({ state, onChanged }: { state: PasskeyState | null | undefined; onChanged: () => void }) {
  if (!state || !canRegister(state)) return null;
  return (
    <div className="update-notice is-line is-warning" role="status" data-testid="passkey-notice">
      <span>이 기기에 passkey가 없다. {minutesLeft(state)}분 안에만 등록할 수 있다.</span>
      <RegisterButton onDone={onChanged} primary />
    </div>
  );
}

/** 설정 → 폰 및 기기: 이 기기 passkey 한 줄 */
export function PasskeyStatus({ state, role, onChanged }: { state: PasskeyState | null | undefined; role: DeviceRole; onChanged: () => void }) {
  if (state === undefined) return <span className="op-muted">불러오는 중</span>;
  if (role === "reader") return <span className="op-muted" data-testid="passkey-status">보기 기기는 passkey를 쓰지 않는다.</span>;
  if (state === null) return <span className="op-muted" data-testid="passkey-status">이 화면(host Mac)은 passkey 없이 승인한다.</span>;
  if (state.registered)
    return (
      <span data-testid="passkey-status">등록됨({state.rp_id})</span>
    );
  if (canRegister(state))
    return (
      <span data-testid="passkey-status">
        <span className="op-muted">없음. {minutesLeft(state)}분 안에 등록한다. </span>
        <RegisterButton onDone={onChanged} />
      </span>
    );
  return (
    <span className="op-muted" data-testid="passkey-status">
      없음. 짝지은 뒤 10분이 지나 이 브라우저에서는 등록할 수 없다.
    </span>
  );
}

type Ask = { seq: number; what: string; run: () => Promise<unknown>; resolve: (v: unknown) => void; reject: (e: Error) => void };

/** 누름이 지난 뒤의 passkey 확인(WebKit): 이 창의 단추 처리기가 WebAuthn을 바로 부른다. 다른 확인 창(휴대폰 sheet) 위에 뜨므로
 * 시스템 「뒤로」 기록·밀어 닫기를 쓰지 않는다: 그 기록은 아래 창의 것이다 */
export function PasskeyGesture() {
  const [ask, setAsk] = useState<Ask | null>(null);
  const seq = useRef(0);
  useEffect(() => {
    setPasskeyGesture(
      (what, run) =>
        new Promise((resolve, reject) =>
          setAsk((old) => {
            old?.reject(new PasskeyCancelled("passkey 확인을 다른 승인이 대신했다."));
            return { seq: ++seq.current, what, run, resolve, reject };
          }),
        ),
    );
    return () => setPasskeyGesture(null);
  }, []);
  if (!ask) return null;
  return <GestureDialog key={ask.seq} ask={ask} onDone={() => setAsk(null)} />;
}

function GestureDialog({ ask, onDone }: { ask: Ask; onDone: () => void }) {
  const id = useId();
  const cancelRef = useRef<HTMLButtonElement>(null);
  const surface = useFocusTrap<HTMLDivElement>(true, { initialFocus: cancelRef });
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const cancel = useCallback(() => {
    onDone();
    ask.reject(new PasskeyCancelled("passkey 확인을 취소했다."));
  }, [ask, onDone]);
  // Esc는 이 창의 것이다(아래 창이 같이 닫히지 않게 capture에서 멈춘다)
  useEffect(() => {
    const onKey = (event: KeyboardEvent): void => {
      if (event.key !== "Escape") return;
      event.stopPropagation();
      event.preventDefault();
      if (!pending) cancel();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [cancel, pending]);
  // 누름 처리기 안에서 바로 WebAuthn을 부른다(await 전에)
  const confirm = (): void => {
    setPending(true);
    setError(null);
    ask.run().then(
      (v) => {
        onDone();
        ask.resolve(v);
      },
      (e: unknown) => {
        setError(message(e));
        setPending(false);
      },
    );
  };
  return createPortal(
    <div className="modal-scrim">
      <div ref={surface} className="modal confirm-dialog" role="alertdialog" aria-modal="true" aria-labelledby={`${id}-title`} aria-describedby={`${id}-body`} tabIndex={-1}>
        <header className="modal-header">
          <h2 className="modal-title" id={`${id}-title`}>
            passkey로 {ask.what}할까요?
          </h2>
        </header>
        <div className="modal-body">
          <p className="confirm-body" id={`${id}-body`}>
            이 기기의 Face ID·지문·암호로 사용자 확인을 합니다.
          </p>
          {error && (
            <p className="confirm-error" role="alert">
              {error}
            </p>
          )}
        </div>
        <footer className="modal-footer">
          <button ref={cancelRef} type="button" className="btn" disabled={pending} onClick={cancel}>
            취소
          </button>
          <button type="button" className="btn btn-primary" disabled={pending} onClick={confirm}>
            passkey로 {ask.what}
          </button>
        </footer>
      </div>
    </div>,
    document.body,
  );
}

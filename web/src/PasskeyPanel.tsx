// 조작 기기 passkey(관리 GUI W7): 짝지은 뒤 10분 안 등록 안내 줄(본문 위)과 설정 → 폰 및 기기의 이 기기 passkey 상태.
// 상태는 `/api/v1/me`의 `passkey`(공개 주소로 연 화면만)다. host Mac에서 연 loopback 화면은 passkey 없이 승인한다.
import { useCallback, useEffect, useState } from "react";
import type { DeviceRole, PasskeyState } from "./gen/proto";
import { ApiError, me, registerPasskey } from "./api";
import { canRegister, minutesLeft } from "./passkey";

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
      <span>
        이 기기에 passkey를 등록한다. 공개 주소에서 승인하려면 필요하고, 짝지은 뒤 {minutesLeft(state)}분 안에만 등록할 수 있다.
      </span>
      <RegisterButton onDone={onChanged} primary />
    </div>
  );
}

/** 설정 → 폰 및 기기: 이 기기 passkey 한 줄 */
export function PasskeyStatus({ state, role, onChanged }: { state: PasskeyState | null | undefined; role: DeviceRole; onChanged: () => void }) {
  if (state === undefined) return <span className="op-muted">불러오는 중</span>;
  if (role === "reader") return <span className="op-muted" data-testid="passkey-status">보기 기기는 승인하지 않아 passkey를 쓰지 않는다.</span>;
  if (state === null) return <span className="op-muted" data-testid="passkey-status">이 화면(host Mac)은 passkey 없이 확인 창으로 승인한다.</span>;
  if (state.registered)
    return (
      <span data-testid="passkey-status">
        등록됨({state.rp_id}). 승인할 때마다 이 기기의 사용자 확인(Face ID·지문·암호)을 한다.
      </span>
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
      없음. 짝지은 뒤 10분이 지나 이 브라우저에서는 등록할 수 없다. host Mac 화면이나 passkey가 있는 기기에서 짝짓기 코드를 받아 다시
      짝지은 뒤 10분 안에 등록한다. 그때까지 이 기기는 승인할 수 없다.
    </span>
  );
}

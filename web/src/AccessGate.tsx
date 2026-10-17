// 연결(pairing) 화면. herdr-web-ui(aa624cb) src/components/AccessGate.tsx를 옮겼다(모양 herdr/AccessGate.css는 그대로, 문구는 참고 i18n.ko.ts).
// 다른 곳: aaa에는 공유 token 입장이 없어 「액세스 토큰」 펼침을 뺐다(§6.4 「입장은 pairing만」). 코드는 host 머신의
// 설정 → 폰 및 기기나 `aaa web pair`에서 받는다. QR이 연 주소의 `?pair=`는 App이 읽자마자 주소에서 지우고 여기서 바로 낸다.
import "./herdr/AccessGate.css";

import { type SubmitEvent, useEffect, useRef, useState } from "react";
import type { DeviceRole } from "./gen/proto";
import { ApiError, pair } from "./api";
import { deviceLabel } from "./herdr/phone";

export function AccessGate({ initialCode, onUnlocked }: { initialCode: string; onUnlocked: (role: DeviceRole) => void }) {
  const codeRef = useRef<HTMLInputElement | null>(null);
  const [code, setCode] = useState(initialCode);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const scanned = useRef(code.length === 6);

  const submit = async (value: string): Promise<void> => {
    if (submitting) return;
    setSubmitting(true);
    setError(null);
    try {
      const r = await pair(value, deviceLabel(navigator.userAgent, navigator.maxTouchPoints ?? 0));
      onUnlocked(r.role);
    } catch (err) {
      // 틀림·이미 씀·만료는 host가 unauthorized로, 너무 많이 틀려 잠긴 동안은 unavailable(남은 초)로 답한다
      setError(
        err instanceof ApiError && err.body.code === "unauthorized"
          ? "코드가 틀렸거나 이미 쓰였거나 만료되었습니다. host 머신에서 새 페어링을 시작하세요."
          : err instanceof Error
            ? err.message
            : String(err),
      );
      setSubmitting(false);
      codeRef.current?.select();
    }
  };

  // QR 코드가 코드를 가져왔다: 폰에서 칠 것이 없다
  useEffect(() => {
    if (scanned.current) {
      scanned.current = false;
      void submit(code);
    }
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const onSubmit = (event: SubmitEvent<HTMLFormElement>): void => {
    event.preventDefault();
    if (code.length !== 6) {
      codeRef.current?.focus();
      return;
    }
    void submit(code);
  };

  return (
    <main className="access-gate-screen">
      <div className="access-gate" data-testid="token-gate">
        <img src="/icon.svg" alt="" width="44" height="44" className="access-gate-mark" />
        <h1 id="access-gate-title" className="access-gate-title">
          aaa
        </h1>
        <p className="access-gate-copy">
          host 머신에서 받은 코드로 이 기기를 페어링하세요. 코드는 이미 페어링한 기기의 설정 → 폰 및 기기에서, 또는 host 머신 터미널의 <code>aaa web pair</code>로 받습니다.
        </p>
        <form aria-labelledby="access-gate-title" onSubmit={onSubmit}>
          <label className="access-gate-label" htmlFor="access-gate-code">
            페어링 코드
          </label>
          <input
            ref={codeRef}
            id="access-gate-code"
            className="access-gate-input"
            type="text"
            inputMode="numeric"
            autoComplete="one-time-code"
            pattern="[0-9]*"
            maxLength={7}
            placeholder="000 000"
            autoFocus
            aria-invalid={error !== null}
            aria-describedby={error !== null ? "access-gate-error" : undefined}
            value={code}
            onChange={(event) => setCode(event.target.value.replace(/\D/g, "").slice(0, 6))}
          />
          <button type="submit" className="access-gate-submit" disabled={submitting}>
            {submitting ? "페어링 중…" : "이 기기 페어링"}
          </button>
        </form>
        {error !== null && (
          <p id="access-gate-error" className="access-gate-error" role="alert">
            {error}
          </p>
        )}
      </div>
    </main>
  );
}

import { apiFetch } from "./apiRequest";
import { useCallback, useEffect, useRef, useState } from "react";
import type { AccountQuota, AccountQuotaReport, Outcome } from "./gen/proto";
import { onAccountsChanged } from "./api";
import { usePageVisible } from "./pageVisible";
import { quotaLabel, quotaLeft, quotaProblem, quotaState, quotaTime } from "./accountQuotaModel";

export function useAccountQuota(enabled: boolean) {
  const [report, setReport] = useState<AccountQuotaReport | null>(null);
  const [error, setError] = useState(false);
  const [busy, setBusy] = useState(false);
  const [now, setNow] = useState(Date.now);
  const seq = useRef(0);
  const visible = usePageVisible();
  const load = useCallback(async (refresh = false) => {
    const mine = ++seq.current;
    setBusy(true);
    try {
      const res = await apiFetch(`/api/v1/account-usage${refresh ? "?refresh=1" : ""}`, { credentials: "same-origin", cache: "no-store" });
      const v = await res.json() as Outcome<AccountQuotaReport>;
      if (!res.ok || "err" in v) throw new Error("quota");
      if (mine === seq.current) { setReport(v.ok); setError(false); setNow(Date.now()); }
    } catch { if (mine === seq.current) setError(true); }
    finally { if (mine === seq.current) setBusy(false); }
  }, []);
  useEffect(() => {
    if (!enabled || !visible) return;
    void load();
    const timer = setInterval(() => { setNow(Date.now()); void load(); }, 60_000);
    const clock = setInterval(() => setNow(Date.now()), 1000);
    const off = onAccountsChanged(() => { setReport(null); void load(); });
    return () => { clearInterval(timer); clearInterval(clock); off(); seq.current++; };
  }, [enabled, visible, load]);
  return { report, error, busy, now, refresh: () => void load(true) };
}

export function AccountQuotaCard({ quota, now, failed }: { quota: AccountQuota | undefined; now: number; failed: boolean }) {
  const windows = quota?.windows ?? [];
  return <section className="acct-quota" data-testid="account-quota" aria-label="구독 남은 사용량">
    <h4>남은 사용량</h4>
    {(failed || quota?.problem) && <p className="op-muted" data-testid="quota-problem">{failed ? "다시 읽지 못함 · 마지막 관측" : quotaProblem[quota!.problem!]}</p>}
    {windows.map((w) => {
      const state = failed ? "unknown" : quotaState(w, now);
      return <div key={`${w.limit_id}/${w.scope}/${w.duration_mins}/${w.window_id}`} className={`acct-quota-window is-${state}`} data-testid="quota-window" data-state={state}>
        <div className="acct-quota-title"><span>{quotaLabel(w)}</span><strong>{state === "unknown" ? `마지막 ${quotaLeft(w)}` : quotaLeft(w)}</strong></div>
        <meter min={0} max={100} value={Number.isFinite(w.used_percent) ? 100 - w.used_percent : 0} aria-label={`${quotaLabel(w)} 남은 양`} />
        <p>{state === "fresh" ? "최신" : state === "stale" ? "오래됨" : "모름 · 재조회 필요"}{w.problem ? ` · ${quotaProblem[w.problem]}` : ""}{w.conflict ? " · 관측 충돌(작은 잔량)" : ""}</p>
        <p>리셋 {quotaTime(w.resets_at_ms)} · 관측 {quotaTime(w.observed_at_ms)}</p>
        <p>{w.machine} · {w.source === "codex_rpc" ? "공식 조회" : "상태줄 관측"}</p>
      </div>;
    })}
    {[300, 10080].filter((n) => !windows.some((w) => w.duration_mins === n && !w.scope)).map((n) => <p key={n} className="op-muted">{n === 300 ? "5시간" : "주간"} 사용량 모름</p>)}
    {quota?.retry_at_ms && quota.retry_at_ms > now ? <p className="op-muted">다시 조회 가능 {quotaTime(quota.retry_at_ms)}</p> : null}
  </section>;
}

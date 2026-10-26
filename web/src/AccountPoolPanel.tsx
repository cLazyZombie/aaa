// 계정별 사용 설정과 대기의 정본은 host다. 웹은 비밀 없는 설정·상태만 읽고 요청한다.
import { useCallback, useEffect, useRef, useState } from "react";
import type { AccountPoolView, AccountStartView, FrontRequest } from "./gen/proto";
import { call, onAccountsChanged } from "./api";
import { SettingsGroup } from "./herdr/SettingsControls";

export function useAccountPool(enabled: boolean) {
  const [pool, setPool] = useState<AccountPoolView | null>(null);
  const [busy, setBusy] = useState(false);
  const [pending, setPending] = useState<{ account: string; enabled: boolean } | null>(null);
  const [error, setError] = useState("");
  const seq = useRef(0);
  const reading = useRef<Promise<void>>(Promise.resolve());
  const changing = useRef(false);
  const load = useCallback(() => {
    const mine = ++seq.current;
    const result = call({ op: "account_pool" }).then((r) => {
      if (mine === seq.current && r.kind === "account_pool") setPool(r.pool);
    });
    reading.current = result;
    return result;
  }, []);
  useEffect(() => {
    if (!enabled) return;
    let active = true;
    const reload = () => { if (active) void load().catch((e) => active && setError(String(e))); };
    reload();
    const off = onAccountsChanged(reload);
    return () => { active = false; ++seq.current; off(); };
  }, [enabled, load]);
  const change = async (request: FrontRequest) => {
    if (changing.current) return;
    changing.current = true;
    setBusy(true); setError("");
    try {
      await call(request);
      // accounts 사건이 저장 뒤 읽기를 앞질렀으면 그 최신 읽기까지 기다린다.
      // 버려진 응답 하나가 끝났다고 pending을 풀면 옛 설정·revision으로 다음 클릭을 받는다.
      let current = load();
      for (;;) {
        await current;
        if (current === reading.current) break;
        current = reading.current;
      }
    }
    catch (e) { setError(String(e)); }
    finally { changing.current = false; setBusy(false); }
  };
  const toggle = (account: string, enabled: boolean) => {
    if (!pool || changing.current) return;
    setPending({ account, enabled });
    void change({ op: "account_pool_member", revision: pool.revision, account,
      member: enabled ? { account, group: account, models: [] } : null }).finally(() => setPending(null));
  };
  return { pool, busy, pending, error, toggle, change };
}
type PoolState = ReturnType<typeof useAccountPool>;

export function AccountAutoAssign({ account, state, canEdit }: { account: string; state: PoolState; canEdit: boolean }) {
  return <label className="acct-auto">
    <input type="checkbox" checked={state.pending?.account === account ? state.pending.enabled : state.pool?.members.some((m) => m.account === account) ?? false}
      disabled={!canEdit || state.busy || !state.pool}
      // root의 capture/bubble 사이에 부모가 갱신되어도 실제 클릭 의도를 먼저 보존한다.
      // 같은 클릭의 onChange는 change의 동기 잠금으로 중복 저장하지 않는다.
      onClickCapture={(e) => { if (!e.currentTarget.disabled) state.toggle(account, e.currentTarget.checked); }}
      onChange={(e) => state.toggle(account, e.target.checked)}
      aria-label={`${account} 자동 배정에 사용`} data-testid="account-auto-assign" />
    자동 배정에 사용
  </label>;
}

export function AccountPoolPanel({ state, canEdit }: { state: PoolState; canEdit: boolean }) {
  const [starts, setStarts] = useState<AccountStartView[]>([]);
  const [startsError, setStartsError] = useState("");
  const seq = useRef(0);
  useEffect(() => {
    let active = true;
    const reload = () => {
      const mine = ++seq.current;
      void call({ op: "account_starts" }).then((r) => {
        if (active && mine === seq.current && r.kind === "account_starts") { setStarts(r.starts); setStartsError(""); }
      }, (e) => { if (active && mine === seq.current) setStartsError(String(e)); });
    };
    reload(); const off = onAccountsChanged(reload);
    return () => { active = false; off(); };
  }, []);
  return <SettingsGroup title="자동 배정" section="account-pool">
    <div className="settings-item acct-pool" data-testid="account-pool">
      <p>같은 harness 안에서 진행 중 작업이 적은 계정부터 골고루 사용합니다. 새 계정은 로그인하면 바로 사용합니다.</p>
      <p className="op-muted">사용량을 판단할 수 없어도 시작하며, 한도에 도달했거나 인증이 끊긴 계정은 건너뜁니다. 사용할 계정이 없는 harness는 기본 계정으로 시작합니다. 진행 중 작업의 계정은 유지합니다.</p>
      {startsError && <p className="op-warn" role="alert">배정 상태를 읽지 못했습니다: {startsError}</p>}
      {state.error && <p className="op-warn" role="alert">{state.error}</p>}
      <ul className="acct-list" data-testid="account-pool-starts">
        {starts.filter((s) => s.state === "queued" || s.state === "failed").map((s) => <li key={`${s.project}/${s.id}`} data-start-id={s.id}>
          <b>{s.title}</b><span>{s.state === "queued" ? "배정 대기" : "시작 실패"} · {s.reason}</span>
          {canEdit && s.state === "queued" && <button className="btn small" disabled={state.busy}
            onClick={() => void state.change({ op: "account_start_cancel", project: s.project, id: s.id })}>배정 대기 취소</button>}
        </li>)}
      </ul>
    </div>
  </SettingsGroup>;
}

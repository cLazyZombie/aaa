// 계정별 사용 설정과 대기의 정본은 host다. 웹은 비밀 없는 설정·상태만 읽고 요청한다.
import { useCallback, useEffect, useRef, useState } from "react";
import type { AccountPoolView, AccountStartView, FrontRequest } from "./gen/proto";
import { call, onAccountsChanged } from "./api";

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
  return { pool, busy, pending, error, toggle };
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

/** 할 일 화면의 계정 배정 대기. 대기 중에만 보이고 끝난 시작 기록은 그리지 않는다(배정 뒤 실패는 host inbox) */
export function AccountWaits({ project, canEdit }: { project: string; canEdit: boolean }) {
  const [starts, setStarts] = useState<AccountStartView[]>([]);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const seq = useRef(0);
  useEffect(() => {
    let active = true;
    const reload = () => {
      const mine = ++seq.current;
      void call({ op: "account_starts", project }).then((r) => {
        if (active && mine === seq.current && r.kind === "account_starts") { setStarts(r.starts); setError(""); }
      }, (e) => { if (active && mine === seq.current) setError(String(e)); });
    };
    reload(); const off = onAccountsChanged(reload);
    return () => { active = false; off(); };
  }, [project]);
  const queued = starts.filter((s) => s.state === "queued");
  if (queued.length === 0 && !error) return null;
  const cancel = async (id: string) => {
    setBusy(true); setError("");
    try { await call({ op: "account_start_cancel", project, id }); }
    catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  };
  return <section className="task-list task-account-waits" aria-label="계정 배정 대기" data-testid="tasks-account-waits">
    <h3 className="task-list-head"><span className="task-list-static">계정 배정 대기 <span className="tasks-count">{queued.length}</span></span></h3>
    {error && <p className="op-warn" role="alert">배정 대기를 읽지 못했습니다: {error}</p>}
    <ul className="task-rows">
      {queued.map((s) => <li key={s.id} className="task-row task-account-wait" data-start-id={s.id}>
        <span className="task-account-wait-text"><span className="task-title">{s.title}</span><span className="muted">{s.reason}</span></span>
        {canEdit && <button type="button" className="btn btn-ghost task-unlinked-action" disabled={busy} onClick={() => void cancel(s.id)}>배정 대기 취소</button>}
      </li>)}
    </ul>
  </section>;
}

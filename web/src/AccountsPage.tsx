// 설정 창 「계정」(관리 GUI W5a, plan §8.346): host 계정을 harness마다 보이고(별칭·메일·plan·최초 인증일·최근 인증된 날·상태·기본),
// 조작 기기는 로그인·다시 로그인·취소·기본 지정/해제를 한다. 로그인은 host의 공식 CLI가 host Mac 브라우저를 열어 끝낸다:
// 이 화면은 인증 주소·토큰을 받지 않고 진행·취소만 보인다(사용자 답 4A). 메일은 host가 조작 기기에만 싣는다(5A).
// 다시 로그인에 다른 메일이 들어오면 host가 옛 인증을 둔 채 기다리고, 여기서 바꿀지 고른다. 바뀌면 host `accounts` 사건으로 다시 읽는다.
import "./ThreadActions.css";
import "./AccountsPage.css";

import { useCallback, useEffect, useRef, useState } from "react";
import type { AccountDefault, AccountLoginView, AccountView, DeviceRole } from "./gen/proto";
import { ApiError, call, me, onAccountsChanged } from "./api";
import { SettingsGroup } from "./herdr/SettingsControls.tsx";
import { canAccounts, day, groupAccounts, HARNESS_LABEL, HARNESSES, healthWord, loginText, validAlias } from "./accountsModel.ts";

const say = (e: unknown) => (e instanceof ApiError ? e.message : e instanceof Error ? e.message : String(e));

type Loaded = { accounts: AccountView[]; defaults: AccountDefault[]; full: boolean };

/** 목록·기본과 보던 로그인을 함께 읽는다. 겹친 읽기는 마지막 것만 반영한다 */
function useAccounts(loginId: string | null) {
  const [data, setData] = useState<Loaded | null>(null);
  const [login, setLogin] = useState<AccountLoginView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const seq = useRef(0);
  const reload = useCallback(() => {
    const mine = ++seq.current;
    (async () => {
      const m = await me().catch(() => null);
      const full = canAccounts(m?.front_api);
      const [list, defaults] = await Promise.all([call({ op: "account_list" }), call({ op: "account_defaults" })]);
      const status = full && loginId ? await call({ op: "account_login_status", id: loginId }) : null;
      if (mine !== seq.current) return;
      setData({
        accounts: list.kind === "accounts" ? list.accounts : [],
        defaults: defaults.kind === "account_defaults" ? defaults.defaults : [],
        full,
      });
      if (status?.kind === "account_login") setLogin(status.login);
      setError(null);
    })().catch((e: unknown) => {
      if (mine === seq.current) setError(say(e));
    });
  }, [loginId]);
  useEffect(() => {
    reload();
    return onAccountsChanged(reload);
  }, [reload]);
  return { data, login, setLogin, error, reload };
}

export function AccountsPage({ role }: { role: DeviceRole }) {
  const [loginId, setLoginId] = useState<string | null>(null);
  const { data, login, setLogin, error } = useAccounts(loginId);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);
  const [alias, setAlias] = useState("");
  const [harness, setHarness] = useState<string>("claude");
  const now = Date.now();
  if (!data) return error ? <p className="op-warn">{error}</p> : <p className="op-muted">읽는 중…</p>;
  const canEdit = role !== "reader" && data.full;
  const run = async (work: () => Promise<unknown>) => {
    setBusy(true);
    setMsg(null);
    try {
      await work();
    } catch (e) {
      setMsg(say(e));
    } finally {
      setBusy(false);
    }
  };
  const start = (name: string, h: string) =>
    run(async () => {
      const r = await call({ op: "account_login", name, harness: h });
      if (r.kind === "account_login") {
        setLogin(r.login);
        setLoginId(r.login.id);
      }
    });
  const active = login && !loginText(login).done;
  const groups = groupAccounts(data.accounts, data.defaults);
  return (
    <div className="op-page" data-testid="settings-accounts">
      {error && (
        <p className="op-warn" role="alert">
          계정을 다시 읽지 못했다(보이는 값이 옛 것일 수 있다): {error}
        </p>
      )}
      {!data.full && <p className="op-muted">이 host는 웹 계정 관리를 모른다. 목록만 보인다(로그인·기본은 host 터미널의 `aaa account`).</p>}
      {role === "reader" && data.full && <p className="op-muted">보기 전용 기기는 별칭·상태만 본다. 메일과 로그인은 조작 기기에서.</p>}
      {login && <LoginCard login={login} busy={busy} run={run} onClose={() => { setLogin(null); setLoginId(null); }} />}
      {msg && (
        <p className="op-warn" role="alert">
          {msg}
        </p>
      )}
      {groups.map((g) => (
        <SettingsGroup key={g.harness} title={HARNESS_LABEL[g.harness]} section={`accounts-${g.harness}`}>
          <div className="settings-item">
            {g.rows.length === 0 ? (
              <p className="op-muted">이 harness의 host 계정이 없다. 없으면 thread는 그 머신의 로컬 로그인을 쓴다.</p>
            ) : (
              <ul className="acct-list" data-testid={`accounts-${g.harness}`}>
                {g.rows.map((a) => {
                  const h = healthWord(a.health, a.expires_ms, now);
                  return (
                    <li key={a.name} data-testid="account-row" data-account={a.name} data-health={a.health ?? ""}>
                      <div className="acct-head">
                        <span className="acct-name">{a.name}</span>
                        {a.isDefault && <span className="acct-badge is-default">기본</span>}
                        {h.text && <span className={`acct-badge is-${h.tone}`} data-testid="account-health">{h.text}</span>}
                      </div>
                      {data.full && (
                        <dl className="acct-facts">
                          {role !== "reader" && (
                            <>
                              <dt>메일</dt>
                              <dd data-testid="account-email">{a.email ?? "모름"}</dd>
                              <dt>plan</dt>
                              <dd>{a.plan ?? "모름"}</dd>
                            </>
                          )}
                          <dt>최초 인증일</dt>
                          <dd data-testid="account-first">{day(a.first_auth_ms)}</dd>
                          <dt>최근 인증된 날</dt>
                          <dd data-testid="account-last">{day(a.last_auth_ms)}</dd>
                          {a.expires_ms !== undefined && (
                            <>
                              <dt>만료</dt>
                              <dd>{day(a.expires_ms)}</dd>
                            </>
                          )}
                        </dl>
                      )}
                      {canEdit && (
                        <div className="op-buttons acct-actions">
                          <button type="button" className="btn small" disabled={busy || !!active} onClick={() => void start(a.name, a.harness)} aria-label={`${a.name} 다시 로그인`}>
                            다시 로그인
                          </button>
                          <button
                            type="button"
                            className="btn small"
                            disabled={busy}
                            aria-label={a.isDefault ? `${a.name} 기본 해제` : `${a.name} 기본으로`}
                            onClick={() => void run(() => call({ op: "account_default", harness: a.harness, account: a.isDefault ? null : a.name }))}
                          >
                            {a.isDefault ? "기본 해제" : "기본으로"}
                          </button>
                        </div>
                      )}
                    </li>
                  );
                })}
              </ul>
            )}
          </div>
        </SettingsGroup>
      ))}
      {canEdit && (
        <SettingsGroup title="새 계정 로그인" note="host의 공식 CLI가 host Mac 브라우저를 엽니다. 그 브라우저에서 인증을 마치면 이 목록에 생깁니다.">
          <form
            className="settings-item acct-new"
            onSubmit={(e) => {
              e.preventDefault();
              void start(alias.trim(), harness);
            }}
          >
            <label className="op-field">
              <span>별칭(영문·숫자·-·_)</span>
              <input value={alias} onChange={(e) => setAlias(e.target.value)} placeholder="work-claude" disabled={busy} data-testid="account-alias" />
            </label>
            <label className="op-field">
              <span>harness</span>
              <select value={harness} onChange={(e) => setHarness(e.target.value)} disabled={busy} data-testid="account-harness">
                {HARNESSES.map((h) => (
                  <option key={h} value={h}>
                    {HARNESS_LABEL[h]}
                  </option>
                ))}
              </select>
            </label>
            <button type="submit" className="btn btn-primary" disabled={busy || !!active || !validAlias(alias.trim())}>
              로그인
            </button>
          </form>
        </SettingsGroup>
      )}
    </div>
  );
}

function LoginCard({ login, busy, run, onClose }: { login: AccountLoginView; busy: boolean; run: (w: () => Promise<unknown>) => Promise<void>; onClose: () => void }) {
  const t = loginText(login);
  return (
    <section className={`acct-login is-${t.tone}`} role="status" aria-live="polite" data-testid="account-login" data-state={login.state}>
      <h3>{t.title}</h3>
      {t.lines.map((l) => (
        <p key={l}>{l}</p>
      ))}
      <div className="op-buttons">
        {login.state === "waiting" && (
          <button type="button" className="btn" disabled={busy} onClick={() => void run(() => call({ op: "account_login_cancel", id: login.id }))}>
            취소
          </button>
        )}
        {login.state === "mismatch" && (
          <>
            <button type="button" className="btn btn-primary" disabled={busy} onClick={() => void run(() => call({ op: "account_login_confirm", id: login.id, switch: true }))}>
              새 메일로 바꾸기
            </button>
            <button type="button" className="btn" disabled={busy} onClick={() => void run(() => call({ op: "account_login_confirm", id: login.id, switch: false }))}>
              옛 인증 그대로 두기
            </button>
          </>
        )}
        {t.done && (
          <button type="button" className="btn" onClick={onClose}>
            닫기
          </button>
        )}
      </div>
    </section>
  );
}

import { ignoredFailure } from "./errorDiagnostic.ts";
// 설정 창 「계정」(관리 GUI W5a, plan §8.351): host 계정을 harness마다 보이고(별칭·메일·plan·최초 인증일·최근 인증된 날·상태·기본),
// 조작 기기는 로그인·다시 로그인·취소·기본 지정/해제를 한다. FRONT_API 1.151부터 접속 기기의 공통 인증 창을 연다.
// 옛 host에서는 기존 host 브라우저 로그인을 유지한다. 메일은 host가 조작 기기에만 싣는다.
// 다시 로그인에 다른 메일이 들어오면 host가 옛 인증을 둔 채 기다리고, 여기서 바꿀지 고른다. 바뀌면 host `accounts` 사건으로 다시 읽는다.
import "./ThreadActions.css";
import "./AccountsPage.css";
import { openAccountLogin } from "./AccountRenewal";
import { AccountQuotaCard, useAccountQuota } from "./AccountQuota";
import { AccountAutoAssign, useAccountPool } from "./AccountPoolPanel";
import { apiAtLeast } from "./automationModel";

import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import type { AccountDefault, AccountLoginView, AccountView, AccountPlanOption, DeviceRole } from "./gen/proto";
import { ApiError, call, me, onAccountsChanged } from "./api";
import { SettingsGroup } from "./herdr/SettingsControls.tsx";
import { canAccounts, day, dayTime, groupAccounts, HARNESS_LABEL, HARNESSES, healthWord, loginText, refreshFailureText, validAlias } from "./accountsModel.ts";

const say = (e: unknown) => (e instanceof ApiError ? e.message : e instanceof Error ? e.message : String(e));

type Loaded = { plans: AccountPlanOption[] | null; emailEdit: boolean; usageLogin: boolean; remote: boolean; accounts: AccountView[]; defaults: AccountDefault[]; full: boolean; quota: boolean; pool: boolean; rename: boolean };

/** 목록·기본과 보던 로그인을 함께 읽는다. 겹친 읽기는 마지막 것만 반영한다 */
function useAccounts(loginId: string | null) {
  const [data, setData] = useState<Loaded | null>(null);
  const [login, setLogin] = useState<AccountLoginView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const seq = useRef(0);
  const reload = useCallback(() => {
    const mine = ++seq.current;
    return (async () => {
      const m = await me().catch(ignoredFailure("AccountsPage.tsx:31", () => null));
      const full = canAccounts(m?.front_api);
      const [list, defaults, plans] = await Promise.all([call({ op: "account_list" }), call({ op: "account_defaults" }),
        apiAtLeast(m?.front_api, 1, 157) ? call({ op: "account_plans" }) : null]);
      // 보던 로그인을 host가 잊었으면(다시 시작·오래된 작업 정리) 목록은 그대로 새로 읽고 카드만 「더 읽지 못함」으로 둔다
      // host가 답한 거부만 「잃음」이다. 전송 실패는 아래 catch가 오류로 알리고 카드를 그대로 둔다
      const status =
        full && loginId
          ? await call({ op: "account_login_status", id: loginId }).catch((e: unknown) => {
              if (e instanceof ApiError) return "lost" as const;
              throw e;
            })
          : null;
      if (mine !== seq.current) return;
      setData({
        plans: plans?.kind === "account_plans" ? plans.plans : null,
        emailEdit: apiAtLeast(m?.front_api, 1, 156),
        usageLogin: apiAtLeast(m?.front_api, 1, 155),
        remote: apiAtLeast(m?.front_api, 1, 151),
        accounts: list.kind === "accounts" ? list.accounts : [],
        defaults: defaults.kind === "account_defaults" ? defaults.defaults : [],
        full,
        quota: apiAtLeast(m?.front_api, 1, 83),
        pool: apiAtLeast(m?.front_api, 1, 129),
        rename: apiAtLeast(m?.front_api, 1, 128),
      });
      if (status === "lost") setLogin((prev) => (prev ? { ...prev, state: "lost" } : prev));
      else if (status?.kind === "account_login") setLogin(status.login);
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
  const { data, login, setLogin, error, reload } = useAccounts(loginId);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);
  const [alias, setAlias] = useState("");
  const [email, setEmail] = useState("");
  const [editingEmail, setEditingEmail] = useState<string | null>(null);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [newName, setNewName] = useState("");
  const renameInput = useRef<HTMLInputElement>(null);
  const [renameError, setRenameError] = useState<string | null>(null);
  const renameErrorElement = useRef<HTMLParagraphElement>(null);
  const [renamed, setRenamed] = useState<{ from: string; to: string } | null>(null);
  const [harness, setHarness] = useState<string>("claude");
  useLayoutEffect(() => {
    if (!renameError) return;
    const element = renameErrorElement.current;
    const list = element?.closest(".acct-list");
    if (!element || !list) return;
    const show = () => element.scrollIntoView({ block: "nearest" });
    show();
    // 목록 삽입 뒤 사용량·풀 응답이 각 행의 높이를 바꿔도 오류를 화면 안에 둔다.
    const observer = new ResizeObserver(show);
    observer.observe(list);
    for (const row of list.children) observer.observe(row);
    return () => observer.disconnect();
  }, [renameError, data?.accounts]);
  useLayoutEffect(() => {
    if (!renaming || busy || role === "reader") return;
    const element = renameInput.current;
    const view = element?.ownerDocument.defaultView;
    if (!element || !view) return;
    // 계정·사용량 갱신이 root capture와 bubble 사이에 commit해도 새 별칭을 잃지 않는다.
    // 대화·직접 답과 같은 경계이며 기존 onChange도 유지한다.
    const input = (event: Event) => { if (event.target === element) setNewName(element.value); };
    view.addEventListener("input", input, true);
    return () => view.removeEventListener("input", input, true);
  }, [renaming, busy, role, data?.accounts]);
  const usage = useAccountQuota(data?.quota ?? false);
  const pool = useAccountPool(data?.pool ?? false);
  const now = Date.now();
  if (!data) return error ? <p className="op-warn">{error}</p> : <p className="op-muted">읽는 중…</p>;
  const canEdit = role !== "reader" && data.full;
  const run = async (work: () => Promise<unknown>, onError = setMsg) => {
    setBusy(true);
    setMsg(null);
    try {
      await work();
    } catch (e) {
      onError(say(e));
    } finally {
      setBusy(false);
    }
  };
  const start = (name: string, h: string, registrationEmail?: string) => {
    if (data.remote) { openAccountLogin(name, h, false, registrationEmail); return; }
    return run(async () => {
      const r = await call({ op: "account_login", name, harness: h });
      if (r.kind === "account_login") {
        setLogin(r.login);
        setLoginId(r.login.id);
      }
    });
  };
  const active = login && !loginText(login).done;
  const groups = groupAccounts(data.accounts, data.defaults);
  return (
    <div className="op-page" data-testid="settings-accounts">
      {error && (
        <p className="op-warn" role="alert">
          계정을 다시 읽지 못했다(보이는 값이 옛 것일 수 있다): {error}
        </p>
      )}
      {!data.full && <p className="op-muted">이 host는 웹 계정 관리를 모른다. 목록만 보인다.</p>}
      {role === "reader" && data.full && <p className="op-muted">보기 전용 기기는 별칭·상태만 본다. 메일과 로그인은 조작 기기에서.</p>}
      {login && <LoginCard login={login} busy={busy} run={run} onClose={() => { setLogin(null); setLoginId(null); }} />}
      {msg && (
        <p className="op-warn" role="alert">
          {msg}
        </p>
      )}
      {data.pool && pool.error && <p className="op-warn" role="alert">{pool.error}</p>}
      {data.quota && <div className="op-buttons"><button className="btn small" type="button" onClick={usage.refresh} disabled={usage.busy}>사용량 다시 읽기</button></div>}
      {groups.map((g) => (
        <SettingsGroup key={g.harness} title={HARNESS_LABEL[g.harness]} section={`accounts-${g.harness}`}>
          <div className="settings-item">
            {g.rows.length === 0 ? (
              <p className="op-muted">이 harness의 host 계정이 없다.</p>
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
                        {a.refresh_failure && !a.refresh_failure.relogin && <span className="acct-badge is-warn" data-testid="account-refresh-badge">갱신 실패</span>}
                      </div>
                      {data.full && (
                        <dl className="acct-facts">
                          {role !== "reader" && (
                            <>
                              <dt>메일</dt>
                              <dd data-testid="account-email">{a.email ?? "모름"}</dd>
                              <dt>요금제</dt>
                              <dd>{data.plans ? <select aria-label={`${a.name} 요금제`} value={a.plan_override ?? ""} disabled={!canEdit || busy}
                                onChange={e => { const plan = e.target.value || null; void run(async () => { await call({ op: "account_plan", name: a.name, plan }); await reload(); usage.refresh(); }); }}>
                                <option value="">자동 감지 · {a.detected_plan ?? "모름"}</option>
                                {data.plans.filter(p => p.harness === a.harness).map(p => <option key={p.id} value={p.id}>{p.label}</option>)}
                              </select> : a.plan ?? "모름"}</dd>
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
                          {a.token_expires_ms !== undefined && (
                            <>
                              <dt>인증 만료 예정</dt>
                              <dd data-testid="account-token-expiry">{dayTime(a.token_expires_ms)}</dd>
                            </>
                          )}
                          {a.refreshed_ms !== undefined && (
                            <>
                              <dt>마지막 갱신</dt>
                              <dd data-testid="account-refreshed">{dayTime(a.refreshed_ms)}</dd>
                            </>
                          )}
                          {a.refresh_failure && (
                            <>
                              <dt>갱신 실패</dt>
                              <dd data-testid="account-refresh-failure">{refreshFailureText(a.refresh_failure)}</dd>
                            </>
                          )}
                        </dl>
                      )}
                      {data.pool && <AccountAutoAssign account={a.name} state={pool} canEdit={canEdit} />}
                      {data.quota && <AccountQuotaCard quota={usage.report?.accounts.find((q) => q.account === a.name && q.harness === a.harness)} now={usage.now} failed={usage.error} />}
                      {canEdit && (
                        <div className="op-buttons acct-actions">
                          {data.usageLogin && a.harness === "claude" && <button type="button" className="btn small" disabled={busy || !!active} aria-label={`${a.name} 사용량 조회 로그인`} onClick={() => openAccountLogin(a.name, a.harness, true, a.email)}>조회 로그인</button>}
                          {data.emailEdit && <button type="button" className="btn small" disabled={busy || !!active} aria-label={`${a.name} 메일 수정`} onClick={() => { setEditingEmail(a.name); setMsg(null); }}>메일 수정</button>}
                          {data.rename && <button type="button" className="btn small" disabled={busy || !!active} aria-label={`${a.name} 이름 바꾸기`} onClick={() => { setRenaming(a.name); setNewName(a.name); setMsg(null); setRenameError(null); setRenamed(null); }}>이름 바꾸기</button>}
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
                      {canEdit && editingEmail === a.name && <AccountEmailForm key={a.name} account={a} busy={busy} cancel={() => setEditingEmail(null)} save={value => run(async () => {
                        await call({ op: "account_email", name: a.name, email: value });
                        setEditingEmail(null);
                        reload();
                      })} />}
                      {renamed?.to === a.name && <p role="status" data-testid="account-renamed">{renamed.from} → {renamed.to}: 이름을 바꿨습니다. 인증은 그대로 사용합니다.</p>}
                      {canEdit && renaming === a.name && (
                        <form className="acct-rename" data-testid="account-rename" onSubmit={(e) => {
                          e.preventDefault();
                          const to = newName.trim();
                          if (!validAlias(to) || to === a.name || data.accounts.some((x) => x.name === to)) return;
                          setRenameError(null);
                          void run(async () => {
                            await call({ op: "account_rename", name: a.name, to });
                            setRenaming(null);
                            setRenamed({ from: a.name, to });
                            reload();
                          }, setRenameError);
                        }}>
                          <label className="op-field">
                            <span>새 이름</span>
                            <input ref={renameInput} aria-label={`${a.name} 새 이름`} value={newName} onChange={(e) => setNewName(e.target.value)} disabled={busy} maxLength={64} autoComplete="off" spellCheck={false} aria-describedby={renameError ? "account-rename-note account-rename-error" : "account-rename-note"} />
                          </label>
                          {(!validAlias(newName.trim()) || data.accounts.some((x) => x.name === newName.trim() && x.name !== a.name)) && (
                            <p id="account-rename-note" className="op-muted">
                              {!validAlias(newName.trim()) ? "영문·숫자·-·_ 1~64자를 입력하세요." : "이미 쓰는 계정 이름입니다."}
                            </p>
                          )}
                          {renameError && <p ref={renameErrorElement} id="account-rename-error" className="op-warn" role="alert">{renameError}</p>}
                          <div className="op-buttons">
                            <button type="submit" className="btn btn-primary" disabled={busy || !validAlias(newName.trim()) || newName.trim() === a.name || data.accounts.some((x) => x.name === newName.trim())}>이름 저장</button>
                            <button type="button" className="btn" disabled={busy} onClick={() => { setRenaming(null); setMsg(null); setRenameError(null); }}>취소</button>
                          </div>
                        </form>
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
        <SettingsGroup title="새 계정 로그인">
          <form
            className="settings-item acct-new"
            onSubmit={(e) => {
              e.preventDefault();
              void start(alias.trim(), harness, data.emailEdit ? email.trim() : undefined);
            }}
          >
            <label className="op-field">
              <span>별칭(영문·숫자·-·_)</span>
              <input value={alias} onChange={(e) => setAlias(e.target.value)} placeholder="work-claude" disabled={busy} data-testid="account-alias" />
            </label>
            {data.emailEdit && <label className="op-field">
              <span>메일</span>
              <input type="email" required maxLength={254} value={email} onChange={e => setEmail(e.target.value)} autoComplete="email" disabled={busy} data-testid="account-registration-email" />
            </label>}
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
            <button type="submit" className="btn btn-primary" disabled={busy || !!active || !validAlias(alias.trim()) || (data.emailEdit && !email.trim())}>
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

function AccountEmailForm({ account, busy, save, cancel }: { account: AccountView; busy: boolean; save: (email: string) => Promise<void>; cancel: () => void }) {
  const [email, setEmail] = useState(account.email ?? "");
  return <form className="acct-rename" data-testid="account-email-edit" onSubmit={e => { e.preventDefault(); void save(email.trim()); }}>
    <label className="op-field"><span>메일</span><input aria-label={`${account.name} 메일`} type="email" required maxLength={254} value={email} onChange={e => setEmail(e.target.value)} disabled={busy} autoComplete="email" /></label>
    <div className="op-buttons"><button type="submit" className="btn btn-primary" disabled={busy || !email.trim()}>메일 저장</button><button type="button" className="btn" disabled={busy} onClick={cancel}>취소</button></div>
  </form>;
}

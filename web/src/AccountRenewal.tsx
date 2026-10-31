// 메뉴 하단과 설정이 같은 원격 인증 창을 연다. 인증 자료는 이 컴포넌트 메모리에만 둔다.
import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { ChevronRight, KeyRound } from "lucide-react";
import type { AccountLoginPrompt, AccountLoginView, AccountView } from "./gen/proto";
import { ApiError, call, me, onAccountsChanged, onSignedOut } from "./api";
import { apiAtLeast } from "./automationModel";
import { HARNESS_LABEL, renewalAccounts } from "./accountsModel";
import { AgentMark } from "./herdr/AgentMark.tsx";
import { nativeModalOver, useFocusTrap } from "./herdr/useFocusTrap";
import { usePhoneSheetBack } from "./herdr/sheetHistory";
import "./AccountRenewal.css";

const eventName = "aaa:account-renewal";
type Target = { name: string; harness: string };
export function openAccountLogin(name: string, harness: string) {
  window.dispatchEvent(new CustomEvent<Target>(eventName, { detail: { name, harness } }));
}
const message = (error: unknown) => error instanceof ApiError || error instanceof Error ? error.message : "인증 요청에 실패했습니다";
const active = (login: AccountLoginView | null) => login?.state === "waiting" || login?.state === "mismatch";

export function AccountRenewal() {
  const [accounts, setAccounts] = useState<AccountView[]>([]);
  const [enabled, setEnabled] = useState(false);
  const [login, setLogin] = useState<AccountLoginView | null>(null);
  const [prompt, setPrompt] = useState<AccountLoginPrompt | null>(null);
  const [target, setTarget] = useState<Target | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const id = useRef<string | null>(null);
  const sequence = useRef(0);
  const starting = useRef(false);
  const closed = useRef(false);
  const clear = useCallback(() => { ++sequence.current; id.current = null; setPrompt(null); setLogin(null); setTarget(null); setError(null); }, []);
  const reload = useCallback(async () => {
    const seq = ++sequence.current;
    try {
      const who = await me();
      const can = who !== null && who.role !== "reader" && apiAtLeast(who.front_api, 1, 151);
      if (seq !== sequence.current || closed.current) return;
      setEnabled(can);
      if (!can) { setAccounts([]); clear(); return; }
      const [list, flow] = await Promise.all([
        call({ op: "account_list" }),
        call({ op: "account_login_interaction", id: id.current }),
      ]);
      if (seq !== sequence.current || closed.current) return;
      if (list.kind === "accounts") setAccounts(renewalAccounts(list.accounts));
      if (flow.kind === "account_login_interaction" && flow.login) {
        id.current = flow.login.id;
        setTarget({ name: flow.login.account, harness: flow.login.harness });
        setLogin(flow.login); setPrompt(flow.prompt); setError(null);
      }
    } catch (e) { if (seq === sequence.current && !closed.current) { setPrompt(null); setError(message(e)); if (e instanceof ApiError && ["bad_request", "forbidden"].includes(e.body.code)) { id.current = null; setLogin(prev => prev ? { ...prev, state: "lost" } : null); } } }
  }, [clear]);
  useEffect(() => {
    closed.current = false;
    void reload();
    const off = onAccountsChanged(() => void reload());
    const signedOut = onSignedOut(() => { closed.current = true; setAccounts([]); setEnabled(false); clear(); });
    const focus = () => { if (!document.hidden) void reload(); };
    window.addEventListener("focus", focus);
    document.addEventListener("visibilitychange", focus);
    return () => { closed.current = true; ++sequence.current; off(); signedOut(); window.removeEventListener("focus", focus); document.removeEventListener("visibilitychange", focus); };
  }, [clear, reload]);
  const start = useCallback(async (next: Target) => {
    if (!enabled || starting.current) return;
    if (active(login)) { setTarget({ name: login!.account, harness: login!.harness }); setError("진행 중인 인증을 먼저 마치거나 취소하세요"); return; }
    setLogin(null); id.current = null;
    starting.current = true; ++sequence.current; setBusy(true); setError(null); setTarget(next); setPrompt(null);
    try {
      const response = await call({ op: "account_login_remote", ...next });
      if (closed.current) return;
      if (response.kind === "account_login") { id.current = response.login.id; setLogin(response.login); await reload(); }
    } catch (e) { if (!closed.current) setError(message(e)); }
    finally { starting.current = false; setBusy(false); }
  }, [enabled, reload, login]);
  useEffect(() => {
    const listener = (e: Event) => { const next = (e as CustomEvent<Target>).detail; if (next) void start(next); };
    window.addEventListener(eventName, listener);
    return () => window.removeEventListener(eventName, listener);
  }, [start]);
  const run = async (action: () => Promise<unknown>) => {
    if (busy) return;
    setBusy(true); setError(null);
    try { await action(); await reload(); }
    catch (e) { setError(message(e)); }
    finally { setBusy(false); }
  };
  const close = () => {
    if (busy) return;
    if (active(login) && id.current) {
      void run(async () => { await call({ op: "account_login_cancel", id: id.current! }); clear(); });
    } else clear();
  };
  if (!enabled) return null;
  return <>
    {accounts.length > 0 && <section className="account-renewal" aria-label="인증 갱신" data-testid="account-renewal">
      <div className="account-renewal-heading"><span><KeyRound aria-hidden="true" />인증 갱신</span><span>{accounts.length}</span></div>
      {accounts.map(a => <button className="account-renewal-row" key={a.name} type="button" aria-label={`${a.name} 인증 갱신`} onClick={() => void start(a)} disabled={busy}>
        <AgentMark agent={a.harness} size={20} /><span className="account-renewal-name"><strong>{HARNESS_LABEL[a.harness] ?? a.harness}</strong><span>{a.name}</span></span>
        <span className={`account-renewal-status ${a.health === "expiring" ? "is-soon" : ""}`}>{a.health === "expiring" ? "곧 만료" : "갱신 필요"}</span><ChevronRight aria-hidden="true" />
      </button>)}
    </section>}
    {target && <LoginDialog target={target} login={login} prompt={prompt} busy={busy} error={error} close={close}
      submit={code => run(() => call({ op: "account_login_code", id: id.current!, code }))}
      confirm={choice => run(() => call({ op: "account_login_confirm", id: id.current!, switch: choice }))} />}
  </>;
}

function LoginDialog({ target, login, prompt, busy, error, close, submit, confirm }: {
  target: Target; login: AccountLoginView | null; prompt: AccountLoginPrompt | null; busy: boolean; error: string | null;
  close: () => void; submit: (code: string) => Promise<void>; confirm: (choice: boolean) => Promise<void>;
}) {
  const [code, setCode] = useState("");
  const [copied, setCopied] = useState(false);
  const [copyError, setCopyError] = useState(false);
  const closeButton = useRef<HTMLButtonElement>(null);
  const surface = useFocusTrap<HTMLDivElement>(true, { initialFocus: closeButton });
  usePhoneSheetBack(() => { if (busy) return false; close(); });
  useEffect(() => { setCode(""); setCopied(false); }, [login?.id, prompt?.kind]);
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || nativeModalOver(surface.current)) return;
      e.stopPropagation(); e.preventDefault(); if (!busy) close();
    };
    window.addEventListener("keydown", key, true);
    return () => window.removeEventListener("keydown", key, true);
  }, [busy, close, surface]);
  const waiting = login?.state === "waiting";
  const mismatch = login?.state === "mismatch";
  let url: string | null = null;
  if (prompt && waiting) {
    try { const u = new URL(prompt.url); if (u.protocol === "https:" && !u.username && !u.password && !u.port && (target.harness === "codex" ? u.hostname === "auth.openai.com" && u.pathname === "/codex/device" : ["claude.ai", "claude.com"].includes(u.hostname) && ["/oauth/authorize", "/cai/oauth/authorize"].includes(u.pathname))) url = u.href; } catch { /* 호스트의 자료도 링크로 쓰기 전에 검사한다. */ }
  }
  const stateText: Record<string, string> = { succeeded: "로그인 완료", failed: "로그인하지 못했습니다", cancelled: "인증을 취소했습니다", kept: "기존 인증을 유지했습니다" };
  return createPortal(<div className="modal-scrim"><div className="modal account-login-dialog" role="dialog" aria-modal="true" aria-labelledby="account-login-title" tabIndex={-1} ref={surface} data-testid="account-login-dialog">
    <header className="modal-header"><h2 className="modal-title" id="account-login-title">{HARNESS_LABEL[target.harness] ?? target.harness} 다시 로그인</h2></header>
    <div className="modal-body"><p className="account-login-alias">{target.name}</p>
      {waiting && login.expect_email && <p className="account-login-identity">{login.expect_email}</p>}
      {waiting && url && <a className="btn primary" href={url} target="_blank" rel="noopener noreferrer" referrerPolicy="no-referrer">인증 열기</a>}
      {waiting && url && prompt?.kind === "device_code" && <><div className="account-login-code">{prompt.user_code}</div><button type="button" className="btn" onClick={() => { void navigator.clipboard.writeText(prompt.user_code ?? "").then(() => { setCopied(true); setCopyError(false); }, () => setCopyError(true)); }}>{copied ? "복사됨" : "코드 복사"}</button>{copyError && <p role="alert">코드를 복사하지 못했습니다</p>}</>}
      {waiting && url && prompt?.kind === "paste_code" && <form className="account-login-input" onSubmit={e => { e.preventDefault(); const value = code.trim(); setCode(""); void submit(value); }}><label htmlFor="account-auth-code">인증 코드</label><input id="account-auth-code" type="password" autoComplete="off" spellCheck={false} autoCapitalize="none" value={code} onChange={e => setCode(e.target.value)} placeholder="인증 코드 붙여넣기" disabled={busy} /><button className="btn primary" type="submit" disabled={busy || !code.trim()}>확인</button></form>}
      {waiting && <p role="status">{prompt ? "인증 대기 중" : "확인 중"}</p>}
      {!login && !error && <p role="status">인증 준비 중</p>}
      {mismatch && <><p role="status">{login.email ? "다른 계정입니다" : "계정 확인 불가"}</p><p>{login.expect_email ?? "기존 계정 모름"} → {login.email ?? "새 계정 모름"}</p><div className="op-buttons"><button className="btn" disabled={busy} onClick={() => void confirm(false)}>유지</button><button className="btn primary" disabled={busy} onClick={() => void confirm(true)}>교체</button></div></>}
      {login && !active(login) && <p role="status">{login.error ?? stateText[login.state] ?? "인증이 종료됐습니다"}</p>}
      {error && <p role="alert" className="op-warn">{error}</p>}
    </div><footer className="modal-footer"><button ref={closeButton} type="button" className="btn" disabled={busy} onClick={close}>{active(login) ? "취소" : "닫기"}</button></footer>
  </div></div>, document.body);
}

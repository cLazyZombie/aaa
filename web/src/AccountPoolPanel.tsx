// 풀의 정본·권한·선택·대기는 host가 소유한다. 웹은 별칭과 비밀 없는 결과만 편집·표시한다.
import { useCallback, useEffect, useRef, useState } from "react";
import type { AccountPoolView, AccountStartView, AccountView, FrontRequest, ProjectView } from "./gen/proto";
import { call, onAccountsChanged } from "./api";
import { SettingsGroup } from "./herdr/SettingsControls";

export function AccountPoolPanel({ accounts, canEdit }: { accounts: AccountView[]; canEdit: boolean }) {
  const [pool, setPool] = useState<AccountPoolView | null>(null);
  const [projects, setProjects] = useState<ProjectView[]>([]);
  const [project, setProject] = useState("");
  const [starts, setStarts] = useState<AccountStartView[]>([]);
  const [account, setAccount] = useState("");
  const [group, setGroup] = useState("");
  const [models, setModels] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [projectChange, setProjectChange] = useState<{ project: string; enabled: boolean } | null>(null);
  const loadSeq = useRef(0);
  const startsSeq = useRef(0);
  const load = useCallback(async () => {
    const mine = ++loadSeq.current;
    const [p, ps] = await Promise.all([call({ op: "account_pool" }), call({ op: "projects", all: false })]);
    if (mine !== loadSeq.current) return;
    if (p.kind === "account_pool") setPool(p.pool);
    if (ps.kind === "projects") {
      setProjects(ps.projects);
      setProject((old) => old || ps.projects[0]?.name || "");
    }
  }, []);
  useEffect(() => {
    let active = true;
    const reload = () => { if (active) void load().catch((e) => active && setError(String(e))); };
    reload(); const off = onAccountsChanged(reload);
    return () => { active = false; off(); };
  }, [load]);
  useEffect(() => {
    if (!project) { setStarts([]); return; }
    let active = true;
    const reload = () => { const mine = ++startsSeq.current; void call({ op: "account_starts", project }).then((r) => { if (active && mine === startsSeq.current && r.kind === "account_starts") setStarts(r.starts); }, (e) => active && setError(String(e))); };
    reload(); const off = onAccountsChanged(reload);
    return () => { active = false; off(); };
  }, [project]);
  const change = async (request: FrontRequest) => {
    setBusy(true); setError("");
    try { await call(request); await load(); }
    catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  };
  const pick = (name: string) => {
    setAccount(name);
    const member = pool?.members.find((m) => m.account === name);
    setGroup(member?.group ?? name); setModels(member?.models.join(", ") ?? "");
  };
  return <SettingsGroup title="자동 배정" section="account-pool">
    <div className="settings-item acct-pool" data-testid="account-pool">
      <p>host 공통 허용 풀에서 새 thread의 계정을 고릅니다. 프로젝트 참여를 켜야 적용됩니다. 새 로그인 계정은 자동으로 추가하지 않습니다.</p>
      <p className="op-muted">최신 잔량 후보가 없으면 미관측 계정의 첫 실행을 허용합니다. 오래된 관측·조회 실패는 기다립니다. 같은 계정의 여러 작업은 허용하며 진행 중 계정은 바꾸지 않습니다.</p>
      {error && <p className="op-warn" role="alert">{error}</p>}
      {!pool ? <p>풀을 읽는 중…</p> : <>
        <ul className="acct-list" data-testid="account-pool-members">
          {pool.members.map((m) => <li key={m.account} data-account={m.account}>
            <span><b>{m.account}</b> · 한도 그룹 {m.group} · 모델 {m.models.join(", ") || "전체"}</span>
            {canEdit && <div className="op-buttons">
              <button className="btn small" disabled={busy} onClick={() => pick(m.account)} aria-label={`${m.account} 풀 고치기`}>고치기</button>
              <button className="btn small" disabled={busy} onClick={() => void change({ op: "account_pool_member", revision: pool.revision, account: m.account, member: null })} aria-label={`${m.account} 풀에서 빼기`}>풀에서 빼기</button>
            </div>}
          </li>)}
          {!pool.members.length && <li>허용한 계정 없음</li>}
        </ul>
        {canEdit && <form className="acct-pool-form" onSubmit={(e) => {
          e.preventDefault();
          if (account && group.trim()) void change({ op: "account_pool_member", revision: pool.revision, account, member: { account, group: group.trim(), models: models.split(/[,\s]+/).filter(Boolean) } });
        }}>
          <label>풀의 계정<select value={account} onChange={(e) => pick(e.target.value)} disabled={busy} data-testid="pool-account"><option value="">고르기…</option>{accounts.map((a) => <option key={a.name} value={a.name}>{a.name} ({a.harness})</option>)}</select></label>
          <label>공유 한도 그룹<input value={group} onChange={(e) => setGroup(e.target.value)} disabled={busy} data-testid="pool-group" /></label>
          <label>허용 모델<input value={models} onChange={(e) => setModels(e.target.value)} placeholder="비우면 전체, 여러 개는 쉼표로" disabled={busy} data-testid="pool-models" /></label>
          <p className="op-muted">실제 한도가 같은 별칭끼리는 같은 그룹을 지정하세요. 서로 다른 harness는 같은 그룹으로 묶지 않습니다.</p>
          <button className="btn" type="submit" disabled={busy || !account || !group.trim()} data-testid="pool-save">풀에 저장</button>
        </form>}
        {!!projects.length && <div className="acct-pool-project">
          <label>프로젝트<select value={project} onChange={(e) => setProject(e.target.value)} data-testid="pool-project">{projects.map((p) => <option key={p.name} value={p.name}>{p.name}</option>)}</select></label>
          <label><input type="checkbox" checked={projectChange?.project === project ? projectChange.enabled : pool.projects.includes(project)} disabled={!canEdit || busy || !project} onChange={(e) => {
            const enabled = e.target.checked;
            setProjectChange({ project, enabled });
            void change({ op: "account_pool_project", revision: pool.revision, project, enabled }).finally(() => setProjectChange(null));
          }} data-testid="pool-project-enabled" /> 이 프로젝트에서 공통 풀 사용</label>
        </div>}
        <ul className="acct-list" data-testid="account-pool-starts">
          {starts.filter((s) => s.state === "queued" || s.state === "failed").map((s) => <li key={s.id} data-start-id={s.id}>
            <b>{s.title}</b><span>{s.state === "queued" ? "배정 대기" : "시작 실패"} · {s.reason}</span>
            {canEdit && s.state === "queued" && <button className="btn small" disabled={busy} onClick={() => void change({ op: "account_start_cancel", project: s.project, id: s.id })}>배정 대기 취소</button>}
          </li>)}
        </ul>
      </>}
    </div>
  </SettingsGroup>;
}

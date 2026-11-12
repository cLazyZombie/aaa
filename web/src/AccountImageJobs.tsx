import { useCallback, useEffect, useRef, useState } from "react";
import type { AccountImageJob } from "./gen/proto";
import { call, onAccountsChanged } from "./api";
import { dayTime } from "./accountsModel";

/** 사람만 볼 수 있는 계정별 이미지 차단 항목. 프롬프트·인증은 받지 않는다. */
export function AccountImageJobs({ name, changed }: { name: string; changed: () => void }) {
  const [jobs, setJobs] = useState<AccountImageJob[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [confirming, setConfirming] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const sequence = useRef(0);
  const reload = useCallback(async () => {
    const mine = ++sequence.current;
    try {
      const reply = await call({ op: "account_image_jobs", name });
      if (mine !== sequence.current) return;
      if (reply.kind === "account_image_jobs") { setJobs(reply.jobs); setError(null); }
    } catch (e) { if (mine === sequence.current) setError(e instanceof Error ? e.message : String(e)); }
  }, [name]);
  useEffect(() => {
    void reload();
    const off = onAccountsChanged(reload);
    return () => { ++sequence.current; off(); };
  }, [reload]);
  if (jobs?.length === 0 && !error) return null;
  return <div className="acct-image-jobs" data-testid="account-image-jobs">
    <strong>이름 변경을 막는 이미지 작업{jobs ? ` ${jobs.length}건` : ""}</strong>
    {!jobs && <p className="op-muted">읽는 중…</p>}
    {error && <p className="op-warn" role="alert">{error}</p>}
    <ul>{jobs?.map(job => <li key={job.id} data-image-job={job.id}>
      <div className="acct-image-job-head"><span>{job.owner} · {job.state === "running" ? "생성 중" : "결과 불명"}</span>
        {job.state === "unknown" && confirming !== job.id && <button type="button" className="btn small" disabled={busy} onClick={() => setConfirming(job.id)}>정리</button>}
      </div>
      <div className="op-muted">{job.project} · {dayTime(job.selected_at_ms)}</div>
      <code>{job.id}</code>
      <p>{job.state === "running" ? "완료 뒤 이름 변경 가능" : job.reason}</p>
      {confirming === job.id && <div className="acct-image-confirm">
        <p>결과 불명 기록을 정리할까요? 같은 요청은 다시 실행하지 않습니다.</p>
        <div className="op-buttons">
          <button type="button" className="btn" disabled={busy} onClick={() => {
            setBusy(true); setError(null);
            void call({ op: "account_image_dismiss", name, id: job.id }).then(async () => {
              setConfirming(null); await reload(); changed();
            }).catch((e: unknown) => setError(e instanceof Error ? e.message : String(e))).finally(() => setBusy(false));
          }}>확인하고 정리</button>
          <button type="button" className="btn" disabled={busy} onClick={() => setConfirming(null)}>취소</button>
        </div>
      </div>}
    </li>)}</ul>
  </div>;
}

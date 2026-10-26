import { useEffect, useState } from "react";
import type { ThreadFollowup, ThreadView } from "./gen/proto";
import { call, me } from "./api";
import { apiAtLeast } from "./automationModel.ts";
import { Markdown } from "./Markdown";

/** 완료 보고의 Next는 보고 교체나 thread 종료와 별개로 처리한다. 읽기 기기도 같은 원장을 본다. */
export function ThreadFollowups({ t, canAct }: { t: ThreadView; canAct: boolean }) {
  const [supported, setSupported] = useState(false);
  useEffect(() => {
    let live = true;
    void me().then(m => { if (live) setSupported(apiAtLeast(m?.front_api, 1, 131)); }, () => {});
    return () => { live = false; };
  }, []);
  const [saved, setSaved] = useState<ThreadView | null>(null);
  // 서버 사건으로 갱신한 props를 우선하고, 요청 직후에는 같은 revision의 응답을 보인다.
  const entries = saved && saved.updated_at_ms > t.updated_at_ms ? saved.followups : t.followups;
  if (!entries?.length) return null;
  const left = entries.filter(f => f.handled_ms === null).length;
  return <section className="card thread-followups" data-testid="thread-followups">
    <h2>{left ? `후속 남음 · ${left}개` : "후속 처리 기록"}</h2>
    <p>후속 할 일을 만든 뒤 ID를 연결하거나, 버림·정리 완료 사유를 기록합니다.</p>
    <ul>{entries.map(f => <FollowupLine key={f.id} f={f} t={t} canAct={canAct && supported} onSaved={setSaved} />)}</ul>
  </section>;
}

function FollowupLine({ f, t, canAct, onSaved }: { f: ThreadFollowup; t: ThreadView; canAct: boolean; onSaved: (t: ThreadView) => void }) {
  const [task, setTask] = useState("");
  const [reason, setReason] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  async function handle(link: boolean) {
    setBusy(true); setError(null);
    try {
      const r = await call({ op: "thread_followup", project: t.project, thread: t.id, id: f.id, task: link ? task.trim() : null, reason: link ? null : reason.trim() });
      if (r.kind === "thread") onSaved(r);
    } catch (e) { setError(e instanceof Error ? e.message : String(e)); }
    finally { setBusy(false); }
  }
  return <li data-testid="followup-line" data-pending={f.handled_ms === null}>
    <Markdown>{f.line}</Markdown>
    {f.handled_ms !== null ? <div className="muted" data-testid="followup-receipt">
      {f.task ? <Markdown>{`후속 할 일: ${f.task}`}</Markdown> : `사유: ${f.reason}`}
      <span>{f.actor} · {new Date(f.handled_ms).toLocaleString()}</span>
    </div> : canAct && <div className="followup-actions">
      <label>후속 할 일 ID<input aria-label="후속 할 일 ID" placeholder="d-0001" value={task} onChange={e => setTask(e.target.value)} disabled={busy} /></label>
      <button type="button" className="btn small" disabled={busy || !task.trim()} onClick={() => void handle(true)}>할 일 연결</button>
      <label>버림·정리 완료 사유<input aria-label="버림·정리 완료 사유" value={reason} onChange={e => setReason(e.target.value)} disabled={busy} maxLength={2000} /></label>
      <button type="button" className="btn small" disabled={busy || !reason.trim()} onClick={() => void handle(false)}>사유 기록</button>
      {error && <p role="alert" className="err">{error}</p>}
    </div>}
  </li>;
}

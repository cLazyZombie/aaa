// 예약의 정본은 host다. 이 컴포넌트에는 scheduler나 별도 저장을 두지 않는다.
import { useEffect, useState } from "react";
import { call, onTasksChanged } from "./api";
import type { ArchivedTask, QueueOp, QueueState, TaskBoard, TaskView, ThreadStartPreview } from "./gen/proto";
import { Markdown } from "./Markdown";
const words: Record<QueueState, string> = { queued: "예약", check: "확인 필요", starting: "시작 중", running: "진행", finished: "resolve 완료", cancelled: "예약 취소" };

export function QueueControls({ task, board, canEdit }: { task: TaskView; board: TaskBoard; canEdit: boolean }) {
  const [preview, setPreview] = useState<ThreadStartPreview | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const q = board.queue?.find((q) => q.task_id === task.id);
  const pending = q && ["queued", "check"].includes(q.state);
  async function action(action: QueueOp) {
    if (busy) return;
    setBusy(true); setError("");
    try { await call({ op: "task_queue", project: board.project, action }); setPreview(null); }
    catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  }
  async function inspect() {
    setBusy(true); setError("");
    try {
      const r = await call({ op: "thread_start_preview", project: board.project, from_task: task.title, task_block: task.block_sha256 });
      if (r.kind === "thread_start_preview") setPreview(r);
    } catch (e) { setError(String(e)); } finally { setBusy(false); }
  }
  return <section className="task-queue-controls" data-testid="task-queue-controls" data-state={q?.state ?? "none"}>
    {q && <p data-testid="task-queue-status">{words[q.state]} {pending ? q.position : ""} · {q.reason} · 모델 {q.profile} {q.model} · effort {q.effort || "기본"} · 머신 {q.machine ?? "자동"}{q.thread && ` · ${q.thread}`}</p>}
    {canEdit && (!q || ["queued", "check", "cancelled", "finished"].includes(q.state)) && <button className="btn" type="button" disabled={busy} onClick={() => void inspect()} data-testid="task-reserve-preview">{pending ? "예약 내용 갱신" : "자리 나면 시작 예약"}</button>}
    {canEdit && pending && <div className="task-queue-actions">
      <button className="btn" type="button" disabled={busy} onClick={() => void action({op:"cancel",task_id:q.task_id})}>예약 취소</button>
      <button className="btn" type="button" disabled={busy} onClick={() => void action({op:"move",task_id:q.task_id,position:Math.max(1,q.position-1)})}>예약 위로</button>
      <button className="btn" type="button" disabled={busy} onClick={() => void action({op:"move",task_id:q.task_id,position:q.position+1})}>예약 아래로</button>
      <button className="btn" type="button" disabled={busy} onClick={() => void action({op:"move",task_id:q.task_id,position:0})}>예약 맨 앞으로</button>
    </div>}
    {preview && <div data-testid="task-reserve-confirm">
      <p>모델 {preview.profile} {preview.model} · effort {preview.effort || "기본"} · 머신 {preview.machine ?? "자동 배정"} · 저장소 {task.repo ?? "정하지 않음"}</p>
      <Markdown>{preview.task}</Markdown>
      {(!task.repo || task.notes.length===0) && <p role="alert">먼저 설명·완료 범위 메모와 저장소 또는 작업 폴더를 저장한다.</p>}
      <button className="btn btn-primary" type="button" disabled={busy || !task.repo || task.notes.length===0} onClick={() => void action({op:"reserve",task:task.title,block:preview.task_block,profile:preview.profile,profile_revision:preview.profile_revision})} data-testid="task-reserve-go">이 내용으로 예약</button>
      <button className="btn" type="button" disabled={busy} onClick={() => setPreview(null)}>닫기</button>
    </div>}
    {error && <p role="alert">{error}</p>}
  </section>;
}

export function TaskQueuePanel({ board, canEdit, onThread }: { board: TaskBoard; canEdit: boolean; onThread: (id:string)=>void }) {
  const [show, setShow] = useState(false);
  const [entries, setEntries] = useState<ArchivedTask[]>([]);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (!show) return;
    let live=true;
    const load=()=> { void call({op:"task_archive",project:board.project,restore:null}).then(r=> {if(live&&r.kind==="task_archive")setEntries(r.entries);},e=> {if(live)setError(String(e));}); };
    load(); const off=onTasksChanged(load); return ()=> {live=false;off();};
  },[show,board.project]);
  async function restore(id:string) {
    setBusy(true);setError("");
    try {const r=await call({op:"task_archive",project:board.project,restore:id});if(r.kind==="task_archive")setEntries(r.entries);}
    catch(e){setError(String(e));}finally{setBusy(false);}
  }
  return <div className="task-queue-panel" data-testid="task-queue-panel">
    <details><summary>예약 {board.queue?.filter(q=>["queued","check"].includes(q.state)).length ?? 0}</summary>
      {board.queue?.filter(q=>q.state!=="cancelled"&&q.state!=="finished").map(q=><p key={q.task_id} data-testid={`queue-${q.task_id}`} data-state={q.state}>{q.position} · {q.task.title} · {words[q.state]} · {q.reason} · {q.profile} · {q.machine ?? "자동"} {q.thread&&<button className="btn" type="button" onClick={()=>onThread(q.thread!)}>{q.thread}</button>}</p>)}
    </details>
    <button className="btn" type="button" aria-expanded={show} onClick={()=>setShow(!show)} data-testid="task-archive-open">완료 보관</button>
    {show&&<section aria-label="완료 보관" data-testid="task-archive-list">
      {entries.length===0&&<p>완료 보관이 비어 있다</p>}
      {entries.map(a=><article key={a.task_id} data-testid={`archive-${a.task_id}`}><h3>{a.task.title}</h3><p>{a.committed ? a.reason : `보관 확정 확인 필요 · ${a.reason}`} · {new Date(a.completed_ms).toLocaleString()}</p><Markdown>{a.body}</Markdown>{a.task.thread&&<button className="btn" type="button" onClick={()=>onThread(a.task.thread!)}>{a.task.thread} 결과</button>}{canEdit&&<button className="btn" type="button" disabled={busy} onClick={()=>void restore(a.task_id)}>활성 목록으로 복원</button>}</article>)}
    </section>}
    {error&&<p role="alert">{error}</p>}
  </div>;
}

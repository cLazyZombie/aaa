import { ThreadStageBadge } from "./ThreadStageBadge";
// 예약의 정본은 host다. 이 컴포넌트에는 scheduler나 별도 저장을 두지 않는다. 할 일 한 목록(plan §8.393): 예약 행은 숫자·까닭 글 없이
// 보라 표지만 보이고, 기다리는 특별한 까닭(머신 연결 대기·확인 필요)과 풀면 돌아갈 자리는 여기 상세에만 보인다.
import { useEffect, useRef, useState } from "react";
import { call } from "./api";
import type { QueueOp, QueueState, TaskBoard, TaskView, ThreadStartPreview } from "./gen/proto";
import { Markdown } from "./Markdown";
import { queueWhy, unreserveHome, type MoveDir } from "./tasksModel";
const words: Record<QueueState, string> = { queued: "예약됨 · 자리 나면 시작", check: "예약 · 확인 필요", starting: "시작 중", running: "진행", finished: "resolve 완료", cancelled: "예약 풀림" };

/** 목록이 연 예약(끌어 놓기·⋯ 「자리 나면 시작 예약」): 내용 확인을 바로 열고, 예약한 뒤 그 차례(host 순번, 없으면 맨 뒤)로 옮긴다 */
export type ReserveRequest = { seq: number; position: number | null };

export function QueueControls({ task, board, canEdit, afterApi, request, onStep, onUnreserved }: {
  task: TaskView;
  board: TaskBoard;
  canEdit: boolean;
  /** 예약 확인에서 앞 할 일을 고른다(host가 그 할 일이 완료 보관될 때까지 기다린다) */
  afterApi?: boolean;
  request?: ReserveRequest | null;
  /** 예약 위로·아래로·맨 앞으로(목록이 거르기에 맞춰 순번을 정한다). 없으면 그 일을 하지 않는다 */
  onStep?: (dir: MoveDir) => void;
  /** 예약을 풀었다(목록이 원래 자리에서 잠깐 알린다) */
  onUnreserved?: (t: TaskView) => void;
}) {
  const [preview, setPreview] = useState<ThreadStartPreview | null>(null);
  const [position, setPosition] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const q = board.queue?.find((q) => q.task_id === task.id);
  const pending = q && ["queued", "check"].includes(q.state);
  const why = q && pending ? queueWhy(q) : null;
  // 앞 할 일(ULID, 빈 글은 없음). 예약 내용 갱신도 지금 앞 할 일을 그대로 다시 보낸다
  const [after, setAfter] = useState("");
  const afterWait = q?.state === "queued" && !!q.after && q.reason.startsWith("앞 할 일");
  const choices = board.tasks.filter((t) => t.id && t.id !== task.id && !t.done);
  async function action(action: QueueOp) {
    if (busy) return;
    setBusy(true); setError("");
    try {
      await call({ op: "task_queue", project: board.project, action });
      setPreview(null);
      if (action.op === "cancel") onUnreserved?.(task);
    }
    catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  }
  async function inspect() {
    setBusy(true); setError("");
    try {
      const r = await call({ op: "thread_start_preview", project: board.project, from_task: task.short_id ?? task.title, task_block: task.block_sha256 });
      if (r.kind === "thread_start_preview") { setAfter(pending ? (q?.after ?? "") : ""); setPreview(r); }
    } catch (e) { setError(String(e)); } finally { setBusy(false); }
  }
  async function reserve(p: ThreadStartPreview) {
    if (busy) return;
    setBusy(true); setError("");
    try {
      const r = await call({ op: "task_queue", project: board.project, action: { op: "reserve", task: task.short_id ?? task.title, block: p.task_block, profile: p.profile, profile_revision: p.profile_revision, ...(afterApi && after ? { after } : {}) } });
      // 예약은 맨 뒤에 붙는다. 끌어 놓은 자리가 있으면 그 차례로 옮긴다
      const mine = r.kind === "task_queue" ? r.entries.find((e) => (task.id ? e.task_id === task.id : e.task.title === task.title) && e.state === "queued") : undefined;
      if (position !== null && mine) await call({ op: "task_queue", project: board.project, action: { op: "move", task_id: mine.task_id, position } });
      setPreview(null);
      setPosition(null);
    } catch (e) { setError(String(e)); } finally { setBusy(false); }
  }
  // 목록이 연 예약: 같은 요청은 한 번만
  const asked = useRef(0);
  useEffect(() => {
    if (!request || request.seq === asked.current || !canEdit) return;
    asked.current = request.seq;
    setPosition(request.position);
    void inspect();
  }, [request, canEdit]); // eslint-disable-line react-hooks/exhaustive-deps
  return <section className={`task-queue-controls${pending ? " is-reserved" : ""}`} data-testid="task-queue-controls" data-state={q?.state ?? "none"}>
    {q && <p className="task-queue-status" data-testid="task-queue-status">{pending && <span className={`task-reserved-dot${q.state === "check" ? " is-check" : ""}`} aria-hidden="true" />}<ThreadStageBadge stage={q.stage} /><span className="task-queue-word">{afterWait ? "예약됨 · 앞 할 일 완료 뒤 시작" : words[q.state]}</span>{q.thread && ` · ${q.thread}`}</p>}
    {why && <div className="task-queue-why" role="note" data-testid="task-queue-why"><b>기다리는 까닭: {why}</b>{q?.state === "queued" && <span>{afterWait ? "앞 할 일이 완료 보관되면 자리 차례에 든다." : "다른 머신으로 바꾸지 않고 기다린다."} 그동안 뒤의 예약이 먼저 시작될 수 있다.</span>}</div>}
    {q && <p className="task-queue-meta">모델 {q.profile} {q.model} · effort {q.effort || "기본"} · 머신 {q.machine ?? "자동"}{q.after && <span data-testid="task-queue-after"> · {q.after_short_id ?? q.after} 완료 뒤 시작</span>}</p>}
    {canEdit && (!q || ["queued", "check", "cancelled", "finished"].includes(q.state)) && <button className="btn" type="button" disabled={busy} onClick={() => { setPosition(null); void inspect(); }} data-testid="task-reserve-preview">{pending ? "예약 내용 갱신" : "자리 나면 시작 예약"}</button>}
    {canEdit && pending && <div className="task-queue-actions">
      {onStep && <>
        <button className="btn task-queue-move" type="button" disabled={busy} onClick={() => onStep("up")}>예약 위로</button>
        <button className="btn task-queue-move" type="button" disabled={busy} onClick={() => onStep("down")}>예약 아래로</button>
        <button className="btn task-queue-move" type="button" disabled={busy} onClick={() => onStep("top")}>예약 맨 앞으로</button>
      </>}
      <button className="btn task-queue-cancel" type="button" disabled={busy} onClick={() => void action({op:"cancel",task_id:q.task_id})}>예약 풀기</button>
    </div>}
    {pending && <p className="task-queue-home" data-testid="task-queue-home">예약을 풀면 원래 자리로 돌아간다: {unreserveHome(board, task)}. 차례는 host가 저장하고 검색·거르기는 차례를 바꾸지 않는다.</p>}
    {preview && <div data-testid="task-reserve-confirm">
      <p>모델 {preview.profile} {preview.model} · effort {preview.effort || "기본"} · 머신 {preview.machine ?? "자동 배정"} · 저장소 {task.repo ?? "정하지 않음"}{position !== null && ` · 예약 ${position}번째 자리에`}</p>
      <Markdown>{preview.task}</Markdown>
      {afterApi && <label className="task-reserve-after">앞 할 일
        <select className="input" value={after} disabled={busy} onChange={(e) => setAfter(e.target.value)} data-testid="task-reserve-after">
          <option value="">없음 · 자리 나면 바로 시작</option>
          {after && !choices.some((t) => t.id === after) && <option value={after}>{q?.after_short_id ?? after} · 완료 보관됨</option>}
          {choices.map((t) => <option key={t.id} value={t.id!}>{t.short_id ? `${t.short_id} · ` : ""}{t.title}</option>)}
        </select>
        <span>고르면 그 할 일이 완료 보관된 뒤에 자리 차례에 든다.</span>
      </label>}
      {(!task.repo || task.notes.length===0) && <p role="alert">먼저 설명·완료 범위 메모와 저장소 또는 작업 폴더를 저장한다.</p>}
      <button className="btn btn-primary" type="button" disabled={busy || !task.repo || task.notes.length===0} onClick={() => void reserve(preview)} data-testid="task-reserve-go">이 내용으로 예약</button>
      <button className="btn" type="button" disabled={busy} onClick={() => { setPreview(null); setPosition(null); }}>닫기</button>
    </div>}
    {error && <p role="alert">{error}</p>}
  </section>;
}

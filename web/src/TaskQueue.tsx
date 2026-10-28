import { ThreadStageBadge } from "./ThreadStageBadge";
// 예약의 정본은 host다. 이 파일에는 scheduler나 별도 저장을 두지 않는다. 할 일 한 목록(plan §8.393): 예약 행은 숫자·까닭 글 없이
// 보라 표지만 보이고, 기다리는 특별한 까닭(머신 연결 대기·확인 필요)과 풀면 돌아갈 자리는 여기 상세에만 보인다.
// 상세 정리(t-1097): 예약 상태는 본문 위(`QueueStatus`), 예약 확인은 상세 아래 고정 줄의 그 자리(`ReservePanel`)에 연다. 위쪽 작은 칸 안의
// 스크롤에 「이 내용으로 예약」이 가리지 않게 한다. 확인에는 맡길 글 전체를 다시 그리지 않는다(메모는 본문에 이미 있다).
import { useEffect, useRef, useState } from "react";
import { Clock, X } from "lucide-react";
import { call } from "./api";
import type { QueueOp, QueueState, TaskBoard, TaskReservation, TaskView, ThreadStartPreview } from "./gen/proto";
import { queueWhy, unreserveHome } from "./tasksModel";
import { AutoMergeCheck, setAutoMerge } from "./AutoMerge";
const words: Record<QueueState, string> = { queued: "예약됨 · 자리 나면 시작", check: "예약 · 확인 필요", starting: "시작 중", running: "진행", finished: "resolve 완료", cancelled: "예약 풀림" };

/** 목록이 연 예약(끌어 놓기·⋯ 「자리 나면 시작 예약」): 내용 확인을 바로 열고, 예약한 뒤 그 차례(host 순번, 없으면 맨 뒤)로 옮긴다 */
export type ReserveRequest = { seq: number; position: number | null };

export interface Queue {
  q: TaskReservation | undefined;
  /** 자리 차례를 기다리는 예약(queued·check) */
  pending: boolean;
  /** 「예약」·「예약 갱신」을 누를 수 있다 */
  canReserve: boolean;
  /** 열린 예약 확인(host 시작 미리보기) */
  preview: ThreadStartPreview | null;
  position: number | null;
  busy: boolean;
  error: string;
  after: string;
  setAfter: (after: string) => void;
  /** 예약 확인의 자동 병합 칸(처음은 할 일의 지금 값). 예약할 때 바뀌었으면 먼저 저장한다 */
  autoMerge: boolean;
  setAutoMerge: (on: boolean) => void;
  inspect: () => void;
  reserve: () => void;
  close: () => void;
  cancel: () => void;
}

/** 한 task의 예약 상태와 조작. task가 없으면(새 task) 아무것도 하지 않는다 */
export function useQueue({ task, board, canEdit, afterApi, autoMergeApi, request, onUnreserved }: {
  task: TaskView | null;
  board: TaskBoard;
  canEdit: boolean;
  /** 예약 확인에서 앞 할 일을 고른다(host가 그 할 일이 완료 보관될 때까지 기다린다) */
  afterApi?: boolean;
  /** host가 자동 병합을 안다(FRONT_API 1.143) */
  autoMergeApi?: boolean;
  request?: ReserveRequest | null;
  /** 예약을 풀었다(목록이 원래 자리에서 잠깐 알린다) */
  onUnreserved?: (t: TaskView) => void;
}): Queue {
  const [preview, setPreview] = useState<ThreadStartPreview | null>(null);
  const [position, setPosition] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const q = task ? board.queue?.find((q) => q.task_id === task.id) : undefined;
  const pending = !!q && ["queued", "check"].includes(q.state);
  // 앞 할 일(ULID, 빈 글은 없음). 예약 내용 갱신도 지금 앞 할 일을 그대로 다시 보낸다
  const [after, setAfter] = useState("");
  const [autoMerge, setMerge] = useState(false);
  async function action(action: QueueOp) {
    if (busy || !task) return;
    setBusy(true); setError("");
    try {
      await call({ op: "task_queue", project: board.project, action });
      setPreview(null);
      if (action.op === "cancel") onUnreserved?.(task);
    }
    catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  }
  async function inspect(at: number | null) {
    if (!task) return;
    setPosition(at);
    setBusy(true); setError("");
    try {
      const r = await call({ op: "thread_start_preview", project: board.project, from_task: task.short_id ?? task.title, task_block: task.block_sha256 });
      if (r.kind === "thread_start_preview") { setAfter(pending ? (q?.after ?? "") : ""); setMerge(!!task.auto_merge); setPreview(r); }
    } catch (e) { setError(String(e)); } finally { setBusy(false); }
  }
  async function reserve() {
    if (busy || !task || !preview) return;
    setBusy(true); setError("");
    try {
      const p = preview;
      if (autoMergeApi && autoMerge !== !!task.auto_merge) await setAutoMerge(board.project, task.short_id ?? task.title, autoMerge);
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
    void inspect(request.position);
  }, [request, canEdit]); // eslint-disable-line react-hooks/exhaustive-deps
  return {
    q,
    pending,
    canReserve: canEdit && !!task && (!q || ["queued", "check", "cancelled", "finished"].includes(q.state)),
    preview,
    position,
    busy,
    error,
    after,
    setAfter,
    autoMerge,
    setAutoMerge: setMerge,
    inspect: () => void inspect(null),
    reserve: () => void reserve(),
    close: () => { setPreview(null); setPosition(null); },
    cancel: () => q && void action({ op: "cancel", task_id: q.task_id }),
  };
}

/** 예약 상태(본문 위): 상태 글·모델/머신·앞 할 일, 특별한 까닭, 풀면 돌아갈 자리 */
export function QueueStatus({ task, board, queue }: { task: TaskView; board: TaskBoard; queue: Queue }) {
  const { q, pending } = queue;
  if (!q) return null;
  const why = pending ? queueWhy(q) : null;
  const afterWait = q.state === "queued" && !!q.after && q.reason.startsWith("앞 할 일");
  return <section className={`task-queue-controls${pending ? " is-reserved" : ""}`} data-testid="task-queue-controls" data-state={q.state}>
    <p className="task-queue-status" data-testid="task-queue-status">{pending && <span className={`task-reserved-dot${q.state === "check" ? " is-check" : ""}`} aria-hidden="true" />}<ThreadStageBadge stage={q.stage} /><span className="task-queue-word">{afterWait ? "예약됨 · 앞 할 일 완료 뒤 시작" : words[q.state]}</span>{q.thread && ` · ${q.thread}`}</p>
    <p className="task-queue-meta">{q.profile} {q.model} · effort {q.effort || "기본"} · 머신 {q.machine ?? "자동"}{q.after && <span data-testid="task-queue-after"> · {q.after_short_id ?? q.after} 완료 뒤 시작</span>}</p>
    {why && <div className="task-queue-why" role="note" data-testid="task-queue-why"><b>기다리는 까닭: {why}</b></div>}
    {pending && <p className="task-queue-home" data-testid="task-queue-home">풀면 돌아갈 자리: {unreserveHome(board, task)}</p>}
  </section>;
}

/** 예약 확인(상세 아래 고정 줄 자리): 모델·머신·저장소 한 줄, 앞 할 일, 예약·취소 */
export function ReservePanel({ task, board, afterApi, autoMergeApi, queue }: { task: TaskView; board: TaskBoard; afterApi?: boolean; autoMergeApi?: boolean; queue: Queue }) {
  const { preview, position, busy, after, q } = queue;
  if (!preview) return null;
  const choices = board.tasks.filter((t) => t.id && t.id !== task.id && !t.done);
  const ready = !!task.repo && task.notes.length > 0;
  return <div className="task-reserve" data-testid="task-reserve-confirm">
    <div className="task-reserve-head"><Clock aria-hidden="true" /><span>{queue.pending ? "예약 내용 갱신" : "자리 나면 시작 예약"}</span>
      <button type="button" className="task-reserve-x" aria-label="예약 닫기" disabled={busy} onClick={queue.close}><X aria-hidden="true" /></button>
    </div>
    <p className="task-reserve-sum">{preview.profile}{preview.model && ` ${preview.model}`} · effort {preview.effort || "기본"} · 머신 {preview.machine ?? "자동 배정"} · {task.repo ?? "저장소 정하지 않음"}{position !== null && ` · 예약 ${position}번째 자리에`}</p>
    {afterApi && <label className="task-reserve-after"><span>앞 할 일</span>
      <select className="input" value={after} disabled={busy} onChange={(e) => queue.setAfter(e.target.value)} data-testid="task-reserve-after">
        <option value="">없음 · 자리 나면 바로 시작</option>
        {after && !choices.some((t) => t.id === after) && <option value={after}>{q?.after_short_id ?? after} · 완료 보관됨</option>}
        {choices.map((t) => <option key={t.id} value={t.id!}>{t.short_id ? `${t.short_id} · ` : ""}{t.title}</option>)}
      </select>
    </label>}
    {autoMergeApi && <AutoMergeCheck checked={queue.autoMerge} disabled={busy} onChange={queue.setAutoMerge} testid="task-reserve-auto-merge" />}
    {!ready && <p className="task-reserve-need" role="alert">먼저 메모와 저장소(또는 작업 폴더)를 저장한다.</p>}
    <div className="task-detail-actions">
      <button className="btn btn-primary" type="button" disabled={busy || !ready} onClick={queue.reserve} data-testid="task-reserve-go"><Clock aria-hidden="true" />이 내용으로 예약</button>
      <button className="btn" type="button" disabled={busy} onClick={queue.close}>취소</button>
    </div>
  </div>;
}

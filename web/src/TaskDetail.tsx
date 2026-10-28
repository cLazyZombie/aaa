// 할 일 상세와 편집 폼(할 일 보기 T1 §8.306의 상세를 편집 T2 §8.311에서 폼으로 넓혔다). 칸은 t-0469 §3.4: 제목·owner
// (미할당·나·agent·사람)·메모(Markdown 미리보기)·상위 task·연결 thread. 폼 모양과 저장 흐름은 t3code ScheduledTasksSettings.tsx
// `ScheduledTaskEditorForm`(948–1124: 저장 중 두 번 보내지 않음, 실패는 폼 안에, 닫지 않음)을 따른다. 보기 권한 기기는 T1의 읽기
// 전용 상세 그대로다. thread 연결(T3, §8.316): 「coordinator에게 맡기기」(기본, herdr-projects popup.rs `task_key` `d`와 같은 뜻이나
// 바로 보내지 않고 입력칸을 채운다)·「바로 시작」, 「확인 필요」 task의 「끝남으로 지우기」·「다시 맡기기」·「연결만 풀기」.
//
// 편집을 시작한 판(`base`)의 블록 hash로 저장한다: 그사이 파일에서 이 task가 바뀌면 host가 쓰지 않고 충돌로 돌려준다(t-0469 §2.3).
// 손대지 않은 폼은 새 판을 따라가고, 손댄 폼은 「파일에서 바뀌었다」를 보인다.
//
// 첨부 그림(§8.327, TaskAttachments.tsx): 손대지 않은 폼에 붙이면 메모 끝에 첨부 줄을 더해 바로 저장한다(사진 고르기 없이 「붙여넣기」
// 한 번으로 그 task에 붙는다). 고치는 중인 폼(손댄 폼·새 task)은 초안 메모에 더하고 「저장」 때 함께 쓴다:
// 그사이 바로 저장하면 편집을 시작한 판의 블록 hash가 낡아 폼 저장이 충돌로 돌아온다. 메모 입력칸은 첨부 줄을 빼고 보이며(첨부 칸이
// 보인다) 첨부 줄은 메모 끝에 둔다.
//
// 상세 정리(t-1097, 사용자 10-26 q-1380 1A·q-1381 2A·q-1382 3B): 제목·예약 상태·속성·메모가 위, 시작시키는 단추(맡기기·바로 시작·
// 예약)는 상세 아래 고정 줄 한곳이다. 예약 확인도 그 줄 자리에서 열려 스크롤에 가리지 않는다. 속성은 Plane 옆 패널
// (`issue-properties.tsx`, AGPL-3.0, 모양만 참고)처럼 이름·값 한 줄이고 고르면 바로 저장한다. 제목·메모는 모아서 「저장」한다.
import { useEffect, useLayoutEffect, useMemo, useRef, useState, type ClipboardEvent, type ReactNode, type SyntheticEvent } from "react";
import { ArrowDown, ArrowUp, ArrowUpToLine, Bot, Check, ChevronLeft, Circle, CircleDot, CirclePause, Clock, Ellipsis, Monitor, Play, Trash2, Triangle, TriangleAlert, Unlink, X } from "lucide-react";
import type { OwnerChoice, TaskAnchor, TaskBoard, TaskLogEntry, TaskOp, TaskView } from "./gen/proto";
import { QueueStatus, ReservePanel, useQueue, type Queue, type ReserveRequest } from "./TaskQueue";
import { RowMenu, type RowMenuItem } from "./RowMenu";
import { Markdown } from "./Markdown";
import { uploadTaskFile } from "./api";
import { TaskAttachments, dragHasFiles, filesOf, resolveReportLink, type PendingUpload } from "./TaskAttachments";
import { GROUP_WORD } from "./sidebarModel";
import { usePhoneSheetBack } from "./herdr/sheetHistory";
import { AutoMergeCheck, AutoMergeSwitch, setAutoMerge } from "./AutoMerge";
import type { MyEdit } from "./TaskDialogs";
import {
  addAttachment,
  fileKind,
  memoText,
  withMemoText,
  pickAttachable,
  removeAttachment,
  withoutAttachments,
  TASK_STATE_LABEL,
  anchorOf,
  canonicalLine,
  descendants,
  draftOf,
  draftOps,
  agentParts,
  draftOwner,
  emptyDraft,
  linkOp,
  noteLines,
  ownerLabel,
  parentAnchor,
  REPO_DIR,
  startable,
  taskDepth,
  taskState,
  type TaskAttachment,
  type DraftWho,
  type MoveDir,
  type TaskDraft,
  type TaskState,
  type ThreadGroups,
} from "./tasksModel";

/** 저장 결과: 썼다(새 board의 이 task 제목), 충돌(창은 부른 쪽이 띄운다), 바뀐 것 없음 */
export type SaveResult = { kind: "saved"; title: string } | { kind: "conflict" } | { kind: "none" };

export function StateIcon({ state }: { state: TaskState }) {
  const Icon = state === "problem" ? Triangle : state === "check" ? TriangleAlert : state === "run" ? CircleDot : state === "parked" ? CirclePause : state === "done" ? Check : Circle;
  return <Icon className={`task-state is-${state}`} aria-hidden="true" />;
}

/** thread 칩: `t-0468 · 작업 중`, 누르면 그 thread로. 모르는 thread는 누를 수 없다 */
export function ThreadChip({ id, groups, onThread }: { id: string; groups: ThreadGroups; onThread: (id: string) => void }) {
  const g = groups.get(id);
  const word = g ? (GROUP_WORD[g] ?? g) : "모름";
  return (
    <button
      type="button"
      className={`task-chip task-thread is-${g ?? "unknown"}`}
      disabled={g === undefined}
      aria-label={`${id} ${word}, thread 열기`}
      data-testid={`task-thread-${id}`}
      onClick={(e) => {
        e.stopPropagation();
        onThread(id);
      }}
    >
      {id} · {word}
    </button>
  );
}

/**
 * 맡은 쪽 칩(plan §8.335): agent 일은 모델·머신 칩 둘(정하지 않은 칸은 칩이 없다), 나·사람·미할당은 담당 칩 하나. `labeled`면 칩 글
 * 앞에 칸 이름을 쓰고(상세), 아니면 아이콘으로 줄인다(목록 줄)
 */
export function WhoChips({ t, agents = [], labeled = false }: { t: TaskView; agents?: readonly string[]; labeled?: boolean }) {
  const parts = agentParts(t, agents);
  if (parts) {
    return (
      <>
        {parts.profile && (
          <span className="task-chip task-model" title="모델" data-testid="task-chip-model">
            {labeled ? "모델 · " : <Bot aria-label="모델" />}
            {parts.profile}
          </span>
        )}
        {parts.machine && (
          <span className="task-chip task-machine" title="머신" data-testid="task-chip-machine">
            {labeled ? "머신 · " : <Monitor aria-label="머신" />}
            {parts.machine}
          </span>
        )}
      </>
    );
  }
  const owner = ownerLabel(t, agents);
  if (!owner) return null;
  return <span className={`task-chip task-owner${owner === "미할당" ? " is-dim" : ""}`}>{labeled ? `담당 · ${owner}` : owner}</span>;
}

/** 모델 칸 고르기 값(plan §8.335): 연결된 머신의 profile. 머신을 골랐으면 그 머신의 것만이고, 지금 값이 없으면 앞에 둔다 */
function modelOptions(owners: readonly OwnerChoice[], machine: string, current: string): string[] {
  const on = owners.filter((o) => o.connected && (machine === "" || o.machine === machine));
  const names = [...new Set(on.flatMap((o) => o.profiles))];
  if (current && !names.includes(current)) names.unshift(current);
  return names;
}

/** 머신 칸 고르기 값: 모든 머신(끊긴 머신은 고를 수 없고, 지금 값이 목록에 없으면 앞에 둔다) */
function machineOptions(owners: readonly OwnerChoice[], current: string): { value: string; label: string; disabled: boolean }[] {
  const out = owners.map((o) => ({ value: o.machine, label: o.connected ? o.machine : `${o.machine}(끊김)`, disabled: !o.connected && o.machine !== current }));
  if (current && !owners.some((o) => o.machine === current)) out.unshift({ value: current, label: `${current}(모르는 머신)`, disabled: false });
  return out;
}

export function agentNames(owners: readonly OwnerChoice[] | null): string[] {
  if (!owners) return [];
  const names = new Set<string>();
  for (const o of owners) {
    names.add(`@${o.machine}`);
    for (const p of o.profiles) {
      names.add(p);
      names.add(`${p}@${o.machine}`);
    }
  }
  return [...names];
}

export interface DetailProps {
  queueApi?: boolean;
  /** host가 예약의 앞 할 일을 안다(FRONT_API 1.113) */
  afterApi?: boolean;
  /** host가 자동 병합을 안다(FRONT_API 1.136): 상세의 스위치·새 할 일·예약 확인의 칸 */
  autoMergeApi?: boolean;
  /** 목록이 연 예약(끌어 놓기·⋯): 내용 확인을 바로 연다 */
  reserveRequest?: ReserveRequest | null;
  /** 예약 위로·아래로·맨 앞으로(목록이 거르기에 맞춰 순번을 정한다) */
  onQueueStep?: (t: TaskView, dir: MoveDir) => void;
  /** 예약을 풀었다 */
  onUnreserved?: (t: TaskView) => void;
  /** 고른 task(새 task를 만드는 중이면 없음) */
  t: TaskView | null;
  board: TaskBoard;
  byLine: ReadonlyMap<number, TaskView>;
  groups: ThreadGroups;
  /** 조작 기기이고 host가 편집을 안다(FRONT_API 1.62) */
  canEdit: boolean;
  /** 맡길 수 있는 owner(host `assignable`, 읽기 전이면 없음) */
  owners: OwnerChoice[] | null;
  /** 모델·머신 칸을 고르려 할 때 host 목록을 다시 읽는다 */
  onRefreshOwners?: () => void;
  /** 새 task 폼의 처음 값(「+ 할 일」·「하위 task 더하기」·「할 일로 옮기기」) */
  start: TaskDraft | null;
  /** 최근 기록(기록 한 줄·지난 thread) */
  log: readonly TaskLogEntry[];
  onThread: (id: string) => void;
  onPick: (t: TaskView) => void;
  onClose: () => void;
  onSave: (ops: TaskOp[], mine: MyEdit | null) => Promise<SaveResult>;
  /** 속성 칸 바로 저장(상세를 다시 열지 않는다): 새 board와 그 안의 이 task, 충돌이면 null(창은 부른 쪽이 띄운다) */
  onSaveProps?: (ops: TaskOp[], mine: MyEdit | null) => Promise<{ board: TaskBoard; task: TaskView | null } | null>;
  onDelete: (t: TaskView) => Promise<void>;
  /** 고치던 task가 파일에서 빠졌다(다른 곳이 지웠거나 제목을 바꿨다). 손댄 폼은 닫지 않고 그대로 둔다 */
  gone?: boolean;
  /** 폼에 손댔는가(손댄 폼은 task가 파일에서 빠져도 닫지 않는다) */
  onDirty?: (dirty: boolean) => void;
  /** host가 thread 연결을 안다(FRONT_API 1.63): 맡기기 단추와 「확인 필요」 단추 */
  linkApi?: boolean;
  /** 「coordinator에게 맡기기」(`again`: 「다시 맡기기」): coordinator 입력칸을 채우고 그 pane을 연다 */
  onHandOff?: (t: TaskView, again: boolean) => Promise<void>;
  /** 「바로 시작」 확인창을 연다 */
  onStart?: (t: TaskView) => void;
  /** host가 task 첨부를 안다(FRONT_API 1.71): 붙이기·떼기. 보기는 늘 한다 */
  attachApi?: boolean;
  /** host가 task 저장소 칸을 안다(FRONT_API 1.73): 폼의 저장소 고르기 */
  repoApi?: boolean;
  /** 첨부 문서 열기(올린 md·html, 보고서 안 문서: 파일 보기) */
  onOpenAttachment?: (a: TaskAttachment) => void;
  /** 손대지 않은 폼의 첨부 붙이기·떼기: 그 task 메모를 최신 판에서 고쳐 바로 저장한다(차례로 줄 세운다) */
  onChangeNotes?: (t: TaskView, change: (notes: string) => string) => Promise<void>;
  /** 상세 위에 그림 크게 보기가 떴다(그동안 휴대폰 뒤로 기록은 그 창 것이다). TaskDetail이 채운다 */
  onCover?: (covered: boolean) => void;
  /** 앱 파일 보기가 떠 있다(첨부 문서를 열었다): 그동안 휴대폰 뒤로 기록은 파일 보기 것이다 */
  fileOpen?: boolean;
  /** 깊은 주소로 연 상세: 주소 이동 자체가 기록 한 칸이므로 휴대폰 시트 칸을 더하지 않는다 */
  linked?: boolean;
}

export function TaskDetail(props: DetailProps) {
  const { t, onClose } = props;
  // 휴대폰은 전체 화면 시트: 시스템 Back이 닫는다(herdr-web-ui, sheetHistory.ts). 위에 크게 보기·파일 보기·⋯ 시트가 뜬 동안은 그 창이
  // 기록을 갖는다(시트 칸을 파일 보기가 차지한 채 두면 파일 보기를 닫은 뒤의 뒤로·화면 이동이 시트 칸과 어긋난다)
  const [covered, setCovered] = useState(false);
  usePhoneSheetBack(() => onClose(), !props.linked && !covered && !props.fileOpen);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      // 위에 뜬 창(팔레트·설정·확인창)의 Esc는 그 창 것이다
      if (e.key !== "Escape" || e.defaultPrevented || document.querySelector('[aria-modal="true"]')) return;
      e.preventDefault();
      onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  const queue = useQueue({ task: t, board: props.board, canEdit: props.canEdit && !!props.queueApi, afterApi: props.afterApi, autoMergeApi: props.autoMergeApi, request: props.reserveRequest, onUnreserved: props.onUnreserved });
  const state = t ? taskState(t, props.groups) : "wait";
  const label = t ? `할 일 ${t.title}` : props.gone ? "파일에서 빠진 할 일" : "새 할 일";
  return (
    <aside className="task-detail" role="dialog" aria-label={label} data-testid="task-detail" data-task-state={t ? state : props.gone ? "gone" : "new"}>
      <header className="task-detail-head">
        {/* 휴대폰은 「‹ 할 일」(전체 화면 시트의 뒤로), 넓은 화면은 닫기 X(오른쪽 끝) */}
        <button type="button" className="task-detail-close" aria-label="할 일 목록으로" title="할 일 목록으로" data-testid="task-detail-close" onClick={onClose}>
          <X className="task-close-x" aria-hidden="true" />
          <ChevronLeft className="task-close-back" aria-hidden="true" />
          <span className="task-close-back">할 일</span>
        </button>
        {t ? (
          <span className="task-detail-list">
            <StateIcon state={state} />
            {t.short_id && <span className="task-short-id" data-testid="task-detail-id">{t.short_id}</span>}
            <span className="task-chip">{TASK_STATE_LABEL[state]}</span>
            {t.thread && <ThreadChip id={t.thread} groups={props.groups} onThread={props.onThread} />}
            {t.done && <span className="task-chip is-dim">[x] 완료 표시</span>}
          </span>
        ) : (
          <span className="task-detail-list">{props.gone ? "파일에서 빠짐" : "새 할 일"}</span>
        )}
      </header>
      {props.canEdit ? <TaskForm {...props} queue={queue} onCover={setCovered} /> : t && <ReadOnly {...props} queue={queue} onCover={setCovered} t={t} />}
    </aside>
  );
}

/** 보기 권한: 같은 차례(제목·예약·맡은 쪽·형식 어긋남·메모·첨부·하위·자세히)를 읽기만 한다 */
function ReadOnly({ t, board, byLine, groups, onPick, onOpenAttachment, onCover, queue }: DetailProps & { t: TaskView; queue: Queue }) {
  return (
    <div className="task-detail-body">
      <h2 className="task-detail-title">{t.title}</h2>
      <QueueStatus task={t} board={board} queue={queue} />
      <div className="task-detail-chips">
        <WhoChips t={t} labeled />
        {t.repo && <span className="task-chip task-repo">저장소 · {t.repo}</span>}
      </div>
      {t.problems.length > 0 && <Problems t={t} />}
      {t.attention && <Attention t={t} />}
      <section className="task-detail-section">
        <h3>메모</h3>
        {withoutAttachments(t.notes.join("\n")) ? <Markdown className="task-notes">{withoutAttachments(t.notes.join("\n"))}</Markdown> : <p className="muted">메모 없음</p>}
      </section>
      <TaskAttachments project={board.project} notes={t.notes.join("\n")} canEdit={false} onOpen={onOpenAttachment} onCover={onCover} />
      <Related t={t} byLine={byLine} groups={groups} onPick={onPick} />
      <details className="task-more">
        <summary>자세히</summary>
        <section className="task-detail-section">
          <h3>원문 줄</h3>
          <pre className="task-raw" data-testid="task-raw">
            <code>{t.raw}</code>
          </pre>
        </section>
      </details>
    </div>
  );
}

function Problems({ t }: { t: TaskView }) {
  return (
    <section className="task-detail-section task-problems" data-testid="task-problems">
      <h3>형식 어긋남</h3>
      <ul>
        {t.problems.map((p) => (
          <li key={p}>{p}</li>
        ))}
      </ul>
    </section>
  );
}

/** 상위·하위 task로 가기. 편집 폼은 상위를 속성 줄에서 고르므로 하위만 그린다(`parent={false}`) */
function Related({ t, byLine, groups, onPick, parent: withParent = true }: { t: TaskView; byLine: ReadonlyMap<number, TaskView>; groups: ThreadGroups; onPick: (t: TaskView) => void; parent?: boolean }) {
  const parent = withParent && t.parent !== null ? byLine.get(t.parent) : undefined;
  const children = [...byLine.values()].filter((c) => c.parent === t.line);
  if (!parent && children.length === 0) return null;
  return (
    <section className="task-detail-section">
      {parent && (
        <>
          <h3>상위 task</h3>
          <ul className="task-related">
            <li>
              <button type="button" className="linkish" onClick={() => onPick(parent)}>
                {parent.title}
              </button>
            </li>
          </ul>
        </>
      )}
      {children.length > 0 && (
        <>
          <h3>하위 task</h3>
          <ul className="task-related">
            {children.map((c) => (
              <li key={c.line}>
                <StateIcon state={taskState(c, groups)} />
                <button type="button" className="linkish" onClick={() => onPick(c)}>
                  {c.title}
                </button>
              </li>
            ))}
          </ul>
        </>
      )}
    </section>
  );
}

/** 「확인 필요」: 이어진 thread가 끝났는데 완료로 판정하지 못했다(host `attention`). 조작 기기는 끝남으로 지우기·다시 맡기기·연결만 풀기 */
function Attention({ t, actions }: { t: TaskView; actions?: ReactNode }) {
  return (
    <section className="task-detail-section task-attention" data-testid="task-attention">
      <h3>
        <TriangleAlert aria-hidden="true" />
        확인 필요
      </h3>
      <p>{t.attention}</p>
      {actions && <div className="task-attention-actions">{actions}</div>}
    </section>
  );
}

function TaskForm(props: DetailProps & { queue: Queue }) {
  const { t, board, byLine, groups, owners, log, onPick, onSave, onSaveProps, onDelete, linkApi, onHandOff, onStart, attachApi, repoApi, onOpenAttachment, onChangeNotes, queue } = props;
  const agents = useMemo(() => agentNames(owners), [owners]);
  // 편집을 시작한 판: 이 판의 블록 hash로 저장한다. 상위 task도 그 판의 고정점으로 든다(줄 번호는 그사이 밀릴 수 있다)
  const [base, setBase] = useState<TaskView | null>(t);
  const [baseParent, setBaseParent] = useState<TaskAnchor | null>(() => (t ? parentAnchor(board, t) : null));
  const [draft, setDraft] = useState<TaskDraft>(() => (t ? draftOf(t, agents, baseParent) : (props.start ?? emptyDraft(board.lists.find((l) => l.name === "Backlog")?.name ?? board.lists[0]?.name ?? "Backlog"))));
  // 메모가 있는 task는 그린 메모부터 보인다(보기 기기와 같다). 「고치기」가 입력 칸으로 바꾼다
  const [preview, setPreview] = useState(() => (t?.notes.length ?? 0) > 0);
  const notesInput = useRef<HTMLTextAreaElement>(null);
  const [noteToAdd, setNoteToAdd] = useState("");
  const [addingNote, setAddingNote] = useState(false);
  const editNotes = useRef(false);
  useEffect(() => {
    if (!preview && editNotes.current) notesInput.current?.focus();
    editNotes.current = false;
  }, [preview]);
  // 제목은 여러 줄로 감싸 다 보인다(줄바꿈은 받지 않는다)
  const titleInput = useRef<HTMLTextAreaElement>(null);
  useLayoutEffect(() => {
    const el = titleInput.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${el.scrollHeight}px`;
  }, [draft.title]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const submitting = useRef(false);
  const [newMerge, setNewMerge] = useState(false);
  // 속성(저장소·담당·모델·머신·상위)은 고르면 바로 저장하고(사용자 10-26 q-1382 3B), 제목·메모·사람 이름은 모아서 「저장」한다.
  // `saved`는 편집을 시작한 판의 값이고, 고쳤는지는 글 칸만 그 판에 얹어 본다. 사람을 고르고 이름을 아직 쓰지 않은 담당도 글 칸이다
  const saved = useMemo(() => (base ? draftOf(base, agents, baseParent) : null), [base, agents, baseParent]);
  const withText = (s: TaskDraft, d: TaskDraft): TaskDraft => (d.who === "person" ? { ...s, title: d.title, notes: d.notes, who: "person", person: d.person } : { ...s, title: d.title, notes: d.notes });
  // 고쳤는지는 보낼 동작이 있는지로 본다: owner 목록이 늦게 와 agent·사람 구분만 달라진 폼은 손대지 않은 폼이다
  // 사람을 고른 직후에는 아직 저장할 owner가 없어도 편집 중이다. 늦은 목록이 이름 칸을 지우지 않게 한다.
  const emptyPerson = draft.who === "person" && draft.person.trim() === "";
  const textDraft = saved ? withText(saved, draft) : draft;
  const dirty = base ? emptyPerson || draftOps(base, textDraft, baseParent).length > 0 : draft.title.trim() !== "";
  const rebase = (x: TaskView) => {
    const p = parentAnchor(board, x);
    setBase(x);
    setBaseParent(p);
    setDraft(draftOf(x, agents, p));
  };
  // 손대지 않은 폼은 파일의 새 판을 따라간다. 손댄 폼은 그대로 두고 「바뀌었다」를 보인다
  useEffect(() => {
    if (t && base && t.block_sha256 !== base.block_sha256 && !dirty) rebase(t);
  }, [t, base, dirty, agents]); // eslint-disable-line react-hooks/exhaustive-deps
  // owner 목록을 늦게 받으면 agent·사람 구분을 다시 한다(손대기 전만)
  useEffect(() => {
    if (base) setDraft((current) => {
      // 목록을 그린 직후 선택한 값도 지킨다. effect를 예약한 render의 dirty는 옛 값일 수 있다.
      const editingPerson = current.who === "person" && current.person.trim() === "";
      if (editingPerson || draftOps(base, current, baseParent).length > 0) return current;
      return draftOf(base, agents, baseParent);
    });
  }, [agents]); // eslint-disable-line react-hooks/exhaustive-deps
  const changedOnDisk = base !== null && (t === null ? !!props.gone : t.block_sha256 !== base.block_sha256);
  const { onDirty } = props;
  useEffect(() => onDirty?.(dirty), [dirty, onDirty]);
  useEffect(() => () => onDirty?.(false), [onDirty]);
  const set = (patch: Partial<TaskDraft>) => setDraft((d) => ({ ...d, ...patch }));
  const reopen = () => {
    if (!t) return;
    rebase(t);
    setError(null);
  };
  const children = base ? descendants(board, base) : new Set<number>();
  const parentChoices = board.tasks.filter((x) => !base || (x.line !== base.line && !children.has(x.line)));
  const models = modelOptions(owners ?? [], draft.machine, draft.profile);
  const machines = machineOptions(owners ?? [], draft.machine);
  const mineOf = (d: TaskDraft): MyEdit => {
    const { owner } = draftOwner(d);
    return { title: d.title.trim(), owner: owner || null, repo: d.repo || null, thread: base?.thread ?? null, notes: noteLines(d.notes) };
  };
  const run = async (ops: TaskOp[], edit: MyEdit | null) => {
    if (submitting.current || ops.length === 0) return;
    submitting.current = true;
    setBusy(true);
    setError(null);
    try {
      const r = await onSave(ops, edit);
      // 새 할 일의 자동 병합은 만든 뒤 그 제목으로 정한다(host 값이라 TASKS.md 줄에는 없다)
      if (r.kind === "saved" && !base && newMerge) await setAutoMerge(board.project, r.title, true);
      if (r.kind === "saved" && !base) {
        setDraft(emptyDraft(draft.list));
        setNewMerge(false);
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      submitting.current = false;
      setBusy(false);
    }
  };
  const submit = (e?: SyntheticEvent) => {
    e?.preventDefault();
    if (!dirty || draft.title.trim() === "" || (draft.who === "person" && draft.person.trim() === "")) return;
    void run(draftOps(base, textDraft, baseParent), mineOf(textDraft));
  };
  /**
   * 속성 칸 고르기. 새 task는 초안에만 둔다. 있는 task는 그 칸만 편집을 시작한 판에 얹어 바로 저장하고, 고치던 제목·메모는 그대로 둔 채
   * 저장한 새 판으로 옮겨 탄다(다음 「저장」이 낡은 블록 hash로 충돌하지 않게). 충돌·실패면 칸을 그 판 값으로 되돌린다
   */
  const setProp = (patch: Partial<TaskDraft>) => {
    const next = { ...draft, ...patch };
    set(patch);
    if (!base || !saved || submitting.current) return;
    // 사람은 이름을 쓴 뒤 「저장」한다: 그동안 담당 칸은 그 판 값으로 보낸다
    const who = next.who === "person" ? { who: saved.who, person: saved.person, profile: saved.profile, machine: saved.machine } : { who: next.who, profile: next.profile, machine: next.machine };
    const want: TaskDraft = { ...saved, ...who, repo: next.repo, parent: next.parent };
    const ops = draftOps(base, want, baseParent);
    if (ops.length === 0) return;
    const revert = () => setDraft((d) => withText(saved, d));
    submitting.current = true;
    setBusy(true);
    setError(null);
    const save = onSaveProps ? onSaveProps(ops, mineOf(want)) : onSave(ops, mineOf(want)).then(() => null);
    void save.then(
      (r) => {
        if (!mounted.current) return;
        if (!r?.task) {
          if (onSaveProps) revert();
          return;
        }
        const p = parentAnchor(r.board, r.task);
        setBase(r.task);
        setBaseParent(p);
        setDraft((d) => withText(draftOf(r.task!, agents, p), d));
      },
      (e: unknown) => {
        if (!mounted.current) return;
        setError(e instanceof Error ? e.message : String(e));
        revert();
      },
    ).finally(() => {
      submitting.current = false;
      if (mounted.current) setBusy(false);
    });
  };
  // 첨부(§8.327). 올리는 동안 폼이 바뀔 수 있어 붙일 때의 값을 ref로 본다
  const [pending, setPending] = useState<PendingUpload[]>([]);
  const latest = useRef({ base, dirty });
  latest.current = { base, dirty };
  const mounted = useRef(true);
  useEffect(() => () => {
    mounted.current = false;
  }, []);
  const uploadSeq = useRef(0);
  const canAttach = !!attachApi && !!onChangeNotes;
  /** 메모 고치기: 고치는 중인 폼은 초안에, 손대지 않은 폼은 바로 저장한다(폼은 새 판을 따라간다) */
  const changeNotes = (f: (notes: string) => string) => {
    const now = latest.current;
    if (!now.base || now.dirty || !onChangeNotes) {
      setDraft((d) => ({ ...d, notes: f(d.notes) }));
      return;
    }
    onChangeNotes(now.base, f).catch((e: unknown) => mounted.current && setError(e instanceof Error ? e.message : String(e)));
  };
  const attachFiles = async (files: File[]) => {
    if (!canAttach || files.length === 0) return;
    const { ok, skipped } = pickAttachable(files);
    setError(skipped.length > 0 ? `첨부하지 않은 것: ${skipped.join(", ")}` : null);
    const picked = ok.map((i) => files[i]).filter((f): f is File => f !== undefined);
    if (picked.length === 0) return;
    const tiles = picked.map((f) => ({ id: ++uploadSeq.current, preview: fileKind(f) === "image" ? URL.createObjectURL(f) : null, name: f.name || "그림" }));
    setPending((p) => [...p, ...tiles]);
    const added: TaskAttachment[] = [];
    try {
      for (const f of picked) {
        const kind = fileKind(f);
        const target = await uploadTaskFile(board.project, f, kind === "image" || kind === null ? undefined : kind);
        added.push(kind === "image" ? { kind: "image", target, label: "그림" } : { kind: "upload", target, label: f.name || target });
      }
    } catch (e) {
      if (mounted.current) setError(`파일을 올리지 못했다: ${e instanceof Error ? e.message : String(e)}`);
    } finally {
      for (const tile of tiles) if (tile.preview) URL.revokeObjectURL(tile.preview);
      if (mounted.current) setPending((p) => p.filter((x) => !tiles.some((tile) => tile.id === x.id)));
    }
    // 올린 것만 한 번에 붙인다(여러 개가 저장 한 번이다). 그사이 상세를 닫았으면 붙이지 않는다(파일은 uploads/에 남는다)
    if (added.length === 0 || !mounted.current) return;
    attach(added);
  };
  const attach = (added: readonly TaskAttachment[]) => changeNotes((notes) => added.reduce(addAttachment, notes));
  // 보고서 주소(공유 링크·앱 보고서 주소)는 앱 안 보고서 문서로 바꿔 붙인다. 그사이 상세를 닫았으면 붙이지 않는다
  const attachLink = async (a: TaskAttachment) => {
    const { link, note } = await resolveReportLink(board.project, a);
    if (!mounted.current) return;
    setError(note);
    attach([link]);
  };
  const removeFile = (target: string) => changeNotes((notes) => removeAttachment(notes, target));
  // 상세가 열린 동안 어디서 붙여 넣어도(목록 행·빈 곳) 이 task에 붙는다. 다른 입력칸(검색·제목)의 붙여 넣기는 그 칸 것이다
  useEffect(() => {
    if (!canAttach) return;
    const onPaste = (e: globalThis.ClipboardEvent) => {
      if (e.defaultPrevented || document.querySelector('[aria-modal="true"]')) return;
      const target = e.target instanceof HTMLElement ? e.target : null;
      if (target?.closest("input, textarea, select, [contenteditable='true'], [contenteditable='']")) return;
      const files = filesOf(e.clipboardData);
      if (files.length === 0) return;
      e.preventDefault();
      void attachFiles(files);
    };
    window.addEventListener("paste", onPaste);
    return () => window.removeEventListener("paste", onPaste);
  }); // 렌더마다 최신 attachFiles를 건다
  const pasteIntoNotes = (e: ClipboardEvent<HTMLTextAreaElement>) => {
    if (!canAttach) return;
    const files = filesOf(e.clipboardData);
    if (files.length === 0) return;
    e.preventDefault();
    void attachFiles(files);
  };
  const [dropping, setDropping] = useState(false);
  const s = base?.suggestion ?? null;
  const applySuggestion = () => {
    if (!base || !s) return;
    const ownerNow = base.owner === "agent" ? null : base.owner;
    const op: TaskOp = {
      op: "update",
      at: anchorOf(base),
      title: s.title,
      owner: (s.owner ?? null) === ownerNow ? null : (s.owner ?? ""),
      person: false,
      profile: null,
      machine: null,
      notes: s.notes,
      thread: (s.thread ?? null) === base.thread ? null : (s.thread ?? ""),
      repo: null,
      done: null,
    };
    void run([op], { title: s.title, owner: s.owner, thread: s.thread, notes: s.notes });
  };
  // thread로 맡기기(T3): 손대지 않은 폼의 task만(저장한 뒤 맡긴다). coordinator 입력칸 채우기는 pane을 열 때까지 막는다
  const [handing, setHanding] = useState(false);
  const handOff = (again: boolean) => {
    if (!t || !onHandOff || handing) return;
    setHanding(true);
    setError(null);
    void onHandOff(t, again).then(
      () => setHanding(false),
      (e: unknown) => {
        setError(e instanceof Error ? e.message : String(e));
        setHanding(false);
      },
    );
  };
  const canStart = !!linkApi && !!t && !!base && !dirty && startable(t, groups, agents);
  const unlink = () => {
    if (!base) return;
    void run([linkOp(base, "")], { title: base.title, owner: base.owner === "agent" ? null : base.owner, thread: null, notes: base.notes });
  };
  const remove = () => {
    if (!base) return;
    setBusy(true);
    setError(null);
    void onDelete(base).then(
      () => setBusy(false),
      (e: unknown) => {
        setError(e instanceof Error ? e.message : String(e));
        setBusy(false);
      },
    );
  };
  // ⋯: 예약 차례 옮기기와 지우기(자주 쓰지 않는 것). 휴대폰은 아래 시트이고 그동안 뒤로 기록은 시트 것이다
  const [more, setMore] = useState<HTMLElement | null>(null);
  const { onCover } = props;
  useEffect(() => onCover?.(more !== null), [more, onCover]);
  const moreItems: RowMenuItem[] = [
    ...(queue.pending && props.onQueueStep && t
      ? ([["up", "예약 위로", ArrowUp], ["down", "예약 아래로", ArrowDown], ["top", "예약 맨 앞으로", ArrowUpToLine]] as const).map(([dir, label, icon]) => ({ id: `queue-${dir}`, label, icon, run: () => props.onQueueStep!(t, dir) }))
      : []),
    ...(base && !t?.attention ? [{ id: "delete", label: children.size > 0 ? `하위 ${children.size}개와 함께 지우기` : "끝남으로 지우기", title: "기록에 남아 되살릴 수 있다", icon: Trash2, danger: true, divider: queue.pending && !!props.onQueueStep, run: remove }] : []),
  ];
  const history = t ? log.filter((e) => e.title === t.title || e.title === base?.title) : [];
  const pastThreads = [...new Set(history.flatMap((e) => (e.thread && e.thread !== t?.thread ? [e.thread] : [])))];
  const quiet = busy || handing || queue.busy;
  return (
    <form className={`task-form${dropping ? " is-dropping" : ""}`} onSubmit={submit} onKeyDown={(e) => {
      if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) submit(e);
    }}
      onDragOver={(e) => {
        if (!canAttach || !dragHasFiles(e)) return;
        e.preventDefault();
        setDropping(true);
      }}
      onDragLeave={(e) => {
        if (e.currentTarget.contains(e.relatedTarget as Node | null)) return;
        setDropping(false);
      }}
      onDrop={(e) => {
        if (!canAttach || !dragHasFiles(e)) return;
        e.preventDefault();
        setDropping(false);
        void attachFiles(filesOf(e.dataTransfer));
      }}
      aria-busy={busy} data-task-saving={busy ? "true" : "false"} data-testid="task-form">
      <div className="task-detail-body">
        {changedOnDisk && (
          <div className="banner task-form-stale" role="status" data-testid="task-form-stale">
            {t ? "이 task가 파일에서 바뀌었다. 저장하면 충돌로 돌아온다." : "이 task가 파일에서 빠졌다(지워졌거나 제목이 바뀜). 고친 것은 여기 남아 있다."}{" "}
            {t && (
              <button type="button" className="linkish" onClick={reopen}>
                최신으로 다시 열기
              </button>
            )}
          </div>
        )}
        {base && base.problems.length > 0 && <Problems t={base} />}
        {t?.attention && (
          <Attention
            t={t}
            actions={
              linkApi && !t.attention?.includes("후속 남음") ? (
                <>
                  <button type="button" className="btn btn-danger" disabled={busy || dirty} data-testid="task-attention-delete" onClick={() => base && void onDelete(base).catch((e: unknown) => setError(e instanceof Error ? e.message : String(e)))}>
                    끝남으로 지우기
                  </button>
                  <button type="button" className="btn" disabled={busy || dirty || handing || !onHandOff} data-testid="task-attention-again" onClick={() => handOff(true)}>
                    <Bot aria-hidden="true" />
                    다시 맡기기
                  </button>
                  <button type="button" className="btn btn-ghost" disabled={busy || dirty} data-testid="task-attention-unlink" onClick={unlink}>
                    <Unlink aria-hidden="true" />
                    연결만 풀기
                  </button>
                  {canStart && onStart && (
                    <button type="button" className="linkish" disabled={busy} data-testid="task-attention-start" onClick={() => onStart(t)}>
                      바로 다시 시작
                    </button>
                  )}
                </>
              ) : undefined
            }
          />
        )}
        {!t && draft.thread && (
          <div className="task-detail-chips" data-testid="task-new-thread">
            <span className="task-chip task-thread is-working">이을 thread · {draft.thread}</span>
          </div>
        )}
        {s && (
          <section className="task-detail-section task-suggestion" data-testid="task-suggestion">
            <h3>정리 제안</h3>
            <ul>
              {s.changes.map((c) => (
                <li key={c}>{c}</li>
              ))}
            </ul>
            <pre className="task-raw">
              <code>{[s.line, ...s.notes.map((n) => `  ${n}`)].join("\n")}</code>
            </pre>
            <button type="button" className="btn" disabled={busy || dirty} onClick={applySuggestion} data-testid="task-suggestion-apply">
              이대로 저장
            </button>
          </section>
        )}
        <label className="task-title-field">
          <span className="sr-only">제목</span>
          <textarea
            ref={titleInput}
            className="input task-title-input"
            rows={1}
            value={draft.title}
            disabled={busy}
            required
            maxLength={400}
            placeholder="제목"
            onChange={(e) => set({ title: e.target.value.replace(/\n/g, " ") })}
            onKeyDown={(e) => {
              // 한 줄 입력칸처럼 Enter가 저장이다(한글 조합 중 Enter는 조합 확정)
              if (e.key !== "Enter" || e.nativeEvent.isComposing || e.metaKey || e.ctrlKey) return;
              e.preventDefault();
              submit(e);
            }}
            data-testid="task-field-title"
            autoFocus={!t}
          />
        </label>
        {t && <QueueStatus task={t} board={board} queue={queue} />}
        <div className="task-props">
          {repoApi && (
            <label className="task-prop">
              <span>저장소</span>
              <select className="input" value={draft.repo} disabled={busy} onChange={(e) => setProp({ repo: e.target.value })} data-testid="task-field-repo">
                <option value="">정하지 않음</option>
                {(board.repos ?? []).map((r) => (
                  <option key={r} value={r}>
                    {r}
                  </option>
                ))}
                <option value={REPO_DIR}>작업 폴더(저장소 없음)</option>
                {draft.repo && draft.repo !== REPO_DIR && !(board.repos ?? []).includes(draft.repo) && <option value={draft.repo}>{draft.repo}(project.toml에 없음)</option>}
              </select>
            </label>
          )}
          <label className="task-prop">
            <span>담당</span>
            <select className="input" value={draft.who} disabled={busy} onChange={(e) => setProp({ who: e.target.value as DraftWho })} data-testid="task-field-owner">
              <option value="agent">agent</option>
              <option value="me">나(me)</option>
              <option value="person">사람…</option>
            </select>
          </label>
          {draft.who === "agent" && (
            <>
              <label className="task-prop">
                <span>모델</span>
                <select className="input" value={draft.profile} disabled={busy} title="자동: 시작할 때 프로젝트 기본 profile" onFocus={props.onRefreshOwners} onChange={(e) => setProp({ profile: e.target.value })} data-testid="task-field-profile">
                  <option value="">자동</option>
                  {models.map((p) => (
                    <option key={p} value={p}>
                      {p}
                    </option>
                  ))}
                  {owners === null && <option disabled>모델 목록 읽는 중</option>}
                </select>
              </label>
              <label className="task-prop">
                <span>머신</span>
                <select className="input" value={draft.machine} disabled={busy} title="자동: 시작할 때 그 저장소가 있는 머신 중 덜 붐비는 곳" onFocus={props.onRefreshOwners} onChange={(e) => setProp({ machine: e.target.value })} data-testid="task-field-machine">
                  <option value="">자동</option>
                  {machines.map((m) => (
                    <option key={m.value} value={m.value} disabled={m.disabled}>
                      {m.label}
                    </option>
                  ))}
                  {owners === null && <option disabled>머신 목록 읽는 중</option>}
                </select>
              </label>
            </>
          )}
          {draft.who === "person" && (
            <label className="task-prop">
              <span>사람 이름</span>
              <input className="input" value={draft.person} disabled={busy} onChange={(e) => set({ person: e.target.value })} data-testid="task-field-person" />
            </label>
          )}
          <label className="task-prop">
            <span>상위</span>
            <select
              className="input"
              value={draft.parent?.block_sha256 ?? ""}
              disabled={busy}
              onChange={(e) => {
                const p = parentChoices.find((x) => x.block_sha256 === e.target.value);
                setProp({ parent: p ? anchorOf(p) : null });
              }}
              data-testid="task-field-parent"
            >
              <option value="">없음</option>
              {draft.parent && !parentChoices.some((x) => x.block_sha256 === draft.parent?.block_sha256) && <option value={draft.parent.block_sha256}>{draft.parent.title}</option>}
              {parentChoices.map((x) => (
                <option key={x.line} value={x.block_sha256}>
                  {"— ".repeat(taskDepth(x, byLine))}
                  {x.title}
                </option>
              ))}
            </select>
          </label>
          {props.autoMergeApi && props.canEdit && (
            <div className="task-prop task-prop-merge">
              {base ? <AutoMergeSwitch project={board.project} target={base.short_id ?? base.title} on={!!t?.auto_merge} disabled={busy} testid="task-field-auto-merge" />
                : <AutoMergeCheck checked={newMerge} disabled={busy} onChange={setNewMerge} testid="task-field-auto-merge-new" />}
            </div>
          )}
        </div>
        <section className="task-detail-section task-memo">
          <h3 className="task-field-head">
            <span>메모</span>
            <button
              type="button"
              className="linkish"
              aria-pressed={preview}
              data-testid="task-notes-toggle"
              onClick={() => {
                editNotes.current = preview;
                setPreview((p) => !p);
              }}
            >
              {preview ? "고치기" : "미리보기"}
            </button>
          </h3>
          {preview ? (
            <div className="task-notes-preview" data-testid="task-notes-preview">{withoutAttachments(draft.notes) ? <Markdown className="task-notes">{withoutAttachments(draft.notes)}</Markdown> : <p className="muted">메모 없음</p>}</div>
          ) : (
            <textarea ref={notesInput} className="input task-notes-input" value={memoText(draft.notes)} disabled={busy} rows={5} onChange={(e) => { const text = e.target.value; setDraft((d) => ({ ...d, notes: withMemoText(d.notes, text) })); }} onPaste={pasteIntoNotes} aria-label="메모" data-testid="task-field-notes" placeholder="Markdown" />
          )}
          <div className="task-note-add">
            <input className="input" value={noteToAdd} disabled={busy || addingNote} aria-label="추가할 메모" data-testid="task-note-add-input" onChange={(e) => setNoteToAdd(e.target.value)} />
            <button type="button" className="btn" disabled={busy || addingNote || !noteToAdd.trim()} data-testid="task-note-add" onClick={() => {
              const line = noteToAdd.trim();
              if (!line) return;
              const add = (notes: string) => notes ? `${notes}\n${line}` : line;
              const now = latest.current;
              if (!now.base || now.dirty || !onChangeNotes) {
                setDraft((d) => ({ ...d, notes: add(d.notes) }));
                setNoteToAdd("");
                return;
              }
              setAddingNote(true);
              setError(null);
              void onChangeNotes(now.base, add).then(
                () => { if (mounted.current) setNoteToAdd(""); },
                (e: unknown) => { if (mounted.current) setError(e instanceof Error ? e.message : String(e)); },
              ).finally(() => { if (mounted.current) setAddingNote(false); });
            }}>메모 추가</button>
          </div>
        </section>
        <TaskAttachments
          project={board.project}
          notes={draft.notes}
          saved={base ? base.notes.join("\n") : ""}
          canEdit={canAttach}
          busy={busy}
          pending={pending}
          onFiles={(files) => void attachFiles(files)}
          onLink={canAttach ? (a) => void attachLink(a) : undefined}
          onRemove={(a) => removeFile(a.target)}
          onOpen={onOpenAttachment}
          onCover={props.onCover}
        />
        {t && (
          <>
            <Related t={t} byLine={byLine} groups={groups} onPick={onPick} parent={false} />
            <details className="task-more">
              <summary>자세히</summary>
              {pastThreads.length > 0 && (
                <div className="task-detail-chips">
                  {pastThreads.map((id) => (
                    <span key={id} className="task-chip is-dim" title="지난 thread(기록)">
                      지난 {id}
                    </span>
                  ))}
                </div>
              )}
              <section className="task-detail-section">
                <h3>원문 줄</h3>
                <pre className="task-raw" data-testid="task-raw">
                  <code>{t.raw}</code>
                </pre>
                {dirty && (
                  <p className="muted task-form-preview">
                    저장하면: <code>{canonicalLine(textDraft.title.trim(), draftOwner(textDraft).owner || null, textDraft.repo || null, t.thread)}</code>
                  </p>
                )}
              </section>
            </details>
          </>
        )}
      </div>
      {/* 아래 고정 줄(사용자 10-26 q-1380 1A): 고치는 중이면 저장·되돌리기, 아니면 맡기기·바로 시작·예약. 예약 확인도 이 자리에서 연다 */}
      <footer className="task-detail-foot" data-testid="task-detail-foot">
        {error && (
          <p className="task-form-error" role="alert" data-testid="task-form-error">
            {error}
          </p>
        )}
        {queue.error && (
          <p className="task-form-error" role="alert">
            {queue.error}
          </p>
        )}
        {t && queue.preview ? (
          <ReservePanel task={t} board={board} afterApi={props.afterApi} autoMergeApi={props.autoMergeApi} queue={queue} />
        ) : !base || dirty ? (
          <div className="task-detail-actions">
            <button className="btn btn-primary" disabled={busy || !dirty || draft.title.trim() === ""} data-testid="task-save">
              {base ? "저장" : "더하기"}
            </button>
            {base && (
              <button type="button" className="btn" disabled={busy} onClick={reopen}>
                되돌리기
              </button>
            )}
          </div>
        ) : (
          <div className="task-detail-actions">
            {canStart && !t?.attention && (onHandOff || onStart) && (
              <span className="task-handoff" data-testid="task-handoff">
                {onHandOff && (
                  <button type="button" className="btn btn-primary" disabled={quiet} data-testid="task-handoff-coordinator" onClick={() => handOff(false)}>
                    <Bot aria-hidden="true" />
                    coordinator에게 맡기기
                  </button>
                )}
                {onStart && (
                  <button type="button" className="btn" disabled={quiet} title="모델·머신·저장소를 확인하고 바로 시작" data-testid="task-handoff-start" onClick={() => t && onStart(t)}>
                    <Play aria-hidden="true" />
                    바로 시작
                  </button>
                )}
              </span>
            )}
            {queue.canReserve && (
              <button type="button" className="btn" disabled={quiet} title={queue.pending ? "예약 내용 갱신" : "자리 나면 시작 예약"} data-testid="task-reserve-preview" onClick={queue.inspect}>
                <Clock aria-hidden="true" />
                {queue.pending ? "예약 갱신" : "예약"}
              </button>
            )}
            {queue.pending && (
              <button type="button" className="btn task-queue-cancel" disabled={quiet} data-testid="task-reserve-cancel" onClick={queue.cancel}>
                예약 풀기
              </button>
            )}
            <span className="spacer" />
            {moreItems.length > 0 && (
              <button type="button" className="btn task-detail-more" aria-label="더 보기" aria-haspopup="menu" aria-expanded={more !== null} disabled={quiet} data-testid="task-detail-more" onClick={(e) => { const el = e.currentTarget; setMore((m) => (m ? null : el)); }}>
                <Ellipsis aria-hidden="true" />
              </button>
            )}
          </div>
        )}
      </footer>
      {more && <RowMenu anchor={more} title={t ? `${t.short_id ?? t.title} 더 보기` : "더 보기"} items={moreItems} onClose={() => setMore(null)} />}
    </form>
  );
}

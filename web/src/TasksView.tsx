import { taskTarget } from "./appIds";
import { ThreadLimitBadge } from "./ThreadLimitBadge";
import type { ThreadLimitOverride } from "./gen/proto";
import { ThreadStageBadge } from "./ThreadStageBadge";
// 본문(오른쪽) 할 일 화면(할 일 보기 T1 §8.306, 편집 T2 §8.311, 사용자 10-20 「하기로 했던 기능들을 다 볼수 있고 (왼쪽에 아이콘이
// 있어서 거기서 들어가게), 필요하면 수정, 삭제 할수 있고」). 활동 막대 「할 일」·`#tasks`가 연다. host `tasks`(TaskBoard)를 읽고
// `task_edit`로 고친다(바뀐 블록만 쓰고 블록 hash로 충돌을 감지한다). thread 연결(T3, §8.316): 상세의 「coordinator에게 맡기기」·
// 「바로 시작」(TaskStart.tsx), 「task 없는 열린 thread」의 「task에 잇기」·「task로 만들기」, 끝난 thread의 자동 정리 알림 줄(되살리기).
// 행 모양과 상태 아이콘은 t3code ComposerTasksBadge.tsx, 메모 표지 `≡`은 herdr-projects popup.rs `build`. 한 목록(plan §8.393, t-0611):
// 「## 목록」 머리로 나누지 않고, 예약은 맨 위에 host 예약 차례대로(보라 표지만), 그 밑에 일반 할 일을 둔다(승인 시안 t-0580 3판).
// 예약 끌기는 놓은 영역이 곧 값이다(Jira·GitHub Projects): 예약 영역에 놓으면 예약(내용 확인 뒤), 아래에 놓으면 예약 풀기.
// 행·목록 머리의 ⋯ 메뉴는 herdr-web-ui RowMenu(휴대폰은 아래 시트), 끌기는 herdr-web-ui Sidebar.tsx 483–491의 HTML 끌어 놓기와
// Alt+↑↓(580–584), 휴대폰은 끌기 대신 메뉴의 위로·아래로·맨 위로(t-0469 §3.3). 지운 뒤 「되살리기」 줄은 t3code
// showThreadUndoNotice.ts(되살리기 한 번만)를 따르되 시간으로 사라지지 않는다(기록 화면에도 남는다).
import { useCallback, useEffect, useMemo, useRef, useState, type DragEvent } from "react";
import {
  ArrowDown,
  ArrowUp,
  ArrowUpDown,
  ArrowUpToLine,
  ChevronDown,
  ChevronRight,
  CircleDashed,
  CircleDot,
  Clock,
  Copy,
  CornerDownRight,
  Ellipsis,
  FileCode,
  Folder,
  FolderGit2,
  GripVertical,
  History,
  Link2,
  ListPlus,
  Paperclip,
  Pencil,
  Plus,
  Layers,
  Search,
  Trash2,
  Undo2,
  X,
  type LucideIcon,
} from "lucide-react";
import type { MachineView, OwnerChoice, QueueOp, TaskBoard, TaskConflict, TaskLogEntry, TaskNotesFile, TaskOp, TaskThread, TaskView, ThreadGroup, ThreadStage, ThreadView } from "./gen/proto";
import { ApiError, call, me, onTasksChanged } from "./api";
import { DrawerToggle, HeadMore, HeadNote } from "./drawer";
import { Markdown } from "./Markdown";
import { GROUP_WORD } from "./sidebarModel";
import { RowMenu, type RowMenuItem } from "./RowMenu";
import { copyText } from "./herdr/clipboard";
import { useMediaQuery } from "./herdr/useMediaQuery";
import { StateIcon, TaskDetail, ThreadChip, WhoChips, type SaveResult } from "./TaskDetail";
import { ConflictDialog, RawEditor, type MyEdit } from "./TaskDialogs";
import { DocAddDialog, LinkDialog, StartDialog } from "./TaskStart";
import { TaskQueuePanel, type ReserveRequest } from "./TaskQueue";
import { TaskLogView } from "./TaskLogView";
import {
  TASK_FILTERS,
  TASK_STATE_LABEL,
  anchorOf,
  dropOp,
  emptyDraft,
  pendingQueue,
  queueDropPosition,
  queueStep,
  repoChoices,
  taskList,
  findTask,
  handOffText,
  itemToTask,
  lastLogId,
  linkOp,
  mineKeys,
  moveOp,
  newAutoCleans,
  noteItems,
  REPO_DIR,
  taskState,
  addAttachment,
  attachmentLine,
  attachmentsOf,
  docTaskTitle,
  noteLines,
  withoutAttachments,
  type TaskAttachment,
  type MoveDir,
  type TaskDraft,
  type TaskFilter,
  type TaskListRow,
  type TaskState,
  type ThreadGroups,
} from "./tasksModel";
import "./TasksView.css";

type Tab = "tasks" | "later" | "log";

/** host가 편집(`task_edit`)을 아는 FRONT_API */
const EDIT_API: readonly [number, number] = [1, 62];
/** host가 thread 연결(`thread_start_preview`·`link_task`·`attention`)을 아는 FRONT_API */
const LINK_API: readonly [number, number] = [1, 63];
/** task 첨부 그림 올리기(plan §8.327) */
const ATTACH_API: readonly [number, number] = [1, 71];
/** task 저장소 칸(plan §8.330) */
const REPO_API: readonly [number, number] = [1, 73];

/** 이 기기가 알린 마지막 자동 정리 기록 id(기기마다, 없으면 처음 읽은 기록까지를 본 것으로 둔다) */
const AUTO_SEEN_KEY = "aaa:tasks:auto-clean-seen";
function readAutoSeen(): number | null {
  try {
    const v = localStorage.getItem(AUTO_SEEN_KEY);
    return v === null ? null : Number(v) || 0;
  } catch {
    return null;
  }
}
function writeAutoSeen(id: number): void {
  try {
    localStorage.setItem(AUTO_SEEN_KEY, String(id));
  } catch {
    // 저장이 막혀도 이 화면 동안은 기억한다
  }
}

/** 저장소 거르기(§8.393): 이 기기에 프로젝트마다 기억한다. 없으면 전체(null), 빈 글은 「정하지 않음」 */
const REPO_KEY = (project: string) => `aaa:tasks:repo:${project}`;
function readRepo(project: string): string | null {
  try {
    return project ? localStorage.getItem(REPO_KEY(project)) : null;
  } catch {
    return null;
  }
}
function writeRepo(project: string, repo: string | null): void {
  try {
    if (!project) return;
    if (repo === null) localStorage.removeItem(REPO_KEY(project));
    else localStorage.setItem(REPO_KEY(project), repo);
  } catch {
    // 저장이 막혀도 이 화면 동안은 기억한다
  }
}

function apiAtLeast(v: string | null | undefined, [major, minor]: readonly [number, number]): boolean {
  const m = /^(\d+)\.(\d+)/.exec(v ?? "");
  return !!m && (Number(m[1]) > major || (Number(m[1]) === major && Number(m[2]) >= minor));
}

/**
 * 할 일 board(활동 막대 배지와 할 일 화면이 함께 쓴다). host 연결이 오를 때, host `tasks` 사건(TASKS.md가 바뀜)마다, 열린 thread
 * 모음이 바뀔 때(「task 없는 열린 thread」), 화면을 열 때 다시 읽는다. 늦게 돌아온 옛 응답은 버린다. 편집 응답의 board는 `replace`로
 * 바로 넣는다. 그 편집의 `tasks` 사건이 시작한 읽기는 버리지 않는다(편집 둘의 응답이 거꾸로 와도 그 읽기가 최신 판으로 덮는다).
 */
export function useTaskBoard(up: boolean, openThreads: string, visible: boolean): { board: TaskBoard | null; error: string | null; replace: (b: TaskBoard) => void } {
  const [board, setBoard] = useState<TaskBoard | null>(null);
  const [error, setError] = useState<string | null>(null);
  const seq = useRef(0);
  const load = useCallback(() => {
    const n = ++seq.current;
    call({ op: "tasks" })
      .then((r) => {
        if (n !== seq.current || r.kind !== "task_board") return;
        const { kind: _kind, ...b } = r;
        setBoard(b);
        setError(null);
      })
      .catch((e: unknown) => {
        if (n === seq.current) setError(e instanceof ApiError ? e.message : String(e));
      });
  }, []);
  const replace = useCallback((b: TaskBoard) => {
    setBoard(b);
    setError(null);
  }, []);
  useEffect(() => {
    if (up) load();
  }, [up, openThreads, load]);
  // 화면을 열 때마다 다시 읽는다: 첫 tick은 기억만 해 사건이 없으니, 연 뒤 만든 프로젝트·연 뒤 처음 본 파일을 놓치지 않게
  useEffect(() => {
    if (up && visible) load();
  }, [up, visible, load]);
  // 같은 hash여도 다시 읽는다: 「나중」 파일·설정만 바뀐 사건도 TASKS.md hash는 그대로다
  useEffect(() => onTasksChanged(() => load()), [load]);
  return { board, error, replace };
}
/**
 * 이 기기가 할 일 화면에서 본 `me` task(활동 막대 배지의 「새 me task」, t-0469 §3.1). 기기마다 localStorage에 둔다. 처음 읽는 기기는
 * 지금 me task를 모두 본 것으로 둔다(배지가 처음부터 쌓이지 않게). 지워진 task 열쇠는 다음 표시 때 걸러진다
 */
const MINE_SEEN_KEY = "aaa:tasks:mine-seen";
export function useSeenMine(board: TaskBoard | null): { seen: ReadonlySet<string> | null; see: (keys: string[]) => void } {
  const [seen, setSeen] = useState<ReadonlySet<string> | null>(() => {
    try {
      const v = localStorage.getItem(MINE_SEEN_KEY);
      return v === null ? null : new Set(JSON.parse(v) as string[]);
    } catch {
      return null;
    }
  });
  const see = useCallback((keys: string[]) => {
    setSeen((old) => {
      if (old !== null && keys.every((k) => old.has(k))) return old;
      const next = new Set([...(old ?? []), ...keys]);
      try {
        localStorage.setItem(MINE_SEEN_KEY, JSON.stringify([...next]));
      } catch {
        // 저장이 막혀도 이 화면 동안은 기억한다
      }
      return next;
    });
  }, []);
  useEffect(() => {
    if (seen === null && board) see(mineKeys(board));
  }, [seen, board, see]);
  return { seen, see };
}

type Key = { title: string; line: number };

/** ⋯ 메뉴(행·목록 머리·「나중」 항목) */
type Menu = { key: string; anchor: HTMLElement; title: string; items: RowMenuItem[] };
/** 창 */
type Dialog =
  | { kind: "conflict"; conflict: TaskConflict; mine: MyEdit | null }
  | { kind: "raw" }
  | { kind: "start"; task: TaskView }
  | { kind: "link"; thread: TaskThread }
  | { kind: "doc-add"; link: TaskAttachment };

/** 할 일 화면 밖에서 여는 새 task: 명령 팔레트 「할 일 더하기」(link 없음), 보고서·thread 결과물 「할 일에 추가」(그 문서, plan §8.327) */
export type TaskAddRequest = { seq: number; link: TaskAttachment | null };
/** 할 일 화면 밖에서 여는 task(「답할 것」의 할 일 확인 필요 「할 일에서 정하기」, plan §8.345): 바뀌면 그 제목의 상세를 연다 */
export type TaskFocusRequest = { seq: number; title: string | null };

export function TasksView({
  shown,
  board,
  error,
  groups,
  machines,
  stages,
  overLimits,
  reader,
  addRequest,
  focusRequest,
  target = null,
  onTarget,
  onBoard,
  onThread,
  onHandOff,
  onSeenMine,
  onOpenAttachment,
  fileOpen,
}: {
  shown: boolean;
  /** 마지막으로 읽은 TaskBoard(App이 링크가 오를 때와 `tasks` 사건마다 읽는다) */
  board: TaskBoard | null;
  error: string | null;
  /** thread id → 묶음(App의 thread 목록, 바뀌면 바로 따라간다) */
  groups: ThreadGroups;
  /** host 상태 사건의 머신 목록(통계·pane 변경은 owner 재조회를 일으키지 않는다) */
  machines: readonly MachineView[];
  stages: ReadonlyMap<string, ThreadStage | undefined>;
  overLimits: ReadonlyMap<string, ThreadLimitOverride | undefined>;
  /** 보기 권한 기기: 편집 단추·끌기·「+」를 숨긴다 */
  reader: boolean;
  /** 바뀌면 새 task 폼(문서가 있으면 「할 일에 추가」 창)을 연다 */
  addRequest: TaskAddRequest;
  focusRequest?: TaskFocusRequest;
  target?: string | null;
  onTarget?: (id: string | null) => void;
  /** 편집 응답의 새 board */
  onBoard: (b: TaskBoard) => void;
  onThread: (id: string) => void;
  /** 「coordinator에게 맡기기」: coordinator 입력칸에 글을 채우고 그 pane을 연다(새 API 없음, t-0469 §3.4) */
  onHandOff?: (text: string) => Promise<void>;
  /** 할 일 화면에서 본 `me` task(활동 막대 배지의 「새 me task」를 걷는다) */
  onSeenMine?: (keys: string[]) => void;
  /** 첨부 문서 열기(파일 보기, plan §8.327) */
  onOpenAttachment?: (project: string, a: TaskAttachment) => Promise<void>;
  /** 앱 파일 보기가 떠 있다(상세 시트는 그동안 뒤로 기록을 내준다) */
  fileOpen?: boolean;
}) {
  const [tab, setTab] = useState<Tab>("tasks");
  const [filter, setFilter] = useState<TaskFilter>("all");
  const [query, setQuery] = useState("");
  const [picked, setPicked] = useState<Key | null>(null);
  const [creating, setCreating] = useState<TaskDraft | null>(null);
  // 저장한 뒤 폼을 새 판으로 다시 연다
  const [saves, setSaves] = useState(0);
  const [menu, setMenu] = useState<Menu | null>(null);
  const [dialog, setDialog] = useState<Dialog | null>(null);
  const [undo, setUndo] = useState<{ title: string; logId: number } | null>(null);
  const [frontApi, setFrontApi] = useState<string | null>(null);
  const [owners, setOwners] = useState<OwnerChoice[] | null>(null);
  const [log, setLog] = useState<TaskLogEntry[]>([]);
  // 끄는 행: 예약 행이면 `qid`(host 예약 task ID). `over`는 놓을 자리(`q:<id>` 예약 행·`t:<줄>` 일반 행·`res-end` 예약 맨 뒤·`rest` 예약 풀기)
  const [drag, setDrag] = useState<{ line: number; qid: string | null; over: string | null; side: "before" | "after" } | null>(null);
  // 휴대폰 「⇅ 순서」: ⋯ 대신 ▲▼
  const [ordering, setOrdering] = useState(false);
  // 예약을 푼 task: 원래 자리에서 잠깐 「↩ 원래 자리로 돌아옴」
  const [flash, setFlash] = useState<string | null>(null);
  // 목록이 연 예약(끌어 놓기·⋯ 「자리 나면 시작 예약」)
  const [reserveReq, setReserveReq] = useState<ReserveRequest | null>(null);
  const project = board?.project ?? "";
  const [repo, setRepoState] = useState<string | null>(null);
  // 저장소 거르기는 이 기기에 프로젝트마다 기억한다
  useEffect(() => setRepoState(readRepo(project)), [project]);
  const setRepo = (r: string | null) => {
    setRepoState(r);
    writeRepo(project, r);
  };
  const coarse = useMediaQuery("(pointer: coarse)");
  const editApi = apiAtLeast(frontApi, EDIT_API);
  const canEdit = !reader && editApi;
  const linkApi = canEdit && apiAtLeast(frontApi, LINK_API);
  const attachApi = canEdit && apiAtLeast(frontApi, ATTACH_API);
  const repoApi = canEdit && apiAtLeast(frontApi, REPO_API);
  // 「바로 시작」으로 시작한 thread(열기 단추와 host 경고)
  const [started, setStarted] = useState<{ id: string; warnings: string[] } | null>(null);
  // 자동 정리 알림 줄(t-0469 §4 「알림 한 줄(되살리기 단추)」): 이 기기가 아직 알리지 않은 자동 정리
  const [autoSeen, setAutoSeen] = useState<number | null>(() => readAutoSeen());
  const seeAuto = useCallback((id: number) => {
    writeAutoSeen(id);
    setAutoSeen(id);
  }, []);
  useEffect(() => {
    if (!shown || frontApi !== null) return;
    me()
      .then((m) => setFrontApi(m?.front_api ?? ""))
      .catch(() => setFrontApi(""));
  }, [shown, frontApi]);
  const ownersSeq = useRef(0);
  const loadOwners = useCallback(() => {
    if (!shown || !canEdit) return;
    const n = ++ownersSeq.current;
    call({ op: "assignable", check: null })
      .then((r) => {
        if (n === ownersSeq.current && r.kind === "assignable") setOwners(r.owners);
      })
      .catch(() => {
        if (n === ownersSeq.current) setOwners((old) => old ?? []);
      });
  }, [shown, canEdit]);
  const machineConnections = JSON.stringify(machines.map((m) => [m.id, m.name, m.connection]).sort());
  const newTask = creating !== null;
  const starting = dialog?.kind === "start";
  // 처음 읽은 끊김 값이 고정되지 않게 연결 변화와 화면·상세·시작 창을 열 때 host에 다시 묻는다.
  // 숨김·다음 조회 뒤에 돌아온 옛 응답은 버리고, 고치던 폼은 그대로 둔다.
  useEffect(() => {
    loadOwners();
    return () => { ++ownersSeq.current; };
  }, [loadOwners, machineConnections, picked, newTask, starting]);
  // 상세의 기록 한 줄·지난 thread: 판이 바뀔 때마다 최근 기록
  const sha = board?.sha256 ?? "";
  useEffect(() => {
    if (!shown || !editApi) return;
    call({ op: "task_log", before: null, limit: 100, filter: null })
      .then((r) => r.kind === "task_log" && setLog(r.entries))
      .catch(() => undefined);
  }, [shown, editApi, sha]);
  // 처음 읽은 기기는 지금 기록까지를 본 것으로 둔다(지난 자동 정리를 한꺼번에 알리지 않는다)
  useEffect(() => {
    if (autoSeen === null && log.length > 0) seeAuto(lastLogId(log));
  }, [autoSeen, log, seeAuto]);
  const autoNews = autoSeen === null ? [] : newAutoCleans(log, autoSeen);
  // 화면을 열면 지금 me task를 모두 본 것으로 둔다(배지의 「새 me task」)
  useEffect(() => {
    if (shown && board) onSeenMine?.(mineKeys(board));
  }, [shown, board, onSeenMine]);
  const notesFiles = board?.notes_files ?? [];
  // 「나중」 파일이 설정에서 빠지면 할 일 탭으로
  useEffect(() => {
    if (tab === "later" && notesFiles.length === 0) setTab("tasks");
  }, [tab, notesFiles.length]);
  // 명령 팔레트 「할 일 더하기」: 화면을 처음 여는 요청도 받는다(처음 mount 전에 수가 올라 있다). host 판(`frontApi`)과 board를
  // 받기 전에는 아직 쓰지 않고 기다린다
  const lastAdd = useRef(0);
  useEffect(() => {
    if (addRequest.seq === lastAdd.current || frontApi === null || !board) return;
    lastAdd.current = addRequest.seq;
    if (!canEdit) return;
    setTab("tasks");
    if (addRequest.link === null) {
      setPicked(null);
      setCreating(emptyDraft(defaultList(board)));
    } else if (!attachApi) setOpError("이 host는 task 첨부를 모른다. host를 갱신한다");
    else setDialog({ kind: "doc-add", link: addRequest.link });
  }, [addRequest, canEdit, attachApi, board, frontApi]);
  const lastFocus = useRef(0);
  useEffect(() => {
    if (!focusRequest || focusRequest.seq === lastFocus.current || !board) return;
    lastFocus.current = focusRequest.seq;
    const t = board.tasks.find((x) => x.title === focusRequest.title);
    if (!t) return;
    setTab("tasks");
    setCreating(null);
    setPicked({ title: t.title, line: t.line });
  }, [focusRequest, board]);
  const linkedTarget = target === null ? null : taskTarget(target);
  const linkedTask = board && linkedTarget && (linkedTarget.project === null || linkedTarget.project === board.project)
    ? board.tasks.find((t) => t.short_id === linkedTarget.id) ?? null : null;
  useEffect(() => {
    if (target === null || !board) return;
    setTab("tasks");
    setCreating(null);
    setFilter("all");
    setQuery("");
    setRepo(null);
    setPicked(linkedTask ? { title: linkedTask.title, line: linkedTask.line } : null);
  }, [target, board?.project, linkedTask?.title, linkedTask?.line]);
  useEffect(() => {
    if (shown && linkedTask) document.querySelector('.task-row.is-selected')?.scrollIntoView({ block: "nearest" });
  }, [shown, linkedTask, picked, filter, query, repo]);
  const byLine = useMemo(() => new Map((board?.tasks ?? []).map((t) => [t.line, t])), [board]);
  const choices = useMemo(() => repoChoices(board), [board]);
  // 기억한 저장소가 목록에서 빠졌으면 전체로 본다
  const repoOn = repo !== null && choices.some((c) => c.key === repo) ? repo : null;
  const model = useMemo(() => (board ? taskList(board, filter, query, repoOn, groups) : null), [board, filter, query, repoOn, groups]);
  const order = useMemo(() => pendingQueue(board).map((q) => q.task_id), [board]);
  const found = findTask(board, picked);
  // 손댄 폼(상세가 알린다): 그 task가 파일에서 빠져도 닫지 않는다(다른 기기·coordinator가 지우거나 제목을 바꿈)
  const [formDirty, setFormDirty] = useState(false);
  // 숨은 동안(다른 화면)은 상세를 내린다: 휴대폰 시트의 뒤로 기록과 Esc를 남기지 않는다. 돌아오면 고른 task가 그대로 열린다
  const here = shown && tab === "tasks";
  const current = here ? found : null;
  const gone = here && picked !== null && board !== null && !found && formDirty;
  const detailOpen = current !== null || gone || (here && creating !== null);
  // 고른 task가 파일에서 빠지면 상세를 닫는다(손댄 폼은 남긴다)
  useEffect(() => {
    if (picked && board && !found && !formDirty) setPicked(null);
  }, [picked, board, found, formDirty]);
  const shownCount = (model?.top.length ?? 0) + (model?.rest.length ?? 0);
  const total = board?.tasks.length ?? 0;
  const queueApi = apiAtLeast(frontApi, [1, 85]);
  const pick = (t: TaskView) => {
    if (target !== null) onTarget?.(t.short_id);
    setCreating(null);
    setPicked({ title: t.title, line: t.line });
  };
  const closeDetail = useCallback(() => {
    if (target !== null) onTarget?.(null);
    setPicked(null);
    setCreating(null);
  }, [target, onTarget]);

  /** host에 동작을 보내고 새 board를 넣는다. 충돌이면 창을 띄우고 null */
  const edit = async (ops: TaskOp[], mine: MyEdit | null) => {
    const r = await call({ op: "task_edit", ops, base_sha256: null });
    if (r.kind !== "task_edited") throw new Error(`예상하지 못한 응답: ${r.kind}`);
    onBoard(r.board);
    if (r.conflict) {
      setDialog({ kind: "conflict", conflict: r.conflict, mine });
      return null;
    }
    return r;
  };
  /** 메뉴·끌기의 동작(실패는 화면 위 띠로) */
  const [opError, setOpError] = useState<string | null>(null);
  const act = (ops: TaskOp[]) => {
    setOpError(null);
    void edit(ops, null).catch((e: unknown) => setOpError(e instanceof Error ? e.message : String(e)));
  };
  // 첨부를 바로 저장하는 메모 고치기(plan §8.327): 차례로 줄 세워 그때의 최신 판에 쓴다. 저장 중에 또 붙여도(빠른 두 번 붙여넣기)
  // 앞 저장이 끝난 판의 블록으로 이어 쓴다. 판은 편집 응답의 board로 바로 넘긴다(부모가 다시 그리기를 기다리지 않는다)
  const boardRef = useRef(board);
  const boardProp = useRef(board);
  if (board !== boardProp.current) {
    boardProp.current = board;
    boardRef.current = board;
  }
  const notesChain = useRef<Promise<void>>(Promise.resolve());
  const changeTaskNotes = (t: TaskView, change: (notes: string) => string): Promise<void> => {
    const next = notesChain.current.then(async () => {
      const cur = findTask(boardRef.current, { title: t.title, line: t.line });
      if (!cur) throw new Error(`「${t.title}」 task가 파일에서 빠졌다`);
      const before = noteLines(cur.notes.join("\n"));
      const after = noteLines(change(cur.notes.join("\n")));
      if (after.join("\n") === before.join("\n")) return;
      const owner = cur.owner === "agent" ? null : cur.owner;
      const r = await edit([{ op: "update", at: anchorOf(cur), title: null, owner: null, person: false, profile: null, machine: null, notes: after, thread: null, done: null, repo: null }], { title: cur.title, owner, thread: cur.thread, notes: after });
      if (r) boardRef.current = r.board;
    });
    notesChain.current = next.catch(() => undefined);
    return next;
  };
  const onSave = async (ops: TaskOp[], mine: MyEdit | null): Promise<SaveResult> => {
    const r = await edit(ops, mine);
    if (!r) return { kind: "conflict" };
    if (r.log_ids.length === 0) return { kind: "none" };
    const title = mine?.title ?? "";
    const t = r.board.tasks.find((x) => x.title === title) ?? r.board.tasks.find((x) => x.title.toLowerCase() === title.toLowerCase());
    setCreating(null);
    if (t) setPicked({ title: t.title, line: t.line });
    setSaves((n) => n + 1);
    return { kind: "saved", title };
  };
  const remove = async (t: TaskView) => {
    const r = await edit([{ op: "delete", at: anchorOf(t) }], null);
    if (!r || r.log_ids.length === 0) return;
    setPicked(null);
    setUndo({ title: t.title, logId: r.log_ids[r.log_ids.length - 1] });
  };
  const restore = async (logId: number, title: string) => {
    const r = await edit([{ op: "restore", log_id: logId, list: null }], null);
    if (!r) return;
    setUndo(null);
    const t = r.board.tasks.find((x) => x.title === title);
    if (t && tab === "tasks") setPicked({ title: t.title, line: t.line });
  };

  const openMenu = (key: string, anchor: HTMLElement, title: string, items: RowMenuItem[]) => setMenu((m) => (m?.key === key ? null : { key, anchor, title, items }));

  // ───── 예약 차례(host 정본)와 한 목록 순서 ─────
  const shownReserved = new Set((model?.top ?? []).flatMap((r) => (r.kind === "reserved" ? [r.q.task_id] : [])));
  const shownLines = new Set((model?.rest ?? []).flatMap((r) => (r.kind === "task" ? [r.t.line] : [])));
  const queueAct = (action: QueueOp, t?: TaskView) => {
    if (!board) return;
    setOpError(null);
    void call({ op: "task_queue", project: board.project, action }).then(
      () => {
        if (action.op === "cancel" && t?.id) setFlash(t.id);
      },
      (e: unknown) => setOpError(e instanceof Error ? e.message : String(e)),
    );
  };
  /** 예약 위로·아래로·맨 앞으로: 거른 채면 보이는 예약과 자리를 바꾸고 숨은 예약은 그대로 둔다 */
  const reservedStep = (id: string, dir: MoveDir): number | null => queueStep(order, id, dir, (x) => shownReserved.has(x));
  const stepReserved = (id: string, dir: MoveDir) => {
    const position = reservedStep(id, dir);
    if (position !== null) queueAct({ op: "move", task_id: id, position });
  };
  /** 일반 할 일 위로·아래로·맨 위로: 거른 채면 보이는 이웃과 자리를 바꾼다(예약한 task는 맨 위에 있어 이웃이 아니다) */
  const taskStep = (t: TaskView, dir: MoveDir): TaskOp | null => (board ? moveOp(board, t, dir, (x) => shownLines.has(x.line) || x.parent !== null) : null);
  /** 예약 내용 확인을 열고, 예약한 뒤 `position`(없으면 맨 뒤) 차례로 */
  const startReserve = (t: TaskView, position: number | null) => {
    setCreating(null);
    setPicked({ title: t.title, line: t.line });
    setReserveReq((r) => ({ seq: (r?.seq ?? 0) + 1, position }));
  };
  const reservedItems = (r: Extract<TaskListRow, { kind: "reserved" }>): RowMenuItem[] => {
    const id = r.q.task_id;
    const step = (sid: string, label: string, icon: LucideIcon, dir: MoveDir): RowMenuItem[] => (reservedStep(id, dir) !== null ? [{ id: sid, label, icon, run: () => stepReserved(id, dir) }] : []);
    return [
      ...step("up", "예약 위로", ArrowUp, "up"),
      ...step("down", "예약 아래로", ArrowDown, "down"),
      ...step("top", "예약 맨 앞으로", ArrowUpToLine, "top"),
      { id: "unreserve", label: "예약 풀기", hint: r.missing ? undefined : "원래 자리로", icon: Undo2, divider: true, danger: true, run: () => queueAct({ op: "cancel", task_id: id }, r.t) },
      ...(r.missing ? [] : [{ id: "open", label: "열어서 고치기", icon: Pencil, run: () => pick(r.t) }]),
    ];
  };
  const rowItems = (t: TaskView): RowMenuItem[] => {
    if (!board) return [];
    const item = (id: string, label: string, icon: LucideIcon, op: TaskOp | null): RowMenuItem[] => (op ? [{ id, label, icon, run: () => act([op]) }] : []);
    const reservable = queueApi && !order.includes(t.id ?? "") && taskState(t, groups) !== "run";
    return [
      ...item("up", "위로", ArrowUp, taskStep(t, "up")),
      ...item("down", "아래로", ArrowDown, taskStep(t, "down")),
      ...item("top", "맨 위로", ArrowUpToLine, taskStep(t, "top")),
      ...(reservable ? [{ id: "reserve", label: "자리 나면 시작 예약", hint: "예약 맨 뒤에", icon: Clock, divider: true, run: () => startReserve(t, null) }] : []),
      { id: "child", label: "하위 task 더하기", icon: CornerDownRight, divider: true, run: () => {
        setPicked(null);
        setCreating(emptyDraft(t.list, anchorOf(t)));
      } },
      { id: "delete", label: "끝남으로 지우기", icon: Trash2, danger: true, run: () => void remove(t).catch((e: unknown) => setOpError(e instanceof Error ? e.message : String(e))) },
    ];
  };

  // 끌어 놓기(넓은 화면·마우스): 행 위 절반이면 앞, 아래 절반이면 뒤. 놓은 영역이 값이다: 예약 영역은 예약(차례), 아래 영역은 일반 순서·예약 풀기
  const dragged = drag ? byLine.get(drag.line) : undefined;
  const sideOf = (e: DragEvent<HTMLElement>): "before" | "after" => {
    const r = e.currentTarget.getBoundingClientRect();
    return e.clientY < r.top + r.height / 2 ? "before" : "after";
  };
  const over = (key: string, side: "before" | "after") => {
    if (drag && (drag.over !== key || drag.side !== side)) setDrag({ ...drag, over: key, side });
  };
  /** 예약 행에 놓기: 예약끼리는 차례 바꾸기, 일반 행은 그 자리에 예약(내용 확인 뒤) */
  const reservedDrop = (qid: string, side: "before" | "after"): (() => void) | null => {
    if (!drag || !dragged) return null;
    if (drag.qid) {
      const position = queueDropPosition(order, drag.qid, qid, side);
      return position === null ? null : () => queueAct({ op: "move", task_id: drag.qid!, position });
    }
    if (!queueApi || taskState(dragged, groups) === "run") return null;
    const position = queueDropPosition(order, dragged.id ?? "", qid, side);
    return position === null ? null : () => startReserve(dragged, position);
  };
  const onReservedDragOver = (qid: string) => (e: DragEvent<HTMLElement>) => {
    const side = sideOf(e);
    if (!reservedDrop(qid, side)) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = "move";
    over(`q:${qid}`, side);
  };
  const onReservedDrop = (qid: string) => (e: DragEvent<HTMLElement>) => {
    e.preventDefault();
    const run = drag ? reservedDrop(qid, drag.side) : null;
    setDrag(null);
    run?.();
  };
  // 예약 행을 끌 때 일반 행은 놓을 자리가 아니다: 아래 영역(예약 풀기)이 받는다
  const onRowDragOver = (t: TaskView) => (e: DragEvent<HTMLElement>) => {
    if (!dragged || !board || drag?.qid) return;
    const side = sideOf(e);
    if (!dropOp(board, dragged, t, side)) return;
    e.preventDefault();
    e.stopPropagation();
    e.dataTransfer.dropEffect = "move";
    over(`t:${t.line}`, side);
  };
  const onRowDrop = (t: TaskView) => (e: DragEvent<HTMLElement>) => {
    if (drag?.qid) return;
    e.preventDefault();
    e.stopPropagation();
    const op = dragged && board && drag ? dropOp(board, dragged, t, drag.side) : null;
    setDrag(null);
    if (op) act([op]);
  };
  const onRestDragOver = (e: DragEvent<HTMLElement>) => {
    if (!drag?.qid) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = "move";
    over("rest", "after");
  };
  const onRestDrop = (e: DragEvent<HTMLElement>) => {
    if (!drag?.qid) return;
    e.preventDefault();
    const qid = drag.qid;
    setDrag(null);
    queueAct({ op: "cancel", task_id: qid }, dragged);
  };
  const onResEndDragOver = (e: DragEvent<HTMLElement>) => {
    if (!dragged || drag?.qid || taskState(dragged, groups) === "run") return;
    e.preventDefault();
    over("res-end", "after");
  };
  const onResEndDrop = (e: DragEvent<HTMLElement>) => {
    e.preventDefault();
    const t = dragged;
    setDrag(null);
    if (t && !drag?.qid) startReserve(t, null);
  };
  // 예약을 푼 task가 원래 자리에 나타나면 보이게 굴리고 잠깐 뒤 알림을 걷는다
  const flashShown = flash !== null && (model?.rest ?? []).some((r) => r.kind === "task" && r.t.id === flash);
  useEffect(() => {
    if (!flashShown) return;
    document.querySelector('.task-row.is-flash')?.scrollIntoView({ block: "nearest" });
    const timer = window.setTimeout(() => setFlash(null), 4000);
    return () => window.clearTimeout(timer);
  }, [flashShown]);
  const repoLabel = repoOn === null ? "전체" : (choices.find((c) => c.key === repoOn)?.label ?? repoOn);
  const repoItems = (): RowMenuItem[] => [
    { id: "repo-all", label: "전체", hint: String(total), icon: Layers, checked: repoOn === null, run: () => setRepo(null) },
    ...choices.map((c, i) => ({ id: `repo-${c.key || "none"}`, label: c.label, hint: String(c.n), icon: c.key === "" ? CircleDashed : c.key === REPO_DIR ? Folder : FolderGit2, divider: i === 0, checked: repoOn === c.key, run: () => setRepo(c.key) })),
  ];
  const rowProps = (r: Exclude<TaskListRow, { kind: "gap" }>) => {
    const t = r.t;
    const reserved = r.kind === "reserved";
    const qid = reserved ? r.q.task_id : null;
    const key = qid ? `q:${qid}` : `t:${t.line}`;
    const menuKey = qid ? `queue:${qid}` : `task:${t.line}`;
    const steps = qid
      ? { up: reservedStep(qid, "up") !== null, down: reservedStep(qid, "down") !== null }
      : { up: taskStep(t, "up") !== null, down: taskStep(t, "down") !== null };
    const step = (dir: "up" | "down") => {
      if (qid) return stepReserved(qid, dir);
      const op = taskStep(t, dir);
      if (op) act([op]);
    };
    return {
      t,
      row: r,
      stage: t.thread ? stages.get(t.thread) : undefined,
      overLimit: t.thread ? overLimits.get(t.thread) : undefined,
      depth: r.kind === "reserved" ? 0 : r.depth,
      state: taskState(t, groups),
      selected: !(reserved && r.missing) && current?.line === t.line,
      groups,
      canEdit,
      ordering: canEdit && ordering,
      steps,
      onStep: step,
      draggable: canEdit && !coarse && r.kind !== "under" && !(reserved && r.missing),
      dragging: drag !== null && (qid ? drag.qid === qid : drag.qid === null && drag.line === t.line),
      drop: drag?.over === key ? drag.side : undefined,
      flash: r.kind === "task" && flash !== null && t.id === flash,
      menuOpen: menu?.key === menuKey,
      onPick: () => (reserved && r.missing ? undefined : pick(t)),
      onThread,
      onMenu: (a: HTMLElement) => openMenu(menuKey, a, t.title, reserved ? reservedItems(r) : rowItems(t)),
      onDragStart: (e: DragEvent<HTMLElement>) => {
        e.dataTransfer.effectAllowed = "move";
        e.dataTransfer.setData("text/plain", t.title);
        setDrag({ line: t.line, qid, over: null, side: "before" });
      },
      onDragEnd: () => setDrag(null),
      onDragOver: qid ? onReservedDragOver(qid) : r.kind === "task" ? onRowDragOver(t) : undefined,
      onDrop: qid ? onReservedDrop(qid) : r.kind === "task" ? onRowDrop(t) : undefined,
    };
  };

  const later = notesFiles.length > 0;
  return (
    <div
      className={`tasks-view${shown ? "" : " is-hidden"}`}
      inert={!shown}
      data-testid="tasks-view"
      data-shown={shown ? "true" : "false"}
      data-tasks-sha={board?.sha256 ?? ""}
      data-can-edit={frontApi === null ? "pending" : canEdit ? "true" : "false"}
    >
      <header className="term-head">
        <DrawerToggle />
        <HeadNote />
        <div className="term-title">
          <strong>할 일</strong>
          <span className="muted">TASKS.md · {total}개</span>
        </div>
        <HeadMore />
      </header>
      {board && queueApi && <TaskQueuePanel board={board} canEdit={canEdit} onThread={onThread} />}
      <div className="tasks-toolbar">
        <div className="tasks-tabs" role="tablist" aria-label="할 일 탭">
          <button type="button" role="tab" aria-selected={tab === "tasks"} className={`tasks-tab${tab === "tasks" ? " is-on" : ""}`} data-testid="tasks-tab-tasks" onClick={() => setTab("tasks")}>
            할 일 <span className="tasks-count">{total}</span>
          </button>
          {later && (
            <button type="button" role="tab" aria-selected={tab === "later"} className={`tasks-tab${tab === "later" ? " is-on" : ""}`} data-testid="tasks-tab-later" onClick={() => setTab("later")}>
              나중
            </button>
          )}
          {editApi && (
            <button type="button" role="tab" aria-selected={tab === "log"} className={`tasks-tab${tab === "log" ? " is-on" : ""}`} data-testid="tasks-tab-log" onClick={() => setTab("log")}>
              <History aria-hidden="true" className="tasks-tab-icon" />
              기록
            </button>
          )}
        </div>
        {tab === "tasks" && (
          <>
            <label className="tasks-search">
              <Search aria-hidden="true" />
              <input className="input" type="search" value={query} placeholder="ID·제목·메모·담당 검색" aria-label="할 일 검색" data-testid="tasks-search" onChange={(e) => setQuery(e.target.value)} />
            </label>
            <div className="tasks-chips">
              <button
                type="button"
                className={`task-chip tasks-repo${repoOn !== null ? " is-on" : ""}`}
                aria-haspopup="menu"
                aria-expanded={menu?.key === "repo"}
                data-testid="tasks-repo"
                data-repo={repoOn ?? ""}
                onClick={(e) => openMenu("repo", e.currentTarget, "저장소로 거르기", repoItems())}
              >
                {coarse ? (repoOn === null ? "저장소" : repoLabel) : `저장소: ${repoLabel}`}
                <ChevronDown aria-hidden="true" />
              </button>
              <div className="tasks-filters" role="group" aria-label="거르기">
                {TASK_FILTERS.filter((f) => f.id !== "reserved" || queueApi).map((f) => (
                  <button key={f.id} type="button" className={`task-chip tasks-filter tasks-filter-${f.id}${filter === f.id ? " is-on" : ""}`} aria-pressed={filter === f.id} data-testid={`tasks-filter-${f.id}`} onClick={() => setFilter(f.id)}>
                    {f.id === "reserved" && <span className="task-reserved-dot is-small" aria-hidden="true" />}
                    {f.label}
                    {model && <span className="tasks-count">{model.counts[f.id]}</span>}
                  </button>
                ))}
              </div>
            </div>
          </>
        )}
        {canEdit && tab === "tasks" && (
          <div className="tasks-actions">
            {coarse && (
              <button type="button" className={`btn btn-ghost tasks-order${ordering ? " is-on" : ""}`} aria-pressed={ordering} data-testid="tasks-order" onClick={() => setOrdering((o) => !o)}>
                <ArrowUpDown aria-hidden="true" />
                {ordering ? "순서 끝" : "순서"}
              </button>
            )}
            <button type="button" className="btn btn-ghost tasks-raw-button" title="TASKS.md 원문 편집(고급)" aria-label="TASKS.md 원문 편집" data-testid="tasks-raw" onClick={() => setDialog({ kind: "raw" })}>
              <FileCode aria-hidden="true" />
            </button>
            <button
              type="button"
              className="btn btn-primary tasks-add"
              data-testid="tasks-add"
              onClick={() => {
                setPicked(null);
                setCreating(emptyDraft(defaultList(board)));
              }}
            >
              <Plus aria-hidden="true" />할 일
            </button>
          </div>
        )}
      </div>
      {error && (
        <div className="banner err" role="alert" data-testid="tasks-error">
          {error}
        </div>
      )}
      {started && (
        <div className="banner task-started" role="status" data-testid="task-started">
          <span>
            {started.id}를 시작했다{started.warnings.length > 0 ? `: ${started.warnings.join(" · ")}` : ". task 줄 끝에 이었다"}
          </span>
          <button type="button" className="linkish" onClick={() => onThread(started.id)}>
            thread 열기
          </button>
          <button type="button" className="linkish" onClick={() => setStarted(null)}>
            닫기
          </button>
        </div>
      )}
      {opError && (
        <div className="banner err" role="alert" data-testid="tasks-op-error">
          {opError}
          <button type="button" className="linkish" onClick={() => setOpError(null)}>
            닫기
          </button>
        </div>
      )}
      {tab === "log" ? (
        <TaskLogView canEdit={canEdit} rev={sha} onRestore={(e) => restore(e.id, e.title)} />
      ) : tab === "later" ? (
        <div className="tasks-scroll" data-testid="tasks-later">
          {notesFiles.map((f) => (
            <NotesFile
              key={f.path}
              file={f}
              canEdit={canEdit}
              menuKey={menu?.key ?? null}
              onMenu={(key, anchor, item, section) =>
                openMenu(key, anchor, "나중 항목", [
                  ...(canEdit
                    ? [{ id: "to-task", label: "할 일로 옮기기", icon: ListPlus, run: () => {
                        const { title, notes } = itemToTask(item);
                        setTab("tasks");
                        setPicked(null);
                        setCreating({ ...emptyDraft(defaultList(board)), title, notes: [...notes, `(나중 ${f.path} · ${section || "머리 없음"}에서 옮김)`].join("\n") });
                      } }]
                    : []),
                  { id: "copy", label: "복사", icon: Copy, run: () => void copyText(item.join("\n")) },
                ])
              }
            />
          ))}
        </div>
      ) : (
        <div className={`tasks-main${detailOpen ? " has-detail" : ""}`}>
          <div className="tasks-scroll" data-testid="tasks-list">
            {target !== null && board !== null && !linkedTask && <p className="tasks-empty" role="status" data-testid="task-missing">할 일 {target}: 없음</p>}
            {board === null && !error && <p className="tasks-empty">불러오는 중</p>}
            {board && total === 0 && <p className="tasks-empty">{canEdit ? "할 일이 없다. 「+ 할 일」로 더한다" : "할 일이 없다"}</p>}
            {board && total > 0 && shownCount === 0 && filter !== "reserved" && <p className="tasks-empty">맞는 할 일이 없다</p>}
            {board && model && filter === "reserved" && model.counts.reserved === 0 && (
              <p className="tasks-empty">예약이 없다{canEdit ? ". 할 일을 맨 위로 끌거나 ⋯ 「자리 나면 시작 예약」으로 예약한다" : ""}</p>
            )}
            {model && model.top.length > 0 && (
              <ul className="task-rows task-reserved-rows" aria-label="예약(위에서부터 차례로 시작)" data-testid="tasks-reserved">
                {model.top.map((r) =>
                  r.kind === "gap" ? (
                    <li key={r.key} className="task-gap" data-testid="task-gap">
                      다른 저장소 예약 {r.n}개 숨김 ({r.repos.join("·")}) · 차례는 그대로
                    </li>
                  ) : (
                    <TaskRow key={r.key} {...rowProps(r)} />
                  ),
                )}
              </ul>
            )}
            {/* 예약과 일반 할 일 사이 점선: 일반 행을 여기 놓으면 예약 맨 뒤(끄는 동안 자리를 늘리지 않는다: 놓을 곳이 밀리지 않게).
                예약이 없으면 맨 위에 같은 줄을 두어 첫 예약을 끌어 넣는다 */}
            {model && model.rest.length > 0 && (model.top.length > 0 || (canEdit && queueApi && !coarse && filter === "all")) && (
              <div
                className={`task-sep${model.top.length === 0 ? " is-empty" : ""}${drag?.over === "res-end" ? " is-over" : ""}`}
                role="separator"
                title="위는 예약 차례, 아래는 할 일 차례"
                data-testid="task-res-end"
                onDragOver={onResEndDragOver}
                onDrop={onResEndDrop}
              >
                {model.top.length === 0 && <span>예약 없음 · 할 일을 여기로 끌면 자리 나면 시작 예약</span>}
              </div>
            )}
            {model && model.rest.length > 0 && (
              <div className={`task-zone${drag?.over === "rest" ? " is-unreserve" : ""}`} onDragOver={onRestDragOver} onDrop={onRestDrop}>
                {drag?.over === "rest" && <p className="task-zone-note">여기 놓으면 예약 풀기 · 원래 자리로</p>}
                <ul className="task-rows" data-testid="tasks-rest">
                  {model.rest.map((r) => (r.kind === "gap" ? null : <TaskRow key={r.key} {...rowProps(r)} />))}
                </ul>
              </div>
            )}
            {model && filter === "reserved" && model.counts.reserved > 0 && (
              <p className="tasks-tip">예약만 보는 중 · 위에서부터 차례로 시작한다. {coarse ? "⋯ 또는 「순서」로" : "끌어서"} 예약 차례를 바꾼다. 「전체」로 돌아가면 같은 차례 그대로 맨 위에 있다.</p>
            )}
            {repoOn !== null && (
              <p className="tasks-tip" data-testid="tasks-repo-tip">
                {repoLabel}만 보는 중 · 옮기면 보이는 행 바로 위·아래로 들어간다. 숨은 다른 저장소 할 일의 차례는 그대로다.
              </p>
            )}
            {board && board.unlinked_threads.length > 0 && (
              <Unlinked
                overLimits={overLimits}
                threads={board.unlinked_threads}
                canLink={linkApi}
                onThread={onThread}
                onLink={(thread) => setDialog({ kind: "link", thread })}
                onMake={(thread) => {
                  setPicked(null);
                  setCreating({ ...emptyDraft(defaultList(board)), title: thread.title, thread: thread.id });
                }}
              />
            )}
          </div>
          {board && detailOpen ? (
            <TaskDetail
              key={picked ? `${picked.line}:${picked.title}:${saves}` : `new:${saves}:${creating?.list}:${creating?.parent?.block_sha256}:${creating?.title}`}
              t={current}
              gone={gone}
              fileOpen={fileOpen}
              onDirty={setFormDirty}
              board={board}
              byLine={byLine}
              groups={groups}
              canEdit={canEdit}
              owners={owners}
              onRefreshOwners={loadOwners}
              start={creating}
              log={log}
              onThread={onThread}
              onPick={pick}
              onClose={closeDetail}
              onSave={onSave}
              onDelete={remove}
              linkApi={linkApi}
              attachApi={attachApi}
              repoApi={repoApi}
              queueApi={queueApi}
              reserveRequest={reserveReq}
              onQueueStep={(t, dir) => t.id && stepReserved(t.id, dir)}
              onUnreserved={(t) => t.id && setFlash(t.id)}
              onChangeNotes={changeTaskNotes}
              onOpenAttachment={
                onOpenAttachment
                  ? (a) => {
                      setOpError(null);
                      onOpenAttachment(board.project, a).catch((e: unknown) => setOpError(`문서를 열지 못했다: ${e instanceof Error ? e.message : String(e)}`));
                    }
                  : undefined
              }
              onHandOff={onHandOff ? (t, again) => onHandOff(handOffText(t, again)) : undefined}
              onStart={(t) => setDialog({ kind: "start", task: t })}
            />
          ) : (
            <aside className="task-detail is-empty" aria-hidden="true">
              <p className="tasks-empty">{canEdit ? "task를 고르면 여기서 고친다" : "task를 고르면 메모·원문 줄·형식 어긋남이 여기 보인다"}</p>
            </aside>
          )}
          {detailOpen && <div className="tasks-scrim" aria-hidden="true" onClick={closeDetail} />}
          {canEdit && !detailOpen && !ordering && (
            <button
              type="button"
              className="tasks-fab"
              aria-label="할 일 더하기"
              data-testid="tasks-fab"
              onClick={() => {
                setPicked(null);
                setCreating(emptyDraft(defaultList(board)));
              }}
            >
              <Plus aria-hidden="true" />
            </button>
          )}
        </div>
      )}
      {!undo && autoNews[0] && shown && (
        <div className="task-undo task-auto-clean" role="status" data-testid="task-auto-clean" data-log-id={autoNews[0].id}>
          <span className="task-undo-text">
            자동 정리: 「{autoNews[0].title}」{autoNews[0].thread ? `(${autoNews[0].thread} 완료)` : ""}
            {autoNews.length > 1 ? ` 외 ${autoNews.length - 1}개` : ""}
          </span>
          {canEdit && (
          <button
            type="button"
            className="btn btn-ghost"
            data-testid="task-auto-clean-restore"
            onClick={() => {
              const e = autoNews[0];
              void restore(e.id, e.title).then(
                () => seeAuto(Math.max(autoSeen ?? 0, e.id)),
                (err: unknown) => setOpError(err instanceof Error ? err.message : String(err)),
              );
            }}
          >
            되살리기
          </button>
          )}
          <button type="button" className="task-undo-close" aria-label="닫기" data-testid="task-auto-clean-close" onClick={() => seeAuto(Math.max(autoSeen ?? 0, autoNews[0].id))}>
            <X aria-hidden="true" />
          </button>
        </div>
      )}
      {undo && shown && (
        <div className="task-undo" role="status" data-testid="task-undo">
          <span className="task-undo-text">「{undo.title}」를 지웠다</span>
          <button type="button" className="btn btn-ghost" data-testid="task-undo-restore" onClick={() => void restore(undo.logId, undo.title).catch((e: unknown) => setOpError(e instanceof Error ? e.message : String(e)))}>
            되살리기
          </button>
          <button type="button" className="task-undo-close" aria-label="닫기" onClick={() => setUndo(null)}>
            <X aria-hidden="true" />
          </button>
        </div>
      )}
      {menu && <RowMenu anchor={menu.anchor} title={menu.title} items={menu.items} onClose={() => setMenu(null)} />}
      {dialog?.kind === "conflict" && (
        <ConflictDialog
          conflict={dialog.conflict}
          mine={dialog.mine}
          onClose={() => setDialog(null)}
          onReopen={() => {
            const latest = dialog.conflict.latest;
            setDialog(null);
            setCreating(null);
            if (latest) setPicked({ title: latest.title, line: latest.line });
            else setPicked(null);
            setSaves((n) => n + 1);
          }}
        />
      )}
      {dialog?.kind === "start" && (
        <StartDialog
          allowOverLimit={apiAtLeast(frontApi, [1, 98])}
          task={dialog.task}
          owners={owners}
          onClose={() => setDialog(null)}
          onStarted={(v: ThreadView) => {
            setDialog(null);
            setStarted({ id: v.id, warnings: v.warnings ?? [] });
          }}
        />
      )}
      {dialog?.kind === "link" && board && (
        <LinkDialog
          thread={dialog.thread}
          board={board}
          groups={groups}
          onClose={() => setDialog(null)}
          onLink={async (t) => {
            const r = await edit([linkOp(t, dialog.thread.id)], null);
            if (r) setPicked({ title: t.title, line: t.line });
          }}
        />
      )}
      {dialog?.kind === "doc-add" && board && (
        <DocAddDialog
          link={dialog.link}
          board={board}
          // 링크 저장이 충돌하면 그사이 충돌 창이 떠 있다: 이 창만 닫는다
          onClose={() => setDialog((d) => (d?.kind === "doc-add" ? null : d))}
          onNew={() => {
            const link = dialog.link;
            setDialog(null);
            setPicked(null);
            setCreating({ ...emptyDraft(defaultList(board)), title: docTaskTitle(link), notes: attachmentLine(link) });
          }}
          onLink={async (t) => {
            await changeTaskNotes(t, (notes) => addAttachment(notes, dialog.link));
            setCreating(null);
            setPicked({ title: t.title, line: t.line });
          }}
        />
      )}
      {dialog?.kind === "raw" && board && (
        <RawEditor
          text={board.text}
          sha256={board.sha256}
          onClose={() => setDialog(null)}
          onSave={async (text, sha256) => {
            const r = await call({ op: "task_edit", ops: [{ op: "raw_save", expected_sha256: sha256, text }], base_sha256: null });
            if (r.kind !== "task_edited") throw new Error(`예상하지 못한 응답: ${r.kind}`);
            onBoard(r.board);
            return r.conflict ? "conflict" : "saved";
          }}
        />
      )}
    </div>
  );
}

/** 새 task의 목록: `Backlog`가 있으면 그것, 없으면 첫 목록 */
function defaultList(board: TaskBoard | null): string {
  const lists = board?.lists ?? [];
  return lists.find((l) => l.name === "Backlog")?.name ?? lists.find((l) => l.name !== "")?.name ?? "Backlog";
}

function MenuButton({ title, open, testid, onToggle }: { title: string; open: boolean; testid: string; onToggle: (anchor: HTMLElement) => void }) {
  return (
    <button
      type="button"
      className="task-menu-button"
      aria-label={`${title} 더보기`}
      aria-haspopup="menu"
      aria-expanded={open}
      data-testid={testid}
      onClick={(e) => {
        e.stopPropagation();
        onToggle(e.currentTarget);
      }}
    >
      <Ellipsis aria-hidden="true" />
    </button>
  );
}

type RowProps = {
  t: TaskView;
  row: Exclude<TaskListRow, { kind: "gap" }>;
  stage?: ThreadStage;
  overLimit?: ThreadLimitOverride;
  depth: number;
  state: TaskState;
  selected: boolean;
  groups: ThreadGroups;
  canEdit: boolean;
  /** 휴대폰 「⇅ 순서」: ⋯ 대신 ▲▼ */
  ordering: boolean;
  steps: { up: boolean; down: boolean };
  onStep: (dir: "up" | "down") => void;
  draggable: boolean;
  dragging: boolean;
  drop: "before" | "after" | undefined;
  /** 예약을 풀어 원래 자리로 막 돌아왔다 */
  flash: boolean;
  menuOpen: boolean;
  onPick: () => void;
  onThread: (id: string) => void;
  onMenu: (anchor: HTMLElement) => void;
  onDragStart: (e: DragEvent<HTMLElement>) => void;
  onDragEnd: () => void;
  onDragOver?: (e: DragEvent<HTMLElement>) => void;
  onDrop?: (e: DragEvent<HTMLElement>) => void;
};

/** 한 목록의 행: 예약은 숫자·까닭 글 없이 보라 표지(확인 필요는 노랑)만, 하위만 예약했으면 상위 경로를 붙인다. 일반 행은 상태 아이콘 */
function TaskRow({ t, row, stage, overLimit, depth, state, selected, groups, canEdit, ordering, steps, onStep, draggable, dragging, drop, flash, menuOpen, onPick, onThread, onMenu, onDragStart, onDragEnd, onDragOver, onDrop }: RowProps) {
  const attachments = attachmentsOf(t.notes.join("\n")).length;
  const reserved = row.kind === "reserved";
  const check = reserved && row.q.state === "check";
  return (
    <li
      className={`task-row is-${state}${reserved ? " is-reserved" : ""}${row.kind === "under" ? " is-under" : ""}${selected ? " is-selected" : ""}${t.done ? " is-done" : ""}${dragging ? " is-dragging" : ""}${drop ? ` is-drop-${drop}` : ""}${flash ? " is-flash" : ""}`}
      style={{ paddingLeft: `calc(${depth} * var(--space-5))` }}
      data-task-state={state}
      data-kind={row.kind}
      data-testid={reserved && row.missing ? `task-q-${row.q.task_id}` : `task-${t.line}`}
      data-title={t.title}
      data-short-id={t.short_id ?? ""}
      onDragOver={onDragOver}
      onDrop={onDrop}
    >
      {draggable && (
        <span className="task-grip" draggable onDragStart={onDragStart} onDragEnd={onDragEnd} aria-hidden="true" title={reserved ? "끌어서 예약 차례 바꾸기 · 아래로 끌면 예약 풀기" : "끌어서 옮기기 · 맨 위로 끌면 예약"} data-testid={`task-grip-${t.line}`}>
          <GripVertical />
        </span>
      )}
      <button
        type="button"
        className="task-pick"
        aria-current={selected ? "true" : undefined}
        onClick={onPick}
        onKeyDown={(e) => {
          // herdr-web-ui Sidebar: Alt+↑↓로 순서 바꾸기(끌기의 키보드 대안). 예약 행은 예약 차례다
          if (!canEdit || !e.altKey || (e.key !== "ArrowUp" && e.key !== "ArrowDown")) return;
          e.preventDefault();
          onStep(e.key === "ArrowUp" ? "up" : "down");
        }}
      >
        <ThreadStageBadge stage={stage} /><ThreadLimitBadge start={overLimit} />
        {reserved ? <span className={`task-reserved-dot${check ? " is-check" : ""}`} role="img" aria-label={check ? "예약 · 확인 필요" : "예약됨"} title={check ? "예약 · 확인 필요(상세에 까닭)" : "예약됨 · 자리 나면 시작"} /> : <StateIcon state={state} />}
        {reserved && row.crumb && (
          <span className="task-crumb" data-testid="task-crumb">
            {row.crumb.short_id ?? ""} {row.crumb.title} ›
          </span>
        )}
        <span className="task-title">
          <span className="sr-only">{reserved ? (check ? "예약 · 확인 필요" : "예약") : TASK_STATE_LABEL[state]}: </span>
          {t.short_id && <span className="task-short-id" data-testid="task-short-id">{t.short_id}</span>}
          {t.title}
        </span>
        {reserved && row.missing && <span className="task-chip task-problem">task 없음</span>}
        {row.kind === "task" && row.reservedKids > 0 && <span className="task-chip task-kids-reserved">하위 {row.reservedKids} 예약됨</span>}
        {flash && <span className="task-flash-note" data-testid="task-flash">↩ 원래 자리로 돌아옴</span>}
        {state === "problem" && <span className="task-chip task-problem">형식 어긋남</span>}
        {state === "check" && <span className="task-chip task-check">확인 필요</span>}
        <WhoChips t={t} />
        {t.repo && (
          <span className="task-chip task-repo" title="저장소">
            {t.repo}
          </span>
        )}
        {withoutAttachments(t.notes.join("\n")) !== "" && (
          <span className="task-notes-mark" role="img" aria-label="메모 있음" title="메모 있음">
            ≡
          </span>
        )}
        {attachments > 0 && (
          <span className="task-attach-mark" role="img" aria-label={`첨부 ${attachments}개`} title={`첨부 ${attachments}개`} data-testid="task-attach-mark">
            <Paperclip aria-hidden="true" />
            {attachments}
          </span>
        )}
      </button>
      {t.thread && <ThreadChip id={t.thread} groups={groups} onThread={onThread} />}
      {ordering ? (
        <span className="task-steps">
          <button type="button" className={`task-step${reserved ? " is-reserved" : ""}`} aria-label={reserved ? "예약 위로" : "위로"} disabled={!steps.up} data-testid={`task-step-up-${t.line}`} onClick={() => onStep("up")}>
            <ArrowUp aria-hidden="true" />
          </button>
          <button type="button" className={`task-step${reserved ? " is-reserved" : ""}`} aria-label={reserved ? "예약 아래로" : "아래로"} disabled={!steps.down} data-testid={`task-step-down-${t.line}`} onClick={() => onStep("down")}>
            <ArrowDown aria-hidden="true" />
          </button>
        </span>
      ) : (
        canEdit && <MenuButton title={t.title} open={menuOpen} testid={reserved ? `task-qmenu-${t.line}` : `task-menu-${t.line}`} onToggle={onMenu} />
      )}
    </li>
  );
}

function Unlinked({ overLimits, threads, canLink, onThread, onLink, onMake }: { overLimits: ReadonlyMap<string, ThreadLimitOverride | undefined>; threads: TaskThread[]; canLink: boolean; onThread: (id: string) => void; onLink: (t: TaskThread) => void; onMake: (t: TaskThread) => void }) {
  return (
    <section className="task-list task-unlinked" aria-label="task 없는 열린 thread" data-testid="tasks-unlinked">
      <h3 className="task-list-head">
        <span className="task-list-static">
          task 없는 열린 thread <span className="tasks-count">{threads.length}</span>
        </span>
      </h3>
      <ul className="task-rows">
        {threads.map((t) => (
          <li key={t.id} className="task-row" data-testid={`task-unlinked-row-${t.id}`}>
            <button type="button" className="task-pick" data-testid={`tasks-unlinked-${t.id}`} onClick={() => onThread(t.id)}>
              <ThreadStageBadge stage={t.stage} /><ThreadLimitBadge start={overLimits.get(t.id)} />
              <CircleDot className="task-state is-run" aria-hidden="true" />
              <span className="task-title">
                {t.id} · {t.title}
              </span>
              <span className={`task-chip task-thread is-${t.group}`}>{GROUP_WORD[t.group as ThreadGroup] ?? t.group}</span>
            </button>
            {canLink && (
              <span className="task-unlinked-actions">
                <button type="button" className="btn btn-ghost task-unlinked-action" title="이 thread를 있는 task에 잇는다" data-testid={`task-unlinked-link-${t.id}`} onClick={() => onLink(t)}>
                  <Link2 aria-hidden="true" />
                  <span>task에 잇기</span>
                </button>
                <button type="button" className="btn btn-ghost task-unlinked-action" title="이 thread를 이은 새 task를 만든다" data-testid={`task-unlinked-make-${t.id}`} onClick={() => onMake(t)}>
                  <Plus aria-hidden="true" />
                  <span>task로 만들기</span>
                </button>
              </span>
            )}
          </li>
        ))}
      </ul>
    </section>
  );
}

/** 「나중」 파일 하나: `##` 절을 접는 묶음, 절 안은 항목마다(Markdown 그대로) ⋯ 메뉴 「할 일로 옮기기」·「복사」(t-0469 §3.6) */
function NotesFile({ file, canEdit, menuKey, onMenu }: { file: TaskNotesFile; canEdit: boolean; menuKey: string | null; onMenu: (key: string, anchor: HTMLElement, item: string[], section: string) => void }) {
  return (
    <section className="task-notes-file" aria-label={file.path} data-testid={`tasks-notes-${file.path}`}>
      <h3 className="task-list-head">
        <span className="task-list-static">{file.path}</span>
      </h3>
      {file.error ? (
        <p className="tasks-empty" data-testid="tasks-notes-error">
          {file.error}
        </p>
      ) : (
        file.sections.map((s, i) => {
          const items = noteItems(s.lines);
          return (
            <details key={`${i}:${s.name}`} className="task-notes-section">
              <summary>
                <ChevronRight className="task-notes-chevron" aria-hidden="true" />
                <span className="task-list-name">{s.name || "(머리 없음)"}</span>
                <span className="tasks-count">{items.length}</span>
              </summary>
              {items.length > 0 ? (
                <ul className="task-notes-items">
                  {items.map((item, k) => {
                    const key = `later:${file.path}:${i}:${k}`;
                    return (
                      <li key={k} className="task-notes-item" data-testid={`tasks-notes-item-${i}-${k}`}>
                        <Markdown className="task-notes">{item.join("\n")}</Markdown>
                        <MenuButton title={canEdit ? "나중 항목" : "나중 항목 복사"} open={menuKey === key} testid={`tasks-notes-menu-${i}-${k}`} onToggle={(a) => onMenu(key, a, item, s.name)} />
                      </li>
                    );
                  })}
                </ul>
              ) : (
                <p className="muted">비었음</p>
              )}
            </details>
          );
        })
      )}
    </section>
  );
}

// 본문(오른쪽) 할 일 화면(할 일 보기 T1 §8.306, 편집 T2 §8.311, 사용자 10-20 「하기로 했던 기능들을 다 볼수 있고 (왼쪽에 아이콘이
// 있어서 거기서 들어가게), 필요하면 수정, 삭제 할수 있고」). 활동 막대 「할 일」·`#tasks`가 연다. host `tasks`(TaskBoard)를 읽고
// `task_edit`로 고친다(바뀐 블록만 쓰고 블록 hash로 충돌을 감지한다). thread 연결(T3, §8.316): 상세의 「coordinator에게 맡기기」·
// 「바로 시작」(TaskStart.tsx), 「task 없는 열린 thread」의 「task에 잇기」·「task로 만들기」, 끝난 thread의 자동 정리 알림 줄(되살리기).
// 행 모양과 상태 아이콘은 t3code ComposerTasksBadge.tsx, 목록 머리(이름·수)와 메모 표지 `≡`은 herdr-projects popup.rs `build`.
// 행·목록 머리의 ⋯ 메뉴는 herdr-web-ui RowMenu(휴대폰은 아래 시트), 끌기는 herdr-web-ui Sidebar.tsx 483–491의 HTML 끌어 놓기와
// Alt+↑↓(580–584), 휴대폰은 끌기 대신 메뉴의 위로·아래로·맨 위로(t-0469 §3.3). 지운 뒤 「되살리기」 줄은 t3code
// showThreadUndoNotice.ts(되살리기 한 번만)를 따르되 시간으로 사라지지 않는다(기록 화면에도 남는다).
import { useCallback, useEffect, useMemo, useRef, useState, type DragEvent } from "react";
import {
  ArrowDown,
  ArrowDownToLine,
  ArrowUp,
  ArrowUpToLine,
  ChevronDown,
  ChevronRight,
  CircleDot,
  Copy,
  CornerDownRight,
  Ellipsis,
  FileCode,
  FolderInput,
  GitMerge,
  GripVertical,
  History,
  Link2,
  ListPlus,
  Paperclip,
  Pencil,
  Plus,
  Search,
  Trash2,
  X,
  type LucideIcon,
} from "lucide-react";
import type { OwnerChoice, TaskBoard, TaskConflict, TaskLogEntry, TaskNotesFile, TaskOp, TaskThread, TaskView, ThreadGroup, ThreadView } from "./gen/proto";
import { ApiError, call, me, onTasksChanged } from "./api";
import { DrawerToggle, HeadMore, HeadNote } from "./drawer";
import { Markdown } from "./Markdown";
import { GROUP_WORD } from "./sidebarModel";
import { RowMenu, type RowMenuItem } from "./RowMenu";
import { copyText } from "./herdr/clipboard";
import { useMediaQuery } from "./herdr/useMediaQuery";
import { StateIcon, TaskDetail, ThreadChip, WhoChips, type SaveResult } from "./TaskDetail";
import { ConflictDialog, NameDialog, RawEditor, type MyEdit } from "./TaskDialogs";
import { DocAddDialog, LinkDialog, StartDialog } from "./TaskStart";
import { TaskLogView } from "./TaskLogView";
import {
  TASK_FILTERS,
  TASK_STATE_LABEL,
  anchorOf,
  dropOp,
  emptyDraft,
  findTask,
  handOffText,
  itemToTask,
  lastLogId,
  linkOp,
  mineKeys,
  moveOp,
  newAutoCleans,
  noteItems,
  taskDepth,
  taskSections,
  taskState,
  toListOp,
  addAttachment,
  attachmentLine,
  attachmentsOf,
  docTaskTitle,
  noteLines,
  withoutAttachments,
  type TaskAttachment,
  type TaskDraft,
  type TaskFilter,
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
  | { kind: "list-add"; after: string | null }
  | { kind: "list-rename"; name: string }
  | { kind: "raw" }
  | { kind: "start"; task: TaskView }
  | { kind: "link"; thread: TaskThread }
  | { kind: "doc-add"; link: TaskAttachment };

/** 할 일 화면 밖에서 여는 새 task: 명령 팔레트 「할 일 더하기」(link 없음), 보고서·thread 결과물 「할 일에 추가」(그 문서, plan §8.327) */
export type TaskAddRequest = { seq: number; link: TaskAttachment | null };

export function TasksView({
  shown,
  board,
  error,
  groups,
  reader,
  addRequest,
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
  /** 보기 권한 기기: 편집 단추·끌기·「+」를 숨긴다 */
  reader: boolean;
  /** 바뀌면 새 task 폼(문서가 있으면 「할 일에 추가」 창)을 연다 */
  addRequest: TaskAddRequest;
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
  const [folded, setFolded] = useState<ReadonlySet<string>>(new Set());
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
  const [drag, setDrag] = useState<{ line: number; over: string | null; side: "before" | "after" } | null>(null);
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
  // owner 고르기 목록(머신마다 묻는다): 편집할 수 있을 때 화면을 열면 한 번
  useEffect(() => {
    if (!shown || !canEdit || owners !== null) return;
    call({ op: "assignable", check: null })
      .then((r) => setOwners(r.kind === "assignable" ? r.owners : []))
      .catch(() => setOwners([]));
  }, [shown, canEdit, owners]);
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
  const byLine = useMemo(() => new Map((board?.tasks ?? []).map((t) => [t.line, t])), [board]);
  const sections = useMemo(() => (board ? taskSections(board, filter, query, groups) : []), [board, filter, query, groups]);
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
  const shownCount = sections.reduce((n, s) => n + s.tasks.length, 0);
  const total = board?.tasks.length ?? 0;
  const toggle = (name: string) =>
    setFolded((f) => {
      const next = new Set(f);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });
  const pick = (t: TaskView) => {
    setCreating(null);
    setPicked({ title: t.title, line: t.line });
  };
  const closeDetail = useCallback(() => {
    setPicked(null);
    setCreating(null);
  }, []);

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
  const lists = board?.lists ?? [];
  const named = lists.filter((l) => l.name !== "");
  const rowItems = (t: TaskView): RowMenuItem[] => {
    if (!board) return [];
    const item = (id: string, label: string, icon: LucideIcon, op: TaskOp | null): RowMenuItem[] => (op ? [{ id, label, icon, run: () => act([op]) }] : []);
    const others = lists.filter((l) => l.name !== t.list || t.parent !== null);
    return [
      ...item("up", "위로", ArrowUp, moveOp(board, t, "up")),
      ...item("down", "아래로", ArrowDown, moveOp(board, t, "down")),
      ...item("top", "맨 위로", ArrowUpToLine, moveOp(board, t, "top")),
      ...others.map((l, i) => ({ id: `to-${l.name}`, label: `${l.name || "(목록 없음)"}(으)로 옮기기`, icon: FolderInput, divider: i === 0, run: () => act([toListOp(t, l.name)!]) })),
      { id: "child", label: "하위 task 더하기", icon: CornerDownRight, divider: true, run: () => {
        setPicked(null);
        setCreating(emptyDraft(t.list, anchorOf(t)));
      } },
      { id: "delete", label: "끝남으로 지우기", icon: Trash2, danger: true, run: () => void remove(t).catch((e: unknown) => setOpError(e instanceof Error ? e.message : String(e))) },
    ];
  };
  const listItems = (name: string): RowMenuItem[] => {
    const at = named.findIndex((l) => l.name === name);
    const items: RowMenuItem[] = [{ id: "add", label: "이 목록에 할 일 더하기", icon: Plus, run: () => {
      setPicked(null);
      setCreating(emptyDraft(name));
    } }];
    if (name === "") return items;
    items.push({ id: "rename", label: "이름 바꾸기", icon: Pencil, run: () => setDialog({ kind: "list-rename", name }) });
    if (at > 0) items.push({ id: "up", label: "목록 위로", icon: ArrowUp, run: () => act([{ op: "list_move", name, before: named[at - 1].name }]) });
    if (at < named.length - 1) items.push({ id: "down", label: "목록 아래로", icon: ArrowDownToLine, run: () => act([{ op: "list_move", name, before: named[at + 2]?.name ?? null }]) });
    named.filter((l) => l.name !== name).forEach((l, i) => items.push({ id: `merge-${l.name}`, label: `${l.name}에 합치기`, icon: GitMerge, divider: i === 0, run: () => act([{ op: "list_merge", name, into: l.name }]) }));
    items.push({ id: "list-add", label: "이 뒤에 새 목록", icon: ListPlus, divider: true, run: () => setDialog({ kind: "list-add", after: name }) });
    if (!board?.tasks.some((t) => t.list === name)) items.push({ id: "delete", label: "빈 목록 지우기", icon: Trash2, danger: true, run: () => act([{ op: "list_delete", name }]) });
    return items;
  };

  // 끌어 놓기(넓은 화면·마우스): 행 위 절반이면 앞, 아래 절반이면 뒤. 목록 머리에 놓으면 그 목록 끝
  const dragged = drag ? byLine.get(drag.line) : undefined;
  const onRowDragOver = (t: TaskView) => (e: DragEvent<HTMLElement>) => {
    if (!dragged || !board) return;
    const r = e.currentTarget.getBoundingClientRect();
    const side = e.clientY < r.top + r.height / 2 ? "before" : "after";
    if (!dropOp(board, dragged, t, side)) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = "move";
    if (drag?.over !== `task:${t.line}` || drag.side !== side) setDrag({ line: dragged.line, over: `task:${t.line}`, side });
  };
  const onRowDrop = (t: TaskView) => (e: DragEvent<HTMLElement>) => {
    e.preventDefault();
    const op = dragged && board && drag ? dropOp(board, dragged, t, drag.side) : null;
    setDrag(null);
    if (op) act([op]);
  };
  const onListDragOver = (name: string) => (e: DragEvent<HTMLElement>) => {
    if (!dragged || !toListOp(dragged, name)) return;
    e.preventDefault();
    if (drag?.over !== `list:${name}`) setDrag({ line: dragged.line, over: `list:${name}`, side: "after" });
  };
  const onListDrop = (name: string) => (e: DragEvent<HTMLElement>) => {
    e.preventDefault();
    const op = dragged ? toListOp(dragged, name) : null;
    setDrag(null);
    if (op) act([op]);
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
              <input className="input" type="search" value={query} placeholder="제목·메모·담당 검색" aria-label="할 일 검색" data-testid="tasks-search" onChange={(e) => setQuery(e.target.value)} />
            </label>
            <div className="tasks-filters" role="group" aria-label="거르기">
              {TASK_FILTERS.map((f) => (
                <button key={f.id} type="button" className={`task-chip tasks-filter${filter === f.id ? " is-on" : ""}`} aria-pressed={filter === f.id} data-testid={`tasks-filter-${f.id}`} onClick={() => setFilter(f.id)}>
                  {f.label}
                </button>
              ))}
            </div>
          </>
        )}
        {canEdit && tab === "tasks" && (
          <div className="tasks-actions">
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
            {board === null && !error && <p className="tasks-empty">불러오는 중</p>}
            {board && total === 0 && <p className="tasks-empty">{canEdit ? "할 일이 없다. 「+ 할 일」로 더한다" : "할 일이 없다"}</p>}
            {board && total > 0 && shownCount === 0 && <p className="tasks-empty">맞는 할 일이 없다</p>}
            {sections.map((s) => {
              const closed = folded.has(s.name);
              // 거르는 동안 빈 목록은 숨긴다(전체·빈 검색이면 task 없는 목록도 머리를 보인다)
              if (s.tasks.length === 0 && (filter !== "all" || query.trim() !== "")) return null;
              const menuKey = `list:${s.name}`;
              return (
                <section key={s.name} className={`task-list${drag?.over === menuKey ? " is-drop" : ""}`} aria-label={s.name || "목록 없음"} data-testid={`task-list-${s.name}`}>
                  <h3 className="task-list-head" onDragOver={onListDragOver(s.name)} onDrop={onListDrop(s.name)}>
                    <button type="button" className="task-list-toggle" aria-expanded={!closed} onClick={() => toggle(s.name)}>
                      {closed ? <ChevronRight aria-hidden="true" /> : <ChevronDown aria-hidden="true" />}
                      <span className="task-list-name">{s.name || "(목록 없음)"}</span>
                      <span className="tasks-count">{s.tasks.length === s.total ? s.total : `${s.tasks.length}/${s.total}`}</span>
                    </button>
                    {canEdit && <MenuButton title={`${s.name || "목록 없음"} 목록`} open={menu?.key === menuKey} testid={`task-list-menu-${s.name}`} onToggle={(a) => openMenu(menuKey, a, `${s.name || "(목록 없음)"} 목록`, listItems(s.name))} />}
                  </h3>
                  {!closed && (
                    <>
                      {s.notes.map((n, i) => (
                        <p key={i} className="task-list-note">
                          {n}
                        </p>
                      ))}
                      <ul className="task-rows">
                        {s.tasks.map((t) => (
                          <TaskRow
                            key={`${t.line}:${t.title}`}
                            t={t}
                            depth={taskDepth(t, byLine)}
                            state={taskState(t, groups)}
                            selected={current?.line === t.line}
                            groups={groups}
                            canEdit={canEdit}
                            draggable={canEdit && !coarse}
                            dragging={drag?.line === t.line}
                            drop={drag?.over === `task:${t.line}` ? drag.side : undefined}
                            menuOpen={menu?.key === `task:${t.line}`}
                            onPick={() => pick(t)}
                            onThread={onThread}
                            onMenu={(a) => openMenu(`task:${t.line}`, a, t.title, rowItems(t))}
                            onKeyMove={(dir) => {
                              const op = board && moveOp(board, t, dir);
                              if (op) act([op]);
                            }}
                            onDragStart={(e) => {
                              e.dataTransfer.effectAllowed = "move";
                              e.dataTransfer.setData("text/plain", t.title);
                              setDrag({ line: t.line, over: null, side: "before" });
                            }}
                            onDragEnd={() => setDrag(null)}
                            onDragOver={onRowDragOver(t)}
                            onDrop={onRowDrop(t)}
                          />
                        ))}
                      </ul>
                    </>
                  )}
                </section>
              );
            })}
            {canEdit && board && filter === "all" && query.trim() === "" && (
              <button type="button" className="btn btn-ghost tasks-list-add" data-testid="tasks-list-add" onClick={() => setDialog({ kind: "list-add", after: null })}>
                <ListPlus aria-hidden="true" />새 목록
              </button>
            )}
            {board && board.unlinked_threads.length > 0 && (
              <Unlinked
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
          {canEdit && !detailOpen && (
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
      {(dialog?.kind === "list-add" || dialog?.kind === "list-rename") && (
        <NameDialog
          title={dialog.kind === "list-add" ? "새 목록" : `「${dialog.name}」 이름 바꾸기`}
          label="목록 이름"
          initial={dialog.kind === "list-rename" ? dialog.name : ""}
          action={dialog.kind === "list-add" ? "더하기" : "바꾸기"}
          onClose={() => setDialog(null)}
          onSave={async (name) => {
            const op: TaskOp = dialog.kind === "list-add" ? { op: "list_add", name, after: dialog.after } : { op: "list_rename", name: dialog.name, to: name };
            await edit([op], null);
          }}
        />
      )}
      {dialog?.kind === "start" && (
        <StartDialog
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

function TaskRow({
  t,
  depth,
  state,
  selected,
  groups,
  canEdit,
  draggable,
  dragging,
  drop,
  menuOpen,
  onPick,
  onThread,
  onMenu,
  onKeyMove,
  onDragStart,
  onDragEnd,
  onDragOver,
  onDrop,
}: {
  t: TaskView;
  depth: number;
  state: TaskState;
  selected: boolean;
  groups: ThreadGroups;
  canEdit: boolean;
  draggable: boolean;
  dragging: boolean;
  drop: "before" | "after" | undefined;
  menuOpen: boolean;
  onPick: () => void;
  onThread: (id: string) => void;
  onMenu: (anchor: HTMLElement) => void;
  onKeyMove: (dir: "up" | "down") => void;
  onDragStart: (e: DragEvent<HTMLElement>) => void;
  onDragEnd: () => void;
  onDragOver: (e: DragEvent<HTMLElement>) => void;
  onDrop: (e: DragEvent<HTMLElement>) => void;
}) {
  const attachments = attachmentsOf(t.notes.join("\n")).length;
  return (
    <li
      className={`task-row is-${state}${selected ? " is-selected" : ""}${t.done ? " is-done" : ""}${dragging ? " is-dragging" : ""}${drop ? ` is-drop-${drop}` : ""}`}
      style={{ paddingLeft: `calc(${depth} * var(--space-5))` }}
      data-task-state={state}
      data-testid={`task-${t.line}`}
      data-title={t.title}
      onDragOver={onDragOver}
      onDrop={onDrop}
    >
      {draggable && (
        <span className="task-grip" draggable onDragStart={onDragStart} onDragEnd={onDragEnd} aria-hidden="true" title="끌어서 옮기기" data-testid={`task-grip-${t.line}`}>
          <GripVertical />
        </span>
      )}
      <button
        type="button"
        className="task-pick"
        aria-current={selected ? "true" : undefined}
        onClick={onPick}
        onKeyDown={(e) => {
          // herdr-web-ui Sidebar: Alt+↑↓로 순서 바꾸기(끌기의 키보드 대안)
          if (!canEdit || !e.altKey || (e.key !== "ArrowUp" && e.key !== "ArrowDown")) return;
          e.preventDefault();
          onKeyMove(e.key === "ArrowUp" ? "up" : "down");
        }}
      >
        <StateIcon state={state} />
        <span className="task-title">
          <span className="sr-only">{TASK_STATE_LABEL[state]}: </span>
          {t.title}
        </span>
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
      {canEdit && <MenuButton title={t.title} open={menuOpen} testid={`task-menu-${t.line}`} onToggle={onMenu} />}
    </li>
  );
}

function Unlinked({ threads, canLink, onThread, onLink, onMake }: { threads: TaskThread[]; canLink: boolean; onThread: (id: string) => void; onLink: (t: TaskThread) => void; onMake: (t: TaskThread) => void }) {
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

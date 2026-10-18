// 생성 파일: crates/aaa-proto의 Rust 타입에서 만든다. 손으로 고치지 않는다.
// 갱신: AAA_UPDATE_TS=1 cargo test -p aaa-proto typescript

export type Version = { major: number, minor: number, };

export type B64 = string;

export type ErrorCode = "bad_request" | "unauthorized" | "forbidden" | "not_found" | "agent_disconnected" | "session_exited" | "unsupported_version" | "not_enrolled" | "enroll_closed" | "enroll_code_mismatch" | "name_taken" | "timeout" | "unavailable" | "internal" | "lease_held" | "subscription_not_found" | "subscription_gone" | "too_large";

export type ErrorBody = { code: ErrorCode, message: string, };

export type Outcome<T> = { "ok": T } | { "err": ErrorBody };

export type SessionState = { "state": "running" } | { "state": "exited", code: number | null, } | { "state": "interrupted" } | { "state": "lost" };

export type ScreenText = { session: string, cols: number, rows: number, 
/**
 * 보이는 화면의 글자(줄 끝 공백 제거)
 */
lines: Array<string>, cursor_row: number, cursor_col: number, 
/**
 * 보이는 화면을 SGR(굵게·흐리게·반전·색) escape와 함께 한 글로(`Vt::read` Visible·Ansi). herdr `agent read --format ansi`와 같은
 * 모양이라 herdr-projects `prompt_box`가 흐린 placeholder와 사람이 친 글을 가른다(plan §8.77). 옛 agent는 비워 보낸다.
 */
ansi: string, };

export type ReadSource = "visible" | "recent" | "recent-unwrapped" | "detection";

export type ReadFormat = "text" | "ansi";

export type ReadView = { pane: string, source: ReadSource, format: ReadFormat, text: string, truncated: boolean, };

export type ThreadPaneView = { project: string, thread: string, pane: string, 
/**
 * 읽거나 보내기 전 harness 상태(hook 없는 harness는 없다)
 */
agent: AgentActivity | null, read: ReadView | null, 
/**
 * 친 글자 수
 */
typed: number, 
/**
 * 누른 키
 */
pressed: Array<string>, };

export type OutputMatch = { "type": "substring", value: string, } | { "type": "regex", value: string, };

export type PaneDirection = "left" | "right" | "up" | "down";

export type WorkspaceSummary = { name: string, tabs: number, panes: number, };

export type ChatRole = "user" | "assistant" | "tool" | "thinking";

export type ChatMessage = { role: ChatRole, text: string, };

export type ChatView = { pane: string, harness: string, messages: Array<ChatMessage>, 
/**
 * 기록이 더 길어 끝부분만 실었다
 */
truncated: boolean, };

export type DeviceRole = "reader" | "controller" | "local";

export type Connection = "connected" | "disconnected";

export type MachineView = { id: string, name: string, local: boolean, connection: Connection, os: string | null, boot_id: string | null, dev_mode: boolean, last_seen_ms: number | null, 
/**
 * 마지막 건강 보고(§8.10). 연결 중이 아니면 오래된 값이다.
 */
health: MachineHealth | null, 
/**
 * agent 시계 - host 시계(보고를 받은 때 기준)
 */
clock_skew_ms: number | null, 
/**
 * 새 세션을 거부하는 이유(연결 중일 때만). 비었으면 정상이다.
 */
problems: Array<HealthProblem>, };

export type MachineHealth = { 
/**
 * agent 시계
 */
at_ms: number, 
/**
 * 작업 root가 있는 디스크의 여유·전체(알 수 없으면 None)
 */
disk_free_bytes: number | null, disk_total_bytes: number | null, 
/**
 * 1분 평균 부하 ×100과 CPU 수
 */
load_x100: number | null, cpus: number, 
/**
 * 전원에 연결됐는가(배터리가 없는 머신은 true, 알 수 없으면 None)
 */
on_ac: boolean | null, toolchain: Array<ToolCheck>, 
/**
 * agent가 본 이상(디스크 부족·toolchain 실패). 새 세션을 거부한다.
 */
problems: Array<HealthProblem>, 
/**
 * 거부하지는 않는 주의(배터리 전원 등)
 */
warnings: Array<string>, };

export type HealthProblem = { code: string, text: string, };

export type TaskView = { 
/**
 * 속한 `## ` 목록 이름
 */
list: string, title: string, 
/**
 * 괄호 안 owner 원문(`me`, `claude`, `codex@mac-b`, 사람 이름 등)
 */
owner: string | null, 
/**
 * ` · t-0001`로 붙은 thread
 */
thread: string | null, 
/**
 * task 줄 아래 들여쓴 메모
 */
notes: Array<string>, done: boolean, };

export type OwnerChoice = { machine: string, connected: boolean, 
/**
 * 끊긴 머신은 비어 있다(모른다)
 */
profiles: Array<string>, };

export type ProjectContext = { project: string, path: string, 
/**
 * project.toml `start_threads`: propose(제안하고 승인을 기다린다) 또는 auto. aaa는 강제하지 않는다.
 */
start_threads: string, tasks: Array<TaskView>, 
/**
 * 열린 thread. `next`는 보고서 `## Next`에 `aaa thread next --add`로 더한 줄까지 붙인 목록이다.
 */
threads: Array<ThreadView>, assignable: Array<OwnerChoice>, 
/**
 * 끝내지 않은 inbox 항목 수
 */
inbox_open: number, 
/**
 * 끝내지 않은 inbox 항목(herdr-projects `context`의 Inbox 절). 지시가 아니라 자료다.
 * 응답 크기 한도 안에서 오래된 것부터 싣는다: `inbox_open`보다 적을 수 있다.
 */
inbox: Array<InboxItem>, 
/**
 * 사후 평가자(지정했을 때만)
 */
evaluator: EvaluatorView | null, };

export type ToolCheck = { name: string, ok: boolean, 
/**
 * 출력 첫 줄(실패면 이유)
 */
version: string, };

export type LeaseView = { 
/**
 * 보유한 front 연결 ID(front는 Welcome·attach 때 받은 자기 ID와 비교한다)
 */
front: string, 
/**
 * 사람이 읽는 이름(예: `tui@mac-a`, `웹(ab12)`)
 */
label: string, };

export type PaneView = { id: string, machine_id: string, machine: string, name: string, argv: Array<string>, cwd: string | null, connection: Connection, 
/**
 * 지금 입력할 수 없으면 true(끊김·종료·권한). lease는 따로 본다.
 */
readonly: boolean, cols: number, rows: number, lease: LeaseView | null, role: Role | null, 
/**
 * harness 상태(hook을 보내는 agent 세션만)
 */
agent: AgentStatus | null, 
/**
 * 사람이 붙인 이름이 있으면 그 이름(`name`과 같다). 웹 메뉴가 「이름 지우기」를 보일지 정한다.
 */
label?: string, 
/**
 * 재부팅 뒤 다시 띄우지 못한 까닭(herdr `restore_error`). 그 pane은 중단으로 남는다.
 */
restore_error?: string, } & ({ "state": "running" } | { "state": "exited", code: number | null, } | { "state": "interrupted" } | { "state": "lost" });

export type SplitDir = "horizontal" | "vertical";

export type LayoutNode = { "node": "pane", pane: string, } | { "node": "split", dir: SplitDir, 
/**
 * a 쪽 비율(천분율, 1..=999)
 */
ratio: number, a: LayoutNode, b: LayoutNode, };

export type Tab = { id: string, name: string, root: LayoutNode, 
/**
 * pane 하나를 탭 가득 보인다(herdr `Tab.zoomed`). 보이는 pane은 front마다 그 front의 포커스 pane이다.
 * 나누기·닫기·옮기기로 탭의 pane이 바뀌면 풀린다.
 */
zoomed: boolean, };

export type Workspace = { name: string, tabs: Array<Tab>, };

export type LayoutChange = { "do": "new_tab", machine: string | null, name: string | null, argv: Array<string>, cwd: string | null, cols: number | null, rows: number | null, label: string | null, from: string | null, } | { "do": "split", pane: string, dir: SplitDir, machine: string | null, argv: Array<string>, cwd: string | null, ratio: number | null, } | { "do": "add_tab", pane: string, } | { "do": "close", pane: string, kill: boolean, } | { "do": "resize", pane: string, direction: PaneDirection, amount: number | null, } | { "do": "rename_tab", tab: string, name: string, } | { "do": "close_tab", tab: string, } | { "do": "move_to_new_tab", pane: string, name: string | null, } | { "do": "zoom", pane: string, zoomed: boolean | null, } | { "do": "swap", pane: string, with: string, } | { "do": "split_ratio", tab: string, path: Array<boolean>, ratio: number, } | { "do": "move_tab", tab: string, index: number, };

export type GridRun = { 
/**
 * 글자들. 넓은 글자(한글 등)는 두 칸을 차지하고 뒤 칸은 따로 적지 않는다.
 */
t: string, fg: number, bg: number, 
/**
 * 1=굵게, 2=기울임, 4=밑줄, 8=반전
 */
a: number, };

export type GridLine = { row: number, runs: Array<GridRun>, };

export type GridFrame = { pane: string, cols: number, rows: number, cursor_row: number, cursor_col: number, cursor_visible: boolean, full: boolean, lines: Array<GridLine>, 
/**
 * 그 pane 프로그램이 켠 모드(1.14부터, 옛 host는 기본값)
 */
modes: PaneModes, 
/**
 * 이 구독이 보는 기록 위치(`PaneScroll`, 1.25부터, 옛 host는 맨 아래)
 */
scroll: GridScroll, };

export type PaneModes = { mouse: MouseMode, mouse_encoding: MouseEncoding, 
/**
 * alternate screen(`CSI ? 1049 h` 등)
 */
alt_screen: boolean, 
/**
 * DECCKM(`CSI ? 1 h`): 화살표를 `ESC O x`로
 */
app_cursor: boolean, };

export type MouseMode = "none" | "press" | "press_release" | "button_motion" | "any_motion";

export type MouseEncoding = "default" | "utf8" | "sgr";

export type GridScroll = { offset: number, max: number, };

export type ScrollBy = { "to": "up", lines: number, } | { "to": "down", lines: number, } | { "to": "bottom" } | { "to": "offset", lines: number, };

export type TextPoint = { row: number, col: number, };

export type TextRange = { start: TextPoint, end: TextPoint, };

export type CopyMotion = "next_word_start" | "previous_word_start" | "next_word_end" | "next_big_word_start" | "previous_big_word_start" | "next_big_word_end" | "previous_paragraph" | "next_paragraph";

export type SearchDirection = "forward" | "backward";

export type PendingEnroll = { name: string, machine_id: string, 
/**
 * 상대 인증서 fingerprint 앞부분(참고용). 확인 코드는 agent 화면에만 보인다.
 */
fingerprint_prefix: string, since_ms: number, };

export type PublishState = { 
/**
 * missing | stopped | running | error
 */
tailscale: string, dns_name: string | null, 
/**
 * aaa 웹을 이미 proxy하는 HTTPS 주소
 */
url: string | null, 
/**
 * aaa가 만든 serve 포트(끌 때 이 포트만 끈다)
 */
owned_port: number | null, web_port: number, message: string | null, };

export type DeviceView = { id: string, label: string, 
/**
 * reader 또는 controller
 */
role: DeviceRole, created_at_ms: number, 
/**
 * 마지막으로 들어온 때(1분 단위로 적는다). 아직 한 번도 안 들어왔으면 없다
 */
last_seen_ms: number | null, };

export type HostStatus = { host_id: string, software: string, proto: Version, dev_mode: boolean, agent_listen: string | null, web_listen: string | null, machines: Array<MachineView>, };

export type FrontRequest = { "op": "status" } | { "op": "machines" } | { "op": "panes", role: Role | null, } | { "op": "pane_run", machine: string | null, name: string | null, argv: Array<string>, cwd: string | null, cols: number | null, rows: number | null, 
/**
 * 이 머신 agent.toml profile로 harness를 띄운다(`argv`는 비운다, herdr-web-ui 새 세션의 agent 고르기)
 */
profile?: string, } | { "op": "pane_read", pane: string, } | { "op": "pane_chat", pane: string, 
/**
 * 생각도 싣는다(웹 설정 「생각 표시」, plan §8.129)
 */
thinking: boolean, } | { "op": "pane_file", pane: string, path: string, } | { "op": "pane_dir", pane: string, path: string, hidden: boolean, } | { "op": "pane_send", pane: string, data: B64, } | { "op": "pane_read_text", pane: string, source: ReadSource, 
/**
 * 줄 수(기본: Recent 80줄, Visible은 화면 전부, 최대 1000)
 */
lines: number | null, format: ReadFormat, } | { "op": "pane_send_text", pane: string, text: string, } | { "op": "pane_send_keys", pane: string, keys: Array<string>, } | { "op": "pane_submit", pane: string, text: string, } | { "op": "pane_wait_output", pane: string, match: OutputMatch, source: ReadSource, lines: number | null, 
/**
 * 없으면 끝없이 기다린다(herdr와 같다)
 */
timeout_ms: number | null, } | { "op": "pane_get", pane: string, } | { "op": "pane_close", pane: string, } | { "op": "workspaces" } | { "op": "workspace_close", name: string, } | { "op": "workspace_rename", name: string, to: string, } | { "op": "pane_resize", pane: string, cols: number, rows: number, } | { "op": "pane_kill", pane: string, } | { "op": "pane_input", pane: string, data: B64, } | { "op": "pane_paste", pane: string, text: string, } | { "op": "pane_take", pane: string, } | { "op": "pane_release", pane: string, } | { "op": "pane_attach", pane: string, offset: number, total: number, sha256: string, data: B64, } | { "op": "pane_resume", pane: string, } | { "op": "pane_forget", pane: string, } | { "op": "pane_rename", pane: string, label: string | null, } | { "op": "pane_watch", pane: string, } | { "op": "pane_unwatch", pane: string, } | { "op": "pane_scroll", pane: string, by: ScrollBy, } | { "op": "pane_selection_read", pane: string, from: TextPoint, to: TextPoint, } | { "op": "pane_copy_motion", pane: string, cursor: TextPoint, motion: CopyMotion, } | { "op": "pane_copy_search", pane: string, query: string, direction: SearchDirection, cursor: TextPoint, previous: TextRange | null, } | { "op": "workspace", name: string | null, } | { "op": "layout_apply", workspace: string | null, change: LayoutChange, } | { "op": "enroll_open", minutes: number, } | { "op": "enroll_pending" } | { "op": "enroll_approve", name: string, code: string, } | { "op": "machine_revoke", name: string, } | { "op": "web_pair", role: DeviceRole, } | { "op": "devices" } | { "op": "device_rename", id: string, label: string, } | { "op": "device_revoke", id: string, } | { "op": "web_publish" } | { "op": "web_publish_off" } | { "op": "web_status" } | { "op": "project_create", name: string, } | { "op": "projects" } | { "op": "profiles", machine: string | null, } | { "op": "thread_start", project: string, title: string, machine: string | null, 
/**
 * 비면 `from_task`의 owner, 그다음 project.toml `thread_profile`(herdr-projects `thread_profile`)
 */
profile: string, 
/**
 * 없으면 repo가 있으면 worktree, 없으면 dir(herdr-projects `placement`)
 */
kind: ThreadKind | null, repo: string | null, base: string | null, 
/**
 * 할 일(threads/<id>.task.md로 저장하고 brief에 넣는다)
 */
task: string, 
/**
 * TASKS.md의 task 제목(대소문자 무시). owner가 profile·machine의 기본값이 되고, 메모가 할 일에 붙는다.
 */
from_task: string | null, } | { "op": "thread_next", project: string, thread: string, line: number | null, add: string | null, } | { "op": "context", project: string, peek: boolean, } | { "op": "assignable", project: string, check: string | null, } | { "op": "threads", project: string | null, role: Role | null, } | { "op": "thread_show", project: string, thread: string, } | { "op": "thread_library", project: string, thread: string, } | { "op": "thread_ack", project: string, thread: string, } | { "op": "thread_resolve", project: string, thread: string, keep_worktree: boolean, 
/**
 * 끝낸 thread를 다시 연다(herdr-projects `thread resolve --reopen`). 아무것도 띄우지 않는다
 */
reopen: boolean, 
/**
 * 마지막 사본 없이 끝낸다(사본을 만들 수 없을 때, worktree는 남긴다. herdr-projects `--skip-copy`)
 */
skip_copy: boolean, 
/**
 * 집에 다 오지 않았어도 worktree를 지운다(herdr-projects `--discard-uncopied`)
 */
discard_uncopied: boolean, } | { "op": "thread_stop", project: string, thread: string, } | { "op": "thread_restart", project: string, thread: string, resume: boolean, 
/**
 * 이 profile로 새 대화를 띄운다(herdr-projects `thread restart --profile`). `resume`과 함께 쓰지 않는다
 */
profile: string | null, } | { "op": "thread_brief", project: string, thread: string, } | { "op": "thread_read", project: string, thread: string, lines: number | null, } | { "op": "thread_keys", project: string, thread: string, keys: Array<string>, text: string | null, } | { "op": "questions", all: boolean, } | { "op": "question_answer", id: string, choice: string, } | { "op": "thread_message", project: string, thread: string, text: string, mode: MessageMode, } | { "op": "preferences" } | { "op": "preferences_set", settings: Record<string, unknown>, pins: Array<string> | null, } | { "op": "inbox", project: string, all: boolean, peek: boolean, } | { "op": "inbox_done", project: string, ids: Array<string>, all: boolean, } | { "op": "ticker_tick" } | { "op": "routines", project: string, } | { "op": "routine_approve", project: string, name: string, command_sha256: string, } | { "op": "routine_toggle", project: string, name: string, on: boolean | null, } | { "op": "events", project: string | null, after: bigint, limit: number | null, } | { "op": "evaluator_assign", project: string, profile: string, machine: string | null, 
/**
 * 개선을 직접 고칠 repo(없으면 repo 없는 작업 폴더)
 */
repo: string | null, 
/**
 * 평가자에게 더할 말(brief 끝에 붙는다)
 */
note: string | null, } | { "op": "evaluator_status", project: string | null, } | { "op": "evaluator_unassign", project: string, } | { "op": "evaluations", project: string, all: boolean, } | { "op": "evaluation_show", project: string, id: string, } | { "op": "evaluation_decide", project: string, id: string, proposals: Array<string>, adopt: boolean, note: string | null, };

export type SharedPreferences = { 
/**
 * 처음 쓰기 전에는 0, 쓸 때마다 하나 오른다
 */
revision: number, 
/**
 * 어느 화면이 나눈 설정만 있다. 아무도 안 바꾼 설정은 없고, 기기마다 제 것으로 남는다
 */
settings: Record<string, unknown>, 
/**
 * 목록 고정 열쇠(agent는 `machine:pane`), 고정한 순서대로. 화면이 처음 고정을 나누기 전에는 없다
 */
pins?: Array<string>, };

export type FrontResponse = { "kind": "status" } & HostStatus | { "kind": "machines", machines: Array<MachineView>, } | { "kind": "panes", panes: Array<PaneView>, } | { "kind": "pane" } & PaneView | { "kind": "next", lines: Array<string>, 
/**
 * `line`으로 보낸 메시지
 */
message: MessageView | null, } | { "kind": "context" } & ProjectContext | { "kind": "assignable", owners: Array<OwnerChoice>, 
/**
 * `check`가 통과했을 때 설명
 */
valid: string | null, } | { "kind": "screen" } & ScreenText | { "kind": "read" } & ReadView | { "kind": "thread_pane" } & ThreadPaneView | { "kind": "output_matched", matched_line: string, read: ReadView, } | { "kind": "workspaces", workspaces: Array<WorkspaceSummary>, } | { "kind": "chat" } & ChatView | { "kind": "pane_file", info: FileInfo | null, candidates: Array<string>, url: string | null, } | { "kind": "pane_dir", listing: DirListing, } | { "kind": "pending", pending: Array<PendingEnroll>, } | { "kind": "pair_code", code: string, role: DeviceRole, expires_in_s: number, } | { "kind": "publish" } & PublishState | { "kind": "devices", devices: Array<DeviceView>, } | { "kind": "workspace" } & Workspace | { "kind": "done" } | { "kind": "selection", pane: string, text: string, truncated: boolean, } | { "kind": "copy_cursor", pane: string, cursor: TextPoint, } | { "kind": "copy_search", pane: string, matches: Array<TextRange>, total: bigint, current: number | null, current_global: bigint | null, } | { "kind": "attached", path: string, inserted: boolean, note: string | null, } | { "kind": "projects", projects: Array<ProjectView>, } | { "kind": "project" } & ProjectView | { "kind": "profiles", machine: string, profiles: Array<ProfileView>, } | { "kind": "threads", threads: Array<ThreadView>, } | { "kind": "thread" } & ThreadView | { "kind": "events", events: Array<FlowEvent>, } | { "kind": "questions", questions: Array<QuestionView>, } | { "kind": "question" } & QuestionView | { "kind": "message" } & MessageView | { "kind": "inbox", items: Array<InboxItem>, 
/**
 * `inbox_done`이 끝낸 항목 수(그때 `items`는 남은 항목). 목록 요청에는 없다.
 */
done?: number, 
/**
 * 응답 크기 한도로 싣지 못한 끝내지 않은 항목 수(실은 것은 오래된 것부터다)
 */
omitted_open: number, 
/**
 * 응답 크기 한도로 싣지 못한 끝낸 항목 수(`all`, 실은 것은 최근 것부터다)
 */
omitted_done: number, } | { "kind": "ticker", runs: bigint, } | { "kind": "routines", routines: Array<RoutineView>, } | { "kind": "library" } & LibraryView | { "kind": "evaluators", evaluators: Array<EvaluatorView>, } | { "kind": "evaluations", evaluations: Array<EvaluationView>, } | { "kind": "evaluation" } & EvaluationView | { "kind": "preferences" } & SharedPreferences;

export type FrontEvent = { "ev": "state", machines: Array<MachineView>, panes: Array<PaneView>, } | { "ev": "workspace" } & Workspace | { "ev": "grid" } & GridFrame | { "ev": "flow" } & FlowEvent | { "ev": "thread" } & ThreadView | { "ev": "question" } & QuestionView | { "ev": "preferences" } & SharedPreferences | { "ev": "pong", id: number, };

export type TermServerMsg = { "t": "snapshot", replay: B64, cols: number, rows: number, readonly: boolean, stream: number, 
/**
 * replay에 싣지 않은 앞 기록이 host에 더 있다: 위로 굴려 끝에 닿으면 `History`로 받아 온다(plan §8.67)
 */
history_more: boolean, } | { "t": "history", id: number, data: B64, lines: number, more: boolean, found: boolean, } | { "t": "readonly", readonly: boolean, reason: string | null, } | { "t": "state", pane: PaneView, } | { "t": "front", id: string, } | { "t": "attached", path: string, inserted: boolean, note: string | null, } | { "t": "submitted", id: number, ok: boolean, message: string | null, } | { "t": "error" } & ErrorBody;

export type TermClientMsg = { "t": "input", data: string, } | { "t": "resize", cols: number, rows: number, } | { "t": "take" } | { "t": "release" } | { "t": "submit", id: number, data: string, } | { "t": "history", id: number, anchor: Array<string>, depth: number, } | { "t": "ack", stream: number, offset: number, } | { "t": "attach", offset: number, total: number, sha256: string, data: B64, };

export type EventsClientMsg = { "t": "presence", active: boolean, 
/**
 * 무엇이 알리게 했나(focus·blur·visibilitychange·pagehide·pageshow·freeze·resume·open·heartbeat)
 */
why?: string, 
/**
 * 그때 `document.visibilityState === "visible"`
 */
visible?: boolean, 
/**
 * 그때 `document.hasFocus()`
 */
focus?: boolean, } | { "t": "alert", project: string | null, target: string, kind: string, outcome: string, } | { "t": "touch" } | { "t": "ping", id: number, };

export type Role = "coordinator" | "worker" | "evaluator";

export type ThreadKind = "worktree" | "dir";

export type Launch = { profile: string, 
/**
 * claude·codex·shell·custom
 */
harness: string, argv: Array<string>, env: Array<[string, string]>, 
/**
 * 같은 대화를 이어 띄우는 argv(모를 때는 빈 배열). daemon 금지 인자 등은 argv와 같다.
 */
resume_argv: Array<string>, 
/**
 * harness 세션 ID(미리 정할 수 있는 harness만, 예: claude `--session-id`)
 */
session_key: string | null, };

export type ProfileView = { name: string, harness: string, model: string | null, effort: string | null, description: string, };

export type LibFile = { path: string, size: number, sha256: string, };

export type Skipped = { path: string, reason: string, 
/**
 * 복사 상한을 넘어 agent에 남긴 일반 파일의 크기(이것만 나중에 지연 스트리밍으로 볼 수 있다)
 */
lazy_size: number | null, };

export type ProjectView = { name: string, path: string, threads: number, };

export type ThreadStatus = "starting" | "open" | "failed" | "resolved";

export type BriefState = "pending" | "delivered" | "unknown";

export type ThreadGroup = "waiting_on_you" | "ready_for_review" | "landing" | "working" | "idle" | "resolved";

export type ThreadView = { project: string, id: string, title: string, machine: string, profile: string, role: Role, 
/**
 * (FrontResponse 태그 `kind`와 겹치지 않게 이름을 달리한다)
 */
thread_kind: ThreadKind, status: ThreadStatus, group: ThreadGroup, brief: BriefState, pane: string | null, cwd: string | null, work_dir: string | null, branch: string | null, 
/**
 * 추적하는 PR(§8.80)
 */
pr: ThreadPr | null, report_hash: string | null, acked_hash: string | null, 
/**
 * report.md `## Next` 줄
 */
next: Array<string>, 
/**
 * 마지막 사본 결과: complete·partial·failed
 */
copy: string | null, error: string | null, 
/**
 * agent harness 상태(hook으로 안다)
 */
agent: AgentStatus | null, created_at_ms: number, updated_at_ms: number, 
/**
 * `thread start`가 알리는 경고(시작은 했다, 예: 동시 thread 한도). 다른 응답에는 없다.
 */
warnings?: Array<string>, };

export type ThreadPr = { url: string, state: string | null, review: string | null, };

export type FlowEvent = { project: string, seq: number, at_ms: number, boot_id: string, 
/**
 * thread_started·brief_delivered·brief_unknown·report_ready·review_acked·thread_resolved·thread_failed …
 */
kind: string, thread: string | null, role: Role | null, profile: string | null, machine: string | null, 
/**
 * 근거 경로(host 프로젝트 폴더 기준, 예: `threads/t-0001.md`)
 */
evidence: string | null, 
/**
 * 사람이 읽는 한 줄
 */
detail: string, };

export type AgentActivity = "started" | "working" | "idle" | "question" | "error" | "exited";

export type AgentStatus = { activity: AgentActivity, since_ms: number, harness: string, message: string | null, 
/**
 * `aaa report --percent`
 */
percent: number | null, 
/**
 * `aaa report --activity`
 */
progress: string | null, 
/**
 * 마지막 `aaa report` 시각. 5분이 지나면 진행 보고로 「작업 중」을 정하지 않는다(herdr-projects `ACTIVITY_TTL_MS`).
 */
progress_at_ms: number | null, harness_session: string | null, 
/**
 * 마지막으로 쉼(`Idle`)이 된 뒤 사람이 이 pane을 봤다. false면 「끝남(안 봄)」이다(herdr `PaneState::seen`:
 * 작업·질문에서 쉼으로 끝날 때 보는 화면이 없으면 false, 그 pane을 보면 true).
 */
seen: boolean, };

export type QuestionState = "open" | "answered" | "gone";

export type QuestionView = { id: string, pane: string, machine: string, project: string | null, thread: string | null, 
/**
 * permission·trust(FrontResponse 태그 `kind`와 겹치지 않게 이름을 바꿨다)
 */
question_kind: string, text: string, 
/**
 * 고를 수 있는 답(예: yes·no)
 */
options: Array<string>, state: QuestionState, answer: string | null, created_at_ms: number, };

export type MessageMode = "interrupt" | "queue";

export type DeliveryState = "queued" | "accepted" | "unknown";

export type MessageView = { id: string, project: string, thread: string, mode: MessageMode, state: DeliveryState, text: string, created_at_ms: number, };

export type InboxItem = { 
/**
 * `<UTC 시각>-<종류>-<대상>-<순번>`
 */
id: string, project: string, 
/**
 * report·question·delivery·thread·session·outage·health·config-error·routine·routine-approval
 */
item_kind: string, 
/**
 * thread ID, 머신 이름, 파일 이름 등
 */
subject: string, 
/**
 * 짧은 사건 이름(nudge 문구에 쓴다)
 */
event: string, 
/**
 * 사람이 읽는 한 줄(nudge 문구에는 넣지 않는다)
 */
summary: string, 
/**
 * 긴 본문(routine 안내·명령 출력). 투영 파일의 front matter 아래에 쓴다.
 */
body: string, done: boolean, 
/**
 * coordinator(또는 사람)가 `aaa inbox`로 본 적이 있다. 처음 보는 항목만 알린다.
 */
seen: boolean, created_at_ms: number, done_at_ms: number | null, };

export type RoutineView = { name: string, 
/**
 * `every 30m`·`daily 09:00`(파일에 적힌 글)
 */
schedule: string, 
/**
 * 비면 안내만 하는 routine
 */
command: string, 
/**
 * 명령을 실행할 머신(없으면 host 머신)
 */
machine: string | null, enabled: boolean, 
/**
 * host.toml `[projects] routine_commands`가 이 프로젝트를 허용한다
 */
commands_allowed: boolean, 
/**
 * 지금 명령(머신 포함)이 승인돼 있다
 */
approved: boolean, command_sha256: string, last_run_ms: number | null, next_run_ms: number | null, 
/**
 * 안내 항목을 아직 처리하지 않아 건너뛴 횟수
 */
skipped: number, 
/**
 * coordinator가 없어 건너뛴 횟수(돌면 0)
 */
no_coordinator: number, 
/**
 * 파일을 읽지 못했다
 */
error: string | null, };

export type LibraryFile = { 
/**
 * library 안 상대 경로
 */
path: string, size: number, 
/**
 * host에 사본이 없다: 볼 때 agent에서 스트리밍한다(작업 폴더가 남아 있을 때만)
 */
on_agent: boolean, };

export type LibraryView = { project: string, thread: string, 
/**
 * 회수한 report.md(없으면 None)
 */
report: string | null, files: Array<LibraryFile>, base: string, expires_at_ms: number, };

export type FileKind = "image" | "video" | "audio" | "pdf" | "text" | "binary";

export type FileInfo = { path: string, name: string, size: number, modified_ms: number, mime: string, kind: FileKind, };

export type DirListing = { path: string, parent: string | null, home: string, directories: Array<string>, files: Array<DirFile>, truncated: boolean, };

export type DirFile = { name: string, size: number, };

export type EvaluatorView = { project: string, thread: ThreadView, 
/**
 * 세션이 돌고 있다(아니면 `thread restart`로 다시 띄운다)
 */
running: boolean, 
/**
 * 마지막으로 넘긴 사건 seq
 */
cursor: number, 
/**
 * 아직 넘기지 않은 사건 수(평가자 자신의 사건 제외)
 */
pending: number, 
/**
 * 넘긴 사건 묶음 수
 */
batches: number, last_batch_ms: number | null, 
/**
 * 받은 평가 기록 수
 */
evaluations: number, 
/**
 * 채택·보류를 정하지 않은 개선안 수
 */
undecided: number, };

export type EvaluationDoc = { 
/**
 * 평가한 thread ID
 */
targets: Array<string>, 
/**
 * 근거로 읽은 사건 묶음 파일 이름
 */
batches: Array<string>, 
/**
 * 흐름 효율 판정 한두 줄
 */
summary: string, 
/**
 * 단계별 시간
 */
stages: Array<EvalStage>, 
/**
 * 재작업(횟수·원인)
 */
rework: Array<string>, 
/**
 * 막힌 곳(기다린 것과 그 시간)
 */
blocked: Array<string>, tool_issues: Array<string>, 
/**
 * 결함을 누가 어느 단계에서 잡았나
 */
defects: Array<EvalDefect>, proposals: Array<Proposal>, };

export type EvalStage = { thread: string, stage: string, 
/**
 * 걸린 시간
 */
minutes: Minutes | null, };

export type Minutes = number;

export type EvalDefect = { what: string, 
/**
 * 잡은 쪽(사람·coordinator·thread·CI·E2E …)
 */
found_by: string, stage: string, };

export type Proposal = { id: string, text: string, 
/**
 * 예상 효과
 */
effect: string, 
/**
 * 대상 도구
 */
tool: string, decision: ProposalDecision | null, };

export type ProposalDecision = { adopted: boolean, by: string, at_ms: number, note: string | null, };

export type EvaluationView = { project: string, 
/**
 * 평가자가 정한 파일 이름(프로젝트 안에서 하나)
 */
id: string, 
/**
 * 평가자 thread ID
 */
evaluator: string, created_at_ms: number, 
/**
 * 사람이 읽는 본문(프로젝트 폴더 기준 `evaluations/<id>.md`)
 */
file: string, doc: EvaluationDoc, };

export type UsageProviderId = "claude" | "codex";

export type UsageWindowKind = "session" | "day" | "week" | "month";

export type UsageWindow = { kind: UsageWindowKind, 
/**
 * plan이 따로 거는 모델·몫("Sonnet", "Premium"). plan 전체 한도면 None
 */
scope: string | null, 
/**
 * 0~100
 */
used_percent: number, 
/**
 * ISO 8601. 구독이 말하지 않으면 None
 */
resets_at: string | null, 
/**
 * 이 창이 세기 시작한 때(ISO 8601). 구독이 그것도 창 길이도 말하지 않으면 None
 */
starts_at: string | null, };

export type UsageProblem = "expired" | "rate_limited" | "failed" | "locked";

export type ProviderUsage = { id: UsageProviderId, 
/**
 * 한 구독의 한 계정. 읽을 때마다 같다: 구독과 계정 id, 누구 것인지 말하지 않는 로그인이면 구독과 찾은 곳(hash).
 * 한 머신이 한 구독의 두 계정에 로그인했으면 둘 다 나온다.
 */
key: string, 
/**
 * 주인이 아는 계정 이름(email, GitHub login). 모르면 None
 */
account: string | null, 
/**
 * 구독이 말하는 plan 이름("max", "pro")
 */
plan: string | null, 
/**
 * 마지막으로 읽은 숫자. 뒤에 실패해도 남고, 그때는 `problem`이 까닭을 말한다
 */
windows: Array<UsageWindow>, problem: UsageProblem | null, 
/**
 * `windows`를 읽은 때(ISO 8601). 한 번도 못 읽었으면 None
 */
checked_at: string | null, };

export type UsageReport = { providers: Array<ProviderUsage>, };


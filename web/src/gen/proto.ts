// 생성 파일: crates/aa-proto의 Rust 타입에서 만든다. 손으로 고치지 않는다.
// 갱신: AA_UPDATE_TS=1 cargo test -p aa-proto typescript

export type Version = { major: number, minor: number, };

export type B64 = string;

export type ErrorCode = "bad_request" | "unauthorized" | "forbidden" | "not_found" | "agent_disconnected" | "session_exited" | "unsupported_version" | "not_enrolled" | "enroll_closed" | "enroll_code_mismatch" | "name_taken" | "timeout" | "unavailable" | "internal" | "lease_held";

export type ErrorBody = { code: ErrorCode, message: string, };

export type Outcome<T> = { "ok": T } | { "err": ErrorBody };

export type SessionState = { "state": "running" } | { "state": "exited", code: number | null, } | { "state": "interrupted" } | { "state": "lost" };

export type ScreenText = { session: string, cols: number, rows: number, 
/**
 * 보이는 화면의 글자(줄 끝 공백 제거)
 */
lines: Array<string>, cursor_row: number, cursor_col: number, };

export type DeviceRole = "reader" | "controller" | "local";

export type Connection = "connected" | "disconnected";

export type MachineView = { id: string, name: string, local: boolean, connection: Connection, os: string | null, boot_id: string | null, dev_mode: boolean, last_seen_ms: number | null, };

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
agent: AgentStatus | null, } & ({ "state": "running" } | { "state": "exited", code: number | null, } | { "state": "interrupted" } | { "state": "lost" });

export type SplitDir = "horizontal" | "vertical";

export type LayoutNode = { "node": "pane", pane: string, } | { "node": "split", dir: SplitDir, 
/**
 * a 쪽 비율(천분율, 1..=999)
 */
ratio: number, a: LayoutNode, b: LayoutNode, };

export type Tab = { id: string, name: string, root: LayoutNode, };

export type Workspace = { name: string, tabs: Array<Tab>, };

export type LayoutChange = { "do": "new_tab", machine: string | null, name: string | null, argv: Array<string>, cwd: string | null, } | { "do": "split", pane: string, dir: SplitDir, machine: string | null, argv: Array<string>, cwd: string | null, } | { "do": "add_tab", pane: string, } | { "do": "close", pane: string, kill: boolean, };

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

export type GridFrame = { pane: string, cols: number, rows: number, cursor_row: number, cursor_col: number, cursor_visible: boolean, full: boolean, lines: Array<GridLine>, };

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
 * aa 웹을 이미 proxy하는 HTTPS 주소
 */
url: string | null, 
/**
 * aa가 만든 serve 포트(끌 때 이 포트만 끈다)
 */
owned_port: number | null, web_port: number, message: string | null, };

export type HostStatus = { host_id: string, software: string, proto: Version, dev_mode: boolean, agent_listen: string | null, web_listen: string | null, machines: Array<MachineView>, };

export type FrontRequest = { "op": "status" } | { "op": "machines" } | { "op": "panes", role: Role | null, } | { "op": "pane_run", machine: string | null, name: string | null, argv: Array<string>, cwd: string | null, cols: number | null, rows: number | null, } | { "op": "pane_read", pane: string, } | { "op": "pane_send", pane: string, data: B64, } | { "op": "pane_resize", pane: string, cols: number, rows: number, } | { "op": "pane_kill", pane: string, } | { "op": "pane_input", pane: string, data: B64, } | { "op": "pane_take", pane: string, } | { "op": "pane_release", pane: string, } | { "op": "pane_resume", pane: string, } | { "op": "pane_forget", pane: string, } | { "op": "pane_watch", pane: string, } | { "op": "pane_unwatch", pane: string, } | { "op": "workspace", name: string | null, } | { "op": "layout_apply", workspace: string | null, change: LayoutChange, } | { "op": "enroll_open", minutes: number, } | { "op": "enroll_pending" } | { "op": "enroll_approve", name: string, code: string, } | { "op": "machine_revoke", name: string, } | { "op": "web_pair", role: DeviceRole, } | { "op": "web_publish" } | { "op": "web_publish_off" } | { "op": "web_status" } | { "op": "project_create", name: string, } | { "op": "projects" } | { "op": "profiles", machine: string | null, } | { "op": "thread_start", project: string, title: string, machine: string | null, profile: string, kind: ThreadKind, repo: string | null, base: string | null, 
/**
 * 할 일(threads/<id>.task.md로 저장하고 brief에 넣는다)
 */
task: string, } | { "op": "threads", project: string | null, } | { "op": "thread_show", project: string, thread: string, } | { "op": "thread_library", project: string, thread: string, } | { "op": "thread_ack", project: string, thread: string, } | { "op": "thread_resolve", project: string, thread: string, keep_worktree: boolean, } | { "op": "thread_stop", project: string, thread: string, } | { "op": "thread_restart", project: string, thread: string, resume: boolean, } | { "op": "thread_brief", project: string, thread: string, } | { "op": "questions", all: boolean, } | { "op": "question_answer", id: string, choice: string, } | { "op": "thread_message", project: string, thread: string, text: string, mode: MessageMode, } | { "op": "decision_add", project: string, text: string, replaces: number | null, } | { "op": "decisions", project: string, } | { "op": "inbox", project: string, all: boolean, peek: boolean, } | { "op": "inbox_done", project: string, ids: Array<string>, all: boolean, } | { "op": "merge_enqueue", project: string, thread: string, } | { "op": "merge_drop", project: string, thread: string, landed: boolean, } | { "op": "merge_list", project: string, } | { "op": "routines", project: string, } | { "op": "routine_approve", project: string, name: string, command_sha256: string, } | { "op": "events", project: string | null, after: bigint, limit: number | null, };

export type FrontResponse = { "kind": "status" } & HostStatus | { "kind": "machines", machines: Array<MachineView>, } | { "kind": "panes", panes: Array<PaneView>, } | { "kind": "pane" } & PaneView | { "kind": "screen" } & ScreenText | { "kind": "pending", pending: Array<PendingEnroll>, } | { "kind": "pair_code", code: string, role: DeviceRole, expires_in_s: number, } | { "kind": "publish" } & PublishState | { "kind": "workspace" } & Workspace | { "kind": "done" } | { "kind": "projects", projects: Array<ProjectView>, } | { "kind": "project" } & ProjectView | { "kind": "profiles", machine: string, profiles: Array<ProfileView>, } | { "kind": "threads", threads: Array<ThreadView>, } | { "kind": "thread" } & ThreadView | { "kind": "events", events: Array<FlowEvent>, } | { "kind": "questions", questions: Array<QuestionView>, } | { "kind": "question" } & QuestionView | { "kind": "message" } & MessageView | { "kind": "decisions", decisions: Array<DecisionView>, } | { "kind": "decision" } & DecisionView | { "kind": "inbox", items: Array<InboxItem>, } | { "kind": "merge", queue: Array<MergeEntry>, } | { "kind": "routines", routines: Array<RoutineView>, } | { "kind": "library" } & LibraryView;

export type FrontEvent = { "ev": "state", machines: Array<MachineView>, panes: Array<PaneView>, } | { "ev": "workspace" } & Workspace | { "ev": "grid" } & GridFrame | { "ev": "flow" } & FlowEvent | { "ev": "thread" } & ThreadView | { "ev": "question" } & QuestionView;

export type TermServerMsg = { "t": "snapshot", replay: B64, cols: number, rows: number, readonly: boolean, } | { "t": "readonly", readonly: boolean, reason: string | null, } | { "t": "state", pane: PaneView, } | { "t": "front", id: string, } | { "t": "error" } & ErrorBody;

export type TermClientMsg = { "t": "input", data: string, } | { "t": "resize", cols: number, rows: number, } | { "t": "take" };

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

export type ThreadGroup = "waiting_on_you" | "ready_for_review" | "working" | "resolved";

export type ThreadView = { project: string, id: string, title: string, machine: string, profile: string, role: Role, 
/**
 * (FrontResponse 태그 `kind`와 겹치지 않게 이름을 달리한다)
 */
thread_kind: ThreadKind, status: ThreadStatus, group: ThreadGroup, brief: BriefState, pane: string | null, cwd: string | null, work_dir: string | null, branch: string | null, report_hash: string | null, acked_hash: string | null, 
/**
 * report.md 첫 줄 상태 태그(예: `진행 중`, `완료`)
 */
report_state: string | null, 
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
agent: AgentStatus | null, 
/**
 * thread를 시작할 때 brief에 넣은 결정 기록 번호(가장 큰 번호). 지금 번호보다 작으면 옛 결정으로 일한다.
 */
decisions_rev: number | null, 
/**
 * 프로젝트의 지금 결정 번호
 */
decisions_now: number | null, created_at_ms: number, updated_at_ms: number, };

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
 * `aa report --percent`
 */
percent: number | null, 
/**
 * `aa report --activity`
 */
progress: string | null, harness_session: string | null, };

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

export type DecisionView = { num: number, 
/**
 * 적은 시각(파일에 적힌 글 그대로)
 */
at: string, text: string, 
/**
 * 이 결정이 대체한 번호
 */
replaces: number | null, 
/**
 * 나중 결정이 이 결정을 대체했다
 */
replaced_by: number | null, };

export type InboxItem = { 
/**
 * `<UTC 시각>-<종류>-<대상>-<순번>`
 */
id: string, project: string, 
/**
 * report·question·delivery·thread·session·outage·config-error
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
 * coordinator(또는 사람)가 `aa inbox`로 본 적이 있다. 처음 보는 항목만 알린다.
 */
seen: boolean, created_at_ms: number, done_at_ms: number | null, };

export type MergeEntry = { project: string, thread: string, 
/**
 * 1부터. 1이 맨 앞(지금 병합할 차례)이다.
 */
position: number, title: string, machine: string, branch: string | null, enqueued_at_ms: number, };

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


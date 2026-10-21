// 생성 파일: crates/aaa-proto의 Rust 타입에서 만든다. 손으로 고치지 않는다.
// 갱신: AAA_UPDATE_TS=1 cargo test -p aaa-proto typescript

export type SecretText = string;

export type AccountView = { name: string, harness: string, version: number,
/**
 * 공식 CLI가 로그인 때 알린 메일(FRONT_API 1.79, 관리 GUI W5a). 조작 기기와 host의 세션 밖 CLI에만 싣는다. 모르면 생략
 */
email?: string,
/**
 * 구독 plan(Codex 인증의 계정 정보). 메일과 같은 곳에만 싣는다
 */
plan?: string,
/**
 * aaa가 처음 기록한 인증 시각(ms). 이 기록 전에 등록한 계정은 생략(모름)
 */
first_auth_ms?: number,
/**
 * 마지막으로 로그인에 성공한 시각(ms)
 */
last_auth_ms?: number,
/**
 * 인증에 적힌 만료 시각(ms). 공식 CLI가 알린 것만(Claude `setup-token`의 유효 기간). 모르면 생략
 */
expires_ms?: number,
/**
 * 인증 상태. 옛 host는 생략한다
 */
health?: AccountHealth, };

export type QuotaSource = "codex_rpc" | "claude_statusline";

export type QuotaProblem = "unknown" | "expired" | "rate_limited" | "locked" | "disconnected" | "unsupported" | "failed" | "clock_skew";

export type QuotaFreshness = "fresh" | "stale" | "unknown";

export type QuotaWindow = { limit_id: string,
/**
 * 기간을 모르는 서로 다른 창도 합치지 않기 위한 제공자 창 이름.
 */
window_id: string, scope: string | null, duration_mins: number | null, used_percent: number, observed_at_ms: number, resets_at_ms: number | null, };

export type QuotaSnapshot = {
/**
 * 로그인·등록 때 새로 만드는 세대. 토큰의 해시가 아니다.
 */
generation: string, source: QuotaSource, sampled_at_ms: number, windows: Array<QuotaWindow>, problem: QuotaProblem | null, retry_at_ms: number | null, };

export type AccountQuotaWindow = { machine: string, source: QuotaSource, received_at_ms: number, freshness: QuotaFreshness, problem: QuotaProblem | null, conflict: boolean, limit_id: string,
/**
 * 기간을 모르는 서로 다른 창도 합치지 않기 위한 제공자 창 이름.
 */
window_id: string, scope: string | null, duration_mins: number | null, used_percent: number, observed_at_ms: number, resets_at_ms: number | null, };

export type AccountQuota = { account: string, harness: string, windows: Array<AccountQuotaWindow>, problem: QuotaProblem | null, retry_at_ms: number | null, };

export type AccountQuotaReport = { accounts: Array<AccountQuota>, };

export type AccountHealth = "ok" | "expiring" | "expired" | "rejected";

export type AccountDefault = { harness: string, account: string, };

export type AccountLoginView = { id: string, account: string, harness: string,
/**
 * waiting·succeeded·failed·cancelled, 그리고 `mismatch`(다시 로그인에 다른 메일이 들어왔다: `account_login_confirm`을 기다린다)·
 * `kept`(바꾸지 않고 옛 인증을 남겼다)
 */
state: string,
/**
 * 다시 로그인할 때 그 별칭에 기록된 메일(「이 메일로 로그인하세요」, FRONT_API 1.79). 조작 기기·세션 밖 CLI에만
 */
expect_email?: string,
/**
 * 로그인한 메일(끝난 뒤, 공식 CLI가 알렸을 때)
 */
email?: string, };

export type Version = { major: number, minor: number, };

export type B64 = string;

export type ErrorCode = "bad_request" | "unauthorized" | "forbidden" | "not_found" | "agent_disconnected" | "session_exited" | "unsupported_version" | "not_enrolled" | "enroll_closed" | "enroll_code_mismatch" | "name_taken" | "timeout" | "unavailable" | "internal" | "lease_held" | "subscription_not_found" | "subscription_gone" | "too_large" | "conflict";

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

export type ChatRole = "user" | "assistant" | "tool" | "thinking" | "compact" | "command" | "command_output";

export type ChatMessage = {
/**
 * 기록 파일/byte/블록 위치. 옛 agent는 생략한다.
 */
id?: string, role: ChatRole, text: string,
/**
 * 사람 글이 harness 대기열에 있다(작업 중에 보내 아직 흡수되지 않음, Claude `queue-operation`). 기록 끝에 붙는다.
 * 옛 agent는 보내지 않고 옛 front는 버린다(plan §8.247).
 */
pending: boolean,
/**
 * 표시 상한으로 자르기 전의 사람 글. 옛 agent에는 없으므로 짧은 글 비교로 돌아간다.
 */
receipt?: ChatReceipt | null, };

export type ChatView = { pane: string, harness: string, messages: Array<ChatMessage>,
/**
 * 기록이 더 길어 끝부분만 실었다
 */
truncated: boolean,
/**
 * chat_pages.v1: 이전 쪽 cursor, null이면 처음. history가 없으면 옛 agent다.
 */
before?: string,
/**
 * 경로를 드러내지 않는 파일 정체성. append에는 같다.
 */
history?: string, };

export type InputTraceEvent = "receipt" | "preparing" | "ctrl_enter_blocked" | "unlocked" | "slow" | "unlock" | "cancel" | "failed" | "pending" | "stuck";

export type InputReceiptMethod = "normal" | "pasted_content" | "queue_operation" | "codex_delivery" | "no_transcript" | "connection" | "rejected" | "unknown" | "held_draft" | "composing" | "readonly" | "locked" | "uploading" | "no_room" | "compacting" | "slash_command";

export type InputEvent = { at_ms: bigint, id: string, pane: string, thread: string | null, harness: string | null, working: boolean, event: string, detail: string, };

export type InputSubmitMeta = { pasted: boolean, attachments: number, ctrl_enter: boolean, };

export type ChatReceipt = { sha256: string, method: InputReceiptMethod, };

export type DeviceRole = "reader" | "controller" | "local";

export type Connection = "connected" | "disconnected";

export type MachineView = {
/**
 * 마지막 PR 조회의 머신별 사용 상태. 조회 전·옛 host는 생략한다.
 */
pr_tracking?: string, id: string, name: string, local: boolean, connection: Connection, os: string | null, boot_id: string | null, dev_mode: boolean, last_seen_ms: number | null,
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
 * 직전 표본부터의 CPU 사용률(0~100, 모든 코어 평균). 옛 agent는 보내지 않는다
 */
cpu_pct?: number,
/**
 * 사용 메모리와 전체 바이트. 사용 = macOS 앱(internal−purgeable)+wired+압축, Linux 전체−MemAvailable, Windows 전체−ullAvailPhys
 */
mem_used_bytes?: number, mem_total_bytes?: number,
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

export type QueueState = "queued" | "check" | "starting" | "running" | "finished" | "cancelled";

export type QueueOp = { "op": "list" } | { "op": "reserve", task: string, block: string, profile: string, profile_revision: string, } | { "op": "cancel", task_id: string, } | { "op": "move", task_id: string, position: number, };

export type TaskChoicePin = { owner: string | null, profile: string, machine: string | null, actor: string, at_ms: number, };

export type TaskReservation = { selection: TaskChoicePin, task_id: string, task: TaskView, approved_block: string, profile: string, model: string, effort: string, profile_revision: string, machine: string | null, position: number, state: QueueState, reason: string,
/**
 * 선점과 함께 발급한 기동 ID. 외부 기동의 exactly-once 보증은 아니다.
 */
launch_id: string | null, thread: string | null, actor: string, created_ms: number, updated_ms: number, };

export type ArchivedTask = { task_id: string, task: TaskView,
/**
 * 복원할 블록 원문. 첨부 참조는 프로젝트의 같은 경로다.
 */
body: string, reason: string, report: string | null, completed_ms: number, restored: boolean,
/**
 * 본문 제거 뒤의 파일 판과 대조하는 복구 영수증.
 */
committed: boolean, removed_sha256: string, history: Array<TaskLogEntry>, };

export type TaskView = {
/**
 * 제목과 독립된 task ID. 기존 task는 명시 쓰기 때 부여한다.
 */
id: string | null,
/**
 * 속한 `## ` 목록 이름
 */
list: string, title: string,
/**
 * 괄호 안 owner 원문(`me`, `claude`, `codex@mac-b`, 사람 이름 등)
 */
owner: string | null,
/**
 * agent에게 맡긴 task의 모델(profile) 칸(FRONT_API 1.75, plan §8.335): owner `모델@머신`의 앞. 맨 이름은 host가 아는 profile일
 * 때만 모델이다(아니면 사람). 비면 시작할 때 고른다. 옛 host는 생략한다
 */
profile: string | null,
/**
 * agent에게 맡긴 task의 머신 칸(FRONT_API 1.75): owner `모델@머신`의 뒤. 비면 그 저장소가 있는 머신 중 덜 붐비는 곳이다
 */
machine: string | null,
/**
 * owner 뒤 대괄호의 저장소 칸(FRONT_API 1.73, plan §8.330): `project.toml` repos 경로의 마지막 이름이나 `작업 폴더`. 비면 정하지
 * 않은 task다(「바로 시작」 창에서 고른다). 옛 host는 생략한다
 */
repo: string | null,
/**
 * ` · t-0001`로 붙은 thread
 */
thread: string | null,
/**
 * task 줄 아래 들여쓴 메모
 */
notes: Array<string>, done: boolean,
/**
 * task 줄 번호(1부터, FRONT_API 1.60). 옛 host는 생략한다.
 */
line: number,
/**
 * task 줄 원문(앞 공백 제외)
 */
raw: string,
/**
 * task 블록(task 줄과 메모 줄, 블록 끝 빈 줄 제외, 줄 끝 문자 없이 `\n`으로 이음)의 sha256. 편집(T2)의 충돌 anchor다.
 */
block_sha256: string,
/**
 * 들여쓴 하위 task면 상위 task의 `line`
 */
parent: number | null,
/**
 * 형식 어긋남(owner 칸이 owner 꼴이 아님, 제목 안 thread 번호 등). 있으면 화면이 원문을 보이고 판단하지 않는다.
 */
problems: Array<string>,
/**
 * 형식 어긋남 task의 「정리 제안」(FRONT_API 1.62): host가 만든 정규형 후보. 사람이 「이대로 저장」해야 바뀐다.
 */
suggestion: TaskSuggestion | null,
/**
 * 「확인 필요」의 까닭(FRONT_API 1.63, 할 일 T3): 이어진 thread가 끝났는데 완료로 판정하지 못했다(보고 표지·버림·하위 task
 * 남음), 하위 task가 모두 끝났다 등. host가 지우지 않고 사람이 「끝남으로 지우기」·「다시 맡기기」·「연결만 풀기」로 정한다
 */
attention: string | null, };

export type TaskListView = {
/**
 * `##` 머리 이름. 첫 `##` 앞의 task·설명 줄은 이름 없는 목록(`""`)이다
 */
name: string,
/**
 * 머리 밑 체크박스 없는 줄(목록 설명). task 블록 안의 메모는 들지 않는다
 */
notes: Array<string>, };

export type TaskThread = { id: string, title: string, group: ThreadGroup, };

export type TaskNotesSection = {
/**
 * `##` 머리 이름. 첫 `##` 앞 줄은 이름 없는 절이다
 */
name: string, lines: Array<string>, };

export type TaskNotesFile = {
/**
 * 프로젝트 폴더 기준 상대 경로
 */
path: string,
/**
 * 읽지 못한 까닭(없음·프로젝트 폴더 밖·너무 큼). 있으면 `sections`는 비어 있다
 */
error: string | null, sections: Array<TaskNotesSection>, };

export type TaskBoard = { queue: Array<TaskReservation>, project: string,
/**
 * 파일 전체 바이트의 sha256(파일이 없으면 빈 글의 hash)
 */
sha256: string, lists: Array<TaskListView>, tasks: Array<TaskView>,
/**
 * 열린 thread 중 어느 task의 `thread`에도 없는 것
 */
unlinked_threads: Array<TaskThread>,
/**
 * project.toml `task_notes`의 파일마다(설정이 비면 비어 있다)
 */
notes_files: Array<TaskNotesFile>,
/**
 * 파일 원문(원문 편집, FRONT_API 1.62). 옛 host는 생략한다
 */
text: string,
/**
 * task 저장소 칸에 쓸 수 있는 이름(FRONT_API 1.73): `project.toml` repos 경로의 마지막 이름, 겹치지 않게 적힌 차례
 */
repos: Array<string>, };

export type TaskSuggestion = { title: string, owner: string | null, thread: string | null,
/**
 * 제안한 뒤의 메모 전체(지금 메모 앞에 옮긴 글을 둔다)
 */
notes: Array<string>,
/**
 * 제안대로 쓴 task 줄(앞 공백 없이)
 */
line: string,
/**
 * 무엇을 바꾸나(한 줄씩)
 */
changes: Array<string>, };

export type TaskAnchor = { list: string, title: string, block_sha256: string, line: number, };

export type TaskOp = { "op": "add", list: string, title: string,
/**
 * 비움 = 미할당
 */
owner: string | null,
/**
 * owner가 사람 이름이다(「사람으로 저장」). 아니면 `assignable --check`와 같은 판정을 거친다
 */
person: boolean,
/**
 * 모델(profile) 칸(FRONT_API 1.75). 머신 칸과 함께 owner `모델@머신`으로 적는다. `owner`(me·사람)와 함께 주면 거부한다
 */
profile: string | null,
/**
 * 머신 칸(FRONT_API 1.75)
 */
machine: string | null, notes: Array<string>, parent: TaskAnchor | null, before: TaskAnchor | null,
/**
 * 이어 줄 thread(` · t-NNNN`, FRONT_API 1.63 「task로 만들기」). 옛 host는 버린다
 */
thread: string | null,
/**
 * 저장소 칸(FRONT_API 1.73): 저장소 이름·경로나 `작업 폴더`. 경로는 host가 이름으로 바꿔 적는다. 비움 = 정하지 않음
 */
repo: string | null, } | { "op": "update", at: TaskAnchor, title: string | null, owner: string | null, person: boolean,
/**
 * 모델 칸(FRONT_API 1.75): `None`은 그대로, 빈 글은 비움. 머신 칸만 바꾸면 모델은 그대로 둔다
 */
profile: string | null,
/**
 * 머신 칸(FRONT_API 1.75, `profile`과 같은 규칙)
 */
machine: string | null, notes: Array<string> | null, thread: string | null, done: boolean | null,
/**
 * 저장소 칸(FRONT_API 1.73, Add의 `repo`와 같은 값)
 */
repo: string | null, } | { "op": "delete", at: TaskAnchor, } | { "op": "move", at: TaskAnchor, to_list: string, before: TaskAnchor | null, parent: TaskAnchor | null, } | { "op": "list_add", name: string, after: string | null, } | { "op": "list_rename", name: string, to: string, } | { "op": "list_merge", name: string, into: string, } | { "op": "list_delete", name: string, } | { "op": "list_move", name: string, before: string | null, } | { "op": "restore", log_id: number, list: string | null, } | { "op": "raw_save", expected_sha256: string, text: string, };

export type TaskConflict = {
/**
 * 맞지 않은 동작의 차례(0부터)
 */
op: number, reason: string,
/**
 * 그 task의 지금 원문(같은 제목으로 찾은 것, 지워졌으면 없음)
 */
latest: TaskView | null, };

export type TaskAction = "add" | "update" | "delete" | "move" | "list_add" | "list_rename" | "list_merge" | "list_delete" | "list_move" | "restore" | "raw" | "auto_clean" | "completed";

export type TaskLogEntry = { id: number, at_ms: number,
/**
 * 누가(원문): `device:<id>`, `coordinator:<pane>`, `local:…`, `file`(파일 직접), `host`(자동 정리)
 */
actor: string,
/**
 * 누가(보이는 이름): 기기 이름·coordinator·터미널·파일 직접·host 자동 정리
 */
by: string, action: TaskAction, list: string,
/**
 * 옮김·목록 동작의 앞 목록(옮기기 전 목록, 이름 바꾸기·합치기 전 이름)
 */
from_list: string | null, title: string,
/**
 * 바뀌기 전 블록 원문(하위 task 포함, 더하기에는 없다)
 */
before: string | null, after: string | null, thread: string | null,
/**
 * 되살린 기록이면 그 항목 id
 */
restored_by: number | null, };

export type TaskLogFilter = "deleted" | "auto";

export type ThreadStartPreview = { profile_revision: string, model: string, effort: string,
/**
 * task 제목(thread 제목이 된다)
 */
title: string,
/**
 * 지금 파일의 그 task 블록 hash(시작 요청의 `task_block`)
 */
task_block: string,
/**
 * thread에 줄 할 일 글(제목과 「Notes from the task list」 메모)
 */
task: string,
/**
 * owner가 정한 profile, 없으면 프로젝트 기본 thread profile
 */
profile: string,
/**
 * owner가 정한 머신(없으면 host가 저장소의 머신이나 로컬로 고른다)
 */
machine: string | null,
/**
 * project.toml `repos`(worktree로 시작할 저장소). 비면 작업 폴더로 시작한다
 */
repos: Array<RepoChoice>,
/**
 * 열린(시작 중 포함) thread 수와 project.toml `max_parallel_threads`. 한도는 coordinator 규칙이라 host는 막지 않는다
 */
open_threads: number, max_parallel_threads: number,
/**
 * 이미 열린 thread가 이 task를 맡았다 등
 */
warnings: Array<string>,
/**
 * task 저장소 칸(FRONT_API 1.73, `TaskView.repo`). 비면 창이 저장소나 작업 폴더를 고르게 한다
 */
repo: string | null,
/**
 * host 로컬 머신 이름(`RepoChoice.machine`이 빈 저장소의 머신)
 */
local_machine: string | null,
/**
 * 덜 붐비는 차례의 머신(연결·점검 이상 없음, 모든 프로젝트의 열린 thread 적은 차례 → 부하 → 이름). owner가 머신을 정하지 않았으면
 * 그 저장소가 있는 머신 가운데 앞의 것이 기본값이다
 */
machine_order: Array<string>, };

export type RepoChoice = { path: string,
/**
 * 그 저장소가 있는 머신(없으면 host 로컬)
 */
machine: string | null, };

export type OwnerChoice = { machine: string, connected: boolean,
/**
 * 끊긴 머신은 비어 있다(모른다)
 */
profiles: Array<string>, };

export type ProjectContext = {
/**
 * inbox를 끝내도 해제 전까지 남는 활성 post 음소거 경고
 */
post_warnings: Array<string>, project: string, path: string,
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
 * 채팅에 쓸 이 host 웹 주소(tailscale serve 공개 origin, plan §8.345). 질문·보고서는 `<주소>/#question=<thread>`·
 * `<주소>/#report=<이름>` Markdown 링크로 가리킨다(터미널에서도 눌린다). 공개하지 않았으면 없다: 그때는 `aaa:` 꼴을 쓴다
 */
web_url?: string, };

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
 * 사람이 읽는 이름(예: `웹(ab12)`)
 */
label: string, };

export type PaneView = {
/**
 * host에 고정한 인증 별칭. 이전 판 thread의 인증은 ThreadView.account로 확인한다.
 */
account?: string, id: string, machine_id: string, machine: string, name: string, argv: Array<string>, cwd: string | null, connection: Connection,
/**
 * 지금 입력할 수 없으면 true(끊김·종료·권한). lease는 따로 본다.
 */
readonly: boolean, cols: number, rows: number, lease: LeaseView | null, role: Role | null,
/**
 * host 머신의 프로젝트 홈에서 도는 coordinator의 소속. host가 canonical cwd로 판정한다.
 * 옛 host는 보내지 않으며 front는 이름·경로로 소속을 추측하지 않는다.
 */
coordinator_project?: string,
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
restore_error?: string,
/**
 * 이 pane이 대신한 옛 pane ID(`pane resume`·재부팅 복원은 새 세션을 띄우고 옛 기록을 지운다, 오래된 것부터). 옛 ID를 보던
 * 화면은 이것으로 새 pane을 따라간다(QA Q19).
 */
replaced?: Array<string>, } & ({ "state": "running" } | { "state": "exited", code: number | null, } | { "state": "interrupted" } | { "state": "lost" });

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

export type MissionView = { project: string, name: string, title: string, thread: ThreadView | null, state: string, blocked_reason: MissionBlockReason | null, applied_sha: string | null,
/**
 * reload로 예약한 판. restart만 요청했으면 현재 적용 판이다.
 */
pending_sha: string | null,
/**
 * 적용 전이면 실제 대기 이유. 요청 수락과 적용 완료를 구분한다.
 */
apply_wait: string | null, file_sha: string | null, approval_sha256: string, approved: boolean, stale: boolean, file_text: string, applied_text: string,
/**
 * TOML의 brief_file에서 읽은 글. 옛 md에서는 비워 중복 표시하지 않는다.
 */
brief_text: string, applied_brief_text: string, diff: string, error: string | null, wakes_today: number,
/**
 * 0이면 하루 깨움 상한이 없다.
 */
max_wakes: number,
/**
 * 누적 작업 분. 깨움당 시간 제한은 wake_work_minutes에 적용한다.
 */
work_minutes: bigint, max_minutes: number, wake_work_minutes: bigint, input_bytes: bigint, posts: number, handled_posts: number, pending: number, next_reset_ms: number | null, usage_wait_until_ms: number | null, };

export type MissionBlockReason = "question" | "message_unknown" | "wake_unknown" | "brief_unknown" | "restart_failed" | "legacy_unknown";

export type PostType = "notice" | "data" | "request";

export type PostInput = { post_id: string, to: string, post_type: PostType, topic: string, title: string, body: string, needs_user: boolean, refs: Array<string>, };

export type PostResult = { "state": "accepted", id: string, } | { "state": "refused", reason: string, existing: string | null, } | { "state": "queued", post_id: string, };

export type PostSender = { thread: string | null, role: Role, mission: string | null, machine: string, session: string | null, verified: string, };

export type PostEnvelope = { id: string, project: string, from: PostSender, caused_by: Array<string>, body_sha256: string, created_ms: number, state: string, post_id: string, to: string, post_type: PostType, topic: string, title: string, body: string, needs_user: boolean, refs: Array<string>, };

export type DiffScope = "branch" | "uncommitted";

export type DiffFileRef = { path: string, old_path?: string, };

export type DiffQuery = { scope?: DiffScope,
/**
 * `--ignore-all-space`(t3code 패널 기본 켬, 카드 숫자는 끔)
 */
ignore_whitespace?: boolean,
/**
 * patch도 읽는다. 끄면 파일 목록과 숫자만(카드)
 */
patch?: boolean,
/**
 * 이 파일의 patch만(파일 목록은 비운다)
 */
file?: DiffFileRef, };

export type DiffStatus = "added" | "deleted" | "modified" | "renamed" | "copied" | "type_changed" | "unmerged" | "untracked";

export type DiffFile = {
/**
 * 저장소 맨 위 기준 경로(`/`)
 */
path: string, old_path?: string, status: DiffStatus,
/**
 * 바이너리면 없다(numstat `-`)
 */
additions: number | null, deletions: number | null, };

export type DiffView = { scope: DiffScope,
/**
 * 비교 기준의 이름: branch 범위는 base ref(`origin/main`), 미커밋은 `HEAD`
 */
base: string,
/**
 * 비교한 commit(merge-base 또는 HEAD). 첫 커밋 전이면 빈 tree
 */
base_sha: string,
/**
 * 지금 branch 이름(분리된 HEAD면 `HEAD`)
 */
head: string,
/**
 * 바뀐 파일(경로 차례). 너무 많으면 없다(`truncated`)
 */
files: Array<DiffFile> | null, patch: string | null,
/**
 * patch를 상한에서 잘랐거나 파일 목록이 모자라다
 */
truncated: boolean,
/**
 * 새 파일이 상한보다 많아 새 파일은 뺐다
 */
untracked_omitted: boolean, };

export type FrontRequest = { "op": "account_import", name: string, harness: string, secret: SecretText, } | { "op": "account_list" } | { "op": "account_defaults" } | { "op": "account_default", harness: string, account: string | null, } | { "op": "account_login", name: string, harness: string, } | { "op": "account_login_status", id: string, } | { "op": "account_login_cancel", id: string, } | { "op": "account_login_confirm", id: string, switch: boolean, } | { "op": "account_remove", name: string, } | { "op": "thread_start_account", account: string, start: FrontRequest, } | { "op": "debug_input", pane: string | null, thread: string | null, since_ms: bigint, } | { "op": "rollout_start", id: string, spec: RolloutSpec, } | { "op": "rollout_status", id: string, } | { "op": "deploy_context" } | { "op": "deploy_start", machine: string | null, id: string, action: DeployAction, } | { "op": "deploy_status", machine: string | null, id: string, } | { "op": "rollout_plan", source_machine: string, path: string,
/**
 * 비었으면 등록된 모든 머신
 */
machines: Array<string>, } | { "op": "rollouts" } | { "op": "deploy_jobs" } | { "op": "post", project?: string, input: PostInput, } | { "op": "post_status", project?: string, post_id: string, } | { "op": "post_handled", project?: string, post_id: string, } | { "op": "post_list", project?: string, } | { "op": "asks", project?: string, } | { "op": "ask_handle", project?: string, item: string, } | { "op": "ask_forget", project?: string, ids: Array<string>, all: boolean, } | { "op": "post_unmute", project?: string, thread: string, } | { "op": "status" } | { "op": "machines" } | { "op": "panes", role: Role | null, } | { "op": "pane_run", machine: string | null, name: string | null, argv: Array<string>, cwd: string | null, cols: number | null, rows: number | null,
/**
 * host 정본 profile로 harness를 띄운다(`argv`는 비운다, herdr-web-ui 새 세션의 agent 고르기)
 */
profile?: string, } | { "op": "pane_read", pane: string, } | { "op": "pane_chat", pane: string,
/**
 * 생각도 싣는다(웹 설정 「생각 표시」, plan §8.129)
 */
thinking: boolean,
/**
 * 정리된 자리(`ChatRole::Compact`, plan §8.309, 1.61)도 싣는다. 옛 front는 생략해 그 줄을 받지 않는다(모르는 role로 답 전체를
 * 읽지 못하므로)
 */
compact: boolean,
/**
 * harness 명령과 결과(`ChatRole::Command`·`CommandOutput`, plan §8.323, 1.68)도 싣는다. 옛 front는 생략한다
 */
commands: boolean,
/**
 * 이전 응답의 before. 생략하면 최신 쪽, 옛 상대는 무시한다.
 */
before?: string, } | { "op": "pane_commands", pane: string, } | { "op": "pane_file", pane: string, path: string, } | { "op": "thread_file", project?: string, thread: string, path: string, } | { "op": "thread_diff", project?: string, thread: string, query?: DiffQuery, } | { "op": "pane_dir", pane: string, path: string, hidden: boolean, } | { "op": "pane_send", pane: string, data: B64, } | { "op": "pane_read_text", pane: string, source: ReadSource,
/**
 * 줄 수(기본: Recent 80줄, Visible은 화면 전부, 최대 1000)
 */
lines: number | null, format: ReadFormat, } | { "op": "pane_send_text", pane: string, text: string, } | { "op": "pane_send_keys", pane: string, keys: Array<string>, } | { "op": "pane_submit", pane: string, text: string, } | { "op": "pane_wait_output", pane: string, match: OutputMatch, source: ReadSource, lines: number | null,
/**
 * 없으면 끝없이 기다린다(herdr와 같다)
 */
timeout_ms: number | null, } | { "op": "pane_get", pane: string, } | { "op": "pane_close", pane: string, } | { "op": "workspaces" } | { "op": "workspace_close", name: string, } | { "op": "workspace_rename", name: string, to: string, } | { "op": "pane_resize", pane: string, cols: number, rows: number, } | { "op": "pane_kill", pane: string, } | { "op": "pane_input", pane: string, data: B64, } | { "op": "pane_ctrl_enter", pane: string, trace: string | null, } | { "op": "pane_paste", pane: string, text: string, } | { "op": "pane_take", pane: string, } | { "op": "pane_release", pane: string, } | { "op": "pane_attach", pane: string, offset: number, total: number, sha256: string, data: B64, } | { "op": "pane_resume", pane: string, } | { "op": "pane_forget", pane: string, } | { "op": "pane_rename", pane: string, label: string | null, } | { "op": "workspace", name: string | null, } | { "op": "layout_apply", workspace: string | null, change: LayoutChange, } | { "op": "enroll_open", minutes: number, } | { "op": "enroll_close" } | { "op": "safety_show", project?: string, scope: SettingsScope, } | { "op": "safety_set", project?: string, scope: SettingsScope, key: string, value: string | null, expected: string, } | { "op": "profile_admin" } | { "op": "profile_plan", project?: string, change: ProfileChangeReq, } | { "op": "profile_apply", project?: string, change: ProfileChangeReq, expected: string, } | { "op": "work_root_show", machine: string, } | { "op": "work_root_check", machine: string, path: string | null, mount: string | null, } | { "op": "work_root_set", machine: string, path: string, mount: string | null, } | { "op": "hook_status", machine: string, } | { "op": "hook_set", machine: string, install: boolean, } | { "op": "enroll_pending" } | { "op": "enroll_approve", name: string, code: string, } | { "op": "machine_revoke", name: string, } | { "op": "web_pair", role: DeviceRole, } | { "op": "devices" } | { "op": "device_rename", id: string, label: string, } | { "op": "device_revoke", id: string, } | { "op": "web_publish" } | { "op": "web_publish_off" } | { "op": "web_status" } | { "op": "project_create", name: string, goal: string,
/**
 * `PATH` 또는 `PATH@MACHINE`(머신이 없으면 host 머신, 상대 경로는 CLI가 절대 경로로 바꿔 보낸다)
 */
repos: Array<string>, thread_profile: string | null, coordinator_profile: string | null, } | { "op": "project_set", project?: string, key: string, value: string, } | { "op": "projects", all?: boolean, } | { "op": "instance" } | { "op": "project_lifecycle", project: string, action: ProjectAction, } | { "op": "project_delete", project: string,
/**
 * 살아 있는 pane이 있어도
 */
force: boolean, } | { "op": "project_rename", project: string, to: string, dry_run: boolean, } | { "op": "project_sweep", project: string, dry_run: boolean, } | { "op": "project_files", fix: boolean, } | { "op": "project_open", project?: string,
/**
 * 안 주면 project.toml `coordinator_profile` → host.toml `[defaults] coordinator_profile` → claude
 */
profile?: string,
/**
 * coordinator가 돌아도 하나 더 띄운다
 */
new?: boolean, } | { "op": "profiles", machine: string | null, } | { "op": "profile_resolve", name: string | null, } | { "op": "thread_start", holds: Array<string>,
/**
 * 준비 완료를 기다리지 않고 Starting 기록을 돌려준다.
 */
background: boolean, project?: string, title: string, machine: string | null,
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
from_task: string | null,
/**
 * 시작하면 그 task 줄 끝에 ` · t-NNNN`을 붙인다(FRONT_API 1.63, 할 일 T3). 연결은 시작할 때 읽은 블록 hash로 하고,
 * 그사이 블록이 바뀌었으면 시작은 그대로 두고 경고만 한다. 옛 host는 버린다(연결하지 않는다)
 */
link_task?: boolean,
/**
 * 화면이 본 `from_task`의 블록 hash(FRONT_API 1.63). 지금 파일의 블록과 다르면 시작하지 않는다(본 것과 다른 일을 맡기지 않게)
 */
task_block?: string, } | { "op": "thread_start_preview", project?: string, from_task: string, task_block?: string, } | { "op": "thread_adopt", project?: string, pane: string, title: string,
/**
 * 할 일. 비면 「하던 일을 이어 하되 이제 이 프로젝트 thread다」
 */
task: string, } | { "op": "thread_next", project?: string, thread: string, line: number | null, add: string | null, } | { "op": "context", project?: string, peek: boolean, } | { "op": "assignable", project?: string, check: string | null, } | { "op": "tasks", project?: string, } | { "op": "task_queue", project?: string, action: QueueOp, } | { "op": "task_archive", project?: string,
/**
 * 비면 보기, 있으면 같은 ID로 활성 목록에 복원(예약은 복원하지 않는다).
 */
restore: string | null, } | { "op": "task_edit", project?: string, ops: Array<TaskOp>, base_sha256: string | null, } | { "op": "task_log", project?: string, before: number | null, limit: number | null, filter: TaskLogFilter | null, } | { "op": "task_log_prune", project?: string, before_ms: number, } | { "op": "threads", project: string | null, role: Role | null, } | { "op": "thread_show", project?: string, thread: string, } | { "op": "thread_summary", project?: string, thread: string, } | { "op": "thread_summary_set", project?: string, thread: string, text: string, } | { "op": "thread_library", project?: string, thread: string, } | { "op": "library_link", project?: string, thread: string, days: number | null, } | { "op": "library_links", project: string | null, } | { "op": "library_link_revoke", id: string, } | { "op": "library_publish", project?: string, name: string, source: string, live: boolean,
/**
 * FRONT_API 1.43. 사용자에게 보내는 웹 보고서(index.html 필수, 「내 보고서」·안 봄·사용자 알림 post)
 */
to_user: boolean,
/**
 * FRONT_API 1.43. 「내 보고서」에 보일 제목(없으면 이름)
 */
title: string | null,
/**
 * FRONT_API 1.56. 보고서를 낸 thread(`t-NNNN`). 없으면 host가 원본 경로·이름에서 그 프로젝트의 thread를 찾는다
 */
from_thread?: string, } | { "op": "library_delete", reports: Array<LibraryRef>, } | { "op": "library_import", project?: string, source: string,
/**
 * 0.2의 t- 이름을 새 thread id와 구분해 가져올 때 쓴다.
 */
prefix: string, } | { "op": "library_prune", project: string | null, older_than_days: number, } | { "op": "library_prune_preview", project: string | null, older_than_days: number | null, } | { "op": "library_rotate" } | { "op": "library_upload_publish", project?: string, upload: string, name: string, files: number, to_user: boolean, title?: string, } | { "op": "library_reports", project: string | null, } | { "op": "library_open", project?: string, name: string, } | { "op": "library_resolve", project?: string, cap: string, } | { "op": "library_usage" } | { "op": "thread_ack", project?: string, thread: string, } | { "op": "thread_resolve", project?: string, thread: string, keep_worktree: boolean,
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
discard_uncopied: boolean, } | { "op": "thread_stop", project?: string, thread: string, } | { "op": "thread_restart", project?: string, thread: string, resume: boolean,
/**
 * 이 profile로 새 대화를 띄운다(herdr-projects `thread restart --profile`). `resume`과 함께 쓰지 않는다
 */
profile: string | null, } | { "op": "thread_brief", project?: string, thread: string, } | { "op": "thread_read", project?: string, thread: string, lines: number | null, } | { "op": "thread_keys", project?: string, thread: string, keys: Array<string>, text: string | null, } | { "op": "questions", all: boolean, } | { "op": "question_answer", id: string, choice: string, menu: string | null, pick: MenuPick | null, } | { "op": "thread_message", project?: string, thread: string, text: string, mode: MessageMode, } | { "op": "thread_answer", project?: string, thread: string,
/**
 * 답하는 질문의 보고 hash(`ThreadQuestions.report_hash`)
 */
report_hash: string,
/**
 * 질문마다 고른 선택지(0부터). 직접 쓴 답으로 대신하는 질문은 null
 */
picks: Array<number | null>, text: string | null, } | { "op": "preferences" } | { "op": "preferences_set", settings: Record<string, unknown>, pins: Array<string> | null, } | { "op": "inbox", project?: string, all: boolean, peek: boolean, } | { "op": "inbox_done", project?: string, ids: Array<string>, all: boolean, } | { "op": "ticker_tick" } | { "op": "mission_list", project: string | null, } | { "op": "mission_show", project?: string, name: string, } | { "op": "mission_approve", project?: string, name: string, approval_sha256: string, } | { "op": "mission_reject", project?: string, name: string, } | { "op": "mission_start", project?: string, name: string, } | { "op": "mission_stop", project?: string, name: string, } | { "op": "mission_pause", project?: string, name: string, } | { "op": "mission_resume", project?: string, name: string, } | { "op": "mission_reload", project?: string, name: string, } | { "op": "mission_restart", project?: string, name: string, } | { "op": "mission_open", project?: string, name: string, } | { "op": "routines", project?: string, } | { "op": "automation_approve", project?: string, name: string, approval_sha256: string, } | { "op": "automation_convert", project?: string, dry_run?: boolean, plan_sha256?: string, } | { "op": "automation_unblock", project?: string, name: string, batch: string, discard: boolean, } | { "op": "automation_read", project?: string, name: string, } | { "op": "automation_validate", project?: string, name: string, text: string, form?: Record<string, unknown>, body?: string, } | { "op": "automation_save", project?: string, name: string, text: string, expected_sha256?: string, body?: string,
/**
 * 본문 파일의 본 판. 없으면 그 파일이 아직 없어야 한다
 */
body_expected_sha256?: string, } | { "op": "automation_delete", project?: string, name: string, expected_sha256: string, } | { "op": "automation_restore", project?: string, id: string, } | { "op": "automation_run", project?: string, name: string, key: string, } | { "op": "project_settings", project?: string, } | { "op": "project_repo_edit", project?: string, from: string, to: string, } | { "op": "thread_resolve_preview", project?: string, thread: string, } | { "op": "library_prune_apply", project: string | null, older_than_days: number | null, expected: Array<string>, } | { "op": "routine_runs", project?: string, name: string, } | { "op": "routine_approve", project?: string, name: string, command_sha256: string, } | { "op": "routine_toggle", project?: string, name: string, on: boolean | null, } | { "op": "events", project: string | null, after: number, limit: number | null,
/**
 * 가장 최근 `limit`개(차례는 그대로 오래된 것부터, FRONT_API 1.77). 웹 알림함 「흐름 사건」이 쓴다. 옛 host는 버린다
 */
tail?: boolean, } | { "op": "evaluator_assign", project?: string, profile: string, machine: string | null,
/**
 * 개선을 직접 고칠 repo(없으면 repo 없는 작업 폴더)
 */
repo: string | null,
/**
 * 평가자에게 더할 말(brief 끝에 붙는다)
 */
note: string | null, } | { "op": "evaluator_status", project: string | null, } | { "op": "evaluator_unassign", project?: string, } | { "op": "evaluations", project?: string, all: boolean, } | { "op": "evaluation_show", project?: string, id: string, } | { "op": "evaluation_decide", project?: string, id: string, proposals: Array<string>, adopt: boolean, note: string | null, };

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

export type RolloutSpec = { from: string,
/**
 * Some이면 이 머신의 빌드 후보를 Agent link로 가져온다. None은 L1 미리 복사 경로다.
 */
source_machine?: string | null, sha256: string, commit: string | null, machines: Array<string>, };

export type RolloutPhase = "transfer" | "preflight" | "host_update" | "host_restart" | "agent_update" | "agent_restart";

export type RolloutStep = { machine: string, phase: RolloutPhase, job_id: string, completed: boolean, result: string | null,
/**
 * 재시작 전에 기록한 host instance. OS boot ID와 별개다.
 */
host_instance: string | null, started_ms: number, transferred_bytes: number, total_bytes: number, baseline: RuntimeIdentity | null, guard: DeployGuardSpec | null, guard_ready: boolean, };

export type RolloutState = "running" | "recovering" | "stopped" | "stopped_rolled_back" | "recovery_failed" | "done";

export type RolloutView = { id: string, project: string, actor: string, spec: RolloutSpec, state: RolloutState, steps: Array<RolloutStep>, cursor: number, version: string | null, error: string | null, finish: string | null,
/**
 * 없는 옛 rollout은 L1/L2 정책으로만 재개한다.
 */
recovery_enabled: boolean, recovery: DeployGuardView | null, recovery_started_ms: number, created_ms: number, updated_ms: number, };

export type RolloutCard = { id: string, state: RolloutState, version: string | null, sha256: string, commit: string | null, cursor: number, steps: Array<RolloutCardStep>, error: string | null, recovery_phase: DeployGuardPhase | null, recovery_sha256: string | null, created_ms: number, updated_ms: number, };

export type RolloutCardStep = { machine: string, phase: RolloutPhase, completed: boolean, started_ms: number, transferred_bytes: number, total_bytes: number, };

export type DeployCandidateView = {
/**
 * 빌드 머신 이름
 */
machine: string,
/**
 * 받은 그대로의 후보 경로(`<절대 경로>/aaa` 또는 `~/<폴더>/aaa`)
 */
path: string, sha256: string,
/**
 * provenance의 40자리 커밋(없으면 None)
 */
commit: string | null,
/**
 * self-check가 답한 판(`0.1.2+<커밋 12자리>`)
 */
version: string, total_bytes: number,
/**
 * provenance의 빌드 시각(RFC 3339, 없으면 None)
 */
built_at: string | null, };

export type RolloutTargetView = { machine: string,
/**
 * host 머신(늘 맨 앞)
 */
host: boolean, problem: string | null, };

export type RolloutPlanView = { candidate: DeployCandidateView, targets: Array<RolloutTargetView>,
/**
 * 진행 중인 rollout이 있어 지금은 시작하지 못한다
 */
busy: boolean, };

export type DeployWork = { "kind": "restart", service: DeployService, } | { "kind": "update" } | { "kind": "rollback" };

export type DeployJobCard = { id: string, machine: string, work: DeployWork,
/**
 * 그 머신 agent에게 물은 상태. 연결이 끊겼거나 기록을 읽지 못하면 None
 */
state: DeployState | null,
/**
 * 실패·중단일 때 화면에 보일 까닭(원문은 coordinator의 배포 기록에만 있다)
 */
error: string | null,
/**
 * 웹 조작 기기가 시작했다(아니면 coordinator)
 */
web: boolean, created_ms: number, updated_ms: number, };

export type FrontResponse = { "kind": "accounts", accounts: Array<AccountView>, } | { "kind": "account_defaults", defaults: Array<AccountDefault>, } | { "kind": "account_login", login: AccountLoginView, } | { "kind": "input_events", events: Array<InputEvent>, } | { "kind": "deploy_context", project: string | null, session: boolean, allowed: boolean, } | { "kind": "rollout", rollout: RolloutView, } | { "kind": "rollout_cards", cards: Array<RolloutCard>, } | { "kind": "rollout_plan", plan: RolloutPlanView, } | { "kind": "deploy_jobs", jobs: Array<DeployJobCard>, } | { "kind": "deployment", machine: string, job: DeployJob, running_software: string, running_sha256: string, } | { "kind": "missions", missions: Array<MissionView>, } | { "kind": "mission", mission: MissionView, } | { "kind": "post", result: PostResult,
/**
 * status 조회에서 확인한 inbox 처리 상태(FRONT_API 1.84). 옛 host는 생략한다.
 */
handled?: boolean, } | { "kind": "posts", posts: Array<PostEnvelope>,
/**
 * 음소거된 보낸 쪽(`post unmute`에 줄 이름, FRONT_API 1.77). 옛 host는 생략한다
 */
muted?: Array<string>, } | { "kind": "asks", requests: Array<UserRequestView>, done: Array<AskRecord>, keep_days: number, } | { "kind": "status" } & HostStatus | { "kind": "machines", machines: Array<MachineView>, } | { "kind": "panes", panes: Array<PaneView>, } | { "kind": "pane" } & PaneView | { "kind": "next", lines: Array<string>,
/**
 * `line`으로 보낸 메시지
 */
message: MessageView | null, } | { "kind": "context" } & ProjectContext | { "kind": "assignable", owners: Array<OwnerChoice>,
/**
 * `check`가 통과했을 때 설명
 */
valid: string | null, } | { "kind": "task_board" } & TaskBoard | { "kind": "task_queue", entries: Array<TaskReservation>, } | { "kind": "task_archive", entries: Array<ArchivedTask>, } | { "kind": "task_edited", board: TaskBoard, conflict: TaskConflict | null, log_ids: number[], } | { "kind": "thread_start_preview" } & ThreadStartPreview | { "kind": "task_log", entries: Array<TaskLogEntry>, next: number | null, } | { "kind": "screen" } & ScreenText | { "kind": "read" } & ReadView | { "kind": "thread_pane" } & ThreadPaneView | { "kind": "output_matched", matched_line: string, read: ReadView, } | { "kind": "workspaces", workspaces: Array<WorkspaceSummary>, } | { "kind": "chat" } & ChatView | { "kind": "pane_file", info: FileInfo | null, candidates: Array<string>, url: string | null, } | { "kind": "pane_dir", listing: DirListing, } | { "kind": "pane_commands", list: CommandList, } | { "kind": "pending", pending: Array<PendingEnroll>,
/**
 * 등록 창이 열려 있으면 닫히는 시각(ms, FRONT_API 1.80). 닫혔으면 생략
 */
open_until_ms?: number, } | { "kind": "pair_code", code: string, role: DeviceRole, expires_in_s: number, } | { "kind": "publish" } & PublishState | { "kind": "devices", devices: Array<DeviceView>, } | { "kind": "workspace" } & Workspace | { "kind": "done" } | { "kind": "attached", path: string, inserted: boolean, note: string | null, } | { "kind": "projects", projects: Array<ProjectView>, } | { "kind": "instance", project: ProjectView | null, } | { "kind": "project" } & ProjectView | { "kind": "project_change" } & ProjectChange | { "kind": "project_opened" } & ProjectOpened | { "kind": "project_files", checks: Array<ProjectFilesCheck>, } | { "kind": "profiles", machine: string, profiles: Array<ProfileView>, } | { "kind": "profile_resolved", profile: ProfileView, } | { "kind": "threads", threads: Array<ThreadView>, } | { "kind": "thread" } & ThreadView | { "kind": "thread_summary" } & ThreadSummaryView | { "kind": "events", events: Array<FlowEvent>, } | { "kind": "questions", questions: Array<QuestionView>, } | { "kind": "question" } & QuestionView | { "kind": "message" } & MessageView | { "kind": "inbox", items: Array<InboxItem>,
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
omitted_done: number, } | { "kind": "ticker", runs: bigint, } | { "kind": "routines", routines: Array<RoutineView>,
/**
 * 지운 자동화(FRONT_API 1.76). 옛 host는 생략한다
 */
trash?: Array<AutomationTrashEntry>, } | { "kind": "automation_doc", doc: AutomationDoc, } | { "kind": "automation_checked", text: string, check: AutomationCheck, } | { "kind": "automation_ran", accepted: boolean, detail: string, } | { "kind": "project_settings" } & ProjectSettingsView | { "kind": "safety" } & SafetyView | { "kind": "profile_admin" } & ProfileAdminView | { "kind": "profile_plan" } & ProfilePlanView | { "kind": "profile_applied", message: string, warnings: Array<string>, view: ProfileAdminView, } | { "kind": "work_root" } & WorkRootView | { "kind": "work_root_checked" } & WorkRootCheckView | { "kind": "hook" } & HookView | { "kind": "resolve_preview" } & ResolvePreview | { "kind": "automation_converted", files: Array<string>,
/**
 * 쓰지 않은 계획이다(FRONT_API 1.58). `files`는 실제 변환과 같은 모양(새 파일과 `.converted/` 보존 사본)이다.
 */
dry_run: boolean,
/**
 * 계획 hash. 실행 요청에 실어 보내면 그 사이 바뀐 계획을 거부한다.
 */
plan_sha256: string, } | { "kind": "routine_runs", runs: Array<RoutineRun>,
/**
 * 파일 원문(FRONT_API 1.58). 임무 조건·깨진 파일·옛 host는 없다.
 */
source: RoutineSource | null, } | { "kind": "library" } & LibraryView | { "kind": "thread_diff", project: string, thread: string, diff: DiffView, } | { "kind": "library_link" } & LinkView | { "kind": "library_resolved", project: string, name: string, } | { "kind": "library_reports", reports: Array<LibraryReport>, } | { "kind": "library_imported", reports: Array<LibraryReport>, skipped: Array<LibrarySkip>, } | { "kind": "library_pruned", removed: Array<string>, } | { "kind": "library_prune_preview" } & LibraryPrunePlan | { "kind": "library_rotated" } | { "kind": "library_usage", size: number, reports: number, live: number, problems: number, max_size: number, max_days: number, protected_size: number, } | { "kind": "library_links", links: Array<LinkView>, } | { "kind": "evaluators", evaluators: Array<EvaluatorView>, } | { "kind": "evaluations", evaluations: Array<EvaluationView>, } | { "kind": "evaluation" } & EvaluationView | { "kind": "preferences" } & SharedPreferences;

export type FrontEvent = { "ev": "state", machines: Array<MachineView>, panes: Array<PaneView>, } | { "ev": "workspace" } & Workspace | { "ev": "flow" } & FlowEvent | { "ev": "thread" } & ThreadView | { "ev": "question" } & QuestionView | { "ev": "preferences" } & SharedPreferences | { "ev": "pong", id: number, } | { "ev": "web", build: string, } | { "ev": "reports", unseen: number, } | { "ev": "asks", requests: number, } | { "ev": "inbox" } | { "ev": "accounts" } | { "ev": "enroll" } | { "ev": "tasks", project: string, sha256: string, } | { "ev": "app_badge", count: number, } | { "ev": "rollout", result: Outcome<RolloutCard | null>, } | { "ev": "notice" } & NoticeView | { "ev": "chat_changed", pane: string, signature: string, } | { "ev": "chat_watching", pane: string, live: boolean, };

export type SubmitStage = "not_sent" | "no_enter" | "too_late";

export type SteerRefusal = "not_working" | "question" | "draft" | "no_queue" | "unknown_screen" | "unsupported" | "failed" | "compacting";

export type TermServerMsg = { "t": "snapshot", replay: B64, cols: number, rows: number, readonly: boolean, stream: number,
/**
 * replay에 싣지 않은 앞 기록이 host에 더 있다: 위로 굴려 끝에 닿으면 `History`로 받아 온다(plan §8.67)
 */
history_more: boolean, } | { "t": "history", id: number, data: B64, lines: number, more: boolean, found: boolean, } | { "t": "readonly", readonly: boolean, reason: string | null, } | { "t": "state", pane: PaneView, } | { "t": "front", id: string,
/**
 * 대기 메시지 「지금 보내기」(`SteerQueued`)를 안다(1.45). 옛 host는 이 칸이 없어 웹이 단추를 숨긴다
 */
steer_queued: boolean, } | { "t": "attached", path: string, inserted: boolean, note: string | null, } | { "t": "submitted", id: number, ok: boolean, message: string | null, } | { "t": "submit_cancelled", id: number, stage: SubmitStage, } | { "t": "steer_queued", id: number, sent: boolean, reason: SteerRefusal | null, message: string | null, } | { "t": "error" } & ErrorBody;

export type TermClientMsg = { "t": "input", data: string, } | { "t": "ctrl_enter", trace: string | null, } | { "t": "resize", cols: number, rows: number, } | { "t": "take" } | { "t": "release" } | { "t": "submit", id: number, data: string, trace: string | null, meta: InputSubmitMeta,
/**
 * 입력창의 Ctrl+Enter(사용자 10-19 15:3x, plan §8.239 후속): 글 뒤 Enter 대신 host가 steering 키를 고른다(작업 중인 Claude는
 * sendNow, 그 밖은 Enter). 옛 host는 이 칸을 버려 보통 보내기가 된다
 */
steer: boolean, } | { "t": "input_trace", id: string, event: InputTraceEvent, method: InputReceiptMethod | null, } | { "t": "steer_queued", id: number, trace: string | null, } | { "t": "submit_cancel", id: number, } | { "t": "history", id: number, anchor: Array<string>, depth: number, } | { "t": "ack", stream: number, offset: number, } | { "t": "attach", offset: number, total: number, sha256: string, data: B64, } | { "t": "colors", fg: string, bg: string, };

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
focus?: boolean,
/**
 * 이 웹 빌드(주 묶음 파일 이름). 배포 뒤에도 옛 페이지가 도는지 본다
 */
build?: string, } | { "t": "alert", project: string | null, target: string, kind: string, outcome: string, } | { "t": "touch" } | { "t": "ping", id: number, } | { "t": "chat_watch", pane: string, enabled: boolean, };

export type Role = "coordinator" | "worker" | "evaluator";

export type ThreadKind = "worktree" | "dir" | "adopted";

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

export type ProfileView = { name: string, harness: string, model: string | null, effort: string | null, description: string,
/**
 * profile이 붙이는 인자(옛 agent는 보내지 않는다)
 */
args: Array<string>,
/**
 * host.toml에 없는 기본 profile(claude·codex): 그 머신에 설치·로그인한 것만 온다
 */
builtin: boolean, };

export type LibFile = { path: string, size: number, sha256: string, };

export type Skipped = { path: string, reason: string,
/**
 * 복사 상한을 넘어 agent에 남긴 일반 파일의 크기(이것만 나중에 지연 스트리밍으로 볼 수 있다)
 */
lazy_size: number | null, };

export type ProjectView = { name: string, path: string, threads: number,
/**
 * FRONT_API 1.33. 옛 host는 보내지 않는다(active)
 */
status?: ProjectStatus,
/**
 * `project open`이 profile 없이 쓸 coordinator profile(project.toml → host.toml `[defaults]` → claude로 푼 것).
 * FRONT_API 1.33. 옛 host는 보내지 않는다(빈 글)
 */
coordinator_profile?: string,
/**
 * coordinator가 쓸 수 있는 profile(safety `coordinator_profiles`, 사람만 바꾼다). 없으면 모두다. 웹 「coordinator 열기」가
 * 고르기 목록을 거른다(참고 popup `profile_picker`의 `usable`). FRONT_API 1.33
 */
coordinator_profiles?: Array<string>, };

export type ProjectStatus = "active" | "paused" | "archived";

export type ProjectAction = "pause" | "resume" | "archive" | "unarchive";

export type ProjectChange = { project: ProjectView, notes: Array<string>, };

export type ProjectFilesCheck = { project: string,
/**
 * 본 때 고칠 것(AGENTS.md·uploads/·routines/pr-followup.md). 비면 맞다
 */
problems: Array<string>,
/**
 * `fix`로 고쳤다
 */
fixed: boolean,
/**
 * 고치지 못한 까닭
 */
error: string | null, };

export type ProjectOpened = { project: ProjectView,
/**
 * 띄운 coordinator, 또는 이미 돌던 것 중 가장 최근에 상태가 바뀐 것
 */
pane: PaneView,
/**
 * 새로 띄웠다(false면 돌던 coordinator)
 */
started: boolean,
/**
 * `pane` 말고도 도는 coordinator 수
 */
others: number,
/**
 * coordinator를 띄운 profile(돌던 것이면 그 pane의 harness)
 */
profile: string,
/**
 * 그 pane이 든 workspace(layout에 없으면 빈 글)
 */
workspace: string, notes: Array<string>, };

export type ThreadStatus = "starting" | "open" | "failed" | "resolved";

export type BriefState = "pending" | "delivered" | "unknown";

export type DeployAction = { "action": "update", from: string | null, version: string | null, dry_run: boolean, } | { "action": "checked_update", from: string, sha256: string, dry_run: boolean, } | { "action": "update_staged", rollout: string, dry_run: boolean, } | { "action": "rollback", name: string | null, dry_run: boolean, } | { "action": "checked_rollback", sha256: string, } | { "action": "restart", service: DeployService, };

export type DeployStageView = { rollout: string, project: string, sha256: string, total: number, offset: number, complete: boolean, version: string | null, };

export type RuntimeIdentity = { sha256: string, software: string, instance: string, started_ms: number,
/**
 * host가 실제로 연 웹 주소(loopback, 관리 GUI W6c, FRONT_API 1.87). 같은 머신 agent에만 싣는다. host 감시는 이 주소의
 * `/api/v1/me`가 답해야 새 판 준비로 본다. 없으면(웹을 끈 설정·옛 host·agent 신원) 실행 신원만 본다
 */
web?: string | null, };

export type DeployGuardSpec = { id: string, rollout: string, project: string, service: DeployService, host_machine: boolean, baseline: RuntimeIdentity, expected_sha256: string, expected_software: string, old_host_instance: string, install_job: string | null, rollback_job: string, restart_job: string, host_restart_job: string, };

export type DeployGuardPhase = "armed" | "awaiting_new" | "new_ready" | "restoring" | "restarting" | "awaiting_old" | "verified_new" | "restored" | "failed" | "cancelled";

export type DeployGuardView = { spec: DeployGuardSpec, phase: DeployGuardPhase, worker_pid: number, deadline_ms: number, install_started: boolean, activated: boolean, restore_started: boolean, restart_index: number, restart_started: boolean, host_restart_ms: number, agent_restart_ms: number, observed: Array<DeployService>, installed_sha256: string, error: string | null,
/**
 * 실행 신원은 맞고 웹 응답을 기다리는 host 웹 주소(W6c). 답하면 NewReady, 시한이 지나면 복구
 */
pending_web?: string | null, };

export type DeployGuardControl = { "op": "status" } | { "op": "begin_install", job_id: string, } | { "op": "activate" } | { "op": "observe", service: DeployService, identity: RuntimeIdentity, } | { "op": "commit" } | { "op": "recover", reason: string, } | { "op": "cancel" };

export type DeployService = "host" | "agent";

export type DeployState = "queued" | "running" | "completed" | "failed" | "interrupted";

export type DeployJob = { id: string, project: string, action: DeployAction, state: DeployState, instance: string, executor_pid: number, expected_sha256: string, result: string | null, created_ms: number, updated_ms: number, };

export type ThreadGroup = "waiting_on_you" | "ready_for_review" | "landing" | "working" | "idle" | "resolved";

export type ReportAttention = "completed" | "needs_user";

export type ThreadView = { project: string, id: string, title: string, machine: string, profile: string, account?: string, role: Role,
/**
 * (FrontResponse 태그 `kind`와 겹치지 않게 이름을 달리한다)
 */
thread_kind: ThreadKind,
/**
 * 명시한 실행 대기 표지. timing이면 그 머신의 측정이 끝날 때까지 자동 시작이 기다릴 수 있다.
 */
holds: Array<string>,
/**
 * 이 thread를 띄운 자동화 규칙 이름(`do.start`, FRONT_API 1.64, plan t-0475 A1). 사람이 띄운 thread와 옛 host는 생략한다.
 * 웹 「작업」 목록이 줄 왼쪽 호박색 막대와 `자동 · <규칙 제목>` 표지로 보인다
 */
automation?: string,
/**
 * 그 규칙의 제목(FRONT_API 1.64). 규칙 파일이 없어졌거나 제목이 비면 이름이다. `automation`이 없으면 없다
 */
automation_title?: string, status: ThreadStatus,
/**
 * 끝남 기록 뒤 아직 정리 응답을 확정하지 못했다. 같은 resolve로 이어간다.
 */
cleanup_pending?: boolean, group: ThreadGroup, brief: BriefState, pane: string | null, cwd: string | null, work_dir: string | null, branch: string | null,
/**
 * 추적하는 PR(§8.80)
 */
pr: ThreadPr | null, report_hash: string | null, report_attention?: ReportAttention, acked_hash: string | null,
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
warnings?: Array<string>,
/**
 * 그 프로젝트의 상태(FRONT_API 1.33, 진행 중이면 없다). 상태를 바꾸면 그 프로젝트 thread 사건을 다시 내서 front가
 * 다시 묻지 않고 멈춤·보관을 안다(웹 sidebar의 「멈춤」, 보관한 프로젝트 숨김)
 */
project_status?: ProjectStatus,
/**
 * 작업 요약 표지(FRONT_API 1.44, plan §8.259): 요약을 쓴 때와 그 뒤 반영 안 된 지시 수. 옛 host는 생략한다
 */
summary?: SummaryMark,
/**
 * thread가 일하는 저장소의 그 머신 경로(FRONT_API 1.44): worktree thread는 시작할 때 고른 repo, 데려온 thread는 그 폴더의
 * git top. 작업 폴더 thread와 옛 기록은 없다. 웹 「작업」 트리가 마지막 폴더 이름으로 묶는다(머신마다 경로가 달라도 한 폴더)
 */
repo?: string,
/**
 * 사용량 한도(FRONT_API 1.44, plan §8.267): 이 thread harness의 구독 창이 다 찼으면 그 창이 다시 차오르는 때(ms).
 * host 머신의 thread이고 그 구독에 계정이 하나일 때만 안다. 다 차지 않았거나 모르면 생략한다
 */
limited_until_ms?: number,
/**
 * 보고 `## Questions`의 질문과 답(FRONT_API 1.72, plan §8.329): 웹 보고서 패널 「답할 것」 카드와 대화창 칩이 쓴다.
 * 질문이 없는 보고와 옛 host는 생략한다
 */
questions?: ThreadQuestions, };

export type ThreadPr = { url: string, state: string | null, review: string | null, };

export type SummaryMark = {
/**
 * 요약을 마지막으로 쓴 때. 요약 파일이 없으면 None
 */
at_ms: number | null,
/**
 * 요약을 쓴 뒤 task.md Follow-ups에 쌓인 지시 수(요약이 없으면 Follow-ups 전부)
 */
pending: number, };

export type ThreadQuestions = {
/**
 * 이 질문이 든 보고 사본의 hash. 답은 이 hash가 지금 보고일 때만 받는다
 */
report_hash: string, items: Array<ThreadQuestion>,
/**
 * 답했으면 그 답(웹 카드·`aaa thread answer`). 모든 기기가 같은 값을 본다
 */
answer: QuestionAnswerView | null,
/**
 * 본문 그림의 주소 앞부분(FRONT_API 1.78, plan §8.345): 본문의 `library/<경로>`는 `<files_base><경로>`로 읽는다.
 * 그 보고의 host 사본에서만 주고, 새 보고가 오면 옛 주소는 열리지 않는다. 옛 host는 생략한다
 */
files_base?: string, };

export type ThreadQuestion = { text: string, choices: Array<QuestionChoice>,
/**
 * `### N. 질문` 머리 아래의 Markdown 본문(FRONT_API 1.78, plan §8.345): 그림·표·code·링크. 옛 꼴과 옛 host는 생략한다
 */
body?: string, };

export type QuestionChoice = { label: string,
/**
 * 「 — 」 뒤의 까닭 한 줄
 */
description: string | null,
/**
 * 「(추천)」 표지가 있었다
 */
recommended: boolean,
/**
 * 선택지 줄 아래 두 칸 들여쓴 Markdown(시안 그림 등, FRONT_API 1.78). 없으면 생략한다
 */
body?: string, };

export type AskKind = "question" | "request" | "task";

export type AskRecord = { id: string, kind: AskKind,
/**
 * 질문은 thread 번호, 확인 필요 post는 inbox 항목 ID, 할 일은 task 제목
 */
subject: string,
/**
 * thread 제목·post 제목·task 제목
 */
title: string,
/**
 * 물은 때(보고를 받은 때·post를 받은 때). 모르면 답한 때
 */
asked_ms: number, done_ms: number,
/**
 * 답한 곳(웹 기기 이름·`CLI`·`coordinator`)
 */
by: string,
/**
 * 「1A 2B」·「답함으로 표시」·「끝남으로 지움」
 */
summary: string,
/**
 * 질문 원문(본문·선택지 본문 포함)과 답
 */
items: Array<ThreadQuestion>, answer?: QuestionAnswerView,
/**
 * 확인 필요 post 본문·할 일 확인 필요 까닭(Markdown 글)
 */
body?: string,
/**
 * 보존한 파일의 주소 앞부분: `<files_base>library/<경로>`가 그림, `<files_base>report.md`가 그때 보고다
 */
files_base?: string, };

export type UserRequestView = {
/**
 * inbox 항목 ID(「답함으로 표시」가 끝낸다)
 */
id: string, title: string,
/**
 * post 본문(Markdown 글). 본문이 지워졌으면 inbox 요약
 */
body: string, created_ms: number, };

export type QuestionAnswerView = {
/**
 * 질문마다 고른 선택지 번호(0부터). 고르지 않고 직접 쓴 질문은 null
 */
picks: Array<number | null>,
/**
 * 직접 쓴 답(질문 번호를 붙여 쓴다, 예: 「2 둘 다 보이게」)
 */
text: string | null, summary: string,
/**
 * 답한 곳(웹 기기 이름·`CLI`)
 */
by: string, at_ms: number, };

export type SummaryEntry = {
/**
 * 사용자·coordinator·agent
 */
who: string | null,
/**
 * 적힌 그대로의 시각(`15:0x`, `12:2x~14:5x`)
 */
at: string | null, text: string, dropped: boolean, };

export type FollowUpLine = { at_ms: number | null, text: string, };

export type SummaryStepState = "pending" | "working" | "done";

export type SummaryStep = { text: string, state: SummaryStepState, };

export type ThreadSummaryView = { project: string, thread: string,
/**
 * 요약 파일 글(없으면 None)
 */
text: string | null, goal: string | null,
/**
 * 목표가 바뀌었을 때 처음 시킨 것
 */
origin: string | null,
/**
 * 전체 큰 작업(최대 5개, FRONT_API 1.86). 옛 요약·옛 host는 목록이 없다.
 */
progress?: Array<SummaryStep>, now: Array<string>, decided: Array<SummaryEntry>, changed: Array<SummaryEntry>, done: Array<string>,
/**
 * thread 보고 `## Decisions` 줄(agent가 정한 것)
 */
decisions: Array<string>, at_ms: number | null,
/**
 * 요약을 쓴 쪽(coordinator·사람)
 */
by: string | null,
/**
 * Follow-ups 전체 수
 */
follow_ups: number,
/**
 * 요약 뒤 지시 수와 그 마지막 몇 개(요약이 없으면 Follow-ups 마지막 몇 개)
 */
pending: number, pending_lines: Array<FollowUpLine>,
/**
 * 요약이 없을 때 보이는 할 일 첫 줄
 */
task: string | null, };

export type FlowEvent = { project: string, seq: number, at_ms: number, boot_id: string,
/**
 * thread_started·brief_delivered·brief_unknown·report_ready·review_acked·thread_resolved·thread_failed …
 */
kind: string, thread: string | null,
/**
 * thread 없는 감시 사건의 저장소. 옛 기록은 없음이다.
 */
repo: string | null, actor: string | null, role: Role | null, profile: string | null, machine: string | null,
/**
 * 근거 경로(host 프로젝트 폴더 기준, 예: `threads/t-0001.md`)
 */
evidence: string | null,
/**
 * 사람이 읽는 한 줄
 */
detail: string, };

export type AgentActivity = "started" | "working" | "idle" | "question" | "error" | "exited";

export type PlanSummary = { done: number, total: number, current: string | null, };

export type ContextUsage = {
/**
 * 마지막 응답에 실린 컨텍스트 토큰(정리가 막 끝났으면 정리 뒤 크기)
 */
used: number,
/**
 * 창 크기. 기록으로 알 수 없으면 null(웹은 %가 아니라 토큰 수로 보인다)
 */
window: number | null,
/**
 * 사용량 %(그 harness 터미널과 같은 식: Claude 사용/창 반올림, Codex 기준 12k를 뺀 「left」의 나머지). 창을 모르거나
 * 사용이 창보다 크면 null
 */
percent: number | null,
/**
 * 이 기록의 마지막 정리(compaction) 전후 크기. 정리가 없었거나 뒤 크기를 아직 모르면 null
 */
compaction: ContextCompaction | null, };

export type ContextCompaction = { before: number, after: number, };

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
seen: boolean,
/**
 * harness가 기록에 남긴 계획(Claude TaskCreate·TaskUpdate, Codex update_plan). 없으면 null(차이 목록 S5)
 */
plan: PlanSummary | null,
/**
 * 아직 도는 배경 작업 수(Claude 하위 agent·배경 명령). 없으면 null
 */
background: number | null,
/**
 * 쉬지만 그 턴이 띄운 배경 작업을 기다린다(herdr-web-ui `background_wait`, host `bgwait.rs`, 차이 목록 S1): 목록은 끝남·쉼
 * 대신 BG다. 옛 host는 보내지 않는다(없으면 false)
 */
background_wait: boolean,
/**
 * 작업 중이지만 harness가 대화를 정리(compaction)하는 중이다(화면 감지 사건 `compacting`, plan §8.309). 그동안 harness는 친 글과
 * 대기 글을 읽지 않고, 웹은 보낸 글을 이 기기에 붙잡았다가 끝나면 보낸다. 옛 host는 보내지 않는다(없으면 false, 1.61)
 */
compacting: boolean,
/**
 * 컨텍스트 사용량(plan §8.321, 1.67). 옛 host·첫 응답 전에는 null
 */
context: ContextUsage | null, };

export type QuestionState = "open" | "answered" | "gone";

export type QuestionView = { id: string, pane: string, machine: string, project: string | null, thread: string | null,
/**
 * permission·trust(FrontResponse 태그 `kind`와 겹치지 않게 이름을 바꿨다)
 */
question_kind: string, text: string,
/**
 * 고를 수 있는 답(예: yes·no)
 */
options: Array<string>, state: QuestionState, answer: string | null, created_at_ms: number,
/**
 * 화면에서 읽은 선택 창(못 읽었으면 None: 그때는 `options`의 예·아니오로 답한다)
 */
menu: QuestionMenu | null, };

export type QuestionMenu = { id: string,
/**
 * question(AskUserQuestion)·approval(도구 승인)·plan(계획 승인)·menu(여러 질문의 검토)
 */
kind: string, title: string, question: string,
/**
 * 참고 글(명령·계획·검토할 답)
 */
body: string | null, options: Array<MenuOption>, multi_select: boolean,
/**
 * 「직접 쓰기」 자리. `options.len()`이면 그 줄은 `options`에 없다(Claude 질문의 「Type something.」)
 */
custom_option_index: number | null, };

export type NoticeKind = "host_issue" | "host_recovered" | "rollout_finished" | "rollout_failed" | "finished" | "post_muted" | "automation" | "report";

export type NoticeView = { kind: NoticeKind,
/**
 * 갈 pane(새 보고는 그 thread의 pane, 없으면 null)
 */
pane: string | null, machine: string, project: string | null, thread: string | null,
/**
 * thread 제목 또는 pane 이름
 */
title: string,
/**
 * harness 이름(claude·codex, 끝남만)
 */
agent: string | null,
/**
 * 끝난 턴의 길이(작업·질문으로 들어선 때부터). host가 그 턴의 시작을 못 봤으면 null
 */
turn_ms: number | null,
/**
 * 긴 턴(herdr-web-ui `LONG_TURN_MS` 60초 이상이거나 길이를 모름): 웹 앱 안 알림은 이것만 띄운다
 */
long: boolean, at_ms: number, };

export type MenuOption = { label: string, description: string | null, };

export type MenuPick = { "by": "option", index: number, } | { "by": "options", indices: Array<number>, } | { "by": "custom", text: string, };

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

export type RoutineView = { title: string, name: string, trigger: string, action: string,
/**
 * automation·routine·legacy. legacy는 변환 안내용 옛 파일이다.
 */
origin: string, file: string, last_result: RoutineRun | null, last_skip: RoutineRun | null, today: number, max_per_day: number | null,
/**
 * 최근 순서, 최대 20건. 전체 50건은 routine_runs로 읽는다.
 */
recent: Array<RoutineRun>,
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
approved: boolean, command_sha256: string, start_approval_sha256: string | null, start_approved: boolean | null, start_scope: string | null, last_run_ms: number | null, next_run_ms: number | null,
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
error: string | null,
/**
 * 내구 입력 큐(FRONT_API 1.58). 옛 host는 생략한다.
 */
queue: AutomationQueue | null,
/**
 * 자동화 기억의 host 사본. 옛 host는 생략한다.
 */
memory: AutomationMemory | null,
/**
 * 입력 처리·완료 보고·Idle가 갖춰진 thread의 끝남 예정 시각.
 */
closing: Array<AutomationClosing>, };

export type AutomationMemory = { text: string, bytes: number, truncated: boolean, thread: string | null, updated_at_ms: number | null, };

export type AutomationClosing = { thread: string, close_at_ms: number, };

export type RoutineRun = { id: string, at_ms: number,
/**
 * done / skipped / waiting / failed
 */
status: string, detail: string, thread: string | null, };

export type AutomationQueue = {
/**
 * 아직 보내지 않은 입력 수
 */
pending: number, inflight: AutomationInflight | null, };

export type AutomationInflight = {
/**
 * 사람이 본 묶음. 풀 때 이 값을 실어 보낸다.
 */
id: string, events: number,
/**
 * 묶음 첫 입력이 들어온 때
 */
at_ms: number,
/**
 * `do.message` 대상 thread(주인 thread면 그 사건의 thread)
 */
thread: string | null,
/**
 * 보냈는지 모른다: 자동으로 다시 보내지 않고 확인 필요로 기다린다
 */
unknown: boolean, };

export type RoutineSource = { file: string, text: string, sha256: string,
/**
 * `text_file`·`brief_file`이 가리키는 본문
 */
body_file: string | null, body: string,
/**
 * 원문(`text`)을 잘랐다
 */
truncated: boolean,
/**
 * 본문(`body`)을 잘랐다
 */
body_truncated: boolean, };

export type AutomationDoc = { name: string,
/**
 * `automations/<이름>.toml`, 옛 형식이면 `routines/<이름>.md`
 */
file: string, text: string, sha256: string,
/**
 * 옛 `.md` 규칙: 웹은 읽기만 하고 저장·삭제를 거부한다
 */
legacy: boolean,
/**
 * `text_file`·`brief_file`이 가리키는 본문 파일과 그 판(없는 파일이면 `body_sha256`이 없다)
 */
body_file: string | null, body: string | null, body_sha256: string | null, check: AutomationCheck, };

export type AutomationCheck = { form: Record<string, unknown> | null,
/**
 * `파일:줄:열 칸: 이유`. 하나라도 있으면 저장하지 않는다
 */
errors: Array<string>,
/**
 * 저장은 되지만 실행 때 막히는 것(시작 후보 머신·저장소·profile을 지금 풀지 못함 등)
 */
warnings: Array<string>, review: AutomationReview | null, };

export type AutomationReview = { enabled: boolean,
/**
 * 조건·동작 낱말(목록 줄과 같다)
 */
trigger: string, action: string,
/**
 * safety `start_threads`(propose·auto): 자동 시작이 바로 띄우는지, 제안 한 줄만 남기는지
 */
start_threads: string, start_scope: string | null, start_approval_sha256: string | null,
/**
 * 지금 승인한 범위 안인가(시작 동작만)
 */
start_approved: boolean | null,
/**
 * 명령 조건의 명령·머신과 그 hash(승인 대상)
 */
command: string, machine: string | null, command_sha256: string, command_approved: boolean,
/**
 * safety `routine_commands`
 */
commands_allowed: boolean, };

export type AutomationTrashEntry = {
/**
 * `.trash/` 안 파일 이름(되살리기 요청에 쓴다)
 */
id: string, name: string, title: string, deleted_at_ms: number, };

export type ProjectSettingsView = { project: string,
/**
 * 설정 파일 이름(`aaa.toml`, 없으면 옛 `project.toml`)
 */
file: string, status: ProjectStatus, goal: string, thread_profile: string, coordinator_profile: string, max_parallel_threads: number, auto_resolve_days: number, nudge: boolean, mute: boolean,
/**
 * 저장소 목록(`[[workspaces]]`·`[[repos]]`). 머신이 없으면 host 머신이다
 */
repos: Array<RepoChoice>,
/**
 * host 로컬 머신 이름(`RepoChoice.machine`이 빈 저장소의 머신, 모르면 없다)
 */
local_machine: string | null, task_notes: Array<string>, };

export type SettingsScope = "project" | "global";

export type SafetyRowView = { key: string, value: string,
/**
 * 프로젝트·모든 프로젝트·옛 자리·기본값
 */
source: string,
/**
 * 적힌 값과 다른 까닭(yolo가 start_threads를 auto로 만든다 등)
 */
note: string,
/**
 * 웹에서 바꿀 수 있는 key(yolo·start_threads·trust_screens·coordinator_deploy)
 */
editable: boolean,
/**
 * 바꿀 수 있는 key만: 이 표의 값을 지우면(「기본」) 되는 값. 웹은 이것이 yolo on이면 한 번 더 확인한다
 */
below: string | null, };

export type SafetyView = { scope: SettingsScope, project: string, rows: Array<SafetyRowView>,
/**
 * thread·coordinator 기본 profile(프로젝트 설정 → host.toml [defaults] → claude)
 */
thread_profile: string, coordinator_profile: string,
/**
 * thread·coordinator가 쓸 수 있는 profile 실효값(`None`이면 모든 profile). 허용 목록 고치기의 처음 값
 */
thread_profiles: Array<string> | null, coordinator_profiles: Array<string> | null,
/**
 * 읽다 버린 값 등
 */
warnings: Array<string>,
/**
 * host.toml 판(sha256). 고칠 때 이 값을 돌려준다(그사이 바뀌었으면 conflict)
 */
rev: string,
/**
 * 바꾼 뒤 알림(적용 시점, host.local.toml이 덮음 등). 읽기에는 없다
 */
notes: Array<string>, };

export type ProfileAdminRow = { name: string, harness: string, model: string, effort: string, description: string, args: Array<string>, env_names: Array<string>,
/**
 * host.toml에 표가 없는 기본 profile(claude·codex)
 */
builtin: boolean,
/**
 * host.local.toml이 정한 profile: 웹에서 고치지 않는다(그 파일이 필드마다 덮는다)
 */
local: boolean,
/**
 * custom harness(실행 명령이 있다): 웹에서 고치지 않는다
 */
custom: boolean,
/**
 * harness 실행 파일 뒤에 붙는 인자 전체(model·effort 플래그와 제 인자). custom은 앞 명령을 빼고 보인다
 */
launch: Array<string>, };

export type ProfileAdminView = { profiles: Array<ProfileAdminRow>, rev: string, };

export type ProfileChangeReq = { "change": "add", name: string, harness: string, model: string, effort: string, description: string, args: Array<string>, } | { "change": "edit", name: string, harness: string | null, model: string | null, effort: string | null, description: string | null,
/**
 * 인자 전체를 바꾼다(빈 목록이면 지운다)
 */
args: Array<string> | null, } | { "change": "remove", name: string, } | { "change": "allow", scope: SettingsScope,
/**
 * thread 또는 coordinator
 */
role: string, names: Array<string> | null, } | { "change": "default", role: string, name: string, };

export type ProfilePlanView = { message: string, before: ProfileAdminRow | null, after: ProfileAdminRow | null,
/**
 * 경고(host.local.toml이 덮음, 허용 목록에 없는 profile 등)
 */
warnings: Array<string>, rev: string, };

export type WorkRootView = { path: string, mount: string | null, last_start: string | null, last_start_mount: string | null, previous_roots: Array<string>,
/**
 * 다음 시작 설정과 마지막 시작 기록이 다르다(agent를 다시 띄워야 닿는다)
 */
restart_required: boolean,
/**
 * 전환했을 때만: 설정을 바꿨나, 가져온 준비 기록 수
 */
changed?: boolean, imported?: number, };

export type WorkRootCheckView = { path: string, real_path: string, mount: string | null, };

export type HookView = {
/**
 * aaa Codex 전역 hook이 있고 hooks 기능이 켜져 있다
 */
codex_installed: boolean,
/**
 * 상태를 읽지 못한 까닭(Codex 설정 파일이 깨짐 등)
 */
codex_problem: string | null,
/**
 * 설치·제거 뒤 만든 백업 수
 */
backups?: number, };

export type ResolvePreview = { project: string, thread: string,
/**
 * thread 종류(응답 꼬리표 `kind`와 겹치지 않게 이름이 다르다)
 */
thread_kind: ThreadKind,
/**
 * 끝내면 지울 수 있는 worktree가 있다(worktree thread이고 작업 폴더 기록이 있다)
 */
worktree: boolean, branch: string | null,
/**
 * 기록된 PR 상태가 병합이다(끝낼 때 branch를 지울 수 있다)
 */
merged: boolean,
/**
 * 마지막 사본 상태(`complete`·`partial`, 없으면 아직 사본 없음)
 */
copy: string | null, copy_notes: Array<string>,
/**
 * HEAD 대비 커밋하지 않은 파일 수
 */
uncommitted: number | null,
/**
 * base(`origin/main` 등) 대비 바뀐 파일 수(커밋하지 않은 것 포함)
 */
changed: number | null,
/**
 * git을 읽지 못한 까닭(머신 끊김·옛 agent·저장소 없음)
 */
unknown: string | null,
/**
 * 끝내기 창이 보일 위험(한 줄씩)
 */
risks: Array<string>,
/**
 * 위험이 있거나 모르면 「worktree 남기기」를 기본으로 켠다
 */
keep_default: boolean, };

export type LibraryFile = {
/**
 * library 안 상대 경로
 */
path: string, size: number,
/**
 * host에 사본이 없다: 볼 때 agent에서 스트리밍한다(작업 폴더가 남아 있을 때만)
 */
on_agent: boolean,
/**
 * 파일 보기가 고를 MIME·종류(파일 보기 `FileInfo`와 같은 표). 옛 목록 파일에는 없어 host가 줄 때 채운다
 */
mime: string, kind: FileKind, };

export type LibraryView = { project: string, thread: string,
/**
 * 회수한 report.md(없으면 None)
 */
report: string | null, files: Array<LibraryFile>, base: string, expires_at_ms: number,
/**
 * FRONT_API 1.36. 조작 권한에만 보이는 보고서 공유 링크. 앱 안 보기는 만료·폐기와 무관하다.
 */
link: LinkView | null,
/**
 * FRONT_API 1.56. 보고서를 낸 thread(`LibraryReport.from_thread`와 같다). 열람 머리에 보인다
 */
from_thread?: string, };

export type LinkView = { id: string, project: string, thread: string, base: string,
/**
 * 공개했을 때만 있는 절대 HTTPS 주소. loopback은 밖 알림에 싣지 않는다.
 */
url: string | null, created_at_ms: number, expires_at_ms: number, last_opened_at_ms: number | null, revoked: boolean,
/**
 * host에 회수한 일반 파일의 합계 바이트
 */
size: number, };

export type LibraryReport = { project: string, name: string, size: number, updated_at_ms: number, live: boolean,
/**
 * 읽기 실패·상한 초과도 목록에서 감추지 않는다.
 */
problem: string | null, link: LinkView | null,
/**
 * FRONT_API 1.43. coordinator가 사용자에게 보낸 웹 보고서(`library publish --to-user`, 「내 보고서」). 아니면 작업 보고다.
 */
to_user: boolean,
/**
 * FRONT_API 1.43. `--to-user`로 보낼 때 준 제목
 */
title?: string,
/**
 * FRONT_API 1.43. 사용자 보고서를 보낸(다시 보낸) 뒤 아직 열지 않았다
 */
unseen: boolean,
/**
 * FRONT_API 1.43. 진행 중 thread 또는 완료 보관이 참조하는 사본이라 지울 수 없다
 */
busy: boolean,
/**
 * FRONT_API 1.56. 보고서를 낸 thread(`t-NNNN`, 사용자 요청 10-20). 근원을 모르면 없다
 */
from_thread?: string, };

export type LibraryRef = { project: string, name: string, };

export type LibraryPrunePlan = { size: number, remaining_size: number, protected_size: number, max_size: number, max_days: number, candidates: Array<LibraryPruneEntry>, };

export type LibraryPruneEntry = { project: string, name: string, size: number,
/**
 * 「기한」 또는 「용량」. 기한 정리 뒤 용량을 판정한다.
 */
reason: string, };

export type LibrarySkip = { name: string, reason: string, };

export type FileKind = "image" | "video" | "audio" | "pdf" | "text" | "binary";

export type FileInfo = { path: string, name: string, size: number, modified_ms: number, mime: string, kind: FileKind, };

export type DirListing = { path: string, parent: string | null, home: string, directories: Array<string>, files: Array<DirFile>, truncated: boolean, };

export type CommandList = { harness: string,
/**
 * 내장 표가 기준으로 삼은 harness 버전(`2.1.304`, `0.163`)
 */
builtin_version: string, commands: Array<SlashCommand>, };

export type SlashCommand = {
/**
 * 트리거 뒤 이름(`compact`, `frontend:review`, `plugin:skill`)
 */
name: string, description: string, source: CommandSource, kind: CommandKind,
/**
 * 인자 안내(`[지시]`) 또는 선택창을 건너뛰는 꼴(`/model sonnet`)
 */
hint: string | null,
/**
 * 같은 명령의 다른 이름(Claude `/reset`·`/new`는 `/clear`)
 */
aliases: Array<string>,
/**
 * 작업 중에는 목록에서 뺀다(harness가 막거나 턴 뒤로 미룬다)
 */
idle_only: boolean,
/**
 * 고르면 입력칸에 넣을 글(`/compact `, Codex skill은 `$name `)
 */
insert: string, };

export type CommandSource = "builtin" | "user" | "project" | "skill" | "plugin";

export type CommandKind = "send" | "picker" | "screen" | "confirm" | "danger";

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

export type InstallRole = "host" | "agent";

export type InstallOs = "macos" | "linux" | "windows";

export type InstallArch = "aarch64" | "x86_64";

export type InstallMode = "fresh" | "repair" | "reinstall";

export type InstallToggle = "enabled" | "disabled";

export type InstallProjectMode = "create" | "use" | "skip";

export type InstallStorage = "internal";

export type InstallHarness = "claude" | "codex" | "none";

export type InstallLogin = "require" | "defer";

export type InstallHooks = "install" | "preserve" | "skip";

export type InstallService = "auto" | "launchd" | "systemd-system" | "systemd-user" | "windows-scm" | "none";

export type InstallAccount = "current-user";

export type InstallPasswordSource = "prompt" | "stdin" | "broker";

export type InstallStartup = "auto" | "manual";

export type InstallStart = "now" | "deferred";

export type InstallLinger = "require" | "preserve";

export type InstallPathPolicy = "preserve" | "user";

export type InstallLauncherPolicy = "install" | "preserve";

export type InstallNetwork = "tailscale" | "direct";

export type InstallWeb = "local" | "off";

export type InstallServe = "on" | "off";

export type InstallPairing = "controller" | "skip";

export type InstallEnroll = "wait" | "grant" | "skip";

export type InstallOpen = "yes" | "no";

export type InstallPrompt = "auto" | "plain";

export type InstallStatus = "planned" | "completed" | "unchanged" | "configured" | "needs_user" | "failed" | "cancelled";

export type InstallChangeState = "planned" | "applied" | "unchanged" | "pending" | "reverted" | "failed";

export type InstallCheckState = "passed" | "failed" | "not_run" | "not_requested" | "unknown";

export type InstallRollbackState = "not_requested" | "completed" | "partial" | "failed";

export type InstallPhase = "detect" | "preflight" | "plan" | "authorize" | "stage" | "configure" | "hooks" | "work_root" | "service" | "enroll" | "web" | "pairing" | "verify" | "rollback";

export type InstallEventState = "pending" | "running" | "completed" | "unchanged" | "skipped" | "needs_user" | "failed" | "reverted";

export type InstallOperation = "write_file" | "stage_binary" | "stage_launcher" | "configure_hook" | "prepare_work_root" | "register_service" | "start_service" | "publish_web" | "enroll_agent" | "create_project" | "modify_path";

export type InstallSchema = "aaa.install.v1";

export type InstallPlanSchema = "aaa.install.plan.v1";

export type InstallEventSchema = "aaa.install.event.v1";

export type InstallAuthorizationSchema = "aaa.install.authorization.v1";

export type InstallGrantSchema = "aaa.install.enroll-grant.v1";

export type InstallPlatform = { os: InstallOs, arch: InstallArch, };

export type InstallChoices = { role: InstallRole | null, instance: string | null, config_dir: string | null, state_dir: string | null, mode: InstallMode | null, name: string | null, local_agent: InstallToggle | null, bin_path: string | null, version: string | null, from: string | null, sha256: string | null, expected_os: InstallOs | null, expected_arch: InstallArch | null, project_mode: InstallProjectMode | null, project: string | null, projects_root: string | null, goal: string | null, work_root: string | null, work_mount: string | null, work_storage: InstallStorage | null, min_free_gb: number | null, default_cwd: string | null, shell: string | null, claude_path: string | null, codex_path: string | null, claude_home: string | null, codex_home: string | null, login: InstallLogin | null, hooks: InstallHooks | null, thread_profile: string | null, coordinator_profile: string | null, service: InstallService | null, service_account: InstallAccount | null, service_password_source: InstallPasswordSource | null, startup: InstallStartup | null, start: InstallStart | null, linger: InstallLinger | null, path: InstallPathPolicy | null, launcher: string | null, launcher_policy: InstallLauncherPolicy | null, host: string | null, agent_listen: string | null, web_listen: string | null, network: InstallNetwork | null, web: InstallWeb | null, tailscale_serve: InstallServe | null, serve_port: number | null, pairing: InstallPairing | null, enroll: InstallEnroll | null, wait_timeout: number | null, open_coordinator: InstallOpen | null, repo: Array<string>, harness: Array<InstallHarness>, harness_env: Array<InstallHarnessEnv>, profile_file: Array<string>, allow_thread_profile: Array<string>, allow_coordinator_profile: Array<string>, no_repo: boolean, };

export type InstallHarnessEnv = { harness: InstallHarness, key: string, value: string, };

export type InstallInvocation = { choices: InstallChoices, non_interactive: boolean, yes: boolean, dry_run: boolean, json: boolean, json_stream: boolean, prompt: InstallPrompt | null, events_file: string | null, authorization_stdin: boolean, authorization_file: string | null, secrets_stdin: boolean, service_password_ref: string | null, resume: string | null, status: string | null, };

export type InstallPlanContext = { platform: InstallPlatform, version: string, binary_sha256: string, bin_path: string, config_dir: string, state_dir: string, projects_root: string, default_shell: string, effective_config_sha256: string, target_account: string, };

export type InstallPlan = { schema: InstallPlanSchema, context: InstallPlanContext, choices: InstallChoices, };

export type InstallError = { code: string, message: string, details: Array<string>, };

export type InstallChange = { kind: string, action: string, state: InstallChangeState, };

export type InstallCheck = { name: string, state: InstallCheckState, };

export type InstallNeedsUser = { code: string, target: string, message: string, };

export type InstallRollback = { state: InstallRollbackState, remaining: Array<string>, };

export type InstallResume = { installation_id: string, };

export type InstallResult = { schema: InstallSchema, installation_id: string | null, plan_sha256: string | null, status: InstallStatus, exit_code: number, role: InstallRole | null, instance: string | null, platform: InstallPlatform | null, version: string | null, binary_sha256: string | null, changes: Array<InstallChange>, checks: Array<InstallCheck>, needs_user: Array<InstallNeedsUser>, error: InstallError | null, rollback: InstallRollback, resume: InstallResume | null, };

export type InstallProgress = { done: number, total: number, unit: string, };

export type InstallStreamItem = { "type": "event", schema: InstallEventSchema, installation_id: string | null, seq: number, phase: InstallPhase, state: InstallEventState, code: string, message: string, progress?: InstallProgress, needs_user?: InstallEventNeedsUser, } | { "type": "result", schema: InstallEventSchema, installation_id: string | null, seq: number, result: InstallResult, };

export type InstallEventNeedsUser = { kind: string, resume_id: string | null, };

export type InstallPermission = { operation: InstallOperation, target: string, before_sha256: string | null, after_sha256: string | null, };

export type InstallAuthorizationClaims = { schema: InstallAuthorizationSchema, installation_id: string, plan_sha256: string, platform: InstallPlatform, target_account: string, instance: string | null, binary_sha256: string, permissions: Array<InstallPermission>, issued_at_ms: number, expires_at_ms: number, nonce: string, issuer: string, };


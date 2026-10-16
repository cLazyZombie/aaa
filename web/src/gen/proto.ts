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
readonly: boolean, cols: number, rows: number, lease: LeaseView | null, } & ({ "state": "running" } | { "state": "exited", code: number | null, } | { "state": "interrupted" } | { "state": "lost" });

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

export type FrontRequest = { "op": "status" } | { "op": "machines" } | { "op": "panes" } | { "op": "pane_run", machine: string | null, name: string | null, argv: Array<string>, cwd: string | null, cols: number | null, rows: number | null, } | { "op": "pane_read", pane: string, } | { "op": "pane_send", pane: string, data: B64, } | { "op": "pane_resize", pane: string, cols: number, rows: number, } | { "op": "pane_kill", pane: string, } | { "op": "pane_input", pane: string, data: B64, } | { "op": "pane_take", pane: string, } | { "op": "pane_release", pane: string, } | { "op": "pane_resume", pane: string, } | { "op": "pane_forget", pane: string, } | { "op": "pane_watch", pane: string, } | { "op": "pane_unwatch", pane: string, } | { "op": "workspace", name: string | null, } | { "op": "layout_apply", workspace: string | null, change: LayoutChange, } | { "op": "enroll_open", minutes: number, } | { "op": "enroll_pending" } | { "op": "enroll_approve", name: string, code: string, } | { "op": "machine_revoke", name: string, } | { "op": "web_pair", role: DeviceRole, } | { "op": "web_publish" } | { "op": "web_publish_off" } | { "op": "web_status" };

export type FrontResponse = { "kind": "status" } & HostStatus | { "kind": "machines", machines: Array<MachineView>, } | { "kind": "panes", panes: Array<PaneView>, } | { "kind": "pane" } & PaneView | { "kind": "screen" } & ScreenText | { "kind": "pending", pending: Array<PendingEnroll>, } | { "kind": "pair_code", code: string, role: DeviceRole, expires_in_s: number, } | { "kind": "publish" } & PublishState | { "kind": "workspace" } & Workspace | { "kind": "done" };

export type FrontEvent = { "ev": "state", machines: Array<MachineView>, panes: Array<PaneView>, } | { "ev": "workspace" } & Workspace | { "ev": "grid" } & GridFrame;

export type TermServerMsg = { "t": "snapshot", replay: B64, cols: number, rows: number, readonly: boolean, } | { "t": "readonly", readonly: boolean, reason: string | null, } | { "t": "state", pane: PaneView, } | { "t": "front", id: string, } | { "t": "error" } & ErrorBody;

export type TermClientMsg = { "t": "input", data: string, } | { "t": "resize", cols: number, rows: number, } | { "t": "take" };


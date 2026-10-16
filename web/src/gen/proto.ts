// 생성 파일: crates/aa-proto의 Rust 타입에서 만든다. 손으로 고치지 않는다.
// 갱신: AA_UPDATE_TS=1 cargo test -p aa-proto typescript

export type Version = { major: number, minor: number, };

export type B64 = string;

export type ErrorCode = "bad_request" | "unauthorized" | "forbidden" | "not_found" | "agent_disconnected" | "session_exited" | "unsupported_version" | "not_enrolled" | "enroll_closed" | "enroll_code_mismatch" | "name_taken" | "timeout" | "unavailable" | "internal";

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

export type PaneView = { id: string, machine_id: string, machine: string, name: string, argv: Array<string>, cwd: string | null, connection: Connection, 
/**
 * 지금 입력할 수 없으면 true(끊김·종료·권한)
 */
readonly: boolean, cols: number, rows: number, } & ({ "state": "running" } | { "state": "exited", code: number | null, } | { "state": "interrupted" } | { "state": "lost" });

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

export type FrontRequest = { "op": "status" } | { "op": "machines" } | { "op": "panes" } | { "op": "pane_run", machine: string | null, name: string | null, argv: Array<string>, cwd: string | null, cols: number | null, rows: number | null, } | { "op": "pane_read", pane: string, } | { "op": "pane_send", pane: string, data: B64, } | { "op": "pane_resize", pane: string, cols: number, rows: number, } | { "op": "pane_kill", pane: string, } | { "op": "enroll_open", minutes: number, } | { "op": "enroll_pending" } | { "op": "enroll_approve", name: string, code: string, } | { "op": "machine_revoke", name: string, } | { "op": "web_pair", role: DeviceRole, } | { "op": "web_publish" } | { "op": "web_publish_off" } | { "op": "web_status" };

export type FrontResponse = { "kind": "status" } & HostStatus | { "kind": "machines", machines: Array<MachineView>, } | { "kind": "panes", panes: Array<PaneView>, } | { "kind": "pane" } & PaneView | { "kind": "screen" } & ScreenText | { "kind": "pending", pending: Array<PendingEnroll>, } | { "kind": "pair_code", code: string, role: DeviceRole, expires_in_s: number, } | { "kind": "publish" } & PublishState | { "kind": "done" };

export type FrontEvent = { "ev": "state", machines: Array<MachineView>, panes: Array<PaneView>, };

export type TermServerMsg = { "t": "snapshot", replay: B64, cols: number, rows: number, readonly: boolean, } | { "t": "readonly", readonly: boolean, reason: string | null, } | { "t": "state", pane: PaneView, } | { "t": "error" } & ErrorBody;

export type TermClientMsg = { "t": "input", data: string, } | { "t": "resize", cols: number, rows: number, };


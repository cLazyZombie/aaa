//! aaa 역할들이 함께 쓰는 바탕. 역할(host·agent·session·front) crate에 의존하지 않고, 역할 경계를 넘는 통로가 되지 않게
//! 전송(IPC·frame·TLS)·OS 유틸·Front API 클라이언트와, agent·host가 같은 규칙을 써야 하는 순수 함수(PR 주소·origin 해석)만 둔다(결정 13).

pub mod caller;
pub mod ci;
pub mod conn;
pub mod diff;
pub mod front;
pub mod git_watch;
pub mod ipc;
pub mod pr;
pub mod process;
pub mod resource;
pub mod tls;
pub mod util;

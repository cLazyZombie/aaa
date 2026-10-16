//! aaa 역할들이 함께 쓰는 바탕. 역할(host·agent·session·front) crate에 의존하지 않고, 역할 경계를 넘는 통로가 되지 않게
//! 전송(IPC·frame·TLS)·OS 유틸·Front API 클라이언트만 둔다(결정 13).

pub mod conn;
pub mod front;
pub mod ipc;
pub mod tls;
pub mod util;

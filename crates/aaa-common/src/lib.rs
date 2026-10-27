//! aaa 역할들이 함께 쓰는 바탕. 역할(host·agent·session·front) crate에 의존하지 않고, 역할 경계를 넘는 통로가 되지 않게
//! 전송(IPC·frame·TLS)·OS 유틸·Front API 클라이언트와, agent·host가 같은 규칙을 써야 하는 순수 함수(GitHub origin 해석)만 둔다(결정 13).

pub mod caller;
pub mod child_process;
pub mod ci;
pub mod conn;
pub mod diagnostic;
pub mod diff;
pub mod front;
pub mod git_watch;
pub mod github;
pub mod harness_version;
pub mod ipc;
pub mod panic_log;
pub mod process;
pub mod quota;
pub mod redact;
pub mod resource;
pub mod response_parts;
pub mod tls;
pub mod util;

/// 역할 사이의 사용자 전용 파일·ACL/잠금 경계. 기존 파일 권한은 고치지 않는다.
pub mod private_file;

/// 설치 서명 canonical bytes. 신뢰 root/권한 판정과 분리한다.
pub mod install_crypto;

#[cfg(all(test, windows))]
mod windows_acl_tests;

/// 협상한 터미널 snapshot/history의 제한된 전송 코덱.
pub mod terminal_payload;

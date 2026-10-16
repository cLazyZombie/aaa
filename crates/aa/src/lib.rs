//! aa 단일 실행 파일의 역할들. `aa host`·`aa agent`·`aa session-host`·CLI front가 같은 binary다.

pub mod agent;
pub mod conn;
pub mod doctor;
pub mod host;
pub mod ipc;
pub mod ring;
pub mod service;
pub mod session;
pub mod tls;
pub mod util;
pub mod vt;

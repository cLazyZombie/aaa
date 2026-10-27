//! 지금 운영(herdr 0.9.5 + herdr-projects 0.2.x)에서 aaa로 옮기기(M4, plan §15, `docs/migration.md`).
//!
//! - `inventory`: 0.2 프로젝트 폴더와 herdr 세션 스냅샷을 **읽기만** 해서 옮길 목록·hash·thread별 계획을 낸다.
//! - `convert`: inventory를 확인한 뒤 aaa 시험(또는 운영) 상태 폴더에 프로젝트·끝난 thread·inbox를 만든다.
//!   열린 thread는 가져오지 않는다(소유 도구는 하나, §15 3·4). 원본은 바꾸지 않는다.
//! - `shadow`: 같은 시각에 모은 원자료(`scripts/m4/shadow-capture.sh`)에 herdr-projects 규칙과 aaa 규칙
//!   (`orch::thread_group`, 실제 코드)을 같은 입력으로 돌려 차이를 표로 낸다.

pub mod convert;
pub mod inventory;
pub mod shadow;
pub mod source;

use std::io::BufRead as _;
use std::path::Path;

use sha2::Digest;

/// 이 도구가 형식을 확인한 원본 판(`research/herdr-projects`). 다른 판은 `--source-version`으로 기록만 하고 막지 않는다.
pub const SOURCE_PIN: &str = "herdr-projects 0.2.35 (1f05c00), herdr 0.9.5 (2ecf19a)";

pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    // 작은 inbox 파일마다 1 MiB Vec를 0으로 채우지 않는다. 큰 파일의 읽기
    // 용량은 유지하고 BufReader가 실제 읽은 바이트만 hash에 넣는다.
    let mut f = std::io::BufReader::with_capacity(1 << 20, std::fs::File::open(path)?);
    let mut h = sha2::Sha256::new();
    loop {
        let bytes = f.fill_buf()?;
        let n = bytes.len();
        if n == 0 {
            break;
        }
        h.update(bytes);
        f.consume(n);
    }
    Ok(hex::encode(h.finalize()))
}

/// 파일 이름 쓰기 전에 원자적으로(임시 파일 → rename)
fn write_new(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    crate::orch::write_atomic(path, bytes)
}

/// 사람이 읽는 크기
pub fn human_bytes(n: u64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 { format!("{n} B") } else { format!("{v:.1} {}", U[i]) }
}

/// Markdown 표 칸: `|`와 줄바꿈을 지운다
fn cell(s: &str) -> String {
    s.replace('|', "\\|").replace(['\n', '\r'], " ")
}

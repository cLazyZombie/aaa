//! 발행 전 질문 검사는 host 연결 없이 파일과 표준 입력을 읽는다.

use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn question_check_reports_source_lines_before_publication() {
    let root = std::env::temp_dir().join(format!("aaa-question-{}", aaa_common::util::new_id()));
    let mut command = Command::new(env!("CARGO_BIN_EXE_aaa"));
    command
        .env_remove("AAA_INSTANCE")
        .env("AAA_CONFIG_DIR", root.join("config"))
        .env("AAA_STATE_DIR", root.join("state"))
        .args(["question", "check"]);
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/../aaa-proto/tests/fixtures/deploy-approval-questions.md");
    let out = command.arg(fixture).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "질문 카드 형식이 맞다");

    let mut child = Command::new(env!("CARGO_BIN_EXE_aaa"))
        .env_remove("AAA_INSTANCE")
        .env("AAA_CONFIG_DIR", root.join("config"))
        .env("AAA_STATE_DIR", root.join("state"))
        .args(["question", "check", "--json", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let report = concat!(
        "[결정 필요] 질문 2개: 배포\r\n\r\n## Questions\r\n",
        "### 1. 진행\r\n- A 진행\r\n- B 보류\r\n",
        "### 2. 배포\r\n- t-2261(replay resume) 재개 수리\r\n- A 배포\r\n",
    );
    child.stdin.take().unwrap().write_all(report.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
    let error: serde_json::Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(error["error"]["code"], "bad_request");
    let message = error["error"]["message"].as_str().unwrap();
    assert!(message.contains("2번 질문 · 7번 줄. 선택지가 1개다"), "{message}");
    assert!(message.contains("선택지를 2개 이상 쓴다"), "{message}");
    assert_eq!(message.lines().count(), 1, "{message}");
    assert!(!root.exists(), "검사에서는 설정이나 상태를 쓰지 않는다");
}

//! CLI의 hook 응답·종료 코드 계약. 자식은 스스로 끝나고 신호를 보내지 않는다.
#![cfg(unix)]
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::process::Command;

fn reply_to_post(mut stream: UnixStream, reply: &str) {
    // 실제 hooks::read_msg는 JSON 완성으로 응답한다. ACK/종료 코드 시험을 EOF 전달에 묶지 않는다.
    let request =
        serde_json::Deserializer::from_reader(&mut stream).into_iter::<serde_json::Value>().next().unwrap().unwrap();
    assert_eq!(request["harness"], "post");
    stream.write_all(reply.as_bytes()).unwrap();
}

#[test]
fn post_fixture_replies_to_complete_json_before_write_eof() {
    let (mut client, server) = UnixStream::pair().unwrap();
    client.set_read_timeout(Some(std::time::Duration::from_secs(3))).unwrap();
    let server = std::thread::spawn(move || reply_to_post(server, "ok\n"));
    client.write_all(br#"{"harness":"post"}"#).unwrap();
    // 실제 agent의 read_msg처럼 완성된 JSON에 답한다. 요청 쓰기 쪽은 아직 열려 있다.
    let mut reply = String::new();
    client.read_to_string(&mut reply).unwrap();
    assert_eq!(reply, "ok\n");
    server.join().unwrap();
}

#[test]
fn post_cli_rejects_old_agent_and_preserves_ack_exit_codes() {
    let path = std::env::temp_dir().join(format!("aap-cli-{}", aaa_common::util::new_id()));
    std::fs::create_dir_all(&path).unwrap();
    let sock = path.join("hook.sock");
    let listener = UnixListener::bind(&sock).unwrap();
    let fixtures = [
        ("ok\n".to_string(), 1, "이 agent는 post를 모른다"),
        ("ok\npost.v1 {\"state\":\"accepted\",\"id\":\"p-00000001\"}".to_string(), 0, "ok p-00000001"),
        (
            "ok\npost.v1 {\"state\":\"refused\",\"reason\":\"행렬 거부\",\"existing\":null}".to_string(),
            1,
            "err 행렬 거부",
        ),
        (format!("ok\npost.v1 {{\"state\":\"queued\",\"post_id\":\"{:032x}\"}}", 1), 2, "queued"),
    ];
    let replies = fixtures.iter().map(|f| f.0.clone()).collect::<Vec<_>>();
    let server = std::thread::spawn(move || {
        for reply in replies {
            let (s, _) = listener.accept().unwrap();
            reply_to_post(s, &reply);
        }
    });
    for (_, code, expected) in fixtures {
        let out = Command::new(env!("CARGO_BIN_EXE_aaa"))
            .args(["post", "coordinator", "--notice", "--topic", "ci-red", "--title", "자료"])
            .env("AAA_SESSION_ID", "s1")
            .env("AAA_HOOK_TOKEN", "test-hook-secret")
            .env("AAA_HOOK_SOCK", &sock)
            .env("AAA_STATE_DIR", &path)
            .env("AAA_CONFIG_DIR", &path)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(code));
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        assert!(text.contains(expected), "{text}");
    }
    server.join().unwrap();
    std::fs::remove_dir_all(path).unwrap();
}

//! CLI의 hook 응답·종료 코드 계약. 자식은 스스로 끝나고 신호를 보내지 않는다.
#![cfg(unix)]
use std::io::Write;
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
            .env("AAA_HOOK_TOKEN", aaa_test_support::token("test-hook-secret").as_str())
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

#[tokio::test]
async fn local_post_distinguishes_first_reply_from_request_timeout() {
    use aaa_common::conn::{FrameRx, spawn_writer};
    use aaa_proto::{DeviceRole, ErrorBody, ErrorCode, FRONT_API, FrontMsg, FrontRequest, MAX_FRAME, Outcome};
    use std::time::Duration;

    let path = std::env::temp_dir().join(format!("aap-front-{}", aaa_common::util::new_id()));
    std::fs::create_dir_all(path.join("host")).unwrap();
    let listener = aaa_common::ipc::LocalListener::bind(&aaa_common::front::socket(&path)).unwrap();
    for (welcomed, present, absent) in
        [(false, "첫 응답 실패", "결과 미확정"), (true, "요청 처리 시간 초과", "이번 요청 미전송")]
    {
        let mut cmd = tokio::process::Command::new(env!("CARGO_BIN_EXE_aaa"));
        cmd.args(["post", "coordinator", "--notice", "--topic", "lab", "--title", "자료"])
            .env("AAA_SESSION_ID", "s1")
            .env_remove("AAA_HOOK_TOKEN")
            .env_remove("AAA_HOOK_SOCK")
            .env("AAA_STATE_DIR", &path)
            .env("AAA_CONFIG_DIR", &path)
            .kill_on_drop(true);
        let (output, ()) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(cmd.output(), async {
                let (stream, _) = listener.accept_peer().await.unwrap();
                let (rx, tx) = tokio::io::split(stream);
                let mut rx = FrameRx::new(rx, MAX_FRAME);
                assert!(matches!(rx.control::<FrontMsg>().await.unwrap(), FrontMsg::Hello { .. }));
                if welcomed {
                    let tx = spawn_writer(tx, MAX_FRAME);
                    tx.json(&FrontMsg::Welcome {
                        proto: FRONT_API,
                        host_id: "test".into(),
                        role: DeviceRole::Local,
                        front: "f1".into(),
                        warnings: vec![],
                    })
                    .await
                    .unwrap();
                    let FrontMsg::Req { id, body: FrontRequest::Post { .. } } = rx.control().await.unwrap() else {
                        panic!("로컬 post 요청이 없다")
                    };
                    tx.json(&FrontMsg::Res {
                        id,
                        result: Outcome::Err(ErrorBody::new(ErrorCode::Timeout, "시험 응답")),
                    })
                    .await
                    .unwrap();
                }
            })
        })
        .await
        .unwrap();
        let output = output.unwrap();
        assert_eq!(output.status.code(), Some(1));
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains(present), "{error}");
        assert!(!error.contains(absent), "{error}");
        assert!(error.contains(if welcomed { "결과 미확정" } else { "이번 요청 미전송" }), "{error}");
    }
    drop(listener);
    std::fs::remove_dir_all(path).unwrap();
}

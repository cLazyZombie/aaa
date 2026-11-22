use super::*;

fn info(argv: &[&str], state: SessionState, role: Option<aaa_proto::Role>) -> SessionInfo {
    SessionInfo {
        responsibility: None,
        id: "s".into(),
        name: "s".into(),
        argv: argv.iter().map(|a| a.to_string()).collect(),
        cwd: None,
        pid: None,
        started_at_ms: 0,
        boot_id: "b".into(),
        cols: 80,
        rows: 24,
        state,
        role,
        label: None,
        harness: None,
    }
}

/// 스스로 끝난 셸만 닫는다. 복원 대상(재부팅·유실)·host가 끝낸 것·역할·프로그램 pane·실행 중을 못 본 것은 남긴다.
#[test]
fn only_a_shell_that_exited_by_itself_closes() {
    use SessionState::*;
    let exited = || Exited { code: Some(0) };
    let role = Some(aaa_proto::Role::Worker);
    let cases = [
        ("기본 셸 exit", info(&[], Running, None), info(&[], exited(), None), false, true),
        (
            "login 셸 exit 3",
            info(&["/bin/zsh", "-l"], Running, None),
            info(&["/bin/zsh", "-l"], Exited { code: Some(3) }, None),
            false,
            true,
        ),
        ("pane kill", info(&["sh"], Running, None), info(&["sh"], exited(), None), true, false),
        ("역할 pane", info(&["sh"], Running, role), info(&["sh"], exited(), role), false, false),
        (
            "프로그램 pane",
            info(&["sh", "-c", "exit 0"], Running, None),
            info(&["sh", "-c", "exit 0"], exited(), None),
            false,
            false,
        ),
        ("재부팅", info(&["sh"], Running, None), info(&["sh"], Interrupted, None), false, false),
        ("유실", info(&["sh"], Running, None), info(&["sh"], Lost, None), false, false),
        ("이미 끝나 있던 것", info(&["sh"], Interrupted, None), info(&["sh"], exited(), None), false, false),
    ];
    for (what, old, new, kill_sent, want) in cases {
        assert_eq!(exited_by_itself(&old, &new, kill_sent), want, "{what}");
    }
}

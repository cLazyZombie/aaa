#[test]
fn verify_uses_a_common_entry_and_preserves_explicit_command_arguments() {
    use super::*;
    let cli = Cli::try_parse_from(["aaa", "verify"]).unwrap();
    assert!(matches!(cli.cmd, Cmd::Verify { command } if command.is_empty()));
    let cli = Cli::try_parse_from(["aaa", "verify", "--", "bash", "scripts/verify.sh", "--coverage"]).unwrap();
    assert!(matches!(cli.cmd, Cmd::Verify { command } if command == ["bash", "scripts/verify.sh", "--coverage"]));
}

#[test]
fn public_port_change_parses_and_rejects_off_and_invalid_ports() {
    use super::{Cli, Cmd, WebCmd};
    use clap::Parser as _;
    let cli = Cli::try_parse_from(["aaa", "web", "publish", "--port", "7443"]).unwrap();
    assert!(matches!(cli.cmd, Cmd::Web { cmd: WebCmd::Publish { off: None, port: Some(7443) } }));
    for args in [
        vec!["aaa", "web", "publish", "off", "--port", "7443"],
        vec!["aaa", "web", "publish", "--port", "0"],
        vec!["aaa", "web", "publish", "--port", "65536"],
    ] {
        assert!(Cli::try_parse_from(args).is_err());
    }
}
#[test]
fn project_lifecycle_help_omits_legacy_project_arguments() {
    use super::Cli;
    use clap::Parser as _;
    for verb in ["pause", "resume", "archive", "delete", "sweep", "rename"] {
        let help = Cli::try_parse_from(["aaa", "project", verb, "--help"]).err().unwrap().to_string();
        assert!(!help.contains("[PROJECT]") && !help.contains("<PROJECT>") && !help.contains("LEGACY"), "{help}");
        let mut args = vec!["aaa", "project", verb];
        if verb == "rename" {
            assert!(help.contains("<NEW_NAME>"), "{help}");
            assert!(Cli::try_parse_from(&args).is_err(), "새 이름은 필수다");
            args.push("next");
        }
        assert!(Cli::try_parse_from(&args).is_ok(), "{args:?}");
        args.push("old");
        assert!(Cli::try_parse_from(&args).is_ok(), "옛 꼴: {args:?}");
        args.push("extra");
        assert!(Cli::try_parse_from(&args).is_err(), "넘치는 인자: {args:?}");
    }
}

#[test]
fn update_accepts_release_version_and_keeps_local_and_subcommand_modes() {
    use super::{Cli, Cmd};
    use clap::Parser as _;
    for args in [
        vec!["aaa", "update"],
        vec!["aaa", "update", "--version", "v0.2.0", "--dry-run"],
        vec!["aaa", "update", "--from", "aaa.new", "--dry-run"],
        vec!["aaa", "update", "rollback", "--dry-run"],
    ] {
        assert!(Cli::try_parse_from(args).is_ok());
    }
    let cli = Cli::try_parse_from(["aaa", "update", "--version", "v0.2.0"]).unwrap();
    assert!(matches!(cli.cmd, Cmd::Update { release_version: Some(v), .. } if v == "v0.2.0"));
    assert!(Cli::try_parse_from(["aaa", "update", "--from", "aaa.new", "--version", "0.2.0"]).is_err());
    assert!(Cli::try_parse_from(["aaa", "update", "rollback", "--version", "0.2.0"]).is_err());
}

#[test]
fn follow_fills_a_gap_instead_of_dropping_it() {
    use super::{FollowStep, follow_step};
    assert_eq!(follow_step(20, 21), FollowStep::Show);
    assert_eq!(follow_step(20, 20), FollowStep::Skip);
    // 22가 21보다 먼저 왔다(host가 21을 늦게 보냈거나 놓쳤다): 21을 버리지 않고 20부터 다시 읽는다
    assert_eq!(follow_step(20, 22), FollowStep::Refill { after: 20 });
    // 다시 읽어 22까지 보인 뒤 늦게 온 21은 이미 본 것이다
    assert_eq!(follow_step(22, 21), FollowStep::Skip);
    // 처음 읽기에 없던 프로젝트: cursor 0이면 첫 사건은 seq 1이다
    assert_eq!(follow_step(0, 1), FollowStep::Show);
    assert_eq!(follow_step(0, 3), FollowStep::Refill { after: 0 });
}

use super::*;

#[test]
fn message_requires_text_or_file() {
    for tail in [vec![], vec!["--text", "x", "--text-file", "task.md"]] {
        let args = [&["aaa", "thread", "message", "demo", "t-0001"][..], &tail].concat();
        assert!(Cli::try_parse_from(args).is_err());
    }
    for tail in [["--text", "x"], ["--text-file", "task.md"], ["--text-file", "-"]] {
        let args = [&["aaa", "thread", "message", "demo", "t-0001"][..], &tail].concat();
        assert!(Cli::try_parse_from(args).is_ok());
    }
}

#[test]
fn post_help_explains_required_topic_without_changing_lookup_commands() {
    let help = Cli::try_parse_from(["aaa", "post", "--help"]).err().unwrap().to_string();
    assert!(help.contains("--topic") && help.contains("필수"), "{help}");
    // 조회 명령에는 전송 인자를 요구하지 않는다.
    assert!(Cli::try_parse_from(["aaa", "post", "list", "demo"]).is_ok());
}

/// `aaa report`는 herdr-projects `report`와 같은 인자를 받는다(plan §8.78)
#[test]
fn report_takes_percent_or_unknown_and_an_activity() {
    let ok = |a: &[&str]| Cli::try_parse_from([&["aaa", "report"][..], a].concat()).is_ok();
    assert!(ok(&["--percent", "25", "--activity", "코드 읽기"]));
    assert!(ok(&["--unknown", "--activity", "일 가늠"]));
    // 백분율이나 --unknown 하나, 활동은 늘 있어야 한다
    assert!(!ok(&["--activity", "코드 읽기"]));
    assert!(!ok(&["--percent", "25", "--unknown", "--activity", "x"]));
    assert!(!ok(&["--percent", "101", "--activity", "x"]));
    assert!(!ok(&["--percent", "25"]));
}

/// `aaa thread answer`의 답 글(plan §8.329): 띄어 쓰기·쉼표·붙여 쓰기·소문자를 받고, 없는 질문·꼴이 아닌 글은 거절한다
#[test]
fn answer_picks_read_question_numbers_and_letters() {
    use super::answer_picks;
    assert_eq!(answer_picks("1A 2B", 2).unwrap(), [Some(0), Some(1)]);
    assert_eq!(answer_picks("1a,2c", 3).unwrap(), [Some(0), Some(2), None]);
    assert_eq!(answer_picks("2B1A", 2).unwrap(), [Some(0), Some(1)]);
    assert_eq!(answer_picks("", 2).unwrap(), [None, None]);
    assert!(answer_picks("3A", 2).unwrap_err().to_string().contains("3번 질문이 없다"));
    assert!(answer_picks("A1", 2).is_err());
    assert!(answer_picks("1", 2).is_err());
}

/// 이미 답한 번호가 빠진 카드에도 원래 번호로 의견을 붙이고 잘못된 입력은 거절한다.
#[test]
fn answer_comments_keep_source_numbers_and_multiline_text() {
    assert!(
        Cli::try_parse_from([
            "aaa",
            "thread",
            "answer",
            "t-1000",
            "3A",
            "--comment",
            "3=간격 유지",
            "--comment",
            "7=둘 다"
        ])
        .is_ok()
    );
    let mut items = aaa_proto::report_questions("## Questions\n### 1. 자리\n- A 유지\n### 2. 색\n- A 유지\n");
    items[0].source_number = Some(3);
    items[1].source_number = Some(7);
    let values = vec!["7=둘 다\n보이게=좋다".into(), "3=간격 유지".into()];
    assert_eq!(
        super::answer_comments(&values, &items).unwrap(),
        vec![Some("간격 유지".into()), Some("둘 다\n보이게=좋다".into())]
    );
    for bad in
        [vec!["1=없음".into()], vec!["3=".into()], vec!["3=첫째".into(), "3=둘째".into()], vec!["형식 없음".into()]]
    {
        assert!(super::answer_comments(&bad, &items).is_err());
    }
    assert!(super::answer_comments(&[], &items).unwrap().is_empty());
}

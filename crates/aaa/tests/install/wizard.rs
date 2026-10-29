use super::authorization::terminal_steps;
use super::*;
#[test]
fn wizard_reports_disk_capacity_and_path_before_asking_to_change_choices() {
    let lab = Lab::new();
    let mut args = lab.args();
    let at = args.iter().position(|a| a == "--min-free-gb").unwrap();
    args[at + 1] = u32::MAX.to_string();
    args.extend([
        "--mode".into(),
        "fresh".into(),
        "--login".into(),
        "defer".into(),
        "--path".into(),
        "preserve".into(),
        "--prompt".into(),
        "plain".into(),
    ]);
    let before = snapshot(&lab.0);
    let (code, text) = terminal_steps(&lab, &args, &[("선택을 바꿀까요", ":cancel\r")], false);
    assert_eq!(code, 130, "{text}");
    assert!(text.contains("디스크 여유 부족: 필요 4294967295GB, 남은 "), "{text}");
    assert!(text.contains(lab.0.canonicalize().unwrap().join("작업 root").to_str().unwrap()), "{text}");
    assert_eq!(snapshot(&lab.0), before);
}

#[test]
fn dry_run_cannot_save_a_draft_or_create_any_other_file_from_either_prompt_backend() {
    for plain in [true, false] {
        let lab = Lab::new();
        let before = snapshot(&lab.0);
        let path = lab.0.join("draft.json");
        let save = format!(":save {}\r", path.display());
        let mut args = lab.args();
        if plain {
            args.extend(["--prompt".into(), "plain".into()]);
        }
        let (code, text) = terminal_steps(&lab, &args, &[("설치 방식", &save)], false);
        assert_eq!(code, 2, "dry-run에서는 명시 저장도 새 파일을 만들지 않아야 한다: {text}");
        assert_eq!(snapshot(&lab.0), before);
    }
}
#[test]
fn both_wizards_back_from_host_choices_then_submit_the_same_agent_plan_as_explicit_cli() {
    for plain in [true, false] {
        let lab = Lab::new();
        let before = snapshot(&lab.0);
        let mut args = lab.args();
        for flag in ["--role", "--name", "--min-free-gb"] {
            let at = args.iter().position(|a| a == flag).unwrap();
            args.drain(at..at + 2);
        }
        if plain {
            args.extend(["--prompt".into(), "plain".into()]);
        }
        let back = if plain { ":back\r" } else { "\x1b" };
        let steps = [
            ("설치 방식", "fresh\r"),
            ("이 머신의 역할", "host\r"),
            ("머신 표시 이름", "host-a\r"),
            ("host에 로컬 agent", "disabled\r"),
            ("프로젝트", "skip\r"),
            ("사용자 PATH", back),
            ("프로젝트", back),
            ("host에 로컬 agent", back),
            ("머신 표시 이름", back),
            ("이 머신의 역할", "agent\r"),
            ("머신 표시 이름", "agent-final\r"),
            ("남겨 둘 여유 GB", "0\r"),
            ("harness 로그인 검사", "require\r"),
            ("사용자 PATH", "preserve\r"),
        ];
        let (code, text) = terminal_steps(&lab, &args, &steps, false);
        assert_eq!(code, 0, "{text}");
        assert_eq!(snapshot(&lab.0), before, "질문/뒤로/탐지는 변경 0");
        let mut direct = lab.args();
        let at = direct.iter().position(|a| a == "--name").unwrap();
        direct[at + 1] = "agent-final".into();
        direct.extend(["--non-interactive".into(), "--json".into()]);
        let result = json(&lab.run(&direct));
        assert_eq!(result["exit_code"], 0);
        assert!(
            text.contains(result["plan_sha256"].as_str().unwrap()),
            "두 prompt와 argv는 같은 planner/hash를 써야 한다: {text}"
        );
    }
}

#[test]
fn explicitly_saved_private_draft_never_overrides_cli_or_silently_fills_an_automated_run() {
    for plain in [true, false] {
        let lab = Lab::new();
        let path = lab.0.join("draft.json");
        let mut args = lab.args_asking_free_space();
        args.retain(|a| a != "--dry-run");
        if plain {
            args.extend(["--prompt".into(), "plain".into()]);
        }
        let save = format!(":save {}\r", path.display());
        let (code, text) = terminal_steps(&lab, &args, &[("설치 방식", "repair\r"), ("남겨 둘 여유 GB", &save)], false);
        assert_eq!(code, 130, "초안 저장 뒤 설치 취소는 v1 cancelled/130 계약이다: {text}");
        let draft: Value =
            serde_json::from_slice(&aaa_common::private_file::read(&path, 1024 * 1024).unwrap()).unwrap();
        assert_eq!(draft["schema"], "aaa.install.draft.v1");
        assert_eq!(draft["choices"]["mode"], "repair");
        assert!(!lab.0.join("state/install").exists());
        assert!(!lab.0.join("config").exists());
        let before = snapshot(&lab.0);
        let mut automated = lab.args();
        automated.extend([
            "--draft".into(),
            path.to_string_lossy().into_owned(),
            "--non-interactive".into(),
            "--json".into(),
        ]);
        assert_eq!(json(&lab.run(&automated))["error"]["code"], "draft_requires_interactive");
        assert_eq!(snapshot(&lab.0), before);
        let mut resumed = lab.args_asking_free_space();
        resumed.extend(["--draft".into(), path.to_string_lossy().into_owned(), "--mode".into(), "fresh".into()]);
        let name = resumed.iter().position(|a| a == "--name").unwrap();
        resumed[name + 1] = "explicit-agent".into();
        if plain {
            resumed.extend(["--prompt".into(), "plain".into()]);
        }
        let (code, text) = terminal_steps(
            &lab,
            &resumed,
            &[("남겨 둘 여유 GB", "0\r"), ("harness 로그인 검사", "require\r"), ("사용자 PATH", "preserve\r")],
            false,
        );
        assert_eq!(code, 0, "{text}");
        assert_eq!(snapshot(&lab.0), before, "초안 읽기·dry-run은 변경 0");
        let mut direct = lab.args();
        let at = direct.iter().position(|a| a == "--name").unwrap();
        direct[at + 1] = "explicit-agent".into();
        direct.extend(["--mode".into(), "fresh".into(), "--json".into(), "--non-interactive".into()]);
        assert!(
            text.contains(json(&lab.run(&direct))["plan_sha256"].as_str().unwrap()),
            "명시 argv가 초안보다 우선한다"
        );
    }
}

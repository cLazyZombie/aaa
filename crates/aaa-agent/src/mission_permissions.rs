//! 특수 임무의 실행별 최소 허용 목록. 전역 harness 설정은 바꾸지 않는다.
use std::path::Path;

use aaa_proto::Launch;
use anyhow::{Result, ensure};
use serde_json::{Value, json};

pub(super) fn claude(settings: &mut Value, exe: &Path, work: &Path, project: &Path) {
    let mut allow = vec![];
    for command in ["aaa".to_string(), exe.to_string_lossy().into_owned(), shell_quote(exe)] {
        allow.push(format!("Bash({command} report *)"));
        allow.push(format!("Bash({command} post coordinator *)"));
    }
    allow.sort();
    allow.dedup();
    // 기존 임무처럼 프로젝트 루트로 cd한 뒤 threads/...를 읽는 명령도 읽기 범위로 판정한다.
    allow.push(format!("Bash(cd {})", project.display()));
    allow.push(format!("Bash(cd {})", shell_quote(project)));
    // additionalDirectories는 읽기 범위다. default 모드에서 쓰기는 아래의 자기 폴더 규칙만 허용한다.
    for tool in ["Read", "Edit", "Write"] {
        allow.push(format!("{tool}(//{}/**)", work.to_string_lossy().replace('\\', "/").trim_start_matches('/')));
    }
    settings["permissions"] = json!({
        "defaultMode": "default",
        "disableBypassPermissionsMode": "disable",
        "allow": allow,
        "additionalDirectories": [project.join("threads"), project.join("library"), project.join("missions")],
    });
}

fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

pub(super) fn codex(launch: &mut Launch, exe: &Path, work: &Path) -> Result<()> {
    // 이 폴더의 config layer에서만 rules를 읽는다. 자기 폴더 밖 쓰기와 외부 통신은 sandbox가 막는다.
    let config = work.join(".codex");
    let rules = config.join("rules");
    for dir in [&config, &rules] {
        if dir.exists() || dir.is_symlink() {
            ensure!(std::fs::symlink_metadata(dir)?.is_dir(), "임무 설정 폴더는 symlink가 아니어야 한다");
        } else {
            std::fs::create_dir(dir)?;
        }
    }
    let mut policy = String::from("# aaa 특수 임무: 보고와 coordinator post만 sandbox 밖에서 허용한다.\n");
    for command in ["aaa".to_string(), exe.to_string_lossy().into_owned()] {
        for args in [vec![&command, "report"], vec![&command, "post", "coordinator"]] {
            policy.push_str(&format!("prefix_rule(pattern={}, decision=\"allow\")\n", serde_json::to_string(&args)?));
        }
    }
    for (path, text) in [
        (config.join("config.toml"), "# aaa 임무 전용 config layer\n".to_string()),
        (rules.join("aaa-mission.rules"), policy),
    ] {
        if path.exists() || path.is_symlink() {
            ensure!(std::fs::symlink_metadata(&path)?.is_file(), "임무 설정 파일은 symlink가 아니어야 한다");
        }
        // 같은 파일을 hard link로 자기 폴더 밖에 붙여 두었어도 그 파일을 덮어쓰지 않는다.
        let temp = path.with_file_name(format!(".aaa-{}", aaa_common::util::new_id()));
        std::fs::write(&temp, text)?;
        std::fs::rename(temp, path)?;
    }
    let work = work.to_string_lossy();
    let config = [
        "default_permissions=\"aaa_mission\"".to_string(),
        format!(
            "permissions.aaa_mission={{filesystem={{\"/\"=\"read\",{}=\"write\"}},network={{enabled=false}}}}",
            json!(work)
        ),
        format!("projects.{}.trust_level=\"trusted\"", json!(work)),
        "approvals_reviewer=\"user\"".to_string(),
    ];
    for argv in [&mut launch.argv, &mut launch.resume_argv] {
        // host가 넣은 호환 sandbox flag와 named permission profile을 함께 주지 않는다.
        if let Some(i) = argv.iter().position(|a| a == "--sandbox") {
            argv.drain(i..(i + 2).min(argv.len()));
        }
        argv.extend(["--cd".into(), work.to_string()]);
        for item in &config {
            argv.extend(["--config".into(), item.clone()]);
        }
    }
    Ok(())
}

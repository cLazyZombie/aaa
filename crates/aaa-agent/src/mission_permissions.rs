//! 특수 임무의 실행별 최소 허용 목록. 전역 harness 설정은 바꾸지 않는다.
use std::path::Path;

use aaa_proto::Launch;
use anyhow::{Result, ensure};
use serde_json::{Value, json};

pub(super) fn prepare_inbox(work: &Path) -> Result<()> {
    let inbox = work.join("inbox");
    match std::fs::symlink_metadata(&inbox) {
        Ok(m) => ensure!(m.is_dir(), "임무 inbox는 symlink가 아닌 폴더여야 한다"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir(&inbox)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&inbox, std::fs::Permissions::from_mode(0o700))?;
            }
        }
        Err(e) => return Err(e.into()),
    }
    Ok(())
}

/// 상태 조회는 자기 프로젝트·임무로 고정한다. 명령 탐색도 부작용 없는 help 두 개만 허용한다.
pub(super) fn queries(launch: &Launch, project: &Path) -> Result<Vec<Vec<String>>> {
    let mut queries = vec![vec!["--help".into()], vec!["mission".into(), "--help".into()]];
    if let Some((_, name)) = launch.env.iter().find(|(k, _)| k == "AAA_MISSION_NAME") {
        let project = project.file_name().and_then(|s| s.to_str()).unwrap_or_default();
        ensure!(aaa_proto::valid_project(project) && aaa_proto::valid_project(name), "임무 조회 이름 형식이 아니다");
        queries.push(vec!["mission".into(), "show".into(), project.into(), name.clone(), "--json".into()]);
    }
    Ok(queries)
}

pub(super) fn claude(settings: &mut Value, exe: &Path, work: &Path, project: &Path, queries: &[Vec<String>]) {
    let mut allow = vec![];
    allow.push(format!("Bash({})", aaa_proto::mission_inbox_list_command(&work.to_string_lossy())));
    for command in aaa_proto::mission_git_read_commands(&work.to_string_lossy()) {
        allow.push(format!("Bash({command} *)"));
    }
    for command in [
        "aaa".to_string(),
        exe.to_string_lossy().into_owned(),
        shell_quote(exe),
        aaa_proto::fill_brief(aaa_proto::BRIEF_EXE, "", &exe.to_string_lossy()),
    ] {
        allow.push(format!("Bash({command} report *)"));
        allow.push(format!("Bash({command} post coordinator *)"));
        for query in queries {
            allow.push(format!("Bash({command} {})", query.join(" ")));
        }
    }
    allow.sort();
    allow.dedup();
    // 기존 임무처럼 프로젝트 루트로 cd한 뒤 threads/...를 읽는 명령도 읽기 범위로 판정한다.
    allow.push(format!("Bash(cd {})", project.display()));
    allow.push(format!("Bash(cd {})", shell_quote(project)));
    // 자기 폴더의 추가 허용만 보강한다. 승인·권한 모드는 host의 일반 thread safety를 따른다.
    for tool in ["Read", "Edit", "Write"] {
        allow.push(format!("{tool}(//{}/**)", work.to_string_lossy().replace('\\', "/").trim_start_matches('/')));
    }
    settings["permissions"] = json!({
        "allow": allow,
        "additionalDirectories": [project.join("threads"), project.join("library"), project.join("missions")],
    });
}

fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}

pub(super) fn codex(launch: &mut Launch, exe: &Path, work: &Path, queries: &[Vec<String>]) -> Result<()> {
    // 자기 폴더의 추가 rules만 둔다. sandbox·외부 통신은 일반 thread의 설정을 그대로 따른다.
    let config = work.join(".codex");
    let rules = config.join("rules");
    for dir in [&config, &rules] {
        if dir.exists() || dir.is_symlink() {
            ensure!(std::fs::symlink_metadata(dir)?.is_dir(), "임무 설정 폴더는 symlink가 아니어야 한다");
        } else {
            std::fs::create_dir(dir)?;
        }
    }
    let mut policy = String::from("# aaa 특수 임무의 추가 허용: 보고·coordinator post·자기 임무 조회·help.\n");
    for command in ["aaa".to_string(), exe.to_string_lossy().into_owned()] {
        for args in [vec![&command, "report"], vec![&command, "post", "coordinator"]] {
            policy.push_str(&format!("prefix_rule(pattern={}, decision=\"allow\")\n", serde_json::to_string(&args)?));
        }
        for query in queries {
            let args: Vec<_> = std::iter::once(command.as_str()).chain(query.iter().map(String::as_str)).collect();
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
    // 권한·승인 모드는 일반 thread와 같은 host safety 인자를 보존한다.
    let config = [format!("projects.{}.trust_level=\"trusted\"", json!(work))];
    for argv in [&mut launch.argv, &mut launch.resume_argv] {
        argv.extend(["--cd".into(), work.to_string()]);
        for item in &config {
            argv.extend(["--config".into(), item.clone()]);
        }
    }
    Ok(())
}

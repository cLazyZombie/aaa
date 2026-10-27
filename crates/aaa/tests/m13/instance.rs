//! 같은 Mac의 둘째 aaa(plan §8.294). HOME만 실험실로 바꾸고 dir env는 비워, `--instance`가 정하는 실제 기본 경로를 본다.
//! 운영 `~/.config/aaa`·launchd에는 닿지 않는다(HOME이 실험실이고 감독자는 설치하지 않는다).
#![cfg(target_os = "macos")]
use super::*;
use aaa_common::child_process::StdCommandExt;

/// 실험실 HOME에서 dir env 없이 도는 명령. 빈 값은 「정하지 않음」이다(`aaa_config::paths`)
fn bare(node: &Node, home: &Path, args: &[&str]) -> Command {
    let mut c = node.cmd(args);
    c.env("HOME", home)
        .env("AAA_CONFIG_DIR", "")
        .env("AAA_STATE_DIR", "")
        .env("XDG_CONFIG_HOME", "")
        .env("XDG_STATE_HOME", "")
        .env_remove("AAA_INSTANCE");
    c
}

fn bare_env(home: &Path) -> Vec<(&'static str, String)> {
    vec![
        ("HOME", home.to_string_lossy().into_owned()),
        ("AAA_CONFIG_DIR", String::new()),
        ("AAA_STATE_DIR", String::new()),
        ("XDG_CONFIG_HOME", String::new()),
        ("XDG_STATE_HOME", String::new()),
    ]
}

fn json_of(o: &std::process::Output) -> Value {
    serde_json::from_slice(&o.stdout).unwrap_or_else(|e| {
        panic!("JSON이 아니다({e}): {}\n{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
    })
}

/// 실제 HTTP 한 번(`Connection: close`): 상태 줄과 머리를 돌려준다
fn http_post(port: u16, path: &str, body: &str) -> String {
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    let origin = format!("http://127.0.0.1:{port}");
    write!(
        s,
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: {origin}\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    out
}

#[test]
fn second_instance_has_its_own_folders_host_and_cookie() {
    let mut lab = Lab::new();
    let home = lab.root.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let node = lab.node("x", None);
    let env = bare_env(&home);
    let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();

    // 실험실 dir env가 남아 있으면 그 폴더를 쓰게 되므로 거부한다
    let o = node.cmd(&["--instance", "work", "init"]).env("HOME", &home).capture_output().unwrap();
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("AAA_CONFIG_DIR가 정해져 있다"), "{o:?}");
    // 이름 규칙
    let o = bare(&node, &home, &["--instance", "Work", "status"]).capture_output().unwrap();
    assert_eq!(o.status.code(), Some(2), "{o:?}");
    assert!(String::from_utf8_lossy(&o.stderr).contains("인스턴스 이름 `Work`"), "{o:?}");
    // 사람 터미널이 아니면 만들지 않는다
    let o = bare(&node, &home, &["--instance", "work", "init"]).capture_output().unwrap();
    assert!(String::from_utf8_lossy(&o.stderr).contains("터미널"), "{o:?}");
    assert!(!home.join(".config/aaa-work").exists());
    // 기본 인스턴스에 같은 이름의 프로젝트가 있으면 branch 내부 이름이 부딪힌다
    std::fs::create_dir_all(home.join(".aaa/projects/clash")).unwrap();
    let out = node.in_terminal_env(&["--instance", "clash", "init"], &env, &[], T);
    assert!(out.contains("기본 인스턴스에 프로젝트 `clash`"), "{out}");
    assert!(!home.join(".config/aaa-clash/host.toml").exists());

    let out = node.in_terminal_env(&["--instance", "work", "init"], &env, &[], T);
    assert!(out.contains("만들었다: 인스턴스 work"), "{out}");
    let cfg = home.join(".config/aaa-work");
    let text = std::fs::read_to_string(cfg.join("host.toml")).unwrap();
    let port = |key: &str| -> u16 {
        let line = text.lines().find(|l| l.starts_with(key)).unwrap_or_else(|| panic!("{key}: {text}"));
        line.trim_end_matches('"').rsplit(':').next().unwrap().parse().unwrap()
    };
    let (link, web) = (port("agent_listen"), port("listen"));
    assert!(link != 7430 && web == link + 1, "{text}");
    assert!(home.join(".aaa-work/projects").is_dir());
    assert!(!home.join(".config/aaa").exists(), "기본 인스턴스 폴더를 만들었다");
    // 두 번째 init은 덮지 않는다
    let again = node.in_terminal_env(&["--instance", "work", "init"], &env, &[], T);
    assert!(again.contains("이미 있다"), "{again}");
    // Agent link는 실험실 밖으로 열지 않는다(포트는 init이 고른 그대로)
    std::fs::write(cfg.join("host.toml"), text.replace("0.0.0.0:", "127.0.0.1:")).unwrap();

    // 인스턴스 host와 로컬 agent: env는 `AAA_INSTANCE`뿐이고 폴더는 그 이름에서 나온다
    lab.spawn_command("work-host", bare(&node, &home, &["--instance", "work", "host"]));
    let state = home.join("Library/Application Support/aaa-work");
    wait_for("인스턴스 host front.sock", T, || state.join("host/front.sock").exists().then_some(()));
    lab.spawn_command("work-agent", bare(&node, &home, &["--instance", "work", "agent"]));
    wait_for("인스턴스 로컬 agent 연결", T, || {
        let o = bare(&node, &home, &["--instance", "work", "--json", "machine", "list"]).capture_output().ok()?;
        let v: Value = serde_json::from_slice(&o.stdout).ok()?;
        v["machines"].as_array()?.iter().any(|m| m["connection"] == "connected").then_some(())
    });
    assert!(home.join("aaa-work-work").is_dir(), "작업 root가 인스턴스 기본 자리가 아니다");

    // 감독자 plist: label과 `AAA_INSTANCE`(launchd가 띄운 aaa가 같은 폴더를 본다). 읽기만 하고, launchctl은 실험실 가짜다
    let fake = lab.root.join("fake-bin");
    std::fs::create_dir_all(&fake).unwrap();
    std::fs::write(
        fake.join("launchctl"),
        "#!/bin/sh
exit 1
",
    )
    .unwrap();
    std::fs::set_permissions(fake.join("launchctl"), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let launcher = lab.root.join("aaa-launcher");
    std::fs::write(&launcher, "").unwrap();
    let path = format!("{}:{}", fake.display(), std::env::var("PATH").unwrap());
    let show = bare(&node, &home, &["--instance", "work", "service", "show", "--role", "host", "--aaa", lab::BIN])
        .args(["--launcher", path_str(&launcher)])
        .env("PATH", &path)
        .capture_output()
        .unwrap();
    let plist = String::from_utf8_lossy(&show.stdout);
    assert!(show.status.success(), "{show:?}");
    assert!(plist.contains("<string>dev.aaa.work.host</string>"), "{plist}");
    assert!(plist.contains("<key>AAA_INSTANCE</key>\n\t\t<string>work</string>"), "{plist}");
    assert!(plist.contains("Library/Logs/aaa-work/host.log"), "{plist}");
    assert!(!home.join("Library/LaunchAgents").exists(), "show가 plist를 썼다");
    assert!(!home.join("aaa-work").exists());

    // 기본 인스턴스(이름 없음)는 같은 HOME에서도 이 host를 보지 않는다
    let o = bare(&node, &home, &["--json", "status"]).capture_output().unwrap();
    assert!(!o.status.success());
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("Application Support/aaa/host/front.sock"), "{err}");
    // pane 안처럼 env로만 준 인스턴스도 같은 host로 간다
    let o = bare(&node, &home, &["--json", "status"]).env("AAA_INSTANCE", "work").capture_output().unwrap();
    assert!(o.status.success(), "{o:?}");

    // 프로젝트 이름을 생략하면 인스턴스 이름: worktree·branch 내부 이름이 다른 인스턴스와 겹치지 않는다
    let o = bare(&node, &home, &["--instance", "work", "--json", "project", "create"]).capture_output().unwrap();
    assert!(o.status.success(), "{o:?}");
    assert!(home.join(".aaa-work/projects/work/PROJECT.md").is_file());
    let o = bare(&node, &home, &["--json", "project", "create"]).capture_output().unwrap();
    assert!(String::from_utf8_lossy(&o.stderr).contains("프로젝트 이름이 필요하다"), "{o:?}");

    // 웹 장치 cookie 이름에 인스턴스 꼬리: 같은 hostname의 기본 인스턴스 로그인을 덮지 않는다
    let pair = json_of(&bare(&node, &home, &["--instance", "work", "--json", "web", "pair"]).capture_output().unwrap());
    let code = pair["code"].as_str().unwrap_or_else(|| panic!("{pair}"));
    let res = http_post(web, "/api/v1/pair", &format!(r#"{{"code":"{code}","label":"lab"}}"#));
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
    let cookie =
        res.lines().find(|l| l.to_ascii_lowercase().starts_with("set-cookie:")).unwrap_or_else(|| panic!("{res}"));
    assert!(cookie.contains(" aaa-work="), "{cookie}");
}

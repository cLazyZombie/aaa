//! `aaa --instance <이름> init`(plan §8.294): 같은 Mac에 둘째 aaa를 만든다.
//!
//! - 설정·상태·홈은 이름에서 정해지는 형제 폴더다(`aaa_config::paths`). init은 host.toml에 이 Mac의 다른 aaa와 겹치지
//!   않는 포트 쌍(Agent link·웹)을 적고 홈을 만든 뒤, 사람이 칠 다음 명령을 보인다. 감독자 설치·등록은 하지 않는다.
//! - 기본 인스턴스의 홈에 같은 이름의 프로젝트가 있으면 거부한다: 그 이름이 worktree·branch의 내부 이름이라 같은 저장소에서
//!   `aaa/<이름>/t-…`가 부딪힌다.
//! - 가장 긴 unix socket이 SUN_LEN(104)을 넘는 이름은 거부한다(상태 폴더 아래 `agent/run/<접두><ID>.sock`). init을 돌리지
//!   않는 다른 Mac(사용자 이름이 길 수 있다)은 인스턴스 agent가 시작할 때 같은 검사를 한다.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Result, bail};

/// 포트 쌍(Agent link, 웹)을 찾는 범위. 기본 인스턴스의 7430·7431 다음부터 둘씩
const PORTS: std::ops::Range<u16> = 7432..7600;
/// macOS `sockaddr_un.sun_path` 크기(끝 NUL 포함)
const SUN_LEN: usize = 104;

pub fn run(json: bool) -> Result<ExitCode> {
    let Some(name) = aaa_config::paths::instance() else {
        bail!(
            "`aaa --instance <이름> init`으로 둘째 aaa를 만든다. 기본 인스턴스는 설치 절차(docs/install-runbook.md)대로 둔다"
        );
    };
    if cfg!(windows) {
        bail!("인스턴스 만들기는 macOS·Linux에서 한다");
    }
    for env in ["AAA_CONFIG_DIR", "AAA_STATE_DIR"] {
        if std::env::var_os(env).is_some_and(|v| !v.is_empty()) {
            bail!(
                "{env}가 정해져 있다: 인스턴스 폴더 대신 그 폴더를 쓰게 된다. 그 env와 --config-dir·--state-dir 없이 다시 한다"
            );
        }
    }
    crate::person::require("인스턴스 만들기")?;
    let config = aaa_config::paths::config_dir();
    let host_toml = config.join("host.toml");
    if host_toml.exists() {
        bail!("이미 있다: {}. 그 인스턴스를 그대로 쓴다(`aaa --instance {name} config check`)", host_toml.display());
    }
    let state = aaa_config::paths::state_dir();
    socket_fits(&state)?;
    let taken = default_projects_root().join(&name);
    if taken.exists() {
        bail!(
            "기본 인스턴스에 프로젝트 `{name}`이 있다({}). 인스턴스 이름은 worktree·branch의 내부 이름이라 같은 저장소에서 부딪힌다: 다른 이름을 쓴다",
            taken.display()
        );
    }
    let used = used_ports();
    let Some(port) = pick_ports(&used, bindable) else {
        bail!("{}~{} 안에 빈 포트 쌍이 없다", PORTS.start, PORTS.end);
    };
    let home = aaa_config::paths::projects_root_default();
    std::fs::create_dir_all(&config)?;
    std::fs::write(&host_toml, host_toml_text(&name, port))?;
    if let Err(e) = aaa_config::load_host(&config) {
        aaa_common::diagnostic::ignored(
            std::fs::remove_file(&host_toml),
            "정리 실패가 본 작업의 결과를 덮지 않게 한다",
        );
        bail!("만든 host.toml을 읽지 못해 지웠다:\n{}", e.render());
    }
    std::fs::create_dir_all(&home)?;
    let work_root = aaa_config::work_root::resolve("", &aaa_config::paths::home()).map_err(anyhow::Error::msg)?;
    if json {
        println!(
            "{}",
            serde_json::json!({"instance": name, "config": config, "state": state, "home": home,
                "work_root": work_root, "agent_listen": format!("0.0.0.0:{port}"), "web": format!("127.0.0.1:{}", port + 1)})
        );
        return Ok(ExitCode::SUCCESS);
    }
    let a = format!("aaa --instance {name}");
    println!("만들었다: 인스턴스 {name}");
    println!("  설정: {} (Agent link 0.0.0.0:{port} · 웹 127.0.0.1:{})", host_toml.display(), port + 1);
    println!("  상태: {}", state.display());
    println!("  홈: {}", home.display());
    println!("  작업 root(각 Mac 기본): {}", work_root.display());
    println!("다음은 사람이 터미널에서 한다:");
    println!("  1. ✋ {a} service install --role host --aaa ~/.local/bin/aaa");
    println!("  2. ✋ {a} service install --role agent --aaa ~/.local/bin/aaa");
    println!("  3. {a} project create   (이름을 생략하면 `{name}`)");
    println!(
        "  4. 다른 Mac: {a} agent enroll <이 Mac>:{port} → ✋ {a} service install --role agent --aaa ~/.local/bin/aaa"
    );
    println!("  5. {a} web publish   (이 Mac의 다른 aaa가 쓰지 않는 tailnet 포트를 고른다)");
    println!("Codex hook은 binary를 같이 쓰면 그대로 둔다: 한 줄이 pane 환경으로 자기 인스턴스를 찾는다");
    Ok(ExitCode::SUCCESS)
}

fn host_toml_text(name: &str, port: u16) -> String {
    format!(
        "schema = {}\n\
         # `aaa --instance {name} init`이 만들었다(plan §8.294). 포트는 이 Mac의 다른 aaa와 겹치지 않게 골랐다\n\
         [host]\n\
         agent_listen = \"0.0.0.0:{port}\"\n\
         [web]\n\
         listen = \"127.0.0.1:{}\"\n\
         [local_agent]\n\
         mode = \"launchd\"\n",
        aaa_config::SCHEMA,
        port + 1
    )
}

/// 가장 긴 unix socket(배포 guard `agent/run/b<ID>.sock`)이 들어가는가
pub(crate) fn socket_fits(state: &Path) -> Result<()> {
    let longest = longest_socket(state);
    let len = longest.as_os_str().len();
    if len >= SUN_LEN {
        bail!(
            "상태 폴더 경로가 길어 unix socket({len}자, 한도 {})을 열 수 없다: {}. 더 짧은 인스턴스 이름을 쓴다",
            SUN_LEN - 1,
            longest.display()
        );
    }
    Ok(())
}

fn longest_socket(state: &Path) -> PathBuf {
    state.join("agent").join("run").join(format!("b{}.sock", "0".repeat(26)))
}

/// 기본 인스턴스의 홈(host.toml `[projects] root`, 비었으면 기본 자리)
fn default_projects_root() -> PathBuf {
    let dir = aaa_config::paths::default_config_dir_for(None);
    match aaa_config::load_host(&dir) {
        Ok(l) if !l.config.projects.root.is_empty() => PathBuf::from(&l.config.projects.root),
        _ => aaa_config::paths::projects_root_default_for(None),
    }
}

/// 이 Mac의 aaa들(기본·다른 인스턴스) host.toml이 쓰는 포트
fn used_ports() -> BTreeSet<u16> {
    let mut used = BTreeSet::new();
    let base = aaa_config::paths::default_config_dir_for(None);
    let Some(parent) = base.parent() else { return used };
    let Ok(entries) = std::fs::read_dir(parent)
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다"))
    else {
        return used;
    };
    for e in entries.flatten() {
        let n = e.file_name().to_string_lossy().into_owned();
        if n != "aaa" && !n.starts_with("aaa-") {
            continue;
        }
        let Ok(l) = aaa_config::load_host(&e.path())
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다"))
        else {
            continue;
        };
        if l.files.is_empty() {
            continue;
        }
        for addr in [&l.config.host.agent_listen, &l.config.web.listen] {
            if let Some(p) = addr.rsplit_once(':').and_then(|(_, p)| {
                p.parse()
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
            }) {
                used.insert(p);
            }
        }
    }
    // 기본 인스턴스는 host.toml이 없어도 기본 포트를 쓴다
    used.extend([7430, 7431]);
    used
}

fn bindable(addr: &str) -> bool {
    std::net::TcpListener::bind(addr)
        .as_ref()
        .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
        .is_ok()
}

/// 다른 aaa가 적어 두지 않았고 지금 열 수 있는 첫 포트 쌍(Agent link p, 웹 p+1)
fn pick_ports(used: &BTreeSet<u16>, can_bind: impl Fn(&str) -> bool) -> Option<u16> {
    PORTS.step_by(2).find(|&p| {
        !used.contains(&p)
            && !used.contains(&(p + 1))
            && can_bind(&format!("0.0.0.0:{p}"))
            && can_bind(&format!("127.0.0.1:{}", p + 1))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ports_skip_other_instances_and_busy_ones() {
        let used = BTreeSet::from([7430, 7431, 7432, 7433, 7435]);
        // 7432는 다른 인스턴스, 7434의 짝 7435도 다른 인스턴스, 7436은 지금 다른 프로그램이 연다
        assert_eq!(pick_ports(&used, |a| !a.ends_with(":7436")), Some(7438));
        assert_eq!(pick_ports(&used, |_| false), None);
    }

    #[test]
    fn host_toml_is_valid_and_names_the_ports() {
        let dir = std::env::temp_dir().join(format!("aaa-init-{}", aaa_common::util::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("host.toml"), host_toml_text("work", 7432)).unwrap();
        let l = aaa_config::load_host(&dir).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(l.config.host.agent_listen, "0.0.0.0:7432");
        assert_eq!(l.config.web.listen, "127.0.0.1:7433");
        assert_eq!(l.config.local_agent.mode, aaa_config::LocalAgentMode::Launchd);
    }

    #[test]
    fn long_state_dirs_are_refused() {
        assert!(socket_fits(Path::new("/opt/example/Library/Application Support/aaa-work")).is_ok());
        let long = format!("/opt/{}/Library/Application Support/aaa-work", "x".repeat(40));
        assert!(socket_fits(Path::new(&long)).unwrap_err().to_string().contains("짧은 인스턴스 이름"));
    }
}

//! 설치 흐름: 가짜 Windows(`FakeSys`)와 정해 둔 답(`Script`)으로 질문 순서·바꾼 것·되돌림을 본다.
use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::time::Duration;

use aaa_proto::{ErrorBody, ErrorCode};
use anyhow::{Result, bail};

use super::Args;
use super::flow::*;

struct Lab(PathBuf);
impl Lab {
    fn new() -> Lab {
        let root = std::env::temp_dir().join(format!("aaa-setup-{}", aaa_common::util::new_id()));
        std::fs::create_dir_all(root.join("download")).unwrap();
        std::fs::write(root.join("download/aaa-setup.exe"), b"new-binary").unwrap();
        Lab(root)
    }
    fn layout(&self) -> Layout {
        let home = self.0.join("home");
        let local = home.join("AppData/Local/aaa");
        Layout {
            instance: None,
            source: self.0.join("download/aaa-setup.exe"),
            exe: home.join(".local/bin/aaa.exe"),
            config: local.join("config"),
            state: local.join("state"),
            explicit_dirs: false,
            home,
        }
    }
}
impl Drop for Lab {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Default)]
struct Script {
    answers: VecDeque<&'static str>,
    secrets: VecDeque<&'static str>,
    asked: Vec<String>,
    out: Vec<(Tone, String)>,
}
impl Script {
    fn new(answers: &[&'static str], secrets: &[&'static str]) -> Script {
        Script {
            answers: answers.iter().copied().collect(),
            secrets: secrets.iter().copied().collect(),
            ..Default::default()
        }
    }
    fn printed(&self, needle: &str) -> bool {
        self.out.iter().any(|(_, line)| line.contains(needle))
    }
}
impl Ui for Script {
    fn say(&mut self, tone: Tone, line: &str) {
        self.out.push((tone, line.to_string()));
    }
    fn ask(&mut self, label: &str, default: Option<&str>) -> Result<String> {
        self.asked.push(label.to_string());
        let Some(answer) = self.answers.pop_front() else { bail!("답이 준비되지 않은 질문: {label}") };
        if answer == "^C" {
            return Err(Cancelled.into());
        }
        Ok(if answer.is_empty() { default.unwrap_or_default().to_string() } else { answer.to_string() })
    }
    fn secret(&mut self, label: &str) -> Result<Secret> {
        self.asked.push(format!("secret:{label}"));
        let Some(value) = self.secrets.pop_front() else { bail!("암호가 준비되지 않았다") };
        Ok(Secret::new(value.encode_utf16().collect()))
    }
    fn spin(&mut self, _: &str, _: Duration) {}
    fn spin_end(&mut self) {}
    fn cancelled(&self) -> bool {
        false
    }
    fn pause(&mut self, _: Duration) {}
}

struct FakeSys {
    status: PathBuf,
    password: &'static str,
    right: Right,
    services: BTreeMap<String, ServiceInfo>,
    enroll: VecDeque<Option<ErrorCode>>,
    enrolled_as: Vec<String>,
    path: Option<String>,
    unreachable: Vec<String>,
    fail_start: bool,
    connects: bool,
    ops: Vec<String>,
}
impl FakeSys {
    fn new(layout: &Layout) -> FakeSys {
        FakeSys {
            status: layout.status(),
            password: "correct horse",
            right: Right::Missing,
            services: BTreeMap::new(),
            enroll: VecDeque::new(),
            enrolled_as: vec![],
            path: Some(r"C:\tools;%USERPROFILE%\scoop\shims".into()),
            unreachable: vec![],
            fail_start: false,
            connects: true,
            ops: vec![],
        }
    }
    fn did(&self, op: &str) -> bool {
        self.ops.iter().any(|o| o == op)
    }
}
fn account() -> Account {
    Account {
        sam: r"WIN-A\person".into(),
        sid: "S-1-5-21-1-2-3-1001".into(),
        profile: PathBuf::from(r"C:\Users\person"),
    }
}
impl System for FakeSys {
    fn account(&mut self) -> Result<Account> {
        Ok(account())
    }
    fn computer(&mut self) -> String {
        "WIN-A".into()
    }
    fn service(&mut self, name: &str) -> Result<Option<ServiceInfo>> {
        Ok(self.services.get(name).cloned())
    }
    fn aaa_services(&mut self) -> Result<Vec<ServiceInfo>> {
        Ok(self.services.values().cloned().collect())
    }
    fn reach(&mut self, addr: &str) -> Result<()> {
        self.ops.push(format!("reach {addr}"));
        if self.unreachable.iter().any(|a| a == addr) {
            bail!("연결 거부");
        }
        Ok(())
    }
    fn enroll(&mut self, ui: &mut dyn Ui, state: &Path, addr: &str, name: &str) -> Result<()> {
        self.ops.push(format!("enroll {name}"));
        let ident = aaa_agent::identity::load_or_create(state)?;
        let _ = aaa_common::tls::load_or_create(state, "aaa-agent")?;
        assert_eq!(ident.machine_id.len(), 26);
        if let Some(Some(code)) = self.enroll.pop_front() {
            return Err(aaa_agent::link::EnrollRejected(ErrorBody::new(code, "fake")).into());
        }
        for (tone, line) in code_lines(name, "123456") {
            ui.say(tone, &line);
        }
        self.enrolled_as.push(name.into());
        aaa_agent::identity::save_pin(
            state,
            &aaa_agent::identity::HostPin { address: addr.into(), fingerprint: "f".repeat(64), enrolled_at_ms: 1 },
        )
    }
    fn logon(&mut self, account: &Account, secret: &Secret, service: bool) -> Result<Logon> {
        assert_eq!(account.sid, "S-1-5-21-1-2-3-1001");
        self.ops.push(if service { "logon service".into() } else { "logon network".into() });
        if service && self.right != Right::Granted {
            return Ok(Logon::NotGranted);
        }
        Ok(match secret.text().as_str() {
            "locked" => Logon::Other("계정이 잠겼습니다. 잠금이 풀린 뒤 다시 하세요".into()),
            text if text == self.password => Logon::Ok,
            _ => Logon::WrongPassword,
        })
    }
    fn logon_right(&mut self, _: &Account) -> Result<Right> {
        Ok(self.right)
    }
    fn account_services(&mut self, account: &Account) -> Result<Vec<String>> {
        Ok(self.services.values().filter(|s| s.account == account.short()).map(|s| s.name.clone()).collect())
    }
    fn set_logon_right(&mut self, _: &Account, on: bool) -> Result<()> {
        self.ops.push(format!("right {on}"));
        self.right = if on { Right::Granted } else { Right::Missing };
        Ok(())
    }
    fn create_service(&mut self, def: &ServiceDef, account: &Account, secret: &Secret) -> Result<()> {
        assert_eq!(secret.text(), self.password, "확인한 암호로 등록한다");
        self.ops.push(format!("create {}", def.name));
        self.services.insert(
            def.name.clone(),
            ServiceInfo {
                name: def.name.clone(),
                running: false,
                pid: 0,
                automatic: true,
                account: account.short(),
                command: def.command.clone(),
            },
        );
        Ok(())
    }
    fn start_service(&mut self, name: &str) -> Result<u32> {
        self.ops.push(format!("start {name}"));
        if self.fail_start {
            bail!("Windows가 이 계정으로 서비스를 띄우지 못했습니다(1069)");
        }
        let service = self.services.get_mut(name).expect("등록한 서비스");
        service.running = true;
        service.pid = 4242;
        if self.connects {
            let status = ServiceStatus { pid: 4242, connected: true, changed_ms: 1 };
            std::fs::write(&self.status, serde_json::to_vec(&status)?)?;
        }
        Ok(4242)
    }
    fn stop_service(&mut self, name: &str) -> Result<()> {
        self.ops.push(format!("stop {name}"));
        if let Some(service) = self.services.get_mut(name) {
            service.running = false;
            service.pid = 0;
        }
        Ok(())
    }
    fn delete_service(&mut self, name: &str) -> Result<()> {
        self.ops.push(format!("delete {name}"));
        self.services.remove(name);
        Ok(())
    }
    fn user_path(&mut self) -> Result<Option<String>> {
        Ok(self.path.clone())
    }
    fn set_user_path(&mut self, value: Option<&str>) -> Result<()> {
        self.ops.push(format!("path {value:?}"));
        self.path = value.map(str::to_owned);
        Ok(())
    }
}

/// host · 이름 · 작업 폴더 · 진행
const DEFAULTS: [&str; 4] = ["studio.example.ts.net", "", "", ""];

#[test]
fn fresh_install_asks_three_questions_and_a_password_then_connects() {
    let lab = Lab::new();
    let layout = lab.layout();
    let mut sys = FakeSys::new(&layout);
    let mut ui = Script::new(&DEFAULTS, &["correct horse"]);
    Setup::new(&mut sys, &mut ui, layout.clone(), "aaa 0.1.5".into()).install(&Args::default()).unwrap();

    assert_eq!(
        ui.asked,
        [
            "host 주소 (이름:포트)",
            "이 PC 이름 (host 목록에 보일 이름)",
            "작업 폴더 (thread·worktree가 생기는 곳)",
            "secret:Windows 암호",
            "진행할까요? (Y/n)"
        ]
    );
    // 바꾸기 전에 host 연결·암호를 보고, 등록 승인부터 한 뒤 서비스를 만든다
    let path = sys.path.clone().unwrap();
    let path_op = format!("path Some({path:?})");
    let order: Vec<&str> = sys.ops.iter().map(String::as_str).collect();
    assert_eq!(
        order,
        [
            "reach studio.example.ts.net:7430",
            "logon network",
            "enroll win-a",
            &path_op,
            "right true",
            "logon service",
            "create aaa-agent",
            "start aaa-agent"
        ]
    );
    assert!(sys.did("start aaa-agent"));
    assert!(path.ends_with(&format!(";{}", layout.exe.parent().unwrap().display())), "{path}");
    assert_eq!(std::fs::read(&layout.exe).unwrap(), b"new-binary");
    let toml = std::fs::read_to_string(layout.config.join("agent.toml")).unwrap();
    let doc: toml_edit::DocumentMut = toml.parse().unwrap();
    assert_eq!(doc["agent"]["host"].as_str(), Some("studio.example.ts.net:7430"));
    assert_eq!(doc["agent"]["name"].as_str(), Some("win-a"));
    assert_eq!(doc["agent"]["work_root"].as_str(), Some(layout.home.join("aaa-work").to_str().unwrap()));
    assert!(layout.home.join("aaa-work").is_dir());
    let receipt = Receipt::read(&layout.receipt()).unwrap();
    assert!(receipt.logon_right_added);
    assert_eq!(receipt.path_added.as_deref(), layout.exe.parent());
    assert_eq!((receipt.host.as_str(), receipt.name.as_str()), ("studio.example.ts.net:7430", "win-a"));
    let command = &sys.services["aaa-agent"].command;
    assert!(command.ends_with(" setup-service") && command.contains("aaa.exe"), "{command}");
    assert!(ui.printed("확인 코드  123 456"));
    assert!(ui.printed("완료"));
}

#[test]
fn failure_after_registration_rolls_back_everything_but_the_registration() {
    let lab = Lab::new();
    let layout = lab.layout();
    std::fs::create_dir_all(layout.exe.parent().unwrap()).unwrap();
    std::fs::write(&layout.exe, b"old-binary").unwrap();
    std::fs::create_dir_all(&layout.config).unwrap();
    std::fs::write(layout.config.join("agent.toml"), "schema = 1\n# 내 설정\n[health]\nmin_free_gb = 5\n").unwrap();
    let mut sys = FakeSys::new(&layout);
    sys.fail_start = true;
    let before_path = sys.path.clone();
    let mut ui = Script::new(&DEFAULTS, &["correct horse"]);
    let error =
        Setup::new(&mut sys, &mut ui, layout.clone(), "aaa 0.1.5".into()).install(&Args::default()).unwrap_err();
    assert!(format!("{error:#}").contains("1069"), "{error:#}");

    assert!(sys.did("delete aaa-agent") && sys.did("right false"));
    assert!(sys.services.is_empty());
    assert_eq!(sys.path, before_path, "PATH는 원문 그대로 돌아간다");
    assert_eq!(std::fs::read(&layout.exe).unwrap(), b"old-binary", "기존 실행 파일을 되돌린다");
    assert_eq!(
        std::fs::read_to_string(layout.config.join("agent.toml")).unwrap(),
        "schema = 1\n# 내 설정\n[health]\nmin_free_gb = 5\n"
    );
    assert!(!layout.receipt().exists());
    assert!(!layout.home.join("aaa-work").exists(), "이번에 만든 빈 작업 폴더도 지운다");
    assert!(layout.config.is_dir(), "원래 있던 폴더는 남긴다");
    // host 등록과 장치 키는 남겨 다시 실행할 때 승인 없이 잇는다
    assert!(aaa_agent::identity::load_pin(&layout.state.join("agent")).is_some());
    assert!(ui.printed("되돌림: 서비스 aaa-agent"));
    assert!(ui.printed("되돌림: 「서비스로 로그온」 권한"));
    assert!(ui.out.iter().any(|(tone, line)| *tone == Tone::Warn && line.starts_with("남음: host 등록")));

    // 다시 실행: 등록은 건너뛰고 끝까지 간다
    sys.fail_start = false;
    let mut ui = Script::new(&DEFAULTS, &["correct horse"]);
    Setup::new(&mut sys, &mut ui, layout.clone(), "aaa 0.1.5".into()).install(&Args::default()).unwrap();
    assert_eq!(sys.enrolled_as, ["win-a"], "두 번째 실행은 다시 승인받지 않는다");
    assert!(ui.printed("이미 이 host에 등록돼 있습니다"));
    assert!(ui.printed("「win-a」로 등록(이미 등록됨, 승인 필요 없음)"));
    let toml = std::fs::read_to_string(layout.config.join("agent.toml")).unwrap();
    assert!(toml.contains("# 내 설정") && toml.contains("min_free_gb = 5"), "다른 설정은 남긴다: {toml}");
    let kept: Vec<_> = std::fs::read_dir(layout.exe.parent().unwrap().join(".aaa-old")).unwrap().flatten().collect();
    assert_eq!(kept.len(), 1);
    assert_eq!(std::fs::read(kept[0].path()).unwrap(), b"old-binary");
}

#[test]
fn cancelling_before_changes_leaves_nothing() {
    let lab = Lab::new();
    let layout = lab.layout();
    let mut sys = FakeSys::new(&layout);
    let mut ui = Script::new(&["studio.example.ts.net", "", "", "n"], &["correct horse"]);
    let error = Setup::new(&mut sys, &mut ui, layout.clone(), "v".into()).install(&Args::default()).unwrap_err();
    assert!(error.downcast_ref::<Cancelled>().is_some());
    assert_eq!(sys.ops, ["reach studio.example.ts.net:7430", "logon network"]);
    assert!(ui.printed("취소했습니다. 아무것도 바꾸지 않았습니다."));
    assert!(!layout.home.exists(), "파일을 하나도 만들지 않는다");

    // Ctrl+C도 같다
    let mut ui = Script::new(&["^C"], &[]);
    let error = Setup::new(&mut sys, &mut ui, layout.clone(), "v".into()).install(&Args::default()).unwrap_err();
    assert!(error.downcast_ref::<Cancelled>().is_some());
    assert!(!layout.home.exists());
}

#[test]
fn wrong_passwords_are_asked_again_and_three_misses_stop_before_changes() {
    let lab = Lab::new();
    let layout = lab.layout();
    let mut sys = FakeSys::new(&layout);
    let mut ui = Script::new(&DEFAULTS, &["nope", "", "correct horse"]);
    Setup::new(&mut sys, &mut ui, layout.clone(), "v".into()).install(&Args::default()).unwrap();
    assert!(ui.printed("암호가 맞지 않습니다. 다시 넣어 주세요(2번 남음)."));
    assert!(ui.printed("빈 암호로는 서비스를 띄울 수 없습니다"));

    let lab = Lab::new();
    let layout = lab.layout();
    let mut sys = FakeSys::new(&layout);
    let mut ui = Script::new(&DEFAULTS[..3], &["a", "b", "c"]);
    let error = Setup::new(&mut sys, &mut ui, layout.clone(), "v".into()).install(&Args::default()).unwrap_err();
    assert!(format!("{error:#}").contains("암호가 3번 맞지 않았습니다"));
    assert!(ui.printed("아무것도 바꾸지 않았습니다."));
    assert!(!sys.ops.iter().any(|o| o.starts_with("enroll")), "{:?}", sys.ops);
    assert!(!layout.home.exists());
}

#[test]
fn unreachable_host_is_asked_again_and_port_defaults_to_7430() {
    let lab = Lab::new();
    let layout = lab.layout();
    let mut sys = FakeSys::new(&layout);
    sys.unreachable.push("wrong.example:7430".into());
    let mut ui = Script::new(&["wrong.example", "https://studio.example.ts.net:7431/", "", "", ""], &["correct horse"]);
    Setup::new(&mut sys, &mut ui, layout.clone(), "v".into()).install(&Args::default()).unwrap();
    assert_eq!(sys.ops[..2], ["reach wrong.example:7430", "reach studio.example.ts.net:7431"]);
    assert!(ui.printed("wrong.example:7430에 연결하지 못했습니다"));
    assert_eq!(Receipt::read(&layout.receipt()).unwrap().host, "studio.example.ts.net:7431");
}

#[test]
fn closed_window_and_taken_name_are_fixed_in_the_same_run() {
    let lab = Lab::new();
    let layout = lab.layout();
    let mut sys = FakeSys::new(&layout);
    sys.enroll = [Some(ErrorCode::EnrollClosed), Some(ErrorCode::NameTaken), None].into();
    // 질문 4개 → 등록 창 Enter → 새 이름
    let mut ui = Script::new(&["studio.example.ts.net", "", "", "", "", "win-a-2"], &["correct horse"]);
    Setup::new(&mut sys, &mut ui, layout.clone(), "v".into()).install(&Args::default()).unwrap();
    assert!(ui.printed("host의 등록 창이 닫혀 있습니다."));
    assert!(ui.printed("이름 「win-a」는 host에서 다른 등록이 쓰고 있습니다."));
    assert_eq!(sys.enrolled_as, ["win-a-2"]);
    assert_eq!(Receipt::read(&layout.receipt()).unwrap().name, "win-a-2");
    let toml = std::fs::read_to_string(layout.config.join("agent.toml")).unwrap();
    assert!(toml.contains("name = \"win-a-2\""), "{toml}");
}

#[test]
fn enrollment_that_never_succeeds_removes_the_new_device_key() {
    let lab = Lab::new();
    let layout = lab.layout();
    let mut sys = FakeSys::new(&layout);
    sys.enroll = [Some(ErrorCode::UnsupportedVersion)].into();
    let mut ui = Script::new(&DEFAULTS, &["correct horse"]);
    Setup::new(&mut sys, &mut ui, layout.clone(), "v".into()).install(&Args::default()).unwrap_err();
    assert!(ui.printed("host와 판이 맞지 않습니다"));
    assert!(!layout.state.join("agent").exists(), "등록 전 만든 장치 키는 지운다");
    assert!(!layout.state.exists() && !layout.home.exists(), "그때 새로 생긴 위 폴더도 비면 지운다");
    assert!(ui.printed("되돌림: 새로 만든 장치 키(등록 전)"));
    assert!(!sys.ops.iter().any(|o| o.starts_with("create") || o.starts_with("right")));
}

#[test]
fn rerun_updates_the_binary_and_restarts_without_asking_again() {
    let lab = Lab::new();
    let layout = lab.layout();
    let mut sys = FakeSys::new(&layout);
    let mut ui = Script::new(&DEFAULTS, &["correct horse"]);
    Setup::new(&mut sys, &mut ui, layout.clone(), "aaa 0.1.5".into()).install(&Args::default()).unwrap();

    let mut ui = Script::new(&[], &[]);
    Setup::new(&mut sys, &mut ui, layout.clone(), "aaa 0.1.5".into()).install(&Args::default()).unwrap();
    assert!(ui.printed("같은 판이 이미 돌고 있습니다"));
    assert!(ui.asked.is_empty());

    std::fs::write(&layout.source, b"newer-binary").unwrap();
    sys.ops.clear();
    let mut ui = Script::new(&[""], &[]);
    Setup::new(&mut sys, &mut ui, layout.clone(), "aaa 0.1.6".into()).install(&Args::default()).unwrap();
    assert_eq!(ui.asked, ["실행 파일을 이 판으로 바꾸고 서비스를 다시 시작할까요? (Y/n)"]);
    assert_eq!(sys.ops, ["stop aaa-agent", "start aaa-agent"]);
    assert_eq!(std::fs::read(&layout.exe).unwrap(), b"newer-binary");
    assert_eq!(Receipt::read(&layout.receipt()).unwrap().version, "aaa 0.1.6");
}

#[test]
fn uninstall_reverts_only_what_the_receipt_records() {
    let lab = Lab::new();
    let layout = lab.layout();
    let mut sys = FakeSys::new(&layout);
    sys.right = Right::Granted;
    sys.path = Some(format!(r"C:\tools;{}", layout.exe.parent().unwrap().display()));
    let mut ui = Script::new(&DEFAULTS, &["correct horse"]);
    Setup::new(&mut sys, &mut ui, layout.clone(), "v".into()).install(&Args::default()).unwrap();
    let receipt = Receipt::read(&layout.receipt()).unwrap();
    assert!(!receipt.logon_right_added && receipt.path_added.is_none(), "이미 있던 권한·PATH는 기록하지 않는다");

    sys.ops.clear();
    let mut ui = Script::new(&[""], &[]);
    let args = Args { uninstall: true, ..Default::default() };
    Setup::new(&mut sys, &mut ui, layout.clone(), "v".into()).uninstall(&args).unwrap();
    assert_eq!(sys.ops, ["stop aaa-agent", "delete aaa-agent"], "넣지 않은 권한·PATH는 그대로 둔다");
    assert!(!layout.receipt().exists());
    assert!(layout.config.join("agent.toml").exists() && layout.state.join("agent/host.json").exists());
    assert!(ui.printed("host 웹 › 설정 › 머신에서 「win-a」를 폐기"));

    // 다시 설치 → 권한·PATH를 넣은 설치의 제거는 그것들도 뺀다
    sys.right = Right::Missing;
    sys.path = None;
    let mut ui = Script::new(&DEFAULTS, &["correct horse"]);
    Setup::new(&mut sys, &mut ui, layout.clone(), "v".into()).install(&Args::default()).unwrap();
    sys.ops.clear();
    let mut ui = Script::new(&[""], &[]);
    let args = Args { uninstall: true, purge: true, ..Default::default() };
    Setup::new(&mut sys, &mut ui, layout.clone(), "v".into()).uninstall(&args).unwrap();
    assert_eq!(sys.ops, ["stop aaa-agent", "delete aaa-agent", "right false", "path None"]);
    assert!(!layout.config.exists() && !layout.state.exists());
}

#[test]
fn a_service_this_setup_did_not_make_is_left_alone() {
    let lab = Lab::new();
    let layout = lab.layout();
    let mut sys = FakeSys::new(&layout);
    let foreign = ServiceInfo {
        name: "aaa-agent".into(),
        running: false,
        pid: 0,
        automatic: true,
        account: r".\person".into(),
        command: r#""C:\old\aaa.exe" service-run"#.into(),
    };
    sys.services.insert("aaa-agent".into(), foreign);
    let mut ui = Script::new(&[], &[]);
    Setup::new(&mut sys, &mut ui, layout.clone(), "v".into()).install(&Args::default()).unwrap_err();
    assert!(ui.printed("이 설치기가 만든 것이 아닙니다"));
    assert!(sys.ops.is_empty());
    let mut ui = Script::new(&[], &[]);
    let args = Args { uninstall: true, ..Default::default() };
    Setup::new(&mut sys, &mut ui, layout.clone(), "v".into()).uninstall(&args).unwrap_err();
    assert!(sys.ops.is_empty() && sys.services.contains_key("aaa-agent"));
}

#[test]
fn other_aaa_services_and_sibling_config_shape_the_defaults() {
    let lab = Lab::new();
    let layout = lab.layout();
    let sibling = layout.config.parent().unwrap().parent().unwrap().join("aaa-install-check/config");
    std::fs::create_dir_all(&sibling).unwrap();
    std::fs::write(
        sibling.join("agent.toml"),
        "schema = 1\n[agent]\nhost = \"studio.example.ts.net:7430\"\nname = \"win-a\"\n",
    )
    .unwrap();
    let mut sys = FakeSys::new(&layout);
    sys.services.insert(
        "aaa-install-check-agent".into(),
        ServiceInfo {
            name: "aaa-install-check-agent".into(),
            running: false,
            pid: 0,
            automatic: true,
            account: r".\person".into(),
            command: String::new(),
        },
    );
    let mut ui = Script::new(&["", "", "", ""], &["correct horse"]);
    Setup::new(&mut sys, &mut ui, layout.clone(), "v".into()).install(&Args::default()).unwrap();
    assert_eq!(sys.ops[0], "reach studio.example.ts.net:7430", "다른 인스턴스의 host를 기본값으로 쓴다");
    assert!(ui.printed("다른 aaa 서비스 aaa-install-check-agent(정지)"));
    assert!(!sys.ops.iter().any(|o| o.contains("install-check")), "다른 서비스는 건드리지 않는다");
}

#[test]
fn instance_layout_names_the_service_and_passes_explicit_dirs() {
    let layout = Layout {
        instance: Some("aaa-test-0878-a".into()),
        source: PathBuf::from(r"C:\aaa-test\aaa-setup.exe"),
        exe: PathBuf::from(r"C:\Users\Test User\AppData\Local\aaa-aaa-test-0878-a\bin\aaa.exe"),
        config: PathBuf::from(r"C:\x\config"),
        state: PathBuf::from(r"C:\x\state"),
        explicit_dirs: true,
        home: PathBuf::from(r"C:\Users\Test User"),
    };
    assert_eq!(layout.service(), "aaa-test-0878-a-agent");
    assert_eq!(Layout { instance: Some("work".into()), ..layout.clone() }.service(), "aaa-work-agent");
    assert_eq!(
        layout.command(),
        r#""C:\Users\Test User\AppData\Local\aaa-aaa-test-0878-a\bin\aaa.exe" --instance aaa-test-0878-a --config-dir C:\x\config --state-dir C:\x\state setup-service"#
    );
}

#[test]
fn plan_columns_line_up_with_korean_labels() {
    let lab = Lab::new();
    let layout = lab.layout();
    let mut sys = FakeSys::new(&layout);
    let mut ui = Script::new(&["studio.example.ts.net", "", "", "n"], &["correct horse"]);
    let _ = Setup::new(&mut sys, &mut ui, layout.clone(), "v".into()).install(&Args::default());
    let columns = |text: &str| text.chars().map(|c| if ('가'..='힣').contains(&c) { 2 } else { 1 }).sum::<usize>();
    let exe = layout.exe.display().to_string();
    for (label, value) in [("host", "studio.example.ts.net"), ("실행 파일", exe.as_str()), ("서비스", "aaa-agent ·")]
    {
        let row = ui.out.iter().map(|(_, l)| l).find(|l| l.starts_with(&format!("  {label} "))).expect(label);
        let at = row.find(value).expect(value);
        assert_eq!(columns(&row[..at]), 13, "값은 같은 칸에서 시작한다: {row:?}");
    }
}

#[test]
fn host_addresses_are_normalized() {
    for (raw, want) in [
        ("studio.example.ts.net", Some("studio.example.ts.net:7430")),
        ("  studio.example.ts.net:7430 ", Some("studio.example.ts.net:7430")),
        ("https://studio.example.ts.net:8443/", Some("studio.example.ts.net:8443")),
        ("192.0.2.10", Some("192.0.2.10:7430")),
        ("[2001:db8::1]:7430", Some("[2001:db8::1]:7430")),
        ("2001:db8::1", Some("[2001:db8::1]:7430")),
        ("studio:0", None),
        ("studio:http", None),
        ("", None),
        ("a b", None),
        ("studio/path", None),
    ] {
        assert_eq!(normalize_host(raw).as_deref(), want, "{raw}");
    }
}

#[test]
fn setup_named_binary_runs_the_wizard_unless_another_command_is_given() {
    use std::ffi::OsString;
    let is_command = |name: &str| matches!(name, "setup" | "agent" | "status");
    let implied = |args: &[&str]| super::implied(&args.iter().map(OsString::from).collect::<Vec<_>>(), is_command);
    assert!(implied(&[r"C:\Downloads\aaa-setup.exe"]));
    assert!(implied(&["AAA-SETUP (1).exe"]));
    assert!(implied(&["aaa-setup.exe", "--uninstall"]));
    assert!(implied(&["aaa-setup.exe", "--instance", "work"]));
    assert!(!implied(&["aaa-setup.exe", "status"]));
    assert!(!implied(&["aaa-setup.exe", "--version"]));
    assert!(!implied(&[r"C:\bin\aaa.exe"]));
    assert!(!implied(&["aaa.exe", "--uninstall"]));
}

#[test]
fn locked_account_stops_with_the_reason_before_changes() {
    let lab = Lab::new();
    let layout = lab.layout();
    let mut sys = FakeSys::new(&layout);
    let mut ui = Script::new(&DEFAULTS[..3], &["locked"]);
    let error = Setup::new(&mut sys, &mut ui, layout.clone(), "v".into()).install(&Args::default()).unwrap_err();
    assert!(format!("{error:#}").contains("계정이 잠겼습니다"), "{error:#}");
    assert!(!layout.home.exists());
}

#[test]
fn status_shows_service_and_connection() {
    let lab = Lab::new();
    let layout = lab.layout();
    let mut sys = FakeSys::new(&layout);
    let mut ui = Script::new(&[], &[]);
    Setup::new(&mut sys, &mut ui, layout.clone(), "v".into()).status().unwrap();
    assert!(ui.printed("등록돼 있지 않습니다"));

    let mut ui = Script::new(&DEFAULTS, &["correct horse"]);
    Setup::new(&mut sys, &mut ui, layout.clone(), "aaa 0.1.5".into()).install(&Args::default()).unwrap();
    let mut ui = Script::new(&[], &[]);
    Setup::new(&mut sys, &mut ui, layout.clone(), "v".into()).status().unwrap();
    assert!(ui.printed("실행 중 · 자동 시작 · 계정 .\\person"));
    assert!(ui.printed("host studio.example.ts.net:7430 · 이름 「win-a」 · 설치 판 aaa 0.1.5"));
    assert!(ui.printed("host에 연결됨"));
}

#[test]
fn uninstall_keeps_the_logon_right_while_another_service_uses_the_account() {
    let lab = Lab::new();
    let layout = lab.layout();
    let mut sys = FakeSys::new(&layout);
    let mut ui = Script::new(&DEFAULTS, &["correct horse"]);
    Setup::new(&mut sys, &mut ui, layout.clone(), "v".into()).install(&Args::default()).unwrap();
    assert!(Receipt::read(&layout.receipt()).unwrap().logon_right_added);
    let other =
        ServiceInfo { name: "aaa-work-agent".into(), account: account().short(), ..sys.services["aaa-agent"].clone() };
    sys.services.insert(other.name.clone(), other);
    sys.ops.clear();
    let mut ui = Script::new(&[""], &[]);
    Setup::new(&mut sys, &mut ui, layout.clone(), "v".into())
        .uninstall(&Args { uninstall: true, ..Default::default() })
        .unwrap();
    assert!(sys.did("delete aaa-agent") && !sys.did("right false"), "권한을 빼지 않는다: {:?}", sys.ops);
    assert_eq!(sys.right, Right::Granted);
    assert!(ui.printed("권한은 남깁니다: aaa-work-agent가 같은 계정으로 돕니다"));
}

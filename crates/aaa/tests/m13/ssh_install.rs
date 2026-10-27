//! 「SSH로 추가」 설치·등록의 실제 경로(plan §8.420): root가 띄운 시스템 `/usr/sbin/sshd`(install 검증 SSH 입구가 믿는 계보)로 host가
//! aaa와 launcher package를 보내고, 원격 install이 계획 → host 서명 승인 → bootstrap → grant 등록 → LaunchAgent 시작을 한 뒤 agent가
//! host에 붙고 끝 확인 pane이 줄을 낸다.
//!
//! sshd·키 준비는 `scripts/ci-root-sshd.sh`(macos-15 CI 일회성 runner)가 하고 그 폴더를 `AAA_ROOT_SSHD_LAB`으로 준다. 원격 HOME은 sshd
//! `SetEnv HOME=`의 실험실 폴더다: 설치 자리·LaunchAgent plist·launcher가 그 아래에 놓이고 runner의 실제 HOME에는 쓰지 않는다(이 시험이
//! 확인한다). gui domain 등록(`dev.aaa.agent`)은 계정 전역이라 적용 전에 그 label이 없어야 하고, 끝나면 bootout한다.
#![cfg(target_os = "macos")]
use super::*;
use aaa_common::child_process::StdCommandExt;
use aaa_common::front::FrontClient;
use aaa_proto::install::InstallLogin;
use aaa_proto::{FrontRequest, FrontResponse, MachineSetupPhase, MachineSetupSettings, MachineSetupView};

/// role agent·기본 instance의 LaunchAgent label(`crate::launchd::label_in`)
const LABEL: &str = "dev.aaa.agent";
/// 원격 적용(설치·LaunchAgent 시작·agent 등록)의 상한
const APPLY: Duration = Duration::from_secs(15 * 60);

/// 시험이 등록한 label만 끝에 내린다
struct Booted(String);
impl Drop for Booted {
    fn drop(&mut self) {
        let _ = Command::new("/bin/launchctl").args(["bootout", &self.0]).capture_output();
    }
}

fn gui_target() -> String {
    let uid = Command::new("id").arg("-u").capture_output().unwrap();
    format!("gui/{}/{LABEL}", String::from_utf8_lossy(&uid.stdout).trim())
}

/// host가 실행할 aaa 사본과 그 옆 launcher package(같은 판·같은 SHA에 묶음, I3 시험과 같은 만들기)
fn factory(lab: &Lab) -> PathBuf {
    let dir = lab.root.join("factory");
    std::fs::create_dir_all(&dir).unwrap();
    let exe = dir.join("aaa");
    std::fs::copy(lab::BIN, &exe).unwrap();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let built = Command::new("bash")
        .arg(repo.join("p0/launcher/install.sh"))
        .arg("--dest")
        .arg(dir.join("bundle"))
        .capture_output()
        .unwrap();
    assert!(built.status.success(), "launcher build: {}", String::from_utf8_lossy(&built.stderr));
    let version = String::from_utf8(Command::new(&exe).arg("--version").capture_output().unwrap().stdout).unwrap();
    let version = version.trim().strip_prefix("aaa ").unwrap().to_string();
    let platform = format!("macos-{}", std::env::consts::ARCH);
    let packed = Command::new("python3")
        .arg(repo.join("scripts/launcher_asset.py"))
        .arg("--app")
        .arg(dir.join("bundle/aaa-launcher.app"))
        .arg("--binary")
        .arg(&exe)
        .args(["--platform", &platform, "--version", &version, "--output"])
        .arg(dir.join(format!("aaa-launcher-{platform}.json")))
        .capture_output()
        .unwrap();
    assert!(packed.status.success(), "launcher pack: {}", String::from_utf8_lossy(&packed.stderr));
    exe
}

async fn call(c: &mut FrontClient, req: FrontRequest) -> FrontResponse {
    let what = format!("{req:?}");
    match c.call(req).await.unwrap() {
        Ok(r) => r,
        Err(e) => panic!("{what}: {}", e.message),
    }
}

async fn view(c: &mut FrontClient, job: &str) -> MachineSetupView {
    match call(c, FrontRequest::MachineSetupList).await {
        FrontResponse::MachineSetups { setups } => setups.into_iter().find(|s| s.job == job).expect("작업"),
        other => panic!("{other:?}"),
    }
}

/// 작업이 `until`을 만족할 때까지 목록을 다시 읽는다(진행은 host 작업 단계뿐이라 짧은 간격으로 읽는다)
async fn wait(
    c: &mut FrontClient,
    job: &str,
    what: &str,
    timeout: Duration,
    until: impl Fn(&MachineSetupView) -> bool,
) -> MachineSetupView {
    let end = Instant::now() + timeout;
    loop {
        let v = view(c, job).await;
        if until(&v) {
            return v;
        }
        assert!(Instant::now() < end, "{what}: 시간 안에 끝나지 않았다: {v:?}");
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// 원격 install의 서비스 로그(실패 진단)
fn service_logs(home: &Path) -> String {
    let jobs = home.join("Library/Application Support/aaa/install/jobs");
    let mut out = String::new();
    for job in std::fs::read_dir(jobs).into_iter().flatten().flatten() {
        if let Ok(t) = std::fs::read_to_string(job.path().join("services/agent/service.log")) {
            out.push_str(
                &t.lines().rev().take(30).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n"),
            );
        }
    }
    out
}

/// runner 실제 HOME 아래 이 설치가 쓸 수 있는 자리(전후가 같아야 한다)
fn real_home_marks(real: &Path) -> Vec<(PathBuf, bool)> {
    [
        format!("Library/LaunchAgents/{LABEL}.plist"),
        ".local/bin/aaa".into(),
        "Applications/aaa-launcher.app".into(),
        "Library/Application Support/aaa".into(),
    ]
    .into_iter()
    .map(|p| {
        let p = real.join(p);
        let e = p.exists();
        (p, e)
    })
    .collect()
}

#[test]
#[ignore = "CI(macos-15) 전용: scripts/ci-root-sshd.sh가 root sshd를 띄우고 AAA_ROOT_SSHD_LAB을 준다"]
fn ssh_install_through_the_system_sshd_starts_a_launch_agent_that_connects_and_checks() {
    let ci = PathBuf::from(std::env::var_os("AAA_ROOT_SSHD_LAB").expect("AAA_ROOT_SSHD_LAB(scripts/ci-root-sshd.sh)"));
    assert!(std::env::var_os("AAA_SESSION_ID").is_none(), "aaa pane 밖에서 돌린다: 사람 요청이 세션 안으로 거부된다");
    let home = ci.join("h").canonicalize().unwrap();
    let real = PathBuf::from(std::env::var_os("HOME").unwrap());
    assert_ne!(real, home);
    let before = real_home_marks(&real);

    let mut lab = Lab::new();
    let exe = factory(&lab);
    let h = lab.node(
        "h",
        Some(&format!(
            "schema = 1\n[host]\nagent_listen = \"127.0.0.1:0\"\nssh_config = {:?}\n[web]\nlisten = \"\"\n\
             [local_agent]\nmode = \"none\"\n",
            path_str(&ci.join("ssh_config"))
        )),
    );
    lab.spawn_command("host", h.cmd_bin(&exe, &["host"]));
    let st = wait_for("host 준비", T, || h.json(&["status"]).ok());
    let addr = st["agent_listen"].as_str().unwrap().to_string();

    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let mut c = FrontClient::connect_as(&h.state, "ssh-install").await.unwrap();
        // 접속 → 처음 보는 host key → 그 지문을 믿음 → 감지(원격 HOME은 실험실)
        let job = match call(&mut c, FrontRequest::MachineSetupStart { target: "ci".into() }).await {
            FrontResponse::MachineSetup { setup } => setup.job,
            other => panic!("{other:?}"),
        };
        let v = wait(&mut c, &job, "접속", T, |v| v.phase != MachineSetupPhase::Connecting).await;
        assert_eq!(v.phase, MachineSetupPhase::Trust, "{:?}", v.error);
        let fp = Command::new("ssh-keygen").arg("-lf").arg(ci.join("hostkey.pub")).capture_output().unwrap();
        let fp = String::from_utf8_lossy(&fp.stdout).split_whitespace().nth(1).unwrap().to_string();
        assert_eq!(v.host_keys.iter().map(|k| k.sha256.as_str()).collect::<Vec<_>>(), vec![fp.as_str()]);
        call(&mut c, FrontRequest::MachineSetupTrust { job: job.clone(), host_key_sha256: fp }).await;
        let v = wait(&mut c, &job, "감지", T, |v| v.phase != MachineSetupPhase::Detecting).await;
        assert_eq!(v.phase, MachineSetupPhase::Ready, "{:?}", v.error);
        let d = v.detected.unwrap();
        assert_eq!(d.os, "macos");
        assert_eq!(
            d.home.as_deref().map(|h| h.trim_end_matches('/')),
            Some(home.to_str().unwrap()),
            "원격 HOME은 실험실"
        );

        // 계획: aaa·launcher 전송 → 원격 install 계획(검증 SSH 입구가 이 root sshd를 믿는다)
        let work = home.join("aaa-work");
        let settings = MachineSetupSettings {
            name: "ci-mac".into(),
            host: addr.clone(),
            work_root: work.to_string_lossy().into(),
            work_mount: None,
            harness: vec![],
            login: InstallLogin::Defer,
        };
        call(&mut c, FrontRequest::MachineSetupPlan { job: job.clone(), settings }).await;
        let v = wait(&mut c, &job, "계획", Duration::from_secs(300), |v| v.phase != MachineSetupPhase::Planning).await;
        assert_eq!(v.phase, MachineSetupPhase::Review, "{:?} {:?}", v.error, v.review);
        let review = v.review.unwrap();
        eprintln!("승인 범위: {:?}, 바꿀 것: {:?}", review.operations, review.changes);
        assert!(review.operations.iter().any(|o| o == "enroll_agent"), "{:?}", review.operations);

        // 적용: 계정 전역 label이 비어 있을 때만 등록한다(운영 계정 보호), 끝나면 내린다
        let target = gui_target();
        let taken = Command::new("/bin/launchctl").args(["print", &target]).capture_output().unwrap().status.success();
        assert!(!taken, "{target}가 이미 있다: 이 계정에서 돌리지 않는다");
        let _booted = Booted(target);
        call(
            &mut c,
            FrontRequest::MachineSetupApply {
                job: job.clone(),
                plan_sha256: review.plan_sha256.clone(),
                service_password: None,
            },
        )
        .await;
        let v = wait(&mut c, &job, "적용", APPLY, |v| v.phase != MachineSetupPhase::Installing).await;
        assert_eq!(
            v.phase,
            MachineSetupPhase::Installed,
            "{:?} {:?}\nservice.log:\n{}",
            v.error,
            v.install,
            service_logs(&home)
        );
        assert_eq!(v.install.as_ref().and_then(|i| i.machine.as_deref()), Some("ci-mac"));
        wait_for("ci-mac 연결", T, || h.connected("ci-mac").then_some(()));

        // 끝 확인: 그 머신 작업 root의 shell pane이 고정 스크립트 줄을 낸다
        call(&mut c, FrontRequest::MachineSetupCheck { job: job.clone() }).await;
        let v = wait(&mut c, &job, "끝 확인", Duration::from_secs(90), |v| {
            v.install.as_ref().is_some_and(|i| !i.checks.is_empty())
        })
        .await;
        let checks = v.install.unwrap().checks;
        assert!(checks.iter().any(|l| l.starts_with("작업 폴더 ") && l.ends_with("/aaa-work")), "{checks:?}");
        assert!(checks.iter().any(|l| l.starts_with("PATH ")), "{checks:?}");
    });

    // 설치 자리는 실험실 HOME 아래이고 runner 실제 HOME은 그대로다
    assert!(home.join(format!("Library/LaunchAgents/{LABEL}.plist")).is_file());
    assert!(home.join(".local/bin/aaa").is_file());
    assert_eq!(real_home_marks(&real), before, "runner 실제 HOME에 쓰지 않았다");
}

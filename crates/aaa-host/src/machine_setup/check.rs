//! 설치·등록된 머신의 끝 확인과 harness 로그인(웹 머신 추가 M3, 계획 t-0471 §1.8a·Q5·Q11). 둘 다 그 머신의 agent에 pane을 연다:
//!
//! - **끝 확인**: shell pane 하나(모델 비용 없음)가 작업 root에서 고정 스크립트로 작업 폴더·git·harness 실행 파일·PATH를 적는다. host가 그
//!   출력의 표지 사이를 읽어 작업에 남긴다. pane은 사람이 볼 수 있게 남겨 둔다.
//! - **harness 로그인**: 그 머신에 `claude` 또는 `codex login` pane을 연다. OAuth는 사람이 그 pane에서 끝낸다(aaa CLI 입력이 아니다).
use std::sync::Arc;

use aaa_proto::install::InstallHarness;
use aaa_proto::{
    DeviceRole, ErrorBody, ErrorCode, FrontResponse, MachineSetupPhase, MachineSetupView, OutputMatch, ReadSource,
    SpawnSpec,
};

use crate::Host;

pub(crate) const CHECK: &str = "AAA-CHECK-1";
pub(crate) const CHECK_END: &str = "AAA-CHECK-END";
/// 끝 확인 출력을 기다리는 시한(셸 시작·PATH 읽기뿐)
const CHECK_TIMEOUT_MS: u64 = 60_000;

/// 끝 확인 pane이 돌릴 고정 스크립트(agent가 argv로 띄우므로 셸 인용은 이 글 안에만 있다)
pub(crate) fn check_argv() -> Vec<String> {
    let body = [
        format!("echo {CHECK}"),
        r#"printf "작업 폴더 %s\n" "$(pwd)""#.to_string(),
        r#"if command -v git >/dev/null 2>&1; then printf "%s\n" "$(git --version)"; else echo "git 없음"; fi"#.into(),
        r#"for h in claude codex; do p=$(command -v "$h" 2>/dev/null); if [ -n "$p" ]; then printf "%s %s\n" "$h" "$p"; else printf "%s 없음\n" "$h"; fi; done"#.into(),
        r#"printf "PATH %s\n" "$PATH""#.into(),
        format!("echo {CHECK_END}"),
    ]
    .join("; ");
    vec!["sh".into(), "-c".into(), body]
}

pub(crate) fn windows_check_argv() -> Vec<String> {
    let body = format!(
        r#"
Write-Output '{CHECK}'
Write-Output ('작업 폴더 ' + (Get-Location).Path)
if (Get-Command git -ErrorAction SilentlyContinue) {{ git --version }} else {{ Write-Output 'git 없음' }}
foreach ($h in @('claude','codex')) {{ $c=Get-Command $h -ErrorAction SilentlyContinue; if ($c) {{ Write-Output ($h + ' ' + $c.Source) }} else {{ Write-Output ($h + ' 없음') }} }}
Write-Output ('PATH ' + $env:PATH)
Write-Output '{CHECK_END}'
"#
    );
    use base64::Engine as _;
    vec![
        "powershell.exe".into(),
        "-NoProfile".into(),
        "-EncodedCommand".into(),
        base64::engine::general_purpose::STANDARD
            .encode(body.encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<_>>()),
    ]
}

/// 끝 확인 출력에서 표지 사이 줄
pub(crate) fn check_lines(text: &str) -> Option<Vec<String>> {
    let mut lines = text.lines().map(str::trim_end);
    lines.find(|l| *l == CHECK)?;
    let body: Vec<String> = lines.by_ref().take_while(|l| *l != CHECK_END).map(str::to_string).collect();
    (!body.is_empty()).then_some(body)
}

/// harness 로그인 pane의 명령
pub(crate) fn login_argv(h: InstallHarness) -> Option<Vec<String>> {
    match h {
        InstallHarness::Claude => Some(vec!["claude".into()]),
        InstallHarness::Codex => Some(vec!["codex".into(), "login".into()]),
        InstallHarness::None => None,
    }
}

fn conflict(msg: &str) -> ErrorBody {
    ErrorBody::new(ErrorCode::Conflict, msg)
}

impl Host {
    /// 설치·등록된 작업의 머신 ID와 작업 root
    fn setup_installed(&self, id: &str) -> Result<(MachineSetupView, String, Option<String>), ErrorBody> {
        let job = self.setup_load(id)?;
        if job.view.phase != MachineSetupPhase::Installed {
            return Err(conflict("설치·등록이 끝난 작업에서만 연다"));
        }
        let name = job
            .view
            .install
            .as_ref()
            .and_then(|i| i.machine.clone())
            .ok_or_else(|| conflict("등록된 머신 이름이 없다"))?;
        let mid = self.resolve_machine(Some(&name))?;
        if self.conn_of(&mid).is_none() {
            return Err(ErrorBody::new(
                ErrorCode::AgentDisconnected,
                format!("{name}의 agent가 아직 연결되지 않았다(그 머신의 감독자가 agent를 띄우면 다시 누른다)"),
            ));
        }
        let root = job.view.settings.as_ref().map(|s| s.work_root.clone());
        Ok((job.view, mid, root))
    }

    pub(crate) async fn setup_check(self: &Arc<Self>, id: &str, actor: &str) -> Result<MachineSetupView, ErrorBody> {
        let (view, mid, root) = self.setup_installed(id)?;
        let spec = SpawnSpec {
            name: "aaa 끝 확인".into(),
            argv: if view.detected.as_ref().is_some_and(|d| d.os == "windows") {
                windows_check_argv()
            } else {
                check_argv()
            },
            cwd: root,
            env: vec![],
            cols: 160,
            rows: 40,
            role: None,
            harness: None,
            profile: None,
            resolved_profile: None,
        };
        let pane = self.spawn_on(&mid, spec, actor).await?;
        self.db.audit(actor, "machine_setup_check", &format!("{id} {pane}"));
        let p = pane.clone();
        let job = self.setup_update(id, MachineSetupPhase::Installed, move |j| {
            if let Some(i) = j.view.install.as_mut() {
                i.check_pane = Some(p);
                i.checks.clear();
            }
        });
        let host = self.clone();
        let id2 = id.to_string();
        tokio::spawn(async move {
            let got = host
                .pane_wait_output(
                    &pane,
                    OutputMatch::Substring { value: CHECK_END.into() },
                    ReadSource::RecentUnwrapped,
                    Some(200),
                    Some(CHECK_TIMEOUT_MS),
                )
                .await;
            let lines = match got {
                Ok(FrontResponse::OutputMatched { read, .. }) => {
                    check_lines(&read.text).unwrap_or_else(|| vec!["끝 확인 출력을 읽지 못했다".into()])
                }
                Ok(_) => vec!["끝 확인 출력을 읽지 못했다".into()],
                Err(e) => vec![format!("끝 확인: {}", e.message)],
            };
            host.setup_update(&id2, MachineSetupPhase::Installed, |j| {
                if let Some(i) = j.view.install.as_mut() {
                    i.checks = lines;
                }
            });
        });
        job.map(|j| j.view).ok_or_else(|| conflict("작업이 그사이 바뀌었다"))
    }

    pub(crate) async fn setup_login(
        self: &Arc<Self>,
        role: DeviceRole,
        id: &str,
        harness: InstallHarness,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        let (_, mid, root) = self.setup_installed(id)?;
        let argv =
            login_argv(harness).ok_or_else(|| ErrorBody::new(ErrorCode::BadRequest, "claude·codex만 로그인한다"))?;
        let name = format!("{} 로그인", argv[0]);
        let spec = SpawnSpec {
            name,
            argv,
            cwd: root,
            env: vec![],
            cols: 120,
            rows: 32,
            role: None,
            harness: None,
            profile: None,
            resolved_profile: None,
        };
        let pane = self.spawn_on(&mid, spec, actor).await?;
        self.db.audit(actor, "machine_setup_login", &format!("{id} {pane}"));
        self.pane_view_of(&pane, role)
            .map(FrontResponse::Pane)
            .ok_or_else(|| ErrorBody::new(ErrorCode::Internal, "pane 목록에 없다"))
    }
}

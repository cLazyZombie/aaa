//! pane·tab·workspace 명령.

use super::*;

#[derive(Subcommand)]
pub(super) enum PaneCmd {
    /// pane 목록(기본은 평가자 세션을 뺀다)
    List {
        /// 이 workspace의 layout에 있는 pane만
        #[arg(long)]
        workspace: Option<String>,
        /// 이 역할(coordinator·worker·evaluator)의 세션만
        #[arg(long)]
        role: Option<String>,
    },
    /// pane 하나(ID·ID 앞부분·이름)
    Get { pane: String },
    /// 이 명령을 실행한 aaa 세션의 pane(`AAA_SESSION_ID`). `--pane`으로 다른 pane을 줄 수 있다
    Current {
        #[arg(long, conflicts_with = "current")]
        pane: Option<String>,
        #[arg(long)]
        current: bool,
    },
    /// 글로 읽는다. 기본은 기록과 화면의 마지막 80줄(--source recent). detection은 상태 감지가 읽는 글이다.
    /// 출력은 글 그대로(--json이면 JSON)
    Read {
        pane: String,
        #[arg(long, value_parser = ["visible", "recent", "recent-unwrapped", "detection"], default_value = "recent")]
        source: String,
        /// 줄 수(최대 1000)
        #[arg(long)]
        lines: Option<u32>,
        #[arg(long, value_parser = ["text", "ansi"], default_value = "text")]
        format: String,
        /// --format ansi와 같다
        #[arg(long)]
        ansi: bool,
        /// --format ansi와 같다(herdr `--raw`)
        #[arg(long)]
        raw: bool,
    },
    /// pane 자리를 나눠 새 pane을 띄운다(명령이 없으면 그 머신 사용자의 기본 셸)
    Split {
        /// 나눌 pane(없으면 --pane·--current)
        target: Option<String>,
        #[arg(long, conflicts_with_all = ["target", "current"])]
        pane: Option<String>,
        #[arg(long, conflicts_with = "target")]
        current: bool,
        /// right(좌우) 또는 down(위아래)
        #[arg(long, value_parser = ["right", "down"])]
        direction: String,
        /// 원래 pane 쪽 비율(0.1~0.9, 기본 0.5)
        #[arg(long)]
        ratio: Option<f32>,
        #[arg(long)]
        cwd: Option<String>,
        /// 새 pane을 띄울 머신(기본: 나눌 pane의 머신)
        #[arg(long)]
        machine: Option<String>,
        /// 이 workspace의 layout에서 나눈다(기본: 그 pane이 든 workspace)
        #[arg(long, short)]
        workspace: Option<String>,
        #[arg(last = true)]
        argv: Vec<String>,
    },
    /// pane을 지금 자리에서 빼 새 탭에 둔다
    Move {
        pane: String,
        #[arg(long, required = true)]
        new_tab: bool,
        /// 이 workspace의 새 탭으로(기본: 그 pane이 든 workspace)
        #[arg(long, short)]
        workspace: Option<String>,
        #[arg(long)]
        label: Option<String>,
    },
    /// pane을 닫는다: 세션을 끝내고 layout에서 뺀다
    Close { pane: String },
    /// pane 가장자리를 그 방향으로 옮긴다(나뉜 자리 비율)
    Resize {
        #[arg(long, conflicts_with = "current")]
        pane: Option<String>,
        #[arg(long)]
        current: bool,
        #[arg(long, value_parser = ["left", "right", "up", "down"])]
        direction: String,
        /// 옮길 만큼(0~0.5, 기본 0.05)
        #[arg(long)]
        amount: Option<f32>,
    },
    /// 글을 그대로 보낸다(Enter 없음, 여러 인자는 빈칸으로 잇는다)
    SendText {
        pane: String,
        #[arg(required = true, allow_hyphen_values = true, trailing_var_arg = true)]
        text: Vec<String>,
    },
    /// 키를 보낸다: enter·esc·tab·shift+tab·backspace·space·up·down·left·right·ctrl+c·alt+x·f1~f20·글자 하나
    SendKeys {
        pane: String,
        #[arg(required = true, allow_hyphen_values = true, trailing_var_arg = true)]
        keys: Vec<String>,
    },
    /// 글 + Enter(명령 실행). 프로그램이 bracketed paste를 켰으면 글을 감싸 보낸다
    Run {
        pane: String,
        #[arg(required = true, allow_hyphen_values = true, trailing_var_arg = true)]
        command: Vec<String>,
    },
    /// 출력에 글이 나올 때까지 기다린다(--match 글 또는 --regex 정규식). 맞은 줄을 JSON으로 보이고, 시한이 지나면 실패한다
    WaitOutput {
        pane: String,
        #[arg(long = "match", conflicts_with = "regex", required_unless_present = "regex")]
        substring: Option<String>,
        #[arg(long)]
        regex: Option<String>,
        #[arg(long, value_parser = ["visible", "recent", "recent-unwrapped"], default_value = "recent")]
        source: String,
        #[arg(long)]
        lines: Option<u32>,
        /// 밀리초(없으면 끝없이 기다린다)
        #[arg(long)]
        timeout: Option<u64>,
    },
    /// Claude·Codex 대화 기록의 끝부분(읽기 전용, 도구 결과 본문은 빼고, 생각은 --thinking일 때만). 출력은 글(--json이면 JSON)
    Chat {
        pane: String,
        /// 생각(Claude thinking, Codex reasoning 요약)도 싣는다
        #[arg(long)]
        thinking: bool,
    },
    /// 그림 파일(PNG·JPEG·GIF·WebP)을 pane 머신의 세션 폴더에 올린다. harness 입력창이면 경로를 넣는다(Enter 없음).
    /// raw ssh 셸에서 그림 파일을 pane에 첨부한다.
    Attach { pane: String, file: std::path::PathBuf },
    /// 세션만 끝낸다(layout 자리와 기록은 남아 `pane resume`으로 다시 띄울 수 있다)
    Kill { pane: String },
    /// 끝난(중단·유실·종료) pane을 같은 spec으로 다시 띄운다(layout 자리도 바꾼다)
    Resume { pane: String },
    /// 끝난 pane 기록을 지운다
    Forget { pane: String },
    /// pane에 이름을 붙인다(herdr `pane rename <pane> <label>|--clear`). 낱말 여럿은 띄어 잇고, --clear는 띄울 때 이름으로 돌린다
    Rename {
        pane: String,
        #[arg(num_args = 1.., required_unless_present = "clear", conflicts_with = "clear")]
        label: Vec<String>,
        #[arg(long)]
        clear: bool,
    },
    /// (시험·진단) PTY 크기를 바꾼다. 쓰는 화면(lease)이 없을 때만 된다. 보통은 붙은 화면이 정한다
    #[command(hide = true)]
    PtySize { pane: String, cols: u16, rows: u16 },
}

#[derive(Subcommand)]
pub(super) enum TabCmd {
    /// workspace의 탭(기본 main)
    List {
        #[arg(long, short)]
        workspace: Option<String>,
    },
    /// 새 탭에 새 pane을 띄운다(명령이 없으면 그 머신 사용자의 기본 셸)
    Create {
        #[arg(long, short)]
        workspace: Option<String>,
        /// 띄울 머신(기본: host 머신)
        #[arg(long)]
        machine: Option<String>,
        /// 탭(pane) 이름. pane을 이 이름으로도 부를 수 있다
        #[arg(long)]
        label: Option<String>,
        #[arg(long)]
        cwd: Option<String>,
        /// 화면이 붙기 전 처음 PTY 폭(기본 120). 붙은 화면이 있으면 그 화면이 크기를 정한다
        #[arg(long)]
        cols: Option<u16>,
        /// 처음 PTY 높이(기본 32)
        #[arg(long)]
        rows: Option<u16>,
        #[arg(last = true)]
        argv: Vec<String>,
    },
    /// 탭 하나와 그 workspace
    Get {
        tab: String,
    },
    Rename {
        tab: String,
        label: String,
    },
    /// 탭을 닫는다: 그 안 pane을 끝낸다(thread·coordinator pane은 layout에서만 뺀다)
    Close {
        tab: String,
    },
}

#[derive(Subcommand)]
pub(super) enum WorkspaceCmd {
    List,
    /// workspace의 탭·나누기(기본 main)
    Get {
        name: Option<String>,
    },
    /// 새 workspace를 만들고 첫 탭에 새 pane을 띄운다
    Create {
        #[arg(long)]
        label: String,
        #[arg(long)]
        machine: Option<String>,
        #[arg(long)]
        cwd: Option<String>,
        #[arg(last = true)]
        argv: Vec<String>,
    },
    Rename {
        name: String,
        label: String,
    },
    /// workspace를 닫는다: 그 안 pane을 끝내고 layout을 지운다(thread·coordinator pane은 layout에서만 뺀다)
    Close {
        name: String,
    },
}

/// herdr `parse_read_source`
pub(super) fn read_source(s: &str) -> aaa_proto::ReadSource {
    match s {
        "visible" => aaa_proto::ReadSource::Visible,
        "recent-unwrapped" => aaa_proto::ReadSource::RecentUnwrapped,
        "detection" => aaa_proto::ReadSource::Detection,
        _ => aaa_proto::ReadSource::Recent,
    }
}

/// `--pane ID|--current`: 둘 다 없으면 이 aaa 세션(herdr `parse_optional_current_pane_args`, `HERDR_PANE_ID` 대신 `AAA_SESSION_ID`)
pub(super) fn current_pane(pane: Option<String>) -> Result<String> {
    match pane {
        Some(p) => Ok(p),
        None => std::env::var("AAA_SESSION_ID")
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()
            .filter(|s| !s.is_empty())
            .context("--current는 aaa 세션 안에서만 쓴다(AAA_SESSION_ID가 없다). --pane으로 pane을 준다"),
    }
}

/// herdr `pane` 명령: 출력은 늘 JSON 한 줄(읽기·대화 기록은 글, `--json`이면 JSON)
pub(super) fn pane_cmd(json: bool, cmd: PaneCmd) -> Result<ExitCode> {
    let req = match cmd {
        PaneCmd::List { workspace, role } => {
            let role = match role {
                Some(r) => Some(Role::parse(&r).with_context(|| format!("모르는 역할: {r}"))?),
                None => None,
            };
            let mut res = front_call(FrontRequest::Panes { role })?;
            if let (Some(name), Ok(FrontResponse::Panes { panes })) = (workspace, &mut res) {
                let w = match front_call(FrontRequest::Workspace { name: Some(name) })? {
                    Ok(FrontResponse::Workspace(w)) => w,
                    Ok(_) => anyhow::bail!("예상하지 못한 응답"),
                    Err(e) => return front_print(true, Err(e)),
                };
                panes.retain(|p| aaa_proto::layout::ws_contains(&w, &p.id));
            }
            return front_print(true, res);
        }
        PaneCmd::Get { pane } => FrontRequest::PaneGet { pane },
        PaneCmd::Current { pane, current: _ } => FrontRequest::PaneGet { pane: current_pane(pane)? },
        PaneCmd::Read { pane, source, lines, format, ansi, raw } => {
            let format =
                if ansi || raw || format == "ansi" { aaa_proto::ReadFormat::Ansi } else { aaa_proto::ReadFormat::Text };
            return front_print(
                json,
                front_call(FrontRequest::PaneReadText { pane, source: read_source(&source), lines, format })?,
            );
        }
        PaneCmd::Chat { pane, thinking } => {
            return front(json, FrontRequest::PaneChat { pane, thinking, compact: true, commands: true, before: None });
        }
        PaneCmd::Split { target, pane, current: _, direction, ratio, cwd, machine, workspace, argv } => {
            let pane = match target {
                Some(t) => t,
                None => current_pane(pane)?,
            };
            FrontRequest::LayoutApply {
                workspace,
                change: LayoutChange::Split {
                    pane,
                    dir: if direction == "down" { SplitDir::Vertical } else { SplitDir::Horizontal },
                    machine,
                    argv,
                    cwd,
                    ratio: ratio.filter(|r| r.is_finite()).map(|r| (r.clamp(0.1, 0.9) * 1000.0).round() as u16),
                },
            }
        }
        PaneCmd::Move { pane, new_tab: _, workspace, label } => {
            FrontRequest::LayoutApply { workspace, change: LayoutChange::MoveToNewTab { pane, name: label } }
        }
        PaneCmd::Close { pane } => FrontRequest::PaneClose { pane },
        PaneCmd::Resize { pane, current: _, direction, amount } => FrontRequest::LayoutApply {
            workspace: None,
            change: LayoutChange::Resize {
                pane: current_pane(pane)?,
                direction: match direction.as_str() {
                    "left" => aaa_proto::PaneDirection::Left,
                    "up" => aaa_proto::PaneDirection::Up,
                    "down" => aaa_proto::PaneDirection::Down,
                    _ => aaa_proto::PaneDirection::Right,
                },
                amount: amount.filter(|a| a.is_finite()).map(|a| (a.abs().min(0.5) * 1000.0).round() as u16),
            },
        },
        PaneCmd::SendText { pane, text } => FrontRequest::PaneSendText { pane, text: text.join(" ") },
        PaneCmd::SendKeys { pane, keys } => FrontRequest::PaneSendKeys { pane, keys },
        PaneCmd::Run { pane, command } => FrontRequest::PaneSubmit { pane, text: command.join(" ") },
        PaneCmd::WaitOutput { pane, substring, regex, source, lines, timeout } => FrontRequest::PaneWaitOutput {
            pane,
            matcher: match (substring, regex) {
                (_, Some(value)) => aaa_proto::OutputMatch::Regex { value },
                (Some(value), None) => aaa_proto::OutputMatch::Substring { value },
                (None, None) => anyhow::bail!("--match 또는 --regex가 필요하다"),
            },
            source: read_source(&source),
            lines,
            timeout_ms: timeout,
        },
        PaneCmd::Attach { pane, file } => {
            let bytes = std::fs::read(&file).with_context(|| format!("{}를 읽지 못했다", file.display()))?;
            if bytes.is_empty() {
                anyhow::bail!("{}가 비었다", file.display());
            }
            let res = runtime()?.block_on(async {
                let mut c = aaa_common::front::FrontClient::connect(&aaa_config::paths::state_dir()).await?;
                let mut last = Ok(FrontResponse::Done);
                for req in aaa_common::front::attach_requests(&pane, &bytes) {
                    last = c.call(req).await?;
                    if last
                        .as_ref()
                        .inspect_err(|error| {
                            aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")
                        })
                        .is_err()
                    {
                        break;
                    }
                }
                anyhow::Ok(last)
            })?;
            return front_print(true, res);
        }
        PaneCmd::Kill { pane } => FrontRequest::PaneKill { pane },
        PaneCmd::Resume { pane } => FrontRequest::PaneResume { pane },
        PaneCmd::Forget { pane } => FrontRequest::PaneForget { pane },
        PaneCmd::Rename { pane, label, clear } => {
            FrontRequest::PaneRename { pane, label: (!clear).then(|| label.join(" ")) }
        }
        PaneCmd::PtySize { pane, cols, rows } => FrontRequest::PaneResize { pane, cols, rows },
    };
    front(true, req)
}

/// 모든 workspace에서 탭을 찾는다
pub(super) fn find_tab(tab: &str) -> Result<Option<(String, aaa_proto::Tab)>> {
    let names = match front_call(FrontRequest::Workspaces)? {
        Ok(FrontResponse::Workspaces { workspaces }) => workspaces,
        Ok(_) => anyhow::bail!("예상하지 못한 응답"),
        Err(e) => anyhow::bail!("{e}"),
    };
    for w in names {
        if let Ok(FrontResponse::Workspace(w)) = front_call(FrontRequest::Workspace { name: Some(w.name) })?
            && let Some(t) = w.tabs.iter().find(|t| t.id == tab)
        {
            return Ok(Some((w.name.clone(), t.clone())));
        }
    }
    Ok(None)
}

/// herdr `tab` 명령(출력은 늘 JSON 한 줄)
pub(super) fn tab_cmd(cmd: TabCmd) -> Result<ExitCode> {
    let req = match cmd {
        TabCmd::List { workspace } => FrontRequest::Workspace { name: workspace },
        TabCmd::Create { workspace, machine, label, cwd, cols, rows, argv } => FrontRequest::LayoutApply {
            workspace,
            // herdr `tab create --label`은 탭 이름이다. pane 이름도 같게 둔다(예전 `1:이름` 이름표가 보이던 값)
            change: LayoutChange::NewTab { machine, name: label.clone(), argv, cwd, cols, rows, label, from: None },
        },
        TabCmd::Get { tab } => {
            return match find_tab(&tab)? {
                Some((workspace, t)) => {
                    println!("{}", serde_json::json!({ "kind": "tab", "workspace": workspace, "tab": t }));
                    Ok(ExitCode::SUCCESS)
                }
                None => front_print(
                    true,
                    Err(aaa_proto::ErrorBody::new(aaa_proto::ErrorCode::NotFound, format!("탭이 없다: {tab}"))),
                ),
            };
        }
        TabCmd::Rename { tab, label } => {
            FrontRequest::LayoutApply { workspace: None, change: LayoutChange::RenameTab { tab, name: label } }
        }
        TabCmd::Close { tab } => FrontRequest::LayoutApply { workspace: None, change: LayoutChange::CloseTab { tab } },
    };
    front(true, req)
}

/// herdr `workspace` 명령(출력은 늘 JSON 한 줄)
pub(super) fn workspace_cmd(cmd: WorkspaceCmd) -> Result<ExitCode> {
    let req = match cmd {
        WorkspaceCmd::List => FrontRequest::Workspaces,
        WorkspaceCmd::Get { name } => FrontRequest::Workspace { name },
        WorkspaceCmd::Create { label, machine, cwd, argv } => {
            if let Ok(FrontResponse::Workspaces { workspaces }) = front_call(FrontRequest::Workspaces)?
                && workspaces.iter().any(|w| w.name == label && w.tabs > 0)
            {
                return front_print(
                    true,
                    Err(aaa_proto::ErrorBody::new(
                        aaa_proto::ErrorCode::NameTaken,
                        format!("이미 있는 workspace: {label}"),
                    )),
                );
            }
            FrontRequest::LayoutApply {
                workspace: Some(label),
                change: LayoutChange::NewTab {
                    machine,
                    name: None,
                    argv,
                    cwd,
                    cols: None,
                    rows: None,
                    label: None,
                    from: None,
                },
            }
        }
        WorkspaceCmd::Rename { name, label } => FrontRequest::WorkspaceRename { name, to: label },
        WorkspaceCmd::Close { name } => FrontRequest::WorkspaceClose { name },
    };
    front(true, req)
}

pub(super) fn render_node(n: &aaa_proto::LayoutNode) -> String {
    match n {
        aaa_proto::LayoutNode::Pane { pane } => pane.clone(),
        aaa_proto::LayoutNode::Split { dir, a, b, .. } => {
            let sep = if *dir == SplitDir::Horizontal { " | " } else { " / " };
            format!("({}{sep}{})", render_node(a), render_node(b))
        }
    }
}

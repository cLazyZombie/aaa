//! 할 일·자동화·routine 명령.

use super::*;

#[derive(Subcommand)]
pub(super) enum TaskCmd {
    /// 예약 차례·실제 대기 이유·실행 연결만 읽는다(예약을 바꾸지 않음)
    Queue,
    /// 현재 내용으로 실행을 승인하고 자리 대기에 넣는다(같은 예약의 내용 갱신도 이 명령)
    Reserve {
        task: String,
        #[arg(long)]
        block: Option<String>,
        #[arg(long)]
        profile: Option<String>,
        /// 시작할 때까지 유지할 계정. 쓸 수 없으면 다른 계정으로 바꾸지 않고 기다린다
        #[arg(long)]
        account: Option<String>,
        /// 앞 할 일(ID·제목). 모두 완료 보관된 뒤 자리 대기에 든다. 여러 값·반복 지정 가능
        #[arg(long, value_name = "ID_OR_TITLE", num_args = 1..)]
        after: Vec<String>,
        /// 예약 전에 자동 병합을 정한다(on: `[병합 준비됨]` 뒤 coordinator가 묻지 않고 착지 창)
        #[arg(long, value_parser = ["on", "off"])]
        auto_merge: Option<String>,
        /// coordinator: 자동 병합을 바꾸라는 사용자 지시 근거(한 줄)
        #[arg(long, value_name = "REASON", requires = "auto_merge")]
        user_request: Option<String>,
    },
    /// 아직 시작하지 않은 예약을 취소한다(실행 중 thread는 그대로)
    Unreserve { task_id: String },
    /// 예약 순번을 바꾼다(1부터, 0은 맨 앞)
    QueueMove { task_id: String, position: u32 },
    /// 완료 보관을 읽거나 같은 task ID로 복원한다(재예약하지 않음)
    Archive {
        #[arg(long)]
        restore: Option<String>,
    },
    /// 목록별 task ID와 형식 어긋남
    List,
    /// task를 더한다(정규형 `- [ ] 제목 (모델@머신) [저장소]`, 같은 제목은 거부)
    Add {
        title: String,
        /// 목록(`##` 이름). 없으면 파일 끝에 만든다
        #[arg(long, default_value = "Backlog")]
        list: String,
        /// agent에게 맡길 모델(profile). 비우면 시작할 때 고른다
        #[arg(long)]
        profile: Option<String>,
        /// agent에게 맡길 머신. 비우면 시작할 때 그 저장소가 있는 머신 중 덜 붐비는 곳
        #[arg(long)]
        machine: Option<String>,
        /// 사람의 할 일: `me`(사용자)나 사람 이름(--person). 옛 꼴 profile@machine도 받는다. --profile·--machine과 함께 쓰지 않는다
        #[arg(long)]
        owner: Option<String>,
        /// owner가 사람 이름이다
        #[arg(long)]
        person: bool,
        /// 메모 한 줄(여러 번)
        #[arg(long = "note")]
        notes: Vec<String>,
        /// 이 task의 하위로
        #[arg(long, value_name = "ID_OR_TITLE")]
        parent: Option<String>,
        /// 이 task 앞에
        #[arg(long, value_name = "ID_OR_TITLE")]
        before: Option<String>,
        /// 이어 줄 thread(` · t-NNNN`, task 없는 열린 thread를 task로 만들 때)
        #[arg(long)]
        thread: Option<String>,
        /// 띄울 저장소: project.toml repos 경로나 그 마지막 이름(이름으로 적는다), 또는 `작업 폴더`. 비우면 정하지 않음
        #[arg(long)]
        repo: Option<String>,
        /// 자동 병합(기본 off). on이면 `[병합 준비됨]` 뒤 coordinator가 묻지 않고 착지 창을 준다
        #[arg(long, value_parser = ["on", "off"])]
        auto_merge: Option<String>,
        /// coordinator: 자동 병합을 켜라는 사용자 지시 근거(한 줄)
        #[arg(long, value_name = "REASON", requires = "auto_merge")]
        user_request: Option<String>,
    },
    /// task 칸을 바꾼다(준 칸만, 나머지 줄은 바이트 그대로)
    Edit {
        /// 고칠 task ID(d-0001·ULID) 또는 제목(대소문자 무시)
        task: String,
        /// 사용자 지시로 모델·머신·자동 병합을 바꾸는 근거 한 줄. --profile·--machine·--auto-merge 변경에만 쓴다(예약 유지)
        #[arg(long, value_name = "REASON")]
        user_request: Option<String>,
        /// 자동 병합 on/off. on이면 `[병합 준비됨]` 뒤 coordinator가 묻지 않고 착지 창을 준다
        #[arg(long, value_parser = ["on", "off"])]
        auto_merge: Option<String>,
        /// 읽은 판의 블록 hash(`task list --json`의 `block_sha256`). 주면 그 블록이 그대로일 때만 고친다(아니면 revision_conflict)
        #[arg(long, value_name = "SHA256")]
        block: Option<String>,
        #[arg(long)]
        title: Option<String>,
        /// 모델(profile) 칸. 빈 글이면 비운다(머신 칸은 그대로)
        #[arg(long)]
        profile: Option<String>,
        /// 머신 칸. 빈 글이면 비운다(모델 칸은 그대로)
        #[arg(long)]
        machine: Option<String>,
        /// 사람의 할 일(`me`·사람 이름). 빈 글이면 미할당
        #[arg(long)]
        owner: Option<String>,
        #[arg(long)]
        person: bool,
        /// 메모 전체를 이 줄들로 교체(여러 번). 추가는 --add-note
        #[arg(long = "note", conflicts_with_all = ["add_notes", "clear_notes"])]
        notes: Vec<String>,
        /// 기존 메모 끝에 한 줄 추가(여러 번)
        #[arg(long = "add-note", conflicts_with = "clear_notes")]
        add_notes: Vec<String>,
        /// 메모를 지운다
        #[arg(long)]
        clear_notes: bool,
        /// 이은 thread(빈 글이면 연결을 푼다)
        #[arg(long)]
        thread: Option<String>,
        /// 저장소 칸(`task add --repo`와 같은 값, 빈 글이면 비운다)
        #[arg(long)]
        repo: Option<String>,
        #[arg(long, conflicts_with = "undone")]
        done: bool,
        #[arg(long)]
        undone: bool,
    },
    /// task를 하위 task와 함께 옮긴다: --before 앞, --parent 하위 끝, 아니면 --list 끝
    Move {
        task: String,
        /// 읽은 판의 블록 hash(`task list --json`의 `block_sha256`). 주면 그 블록이 그대로일 때만 고친다(아니면 revision_conflict)
        #[arg(long, value_name = "SHA256")]
        block: Option<String>,
        #[arg(long, default_value = "")]
        list: String,
        #[arg(long, value_name = "ID_OR_TITLE")]
        before: Option<String>,
        #[arg(long, value_name = "ID_OR_TITLE")]
        parent: Option<String>,
    },
    /// task를 하위 task와 함께 지운다(기록에 남아 `restore`로 되살린다)
    Rm {
        task: String,
        /// 읽은 판의 블록 hash(`task list --json`의 `block_sha256`). 주면 그 블록이 그대로일 때만 고친다(아니면 revision_conflict)
        #[arg(long, value_name = "SHA256")]
        block: Option<String>,
    },
    /// 기록의 삭제 항목을 되살린다(원래 목록 끝, 또는 --list)
    Restore {
        /// 삭제 기록 번호, d-0001, ULID 또는 제목
        task: String,
        #[arg(long)]
        list: Option<String>,
    },
    /// 변경 기록(최신부터): 누가·언제·무엇
    Log {
        /// d-0001·ULID 또는 제목(생략하면 모든 기록)
        task: Option<String>,
        #[arg(long, default_value_t = 30)]
        limit: u32,
        /// 이 기록 id 앞의 것
        #[arg(long)]
        before: Option<i64>,
        /// 삭제·자동 정리만(되살릴 수 있는 것)
        #[arg(long)]
        deleted: bool,
    },
    /// 새 목록(`##`, 파일 끝 또는 --after 목록 뒤)
    ListAdd {
        name: String,
        #[arg(long)]
        after: Option<String>,
    },
    /// 목록 이름 바꾸기
    ListRename { name: String, to: String },
    /// 목록의 task·설명 줄을 다른 목록 끝으로 옮기고 머리를 지운다
    ListMerge { name: String, into: String },
    /// task가 없는 목록을 지운다
    ListRm { name: String },
    /// 목록을 --before 목록 앞(없으면 파일 끝)으로
    ListMove {
        name: String,
        #[arg(long)]
        before: Option<String>,
    },
}

/// 제목으로 찾는 대상(host가 제목으로 블록을 찾는다)
pub(super) fn task_at(title: &str) -> aaa_proto::TaskAnchor {
    aaa_proto::TaskAnchor { title: title.to_string(), ..Default::default() }
}

/// `--block`을 주면 그 블록 hash로 찾는다(그사이 바뀌었으면 충돌)
pub(super) fn task_at_block(title: &str, block: Option<String>) -> aaa_proto::TaskAnchor {
    aaa_proto::TaskAnchor { block_sha256: block.unwrap_or_default(), ..task_at(title) }
}

/// `aaa task` 고치기: 충돌이면 그 까닭과 지금 원문을 보이고 종료 코드 1이다
pub(super) fn task_edit_cmd(json: bool, ops: Vec<aaa_proto::TaskOp>, user_request: Option<String>) -> Result<ExitCode> {
    let res =
        front_call(FrontRequest::TaskEdit { project: project_or_here(None)?, ops, base_sha256: None, user_request })?;
    if let Ok(FrontResponse::TaskEdited { conflict: Some(c), .. }) = &res {
        if json {
            eprintln!("{}", serde_json::to_string(&serde_json::json!({ "conflict": c }))?);
        } else {
            eprintln!("aaa: revision_conflict: {}", c.reason);
            if let Some(t) = &c.latest {
                eprintln!("  지금 원문({}줄): {}", t.line, t.raw);
            }
        }
        return Ok(ExitCode::from(1));
    }
    front_print(json, res)
}

#[derive(Subcommand)]
pub(super) enum AutomationCmd {
    /// 자기 입력의 처리 결과를 인증된 post 경로로 남긴다
    Input {
        #[command(subcommand)]
        cmd: AutomationInputCmd,
    },
    /// thread 시작 또는 명령 실행 범위를 본 hash로 승인한다(사람의 aaa 밖 터미널)
    Approve {
        name: String,
        approval_sha256: String,
        #[arg(long, default_value="thread", value_parser=["thread","command"])]
        capability: String,
    },
    /// 자동화 목록과 현재 상태
    List,
    /// 설정 원문과 검증 결과
    Show {
        name: String,
    },
    /// 실행 원장과 옛 관측 기록
    Runs {
        name: String,
        #[arg(long)]
        before: Option<String>,
    },
    Enable {
        name: String,
    },
    Disable {
        name: String,
    },
    /// schema 2로 이관한다. 미리보기 hash를 적용 때 그대로 준다
    Migrate {
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        plan_sha256: Option<String>,
    },
    /// 옛 파일을 검증 뒤 TOML로 옮기고 원본은 .converted/에 보존한다
    Convert {
        project: Option<String>,
    },
    /// 결과 불명인 명령·입력 전달 묶음을 확인한 뒤 푼다(사람의 aaa 밖 터미널). 그 묶음은 다시 실행하지 않는다.
    /// 묶음 id는 `routine list --json`의 `queue.inflight.id`다
    Unblock {
        name: String,
        #[arg(long)]
        batch: String,
        /// 뒤에 쌓인 입력도 버린다(없으면 이어 보낸다)
        #[arg(long)]
        discard: bool,
    },
    /// 지금 실행한다. run show <실행 ID>는 상세 조회다. 수동 실행도 꺼짐·승인·정책을 지킨다
    Run {
        name: String,
        /// `run show <실행 ID>`로 상세를 읽는다
        id: Option<String>,
    },
    /// 끈 뒤 automations/.trash/로 옮긴다. 대기 입력은 취소하고 기록·본문 파일은 둔다
    Delete {
        name: String,
    },
    /// 지운 자동화를 꺼진 채로 되살린다. id는 `routine list`의 「지운 자동화」 줄이다
    Restore {
        id: String,
    },
    /// Rust 정본 타입에서 JSON Schema를 출력한다
    Schema {
        #[arg(long)]
        mission: bool,
    },
}

#[derive(Subcommand)]
pub(super) enum AutomationInputCmd {
    Complete {
        #[arg(required = true, num_args = 1..)]
        runs: Vec<String>,
        #[arg(long,value_parser=["completed","failed","needs-decision"])]
        outcome: String,
        #[arg(long)]
        report: Option<String>,
    },
}

#[derive(Subcommand)]
pub(super) enum RoutineCmd {
    /// 최근 실행·건너뜀 기록(최대 50건, 최근 순서)
    Runs {
        name: String,
        /// (옛 꼴 `<프로젝트> <대상>`의 대상)
        #[arg(hide = true)]
        legacy: Option<String>,
    },
    List {
        /// (옛 꼴의 프로젝트, 생략한다)
        #[arg(hide = true)]
        project: Option<String>,
    },
    /// 켜거나 끈다(front matter `enabled`). `--on`·`--off`가 없으면 뒤집는다. 명령 승인은 그대로 필요하다
    Toggle {
        name: String,
        /// (옛 꼴 `<프로젝트> <대상>`의 대상)
        #[arg(hide = true)]
        legacy: Option<String>,
        #[arg(long, conflicts_with = "off")]
        on: bool,
        #[arg(long)]
        off: bool,
    },
    /// 명령을 승인한다. 사람이 host 머신 터미널(aaa 세션 밖)에서 명령을 보고 routine 이름을 쳐야 한다
    Approve {
        name: String,
        /// (옛 꼴 `<프로젝트> <대상>`의 대상)
        #[arg(hide = true)]
        legacy: Option<String>,
    },
}

/// routine 명령 승인: agent가 대신 승인하지 못하게, aaa 세션 안이거나 터미널이 아니면 거부하고 사람에게 이름을 치게 한다.
/// 남는 한계: 사람의 터미널을 흉내 내는 다른 수단(aaa 밖의 PTY)까지 막지는 못한다(plan §8.19).
pub(super) fn routine_approve(json: bool, project: String, name: String) -> Result<ExitCode> {
    crate::person::require("`aaa routine approve`(routine 명령 승인)")?;
    let rt = runtime()?;
    let list = rt.block_on(async {
        let mut c = aaa_common::front::FrontClient::connect(&aaa_config::paths::state_dir()).await?;
        c.call(FrontRequest::Routines { project: project.clone() }).await
    })?;
    let routines = match list {
        Ok(FrontResponse::Routines { routines, .. }) => routines,
        Ok(_) => anyhow::bail!("예상하지 못한 응답"),
        Err(e) => anyhow::bail!("{e}"),
    };
    let r = routines
        .into_iter()
        .find(|r| r.name == name)
        .ok_or_else(|| anyhow::anyhow!("routine이 없다: {project}/{name}"))?;
    if let Some(e) = &r.error {
        anyhow::bail!("routine 파일을 읽지 못한다: {e}");
    }
    if r.command.is_empty() {
        anyhow::bail!("명령이 없는 routine이다. 승인할 것이 없다");
    }
    let machine = r.machine.as_deref().unwrap_or("host 머신");
    println!("routine `{name}`({project})은 {machine}에서 이 명령을 `sh -c`로 실행한다. 일정: {}\n", r.schedule);
    println!("    {}\n", r.command);
    if !r.commands_allowed {
        println!(
            "지금은 safety routine_commands가 꺼져 있어, 승인해도 켜기 전에는 돌지 않는다(`aaa safety set routine_commands on`).\n"
        );
    }
    if crate::person::ask("승인하려면 routine 이름을 친다: ")? != name {
        anyhow::bail!("이름이 다르다. 승인하지 않았다");
    }
    front(json, FrontRequest::RoutineApprove { project, name, command_sha256: r.command_sha256 })
}

/// 각 frame은 작은 페이지다. CLI의 기존 전체 목록 출력은 로컬에서 합친다.
pub(super) fn task_archive_cli(json: bool, project: String, mut restore: Option<String>) -> Result<ExitCode> {
    let mut all = Vec::new();
    let mut after = None;
    loop {
        let response = front_call(FrontRequest::TaskArchive {
            project: project.clone(),
            restore: restore.take(),
            view: Some(aaa_proto::TaskArchiveQuery { after: after.clone(), ..Default::default() }),
        })?;
        let (entries, next, repos, profiles, machines) = match response {
            Ok(FrontResponse::TaskArchive { entries, next, repos, profiles, machines }) => {
                (entries, next, repos, profiles, machines)
            }
            other => return front_print(json, other),
        };
        all.extend(entries);
        if next.is_none() {
            return front_print(
                json,
                Ok(FrontResponse::TaskArchive { entries: all, next: None, repos, profiles, machines }),
            );
        }
        anyhow::ensure!(next != after, "완료 보관 페이지 커서가 나아가지 않았다");
        after = next;
    }
}

pub(super) fn task_text(file: Option<&str>) -> Result<String> {
    Ok(match file {
        None => String::new(),
        Some("-") => {
            let mut t = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut t)?;
            t
        }
        Some(f) => std::fs::read_to_string(f).with_context(|| format!("할 일 파일: {f}"))?,
    })
}

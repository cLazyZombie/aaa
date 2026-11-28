//! thread와 post 명령.

use super::*;

#[derive(Args)]
pub(super) struct PostArgs {
    /// 목적지(user·coordinator) 또는 status·handled·list·unmute. handled는 사용자 답의 처리 표시만 한다
    pub(super) first: String,
    pub(super) second: Option<String>,
    pub(super) third: Option<String>,
    #[arg(long, conflicts_with_all = ["data", "notice"])]
    pub(super) request: bool,
    #[arg(long, conflicts_with_all = ["request", "notice"])]
    pub(super) data: bool,
    #[arg(long, conflicts_with_all = ["request", "data"])]
    pub(super) notice: bool,
    #[arg(long)]
    pub(super) needs_user: bool,
    /// 전송할 때 필수인 주제(--request·--data·--notice). list·status·handled·unmute에서는 생략한다
    #[arg(long)]
    pub(super) topic: Option<String>,
    #[arg(long)]
    pub(super) title: Option<String>,
    #[arg(long)]
    pub(super) body_file: Option<String>,
    /// library의 큰 자료 경로(내용은 post에 넣지 않는다)
    #[arg(long = "ref")]
    pub(super) refs: Vec<String>,
    /// 재전송할 때 같은 ID를 쓴다(32자리 16진수). 생략하면 새 ID
    #[arg(long)]
    pub(super) post_id: Option<String>,
}

#[derive(Args)]
pub(super) struct ThreadStartArgs {
    /// 사용자가 직접 실행 한도를 무시하고 시작한다(자동 시작·예약에는 쓰지 않는다)
    #[arg(long)]
    pub(super) over_limit: bool,
    /// coordinator가 해당 task를 강제 시작하거나 자동 병합을 바꾸라는 사용자 지시 근거를 남긴다(--over-limit·--auto-merge·
    /// --ignore-reservation과 함께)
    #[arg(long)]
    pub(super) user_request: Option<String>,
    /// 다른 일이 독점 예약한 머신에도 시작한다(사용자, 또는 --user-request를 준 coordinator)
    #[arg(long)]
    pub(super) ignore_reservation: bool,
    /// 시작하며 이어진 할 일의 자동 병합을 정한다(on: `[병합 준비됨]` 뒤 coordinator가 묻지 않고 착지 창)
    #[arg(long, value_parser = ["on", "off"])]
    pub(super) auto_merge: Option<String>,
    /// (옛 꼴의 프로젝트, 생략한다)
    #[arg(hide = true)]
    pub(super) project: Option<String>,
    /// timing 같은 측정 표지(쉼표 구분). 그 머신의 자동화가 명시한 wait를 지킨다.
    #[arg(long, value_delimiter = ',')]
    pub(super) holds: Vec<String>,
    /// 제목(--from-task면 비워도 된다). 직접 시작해도 host가 할 일을 먼저 만들고 연결한다
    #[arg(long)]
    pub(super) title: Option<String>,
    #[arg(long)]
    pub(super) machine: Option<String>,
    /// profile(모델). 없으면 --from-task의 모델 칸, 그다음 project.toml thread_profile
    #[arg(long)]
    pub(super) profile: Option<String>,
    /// host에 등록한 계정 이름(profile과 별개)
    #[arg(long, conflicts_with = "account_pool")]
    pub(super) account: Option<String>,
    /// 같은 harness의 자동 배정 계정에서 고른다. 없으면 기본 계정을 쓴다.
    #[arg(long)]
    pub(super) account_pool: bool,
    #[arg(long, hide = true, requires="account_pool", value_parser=["small","medium","large"])]
    pub(super) account_size: Option<String>,
    /// 재전송할 때 같은 시작 ID를 준다. 생략하면 새 ID다.
    #[arg(long, requires = "account_pool")]
    pub(super) start_id: Option<String>,
    /// worktree(--repo 필요) 또는 dir(repo 없는 작업 폴더). 없으면 --repo가 있으면 worktree, 없으면 dir
    #[arg(long, value_parser = ["worktree", "dir"])]
    pub(super) kind: Option<String>,
    /// 그 머신의 repo 경로. --from-task면 task 저장소 칸이 정하고(다르면 거부), 칸이 빈 task에는 경로나 이름을 준다
    #[arg(long)]
    pub(super) repo: Option<String>,
    /// 기준 ref(기본: origin/HEAD, 없으면 현재 branch)
    #[arg(long)]
    pub(super) base: Option<String>,
    /// 할 일 파일(`-`면 표준 입력). --from-task면 비워도 된다.
    #[arg(long)]
    pub(super) task_file: Option<String>,
    /// TASKS.md의 task 제목. 모델·머신 칸이 profile·machine 기본값이 되고 메모가 할 일에 붙는다. 저장소 칸이 저장소를 정하고,
    /// 머신을 정하지 않았으면 그 저장소가 있는 머신 가운데 덜 붐비는 곳이다. 칸이 비면 작업 폴더로 띄우고 경고한다.
    #[arg(long)]
    pub(super) from_task: Option<String>,
    /// 호환 옵션. --from-task로 시작하면 생략해도 그 할 일에 자동 연결한다
    #[arg(long)]
    pub(super) link_task: bool,
    /// 준비는 host 배경 작업으로 계속하고 바로 thread ID·준비 중 상태를 돌려준다
    #[arg(long)]
    pub(super) background: bool,
    /// 시작한 뒤 작업 요약을 쓴다(`aaa thread summary --file`과 같다, `-`면 표준 입력)
    #[arg(long)]
    pub(super) summary_file: Option<String>,
}

#[derive(Subcommand)]
pub(super) enum ThreadCmd {
    /// thread를 시작한다: 그 머신 agent가 worktree(또는 작업 폴더)를 만들고 profile로 agent를 띄운 뒤 brief를 안내한다
    Start(Box<ThreadStartArgs>),
    /// 이미 띄운 Claude·Codex pane을 이 프로젝트의 thread로 기록하고 brief를 안내한다(herdr-projects `thread adopt`).
    /// pane은 그대로 두고 그 폴더에 작업 폴더를 만든다. 다시 시작(restart)은 하지 않고, resolve도 pane을 끝내지 않는다.
    Adopt {
        /// (옛 꼴의 프로젝트, 생략한다)
        #[arg(hide = true)]
        project: Option<String>,
        /// 데려올 pane(ID·앞 4자 이상·이름)
        #[arg(long)]
        pane: String,
        #[arg(long)]
        title: String,
        /// 할 일 파일(`-`면 표준 입력). 없으면 「하던 일을 이어 하되 이제 이 프로젝트 thread다」
        #[arg(long)]
        task_file: Option<String>,
    },
    /// thread의 Next 목록(보고서 `## Next` + 더한 줄). --line N은 그 줄을 thread에 메시지로 보낸다.
    Next {
        thread: String,
        /// (옛 꼴 `<프로젝트> <대상>`의 대상)
        #[arg(hide = true)]
        legacy: Option<String>,
        #[arg(long, conflicts_with = "add")]
        line: Option<u32>,
        /// Next 목록에 한 줄 더한다
        #[arg(long)]
        add: Option<String>,
    },
    /// 완료 보고의 후속을 할 일에 잇거나 사유를 기록한다. 목록/ID는 thread show로 읽는다.
    Followup {
        thread: String,
        #[arg(hide = true)]
        legacy: Option<String>,
        #[arg(long)]
        id: String,
        #[arg(long, conflicts_with = "reason_file", required_unless_present = "reason_file")]
        task: Option<String>,
        /// 버림·정리 완료 사유 파일(`-`면 표준 입력)
        #[arg(long, conflicts_with = "task")]
        reason_file: Option<String>,
    },
    /// thread 목록. 기본은 전체를 작은 페이지로 이어 읽는다. --open은 끝난 thread를 뺀다.
    List {
        /// 프로젝트(생략하면 모든 프로젝트)
        project: Option<String>,
        /// 이 역할만(생략하면 모든 역할)
        #[arg(long, value_parser = ["coordinator", "worker", "evaluator"])]
        role: Option<String>,
        /// 끝난 thread를 뺀다(준비·실패·보류 포함)
        #[arg(long, conflicts_with = "status")]
        open: bool,
        /// 이 상태만
        #[arg(long, value_parser = ["starting", "open", "failed", "parked", "resolved"])]
        status: Option<String>,
        /// 출력할 최대 개수. 남은 목록은 응답의 커서를 --after에 주어 읽는다
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
        limit: Option<u32>,
        /// 앞 응답의 next_after 커서
        #[arg(long)]
        after: Option<String>,
    },
    /// 보고서와 결과물 목록, 웹에서 볼 읽기 전용 경로(2시간)
    Library {
        thread: String,
        /// (옛 꼴 `<프로젝트> <대상>`의 대상)
        #[arg(hide = true)]
        legacy: Option<String>,
    },
    Show {
        thread: String,
        /// (옛 꼴 `<프로젝트> <대상>`의 대상)
        #[arg(hide = true)]
        legacy: Option<String>,
    },
    /// 작업 요약(웹 hover 카드·「요약」 단추). --file이면 통째로 다시 쓴다(큰 작업의 상태·방향이 바뀔 때).
    /// 쉬운 말의 짧은 구로 몇 줄만 쓴다. 형식:
    ///
    /// # 목표 / 목표 한 줄 / 처음: 처음 일(선택) / ## 진행 / - [x] 완료 / - [~] 진행 중 / - [ ] 대기 /
    /// ## 정함 / - 사용자 15:0x 정한 것 / ## 바뀜 / - ~~사용자 14:0x 무효가 된 것~~ / - 사용자 14:1x 최근 바뀐 것(맨 아래).
    /// 큰 작업 3~6개(최대 6개)를 순서대로 두고 완료해도 남긴다. 한 줄 30자, 진행 외 한 절 3줄, 전체 1KB까지.
    /// 옛 ## 지금 / ## 끝남도 읽는다. 상태 없는 항목은 완료 여부를 추측하지 않는다.
    Summary {
        /// 볼 요약의 thread ID
        #[arg(required_unless_present = "check_file")]
        thread: Option<String>,
        /// (옛 꼴 `<프로젝트> <대상>`의 대상)
        #[arg(hide = true)]
        legacy: Option<String>,
        /// 요약 파일(`-`면 표준 입력)
        #[arg(long)]
        file: Option<String>,
        /// host 연결 없이 요약 형식만 검사한다(`-`면 표준 입력)
        #[arg(long, conflicts_with_all = ["thread", "legacy", "file"])]
        check_file: Option<String>,
    },
    /// 지금 보고를 확인했다
    Ack {
        thread: String,
        /// (옛 꼴 `<프로젝트> <대상>`의 대상)
        #[arg(hide = true)]
        legacy: Option<String>,
    },
    /// 끝낸다: 미처리 후속 확인 → 마지막 사본 → 세션 종료 → worktree 정리(force 없음, 병합된 branch만 삭제)
    Resolve {
        thread: String,
        /// (옛 꼴 `<프로젝트> <대상>`의 대상)
        #[arg(hide = true)]
        legacy: Option<String>,
        /// worktree와 branch를 남긴다
        #[arg(long)]
        keep_worktree: bool,
        /// 끝낸 thread를 다시 연다(아무것도 띄우지 않는다. 띄우려면 restart)
        #[arg(long, conflicts_with_all = ["keep_worktree", "skip_copy", "discard_uncopied"])]
        reopen: bool,
        /// 마지막 사본을 만들 수 없어도 끝낸다(worktree는 남긴다)
        #[arg(long)]
        skip_copy: bool,
        /// 집에 다 오지 않았어도 worktree를 지운다
        #[arg(long)]
        discard_uncopied: bool,
        /// 끝내지 않고 잃을 수 있는 것(커밋하지 않은 파일·병합되지 않은 변경·사본 상태)만 보인다
        #[arg(long, conflicts_with_all = ["reopen", "keep_worktree", "skip_copy", "discard_uncopied"])]
        dry_run: bool,
    },
    /// brief 안내를 사람이 다시 보낸다(자동 재전송은 하지 않는다)
    Brief {
        thread: String,
        /// (옛 꼴 `<프로젝트> <대상>`의 대상)
        #[arg(hide = true)]
        legacy: Option<String>,
    },
    /// thread pane이 지금 보이는 것(신뢰 창·질문 메뉴·권한 확인창)을 보인다
    Read {
        thread: String,
        /// (옛 꼴 `<프로젝트> <대상>`의 대상)
        #[arg(hide = true)]
        legacy: Option<String>,
        /// 보이는 화면 대신 기록 끝 N줄
        #[arg(long, value_name = "N")]
        lines: Option<u32>,
    },
    /// thread pane이 보이는 것에 답한다: --text를 치고(Enter 없음) 키를 누른다(enter·esc·up·down·tab·1·y·ctrl+c)
    Keys {
        thread: String,
        #[arg(value_name = "KEY")]
        keys: Vec<String>,
        /// 키보다 먼저 치는 글
        #[arg(long, value_name = "TEXT", allow_hyphen_values = true)]
        text: Option<String>,
    },
    /// thread agent를 멈춘다(Esc 한 번). 세션·작업 폴더는 그대로다
    Stop {
        thread: String,
        /// (옛 꼴 `<프로젝트> <대상>`의 대상)
        #[arg(hide = true)]
        legacy: Option<String>,
    },
    /// 보류한다: 자리·pane을 돌려주고 worktree·대화·보고를 남긴다(plan §8.426). 기본은 thread에게 커밋·push·`## Handoff`를 맡기고
    /// `[보류 준비됨]` 보고가 오면 host가 pane을 닫는다. coordinator는 사용자 지시가 있을 때만 `--user-request`와 함께 쓴다
    Park {
        thread: String,
        /// (옛 꼴 `<프로젝트> <대상>`의 대상)
        #[arg(hide = true)]
        legacy: Option<String>,
        /// 정리를 맡기지 않고 바로 닫는다(응답이 없거나 세션이 끝난 thread)
        #[arg(long, conflicts_with = "cancel")]
        now: bool,
        /// 보류 까닭(이어 할 때 brief에 들어간다)
        #[arg(long, value_name = "TEXT")]
        note: Option<String>,
        /// coordinator: 보류를 말한 사용자 지시 근거(한 줄)
        #[arg(long, value_name = "TEXT")]
        user_request: Option<String>,
        /// 맡긴 정리를 거둔다(아직 닫지 않았을 때)
        #[arg(long)]
        cancel: bool,
    },
    /// 이어진 할 일의 자동 병합을 켜고 끈다(생략하면 지금 값). on이면 `[병합 준비됨]` 뒤 coordinator가 사용자에게 묻지 않고 착지 창을 준다.
    /// coordinator는 사용자 지시가 있을 때만 `--user-request`와 함께 쓴다
    AutoMerge {
        thread: String,
        #[arg(value_parser = ["on", "off"])]
        mode: Option<String>,
        /// coordinator: 자동 병합을 바꾸라는 사용자 지시 근거(한 줄)
        #[arg(long, value_name = "TEXT", requires = "mode")]
        user_request: Option<String>,
    },
    /// 보류를 풀고 이어 한다. 같은 머신(기본)은 같은 대화를 잇고, 다른 머신은 push된 branch로 새 worktree와 요약 brief의 새 대화다
    Unpark {
        thread: String,
        /// (옛 꼴 `<프로젝트> <대상>`의 대상)
        #[arg(hide = true)]
        legacy: Option<String>,
        /// 이어 할 머신(기본: 보류한 머신)
        #[arg(long)]
        machine: Option<String>,
        /// 다른 머신의 저장소 경로(같은 이름의 저장소가 하나면 생략)
        #[arg(long)]
        repo: Option<String>,
        /// 머신별 길과 막힌 까닭만 보인다
        #[arg(long)]
        dry_run: bool,
    },
    /// 끝난 thread 세션을 다시 띄운다. 기본은 task를 다시 읽어 새 brief로 새 대화
    Restart {
        thread: String,
        /// (옛 꼴 `<프로젝트> <대상>`의 대상)
        #[arg(hide = true)]
        legacy: Option<String>,
        /// 저장된 대화를 잇는다(brief 안내 없음)
        #[arg(long)]
        resume: bool,
        /// 이 profile로 띄운다. 같은 harness는 --resume 가능, 다른 harness는 새 thread
        #[arg(long)]
        profile: Option<String>,
        /// coordinator가 채팅에서 받은 사용자의 모델 변경 지시를 한 줄로 인용
        #[arg(long, requires = "profile")]
        user_request: Option<String>,
        /// 실행 중인 응답을 중단하고 pane 종료를 확인한 뒤 모델 변경(먼저 저장 상태 확인)
        #[arg(long, requires = "profile")]
        stop_running: bool,
    },
    /// thread agent에 메시지를 보낸다. 작업 중이어도 바로 입력한다(harness가 다음 턴으로 줄 세운다)
    Message {
        thread: String,
        /// (옛 꼴 `<프로젝트> <대상>`의 대상)
        #[arg(hide = true)]
        legacy: Option<String>,
        /// 작업 중이면 Esc로 멈춘 뒤 보낸다. 없으면 바로 입력한다(작업 중이면 harness가 다음 턴에 읽는다)
        #[arg(long)]
        interrupt: bool,
        /// 보낼 글 파일(`-`면 표준 입력)
        #[arg(long, conflicts_with = "text", required_unless_present = "text")]
        text_file: Option<String>,
        /// 보낼 글(--text-file과 둘 중 하나 필수)
        #[arg(long, required_unless_present = "text_file")]
        text: Option<String>,
        /// 보낸 뒤 작업 요약을 다시 쓴다(방향이 바뀌는 말이면 함께, `aaa thread summary --file`과 같다)
        #[arg(long)]
        summary_file: Option<String>,
    },
    /// thread 보고 `## Questions`에 답한다(예: `aaa thread answer t-0012 1A 2B`). 웹 질문 카드와 같은 길이라 모든 기기가 「답함」을
    /// 본다. 고르지 않은 질문은 --text로 직접 쓴 답이 대신한다
    Answer {
        thread: String,
        /// 질문 번호와 선택지 글자(`1A 2B`, `1a,2b`)
        answers: Vec<String>,
        /// 직접 쓴 답(질문 번호를 붙여 쓴다, 예: "2 둘 다 보이게")
        #[arg(long)]
        text: Option<String>,
        /// 질문별 직접 답·의견(예: --comment '1=오른쪽에 둬' --comment '2=둘 다 보이게')
        #[arg(long = "comment", value_name = "번호=의견")]
        comments: Vec<String>,
    },
}

/// IPC의 작은 페이지를 로컬에서 합쳐 기존 JSON 목록 모양을 유지한다.
pub(super) fn threads_cli(
    json: bool,
    project: Option<String>,
    role: Option<aaa_proto::Role>,
    mut query: aaa_proto::ThreadListQuery,
) -> Result<ExitCode> {
    let mut all = Vec::new();
    let max = query.limit;
    loop {
        query.limit = max.map(|n| n - all.len() as u32);
        let response = front_call(FrontRequest::Threads { project: project.clone(), role, query: query.clone() })?;
        let (threads, next_after) = match response {
            Ok(FrontResponse::Threads { threads, next_after }) => (threads, next_after),
            other => return front_print(json, other),
        };
        all.extend(threads);
        if next_after.is_none() || max.is_some_and(|n| all.len() >= n as usize) {
            return front_print(json, Ok(FrontResponse::Threads { threads: all, next_after }));
        }
        anyhow::ensure!(next_after != query.after, "thread 목록 페이지 커서가 나아가지 않았다");
        query.after = next_after;
    }
}

/// `--text` 또는 `--text-file`(`-`면 표준 입력)
pub(super) fn post_front_call(
    rt: &tokio::runtime::Runtime,
    control: &mut Option<aaa_common::front::FrontClient>,
    req: FrontRequest,
) -> Result<Result<FrontResponse, aaa_proto::ErrorBody>> {
    rt.block_on(async {
        match control {
            Some(c) => c.call(req).await,
            None => {
                let mut c = aaa_common::front::FrontClient::connect(&aaa_config::paths::state_dir()).await?;
                c.call(req).await
            }
        }
    })
}

pub(super) fn post_cmd(json: bool, a: PostArgs) -> Result<ExitCode> {
    use aaa_proto::{PostInput, PostResult, PostType};
    let session = std::env::var_os(aaa_agent::hooks::ENV_SESSION).is_some();
    if a.first == "list" {
        anyhow::ensure!(a.third.is_none(), "post 인자가 너무 많다");
        return front(json, FrontRequest::PostList { project: project_or_here(a.second)? });
    }
    if a.first == "unmute" {
        let (project, thread) = on_target(a.second.context("thread가 필요하다")?, a.third)?;
        return front(json, FrontRequest::PostUnmute { project, thread });
    }
    if a.first == "handled" {
        let (project, post_id) = on_target(a.second.context("post ID가 필요하다")?, a.third)?;
        return front(json, FrontRequest::PostHandled { project, post_id });
    }
    let rt = runtime()?;
    // 세션 안: 조작 권한으로 붙으면(coordinator·사람의 셸 pane) Front로, 보기 권한(thread pane)이면 hook으로 보낸다.
    // 프로젝트는 생략한다(plan §8.288): Front 길이면 host가 맡은 하나를 채운다
    let mut control = if session { local_front_in_session(&rt)?.filter(|c| c.role.can_control()) } else { None };
    let use_hook = session && control.is_none();
    let mut handled = None;
    let result = if a.first == "status" {
        if use_hook {
            let id = a.third.or(a.second).context("post ID가 필요하다")?;
            rt.block_on(aaa_agent::post::send(serde_json::json!({"status":id})))?
        } else {
            let (project, post_id) = on_target(a.second.context("post ID가 필요하다")?, a.third)?;
            match post_front_call(&rt, &mut control, FrontRequest::PostStatus { project, post_id })? {
                Ok(FrontResponse::Post { result, handled: state }) => {
                    handled = state;
                    result
                }
                Ok(_) => anyhow::bail!("post 응답이 아니다"),
                Err(e) => anyhow::bail!("{}", e.message),
            }
        }
    } else {
        anyhow::ensure!(a.request || a.data || a.notice, "--request·--data·--notice 중 하나가 필요하다");
        let (project, to) = match a.second {
            Some(to) => (Some(a.first), to),
            None => (None, a.first),
        };
        anyhow::ensure!(a.third.is_none(), "post 인자가 너무 많다");
        use sha2::Digest;
        let id = a
            .post_id
            .unwrap_or_else(|| hex::encode(&sha2::Sha256::digest(aaa_common::util::new_id().as_bytes())[..16]));
        let input = PostInput {
            post_id: id,
            to,
            post_type: if a.request {
                PostType::Request
            } else if a.data {
                PostType::Data
            } else {
                PostType::Notice
            },
            topic: a.topic.context("--topic이 필요하다")?,
            title: a.title.context("--title이 필요하다")?,
            body: task_text(a.body_file.as_deref())?,
            needs_user: a.needs_user,
            refs: a.refs,
        };
        if use_hook {
            rt.block_on(aaa_agent::post::send(serde_json::to_value(input)?))?
        } else {
            let project = project_or_here(project)?;
            match post_front_call(&rt, &mut control, FrontRequest::Post { project, input })? {
                Ok(FrontResponse::Post { result, .. }) => result,
                Ok(_) => anyhow::bail!("post 응답이 아니다"),
                Err(e) => anyhow::bail!("{}", e.message),
            }
        }
    };
    let code = match &result {
        PostResult::Accepted { .. } | PostResult::AutomationCompleted { .. } => 0,
        PostResult::Refused { .. } => 1,
        PostResult::Queued { .. } => 2,
    };
    if json {
        let mut value = serde_json::to_value(&result)?;
        if let Some(handled) = handled {
            value["handled"] = serde_json::json!(handled);
        }
        println!("{}", value);
    } else {
        match result {
            PostResult::AutomationCompleted { id, run, runs } => {
                println!("{id} · 입력 처리 기록됨 {}", if runs.is_empty() { run } else { runs.join(" ") });
            }
            PostResult::Accepted { id } => println!(
                "ok {id}{}",
                match handled {
                    Some(true) => " · 처리됨",
                    Some(false) => " · 미처리",
                    None => "",
                }
            ),
            PostResult::Refused { reason, existing } => {
                eprintln!("err {reason}{}", existing.map(|s| format!(" (기존 {s})")).unwrap_or_default())
            }
            PostResult::Queued { post_id } => println!("queued {post_id}"),
        }
    }
    Ok(ExitCode::from(code))
}

pub(super) fn text_arg(text: Option<String>, file: Option<String>) -> Result<String> {
    Ok(match (text, file) {
        (Some(t), _) => t,
        (None, Some(f)) if f == "-" => {
            let mut t = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut t)?;
            t
        }
        (None, Some(f)) => std::fs::read_to_string(&f).with_context(|| format!("글 파일: {f}"))?,
        (None, None) => anyhow::bail!("--text 또는 --text-file이 필요하다"),
    })
}

/// 할 일 파일(`-`면 표준 입력, 없으면 빈 글)
/// 파일 글(`-`면 표준 입력)
pub(super) fn file_text(file: &str, what: &str) -> Result<String> {
    if file == "-" {
        let mut t = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut t)?;
        return Ok(t);
    }
    std::fs::read_to_string(file).with_context(|| format!("{what}: {file}"))
}

pub(super) fn check_summary_file(json: bool, file: &str) -> Result<ExitCode> {
    let text = file_text(file, "요약 파일")?;
    if let Err(error) = aaa_host::validate_thread_summary(&text) {
        return front_print(json, Err(error));
    }
    if json {
        println!("{}", serde_json::json!({ "valid": true }));
    } else {
        println!("작업 요약 형식이 맞다");
    }
    Ok(ExitCode::SUCCESS)
}

/// 시작 뒤 할 일(자동 병합, 작업 요약)을 이어 한다. `merge_after`는 제목으로 띄운 thread의 자동 병합(host가 만든 할 일에 둔다)
pub(super) fn with_summary(
    json: bool,
    req: FrontRequest,
    project: String,
    thread: Option<String>,
    summary: Option<String>,
    merge_after: Option<(String, Option<String>)>,
) -> Result<ExitCode> {
    if let Some(text) = &summary
        && let Err(e) = aaa_host::validate_thread_summary(text)
    {
        return front_print(json, Err(e));
    }
    let res = front_call(req)?;
    if let Some((mode, reason)) = merge_after {
        match &res {
            Ok(FrontResponse::Thread(v)) => auto_merge_first(&project, &v.id, Some(&mode), reason)?,
            _ => eprintln!(
                "thread가 아직 없어 자동 병합은 정하지 않았다. 시작 뒤 `aaa thread auto-merge <thread> {mode}`"
            ),
        }
    }
    let Some(text) = summary else { return front_print(json, res) };
    let thread = match (&res, thread) {
        (Ok(FrontResponse::Thread(v)), _) => v.id.clone(),
        (Ok(_), Some(t)) => t,
        _ => {
            eprintln!("앞 요청이 되지 않아 작업 요약은 쓰지 않았다");
            return front_print(json, res);
        }
    };
    let code = front_print(json, res)?;
    match front_call(FrontRequest::ThreadSummarySet { project, thread, text })? {
        Ok(_) => {
            if !json {
                println!("작업 요약을 썼다");
            }
            Ok(code)
        }
        Err(e) => front_print(json, Err(e)),
    }
}

pub(super) fn auto_merge_request(
    project: String,
    target: String,
    mode: &str,
    user_request: Option<String>,
) -> FrontRequest {
    FrontRequest::AutoMergeSet { project, target, on: mode == "on", user_request }
}

/// 다른 명령 앞뒤에 자동 병합을 정한다. 실패하면 멈춘다(앞이면 뒤 명령을 하지 않는다). 결과는 표준 오류에 한 줄(표준 출력은 본 명령 것)
pub(super) fn auto_merge_first(
    project: &str,
    target: &str,
    mode: Option<&str>,
    user_request: Option<String>,
) -> Result<()> {
    let Some(mode) = mode else { return Ok(()) };
    match front_call(auto_merge_request(project.to_string(), target.to_string(), mode, user_request))? {
        Ok(r) => {
            eprint!("{}", render_front(&r));
            Ok(())
        }
        Err(e) => anyhow::bail!("자동 병합: {}", e.message),
    }
}

//! brief에 채우는 고정 문구와 명령.

/// brief.md에서 그 글을 쓰는 agent가 채우는 자리(plan §8.78). host는 agent가 작업 폴더를 만들기 전에 brief를 짓는다.
/// 작업 폴더(`<cwd>/.aaa/<project>-<thread>`)의 절대 경로
pub const BRIEF_WORK_DIR: &str = "{{aaa:work_dir}}";

/// 그 머신 aaa 실행 파일(진행 보고 명령)
pub const BRIEF_EXE: &str = "{{aaa:exe}}";

/// memory=true 자동화의 재독 안내. host가 세션 명세에 넣으며 hook 요청의 환경·경로는 믿지 않는다.
pub const AUTOMATION_REENTRY_ENV: &str = "AAA_AUTOMATION_REENTRY";

/// 임무 inbox의 목록 조회 한 명령. agent가 채우는 경로와 allow-list가 같은 quoting을 쓴다.
pub const BRIEF_INBOX_LIST: &str = "{{aaa:inbox_list}}";

pub const BRIEF_GIT_READS: &str = "{{aaa:git_reads}}";

pub fn work_inbox_list_command(work_dir: &str) -> String {
    let path = format!("{}/inbox", work_dir.replace('\\', "/").trim_end_matches('/'));
    format!("ls -1 -- '{}'", path.replace('\'', "'\\''"))
}

/// 자기 worktree와 읽기 옵션을 고정한다. 가변 ref/path는 옵션 종료 표지 뒤에서만 받는다.
pub fn work_git_read_commands(work_dir: &str) -> Vec<String> {
    let path = work_dir.replace('\\', "/");
    let Some((cwd, leaf)) = path.rsplit_once("/.aaa/") else { return vec![] };
    if cwd.is_empty() || leaf.is_empty() || leaf.contains('/') || cwd.contains('*') {
        return vec![];
    }
    let git = format!(
        "git -C '{}' --no-pager --no-optional-locks -c core.fsmonitor=false -c log.showSignature=false",
        cwd.replace('\'', "'\\''")
    );
    vec![
        format!("{git} log --no-ext-diff --no-textconv --oneline --max-count=100 --end-of-options"),
        format!("{git} show --no-ext-diff --no-textconv --format=fuller --end-of-options"),
        format!("{git} status --short --"),
    ]
}

/// brief 자리를 채운다. 실행 파일 경로에 빈칸이 있으면 큰따옴표로 감싼다.
pub fn fill_brief(brief: &str, work_dir: &str, exe: &str) -> String {
    let exe = if exe.contains(char::is_whitespace) { format!("\"{exe}\"") } else { exe.to_string() };
    brief
        .replace(
            BRIEF_GIT_READS,
            &work_git_read_commands(work_dir).iter().map(|s| format!("- `{s}`")).collect::<Vec<_>>().join("\n"),
        )
        .replace(BRIEF_INBOX_LIST, &work_inbox_list_command(work_dir))
        .replace(BRIEF_WORK_DIR, work_dir)
        .replace(BRIEF_EXE, &exe)
}

/// 진행 보고 안내(herdr-projects `progress::guidance`를 옮김). thread brief(host)와 SessionStart hook(agent)이 함께 쓴다:
/// hook을 쓰지 못하는 harness·세션은 brief가 유일한 길이다(참고도 둘 다 둔다). `prefix`는 그 머신 aaa 실행 파일이다.
/// thread(`for_thread`)에는 report.md `## 진행` 줄을 더한다. 외부 푸시 정책은 coordinator 안내(AGENTS.md)에만 있다.
pub fn progress_guidance(prefix: &str, for_thread: bool) -> String {
    let thread = if for_thread {
        "report.md의 `## 진행`도 함께 고친다: 명령만으로는 웹 작업 요약의 목록이 바뀌지 않는다.\n"
    } else {
        ""
    };
    format!(
        "일 전체가 얼마나 됐는지 어림해 `{prefix} report --percent 25 --activity '코드 읽기'`로 알린다(5 단위, 활동은 두~네 단어).\n\
         범위가 분명하지 않으면 `{prefix} report --unknown --activity '일 가늠'`. 타이머·도구 수가 아니라 너의 어림이다.\n\
         새 일을 시작할 때, 고비를 넘긴 뒤, 활동이 바뀔 때, 막혔을 때, 내용 있는 답 전에, 일하는 동안 1분에 한 번쯤 보고한다.\n\
         진행을 지어내지 않고 일이 늘면 어림을 낮춘다. 사용자에게 물으려고 멈출 때는 늘 `--activity 'Waiting for you'`(이 말 그대로).\n\
         `--percent 100`은 결과 전체와 확인이 끝났을 때만 쓴다(그 뒤 일을 받으면 다시 낮게).\n\
         {thread}이 pane의 맨 위 agent만 보고하고(subagent·도우미는 하지 않는다), 명령은 끝날 때까지 기다린다(배경 금지).\n\
         보고가 실패해도 일을 멈추지 않는다: 진단 한 줄만 남기고 이어 간다."
    )
}

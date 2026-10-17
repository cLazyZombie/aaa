//! brief.md 글(herdr-projects 0.2.35 `src/thread.rs` `brief_header`·`compose_brief`, `skill/THREAD.md`,
//! `src/progress.rs` `guidance`). 참고 문장을 한국어로 옮겼고 명령은 aaa 이름이다(plan §8.77).
//!
//! - host는 agent가 작업 폴더를 만들기 전에 brief를 짓는다. 그래서 작업 폴더와 aaa 실행 파일 경로는
//!   `aaa_proto::BRIEF_WORK_DIR`·`BRIEF_EXE` 자리로 두고, brief.md를 쓰는 agent가 그 머신 값으로 채운다.

use aaa_proto::{BRIEF_EXE, BRIEF_WORK_DIR};

use crate::project::Repo;

pub(crate) struct BriefInput<'a> {
    /// PROJECT.md 첫 `# ` 제목(없거나 이름과 같으면 이름만 쓴다)
    pub title: &'a str,
    pub project: &'a str,
    pub goal: &'a str,
    pub repos: &'a [Repo],
    /// host의 `projects/<p>/uploads`
    pub uploads_path: &'a str,
    /// thread가 host 머신이 아닌 곳에서 돈다
    pub remote: bool,
    /// 사후 평가자면 thread 규칙 대신 평가자 규칙
    pub guide: &'a str,
    pub instructions: &'a str,
    pub memory_index: &'a str,
    /// (파일 이름, 내용), 넣을 차례대로
    pub memory_files: &'a [(String, String)],
    pub memory_cap: usize,
    pub decisions: &'a str,
    pub task: &'a str,
    pub restart: bool,
}

/// thread 규칙(herdr-projects `skill/THREAD.md`를 옮김). 「aaa에서」 두 줄은 aaa가 보고를 host로 복사하고
/// 세션 안 CLI를 보기만으로 두어서 더했다.
pub(crate) const THREAD_GUIDE: &str = "\
# thread brief

너는 aaa 프로젝트의 thread 하나다. coordinator agent가 이 brief 끝의 할 일을 맡겼다. 다른 thread들이 나란히 다른 일을 하고, 너는 그들과 이야기하지 않는다.

- 할 일을 한다. 위 프로젝트 목표를 염두에 둔다: 이 일이 무엇을 위한 것인지 말해 준다. 필요한 것이 없으면 짐작하지 말고 무엇이 없는지 보고에 정확히 적는다.
- 작업 폴더 안에 머문다. 저장소가 없는 일이면 그 폴더는 위 프로젝트 폴더가 아니라 네 thread 폴더다.
- 할 일에 필요하면 프로젝트 목록에 없는 저장소를 써도 된다. 보고에 그렇게 적으면 coordinator가 목록에 더할지 정한다.
- 사용자가 프로젝트 `uploads/` 폴더(위 경로)에 둔 파일은 읽어도 된다.
- 아래 프로젝트 지침과 memory는 네가 하는 모든 일에 적용된다.
- 프로젝트 memory를 고치지 않는다. 남길 만한 교훈은 보고의 `## Remember`에 적는다. 무엇을 남길지는 coordinator가 정한다.

## 보고

끝냈을 때, 그리고 사용자를 기다리며 멈출 때마다 위에 준 보고 경로에 보고를 쓴다. 사용자에게 줄 파일(문서·내보낸 파일·스크린샷)은 거기 준 library 폴더에 둔다.

보고 형식:

- pull request를 열었으면 첫 줄에 `PR: <url>`(선택). 주소는 `https://github.com/<owner>/<repo>/pull/<number>` 전체를 쓴다.
- `## Report` 절: 무엇을 했고, 무엇을 알아냈고, 무엇이 남았고, 사용자가 정해야 할 것은 무엇인가.
- `## Next` 절(필수): 한 줄에 권하는 행동 하나, 명령형, 100자 이하(`PR을 병합한다`, `실패한 lint 검사를 고친다`, `X를 원하는지 확인한다`, `worktree와 branch를 지운다`). 사용자가 키를 눌러 그 줄을 너에게 돌려보내면 네가 네 도구로 직접 한다. 일이 착지한 뒤 하자고 할 정리도 넣는다. 목록이 비면 할 일이 없다는 뜻이다.
- `## Remember` 절(선택): 앞으로의 thread에 남길 짧고 오래가는 교훈.

보고는 매번 통째로 다시 써서 늘 지금 상태를 말하게 한다.

## aaa에서

- host는 report.md가 바뀔 때 보고와 library를 host로 복사한다. library 파일을 먼저 두고 report.md를 마지막에 쓴다. symlink는 복사되지 않는다.
- 이 세션 안의 `aaa` CLI는 보기만 된다(다른 thread·pane에 지시하지 않는다).
";

/// 진행 보고 안내(herdr-projects `progress::guidance`를 옮김). `prefix`는 그 머신 aaa 실행 파일이다.
pub(crate) fn progress_guidance(prefix: &str) -> String {
    format!(
        "사용자의 지금 일 전체가 얼마나 됐는지 `{prefix} report`로 알린다. 이것은 타이머나 도구 수가 아니라 너의 어림이다. 보고가 실패해도 실제 일을 멈추지 않는다: 짧은 진단 한 줄만 남기고 다시 하지 않고 이어 간다.\n\n\
         5 단위로 어림한 백분율과 두~네 단어의 활동을 보고한다. 활동은 `코드 읽기`, `변경 시험`, `Waiting for you`처럼 쓴다:\n\n\
         `{prefix} report --percent 25 --activity '코드 읽기'`\n\
         `{prefix} report --unknown --activity '일 가늠'`\n\n\
         새 일을 시작할 때, 의미 있는 고비를 넘긴 뒤, 활동이 바뀔 때, 막혔을 때, 그리고 내용 있는 답을 하기 전마다 보고한다. 사용자에게 무언가 물으려고 멈출 때는 늘 `--activity 'Waiting for you'`로 보고한다(이 말 그대로). 일하는 동안에는 자연스러운 도구 경계에서 1분에 한 번쯤 보고한다. 알림을 채우려고 진행을 지어내지 않는다. 일이 더 있다는 걸 알게 되면 어림을 낮춘다. 범위가 분명하지 않은 동안은 `--unknown`을 쓴다.\n\n\
         `--percent 100`은 요청받은 결과 전체와 그 확인이 다 끝났을 때만 쓴다. `Done`으로 보인다. 그 뒤 일을 더 받으면 새로 낮은 백분율을 보고한다.\n\n\
         이 pane의 맨 위 agent만 보고한다. subagent와 도우미는 보고하지 않는다. report 명령은 끝날 때까지 기다리고 배경으로 돌리지 않는다."
    )
}

/// 다시 시작한 thread의 안내(herdr-projects `compose_brief`의 restart 문단)
const RESTART_NOTE: &str = "**이 일을 앞서 시도한 것이 이 작업 폴더(worktree면 branch)에 있다.** 먼저 아래 보고 경로의 보고를 읽고, 이미 있는 것을 본 뒤 거기서 이어 간다.\n\n";

/// brief 머리: thread가 따를 것만 넣는다. coordinator·ticker 설정은 넣지 않는다.
fn brief_header(input: &BriefInput) -> String {
    let work = BRIEF_WORK_DIR;
    let mut out = String::from("# 프로젝트\n\n");
    let title = input.title.trim();
    if title.is_empty() || title == input.project {
        out.push_str(&format!("- 프로젝트: `{}`\n", input.project));
    } else {
        out.push_str(&format!("- 프로젝트: {title} (`{}`)\n", input.project));
    }
    let goal = input.goal.trim();
    out.push_str(&format!("- 목표: {}\n", if goal.is_empty() { "(정하지 않음)" } else { goal }));
    if input.repos.is_empty() {
        out.push_str("- 저장소: (없음)\n");
    } else {
        out.push_str("- 저장소:\n");
        for repo in input.repos {
            match &repo.machine {
                Some(machine) => out.push_str(&format!("  - {} (`{machine}` 머신)\n", repo.path)),
                None => out.push_str(&format!("  - {} (host 머신)\n", repo.path)),
            }
        }
    }
    let note = if input.remote { " (host 머신에만 있다. 네 머신으로 복사되지 않는다)" } else { "" };
    out.push_str(&format!("- 사용자가 준 파일(uploads): `{}`{note}\n", input.uploads_path));
    out.push_str(&format!("- 사용자에게 줄 파일(library): `{work}/library`\n"));
    out.push_str(&format!("- 보고: `{work}/report.md`\n"));
    out
}

pub(crate) fn compose_brief(input: &BriefInput) -> String {
    let work = BRIEF_WORK_DIR;
    let mut brief = brief_header(input);
    brief.push('\n');
    brief.push_str(input.guide.trim_end());
    brief.push_str("\n\n");
    if input.restart {
        brief.push_str(RESTART_NOTE);
    }
    brief.push_str("# 프로젝트 지침\n\n");
    brief.push_str(input.instructions.trim());
    brief.push_str("\n\n# 프로젝트 memory\n\n");
    brief.push_str(input.memory_index.trim());
    brief.push('\n');
    let mut used = input.memory_index.chars().count();
    let mut left_out = Vec::new();
    for (name, text) in input.memory_files {
        let size = text.chars().count();
        if used + size <= input.memory_cap {
            used += size;
            brief.push_str(&format!("\n## memory/{name}\n\n{}\n", text.trim()));
        } else {
            left_out.push(format!("memory/{name}"));
        }
    }
    if !left_out.is_empty() {
        brief.push_str(&format!(
            "\n프로젝트 memory가 {}자를 넘어 넣지 않은 것: {}.\n",
            input.memory_cap,
            left_out.join(", ")
        ));
    }
    brief.push_str("\n# 결정 기록\n\n");
    brief.push_str(input.decisions.trim());
    brief.push_str("\n\n# 진행 보고\n\n");
    brief.push_str(&progress_guidance(BRIEF_EXE));
    brief.push_str("\n\n# 할 일\n\n");
    brief.push_str(input.task.trim());
    brief.push_str(&format!(
        "\n\n# 경로\n\n- 보고: `{work}/report.md`\n- 사용자에게 줄 파일을 두는 library 폴더: `{work}/library`\n- 사용자가 준 파일(uploads): `{}`\n",
        input.uploads_path
    ));
    brief
}

#[cfg(test)]
mod tests {
    use super::*;

    /// herdr-projects `brief_order_and_memory_cap`을 옮겼다
    #[test]
    fn brief_order_and_memory_cap() {
        let cap = 32_000;
        let files = vec![
            ("a.md".to_string(), "alpha fact".to_string()),
            ("b.md".to_string(), "x".repeat(cap)),
            ("c.md".to_string(), "gamma fact".to_string()),
        ];
        let repos = vec![
            Repo { path: "/srv/app".into(), machine: Some("box".into()) },
            Repo { path: "/work/lib".into(), machine: None },
        ];
        let input = BriefInput {
            title: "Demo",
            project: "demo",
            goal: "Ship it",
            repos: &repos,
            uploads_path: "/root/demo/uploads",
            remote: false,
            guide: THREAD_GUIDE,
            instructions: "# Demo\n\nAlways run the tests.",
            memory_index: "# Memory\n- a\n- b\n- c",
            memory_files: &files,
            memory_cap: cap,
            decisions: "# 결정 기록\n\n## 1번",
            task: "Do the thing.",
            restart: true,
        };
        let brief = compose_brief(&input);
        let pos = |needle: &str| brief.find(needle).unwrap_or_else(|| panic!("없다 {needle}"));
        // 머리가 맨 앞이다: 이름, 목표, 머신과 함께 저장소, uploads, library, 보고
        assert!(brief.starts_with("# 프로젝트\n\n- 프로젝트: Demo (`demo`)\n- 목표: Ship it\n"), "{brief}");
        assert!(pos("/srv/app (`box` 머신)") < pos("/work/lib (host 머신)"));
        assert!(pos("- 사용자가 준 파일(uploads): `/root/demo/uploads`\n") < pos("# thread brief"));
        assert!(pos(&format!("- 보고: `{BRIEF_WORK_DIR}/report.md`")) < pos("# thread brief"));
        assert!(pos("# thread brief") < pos("앞서 시도한 것"));
        assert!(pos("앞서 시도한 것") < pos("Always run the tests."));
        assert!(pos("Always run the tests.") < pos("# Memory"));
        assert!(pos("# Memory") < pos("alpha fact"));
        assert!(pos("alpha fact") < pos("# 결정 기록\n\n## 1번"));
        assert!(pos("# 결정 기록") < pos("# 진행 보고"));
        assert!(pos(&format!("`{BRIEF_EXE} report --percent 25")) < pos("Do the thing."));
        assert!(pos("Do the thing.") < pos("# 경로"));
        assert!(brief.ends_with(&format!(
            "# 경로\n\n- 보고: `{BRIEF_WORK_DIR}/report.md`\n- 사용자에게 줄 파일을 두는 library 폴더: `{BRIEF_WORK_DIR}/library`\n- 사용자가 준 파일(uploads): `/root/demo/uploads`\n"
        )));
        assert!(brief.contains("gamma fact"));
        assert!(brief.contains("프로젝트 memory가 32000자를 넘어 넣지 않은 것: memory/b.md."));
        assert!(!brief.contains(&"x".repeat(100)));
        // 상태 태그 첫 줄 규칙은 없다(D9)
        assert!(!brief.contains("[진행 중]") && !brief.contains("[완료]"));
        // 운영 설정은 thread에 가지 않는다
        for word in ["max_parallel_threads", "auto_resolve_days", "start_threads", "trust_screens", "thread_profile"] {
            assert!(!brief.contains(word), "{word}");
        }

        let fresh = compose_brief(&BriefInput {
            title: "demo",
            goal: "",
            repos: &[],
            remote: true,
            instructions: "",
            memory_index: "",
            memory_files: &[],
            task: "t",
            restart: false,
            ..input
        });
        assert!(!fresh.contains("앞서 시도한 것"));
        assert!(
            fresh.starts_with("# 프로젝트\n\n- 프로젝트: `demo`\n- 목표: (정하지 않음)\n- 저장소: (없음)\n"),
            "{fresh}"
        );
        assert!(fresh.contains("`/root/demo/uploads` (host 머신에만 있다. 네 머신으로 복사되지 않는다)"));
    }

    #[test]
    fn progress_guidance_names_the_command() {
        let g = progress_guidance("/opt/aaa");
        assert!(g.contains("`/opt/aaa report --unknown --activity '일 가늠'`"), "{g}");
        assert!(g.contains("`--activity 'Waiting for you'`"));
        assert!(g.contains(crate::orch::WAITING));
    }
}

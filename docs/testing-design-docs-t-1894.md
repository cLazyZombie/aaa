# 기획 1단계: 저장소 기획 폴더 읽기 검증(t-1894)

설계는 [plan §8.565](plan.md#8565-기획-저장소-기획-폴더-읽기t-1894), 사용법은 [usage 「기획」](usage.md#기획)에 있다.

## owner

| 계약 | 시험 | 실행 |
| --- | --- | --- |
| origin 기본 branch의 git 객체만 읽고 checkout HEAD·작업 파일·FETCH_HEAD는 그대로, 같은 이름의 로컬 branch(`refs/heads/origin/main`)에 가려지지 않음, 기획 폴더 밖·없는 문서 거부 | `aaa-agent` `design::tests::tree_reads_origin_main_without_touching_checkout`·`read_refuses_outside_and_missing` | `cargo test -p aaa-agent --lib design::` |
| 트리를 읽은 commit으로 고정한 읽기는 origin이 앞서 가도 옛 글, 고르지 않으면 새 기본 branch, 기본 branch에 없는 로컬 commit은 거부 | `pinned_commit_reads_the_old_text_after_origin_moves` | 위와 같음 |
| origin을 받지 못하면 가진 ref로 읽고 `fetch_error`를 싣는다. 1분 안에 다시 읽어도 같은 까닭 | `fetch_failure_keeps_the_held_tree` | 위와 같음 |
| symlink(120000)·submodule(160000)은 목록에도 없고 읽지도 않음, 글 512KiB 상한, 조각 범위(크기만·끝 조각·끝·범위 밖) | `links_are_not_documents_and_slices_stay_in_bounds` | 위와 같음 |
| 같은 이름 저장소는 `design`을 단 항목, 설정 차례 유지, 잘못된 `design`은 없는 것, 경로는 기획 폴더 안 | `aaa-host` `design::tests::` | `cargo test -p aaa-host --lib design::` |
| 저장소 더하기·빼기·제자리 고치기 뒤에도 `design` 칸이 남는다 | `project::tests::design_folder_survives_repo_edits` | `cargo test -p aaa-host --lib project::tests::design` |
| 경로 정리·폴더 안 판정·제목(YAML 머리 건너뜀) | `aaa-proto` `design::tests` | `cargo test -p aaa-proto` |
| 트리 차례(폴더 먼저·README 먼저·폴더 제목), 상대 경로 풀기(저장소 밖·주소·절대 경로 거부, 퍼센트 부호의 `..`), 그림 주소 | `web/test/designModel.test.mjs` | `cd web && node --test test/designModel.test.mjs` |
| 실제 host·agent·git origin으로 트리 → 문서 → 같은 commit 그림 → 영상 재생 → 문서 안 링크 → 주소 다시 열기, 기획 폴더 없는 저장소 줄, 보기 기기 읽기, 폴더 밖 400·없는 범위 416·Range 206·기기 없음 401, 512KiB 조각을 이어 받은 0.9MiB 파일의 SHA-256과 조각 경계를 넘는 Range | 웹 E2E `135-design-docs`(Chromium·WebKit × 1280·390, 조합마다 한 단계) | `bash scripts/ci-local.sh --only web,e2e,public --spec 135` |
| 2-1 기획 branch: 열기(origin main에서)·다시 열기·저장(새 문서·기준 blob·겹침 conflict·폴더 밖 거부)·branch 트리(고친 파일·앞선 수)·branch 판 읽기와 main으로는 거부·검사 실패면 올리지 않음·올리기·쉼(남은 파일이 있으면 내리지 않음)·다시 꺼내기·버리기(origin까지) | `aaa-agent` `design_write::tests::open_save_push_rest_and_discard` | `cargo test -p aaa-agent --lib design_write::` |
| 2-1 force 없이 올리기(origin에 앞선 판이면 실패), 손으로 고친 worktree에는 쓰지 않음, 로컬에 없는 branch는 origin에서 꺼냄, 잘못된 주제 거부 | `design_write::tests::never_forces_or_writes_over_foreign_changes` | 위와 같음 |
| 2-1 저장소 hook(post-checkout·prepare-commit-msg·post-commit)이 돌지 않음, 쓰다 실패(index 잠금)하면 파일이 남지 않고 다음 저장이 됨, 대소문자만 다른 새 문서 거부, 같은 branch를 기획 자리가 아닌 곳에 꺼냈으면 쓰지도 내리지도 않음 | `design_write::tests::no_hooks_rollback_case_twins_and_foreign_checkouts` | `cargo test -p aaa-agent --lib design_write::` |
| 2-1 버릴 때 origin에 남이 그 위로 올렸으면 아무것도 지우지 않음(lease) | `design_write::tests::never_forces_or_writes_over_foreign_changes` | 위와 같음 |
| 2-1 휴대용 작업 root(activate·mount 있음)에서 기획 자리를 `aaa:` 사유로 잠그고, 남은 파일 때문에 못 지우면 다시 잠그고, 쉼·다시 열기·버리기 | `work_root::recovery_tests::design_worktrees_lock_on_portable_roots` | `cargo test -p aaa-agent --lib work_root::recovery_tests::design` |
| 2-1 thread·평가 세션과 보기 기기는 실제 입구(`Host::call`)에서 기획 branch를 열거나 쓰거나 버리지 못하고, 사람의 셸 pane은 지나간다 | `aaa-host` `design_branch::tests::sessions_and_readers_do_not_write_design_branches` | `cargo test -p aaa-host --lib design_branch::` |
| 2-1 `.git` 조각·끝의 `.`·Windows 금지 글자·예약 이름 경로 거부 | `aaa-proto` `design::tests::clean_path_rejects_escape` | `cargo test -p aaa-proto` |
| 2-1 실제 host·agent·Git과 CLI: `plan new`·`save`·`show --branch`·`list`·`discard`, 겹침·기준 판 없음 거부, 쉼(1초) 뒤 한 번 올림·`design_check` 통과, main과 main checkout 그대로, 버리면 origin branch까지 / 쉼 창 뒤 tick이 worktree를 돌려주고 branch·commit은 남김, 검사 실패면 올리지 않고 까닭을 남김, 쉬는 branch에는 쓰지 않음, 다시 열면 이어 씀 | `aaa` m13 `design_branch::` 둘 | `cargo test -p aaa --test m13 design_branch` |
| 2-2 기획 대화 thread는 보기 권한·같은 머신 pane 모두 자기 branch 저장만 지나가고(commit 저자 표시는 thread ID), 다른 branch·다른 thread·버린 대화에서 남은 thread(대화 기록의 지금 thread가 아님)·다른 프로젝트의 같은 표지 thread·알린 세션만의 범위(`@unverified`)·열기·버리기·말하기는 거부, 중계 목록은 저장만 싣는다 | `aaa-host` `design_branch::tests::design_chat_thread_saves_only_its_own_branch` | `cargo test -p aaa-host --lib design_branch` |
| 2-2 원격 중계는 기획 열기·말하기·버리기를 싣지 않는다 | `thread_read::tests::relay_checks_machine_live_session_and_explicit_read_allowlist` | `cargo test -p aaa-host --lib thread_read` |
| 2-2 아이디어·기획 대화 thread의 [완료] Next는 후속 할 일이 아니고 CLI 끝내기를 막지 않는다 | `followups::tests::chat_only_threads_collect_no_followups` | `cargo test -p aaa-host --lib followups` |
| 2-2 대화 thread 할 일 글이 branch·worktree·checkout·읽기·저장 명령을 가리킨다 | `design_chat::tests::task_text_points_at_branch_worktree_and_commands` | `cargo test -p aaa-host --lib design_chat` |
| 2-2 실제 host·agent 둘(TLS 원격 labB)·Git과 CLI: 저장소가 labB에 있으면 `plan say`가 labB에 대화 thread를 띄우고(할 일 없음) brief 뒤 말을 보내 `pending` 0, front.sock 없는 thread 세션이 agent 중계로 `plan save`(자기 branch, 500KiB 안팎 문서의 저장·읽기와 512KiB 넘음 거절 포함)·`show --branch`·`list`, 다른 branch 저장은 「자기 branch」 거부, 열기·말하기·버리기는 CLI가 보내지 않음, commit 메시지 끝이 thread ID, 저장소 checkout 그대로, 올린 기획 branch에서 `--base origin/design/<주제>`로 시작한 worktree thread가 그 commit과 문서를 가짐, 버리면 대화 thread가 끝나고 origin branch까지 지움 | `aaa` m13 `design_branch::design_chat_thread_on_the_repo_machine_saves_only_its_own_branch` | `cargo test -p aaa --test m13 design_branch` |
| 2-2 같은 m13: 보류한 대화 thread에 다음 말을 하면 같은 thread로 잇고 말이 닿음, 쉼 창(1초) 뒤 tick이 branch를 쉬게 하고 다음 말이 branch를 다시 열어 같은 thread에 보냄, 띄우는 중에 버리면 그 thread가 끝나고 branch가 사라짐, pane 밖 프로세스가 대화 thread의 `AAA_SESSION_ID`를 대고 `plan save`하면 거부 | 위와 같음 | 위와 같음 |
| 2-2 원격 전달 hook 사건은 600KiB 글을 싣고(다른 hook은 256KiB에서 거부), 연결 frame 한도를 넘으면 거부, 기획 글 한도+JSON이 들어간다 | `aaa-agent` `hooks::tests::relay_messages_carry_design_documents_but_other_hooks_keep_the_old_limit` | `cargo test -p aaa-agent --lib hooks::` |
| 2-2 host 머신이 아닌 저장소 머신은 `design_chat.v1`을 아는 agent에서만 대화 thread를 띄우고, 아니면 머신 이름·기능 이름을 담은 Unavailable | `aaa-host` `design_chat::tests::chat_threads_start_only_where_saves_can_reach_the_host` | `cargo test -p aaa-host --lib design_chat` |
| 2-3 그림 붙이기: 조각을 이어 받아 마지막 조각에서 sha256을 보고 그 파일만 새로 commit, 자리 어긋남·sha256 다름·상한 넘음은 거부하고 조각 자리를 지움, 이미 있는 이름은 conflict | `aaa-agent` `design_write::tests::attach_gathers_chunks_and_commits_one_new_file` | `cargo test -p aaa-agent --lib design_write` |
| 2-3 사람 고침 알림(다음 말 앞 「(문서 고침: …)」 뒤 비움)·대화 사건(open·create·push·edit)·고친 줄 수, `plan apply`가 저장소 머신(labB)에 `origin/design/<주제>`에서 갈라진 worktree thread를 띄우고 그 commit을 가짐, 끝나지 않은 반영이 있으면 거부 | `aaa` m13 `design_branch::design_chat_thread_on_the_repo_machine_…` | `cargo test -p aaa --test m13 design_branch` |
| 2-3 쉬는 branch에 말하면 다시 열고 대화 thread를 띄워 말을 보냄(같은 commit) | `aaa` m13 `design_branch::idle_design_branch_rests_without_losing_commits_and_reopens` | 위와 같음 |
| 2-3 트리 branch 표시·새로 만든 문서, branch 덩어리 표시, branch 이름 짓기·꼴, 대화 사건 묶기·문구, branch 판 그림 주소 | `web/test/designModel.test.mjs` 「기획 branch 화면」 | `cd web && node --test test/designModel.test.mjs` |
| 2-3 실제 host·agent·git으로 웹: 트리 branch 수·문서 칩·「+」 문서 → 판 고르기(main·두 branch)·main과 다른 덩어리·다른 branch 알림 → 고치기와 그림 붙이기(그 branch에 commit, 그림 주소 `b=<주제>`, 사람 고침 사건·알림 칩) → 대화 고르기·새 대화(영문 주제로 branch 이름, 잘못된 꼴 막음, 새 대화 사건) → main에 반영(고친 파일·줄 수, 반영 대기, 올린 뒤 worktree thread) → 버리기(commit 수, 트리에서 사라짐) → 휴대폰 칸 | E2E `136-design-branches` | `ci-local --only web,e2e --spec 136` |

- git 실험실은 `-c init.defaultBranch=main`·`-c user.*`를 주고 index에 symlink·gitlink를 직접 넣어 OS의 symlink 권한과 기본 branch 설정에 기대지 않는다.
- 1분 받기 간격은 시험마다 새 임시 저장소(다른 toplevel)라 서로 막지 않는다. 고정 commit 시험은 clone에서 직접 `git fetch`해 시계를 기다리지 않는다.
- E2E의 영상은 기존 fixture `report-video.mp4`이고, WebKit은 응답 기록에 206이 있는지까지 본다. 그림은 `naturalWidth`로 실제 디코드를 본다.

## 변형 확인

소스는 변형마다 백업에서 바이트로 되돌렸다(`cmp` 확인).

- 일반 파일 판정(`regular`)에서 mode 검사를 빼면 `links_are_not_documents_and_slices_stay_in_bounds`가 목록에 `docs/link.md`가 들어와 실패했다.
- 받은 commit의 조상 검사(`merge-base --is-ancestor`)를 빼면 `pinned_commit_reads_the_old_text_after_origin_moves`가 실패했다.
- 1분 간격 안에서 지난 받기 실패를 버리면 `fetch_failure_keeps_the_held_tree`가 실패했다.
- 기본 ref 고르기를 줄인 이름(`rev-parse --abbrev-ref origin/HEAD` → `origin/main`)인 옛 판으로 되돌리면 `tree_reads_origin_main_without_touching_checkout`가 로컬 branch의 옛 판을 읽어 실패했다. fallback 이름만 줄여서는 앞의 `symbolic-ref`가 온전한 이름을 주어 통과한다.

- 2-1: 버리기에서 origin branch를 지우지 않게 바꾸면 m13 `design_branch_saves_…`가 「host가 올린 origin branch도 지운다」에서 실패했다.
- 2-2: `design_writer`에서 thread의 `design`과 요청 branch 비교를 빼면 `design_chat_thread_saves_only_its_own_branch`가 다른 branch 저장이 거부되지 않아(NotFound까지 감) 실패했다.
- 2-2: 중계 목록(`thread_relay_allowed`)에서 `DesignSave`를 빼면 m13 `design_chat_thread_on_the_repo_machine_…`의 thread 안 `plan save`가 「host가 없다(front.sock)」로 실패했다.
- 2-2: m13을 처음 돌렸을 때 `plan discard`가 대화 thread의 [완료] 보고 Next(후속 할 일) 때문에 끝내기를 거부당했다. 대화 전용 thread는 후속을 모으지 않게 고쳤다(아이디어 thread도 같은 경로였다).
- 2-2 advisor 확인에서 찾은 것: agent hook 소켓 한도(256KiB)가 기획 글 한도(512KiB)보다 작아 다른 머신 대화 thread가 큰 문서를 저장하지 못했다(`thread_read`만 frame 크기까지). 옛 agent의 저장소 머신에서는 thread가 떠도 저장이 「host가 없다」로 실패했다(`design_chat.v1` 확인). 말한 직후 버리면 띄우던 thread가 버린 branch에 남을 수 있었다(버림 표지).
- 2-2: 띄우는 중에 버리는 단계를 더하자 버리기가 「준비·정리 작업 중」으로 실패했다. 상태가 열림이 된 뒤에도 기동 일이 thread 작업 잠금을 잠시 쥔다(상태만 보고 기다리게 했을 때도 같은 실패). 잠금을 기다려 끝내게 고쳤다. 이 단계는 실제로는 그 길(기동 일이 잠금을 쥔 동안 버림)을 지난다. 기록하기 전에 버리는 아주 짧은 틈(버림 표지 길)은 시험이 일부러 맞추지 못한다.
- 2-2: Front의 `@unverified` 표시를 되돌리면 m13 `design_chat_thread_on_the_repo_machine_…`의 세션 ID를 댄 `plan save`가 저장에 성공해(`design_saved`) 실패했다.

## 2-2 독립 리뷰에서 고친 것

- coordinator pane·떼어 낸 프로세스가 대화 thread의 `AAA_SESSION_ID`를 대면 actor가 `worker:<그 pane>`이 되어 그 branch에 쓸 수 있었다(보기로 줄이는 규칙이 2-2부터 쓰기 신원이 됐다). 알린 세션만으로 정한 thread 범위는 `@unverified`를 붙인다. 같은 꼴의 아이디어 thread 노트 쓰기도 함께 막힌다.
- 버리기 중의 말하기가 버리는 branch에 thread를 남기거나, 쉬던 branch면 배경 일의 다시 열기가 버린 branch를 되살리고 그 말을 지웠다: 말하기·버리기는 branch 잠금 안에서 보고, 배경 일은 기록이 있는 branch만 다시 연다.
- 버리기가 대화를 먼저 끝내 닫기가 멈춰도 대화가 끝나 있었다: 닫고 기록을 지운 뒤 끝낸다(끝내지 못하면 다시 버릴 때 마저).
- 도는 배경 일이 있으면 새 말의 「새로 띄우기」를 버렸고, 전달이 멈춘 thread를 600초 기다렸다: thread가 멈추면 돌아가고, 남은 말이 있으면 다시 돈다(하나도 못 보내고 세 번이면 까닭을 남김). 자리는 drop으로 놓는다.
- 다른 머신 저장이 host의 올리기 대기(최대 420초) 동안 25·30초에 「시간 초과」가 났다(host는 뒤에 저장): 저장 중계는 600초 기다린다.
- 쓰기 판정이 프로젝트와 대화 기록의 지금 thread를 보지 않았다: 둘 다 본다.
- E2E 120(아이디어 갱신)은 thread 저장을 pane 밖에서 `AAA_SESSION_ID=<thread pane>`으로 흉내 냈다: 위 수리 뒤 「자기 아이디어 노트만」으로 거부되어 실패했다. 그 저장을 실제처럼 thread pane 안에서(`post-probe`) 하도록 바꿨다. 처음에는 첫 말(「준비」)의 글과 Enter 사이에 저장 줄이 끼어 한 줄로 섞였다(host 기록으로 확인): 첫 말이 전달된 뒤(`pending` 비움) 친다.
- 재리뷰에서 더 고친 것: 쉼 판정이 대화 thread를 보류하는 사이(상태는 보류, 세션 닫기까지 thread 작업 잠금을 쥔다) 말하면 잇기가 잠금에 막혀 새 thread를 띄우고 앞 thread는 보류인 채 남았다. 잠금을 기다려 잇고, 그래도 잇지 못해 새로 띄우면 앞 thread를 끝낸다.
- 남긴 것: 다른 머신 저장은 host의 agent 메시지 비밀 가림을 지나 등록된 비밀 조각이 든 글은 `*`로 저장된다(로컬 저장은 그대로). 띄우는 중인 thread가 60초 넘게 잠금을 쥐면 버리기의 끝내기가 실패한다(다시 버리면 잇는다). 같은 머신 thread의 CLI가 pane 자손으로 판정되지 않는 harness라면 그 thread는 자기 문서를 쓰지 못한다(보기만). 재리뷰가 남긴 낮은 것: `plan say`는 branch 잠금을 잡으므로 올리기(최대 420초)·버리기 뒤에서 기다릴 수 있다. 대화를 끝내지 못한 버리기 뒤 다시 버리지 않고 같은 주제를 새로 열면 앞 thread를 가리키는 기록이 지워진다(그 thread는 쓰지 못하지만 끝내는 이가 없다).

## 2-1 독립 리뷰에서 고친 것

- `design_check`를 agent가 고칠 수 있는 프로젝트 설정에서 읽어 승인 없는 명령 실행이 됐다: routine 명령처럼 host.toml `[projects.design_check]`로 옮겼고, 기획 branch 기록도 host 상태 폴더로 옮겼다.
- 버리기가 남의 origin 작업을 지울 수 있었다(origin에서 꺼낸 branch, 남이 그 위로 올린 판): host가 만든 branch만, lease로 host가 올린 판일 때만 지우고, 다르면 아무것도 지우기 전에 멈춘다. 있던 로컬 branch는 남긴다.
- `--no-verify`는 prepare-commit-msg·post-commit·post-checkout hook을 막지 않는다: `core.hooksPath`를 없는 폴더로 둔다(시험이 세 hook을 심어 확인).
- 쓰다 실패하면 파일이 남아 그 뒤 저장·쉼이 막혔다: 되돌린다. worktree를 만들다 실패하면 그 자리만 치운다.
- 대소문자만 다른 이름·Windows 별칭(`a.md.` 등)·`.git` 조각이 기준 판 검사를 지났다: 경로 규칙과 대소문자 쌍 검사로 막는다.
- branch 이름으로만 worktree를 찾아 사람의 checkout에 쓸 수 있었다: 이 모듈의 자리인지 확인하고, 폴더가 사라진 기록은 정리한다.
- 올리기·쉼 실패가 붙박였고(대기 기록이 안 지워져 ticker가 잇지 못함), host 대기(120초)가 agent 검사(600초)보다 짧았다: 실패는 간격을 두고 다시, 끝난 대기는 지움, 검사 300초·host 올리기 대기 420초, ticker 일은 따로 띄운다.
- 쉼이 잠금을 기다린 사이의 저장을 무시했다: 잠금 안에서 쉼 창을 다시 본다. 다시 열 때 origin이 앞서 있으면 따라간다.
- 재리뷰에서 더 고친 것: 휴대용 root에서 만들다 실패한 잠긴 자리는 잠금을 풀고 치운다(git은 `--force` 하나로는 잠긴 자리를 지우지 않는다: 직접 확인). 다시 열며 따라간 남의 commit을 「올린 판」으로 삼아 버릴 때 지울 수 있었다: host가 저장한 commit(`own_head`)이 끝일 때만 올리고 그 판일 때만 지운다. hook 자리를 예측할 수 있는 임시 경로 대신 `/dev/null`·`NUL`로. 사라진 자리 정리가 저장소 전체를 prune했다: 그 기록만 `worktree remove`(폴더가 없어도 된다: 직접 확인)하고 잠긴 자리는 멈춘다. 엄격한 경로 규칙이 읽기까지 막았다: 쓰기에만. 큰 원래 파일 되돌리기(512KiB 상한)와 비ASCII 대소문자: `checkout`을 먼저, 유니코드 소문자 비교. 실패한 올리기 간격을 거듭 실패마다 늘리고(`design_branch::tests::failed_pushes_back_off`), 쉼 실패는 하루 뒤 다시. 디스크가 빠진 작업 root는 자리 판정에서 건너뛴다. 시험: agent `open_save_push_rest_and_discard`에 지워진 자리 다시 꺼내기, m13에 있던 로컬 branch는 버려도 남음.
- advisor 확인에서 더 고친 것: 올린 뒤 더 쓰고 올리기 전에 버리면 origin branch가 남았다(lease가 마지막 저장 판이라 맞지 않음): 올린 판에 lease를 건다(m13에 그 순서를 더함). hook 끄기의 OS 분기를 없앴다(agent 실행 파일 경로). 사람의 aaa 셸 pane CLI까지 막던 것을 thread·coordinator pane만 막게 했다. 휴대용 root 잠금 경로를 실제로 돌리는 시험을 더했다.
- 남긴 것: `git worktree list -z`는 Git 2.36 이상이다(thread 준비도 같은 조건). Windows에서 검사 명령은 `npm.cmd` 같은 이름을 스스로 찾지 않으므로 실행 파일을 그대로 적는다. m13은 Unix에서만 돈다. 새 쓰기 시험은 착지 뒤 GitHub Windows CI에서 처음 돈다(Windows 전용 코드는 없다, 1단계 git 실험실 시험은 Windows CI를 통과했다). coordinator pane을 거부하는 경로는 시험이 없다(실제 coordinator pane이 필요하다).

## 독립 리뷰에서 고친 것

- `log.showSignature=true`인 머신에서 서명된 commit이면 `show --format=%ct` 출력 앞에 서명 줄이 붙어 commit 시각을 읽지 못했다. git 호출마다 `-c log.showSignature=false`를 준다(`orch.rs`와 같은 처리). 서명 열쇠가 필요해 시험은 없다.
- origin 받기 실패가 웹에 보이지 않았고 1분 안에 다시 읽으면 까닭이 사라졌다. agent가 까닭을 기억하고 웹 저장소 줄 경고가 `fetch_error`·`truncated`도 보인다.
- 짧게 읽힌 조각을 그대로 보내 commit 주소로 1년 캐시될 수 있었다. agent와 host가 받은 길이를 확인한다.
- 조각마다 blob을 처음부터 풀어 큰 파일은 제곱 비용이다. 그림·영상 상한을 아이디어 그림과 같은 16MiB로 낮췄다.
- CLI front frame(1MiB)을 넘는 문서가 있어 글 상한을 512KiB로 낮췄다.
- 문서 「새로 읽기」가 host가 기억한 옛 판만 다시 읽었다. 트리를 다시 읽고 판이 바뀌면 문서도 다시 읽는다. host가 정리한 경로로 돌아온 문서가 빈 화면이던 것과 앞 문서 오류가 남던 것도 고쳤다.
- 같은 이름의 다른 저장소가 한 줄로 합쳐지는 것은 고치지 않았다: 웹 작업 폴더 트리도 경로 끝 이름이 같으면 한 저장소로 보는 규칙이다.

## 실제 실행(11-16, macOS)

- `verify.py develop --package aaa-proto --package aaa-agent --package aaa-host --spec 135,130,120`: Rust 시험 proto 57·agent 213·host 928 통과, strict clippy 통과, 웹 단위 723 통과, 공개 검사 통과(리뷰 수리 뒤 다시 돌린 결과도 같다).
- 이 Mac의 전역 Playwright가 1.58.2라 첫 E2E는 준비에서 멈췄다. 전역은 두고 1.64.1 npm 패키지만 작업 폴더 `target/` 아래에 두고 `NODE_PATH`로 돌렸다. 브라우저는 공용 캐시에 있던 판을 썼다.
- 첫 E2E 실행은 135의 마지막 단계에서 실패했다. 400 오류 응답에도 host 공통 CSP 머리가 붙는데 시험이 「없음」을 기대했다. 기대값을 상태·Content-Range만으로 고친 뒤 `ci-local --only web,e2e,public --spec 135,130,120`이 통과했다(135 화면 단계는 첫 실행에서도 네 조합 모두 통과).
- `aaa` crate: `--test deps`·`--bin aaa cli::`·strict clippy 통과. Linux·Windows strict cross clippy 통과.
- 화면 확인(검수): 실험실 host·agent와 origin이 있는 견본 저장소 셋으로 넓은 창·390px 휴대폰에서 트리·문서·그림·링크를 직접 봤다.

## 2-2 실제 실행(11-16, macOS)

- `verify.py develop --package aaa-proto --package aaa-agent --package aaa-host --package aaa --spec 135,130,120,131,122`(리뷰 수리 뒤 마지막 commit): Rust 시험 proto 58·agent 218·host 937·m13 104 통과, strict clippy 통과, 웹 단위 통과, E2E 120·122·130·131·135 통과, 공개 검사 통과. 그 전 판(리뷰 수리 전)에서도 같은 묶음이 통과했고, 「알린 세션」 수리 직후 판에서는 E2E 120이 위 까닭으로 실패했다.
- m13 `design_chat_thread_on_the_repo_machine_…`는 띄우는 중 버리기 단계를 더한 뒤 다섯 번 연달아 통과했다(한 번 7.5초 안팎, 리뷰 수리 전 코드).
- Linux·Windows strict cross clippy 통과(나온 unused dependency 경고는 손대지 않은 manifest의 기존 것).

## 2-2 착지 뒤 GitHub CI(main 120451b1)

- Windows `aaa-host --lib`의 `pairing_tests::the_web_keeps_its_last_controller_and_the_host_machine`이 스택을 넘쳤다(착지 gate의 Windows 검사는 cross clippy라 시험을 돌리지 않는다). 이 Mac의 dev 시험(기본 2MiB)에서도 같은 시험이 넘쳤다. `RUST_MIN_STACK`으로 잰 필요 스택은 앞 main(e1742763) 1,920~2,048KiB, 2-2 뒤 2,048~2,304KiB였다. 앞 main도 2MiB의 마지막 128KiB 안이었다.
- 원인: 기획 버리기가 대화 thread 끝내기(`thread_resolve` future, 16,224바이트)를 그대로 품어 `call_design` future가 3,880 → 20,048바이트가 됐다. `Host::call_from`은 `Box::pin` 전 그 값을 자기 poll 스택에 두므로 모든 요청의 스택이 커졌다(debug 시험 바이너리의 call_from poll 틀 0x12e000 → 0x133000).
- 고침: 대화 thread 끝내기 future를 heap에 두고(`call_design` 4,056바이트), 큰 지역 값을 여럿 쥐는 thread 시작 갈래도 heap의 future로 따로 뒀다(call_from poll 틀 0x114000). 필요 스택은 1,792~1,920KiB가 됐다. `design::tests::design_requests_keep_the_dispatcher_stack_small`이 기획 요청 future를 8KiB 아래로 지킨다(heap에 두지 않게 되돌리면 20,048바이트로 실패).
- 남은 위험: 공용 dispatcher의 debug poll 틀은 여전히 1.1MiB다(갈래마다 임시 값이 겹치지 않고 쌓인다). 큰 갈래를 더 넣을 때는 갈래 안에서 heap에 둔다.
- 같은 run의 macOS web-e2e 109 「화면 복귀: Chromium 1280 … coordinator 본문이 되지 않았다」: 2-2의 웹 변경(`chatOnly`)은 기획 thread가 없는 화면에서 이전과 같고(시험 실험실에는 기획 대화가 없다), 화면 고르기·pane 경로는 바꾸지 않았다. 고친 판으로 109를 이 Mac에서 8번 돌려 8번 통과했다(부하 3 안팎, CI는 18~22). 원인은 찾지 못했고 재현하지 못했다.

## 2-3 웹 기획 화면

- 시안 검수(q-3933 1A) 뒤 그림 붙이기와 시험을 더했다. 시안 촬영 도구(실제 host·agent·git과 가짜 Claude 기획 대화)는 저장소 밖에 두었다.
- 촬영 중 찾아 고친 것: 대화 사건이 그 말 앞에 놓였다(host가 말을 다 보낸 뒤에 자리를 적어, 받자마자 저장한 사건이 자리 없이 맨 앞으로 갔다): 보내기 전에 적는다. 새 대화 사건의 main 판이 `origin/`으로 보였다(agent의 `base`는 ref 이름): host가 main에서 만든 branch의 처음 끝을 쓴다.
- 시험을 쓰다 찾은 것: 원격 대화 m13의 쉼 창 1초가 ticker의 이른 깨움(git·nudge 차례)으로 아무 단계에서나 branch를 쉬게 했다(앞 판은 우연히 지났다). 그 시험은 쉼 창을 기본으로 두고, 쉼 뒤 말하기는 쉼이 언제 와도 되는 로컬 시험으로 옮겼다. `plan apply`의 thread profile은 aaa.toml이 있으면 그 설정을 따른다(E2E 준비가 aaa.toml에 thread profile을 빠뜨려 허용 밖 profile로 거부됐다).
- 2-3 독립 리뷰에서 고친 것: 사람이 고친 뒤 웹에서 보낸 말이 대기 말로 남았다(host가 「(문서 고침: …)」을 붙여 보낸 말 확인이 글과 맞지 않았다): 웹이 보낼 글에 붙이고 host는 붙어 있으면 다시 붙이지 않는다. 그림을 올리는 동안 친 글이 사라졌다: 올리는 동안 글을 읽기만 하게 하고, 올린 줄은 지금 글(같은 branch일 때만)에 넣으며, 여럿 중 하나가 실패해도 올린 것만큼은 넣는다. branch에 없는 문서를 main 판에서 고치면 저장할 수 없었다(기준 판이 main blob): 그 branch에 새로 만든다. 같은 그림을 다시 붙이면 거부됐다: 이름이 내용 hash라 같은 이름·같은 내용은 성공(새 commit 없음). 붙인 그림도 다음 말 앞에 알린다. 조각 자리 이름에 저장소를 넣었다. `main`이라는 주제는 판 고르기와 겹쳐 쓰지 않는다(host·웹 모두, 웹 꼴은 `--`도 막아 host와 같게). 긴 코드 울타리(````) 안의 ```로 덩어리가 끊기지 않는다. 반영 직전에 새 저장이 들면 까닭을 말한다. 대화 사건 상한을 100으로 줄였다(branch 보기마다 실린다).
- 남긴 것(리뷰 낮음): 대화 사건 자리는 불러온 기록 안에서만 찾고, 앞부분 80자가 비슷한 다른 말에 붙을 수 있으며, 대화 thread를 새로 띄우면 앞 사건은 맨 끝으로 간다. 그림을 넘기는 동안(조각 최대 32개) 그 branch 잠금을 쥔다. 끊긴 조각 자리 파일은 다음 같은 그림이 덮어쓸 때까지 남는다. branch 판은 덩어리마다 따로 그려 참조식 링크·각주·여러 문단 목록이 깨질 수 있다. 반영을 맡긴 뒤의 저장은 그 반영에 들지 않는다.

## 3-1 블록 표지

- 시안 검수(q-3949 1A) 뒤 시험을 더했다. 촬영 도구(실제 host·agent·git, 할 일 넷과 thread 하나)는 저장소 밖에 두었다.
- 촬영 중 찾아 고친 것: 말풍선 위 맥락 칩이 고정폭 글꼴이라 한글 간격이 벌어졌다(이름은 보통 글꼴, 경로만 고정폭). 「지금 모습」에서 블록만 남은 절의 제목이 빈 채 남았다(확정 계획대로 숨긴다). 글 속 `d-NNNN` 칩이 빠져 있었다.
- 시험을 쓰다 찾은 것: agent의 표지 수가 `git grep` 줄만 보아 코드 울타리 안 표기 예시와 인용 중간 줄도 셌다(웹은 덩어리 첫 줄만 블록). 표지가 든 문서의 글을 웹과 같은 덩어리 규칙으로 센다. 폴더·저장소 합계가 같은 d-ID를 여러 번 셌다(확정 계획은 한 번).
- 계약 시험: agent `design::tests::marks_count_only_callout_blocks_like_the_web`(울타리·짧은 울타리·물결 울타리·인용 중간 줄·섞인 덩어리·붙여 쓴 d-ID)·`tree_counts_marks_per_document`(실제 origin의 트리, 기획 폴더 밖·울타리 안 예시는 0). 웹 단위 `블록 표지(3-1)`(블록 판정·절·할 일 상태 일곱 갈래·합계·맥락 칩). E2E 136 「블록 표지」 단계: 실제 할 일 셋(thread가 맡음·기다림·끝남)과 없는 d-9999로 트리 수·「할 것」·상태 칩과 그 링크·글 속 칩·보기 셋·남은 것의 절 표시·「대화로 정하기」 칩을 본다.
- 독립 리뷰에서 고친 것: 웹과 agent의 표지 판정이 `[!미정] d-N`(웹만 할 일로 봄)·CRLF(웹만 표지 아님)·짧은 번호 `d-12`(정규형이 아님)에서 달랐다. 두 쪽을 같은 글자 규칙으로 바꾸고 공용 견본(`web/test/fixtures/design-marks.md`, LF·CRLF)을 두 시험이 함께 읽는다. 표지·그림·제목 판정 정규식이 긴 줄(빈칸 20만 개, `![` 10만 번)에서 몇 초씩 탭을 멈췄다: 되돌기 없는 글자 읽기로 바꿨다. 머신의 `color.grep=always`가 파일 이름을 칠해 수가 모두 0이 됐다(`--no-color`). host가 기획 branch마다 읽는 트리도 표지를 셌고, 느린 세기가 트리 전체 시한을 썼으며, 표지 든 문서를 모두 메모리에 모았다: branch 트리는 세지 않고, 세기는 5초 시한, blob은 읽는 대로 센다. 세지 못한 것이 0과 같아 「할 것」이 문서를 모두 숨겼다(`DesignTree.marks`). 보관 찾기가 부분 일치 다섯 개만 보고, 읽지 못하면 「할 일 없음」으로 그렸으며, 할 일이 바뀌어도 다시 읽지 않았다. 「남은 것」이 같은 이름의 절을 하나로 묶었다.
- 후보(c696a75a)를 낸 뒤 찾은 회귀: main 판 문서도 덩어리마다 그려, 빈 줄 뒤 목록의 이어지는 문단이 목록 밖으로 나오고 느슨한 번호 목록이 항목마다 따로 목록이 됐다(2-3까지 main 판은 문서 전체를 그렸다). 앱 Markdown 해석기로 통째·덩어리를 견주면 YAML 머리·표·인용 여러 문단·코드 울타리·들여 쓴 코드는 같다(참조식 링크·각주는 해석기가 처음부터 지원하지 않는다). 고침: 표지가 없으면 문서 전체를, 있으면 표지 블록 사이의 보통 글을 이어 한 번에 그린다. E2E 136이 목록 안의 이어지는 문단을 본다(덩어리마다 그리면 실패). 남은 한계: 표지 블록을 사이에 둔 구성(표지 앞뒤로 이어지는 목록)은 나뉘고, branch 판은 main과 다른 덩어리를 표시하려고 덩어리마다 그린다(2-3과 같다).
- 실행하지 못한 것: 「표지 수를 세지 못해 모두 보인다」 줄(세기 실패·옛 agent)은 시험·촬영에서 그려 보지 못했다. 새 `git grep` 세기 시험은 착지 뒤 GitHub Windows CI에서 처음 돈다. agent 시험 둘과 E2E 136 「할 것」 단계는 세기가 5초 시한 안에 끝나는 데 기댄다.

## 3-2 구현 맡기기

- 시안 검수(q-3956 1A·q-3957 2A) 뒤 host 흐름과 시험을 더했다. 촬영 도구는 저장소 밖에 두었다.
- 시험을 쓰다 찾은 것: 맡김 할 일의 머신 칸을 비워 두니 예약이 「자동 배정이 허용된 연결 머신이 없다」로 자리를 기다리기만 했다. 머신 칸에 그 문서 저장소의 머신을 적는다(창의 「프로젝트 기본 · <머신>」과 같다).
- 계약 시험: proto `handoff_marks_only_the_picked_plan_blocks`(공용 견본 LF·CRLF: 고른 블록만 d-ID, 줄 수·CR 그대로, 미정·이미 넘긴 것·없는 차례·빈 고르기는 거절, 절 이름). host `design_handoff::tests`(brief 글, 기록 파일 이름 꼴, 하위 할 일로 읽힐 메모 줄은 인용). E2E 136 「구현 맡기기」 단계: 실제 host·agent·git으로 기획 branch 판의 「남은 것」에서 둘을 골라 창(자동 제목·메모·예약 기본)으로 맡기고, 할 일·두 표지의 같은 d-ID·바로 올림·`handoff` 사건·화면 칩과 대화 줄을 본 뒤, 예약이 띄운 구현 thread의 brief(맡긴 자리)와 worktree가 기획 branch 끝을 조상으로 갖는지 본다.
- 독립 리뷰에서 고친 것: 창이 연 뒤 다시 읽힌 문서의 판과 옛 고르기 번호를 함께 보내 다른 블록에 표지를 달 수 있었다(창을 열 때 판·블록을 고정하고, 블록 첫 줄도 보내 host가 같은 블록인지 다시 본다. 웹과 같은 빈칸 규칙(BOM·U+0085)으로 덩어리를 나눈다). 읽기와 표지 commit 사이에 대화 thread의 저장이 끼면 할 일만 남았다(그 branch 잠금 안에서 읽기·할 일·기록·표지 commit까지 하고, 그래도 commit에 실패하면 할 일과 기록을 지우고 거절한다). 올리지 못한 판에서 thread가 갈라질 수 있었다(기록에 표지 commit과 그 때를 두고, origin에 그 뒤 올린 것이 확인되기 전에는 시작을 미룬다. 예약은 대기에 남는다). 맡김 기록을 번호로만 찾아 버린 branch·새로 연 같은 이름 branch·번호가 다시 쓰인 다른 할 일에 붙을 수 있었다(할 일의 바뀌지 않는 ID·저장소·branch 만든 때를 대조하고, 버리기가 그 branch의 기록을 지운다). `[!계획]` 뒤에 글이 붙은 줄은 d-ID가 숫자와 이어졌다(띄운다). 그 사이 새 저장이 올라가면 올림을 실패로 보았다(`pushed == head`). 저장소 항목에 머신이 없으면 머신 칸이 비었다(해석한 머신). 쉬는 branch는 할 일을 만들기 전에 거절한다.
- 계약 시험 더함: proto(붙은 표지 띄우기, BOM·U+0085 빈칸), host `start_forks_from_the_design_branch_only_for_its_task_after_the_push`(올리기 전 미룸·commit 전에 올린 판·올린 뒤 갈라짐·기준을 준 시작·다른 할 일·다른 저장소·새로 연 branch·버리기 정리).
- 후보(0a0e49d0) 뒤 더한 시험: host `handoff_task_is_added_and_removed`(맡김 할 일 더하기·같은 제목 거절·표지를 적지 못했을 때 지우기)·`pending_record_defers_then_expires`, E2E 136에 「창을 연 뒤 문서가 바뀌면 맡기지 않고 할 일도 만들지 않는다」(창은 연 때의 판을 보내고 host가 거절). 기록 뒤 표지 commit 전에 host가 멈추면 그 할 일이 계속 미뤄졌다: 10분 넘게 commit 없이 남은 기록은 끝나지 못한 맡김으로 보고 평소처럼 시작한다. 아직 실행하지 않은 갈래: 맡김의 올리기 실패(경고·예약 안 함), 표지 commit 실패(할 일 지우기 경로는 host 시험으로 따로 봄), 쉬는 branch 거절.

## 3-3 넘긴 블록 고침 알림·main 문서에서 바로 맡기기

- 시안 검수(q-3964 1A) 뒤 시험을 더했다. 촬영 도구(실제 host·agent·git, 가짜 Claude)는 저장소 밖에 두었다.
- 촬영 중 찾아 고친 것: 막 뜬 구현 thread는 brief를 받기 전이라 고침을 보내지 못했는데, 기록의 블록 글을 먼저 바꿔 두어 다음 저장에도 다시 알리지 않았다(brief 전달까지 thread 사건마다 다시 보고, 보낸 뒤에만 기록을 바꾼다). 같은 할 일의 바뀌지 않은 블록까지 보냈다(바뀐 블록만). 시험을 쓰다 찾은 것: main 판에도 맡기기 단추가 생겨 기획 branch 판으로 바뀌기 전에 단추를 세던 E2E를 판이 바뀐 뒤에 세게 했다.
- 계약 시험: proto 블록 글·끝 줄. host `main_handoff_notes_start_and_relay_without_thread`(살아 있는 할 일의 main 기록만 싣기, main 시작 brief·기준 그대로, thread 없는 고침은 글만 바꿈)·`only_changed_blocks_are_relayed`. 웹 단위 사건 줄 글. E2E 136 「넘긴 블록 고침 알림」(3-2 단계가 맡긴 블록을 사람이 고치면 올린 뒤 `relay` 사건)·「main 문서에서 바로 맡기기」(main 판 창·어느 branch에도 commit 없음·칩·다시 맡기지 않음·예약한 구현 thread brief의 첫 commit 안내와 블록 글).
- 실행하지 않은 것: 구현 thread가 실제로 첫 commit에 표지를 적고 착지해 main 칩이 표지로 바뀌는 흐름(가짜 harness), 고침 알림의 10분 시한 만료.


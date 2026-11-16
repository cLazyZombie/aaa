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
| 2-2 기획 대화 thread는 보기 권한·같은 머신 pane 모두 자기 branch 저장만 지나가고(commit 저자 표시는 thread ID), 다른 branch·다른 thread·열기·버리기·말하기는 거부, 중계 목록은 저장만 싣는다 | `aaa-host` `design_branch::tests::design_chat_thread_saves_only_its_own_branch` | `cargo test -p aaa-host --lib design_branch` |
| 2-2 원격 중계는 기획 열기·말하기·버리기를 싣지 않는다 | `thread_read::tests::relay_checks_machine_live_session_and_explicit_read_allowlist` | `cargo test -p aaa-host --lib thread_read` |
| 2-2 아이디어·기획 대화 thread의 [완료] Next는 후속 할 일이 아니고 CLI 끝내기를 막지 않는다 | `followups::tests::chat_only_threads_collect_no_followups` | `cargo test -p aaa-host --lib followups` |
| 2-2 대화 thread 할 일 글이 branch·worktree·checkout·읽기·저장 명령을 가리킨다 | `design_chat::tests::task_text_points_at_branch_worktree_and_commands` | `cargo test -p aaa-host --lib design_chat` |
| 2-2 실제 host·agent 둘(TLS 원격 labB)·Git과 CLI: 저장소가 labB에 있으면 `plan say`가 labB에 대화 thread를 띄우고(할 일 없음) brief 뒤 말을 보내 `pending` 0, front.sock 없는 thread 세션이 agent 중계로 `plan save`(자기 branch)·`show --branch`·`list`, 다른 branch 저장은 「자기 branch」 거부, 열기·말하기·버리기는 CLI가 보내지 않음, commit 메시지 끝이 thread ID, 저장소 checkout 그대로, 올린 기획 branch에서 `--base origin/design/<주제>`로 시작한 worktree thread가 그 commit과 문서를 가짐, 버리면 대화 thread가 끝나고 origin branch까지 지움 | `aaa` m13 `design_branch::design_chat_thread_on_the_repo_machine_saves_only_its_own_branch` | `cargo test -p aaa --test m13 design_branch` |

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

# 자주 깨지는 시험 전수 정리(t-1266)

## 수집 범위와 빠진 자료

- GitHub main CI: 2026-10-27 00시~10-28 08시(UTC) 실패 22회의 job·공개 annotation. 그 뒤 main CI는 다음 push로
  모두 취소돼 끝난 판이 없다. 그 전 기간은 공개 API 시간당 한도 때문에 받지 않았다.
- 로컬: git 기록(10-18~), `docs/testing*.md`의 수리 기록, 검증 Mac에 남은 착지 폴더의 ci-local 로그.
- 빠진 것: 검증 Mac은 host가 아니어서 `aaa search`·inbox의 ci 항목·다른 머신의 착지 원문을 읽지 못했다.
  host 머신의 착지 실패는 할 일 메모와 commit 기록으로만 셌다.
- 표에는 실패만 둔다. libtest의 「60초 넘게 도는 중」 경고, 컴파일 줄, 실패가 아닌 로그 덤프는 뺀다.

## 실패 표

| 시험 | 횟수·곳 | 조건 | 처리 |
| --- | --- | --- | --- |
| install `wizard::both_wizards_back_from_host_choices_then_submit_the_same_agent_plan_as_explicit_cli`, `wizard::explicitly_saved_private_draft_never_overrides_cli_or_silently_fills_an_automated_run` | CI macOS 1(06c77f26) | 승인 PTY 60초 상한 | 이 기록 A |
| install `authorization::private_tty_enrollment_waits_on_the_real_tls_connection_and_codes_never_enter_public_events` | CI macOS 1 | exit 2 `work_space_insufficient` | 이 기록 A |
| install `recovery::closed_enrollment_window_keeps_the_id_and_action_after_files_are_applied` | CI macOS 1, Windows 1 | macOS는 exit 2 `work_space_insufficient`, Windows는 PTY 정지 | macOS는 A, Windows는 d-0490([t-1246](testing-install-pty-t-1246.md)) |
| install `authorization::external_tty_reviews_the_same_real_plan_and_issues_private_bound_authorization` | CI Windows 1 | 승인 입력 뒤 PTY 정지 60초 | d-0490 |
| m13 `claude_menu_dismisses_escape_before_waiting_for_its_suffix` | CI Ubuntu 1(10-27, 셸 fixture 30초), macOS 1(10-28, EIO) | 부하 | 이 기록 B |
| m13 `ssh_install::ssh_install_through_the_system_sshd_starts_a_launch_agent_that_connects_and_checks` | CI macOS 1 | 원격 계획 exit 4 | 남김(아래) |
| agent `diff::tests::full_context_is_file_only_bounded_and_keeps_default_patch_small` | CI Ubuntu 1(4d09d7f3) | 「원본 저장소를 바꾸면 안 된다」 | 이 기록 C |
| E2E77 「보고서 본문의 ID도 링크, 삭제·자동 보관된 할 일은 없음과 일반 글」 | CI 웹 macOS 1(2b75d22b) | `The expression evaluated to a falsy value` | 이 기록 D |
| E2E16 긴 소스 가상화 | CI 웹 4 | 20초·5초 상한 | t-1209(b233a489) 뒤 재발 없음 |
| host `rollout::web_tests` 두 시험 | CI Windows 4(10-27 연속) | Windows 경로 형식 BadRequest(결정적) | t-1183(2988273b, 원격 rollout 경로 문법) 뒤 재발 없음 |
| common `child_process::tests::pipe_window_*`·`subprocess_fixture` | CI macOS 3 | SIGILL | t-1137·t-1146 수리 뒤 재발 없음 |
| m13 `accounts::renamed_account_starts_with_the_same_auth_without_login_and_rejects_while_running` | CI macOS 2 | 첫 이름 변경이 빈 자동화 조회 잠금과 겹침 | t-1152(a79dfbdd) 뒤 재발 없음 |
| host `link::response_parts_tests::large_report_and_bad_parts_keep_requests_on_one_link` | CI macOS 2 | 5초 상한, 조각 경계 가림 3.9~4.4초 | t-1216(adc8a091, 가림의 해시 검색 제거) 뒤 재발 없음 |
| m13 `task_memory_forgets_deleted_project` | CI macOS 1 | — | t-1172(10-27 16:40)가 host unit으로 옮김 |
| E2E 한글 조합·PWA 대화 focus·PageUp/PageDown·모델 카드·화면 복귀·Reader 설정 | CI 웹 각 1 | — | 한글 조합은 t-1151, 화면 복귀·Reader는 425e602a. 나머지는 수리 commit 미확인, 그 뒤 성공 6판에서 재발 없음 |
| macOS CI test 단계 15분 초과 | CI macOS 3 | 재컴파일 | [t-1221](testing-macos-ci-budget.md) |
| deploy_start host 재시작 5초 | 착지 1 | load 36~45 | t-1244([기록](testing-deploy-start-reconnect.md)) |
| db_backup 정기 백업 5초 | 착지 1 | load 36 | [t-1238](testing-db-backup-ticker-boundary.md) |
| resource_limits web listen | 착지 1 | load 45 | t-1244([기록](testing-resource-web-ready.md)) |
| preparation 중단 기동 회수 | 착지 1 | load 42 | [t-1251](testing-preparation-recovery.md) |
| E12 thread 목록 reload | 착지 여러 번 | WebKit·Chromium | t-1225·[t-1232](testing-thread-events-liveness.md) |
| E42 thread-diff·m13 질문 이력 | 착지 | — | [t-1236](testing-claude-menu-input-batch.md) |
| E72 machine-add 30초 | 착지 1 | load 132 | [t-1238](testing-machine-plan-boundary.md) |
| E103 commit 응답 정지 | 착지 1 | load 95 | t-1216 |
| idle-audit Windows stack overflow | — | — | t-1208 |
| host `backup::tests::wal_backup_restores_auth_history_and_rotates_only_its_files` | 착지 1 | 백업 중복 실행 잠금 | t-1263(0a206eca, 백업 owner 종료 때 Unix 잠금 해제) 수리 |

60초 넘게 돈 시험(실패 아님): rollout 세 시험(12회), `coordinator_deployment_is_gated_and_survives_backend_restarts`(5),
accounts 두 시험(5·3), image(2). 개별 시험 5초 기준 작업(t-1117/d-0385)에 넘긴다.

## A. install 실험실의 디스크 여유 기준

### 원인

- `Lab::args`는 `--min-free-gb`를 주지 않았다. 계획은 `aaa-config` install 정규화의 기본 20GB를 쓰고
  `detect.rs`는 작업 root 볼륨의 남은 공간이 그보다 적으면 `work_space_insufficient`(exit 2)로 끝낸다.
- GitHub 문서의 `macos-15`(arm64) 표준 러너는 CPU 3·메모리 7GB·저장 공간 14GB다. 20GB 여유는 보장되지 않고,
  빌드·병렬 시험이 디스크를 쓰는 동안 남은 공간이 그 경계를 오간다. 원 실행의 `df`는 보지 못했으므로
  실패 순간의 남은 공간 숫자는 단정하지 않는다. 근거는 같은 오류 코드·종료 코드와 아래 경계 재현이다.
- 승인 PTY 시험은 승인 물음 전에 exit 2로 끝난다(`private_tty_enrollment…`, `closed_enrollment_window…`).
- wizard 시험은 「남겨 둘 여유 GB」에 20을 답했다. 현지 검사가 실패하면 wizard는
  「현지 검사를 통과하지 못했습니다」 뒤 「선택을 바꿀까요(:back / :cancel)」를 묻는다.
  시험은 다음 물음(「harness 로그인 검사」)을 기다리다 60초 상한에 걸린다.
- `recovery.rs`의 외부 콘솔 시험 하나만 이미 0을 답해 이 경계를 피하고 있었다.

### 수리

- `Lab::args`가 `--min-free-gb 0`을 준다. 직접 실행·`--resume`·dry-run 계획은 러너 디스크와 무관해진다.
- wizard가 그 물음을 묻는 경로는 `Lab::args_asking_free_space`로 이 값을 빼고 0을 답한다
  (wizard 두 시험, `worker_lifetime::approved_worker_outlives_its_cli_and_records_the_same_id_after_private_display_disappears`).
  명시 argv와 wizard 답이 같은 0이므로 plan hash 비교 계약은 그대로다.
- `install_reads_local_overrides_and_preserves_existing_installations`는 기존 agent 설정의 값이 계획에 들어가는지
  봐야 하므로 같은 helper를 쓰고, 설정 값 7·9를 0·1로 바꾼다. 두 값이 달라 hash가 바뀌는 계약만 본다.

### 유지한 계약

- 제품 기본 20GB·`work_space_insufficient` 판정·wizard 재시도 물음은 바꾸지 않았다. 이 거부 경로를 단언하는
  기존 시험은 없었고, 이 일에서 새로 만들지 않는다.
- wizard가 「남겨 둘 여유 GB」를 묻고 답을 계획에 넣는 경로, 초안 저장·명시 argv 우선·같은 planner/hash 단언,
  시한·재시도·단계 수는 그대로다. 시험 삭제는 없다.
- 영향 시험은 Windows CI에서도 돈다. 착지 전에 Windows 실기 또는 diagnostic CI 한 번이 필요하다.

### 재현(시험 실행 아님)

설치된 `aaa 0.1.6+ef0ccf77`을 격리한 임시 실험실(HOME·설정·상태·instance 분리)에서 돌렸다.
`git diff ef0ccf77 HEAD -- crates/aaa/src/install crates/aaa-config/src/install.rs`는 비어 있어 같은 install 코드다.

| 실행 | 결과 |
| --- | --- |
| `install --dry-run --json`, 기준 0·20·520GB, 기준 없음 | `exit_code 0`, `planned` |
| 같은 실행, 기준 99999GB | `exit_code 2`, `work_space_insufficient`(CI와 같음) |
| `--prompt plain` PTY wizard, 여유 답 99999 | 「현지 검사를 통과하지 못했습니다: work_space_insufficient」 뒤 「선택을 바꿀까요」에서 멈춤 |
| 같은 wizard, 여유 답 0 | 「계획을 검증했습니다」와 plan_sha256 출력 |

## B. Claude 메뉴 Esc 시험의 PTY 쓰기 경쟁

### 원인

- t-1236의 `claude-menu.py`는 Esc를 읽으면 질문을 지우고 Working을 그린 뒤 suffix를 0.2초만 기다린다.
  더 오지 않으면 `cancel`로 끝나고 셸도 바로 끝난다.
- 시험은 Working을 본 뒤 `x`를 썼다. 부하로 그 쓰기가 0.2초 창을 넘기면 자식은 이미 끝났고,
  PTY master 쓰기가 EIO(`Os { code: 5 }`, `lab/mod.rs`의 `write_all`)로 실패한다.
- `x`는 어떤 단언에도 쓰이지 않는다. 늦든 이르든 결과는 `pick:<cancel>`이다.
- 10-27 Ubuntu 실패는 t-1236 전의 셸 fixture에서 30초 출력 상한으로 끝난 다른 모양이다.

### 수리

- 두 번째 키(`x`)를 보내지 않는다. fixture의 원래 0.2초 suffix 판별로 끝난다. 이 창을 늘리면 Esc로 답하는
  다른 흐름이 모두 느려지므로 늘리지 않는다.
- 출력에 Working 표지가 없으면 실패한다. 전에는 `split(...).next()`가 늘 성공하고 `before`가 Working 줄을
  스스로 덧붙여, fixture가 표지를 내지 않아도 통과할 수 있었다.

### 유지한 계약

- 질문 즉시 지움·suffix 판별 중 `Working`·`visible_working` 감지·`pick:<cancel>`·30초 상한은 그대로다.
- fixture의 200ms 상한과 다른 메뉴 시험은 바꾸지 않았다. Unix m13에만 있는 시험이다.

### 재현(시험 실행 아님)

Python PTY로 같은 fixture를 띄워 Esc를 보내고, Working을 본 뒤 계속 읽으면서 쓰기를 늦췄다.

| Working 뒤 쓰기 지연 | 결과 |
| --- | --- |
| 0초·0.1초 | 쓰기 성공, `pick:<cancel>` |
| 0.3초(2회)·1.0초 | 자식 종료 뒤 쓰기 `errno=5`(EIO), `pick:<cancel>` |

출력을 읽지 않고 기다리면 재현되지 않는다. macOS는 tty를 마지막으로 닫을 때 읽히지 않은 출력이 빠질 때까지
기다려 자식이 끝나지 않는다. lab의 reader thread처럼 계속 읽어야 같은 순서가 된다.

## C. diff 시험의 원본 불변 단언과 Git 자동 maintenance 잠금

### 원인

- `diff::tests`의 `Snapshot`은 index 바이트·HEAD·ref와 `.git` 안을 포함한 모든 파일의 경로·내용을 비교 전후로 잡는다.
  시험 준비의 `Repo::git`은 자동 maintenance를 끄지 않았다. 제품 `git_prefix`는 `gc.auto=0`·`maintenance.auto=false`로 이미 끈다.
- `git commit`은 끝에 `git maintenance run --auto --quiet --detach`를 띄운다. Git 2.55의
  [6e95b07e5 「builtin/maintenance: fix locking with "--detach"」](https://github.com/git/git/commit/6e95b07e5)부터
  `objects/maintenance.lock`의 소유권이 분리된 자식으로 넘어가, 자식이 자동 조건 검사를 마칠 때까지 잠금 파일이 남는다.
  `git commit`은 이 자식을 기다리지 않는다. 2.54까지는 부모가 끝날 때 이 파일을 지웠다.
- GitHub Ubuntu 24.04 러너는 Git 2.55다. 자식이 부하로 늦으면 비교 전 snapshot에만 0바이트 잠금 파일이 잡히고,
  비교 뒤에는 사라져 단언이 틀린다. 검증 Mac의 Apple Git 2.54에서는 같은 순서가 생기지 않는다.
- 나눈 가설과 판정:
  - 제품 쓰기: 비교는 임시 index·임시 객체 폴더·`--no-optional-locks`로만 git을 부른다. 아래 재현의 바뀐 항목은 index·HEAD·ref·작업 파일이 아니었다.
  - Git 읽기의 index stat 갱신: 실제 index 바이트는 바뀌지 않았다.
  - 병렬 시험 격리: 저장소는 시험마다 새 ID 폴더이고 aaa-agent 시험은 환경 변수·현재 폴더를 바꾸지 않는다. 단독 반복에서도 같은 항목만 바뀐다.
  - 시험 자신의 commit이 남긴 배경 maintenance 잠금: 확정.

### 수리

- `Repo::git`이 제품과 같이 `-c maintenance.auto=false -c gc.auto=0`을 준다. 배경 작업 자체가 생기지 않으므로 기다릴 사건도 없다.
- 실패하면 바뀐 항목(index 길이·HEAD/ref·파일 경로와 길이)을 단언 문구에 남긴다. CI 원문에는 바뀐 경로가 없었다.
- 원본 불변 계약(index·HEAD·ref·`.git` 안 포함 전체 파일), 시한, 다른 단언은 바꾸지 않았다. 같은 helper를 쓰는
  `branch_and_uncommitted_read_every_change_without_touching_the_repo`도 같은 commit 경로라 함께 보호된다.

### 재현과 확인

같은 소스 판의 Git 2.55.0을 임시 폴더에 빌드해 PATH 앞에 두었다. 시스템 git은 바꾸지 않았다.

| 조건 | 결과 |
| --- | --- |
| Git 2.55, commit 직후 잠금 파일 관측(부하 없음) | 30/30 관측, 약 0.1~0.2ms 남음 |
| 같은 관측, `-c maintenance.auto=false` | 0/30 |
| Apple Git 2.54, 같은 관측(부하 없음·바쁜 프로세스 30·60개) | 0/40·0/60·0/200 |
| 수리 전 시험, Git 2.55, 부하 없음 40회 | 40 통과 |
| 수리 전 시험, Git 2.55, 바쁜 프로세스 60개 아래 8 worker×20회 | 첫 실행 60/160 실패, 다음 세 번 0/160(빌드 직후 첫 실행 지연과 겹친 것으로 보며 단정하지 않는다) |
| 수리 전 시험, Git 2.55, `GIT_EXEC_PATH` 감싸기로 maintenance만 `nice -n 20`, 같은 부하 | 63/160 실패, 63건 모두 `.git/objects/maintenance.lock: Some(0)→None` |
| 수리 뒤, 마지막 조건 두 번 | 0/320 |

감싸기는 분리된 자식이 다른 일에 밀려 늦게 끝나는 CI 상황을 우선순위로 흉내 낸 결함 심기다. 원 CI 실행의 실제 지연 원인은 원문이 없어 단정하지 않는다.
바쁜 프로세스만으로는 이 Mac에서 재현이 들쭉날쭉했다(60/160 뒤 0/160 세 번). 재현 기준은 감싸기 조건이다.

## D. E2E77 완료 보관 단언과 제자리에 쓰는 fake 보고

### 원인

- 원 CI 로그는 단언 첫 줄에서 잘려 식이 없었다. 아래 결함 심기에서 같은 문구로 실패한 식은
  `assert.ok(aaa("task", "archive").entries.some(e => e.task.short_id === other))`다.
- 공통 fake는 `cp`로 `report.md`를 제자리에 쓴다. agent는 kqueue 쓰기 사건마다 hash를 다시 읽으므로,
  파일이 만들어진 직후의 빈 판도 `report_hash`로 기록될 수 있다.
- 시험은 `report_hash`가 있기만 하면 넘어가 그 순간의 후속(followups)을 처리하고 resolve했다. 그 뒤 `[완료]`와
  Next(「결과 확인」)가 담긴 마지막 판이 들어오면 처리하지 않은 후속이 생긴다. host는 계약대로 완료 보관을 막는다.
- 제품 결함이 아니다. 후속을 판마다 기록하고 처리되지 않은 후속이 있으면 보관하지 않는 동작은 의도된 계약이다.
  시험이 자신이 기대는 마지막 보고를 기다리지 않은 준비 순서 결함이다.

### 수리

- `report-fixture.md`의 sha256과 thread의 `report_hash`가 같아질 때까지 기다린다. 앞 단계의 링크 본문 대기와 같은 방식이다.
- 후속 처리·resolve·`ticker tick`·보관 단언과 시한은 그대로다. 공통 fake와 다른 spec은 바꾸지 않았다.
  `report_hash`가 있기만 하면 넘어가는 다른 spec 대기는 이번 실패와 연결된 근거가 없어 그대로 두었다.

### 재현과 확인

| 조건 | 결과 |
| --- | --- |
| 수리 전, 원래 fake, 단독 | 통과 |
| 수리 전, fake의 `cp` 앞에 빈 `report.md`와 0.5초를 심음 | 실패, CI와 같은 `The expression evaluated to a falsy value:` |
| 수리 뒤, 같은 심기 3회 | 3/3 통과 |
| 수리 뒤, 원래 fake, `--repeat 8 --workers 4` | 8/8 통과(중간 12.0초) |

## 남긴 것

- `ssh_install` exit 4: install의 exit 4 오류는 `binary_target_conflict`, `existing_installation_conflict`,
  `installation_content_conflict`, `installation_draft_conflict`, `installation_events_path_conflict`,
  `installation_resume_changes_require_new_plan`, `installation_target_mismatch`, `installation_worker_unverified`다.
  1회뿐이고 원격 계획 원문은 CI artifact(로그인 필요)에만 있어 원인을 정하지 않았다.
- 웹 머신 추가의 원격 install 계획은 `min_free_gb`를 비워 원격 머신에도 기본 20GB를 요구한다. 그 실패(exit 2)는
  `remote_why`가 「실행 인자·사용법이 잘못됐다」로 적는다. 제품 진단 문구라 이 일에서는 바꾸지 않았다.
- Windows install 승인 PTY 정지는 d-0490이 맡는다.

## 참고

- [pexpect/ptyprocess](https://github.com/pexpect/ptyprocess/blob/master/ptyprocess/ptyprocess.py)
  ([ISC](https://github.com/pexpect/ptyprocess/blob/master/LICENSE)): 자식이 끝난 뒤 PTY 입출력의 EIO를 끝으로 다룬다.
  끝난 자식에 쓰지 않는 근거로만 참고했고 코드를 가져오지 않았다.
- [GitHub-hosted runners 사양](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)과
  [actions/runner-images](https://github.com/actions/runner-images)([MIT](https://github.com/actions/runner-images/blob/main/LICENSE)):
  러너 저장 공간 보장 값을 확인했다.

- [git/git](https://github.com/git/git)([GPL-2.0](https://github.com/git/git/blob/master/COPYING))의 `builtin/gc.c`·`setup.c`·`tempfile.c`:
  maintenance 잠금·분리·임시 파일 소유권 순서를 읽었다. 코드를 가져오지 않았고 시험용 실행 파일만 임시로 빌드했다.

## 검증

- A: `nice -n 15 cargo test --profile verify --locked -p aaa --test install -- wizard:: worker_lifetime:: install_reads_local_overrides authorization:: recovery::` 18 통과(13.63초, load 3.8~5.6),
  install 전체 33 통과·기존 ignore 1(14.15초). 결함 심기로 `Lab::args`의 기준을 99999로 두면
  `private_tty_enrollment…`·`closed_enrollment_window…`가 `work_space_insufficient`로 실패했고 되돌렸다.
- B: `--test m13 -- claude_menu_` 3 통과를 5번 연속, 같은 실행 파일 10개 동시×3번 30/30 통과.
- C: `-p aaa-agent --lib` 188 통과(4.75초), 위 재현 표.
- `cargo clippy --profile verify --locked -p aaa --tests -- -D warnings`, `-p aaa-agent --all-targets -- -D warnings`, `cargo fmt --all --check`,
  `git diff --check`, `node web/test/fixed-sleeps.mjs`, `bash scripts/check-public.sh` 통과.
- Windows 실기 PC(SSH 세션, 작업·TEMP는 시험 전용 폴더): 후보 판 install 전체 30개 중 27개 통과.
  바뀐 시험(wizard 3·worker_lifetime·install_reads_local_overrides·authorization·recovery의 승인 PTY)은 모두 통과했다.
  실패한 세 시험은 수리 전 main 2b75d22b에서도 같은 환경에서 똑같이 실패했다.
  - `install_does_not_apply_or_execute_selected_files_and_reads_repos_without_writes`: SSH 세션 PATH에 git이 없어 `git init` 실패. git을 PATH에 넣으면 두 판 모두 통과.
  - `recovery::one_line_entry_enters_setup_and_cancels_without_installation_changes`(program not found)와
    `authorization::ssh_environment_and_helper_hash_flags_do_not_bootstrap_a_new_trust_root`(exit 4, 기대 3): 두 판 모두 같은 실패.
    SSH 세션 안에서 돈 환경 차이로 보며, 이 일의 회귀가 아니다. 원인은 따로 확정하지 않았다.
  - 시작 전·끝 뒤의 예약 작업·Run 키·사용자 PATH·방화벽 규칙은 같았다. 서비스는 OS가 켜는 gpsvc·wuauserv의 실행 상태만 달랐다. 시험 폴더는 지웠다.

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
| host `backup::tests::wal_backup_restores_auth_history_and_rotates_only_its_files` | 착지 1 | 백업 중복 실행 잠금 | t-1263이 수리 중 |

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

## 남긴 것

- `ssh_install` exit 4: install의 exit 4 오류는 `binary_target_conflict`, `existing_installation_conflict`,
  `installation_content_conflict`, `installation_draft_conflict`, `installation_events_path_conflict`,
  `installation_resume_changes_require_new_plan`, `installation_target_mismatch`, `installation_worker_unverified`다.
  1회뿐이고 원격 계획 원문은 CI artifact(로그인 필요)에만 있어 원인을 정하지 않았다.
- 웹 머신 추가의 원격 install 계획은 `min_free_gb`를 비워 원격 머신에도 기본 20GB를 요구한다. 그 실패(exit 2)는
  `remote_why`가 「실행 인자·사용법이 잘못됐다」로 적는다. 제품 진단 문구라 이 일에서는 바꾸지 않았다.
- Windows install 승인 PTY 정지(d-0490)와 백업 잠금(t-1263)은 각 owner가 맡는다.

## 참고

- [pexpect/ptyprocess](https://github.com/pexpect/ptyprocess/blob/master/ptyprocess/ptyprocess.py)
  ([ISC](https://github.com/pexpect/ptyprocess/blob/master/LICENSE)): 자식이 끝난 뒤 PTY 입출력의 EIO를 끝으로 다룬다.
  끝난 자식에 쓰지 않는 근거로만 참고했고 코드를 가져오지 않았다.
- [GitHub-hosted runners 사양](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)과
  [actions/runner-images](https://github.com/actions/runner-images)([MIT](https://github.com/actions/runner-images/blob/main/LICENSE)):
  러너 저장 공간 보장 값을 확인했다.

## 검증

- 위 두 재현만 돌렸다. 고친 Rust 시험 실행·clippy·fmt·공개 검사는 검증 Mac의 다른 계측 차례가 끝난 뒤 한다.

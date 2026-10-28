# 머신 독점 예약·착지 제외·비중 배정 검증

t-1265, 2026-10-28. macOS arm64에서 실행했다. 제품 규칙은 [머신 배정](machine-assignment.md)에 있다. FRONT_API 1.146.

## 새 시험과 필요한 까닭

| 시험 | 지키는 계약 | 시간 |
|---|---|---|
| `machine_reserve::tests::reservation_keeps_others_out_until_the_holder_task_leaves` | 주인이 아닌 시작은 자동 배정에서 빠지고 지정 시작은 `start_wait`, 주인 할 일·`--ignore-reservation`은 통과, 다른 주인은 `conflict`, 주인 할 일이 목록에서 빠지면 해제 | 1초 안 |
| `machine_reserve::tests::thread_holds_only_its_own_machine_and_release_follows_the_thread` | thread는 post 길로 자기 머신만, 세션 안·보기 권한은 Front 거부, 비중은 Owner만, 주인 thread가 끝나면 해제 | 1초 안 |
| `machine_reserve::tests::transaction_recheck_sees_a_reservation_made_after_the_precheck` | 판정 뒤에 걸린 예약도 DB 선점에서 막고 시한 지난 기록은 막지 않음 | 1초 안 |
| `task_repo::tests::least_busy_divides_open_threads_by_weight_and_skips_zero` | 열린 thread÷비중 차례와 비중 0 제외 | 1초 안 |
| `automation::start_tests::start_without_machines_skips_reserved_and_landing_machines` | `machines` 없는 자동화가 예약·착지 중 머신을 빼고 고르며 풀리면 그 머신을 고름, 고정 목록 승인은 자동 범위를 덮지 않음 | 1초 안 |
| `backup::tests::staging_lock_releases_even_when_a_forked_copy_survives` | 백업 staging 잠금이 fork 사본이 남아도 실행 끝에 풀림(아래) | 1초 안 |
| 웹 E2E `99-machine-workspaces` 「머신 독점 예약·비중」 | 웹 걸기·풀기, CLI 지정 시작 대기, Reader 잠김, 비중 0 저장과 CPU 수 되돌리기 | 약 2초 |

기존 `automation::tests::start_toml_captures_safe_brief_and_rejects_ambiguous_or_outside_files`는 빈 `machines`를 오류로 단언했다. 빈 목록이 자동 머신이 된 계약 변경에 맞춰, 생략한 규칙이 자동 머신으로 읽히는지와 빈 이름 거부로 바꿨다.

## 병렬 시험에서 드러난 백업 잠금 결함

첫 전체 `ci-local`의 test 단계에서 `backup::tests::wal_backup_restores_auth_history_and_rotates_only_its_files`가 둘째 백업에서 「백업 중복 실행 잠금」(EWOULDBLOCK)으로 실패했다. 단독으로는 통과했다. 이 변경은 백업 코드를 건드리지 않았다.

원인은 plan §8.233과 같은 계열이다. `backup::run`은 staging 잠금을 File close로만 풀었다. 같은 프로세스의 다른 시험이 병렬로 프로세스를 띄우는 fork~exec 사이에 잠금 FD 사본이 남으면 flock이 풀리지 않는다. 실행 끝에 명시적으로 unlock하는 guard를 넣었다. 새 시험은 `try_clone`으로 fork 사본을 결정적으로 만든다. unlock을 빼면 CI와 같은 `WouldBlock`으로 실패하고(RED), 넣으면 통과한다(GREEN). 실패 당시 사본을 물려받은 자식 PID는 로그에 없어 특정하지 않았다.

## 자동 검증

| 검증 | 결과 |
|---|---|
| `nice -n 15 bash scripts/ci-local.sh`(web·fmt·build·clippy·test·e2e·public) | 마지막 실행: e2e의 `90-workspace-metrics` 한 단계 밖 모두 통과(아래) |
| `bash scripts/ci-local.sh --only linux-clippy` | 통과 |
| `AAA_UPDATE_TS=1`·`AAA_UPDATE_AUTOMATION_SCHEMA=1`로 다시 만든 `web/src/gen/proto.ts`·`docs/automation.schema.json` | 생성물 비교 시험 통과 |

Playwright는 저장소 요구 판 1.64.1 npm 패키지만 작업 전용 도구 폴더에 두고, 브라우저는 공용 캐시를 썼다. 웹 E2E는 실제 임시 host·agent를 쓰며 정한 시간을 자지 않는다.

## 마지막 실행과 부하

수리 뒤 전체 `ci-local`은 web·fmt·build·clippy·test(Rust 전체)·public이 통과했다. e2e는 121개 파일 중 `90-workspace-metrics` 하나가 「별도 로그의 실제 ci-local 종료를 … 보존한다」 단계의 20초 시한으로 실패했다. 이 변경 전 같은 브랜치의 첫 전체 실행에서는 이 파일이 통과했다.

부하 근거: 실패 직후 1분 부하 평균이 156~166(10코어)이었고, 같은 머신에서 다른 작업의 E2E CI(`aaa-ci-e2e` 실행 파일 여러 개와 headless Chromium)가 돌고 있었다. 그 상태에서 이 파일만 세 번 단독으로 돌렸다. 첫 번은 다른 단계(「긴 workspace 목록 … 24개 수집」 20초 시한)에서 실패했고, 부하가 내려간 뒤 두 번은 모두 통과했다(139.7초·96.0초, 같은 단계 18~20초). 전체 실행에서 같은 단계는 9~11초였다. 실패한 단계가 실행마다 달라 이 변경의 결함이 아니라 부하로 본다. 이 단독 세 번은 마지막 수리 전(4b288fe5 이전)에 빌드한 e2e 실행 파일이었다. 마지막 판(6fcedb01)으로 e2e 실행 파일을 다시 빌드해 `--spec 90,99`를 한 번 더 돌렸고 둘 다 통과했다(1분 부하 71~87, 90은 83.4초·같은 단계 15.3초, 99는 20.3초). 이 파일은 머신 예약·비중 경로를 쓰지 않으며 예약이 없을 때 `machine_views`에 더한 일은 머신당 DB meta 읽기 두 번이다. 시한·기대값은 바꾸지 않았다.

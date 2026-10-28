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
| `nice -n 15 bash scripts/ci-local.sh`(web·fmt·build·clippy·test·e2e·public) | 아래 「마지막 실행」 |
| `bash scripts/ci-local.sh --only linux-clippy` | 통과 |
| `AAA_UPDATE_TS=1`·`AAA_UPDATE_AUTOMATION_SCHEMA=1`로 다시 만든 `web/src/gen/proto.ts`·`docs/automation.schema.json` | 생성물 비교 시험 통과 |

Playwright는 저장소 요구 판 1.64.1 npm 패키지만 작업 전용 도구 폴더에 두고, 브라우저는 공용 캐시를 썼다. 웹 E2E는 실제 임시 host·agent를 쓰며 정한 시간을 자지 않는다.

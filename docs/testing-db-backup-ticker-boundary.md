# 정기 DB 백업 시험의 기동·ticker·저장 경계(t-1238)

## 실패와 수리 전 재현

기준 main `60052d5c`에서 병합한 후보 `f4c4428e`의
`db_backup::host_ticker_backs_up_once_and_remembers_attempt_after_restart`가
「5s 안에 정기 백업 성공 기록이 되지 않았다」로 실패했다(두 시험 중 1실패, 5.09초).
보존 DB에는 `db-backup:last-attempt-ms`와 `last-success-ms`가 없고 host 로그는 초기 파일 한도 기록뿐이었다.
이 자료는 백업 시도 전의 지연을 보이며, 어느 host 기동 하위 단계가 늦었는지는 확정하지 않는다.

기존 시험은 host를 spawn하자마자 5초 안의 `db-backup status` 성공 기록을 기다렸다.
이 명령은 IPC로 host의 준비를 확인하지 않고 DB를 직접 읽는다. 따라서 host 기동과 자연 ticker 대기까지
백업 저장의 5초에 포함됐다. 재시작 뒤에도 준비를 구분하지 않고 ticker 요청을 5초 안에 반복했다.

수리 전 시험에 관측용 wrapper를 넣어 실제 host의 exec만 FIFO에서 보류했다.
원래 5초 대기는 그대로 실패했고, 그 실패 사건에서 FIFO를 풀면 실제 host의 ticker와 성공 기록이 확인됐다.
원래 panic을 다시 올려 exit 101·실패 원문을 보존했다. 고정 sleep이나 CPU 부하는 쓰지 않았다.
이는 원본 실행에 FIFO 정체가 있었다는 뜻이 아니라, 기동과 저장을 묶은 같은 실패 경계의 재현이다.
계측 patch·원문·종료 코드·실험실은 작업 보고 library에 보존했고 계측은 제거했다.

## 시험만 바꾼 부분

| 순서 | 관측과 단언 |
| --- | --- |
| 최초 host 준비 | 읽기 전용 `status`의 Front 응답을 조건으로 기다린다. 준비 실패는 마지막 IPC 오류와 host 로그를 남긴다. |
| 최초 ticker | 준비 뒤 `ticker tick`을 한 번만 보내 실제 요청 뒤에 시작한 tick의 완료 응답을 기다린다. 실패한 변경 요청을 반복하지 않는다. |
| 성공 저장 | 먼저 `HostWatch`를 연결한다. tick 응답 뒤에도 백업 worker가 돌 수 있으므로 초기 상태·실제 State 사건에서 `db-backup status`를 읽어 성공 기록을 기다린다. |
| 재시작 | 이전 구독을 닫고 실제 host를 종료·재기동한다. 새 host 준비와 실제 ticker 한 번 완료 뒤 전체 status가 최초 기록과 같은지 비교한다. |

성공 기록의 5초와 최초 `last_success_ms`·재시작 뒤 status 전체 동일 단언은 유지한다.
별도 host 준비는 30초 안전 상한이다. 원본 5.09초 동안 시도조차 없었던 기동 지연을 저장 성능과 분리하며,
Front handshake 자체의 10초 상한보다 짧던 전체 5초 결합을 없앤다. 준비되면 즉시 끝나고 30초를 자지 않는다.
준비 조회는 기존 조건 대기 도구, 저장 완료는 기존 Front 상태 사건 구독을 사용한다.
백업을 수동 run하거나 성공 값을 만들어 넣지 않는다. 제품 코드·설정 정책·암호화·중복 방지·기대값은 바꾸지 않았다.

기존 `lab::watch::HostWatch`와 host의 `scheduled_success_survives_restart_without_duplicate_backup`이
상태 변경을 먼저 구독한 뒤 저장 완료를 확인하는 방식을 따른다. Front의 State는 host의 변경 watch를 받고,
백업 worker는 기록 저장 뒤 bump하므로 구독 사이에 저장이 끝나도 초기 상태 조회로 확인한다.
공용 lab helper나 의존성은 바꾸지 않았다.

## 검증

- 계측한 수리 전 시험: 같은 5초 실패, 보류 해제 뒤 실제 ticker·성공 기록 확인, exit 101(6.23초).
- 수리한 `cargo test --profile verify --locked -p aaa --test db_backup -- --nocapture`: 2건 통과, 전체 1.96초.
- 이 회귀는 실제 CLI 암호화/복원과 실제 host 재시작의 정기 시도 보존을 지킨다. 기존 host 유닛은 예정 시각·시계 역행·실패 중복 방지의 시간 규칙을 담당한다.
- 전체 착지 검증은 최신 원격 main의 새 전용 worktree에서 수행하고 결과를 작업 보고에 남긴다.

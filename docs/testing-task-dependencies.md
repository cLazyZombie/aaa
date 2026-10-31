# 여러 선행 할 일 예약 검증

`--after d-0001 d-0002`와 반복 `--after`를 모두 받는다. 예약 때 고정 ID로 해석·중복 제거하고 모든 선행의 완료 보관 뒤에만 기존 실행 한도·기동 선점을 통과한다.
단일 `after`와 과거 저장 자료는 유지한다. `after_all`을 명시한 새 요청은 전체 조건을 교체하며 빈 목록은 해제다. 여러 조건을 구판 클라이언트가 하나로 덮는 갱신은 거부한다.

## 필요한 시험

- `aaa-host task_queue::tests::multiple_predecessors_are_fixed_deduplicated_and_cycle_checked_on_every_branch`: 두 번째 간선의 순환·자기 참조·없는 ID가 이전 승인을 훼손하지 않아야 한다. 중복 ID, 다이아몬드, 구판 저장 자료·구판 갱신 거부, 조건 해제 때 차례 보존과 다중 조건 취소 후 완료·재시작도 검사한다.
- `aaa-host task_queue::tests::multiple_predecessors_wait_through_failure_park_restart_and_detect_deleted_branch`: 일부 완료·실패·보류·재시작으로 합류를 풀지 않아야 한다. 완료 복원과 다른 선행 삭제가 겹치면 삭제 경고를 숨기지 않으며 대기 중 새 thread를 만들지 않는다.
- `aaa --test m13 task_dependencies::multiple_predecessors_start_once_after_all_archives_and_capacity_across_restart`: 실제 CLI·host·agent로 여러 값/반복 옵션, 일부 완료 및 기동 직후 재시작, 독립 예약 통과, 전체 완료 뒤에도 한도 대기, 자리 반환 후 단일 pane, 중복 tick을 검증한다.
- 웹 E2E `115-task-dependencies`: Chromium·WebKit 각각 데스크톱·휴대폰에서 체크박스 선택, 모든 대기 ID 표시, 다시 연 확인의 선택 보존, 한 조건 해제의 실제 저장을 검사한다.

기존 예약 owner의 취소와 선점 경합, 기동 응답 유실 대조, 기동 전 중단과 재시작, 동시 자리 한도 검사를 함께 유지한다. 고정 sleep·재시도·시한 연장은 추가하지 않았다.

## 참고 구현과 범위

- [shepherdr dependencies.ts (c3f0f594)](https://github.com/eliasstravik/shepherdr/blob/c3f0f594/server/core/dependencies.ts), [MIT](https://github.com/eliasstravik/shepherdr/blob/c3f0f594/LICENSE): 등록 때 고정 ID로 해석·중복 제거하고 재귀 없이 간선을 순회하는 접근을 참고했다.
- [BullMQ Flows](https://docs.bullmq.io/guide/flows/), [MIT](https://github.com/taskforcesh/bullmq/blob/master/LICENSE): 모든 선행 완료와 실행 가능 상태를 나누는 접근을 참고했다.

외부 코드를 복사하거나 런타임 의존성을 추가하지 않았다. shepherdr의 병합 상태와 agent 미리 생성 정책은 가져오지 않았다. aaa의 완료 보관·기동 의도·실행 한도 계약을 유지하며 저장소 착지 gate·자동 병합은 범위 밖이다.

## 결과

개발 owner 검사 결과:

| 검사 | 결과 |
| --- | --- |
| `cargo test --profile verify --locked -p aaa-host --lib task_queue::tests::` | 41개 통과 |
| 보강한 `task_queue::tests::multiple_predecessors` | 2개 통과, 합계 0.47초 |
| `cargo test --profile verify --locked -p aaa --test m13 task_dependencies::` | 1개 통과, 기동 직후 재시작 포함 4.37초 |
| `cargo test --profile verify --locked -p aaa --test deps` | 의존 경계 2개 통과 |
| `cargo test --profile verify --locked -p aaa-proto` | 전체 및 TypeScript 생성 일치 통과 |
| `npm run build`, `npm test` | 타입·번들 및 유닛 664개 통과 |
| `ci-local.sh --only e2e --spec 115` | 네 브라우저·폭 조합 통과, 각 단계 0.62~1.10초 |
| 관련 Rust owner strict Clippy | 통과 |
| 공개 저장소 검사·fmt | 통과 |

최초 타입 생성 빌드에서 `Vec`에 `ts(optional)`만 적용한 오류를 발견했다. 기존 규칙의 `as = "Option<Vec<String>>"`을 적용해 타입 생성과 전체 protocol 검사를 통과했다. 시험 재시도·시한 연장은 없었다.
개발 중 미커밋 변경 때문에 ci-local의 finish 출처 요약은 보관되지 않았다. E2E 실행 결과·선택 기록은 정상 보존되었다.
이 결과는 관련 owner 검사다. 착지 창의 최종 통합 SHA 전체 gate는 [verification.md](verification.md)를 따른다.

# 예약 지정 계정 검증(t-1615)

## 변경과 경계

`QueueOp::Reserve.account`와 `TaskReservation.account`를 Front API 1.164에 추가했다.
계정 별칭만 기존 SQLite 예약 JSON에 저장하고 실제 인증은 기존 금고·agent 전달 경로를 사용한다.
구판 저장값의 계정 생략은 자동 선택이며 새 CLI는 계정을 버리는 구판 host 요청을 거절한다.
웹은 새 host에서만 같은 harness의 계정 선택을 열고 갱신 시 기존 값을 채운다.

예약 시작에서만 지정 계정의 현재 한도와 실제 인증 상태를 확인한다. 사용량 미관측·조회 인증 실패를
작업 인증 끊김과 구분하며, 사용 불가 계정은 `queued` 이유로 보존한다. 직접 시작의 명시 계정 정책은 유지한다.
공통 Rust·protocol·웹 변경이며 Windows 전용 경로·production 산출물은 변경하지 않았다.

## 꼭 필요한 시험

| owner | 지키는 계약 | 결과 |
| --- | --- | --- |
| host `task_queue::tests` | 예약 저장·선행 조건·재열기·구판 기본값과 기존 예약·기동 경계 | 42개 통과 |
| host `account_pool::tests` | 인증 끊김·현재 한도·잠김 대기와 조회 실패 허용, 기존 자동 선택 | 25개 통과 |
| proto 전체 | Rust 계약·직렬화·생성 TypeScript 최신성 | 54개 통과 |
| m13 새 지정 계정 시험 | 실제 CLI `--account --after` → host 재시작 → 잠김 대기 → 키 복원 → 지정 인증의 agent 수신 | 통과, 3.67초 |
| m13 기존 다중 선행 시험 | 계정 생략의 기존 시작·선행 완료·재시작·중복 기동 방지 | 통과, 7.65초(기존 시험 유지) |
| 의존 경계 `deps` | frontend·host·agent crate 경계 | 2개 통과 |
| E2E116 | Chromium·WebKit의 1280·390 폭에서 선행 조건과 계정 선택·재열기·갱신 보존 | 네 조합 통과, 각 0.77~1.33초 |
| 웹 전체 | 타입·번들·기존 웹 유닛 | 713개 통과 |
| strict Clippy·fmt·공개 검사 | 관련 세 crate 모든 target·서식·공개 정보 경계 | 통과 |

새 m13은 가짜 인증 두 개를 공통 생성기로 만들고, agent가 지정 인증과 일치하는지만 파일로 알린다.
외부 제공자·운영 인증은 사용하지 않는다. 새 대기는 사건과 5초 실패 상한을 쓰며 고정 sleep은 추가하지 않았다.
시험 삭제·기대값 완화·시한 확대는 없다.

## 실행과 수리

검사는 모두 `aaa verify -- <명령>`으로 실행했다. 미커밋 개발 상태이므로 공통 원장은 출처 미확인으로
자동 수집하지 않았고 아래 명령의 종료 결과와 owner 단언을 확인했다.

- `cargo test --profile verify --locked -p aaa-host --lib task_queue::tests::`
- `cargo test --profile verify --locked -p aaa-host --lib account_pool::tests::`
- `cargo test --profile verify --locked -p aaa-proto`
- `cargo test --profile verify --locked -p aaa --test m13 task_dependencies::reserved_account`
- `cargo test --profile verify --locked -p aaa --test m13 task_dependencies::multiple_predecessors`
- `cargo test --profile verify --locked -p aaa --test deps`
- `bash scripts/ci-local.sh --only web,public`
- `bash scripts/ci-local.sh --only e2e --spec 116`
- `cargo clippy --profile verify --locked -p aaa-host -p aaa-proto -p aaa --all-targets -- -D warnings`
- `cargo fmt --all --check`
- `bash scripts/check-public.sh`

최초 m13은 시험용 금고 경로에서 `host/`를 빠뜨려 키 이동에 실패했다. 실제 Host 상태 경로로 바로잡고 통과했다.
최초 웹 정적 검사는 셸 비교식의 인증 환경 변수를 평문 인증 등록으로 읽었다. 같은 비교에서 파일값을 왼쪽에 두어
인증 필드 대입과 구별했으며 검사기 예외는 추가하지 않았다.
최초 E2E는 상대 `NODE_PATH`를 harness의 `createRequire`에 전달해 시작 전에 거절됐다. 작업 폴더의 Playwright
1.64.1 절대 경로로 수정했으며 전역 패키지는 바꾸지 않았다. 이후 실제 브라우저 네 조합을 통과했다.

개발의 관련 owner 검사이며 착지 전체 gate·양 OS cross·Windows production 검증은 아니다.
착지는 승인받은 창에서 최종 통합 SHA로 `docs/verification.md`에 따라 수행한다.

## 참고 구현

- [Celery Calling API](https://docs.celeryq.dev/en/stable/userguide/calling.html),
  [BSD 3-Clause](https://github.com/celery/celery/blob/main/LICENSE): 실행 인자를 작업 메시지로 넘기는 구조를 대조했다.
- [BullMQ Job Data](https://docs.bullmq.io/guide/jobs/job-data),
  [MIT](https://github.com/taskforcesh/bullmq/blob/master/LICENSE): 작업의 사용자 데이터를 직렬화해 worker에 전달하는 구조를 대조했다.

이 비교에 따라 계정 선택도 예약 payload에 두고 기존 시작 인자로 연결했다. 외부 코드·의존성은 추가하지 않았다.

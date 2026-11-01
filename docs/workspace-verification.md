# 공통 검증 수집 계약

`aaa verify`는 저장소의 기존 검증을 실행하고 Git 공용 폴더 `aaa-workspace-runs/`에 요약을 남긴다.
스크립트 탐색·실행 방법은 [사용법](usage.md#검증-자동-수집)을 따른다. 새 thread brief에도 이 공통 입구를 안내한다.
검증을 추가로 실행하거나 기존 스크립트를 수정하지 않는다.

## 원장과 출처

실행 전후의 commit·tree·origin 신원 해시와 clean 여부를 비교한다. 미추적 입력도 변경에 포함한다.
출처가 바뀌거나 dirty면 검증은 실행하지만 main 지표 원장은 만들지 않는다. 전체 경과는 단조 시계로 재고,
상세 결과의 연결 범위는 시작·종료의 실제 시각이다. 시각이 역행한 실행은 원장에 넣지 않는다.

원장은 임시 폴더에서 완성한 뒤 `run-<시각>-<ID>`로 원자적으로 공개한다.
`provenance.json`의 `format=3`, `runner=aaa-verify`와 `metrics.json`의 `MetricRun`을 함께 읽는다.
파일 안의 commit·run ID·수집 시각·source를 그대로 신뢰하지 않고 수집기가 검증 출처와 디렉터리 ID로 채운다.
기존 format 1·2와 확정 캐시는 유지하며 parser 판이 바뀌면 남아 있는 입력을 다시 해석한다.

agent는 push된 main의 첫 부모 계보 또는 같은 tree의 둘째 부모 착지 병합을 확인한 뒤 원장을 채택한다.
다른 브랜치의 같은 tree나 후보 조상만으로는 채택하지 않는다. 보고 회수 때 등록 저장소의 Git 공용 폴더를 읽으므로
검증한 worktree가 삭제되거나 등록 checkout이 옛 main이어도 원장과 Git 객체가 남으면 회수할 수 있다.

기존 writer가 로컬 시작 표지와 같은 출처·구간의 새 종료 원장을 만들면 공통 원장을 추가하지 않는다.
rc·pt·engine·ci-local은 기존 상세 결과와 단위를 유지한다. 기존 스크립트가 자체 원장을 만들지 못하면 공통 요약을 남긴다.
같은 checkout의 검증을 동시에 실행하지 않는다. 기존 스크립트의 target 결과도 동시 실행에 독립적이지 않다.

## 자동으로 얻는 값

| 값 | 근거 | 해석 |
| --- | --- | --- |
| 성공·실패 | 자식 프로세스 종료 상태 | 수집 실패로 검증 종료 코드를 바꾸지 않음 |
| 검증 시간 | 명령 시작부터 종료·출력 회수까지 | 전체 실행 경과 |
| 테스트 수 | stdout의 libtest `test result:` 종료 요약 | doctest 포함, 출력에 요약이 있는 범위 합계 |
| 빌드 시간 | stderr의 Cargo `Finished` 컴파일 경과 | clippy 포함 컴파일 작업 합계, 병렬 전체 경과와 다름 |
| Cargo profile | 위 Cargo 완료 줄 | 하나면 profile 칸, 여러 개면 목록 메모 |

출력은 즉시 터미널로 보내며 원문은 원장에 저장하지 않는다. 한 줄의 해석 상한은 16 KiB다.
출력 읽기·전달 실패나 상한 초과가 있으면 테스트 수를 확정하지 않고 미수집 사유를 남긴다.
중단된 검증에서 이미 출력된 테스트 수는 전체 검증의 예상 개수가 아니라 완료된 요약의 범위다.
일반 libtest 출력에는 개별 시간이 없으므로 개별 테스트 TOP 5를 만들지 않는다.

## 기존 상세 보고 연결

`target/verification/<suite>/report.json`의 기존 tests/suites 배열을 읽는다. 64개 직계 항목까지만 살피며,
상세 자료 읽기는 8 MiB로 제한한다. 보고 파일의 수정 시각은 이번 실행 시작과 종료 사이여야 한다.
tests·suites 개수 일치, 각 테스트의 `exec_time`, binary의 `exit`로 개수·시간의 완전성을 판정한다.
stdout에 테스트 수가 없을 때는 온전한 상세 보고의 개수를 쓴다. 실패한 binary의 시간 순위는 부분 결과다.

E2E는 다음 기존 형식 중 이번 실행에서 만들어진 첫 보고를 읽는다.

- `target/verification/e2e.json`
- `target/pt-e2e/report.json`
- `target/rc-e2e/report.json`
- `target/no2-e2e/all-headless.json`

보고에는 `status`와 `reports` 배열이 있다. 각 항목은 `scenario`, `status`, `duration_ms`를 가진다.
`timing_unit=scenario-wall`이면 시나리오 경과, 기존 나머지 형식은 `scenario-step-sum`으로 구분한다.
단순 headless 캡처 명령을 임의로 E2E 한 개로 바꾸지 않는다. 여러 E2E 보고를 합쳐 중복 개수를 만들지 않는다.
필요한 상세 보고가 없으면 미수집으로 남긴다.

## 관련 범위와 참고

dungeon의 예전 스크립트는 원장·테스트 상세 보고를 남기지 않아 수집할 입력이 없었다. 코드 줄 수는 Git 객체를 별도로
읽으므로 표시될 수 있었다. 이 변경은 공통 입구에서 앞으로 생기는 기록을 수집한다. 저장하지 않은 과거 시간을 복원하지 않는다.
d-0542의 profile 계약 중 공통 입구에서 실제로 관측한 profile 기록을 다루며, 기존 원장에 없는 profile의 소급 추정은 하지 않는다.
d-0543의 일반 상세 결과 재수집·동일 ID 보강은 범위에 포함하지 않는다.

- [dorny/test-reporter](https://github.com/dorny/test-reporter), MIT: 기존 보고를 명시적인 adapter로 공통 결과에 연결하는 구조를 참고했다.
- [cargo-nextest JUnit](https://github.com/nextest-rs/nextest/blob/main/site/src/docs/machine-readable/junit.md), MIT 또는 Apache-2.0:
  개별 시간은 실행기가 실제 결과에 남겼을 때만 사용하는 방식을 참고했다.

외부 코드를 복사하거나 새 의존성을 추가하지 않았다.

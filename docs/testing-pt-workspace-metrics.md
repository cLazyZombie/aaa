# pt 상세 검증 결과 수집

pt가 새로 보관한 `verification/pt-tests/report.json`과 `e2e.json`을 agent가 기존
main 귀속·freshness 검사 안에서 읽는다. 테스트 보고는 기존 target·exit·tests·suites
계약을 사용한다. headless 보고의 `timing_unit=scenario-wall`은 빌드를 뺀 예제 실행 경과다.
이 단위는 엔진·rc의 `scenario-step-sum`과 구분해 화면에 표시한다.

공통 writer는 pt의 테스트·E2E 부속 결과를 이번 실행 구간에서만 보관한다. 오류 출력이나
원문 경로는 넣지 않고 이름·상태·시간·개수만 남긴다. 기존 rc·engine 결과와 예전 원장의
읽기는 유지한다. profile 기록·동일 ID 보강·과거 기록 재해석은 이번 변경에 포함하지 않는다.

## 필요한 회귀와 실행

- 기존 원장 다중 runner 시험에 pt 상세 결과·시간 단위를 추가한다. pt 분기가 빈 목록으로
  남아도 일반 원장 성공만으로 시험이 통과하던 누락을 잡는다.
- `pt_actual_writer_preserves_failed_test_and_headless_results_through_collection`은 실제
  Python writer를 임시 Git 저장소에서 실행하고 Rust 수집까지 연결한다. 실패·ignored 개수,
  개별 시간, E2E 실행 경과와 부분 순위가 파일 보관·수집 경계를 지나도 남아야 해서 필요하다.
- Python 공통 writer 시험은 pt 파일 보관·원문 제외·freshness를 기존 계약 안에서 검사한다.

```sh
nice -n 15 cargo test --profile verify --locked -p aaa-agent --lib workspace_metrics::
nice -n 15 python3 -m unittest scripts.test_workspace_record
```

웹은 단위 문자열의 표시만 바뀐다. 타입·번들·웹 시험과 기존 workspace E2E를 포함한 전체
`scripts/ci-local.sh`로 확인한다. 새 E2E 번호나 고정 대기·재시도는 추가하지 않는다.

원본 형식은 [nextest JUnit](https://nexte.st/docs/machine-readable/junit/)과
[Rust 테스트 안내](https://doc.rust-lang.org/rustc/tests/)를 비교했다.
둘 다 MIT / Apache-2.0이며 외부 코드를 복사하지 않았다. pt는 stable을 유지하고 nextest가
기록한 개별 시간을 기존 테스트 JSON 계약으로 변환한다.

# 같은 검증 실행의 상세 결과 재수집

부속 결과만 변경되면 agent가 재해석하지 않고, host도 같은 실행 ID의 새 값을 버리던 경로를
수리한다. 공통 보강 함수는 agent·host 모두에서 기존 값의 누락과 퇴행을 막는다.

## 필요한 회귀

- `archive_recollects_detail_changes_without_new_ids_or_losing_confirmed_results`:
  실제 Git 원장을 먼저 빈 상세로 수집한 뒤 출처 파일을 바꾸지 않고 테스트·E2E를 추가한다.
  개수와 개별 시간 수집, 부분→완전 순위, 파일 손상·삭제 뒤 기존 값 보존과 실행 한 개를 확인한다.
  동일 ID의 종료 시각을 바꾸면 기존 기록과 충돌 사유가 남아야 한다.
  캐시 함수만 검사하면 보고 회수 경계의 누락을 놓치므로 실제 수집을 사용한다.
- `ledger_enriches_the_same_run_and_preserves_details_after_sparse_or_conflicting_recollection`:
  실제 SQLite 저장 함수로 같은 ID를 보강하고 빈 결과·부분 결과·충돌한 신원으로 재수집한다.
  재시작 뒤에도 한 행과 완전한 순위가 남는지 확인한다. 단순 UPSERT만 쓰면 기존 상세가
  사라지는 회귀를 막는 데 필요하다.
- 기존 workspace owner는 parser 갱신·원본 삭제·구형 target·여러 runner·main 귀속·읽기 상한과
  조회 전용 화면 계약을 함께 검사한다. 테스트에 고정 대기나 재시도를 추가하지 않는다.

## 실행 결과

owner·Clippy·의존 경계·공개 검사는 `aaa verify --`로 실행했다.

| 검사 | 결과 |
| --- | --- |
| `cargo test --profile verify --locked -p aaa-agent -p aaa-host -p aaa-common --lib workspace_metrics::` | agent 27개·host 5개 통과 |
| 최종 빈 완료 순위 보존 보강 뒤 같은 host 저장 owner 단독 실행 | 1개 통과, 0.02초 |
| `cargo clippy --profile verify --locked -p aaa-common -p aaa-agent -p aaa-host --all-targets -- -D warnings` | 통과 |
| `cargo test --profile verify --locked -p aaa --test deps` | 2개 통과 |
| `bash scripts/check-public.sh` | 통과 |
| `cargo fmt --all -- --check`와 `git diff --check` | 통과 |

agent owner 전체 실행 3.62초, host owner 전체 0.71초로 이번 개별 시험도 모두 5초 미만이다.
새 회귀 2개 외에는 기존 검사만 보강했다. 실패·재시도·시한 연장·기대값 완화는 없다.
기존 ts-rs serde 속성 해석 경고와 proc-macro-error2의 future-incompat 안내가 있었지만 종료는 성공했다.
미커밋 개발 검증이므로 `aaa verify`의 main 지표 원장에는 저장하지 않았다.

전체 착지 gate와 양 OS cross는 착지 창의 최종 통합 SHA에서 수행한다. 웹·Windows 전용 경로는
변경하지 않았으며 배포·운영 데이터 재수집은 수행하지 않았다.

## 참고한 공개 구현

- [SQLite UPSERT](https://www.sqlite.org/lang_upsert.html),
  [public domain](https://www.sqlite.org/copyright.html): 충돌한 키의 행을 갱신하는 방식을 따른다.
  JSON 통째 교체 대신 기존 행과 보강한 JSON을 같은 transaction에서 저장한다.
- [Sphinx Environment Collector](https://www.sphinx-doc.org/en/master/extdev/collectorapi.html),
  [BSD-2-Clause](https://github.com/sphinx-doc/sphinx/blob/master/LICENSE.rst): 원본 하나만 보지 않고
  관련 입력의 추가·변경·삭제로 재수집 대상을 정하는 접근을 참고했다.

외부 코드를 복사하거나 새 의존성을 추가하지 않았다.

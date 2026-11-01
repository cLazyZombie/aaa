# 공통 검증 자동 수집 검증(t-1398)

dungeon의 `scripts/verify.sh`에는 원장 작성이 없고, 기존 수집기는 특정 runner의 결과 파일만 읽고 있었다.
`aaa verify`가 기존 스크립트를 실행하며 공통 format 3 종료 원장을 남기고 agent가 기존 main 귀속 규칙으로 읽도록 바꿨다.
제품 계약·상세 보고 경로·미수집 범위는 [공통 수집 계약](workspace-verification.md)에 있다.

## 꼭 필요한 회귀

- `common_verify_runs_real_cargo_then_collects_from_an_old_checkout_after_main_lands`:
  별도 adapter 없는 임시 저장소에서 실제 Cargo 테스트를 실행한다. 테스트·ignored 개수, Cargo 컴파일 경과·profile을 읽고,
  main 반영 전 대기 → 반영 뒤 옛 checkout에서 수집 → 원본 정리 뒤 확정 캐시 보존을 확인한다.
  dungeon처럼 기존 stdout만 있는 저장소가 실제 수집까지 연결되는지를 지키는 owner다.
- `common_verify_keeps_failure_and_fresh_details_but_rejects_dirty_sources`:
  실패 코드 7·9를 보존하고, 이번 실행이 만든 개별 테스트·E2E 상세만 연결한다. 실행 전 dirty와 실행 중 변경은
  main 원장에 넣지 않는다. 수집 실패로 검증의 실패를 성공으로 바꾸지 않기 위해 필요하다.
- `common_verify_reuses_a_native_writer_without_a_duplicate_run`:
  실제 기존 Python writer를 실행해 native 원장과 공통 원장이 중복되지 않는지 본다. 새 native 원장이 없거나 요약이 손상되면
  공통 기록으로 보완해야 한다. 기존 저장소를 공통 명령으로 실행할 때의 호환 owner다.
- 스트리밍 시험 2개는 큰 출력의 메모리 상한·원문 전달·stderr 요약 중복 제외와 Cargo 혼합 profile·컴파일 시간 단위를 지킨다.
- 실제 CLI 통합 시험은 host 없이 자동 탐색·명시 인자·stdout/stderr 전달·실패 코드·JSON 출력을 확인한다.
  라이브러리 호출만으로는 clap과 실제 실행 파일의 출력 경계를 확인할 수 없어 필요하다.
- 기존 brief 조합 시험에 공통 명령 안내를 추가했다. 새 thread가 저장소별 수집 코드를 따로 만들지 않고 공통 입구를 사용하게 한다.

모든 새 회귀는 고정 sleep 없이 프로세스 종료·파일 완성·Git 참조 변경으로 끝난다.
중첩 Cargo/libtest 출력은 시험의 sink로 보내 바깥 runner의 테스트 수 집계를 오염시키지 않는다.

## 실행 범위

| 검사 | 결과 |
| --- | --- |
| agent 수집 owner | 26개 통과, 실행 3.49초 |
| 실제 CLI 통합 | 1개 통과, 실행 0.49초 |
| crate 의존 경계 | 2개 통과 |
| CLI 인자 계약 | 1개 통과 |
| thread brief 조합 | 4개 통과 |
| 관련 3개 package strict Clippy | 통과 |
| fmt·공개 검사 | 통과 |

개별 새 시험은 5초 안에 끝났고 실패를 재시도하거나 시한·기대값을 늘리지 않았다.

```sh
nice -n 15 cargo test --profile verify --locked -p aaa-agent --lib workspace_metrics::
nice -n 15 cargo test --profile verify --locked -p aaa --test verify --test deps
nice -n 15 cargo test --profile verify --locked -p aaa --bin aaa cli::tests::verify_
nice -n 15 cargo test --profile verify --locked -p aaa-host --lib brief_text::tests::
nice -n 15 cargo clippy --profile verify --locked -p aaa-agent -p aaa-host -p aaa --all-targets -- -D warnings
cargo fmt --all -- --check
bash scripts/check-public.sh
```

개발의 관련 owner만 검사한다. 웹·protocol·의존성·Windows 전용 코드는 바꾸지 않았고 새 의존성도 없다.
전체 gate·cross·운영 배포는 착지 창의 최종 통합 SHA에서 수행한다. 실제 dungeon 전체 검증과 운영 대시보드 갱신은
이번 임시 저장소 회귀의 실행 결과로 주장하지 않는다.

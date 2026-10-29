# 자동화 입력의 묶음 완료 등록 검증

## 변경 계약

`aaa automation input complete <실행 ID>... --outcome completed|failed|needs-decision [--report r-NNNN]`은
같은 결과와 보고를 여러 실행에 기록한다. ID 중복은 한 번만 처리하고 입력 순서를 유지한다.
묶음 중 없는 실행·다른 thread의 입력·준비되지 않은 보고·기존 결과와 충돌하는 실행이 있으면
SQLite transaction 전체를 취소한다. 실패 응답에는 해당 실행 ID와 원인을 함께 남긴다.

전송은 기존 peer PID·session token 검증 경로를 사용한다. host가 인증된 자동화 worker와 실행·보고의
소유자를 확인한 뒤 완료 전용 경로로 처리한다. 일반 post 속도 제한·음소거·본문 중복 제한을 거치지
않고 일반 한도를 소모하지도 않는다. 완료는 실행 원장·영수증·감사에 남고 coordinator inbox나
사용자 알림을 만들지 않는다. 기존 일반 post의 한도와 음소거는 유지한다.

단건 요청은 기존 `run` 본문과 영수증을 유지한다. 묶음 요청은 `runs` 배열을 보내며
`automation_completed` 영수증에는 첫 ID인 `run`과 전체 ID 배열 `runs`가 있다.
새 CLI는 요청한 전체 ID가 영수증과 같아야 성공한다. 구형 host는 새 본문을 거부하고 구형 agent가
전체 ID 목록을 버린 경우에도 성공으로 오인하지 않는다. CLI·agent·host를 함께 갱신한다.
새 Front 요청·Agent link 사건·기능 플래그·의존성은 추가하지 않았다.

## 꼭 필요한 시험

- host `automation_completion_batch_is_independent_of_post_quota_and_mute`: 장애 때와 같은 74개를 한 번에
  기록하고 단건도 74번 등록한다. 완료를 알림 한도에 넣으면 뒤 일반 post의 한도 경계에서 실패한다.
  실제 rate 거부로 음소거를 만든 뒤에도 새 묶음을 받으며, 음소거 자체는 유지하는지 본다.
  동기식 수신·저장 반환 뒤 inbox와 post 목록이 비어 있는지 확인한다.
- host `automation_completion_batch_rolls_back_invalid_ids_reports_and_conflicts`: 유효한 ID 뒤에 없는 ID나
  다른 thread ID를 섞어 앞부분만 저장하는 결함을 잡는다. 보고 오류·빈 묶음·ID 중복과 결과 충돌도 본다.
- m13 `automation_completion_batch_from_muted_pane_records_all_ids`: 실제 pane 자식 CLI가 peer+token agent와
  host를 거쳐 음소거 중 단건 및 중복이 섞인 74개 묶음을 등록한다. CLI 종료값·전체 영수증·74개 원장·
  실행 상세와 post 목록을 비교한다. 가짜 셸이 명령과 영수증 읽기를 끝낸 출력 사건을 기다린다.
  큐 기동은 기존 시험이 담당하므로 이 시험은 전달된 입력 원장만 fixture로 준비한다.

## 실행 결과

- 새 host 회귀 3개(기존 인증 시험 포함): 통과, 합계 0.74초.
- 새 실제 전송 E2E: 통과, 1.32초.
- 기존 post 시험 15개: 통과, 합계 1.54초.
- 실행 원장 시험 2개: 통과, 합계 0.03초.
- 웹 `ci-local --only web`: 빌드·타입 검사·유닛 시험 655개 통과.
- `cargo clippy --profile verify --workspace --all-targets --locked --keep-going -- -D warnings`: 통과.
- agent post 시험 2개: 통과, 합계 0.52초.
- proto 시험 50개·crate 의존성 시험 2개·CLI/실행 파일 시험 101개: 모두 통과.
- 결함 되심기 2건: 각각 의도한 단언에서 실패했고 제품 소스는 SHA-256을 대조해 바이트로 복원했다.
  완료 경로에 inbox 등록을 넣으면 `is_independent_of_post_quota_and_mute`의 빈 inbox 단언이 실패한다.
  입력마다 COMMIT을 끼워 부분 저장을 만들면 `rolls_back_invalid_ids_reports_and_conflicts`의
  첫 유효 ID 미처리 단언이 실패한다. 실패 로그를 보존했다. 복원 뒤 post 15개(1.54초)와 E2E(1.45초), fmt·workspace clippy가 다시 통과했다.
- `scripts/check-public.sh`와 `git diff --check`: 통과.

명령은 `cargo test --profile verify --locked`에 각각 `-p aaa-host --lib post::tests::`,
`-p aaa-host --lib db::automation_runs::tests::`, `-p aaa-agent --lib post::tests::`,
`-p aaa-proto`, `-p aaa --test deps`, `-p aaa --bin aaa`를 붙인다.
새 E2E는 `-p aaa --test m13 automation_completion_batch_from_muted_pane_records_all_ids`로 고른다.

## 참고 구현과 라이선스

- [GitHub Actions runner의 JobServerQueue](https://github.com/actions/runner/blob/main/src/Runner.Common/JobServerQueue.cs)
  ([MIT](https://github.com/actions/runner/blob/main/LICENSE)): console 출력과 timeline/result 갱신 큐를
  구분하는 구조를 참고해 완료 저장을 사용자 알림 처리에서 분리했다.
- [Redis 7.2의 MSETNX](https://github.com/redis/redis/blob/7.2/src/t_string.c)
  ([BSD-3-Clause](https://github.com/redis/redis/blob/7.2/COPYING)): 여러 키 중 하나의 조건 실패가
  전체 변경을 막는 계약을 비교해 완료 묶음도 전부 기록되거나 전부 취소되게 했다.

외부 코드를 복사하지 않았고 새 라이브러리를 추가하지 않았다. 원본 실패 로그와 최종 검증 로그는
해당 작업 보고의 library에 남긴다.


## 정규 착지 준비

q-1851 1A와 착지 창에 따라 main `7f2bf719`에서 후보 끝 `91c40372`를 no-ff로 통합한다.
main의 마지막 계획 §8.543 다음 §8.544를 추가하고 FRONT_API 1.148·AGENT_LINK 1.11과 웹 E2E123까지를 보존한다.
새 Front 요청·Agent link 사건·웹 E2E는 없으며 Rust m13에 전송 계약 시험을 둔다.
testing.md의 충돌은 양쪽 링크를 모두 보존하고 해당 문서에 추가하는 내용은 링크 한 줄뿐이다.
전체 ci-local에는 salt를 넣지 않고 LLVM PATH·작업 폴더의 Playwright 1.64.1과 공용 브라우저 캐시를 쓴다.
제품 검증의 첫 실패는 원문·uptime을 보존하고 재시도 없이 멈춘다.

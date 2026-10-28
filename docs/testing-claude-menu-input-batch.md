# 가짜 Claude 메뉴의 답 묶음 처리(t-1236)

## 실패와 원인

- 기준 `e321baab`의 전체 검증에서 m13 `claude_thread_states_questions_and_messages`가
  원래 다섯 질문 사이에 `(trust, -)` 하나를 더 남겨 실패했다. 최초 로그·실험실은 t-1234가 보존했다.
- 보존 로그를 읽기 전용으로 대조했다. 첫 Question은 답보다 먼저 왔고, 답 4바이트의 PTY 쓰기는 완료됐다.
  agent는 입력 수신에서 867ms 뒤 같은 Question을 다시 알렸다. 이후 Idle이 그 질문을 Gone으로 닫았다.
  첫 관측이 답보다 늦는 t-1212와 다른 순서다. 실제 Claude 설치·로그인을 쓰는 시험도 아니다.
- 가짜 메뉴는 `dd bs=3 count=1`로 화살표만 읽고, 셸의 선택 문자열·화면 조립·출력·`stty`·다음 `dd`를 거쳐
  이미 TTY에 도착한 Enter를 읽었다. 그 사이 남은 신뢰 창은 agent의 원래 800ms 재질문 경계를 넘을 수 있다.
  단독 원본 m13은 13.30초에 통과했으므로, 그 실행을 최초 실패의 재현이나 수리 증거로 삼지 않는다.
- 실제 PTY에 화살표와 Enter를 한 번에 쓰는 회귀를 만들었다. 원래 fixture는 선택을 옮긴 질문을 다시 그려
  제목 관측 횟수 `2 != 1`로 0.12초 안에 실패했다. CPU 부하·고정 sleep 없이 잘못된 중간 화면을 재현한다.
  최초 실패 당시 각 셸 자식의 실행 시간까지 측정한 것은 아니므로 머신별 지연의 세부 원인은 단정하지 않는다.

## 수리와 지키는 계약

- `crates/aaa/tests/fixtures/claude-menu.py`가 메뉴를 그리기 전에 입력 모드를 준비하고,
  같은 입력 묶음의 화살표·Enter를 한 프로세스에서 처리한다. 대기 중인 Enter보다 선택 화면을 먼저 그리지 않는다.
  화면 한 번은 PTY 쓰기 한 번이며, shell은 fd 3으로 화면을 내보내고 stdout으로 선택 결과를 받는다.
- 화살표만 오면 확인창을 유지하고 선택만 옮긴다. Enter는 원래 선택을 돌려준다.
  단독 Esc는 suffix 판별 전에 확인창을 지우고 Working을 그린다. 나눠 온 화살표 suffix도 처리한다.
  Python은 기존 m13의 다른 fixture도 사용하는 의존성이다. 이 helper는 Unix m13에만 쓰인다.
- 새 시험 `claude_menu_consumes_a_batched_answer_before_redrawing`은 입력 묶음 사이의 재그림 회귀를 막기 위해 필요하다.
  기존 `claude_menu_arrow_preserves_the_question_until_enter`와
  `claude_menu_dismisses_escape_before_waiting_for_its_suffix`는 함수 대역 대신 실제 PTY 키와 출력으로 같은 계약을 검사한다.
  `Terminal::read_raw`는 Enter를 덧붙이지 않는다. 기존 `read` 호출의 줄 단위 동작은 유지한다.
- 제품 코드·원래 m13의 정확한 질문/메시지 이력·agent의 800ms 규칙·시험 시한은 바꾸지 않았다.
  새 고정 sleep·재시도·시험 삭제는 없다. 메뉴의 Esc suffix 판별은 기존 200ms 입력 상한을 유지한다.

## 참고 구현

- [herdr v0.9.5 감지](https://github.com/herdrdev/herdr/blob/v0.9.5/src/pane/agent_detection.rs)의
  보이는 막힘 800ms 갱신을 확인했다([Apache-2.0](https://github.com/herdrdev/herdr/blob/v0.9.5/LICENSE)).
- [herdr-web-ui 고정 판의 prompt.ts](https://github.com/devswha/herdr-web-ui/blob/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741/server/prompt.ts)의
  `askingEnded`와 `closeQueue`는 답 뒤 남은 같은 질문을 재질문으로 구분한다
  ([MIT](https://github.com/devswha/herdr-web-ui/blob/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741/LICENSE)).
  두 구현의 기존 감지·재질문 의도를 유지하고, fixture의 입력 처리만 바로잡았다. 외부 코드를 복사하지 않았다.

## 검증

- 수리 전 새 PTY 회귀: 실패(`2 != 1`, 0.12초). 수리 뒤 메뉴 3개 PTY keeper: 통과.
- 기존 keeper를 실제 키 사건으로 옮기면서 선택 결과 출력 한 줄을 빠뜨려 1개가 실패했다.
  결과 출력을 복구해 원래 선택값 단언을 통과시켰으며, 시한이나 단언은 완화하지 않았다.
- `nice -n 15 cargo test --profile verify --locked -p aaa --test m13 -- claude_menu_ claude_thread_states_questions_and_messages thread_read_and_keys thread_adopt_claude_started_in_a_shell --nocapture`:
  6개 통과, 13.49초. 새 메뉴 keeper 세 개는 앞선 묶음에서 전체 0.26초 이내였다.
- `nice -n 15 cargo test --profile verify --locked -p aaa --test m13 -- --test-threads=4 --nocapture`:
  148개 통과·기존 ignore 1개·실패 0개, 178.67초. 원래 실패한 질문/메시지 시험도 이 묶음에서 통과했다.
- `cargo fmt --all --check`·`git diff --check`·`node web/test/fixed-sleeps.mjs`·`bash scripts/check-public.sh` 통과.
  고정 sleep 수는 늘지 않았고 공개 검사는 충돌 표지 네 종류도 포함한다.
- `nice -n 15 cargo clippy --profile verify --locked -p aaa --tests -- -D warnings` 통과.
  `nice -n 15 cargo test --profile verify --locked -p aaa --test deps`의 의존 경계 2개도 통과했다.
  이번 범위는 Unix m13 fixture이며 웹·Windows 제품 경로는 변경하지 않았다.
  전체 ci-local은 착지 창에서 최신 main과 통합한 뒤 수행한다.

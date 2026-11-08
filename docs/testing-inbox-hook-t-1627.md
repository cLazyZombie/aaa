# 턴 도중 inbox hook 알림 검증(t-1627)

## 변경과 경계

coordinator 맨 위 agent의 PostToolUse hook이 host에 `inbox_hook_notice`(Front API 1.165)를 묻고, 아직 알리지 않은 처음 보는
inbox 항목을 `[aaa inbox] …` 한 줄로 `additionalContext`에 붙인다. 알린 항목은 host 메모리와 묶음 hash에 남아 턴 끝 nudge가
다시 보내지 않는다. 턴 끝 nudge 기본값은 `nudge_idle_secs = 3`·`nudge_quiet_secs = 2`이며 시간 조건만 남으면 ticker가 그때 깬다.
thread hook, 입력창이 빌 때만 보내는 검사, 질문 누락 즉시 알림은 그대로다. 설계는 plan §8.561에 있다.

공통 Rust·protocol·생성 TypeScript 변경이며 Windows 전용 경로·production 산출물은 바꾸지 않았다.

## 꼭 필요한 시험

| owner | 지키는 계약 | 왜 필요한가 |
| --- | --- | --- |
| host `inbox::hook_tests::hook_tells_each_new_item_once_and_turn_end_nudge_skips_what_hook_told` | hook은 새 항목만 한 번씩, thread·자동화·다른 pane은 거절, hook이 알린 묶음을 턴 끝 nudge가 다시 보내지 않음, 마지막 도구 뒤 도착한 항목은 nudge가 보냄, nudge가 보낸 항목은 hook이 다시 붙이지 않음 | 중복 알림과 알림 가로채기를 실제 Front 요청·nudge 전달 경로에서 잡는다 |
| host `inbox::hook_tests::turn_end_nudge_wakes_the_ticker_instead_of_waiting_for_the_period` | tick 주기 60초에서도 쉼·입력창 확인 시간(1초·1초)만 지나 알림이 감 | 턴 끝 지연이 주기 tick으로 되돌아가는 회귀를 잡는다 |
| E2E100 첫 단계 | 실제 coordinator pane의 `aaa hook --agent claude` PostToolUse → agent → host 왕복으로 `[aaa inbox]` 줄이 붙고, 다음 도구 결과에는 다시 붙지 않음 | agent의 project·pane 해석, 세션 비교, host 권한 판정을 실제 실행 파일로 확인한다 |

두 host 시험은 각각 수리 부분을 되돌리면 실패하는 것을 확인했다(ticker 깨움을 빼면 둘째가, hook의 묶음 hash 기록을 빼면 첫째가 실패).
새 대기는 사건(가짜 agent가 받은 입력)과 5초 실패 상한을 쓰며 고정 sleep은 없다. 시험 삭제·기대값 완화·시한 확대는 없다.

## 기존 시험에 미치는 영향

턴 끝 기본 대기가 60초·10초에서 3초·2초로 줄어 가짜 Claude 입력 상자를 그리는 coordinator가 있는 시험에 알림이 더 일찍 들어갈 수 있다.
그래서 관련 crate 전체와 웹 E2E 전체를 돌려 기존 단언이 그대로인지 확인했다. 기존 시험은 고치지 않았다.

## 실행

검사는 모두 `aaa verify -- <명령>`으로 실행했다. 미커밋 개발 상태라 공통 원장은 출처 미확인으로 자동 수집하지 않았다.

| 명령 | 결과 |
| --- | --- |
| `scripts/verify.py develop --package aaa-host --package aaa-agent --package aaa-proto --package aaa-config --package aaa --spec 100,117` | 다섯 crate 시험(aaa-host lib 871개 포함)·strict Clippy·E2E100·E2E117(Chromium·WebKit 전 조합)·공개 검사 통과 |
| `scripts/ci-local.sh --only web,e2e,public` | 웹 유닛 714개, 웹 E2E 전체 39개 파일(50개 작업, 147초), 공개 검사 통과 |
| `cargo fmt --all -- --check` | 통과(첫 실행 뒤 서식만 맞춤) |

Playwright는 작업 폴더의 1.64.1 패키지를 `NODE_PATH`로 가리켰고 브라우저는 공용 캐시를 썼다.

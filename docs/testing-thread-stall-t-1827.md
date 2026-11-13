# thread 멈춤 감시와 자동 재개 검증

## 계약과 owner

- `aaa-host/src/thread_stall.rs`: ticker의 idle 멈춤 판정, 화면 마지막 결과, DB 중복 방지와 재개 예산, 기존 harness 입력 경로.
- `aaa-config`: `[projects] thread_stall_secs` 기본 90초, 0으로 감시·재개 끔. protocol·agent·session 의존 경계는 그대로다.
- `aaa-host/src/guide.rs`의 threads·prompts와 [사용법](usage.md#thread-멈춤-감시와-일시-오류-재개)을 함께 갱신한다.

기존 `transitions.rs::thread_pass`는 묶음이 확인 필요·쉼으로 바뀔 때와 새 보고를 알린다. 진행률이 미완료인 idle은 5분간
작업 중으로 남을 수 있다. 이번 감시는 같은 ticker에서 독립적으로 관측하고 `thread stalled`를 낸다. 확인창은 기존 30초
전이 경로다. 보고 회수 중·재연결 중은 회복으로 취급하지 않아 동일한 idle의 알림 기록을 지우지 않는다.

## 참고한 공개 구현

코드를 복사하거나 의존성을 추가하지 않고 아래 판정 원칙만 참고했다.

- [Agent of Empires 상태 감지](https://github.com/agent-of-empires/agent-of-empires/blob/main/src/tmux/status_detection.rs),
  [MIT 라이선스](https://github.com/agent-of-empires/agent-of-empires/blob/main/LICENSE): 화면의 현재 활동 표시와 입력창 근처
  영역으로 상태를 판정한다. aaa는 이미 받은 harness 상태를 먼저 확인하고 마지막 결과만 오류 근거로 삼는다.
- [tmux 무출력 감시](https://github.com/tmux/tmux/wiki/Advanced-Use),
  [ISC 계열 라이선스](https://github.com/tmux/tmux/blob/master/COPYING): 출력이 없는 시간을 재는 개념을 참고했다.
  aaa에서는 무출력 자체로 판정하지 않고 idle·미완료 작업·보고 갱신·배경 작업을 함께 본다.
- [Supervisor 재개 제한](https://supervisord.org/configuration.html#program-x-section-values),
  [라이선스](https://github.com/Supervisor/supervisor/blob/main/LICENSES.txt): 제한을 넘긴 자동 시도를 중단하고 사람이 확인하게 한다.
  aaa는 프로세스를 재시작하지 않고 동일 대화에 고정 문장을 보내며, 최근 10분에 최대 3회만 시도한다.

## 판정 사례

| 입력·상태 | 기대 |
| --- | --- |
| 작업 의도 + idle, 90초 전 | 대기 |
| 90초 뒤, 같은 idle 반복·DB 재읽기 | 멈춤 알림 한 번 |
| 새 보고·진행 보고 | 90초를 다시 잼 |
| 마지막 결과의 일시 연결 오류 | 멈춤 알림 + 고정 문장 재개 한 번 |
| 사용량·rate limit·인증 거부·panic | 알림만 |
| 옛 오류 뒤 새 응답·새 입력 | 옛 오류로 자동 재개 안 함 |
| 정상 답변에서 오류 용어를 설명 | 자동 재개 안 함 |
| 진행 표시·잘린 추론 제목·배경 작업 | 멈춤 아님 |
| 검수·결정·완료 보고, 질문·보류·종료 | 감시 제외 |
| 초안·모르는 입력창 | 멈춤 알림만, 자동 입력 안 함 |
| 10분 내 네 번째 멈춤 | 알림만; 예산 초과 표시 |
| 새 턴의 멈춤, 이전 시도가 10분 밖 | 예산 회복 |

단위 시험에 `now`를 직접 준다. 실제 host pass 시험은 동기화된 VT 사본과 가짜 agent link를 붙여 입력 바이트·수락 hook·
inbox 사건을 확인한다. 준비·종료에 정한 시간을 자지 않는다. 제한·중복 방지는 DB 직렬화 왕복을 포함한다.

## 결과

- 새 판정·예산·host 입력 경로 시험 4개는 통과했다. host 실행 경로는 실제 입력 바이트와 수락 hook까지 확인했다.
- 관련 owner 첫 실행은 coordinator 기본 안내 길이 제한에 걸렸다. 중복 안내를 제거하고 자세한 내용은 threads·prompts에 뒀다.
  길이 제한과 다른 시험의 기대값은 바꾸지 않았다.
- 관련 owner: `aaa-config` 41개·`aaa-host` 910개 통과, 기존 ignored 2개 유지. 두 crate의 all-targets strict Clippy와 공개 검사도 통과했다.
- 전체 검증은 `aaa verify -- nice -n 15 bash scripts/ci-local.sh`로 실행한다. 실행 산출물과 단계별 결과는
  `target/ci-local/`에 남기고, 병합 준비 보고에서 해당 실행 결과를 가리킨다. 착지 창의 최종 통합 SHA gate와 배포는 별도다.
- 자체 코드 리뷰에서 재연결·VT 재동기화를 새 멈춤으로 세지 않도록 기록을 보존하고, 전송 잠금·화면 RPC 뒤에 보고와
  idle을 다시 읽도록 보강했다. 실제 host 경로 시험이 이 경계와 초안 보호를 검사한다.

최종 점검에서 입력창을 못 읽는 화면도 일반 멈춤 알림은 유지하도록 보강했다. 입력창 모름은 일시 오류 재개 근거로 쓰지 않으며 host 경로 시험에서 입력 없이 알림만 남는 것을 단언한다.

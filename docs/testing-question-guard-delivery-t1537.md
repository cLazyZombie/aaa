# 질문 안내 오탐과 메시지 수락 수리 검증

q-2810 1A로 조사 후속 두 범위를 구현했다. 제품 규칙은 [질문 카드](questions.md#coordinator의-카드-누락-확인)와
[메시지 수락 확인](usage.md#메시지-수락-확인), 설계는 [plan §8.556](plan.md#8556-질문-안내-오탐과-windows-메시지-수락-수리t-1537)에 둔다.
새 의존성·프로토콜 필드·웹 E2E 번호는 추가하지 않았다.

## 지키는 계약

- 지금 필요한 사용자 결정·정보 요청은 계속 감지한다. 선택지가 없는 요청, 존재하지 않는 번호와 다른 프로젝트의 번호,
  기존 카드 뒤의 별개 요청을 포함한다.
- 사용법·처리한 목록·현재 답이 필요 없는 조건부 제안과, 같은 주제의 여러 문단에 나뉜 카드 안내는 새 누락으로 보지 않는다.
- 정상 접수 뒤 빠르게 답한 카드의 증거는 남고, 닫힘·교체는 별도 안내로 분리한다. 진단에는 규칙·행·q-ID 상태를 남기며
  본문을 복사하지 않는다.
- Windows Codex의 긴 붙여넣기는 비문자 경계를 거친 뒤 Enter 한 번으로 제출한다. 경계 사이에 승인 창이 뜨면 제출하지 않는다.
- 작업 중이라는 이전 상태나 과거 같은 본문의 기록·기존 큐 항목 소비는 이번 보내기의 수락 근거가 아니다.
  같은 실행 세대·harness 세션·기록의 새 본문 또는 새 큐 영수증을 확인한다. 구분할 수 없으면 Unknown으로 남긴다.
- 원래 붙여넣기 간격과 수락 시한은 유지한다. 본문·추가 Enter를 자동 재전송하지 않는다.

## 재현과 수리

기준 `ca98a8de`의 Windows 실행 파일과 실제 Codex CLI 0.166.0을 격리 host/agent의 ConPTY에서 실행했다.
실제 계정·운영 서비스 대신 가짜 인증과 loopback Responses fixture를 사용했다.

짧은 입력은 수락됐지만, 여러 줄 1,097자 입력은 Unknown이 됐다. 입력창에는 `Pasted Content 1098 chars`가 남았다.
제출용 Enter 한 번이 본문의 줄바꿈으로 들어간 관측이다. Codex의 paste-burst 경로가 Enter를 줄바꿈으로 처리하며
비문자 입력이 버퍼와 억제 상태를 확정하는 소스와 일치한다.

Windows Codex에 본문을 넣은 뒤 Right 키로 확정하고 승인 창을 다시 검사한 다음 Enter를 한 번 보낸다.
Right는 새 입력 끝에서 본문을 바꾸지 않는다. 최종 실기에서는 대화 원문과 보낸 본문을 정확히 비교하고 한 번만 기록되는지,
전송 진단의 붙여넣기·확정 경계·Enter·수락이 각각 한 번인지 확인한다.

실제 재현을 보존한 채 검사 도구도 고쳤다. 처음 도구는 실행 중인 rollout 파일을 공유 없이 열어 파일 잠금 오류로 멈췄다.
ReadWrite 공유 읽기로 고친 도구에서 위 Unknown을 재현했다. 제품 실패를 다시 돌려 성공으로 채택하지 않았다.
Rust 개발 중의 A·B 목록 오판과 시험 코드의 Digest import 누락도 원인을 고친 뒤 확인했다. 답한 카드의 재안내 계약은
새 요구대로 바꾸되, 그 뒤의 별개 새 승인 요청이 여전히 경고되는 단언을 함께 유지했다.

## 관련 owner

모든 명령은 `aaa verify -- <명령>`으로 실행한다. 무거운 명령은 `nice -n 15`로 실행했다.

| 범위 | 명령·owner | 결과 |
| --- | --- | --- |
| 질문 감지·수락·세션·키 | `cargo test --profile verify --locked -p aaa-host --lib -- question_guard harness::tests:: prompt_receipt` | 33개 통과 |
| 네이티브 Rust lint | `cargo clippy --profile verify --locked -p aaa-host --all-targets -- -D warnings` | 통과 |
| Windows Rust lint | `cargo xwin clippy --profile verify --locked -p aaa-host --all-targets --target x86_64-pc-windows-msvc -- -D warnings` | 통과 |
| 웹 타입·번들·유닛 전체 | `bash scripts/ci-local.sh --only web` | 통과 |
| 실제 host/브라우저 | `bash scripts/ci-local.sh --only e2e --spec 37,53,69,117` | 통과 |
| Windows 실제 Codex | `scripts/test-windows-codex-onboarding.ps1 -Messages` | 통과 |
| crate 의존 경계 | `cargo test --profile verify --locked -p aaa --test deps` | 통과 |
| 공개 저장소 | `bash scripts/check-public.sh` | 통과 |

웹 E2E는 기록 페이지·thread 카드·coordinator 카드·inbox 입력 중 알림의 기존 owner를 그대로 실행한다.
작업 폴더에 고정한 Playwright 1.64.1과 공용 브라우저를 사용하며 전역 설치·사용자 설정은 바꾸지 않는다.

새 Rust 시험은 승인 창을 끼운 실제 송신 경계, 같은 본문의 옛/새 영수증·이전 큐 소비·실행 세대 구별,
빠른 답 뒤 post 접수 유지, 닫힘과 다른 프로젝트 참조, 문구·문단 회귀를 검증하기 위해 필요하다.
기존 질문 guard의 단일 문구 시험을 의미별 세 시험으로 나누고 기존 post/harness owner를 보강했다. 삭제한 계약은 없다.
시계 시험은 Tokio 정지 시계와 실제 사건을 사용하고, 재시도·고정 sleep·시한 확대는 추가하지 않았다.

## Windows 실기 owner

다음 형태로 작업 전용 폴더 안의 실행 파일만 사용한다.

```powershell
pwsh -NoProfile -File scripts/test-windows-codex-onboarding.ps1 `
  -Aaa C:\aaa-test\t-0001\aaa.exe -Codex C:\aaa-test\t-0001\codex.exe `
  -Lab C:\aaa-test\t-0001 -Messages
```

기존 최초/재시작 brief와 실제 명령 실행 계약을 유지한다. 두 실행에서 64·1,046·4,096자 꼬리를 포함한 여러 줄 메시지를 각각 보내고,
모델 응답을 사건으로 보류한 상태에서 두 메시지를 더 보내 작업 중 전송과 턴 경계를 확인한다. 총 메시지 8개다.
API 응답을 푸는 시점은 작업 중 수락 준비 사건으로 정하며 정해 둔 시간을 자지 않는다.
각 본문은 실제 user 기록에 정확히 한 번 존재해야 하고, 작업 중 후속은 영수증 근거도 있어야 한다.

추가 실행이 필요한 새 후보는 `-RunSuffix final`처럼 별도 결과 폴더를 쓴다. 기존 실패 폴더는 덮어쓰지 않는다.
실제 PID와 시험 경로를 확인한 프로세스만 정리한다. 서비스·예약 작업·Run 키·사용자 PATH·방화벽 기준 목록은 시작 전과 끝난 뒤 대조한다.

이번 정리에서는 시험 프로세스 0·시험 폴더 삭제를 확인했다. 서비스·예약 작업·Run 키·방화벽의 분류별 지문도 같았다.
사용자 PATH는 버전 고정 Scoop 경로 10개가 앞에 추가된 차이가 있었다. 그 접두를 제외한 문자열의 지문은 시작 전과 정확히 같았고,
시험 경로는 없었다. 변경 주체는 확정하지 못해 원복하지 않았다. 시험 도구의 환경 변경은 Process 범위이며 실제 shell 호출도
`login=false`였음을 기록에서 확인했다. 이를 환경 불변 검사 통과로 적지 않는다.

이 실행 파일은 검증용이다. Windows production·PDB 전달 검사나 착지 gate를 수행한 것으로 기록하지 않는다.
최종 통합 SHA의 gate와 실제 배포 파일 검증은 착지 창에서 별도로 한다.

## 조사 자료 재생의 한계

조사한 완료 답 94개를 현재 순수 감지 함수에 재생했다. 실제 카드 안내로 판정한 참조는 알려진 카드로 입력했다.
비질문 15·기존 카드 안내 40·조건부 32는 새 누락 안내 없이 처리됐고, 카드 없는 실제 요청 7은 유지됐다.
이는 과거의 전체 host/DB 상태를 복원한 시험이나 새 운영 오탐률 측정이 아니다. 자연어의 모든 의도를 판별한다는 보장도 없다.
개인정보가 있는 원문을 저장소 시험 fixture나 보고 첨부에 옮기지 않았다.

## 착지에서 발견한 Claude 큐 회귀와 수리

첫 통합 gate의 `claude_thread_states_questions_and_messages`는 작업 중 두 번째 메시지를
`accepted`로 확인하는 기존 단언에서 실제 `unknown`을 받아 실패했다. 첫 실패 원문과 실험실을 보존하고
그 gate를 중단했다. 재실행·시한 확대·기대값 완화로 넘기지 않았다.

이번 변경은 기존 Working 재관측을 수락 증거로 쓰지 않는다. 기존 가짜 Claude는 큐를 메모리에만 쌓았고
SessionStart의 기록 경로·enqueue를 내지 않아 영수증을 읽을 수 없었다. 실제 Claude의 큐 형식을 fixture에 추가했다.
기존 수락·턴 순서·보류 계약의 단언과 시한은 유지하고, 두 번째 메시지의 `queued_receipt` 사건 단언을 더했다.

agent는 사람이 쓴 pending 글에 전체 본문 해시와 대화 파일·enqueue byte 위치 ID를 붙인다. host는 그 ID가
전송 전 기준보다 새로운지 확인한다. 같은 본문이 이미 큐에 있어도 새 enqueue만 수락하며, 기존 항목 소비를
새 수락으로 보지 않는다. 표시 축약·시스템 알림 제외는 유지한다. 선택 필드의 값만 채우며 API 판은 바꾸지 않는다.

회귀 owner는 다음과 같다.

- `chat::tests::claude_queue_receipts_identify_each_enqueue_before_and_after_consumption`: 긴 본문의 전체 해시,
  동일 본문 두 enqueue의 서로 다른 ID, 앞 항목 소비 뒤 남은 ID 보존을 실제 파일로 확인한다.
- `prompt_receipt::tests::only_new_matching_receipts_in_the_same_execution_accept_a_submission`: 새 큐 ID만 수락하고
  기존 본문·옛 세션·다른 실행 세대·기존 큐의 소비는 거부한다.
- `harness::tests::a_busy_submission_requires_its_new_receipt_instead_of_the_previous_working_state`: Claude·Codex 각각
  새 영수증이 없을 때 Unknown, 있을 때만 Accepted임을 원래 수락 시한의 정지 시계로 확인한다.
- 기존 m13 `claude_thread_states_questions_and_messages`: 전체 host·agent·PTY와 실제 JSONL 큐를 잇는다.
  두 번째 메시지가 첫 턴 종료 전 수락되고 다음 턴으로 순서대로 처리되는 기존 단언에 영수증 증거를 추가한다.

최종 gate·cross·Windows production의 결과와 전달 SHA는 착지 보고에 남긴다. 첫 실패를 성공으로 바꾸어 기록하지 않는다.

## ID 있는 pending 큐와 웹 페이지 병합

Claude 큐 ID 추가 뒤 새 통합 gate에서 Rust 전체는 통과했지만 E2E 23의 「흡수 뒤 보통 말」이 실패했다.
`mergeChatPage`가 모든 ID 있는 항목을 누적해, incoming에서 사라진 pending 큐까지 보존한 것이 원인이다.
첫 실패 원문을 보존하고 그 gate에서 중단했다.

완료된 ID 기록만 누적하고 pending은 ID 유무와 관계없이 최신 snapshot에서 가져오도록 고쳤다.
앞 페이지 응답에는 현재 최신 큐를 유지하며, 기록 cursor의 처음 위치도 완료 기록에서만 찾는다.
같은 본문을 다른 큐·완료 말로 보낸 경우를 본문으로 합치지 않고, 파일 교체 시 이전 기록을 버린다.

새 `web/test/chatPages.test.mjs`는 큐 흡수 뒤 옛 ID·pending 제거와 보통 말 1개 유지, 동일 본문 일부 소비,
늦은 앞 페이지와 최신 큐·처음 cursor 보존, ID 없는 옛 큐와 파일 교체의 네 경우를 확인한다.
기존 E2E 23의 pending 표지·단추·설명 줄 제거와 정확히 한 번 표시하는 단언·시한은 그대로다.

이 수리는 전체 gate 전에 웹 타입·번들·웹 유닛과 웹 E2E 전부를 실행한다. 큐·메시지·질문 spec을 일부 고르지 않는다.
Rust는 agent chat, host question_guard·harness·prompt_receipt, m13 Claude owner를 먼저 확인한다.
최종 SHA의 전체 gate와 OS cross·Windows production·전달은 이 개발 확인을 마친 뒤 별도로 기록한다.

## 참고 구현과 라이선스

- [Codex chat composer](https://github.com/openai/codex/blob/main/codex-rs/tui/src/bottom_pane/chat_composer.rs)와
  [paste burst](https://github.com/openai/codex/blob/main/codex-rs/tui/src/bottom_pane/paste_burst.rs):
  [Apache-2.0](https://github.com/openai/codex/blob/main/LICENSE). 비문자 키가 입력 버퍼·Enter 억제를 확정하는 경계를 참고했다.
- [tmux input keys](https://github.com/tmux/tmux/blob/master/input-keys.c):
  [ISC](https://github.com/tmux/tmux/blob/master/COPYING). bracketed-paste 모드를 구별하는 방식을 비교했다.

외부 구현 코드는 복사하지 않고, 기존 aaa의 터미널 키 인코더와 ChatReceipt를 사용했다.

# macOS Intel 지정 계정 예약 재연결 진단

[fabc2557의 Intel CI](https://github.com/cLazyZombie/aaa/actions/runs/44854273377/job/136692649812)의 `m13-81`은 host 재시작 뒤 `labA` 연결을 5초 안에 관측하지 못해 종료 코드 101로 끝났다. 같은 실행의 host 단위 시험에서는 지정 계정·선행 할 일의 DB 복원과 지정 인증·한도 대기가 통과했다.

| 단계 | 원래 실행에서 확인한 근거 |
| --- | --- |
| 재시작 전 | CLI 예약과 tick 뒤 계정 `chosen`·선행 할 일·미기동 단언을 통과했다. 실패 위치는 이후의 재연결 대기다. |
| 재시작 | 새 host는 02:41:07.789 UTC에 TLS listener를 열었다. |
| 재연결 | host는 02:41:09.194 UTC에 로컬 agent 연결을 기록했고 02:41:09.917 UTC에 끊김을 기록했다. |
| 조회 | 시험은 `machine list`의 조회 오류와 마지막 연결 상태를 버렸다. 기존 기록으로는 첫 조회 시각과 실패 직전의 응답을 재구성할 수 없다. |
| 복원·대기·기동 | 재연결 대기에서 실패해, 선행 할 일 완료와 인증 복원 및 agent의 인증 수신 단계까지 가지 못했다. 연결 직후 인증 파일 조회에서 기록한 `NotFound`는 시험이 금고 키를 옮긴 상태와 일치한다. |

Rust 실행기의 `summary.json`에서 해당 분할은 12.15초, runner는 4 CPU 슬롯과 libtest thread 1개다. 1·5·15분 부하는 시작 전 `10.04 / 8.37 / 8.25`, 종료 뒤 `10.20 / 8.45 / 8.28`이었다. `events.jsonl`에서 다른 분할 세 개가 함께 실행 중이었고 인접한 재시작 예약 분할 `m13-80`은 통과했다. 부하는 지연 가능성을 보여 주지만 마지막 조회 지연이나 잘못된 상태의 원인은 확정할 수 없다.

t-2117의 질문 답 정정 변경에서 `FRONT_API`는 1.191로 올랐다. 시험의 host·agent·CLI는 같은 빌드이며 agent는 별도의 `AGENT_LINK 1.11`을 협상한다. 답 정정과 thread 상태 저장 변경은 이 실패까지 호출되는 예약·재연결·머신 목록 분기에 없다. 이 범위에서 변경과 실패를 연결할 증거는 없다.

## 다음 실패의 기록

기존 `machine list` 대기와 5초 상한을 쓰면서 각 조회의 시작 오프셋·경과·종료 상태·머신 연결 상태를 모은다. 분할 시험 로그에 마지막 10개와 그 밖으로 밀려날 수 있는 최근 조회 오류를 따로 남긴다. UTC 기준 시각과 실패 시점의 부하, host·agent 준비와 연결 표지를 함께 남겨 대기와 로그를 대조한다. 연결 로그에서는 시각과 고정 표지만 뽑는다. t-2114의 설치 PTY 계측과 같은 CI 분할 로그를 사용한다.

예약 시험은 선행 완료 전에 복원된 같은 할 일·계정·선행 조건을 확인한다. 기존 금고 잠금 대기를 먼저 확인하고, 금고 키를 돌려준 뒤에는 지정 계정 파일만 보류한다. 이 상태에서 CLI가 다른 계정의 인증을 확인해 자동 배정 풀에 넣어도 예약은 같은 계정을 기다려야 한다. 지정 계정 파일을 돌려주면 실제 agent의 가짜 harness가 수신한 인증을 비교해 성공 표지만 남긴다.

## 개발 검증

macOS arm64에서 `m13 task_dependencies::` 두 건이 통과했다. 지정 예약의 재연결과 복원, 금고 잠금 대기, 사용할 수 있는 대체 계정이 있는 상태에서의 미기동, 지정 인증 수신을 로그에서 순서대로 확인했다.
host의 `reserved_account` 두 건도 통과했다. 저장·레거시 복원과 인증·한도 대기를 각각 검사한다.

실제 재시작에서 잠깐 다른 머신 이름을 조회한 검증은 5초 상한에서 종료 코드 101로 끝났다. 마지막 조회에는 응답 성공과 대상 없음이 기록됐다. 그 앞의 `Connection refused`가 마지막 10회 밖에 있어도 최근 오류 항목에 남았고, host listener·agent 양쪽 연결 시각과 부하도 분할 로그에 있었다. 이 검증이 끝난 뒤 실제 시험은 원래 대상인 `labA`로 실행했다.

## 참고한 구현

실패 시점의 앱 로그를 함께 보는 pytest와 준비 사건의 시각을 기록하는 Kubernetes E2E 방식을 비교했다. 저장소의 Rust 대기와 실패 출력을 재사용했다. [pytest 로그 출력](https://docs.pytest.org/en/7.0.x/how-to/logging.html) · [MIT](https://github.com/pytest-dev/pytest/blob/main/LICENSE) · [Kubernetes E2E](https://github.com/kubernetes/kubernetes/blob/master/test/e2e_node/density_test.go) · [Apache-2.0](https://github.com/kubernetes/kubernetes/blob/master/LICENSE)

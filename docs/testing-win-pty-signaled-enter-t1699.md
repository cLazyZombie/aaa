# Windows 설치 PTY: 입력 사건이 남은 마지막 PATH 읽기(t-1699)

## 결론

plain 마법사의 마지막 PATH 질문에서 부모는 응답을 모두 보냈지만 열네 번째 `ReadConsoleW`가 반환하지 않았다.
실패 뒤에도 입력 큐 앞부분에 Enter KEY_DOWN이 있었고, 별도로 연 `CONIN$`의 zero-time wait는 signaled였다.
선택 flags는 0, 화면 버퍼와 window는 모두 120×36이었다. 터미널 질의 회신 누락도 관측되지 않았다.
**멈춘 경계는 확인했지만 근본 원인은 미확정이다. 수리 완료나 CI 회복으로 판정하지 않는다.**

할 일의 중단 조건에 따라 제품·시험·CI 구현은 바꾸지 않고 근거와 다음 진단 계획을 남긴다.
시한·기대값을 바꾸거나 실패 시험을 다시 실행하지 않았다.

## 확보한 자료와 동일성

- 기준은 main `5818484a651ac3f680f11eedcc8bedf86649fa88`, attempt 1이다.
- [Windows job 전체 로그](https://github.com/cLazyZombie/aaa/actions/runs/42736238910/job/129566059539)와
  `rust-tests-windows-2025-42736238910-1` artifact를 받았다. artifact ID는 `13728465095`다.
- artifact에는 `test.log` 하나만 있다. 별도 console server 추적·dump·PTY 원바이트 파일은 없다.
- 부모 입력 쓰기 14개, 터미널 질의 사건 2개, 정지 판정 문자열이 두 원문에서 일치했다.
  CLI 스택·콘솔 상태는 JSON으로 파싱해 전체 값이 일치하는 것도 확인했다.
  job 끝의 잘린 실패 요약 재출력은 JSON 대조에 쓰지 않고 정규 test 단계의 완전한 사건을 사용했다.
- OS는 Windows Server 2025 `10.0.26100`, image는 `windows-2025-vs2026`의 `20261103.298.1`이다.
  검증한 시스템 conhost의 고정 resource 판은 `10.0.26100.33648`이다.
- [t-1658의 진단](testing-win-pty-readiness-t1658.md)이 실패 SHA에 포함돼 있다.
  이번 작업의 시작 HEAD까지 설치·wizard·PTY helper 소스에는 차이가 없다.
- 원문 로그·압축 파일은 작업 전용 scratch에만 보존한다. 이 문서는 검토한 사건과 숫자만 옮겼다.

## 마법사와 승인 경계

대상은 `wizard::both_wizards_back_from_host_choices_then_submit_the_same_agent_plan_as_explicit_cli`다.
코드는 plain 다음 TUI를 실행한다. 실패에는 `prompt::windows::line`의 실제 read 사건이 있으므로
첫 plain 순회다. TUI 순회는 시작하지 않았다.

| 입력 순서 | 질문·행동 | 확인한 사건 |
| --- | --- | --- |
| 1–5 | 설치 방식·host 역할·이름·로컬 agent·프로젝트 | read 1–5 성공 반환 |
| 6–9 | PATH → 프로젝트 → 로컬 agent → 이름에서 뒤로 이동 | read 6–9 성공 반환 |
| 10 | 역할을 agent로 변경 | read 10 성공 반환 |
| 11 | 최종 머신 이름 | read 11 성공 반환 |
| 12 | 남길 여유 용량 | read 12 성공 반환 |
| 13 | harness 로그인 검사 | read 13 성공 반환 |
| 14 | 사용자 PATH에 preserve와 CR | 부모 쓰기·flush와 Enter 관측, read 14 미반환 |

제목·안내 출력이 아니라 read 반환과 다음 질문의 실제 호출로 뒤로 이동 및 재선택 진행을 확인했다.
부모의 `answered=14/14`는 쓰기를 끝냈다는 뜻이다. 콘솔이 마지막 응답을 소비했다는 뜻이 아니다.

driver의 다음 predicate는 `waiting=child_exit`, `needle=None`이다.
그러나 CLI의 실제 스택은 입력 읽기이므로 이를 자식 종료 뒤 출력 EOF 정체로 분류하면 안 된다.
`child_exit=None`, `reader_done=false`였고, 자식 종료 사건은 관측되지 않았다.

이 시험은 `--dry-run` 질문 단계다. 승인 질문·승인서 발급·적용 worker로 진행하지 않았다.
마지막 PATH 선택의 반영, plan 출력·명시 CLI와의 해시 비교도 도달하지 못했다.
따라서 시험 helper의 이름인 「승인 PTY」를 실제 승인 완료의 근거로 쓰지 않는다.

## 마지막 성공과 정지 시각

아래 UTC는 CLI 로그와 부모가 기록한 UTC다. 경과 시간과 섞지 않았다.
부모 UTC는 write 직전 표본이며 flush의 별도 UTC 표본은 없다.

| 사건 | 관측 |
| --- | --- |
| 마지막 성공 read 13 반환 | `01:42:34.699482Z`, success=true |
| read 14 입력 준비 | `01:42:34.702048Z`, original_mode=503, input_mode=502 |
| read 14 호출 직전 표지 | `01:42:34.702417Z` |
| 부모 마지막 write 직전 UTC | `01:42:34.702694Z` |
| 부모 마지막 write·flush 경과 | 시작 113,327µs, flush 완료 113,354µs |
| read 14 관측기 준비 | `01:42:34.707666Z` |
| read 14 Enter 관측 | `01:42:34.713120Z`, key_down·enter_down=true |
| 실패 확정 | 경과 60.0021102초 |
| 실패 뒤 helper 종료 | collection_ms=622, exit=0, response_ready |
| 실제 스택 결과 기록 | collection_ms=630 |

read 1–13의 성공 반환 표지는 모두 있고 read 14의 반환 표지는 없다.
마지막 관측의 cancel_down·eof_down은 false다. ConPTY의 줄 재표시로 나뉜 필드도 이어서 확인했다.
호출 직전 표지는 서버 내부 read 대기 등록 완료 표지가 아니다.
부모 flush와 관측기의 준비 순서만으로 OS 내부의 실행 순서를 확정하지 않는다.

## 추가 진단으로 확인한 상태

| 자료 | 관측 | 판단의 한계 |
| --- | --- | --- |
| 터미널 질의 | 시작 11,085µs에 cursor_position 수신, 11,106µs에 flush, answered=0, omitted=0 | 지원 질의에 대한 회신 기록이며 임의의 escape 전체를 판별하지 않음 |
| 입력 mode | 502 | 실패 뒤 표본이며 호출 전체의 mode 이력은 아님 |
| bounded Peek | records_present·prefix_key_down·prefix_enter_down=true | 앞 128개 사건의 긍정 관측이며 마지막 응답 전체가 남았다는 뜻은 아님 |
| 별도 CONIN$ wait | result=0, os_code=null | signaled. CLI의 원래 stdin handle 자체를 조회한 것은 아님 |
| 선택 정보 | status=ok, flags=0 | 수집 시점의 상태이며 과거 순간의 선택까지 배제하지 않음 |
| 화면 | buffer/window 모두 120×36 | 수집 시점의 크기이며 전체 cursor·줄 배치 이력은 없음 |
| conhost resource 판 | 10.0.26100.33648 | 디스크 파일 판이며 실행 모듈의 바이트 동일성 증명은 아님 |

콘솔 조회는 helper의 `console_input` 5ms 뒤 mode·Peek 9ms, wait·selection 10ms, screen 11ms에 시작했다.
각 API 조회는 순차 표본이며 하나의 원자적인 시점으로 다루지 않는다.
관측기 Enter와 실패 뒤 Enter가 정확히 같은 개별 사건인지도 식별할 수 없다.

CLI에서 수집한 thread는 하나이며 실제 스택은 다음 경계다.

```text
NtDeviceIoControlFile → ReadConsoleW
→ install::prompt::windows::line → install::prompt::line
→ install::wizard::ask → install::wizard::run
```

관측기 join·승인 worker·부모의 writer 대기 스택이 아니다.
conhost의 다섯 thread는 각각 WCT kind=8, status=3의 한 노드, cycle=false, os_code=0이다.
누적 CPU는 kernel과 user 각각 156,250×100ns다. 순간 부하·대기 오브젝트·read 등록 이력이 없어
이 값으로 바쁨·교착·깨우기 유실 중 하나를 확정하지 않는다.

[Microsoft의 입력 버퍼 계약](https://learn.microsoft.com/en-us/windows/console/low-level-console-input-functions)은
미소비 사건이 있으면 입력 handle이 signaled가 된다고 설명한다.
[ReadConsole 계약](https://learn.microsoft.com/en-us/windows/console/readconsole)은 cooked mode의 Enter 완료를 설명한다.
이번 관측은 입력 사건 존재와 read 미반환을 함께 보여 주지만, 어느 서버 내부 단계가 막혔는지는 보여 주지 않는다.

## 이전 실패와 비교한 판단

[t-1658 자연 실패](testing-win-pty-queued-enter-t1658.md)는 read 12, 여유 용량 입력 뒤 다음 질문을 기다렸다.
이번에는 그 read와 로그인 검사가 실제 반환했고 마지막 read 14에서 멈췄다.
같은 읽기 경계지만 특정 back 전환·역할·숫자 검증에만 묶인 실패라고 볼 수 없다.

이번에는 큐의 Enter뿐 아니라 signaled·선택 없음·정상 크기를 함께 확인했다.
시작할 때의 커서 질의는 답했고 이후 지원 질의의 미회신도 기록되지 않았다.
「부모가 마지막 응답을 안 보냄」·「자식이 끝난 뒤 출력 pipe EOF만 기다림」은 이번 자료와 맞지 않는다.
단, 별도 입력 handle의 signaled 상태를 근거로 원래 read의 통지 유실을 확정할 수는 없다.

[Microsoft Terminal의 입력 버퍼](https://github.com/microsoft/terminal/blob/main/src/host/inputBuffer.cpp),
[대기 큐](https://github.com/microsoft/terminal/blob/main/src/server/WaitQueue.cpp),
[cooked read](https://github.com/microsoft/terminal/blob/main/src/host/readDataCooked.cpp)를 읽어
입력 보관·대기 등록·통지·완료가 구별되는 것을 대조했다(MIT, 외부 코드 실행·도입 없음).
공개 main은 이번 inbox conhost와 같은 코드라고 입증되지 않았다.
이 구조에 비춰 서버 대기 등록·통지와 동시 Peek의 상호작용은 조사 후보라는 추론까지만 한다.

## 다음 진단 계획 — 미구현

1. Windows Server 2025 image `20261103.298.1`과 conhost `10.0.26100.33648`을 기준으로 환경을 확인한다.
   같은 판의 파일·모듈·symbol을 대응시킬 수 있는지도 먼저 확인한다. 접근 불가를 다른 OS 통과로 대체하지 않는다.
2. 기존 수동 fixture와 제품의 차이를 줄인다. 현재 fixture는 mode를 바깥에서 한 번 설정하고 reader thread를
   매 read마다 만든다. 제품은 같은 호출 thread에서 매 질문 mode를 준비·복구하고 관측기를 생성·정리한다.
   이 수명과 원래 stdin 사용, 한글 진단 출력·36×120 화면을 보존한 최소 도구를 만든다.
3. 입력이 read 전 도착하는 순서, 실제 reader I/O pending 뒤 도착하는 순서, 관측기 준비와 Peek의 순서를
   각각 사건으로 통제한다. 관측기 유무·진단 출력 유무를 정한 행렬로 한 번씩 대조한다.
   통과할 때까지 반복하지 않으며 실패 시 원래 상한과 증거를 보존한다.
4. 재현을 얻으면 해당 conhost의 read 요청 등록·입력 통지·cooked 완료·클라이언트 반환을 구분할 추적을
   설계한다. 사용 가능한 provider·symbol과 payload를 먼저 검토해 입력 원문 없는 단계·상태만 수집한다.
   기존 WCT 한 노드나 API 직전 표지로 서버 내부 완료를 대신하지 않는다.
5. 원인이 확인된 조건에서 원본이 실패하는 회귀를 만든 뒤 수리한다. 두 wizard, 실제 승인 발급,
   명시 CLI와 같은 plan, private 발급·무변경 계약을 Windows 관련 owner로 확인한다.

새 진단 구현·원격 CI push는 이번 작업에서 수행하지 않았다. 기존 원격 진단 branch도 건드리지 않았다.
근본 원인이 확인되기 전 raw 입력 교체·mode 변경·관측기 제거를 수리로 채택하지 않는다.

## 이번 검증 범위

변경은 이 조사 문서와 testing 색인뿐이다. 로그·artifact 사건 동일성, 실패 SHA의 코드,
관련 owner의 predicate 및 실제 스택을 대조했고 `aaa verify`를 통한 문서 범위 공개 검사가 통과했다.
미커밋 문서 변경 때문에 workspace 원장 수집은 되지 않았다. 검사 통과와 원장 수집을 구분한다.
제품 수리가 없으므로 Windows owner 재실행·실기 통과를 회복 근거로 만들지 않는다.
Windows 실기·시스템 설정·서비스·기존 설치에는 접근하거나 변경하지 않았다.
착지 gate·cross·Windows production·병합·push는 수행하지 않는다.

# Windows 설치 PTY의 Enter 관측 뒤 읽기 정체(t-1583)

이 문서는 첫 조사 당시의 결과다. 검수 뒤 구현한 진단과 실기 확인은
[실패 뒤 입력 큐 스냅샷 기록](testing-win-pty-input-snapshot-t1583.md)에 있다.

## 결론

새 자연 실패는 plain wizard의 역할 변경 질문에서 Enter가 콘솔 입력에 관측됐는데도
`ReadConsoleW`가 반환하지 않은 사례다. 살아 있는 CLI의 입력 읽기가 대기 중이며,
종료한 자식의 출력 EOF나 설치 worker 메시지를 기다린 사례로 분류할 수 없다.
**근본 원인은 아직 미확정이고 수리 완료·CI 회복으로 판정하지 않는다.**

기존 [t-1472](testing-win-pty-read-t1472.md)의 진단 뒤 생긴 이전 자연 실패도 추가로 읽었다.
그 실패에도 Enter 관측과 같은 읽기 대기가 있으므로, 이번 실행이 해당 진단 뒤 첫 자연 실패인 것은 아니다.
두 wizard 실패의 관측 경계는 같지만 원래 Ctrl+Z 실패까지 같은 근본 원인이라고 확정하지 않는다.

제품·시험 입력 코드와 시한은 변경하지 않았다. 이번 변경은 조사 기록뿐이다.

## 최신 자연 실패의 사건

[실패 Windows job](https://github.com/cLazyZombie/aaa/actions/runs/42316855981/job/128231464752)의 전체 로그와
`rust-tests-windows-2025-42316855981-1` artifact 전체 `test.log`를 직접 대조했다.
artifact에는 `test.log`만 있다. 아래 스냅샷·입력 쓰기·상한·대기 상태가 두 원문에 모두 있다.
원문은 작업 전용 target에 보존하고 공개 문서나 보고 library로 복사하지 않는다.

- SHA는 `289989e78cdf30b080d13ca50e9249e239eb5834`, attempt 1이다.
- prompt 진단 `af8f4b13`과 worker 진단 `27f9b5f3` 모두 실패 SHA의 조상이다.
- OS는 Windows Server 2025 `10.0.26100`, 이미지 계열은 `windows-2025-vs2026`, 판은 `20261103.298.1`이다.
- 대상은 `wizard::both_wizards_back_from_host_choices_then_submit_the_same_agent_plan_as_explicit_cli`다.
  첫 `plain=true` 순회가 실패했으므로 뒤의 TUI 순회는 이 실행에서 시작하지 않았다.
- 처음 다섯 선택 뒤 `:back` 네 번은 모두 반환했다. 열 번째 응답인 역할 `agent`와 CR까지 전송했다.
- 마지막 입력 쓰기는 시작 87,265µs, flush 완료 87,294µs다.
  driver는 전체 14개 중 10개를 보냈고 다음 `머신 표시 이름` 질문을 기다렸다.
- `read_id=10`의 준비·호출은 있지만 반환은 없다. read 1~9는 모두 성공 반환했다.
- 역할 질문의 호출 표지는 `00:24:59.503044Z`다. 입력 쓰기의 UTC 시각은 `00:24:59.503342Z`다.
  이후 `00:24:59.507959Z`에 `key_down=true`, `enter_down=true`,
  `cancel_down=false`, `eof_down=false`가 기록됐다. 준비부터 관측까지는 5,264µs다.
  관측 로그의 필드명이 ConPTY 줄 재표시로 나뉘어 있어 이어지는 줄도 확인했다.
- 상한은 60.0016587초이며 `answered=10/14`, `waiting=prompt`, `child_exit=None`, `reader_done=false`다.
- 상한 확정 뒤 helper는 566ms에 정상 종료했고 실제 스택 기록은 572ms에 끝났다.
  CLI 주 thread는 `NtDeviceIoControlFile → ReadConsoleW → prompt::windows::line → wizard::ask → wizard::run`이다.
  수집한 CLI thread는 하나다. 관측 thread의 join에서 막힌 스택이 아니다.
- conhost 누적 CPU는 kernel 312,500×100ns, user 0이다. 다섯 thread 모두 WCT가 종류 8·상태 3인
  한 노드, cycle false, OS 오류 0으로 기록됐다. 이는 thread의 blocked 상태이며 대기 오브젝트·원인은 나오지 않았다.
  한 번의 누적 CPU와 cycle false로 순간 부하나 교착의 유무를 확정하지 않는다.
- install 묶음은 36통과·1실패·기존 helper 1개 ignored, 89.68초다. 묶음 시간은 개별 시험 시간이 아니다.

## 이전 자연 실패와의 대조

추가로 [이전 Windows job](https://github.com/cLazyZombie/aaa/actions/runs/41580211249/job/125733027103)의
전체 로그를 읽었다. 최신 실행과 달리 이 과거 실행은 별도 artifact까지 대조하지 않았다.

| 구분 | 이전 wizard | 최신 wizard |
| --- | --- | --- |
| SHA | `29b17b88` | `289989e7` |
| 마지막 응답 | 프로젝트 질문의 두 번째 `:back`+CR | 역할 질문의 `agent`+CR |
| 답한 개수 / 전체 | 7/14 | 10/14 |
| 미반환 read | 7 | 10 |
| 다음 기다린 질문 | host에 로컬 agent | 머신 표시 이름 |
| 키 관측 | key_down·enter_down true | key_down·enter_down true |
| Ctrl+C·Ctrl+Z 관측 | 모두 false | 모두 false |
| CLI 실제 대기 | ReadConsoleW | ReadConsoleW |
| 상한 | 60.0009797초 | 60.0016587초 |
| 콘솔 호스트 WCT | 다섯 thread, 각 blocked 한 노드 | 다섯 thread, 각 blocked 한 노드 |

이전 실행의 마지막 Enter 관측은 `read_id=7`, 준비 뒤 8,603µs다.
입력 쓰기 완료는 94,736µs이고 실제 읽기 반환은 없다. 최신 실행과 마찬가지로 plain 첫 순회에서 멈췄다.
따라서 특정 머신 이름 검증이나 agent 역할의 후속 계획 계산만으로 두 실패를 설명할 수 없다.

원래 [Ctrl+Z 실패](testing-win-auth-t-1303.md)는 콘솔 입력 도착을 확인할 진단이 없었다.
또 [승인 뒤 worker 대기](testing-win-worker-stall-t1487.md)는 입력이 없는 apply 호출이며
CLI 스택과 공개 단계가 달랐다. 이들을 이번 두 wizard 실패와 같은 원인으로 합치지 않는다.

## 알게 된 것과 남은 증거

Enter 관측은 부모의 쓰기·flush보다 강한 근거다. 적어도 해당 read의 관측기가 콘솔 입력 큐에서
Enter KEY_DOWN을 보았으므로, 부모가 응답을 보내지 않았거나 Enter가 전혀 도착하지 않았다는 설명은 맞지 않는다.
그러나 Peek는 소비하지 않으며 관측 뒤 그 Enter가 읽혔는지, 큐에 남았는지는 기록하지 않는다.
이 구분은 [PeekConsoleInput의 계약](https://learn.microsoft.com/en-us/windows/console/peekconsoleinput)과 부합한다.

CLI의 실제 스택과 반환 표지 부재는 API 읽기 경계의 대기를 뒷받침한다. 반면 기존 WCT는
blocked 상태만 보여 주므로 콘솔 내부 입력 소비·깨우기·읽기 완료 중 어느 단계가 멈췄는지는 알 수 없다.
[Microsoft의 WCT 설명](https://learn.microsoft.com/en-us/archive/msdn-magazine/2007/july/bugslayer-wait-chain-traversal)도
여러 오브젝트 대기 등에서 교착을 모두 보고하지 못하는 한계를 설명한다.

따라서 입력 유실, 콘솔 내부 깨우기 결함, 진단 관측과 cooked read의 상호작용 중 어느 것도 확정하지 않는다.
root cause가 확인되지 않은 상태에서 raw 입력으로 교체하거나 mode·wakeup mask를 바꾸는 수리는 선택하지 않았다.

## Windows 실기 대조

검증은 `aaa verify`를 통해 실행했다. 전용 폴더와 TEMP·TMP를 사용하며 실제 설치·서비스·사용자 설정을 바꾸지 않았다.
고정 sleep·반복 재시도·시한 연장·기대값 완화는 하지 않았다.

실제 Win32 ConPTY를 만든 독립 probe에서 제품과 같은 line/echo 설정, processed·VT input 해제,
Ctrl+C/Z wakeup mask와 16,384 UTF-16 버퍼를 사용했다. 원본 wizard와 같은 14개의 공개 fixture 응답을
read마다 정확히 비교하고 자식 exit 0을 확인했다. 각 대조는 한 번 실행했다.

| 대조 | 결과 |
| --- | --- |
| 관측 thread 없이 읽기, 긴 한글 진단 출력 | 14개 정확히 반환, 281ms |
| Peek 관측 thread와 읽기, 짧은 출력 | 14개 정확히 반환, 266ms |
| Peek 관측 thread와 읽기, 긴 한글 진단 출력 | 14개 정확히 반환, 282ms |
| Peek의 KEY_DOWN 관측 사건 뒤에만 읽기 시작 | 14개 모두 관측 뒤 정확히 반환, 485ms |

일반 동시 대조에서는 읽기가 먼저 소비해 관측기가 키를 보지 못할 수 있다.
마지막 대조는 14번 모두 실제 관측 사건을 기다린다. 이 결과로 모든 경쟁 조건을 배제하지 않는다.
probe는 Python에서 Win32 API를 직접 호출한 도구로 실제 제품·CI 실행 파일의 대체가 아니다.
완료 표지 출력은 자식 종료보다 늦게 부모에 도착할 수 있어 성공 근거는 각 입력 비교와 exit 0이다.

probe 준비 과정의 실패도 보존했다. 첫 도구는 SSH에서 상속한 표준 입력 handle이 콘솔이 아니어서
OS 오류 6으로 끝났다. 전용 자식의 CONIN$/CONOUT$을 직접 열어 연결했다.
다음 도구는 prompt 끝 공백을 ConPTY가 `CSI C`로 표시해 부모의 문자열 조건이 기다렸다.
다음 질문을 식별하는 고정 표지만 비교하도록 고쳤다. 두 준비 실패는 제품 정체의 재현으로 세지 않는다.
입력 비교·기대 종료 코드·5초 상한은 유지했다.

같은 SHA의 변경 없는 checkout에서 원본 owner도 각각 한 번 실행했다.

| 원본 owner | 결과 |
| --- | --- |
| 두 wizard 뒤로 이동·명시 CLI와 동일 계획 | 1통과, 1.43초 |
| Ctrl+Z/C 취소·자동화·세션 내부 거부·issuer/approval 무변경 | 1통과, 1.95초 |

실기는 Windows `10.0.26200`으로 CI의 `10.0.26100`과 다르다. 개발용 verify 실행 파일을 썼고 웹 번들은 만들지 않았다.
CI와 같은 바이트의 실행 파일이나 같은 이미지 재현으로 쓰지 않는다. 원본 정체는 재현되지 않았으며,
성공한 두 owner를 원래 CI 회복 근거로 쓰지 않는다.

probe와 wizard 실행은 검증 원장을 남겼다. 취소 owner와 정리 확인은 실제 명령 exit 0이었으나,
작성 중인 로컬 조사 문서가 미커밋 상태여서 `aaa verify`가 원장을 수집하지 않았다.
시험 결과와 원장 수집 결과를 구분하며 통과한 시험을 원장 때문에 재실행하지 않았다.

전후 서비스·예약 작업·Run 키·사용자 PATH·방화벽 기준은 모두 같았다.
작업 전용 폴더를 사용하는 생존 프로세스와 reparse point는 각각 0개였다.
기준 목록을 회수한 뒤 reparse point와 프로세스를 다시 확인하고 전용 실험실 폴더를 삭제했다.

## 권하는 후속 진단 범위

**아래는 검토할 설계이며 아직 구현하지 않았다.** 다음 자연 실패에는 마지막 관측 뒤 Enter가
큐에 남아 있는지 구분하는 증거가 필요하다.

1. 기존 60초 실패 판정을 먼저 확정한 뒤, 이미 있는 5초 helper 안에서만 수집한다.
2. 소유 CLI의 부모·전체 이미지 경로·동일 process handle 검증을 유지하고 그 CLI의 콘솔에만 연결한다.
3. 입력 mode와 큐의 키·Enter·Ctrl+C/Z 존재 여부만 bounded Peek로 읽는다.
   입력 문자·길이·질문·승인서·명령줄·메모리 원문은 출력하지 않는다.
4. bounded Peek의 앞부분에 키가 없다는 사실을 전체 큐에 키가 없다는 증거로 쓰지 않는다.
   API 오류·콘솔 연결 거부·확인 불가는 단계와 OS 코드로 남긴다.
5. 입력을 소비·주입·flush하지 않고 mode도 바꾸지 않는다. 호스트 thread를 멈추거나
   살아 있는 conhost의 스택을 직접 걷는 방식을 추가하지 않는다.
6. 기존 스냅샷 owner에 실제 콘솔 입력 view와 비밀 비노출을 확인하고, 격리한 큐 fixture에서
   Peek 뒤 원래 입력이 그대로 남는 계약을 검증한다. Windows 실기 확인을 거친다.

Enter가 남아 있다면 큐가 있는 상태의 읽기 깨우기 경계를, 비어 있다면 큐 이후의 cooked read·완료 경계를
다음 조사 대상으로 삼을 수 있다. 빈 큐 하나로 정상 소비나 특정 내부 결함을 확정할 수는 없다.
이 보강만으로 근본 원인이 자동 확정된다고 약속하지 않는다. main 반영은 별도 착지 승인이 필요하다.

## 참고 구현

구현 전 조사 원칙에 따라 다음 후보를 비교했다. 외부 코드를 실행하거나 제품에 복사하지 않았다.

- [Microsoft Terminal 입력 버퍼](https://github.com/microsoft/terminal/blob/main/src/host/inputBuffer.cpp)와
  [cooked read](https://github.com/microsoft/terminal/blob/main/src/host/readDataCooked.cpp),
  [MIT](https://github.com/microsoft/terminal/blob/main/LICENSE): 큐·캐시·대기 통지·Enter 완료 경계를 대조했다.
  이 소스가 CI inbox conhost와 같은 바이트의 구현이라는 가정은 하지 않는다.
- [Crossterm Windows poll](https://github.com/crossterm-rs/crossterm/blob/master/src/event/sys/windows/poll.rs),
  [MIT](https://github.com/crossterm-rs/crossterm/blob/master/LICENSE): 입력과 별도 깨우기 사건의 대기를 비교했다.
- [Rustyline Windows 입력](https://github.com/kkawakam/rustyline/blob/master/src/tty/windows.rs),
  [MIT](https://github.com/kkawakam/rustyline/blob/master/LICENSE): 별도 출력·입력 사건 처리와 모드 복원을 비교했다.
  원인 증거가 없어 이 입력 구조로 제품을 교체하지 않았다.

새 런타임 의존성·새 제품 시험·서비스 등록은 없다. 조사 기록의 공개 검사만 수행하며
착지 창의 전체 gate·cross·production 빌드·전달·병합·push는 수행하지 않는다.

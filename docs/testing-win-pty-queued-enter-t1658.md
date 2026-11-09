# Windows 설치 PTY: 실패 뒤에도 남아 있는 Enter(t-1658)

아래는 첫 조사 기록이다. 승인 뒤 추가한 진단과 남은 실기·재현 범위는
[읽기 준비 상태·질의 회신 진단](testing-win-pty-readiness-t1658.md)에 기록한다.

## 결론

plain wizard의 열두 번째 `ReadConsoleW`가 반환하지 않았으며, 실패 판정 뒤에도 콘솔 입력 큐에
Enter KEY_DOWN이 있었다. 부모가 답을 보내지 않은 경우나 자식 종료 뒤 출력 EOF 대기와 구별된다.
**근본 원인은 미확정이다. 수리 완료나 CI 회복으로 판정하지 않는다.**

이전 [Enter 관측 조사](testing-win-pty-enter-t1583.md)와 달리 이번에는
[실패 뒤 입력 큐 진단](testing-win-pty-input-snapshot-t1583.md)이 실제 자연 실패에서 동작했다.
큐가 비어 있는지에 관한 불확실성은 줄었지만, 콘솔 내부의 읽기 대기 등록·깨우기 상태는 아직 없다.
할 일의 중단 조건에 따라 제품·시험 구현은 바꾸지 않고 후속 진단 계획을 남긴다.

## 확보한 원문과 대조

- 실패 기준은 main `c9119f6a`, attempt 1이다. 입력 큐 진단 `a816c942`가 포함된 판이다.
- [Windows job 전체 로그](https://github.com/cLazyZombie/aaa/actions/runs/42539284534/job/128910391285)와
  같은 run의 `rust-tests-windows-2025-42539284534-1` artifact를 받았다.
- artifact에는 `test.log` 하나만 있다. 별도 dump·PTY 원바이트 파일·console server 대기열 자료는 없다.
- 전체 job과 artifact에서 이 실패의 `input_writes` 12개와 스택·입력 큐 스냅샷 JSON이 일치했다.
  job 끝의 다른 시험·backtrace를 같은 read의 사건으로 합치지 않았다.
- OS는 Windows Server 2025 `10.0.26100`, 이미지 계열은 `windows-2025-vs2026`,
  판은 `20261103.298.1`이다. inbox `conhost.exe`의 실제 파일 판·해시는 이 자료에 없다.
- 원시 로그·압축 파일·조회 응답은 작업 폴더의 비공개 scratch에만 보존한다.
  보고 library에는 판단에 필요한 사건·불리언 상태만 발췌한다. 원문 전체를 복사하지 않는다.

## 마지막 성공 사건과 실제 대기

대상은 `wizard::both_wizards_back_from_host_choices_then_submit_the_same_agent_plan_as_explicit_cli`다.
시험은 `plain=true` 다음 TUI 순서로 실행한다. 이번 실패에는 `prompt::windows::line` 호출과
console read 사건이 있으므로 첫 plain 순회이며, 두 번째 TUI 순회는 시작하지 않았다.

| 입력 순서 | 답한 질문·행동 | 관측 결과 |
| --- | --- | --- |
| 1–5 | 설치 방식·host 역할·이름·로컬 agent·프로젝트 선택 | read 1–5 성공 반환 |
| 6–9 | PATH → 프로젝트 → 로컬 agent → 이름에서 뒤로 이동 | read 6–9 성공 반환 |
| 10 | 역할을 agent로 변경 | read 10 성공 반환 |
| 11 | 최종 머신 이름 입력 | read 11 성공 반환 |
| 12 | 여유 용량에 `0`과 CR 입력 | 부모 flush 완료, read 12 미반환 |
| 13–14 | 로그인 검사·PATH 선택 | 질문 도달 전이며 부모 입력 없음 |

제목·설치 안내 출력이 아닌 실제 준비·API 호출·반환 사건과 스택으로 나눴다.
주요 시각은 아래와 같다. UTC 시각과 부모의 경과 시간을 혼용하지 않는다.

| 사건 | 관측 |
| --- | --- |
| 마지막 성공 read 11 반환 | `00:28:38.499121Z`, success=true |
| read 12 입력 준비 | `00:28:38.500254Z`, original_mode=503, input_mode=502 |
| read 12 API 호출 표지 | `00:28:38.500558Z` |
| 부모의 마지막 입력 쓰기 | `00:28:38.500805Z`, 경과 98,816µs부터 98,835µs에 flush 완료 |
| read 12 관측기 | `00:28:38.505408Z`, key_down=true, enter_down=true, cancel/eof=false |
| 실패 판정 | 경과 60.0057982초, answered=12/14, waiting=prompt |
| 다음 기다리던 조건 | `harness 로그인 검사` 질문 문자열 |
| 자식·출력 상태 | child_exit=None, reader_done=false |

`install_console_read_call`은 API 직전의 표지다. 콘솔 서버가 대기 등록을 끝냈다는 표지는 아니다.
부모의 flush도 콘솔이 그 줄을 소비했다는 뜻이 아니다.
반면 read 1–11에는 실제 성공 반환 표지가 있고 read 12에는 없다.
따라서 마지막 `0`이 계획이나 승인에 반영됐다고 보지 않는다.
이 실행은 dry-run 질문 단계이므로 승인서 발급·설치 worker·파일 적용까지 가지 않았다.

## 실패 뒤 스냅샷

기존 helper가 실패 확정 뒤 정상 종료했다. 입력 조회는 helper의 `console_input` 단계 5ms부터
다음 `initialize_symbols` 단계 8ms 사이에 이루어져 CLI 스택 조회보다 앞선다.
helper 전체 수집은 544ms, 실제 스택 기록은 552ms다.

콘솔 입력 view는 아래 상태였다. 입력 문자·길이·키 원문은 수집 결과에 없다.

```text
input_mode=502
records_present=true
prefix_key_down=true
prefix_enter_down=true
prefix_cancel_down=false
prefix_eof_down=false
```

이는 앞 128개 사건 안에 Enter KEY_DOWN이 있었다는 긍정 관측이다.
Ctrl+C/Z의 false를 전체 큐에 해당 키가 없다는 증거로 확대하지 않는다.
이 view는 CLI의 stdin handle 자체가 아닌, 검증한 동일 콘솔의 별도 `CONIN$` handle에서 얻었다.
호출 중인 read와 큐의 Enter 사이의 서버 내부 연결 관계까지 보여 주지는 않는다.

CLI에서 수집된 thread는 하나이며 실제 스택은 다음 경계다.

```text
NtDeviceIoControlFile → ReadConsoleW
→ install::prompt::windows::line → install::prompt::line
→ install::wizard::ask → install::wizard::run
```

관측기 thread의 join이나 설치 자식 종료 대기가 아니다.
콘솔 호스트의 다섯 thread는 모두 WCT 종류 8·상태 3의 한 노드이며 cycle=false, os_code=0이다.
누적 CPU는 kernel 0, user 156,250×100ns다. 이 한 시점의 누적치로 부하나 교착을 확정하지 않는다.
WCT에는 대기 대상·입력 read 등록·깨우기 이력이 없고 console server 스택도 없다.

## 확인한 것과 확정하지 못한 것

부모는 해당 응답과 CR을 썼고, 관측기는 Enter를 보았으며, 실패 뒤에도 큐에 Enter가 있었다.
따라서 「부모가 응답하지 않았다」 또는 「정상 종료한 자식의 출력 pipe가 안 닫혔다」는 설명은 맞지 않는다.
back 전환과 역할·이름 입력도 실제 반환했으므로 그보다 앞 단계에서 멈췄다고 보지 않는다.

아직 구별하지 못한 후보는 콘솔 서버의 읽기 등록·깨우기, cooked-read 처리 상태,
동시 Peek 관측과 읽기의 상호작용이다. 서버 내부 사건이나 같은 조건의 결정적 재현이 없으므로
어느 하나를 원인으로 정하지 않는다. 큐가 남아 있다는 사실만으로 OS 결함이나 관측기 결함을 확정하지 않는다.
이전 실패와 정체 경계는 같지만 같은 근본 원인이라고 단정하지 않는다.

## 참고 구현·계약 대조

외부 소스는 읽기만 했으며 실행하거나 제품에 가져오지 않았다.

- [Microsoft Terminal inputBuffer](https://github.com/microsoft/terminal/blob/main/src/host/inputBuffer.cpp),
  [WaitQueue](https://github.com/microsoft/terminal/blob/main/src/server/WaitQueue.cpp),
  [cooked read](https://github.com/microsoft/terminal/blob/main/src/host/readDataCooked.cpp),
  [MIT](https://github.com/microsoft/terminal/blob/main/LICENSE): 입력 저장·대기자 통지·read 완료를 분리한다.
  이 공개 main 소스를 CI의 inbox conhost와 같은 구현으로 간주하지 않았다.
- [Crossterm Windows poll](https://github.com/crossterm-rs/crossterm/blob/master/src/event/sys/windows/poll.rs),
  [MIT](https://github.com/crossterm-rs/crossterm/blob/master/LICENSE): 입력 사건과 별도 깨우기 사건을 구분한다.
- [Rustyline Windows reader](https://github.com/kkawakam/rustyline/blob/master/src/tty/windows.rs),
  [MIT](https://github.com/kkawakam/rustyline/blob/master/LICENSE): 사건 대기 뒤 `ReadConsoleInputW`로 소비한다.
  이 raw 입력 구조는 현재 cooked-read 계약과 달라 원인 증거 없이 교체하지 않았다.

[ReadConsole 계약](https://learn.microsoft.com/en-us/windows/console/readconsole)은 cooked mode의 Enter 완료를,
[입력 버퍼 계약](https://learn.microsoft.com/en-us/windows/console/low-level-console-input-functions)은
큐의 비소비 Peek와 입력 handle의 signaled 상태를 설명한다. 현재 자료에는 signaled 상태 자체가 없다.

알려진 [Microsoft Terminal #19994](https://github.com/microsoft/terminal/pull/19994)는
폭 1칸에서 2칸 문자를 입력하는 재현이며 이번 PTY는 폭 120칸, 마지막 입력은 ASCII `0`이다.
[#17905](https://github.com/microsoft/terminal/pull/17905)는 제어 문자 표시와 줄 배치의 무한 루프를 고친다.
두 변경을 이번 원인의 증거로 쓰지 않는다. 특정 Windows 패치·입력 모드 변경도 수리로 제안하지 않는다.

## 다음 진단 계획

아래는 미구현 계획이다. 원인 미확정 시 멈추라는 할 일의 조건에 따라 이번에 추가하지 않았다.

1. 실패 확정 뒤 기존 helper의 같은 5초 상한 안에서 입력 handle의 zero-time wait 결과,
   `GetConsoleSelectionInfo`의 정적 flags, 출력 화면 크기, conhost 파일 판을 함께 수집한다.
   새 `CONIN$` view와 원래 stdin의 상태를 구분하고, 확인할 수 없는 값은 단계·OS 오류로 남긴다.
   입력을 소비·주입·flush하거나 모드·살아 있는 호스트 thread를 바꾸지 않는다.
2. PTY driver에서 질의 종류·응답 여부만 기록해 부모 응답 외의 입력인 터미널 질의 회신을 분리한다.
   현재 `input_writes`는 질문 응답만 기록하므로 터미널 질의 회신의 완전한 시간선이 아니다.
   질문·키·응답 bytes 원문은 기록하지 않는다.
3. 같은 Windows Server 2025 이미지·conhost 판의 격리 환경에서 read 호출 전 입력 도착,
   대기 후 입력 도착, Peek 동시 진입을 사건으로 통제하는 최소 재현을 만든다.
   관측기 유무와 출력 동시성을 명시한 대조를 하며 기존 시험을 통과할 때까지 반복하지 않는다.
   문구 출력만으로 서버 대기 등록을 보장했다고 쓰지 않는다.
4. 입력 handle은 signaled인데 read가 계속 대기하는 재현을 얻으면 console server의 대기 등록·통지를
   최소 사건 추적으로 대조한다. 공개 main과 inbox 구현의 동일성을 먼저 확인한다.
   메모리 dump나 입력 원문을 보고·artifact에 공개하지 않는다.
5. 원인이 확정된 뒤 해당 조건에서 먼저 실패하는 회귀와 수리를 만들고 설치 prompt·wizard·승인 owner를
   실제 Windows에서 검증한다. 60초 상한·기대값·시험을 그대로 유지한다.

## 이번 검증 범위

개발 변경은 이 조사 문서와 testing 색인뿐이다. 제품·시험·CI 코드는 변경하지 않았다.
전체 로그와 artifact의 사건·스냅샷 대조 및 문서 공개 검사를 수행한다.
관련 실행 owner는 설치 통합 시험의 wizard·authorization이며, 원인 수리가 없으므로 Windows 재실행을
CI 회복 확인으로 대신하지 않았다. 실제 Windows에는 도구 경로의 읽기 전용 확인만 했고
시험 폴더·서비스·설정·프로세스는 만들지 않았다.
착지 gate·cross·Windows production·병합·push는 수행하지 않는다.

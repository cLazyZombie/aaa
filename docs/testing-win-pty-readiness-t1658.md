# Windows 설치 PTY의 읽기 준비 상태·질의 회신 진단(t-1658)

아래는 실기 단절 때의 첫 구현 기록이다. 이후 실행·버전 조회 수리와 Server CI 결과는
[실기·Server 2025 검증](testing-win-pty-native-ci-t1658.md)에 기록한다.

## 현재 결론

사용자 승인 뒤 [첫 조사](testing-win-pty-queued-enter-t1658.md)의 진단 계획 1·2와 사건 제어 도구를 구현했다.
제품의 prompt·wizard·승인 처리와 기존 60초 판정은 바꾸지 않았다.
**근본 원인은 미확정이며 Windows 실행 검증·동일 Server 2025 이미지 재현이 남아 있다.**

개발 도중 승인된 Windows 실기 머신의 SSH가 시간 초과했고 네트워크와 host 머신 목록에서도
오프라인·disconnected로 확인됐다. 초기 읽기 전용 조회 때 이 실기는 Windows 11 `10.0.26200`,
시스템 conhost 파일 판은 `10.0.26100.8875`였다. 실패한 Server 2025 환경의 대체로 세지 않는다.
실패 원문에는 conhost 파일 판이 없으며, 같은 이미지에 접근할 경로도 현재 확보하지 못했다.
새 코드의 실행 결과를 추측하거나 실기를 교차 컴파일 결과로 대신하지 않는다.

## 실패 뒤 읽기 전용 수집

대상은 `tests/install/snapshot_console.rs`와 `snapshot_native.rs`, 결과 형식은 `terminal_snapshot.rs`다.
기존과 같이 부모·전체 실행 파일 경로·소유 process handle을 검증한 CLI와 시스템 conhost만 조회한다.
helper가 CLI와 같은 콘솔에 연결됐고 client가 두 개뿐인지 확인한 뒤 수집한다.
기존 60초 실패를 확정한 **뒤** 실행하며 helper의 5초 상한도 유지한다.

| 새 필드 | 수집 방법·의미 |
| --- | --- |
| `console_input.conin_wait` | helper가 새로 연 읽기 전용 `CONIN$`에 zero-time `WaitForSingleObject`; 반환 코드와 실패 시 OS 코드 |
| `console_input.selection_flags` | `GetConsoleSelectionInfo`의 flags만; 선택 영역 좌표·내용은 제외 |
| `console_input.screen_size` | 읽기 전용 `CONOUT$`의 화면 버퍼·window 가로/세로 크기만 |
| `console_host.file_version` | 검증한 시스템 conhost 경로의 고정 버전 resource에서 숫자 네 개만 |

입력 mode·bounded Peek를 먼저 조회하고 부가 API를 차례로 부른다. 각 API 진입 전 journal에
`console_input_mode`, `console_input_peek`, `console_input_wait`, `console_selection_info`,
`console_screen_size`를 남겨 API가 반환하지 않은 경우도 단계를 구별한다.
선택·화면·버전 조회가 오류를 반환하면 정적인 단계 이름과 OS 코드로 각각 보존한다.
그 부가 오류 때문에 이미 얻은 입력 큐 관측이나 스택을 버리지 않는다.

`conin_wait`는 CLI의 원래 stdin handle 자체를 조회한 값이 아니다. 결과 이름에도 `conin`을 붙였다.
큐·wait·선택·화면은 순차 표본이며 원자적인 한 스냅샷으로 해석하지 않는다.
conhost 판은 조회 시점의 디스크 파일 판이며 실행 중 모듈의 바이트 동일성까지 보증하지 않는다.
버전 문자열·경로·MUI resource·입력 문자·길이·화면 글자는 결과에 넣지 않는다.

수집 경로는 입력을 소비·주입·flush하거나 mode를 바꾸지 않는다. 살아 있는 conhost thread도 멈추지 않는다.
기존 fixture의 키 작성·소비는 새로 만든 격리 콘솔에서 진단의 비소비 계약을 검사하는 시험 동작이며
실제 실패 수집 경로와 구별한다.

## PTY의 터미널 질의 회신

공유 시험 helper `tests/support/terminal_queries.rs`에 관측 callback을 더하고 기존 `feed` 동작은 유지했다.
실제 설치 PTY driver가 지원하는 커서 위치·DA1·DA2·기본 글자/바탕 색 질의를 아래 단계로 남긴다.

- `received`: parser가 완전한 지원 질의를 찾음.
- `flushed`: 실제 writer의 `write_all`과 `flush`가 성공함.
- `unavailable`: 현재 helper가 그 색 질의의 답을 만들 수 없음.
- `write_failed`·`flush_failed`: 해당 I/O 실패와 숫자 OS 코드.
- `after_exit`: 자식 종료 뒤 sink로 흘린 답. 실제 회신으로 세지 않음.

기존 입력 쓰기와 같은 부모의 경과 시간·답한 단계 번호를 사용한다.
종류·단계·OS 코드만 최대 256건 보존하고 초과 건수는 따로 남긴다.
상한 실패 또는 질의 I/O 오류 때 출력하며 요청·응답 bytes와 입력 원문을 기록하지 않는다.
지원하지 않는 임의의 escape sequence를 모두 질의로 판정하는 도구는 아니다.
flush 성공을 자식의 응답 소비나 cooked read 완료로 해석하지 않는다.

## 사건 제어 재현 도구

`tests/install/console_repro.rs`는 제품 설치를 수행하지 않는 독립 ConPTY fixture다.
실제 `ReadConsoleW`, 제품과 같은 cooked/echo·processed/VT 해제·Ctrl+C/Z wakeup mask·버퍼 크기를 쓴다.
각 조건에서 14개의 고정 fixture 줄을 정확히 비교하며 관측 결과만 남긴다.

| 조건 | 입력 전송 전에 확인하는 사건 |
| --- | --- |
| `prequeued` | reader를 gate에 둔 채 입력을 보내고 Peek로 Enter 도착을 확인한 뒤 reader를 놓음 |
| `read_pending` | 소유 reader thread의 `GetThreadIOPendingFlag`가 true인 것을 확인한 뒤 부모에게 입력 요청 |
| `peek_armed` | 별도 Peek 관측기를 준비하고 reader I/O pending을 확인한 뒤 입력 요청 |

각 조건을 긴 한글·SGR·줄 바꿈 출력의 유무와 조합해 총 6개를 한 번씩 실행한다.
사건·I/O 상태로 끝나며 고정 sleep·재시도는 없다. fixture 자체의 5초 안전 상한은 성공 조건이 아니다.
기존 PTY의 60초 상한과 실패 판정은 그대로다. 도구 자식의 실패 스택은 CLI 경로로 오인하지 않고
그 자식 test executable의 실제 경로를 검증하도록 driver의 진단 이미지 인자를 분리했다.

I/O pending은 실제 kernel 요청을 관측한 것이지만 console server의 대기 등록 완료 표지는 아니다.
`peek_armed`도 준비 사건을 통제하는 것이며 ReadConsole/Peek의 서버 내부 실행 순서를 강제하지 않는다.
ReadConsole이 먼저 소비하면 관측기가 Enter를 못 볼 수 있으므로 그 사실만으로 실패시키지 않는다.
실제 정체를 재현하기 전에는 이 도구를 결정적 회귀 시험이나 원인 확정 증거로 부르지 않는다.

새 도구는 수동 진단 입구 하나를 `ignored`로 두었으며 기존 시험을 삭제·무시 처리하지 않았다.
일반 owner 실행에 조사용 조건 여섯 개를 추가하지 않는다. 같은 OS·conhost 판인지 먼저 확인하고
아래 입구를 **한 번** 실행한다. 출력은 원시 scratch 로그에만 보존한다.

```powershell
# 실제 확인한 값으로 지정한다. Server 2025의 product_type은 3, Windows 11은 1이다.
$env:AAA_TEST_CONSOLE_EXPECTED_BUILD = '<확인한 build>'
$env:AAA_TEST_CONSOLE_EXPECTED_PRODUCT_TYPE = '<확인한 product_type>'
$env:AAA_TEST_CONSOLE_EXPECTED_VERSION = '<확인한 conhost 숫자 네 자리>'
aaa verify -- cargo test --profile verify --locked -p aaa --test install `
  authorization::terminal_snapshot::native::repro::cooked_read_event_orders `
  -- --exact --ignored --nocapture
```

도구는 OS build·product type·conhost 판을 출력하고 지정한 기대값과 다르면 실행 전에 실패한다.
Server 2025라는 제품명이나 build 하나만으로 실패 이미지와 같은 환경이라고 판단하지 않는다.

## 필요한 검증과 현재 결과

새 질의 owner는 chunk 경계의 질의 인식, 실제 회신 bytes 보존, 회신·flush 실패 경계와
원문 비노출을 확인한다. 기존 입력 큐 owner는 새 수집 뒤 mode·키·순서가 남는지와
빈 큐/입력 있는 큐의 zero-time wait, 화면 크기·선택 상태·잘못된 화면 handle의 OS 오류를 확인하도록 확장했다.
기존 실제 스택 owner는 새 화면·선택·버전 view가 정상 수집되는지도 확인한다.

macOS의 설치 통합 owner와 관련 strict Clippy를 `aaa verify`로 확인한다.
Windows cross strict Clippy도 같은 진입점으로 확인한다. 세부 결과는 보고 상세에 기록한다.
Windows native owner와 수동 재현은 아직 실행하지 못했다. 특히 공유 helper의 Windows 암호 입력 owner는
native 검사 범위에 포함해야 한다. 교차 Clippy 성공을 Windows API 실행 성공으로 쓰지 않는다.

실기 연결이 복구되면 전용 작업 폴더 생성 여부를 먼저 확인하고 준비·정리 기록을 회수한다.
원격 준비 명령에는 정상 완료 표지가 없어서 폴더 생성·정리 여부도 현재 확인하지 못했다.
소스 bundle은 로컬에만 준비했으며 Windows 빌드·시험 프로세스는 시작하지 않았다.
시험 전후 서비스·예약 작업·Run 키·사용자 PATH·방화벽 목록을 대조하고,
직접 확인한 자기 PID·reparse point를 검사한 뒤 전용 폴더만 정리한다.

## 아직 못 한 원인 조사

승인 계획 3의 같은 Server 2025 이미지·conhost 판 재현은 환경 확보가 필요하다.
계획 4의 console server 읽기 등록·통지 추적은 실제 정체 재현이 선행 조건이다.
재현 결과가 없으므로 그 추적과 계획 5의 원인 회귀·수리를 시작하지 않았다.
raw 입력 교체·관측기 제거·timeout 연장 같은 변경으로 통과시키지 않는다.
진단만 착지하더라도 Windows 실행 검증과 별도 착지 승인이 필요하다. 병합·push는 하지 않았다.

## 참고한 계약·구현

외부 코드는 실행하거나 복사하지 않았다. 첫 조사에서 비교한 Microsoft Terminal·Crossterm·Rustyline의
MIT 구현과 아래 공식 API 계약을 대조했다.

- [WaitForSingleObject](https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-waitforsingleobject):
  timeout 0은 대기하지 않고 현재 상태를 반환한다.
- [콘솔 handle](https://learn.microsoft.com/en-us/windows/console/console-handles):
  `CONIN$`·`CONOUT$`과 프로세스의 표준 handle은 구별한다.
- [GetConsoleSelectionInfo](https://learn.microsoft.com/en-us/windows/console/getconsoleselectioninfo):
  선택 flags만 취하며 실패는 OS 코드로 남긴다.
- [GetFileVersionInfoW](https://learn.microsoft.com/en-us/windows/win32/api/winver/nf-winver-getfileversioninfow):
  고정 버전 resource만 사용한다.
- [GetThreadIOPendingFlag](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getthreadiopendingflag):
  thread 소유권·조회 권한을 확인하고 실제 pending 상태로 입력 순서를 제어한다.

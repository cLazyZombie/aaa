# Windows 설치 PTY 자연 실패의 원래 stdin·reader 상태(t-1699)

## 범위

q-3296 1A에 따라 [제품 수명 진단](testing-win-pty-product-lifetimes-t1699.md)의 후속 수집을 구현·검증했다.
원래 정체의 원인 수리가 아니라 다음 자연 실패의 관측 경계를 넓히는 변경이다.
기존 PTY의 60초 실패 판정·질문·입력·승인 조건을 유지한다. 수집 때문에 성공 대기를 다시 시작하지 않는다.

## 소유 CLI 안의 읽기 상태

Windows prompt는 기존 read ID에 다음 등록을 붙인다.

- ReadConsoleW에 넘길 원래 stdin handle 값은 CLI 안에서만 유지한다. CONIN$를 새로 열거나 handle을 복제하지 않는다.
- reader의 현재 thread ID로 query 권한의 실제 thread handle을 보유해 ID 재사용과 구분한다.
- 준비·API 호출 직전·반환 직후의 phase를 기록한다. API 직전 표지를 서버 등록 완료로 부르지 않는다.
- 시험 전용 trace와 private IPC 환경이 모두 있을 때만 별도 수집 thread를 한 번 만든다.
  정상 입력 중에는 IPC 요청을 기다리며 stdin이나 reader의 I/O 상태를 조회하지 않는다.

시험 부모는 사용자 전용 Named Pipe를 준비하고, CLI와 부모 양쪽이 OS peer PID를 대조한다.
부모는 실제 띄운 PTY 자식 PID의 응답만 받는다. trace·IPC 설정이 없으면 이 경로를 만들지 않는다.
이름·경로·입력 원문은 응답 형식이나 오류 출력에 넣지 않는다.

부모가 기존 60초 실패를 먼저 확정한 뒤 한 번 요청하면 CLI 안에서 아래 값을 수집한다.

| 필드 | 의미 |
| --- | --- |
| cause·decided_unix_us | 부모의 실패 확정 또는 통제 fixture라는 출처와 그 표본 시각 |
| process_id·read_id·read_thread | 실제 응답한 CLI·해당 read·보유한 reader thread |
| phase_before·phase_after | 조회 전후 read의 진행 상태. 반환 경주를 구분함 |
| sampled_unix_us·query_elapsed_us | CLI 조회 시각과 두 API 조회 경과 |
| original_stdin_wait | 원래 stdin에 대한 zero-time WaitForSingleObject 결과 또는 OS 오류 |
| reader_io_pending | GetThreadIOPendingFlag 결과 또는 OS 오류 |

두 API는 순차 조회이며 원자적 표본이 아니다. thread의 모든 I/O에 관한 pending이지 서버 내부 read 등록 증거가 아니다.
상태 등록이 끝났으면 no_active_read, 잘못된 요청은 invalid_request, 연결·권한·수집 불가는 정적 단계와 OS 코드로 남긴다.
입력을 소비·주입·flush하거나 모드를 바꾸는 API는 새 수집기에 없다.

원래 helper의 별도 CONIN$ 표본은 그대로 남겨 새 original_stdin 응답과 구별한다.
실패 뒤 원래 5초 수집 상한 안에서 CLI 표본과 기존 helper를 병렬로 수집한다.
살아 있는 conhost thread는 중지하지 않는다. 기존 helper의 소유 CLI 스택 수집과 서버 thread 수집은 구분한다.
PowerShell 같은 입구의 후손이 응답하더라도 PTY 자식 PID와 다르면 거부한다. 그 표본을 직계 CLI로 오인하지 않는다.

## 공개 symbol·provider 조사

[Microsoft 공개 symbol server](https://learn.microsoft.com/en-us/windows-hardware/drivers/debugger/microsoft-public-symbols)에서
이전 실제 conhost의 CodeView GUID·age 키로 PDB를 받아 읽기 전용 LLVM 도구로 살폈다.
GUID는 기존 기록과 같았으나 받은 PDB summary의 age는 3, 기존 PE의 CodeView 기록은 1이었다.
이를 일치하는 PDB로 로드하거나 기대 age를 바꿔 통과시키지 않았다. 실제 inbox와 symbol 대응은 아직 확정하지 않는다.
읽은 public 이름에는 cooked read·NotifyWaiters 등이 있지만, 함수 이름만으로 실행 시각이나 대기 등록·통지 이력을 얻을 수는 없다.
PDB와 원시 public 목록은 작업 scratch에만 두고 공개 문서·보고 library에는 복사하지 않는다.

[공개 Terminal tracing 구현](https://github.com/microsoft/terminal/blob/main/src/host/tracing.cpp)의 provider 이름은
`Microsoft.Windows.Console.Host`다. 앞선 `Microsoft-Windows-Console` manifest 조회 실패와 같은 의미가 아니다.
입력 키 사건에는 문자가, cooked read 사건에는 읽은 본문이 포함된다. 이 provider를 통째로 켜거나 ETL을 저장하지 않는다.
[WaitBlock](https://github.com/microsoft/terminal/blob/main/src/server/WaitBlock.cpp)·
[WaitQueue](https://github.com/microsoft/terminal/blob/main/src/server/WaitQueue.cpp)를 대조했지만
현재 읽은 공개 구현에서 payload 없는 등록·통지·완료의 완전한 사건 묶음을 확인하지 못했다.
공개 main과 해당 inbox conhost가 같은 구현이라는 증거도 없다.

따라서 이번 변경은 서버 내부 세 단계를 구분한다고 약속하지 않는다.
ETW session 시작·메모리 dump·debugger attach·conhost thread 정지는 추가하지 않는다.

## 회귀와 검증 결과

실제 CLI의 입력 없는 read를 I/O pending 사건으로 확인한 뒤 원래 stdin wait=unsignaled·reader pending=true를 받는다.
그 뒤 실제 yes 승인이 성공하고 plan hash가 출력에 남는 것을 확인한다.
독립 ConPTY fixture에서는 입력을 먼저 큐에 두고 원래 stdin wait=signaled·준비 phase를 수집한 뒤
같은 입력 전체가 ReadConsoleW에서 반환되고 모드가 유지되는지 확인한다.
별도 pending fixture도 실제 I/O pending 사건 뒤 수집하고 같은 응답을 받는다.
통제 fixture의 응답은 자연 실패로 표시하지 않는다. 수집 성공을 위해 정한 시간을 자지 않는다.

정규 전체 ci-local과 실제 Windows 설치 owner·native strict Clippy, Windows 교차 strict Clippy를 통과했다.
검증은 `aaa verify`를 거쳤다. 정규 전체 실행 중에는 커밋·추적 파일 변경을 하지 않았다.

| 범위 | 결과 |
| --- | --- |
| 정규 전체 ci-local | web·fmt·build·test·e2e·public 모두 통과, 전체 291초 |
| Rust 실행 owner | 343개 작업 모두 exit 0 |
| Python | 88개, 기존 2개 무시, 통과 |
| 웹 E2E | 선택 범위를 줄이지 않은 전체 실행 통과 |
| 관련 macOS native strict Clippy | CLI·설치 통합 owner 통과 |
| 관련 Windows 교차 strict Clippy | CLI·설치 통합 owner 통과 |
| 실제 Server 2025 native strict Clippy | aaa all-targets 통과 |
| 실제 Windows 설치 owner 전체 | 43통과·기존 수동/격리 입구 3무시, 83.10초 |

실제 Windows는 [진단 CI](https://github.com/cLazyZombie/aaa/actions/runs/42751265917), attempt 1이다.
OS 10.0.26100·product type 3, image 20261103.298.1, conhost 10.0.26100.33648을 확인했다.
서버 provider 목록 조회는 성공했으나 그 시점에 `Microsoft.Windows.Console.Host` 이름은 없었다.
이 결과를 다른 시점·다른 provider의 부재로 확대하지 않는다. ETW 수집은 켜지 않았다.

job 전체와 artifact에서 아래 세 표본이 일치했다. 모두 controlled_fixture 출처다.

| 실제 경계 | read ID | phase 전후 | 원래 stdin wait | reader I/O pending |
| --- | --- | --- | --- | --- |
| 선입력 fixture | 41 | prepared → prepared | 0(signaled) | false |
| 읽기 대기 fixture | 41 | call → call | 258(unsignaled) | true |
| 실제 승인 CLI | 1 | call → call | 258(unsignaled) | true |

선입력 전체 반환·모드 보존과 실제 승인 발급·plan hash 대조까지 통과했다.
표본 JSON에는 입력·handle 값·경로가 없고 별도 CONIN$ 필드와 혼동하지 않는다.
이는 다음 자연 실패에서 원래 handle을 수집할 수 있는 경로 검증이며 정체 원인 확정·CI 회복 증거는 아니다.
실제 60초 실패를 만들려고 시간을 자거나 시한을 줄여 강제로 발생시키지는 않았다.

개발 중 shared wire 모듈을 두 번 포함한 Clippy 오류는 같은 모듈을 참조하도록 고쳐 해결했다.
시험을 재실행해 실패를 통과로 바꾸거나 기대값·시한을 바꾼 일은 없다.
정규 전체와 실제 Windows 실행은 각각 한 번 수행했다.

새 원격 진단 branch만 만들었고 main·기존 진단 branch·tag·force push·착지는 하지 않았다.
진단 workflow는 개발 후보와 분리했다. main 착지는 별도 승인 대상이다.

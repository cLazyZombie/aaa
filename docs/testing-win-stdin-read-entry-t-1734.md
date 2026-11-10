# Windows 원래 stdin 관측의 읽기 진입 경계(t-1734)

## 실패와 원인

[실패한 Windows CI](https://github.com/cLazyZombie/aaa/actions/runs/42832232428/job/129884993866)의
전체 job 로그와 `rust-tests-windows-2025-42832232428-1` artifact의 `test.log`를 대조했다.
실패 SHA는 `1b3ec5c96ff6446a7c61aff5b21fc8aa66f32bc8`이다.
`authorization::read_state_tests::original_stdin_probe_observes_the_owned_cli_read_and_preserves_real_approval`의
실제 단계는 `(Prepared, Prepared)`, 기대 단계는 `(Call, Call)`이었다.
설치 owner 결과는 42통과·1실패·기존 3무시다.

시험은 `[console-read-call]` 출력을 받고 소유 CLI의 실제 reader thread handle을 연 뒤,
`GetThreadIOPendingFlag`가 true가 되면 원래 stdin 표본을 한 번 요청했다.
그러나 [이 API](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getthreadiopendingflag)는
그 thread의 모든 미완료 I/O를 뜻하며 읽기 API를 구별하지 않는다.
제품의 순서는 `Prepared 등록 → 질문 출력 → call 로그 출력 → Call 기록 → ReadConsoleW`다.
부모가 로그의 marker를 받았어도 reader의 같은 로그 출력이 반환했다고 보장할 수 없다.
실제 Prepared 표본은 이 준비 단계의 I/O를 읽기 진입으로 오인한 시험 경합을 입증한다.

부모의 `observe` callback이 성공해야 승인 문자열을 쓰도록 되어 있다.
이번 실행은 callback 안에서 실패했으므로 그 시험의 `yes` 전송·읽기 반환·실제 승인 보존까지 도달하지 못했다.
원래 시험은 JSON 출력보다 먼저 단언했기 때문에 실패 당시의 원래 stdin wait 값과
collector가 조회한 reader pending 값은 두 원본 로그 모두에 없다. 이를 추정해 채우지 않는다.
표본 요청 전 부모가 pending=true를 관측했다는 사실과 collector의 조회 결과도 구별한다.

이 서명은 60초 read 정체와 다르다. 같은 근본 원인이라는 증거는 없으며,
해당 SHA의 다른 기능 변경을 원인으로 귀속하지 않는다.

## 최소 수리와 유지한 계약

통제 시험 부모가 임의 이름의 manual-reset Win32 event를 만들고,
기존 trace·private IPC가 설정된 `ReadGuard`만 그 event를 modify 권한으로 연다.
`ReadGuard::call`은 기존 Call 기록 뒤 event를 신호한다.
실제 CLI는 마지막 로그 출력이 반환한 뒤 이 메서드를 호출하며,
성공한 event 신호부터 `ReadConsoleW`까지 reader thread의 다른 I/O는 없다.

시험은 같은 5초 상한 안에서 이 사건을 받은 뒤 기존 pending 사건을 기다린다.
그 뒤에만 표본을 한 번 요청하고, callback이 끝난 뒤 원래 `yes`를 보낸다.
이 event 자체를 서버 내부 read 등록 완료라고 부르지 않는다.
원래 수집기는 요청이 왔을 때만 stdin·reader를 조회한다. 자연 실패의 60초 판정이나 수집 wire는 바꾸지 않는다.

다음 계약을 그대로 확인한다.

- 실제 CLI의 Call 전후 단계, 원래 stdin wait=258, reader pending=true, read ID=1과 소유 thread 일치.
- 수집 뒤 실제 승인 파일 발급과 출력의 plan hash 일치.
- 선입력 fixture의 Prepared 단계에서는 event가 아직 신호되지 않음.
- 선입력 전체 반환·콘솔 모드 보존, pending fixture의 Call 단계와 입력 반환.
- 두 fixture 모두 Call 이후 event가 신호된 상태임.

기존 두 keeper를 보강했으며 별도의 시험을 중복 추가하지 않았다.
표본 JSON 출력은 모든 상태 단언 앞으로 옮겨 다음 실패에도 단계·stdin·reader 값이 남게 했다.
입력 원문·handle·event 이름·경로는 표본에 넣지 않는다.
고정 sleep·시험 재시도·timeout 연장·기대값 완화·시험 삭제는 없다.

## 동기화 후보와 출처

[Rust Condvar](https://doc.rust-lang.org/std/sync/struct.Condvar.html)와
[Tokio Notify](https://docs.rs/tokio/latest/tokio/sync/struct.Notify.html)는 프로세스 안의 통지에 맞는다.
Rust는 MIT/Apache-2.0, [Tokio는 MIT](https://github.com/tokio-rs/tokio/blob/master/LICENSE)다.
부모와 실제 CLI 사이에는 이미 쓰는 windows-sys의
[Win32 event](https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-setevent)를 선택했다.
windows-sys는 MIT/Apache-2.0이며 새 의존성이나 외부 구현 코드는 들이지 않았다.

## 검증

모든 검증 명령은 `aaa verify`를 거쳤다. 실제 Windows 11(10.0.26200)의 SSH 경로에서 실행했다.

| 범위 | 결과 |
| --- | --- |
| Windows 설치 owner 전체 | 42통과·1실패·기존 3무시, 15.47초 |
| 이번 원래 stdin keeper 두 개 | 모두 통과, 실제 승인 보존까지 확인 |
| Windows native strict Clippy | `aaa` binary·install owner 통과 |
| 기준 main의 별도 SSH 경계 owner | 같은 기대 3·실제 4 실패, 0.77초 |
| Rust fmt | 통과 |
| 공개 저장소 검사 | 통과 |

전체 설치 실행의 실패는 `ssh_environment_and_helper_hash_flags_do_not_bootstrap_a_new_trust_root`다.
실제 OS의 SSH 조상 검증을 통과한 뒤 0으로 채운 helper hash가 conflict/4로 거부됐다.
이 시험의 비SSH 전제와 실기 실행 문맥이 다르다.
기준 main `1b3ec5c9`를 자기 target 아래 별도 detached worktree로 만들고,
그 원본을 Windows의 별도 소스·Cargo target·임시 폴더에 복사해 같은 owner를 한 번 실행했다.
기준에서도 기대 3·실제 4로 실패했다. 원본과 수정본 두 Rust 파일은 각각 SHA-256으로 대조했다.
기존 [같은 SSH 문맥 차이 기록](testing-win-pty-native-ci-t1658.md)과도 일치한다.
전체 설치 owner를 전부 통과했다고 기록하지 않으며 이 실패의 기대값을 고치지 않았다.

실기 검증 실행기의 최초 PowerShell 5 입구는 Cargo의 일반 stderr 컴파일 출력을
`NativeCommandError`로 처리해 시험 전에 중단됐다. 남은 Cargo·rustc가 없음을 확인하고,
`cmd`의 파일 리디렉션으로 출력 수집을 고쳤다. 실패한 시험을 재시도한 것이 아니다.
그 뒤 설치 전체·strict Clippy·기준 대조를 각각 한 번 실행했다.

서비스·예약 작업·Run 키·사용자 PATH·방화벽 규칙의 시작/종료 목록이 같았고,
소유한 시험 프로세스도 남지 않았다. Windows 전용 작업 폴더와 기준 detached worktree는 회수했다.
전체 착지 gate·병합·push·배포는 별도 착지 창에서 수행한다.

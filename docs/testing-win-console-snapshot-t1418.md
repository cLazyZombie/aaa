# Windows 콘솔 스냅샷의 실행 의존 제거(t-1418)

## 확인한 실패와 한계

대상은 main `2942e2db`의 [Windows CI job](https://github.com/cLazyZombie/aaa/actions/runs/41316346344/job/124877106817)이다.
전체 job 로그와 `rust-tests-windows-2025-41316346344-1` artifact의 `test.log`를 확인했다.
설치 target은 33개 통과·3개 실패였다.

| 시험 | 실제 실패 |
| --- | --- |
| `console_stack_snapshot_accepts_cargo_target_path_separators` | `snapshot_collection_timeout` unwrap |
| `console_stack_snapshot_records_the_owned_reader_without_private_values` | 같은 timeout unwrap |
| `console_stack_snapshot_rejects_a_different_image_with_typed_error` | 실제 값은 같은 timeout, 기대는 `child_image_mismatch` typed 오류 |

세 실패는 모두 helper의 5초 종료 대기에서 결정됐다. 다른 이미지 거부는 스택 수집 전에
끝나야 하므로 DbgHelp 수집만을 공통 원인으로 지목할 수 없다. 이전 t-1369의
`exit=Some(1), phase=child_image_mismatch`와 달리 이번 기록에는 helper의 종료 코드와 도달 단계가 없다.
같은 경로 비교 결함이라고 단정하지 않는다.

기존 helper는 완료·실패 뒤 임시 파일을 지웠고 artifact에는 `test.log`만 있다.
시작·CIM 준비·컴파일·수집 중 어디서 지연됐는지, 그때의 helper/대상 생존 상태와 CPU 부하는
과거 자료로 복원할 수 없다. 기존 구현은 Windows 실기의 일반 환경과 두 코어·8개 시험 병렬 조건에서도
스냅샷 세 계약이 통과했다. 따라서 이번 변경을 **과거 CI 지연 단계의 재현·확정**이라고 기록하지 않는다.

## 수리한 공통 경로

기존 5초 대기에는 PowerShell 시작, CIM 모듈·서비스 조회, 실행 중 C# 컴파일, 실제 수집과 응답이
모두 들어갔다. 이 실행 의존을 제거하고 수집기를 Rust 설치 시험과 함께 빌드한다.
시한은 5초 그대로이며 재시도·시한 확대·전체 시험 직렬화는 없다.

`terminal_snapshot.rs`가 같은 시험 실행 파일의 지정된 helper 입구를 별도 프로세스로 실행하고,
`snapshot_native.rs`가 Win32 API로 수집한다. helper는 `CREATE_NO_WINDOW`로 실행한다.
추가 실행 파일·공개 CLI·제품 실행 경로는 만들지 않는다.
새 Windows API feature는 시험용 의존성에만 둔다. 기존 PowerShell 수집 스크립트는 삭제한다.

실제 순서는 다음과 같다.

1. 부모가 요청마다 새 private 폴더에 PID·소유 부모 PID·예상 실행 경로를 쓴 뒤 helper를 실행한다.
   요청·응답은 이 폴더의 파일이며 공유 pipe·고정 파일 이름 공간은 없다.
2. helper가 요청 준비 사건을 기록하고 대상 process handle을 연다. 생존 상태, Toolhelp의 부모 PID,
   `QueryFullProcessImageNameW`의 전체 경로 순서로 검사한다. 정규화는 `GetFullPathNameW`,
   비교는 대소문자를 무시하는 `CompareStringOrdinal`이다. 같은 이름의 다른 위치 파일은 거부한다.
3. 검증한 process handle을 끝까지 보관한다. 심볼 초기화 → 소유 thread 열기·소유 PID 대조 →
   suspend → 16바이트 정렬한 AMD64 context와 bounded stack walk → resume → 심볼 정리 순서다.
   심볼 엔진은 helper 프로세스마다 분리한다.
4. 응답 파일과 준비 사건을 기록하고 종료한다. 부모는 종료 사건 뒤 크기·schema·PID·허용 문자를 검사한다.
   모듈 파일 이름·상대 위치·정제한 함수 이름만 반환하며 요청 경로·메모리·입력 원문은 옮기지 않는다.

파일 폴더와 DbgHelp 상태를 요청 사이에 공유하는 경로는 없다. 병렬 fixture 간섭의 근거는
기존 코드와 대조 실행에서 찾지 못했다. 이것을 모든 환경의 간섭 부재 증명으로 넓히지는 않는다.

## 실패 진단과 회귀 계약

helper의 요청 읽기·신원 검사·심볼 초기화·각 thread 수집 전후·정리·응답 준비를 경과 시간과 함께
원자 교체 journal에 남긴다. 실패 종료는 종료 코드·단계·오류 종류·OS 번호를 보존한다.
수집 시한 초과에는 마지막 journal, 종료 전후 코드, 대기·종료 OS 오류와 대상 생존 상태를 함께 남긴다.
진행 journal도 schema·개수·크기·단계 문자 검사를 통과한 것만 출력한다.
일반 PTY 실패의 60초 판정과 실패 뒤에만 수집하는 순서는 그대로다.

기존 세 시험은 실제 reader thread의 aaa frame, 개인값 제외, 입력 뒤 정상 종료와 재개,
슬래시·`.` 경로 허용, 다른 이미지 거부를 유지한다. 다른 이미지 오류의 종류는 .NET 예외 이름에서
native 분류 `Rejected`로 바뀌며 종료 코드·거부 단계·OS 번호까지 계속 정확히 비교한다.
거부를 성공으로 바꾸거나 검사를 느슨하게 하지 않는다.

기존 시험 두 곳만 강화했다.

- 다른 이미지 owner에 자기 자신을 대상으로 하는 소유 부모 PID 거부를 더했다.
  필요한 이유는 CIM을 대체한 부모 검사도 이미지 검사보다 먼저 동작해야 하기 때문이다.
- 기존 오류 정제 owner에 journal의 임의 개인값 거부와 정상 단계 보존을 더했다.
  필요한 이유는 새 단계 진단이 private 원문을 출력하는 통로가 되지 않아야 하기 때문이다.

새 일반 시험은 없다. 목록에 추가되는 ignored 입구 하나는 부모가 별도 프로세스로 호출하는 fixture다.
관련 계약의 세 시험은 묶음 전체가 보통 환경에서 약 1.1초로 끝났으며 고정 sleep은 없다.

## 개발 검증

모든 검증 명령은 `aaa verify -- <명령>`으로 실행했다. Windows 실기는 전용 시험 폴더,
별도 source/target/temp, 기존 kache를 사용했고 시스템 설정·기존 aaa 설치를 바꾸지 않았다.
Cargo target에는 CI처럼 슬래시 표기를 썼다. 실제 SSH 조상 유무를 검사하는 기존 owner를 위해
시험 실행은 같은 계정의 비대화형 WMI 자식에서 했다. 완료 파일의 파일 시스템 사건을 기다린다.

| 범위 | 결과 |
| --- | --- |
| Windows `cargo test --profile verify --locked -p aaa --test install`에 해당하는 전체 실행 | 36개 통과, helper 입구 1개 ignored, 14.77초 |
| 같은 설치 owner 전체, 실행 프로세스만 두 코어로 제한·libtest 8개 병렬 | 36개 통과, helper 입구 1개 ignored, 28.12초 |
| PowerShell을 찾을 수 없는 PATH·빈 `PSModulePath`, 기존 스냅샷 세 시험 | 3개 통과, 1.09초 |
| 실제 helper 수집 경과, 일반 전체 실행 | 성공 각각 353ms, 소유권/이미지 거부 38/32ms |
| 실제 helper 수집 경과, 두 코어 전체 실행 | 성공 726/781ms, 소유권/이미지 거부 79/232ms |

이 수치는 실기의 비교 결과이며 GitHub hosted runner의 보장값은 아니다.
제품 시한이나 시험 기대를 이 수치로 조정하지 않았다.

처음 실기 전체 실행은 `one_line_entry_enters_setup_and_cancels_without_installation_changes`가
실행할 Python을 찾지 못했고, 기존 Python을 PATH에 넣은 뒤에는 3.9에 `tomllib`가 없어 실패했다.
해당 소스와 오류를 확인한 뒤 공식 Python 3.13.7 embeddable package를 전용 시험 폴더 안에만 풀었다.
같은 시험의 HTTP·양 PowerShell 입구·취소·파일 보존 단언을 유지한 채 전체 36개가 통과했다.
native 개발 중 `SYMBOL_INFO.Name`의 signed byte 타입 컴파일 오류는 명시적 byte pointer 변환으로 고쳤다.
보조 실행기 준비 오류(CIM 인자 타입·64비트 handle 선언·환경 키·확인 출력 인용)는 실행기를 고쳤으며
제품 시험의 재시도나 CI 지연의 재현으로 세지 않았다.

콘솔을 만들지 않는 최종 실행 flag를 더한 뒤 같은 셸 없는 세 계약을 다시 확인했다(3개 통과, 1.79초).
최종 source 파일 digest 네 개는 실기 source와 일치했고 삭제한 PowerShell helper가 없음을 확인했다.
Windows native `cargo clippy --profile verify --locked -p aaa --test install -- -D warnings`,
`cargo fmt --all -- --check`, `git diff --check`, 공개 저장소 검사가 통과했다.
실기 시험 전후 서비스·예약 작업·Run 키·사용자 PATH·방화벽 목록은 같았다.
전용 경로를 사용하는 생존 프로세스 0개를 확인한 뒤 시험 폴더와 격리 Python까지 모두 삭제했다.

최종 통합 SHA의 전체 gate·양 OS cross·production·병합·push는 착지 창에서 별도로 한다.

## 참고한 공개 구현

- [StackWalker](https://github.com/JochenKalmbach/StackWalker/blob/master/Main/StackWalker/StackWalker.cpp):
  외부 thread의 context·bounded stack walk와 심볼 엔진 수명을 비교했다.
  [LICENSE](https://github.com/JochenKalmbach/StackWalker/blob/master/LICENSE)는 BSD-2-Clause이며,
  소스 머리의 추가 endorsement 제한도 확인했다. 코드를 가져오지 않았다.
- [Boost.Stacktrace의 MSVC 구현](https://github.com/boostorg/stacktrace/blob/develop/include/boost/stacktrace/detail/frame_msvc.ipp):
  심볼 엔진 공유와 동기화 비용을 비교했다. 소스의 BSL-1.0 고지를 확인했고,
  이 작업은 기존처럼 프로세스를 분리한다. 코드를 가져오거나 의존성을 추가하지 않았다.
- Microsoft의 [Toolhelp](https://learn.microsoft.com/windows/win32/api/tlhelp32/nf-tlhelp32-createtoolhelp32snapshot),
  [실행 경로 조회](https://learn.microsoft.com/windows/win32/api/winbase/nf-winbase-queryfullprocessimagenamew),
  [StackWalk64](https://learn.microsoft.com/windows/win32/api/dbghelp/nf-dbghelp-stackwalk64):
  조회 범위·권한·context·동일 handle 사용 기준을 확인했다.

원시 CI 로그·stack 원문·소스 묶음·개인 경로는 이 문서와 보고 library에 넣지 않는다.

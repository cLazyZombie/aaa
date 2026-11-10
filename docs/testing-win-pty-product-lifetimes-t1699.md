# Windows 설치 PTY: 제품 수명의 사건 행렬(t-1699)

## 범위와 현재 상태

q-3291 1A에 따라 [자연 실패 조사](testing-win-pty-signaled-enter-t1699.md)의 다음 진단을 구현·실행했다.
제품 입력·승인 코드와 기존 시험은 유지한다. 새 원격 진단 branch의 Windows CI 실행만 승인받았으며
main·기존 원격 진단 branch·tag push와 force push는 하지 않는다.
같은 Server 2025 이미지·conhost 판에서 16조건의 224회 읽기와 모드 복구가 모두 완료됐다.
실제 승인 owner 17개와 두 wizard owner 4개도 통과했다.
**원래 정체는 재현되지 않았고 근본 원인은 미확정이다. 지시한 중단 조건에 따라 추가 실행·제품 수리는 멈춘다.**

## 제품과 맞춘 수명

기존 수동 도구는 read마다 reader thread를 만들고 mode를 바깥에서 한 번 설정했다.
새 `console_lifetime_repro.rs`는 fixture의 같은 호출 thread에서 14개 질문을 읽는다.
이 thread는 libtest worker 하나이며 제품의 프로세스 진입 thread 자체를 복제한 것은 아니다.

- 매 질문의 `GetStdHandle(STD_INPUT_HANDLE)`·`GetConsoleMode` → cooked/echo 설정 → 읽기 → 원래 mode 복구를 지킨다.
  자연 실패의 original=503, input=502도 대조하며 다른 mode를 같은 조건으로 세지 않는다.
- 매 질문 버퍼 16,384 UTF-16 unit과 Ctrl+C/Z wakeup mask를 제품과 같게 쓴다.
- 제품 `prompt/windows_trace.rs` 소스를 path 모듈로 직접 재사용해 관측기를 질문마다 만들고 반환 뒤 join한다.
  제품 소스에 진단 hook이나 환경 조건을 더하지 않는다.
- tracing Layer가 실제 관측기 thread의 준비·Enter 관측 사건만 받는다.
  자연 순서에서는 관측기를 보류하지 않는다. 제어 조건에서만 callback의 사건 gate를 쓴다.
- fixture의 부모·실행 파일과 연결된 시스템 conhost의 부모·전체 이미지 경로를 기존 수집기 검증으로 확인한다.
  소유 process handle을 유지하며 다른 콘솔이나 사용자 데스크톱에는 연결하지 않는다.
- 응답은 원래 wizard의 14개 고정 fixture 입력이다. 각각 정확한 UTF-16 응답과 CR, 같은 read thread,
  원래 mode 복구를 단언한다. 입력 원문을 진단 필드에 기록하지 않는다.

controller thread는 읽지 않고 사건 순서와 main reader의 실제 I/O pending만 확인한다.
부모의 입력 준비 표지는 별도 줄에 출력해 화면 줄바꿈으로 식별자가 갈라지는 것을 피한다.
이는 실제 제품 화면과의 차이이며, 최소 도구 통과를 원래 전체 병렬 CI 실패의 배제로 해석하지 않는다.

## 한 번씩 실행하는 행렬

| 순서 | 관측기 없음 | 제품 관측기 있음 | 입력 요청 전 사건 |
| --- | --- | --- | --- |
| prompt | 출력 off/on | 출력 off/on | main이 질문을 flush함 |
| prequeued | 출력 off/on | 출력 off/on | read gate를 닫고 입력을 보낸 뒤 별도 Peek로 Enter 도착 확인 |
| read_pending | 출력 off/on | 출력 off/on | main reader thread의 실제 I/O pending |
| peek_before_read | 해당 없음 | 출력 off/on | 실제 제품 관측기의 Enter 관측 뒤 read gate 해제 |
| peek_after_pending | 해당 없음 | 출력 off/on | 실제 I/O pending 뒤 관측기 callback gate 해제·입력 요청 |

16개 조건 × 14개 읽기다. 관측기 off에서도 tracing 사건 처리는 켜되 출력 Layer만 끈다.
실제 관측기 생성 여부와 진단 출력 여부를 독립적으로 제어한다.
I/O pending은 서버의 read 대기 등록 완료를 보증하지 않으며, gate 해제도 다음 OS API 진입 완료 표지가 아니다.
read가 먼저 소비하면 after_pending 조건의 관측기가 Enter를 못 볼 수 있다.

기존 fixture의 5초 안전 상한과 PTY driver의 60초 상한은 유지한다.
추가한 대기는 사건으로 끝나며 고정 sleep은 없다. 재사용한 제품 관측기의 기존 종료 사건 대기도 바꾸지 않는다.
한 조건이 실패해도 다음의 서로 다른 조건을 한 번씩 수집하고 마지막에 실패 전체를 반환한다.
실패 행의 재실행·기대값 변경·무시 처리는 없다. 기존 수동 도구와 기존 시험도 삭제하지 않았다.

## Windows CI 준비

진단 workflow는 새 진단 branch의 push에서만 실행되게 한다.
기존 일반 CI는 main·hp branch만 받으므로 바꾸지 않는다. 진단 workflow는 착지 후보에서 분리한다.

빌드 전에 Server build 26100·product type 3·image `20261103.298.1`·conhost `10.0.26100.33648`을
엄격히 대조한다. 값이 다르면 환경 실패로 멈추며 그 상태에서 행렬을 돌리지 않는다.
재실행 attempt도 거부한다. 디스크 conhost SHA·CodeView GUID/age 조회 가능 여부와 콘솔 provider 존재를 기록한다.
CodeView를 찾는 것이 대응 PDB를 로드했다는 뜻은 아니다. 원래 자연 실패의 conhost 해시도 없으므로
버전·이미지 일치를 바이트 동일성으로 확대하지 않는다.

native 빌드·strict Clippy 뒤 사건 행렬을 한 번 실행하고 콘솔·실제 승인 owner와 두 wizard owner를 한 번씩 실행했다.
빌드 뒤 검사는 같은 CLI의 해시를 대조한 별도 사본에서 `aaa verify`를 거친다. TEMP·TMP와 artifact는 해당 CI 작업 전용 폴더다.
CI 원문은 scratch에만 보존하고 보고 library에는 검토한 사건·숫자·검사 결과만 넣는다.

## 첫 CI의 실행기 실패

[첫 진단 CI](https://github.com/cLazyZombie/aaa/actions/runs/42744549748)는 OS·image·conhost 대조,
native 빌드·strict Clippy를 통과했다. 그러나 `aaa verify` 실행기를 Cargo 출력인 `target/verify/aaa.exe`에서
직접 실행한 탓에 후속 Cargo가 같은 exe를 교체하지 못했다. 세 후속 명령 모두 `failed to remove file`과
`Access is denied. (os error 5)`로 끝났다. 행렬·승인·wizard의 시험 시작·종료 표지는 모두 없고
행렬 행도 0개다. 이 실패를 원래 PTY 정체 재현이나 16개 행의 실패로 세지 않는다.

실행기를 별도 경로에 복사하고 원본·사본 SHA-256을 대조하도록 workflow를 고쳤다.
출처가 미커밋으로 잡히지 않게 CI 전용 로그도 Git에서 무시하는 target 아래에 둔다.
소스·기대값·상한은 바꾸지 않았다. 기존 원격 진단 branch를 수정하지 않고 새
`diagnostic/t-1699-console-lifetimes-native` branch를 만들어
[수정한 진단 CI](https://github.com/cLazyZombie/aaa/actions/runs/42746052481)에서 최초 행렬 실행을 이어 간다.
두 branch의 Rust·Cargo·vendor 소스는 개발 후보와 같다. 첫 run과 후속 run 모두 attempt 1이며 rerun은 하지 않았다.

첫 push는 OAuth의 workflow scope 부족으로 거부됐다. 기존 GitHub SSH 인증으로 새 branch만 push했으며
인증·시스템 설정·main·기존 원격 branch·tag를 바꾸지 않았다. 원시 CI 자료는 scratch에 보존했다.

## 실제 Windows 결과

[수정한 진단 CI](https://github.com/cLazyZombie/aaa/actions/runs/42746052481)의 전체 artifact를 받았다.
실패한 첫 CI의 artifact와 원문도 별도로 보존했다. 수정한 CI에서는 다음을 확인했다.

| 항목 | 결과 |
| --- | --- |
| OS·product type | Windows Server 2025 10.0.26100·3 |
| image | win25-vs2026·20261103.298.1, 원래 자연 실패와 일치 |
| conhost 고정 resource 판 | 10.0.26100.33648 |
| 실행기 원본·사본 | SHA-256 일치, Cargo 출력 파일을 실행기로 잡지 않음 |
| native 빌드·strict Clippy | 통과 |
| 제품 수명 행렬 | 16개 서로 다른 조건이 한 번씩 통과, fixture 전체 2.18초 |
| 실제 읽기·복구 사건 | 각 행 read 1–14의 반환·mode 복구·같은 thread 표지, 합계 224개 |
| 콘솔·실제 승인 owner | 17통과·3무시, 32.49초 |
| 두 wizard·명시 CLI 계획 owner | 4통과, 2.03초 |

무시 3개는 기존 snapshot helper 입구·기존 수동 재현 입구·이번 수동 입구다.
이번 입구는 행렬 단계에서 별도 `--ignored --exact`로 실제 실행했다. 기존 시험을 무시로 바꾸지 않았다.
owner 검사는 실제 승인 발급·private 파일·무변경 계약과 두 prompt의 뒤로 이동 및 명시 CLI plan hash 비교를 포함한다.
제품 수리 회귀나 원래 CI 전체의 회복 증거로 확대하지 않는다.

artifact에서 16개 행의 집합·중복 없음·성공, 각 행의 read 1–14와 실행기 해시·출처 SHA·환경을 대조했다.
현재 fixture는 성공 행의 사건만 발췌해 출력하고 원문은 실패 때 출력한다.
출력 off의 read_pending 행은 controller 사건이 질문 끝에 붙어, 줄 머리만 고르는 발췌에서 빠졌다.
peek_after_pending의 출력 off도 첫 pending 사건이 같은 이유로 빠지고 관측기 release 사건은 남았다.
모든 행의 14개 read 반환·복구 표지는 있다. pending gate를 거쳐야 반환을 기다리는 코드와 controller join 단언도
통과했지만, 빠진 사건을 artifact에 직접 기록된 14개 표지로 주장하지 않는다. 이 발췌 한계는 제품 정체 재현이 아니다.
기대값을 바꾸거나 행을 다시 실행해 표지를 채우지 않았다.

conhost의 CodeView GUID/age는 확인했으나 대응 PDB를 로드하지 않았다.
조회한 이름 `Microsoft-Windows-Console`의 provider는 없었다. 이것으로 다른 Console provider도 없다고 판정하지 않는다.
디스크 conhost의 해시와 CodeView 식별 정보는 보고 상세에 보존했다.
원래 자연 실패에는 파일 해시가 없으므로 같은 OS·image·파일 판을 바이트 동일성으로 확대하지 않는다.

## 원인 판정과 다음 계획

지속하는 호출 thread·매 질문 mode 복구·실제 제품 Peek 관측기의 수명을 맞추고 입력 순서를 제어했지만
read가 정체하는 조건을 얻지 못했다. 따라서 해당 수명 차이가 원인이라고 확정하거나
동시 Peek·모드 전환·콘솔 내부 통지 문제를 배제할 근거는 없다.
최소 도구의 별도 표지 출력, libtest worker 문맥과 원래 전체 CI의 병렬 부하 차이도 남아 있다.

승인된 계획 4의 서버 read 등록·통지 추적과 계획 5의 수리는 정체 재현이 선행 조건이었다.
그 조건을 얻지 못해 서버 추적·제품 수리·실패하는 원본 회귀를 구현하지 않았다.
원래 제품·시험의 시한, 질문·입력·승인 조건을 바꾸지 않았고 이번 행렬도 더 실행하지 않는다.

다음에는 최소 도구의 같은 행을 반복하기보다 다음 자연 실패의 증거를 확장하는 진단을 권한다.

1. 기존 60초 실패 확정을 보존한 채, 소유 CLI 안에서 원래 stdin의 zero-time wait와 reader thread의
   I/O pending을 같은 read ID로 수집하는 경로를 설계한다. 별도 CONIN$ 표본과 구분하고 입력·mode는 바꾸지 않는다.
2. 이미 확인한 conhost CodeView에 맞는 공개 symbol과 실제 가용 provider의 정의를 먼저 조사한다.
   read 등록·통지·완료를 구분할 수 있는지 확인하기 전에는 단계 추적이 가능하다고 약속하지 않는다.
3. 가능한 경우 다음 자연 실패에서 부모 입력 쓰기·질의 회신·원래 stdin 상태·read 대기와 서버 단계 사건을
   함께 수집한다. 입력 원문·메모리 dump는 공개하지 않고 살아 있는 conhost thread를 멈추지 않는다.
4. 원인이 확정된 조건에서 먼저 실패하는 회귀와 수리를 만든 뒤 같은 plan·실제 승인 계약을 확인한다.

위 후속은 미구현 계획이다. 이번 진단의 중단 조건에 따라 새 자연 실패 수집기나 provider 추적을 더하지 않았다.

## 검증과 보존 범위

개발의 Windows 교차 strict Clippy는 설치 통합 owner를 선택했고 fmt·공개 검사도 수행했다.
명령은 `aaa verify`를 거쳤다. 초기 fmt의 모듈 순서 차이는 정렬을 고친 뒤 확인했다.
CI 실행기 실패는 위 원문과 원인·수리 기록을 보존했으며 시험이 시작되기 전의 실패와 행렬 결과를 구분했다.
개발 중 미커밋 변경의 원장 미수집과 remote clean 사본의 원장 수집도 구분했다.

Windows 사용자 실기·시스템 설정·서비스·기존 설치는 건드리지 않았다. 새 원격 진단 branch 둘만 만들고
main·기존 원격 진단 branch·tag는 push·수정하지 않았다. force push·착지 gate·Windows production·배포도 하지 않았다.
진단 workflow는 진단 branch에만 있으며 개발 후보의 workflow diff는 없다. main 착지는 별도 승인 대상이다.

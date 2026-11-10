# Windows 설치 PTY: 제품 수명의 사건 행렬(t-1699)

## 범위와 현재 상태

q-3291 1A에 따라 [자연 실패 조사](testing-win-pty-signaled-enter-t1699.md)의 다음 진단을 구현한다.
제품 입력·승인 코드와 기존 시험은 유지한다. 새 원격 진단 branch의 Windows CI 실행만 승인받았으며
main·기존 원격 진단 branch·tag push와 force push는 하지 않는다.
**도구 구현 단계이며 근본 원인 수리나 CI 회복을 뜻하지 않는다.**

## 제품과 맞춘 수명

기존 수동 도구는 read마다 reader thread를 만들고 mode를 바깥에서 한 번 설정했다.
새 `console_lifetime_repro.rs`는 fixture의 같은 main thread에서 14개 질문을 읽는다.

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

진단 workflow는 새 `diagnostic/t-1699-console-lifetimes` branch의 push에서만 실행되게 한다.
기존 일반 CI는 main·hp branch만 받으므로 바꾸지 않는다. 진단 workflow는 착지 후보에서 분리한다.

빌드 전에 Server build 26100·product type 3·image `20261103.298.1`·conhost `10.0.26100.33648`을
엄격히 대조한다. 값이 다르면 환경 실패로 멈추며 그 상태에서 행렬을 돌리지 않는다.
재실행 attempt도 거부한다. 디스크 conhost SHA·CodeView GUID/age 조회 가능 여부와 콘솔 provider 존재를 기록한다.
CodeView를 찾는 것이 대응 PDB를 로드했다는 뜻은 아니다. 원래 자연 실패의 conhost 해시도 없으므로
버전·이미지 일치를 바이트 동일성으로 확대하지 않는다.

native 빌드·strict Clippy 뒤 사건 행렬을 한 번 실행하고 콘솔·실제 승인 owner와 두 wizard owner를 한 번씩 실행한다.
빌드 뒤 검사는 같은 CLI의 `aaa verify`를 거친다. TEMP·TMP와 artifact는 해당 CI 작업 전용 폴더다.
CI 원문은 scratch에만 보존하고 보고 library에는 검토한 사건·숫자·검사 결과만 넣는다.

## 판정과 남은 단계

원본이 정체하는 조건을 얻으면 해당 conhost의 read 등록·입력 통지·cooked 완료·클라이언트 반환을
구분하는 추적을 설계한다. provider와 symbol, payload를 먼저 검토해 입력 원문·메모리 dump를 공개하지 않는다.
재현 전 공개 Terminal main 소스나 WCT 한 노드로 원인을 확정하지 않는다.

원인을 확정하면 먼저 실패하는 회귀와 계약을 보존하는 수리를 만들고 실제 Windows 관련 owner로 확인한다.
실제 승인 발급·명시 CLI와 같은 plan도 확인한 뒤 병합 준비 상태로 보고한다.
확정하지 못하면 관측 결과와 다음 계획을 남기고 멈춘다. main 착지는 별도 승인 대상이다.

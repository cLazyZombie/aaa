# 설치 PTY 추가 진단의 Windows 실기·Server 2025 검증(t-1658)

## 결론

추가한 입력 준비 상태·터미널 질의 진단과 수동 재현 도구를 Windows 실기와 Server 2025에서 확인했다.
각 환경에서 여섯 조건, 84회 읽기가 정확히 완료됐다. 원래 60초 `ReadConsoleW` 정체는 재현되지 않았다.
**근본 원인은 여전히 미확정이며 제품 입력의 원인 수리는 하지 않았다.**

새 진단의 파일 판 조회에서 발견한 호환성 버전 오보고는 수리했다.
이는 원래 PTY 정체의 수리가 아니다. 기존 60초 상한·기대값·승인 계약은 유지한다.
현재 후보는 시험 helper·수동 도구·문서이며 제품 prompt·wizard·승인 코드는 바꾸지 않았다.

첫 실패의 사건 대조는 [초기 조사](testing-win-pty-queued-enter-t1658.md),
진단 필드·재현 순서 설계는 [구현 기록](testing-win-pty-readiness-t1658.md)에 있다.

## 원인 판정의 범위

초기 실패에서 확정한 것은 plain wizard의 열두 번째 read가 반환하지 않았고,
실패 뒤에도 입력 큐 앞부분에 Enter KEY_DOWN이 있었다는 사실이다.
이번 통과는 그 정체가 항상 발생하는 조건을 찾았다는 증거가 아니다.

재현 도구가 제어한 것은 다음 순서다. 각 순서에 긴 한글·SGR 출력 유무를 조합했다.

| 순서 | 입력 요청 전 관측 | 조건별 읽기 |
| --- | --- | --- |
| prequeued | reader gate를 닫은 채 Peek로 Enter 도착 확인 | 14 |
| read_pending | 소유 reader thread의 실제 I/O pending 확인 | 14 |
| peek_armed | Peek 관측기 준비와 reader I/O pending 확인 | 14 |

I/O pending은 서버 내부 대기 등록 완료 표지가 아니다. 관측기 준비도 ReadConsole/Peek의
서버 내부 실행 순서를 강제하지 않는다. 첫 순서는 Peek 뒤 읽기를 보장하지만,
동시 관측에서는 reader가 먼저 소비해 관측기가 Enter를 못 볼 수 있다.
원래 CI의 다른 owner들과 같은 전체 병렬 부하를 재현한 실험도 아니다.
따라서 이 결과로 간헐 경쟁이나 콘솔 내부 깨우기 결함을 배제하지 않는다.

승인된 계획 4의 서버 대기 등록·통지 추적은 실제 정체 재현이 선행 조건이다.
이번에는 그 조건을 얻지 못해 추가하지 않았다. 원인 회귀 시험·제품 수리도 아직 없다.
진단만 착지한 뒤 다음 자연 실패의 새 필드를 대조하는 것을 권한다.

## 실기에서 먼저 발견한 조회 결함

실기는 Windows 11 `10.0.26200`, product type 1이다.
같은 시스템 conhost에 대해 아래 세 값이 달랐다.

| 조회 | 실제 관측 |
| --- | --- |
| PowerShell의 표시용 FileVersion 문자열 | 10.0.26100.8875 |
| PowerShell의 고정 숫자 네 자리 | 10.0.26100.9549 |
| manifest 없는 시험 프로세스의 GetFileVersionInfoW | 6.2.26100.9549 |
| 파일을 복사해 PE의 RT_VERSION bytes를 독립 파싱 | 10.0.26100.9549 |

수동 도구는 판이 다르다는 보호 조건으로 환경 대조 단계에서 멈췄으며,
이때 여섯 조건은 아직 실행하지 않았다. 값을 낮추거나 버전 대조를 없애지 않았다.

수집기가 `LoadLibraryExW`의 data/image resource 모드로 파일을 매핑하고 RT_VERSION의
고정 숫자만 읽도록 고쳤다. 실행 코드·import 초기화를 하지 않고 제품 manifest나 시스템 설정도 바꾸지 않는다.
resource 크기·root 길이·종류·UTF-16 key·signature를 검사하고 숫자 네 개만 내보낸다.
잘못된 길이·잘린 resource·다른 key·signature를 거부하는 작은 owner도 추가했다.

실기의 독립 숫자 조회와 수정한 도구가 모두 10.0.26100.9549로 일치한 뒤 조건을 실행했다.
출력·입력 기대값이나 5초 fixture 안전 상한, 기존 PTY의 60초 상한은 바꾸지 않았다.

공식 [버전 resource 구조](https://learn.microsoft.com/en-us/windows/win32/menurc/vs-versioninfo)와
[resource 전용 매핑 계약](https://learn.microsoft.com/en-us/windows/win32/api/libloaderapi/nf-libloaderapi-loadlibraryexw)을 따랐다.
[Microsoft의 동일 API 질의 사례](https://learn.microsoft.com/en-us/answers/questions/2279451/why-does-getfileversioninfo-return-incorrect-file)에도
manifest에 따른 6.2/10.x 차이가 보고돼 있다. 외부 코드는 실행하거나 복사하지 않았다.

## Windows 실기 결과

전용 폴더의 처음 상태는 임시 폴더 하나뿐이었다. 이전 준비 명령이 남긴 기준 기록은 없어서
새 기준 목록을 수집한 뒤 후보 소스의 정확한 HEAD로 빌드했다.
target·TEMP·TMP는 작업 전용 폴더 안에 두고 기존 빌드 캐시는 유지했다.

| owner·대조 | 결과 |
| --- | --- |
| 첫 후보의 설치 통합 전체 | 39통과·1실패·2무시, 64.16초 |
| 공유 helper의 암호 입력·질의 owner | 2통과, 0.13초 |
| 수정한 resource 수집기의 native owner | 3통과·수동 입구 1무시, 0.05초 |
| 수정한 수집기의 실제 스택·이미지 owner | 3통과, 4.39초 |
| 두 wizard owner | 4통과, 2.31초 |
| 수정한 수동 도구 | 6조건 × 14읽기 완료, 1통과, 1.00초 |

무시는 기존 snapshot helper 입구와 새 수동 재현 입구다. 기존 시험을 삭제·무시 처리하지 않았다.
일반 실행에서는 수동 입구를 제외하고 실제 실행은 별도 `--ignored --exact`로 한 번 했다.

### 전체 설치 시험의 SSH 실행 문맥 차이

실패한 것은 `ssh_environment_and_helper_hash_flags_do_not_bootstrap_a_new_trust_root`다.
종료 코드 기대 3에 실제 4가 나왔으며, 입력/파일 변경 단언까지 진행한 시험이 아니다.
이는 이번 PTY 상한이나 새 콘솔 조회 실패가 아니다.

같은 실기의 같은 SSH 경로에서 기준 main `c9119f6a`를 자기 target 아래 별도 detached worktree로 만들고
같은 owner를 한 번 실행했다. 기준에서도 실제 4·기대 3으로 실패했다(0.86초).
같은 실패를 통과할 때까지 반복하거나 기대값을 바꾸지 않았다.

제품은 환경 문자열보다 먼저 실제 시스템 sshd 조상 프로세스를 확인한다.
이 실기 실행은 실제 SSH 자손이므로 그 검증을 지나고, 고정 helper hash 불일치가 conflict/4를 반환한다.
환경 문자열만으로 신뢰를 얻지 못한다는 시험의 비SSH 전제가 여기에는 맞지 않는다.
별도 비SSH Server CI에서는 해당 owner를 포함한 승인 묶음이 통과했다.
제품의 SSH 검증을 완화하거나 이 시험을 삭제하지 않았다.

## Server 2025 진단 CI

사용자 승인으로 원격 진단 branch 하나만 push했다. 실행은
[첫 진단 run](https://github.com/cLazyZombie/aaa/actions/runs/42550688041), attempt 1이다.
재실행하지 않았다. main·tag push와 force push도 하지 않았다.

| 환경 | 관측 |
| --- | --- |
| OS·product type | 10.0.26100, 3(Server) |
| image 계열·판 | win25-vs2026, 20261103.298.1 |
| 실패 run과 image 판 비교 | 같음 |
| conhost 고정 파일 판 | 10.0.26100.33648 |

실패 run과 OS·image 판이 같다. 원래 실패 로그에는 conhost 판·원본 파일이 없으므로
그때의 conhost와 바이트 동일성을 직접 입증한 결과로 확대하지 않는다.

| 검사 | 결과 |
| --- | --- |
| native 빌드·strict Clippy | 통과 |
| 확장한 콘솔·승인 owner | 17통과·수동 helper 2무시, 31.67초 |
| 두 wizard owner | 4통과, 2.08초 |
| 공유 helper의 암호 입력 owner | 2통과, 0.15초 |
| 수동 재현 | 6조건 × 14읽기 완료, 1통과, 0.94초 |

CI 전용 workflow는 이 진단 branch에서만 실행되게 했고, 그 branch는 일반 CI에서 제외했다.
이 workflow 변경은 착지 후보에서 분리했다. 착지 후보와 진단 CI의 Rust·Cargo 소스는 동일하다.
원격 branch는 사용자 승인 없이 삭제하지 않고 남겨 둔다.

## 실기 정리와 설정 대조

예약 작업·Run 키·사용자 PATH·방화벽 목록은 전후 같았다.
수집한 서비스 목록에는 추가·삭제가 없고 BITS의 StartMode 한 값이 Auto에서 Manual로 달랐다.
사용자 pcode 서비스의 수집 항목은 같았다.

System 7040 이벤트에는 첫 Cargo 실행 전부터 BITS의 Auto/Manual 전환이 반복돼 있었다.
수집 구간에서 11건을 확인했다. 호출 주체는 이 자료로 확정하지 않는다.
이 작업의 준비·시험 코드에는 BITS 설정 명령이 없다. 일치하는 결과를 얻으려고 목록을 다시 뽑거나
시스템 값을 되돌리지 않고, 최초 차이를 그대로 보존했다.

초기 프로세스 목록에 잡힌 한 cmd는 조회용 SSH 셸이었고 이미 종료한 것을 확인했다.
마지막 삭제 경계에서는 자기 조회 프로세스와 검증한 조상만 제외한 뒤 writer 0,
기록한 PID·생성 시각과 같은 생존 프로세스 0, reparse point 0을 다시 확인했다.
작업 전용 폴더를 삭제하고 부재를 확인했다. 사용자 pcode 서비스나 다른 작업 프로세스는 종료·변경하지 않았다.

원시 CI·실기·설정 목록·OS 파일 사본·로그는 scratch에만 보존한다.
보고 library에는 사건 값·숫자 판·검사 요약만 발췌한다.

## 남은 단계

진단을 main에 착지하려면 별도 승인이 필요하다. 승인 후 최종 통합 SHA의 gate를 한 번 수행한다.
현재 단계에서 전체 착지 gate·Windows production·배포는 실행하지 않았다.
원래 PTY 정체의 원인 확정·회귀·수리는 미완료이며, 다음 자연 실패의 새 상태 수집을 기다려야 한다.

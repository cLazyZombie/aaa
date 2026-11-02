# Windows plain 설치 입력 정체와 후속 진단(t-1472)

## 결론

main `141a84c2`의 새 wizard 실패는 종료한 자식의 EOF 수집 정체가 아니다.
살아 있는 CLI가 plain 머신 이름 질문의 `ReadConsoleW`에서 멈췄다.
기존 Ctrl+Z·yes+CR 실패와 공통 근본 원인인지, 입력 손실인지 콘솔 내부 대기인지는 미확정이다.
이 변경은 다음 실패를 구분하기 위한 진단이며 정체 수리나 CI 회복 완료가 아니다.

## 자연 실패 원문 대조

[Windows job](https://github.com/cLazyZombie/aaa/actions/runs/41469474112/job/125358949516)의 전체 로그와
`rust-tests-windows-2025-41469474112-1` artifact의 전체 `test.log`를 읽었다.

- 첫 번째 `plain=true` wizard다. host 선택 뒤 네 번 뒤로 이동하고 agent 역할을 선택했다.
- 마지막 전송은 11번째 응답인 공개 fixture `agent-final`과 CR이다. 쓰기 시작 103451µs, flush 완료 103475µs다.
- 14개 중 11개 응답을 보냈고 다음 `남겨 둘 여유 GB` 질문을 기다리다가 60.0104813초에 실패했다.
- `read_id=11`에서 입력 준비·읽기 호출·KEY_DOWN 관측은 있고 읽기 반환은 없다.
- 자식 생존, reader 미완료다. 실패 확정 뒤 365ms에 수집한 자식의 실제 스택은
  `NtDeviceIoControlFile → ReadConsoleW → prompt::windows::line → wizard::ask → wizard::run`이다.
- 승인 발급·TLS 연결·계획 해시 비교 이전의 실패다. 당시 실제 CR 도착·소비, conhost 내부 상태와 순간 부하는 없다.
  KEY_DOWN만으로 어느 키가 도착했는지, CR까지 모두 도착했는지 알 수 없다.

[t-1303](testing-win-auth-t-1303.md)의 Ctrl+Z와 [t-1365](testing-ci-three-os-t1365.md)의 yes+CR도
읽기 미반환이지만 동일 근본 원인으로 확정하지 않는다.
[t-1418](testing-win-console-snapshot-t1418.md)의 별도 수집기 timeout 수리는 기준에 들어 있으며,
이번 세 스냅샷 계약과 실패 뒤 수집기는 정상 완료했다.
기존 배정·보고를 읽어 중복 작업이 없음을 확인했고 원 thread를 재개하지 않았다.

## 변경과 보존하는 계약

입력 관측에 Enter·Ctrl+C·Ctrl+Z KEY_DOWN 존재 여부를 더했다. 기존 Peek와 종료 순서는 유지한다.
입력 값·입력 길이·질문·승인서는 기록하지 않는다. 관측하지 못한 키를 미도착으로 판정하지 않는다.

실패 뒤 실행하는 기존 helper에 같은 CLI의 콘솔 호스트 진단을 더했다.
`ProcessConsoleHostProcess`의 읽기 전용 연관 정보에서 PID를 얻고 하위 flag 두 비트를 제거한다.
기존 검증으로 직계 부모가 시험 프로세스인지, 이미지 전체 경로가 시스템 `conhost.exe`인지 확인하고
같은 process handle을 보유한다. 조건이 맞지 않으면 단계·오류 코드만 남긴다.
연관 정보 조회의 NTSTATUS와 Win32 오류를 버리지 않는다. 이 비공개 정보 클래스의 지원을 제품 동작 조건으로 쓰지 않는다.

호스트의 누적 kernel/user CPU 시간과 최대 32개 thread의 WCT 오브젝트 종류·상태·cycle·OS 오류를 수집한다.
호스트 thread를 중지하거나 메모리를 읽지 않는다. WCT 오브젝트 이름·경로·다른 프로세스 식별 정보는 내보내지 않는다.
WCT가 지원하지 않는 대기는 한 노드로 보일 수 있고, 권한 부족은 오류로 남는다. 한 번의 누적 CPU 수치는 순간 부하 표본이 아니다.

원래 CLI reader 스택 수집, private 요청 폴더, 자식 소유권·이미지 검사, 비밀 비노출은 유지한다.
호스트 진단도 같은 helper의 기존 **5초** 수집 상한 안에서 수행한다. PTY 성공/실패 판정 **60초**는 그대로다.
입력 방식·승인 조건·기대 종료 코드·파일 무변경 단언·뒤로 이동·동일 계획 해시 단언은 바꾸지 않았다.
새 일반 시험을 늘리지 않고 기존 두 실제 스냅샷 owner에 호스트 상태 존재 단언을 더했다.

## 개발 중 확인한 실패와 제외한 가설

- 초기에 Windows 사본에 Git 메타데이터가 없어 `aaa verify`가 실행 전에 거부했다.
  원본 bundle로 사본의 HEAD를 구성했다. origin 부재로 빌드 뒤 원장 기록이 실패한 문제도 사본의 origin을 설정해 고쳤다.
  빌드/시험 결과와 원장 기록 결과는 구분했다.
- 별도 ConPTY 도구의 빌드는 workspace의 vt100 patch·Win32 Security feature 누락으로 실패했다.
  그 도구의 첫 실행에서는 API가 반환했지만 도구가 LF까지 필수라고 잘못 가정한 단언에서 실패했다.
  실제 정체 재현이나 24가지 대조 통과로 세지 않는다. 기대값을 바꿔 성공으로 전환하지 않았다.
- 첫 호스트 식별 후보는 `GetConsoleWindow`의 HWND 소유자를 사용했다. 기존 이미지 검사가
  `child_image_mismatch`로 거부했고 PID 비교로 HWND 소유자와 CLI PID가 같음을 확인했다.
  검사 기대값을 느슨하게 하지 않고 이 식별 방법을 제거했다. 실제 커널 연관 정보와 소유권 검증으로 대체했다.
- 호스트의 외부 stack walk는 사용하지 않는다. 수집기 강제 종료 때 중지한 호스트 thread가 남을 수 있으므로
  호스트를 멈추지 않는 WCT·CPU 조회로 한정했다. 기존 CLI 스택은 기존대로 수집한다.

## 검증

모두 `aaa verify`로 실행했다. 검증 중 같은 CLI를 재빌드하지 않았고 Windows는 작업 전용 사본·TEMP를 사용했다.

| 범위 | 결과 |
| --- | --- |
| 원본 Windows wizard·취소 두 owner | 2개 통과, 1.91초. 원본 CI 미재현이며 회복 근거 아님 |
| 원본 macOS 같은 두 owner | 2개 통과, 2.03초. Windows 대체 판정 아님 |
| Windows 진단 후보의 스냅샷 세 계약·오류 정제·wizard·취소 | 6개 통과, 2.29초 |
| macOS 설치 owner 전체 | 35개 통과·기존 1개 ignored, 12.92초 |
| Windows wizard 전체·실제 승인 발급·private TLS enrollment | 6개 통과, 7.94초 |
| Windows·macOS CLI/install native strict Clippy | 통과 |
| fmt·diff·공개 저장소 검사 | 통과 |

Windows에서 검증한 Rust 네 파일과 로컬 후보의 SHA-256을 대조해 모두 일치했다.
시험 전후 서비스·예약 작업·Run 키·사용자 PATH·방화벽 목록은 같았고 전용 폴더의 생존 실행 프로그램은 0개였다.

착지 창의 최종 통합 SHA gate·cross·production 빌드·전달은 실행하지 않았다.
진단 후보의 owner 성공을 자연 실패의 수리 완료로 쓰지 않는다.

## 참고 구현과 한계

- [Microsoft Terminal cooked read](https://github.com/microsoft/terminal/blob/main/src/host/readDataCooked.cpp),
  [MIT](https://github.com/microsoft/terminal/blob/main/LICENSE): cooked 읽기의 대기·재표시 경계를 대조했다.
  폭 1칸 wide glyph 결함인 [#19994](https://github.com/microsoft/terminal/pull/19994)를 현재 120칸 실패 원인으로 채택하지 않았다.
- [Crossterm Windows mode](https://github.com/crossterm-rs/crossterm/blob/master/src/terminal/sys/windows.rs),
  [MIT](https://github.com/crossterm-rs/crossterm/blob/master/LICENSE)와
  [Rustyline Windows 입력](https://github.com/kkawakam/rustyline/blob/master/src/tty/windows.rs),
  [MIT](https://github.com/kkawakam/rustyline/blob/master/LICENSE): raw/key-event 대안을 비교했지만 원인 근거 없이 교체하지 않았다.
- [PHNT 연관 정보 정의](https://github.com/winsiderss/phnt/blob/master/ntpsapi.h),
  [MIT](https://github.com/winsiderss/phnt/blob/master/LICENSE): 정보 클래스와 PID flag의 ABI를 확인했다.
- [Microsoft WCT](https://learn.microsoft.com/en-us/windows/win32/debug/wait-chain-traversal)와
  [GetThreadWaitChain](https://learn.microsoft.com/en-us/windows/win32/api/wct/nf-wct-getthreadwaitchain):
  동기 수집·권한/미지원 오류와 출력할 필드의 경계를 확인했다. 권한·시스템 설정을 바꾸지 않는다.

외부 구현 코드를 복사하거나 런타임 의존성을 추가하지 않았다. 다음 자연 실패 또는 별도 승인된 진단 CI의
실제 실패 근거가 있어야 CR 미도착·입력 소비·콘솔 호스트 대기를 더 좁힐 수 있다.

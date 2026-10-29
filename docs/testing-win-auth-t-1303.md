# Windows 설치 승인 Ctrl+Z 정체 조사(t-1303)

아직 원인은 확정하지 못했다. 아래 변경은 다음 실패의 관측을 위한 진단이며,
설치 승인 PTY의 60초 정체를 수리했다고 주장하지 않는다.

## 원본 CI에서 확정한 사건

- main `29c780a4`, [Windows job](https://github.com/cLazyZombie/aaa/actions/runs/40813730180/job/123249824520).
  `rust-tests-windows-2025-40813730180-1` artifact의 **전체 test.log**, job 원문, annotation을 대조했다.
- 실패한 호출은 `authorization.rs:289`의 **Ctrl+Z EOF 취소**다. 앞의 `no`, 공백 있는 `yes`,
  세션 내부 거부는 이 호출에 도달하기 전에 통과했다. 뒤의 Ctrl+C·자동화 거부는 실행되지 않았다.
- `elapsed=60.0038546s`, `answered=1/1`, `child_exit=None`, `reader_done=false`.
  계획 뒤 승인 질문까지 출력됐고 driver는 Ctrl+Z 쓰기·flush를 마쳤다. 실제 콘솔에 전달돼
  읽혔는지는 기존 로그로 알 수 없다. 자식이 살아 있으므로 「종료한 자식의 PTY EOF 수집 정체」로 판정할 수 없다.
- install 묶음은 29통과·1실패, 61.62초. 실행 환경은 Windows Server 2025 `10.0.26100`,
  runner image `windows-2025-vs2026` / `20261021.278.1`이다.
- 승인 우회·issuer 생성·부하·간헐성·ConPTY 결함 중 어느 것도 이 실패만으로 확정하지 않는다.

## 원본 코드 대조

허용된 Windows 실기에서 격리 폴더·전용 TEMP를 사용했다. 서비스·예약 작업·Run 키·사용자 PATH·방화벽의
시작 목록을 저장했다. 기존 설치·설정·실제 암호·사용자 데스크톱은 사용하지 않았다.

| 실행 | 결과 | 해석 |
| --- | --- | --- |
| Windows, 원본 대상 시험 단독 | 1통과, 2.09초 | 원본 CI 실패는 재현되지 않음 |
| Windows, 원본 install 전체 | 28통과·2실패, 12.39초 | 대상 취소 시험 통과. 전체 성공 아님 |
| macOS, 원본 authorization 전체 | 11통과, 9.56초 | Windows 회복 근거 아님 |
| 최소 실제 ConPTY + 동일 ReadConsoleW 설정 | 네 경우 모두 30~33ms 반환, `[26]` 또는 문자 뒤 `[26]` | 단순한 선입력/후입력 차이로 재현되지 않음 |

최소 실험은 빈 입력과 문자 뒤 Ctrl+Z 각각에 대해 바로 읽기와 콘솔 입력 사건을 기다린 뒤 읽기를 비교했다.
고정 sleep·반복 재시도는 없다. 단독 성공이나 다른 OS의 성공을 원본 CI의 회복으로 취급하지 않는다.

Windows 전체 묶음의 별도 실패는 다음과 같다. 이 조사에서 기대값을 바꾸거나 재실행해 덮지 않았다.

- `recovery::one_line_entry_enters_setup_and_cancels_without_installation_changes`:
  `recovery.rs:244`의 `Command::new("python")`가 `NotFound`로 실패했다.
- `authorization::ssh_environment_and_helper_hash_flags_do_not_bootstrap_a_new_trust_root`:
  기대 종료 코드 3, 실제 4. 이번 실기는 실제 SSH 자손에서 실행했다. `bootstrap::VerifiedSsh::current`가
  실제 SSH 조상을 허용한 뒤 0 해시를 conflict/4로 거부하는 코드와 부합한다. CI의 Ctrl+Z 정체와는 다른 실패다.

## 준비한 진단 변경

- install 입구도 기존 `AAA_LOG` 필터를 초기화한다. 기본값은 `off`다.
- `AAA_LOG=aaa::install::prompt=debug`에서 Windows 콘솔 모드 준비, `ReadConsoleW` 호출 직전,
  반환·경과 시간·성공 여부·OS 오류 번호를 기록한다. 질문·입력·문자 수·승인서·비밀은 기록하지 않는다.
- Win32 오류 번호는 로그 출력 전에 즉시 보관하고 기존 공개 오류의 안전한 원인 진단에도 연결한다.
- 실제 승인 PTY owner가 이 필터를 사용한다. 기존 Ctrl+Z owner 안에서 준비·호출·반환 표지가 수집되는지
  함께 확인한다. 새 시험을 늘리지 않고, 원래 종료 코드·파일 무변경 단언을 보존한다.
- 상한 진단은 다음 물음 대기, 자식 종료 대기, reader 완료 대기를 구별한다.
  살아 있는 자식을 기다리는 경우를 `<PTY EOF>`로 표시하던 오해를 없앤다.
- 입력 키·승인 조건·사건 상한은 바꾸지 않는다. 제품 입력 구조 변경은 원인 근거를 더 확보한 뒤 판단한다.

## 진단 후보 검증

- Windows 대상 시험: 1통과, 2.03초. 실제 Ctrl+Z의 세 콘솔 단계 표지도 확인했다.
- macOS 대상 시험: 1통과, 1.48초.
- macOS install 전체: 33통과·1 ignored, 11.55초. ignored는 기존 실제 서비스 등록 owner다.
- macOS와 Windows의 `cargo clippy --profile verify -p aaa --bin aaa --test install --locked -- -D warnings`: 통과.
- `cargo fmt --all --check`, `git diff --check`, `bash scripts/check-public.sh`: 통과.
- Windows 시험 종료 뒤 서비스·예약 작업·Run·사용자 PATH·방화벽이 시작 목록과 같고,
  격리 폴더의 실행 중인 실행 파일은 0개임을 확인했다. 격리 폴더를 지웠다.

## 다음 관측

현재 실기는 CI와 OS 빌드가 다르다. 진단 변경을 `hp/t-1303-win-auth-diagnostic`에 한 번 push해 기존 CI의
Windows job에서 원래 묶음을 실행하는 안을 제안한다. 새 workflow·반복 실행·시한 변경은 없다.
`read_call` 뒤 반환이 없는지, 반환 뒤 결과/프로세스 종료가 멈췄는지부터 구별한다.
승인 전에는 push·병합하지 않는다. 성공 한 번만으로 원인 해결로 닫지 않는다.

## 참고 구현

구현 전에 다음 세 후보와 라이선스를 확인했다. 외부 코드·의존성은 가져오지 않았다.

- [Microsoft Terminal cooked read](https://github.com/microsoft/terminal/blob/v1.22.10352.0/src/host/readDataCooked.cpp),
  [MIT](https://github.com/microsoft/terminal/blob/v1.22.10352.0/LICENSE): 제어 문자 wakeup 처리와
  [ReadConsoleW 공식 조건](https://learn.microsoft.com/en-us/windows/console/readconsole)을 대조했다.
- [Crossterm Windows mode](https://github.com/crossterm-rs/crossterm/blob/master/src/terminal/sys/windows.rs),
  [MIT](https://github.com/crossterm-rs/crossterm/blob/master/LICENSE): raw mode의 line/echo/processed 해제를 대조했다.
- [Rustyline Windows input](https://github.com/kkawakam/rustyline/blob/master/src/tty/windows.rs),
  [MIT](https://github.com/kkawakam/rustyline/blob/master/license): key-event 입력과 모드 복구를 대조했다.

현재 모드 설정은 질문 표시보다 먼저이고 최소 cooked-read 대조도 정상이라,
raw/key-event 입력으로 갈아타는 것을 근거 있는 수리로 선택하지 않았다.

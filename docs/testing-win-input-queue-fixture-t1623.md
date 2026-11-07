# Windows 입력 큐 fixture의 ConPTY 결과 판정 수리(t-1623)

## 원인

[CI 실행 42422520087](https://github.com/cLazyZombie/aaa/actions/runs/42422520087)의
Windows job `128553797276`, 기준 `a6f327771259ca493926f837053bb2ddecd80faf`에서
`input_queue_view_preserves_keys_mode_and_private_values_in_an_isolated_console`이 실패했다.
요약 annotation은 OSC 제목 뒤의 줄을 생략했다. job 원문에는 실제 한 시험의 실행·통과와 완료 요약이 모두 있다.
자식 종료 코드 0 검사도 통과했다. 입력 큐 검사 실패나 출력 수집 timeout이 아니다.

완료 문구의 원문 구간을 경로·제목 없이 옮기면 다음과 같다. `\x1b`는 ESC 바이트의 표기다.

```
test result: \x1b[?25h\x1b[?25l\x1b[?9001l\x1b[?1004lok. 1 passed; 0 failed
```

ConPTY의 cursor·입력 mode 제어열이 문장 사이에 들어가 원시 문자열의
`contains("test result: ok. 1 passed")`가 false였다. 화면에는 같은 통과 문구가 보인다.
자연 발생 시점은 Windows 원본 단독 1회와 고정 40회 표본에서 다시 나오지 않았다.
CI 원문의 제어열 구간을 재생하면 원래 판정의 실패가 결정적으로 재현된다.

## 수리와 보존한 계약

이미 의존하는 `aaa-vt`로 같은 36×120 터미널을 재생하고 화면의 libtest 통과 결과를 확인한다.
ConPTY·준비 질의·자식 종료·reader EOF 수집과 60초 실패 상한을 유지한다.
자식 종료 코드 0, 실제 한 시험의 통과 문구, 큐·mode·private 값 비노출·원문/순서 보존 검사를 유지한다.
앞 128개 뒤 Enter와 KEY_UP 판정도 바꾸지 않는다. 제품과 입력 sample 구현은 변경하지 않는다.

공통 회귀 `terminal_fixture_result_uses_the_screen_and_rejects_missing_failed_or_hidden_results`는
CI 제어열에서 원래 판정 false와 수리 판정 true를 검사한다. 빈 출력·0개 시험·실패·OSC 제목에만 있는 성공 문구·
지운 성공 문구를 거부한다. 이 회귀는 제어열 삽입 시점의 우연 없이 실제 실패를 잡기 위해 필요하다.
원시 판정으로 되돌리는 변형에서 같은 회귀가 실패했고, 원문 바이트 복원 뒤 관련 owner가 통과했다.

## 개발 검증

검증은 `aaa verify`를 통해 수행했다. Windows는 전용 checkout·TEMP·TMP와 기본 kache를 사용했다.
OS build 26200·Rust 1.99.1의 개발 verify 빌드이며, 원래 CI build 26100과 같은 이미지는 아니다.
웹 자산 없는 실행 파일로 관련 owner를 확인했다. 전체 착지 gate·production 빌드·전달·배포는 이 단계에 포함하지 않는다.

- macOS 새 회귀: 1통과, 0.00초.
- macOS 원시 판정 변형: 회귀가 exit 101로 실패. 소스를 바이트로 복원했다.
- macOS 복원 뒤 terminal owner: 4통과, 0.07초. 시작 질의·자식 종료와 끝 출력 보존·종료 구독을 포함한다.
- macOS CLI/install native strict Clippy: 통과.
- 실패 SHA + 수리의 Windows authorization: 14통과·1실패·1무시, 10.21초.
  새 회귀와 실제 격리 입력 큐·reader 스냅샷은 통과했다.
- 위 Windows CLI/install native strict Clippy: 통과.
- 최종 후보는 원본 `e6348666` 전체 소스에 수리 두 파일만 적용한 별도 Windows checkout으로 확인했다.
  결과 판정·실제 입력 큐·스냅샷·거부 owner 7통과·1무시, 2.50초. 이 후보의 native strict Clippy도 통과했다.
  두 수리 파일의 SHA-256은 로컬 소스와 일치했다.

Windows 실패는 별도 `ssh_environment_and_helper_hash_flags_do_not_bootstrap_a_new_trust_root`의
기대 3/실제 4다. OS 조상에 실제 신뢰 SSH가 있어 다음의 잘못된 해시 거부(exit 4)까지 진행한다.
독립된 detached 원본 `e6348666`도 같은 실제 SSH 조건에서 기대 3/실제 4로 실패했다(1건, 0.92초).
전체 authorization 통과로 기록하지 않는다. 해당 시험의 기대값·제품은 바꾸지 않는다.

서비스·예약 작업·Run 키·사용자 PATH·방화벽 기준은 전후 모두 같았다.
전용 경로의 생존 프로세스와 reparse point는 0개였고 Windows 실험실을 삭제했다.

## 조사 범위와 관계

원문 확보 전 출력 수명 경합을 조사했으나 끝 출력 유실은 재현되지 않았다.
첫 출력을 전부 보류하는 탐색은 ConPTY 시작 질의도 막았다. 직접 소유 자식·runner만 확인한 PID로 종료했다.
남은 runner 때문에 링크가 잠긴 실패를 확인하고 정리했다. 초기 질의 뒤 출력만 보류한 원본은
실제 큐 계약을 마치고 정상 종료했다. 이 탐색은 원래 CI 실패의 재현으로 세지 않는다.
창 없는 콘솔과 결과 pipe를 분리하는 대안과 모든 임시 계측은 최종 수리에 포함하지 않는다.

d-0540의 ReadConsoleW 반환 정체와 같은 원인이라는 근거는 없다. 이번 서명은 이미 통과한 결과의
문자열 판정 결함이다. 그 할 일은 변경하지 않는다.

[WezTerm portable-pty](https://github.com/wezterm/wezterm/blob/main/pty/src/win/conpty.rs)
([MIT](https://github.com/wezterm/wezterm/blob/main/LICENSE.md)),
[Microsoft Terminal](https://github.com/microsoft/terminal/blob/main/src/cascadia/TerminalConnection/ConptyConnection.cpp)
([MIT](https://github.com/microsoft/terminal/blob/main/LICENSE)),
[Alacritty](https://github.com/alacritty/alacritty/blob/master/alacritty_terminal/src/tty/windows/conpty.rs)
([Apache-2.0](https://github.com/alacritty/alacritty/blob/master/LICENSE-APACHE))의 콘솔·자식·출력 수명과
[ClosePseudoConsole 계약](https://learn.microsoft.com/en-us/windows/console/closepseudoconsole)을 비교했다.
최종 수리는 기존 VT 구현을 사용한다. 외부 코드·의존성을 복사하지 않았다.

# Windows install PTY 사건 상한 조사(t-1246)

## 판단

근본 원인은 미확정이다. 실패 SHA의 Windows 단일 시험은 통과했다. 이 변경은 다음 실패의
진단을 보강하며, 원래 60초 실패를 수리했거나 CI가 회복됐다는 뜻은 아니다.

## 원래 실패에서 확인한 것

- [실패 job](https://github.com/cLazyZombie/aaa/actions/runs/40636511491/job/122644607789),
  SHA `4511c91b3a269d57d4229070137accfd2a4e704f`의 전체 job 로그와 Windows artifact를 확보했다.
- artifact `rust-tests-windows-2025-40636511491-1`에는 `test.log` 한 파일만 있다.
  원래 host 상태·PTY 원바이트·입력 전송 기록·승인 파일·worker 진행 기록은 업로드되지 않았다.
- `recovery::closed_enrollment_window_keeps_the_id_and_action_after_files_are_applied`의
  최초 `authorize`는 끝났다. `recovery.rs:65`의 승인 파일 적용 CLI가 멈췄다.
  `answered=0/0`, `needle="<PTY EOF>"`, `child_exit=None`, `reader_done=false`,
  `elapsed=60.0110761s`다. 이 호출에는 응답할 승인 질문이 없다.
- 수집한 출력은 ConPTY 모드·제목 OSC·커서 표시뿐이다. helper는 종료 또는 출력 완료를
  기다렸고 자식은 안전 상한에 도달할 때까지 생존했다. 실제로 CLI가 어느 내부 단계에
  있었는지와 원시 DSR/DA 질의·응답은 이 출력으로 알 수 없다.
- install target은 29 통과·1 실패/90.48초다. CI 단계 전체 timeout이 아니다.
  함께 실행한 다른 시험의 60초 경고만으로 CPU·디스크 부하나 같은 결함을 단정하지 않는다.

## 대조와 변경

실패 SHA와 현재 기준 `06c77f26` 사이의 install 제품 경로·해당 helper에는 변경이 없다.
실패 SHA를 격리한 Windows 단일 시험은 6.14초에 통과했다.
Windows 실기 OS·임시 경로·동시 실행 수는 CI와 다르다. 실기에서는 웹 front를 빌드하지 않은
verify 실행 파일을 사용했다. CI 실행 파일과 바이트가 같은 재현은 아니다. 같은 실패를 재현하지 못했으므로
제품·PTY·시험 동기화·runner 환경 중 하나를 원인으로 정하지 않는다.

수정은 해당 recovery 시험에 한정한다.

- 준비·승인 발급·승인 파일 적용·status 관찰·재개·정리의 단계와 경과 시간을 남긴다.
- 적용·재개에 각각 다른 `--events-file`을 지정한다. 제품이 이미 제공하는 공개 사건이다.
  발급 입력이나 서명된 계획의 선택을 바꾸지 않는다.
- 실패 unwind 때 사건의 `seq`·typed `phase`·typed `state`, 최종 결과의 종료 코드만 출력한다.
  파일이 없으면 파일 열기 오류를 남겨 Started 이전과 이후를 구분한다. 파일이 없다는 사실만으로
  worker가 생성되지 않았다고 단정하지 않는다.
- 사건의 본문·자유 문자열·승인서·신원·비밀값은 덤프하지 않는다. 깨진 사건도 파싱 오류의
  분류·행·열만 출력한다. 기존 CI가 이 로그를 `test.log`로 보존하므로 workflow 변경은 없다.
- 기존 60초 안전 상한과 종료·EOF 대기, ID·새 nonce·등록 창·단일 파일 job·읽기 전용 status
  단언을 유지한다. 재시도·sleep·시한 연장·기대값 완화는 없다. 새 시험은 추가하지 않았다.

## 확인

모든 빌드·수정은 Mac에서 관리했다. Windows에는 격리 폴더로 시험 소스만 전달해 빌드·실행했다.
기존 시스템 설치·서비스·계정·SSH 설정은 변경하지 않았다.

| 확인 | 결과 |
| --- | --- |
| 현재 기준 Mac 단일 시험 | 통과, 5.29초 |
| 실패 SHA Windows 단일 시험 | 통과, 6.14초 |
| 진단 후보 Mac 단일 시험 | 통과, 5.51초 |
| 의도한 실패 주입, Mac | exit 101, 적용 단계와 공개 사건·종료 코드 보존 |
| 주입 제거 뒤 Mac 단일 시험 | 통과, 5.35초 |
| 진단 후보 Windows 단일 시험 | 통과, 6.28초 |
| `cargo clippy --profile verify -p aaa --test install --locked -- -D warnings` | 통과 |
| `cargo fmt --all --check`·`git diff --check`·공개 검사 | 통과 |

단일 시험 명령은 아래와 같다. Windows에서는 이 시험 하나만 실행했다.

```sh
cargo test --profile verify -p aaa --test install --locked \
  recovery::closed_enrollment_window_keeps_the_id_and_action_after_files_are_applied \
  -- --exact --nocapture
```

실패 주입은 적용 CLI가 끝난 직후 수행했다. 공개 사건 파일에 canary를 알 수 없는 `type`으로
추가하고 강제로 panic했다. 원래 panic과 `failed phase=apply`, Authorize 완료·Stage 시작·
Configure 완료·Enroll 시작·result exit=3, 손상 사건의 파싱 분류가 남았고 canary 원문은 없었다.
주입 코드와 실패 실험실은 제거했다. 이 검증은 진단 경로 검증이며 60초 실패의 재현이 아니다.

Windows 후보의 단계 시간은 준비 0.097초, 발급 1.621초, 적용 3.402초, 적용 관찰 0.270초,
재개 0.756초, 재개 관찰 0.085초, 정리 0.044초다. 단독 표본이 5초를 넘으므로 개별 시험의
5초 목표도 아직 충족하지 못했다. 이 시간을 원래 60초 멈춤의 원인 증거로 쓰지 않는다.
전체 install target·전체 ci-local·원래 CI의 병렬 부하 재현은 실행하지 않았다.

Windows의 서비스·예약 작업·Run 키·사용자 PATH·방화벽 목록을 전후 대조하고,
시험 소유 프로세스가 없음을 확인한 뒤 격리 폴더를 제거했다.

## 참고한 구현

새 PTY 구현은 가져오지 않았다. 아래 세 구현의 수명·질의 경계를 대조했다.

- [portable-pty 0.9.0 / WezTerm](https://docs.rs/crate/portable-pty/0.9.0/source/src/win/psuedocon.rs)
  ([MIT](https://github.com/wezterm/wezterm/blob/main/LICENSE.md)):
  커서 상속 플래그와 기존 `TerminalQueries`의 질의 응답 경계를 확인했다.
- [Alacritty ConPTY](https://github.com/alacritty/alacritty/blob/master/alacritty_terminal/src/tty/windows/conpty.rs)
  ([Apache-2.0](https://github.com/alacritty/alacritty/blob/master/LICENSE-APACHE)):
  소유자 닫힘 동안 출력 pipe를 비워야 한다는 수명 규칙을 대조했다.
- [Microsoft Terminal VtIo](https://github.com/microsoft/terminal/blob/main/src/host/VtIo.cpp)
  ([MIT](https://github.com/microsoft/terminal/blob/main/LICENSE)):
  초기 DSR/DA 질의와 모드 출력을 대조했다. 모드·제목만 남았다고 커서 질의 교착으로 분류하지 않는다.

## 남은 일

진단 후보에 착지 창이 열린 뒤 Windows CI가 실패하면 `closed-window phase`와 공개 사건 순서를
기존 PTY 진단에 함께 대조한다. Started 이전이면 계획·승인 검증·worker 시작을,
Started 이후이면 마지막 파일·등록·private callback 경계를 좁힌다. 원시 PTY 기록과 runner
부하를 추가 확보해야 할 수도 있다. 그 증거에서 원인을 확인한 뒤 최소 수리와 Windows 검증을 한다.

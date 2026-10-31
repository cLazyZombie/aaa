# main CI 세 OS 간헐 실패 조사와 두 경합 수리(t-1365)

## 범위와 현재 판단

기준은 `e71d9ac3`이다. 검증 시간 축소(t-1310, `5304098d`)는 기준에 포함돼 있고 세 시험은 모두 남아 있다.
최근 main CI의 완료된 여덟 run은 성공했다. `2e0bcf04`의 첫 실행은 Linux에서 같은 실패가 있었고,
그 run의 재실행은 성공했다. 조사 시 진행 중인 두 run은 성공 근거에 넣지 않았다.

- [원래 세 OS 실패](https://github.com/cLazyZombie/aaa/actions/runs/40851405288), `dd2c5ebc`.
- [축소 이후 Linux 재발 및 재실행](https://github.com/cLazyZombie/aaa/actions/runs/41035602216), `2e0bcf04`.
- [최근 완료된 성공](https://github.com/cLazyZombie/aaa/actions/runs/41158555758), `ce7f29df`.

Linux의 수신 기록 대기와 macOS의 빈 예약 조회 잠금은 현재 코드에서 원인을 재현해 수리했다.
Windows의 승인 PTY 정체는 원본 전체 artifact와 실기를 대조했지만 **원인은 미확정**이다.
Windows 제품 입력 방식이나 승인 계약을 바꾸지 않았다. 이 변경을 세 실패 전체의 수리 완료로 해석하지 않는다.

## Linux: host 판정과 PTY 자식의 기록 완료

`unknown_brief_is_not_resent`는 수동 `thread brief`가 unknown으로 반환한 즉시 `mute.log`를 읽었다.
이 반환은 host가 받음을 확인하지 못했다는 판정이지, PTY 자식인 `cat`의 read/write가 끝났다는 신호가 아니다.
원본 두 CI 모두 수동 전송 뒤 `brief.md`의 횟수가 기대 2, 실제 1이었다. host의 PTY 쓰기 완료 로그도 남아 있다.

수리는 첫 전송·수동 전송 각각의 줄 끝이 기록될 때까지 기존 30초 상한 안에서 기다리는 것이다.
그 뒤 기존의 정확한 1회·2회 단언, unknown 상태·그룹·흐름 사건, brief 전 메시지 거부와 비전송 단언을 실행한다.
수신이 없으면 안전 상한에서 실패하며 기대 줄 수와 현재 파일 읽기 결과를 진단한다.
상한·기대값·자동 재전송 정책은 바꾸지 않았다. 새 시험은 추가하지 않았다.

결정적 재현은 이 owner의 mute fixture만 임시로 바꿨다. 첫 줄을 기록한 뒤 FIFO에서 멈춰 두 번째 줄을
읽지 못하게 해도 수동 CLI가 unknown으로 반환한다. 원래의 즉시 단언은 CI와 같은 `1 != 2`로 실패했다(2.53초).
FIFO를 놓고 수신 완료를 기다린 수리판은 통과했다(2.68초). 고정 sleep이나 제품 시한 변경은 없다.
임시 fixture·FIFO 주입은 제거하고 원래 `cat` fixture로 되돌린 뒤 아래 일반 owner를 확인했다.
이 재현은 macOS에서 실제 PTY로 수행했으며 Linux 네이티브 실행 결과라고 쓰지 않는다.

## macOS: 빈 예약 pass와 계정 이름 변경

원 CI의 첫 `account rename before after`는 「예약 처리 중이다. 끝난 뒤 이름을 바꾼다」로 거부됐다.
기존 t-1152 수리는 빈 자동화 조회의 dispatch 잠금을 다뤘고, 이번 예약 `queue_gate`는 별개다.
`queue_pass`는 예약이 전혀 없어도 잠금을 잡고 할 일 파일·보관 기록을 읽은 뒤 빈 목록을 확인했다.
이 동안 계정 변경의 `try_lock`이 실패할 수 있었다.

프로젝트를 확인한 뒤 예약이 비었고 미완료 보관 영수증도 없으면 잠금을 요구하지 않고 반환한다.
새 예약은 다음 pass에서 보며 실제 처리가 있으면 잠금을 잡은 뒤 자료를 다시 읽는다.
예약 없이도 파일 제거 뒤 DB 완료 전에 중단된 보관 기록은 기존 잠금 아래에서 복구한다.
실제 작업 중 계정 이름 변경을 거부하는 계약은 유지한다.

- 새 `idle_queue_pass_does_not_join_the_gate_or_block_the_first_account_rename`은 예약 잠금을 직접 보유하고
  빈 pass future의 첫 poll이 바로 완료돼야 하는지 본다. 수정 전에는 Pending이라 실패했고,
  수정 후에는 완료한다. 같은 owner에서 잠금 점유 중 rename 거부·인증 불변, 해제 뒤 rename 성공도 확인한다.
  필요한 이유는 스케줄 우연에 기대던 빈 조회의 잠금 요구를 결정적으로 재현하기 위해서다.
- 기존 `old_archived_reservation_is_repaired_but_unknown_missing_task_keeps_its_warning`에 실제 예약이
  잠금을 기다리는지 더했다. 기존 복구·경고·종료 상태 단언을 유지했다.
- 기존 `archive_recovers_the_file_removed_before_final_db_receipt`에 예약 없는 미완료 영수증도 잠금을
  기다린 뒤 queue pass로 복구되는지 더했다. 원래 명시 조회의 복구도 같은 미완료 영수증으로 다시 시작해 확인한다.

## Windows: 발급 승인 단계의 ReadConsoleW 정체

[실패 job](https://github.com/cLazyZombie/aaa/actions/runs/40851405288/job/123371509945)의 annotation과
`rust-tests-windows-2025-40851405288-1`의 전체 `test.log`를 확보했다. artifact에는 `test.log`만 있었다.

정체는 TLS/SAS 대기가 아니라 그보다 앞의 `authorize` 승인 발급 호출이다. 승인 질문에 `yes`와 CR을 쓰고
flush한 뒤 60초 상한에 도달했다. `answered=1/1`, `waiting=child_exit`, `child_exit=None`, `reader_done=false`다.
`install_console_input_prepared`(503 → 502)와 `install_console_read_call`은 있지만
`install_console_read_returned`는 없다. 실제 콘솔 입력 도착·소비, 원시 PTY 바이트, 자식의 스택은 남아 있지 않다.

[t-1303 Ctrl+Z 정체](testing-win-auth-t-1303.md)도 같은 콘솔 API의 미반환이었지만 입력이 다르다.
같은 원인이라고 확정할 근거는 없다. 자식이 살아 있으므로 종료 후 PTY EOF 수집 정체로도 분류하지 않는다.

Windows 실기의 격리 폴더·전용 TEMP에서 기준 소스를 빌드해 원래 private enrollment 시험을 실행했다.
실제 TLS·SAS·응답 유실과 재개·공개 사건 비노출을 포함한 기존 시험은 1개 통과했다(8.03초).
이 실행은 CI와 OS 빌드·동시 실행 조건이 다르며 원본 정체의 회복이나 재현으로 해석하지 않는다.

별도 임시 도구로 같은 portable-pty·질의 응답기·36×120 콘솔과 같은 ReadConsoleW 설정을 사용해 다음을 대조했다.
입력 값을 기록하지 않고 각 경우의 성공·종료·경과만 확인했다.

| 축 | 경우 |
| --- | --- |
| 입력 순서 | 실제 콘솔 입력 handle이 준비된 뒤 읽기, 읽기 호출 직전 신호 뒤 보내기 |
| 화면 위치 | 일반 위치, 오른쪽 끝 근처에서 질문을 출력해 줄바꿈 |
| 입력 종류 | yes와 CR, Ctrl+Z |

8가지 모두 26~52ms에 반환·종료했다. 고정 sleep 없이 별도 연결 신호와 콘솔 handle 사건을 썼다.
입력 순서·질문 줄바꿈만으로 정체를 재현하지 못했다. 임시 도구는 제품이나 시험 목록에 추가하지 않았다.

원인 미확정 상태에서 raw/key-event 입력으로 갈아타거나 승인 시한을 늘리지 않는다.
다음 자연 발생 실패에서는 기존 read 준비/호출/반환 진단에 더해 입력의 실제 콘솔 도착이나
정체 스택을 확보해야 한다. 현재 보존 자료만으로 이를 소급 복원할 수 없다.

## 검증

- macOS `cargo test --profile verify --locked -p aaa-host --lib -- account_rename::tests:: task_queue::tests::`:
  최종 관련 46개 통과(2.19초). 새 회귀의 수정 전 실패는 별도로 보존했다.
- macOS `cargo test --profile verify --locked -p aaa --test m13 -- unknown_brief_is_not_resent
  renamed_account_starts_with_the_same_auth_without_login_and_rejects_while_running
  brief_left_in_box_is_submitted_not_retyped`: 3개 통과(12.60초).
- `cargo clippy --profile verify --locked -p aaa-host --lib -p aaa --test m13 -- -D warnings`: 통과.
- `cargo fmt --all --check`, `git diff --check`, 고정 sleep ratchet, `scripts/check-public.sh`: 통과.
- Windows 기준 private enrollment 및 별도 입력 대조는 위 결과와 같다. 후보 공통 예약/계정 owner 46개가 통과했다(1.94초).
  마지막에 추가한 인증 불변·명시 조회 복구 단언을 포함한 최종 세 owner도 통과했다(0.28초).
- 개발의 관련 owner 검사다. 착지 창에서 수행할 최종 통합 SHA 전체 gate·cross·production 빌드나 전달은 하지 않았다.

Windows 준비 중 PowerShell 5의 native stderr를 terminating error로 취급해 첫 빌드 wrapper가 중단됐다.
정상 컴파일 출력을 파일로 받게 했고, Start-Process의 프로세스 트리 대기는 장기 캐시 자식을 기다려 멈췄다.
직접 확인한 자기 wrapper PID만 종료하고 직접 실행한 Cargo의 완료만 기다리게 했다. 캐시나 다른 작업의 프로세스는 종료하지 않았다.
두 준비 오류를 시험 실패나 통과로 세지 않았다. 테스트 실행 실패를 재시도로 덮지 않았다.
시험 전후 서비스·예약 작업·Run 키·사용자 PATH·방화벽 목록은 같았고, 격리 폴더의 실행 중인 프로그램은 0개였다.

Windows 개별 시험의 8.03초는 기존 5초 목표를 넘는다. 이번 조사에서 시나리오를 축소하거나 성능 목표를 달성했다고 쓰지 않는다.

## 참고한 공개 구현

- [Tokio Mutex/Notify](https://docs.rs/tokio/latest/tokio/sync/struct.Notify.html)
  ([MIT](https://github.com/tokio-rs/tokio/blob/master/LICENSE)):
  호출/판정 완료와 소비자의 처리 완료를 별도 사건으로 보는 경계를 참고했다.
- [robfig/cron chain](https://github.com/robfig/cron/blob/master/chain.go)
  ([MIT](https://github.com/robfig/cron/blob/master/LICENSE)):
  실제 작업의 잠금 범위를 비교했다. 예약 처리 중 충돌 거부는 유지했다.
- [Microsoft Terminal cooked read](https://github.com/microsoft/terminal/blob/main/src/host/readDataCooked.cpp)
  ([MIT](https://github.com/microsoft/terminal/blob/main/LICENSE))와
  [Alacritty ConPTY](https://github.com/alacritty/alacritty/blob/master/alacritty_terminal/src/tty/windows/conpty.rs)
  ([Apache-2.0](https://github.com/alacritty/alacritty/blob/master/LICENSE-APACHE)):
  콘솔 읽기와 종료 후 pipe 수명을 대조했다. 폭 1칸의 wide 문자 결함과 현재 조건을 같다고 보지 않았다.

외부 코드·런타임 의존성은 추가하지 않았다.

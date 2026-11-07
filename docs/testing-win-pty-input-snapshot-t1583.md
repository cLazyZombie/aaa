# Windows PTY 실패 뒤 입력 큐 스냅샷(t-1583)

## 바꾼 이유와 범위

[자연 실패 조사](testing-win-pty-enter-t1583.md)에서 Enter KEY_DOWN 관측 뒤 `ReadConsoleW`가 반환하지 않는
plain wizard 두 사례를 확인했다. 기존 진단에는 실패 뒤 Enter가 큐에 남았는지 확인할 자료가 없었다.
q-2899의 검수 1A에 따라 기존 실패 수집기에 읽기 전용 입력 큐 진단을 구현했다.
**근본 원인 수리나 원래 CI 회복을 완료한 변경은 아니다.**

변경 owner는 CLI의 `install` 통합 시험이다. `authorization.rs`, `terminal_snapshot.rs`, `snapshot_native.rs`와
새 `snapshot_console.rs`만 바꿨다. 제품의 prompt·wizard·worker 입력 구현은 바꾸지 않았다.

## 수집하는 것

기존 60초 PTY 실패를 확정한 뒤의 helper에서 입력을 조회한다. 기존 helper의 5초 상한은 유지한다.
기존 성공 owner도 명시적으로 같은 helper를 호출해 실제 API를 검증한다.

- 검증된 CLI의 부모·전체 이미지 경로와 동일 process handle을 유지한다.
- CLI에 연결된 호스트의 부모·시스템 conhost 전체 경로를 검증하고 host handle도 보관한다.
- 별도 helper만 자기 콘솔을 분리한 뒤 CLI 콘솔에 붙는다. 연결 뒤 helper의 커널 콘솔 연관 정보도 같은 host인지 대조한다.
- client 목록에 대상 CLI와 helper만 있어야 한다. 다른 client가 있는 공유 콘솔은 `console_input_shared`로 거부하고 입력을 조회하지 않는다.
- 새 CONIN$ handle을 GENERIC_READ로 열어 mode와 앞의 최대 128개 사건을 Peek한다.
- JSON에는 `input_mode`, `records_present`, `prefix_key_down`, `prefix_enter_down`, `prefix_cancel_down`, `prefix_eof_down`만 넣는다.
  입력 문자·길이·질문·승인서·명령줄·메모리 원문·다른 client PID는 출력하지 않는다.
- 실패는 기존 스택 수집을 막지 않고 `console_input_error`와 `console_input_code`로 단계·OS 코드를 남긴다.
  helper가 API 안에서 멈추면 기존 상한이 종료하고 journal의 `console_input` 단계가 남는다.
- 입력 sample은 심볼 조회·thread stack 수집보다 먼저 실행한다. 기존 journal의 `console_input`과 다음 단계 시각으로 조회 구간을 구분한다.

수집기는 입력을 소비·주입·flush하거나 mode를 바꾸지 않는다. 호스트 thread를 중지하거나 live conhost 스택을 걷지 않는다.
새 필드는 optional이므로 기존 스냅샷 형식 1의 필드는 유지한다. 오류 단계 문자열도 기존 검증처럼 정적인 단어만 허용한다.

## 판정의 한계

`prefix_*`는 조회한 앞부분에 해당 KEY_DOWN이 있었는지를 뜻한다. false로 전체 큐에 키가 없다고 단정하지 않는다.
KEY_UP은 종료 키 관측으로 세지 않는다. `records_present=false`도 조회 시점의 빈 sample이지 60초 전 입력 이력의 부재가 아니다.

조회는 실패 확정 뒤 별도 helper에서 한다. 실패 순간을 원자적으로 동결한 자료가 아니며,
helper 연결과 진단 사이에 CLI가 진행할 수 있다. Enter가 보이면 남은 입력과 읽기 깨우기 경계를,
보이지 않으면 조회 범위·소비·cooked read 완료 경계를 추가로 구분해야 한다.
새 필드만으로 특정 콘솔 내부 결함을 확정할 수는 없다.

## 필요한 회귀 시험

새 owner `input_queue_view_preserves_keys_mode_and_private_values_in_an_isolated_console`은 자기 자식을 실제 ConPTY에 띄운다.
사용자 데스크톱이나 공유 console을 사용하지 않는다. 빈 큐·private canary와 Enter/Ctrl+C/Z·128개 사건 뒤 Enter·KEY_UP만 있는 큐를 검사한다.

Peek 뒤 OS mode와 큐의 원문·순서가 그대로인지 확인하고, 입력이 모두 남았음을 API로 확인한 뒤에만 실제 읽기를 한다.
canary가 직렬화 결과에 없는지도 확인한다. 새 시험이 필요한 이유는 수집기가 입력을 소비하거나,
조회 범위 밖의 키와 KEY_UP을 잘못 판정하면 다음 자연 실패의 분류 자체가 틀리기 때문이다.
고정 sleep·반복 재시도·시한/기대값 완화는 없다.

기존 두 실제 reader 스냅샷 owner에는 입력 view의 성공을 추가했다. 기존 실제 frame·host view·경로/비밀 비노출·
수집 뒤 정상 승인 완료 단언을 유지했다. 잘못된 부모·이미지는 기존처럼 콘솔 연결 전에 거부한다.

## 개발 검증

검증은 `aaa verify`를 통해 실행했고 코드 후보는 `1ccd52cbed1e967b6d436bf9d217641479f15d24`다.
Windows는 전용 checkout·TEMP·TMP를 쓰고 기본 kache를 유지했다. 직접 만든 공유 서비스·시스템 설정 변경은 없다.

| 범위 | 결과 |
| --- | --- |
| Windows 새 실제 입력 큐 owner | 1통과, 0.03초 |
| Windows 실제 reader 스냅샷·경로·부모/이미지 거부 | 3통과, 묶음 5.21초 |
| Windows helper 오류 정제 | 1통과, 0.03초 |
| Windows wizard 전체 | 4통과, 1.74초 |
| Windows Ctrl+Z/C·자동화·세션 거부·무변경 | 1통과, 1.31초 |
| Windows detached worker 수명·직계 자식 스택 | 1통과, 6.75초 |
| macOS authorization owner 전체 | 11통과, 41.65초 |
| Windows·macOS CLI/install native strict Clippy | 모두 통과 |
| fmt check | 통과 |

Windows helper의 실제 조회·스택 수집은 881ms·901ms, worker까지 포함한 경우 1165ms로 기존 5초 안에 끝났다.
새 `console_input` 조회는 journal의 19~39ms 또는 20~40ms 구간에서 끝났다.
Windows 11개와 macOS 11개는 부분 owner 검증이며 install 전수·전체 gate 통과로 쓰지 않는다.
Windows worker owner는 기존 5초 목표를 넘었다. 이전 원본의 6.06초 기록과 구분해 남기며, 이 변경으로 성능 목표를 달성했다고 하지 않는다.

실기의 OS는 `10.0.26200`으로 원래 CI의 `10.0.26100`과 다르며 웹 번들 없는 개발 verify 실행 파일이다.
원래 정체는 이번에도 재현되지 않았다. 원본 회복이나 같은 이미지 재현으로 판정하지 않는다.
Windows checkout의 네 Rust 파일은 CRLF였고 로컬 LF를 CRLF로 바꾼 SHA-256이 모두 일치했다. checkout은 변경 없이 검증했다.

Windows·macOS native strict Clippy와 fmt check는 통과했고 검증 원장을 남겼다.
전후 서비스·예약 작업·Run 키·사용자 PATH·방화벽 기준은 모두 같았다.
전용 폴더를 쓰는 생존 프로세스와 reparse point는 각각 0개였으며, 기준 목록 회수 뒤 재확인하고 실험실을 삭제했다.
공개 검사 결과는 보고의 최종 기록에 남긴다.

착지 창의 최종 통합 SHA gate·OS cross·production 빌드·전달·병합·push는 실행하지 않았다.
진단을 main에 반영하려면 brief에 정한 별도 착지 창이 필요하다. 그 뒤의 자연 실패에서 새 필드를 확인해야 근본 원인 조사가 이어진다.

## 참고한 공개 구현과 API

앞선 조사에서 비교한 [Microsoft Terminal](https://github.com/microsoft/terminal/blob/main/src/host/inputBuffer.cpp),
[Crossterm](https://github.com/crossterm-rs/crossterm/blob/master/src/event/sys/windows/poll.rs),
[Rustyline](https://github.com/kkawakam/rustyline/blob/master/src/tty/windows.rs)의 MIT 구현을 참고했다. 외부 코드를 복사하지 않았다.

[AttachConsole](https://learn.microsoft.com/en-us/windows/console/attachconsole)의 프로세스별 콘솔 연결,
[GetConsoleProcessList](https://learn.microsoft.com/en-us/windows/console/getconsoleprocesslist)의 목록/버퍼 반환 계약,
[PeekConsoleInput](https://learn.microsoft.com/en-us/windows/console/peekconsoleinput)의 비소비 조회 계약을 확인했다.
API는 진단 helper에서만 쓰며 지원·권한·연결 실패를 제품 동작 조건으로 만들지 않는다.

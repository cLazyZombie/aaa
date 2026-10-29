# 시험 쓰는 법

시험을 쓰거나 고칠 때 따르는 규칙이다. 기능별 검증 기록은 [testing.md](testing.md)와 그 문서가 가리키는 별도 기록에 있다.
개별 시험의 시간 기준은 [testing.md 「개별 시험의 5초 기준」](testing.md#개별-시험의-5초-기준t-1117)을 따른다.
개발의 관련 검사와 착지 gate의 범위·명령은 [verification.md](verification.md)를 따른다. 전체 전수·OS cross·Windows production을 개발 후보마다 반복하지 않는다.

## 원칙

- **사건으로 끝낸다.** 시험은 정한 시간을 자지 않고, 기다리는 것이 왔다는 신호(흐름 사건·출력·화면 표지·fake 기록·tick 끝남)를 기다린다. 「시간 제한이 있어야 통과하는 시험은 틀렸다」(t3code `AGENTS.md`). 시한은 오지 않을 때 멈추는 안전 상한일 뿐이다.
- **시간 규칙은 시계를 흉내 내지 않고 `now` 인자와 설정 값으로 본다.** 판정 함수는 `now`를 받는다(herdr `*_at(now)`, herdr-projects `Outage::record(now)`, aaa `inbox::Outage::record`). 그 함수를 유닛 시험에서 여러 `now`로 부른다. E2E는 설정 값으로 시간 창을 줄이고(아래 표), 창이 지난 뒤 `ticker tick`으로 host가 판정했음을 확인한다.
- **실패를 다시 돌려 넘기지 않는다.** 재시도·시한 늘리기·기대값 완화·건너뛰기로 통과시키지 않는다. 실패의 증거(실험실 폴더·화면·로그)를 남기고 원인을 찾는다.
- **실험실은 남의 것을 건드리지 않는다.** 빈 port, 짧은 임시 경로, 자기가 띄운 PID만 정리한다. 운영 host·사용자 셸 설정·실제 clipboard·소리에 닿지 않는다.
- **웹 E2E 실행 파일은 병렬 단계 전에 고정한다.** `ci-local.sh`는 이번 빌드의 실행 파일을 임시 사본에 복사·바이트 대조한 뒤 `AAA_BIN`으로 넘긴다. Cargo의 fresh `test --no-run`도 공용 실행 파일을 지웠다 다시 연결하므로, 그 경로로 병렬 실행하면 짧은 ENOENT가 난다. 사본은 전체 검증 종료 때 지운다. 준비 실패는 E2E 실패이며 재시도하지 않는다.
- **실행할 파일은 Unix에서 자식이 복사한다.** 병렬 시험의 부모가 실행 파일을 쓰면 다른 fork가 쓰기 fd를 물려받아 복사 뒤 exec도 `ETXTBSY`로 실패할 수 있다([Rust #114554](https://github.com/rust-lang/rust/issues/114554)). 별도 복사 프로세스가 끝난 뒤 실행한다. 재시도·sleep·시험 전체 직렬화로 덮지 않는다. Codex 설치·hook 시험의 `Home::moved_bin`이 이 경계를 쓴다.
- **웹 실험실은 writer 자손까지 종료한 뒤 지운다.** host·agent 종료 뒤 `meta.json`과 명령줄로 확인한 session host를
  먼저 멈추고, 자손도 부모부터 멈춰 추가 fork를 막는다. 확인한 PID를 종료한 뒤 모두 종료 상태가 된 것을 보고
  `rmSync`를 한 번 부른다. `node --test web/test/session-cleanup.test.mjs`는 별도 process group에서 계속
  mkdir/write하는 실제 자손과 무관한 프로세스를 함께 띄워, writer 종료·무관한 PID 보존·재시도 없는 삭제를 본다.
  session host만 죽이는 원래 정리는 writer 생존 단언에서 실패한다. 웹 E2E 13의 재부팅 흉내도 같은 완료를 기다린다.

## 어디에 두나

Windows M4p(t-0613, plan §8.394)는 `cargo test -p aaa-common --lib windows_acl_tests::`와
`cargo test -p aaa-common --lib ipc::tests::`로 실행한다. 실제 state/TLS 폴더·키·원자 교체 파일과 첫 Pipe·accept 뒤
두 인스턴스의 OS DACL을 읽고, Authz의 그룹 조회 없는 SID context로 현재 사용자 허용과 Everyone·Authenticated Users·
Users·Administrators·별도 SID 거부를 판정한다. 파일의 SYSTEM 예외와 Pipe의 SYSTEM 거부도 구분한다.
공개 ACL fixture는 실제로 Everyone 접근이 되는지 먼저 판정한 뒤 기존 키 재읽기·신원 보존·권한 이행을 본다.
상위 폴더 ACL 보존·hard link·잘못된 종류 거부·같은 사용자 바이트 전송·peer PID·중복 bind 거부를 함께 검사한다.
계정·시스템 정책·실제 사용자 설정을 바꾸지 않고, 고정 sleep 없이 연결·읽기 완료로 끝낸다.
웹 `node --test web/test/machineDetail.test.mjs`는 연결/끊김 Windows에만 제한 안내가 나오는지 검사한다.
이 시험들은 기존 workspace test에 포함되어 GitHub CI windows-2025에서 실행된다. macOS의
`cargo xwin clippy --workspace --all-targets --target x86_64-pc-windows-msvc --locked --keep-going -- -D warnings`와
같은 대상의 `cargo xwin build --workspace --all-targets --target x86_64-pc-windows-msvc --locked`는 실행 결과가 아니다.

M4p의 전체 검증에서 발견한 E2E 49의 기억 본문 검사는 focus 이후 새 routines 응답의 DOM 반영을 기다린다.
기준 판의 실제 새 응답을 gate로 보류하면 host에는 새 기억이 있어도 기존 상세는 비어 있고, 즉시 검사하던 시험이
「기억 본문 없음」으로 실패한다. E2E 46은 다른 기기가 저장하기 전에 둘째 폼의 제목·저장 가능 상태로 편집 반영을 확인한다.
원래 본문·갱신 thread·충돌·파일 보존 단언과 시한을 유지하며 sleep이나 재실행으로 통과시키지 않는다.

Windows named pipe BUSY 회귀는 `cargo test -p aaa-common --lib ipc::tests::`로 실행한다. 실제 `LocalListener`를
한 연결로 점유하고 두 번째 연결의 OS 오류 231, 계속 점유할 때의 제한된 실패, accept 뒤 다음 연결과 바이트·peer PID를
검사한다. 없는 pipe 오류는 첫 poll에서 반환해야 한다. 고정 sleep 없이 accept 사건으로 연결을 푼다.
hook CLI 진단은 기존 `codex_integration`의 무음 시험이 기본 exit 0·빈 출력과 `AAA_LOG=debug`일 때만 나오는
전송 실패 원인, token·payload 비노출을 함께 확인한다.

| 무엇 | 위치 | 실행 |
|---|---|---|
| 순수 규칙·판정(Rust) | 소스 옆 `#[cfg(test)]` | `cargo test -p <crate>` |
| 여러 프로세스(host·agent·session·CLI) | `crates/aaa/tests/*.rs` + 실험실 `crates/aaa/tests/lab/mod.rs` | `cargo test -p aaa --test m13 <이름>` |
| 웹 순수 규칙 | `web/test/*.test.mjs`(`node --test`) | `cd web && npm test` |
| 웹 화면(실제 host·agent + Playwright) | `web/e2e/specs/번호-기능.mjs` + `web/e2e/lib/harness.mjs`(아래 「웹 E2E」) | `node web/e2e/web.e2e.mjs [--spec 07]` |

## 기다리기

### 부하 때 프로세스 시작·전달 대기(t-0606)

t-0578 첫 착지의 동시 검증에서 m13 다섯 시험과 웹 E2E 63의 아래 대기가 30초를 넘겼다.
실패한 m13의 머신 건강에는 CPU 10개에서 1분 부하 28.96(뒤 표본 24.89·21.06)이 기록됐다.
다음 전체 검증에서는 모두 통과했고 웹 후보 점검 단계는 1491ms였다. 현재 코드의 기존 상한 대조에서도
해당 m13 다섯 시험·task_queue·task_edit와 웹 63 전체가 통과했다.
추가로 Rust·웹 전체를 겹친 대조에서도 기존 30초의 m13 103개와 웹 63은 통과했다.
1분 부하가 74.13까지 올랐지만 원래 실패는 다시 나오지 않았다. 따라서 이 조정으로 재현한 결함을
고쳤다거나 60초면 어떤 부하에서도 충분하다고 주장하지 않는다. 원본 부하 실패 지점에만 여유를 둔다.

아래 지점에만 60초의 실패 상한을 쓴다. 기존 30초의 두 배로 시작·전달의 스케줄 여유를 두며,
정상 실행을 60초 기다리는 sleep이나 성능 합격선으로 쓰지 않는다. Rust는 `lab::LOAD_TIMEOUT`,
웹은 harness의 `LOAD_TIMEOUT_MS`에 모은다. 사건·파일·DOM이 오면 즉시 끝나고,
실패한 명령의 재실행·기대값 완화·전역 시한 변경은 하지 않는다.

| 시험 | 30초 → 60초인 대기 | 유지하는 경계 |
| --- | --- | --- |
| m13 `rollout::rollout_*` 세 시나리오 | 처음 사본 agent 둘 연결 | 사전 검사·설치·서비스·복구의 각 30초 |
| m13 `terminal_capture_keeps_output_after_stdio_close_before_reading` | 자식의 표준 FD 닫힘 영수증 | reader를 늦게 여는 순서·정확한 출력·수집 30초 |
| m13 `thread_adopt_existing_pane` | 처음 SessionStart의 대화 ID | 데려오기 거부·보고·재기동·권한 |
| m13 `thread_read_and_keys` | 처음 trust 질문 사건 | 뒤 권한 질문·응답·신뢰 창 거부 |
| m13 `thread_adopt_claude_started_in_a_shell` | 데려온 뒤 첫 brief 전달 | Claude 감지·보고·pane 안 CLI 결과 30초 |
| 웹 E2E `63-deploy` | 맞는 후보의 `deploy-sha` 표지 | SHA 거부·배포 중단·서비스 재시작·기본 locator 30초·단계 120초 |

다음 실패는 같은 방식으로 늘리지 않는다.

- E2E 47은 파일 사건 뒤 최종 DB 판정을 저장하고 갱신 사건을 빠뜨린 제품 결함이다. t-0573의
  결정적 재현·수정을 유지한다. `resource_limits`의 단발 Front EMFILE 미관측은 원인 미확정이며
  15초와 실패 진단을 유지한다.
- Ubuntu CI [39529369100](https://github.com/cLazyZombie/aaa/actions/runs/39529369100)의
  `task_queue_runs_possible_fifo_after_resolve_without_coordinator`는 `thread start`가
  「준비·정리 작업 중」으로 거부된 오류다. [39531630027](https://github.com/cLazyZombie/aaa/actions/runs/39531630027)의
  `task_edit_keeps_bytes_conflicts_and_logs`는 `파일 직접/update` 대신 `파일 직접/add`를 관측한
  단언 실패다. 둘 다 대기 시한 초과가 아니므로 별도 경합으로 조사한다.
- t-0575 rollout의 후보 SHA 거부 뒤 `recovering/restoring`·서비스 재시작 전 상태는
  사본 agent 시작 대기와 다르다. t-0587의 원격 교체 단계 진입 뒤 대기하는 수리를 유지한다.

검증은 기존 상한의 단독 실행과 `scripts/ci-local.sh --only test,e2e`의 동시 부하 대조를 남긴 뒤,
변경 후보의 `scripts/ci-local.sh` 전체로 한다. 부하 대조는 같은 조건에서도 항상 실패한다는 뜻이
아니다. 실패한 대기와 그때의 부하·관측 상태를 함께 기록하고, 부하 없는 실패나 상태 오류가 나오면
시한을 더 늘려 덮지 않는다. 진행이 없는 사건에 상한을 되풀이해 새로 시작하지 않는다.

| 기다릴 것 | 쓸 것 |
|---|---|
| 흐름 사건(thread 시작·보고·질문·병합) | `lab.follow(..)` → `Follow::kind`·`until`(`aaa events <p> --follow --json`, cursor 0부터라 놓치지 않는다) |
| pane 출력에 글 | `Node::wait_output`·웹 `waitOutput`(`aaa pane wait-output`, host가 출력 사건마다 다시 본다) |
| 「host가 보고도 아무것도 하지 않았다」(중복 항목·다시 알림이 없음) | `Node::ticks(n)`(`aaa ticker tick`: 이 명령 뒤에 시작한 tick이 끝나면 돌아온다) 뒤에 단언. herdr-projects 시험이 `ticker::tick_project`를 직접 돌리는 것과 같다 |
| 시간 창이 지난 뒤의 판정(idle·quiet·schedule·outage) | 설정 값으로 창을 줄이고, 창이 지난 것을 확인한 뒤 `ticks` |
| 상태 전이(머신 연결·pane 끝남) | `wait_for`(시한 있는 반복). 사건이 생기면 그것으로 바꾼다 |
| 웹 화면이 그렸다 | 제품 표지(`data-view-top`·`data-view-base`·`data-history-*`·`data-status`·`data-kind`)를 기다린다. 「몇 frame 뒤」를 어림하지 않는다 |
| 웹 화면이 판단을 끝냈다(그래서 하지 않았다) | 판단 자리에서 다는 제품 표지 뒤에 본다. Term `data-parsed-top`·`data-parsed-base`(받은 출력을 다 읽은 뒤 버퍼 자리: 올려 둔 동안 아래에 줄만 붙으면 그리기·`data-view-*`는 오지 않는다), `data-retry`(다시 붙기를 건 close code·ms), `data-fit`(맞추기 판단 off·released·other·keyboard·input·unsized·sent·same)·`data-fit-pending`(크기 바뀜 뒤 맞추기를 기다리는 동안), App `data-wake-lock`(wake lock effect가 돈 설정·걸지·보는 것). 크기·다시 붙기는 그 판단 안에서 바로 보내므로 표지 뒤 `sentFrames`가 비면 보내지 않았다 |
| agent·harness 사건을 host가 처리했다(버린 Stop 등) | 같은 agent 연결로 뒤따르는 표지를 기다린다: 진행 보고(`aaa report` → `thread show`의 `agent.progress`), 그 뒤의 출력. 연결 하나는 순서를 지킨다 |
| host가 일을 끝냈다는 사건이 있다 | 그 사건을 기다린다(`routine_ran`·`nudge_sent`, 새 점검 보고 `health.at_ms`, routine `last_run_ms`). 「몇 초면 돌았겠지」를 어림하지 않는다 |
| 프로세스가 끝났다(session host) | `Node::session_host_gone(pid, session)`(PID는 지우기 전 `session_host_pid`로) |
| 웹 화면이 보내지 않았다(reader 입력·조합 중 Enter·끌기의 마우스 보고) | `sentFrames(page, from)`·`sentText`(harness가 pane WebSocket으로 보낸 메시지를 기록한다). 키·마우스·휠 처리는 그 사건 안에서 보내므로 동작이 끝난 뒤 바로 본다. 타이머로 늦게 보내는 규칙(꾹 누름 되풀이)은 가짜 시계 유닛 시험(`web/test/keyRepeat.test.mjs`)이 본다 |

- **고정 sleep ratchet.** `web/test/fixed-sleeps.test.mjs`가 `web/e2e`·`web/test`·`web/src`의 `sleep(N)`·`waitForTimeout`·setTimeout Promise와 Rust 시험의 `thread::sleep`·`tokio::time::sleep`을 센다(herdr-web-ui `scripts/fixed-sleeps.ts` 그대로). 파일마다 `web/test/fixed-sleeps.json`의 수를 넘으면 실패하고, 줄면 그 수로 낮춰야 하며, origin/main보다 올릴 수 없다(새 파일로 옮긴 것은 합계로 본다). 다른 파일에서 import한 sleep helper(spec이 쓰는 harness `sleep`)도 센다. 시한이 있는 반복 안의 쉼과 `Promise.race`의 시한은 세지 않는다. 일부러 고정한 기다림(「아무것도 오지 않음」 증명)은 그 줄이나 윗줄에 `// fixed wait: <이유>`를 단다. 지금 수: `node web/test/fixed-sleeps.mjs`(`--list`면 줄마다).
- **tick만으로 증명되지 않는 것.** 시간 규칙이 막는 일(예: idle 2초 전에는 nudge하지 않음)은 tick을 백 번 돌려도 일어나지 않는다. 그런 「없음」은 창이 지난 뒤에 tick해야 결함을 잡는다. 순서를 틀리면 시험이 늘 통과한다. 창은 `// fixed wait: <이유>`를 단 sleep으로 지난다(그 시험이 보는 규칙 자체라 표시한 고정 기다림이다). nudge는 m13 `nudge_windows(idle, quiet)`(처음 보는 tick → idle 창 → 입력창을 보는 tick → quiet 창 → 보내는 tick, 창은 그 시험의 host.toml 값).
- **「없음」은 울타리 뒤에 단언한다.** 고정 sleep 뒤에 「일어나지 않았다」를 단언하면 그보다 늦게 온 결함을 놓친다(거짓 통과, 보안 감사 t-0340). 울타리는 위 표의 셋이다: host 판정(창 + `ticks`), 같은 연결의 뒤따르는 표지·끝남 사건, 화면이 보낸 것(`sentFrames`). 바꾼 시험은 그 「없음」을 깨는 결함을 제품에 심어 실패하는지 본다(plan §8.121).

### 시험에서 시간 창을 줄이는 설정

| 규칙 | 설정 |
|---|---|
| ticker 주기 | `host.toml [projects] tick_secs`(1 이상) |
| 머신 끊김 알림 | `[projects] outage_secs` |
| coordinator nudge | `[projects] nudge_idle_secs`·`nudge_quiet_secs` |
| 머신 건강 | `[health] every_s` |
| routine | routine 머리 `schedule = "every 2s"`(host에 `AAA_TEST_ROUTINE_SECONDS=1`이 있을 때만 받는다, lab이 붙인다) |
| 알림 묶음 | `[notify] debounce_secs`(초). 1초 밑은 `AAA_TEST_NOTIFY_DEBOUNCE_MS`(시험에서만, 웹 E2E 17·18 100. 설정 1초 길은 12) |
| 웹 상태 사건 ping | `AAA_TEST_EVENTS_PING_MS`(시험에서만) |
| thread brief를 치기 전 화면 멈춤(3초) | `AAA_TEST_BRIEF_SETTLE_MS`(시험에서만, 웹 E2E·m13 300. m13은 `spawn_host`로 띄운다) |
| 「쓰는 화면」 lease(25초, 앱 글이 그만큼 없으면 쓰는 화면에서 뺀다) | `AAA_TEST_PRESENCE_LEASE_MS`(시험에서만, 웹 E2E 18만 4000: 그 파일의 쓰는 화면은 presence를 1초마다) |
| 끝난 session host가 늦게 붙는 agent를 위해 더 응답하는 창(2초) | `AAA_TEST_SESSION_LINGER_MS`(시험에서만, 늘리는 쪽: m13은 8000으로 forget 한도 5초보다 길게 두어 지우기가 이 창을 기다리지 않는지 본다, plan §8.160) |
| 구독 사용량 억지 다시 읽기 간격(30초) | `AAA_TEST_USAGE_REFRESH_MS`(시험에서만, 웹 E2E 32 0: 가짜 구독 숫자를 바꾼 뒤 `?refresh=1`로 바로 읽는다) |
| harness 화면 감지 시작 유예(3초, 시작 화면을 상태로 읽지 않는다) | `AAA_TEST_DETECT_GRACE_MS`(agent, 시험에서만, 공통 Rust Lab·웹 E2E 0. 기본 3초는 `detect::state::tests::startup_grace_and_skipped_viewer_screens` 시각 주입 owner) |
| 긴 턴(60초, 웹 앱 안 알림은 그보다 긴 턴의 끝남만 띄운다, plan §8.185) | `AAA_TEST_LONG_TURN_MS`(host, 시험에서만, 웹 E2E 19 3000. 기본값 길은 m13 `turn_end_notices_follow_reports`) |
| pane의 macOS 책임 경로 관찰(실행기를 띄운 경로를 물려받는다) | `AAA_TEST_RESPONSIBILITY=none`(agent, 시험에서만, 웹 E2E 전체: ssh·CI 러너에서 돈 실험실에 「느린 빌드」 배너가 떠 화면 높이가 바뀌지 않게. 배너·뱃지는 89가 관측값을 주입해 본다) |

## 웹 E2E

### 두 controller의 전송·focus 순서(t-0488)

- E2E02는 입력줄 전송의 첫 resize를 완료로 보지 않는다. 글 뒤 Enter도 lease를 얻으므로,
  `submitted`를 처리한 입력줄(빈 값·다시 켜진 Enter 단추)과 해당 PTY 출력을 기다린 뒤 focus·blur·숨김으로 간다.
  host의 크기·lease와 두 화면의 제목·실제 행 수·`data-fit`·`data-fit-pending`을 함께 확인해 이전 state를 기준으로 삼지 않는다.
- 보이는 창의 blur는 페이지 시계를 멈춰 999ms에는 release 없음, 1000ms에는 release 있음을 확인한다.
  첫 `pauseAt`으로 설치와 정지를 함께 한다. 별도 `install` 뒤 벽시계로 멈추면 과거 시각을 지정할 수 있다.
  숨김은 `visibilitychange`를 보낸 같은 task에서 pane WebSocket 기록을 닫아 즉시 release를 본다.
  자동화 왕복 시간·700ms 벽시계 한도·Playwright `framesent` 도착 순서에 기대지 않는다.
- E2E03의 크기 되찾기도 500ms를 자지 않고 첫 화면의 새 grid·맞추기 판단을 기다린다.
  키보다 resize가 먼저 간다는 기존 전송 순서 단언은 유지한다. fixed-sleep 상한은 하나 낮춘다.
- 원본은 `AAA_E2E_THROTTLE=2`·`--spec 02 --grep '두 controller' --repeat 6 --workers 6`에서
  focus 실패 3/6을 재현했다. 고친 시험은 같은 조건 6/6, `AAA_TEST_SUBMIT_GAP_MS=1500`으로
  Enter를 늦춘 전체 02도 3/3 통과했다. 시한·허용 오차·시나리오는 줄이거나 늘리지 않는다.
- focus 되찾기 제거, blur의 1초 미만 release, 숨김 즉시 release 제거를 독립 변형으로 심어
  기존 keeper가 각각 실패하는지 본다. 제품 소스는 바이트로 복원하고 웹·실행 파일을 다시 빌드한 뒤 전수 gate를 실행한다.

### diff index와 입력줄 시계의 간헐 실패(t-0496)

- `cargo test -p aaa-agent --lib diff::tests::`의 기존 callback 시험은 같은 크기·mtime의 덮어쓰기와
  그 시각의 index를 만든다. `core.checkStat=minimal`·`core.trustctime=false`로 stat 정밀도의 우연을 없앤다.
  Linux에서 원본은 `+two` 단언에 실패했고 index 사본의 mtime 보존 뒤 통과했다. macOS의 원본 통과로
  Linux를 대신 판정하지 않는다. 원본 index의 바이트·mtime과 외부 callback 미실행도 그대로 검사한다.
  index mtime은 [Git의 racy entry 내용 재검사 기준](https://git-scm.com/docs/racy-git)이므로 사본을 만든
  현재 시각으로 바꾸지 않는다. 열린 원본 파일에서 내용과 시각을 함께 가져온다.
- E2E08은 `clock.install()` 뒤 별도 벽시계의 `pauseAt(new Date())`로 멈추지 않는다. 설치된 페이지 시계가
  벽시계보다 앞서면 과거로 진행하려다 실패한다. `install` 뒤 `runFor(1000)`을 넣은 결정적 재현에서
  CI와 같은 오류를 확인했다. 첫 `pauseAt`이 설치와 정지를 함께 하도록 하며 조합 취소·기한·PTY 단언은 유지한다.
  [Playwright Clock](https://playwright.dev/docs/api/class-clock)의 공개 API를 쓰고 타이머·자동화 왕복을 기다리지 않는다.

### 크기 기준값과 조합 확정 기한(t-0463)

- E2E06의 글꼴 기준값은 제목과 host 열 수가 같은 것만으로 정하지 않는다. 둘 다 이전 snapshot의 크기일 수 있으므로
  `data-fit="same"`이고 `data-fit-pending`이 없어 이 화면의 맞추기까지 끝났을 때 잰다. 제목은 state 수신 때 먼저 바뀌고
  xterm resize는 출력 큐 뒤에 실행되므로, 실제 행 수와 현재 글꼴의 영문 advance 대비 칸 너비까지 맞아야 기준값이다.
  영문 비교는 공용 `fontOk`의 0.02px를 그대로 쓰고 실패에는 기준값도 남긴다. 글꼴 응답은 route gate로
  붙잡고, 받기 전에는 기존 글꼴·칸 크기, 놓은 뒤에는 1.5배 칸 크기·PTY 크기를 확인한다. reload도 요청 도달 뒤 gate를 놓는다.
- E2E08의 합성 조합 확정은 페이지 시계를 멈춰 본다. 자동화의 왕복 시간이 300ms 확정 기한을 소비하지 않도록 하고,
  compositionend가 건 task와 이어지는 조합의 450ms는 `clock.runFor`로 진행한다. 취소·기한 뒤 보내지 않음은
  페이지가 보낸 frame 기록까지 확인한다. 실제 CDP IME·host의 보내기 확인·PTY 수신은 실제 사건으로 기다린다.
- E2E09의 IME 기록은 700ms 뒤 PTY를 읽지 않는다. 페이지가 보낸 전체 글을 기록으로 기다려 기대 문자열과
  비교하고, PTY의 GOT가 그 길이만큼 도착한 뒤 전체 문자열을 다시 비교한다. 다음 MARK는 그 수신 뒤에 보낸다.
  전송 지연이 앞 사례의 입력을 다음 사례에 섞지 않으며, 덧붙은 자모·중복 글도 그대로 실패한다.
- 두 시험은 `node web/e2e/web.e2e.mjs --spec 06-settings,08-input-line --repeat 5 --workers 1`로 좁게 돈다.
  느린 조건의 원본 실패와 수리 뒤 연속 통과는 별도로 남긴다. 기존 시간 상한·허용 오차·조합 수는 바꾸지 않는다.

보고서 열람은 `aaa-host`의 `report_navigation_records_seen_and_opened_but_not_previews`가 실제 HTTP 응답 뒤 DB의
안 봄·마지막 열람·배지 사건을 함께 확인한다. HEAD·미리보기·fetch·그림·실패 응답은 기록하지 않는다.
웹 E2E 21은 제목·공유 링크 클릭과 cookie 없는 Chromium·WebKit 탭의 최초 주소 탐색 뒤 앱 배지가 줄어드는지 본다.
HTML fixture에는 열람 기록용 스크립트를 넣지 않는다. 기록은 브라우저 문서 요청을 받은 host가 한다.
웹 E2E 44는 대화 안 찾기(plan §8.308)를 가상화된 400줄 기록에서 본다. 이동이 끝났는지는 「몇 frame 뒤」가 아니라 칸의 `data-find-index`·`data-find-revealed`로 기다리고, 그린 줄의 데이터 개수와 그린 글의 찾음 수(`data-find-count`·`data-find-dom`)가 같은지 본다. 「앞 기록도 찾기」(§8.310)는 칸의 `data-find-earlier`(idle·loading·failed)·`data-find-pages`를 기다린다. 「멈춘 뒤 앞 쪽을 더 부르지 않음」은 앞 쪽 요청을 route로 센 뒤, 기록에 말을 더해 같은 한 줄 차례(`ConversationRefresh`)의 최신 쪽 읽기가 그려진 것을 울타리로 삼아 단언한다. Codex 기록은 같은 웹 길을 iPad WebKit으로 본다.
웹 E2E 38은 대화에서 연 보고서(plan §8.300)를 WebKit iPhone 390·430 Safari 탭·홈 화면 앱 흉내로 본다. 「새 창이 없음」은 앱 안 보기(`file-viewer`)가 그려진 뒤에 보고, 휴대폰 sheet는 올라오는 움직임(`getAnimations().finished`)이 끝난 뒤 닫기 자리를 잰다. 헤드리스 WebKit의 휴대폰 꼴(hasTouch)도 `pointer: coarse`다. 같은 화면에서 파일 보기 머리가 한 줄(64px 이하, 칸 겹침·잘림 없음)이고 몸이 화면 높이의 85% 이상인지도 잰다(plan §8.302).
웹 E2E 42는 thread 변경 파일 카드와 diff 패널(plan §8.305)을 실제 git 저장소 thread로 본다. 가짜 agent가 보고를 쓴 뒤 worktree에 만드는 새 파일을 파일 존재로 기다리고 나서 worktree를 바꾼다. 카드는 `data-files`(받은 파일 수), 패널은 `data-diff-status="ready"`·`data-diff-lazy`·`data-diff-loaded`(큰 비교에서 읽은 파일 수) 표지를 기다린다. diff 줄은 CodeView shadow root 안의 글로 본다. agent 쪽 읽기 전용 경계(index·HEAD·ref·`.git` 안 파일이 바이트로 같음, 외부 callback 안 돎)는 `cargo test -p aaa-agent --lib diff::`가 실제 임시 저장소로 본다.

### 대화 찾기의 가상 줄 관측 묶음(t-0542, plan §8.337)

- E2E44 「앞 기록의 가상 줄 관측 묶음」은 기존 멈춤·이어 찾기 시나리오를 그대로 실행한다. 첫 찾음 줄의 실제
  IntersectionObserver 화면 밖 관측을 보류하고, Enter로 그 줄에 갈 때 생기는 실제 화면 안 관측과 시간순으로 함께 전달한다.
  보류 표지를 기다리고 전달 묶음 `false,true`도 단언한다. 원본과 주입 단계의 울타리 문구를 구분해 이번 append의 commit을 기다린다.
  가짜 좌표·entry·고정 sleep·시한 연장은 없다.
- [관측 규칙](https://www.w3.org/TR/intersection-observer/#notify-intersection-observers)은 모인 entry 전체를 전달한다.
  한 줄에 observer 하나여도 첫 entry만 쓰면 최신 화면 안 관측을 버려 본문이 빈 채로 남고 찾기 재개용 DOM 변화도 오지 않는다.
  ChatRow는 마지막 entry로 본문을 그릴지 정한다. 느린 통지에서 사용자가 찾거나 스크롤할 때도 해당하는 제품 결함이다.
- 수정 전 주입은 기존 20초 「앞 기록의 처음 1/7」에서 count=7·index=0·revealed=false·earlier=idle로 RED였다.
  첫 찾음 shell은 화면 안에 있지만 본문이 없었다. 같은 주입 GREEN과 주입 없는 기존 네 단계의 반복 통과를 별도로 보존한다.
  최초 착지 실패의 DOM은 남아 있지 않아 그 실행의 실제 entry 묶음까지 확인한 것은 아니다.
  원래 단언(강조 Range의 실제 보임·접힘 펼침·개수 일치·가상화)과 시한을 유지하며 전체 ci-local·check-public을 실행한다.

```
web/e2e/web.e2e.mjs        입구: spec 파일마다 lib/child.mjs 프로세스를 띄우고(worker N개가 함께) 결과를 모은다
web/e2e/durations.json     파일마다 기록한 초. 함께 돌 때 긴 파일부터 넣는다
web/e2e/lib/harness.mjs    실험실(host·agent·가짜 push·구독 서버·시험 프로그램)·Chromium·step·기다림·화면·IME·push 도우미, runSpec
web/e2e/lib/devices.mjs    기기 꼴과 그 기기 규칙의 가짜(iOS 소리·알림), 기록을 사건으로 기다림(아래 「실기와 headless」)
web/e2e/specs/번호-기능.mjs  기능별 단계(01-http-pwa … 19-turn-alerts). export default async function ({ ctl, page }) { … }
```

- **파일마다 새 실험실.** host·agent·Chromium을 파일마다 새로 띄운다(`/tmp/aaw-<pid>`, 준비 약 1.5초, 웹 port는 host가 고른다). 앞 파일이 만든 pane·질문·구독·설정에 기대지 않으므로 파일 하나를 혼자, 여러 번, 다른 파일과 함께 돌릴 수 있다. 한 파일 안 단계는 차례로 돌고 앞 단계가 만든 상태를 쓸 수 있다(그래서 `--grep`으로 뒤 단계만 고르면 앞 단계가 만든 것이 없을 수 있다: 그때는 `--spec`으로 파일을 돈다).
- **준비(`runSpec`).** host·agent(`labA`)·셸 탭 `t1`을 띄우고, 첫 화면 `ctl`·`page`(1280×800, 입력줄)를 t1에 연다. t1에는 `t1-42`가 찍혀 있다: 단계가 t1로 돌아왔는지는 이 글로 본다. 단계가 실패하면 그 파일의 나머지 단계는 돌지 않고 다음 파일로 간다. 파일 안 여러 단계가 같은 준비에 기대면 harness의 준비 도우미를 부른다(`pushLab`: push 서명 키·구독 하나·demo, 17·18). 준비는 `step(…, { always: true })`라 `--grep`으로 단계 하나만 골라도 돈다. 그 파일만 실험실 환경을 바꾸려면 `export const labEnv = { … }`(11: `AAA_TEST_DETECT_GRACE_MS`, 17·18: `AAA_TEST_NOTIFY_DEBOUNCE_MS`, 18: `AAA_TEST_PRESENCE_LEASE_MS`). 제품 시간을 줄이는 값은 기본 시간의 직전/정각을 시각 주입·가상 시계 owner에서 보존하고, 실제 연결 keeper는 화면·PTY·완료 사건을 기다린다(위 표).
- **함께 돌기.** 파일은 worker 여러 개가 함께 돈다. 기본은 10 CPU 이상이면 CPU 수의 5/8 내림·최대10개, 그 밖은 core 둘마다 하나다(`MAX_WORKERS`, t-0670·t-0810의 같은 머신 전체 검증으로 비교), `--workers 1`·`AAA_E2E_WORKERS=1`이면 예전처럼 차례로 돌고 출력을 바로 보인다. 2 이상이면 파일이 끝날 때 그 파일의 `## specs/…`와 단계 줄을 한 덩어리로 찍는다. 함께 돌 때는 `durations.json`의 긴 파일부터 넣는다(LPT): 가장 긴 파일이 늦게 시작해 혼자 남지 않게. 적지 않은 새 spec은 가장 긴 것으로 보고 맨 앞에 넣는다. 파일 시간이 크게 바뀌면 `--write-durations`(모두 통과한 실행의 시간을 적는다)로 다시 적는다. 시간이 늘어도 재시도·timeout 연장으로 넘기지 않는다: 한 파일이 혼자 오래 걸리면 그 파일을 기능별로 나눈다(앞 단계 상태에 기대는 단계는 그 준비를 도우미로).
- **고르기.** `--spec 07`·`--spec 07-scroll-history,09`(파일), `--grep '앞 기록'`(단계 이름 정규식, `AAA_E2E_ONLY`와 같다), `--repeat 25`(고른 파일을 새 실험실로 25번, 끝에 통과 수와 중간·가장 긴 시간. 기본 worker 수로 함께 돈다: 혼자 돌 때 간헐 실패는 `--workers 1`), `--list`(돌지 않고 이름만). 스크린샷 폴더는 첫 인자.
- **번호·수집.** 파일 번호는 두 자리 이상 숫자다(`01-…`, `99-…`, `100-…`). 목록·단일 worker 실행·요약·시간 기록·patch 도구의 파일 목록은 숫자 순으로 정렬한다. 병렬 worker의 기존 긴 작업 우선 배치는 유지하고 같은 우선순위에서는 숫자 순을 따른다. 새 기능은 빈 다음 번호를 쓰고 기존 번호를 재사용하지 않는다. 파일 규칙과 실제 runner `--list` 회귀는 모든 파일의 수집 및 99→100 순서를 함께 확인한다.
- **새 기능.** 그 기능의 spec 끝에 단계를 더한다. 새 화면·새 기능 묶음이면 다음 번호로 새 spec을 만든다. 도우미는 두 spec 이상이 쓰면 harness에 두고 끝의 `export { … }`에 더하며, 하나만 쓰면 그 spec 안에 둔다. 단계 이름은 모든 spec에서 하나다. harness 이름을 쓰면 spec 머리 `import { … } from "../lib/harness.mjs"`에 더한다(빠지면 그 단계가 ReferenceError로 실패한다). 틀은 `web/test/e2e-layout.test.mjs`가 본다.
- **기다리기.** pane 출력은 `waitOutput`(host 사건), DOM은 `domIs`(MutationObserver), 주소는 `hashIs`, 그 밖은 `until`(시한 있는 반복). 새 시험은 앞 셋을 먼저 쓴다. 「이 화면이 보내지 않았다」는 `sentFrames`로 바로 보고, 「pane이 다 받았다」는 보낸 글(`sentText`)과 받은 글이 같아질 때까지 기다린다(03 키 막대 `sent`).
- **나누기 전 한 파일을 고친 branch.** 병합에서 `web/e2e/web.e2e.mjs`가 충돌하면 main의 입구를 받고 자기 변경을 옮긴다:
  ```
  git diff $(git merge-base <내 branch> origin/main) <내 branch> -- web/e2e/web.e2e.mjs > /tmp/mine.patch
  git checkout origin/main -- web/e2e/web.e2e.mjs
  node web/e2e/tools/port-patch.mjs /tmp/mine.patch   # hunk마다 같은 글이 있는 spec·harness를 찾아 고친다, 새 helper는 export·import까지
  ```
  단계 본문은 글자 그대로 옮겼다(바꾼 줄은 t1 표지 기다림 `"한글-8"` → `"t1-42"`과 `pushKey = …` → `setPushKey(…)` 둘, 도구가 같이 바꿔 찾는다). 도구가 못 옮긴 hunk(새 단계 등)는 찍어 주므로 그 기능의 spec에 손으로 둔다.

## 실험실

- Rust: `lab::Lab::new()`가 `/tmp/aae-<pid>-<n>`을 만든다. macOS UDS 경로 한도(104바이트) 때문에 worktree 안이 아니라 짧은 `/tmp`에 둔다(herdr `/tmp/hcli-…`도 같다). Windows는 Named Pipe라 임시 폴더다.
- `lab.node(name, host_toml)`이 설정·상태 폴더를 따로 만든다. host 여러 개·agent 여러 개를 같은 머신에 띄워 여러 머신을 흉내 낸다. listen은 `127.0.0.1:0`(OS가 빈 port를 고른다).
- 명령 없는 pane의 기본 셸은 `/bin/sh`다(`SHELL`을 lab이 정한다, herdr harness와 같다). 사람의 zsh 설정이 화면에 섞이지 않고 로컬과 CI가 같은 화면을 본다.
- 그 셸은 login이라 기계 rc를 읽는다(macOS `/etc/bashrc`는 prompt가 `\h:\W \u\$ `, hostname 길이가 기계마다 다르다). 그래서 lab은 노드 설정 폴더의 `shrc`(`PS1='$ '`)를 `ENV`로 주고 `TERM_PROGRAM`을 뺀다. sh는 rc 다음에 `ENV`를 읽어 모든 시험 셸 prompt가 `$ `다(`tab create -- /bin/sh`도). prompt 모양에 기대는 시험은 이 값을 쓴다.
- 띄운 프로세스는 `Child`로만 잡고 끝날 때 그 PID만 끝낸다. session host는 meta.json의 PID 중 명령줄이 이 실험실 경로인 것만 끝낸다. 시험이 실패하면 폴더를 지우지 않고 「실험실 기록을 남겼다: …」를 찍는다(CI는 그 폴더의 로그를 annotation으로 남긴다).
- Rust 실험실은 `AAA_USAGE_HOME`을 노드 안 빈 폴더로 고정한다. ticker의 주기 조회가 실제 CLI 로그인·keychain·외부 구독에 닿지 않는다. 사용량을 보는 host 유닛은 `UsageContext`와 loopback 서버를 직접 제공하고, 웹 E2E는 기존 가짜 구독 서버를 쓴다.
- ticker를 띄운 Host 유닛 시험은 runtime을 종료한 뒤 임시 폴더를 지운다. ticker·사용량 조회·자동화 task가 Host를 보유하므로 `drop(h)`만으로 SQLite가 닫히지 않는다. `ticker_refreshes_usage_without_a_browser_and_releases_pending_input`는 종료 뒤 `Weak<Host>`가 해제됐는지 단언한다. Windows SQLite 핸들은 삭제 공유를 허용하지 않아, 이 경계 전에 폴더를 지우면 공유 위반이 난다.
- 자주 쓰는 것: `Node::ok`·`json`(CLI JSON), `screen`, `new_pane`, `wait_output`, `connected`, `ticks`, `Lab::follow`, `kill`(SIGKILL로 갑작스러운 죽음), `kill_session_hosts`(재부팅 흉내).

## 선택자(웹)

1. 사람이 보는 역할과 이름: `getByRole("button", { name: "연결" })`, `getByLabel("보낼 글")`. 접근성과 같이 지켜진다(herdr-web-ui 시험이 가장 많이 쓴다).
2. 역할이 없는 자리는 `data-testid`(`pane-<label>`, `machine-<name>`, `terminal`).
3. 상태는 `data-*` 표지로 읽는다(`data-status`, `data-kind`, `data-phase`, `data-view-*`). 글자나 색으로 상태를 짐작하지 않는다.
4. class 선택자는 모양(크기·색·겹침)을 재는 시험에만 쓴다.

## 실기와 headless

- headless로 못 보는 것(실제 iOS 키보드 내림·노치·무음 스위치·홈 화면 앱의 잠듦)은 plan 각 절의 「실기 확인 필요」에 적고, 실기에서 본 사건 순서를 기록해(입력 기록, `web/src/inputTrace.ts`) headless 재생으로 옮긴다.
- 기기 능력(소리·알림·push·화면 키보드·안전 영역·wake lock·설치)은 실제를 쓰지 않고 기록하는 가짜로 바꾼다. 가짜는 그 기기의 규칙을 따라야 실기 결함을 잡는다. 헤드리스 Chromium의 진짜 AudioContext·Notification에는 iOS 규칙이 없다.
- **`web/e2e/lib/devices.mjs`.** `pairDevice(browser, code, "iphone" | "ipad" | "desktop", { standalone, silentSwitch, permission })`가 그 기기 꼴(크기·UA·platform·터치)로 pairing하고 iOS 규칙 가짜를 건다:
  - AudioContext는 suspended로 태어나고 사람이 누른 직후(1초 안)의 `resume`만 running이 된다. 홈 화면으로 나가면 interrupted이고 다시 누르기 전에는 그대로다. running이 아닐 때 낸 소리는 `silent <Hz>`로 남는다.
  - 무음 스위치(`silentSwitch`)를 켠 기기는 소리 범주(`navigator.audioSession.type`)가 playback·play-and-record가 아니면 `muted <Hz>`.
  - Safari 탭에는 `Notification`·`PushManager`가 없다. 홈 화면 앱(`standalone`)은 `navigator.standalone`이고, 알림 권한은 누른 직후에만 묻는다(아니면 `permission-blocked`).
  - 기록은 `exposeBinding`으로 Node에 바로 온다. `device.mark()`로 자리를 잡고 `device.next("무엇", (e) => e === "tone 880", mark)`로 기다린다(page를 다시 묻지 않는다). `device.skipAudio()`는 울리는 소리 끝 뒤로 소리 시계를 옮긴다(제품은 울리는 동안 온 알림을 겹쳐 울리지 않는다, t3code `TestClock.adjust`처럼 자지 않고 건너뛴다). `device.visible(false|true)`는 나감·돌아옴.
  - 쓰는 곳: `specs/15-ios-rules.mjs`. 결함 심기로 본 것: 「interrupted에서 다시 풀지 않음」과 「탭에서 없는 `Notification`을 부름」은 이 가짜만 잡고, 규칙 없는 옛 가짜(`12-threads-push` 「이 기기 알림」)는 둘 다 통과했다.
  - 새 기기 규칙(안전 영역·화면 키보드·wake lock)도 여기에 더한다. 단계 안에서 init script로 새 가짜를 만들지 않는다.

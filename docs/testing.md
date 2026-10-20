# 시험 쓰는 법

새 기능을 만드는 thread가 따르는 짧은 규칙이다. 근거는 참고 넷(herdr 0.9.5·herdr-projects 0.2.35·herdr-web-ui `aa624cb`·t3code `4b1024da`)과 이 저장소에서 실제로 난 간헐 실패다. 전략과 도구 목록은 [plan §13](plan.md#13-테스트-전략과-직접-만들-e2e-도구)에 있다.

## 원칙

- **사건으로 끝낸다.** 시험은 정한 시간을 자지 않고, 기다리는 것이 왔다는 신호(흐름 사건·출력·화면 표지·fake 기록·tick 끝남)를 기다린다. 「시간 제한이 있어야 통과하는 시험은 틀렸다」(t3code `AGENTS.md`). 시한은 오지 않을 때 멈추는 안전 상한일 뿐이다.
- **시간 규칙은 시계를 흉내 내지 않고 `now` 인자와 설정 값으로 본다.** 판정 함수는 `now`를 받는다(herdr `*_at(now)`, herdr-projects `Outage::record(now)`, aaa `inbox::Outage::record`). 그 함수를 유닛 시험에서 여러 `now`로 부른다. E2E는 설정 값으로 시간 창을 줄이고(아래 표), 창이 지난 뒤 `ticker tick`으로 host가 판정했음을 확인한다.
- **실패를 다시 돌려 넘기지 않는다.** 재시도·시한 늘리기·기대값 완화·건너뛰기로 통과시키지 않는다. 실패의 증거(실험실 폴더·화면·로그)를 남기고 원인을 찾는다.
- **실험실은 남의 것을 건드리지 않는다.** 빈 port, 짧은 임시 경로, 자기가 띄운 PID만 정리한다. 운영 host·사용자 셸 설정·실제 clipboard·소리에 닿지 않는다.
- **실행할 파일은 Unix에서 자식이 복사한다.** 병렬 시험의 부모가 실행 파일을 쓰면 다른 fork가 쓰기 fd를 물려받아 복사 뒤 exec도 `ETXTBSY`로 실패할 수 있다([Rust #114554](https://github.com/rust-lang/rust/issues/114554)). 별도 복사 프로세스가 끝난 뒤 실행한다. 재시도·sleep·시험 전체 직렬화로 덮지 않는다. Codex 설치·hook 시험의 `Home::moved_bin`이 이 경계를 쓴다.

## 어디에 두나

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
| 웹 화면(실제 host·agent + Playwright) | `web/e2e/specs/NN-기능.mjs` + `web/e2e/lib/harness.mjs`(아래 「웹 E2E」) | `node web/e2e/web.e2e.mjs [--spec 07]` |

## 기다리기

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
| harness 화면 감지 시작 유예(3초, 시작 화면을 상태로 읽지 않는다) | `AAA_TEST_DETECT_GRACE_MS`(agent, 시험에서만, 웹 E2E 11·19 0. 기본값 길은 15) |
| 긴 턴(60초, 웹 앱 안 알림은 그보다 긴 턴의 끝남만 띄운다, plan §8.185) | `AAA_TEST_LONG_TURN_MS`(host, 시험에서만, 웹 E2E 19 3000. 기본값 길은 m13 `turn_end_notices_follow_reports`) |

## 웹 E2E

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
웹 E2E 44는 대화 안 찾기(plan §8.308)를 가상화된 400줄 기록에서 본다. 이동이 끝났는지는 「몇 frame 뒤」가 아니라 칸의 `data-find-index`·`data-find-revealed`로 기다리고, 그린 줄의 데이터 개수와 그린 글의 찾음 수(`data-find-count`·`data-find-dom`)가 같은지 본다.
웹 E2E 38은 대화에서 연 보고서(plan §8.300)를 WebKit iPhone 390·430 Safari 탭·홈 화면 앱 흉내로 본다. 「새 창이 없음」은 앱 안 보기(`file-viewer`)가 그려진 뒤에 보고, 휴대폰 sheet는 올라오는 움직임(`getAnimations().finished`)이 끝난 뒤 닫기 자리를 잰다. 헤드리스 WebKit의 휴대폰 꼴(hasTouch)도 `pointer: coarse`다. 같은 화면에서 파일 보기 머리가 한 줄(64px 이하, 칸 겹침·잘림 없음)이고 몸이 화면 높이의 85% 이상인지도 잰다(plan §8.302).
웹 E2E 42는 thread 변경 파일 카드와 diff 패널(plan §8.305)을 실제 git 저장소 thread로 본다. 가짜 agent가 보고를 쓴 뒤 worktree에 만드는 새 파일을 파일 존재로 기다리고 나서 worktree를 바꾼다. 카드는 `data-files`(받은 파일 수), 패널은 `data-diff-status="ready"`·`data-diff-lazy`·`data-diff-loaded`(큰 비교에서 읽은 파일 수) 표지를 기다린다. diff 줄은 CodeView shadow root 안의 글로 본다. agent 쪽 읽기 전용 경계(index·HEAD·ref·`.git` 안 파일이 바이트로 같음, 외부 callback 안 돎)는 `cargo test -p aaa-agent --lib diff::`가 실제 임시 저장소로 본다.

```
web/e2e/web.e2e.mjs        입구: spec 파일마다 lib/child.mjs 프로세스를 띄우고(worker N개가 함께) 결과를 모은다
web/e2e/durations.json     파일마다 기록한 초. 함께 돌 때 긴 파일부터 넣는다
web/e2e/lib/harness.mjs    실험실(host·agent·가짜 push·구독 서버·시험 프로그램)·Chromium·step·기다림·화면·IME·push 도우미, runSpec
web/e2e/lib/devices.mjs    기기 꼴과 그 기기 규칙의 가짜(iOS 소리·알림), 기록을 사건으로 기다림(아래 「실기와 headless」)
web/e2e/specs/NN-기능.mjs  기능별 단계(01-http-pwa … 19-turn-alerts). export default async function ({ ctl, page }) { … }
```

- **파일마다 새 실험실.** host·agent·Chromium을 파일마다 새로 띄운다(`/tmp/aaw-<pid>`, 준비 약 1.5초, 웹 port는 host가 고른다). 앞 파일이 만든 pane·질문·구독·설정에 기대지 않으므로 파일 하나를 혼자, 여러 번, 다른 파일과 함께 돌릴 수 있다. 한 파일 안 단계는 차례로 돌고 앞 단계가 만든 상태를 쓸 수 있다(그래서 `--grep`으로 뒤 단계만 고르면 앞 단계가 만든 것이 없을 수 있다: 그때는 `--spec`으로 파일을 돈다).
- **준비(`runSpec`).** host·agent(`labA`)·셸 탭 `t1`을 띄우고, 첫 화면 `ctl`·`page`(1280×800, 입력줄)를 t1에 연다. t1에는 `t1-42`가 찍혀 있다: 단계가 t1로 돌아왔는지는 이 글로 본다. 단계가 실패하면 그 파일의 나머지 단계는 돌지 않고 다음 파일로 간다. 파일 안 여러 단계가 같은 준비에 기대면 harness의 준비 도우미를 부른다(`pushLab`: push 서명 키·구독 하나·demo, 17·18). 준비는 `step(…, { always: true })`라 `--grep`으로 단계 하나만 골라도 돈다. 그 파일만 실험실 환경을 바꾸려면 `export const labEnv = { … }`(11: `AAA_TEST_DETECT_GRACE_MS`, 17·18: `AAA_TEST_NOTIFY_DEBOUNCE_MS`, 18: `AAA_TEST_PRESENCE_LEASE_MS`). 제품 시간을 줄이는 값은 그 기다림을 기본값으로 도는 파일을 하나 남긴다(위 표).
- **함께 돌기.** 파일은 worker 여러 개가 함께 돈다. 기본은 core 8개마다 셋, 많아야 6(`MAX_WORKERS`, 16 core에서 잰 상한: 6에서도 파일 합계 그대로, 한 번 난 07 실패는 agent pump 결함 plan §8.162로 고친 뒤 통과), `--workers 1`·`AAA_E2E_WORKERS=1`이면 예전처럼 차례로 돌고 출력을 바로 보인다. 2 이상이면 파일이 끝날 때 그 파일의 `## specs/…`와 단계 줄을 한 덩어리로 찍는다. 함께 돌 때는 `durations.json`의 긴 파일부터 넣는다(LPT): 가장 긴 파일이 늦게 시작해 혼자 남지 않게. 적지 않은 새 spec은 가장 긴 것으로 보고 맨 앞에 넣는다. 파일 시간이 크게 바뀌면 `--write-durations`(모두 통과한 실행의 시간을 적는다)로 다시 적는다. 시간이 늘어도 재시도·timeout 연장으로 넘기지 않는다: 한 파일이 혼자 오래 걸리면 그 파일을 기능별로 나눈다(앞 단계 상태에 기대는 단계는 그 준비를 도우미로).
- **고르기.** `--spec 07`·`--spec 07-scroll-history,09`(파일), `--grep '앞 기록'`(단계 이름 정규식, `AAA_E2E_ONLY`와 같다), `--repeat 25`(고른 파일을 새 실험실로 25번, 끝에 통과 수와 중간·가장 긴 시간. 기본 worker 수로 함께 돈다: 혼자 돌 때 간헐 실패는 `--workers 1`), `--list`(돌지 않고 이름만). 스크린샷 폴더는 첫 인자.
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


## post 검증(§8.199)

- host의 `post_*` 시험은 `now`로 rate 창 경계를 보고, durable ID·mute·inbox·fence·push를 검사한다.
- agent의 `post_*` 시험은 실제 hook socket과 메모리 Agent link를 쓴다. Named Pipe처럼 쓰기 반쪽을 닫지 않은 채
  응답을 기다려 Windows와 Unix가 같은 JSON 완성 계약을 검사한다. 무송신은 같은 writer의 뒤따르는 Pong으로 확인한다.
- hook 수신 시험은 열린 연결의 분할 JSON·UTF-8, 옛 EOF 방식, 문법 오류·크기 한도·미완성 EOF를 검사한다.
- m13 `post_from_thread_reaches_coordinator_inbox`는 실제 CLI를 로컬·TLS 원격 thread에서 실행한다.
  CLI Ack 뒤 `post-probe-done` 출력과 coordinator nudge 사건을 기다린다. 운영 프로세스·설정은 쓰지 않는다.
- 결함 심기 결과는 작업 report/library에 남긴다. 신호 동작 시험은 새로 만들지 않는다.

## 보고서 publish·등록·이전 검증(§8.200)

- host `library_links` owner 시험이 실제 Front API·HTTP router에서 덮어쓰기, 상한, 실시간 변화·탈출, import 건너뜀/접두사, prune(now), rotate와 pairing을 본다.
- 웹 E2E 21 `library-reports`는 실제 CLI·host로 목록·크기·만료·doctor·열기·복사·폐기·재발급·Reader를 1280/390px에서 본다.
- 기존 E2E 12의 top-level sandbox·SW 제외·형제 그림 검증을 유지한다. 시험 실험실을 정리하는 기존 자식 PID 처리만 쓰고 신호 동작 시험은 추가하지 않는다.
- Windows 공통 시험에는 예약 파일 이름을 만들지 않으며 symlink 생성은 Unix 갈래로 한정한다. 공통 계약 시험 자체는 Windows에서도 실행한다.

- hook 클라이언트의 실제 UDS 회귀는 peer가 정상 응답·종료한 뒤 write shutdown을 하도록 oneshot으로 순서를 고정한다.
  macOS ENOTCONN 뒤에도 정상 응답은 읽으며, 빈 응답·거부를 성공으로 치지 않는다. sleep·재시도·timeout 증가는 없다.
- post m13의 nudge는 기본 15초 ticker 두 주기와 30초 대기 시한을 경주시키지 않는다. 설정한 idle·quiet 창을
  `nudge_windows`로 지난 뒤 판정 tick을 끝내고 coordinator 출력을 본다.

## Codex 전역 연동 검증(§8.201)

- m13 `auto_resolve_and_reopen`의 상태 `resolved`는 정리 시작 때 저장되므로 inbox 완료 신호가 아니다.
  idle은 자동 종료를 끈 상태에서 먼저 확인하고 실제 설정을 켠다. kill 응답 도중 짧은 idle 창이 이미 지나도 중간 상태 관측을 놓치지 않는다. 기존 종료·재열기 assertion과 시한은 유지한다.
  auto-resolve를 실행한 ticker가 끝난 응답(`ticker tick`) 뒤에 `resolved` 항목 하나와 재개 안내를 검사한다.
  `idle`·새 보고 항목은 별개 사건이다. 그 항목들을 지워 기대값에 맞추지 않는다.
- `cargo test -p aaa --test codex_integration`: 임시 HOME/CODEX_HOME의 설치·제거·이동·멱등·백업·기존 hook 보존·doctor 수리.
  설치한 명령의 stdin 사건 → 실제 agent → 실제 host → Front 상태까지 본다. session host만 가짜이며 신호를 보내지 않는다.
  Windows에서는 Codex 기본 runner처럼 COMSPEC/cmd.exe `/C`로 설치 명령을 실행한다(PowerShell을 한 겹 더 감싸지 않는다).
  특수 문자·한글 경로에서 설치한 명령이 인증된 사건의 한글 안내를 UTF-8 JSON으로 돌려주고,
  거부 사건은 종료 0·stdout 0·stderr 0인지 확인한다. PowerShell 진행 CLIXML도 stderr가 있으면 실패한다.
  거부 시험은 실제 화면 감지의 Idle을 먼저 기다리고, 같은 agent link의 PaneRead 응답 뒤 두 pane의 AgentStatus가
  거부 전과 완전히 같은지 본다. 화면 감지는 hook 없이도 상태를 만들므로 `agent == None`으로 거부를 판단하지 않는다.
  안내가 원래 없는 옛 notify에도 잘못된 token·다른 pane peer를 보내, 출력 없이 host만 바꾸는 guard 결함을 잡는다.
  상태 사건은 현재 Panes를 다시 읽는 계기로만 쓰며, 모아 둔 옛 상태로 기다림 정리를 통과시키지 않는다.
  시작·압축·사용자 응답 안내는 올바른 사건 이름의 UTF-8 JSON·한글로 검사하고 진행 보고의 한글 활동도 확인한다.
  비-aaa hook은 stdin을 열린 채로 두어 EOF 전에 종료함을 확인한다. 결함이면 안전 상한에서 stdin을 닫아 정상 종료시킨 뒤 실패한다.
  Windows 설치 명령은 `cmd`의 `if defined`에서 세 필수 env를 먼저 검사해 비-aaa 세션에서 PowerShell을 띄우지 않는다.
  이 시험의 3초는 shell 기동도 포함한다. 실패 문구만으로 EOF 대기를 확정하지 않고, stdin을 닫은 뒤 걸린 시간과 출력을 함께 기록한다.
- `cargo test -p aaa-agent --lib hooks::`·`threads::`·`codex_start_before_pane_registration_waits_for_registration`:
  안내 간격·중첩 세션·사용자 notify·등록 순서를 본다. notify 실행 fixture는 Python 3.11+ 표준 tomllib를 쓴다(Unix).
  등록 순서 시험은 실제 attach가 등록 완료를 알리게 하며 시험이 알림을 대신 공급하지 않는다.
- `cargo test -p aaa --bin aaa integration::tests::`: Windows EncodedCommand를 복호화해 특수 경로·UTF-8·stdin 전달 계약을 검사한다.
  `cargo xwin check --target x86_64-pc-windows-msvc --workspace --all-targets --locked`와 같은 target clippy는 cfg 컴파일 검사다.
  Windows 실행 결과는 Windows runner에서 같은 codex_integration 시험을 실행한 결과로 구분한다.
  Mac의 `cargo xwin test --target x86_64-pc-windows-msvc --locked -p aaa --test codex_integration --no-run`은 실행 파일만 만든다.
  Windows 임시 폴더에 aaa.exe와 시험 exe를 옮긴 경우 `$env:AAA_TEST_BIN = 'C:\lab\aaa.exe'`로 CLI 경로를 지정해
  `& 'C:\lab\codex_integration.exe'`를 실행한다. 이 경로 재지정은 시험 전용이며 운영 설정을 건드리지 않는다.
- 신호 시험 금지 작업에서는 workspace 전체 test·기존 lab 기반 m13·웹 E2E를 실행하지 않는다(그 실험실의 정리가 신호를 쓴다).
  fmt·전체 target build/clippy·deps·위 경계 시험·공개 검사는 실행하고, 제한한 검증 범위를 보고서에 적는다.

## 특수 임무 시험(2단계, §8.205)

`cargo test -p aaa-host -p aaa-agent mission`의 host 시험은 OS 프로세스나 신호를 새로 쓰지 않는다. agent의 Git 권한 keeper는 자기 임시 저장소의 Git 자식 명령만 실행하며 신호·sleep은 쓰지 않는다. 실제 host·DB·frame owner에 가짜 agent를 연결해 승인/적용 사본, 범위·debounce·예산, Kill 전 durable 표지·끝남 사건 뒤 새 대화, 막힘/끝남, mission post·쌍 한도, 무한대/철수 제안, 기존 사용량 값에 따른 입력/cursor 보존과 reset 뒤 재개, 옛 상대 무송신을 본다. agent에서는 상태 파일의 처리 셈·조각 이동/hash와 Claude SessionStart 포인터를 본다. 재부팅 중 pause 보존·reset 후보 profile 일치, 같은 repo 경로의 다른 머신 승인, DB 저장 실패 시 routine hash/입력 원자성과 reset·깨움 무전송도 검사한다. 기존 m13의 evaluator_flow·routines·reboot_resumes_threads와 웹 12의 「특수 임무」 단계는 같은 공식 전체 gate에 남긴다. 새로운 실제 Kill 시험은 만들지 않는다.

## Codex 신뢰·메시지 받음 회귀(§8.206)

- E2E 11의 휴대폰 신뢰 답은 실제 첫 `questions` HTTP 응답을 보류하고 질문 사건을 먼저 받게 한다(§8.207).
  자동 대화 선택(`chat-toggle`의 pressed·chat visible)과 카드 visible 뒤 응답을 풀고,
  App `data-questions-sync` 증가(목록 응답을 화면에 적용함)를 기다려 같은 카드가 유지되고 실제 답이 전달되는지 본다.
  HTTP·WebSocket 순서를 고정하므로 재시도나 CPU 속도·몇 frame에 기대지 않는다.
  `web/test/questionReload.test.mjs`는 늦은 목록의 답/메뉴 덮기와 옛 연결의 응답/실패가 새 연결을 지우는 회귀를 본다.
- `codex_integration::codex_trust_detection_question_and_answer_share_the_front_path`는
  0.163 화면 표본의 실제 감지부터 Agent link·질문·Front 답변·전송 키까지 본다. OS 자식·신호를 쓰지 않는다.
- host `harness::tests::codex_message_and_next_do_not_revive_previous_waiting_report`는
  hook 유무 × 메시지/Next 네 경우를 검사한다. 입력 받음 뒤 중간 idle에서 옛 기다림과 inbox 항목이 생기지 않음을,
  보고 확인 task 완료 알림과 실제 `thread_pass` 뒤 단언한다. 새 기다림과 실제 질문은 계속 보인다.
- 웹 E2E 11의 `Codex 폴더 신뢰:` 단계는 데스크톱·390px 카드의 답이 pane에 도착함을 출력 사건으로 기다린다.
  신호 금지 작업에서는 `AAA_E2E_ONLY='^(확인창 질문:|Codex 폴더 신뢰:)'`로 공식 ci-local e2e를 제한한다.
  두 단계 안에 신호 시험은 없으며 공식 실험실이 직접 띄운 자식 PID 정리만 실행한다.

## release·다운로드 계약(§8.210)

- `cargo test --locked -p aaa --bin aaa update::`는 임시 HTTP 서버와 실제 curl로 manifest 선택·SHA-256 거부·없는 OS/arch·전송 중단·같은 판 무동작·보관/rollback을 본다. 서버는 응답/연결 종료 사건으로 끝나며 운영 설정·키체인·신호·고정 sleep을 쓰지 않는다. 같은 시각에 version이 내려가는 보관 순서는 고정 `now`로 검사한다.
- `cargo test --locked -p aaa --bin aaa cli::tests::update_accepts_release_version_and_keeps_local_and_subcommand_modes`는 공개 CLI의 release 판·로컬 파일·rollback 선택과 충돌을 본다.
- `python3 -B -m unittest scripts.test_release`는 release 도구를 실제 CLI로 실행해 tag/Cargo 판 일치·빠진 실행 파일 거부·생성 manifest와 checksum이 실제 바이트를 가리키는지 본다. `.github/workflows/distribution.yml`에서도 돈다.
- release workflow 실행·tag push는 착지 뒤 coordinator가 한다. actionlint가 없으면 YAML 문법 검사와 install-runbook 9.1의 herdr 차이표로 검토한다. 신호 금지 작업은 공식 ci-local에서 Rust 전체 test를 제외하고 문서의 확인창 E2E 범위를 쓴다. Windows는 웹 빌드가 끝난 뒤 target clippy로 별도 확인한다.


## 보고서 사본 자동 정리(§8.212)

- `cargo test --locked -p aaa-host library_links::tests::retention_`: 실제 임시 사본·manifest·DB·서명 HTTP 경계에서
  기한 삭제/401, 용량의 갱신 순서, 진행 중·실시간 보호와 합계 제외, 보호 초과 무삭제/중복 알림 없음,
  0으로 끄기, 6시간 tick 경계, dry-run 후보 사유·무쓰기를 본다. now를 인자로 주며 sleep·프로세스·신호를 쓰지 않는다.
- `cargo test --locked -p aaa-config library_retention`: 기본값·0/빈 문자열·값 오류 위치·환산 overflow를 본다.
- 기존 웹 E2E 21에 doctor 한도·CLI dry-run·웹 합계/기한 표시를 더했다. 신호 시험 금지 작업에서는
  `AAA_E2E_ONLY='^(확인창 질문:|Codex 폴더 신뢰:|보고서 전체 목록:)'`로 공식 ci-local e2e를 제한한다.
  이 세 단계는 신호 시험을 하지 않으며, 작업 지시가 허용한 공식 실험실의 자식 PID 정리만 실행한다.
  전체 Rust test에는 신호 시험이 있으므로 제외하고 위 host/config·proto·deps를 따로 실행한다.

## 대화 이미지와 Enter(§8.219)

- 웹 E2E 23 `composer-attachments`가 실제 host·agent의 coordinator pane을 열고 Chromium·WebKit 데스크톱·iPhone·iPad에서
  파일을 가진 clipboard paste 사건, drop 사건, 실제 파일 선택 단추를 거친다. OS clipboard와 iOS의 길게 누르기 메뉴 자체는
  headless가 제공하지 않으므로 그 메뉴 뒤 브라우저가 내는 `ClipboardEvent`를 재생한다. 실제 사람의 clipboard는 바꾸지 않는다.
- 업로드 완료 DOM 표지 뒤 프로젝트 `uploads/` 원본 bytes를 대조하고, 보내기 전에는 pane input·submit·attach가 없음을 본다.
  보내기 뒤 실제 PTY 프로그램의 출력 사건을 기다린 후 프로그램이 `@경로`로 연 파일의 SHA-256과 글을 대조한다.
- 프로젝트 밖 pane은 512KiB를 넘는 그림이 agent 세션 폴더에 조각으로 저장되는 경계를 별도로 확인한다. 삭제·형식 거부·Reader·미인증·다른 Origin도 확인한다.
- 휴대폰·패드 Enter는 여러 줄 내용, 데스크톱은 Shift+Enter 줄바꿈과 Enter 전송, IME는 조합 중 Enter 및 WebKit의 확정 keyCode 229를 본다.
  입력창 높이·rows·자동 늘림은 다른 작업 범위다. 새 시험에는 고정 sleep·polling을 넣지 않는다.
- Rust 실험실 `Node::cmd`는 바깥 aaa pane의 `AAA_SESSION_ID`·`AAA_HOOK_SOCK`를 상속하지 않는다. 세션 안 권한 거부 시험은
  각 명령에 세션 ID를 명시한다. 실제 세션 안에서 전체 gate를 돌렸을 때 사람 터미널·폴더 신뢰 시험을 오판하던 격리 결함을 막는다.
  운영 CLI의 사람 판정·host의 프로세스 기반 호출자 판정은 바꾸지 않는다.
- m13 `thread_read_and_keys`는 실제 report 실패 진단 뒤 spinner가 지워지고 권한 창을 화면 감지로 찾아 키로 답하는 경계를 유지한다. 진단 문구는 fixture가 지우므로 순간 화면 대신 CLI 종료값을 파일에 남겨 권한 창 사건 뒤 실패 실행을 확인한다. 예전 상대 4줄 복귀로 바꾸면 spinner가 남아 권한 창 검출이 실패해야 한다.

웹 E2E 12의 「특수 임무: 좁은 창·휴대폰·데스크톱」은 390/500/1280px에서 승인 메뉴·설명 글 조각·각 단추의 화면 경계와 실제 hit target을 확인하고 Controller 승인을 저장한다. 기존 승인 시험에 화면 경계를 더했으며 별도 가짜 위치 계산 시험은 없다. 화면마다 새 탭을 사용해 퇴장 sheet와 다음 크기를 섞지 않는다. `--spec 12 --grep '특수 임무: 좁은'`으로 이 경계만 확인할 수 있다.

## coordinator 전용 외부 푸시(§8.249)

- host `working_threads_publish_landing_reports_once`가 실제 사본·DB·inbox에서 완료·사람 필요 표지를 유지하면서 외부 대기열은 비우는 계약을 본다. 새 `[확인 필요]`와 옛 `[사람 필요]`의 앱 안 알림 의도도 함께 검증한다.
- `post::tests::coordinator_can_request_the_user_without_nudging_itself`는 host 정본 coordinator pane과 두 사용자 post 의도만 외부 대기열에 들어가고, 밖 CLI·위장·다른 프로젝트·웹·시스템 항목·다른 topic은 빠지는지 본다. `post_fence_push_matrix_and_privacy`는 post 본문·제목을 외부에 싣지 않는 계약, `push::tests::payloads`는 실제 암호문 4096바이트 한도를 지킨다.
- `m13::notify_webhook`은 실제 coordinator pane의 자식 CLI가 보낸 사용자 post만 webhook으로 묶이고, thread 보고는 inbox에 남으며 실패한 알림은 재전송하지 않는지 확인한다.
- 웹 E2E 12는 실제 암호문을 복호화해 coordinator 전체 완료의 고정 본문과 프로젝트 클릭 목적지를 확인한다. 17은 coordinator post 푸시의 쓰는 화면·기기별 설정, 18은 coordinator 푸시를 받은 기기에서도 별개 thread 보고가 앱 안에 표시되는 경계를 본다. DOM 반영은 실제 표시 완료로 끝낸다.
- 신호 금지 작업은 공식 ci-local의 Rust 전체 test를 제외하고 `AAA_E2E_ONLY='^(Web Push:|thread 보고서·결과물:|확인창 질문:|Codex 폴더 신뢰:|보고서 전체 목록:)'`로 고른다. 이 단계에는 신호 동작 시험이 없으며 실험실이 직접 띄운 자식 PID 정리만 허용한다.
- iOS의 `from <앱 이름>`과 Android/브라우저의 출처 줄 배치는 플랫폼 소유 UI다. headless 시험은 service worker에 전달한 title/body와 목적지를 확인하며 실기 OS 창의 줄 수를 증명하지 않는다.

## coordinator 배포 safety(§8.228)

m13 `coordinator_deployment_is_gated_and_survives_backend_restarts`는 별도 실행 파일 사본과 TLS agent에서 기본 차단·사람의 safety 허용/철회·coordinator ID 위장·env를 지운 worker 차단·실제 교체/rollback·host/자기 agent 재시작 뒤 동일 pane PID와 결과 조회를 확인한다. probe는 파일 기록 뒤 고유 출력 표지로 끝낸다. 실험실의 로컬·원격 agent를 다시 띄운 뒤에는 새 프로세스의 연결 로그와 host의 connected 상태를 함께 확인하고 coordinator를 연다. 이전 연결 상태나 원격 연결만 보고 로컬 준비를 추측하지 않는다. macOS에서는 격리 HOME/PATH의 launchctl 모형으로 typed host/원격 agent restart와 실패의 영구 기록을 확인한다. 운영 launchd에는 닿지 않는다. agent `deploy::tests`는 같은 ID 중복·동시 작업·다른 요청 충돌·중단 복구/실행기 중복 잠금/SHA 불일치를 담당한다. 기존 update E2E가 public release hash·self-check·pin·rename을 계속 소유한다.

배포 실행기 수명 회귀(§8.233): 기존 `jobs_are_durable_once_and_reconcile_only_after_the_executor_releases_its_lock`에서 실제 잠금 File을 `try_clone`해 fork와 같은 잠금 참조를 남긴다. Unix에서는 원본 FD의 close-on-exec도 확인한다. 실행기가 살아 있으면 중복 configure는 실패하고, drop 뒤에는 복제 FD가 살아 있어도 새 실행기가 즉시 잠금과 journal 복구를 맡아야 한다. dup/fork는 같은 flock을 공유하므로 자식·신호·sleep 없이 병렬 spawn의 fork~exec 창을 결정적으로 재현한다. close-on-exec만으로는 exec 전 잠금 참조가 사라지지 않는다.

local agent 재연결 중 배포 조회(§8.237): host `deploy::tests::coordinator_identity_survives_local_agent_disconnect`가 연결 상태와 독립적인 peer PID/pane 소속·권한 경계를 검사한다. CLI `deployment::tests::status_waits_for_reconnection_but_preserves_nonwait_and_forbidden_errors`는 Front IPC에서 AgentDisconnected 뒤 State 사건을 기다려 같은 ID만 조회하고, nonwait·Forbidden을 즉시 반환하는 경계를 검사한다. 머신 ID·생략을 포함한다. 고정 대기·새 자식·신호는 없으며, 기존 m13의 실제 재시작 검증을 대체하지 않는다.

### Ctrl+Enter 공통 판정

Ctrl+Enter는 agent가 작업 중일 때만 steering하고 줄바꿈을 넣지 않는다(plan §8.239 후속·정정). E2E 25는 감지가 알린 목록 상태(`data-status`)를 사건으로 기다린 뒤 누른다. `aaa-host/src/ctrl_enter.rs`의 유닛 시험이 입력 상자 스타일과 현재 steering 패널, Codex 작업 중·쉼을 판정하고 어느 경우에도 줄바꿈 키가 나오지 않는지 본다. 「보내지 않음」은 E2E 25가 같은 pane 연결의 뒤따르는 입력을 울타리로 삼아 그 조각만 닿았는지로 확인한다. `web/e2e/specs/25-ctrl-enter.mjs`는 실제 host·agent·PTY로 전달된 bytes를 출력 사건으로 확인하며 웹 물리 키·모바일 키 막대·입력줄·IME·반복 키의 연결을 본다. 셸 등 일반 프로그램의 키보드 모드는 기존 `03-keys.mjs`가 소유한다. 웹 입력창(대화 입력창·웹 입력줄)에 글이 있을 때의 Ctrl+Enter는 `submit`의 `steer`다(plan §8.239 「웹 입력창 Ctrl+Enter」). 끝 키는 유닛 `steered_text_ends_with_send_now_only_for_a_working_claude`가 본다. E2E 25 「대화 입력창 Ctrl+Enter」는 PTY에 닿은 bytes 수를 출력 사건으로 기다리고, 무엇을 보냈는지는 페이지가 보낸 frame(`sentFrames`)으로 본다. 쉬는 빈 입력창은 웹이 아무 frame도 보내지 않는지로 「보내지 않음」을 확인한다. 가짜 harness는 기록 위치를 알리지 않으므로 보내짐은 host 전달이고, 입력창이 비고 풀리는 것을 DOM 사건으로 기다린다.

## Mac 웹 터미널 Ctrl+V(§8.232 후속)

기존 E2E 11의 「그림 붙여 넣기·끌어 놓기」가 소유한다. Ctrl+V는 어느 OS에서나 브라우저 붙여넣기 몫이다(그림 읽기는 사용자 결정으로 되돌렸다). 실제 host·agent와 raw PTY에서 Mac Chrome·WebKit·Windows·iPad·iPhone의 Ctrl+V(누름·꾹 누름)가 clipboard를 읽지 않고, 뒤따르는 표준 글 paste가 `^V` 없이 PTY에 닿으며(같은 연결 순서가 「보내지 않았다」의 울타리), 그림 paste는 이번 경로·bytes로 닿는지 본다. 페이지의 Clipboard API만 읽기 수를 세는 가짜여서 사람의 clipboard나 OS 권한 설정에 닿지 않는다. `--spec 11 --grep '그림 붙여 넣기'`로 좁게 돌린다.

## 임무 입력 안내와 조회 권한(§8.246)

기존 host `approved_start_attaches_mission_before_preparing_special_brief`, agent `mission_launch_scopes_permissions_for_both_harnesses`·`mission_settings_do_not_write_through_links`, proto `fill_brief_puts_the_machine_paths`가 입력이 없는 첫 prepare/rebrief의 inbox·자기 임무 문맥·실제 머신 경로로 채운 brief·정확한 목록 및 조회 규칙을 검사한다. 일반 thread에는 임무 권한을 남기지 않고, symlink inbox로 폴더 밖에 쓰지 않으며, 넓은 ls/find/aaa 허용을 넣지 않는다. 새 신호 시험·고정 sleep·production 시험 seam은 없다. `cargo test -p aaa-host -p aaa-agent mission`과 `cargo test -p aaa-proto fill_brief_puts_the_machine_paths` 뒤 공식 전체 gate를 실행한다.


## 임무 빈 입력 우선과 자기 worktree Git 읽기(§8.251)

- 기존 host 승인/brief keeper는 임무 본문 뒤에도 입력 우선 규칙이 와서 빈 입력일 때 즉시 턴을 끝내고,
  초기화·참고 이력 조사·polling을 하지 않는 계약을 본다. 실제 모델의 지시 준수는 배포 뒤 운영 관찰로 확인한다.
- 기존 agent launch keeper는 자기 worktree에 고정된 log/show/status 세 규칙과 일반 thread에 권한이 남지 않음을 본다.
  proto keeper는 brief와 규칙의 같은 경로 quoting·옵션 종료 표지·잘못된 경로의 무허용을 확인한다.
- agent `mission_git_reads_keep_option_and_index_boundaries`는 실제 Git과 공백·따옴표 경로에서
  commit diff·blob·status 읽기를 실행한다. fsmonitor·textconv callback을 설정해 실행되지 않고 index 바이트도
  그대로인지 본다. log/show 뒤 `--output`을 붙여도 쓰지 못해야 한다. 옵션 종료 표지를 제거한 변형이 이 쓰기 경계에서
  실패한 뒤 원본으로 복원됐다. 소유한 임시 폴더만 쓰며 신호·고정 sleep·production 시험 seam은 없다.
- 격리한 실제 Claude CLI(default permission mode)는 로컬 합성 provider가 돌려준 Bash 도구 호출을 처리한다.
  인증·운영 설정을 사용하지 않고, 다른 허용 자료 폴더로 cwd가 바뀐 뒤 옛 cd+git의 trust 거절과 새 git -C 세 읽기의
  무승인 성공을 구분한다. 이것은 권한 엔진 검증이며 실제 모델의 빈 입력 대기 판단을 대신하지 않는다.

## resolve 뒤 종료된 pane 제거(§8.254)

- m13 `resolve_removes_exited_panes_even_when_the_worktree_is_kept_or_adopted`가 실제 CLI→host→agent 경계에서 dir·dirty worktree·adopted pane을 입력으로 exit 1 종료한다. 종료 상태 확인 뒤 resolve하고 pane 목록·layout에서의 제거와 폴더 보존을 단언한다. 새 신호·고정 sleep·production 시험 seam은 없다.
- 기존 `resolve_keeps_dirty_worktree_and_unmerged_branch`는 partial·skip-copy를 포함한 폴더/branch 보존과 별개로 pane을 닫는지 본다. `thread_adopt_existing_pane`은 복원된 살아 있는 pane도 닫고 원래 branch·파일은 남기는 경계를 본다. linger 시험은 기존대로 session host의 종료 유예를 기다리지 않는지 확인한다.

## 삭제된 TUI의 접속 거절

`cargo test -p aaa --test tui_removed`는 host 없는 로컬 `attach`·`tui`·`evaluator open`이 삭제 안내와 실패 코드로 끝나는지, 실제 host socket이 옛 `tui`·`tui@…` hello를 거절하고 CLI 상태 조회를 계속 받는지 본다. 웹 E2E 01의 「옛 TUI WebSocket 거절·웹 API 유지」는 인증한 브라우저에서 옛 `/api/v1/front`에 접속해 분명한 Reject frame을 받고 일반 웹 API가 정상인지 본다. 웹 터미널·Ctrl+Enter·입출력 회귀는 기존 웹 E2E와 host·CLI 시험이 계속 맡는다.

TUI PTY·native clipboard·vendored crossterm 검사는 코드와 함께 제거했다. 목록과 보존 tag `tui-last`는 [삭제 기록](removed-tui.md)에 있다.

## 웹 입력 수신·Ctrl+Enter·사건 진단(t-0408)

- 웹 E2E 23은 실제 host·agent의 JSONL 읽기까지 지나, 같은 글의 옛 기록·다른 글을 수신으로 오인하지 않는지,
  전체 `pasted_content` 봉투가 있는 여러 줄 enqueue, 4천 자 표시 상한을 넘는 글, 표시 앞부분은 같고 끝이 다른 글,
  첨부·10초 경고·잠금 풀기·취소와 같은 보내기 ID의 진단 장부를 검사한다. 글 전문이 진단에 없는지도 본다.
- E2E 25와 host `ctrl_enter` keeper는 실제 Claude 2.1.303의 footer와 `paste again to expand` toast·spinner를 입력으로 쓰며,
  웹의 빈 입력 Ctrl+Enter가 host에 도착해 sendNow 키가 나오는지, 쉬거나 현재 상자를 모르면 키가 없는지 검사한다.
- `inputReceipt.test.mjs`·기존 `pastedContent.test.mjs`는 공용 전체 봉투 경계를 검사한다. host DB keeper
  `input_log_bounds_retention_and_queries_without_message_bodies`는 기간·행·20MiB 순환, 필터, 최신 1,000행의 순서를 본다.
  행 한도를 1 늘린 변형과 같은 ID의 별도 봉투를 걷는 변형에서 keeper가 실패한 뒤 바이트 단위로 복원됐다.
- 격리한 실제 Claude 2.1.303·Codex 0.163.0 pane에 로컬 합성 provider를 연결해 입력·대기·빈 입력 Ctrl+Enter·이미지 전달을 관찰했다.
  Claude는 소유한 FIFO의 실제 Bash 도구를 사건으로 기다리며, sendNow 뒤 enqueue가 흡수되고 다음 provider 요청이 오는 것을 본다.
  Codex는 pending steer의 Esc가 다음 요청을 시작하는 것을 본다. 고정 sleep·재시도·운영 설정·운영 pane 입력은 쓰지 않는다.
  이 실험은 CLI/PTY 수신 계약을 확인하며 실제 모델의 답변 품질을 검증하지 않는다.

조회 방법과 사건 해석은 [웹 입력 사건 보기](input-debug.md)에 있다.

Codex 실제 steering 화면에서 발견한 `<turn_aborted>` 내부 안내는 사람 글에서 제외한다. 기존 `chat::tests::codex_lines`에 실제 기록 형태를 더해 수정 전의 가짜 사람 말(RED)을 확인했고, 같은 keeper가 수리 뒤 통과한다.

E2E 23은 WebCrypto·randomUUID가 없는 브라우저에서도 짧은 글이 실제 pane에 가고 잠금이 풀리는지 검사한다. 새 hash·진단 ID 때문에 기존 일반 글 보내기를 막는 회귀를 수정 전 RED로 확인했다.

## 준비·정리 연결 유실 회귀(t-0412)

`cargo test -p aaa --test m13 preparation::`는 실제 Git의 응답을 socket 사건으로 붙잡는다. 준비·정리 각각 16초 창으로 예전 15초 상한을 실제로 넘긴다(총 약 35초). 연결 유실 시험은 Git이 자리를 만들거나 지운 뒤 응답 전에 실험실 host만 종료하고 새 host의 restart·resolve가 agent 기록을 회수하는지 본다. 별도 1초 host 설정 시험은 준비 deadline 전달·자기 Git 자식 종료·변경 파일 보존·깨끗한 자리 재사용을 본다. 시간 규칙을 보는 socket read 이외의 해제·종료는 사건으로 기다린다.

동일 keeper는 응답을 잃은 두 번째 준비를 restart 없이 바로 resolve하여 host가 agent journal의 경로를 회수하고 worktree가 실제 사라지는지도 본다. 이 확인은 journal 조회가 없던 후보에서 실제 고아 worktree로 먼저 실패했다.

구판 journal 없는 예상 자리가 남아 있으면 조회는 없음으로 확정하지 않고 명시적으로 거부한다. 변경 없는 자리는 안내한 `thread restart`가 소유권을 확인해 회수하고, 그 뒤 resolve하면 정리된다. 변경·다른 Git 신원은 기존대로 보존한다.

짧은 준비 설정 시험은 실패를 의도한 checkout 단계에만 1초를 적용한다. 그 뒤 실제 host 설정을 기본 600초로 되돌리고 실험실 host를 다시 띄운 뒤 정상 회수를 본다. 정상 회수의 Git 작업을 병렬 부하에 따른 1초 성능 시험으로 만들지 않는다.

## 가짜 harness로 agent 상태 만들기(§8.266)

- **새로 띄우는 pane.** 실행 파일 이름이 `claude`·`codex`면 그 harness 규칙으로 화면을 본다(harness `claudeSh`, 25·30의 node 가짜 harness). 대화 기록도 보여야 하면 가짜가 시작할 때 `aaa hook --agent claude`로 `SessionStart`(`transcript_path`)를 알리고, 기록 파일은 실험실 Claude 기록 폴더(`${root}/claude/projects/…`) 안에 둔다(밖이면 「기록 폴더가 없다」). Enter를 받으면 사람 말을 기록에 적어야 웹 보내기 확인이 끝난다(30).
- **이미 도는 셸 pane(thread의 fake profile).** agent는 셸 pane의 앞 프로세스 그룹이 바뀔 때만 앞 agent를 다시 본다(detect/foreground.rs, herdr 규칙). 같은 그룹의 자식(`sh 파일`)은 잡지 않으므로, post-probe 안의 셸이 `set -m`(job control)으로 이름이 `claude`인 script를 새 앞 job으로 띄운다(31).
- **알림이 이 화면에서 처리됐다.** 앱 안 알림(`.droplet`)이 DOM에 붙는 것을 MutationObserver로 세어 울타리로 쓴다. 창 focus는 `document.hasFocus`를 바꾸고 `blur`·`focus` 사건을 보낸다(19·31).

E2E30의 보내기 단계는 새 대화 DOM이 자동으로 끝에 간 직후 MutationObserver에서 위로 옮겨, 첫 native scroll 사건이 합쳐지는 순서를 고정한다. 자동 굴림 위치를 사건에서만 기억하던 제품 코드에서는 `5530 → 0` 이동을 놓쳐 「맨 아래로」가 숨었다. 즉시 실제 위치를 기억하는 수리 뒤 같은 keeper가 통과한다. 첫 사건을 기다려 경쟁을 피하거나 고정 대기·재시도를 넣지 않는다.

E2E17은 push 판정과 앱 안 알림을 확인한 뒤 화면을 정리한다. 알림은 3.6초 hold 뒤 저절로 접히므로, 이미 사라진 닫기 단추를 기다리지 않는다. 같은 브라우저 작업에서 현재 알림을 확인해 남아 있으면 닫고, 접힘·제거 사건으로 끝낸다. 첫 단계는 가상 시계로 자동 닫힘을 먼저 마쳐 조회 중 만료된 순서를 고정한다(실제 sleep 없음). 실제 사람의 카드·닫기 단추 조작은 E2E12가 계속 검증한다. 알림 개수·내용·push 허용/거부 단언이나 제품 수명은 바꾸지 않는다.

## Claude 내부 알림과 사람 글 경계(t-0408 후속)

`aaa-agent` chat의 origin keeper는 실제 Claude2.1.303의 user origin(task-notification·peer), 구조 표지(meta·compact summary·synthetic), human의 XML 인용과 수신 hash를 구별한다. 기존 pending keeper는 숨긴 알림의 remove/dequeue도 큐 자리를 유지하는지 확인한다. 웹 E2E23 「Claude 내부 알림」은 실제 host·agent가 같은 내용의 내부 알림으로 잠금을 풀지 않고, 뒤따른 human 기록에서만 풀며 휴대폰 말풍선에 그 인용 글을 남기는지 본다. JSONL의 뒤따른 assistant 기록을 처리 경계로 삼아 고정 대기를 쓰지 않는다.

## 빠른 답장과 공유 설정의 되돌리기(t-0413 실패·t-0408 후속)

preferences keeper는 전송 중 baseline 값으로 되돌린 설정·고정을 늦은 응답이 덮지 않고 다음 변경으로 남기는지 본다. E2E11 「대화 입력창」은 자동 전송 끔 요청의 실제 응답만 gate로 붙잡고, 다시 켠 뒤 해제해 다음 공유 쓰기를 확인하고 yes가 실제 프로그램에 도착하는지 본다. service worker가 route를 대신 처리하지 않도록 이 단계의 별도 기기만 serviceWorkers=block이다. 우연한 부하·반복 성공·기한 연장에 의존하지 않는다.

E2E14 「설정 되돌리기」는 host의 끔 events를 받은 뒤 HTTP 답만 유실시키고, 보관 문서를 반영할 때도 새 켬을 지켜 다음 켬 요청이 host에 적용되는지 본다.

최신 main의 auto-resolve keeper는 status=resolved가 정리 시작 표지이며 cleanup_pending 동안 group=working인 계약을 따른다. cleanup_pending이 사라진 사건까지 기다린 뒤 기존 group=resolved와 inbox 단언을 검사한다. 상태값 기대나 기한은 바꾸지 않는다.

## 「작업」 트리 작업 요약(t-0421, §8.280)

E2E28은 실제 host·agent의 pane→thread 대응을 거쳐, ⋯ 메뉴로 고정한 thread 줄(맨 앞 카드)도 폴더 안 줄과 같은 요약 카드를 여는지 본다. 「기타」의 thread 없는 일반 agent 줄에는 카드가 없고, 우클릭 메뉴를 열면 닫힌다. hover 지연은 브라우저 시계로 넘기며 고정 sleep은 없다. 휴대폰의 요약 단추·시트는 기존 단계가 맡는다.

## 「작업」 패널 통합(§8.280)

- 웹 유닛 `roster`: 프로젝트 안 coordinator → 작업 공간 폴더(이름순, 「작업 폴더」 끝) → 폴더 안 rank 차례, 임무 thread·그 pane 제외, 「폴더 안 정렬」 활동순. `workFolders`·`sidebarPanel`(옛 `projects`·`agents` 저장값 → `work`, 드러내기)도 같이 본다.
- E2E26은 막대 단추 셋(작업·머신·특수)과 「작업」 배지(확인 필요·검토 대기 thread + 답을 기다리는 agent pane의 합집합), E2E12는 프로젝트가 하나일 때 머리 없이 그리고 thread 줄 메뉴(보고·고정·프로젝트 thread 보기, §8.291부터 「모든 thread」)와 프로젝트가 여럿일 때 프로젝트 머리 안 폴더 들여쓰기, E2E11은 줄 끝 배경 작업 수·계획 n/m, E2E27은 머신 칸 agent 수가 트리의 도는 agent 줄 수와 같음을 본다.

## 한 host 한 프로젝트 시작 검사(§8.283)

- 유닛 `lifecycle::tests`: 보관하지 않은 프로젝트 0개(없음)·1개(그 이름)·멈춘 것은 셈·보관 섞임·2개(목록)와 거부 말.
- m13 `host_starts_only_with_one_project`: 보관한 둘째 프로젝트는 세지 않고 다시 뜬다. 보관을 풀고 다시 띄우면 host가 입구를 열기 전에 목록을 말하고 0이 아닌 코드로 끝난다. 폴더를 `.trash`로 옮기면 다시 뜬다. 시작 검사를 경고로 바꾼 결함을 심어 이 시험이 실패하는 것을 확인했다.
- 실험실(`tests/lab`)은 `[projects]`가 없는 host.toml에 실험실 root를 채운다. 그래서 시험 host가 실제 HOME의 프로젝트를 세지 않는다.

## 프로젝트 인자 없는 CLI(§8.285)

- m13 `cli_takes_no_project_argument`:
  - 인자 없이 `thread start`·`show`·`keys`, `inbox list`, `context`, `aaa set`, `aaa coordinator open`.
  - 옛 꼴은 같은 이름이면 표준 오류 경고와 같은 결과, 다른 이름이면 거부한다.
  - 다른 이름 거부를 뺀 결함을 심어 실패를 확인했다.
- 둘째 프로젝트 거부: m13 `coordinator_flow`이 거부 말을 보고, 폴더가 남지 않는지도 본다.
- 다른 프로젝트를 전제하던 시험은 한 프로젝트 뜻으로 바꿨다.
  - host 유닛: post에서 다른 프로젝트의 coordinator 줄을 뺐다. inbox는 한 프로젝트로 줄였고, pr은 경우마다 host를 하나씩 띄운다.
  - m13 7개: 옵션은 빈 host에서 보고, 한살이는 demo를 보관한 사이에 본다. safety·coordinator profile은 같은 프로젝트의 값을 바꿔 가며 본다. doctor는 실험실의 demo를 새 프로젝트로 쓴다.
  - E2E 12: 멈춤·빈 결과물·임무·coordinator 줄·작업 공간 폴더를 demo로 옮기고, 프로젝트 머리 셈 단계는 뺐다.
  - E2E 21: 보고서를 demo 하나에 두고 프로젝트 고르기 단계를 뺐다.
- 같은 초 삭제: 유닛 `deleting_the_same_name_twice_keeps_both_in_the_trash`. 겹침 검사를 뺀 결함을 심어 실패를 확인했다.

## 요청의 프로젝트 생략·safety 표 하나·aaa.toml(§8.288)

- host 유닛 `omitted_project_is_the_one_this_host_keeps`:
  - `{"op":"context"}`가 빈 `project`로 풀린다.
  - 프로젝트가 없을 때는 NotFound이고 `Instance`가 비어 있다.
  - 만든 뒤에는 `demo`로 채우고, 이름을 주면 그 이름을 쓴다.
  - 채우기를 뺀 결함을 심어 실패를 확인했다.
- m13 `post_from_thread_reaches_coordinator_inbox`:
  - coordinator 셸 pane의 `post user`(새 꼴)와 `post demo user`(옛 꼴, 경고 한 줄)가 모두 coordinator로 접수된다.
  - thread pane의 post는 여전히 `peer+token`이다.
  - `post_sender_is_judged_not_claimed`: 세션 env를 지운 thread pane의 post는 프로젝트를 채운 뒤 front 보기 권한으로 거부된다. thread가 아닌 셸 pane은 Front 길에서 목적지로, Front에 붙지 못한 hook 길에서 「열린 thread 세션에서만」으로 거부된다.
  - JSON 출력을 표준 오류와 함께 읽는 시험(m13 `coordinator_user_posts`, E2E harness의 coordinator 완료 post)은 새 꼴을 쓴다. 옛 꼴의 경고 줄이 JSON 앞에 붙기 때문이다.
  - 세션 안 Front 길을 옛 조건(프로젝트를 줄 때만)으로 되돌린 결함을 심으면 새 꼴이 hook 길로 가서 거부된다.
- safety:
  - 유닛 `one_safety_table_comes_before_the_project_table`: 읽기 차례, 버린 값·모르는 key의 경고, `--global`에는 닿지 않음.
  - 설정 유닛 `one_safety_table_mixes_values_and_tables`: 값과 표가 섞인 `[safety]`를 읽고, local이 덮고, 다시 써도 같고, 틀린 값은 그 줄을 가리킨다. 사용자 정의 읽기를 뺀 결함을 심어 실패를 확인했다.
  - m13 `safety_and_profiles_by_a_person`: 대상 없이 show·yolo·set을 쓰고, 옛 꼴 `safety set demo …`도 쓴다. `[safety] yolo`가 프로젝트 표를 덮고 그렇다고 알리는지 본다.
  - `coordinator_deployment_is_gated_and_survives_backend_restarts`도 대상 없는 `safety set coordinator_deploy on`을 쓴다.
- aaa.toml: 유닛 `aaa_toml_comes_first_and_keeps_workspaces`.
  - aaa.toml이 없으면 만들지 않는다.
  - 있으면 그것을 읽고, `[[workspaces]]`에 더하고 뺀다(`workspaces.remove`). project.toml은 그대로다.
  - 옛 이름 `[[repos]]`가 있는 글은 그 목록을 고친다.
  - 쓰기 자리를 project.toml로 되돌린 결함을 심어 실패를 확인했다.
- 안내: m13 `coordinator_flow`·`doctor_files_brings_the_coordinator_guide_to_existing_projects`·`project_lifecycle`이 `aaa post user`와 남은 `aaa project sweep <p>`를 본다.

## 웹의 프로젝트 층 없애기(§8.291)

- E2E 12:
  - 「끝난 thread 보기」: 더 보기 → 모든 thread(`#threads`)로 간다. 묶음·거르기를 보고, 고르면 짧은 주소 `#thread=t-0001`이 된다. 뒤로 이름은 「모든 thread로」이고 뒤로 가면 `#threads`다. 옛 `#projects`·`#project=demo`도 그 화면을 열고 주소가 `#threads`로 바뀐다.
  - 옛 주소 바꾸기를 뺀 결함을 심어(웹을 다시 빌드하고 host binary도 다시 만든 뒤) 이 단계가 실패하는 것을 확인했다.
  - 줄 메뉴: thread 줄 메뉴는 「보고·고정·모든 thread」다. 짧은 주소 `#thread=<id>`로도 보고를 연다.
  - 멈춤은 모든 thread 머리 둘째 줄의 「멈춤」으로 본다. coordinator 열기는 모든 thread 머리에서 하고, 뒤로 가기는 `#threads`다.
  - push: coordinator 완료 push URL은 `/#threads`다.
- E2E 15: 모든 thread 화면의 ≡ 자리. E2E 19: post 음소거 앱 안 알림은 `#threads`로 간다. E2E 21: 보고서 화면에 프로젝트 고르기가 없다.
- host 유닛: push `payload`의 thread URL `/#thread=t-0001`, post 알림 URL `/#threads`.
- 낱말: brief 유닛(`# aaa`·`- 이름:`·memory 넘침 말), 임무 brief 유닛(「aaa 홈 폴더를 탐색하지 않는다」), m13 `worktree_thread_round_trip`·`memory_and_restart`(`# 지침`)·`coordinator_flow`·`doctor_files_brings_the_coordinator_guide_to_existing_projects`(`## 첫 턴`).

## E2E03 보낸 순서는 화면 안 기록으로

E2E03 「크기 되찾기」의 보낸 순서(resize가 키보다 먼저)는 화면 안 기록(`sentFrames`, init script)으로 읽는다. 예전에는 화면을 연 뒤 Playwright `framesent`를 달았다. 그래서 그 전에 열린 pane WebSocket을 놓치면 「보낸 순서 []」로 실패했다(ci-local에서 한 번). 예전 판에 2초 지연을 넣어 같은 실패를 재현했고, 고친 판은 같은 지연에서 통과했다.

## CI 감시(§8.274)

- `cargo test -p aaa-common ci::`: gh 304 exit 1·200/403 HTTP 해석, GET·머리 주입 거부, 응답 필드 축약, SHA의 일부 workflow 대기·취소·재실행·stuck, 실패 집합·병합 번호 계약을 본다.
- `cargo test -p aaa --test m13 ci_conditional -- --nocapture`: 실제 host·agent와 가짜 gh, 승인된 임무를 띄운다. 기준 초록 → 304 무음 → 빨강의 CI 항목·임무 inbox와 깨움 한 번 → host 재시작 중복 방지·created pagination → 같은 실패 집합 무깨움 → 꺼진 동안 실패 후 회복·CI fixed 무깨움 → 403 outage 한 건을 본다. 로그 잘림과 한 push의 여러 주인 번호도 이 경계에서 본다.
- 실험실의 기존 `AAA_TEST_ROUTINE_SECONDS=1`이 있을 때만 `ci_poll_secs` 하한을 1초로 낮춘다(운영은 30초). 고정 sleep을 넣지 않는다. 새 판정은 flow, 아무 일 없음은 다음 304 두 번(앞 CI pass 완료)과 ticker tick 뒤 확인한다.
- 결함 심기는 HTTP 상태보다 exit를 먼저 거부하기, 같은 실패 집합의 깨움 억제를 제거하기, 회복 자체가 임무를 깨우게 만들기 세 가지를 독립적으로 심는다. 해당 keeper가 의도한 이유로 실패한 로그를 보존하고 바이트 단위 복구 후 정상 통과를 확인한다.
- 운영 host·로그인·설정·실제 GitHub 쓰기는 건드리지 않는다. 배포 뒤 사용량·깨움 수 관찰은 coordinator의 별도 운영 단계다.

## 공용 watch 추출(t-0420 L1)

- 기존 `cargo test -p aaa-host ci:: --lib`와 `cargo test -p aaa --test m13 ci_conditional -- --nocapture`를 그대로 실행한다. E2E의 CI 사건·재시작·같은 실패·회복·outage 단언은 바꾸지 않는다.
- `l0_saved_state_survives_watch_extraction`은 L0 저장 문서 fixture를 실제 DB에 넣고 새 상태로 읽고 쓴 JSON 의미가 같은지 본다. cursor·SHA 판정 이력·미전달 outbox를 보존하고, 임무가 없을 때도 저장 입력이 남는지 확인한다.
- 결함은 각각 독립적으로 ① 프로세스 첫 조회의 따라잡기 누락 ② 전달 입력의 내구 key를 매번 바꿈 ③ source 상태의 최상위 펼침 누락을 심는다. 최근 poll 직후 새 host의 따라잡기 단위·pending 중복 단위·L0 저장 문서 keeper의 RED를 보존하고 바이트 복구 후 정상 gate를 실행한다.
- 첫 조회 표지 제거 변형은 기존 E2E에서 재시작 시간 자체가 단절 창을 넘어 통과했다. 이 감도 부족을 보존하고, 같은 DB의 새 host가 poll 1ms 뒤에도 따라잡는 keeper를 추가했다.
- 전체 gate는 `bash scripts/ci-local.sh`와 `bash scripts/check-public.sh`다. L1은 CI 동작·설정 변경 없이 구조만 추출한다.

## 배포 자동화 L0·L1(§8.276)

- host `rollout::tests::transition_receipts_require_success_hash_and_new_process`는 단계 전이 표, 실패·중단 중지, 영수증 ID/요청/소속, SHA·판과 새 프로세스 판정의 owner다.
- macOS m13 `rollout_preflights_all_targets_and_resumes_same_jobs_after_host_restart`는 격리 사본 host·TLS agent 둘과 기록하는 launchctl로 승인 거부, 사전 SHA 불일치 무교체, 모든 대상 dry-run 뒤 host부터 순차 실행, 실제 host/agent 프로세스 재시작 뒤 같은 job ID 유지, 안내 갱신·coordinator pane PID 지속을 확인한다. 서비스 호출 기록·연결 사건·출력 표지로 끝내며 고정 sleep은 없다. 실제 운영 launchd에 닿지 않는다.
- `python3 -m unittest scripts.test_deploy_build`는 임시 origin/main과 기록하는 빌드 도구로 조상 거부·격리·빌드 1회·salt·provenance를 확인한다. Unix 공식 ci-local test 단계에 포함한다.
- 기존 coordinator_deployment 시험이 worker·위장 coordinator·safety 철회·원시 rollback 권한을 계속 소유한다. L3의 실패 머신 자동 rollback과 host 감시는 이 단계에서 검증했다고 주장하지 않는다.
- 결함 심기는 재개 ID 변경, SHA 대조 누락, 실패 뒤 진행을 각각 독립 실행해 keeper 실패를 보존한 뒤 원복한다. 전체 검증은 scripts/ci-local.sh와 check-public.sh다.

- host 재시작 직후 로컬 agent의 pane 자료보다 CLI가 먼저 붙는 RED를 보존했다. wait 전용 handshake는 host가 local hello의 pane 자료를 기다린 뒤 peer PID를 판정한다. m13은 host 재시작 때 local agent도 잠시 내려 CLI의 대기 진입 사건을 확인하고 hello를 보내 이 순서를 고정한다. Forbidden은 즉시 종료한다. 기존 연결의 권한을 올리거나 세션 자기 신고를 믿지 않는다.

- 검토 회귀: release manifest keeper는 `x.y.z+커밋`에서 최신/명시 공개판 선택을 확인한다. m13은 후보가 staging self-check 도중 원본을 바꾸는 fixture에서 미검증 원본의 실행 표지가 없고 운영 사본이 그대로임을 확인한다. CheckedUpdate는 staging 판/SHA 영수증을 반환하며 원본 경로를 재실행하지 않는다.

## 요약 상세 창의 지시 전문(10-19 사용자 후속)

host summary keeper와 기존 웹 E2E 28이 긴 여러 줄 지시의 끝까지 API/화면에 남는지 확인한다. 상세 창은 줄바꿈·긴 토큰을 감싸고 전체를 스크롤해 읽는다. hover 카드의 짧은 줄 규칙과 최근 지시 개수는 유지한다.

## 설치 앱 아이콘 배지

`app_badge::tests::counts_shared_unseen_and_open_decisions_once_per_pane`는 host 정본의 보고 열람·미처리 요청·질문/thread 중복 제거를 본다. `web/test/appBadge.test.mjs`는 실제 숫자 전달·0 지우기·미지원/거부와 창 없는 worker push(최신 host 수·offline payload·로그아웃)를 본다. `33-app-badge.mjs`는 실제 host·agent의 보고 도착 1 → 다른 기기 열람 0 → 확인 필요 증가 → resolve 감소를 Chromium·WebKit의 가짜 Badging API 기록으로 기다리고, 허용된 coordinator push의 암호문을 독립적으로 풀어 같은 수인지 본다. OS 배지 모양은 [실기기 절차](app-badging.md)를 따른다.

## Git 감시와 thread 없는 사건 repo(t-0420 L2)

- `cargo test -p aaa-common git_watch::`: branch 옵션/패턴 주입 거부·정확한 ls-remote/삭제 해석·head 변화 종류를 본다. host `git_watch::tests`는 문서 예시와 잘못 섞은 명령/일정/사건을 검증한다.
- `cargo test -p aaa --test m13 git_push -- --nocapture`: 실제 host·로컬/원격 agent·로컬 bare 원격·다른 clone의 push를 사용한다. 기준/무변화 무음 → 프로젝트 사건/inbox 한 번·repo로 거른 임무 깨움 한 번 → 재시작 무음 → force·같은 SHA 전이의 재발·삭제 → 실제 ls-remote 연속 실패의 outage 한 건을 본다. HEAD/FETCH_HEAD 보존도 확인한다.
- 고정 sleep 대신 실제 DB의 새 poll commit과 Git trace2의 실패한 ls-remote 종료 두 번을 기다린다. 최초 시험의 잘못된 역할 DB 경로는 원인을 확인해 고쳤으며 첫 로그를 보존한다.
- 결함은 재시작 때 저장 head 무시(중복 사건), thread 없는 사건 repo fallback 제거(임무 깨움 없음), force를 보통 push로 판정(종류 틀림)을 각각 심고 해당 keeper의 RED 뒤 바이트 복구 PASS를 확인한다. 기존 CI E2E도 그대로 통과해야 한다.

- `inactive_watch_sources_release_tick_deadlines`는 실제 CI/Git 파일을 켠 뒤 한 source만 끄기·둘 다 끄기·프로젝트 삭제를 적용한다. 비활성 예약은 없어지고 다른 활성 source 예약은 남아야 한다. 수리 전 비활성 Git 예약 단언의 RED를 보존한다. 고정 sleep·CPU 임계값을 쓰지 않는다.

## 머신별 작업 디스크 L0 (t-0434)

`aaa-config`의 작업 경로 keeper는 설정 덮어쓰기·HOME 확장·기존 기본값과 잘못된 경로의 설정 오류를 본다.
`aaa-agent`의 `work_root` keeper는 없는 mount와 동명의 일반 폴더를 실제 파일시스템에 만들고, preflight·health·routine이
대체 작업 폴더/내장 여유를 만들거나 보고하지 않는지 확인한다. 새 m13 `work_root::agent_refuses_an_unmounted_work_volume_before_creating_a_workspace`는
CLI가 읽은 설정으로 실제 agent가 작업 폴더를 만들기 전에 실패하는 연결을 확인한다. subprocess 종료가 종료 사건이다.
운영 mount·서비스·전역 설정에는 닿지 않고 새 sleep·신호 시험·production 시험 전용 flag는 없다.
설정의 잘못된 경로 거부를 제거한 변형은 config keeper가 상대 경로 수용으로 실패했다. mount를 디렉터리 존재만으로
통과시키는 변형은 agent keeper가 대체 폴더 생성으로 실패했고, m13은 agent가 거부하지 않고 계속 실행되어 종료 사건
시한에서 실패했다. 각 변형의 소스를 바이트 단위로 복원한 뒤 정상 gate를 실행한다.
agent 시작이 작업 root의 쓰기도 점검하므로 공용 Rust 실험실은 기본 agent.toml에도 자기 작업 root를 명시한다.

## 자동화 기록·목록(t-0420 L3)

- proto `routine_view_reads_previous_front_response`는 기록 필드가 없는 N-1 host 응답을 읽는다. 누락 trigger의 실제 RED 뒤 기본값 읽기를 넣었으며 웹은 없는 기록 배열을 빈 것으로 그린다.
- host `routine_runs::tests::durable_bounded_history_keeps_daily_total_and_delivery_identity`는 실제 DB meta를 재오픈해 50건 상한·순서·동일 전달 중복 집계 방지·고리 밖 하루 합계·날짜 전환을 확인한다. 기존 CI outbox replay 시험이 없는 임무→입력 전달→재전달에도 기록 한 건/오늘 한 번을 확인한다.
- 기존 m13 `routines`가 실제 coordinator 없음·미처리 안내·명령 승인 필요 기록을 CLI에서 읽는다. CI E2E는 같은 실패 집합 까닭과 재시작 뒤 기록 불변을, Git E2E는 push 한 건 기록과 재시작 뒤 중복 없음을 확인한다.
- 웹 E2E 34는 실제 일정 판정의 건너뜀을 목록/최근 기록에서 보고, 임무 조건 출처·승인 전 임무의 승인 필요 표시·enabled만 토글(주석/일정/본문 유지)·Reader 서버 차단·390px drawer 폭을 확인한다. 고정 sleep 없이 CLI/DOM 사건으로 끝낸다.
- 결함 검증은 고리 상한 제거·실제 건너뜀 기록 제거·웹 토글 방향 고정을 각각 RED로 확인한 뒤 원본 복구·PASS를 요구한다. 최종 ci-local은 모든 복구 후 실행한다.

### 임무 확인창 막힘 복귀(L3 추가)

- 실제 host Lab의 Question→Working이 기존 코드에서 계속 막힘인 RED를 먼저 보존한다. 수리 뒤 Question→Working/Idle·입력 없는 Idle 무깨움·누적 입력 한 번 전달·DB/API 이유 해제를 확인한다.
- message_unknown·이전 host 깨움·brief 모름·restart 실패·이유 없는 옛 막힘은 Question/Working/Idle과 pause/resume 뒤에도 입력을 보존한다. 명시 restart는 이유를 지운다. Agent 응답을 기다리는 두 경계(상태 조회·입력 파일 쓰기)에서 실제 message_unknown flow를 넣어 merge/깨움 commit이 이를 덮지 않는지 본다.
- m13 `mission_question_answer_recovers_and_delivers_accumulated_input_once`는 승인한 임무의 실제 가짜 Claude 신뢰/권한 질문에 답하고, 확인창 중 routine 명령으로 쌓인 내구 입력이 Idle 뒤 한 번만 전달되는지 본다. 고정 sleep 없이 질문·상태·출력·tick 사건으로 끝낸다. 기존 restart/uncertain-wake 시험도 유지한다.
- 확인창 복귀 분기 제거와 전달 모름까지 자동 해제하는 변형은 각각 해당 회귀가 실패해야 한다. 웹 상태 말은 기존 specialModel 계약에서 모든 이유를 검증한다.

L3의 활동 막대 E2E26은 패널 개수를 고정하지 않고 기억한 머신 단추만 Tab 정지점인지 확인한다. PR 목록 E2E는 새 조건 → 동작·지난 결과·다음·오늘·건너뜀·출처 칼럼을 확인한다.

## Agent link 후보 전송 L2(§8.282)

- `cargo test -p aaa-agent deploy::stage::tests --lib`: 실제 파일/기존 Executor 경계에서 빌드 폴더·provenance·일반파일/symlink·조각 상한, EOF SHA 전에 실행 금지, offset 오류와 agent 객체 재생성 뒤 재개, 설치 시 프로젝트/ID/변조 거부, 최신 후보 보관·잘못된 keep 정리 무삭제를 확인한다. 새 test-only production seam은 없다.
- `cargo test -p aaa-host rollout::tests --lib`: 기존 전이 keeper에 모든 Transfer/Preflight 뒤 첫 교체, stage 영수증의 소속·SHA·완료 크기·판, UpdateStaged 해석을 더한다.
- 기존 m13 rollout keeper를 L2 정상 경로로 확장한다. 실제 TLS agent 앞의 시험 소유 TCP proxy가 다음 source 응답bytes를 channel 사건으로 붙잡은 동안 실제 저장 조각과 pane 입력/출력을 확인하고 host를 내린다. gate를 풀고 같은 rollout/job ID로 이어간다. 전송 뒤 원본을 치워도 상태 폴더 후보만으로 전체 교체가 끝나야 한다. --from의 SHA 실패/원본 변조/둘째 머신 실패 단언은 남긴다. 시간은 proxy gate부터 전체전송/사전검사, pane 왕복을 로그에 기록한다.
- 결함 심기는 재개 새ID·SHA누락·실패무시 필수3개와 전송 offset/EOF SHA/정리 보호를 각 owner 경계에서 확인하고 바이트 원복한다. 최종 gate는 ci-local 전체와 check-public이다. 운영 배포·실제 launchd는 건드리지 않는다.

## 머신별 작업 디스크 L1 (t-0434)

`cargo test -p aaa-agent recovery_tests --lib -- --nocapture`는 실제 Git을 쓰는 두 owner다.
구판 journal 가져오기·원본/정본 보존, A→B 뒤 생성 root의 placement/restart, 옛 mount 부재 거부,
전환 중 준비 거부·root 소유권을 본다. 별도 계약으로 잠긴 worktree 경로를 잠깐 옮긴 뒤 실제 Git prune에도
등록이 남는지, dirty/uncopied/사람 잠금 보존, 중단한 unlock의 재시작 복구와 대상별 remove를 본다.
임시 저장소에서만 실행하며 실제 mount나 서비스는 바꾸지 않는다. 두 owner는 약 1초다.

기존 m13 준비 keeper는 agent 상태 폴더의 journal을 읽는다. 연결 단절·15초 경계·dirty 거부 단언은 유지한다.
`interrupted_placement_and_resolve_resume_from_agent_records`는 준비 복구 뒤 같은 state의 agent를 B root로
다시 띄우고, 기존 A의 resolve 응답 유실·재시작 회수와 새 thread의 B 경로까지 확인한다. 연결/Git/종료 사건으로 기다린다.

결함은 생성 root를 현재 root로 재계산, Git 잠금 생략, 넓은 prune 재도입, 재시작 relock 누락,
사람 잠금 해제, 전환의 공유 잠금 생략을 각각 심어 해당 owner의 assertion RED를 확인했다.
각 소스를 바이트 단위로 복원한 뒤 정상 두 owner PASS를 확인했다. 전체 gate는 ci-local과 check-public이다.


## 머신별 작업 디스크 L2 (t-0434)

- `cargo test -p aaa --test m13 work_root:: -- --nocapture`: 실제 CLI의 mount 인자·시작 거부, 주석/타 설정 보존,
  멱등 set, 마지막 시작값과 다음 설정값 구분, 재시작 전후 새 thread 위치, 옛 pane PID·cwd 보존·resolve·sweep을 본다.
  별도 node는 구판 journal 가져오기와 실제 RootTransaction 등록 완료/config rename 전의 중단 checkpoint에서 이어 하기,
  읽기 전용·symlink·hardlink·잘못된 TOML 무교체를 확인한다.
- `cargo test -p aaa --bin aaa work_root::tests -- --nocapture`: 설정 snapshot 뒤의 외부 편집을 owner에서 만들어
  마지막 rename으로 다른 편집을 덮지 않는지 본다. race를 위한 production hook은 없다.
- L1의 실제 Git owner 둘은 그대로 실행한다. 새 마지막 시작 mount 필드는 구판 상태에서 기본값으로 읽는다.
- macOS 실제 두 mount 확인은 격리 APFS sparse image와 Data 볼륨의 별도 config/state/host/agent/repo만 사용한다.
  정상 전환 → 실제 detach 뒤 새 작업/정리/시작 거부·내장 대체 없음·Git lock 보존 → remount 뒤 명시 resolve 재개를 본다.
  운영 경로·서비스·로그인에는 닿지 않으며 소유한 image/device/프로세스만 정리한다.

L2 결함 검증은 이전 root 가져오기 생략, 재시작 전 applied 변경, 값 주석 유실, readonly 교체 허용,
동시 편집 덮기, 명시 mount 무시, JSON 오류 출력 누락을 각각 심어 owner assertion RED를 확인한다.
모든 소스를 바이트 단위로 복원하고 정상 CLI·snapshot keeper를 통과한 뒤 전체 ci-local/check-public을 실행한다.

## CLI 시작 스택 (t-0446)

- `cargo test -p aaa --test resource_limits cli_parses_and_dispatches_with_a_small_main_stack`는
  Unix 자식 셸의 main 스택을 768KiB로 낮추고 실제 aaa로 exec한다. 도움말의 정상 종료, 삭제된 TUI 명령의
  오류 처리, bash 완성 스크립트 생성까지 확인한다. 부모·운영 프로세스의 한도는 바꾸지 않는다.
- clap derive의 debug 명령 트리 생성은 중첩 프레임이 크다. 생성·파싱만 4MiB thread에서 끝내고
  실제 명령은 원래 main thread에서 실행한다. Box로 future나 CLI enum을 줄여도 이 빌더 프레임은 줄지 않는다.
- macOS에서 수정 전 도움말·완성 생성의 stack overflow와 수정 후 통과를 확인했다. 이 시험은 Windows 실행을
  대신하지 않는다. Windows의 codex_integration·smoke·tui_removed 결과는 GitHub Windows runner에서 확인한다.

## 실패 머신 rollback과 독립 복구 감시 L3(§8.289)

- agent guard policy 두 keeper는 새 실행 판/SHA/instance/시작시각, Activate/Recover 재전달의 deadline·ID 보존, unchanged 취소와 설치 중 취소/expiry, 복구 요청 뒤 old 프로세스와 로컬 host+agent 둘 확인을 소유한다. 제품 timer의 now 경계값으로 무응답120초를 확인해 실제 sleep으로 기다리지 않는다.
- proto `previous_rollout_records_keep_their_non_recovery_policy`는 N-1 JSON의 자동복구 off와 같은 ID를 확인한다. host `recovery_receipts_keep_the_failed_cursor_and_durable_busy_state`는 실제 DB 재오픈 뒤 Recovering의 busy와 terminal 실패 cursor/앞 완료 단계/영수증 보존을 확인한다.
- update `pinned_rollback_ignores_newer_backups_and_never_executes_the_broken_installation`은 실제 파일/실행 표지로 실패 설치·다른 최신 보관본 무실행, 정확 baseline SHA·멱등성, 일반 보관본 삭제 뒤 private pin 복구를 확인한다.
- 기존 macOS m13 rollout keeper는 정상 L2 경계 뒤 unchanged SHA 실패의 무재시작을 유지한다. remote가 옛 바이너리로 잘못 돌아오면 독립 PID가 살아 복구하고, 복구 중 host를 다시 띄워도 같은 IDs/실패 머신만 유지한다. host 실패는 host만 복구하며, 로컬 agent 실패는 공유 파일의 host/agent 둘을 복구한다. 복구 전 뜬 옛 프로세스는 완료로 세지 않는다. private token을 아는 다른 peer PID와 bootstrap 없는 내부 CLI도 거부한다.
- 실제 설치 config check는 시험 소유 Unix socket으로 멈춘다. safety 철회→Recovering→gate 해제→늦은 rename 뒤 baseline 복구→새 old host를 확인한다. self-check30초·primary120초와 기존30초 사건 대기 상한을 늘리지 않는다. 독립 guard는 기록 PID와 실제 실험실 명령 경로를 함께 확인해 Lab이 회수한다.
- 새 복구 ID·실행 SHA 누락·실패 뒤 다음 단계 진행의 필수 변형과 설치 중 취소/새 프로세스 확인 변형을 RED로 확인하고 정확 bytes 복구 뒤 GREEN/전체 ci-local·check-public을 실행한다. 독립 최종 read-only 리뷰는 같은 후보에서 한다.


## 순수 TOML·공통 자동화(L4)

- host automation keeper는 옛/새 형식의 같은 조건·동작·명령 승인 값, 동명 양쪽 차단, 잘못된 키/둘 이상의 조건·글 칸, Unicode 위치 오류, 최종/중간 symlink 거부를 확인한다. 스키마는 Rust 생성값과 고정 산출물을 비교한다.
- 변환은 긴 글·원본 보존·임무 조건 분리·동등성·이름 충돌 전체 무변경·재실행 무음을 확인한다. 실제 host 변환은 옛 mission cursor를 새 조건에 넘기고 변환 뒤 사건을 한 번 받는지 본다. 예산/간격/usage keeper와 실제 DB 재오픈은 pending·하루 횟수 보존을 검사한다. SQLite 저장 실패는 source hash/cursor나 통지 표식만 먼저 전진시키지 않아야 한다.
- 기존 routine·PR·CI/Git·임무 m13을 유지한다. source에서 공통 큐로 인계한 뒤 실제 action pass의 자료 보존·한 번 전달을 관찰한다. CI 회복이 지연된 실패 대기를 취소하는 회귀를 포함한다. 웹 E2E34는 CLI 변환→실제 목록 출처 변경·새 파일 enabled만 편집·Reader의 변환 차단도 확인한다.
- 운영 상한 안내 회귀는 실제 mission_tick→12/12 안내→done→논리90/180/270초 tick→완료 기록 정리→DB 재오픈을 거쳐 같은 날 무안내를 확인한다. 다음 날 예산 초기화 때 무안내, 다시 상한 도달 때 한 번, 구판 완료 항목 이행도 검사한다. 임무가 아닌 helper 호출만으로 반복 결함을 검사하지 않는다.
- 결함 심기는 당일 표식 제거와 알림보다 표식 먼저 쓰기를 독립 적용해 각각 done 뒤 1→2와 DB 실패 확정 단언 RED를 확인하고 원본 bytes/mtime 복구 뒤 PASS한다. 다른 변형도 승인 정규화·동명 차단·공통 상한·변환 인계 경계에서 기존 keeper가 실제 실패해야 한다. 원래 실패 증거를 보존하고 최종 ci-local·check-public은 복구 뒤 수행한다.


### L4 기본 제한 없음(사용자23:3x)

- 생략/0인 임무와 공통 규칙이 기본12/20 상한으로 막히는 첫 RED를 보존한 뒤 무제한을 확인한다. 숫자0 최소 간격도 받는다. 명시한 옛 양수 상한·사용량 대기는 유지한다.
- 실제 임무를 상한 대기·기존 inbox51건으로 만들고 파일의 상한/간격을 없앤다. repo 재승인 없이 같은 tick에 보존 입력 한 번 깨움, 2시간 뒤 시간/철수 안내 없음, 기본 host 동시 상한 없이 승인 임무3개 시작을 검사한다. 낮은 frame의 text+Enter는 한 번 깨움의 두 요청이므로 고정 깨움 본문을 센다.
- 이전 예산 증가 승인 거부 기대는 새 사용자 원칙으로 바꾼다. 실제 repo 경로 변경의 승인 거부·명시 거절 상태에서 재시작 전 Kill 없음은 유지한다. host max_pending을 공유하던 사후 평가자의 0도 무제한으로 읽어, 설정 변경이 전달을 전부 막지 않는지 기존 회귀로 확인한다.


### L4 운영 reload·임무 권한 후속

- 명시 하루 상한1/1에서도 승인된 동일 상한 새 본문의 reload가 Kill→종료 확인→새 후보 적용으로 이어지는지 본다. 수리 전 cap의 앞선 return으로 Kill0(기대1) RED를 보존한다. Working 때는 적용 예약과 이유를 즉시 보이고, 실제 작업 종료·안전한 Idle 전에는 Kill/새 대화가 없어야 한다. 예약 뒤 다른 파일 판·pause/확인창/초안/보는 화면도 구분한다.
- 사용자의23:5x 결정에 따라 이전 manual 강제 기대를 교체한다. 기존 host 승인 start와 reset frame 시험은 실제 ThreadPrepare/ThreadRebrief의 profile args가 일반 thread safety 결과와 같은지 검사한다. agent launch keeper는 Claude/Codex 각각 yolo off/on, launch/resume argv, hook/자기 문맥/일반 thread 분리/링크 쓰기 거부를 유지한다. 강제 manual 또는 bypass 차단이 남아 있으면 RED여야 한다.
- 운영 파일·승인 기록·현재 pane은 사용하지 않는다. 가짜 frame owner와 임시 작업 폴더에서만 인자를 확인한다.


- 자정 경계 회귀: 기존 mission_debounce 예산 시험은 23:59대 now+60001이 다음 날짜가 되어 같은 날 상한 기대가 실패했다. 첫 로그를 보존하고 그 시험의 논리 시각·reset 기준을 다음 날 정오에 고정해 같은 예산 날짜의 간격·범위·상한을 검사한다. 날짜 초기화는 별도 notice/day keeper가 계속 소유한다. 기대값이나 시한을 늘리지 않는다.
- 과거 최소 execpolicy 검증은 manual 모드의 증거다. 사용자23:5x 뒤에는 추가 allow-list와 실행 문맥만 유지하고 실제 승인/sandbox 모드는 일반 thread safety 결과가 정본이다. 임무만 push가 harness에서 막힌다고 가정하지 않으며 금지는 임무 지침에 남는다.


### L4 재개·시간 상한·무제한 표시 후속

- `resumed_mission_time_budget_counts_work_in_the_wake_not_elapsed_wall_time`은 실제 host·DB와 frame owner에서 누적19분/옛 깨움2시간/오늘 깨움0 재개를 검사한다. 경과 시간식은 첫 무알림 단언 RED여야 한다. 관찰 작업59분 무음→멈춤 시간 제외→61분 한 번 안내→done 뒤 무반복과 별도 실제 새 입력의 깨움별 셈 초기화를 검사한다. 실제 input hook과 논리 시각을 섞지 않도록 새 전달 사례는 새 임무의 실제 시각에서 시작한다.
- 기존 reload keeper에 pause 상태의 reload/restart 예약과 멈춤 이유, 무Kill/무Spawn, resume 뒤 동일 후보 적용을 추가한다. 기존 상한 제거 keeper는 resume 전에 상한0 반영과 pause 보존을 함께 검사한다.
- 기존 변환 인계 keeper는 변환 직후 첫 새 source 판정 전에 사건을 넣어 실제 전달 한 번·과거 무재전달을 검사한다. DB 표식 유무만으로 통과시키지 않는다. 옛 mission 사건 조건을 함께 실행하거나 새 cursor seed를 제거하면 실패해야 한다.
- 웹12의 실제 특수 패널은 생략·옛 최대 정수·상한3/60을 읽어 무제한 분모 없음과 명시 N/M을 검사한다. 실제 CLI m13 임무의 list/show도 같은 규칙을 검사한다. 큰 수를 무제한 생성 예시로 쓰지 않는다. 시간 상한은 깨움별 작업에 붙이고 누적 작업은 분리한다.

## 웹 배포 조회 카드 L4 A안(§8.293)

- `cargo test -p aaa-host --lib rollout`: DB commit 뒤 변경 사건·표시용 자료만의 요약·재오픈, 종료 commit 직후 재시작·4가지 terminal 상태·중복 발행과 inbox 정리 뒤 재시작을 확인한다. DB owner는 실제 SQL trigger로 inbox 저장을 거부해 완료 정본만 남지 않는지 확인한다. 오류에 경로/PID를 섞어도 표시용 요약·실제 Reader inbox peek 응답에는 없고 정본에는 남는지, 옛 terminal에서 프로젝트/inbox를 재생성하지 않는지도 확인한다. `cargo test -p aaa-host --lib local_wire_excludes_web_only_rollout_notices`는 실제 front frame 전송에서 새 알림이 N-1 CLI에 닿지 않는지 확인한다. 운영 DB/프로세스를 건드리지 않는다.
- `node web/e2e/web.e2e.mjs --spec 35-rollout`: 실제 Reader pairing/API/상태 WS를 쓰고 rollout 사건만 주입한다. 전송률→해당 단계 복구→재연결의 같은ID→복구 완료와 실패 구분·전체 오류/SHA·Reader 시작 거부·390px 폭·앱 안 알림 이동을 확인한다. reload 뒤 이미 선택한 패널 단추를 누르면 drawer가 닫히므로 현재 drawer를 여는 사건으로 이어 간다.
- 변경 사건 누락·종료 중복 발행·정본과 inbox의 transaction 분리와 웹 사건 소비 누락을 각 owner에서 RED로 확인한 뒤 bytes 원복/GREEN을 확인한다. 같은 후보의 전체 ci-local·check-public·독립 read-only 검토를 마친다.

## 같은 Mac의 둘째 aaa(§8.294)

- 유닛:
  - 파생: 이름이 없으면 지금 이름 그대로이고 이름 규칙을 지킨다(`aaa-config` paths). launchd label·로그(`aaa` launchd), 쿠키 이름(`aaa-host` web).
  - init: 포트 쌍 고르기(다른 인스턴스·지금 쓰는 포트 건너뛰기), host.toml 읽기, 긴 상태 폴더 거부(`aaa` init).
  - tailscale: 같은 Mac의 다른 인스턴스 proxy를 제 것으로 보지 않는다.
- m13 `instance::second_instance_has_its_own_folders_host_and_cookie`(macOS): HOME만 실험실로 바꾸고 dir env를 비워, 실제 기본 경로를 본다.
  - dir env가 남으면 init을 거부한다. 이름 규칙(종료 2)을 본다. 터미널이 아니면 거부한다. 기본 인스턴스 프로젝트와 겹치는 이름도 거부한다.
  - init은 `~/.config/aaa-work/host.toml`(7430이 아닌 포트 쌍)과 `~/.aaa-work/projects`를 만들고 기본 폴더는 만들지 않는다. 두 번째 init은 덮지 않는다.
  - `--instance work host`·`agent`가 `Application Support/aaa-work`에서 뜨고 작업 root는 `~/aaa-work-work`다. 기본 인스턴스는 같은 HOME에서도 그 host를 보지 않는다. env `AAA_INSTANCE`만 준 CLI(pane과 같다)는 그 host로 간다.
  - 이름 없는 `project create`는 프로젝트 `work`를 만든다. 기본 인스턴스에서는 이름을 요구한다.
  - `service show`(가짜 `launchctl`, 읽기만)의 plist가 label `dev.aaa.work.host`, env `AAA_INSTANCE = work`, 로그 `Logs/aaa-work/`를 담고 plist를 쓰지 않는다.
  - 실제 HTTP pairing의 Set-Cookie가 `aaa-work=`이다.
  - 결함 심기 셋(쿠키 이름에서 인스턴스 빼기, 작업 root 기본값에서 인스턴스 빼기, plist env에서 `AAA_INSTANCE` 빼기)을 이 시험이 잡는 것을 확인했다.

## 자동 thread 시작(L5, plan §8.295)

- `cargo test -p aaa-host start_`: 실제 host/DB와 기존 agent frame fixture로 첫/넓힘/좁힘 승인, propose 무시작, Working 수 idlest, 명시 holds·상한·기다림 만료·변경 diff, 범위/동작 변경 취소와 본문만 변경 적용을 본다. `auto_start::tests::week_left_...`는 비교 가능한 주간 숫자와 모호한 계정/창/실패의 첫 profile fallback을 소유한다.
- 같은 사건의 thread+시작 key transaction과 DB 재접속, 생성 직후 최종 receipt INSERT를 SQLite trigger로 실패시킨 실제 inflight 복구를 검사한다. fake가 영수증/완료 상태를 대신 만들지 않는다. 일정형은 coordinator 없는 routine_pass부터 실행하며, cursor 저장 전 crash를 되감아 같은 예정시각 key와 실제 thread 한 건/완료 기록을 본다.
- `cargo test -p aaa --test m13 automation_start_holds`: 실험실 두 agent와 실제 CLI의 holds/PTY 사람 승인, 표지 해제 뒤 원격 worktree 생성, profile fallback, host 재시작 뒤 무중복, 앞 thread가 열린 상태의 다른 사건 시작을 본다. 보고 사건으로 진행하며 고정 sleep은 없다.
- 웹34 전체는 실제 Controller의 범위 확인/승인 반영과 Reader의 단추 없음/API 거부를 본다. 기존 파일 전제가 있으므로 개별 뒷 단계 대신 파일 전체를 실행한다.
- 변형은 승인 우회/ceiling 확대 허용, propose에서 직접 실행, idlest 순서, holds 후보/transaction 검사, DB 영수증 조회, 예정시각 key, 명시 기다림 만료/본문 변경 안내를 독립 제거한다. 각 keeper의 의도한 RED와 원본 bytes 복구 PASS를 확인하고 전수 gate는 정상 소스에서만 돈다.

## 사람 터미널·배포 제안 L5(§8.296)

- `cargo test -p aaa-host --lib automation::`: 실제 TOML의 deploy/main/pushed·min_commits1~100 계약과 schema 정본, 실제 SQL receipt 실패→inbox commit→done/prune→host 재오픈→동일 batch 재개를 확인한다. 제안은 한 번만 남고 rollout/thread를 시작하지 않아야 한다. Reader 본문에 raw source 경로/제목을 복제하지 않는다.
- `cargo test -p aaa --test m13 deploy_proposals_count_main_commits`: 실제 bare origin·조회 agent·push로 등록/한 커밋 무음→host 재시작→두 커밋 제안, 성공 정본 뒤 새 기준/최신 실패 정본 보존, forced/deleted 무제안과 rollout 무변경을 본다. 전체 inbox와 Git 사건에 경로/제목/보고서가 없는지, 이전 성공 기록을 둔 채 독립 history origin으로 교체·재시작한 뒤 새 누적 제안이 나오는지도 검사한다.
- `cargo test -p aaa --test m13 rollout_preflights`: 기존 두 머신 keeper에 실제 사람 PTY no·터미널 없는 CLI의 표준 입력 거부, yes 시작·coordinator safety off·host 재시작/같은 ID/bytes·사람 status를 더했다. 이후 coordinator 실패·safety 철회/독립 복구 keeper는 유지한다. 운영 서비스는 실험실 launchctl 기록 모형이다.
- 사람이 여는 문 제거/사람 승인을 coordinator safety로 재판정/제안 표지 누락/커밋 임계값 무시 중 해당 owner의 의도한 RED를 보존하고 bytes 원복 뒤 GREEN을 확인한다. 최종 후보의 전체 ci-local·check-public과 독립 read-only 검토를 마친다.


## 대화 기록 앞 쪽 읽기(t-0454, plan §8.298)

- `cargo test -p aaa-agent -p aaa-host -p aaa-proto --lib chat --no-fail-fast`: 실제 합성 JSONL reader가 Claude/Codex 수천 메시지, 한 줄 안 블록 쪽 경계, append 중 cursor/ID, 미완성 마지막 줄, 첫 메시지 도달, 17MiB blob 너머 진행, 17MiB 그림/문자열·배열 tool_result와 같은 줄의 사람 글 보존, frame 크기를 확인한다. 파일 원천은 agent에 있으므로 쪽 경계의 owner는 agent다. host 유닛은 Reader의 기존 pane 권한 아래 커서/처음/자란 최신 응답의 무변형 중계와 구 agent 기능 거부를 검사한다. proto는 선택 필드를 모르는 구 요청/응답의 양방향 호환을 소유한다.
- `node web/e2e/web.e2e.mjs --spec 37-chat-pages`: 실제 lab host/agent/JSONL에서 WebKit·Chromium × 1280·390·820 × Claude·Codex를 돈다. 사람/답 2400개(1200턴)에서 첫 메시지까지 이전 쪽을 받고 매번 보던 첫 줄의 viewport 좌표가 1px 안에서 같은지 단언한다. 처음 표지/생략 안내 종료, 300개 이하 본문 mount, 위를 읽는 동안 400개가 자란 최신 gap 보존과 맨 아래 따라가기도 검사한다. row40의 pane 왕복 복원 완료(aria-busy), 실제 ResizeObserver callback이 scroll 사건보다 먼저 오는 순서, 짧은 최신 쪽 자동 채움/위치 보존과 수동 End의 빈 공간 제거와 즉시 End 뒤 pane 왕복의 따라가기 기억도 단언한다. 응답/DOM/animation frame 사건으로 기다리고 고정 sleep은 없다.
- 기존 E2E11·29·30의 대화/입력/위치 기억 회귀를 유지한다. 수정 전 36의 이전 쪽 HTTP 요청 없음 RED와 수정 중 prepend 자동 보정이 기준 줄을 바꾸는 RED를 별도 로그로 보존한다. source owner 변형 RED 후 원본 bytes를 돌려놓고 전수 ci-local/check-public을 실행한다.

### 대화 앞 쪽 CI 회귀(t-0458)

- 웹 E2E37의 기존 390px Claude 짧은 쪽 시나리오에서, 긴 기록이 한 줄로 바뀌어 브라우저가 위치를 줄이며 내는 실제 scroll 사건을 앞 쪽 요청 시작 뒤에 전달한다. 응답 gate로 느린 전송을, frame당 25ms 작업으로 느린 렌더를 흉내 낸다. 고정 sleep 없이 scroll·HTTP 응답·DOM commit 사건으로 끝낸다. 보던 줄 1px 단언과 수동 End의 빈 공간 제거는 그대로 둔다.
- 수정 전 WebKit에서 CI와 같은 204.5419921875→755.7919921875 이동을 재현했다. 제품은 렌더 직후 브라우저의 자동 위치 축소를 기억해, 늦은 scroll을 사람의 이동으로 오인하여 끝 따라가기를 켜지 않게 한다. 회귀는 이 owner를 되돌리면 다시 실패해야 한다. 짧은 쪽 진단은 이전/축소 위치·줄 좌표·터미널 1006 횟수를 남긴다.
- Windows는 `cargo xwin clippy --target x86_64-pc-windows-msvc --workspace --all-targets --locked --keep-going -- -D warnings`로 cfg를 검사한다. 실제 Windows 실행 결과와 구분한다. `file_history`의 Unix identity 추가는 불변 변수 shadowing으로 처리하며 경고 allow는 넣지 않는다.

### E2E37 조합별 단계(t-0462)

- 12조합은 각각 독립된 기록·화면을 열고 같은 검사를 수행한다. 각 조합을 별도 `step`으로 실행하며 이름 끝에 브라우저·폭·harness를 붙인다. 공통 단계 상한 120초와 locator/HTTP 대기 상한은 그대로다. CI run 39346442455는 마지막 WebKit 820 Codex를 시작한 뒤 12조합 합계가 120초를 넘어 실패했다. 기본 1280px 화면의 진단을 정지 조합으로 해석하지 않는다.
- `AAA_CHAT_FRAME_MS=25 AAA_E2E_THROTTLE=2 NODE_PATH=$(npm root -g) node web/e2e/web.e2e.mjs --spec 37 --repeat 3 --workers 3`으로 frame 점유(두 브라우저)·CPU 제한(Chromium) 아래 12조합을 반복한다. frame 점유는 시험 페이지 안에서만 돌며 context를 닫을 때 끝난다. 응답·DOM·scroll 사건과 기존 1px 단언, 300개 본문 상한, 2400개 처음 도달·400개 gap·복원·End 검사를 유지한다. 고정 sleep이나 재시도는 없다.
- 조합별로 최신 줄·처음 도달·row40 복원·최신 gap·끝 따라가기의 경과 시간을 남겨 locator 실패 위치와 합계 상한을 구분한다. 실패하면 수신한 행 수·끝 세 행의 본문과 좌표·굴림 위치·빈 공간·오류·화면을 남겨 수신 누락과 끝 따라가기 실패를 구분한다.

### macOS 27 WebKit 끝 이동 경쟁(t-0474)

- 별도로 남겼던 `기록 2800` locator 30초 실패는 제품의 끝 이동 경쟁이었다. 원본 전체 ci-local에서 WebKit 1280 Claude가 재현됐고, 2801행 수신·오류 없음·끝 세 행은 빈 가상 row인 상태였다. scrollTop 129322 / scrollHeight 334056 / clientHeight 794로 화면이 끝에서 이탈했다.
- 실제 scroll 추적은 smooth 요청 → 첫 이동 565px → row 실측의 즉시 끝 보정 337516px → 옛 애니메이션의 다음 scroll 23190px 순서를 보였다. 제품이 마지막 사건을 손의 위 이동으로 오인해 따라가기를 껐다. Safari에서 과거 대화를 읽다 「맨 아래로」를 누르면 새 답이 도착해도 본문이 화면 밖에 남을 수 있었다.
- 가상화하는 긴 대화의 끝 단추는 즉시 이동한다. 같은 가상화 판정을 row와 끝 이동에 함께 쓰며, 실측마다 실제 끝을 맞춘다. 목적지가 바뀌는 native smooth 이동과 즉시 보정을 섞지 않는다. 새 애니메이션 상태나 테스트 전용 제품 표지는 두지 않는다.
- 회귀 소유자는 기존 E2E37의 12조합이다. 2400행 처음 도달·400행 gap·새 메시지 본문과 끝 따라가기·prepend 1px·300개 본문 상한·pane 복원·End 검사를 그대로 쓴다. 원본 실패와 수리 뒤 같은 전체 ci-local 조건의 3회 연속 결과를 보존한다. 각 실행은 새 웹 번들·실행 파일을 확인하고 Rust 시험·clippy와 다른 E2E를 함께 실행한다. 한 번이라도 실패하면 반복을 멈추고 원인을 조사한다. timeout·재시도·허용 오차·조합은 바꾸지 않는다.

## 휴대폰 뒤로 기록(t-0466, plan §8.301)

- `node web/e2e/web.e2e.mjs --spec 39-phone-back`: WebKit iPhone 390·430 × Safari 탭·홈 화면 앱 흉내에서 pane 셋과 대화의 보고서를 연 뒤 앱 문서 칸(init script가 적은 `history.length`)까지 `history.back()`을 popstate 사건마다 하고 화면 차례를 단언한다. 왼쪽 가장자리 쓸기는 `document.createTouch`로 touchstart·touchmove(frame마다)·touchend/touchcancel을 내고, 겹친 뒤로는 손가락을 뗀 뒤 popstate를 기다린다. 닫힌 pane 칸·「모든 thread」 닫기의 기록 칸 수와 휴대폰·데스크톱 빈 화면 문구도 본다. 고정 sleep은 없고 popstate·DOM·animation frame·drawer transition 사건으로 끝낸다.
- 고치기 전 코드로는 첫 단계가 「(주소 없음)+빈 화면」으로, 쓸기 단계가 목록이 열린 채 뒤로로 실패했다.

## 자동화 상세·변환·막힌 큐(t-0468, plan §8.303)

- `node web/e2e/web.e2e.mjs --spec 40-automation-detail`: 상세의 기록 수는 host의 `routine runs`와 같아질 때까지, 원문은 파일 글과 같아질 때까지 기다린다. 변환은 미리보기 sheet가 그린 묶음(`data-group`) 뒤에 파일이 그대로인지 보고, 실행 결과 표지(`convert-archived`) 뒤에 보존 사본을 원본과 비교한다.
- 막힌 큐는 실제 전송 실패로 만든다: fake thread의 pane을 `pane kill`로 끝내고(목록에서 끝남 확인) 1초 일정의 `do.message` 규칙을 둔 뒤 `ticker tick`으로 host 기록에 「입력 전달 모름 · 확인 필요」가 올 때까지 판정한다. 「다시 보내지 않음」은 tick 두 번(울타리) 뒤 같은 묶음 id·실패 기록 수로 단언한다. 「계속 보내기」 뒤에는 pane이 끝난 채라 다음 입력이 실제로 시도되어 실패하고 새 묶음으로 다시 막히는 것을 `ticker tick`·host 기록으로 기다린다. 그 묶음은 「버리기」로 닫고, 규칙을 끈 뒤 tick 울타리 뒤 큐가 빈 것을 본다. fake agent의 전송 지연(재시작 뒤 첫 글이 0~15초)에 기대지 않는다. 고정 sleep은 없다.
- host 쪽 묶음 상태(보낸 결과 모름·다른 동작 묶음·버리기)는 `cargo test -p aaa-host --lib automation::tests`가 DB의 큐 JSON을 직접 두고 본다.

## 대화의 도구 호출 묶음(t-0477, plan §8.304)

- `node --test web/test/toolGroups.test.mjs`: Claude·Codex 도구 이름 분류, 요약 문구(두 가지까지·우선순위·서로 다른 수정 파일·생각), 이어진 도구·생각 줄 묶기, 앞 쪽이 묶음 앞에 붙은 뒤에도 옛 첫 줄 id로 같은 묶음과 펼침을 찾는지 본다.
- `node web/e2e/web.e2e.mjs --spec 41-chat-tool-groups`: 실제 lab host/agent/Claude JSONL에서 데스크톱 Chromium 1280·패드 WebKit 820·휴대폰 WebKit 390을 열고 묶음 요약·접힌 동안 보이는 도구 줄·펼친 줄을 단언한다. 끝이 아닌 곳에서 펼치고 접은 뒤 머리 좌표가 1px 안에서 같고, 끝에서 펼친 뒤에도 끝인지 DOM·animation frame 사건으로 본다. 스크린샷 폴더를 주면 기기마다 끝·첫 묶음·펼침을 찍는다(전후 캡처). 고치기 전 코드로는 묶음 요약이 비어 실패했다. 둘째 단계는 실험실 `claude`(harness `claudeSh`)가 SessionStart hook으로 기록을 알리고 `pane run` 한 줄마다 작업 화면·입력 상자를 바꾸며, 사이드바 `data-status` 사건을 울타리로 맨 끝 묶음의 글과 `is-live`를 본다. 작업 중 판정을 끄면 「작업 중 마지막 도구 줄」에서 실패한다.

## 웹 할 일 보기(t-0480, plan §8.306)

- `cargo test -p aaa-host --lib tasks::`: 운영에서 나온 오독 꼴 셋(같은 꼴의 가짜 글)이 owner가 아니라 `problems`로 나오고 기존 `lines_owners_notes` 기대가 그대로인지, 메모만 바뀌면 그 task의 `block_sha256`만 바뀌는지 본다. 오독 판정을 끄면 `misread_owner_shapes_become_problems`가 실패한다.
- `cargo test -p aaa --test m13 task_board_reads_tasks_and_follows_file_edits`: `aaa task list`의 목록·형식 어긋남·task 없는 열린 thread, worker 세션(보기 권한) 읽기, `--from-task` 거부 까닭, 「나중」 경로 거부·폴더 밖 symlink·없는 파일. 「고치지 않은 tick은 사건이 없다」는 sleep 없이 본다: 기준 tick → 고치지 않은 tick → 파일 고침 → tick 뒤 연결 뒤 첫 `tasks` 사건이 고친 sha인지 확인한다.
- `node web/e2e/web.e2e.mjs --spec 43-tasks`: 막대 단추로 열기, 행 상태·배지·상세·thread 칩 이동, 파일 직접 편집 + `ticker tick` 뒤 `data-tasks-sha` 갱신, 거르기·검색, Reader, 휴대폰 전체 화면 시트와 Back, 패드 세로 시트·가로 두 칸, 「나중」 탭 있음·없음·「없음」. 가짜 thread가 곧 턴을 끝내 앱 안 알림이 도구줄을 덮으면 Playwright가 알림이 시간으로 사라지기를 기다리므로(6초), 첫 단계에서 그 기기의 「앱 안에서」 알림을 끈다.

## 키 관리자 K0 카나리 찾기(§8.307)

- `cargo test -p aaa --test m13 secret_k0`: 실험실 host·agent를 `AAA_LOG=trace`로 띄운다. 무작위 가짜 값(카나리)을 셋 둔다.
  - A·C: host.toml profile env. 웹 새 세션처럼 `pane_run --profile`로 띄운다. C에는 `/`·`"`·`+`·공백·`&`를 넣어 URL·JSON escape 꼴이 원문과 달라지게 한다.
  - B: thread launch env. `thread start --profile`로 띄운다.
  - 받은 pane은 값의 sha256만 실험실 상태 밖 파일에 쓴다. 시험은 그 hash로 값이 실제로 갔는지 본다(가림 때문에 전달이 끊긴 것을 통과로 착각하지 않으려는 장치다).
- 찾는 꼴: 원문, 무작위 부분의 8자 조각, base64(표준·URL, 앞 0~2바이트 정렬마다 끝 4자를 뺀 가운데), URL 인코딩, JSON escape.
- 찾는 곳:
  - A·C: host DB 파일·WAL·shm 전체.
  - A·B·C: host·agent stderr 로그, `project delete --force`의 `aaa-records.json`, 감사 기록 `pane_run` 행, 상태 폴더 전체.
  - 상태 폴더에서 빼는 K2 범위: agent `sessions/`·`thread-preparations/`. host DB의 B도 뺀다(`threads.data`).
- 기록의 모양도 본다: 감사 기록·export의 env는 이름 목록이다. 평문 경고는 host 로그에 이름으로 나온다.
- 마지막으로 pane 안 `printenv`에 값이 보이고 `ulimit -c`가 0인지 본다. 이 출력은 pane 화면 저장물(K2)에 남으므로, 그 뒤에는 host DB만 다시 찾는다. agent는 core soft 한도를 hard까지 연 채 띄운다. 그래야 끄기를 빼면 이 단계가 실패한다.
- 모두 출력·파일 사건으로 기다리며 고정 sleep은 없다.

## 대화 정리(compaction) 중 감지·보류·표시(t-0479, plan §8.309)

- agent 감지는 실제 화면 fixture(`crates/aaa-agent/src/detect/fixtures/*compact*.txt`, 10-20 Claude 2.1.304·Codex 0.163 `/compact` 실측)로 본다: `cargo test -p aaa-agent --lib detect`. 「(0s)」 Claude 화면은 참고 규칙으로 쉼이라 덧댐이 없으면 시험이 실패한다(결함 심기로 확인).
- `node web/e2e/web.e2e.mjs --spec 45-compacting`: 가짜 Claude가 F7(작업 중)·F5(정리 시작)·F6(정리 끝)을 `aaa pane send-keys`로 받아 실제 정리 화면(spinner 「Compacting conversation… (3s)」, 참고 규칙으로는 쉼)과 실제 기록 차례(compact_boundary → isCompactSummary → 대기열 dequeue)를 그린다. 상태 전이는 가짜가 그린 「mode …」를 `waitOutput`으로, 화면은 `domIs`로 기다린다. 「보내지 않았다」는 보관 직후 `sentFrames`(그 사건 안에서 끝나는 보관)와 pane이 받은 bytes로 본다. 휴대폰·패드는 WebKit `pairDevice`. 같은 브라우저 탭 둘은 같은 context의 page 둘이다: 「한 번만 갔다」는 그 글을 품은 사람 말이 꼭 하나인지(두 탭이 함께 붙여 넣으면 한 말로 붙는다)와, 다른 탭이 다음 글을 보낸 뒤에도 그대로인지(입력창은 앞 글이 보내질 때까지 잠긴다)로 본다. 고정 sleep은 없다(넘겨받기 전 다른 탭에 묻는 500ms는 제품의 기다림이고, 시험은 기록에 오는 것을 기다린다).
- 웹 단위 `node --test web/test/messageQueue.test.mjs`: 정리 뒤 보낼 글의 저장 왕복(넣은 탭 `by`), 표지 떼기·넘겨받기, 정리 뒤 차례(`compactionStep`: 넣은 탭만 보냄, 앞의 보통 글을 앞지르지 않음, 자동 전송을 껐으면 표지를 뗌).
- 결함 심기로 확인한 것: agent 덧댐 끔(detect 시험 둘 실패), 옛 host에 compacting 그대로 보냄(`compaction_goes_to_an_old_host_as_working` 실패), host 표지 안 씀, agent가 청하지 않아도 정리 줄을 냄, 웹 보류 없음·「지금 보내기」 정리 중 켬·정리 중 줄 없음·정리됨 구분선 없음·끝나도 안 보냄·탭 주인 무시(「두 탭 하나두 탭 하나」)·넘겨받기 없음(각각 E2E 45 실패). Codex 답 글 오탐·긴 초안에 밀린 표지는 detect 시험이 고치기 전에 실패했다. 웹 결함은 `npm run build`(tsc 포함)가 실패하면 dist가 그대로라 옛 번들로 돈다: 심은 뒤 binary의 `index-*.js` 이름이 바뀌었는지 먼저 본다.

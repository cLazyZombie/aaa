# 시험 쓰는 법

새 기능을 만드는 thread가 따르는 짧은 규칙이다. 근거는 참고 넷(herdr 0.9.5·herdr-projects 0.2.35·herdr-web-ui `aa624cb`·t3code `4b1024da`)과 이 저장소에서 실제로 난 간헐 실패다. 전략과 도구 목록은 [plan §13](plan.md#13-테스트-전략과-직접-만들-e2e-도구)에 있다.

## 원칙

- **사건으로 끝낸다.** 시험은 정한 시간을 자지 않고, 기다리는 것이 왔다는 신호(흐름 사건·출력·화면 표지·fake 기록·tick 끝남)를 기다린다. 「시간 제한이 있어야 통과하는 시험은 틀렸다」(t3code `AGENTS.md`). 시한은 오지 않을 때 멈추는 안전 상한일 뿐이다.
- **시간 규칙은 시계를 흉내 내지 않고 `now` 인자와 설정 값으로 본다.** 판정 함수는 `now`를 받는다(herdr `*_at(now)`, herdr-projects `Outage::record(now)`, aaa `inbox::Outage::record`). 그 함수를 유닛 시험에서 여러 `now`로 부른다. E2E는 설정 값으로 시간 창을 줄이고(아래 표), 창이 지난 뒤 `ticker tick`으로 host가 판정했음을 확인한다.
- **실패를 다시 돌려 넘기지 않는다.** 재시도·시한 늘리기·기대값 완화·건너뛰기로 통과시키지 않는다. 실패의 증거(실험실 폴더·화면·로그)를 남기고 원인을 찾는다.
- **실험실은 남의 것을 건드리지 않는다.** 빈 port, 짧은 임시 경로, 자기가 띄운 PID만 정리한다. 운영 host·사용자 셸 설정·실제 clipboard·소리에 닿지 않는다.
- **실행할 파일은 Unix에서 자식이 복사한다.** 병렬 시험의 부모가 실행 파일을 쓰면 다른 fork가 쓰기 fd를 물려받아 복사 뒤 exec도 `ETXTBSY`로 실패할 수 있다([Rust #114554](https://github.com/rust-lang/rust/issues/114554)). 별도 복사 프로세스가 끝난 뒤 실행한다. 재시도·sleep·시험 전체 직렬화로 덮지 않는다. Codex 설치·hook 시험의 `Home::moved_bin`이 이 경계를 쓴다.

## 어디에 두나

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
| harness 화면 감지 시작 유예(3초, 시작 화면을 상태로 읽지 않는다) | `AAA_TEST_DETECT_GRACE_MS`(agent, 시험에서만, 웹 E2E 11·19 0. 기본값 길은 15) |
| 긴 턴(60초, 웹 앱 안 알림은 그보다 긴 턴의 끝남만 띄운다, plan §8.185) | `AAA_TEST_LONG_TURN_MS`(host, 시험에서만, 웹 E2E 19 3000. 기본값 길은 m13 `turn_end_notices_follow_reports`) |

## 웹 E2E

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

# 시험 쓰는 법

새 기능을 만드는 thread가 따르는 짧은 규칙이다. 근거는 참고 넷(herdr 0.9.5·herdr-projects 0.2.35·herdr-web-ui `aa624cb`·t3code `4b1024da`)과 이 저장소에서 실제로 난 간헐 실패다. 전략과 도구 목록은 [plan §13](plan.md#13-테스트-전략과-직접-만들-e2e-도구)에 있다.

## 원칙

- **사건으로 끝낸다.** 시험은 정한 시간을 자지 않고, 기다리는 것이 왔다는 신호(흐름 사건·출력·화면 표지·fake 기록·tick 끝남)를 기다린다. 「시간 제한이 있어야 통과하는 시험은 틀렸다」(t3code `AGENTS.md`). 시한은 오지 않을 때 멈추는 안전 상한일 뿐이다.
- **시간 규칙은 시계를 흉내 내지 않고 `now` 인자와 설정 값으로 본다.** 판정 함수는 `now`를 받는다(herdr `*_at(now)`, herdr-projects `Outage::record(now)`, aaa `inbox::Outage::record`). 그 함수를 유닛 시험에서 여러 `now`로 부른다. E2E는 설정 값으로 시간 창을 줄이고(아래 표), 창이 지난 뒤 `ticker tick`으로 host가 판정했음을 확인한다.
- **실패를 다시 돌려 넘기지 않는다.** 재시도·시한 늘리기·기대값 완화·건너뛰기로 통과시키지 않는다. 실패의 증거(실험실 폴더·화면·로그)를 남기고 원인을 찾는다.
- **실험실은 남의 것을 건드리지 않는다.** 빈 port, 짧은 임시 경로, 자기가 띄운 PID만 정리한다. 운영 host·사용자 셸 설정·실제 clipboard·소리에 닿지 않는다.

## 어디에 두나

| 무엇 | 위치 | 실행 |
|---|---|---|
| 순수 규칙·판정(Rust) | 소스 옆 `#[cfg(test)]` | `cargo test -p <crate>` |
| 여러 프로세스(host·agent·session·CLI·TUI) | `crates/aaa/tests/*.rs` + 실험실 `crates/aaa/tests/lab/mod.rs` | `cargo test -p aaa --test m13 <이름>` |
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
| 웹 화면이 판단을 끝냈다(그래서 하지 않았다) | 판단 자리에서 다는 제품 표지 뒤에 본다. Term `data-parsed-top`·`data-parsed-base`(받은 출력을 다 읽은 뒤 버퍼 자리: 올려 둔 동안 아래에 줄만 붙으면 그리기·`data-view-*`는 오지 않는다), `data-retry`(다시 붙기를 건 close code·ms), `data-fit`(맞추기 판단 off·released·other·keyboard·unsized·sent·same)·`data-fit-pending`(크기 바뀜 뒤 맞추기를 기다리는 동안), App `data-wake-lock`(wake lock effect가 돈 설정·걸지·보는 것). 크기·다시 붙기는 그 판단 안에서 바로 보내므로 표지 뒤 `sentFrames`가 비면 보내지 않았다 |
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
| routine | routine 머리 `schedule = "every 2s"` |
| 알림 묶음 | `[notify] debounce_secs` |
| 웹 상태 사건 ping | `AAA_TEST_EVENTS_PING_MS`(시험에서만) |
| thread brief를 치기 전 화면 멈춤(3초) | `AAA_TEST_BRIEF_SETTLE_MS`(시험에서만, 웹 E2E 300) |

## 웹 E2E

```
web/e2e/web.e2e.mjs        입구: spec 파일마다 lib/child.mjs 프로세스를 띄우고(worker N개가 함께) 결과를 모은다
web/e2e/durations.json     파일마다 기록한 초. 함께 돌 때 긴 파일부터 넣는다
web/e2e/lib/harness.mjs    실험실(host·agent·가짜 push·구독 서버·시험 프로그램)·Chromium·step·기다림·화면·IME·push 도우미, runSpec
web/e2e/lib/devices.mjs    기기 꼴과 그 기기 규칙의 가짜(iOS 소리·알림), 기록을 사건으로 기다림(아래 「실기와 headless」)
web/e2e/specs/NN-기능.mjs  기능별 단계(01-http-pwa … 16-file-viewer). export default async function ({ ctl, page }) { … }
```

- **파일마다 새 실험실.** host·agent·Chromium을 파일마다 새로 띄운다(`/tmp/aaw-<pid>`, 준비 약 1.5초, 웹 port는 host가 고른다). 앞 파일이 만든 pane·질문·구독·설정에 기대지 않으므로 파일 하나를 혼자, 여러 번, 다른 파일과 함께 돌릴 수 있다. 한 파일 안 단계는 차례로 돌고 앞 단계가 만든 상태를 쓸 수 있다(그래서 `--grep`으로 뒤 단계만 고르면 앞 단계가 만든 것이 없을 수 있다: 그때는 `--spec`으로 파일을 돈다).
- **준비(`runSpec`).** host·agent(`labA`)·셸 탭 `t1`을 띄우고, 첫 화면 `ctl`·`page`(1280×800, 입력줄)를 t1에 연다. t1에는 `t1-42`가 찍혀 있다: 단계가 t1로 돌아왔는지는 이 글로 본다. 단계가 실패하면 그 파일의 나머지 단계는 돌지 않고 다음 파일로 간다.
- **함께 돌기.** 파일은 worker 여러 개가 함께 돈다. 기본은 core 4개마다 하나, 많아야 4(`MAX_WORKERS`, 16 core에서 잰 상한), `--workers 1`·`AAA_E2E_WORKERS=1`이면 예전처럼 차례로 돌고 출력을 바로 보인다. 2 이상이면 파일이 끝날 때 그 파일의 `## specs/…`와 단계 줄을 한 덩어리로 찍는다. 함께 돌 때는 `durations.json`의 긴 파일부터 넣는다(LPT): 가장 긴 파일이 늦게 시작해 혼자 남지 않게. 적지 않은 새 spec은 가장 긴 것으로 보고 맨 앞에 넣는다. 파일 시간이 크게 바뀌면 `--write-durations`(모두 통과한 실행의 시간을 적는다)로 다시 적는다. 시간이 늘어도 재시도·timeout 연장으로 넘기지 않는다: 한 파일이 혼자 오래 걸리면 그 파일을 기능별로 나눈다(앞 단계 상태에 기대는 단계는 그 준비를 도우미로).
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
